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

//! HPC Matrix-Vector Multiplication with MPI-style Collective Operations
//!
//! This example demonstrates:
//! - TupleSpace for dataflow coordination (MPI-style collectives)
//! - Actor-based workers using ActorBuilder
//! - ConfigBootstrap for configuration
//! - CoordinationComputeTracker for metrics

use matrix_vector_mpi::*;

use plexspaces_node::{NodeBuilder, ConfigBootstrap, CoordinationComputeTracker};
use plexspaces_actor::ActorBuilder;
use plexspaces_tuplespace::TupleSpace;
use plexspaces_mailbox::Message;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== HPC Matrix-Vector Multiplication (MPI-style) ===\n");

    // Load configuration using ConfigBootstrap
    let config: MatrixVectorConfig = ConfigBootstrap::load().unwrap_or_default();
    let num_workers = config.num_workers;
    let num_rows = config.num_rows;
    let num_cols = config.num_cols;

    info!("Configuration:");
    info!("  Matrix A: {}×{}", num_rows, num_cols);
    info!("  Vector x: {}×1", num_cols);
    info!("  Workers:  {}", num_workers);
    info!("  Rows per worker: {}\n", num_rows / num_workers);

    // Create test data
    let matrix = create_test_matrix(num_rows, num_cols);
    let vector = create_test_vector(num_cols);

    info!("Matrix A:");
    print_matrix(&matrix);

    info!("\nVector x:");
    print_vector(&vector);

    // Create node using NodeBuilder
    let node = NodeBuilder::new("mpi-node")
        .build();

    // Create TupleSpace for coordination (dataflow pattern)
    let space = Arc::new(TupleSpace::new());

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("matrix-vector-mpi".to_string());

    // Create and spawn worker actors
    info!("\n=== Creating Worker Actors ===");
    let mut worker_refs = Vec::new();
    for worker_id in 0..num_workers {
        let worker_actor = WorkerActor::new(space.clone(), worker_id, num_cols);
        let behavior = Box::new(worker_actor);
        
        let actor_ref = ActorBuilder::new(behavior)
            .with_name(format!("worker-{}", worker_id))
            .spawn(node.service_locator().clone())
            .await?;
        worker_refs.push(actor_ref);
        info!("  Created worker actor: worker-{}", worker_id);
    }

    // Wait for actors to initialize
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Phase 1: Scatter rows
    info!("\n=== Phase 1: Scatter (Distribute Rows) ===");
    metrics_tracker.start_coordinate();
    scatter_rows(&space, &matrix, num_workers).await?;
    metrics_tracker.end_coordinate();

    // Phase 2: Broadcast vector
    info!("\n=== Phase 2: Broadcast (Send Vector to All) ===");
    metrics_tracker.start_coordinate();
    broadcast_vector(&space, &vector).await?;
    metrics_tracker.end_coordinate();

    // Phase 3: Workers compute (send messages to actors)
    info!("\n=== Phase 3: Local Computation ===");
    metrics_tracker.start_compute();
    for (worker_id, actor_ref) in worker_refs.iter().enumerate() {
        let msg = WorkerMessage::Compute {
            worker_id,
            num_cols,
        };
        let payload = serde_json::to_vec(&msg)?;
        let message = Message::new(payload)
            .with_message_type("Compute".to_string());
        
        actor_ref.tell(message).await?;
    }
    
    // Wait for all workers to complete
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    metrics_tracker.end_compute();

    // Phase 4: Barrier
    info!("\n=== Phase 4: Barrier (Synchronize Workers) ===");
    metrics_tracker.start_coordinate();
    barrier_sync(&space, num_workers).await?;
    metrics_tracker.end_coordinate();

    // Phase 5: Gather results
    info!("\n=== Phase 5: Gather (Collect Results) ===");
    metrics_tracker.start_coordinate();
    let result = gather_results(&space, num_workers, num_rows).await?;
    metrics_tracker.end_coordinate();

    info!("\nResult y = A × x:");
    print_vector(&result);

    // Verify
    let expected = compute_sequential(&matrix, &vector);
    info!("\nExpected (sequential):");
    print_vector(&expected);

    if result == expected {
        info!("\n✅ Parallel result matches sequential computation!");
    } else {
        error!("\n❌ Results don't match!");
        return Err(anyhow::anyhow!("Verification failed"));
    }

    // Finalize metrics and print
    let metrics = metrics_tracker.finalize();
    info!("\n📊 Coordination vs Compute Metrics:");
    info!("  Compute time: {:.2}ms", metrics.compute_duration_ms);
    info!("  Coordination time: {:.2}ms", metrics.coordinate_duration_ms);
    info!("  Granularity ratio: {:.2}×", metrics.granularity_ratio);
    info!("  Efficiency: {:.2}%", metrics.efficiency * 100.0);
    info!("  Message count: {}", metrics.message_count);
    info!("  Barrier count: {}", metrics.barrier_count);

    Ok(())
}

/// Create test matrix
fn create_test_matrix(rows: usize, cols: usize) -> Vec<Vec<f64>> {
    (0..rows).map(|i| {
        (0..cols).map(|j| (i * cols + j + 1) as f64).collect()
    }).collect()
}

/// Create test vector
fn create_test_vector(size: usize) -> Vec<f64> {
    (1..=size).map(|i| i as f64).collect()
}

/// Print matrix
fn print_matrix(matrix: &[Vec<f64>]) {
    for row in matrix {
        print!("[");
        for (i, val) in row.iter().enumerate() {
            if i > 0 { print!("  "); }
            print!("{:3.0}", val);
        }
        println!("]");
    }
}

/// Print vector
fn print_vector(vector: &[f64]) {
    print!("[");
    for (i, val) in vector.iter().enumerate() {
        if i > 0 { print!(" "); }
        print!("{:3.0}", val);
    }
    println!("]");
}

/// Sequential computation for verification
fn compute_sequential(matrix: &[Vec<f64>], vector: &[f64]) -> Vec<f64> {
    matrix.iter().map(|row| {
        row.iter().zip(vector)
            .map(|(a, b)| a * b)
            .sum()
    }).collect()
}
