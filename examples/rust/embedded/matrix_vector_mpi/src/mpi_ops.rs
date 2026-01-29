// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! MPI-style Collective Operations

use plexspaces_tuplespace::{TupleSpace, Tuple, TupleField, Pattern, PatternField};
use std::sync::Arc;
use tokio::time::{Duration, timeout};
use anyhow::Result;

/// MPI_Scatter: Distribute matrix rows to workers
///
/// Pattern: ("scatter", "matrix_rows", worker_id, row_data)
pub async fn scatter_rows(
    space: &TupleSpace,
    matrix: &[Vec<f64>],
    num_workers: usize,
) -> Result<()> {
    let rows_per_worker = matrix.len() / num_workers;

    for worker_id in 0..num_workers {
        let start_row = worker_id * rows_per_worker;
        let end_row = if worker_id == num_workers - 1 {
            matrix.len() // Last worker gets remainder
        } else {
            start_row + rows_per_worker
        };

        let worker_rows: Vec<Vec<f64>> = matrix[start_row..end_row].to_vec();
        let rows_json = serde_json::to_string(&worker_rows)?;

        space.write(Tuple::new(vec![
            TupleField::String("scatter".to_string()),
            TupleField::String("matrix_rows".to_string()),
            TupleField::Integer(worker_id as i64),
            TupleField::String(rows_json),
        ])).await?;

        println!("  Worker {} receives rows {}-{} ({} rows)",
            worker_id, start_row, end_row - 1, worker_rows.len());
    }

    Ok(())
}

/// MPI_Bcast: Broadcast vector to all workers
///
/// Pattern: ("broadcast", "vector", vector_data)
pub async fn broadcast_vector(
    space: &TupleSpace,
    vector: &[f64],
) -> Result<()> {
    let vector_json = serde_json::to_string(vector)?;

    space.write(Tuple::new(vec![
        TupleField::String("broadcast".to_string()),
        TupleField::String("vector".to_string()),
        TupleField::String(vector_json),
    ])).await?;

    println!("  Master broadcasts vector to all workers");

    Ok(())
}

/// Worker computation: Read assigned rows, multiply with vector
pub async fn worker_compute(
    space: Arc<TupleSpace>,
    worker_id: usize,
    _num_cols: usize,
) -> Result<()> {
    // Read assigned rows
    let pattern_rows = Pattern::new(vec![
        PatternField::Exact(TupleField::String("scatter".to_string())),
        PatternField::Exact(TupleField::String("matrix_rows".to_string())),
        PatternField::Exact(TupleField::Integer(worker_id as i64)),
        PatternField::Wildcard,
    ]);

    let rows_tuple = space.read(pattern_rows).await?
        .ok_or_else(|| anyhow::anyhow!("Worker rows not found"))?;

    let rows_json = match &rows_tuple.fields()[3] {
        TupleField::String(s) => s,
        _ => return Err(anyhow::anyhow!("Invalid rows data")),
    };
    let worker_rows: Vec<Vec<f64>> = serde_json::from_str(rows_json)?;

    // Read broadcast vector
    let pattern_vector = Pattern::new(vec![
        PatternField::Exact(TupleField::String("broadcast".to_string())),
        PatternField::Exact(TupleField::String("vector".to_string())),
        PatternField::Wildcard,
    ]);

    let vector_tuple = space.read(pattern_vector).await?
        .ok_or_else(|| anyhow::anyhow!("Vector not found"))?;

    let vector_json = match &vector_tuple.fields()[2] {
        TupleField::String(s) => s,
        _ => return Err(anyhow::anyhow!("Invalid vector data")),
    };
    let vector: Vec<f64> = serde_json::from_str(vector_json)?;

    // Compute local matrix-vector product
    let mut local_result = Vec::new();
    for row in &worker_rows {
        let dot_product: f64 = row.iter().zip(&vector)
            .map(|(a, b)| a * b)
            .sum();
        local_result.push(dot_product);
    }

    println!("  Worker {} computed {} results", worker_id, local_result.len());

    // Write result for gather
    let result_json = serde_json::to_string(&local_result)?;

    space.write(Tuple::new(vec![
        TupleField::String("gather".to_string()),
        TupleField::String("partial_result".to_string()),
        TupleField::Integer(worker_id as i64),
        TupleField::String(result_json),
    ])).await?;

    // Write barrier arrival
    space.write(Tuple::new(vec![
        TupleField::String("barrier".to_string()),
        TupleField::String("compute_done".to_string()),
        TupleField::Integer(worker_id as i64),
    ])).await?;

    Ok(())
}

/// MPI_Barrier: Wait for all workers to complete
///
/// **NOTE**: This uses TupleSpace primitives for barrier synchronization.
/// The gRPC Barrier RPC was intentionally removed - this primitive-based approach
/// is now the recommended pattern.
///
/// **See**: `docs/BARRIER_RPC_REMOVAL.md` for architecture rationale and migration guide.
pub async fn barrier_sync(
    space: &TupleSpace,
    num_workers: usize,
) -> Result<()> {
    println!("  Master registering barrier for {} workers", num_workers);

    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("barrier".to_string())),
        PatternField::Exact(TupleField::String("compute_done".to_string())),
        PatternField::Wildcard,
    ]);

    // Check how many tuples already exist
    let existing_count = {
        let mut count = 0;
        for worker_id in 0..num_workers {
            let check_pattern = Pattern::new(vec![
                PatternField::Exact(TupleField::String("barrier".to_string())),
                PatternField::Exact(TupleField::String("compute_done".to_string())),
                PatternField::Exact(TupleField::Integer(worker_id as i64)),
            ]);
            if space.read(check_pattern).await?.is_some() {
                count += 1;
            }
        }
        count
    };

    println!("  Found {} existing barrier tuples (need {})", existing_count, num_workers);

    if existing_count >= num_workers {
        // All workers already arrived - barrier already satisfied
        println!("  All {} workers synchronized at barrier (already complete)", num_workers);
        return Ok(());
    }

    // Register barrier and wait for remaining workers
    let mut barrier_rx = space.barrier(
        "compute_done".to_string(),
        pattern.clone(),
        num_workers, // Wait for all workers
    ).await;

    // Wait for barrier
    timeout(Duration::from_secs(5), barrier_rx.recv()).await?
        .ok_or_else(|| anyhow::anyhow!("Barrier timeout - not all workers arrived"))?;

    println!("  All {} workers synchronized at barrier", num_workers);

    Ok(())
}

/// MPI_Gather: Collect partial results from all workers
pub async fn gather_results(
    space: &TupleSpace,
    num_workers: usize,
    _total_rows: usize,
) -> Result<Vec<f64>> {
    let mut final_result = Vec::new();

    for worker_id in 0..num_workers {
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("gather".to_string())),
            PatternField::Exact(TupleField::String("partial_result".to_string())),
            PatternField::Exact(TupleField::Integer(worker_id as i64)),
            PatternField::Wildcard,
        ]);

        let result_tuple = space.read(pattern).await?
            .ok_or_else(|| anyhow::anyhow!("Worker {} result not found", worker_id))?;

        let result_json = match &result_tuple.fields()[3] {
            TupleField::String(s) => s,
            _ => return Err(anyhow::anyhow!("Invalid result data")),
        };

        let partial_result: Vec<f64> = serde_json::from_str(result_json)?;
        println!("  Gathered {} values from worker {}", partial_result.len(), worker_id);

        final_result.extend(partial_result);
    }

    Ok(final_result)
}
