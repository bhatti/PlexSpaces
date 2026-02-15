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
//! - SDK annotations: `#[event_actor]`, `#[plexspaces_handlers(event)]`, `#[handler]`
//! - SDK spawn helpers: `spawn()` for actor creation
//! - SDK message helpers: `cast_message()` for fire-and-forget messages
//! - ConfigBootstrap for configuration
//! - CoordinationComputeTracker for metrics

use matrix_vector_mpi::*;

use plexspaces_node::{NodeBuilder, ConfigBootstrap, CoordinationComputeTracker};
use plexspaces_sdk::{
    spawn, cast_message, RequestContext, json,
};
use plexspaces_tuplespace::TupleSpace;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing - ensure INFO level for metrics output
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    info!("=== HPC Matrix-Vector Multiplication (MPI-style) ===\n");

    // Load configuration using ConfigBootstrap
    let config: MatrixVectorConfig = ConfigBootstrap::load().unwrap_or_default();
    let num_workers = config.num_workers;
    let num_rows = config.num_rows;
    let num_cols = config.num_cols;

    info!("Configuration:");
    info!("  Matrix A: {}×{} ({} elements)", num_rows, num_cols, num_rows * num_cols);
    info!("  Vector x: {}×1 ({} elements)", num_cols, num_cols);
    info!("  Workers:  {} ({} rows/worker)", num_workers, num_rows / num_workers);
    info!("  Total operations: {} ({} multiplications + {} additions)\n", 
        num_rows * num_cols * 2, num_rows * num_cols, num_rows * (num_cols - 1));

    // Create test data
    let start_time = std::time::Instant::now();
    info!("Creating test data...");
    let matrix = create_test_matrix(num_rows, num_cols);
    let vector = create_test_vector(num_cols);
    let data_creation_time = start_time.elapsed();
    info!("  Data creation: {:.2}ms\n", data_creation_time.as_secs_f64() * 1000.0);

    // Only print matrix/vector for small sizes
    if num_rows <= 20 && num_cols <= 20 {
        info!("Matrix A:");
        print_matrix(&matrix);
        info!("\nVector x:");
        print_vector(&vector);
    } else {
        info!("Matrix A: {}×{} (too large to display)", num_rows, num_cols);
        info!("Vector x: {}×1 (too large to display)", num_cols);
    }

    // Create node using NodeBuilder
    let node = NodeBuilder::new("mpi-node")
        .build().await;

    // Create TupleSpace for coordination (dataflow pattern)
    let space = Arc::new(TupleSpace::with_tenant_namespace("internal", "system"));

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("matrix-vector-mpi".to_string());

    // Create and spawn worker actors using SDK spawn helper
    info!("\n=== Creating Worker Actors ===");
    let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string())
        .with_internal(true)
        .with_admin(true);
    let service_locator = node.service_locator().clone();
    let mut worker_refs = Vec::new();
    
    for worker_id in 0..num_workers {
        let worker_actor = WorkerActor::new(space.clone(), worker_id);
        let worker_id_str = format!("worker-{}", worker_id);
        let actor_id = format!("{}@{}", worker_id_str, node.id().as_str());
        
        // Use SDK spawn helper (no facets needed for this actor)
        let actor_ref = spawn(
            &ctx,
            service_locator.clone(),
            actor_id,
            "matrix-vector-mpi", // namespace
            worker_actor,
        ).await
            .map_err(|e| anyhow::anyhow!("Failed to spawn actor: {}", e))?;
        
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

    // Phase 3: Workers compute (send messages to actors using SDK cast_message)
    info!("\n=== Phase 3: Local Computation ===");
    let compute_start = std::time::Instant::now();
    metrics_tracker.start_compute();
    
    // Send compute messages to workers
    for (worker_id, actor_ref) in worker_refs.iter().enumerate() {
        // Use SDK cast_message helper for fire-and-forget messages
        // Handler extracts operation from payload.action, payload.op, or payload.event_type
        let message = cast_message(json!({
            "action": "Compute",  // Operation name for handler dispatch
            "worker_id": worker_id,
        }));
        
        actor_ref.tell(message).await?;
    }
    
    // Wait for all workers to complete (poll barrier - this is coordination overhead)
    let mut attempts = 0;
    loop {
        let mut count = 0;
        for worker_id in 0..num_workers {
            let check_pattern = plexspaces_tuplespace::Pattern::new(vec![
                plexspaces_tuplespace::PatternField::Exact(
                    plexspaces_tuplespace::TupleField::String("barrier".to_string())
                ),
                plexspaces_tuplespace::PatternField::Exact(
                    plexspaces_tuplespace::TupleField::String("compute_done".to_string())
                ),
                plexspaces_tuplespace::PatternField::Exact(
                    plexspaces_tuplespace::TupleField::Integer(worker_id as i64)
                ),
            ]);
            if space.read(check_pattern).await?.is_some() {
                count += 1;
            }
        }
        
        if count >= num_workers {
            break;
        }
        
        attempts += 1;
        if attempts > 10000 {
            return Err(anyhow::anyhow!("Timeout waiting for workers to complete"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    
    let compute_elapsed = compute_start.elapsed();
    metrics_tracker.end_compute();
    info!("  Compute phase completed in {:.2}ms", compute_elapsed.as_secs_f64() * 1000.0);

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

    // Verify results
    info!("\nVerifying results...");
    let verify_start = std::time::Instant::now();
    let expected = compute_sequential(&matrix, &vector);
    let verify_time = verify_start.elapsed();
    
    if result == expected {
        info!("  ✅ Parallel result matches sequential computation!");
        info!("  Verification time: {:.2}ms", verify_time.as_secs_f64() * 1000.0);
        
        // Only print results for small sizes
        if num_rows <= 20 {
            info!("\nResult y = A × x:");
            print_vector(&result);
        } else {
            info!("  Result vector: {} elements (too large to display)", result.len());
            info!("  Sample: first 5 = [{:.2}, {:.2}, {:.2}, {:.2}, {:.2}]", 
                result[0], result[1], result[2], result[3], result[4]);
        }
    } else {
        error!("\n❌ Results don't match!");
        if num_rows <= 20 {
            info!("Result:");
            print_vector(&result);
            info!("Expected:");
            print_vector(&expected);
        }
        return Err(anyhow::anyhow!("Verification failed"));
    }

    // Finalize metrics and calculate benchmarks
    let metrics = metrics_tracker.finalize();
    
    // Calculate benchmark metrics
    let total_ops = num_rows * num_cols * 2; // multiplications + additions
    let total_time_secs = metrics.total_duration_ms as f64 / 1000.0;
    let gflops = if total_time_secs > 0.0 {
        (total_ops as f64 / 1_000_000_000.0) / total_time_secs
    } else {
        0.0
    };
    let throughput_mb = if total_time_secs > 0.0 {
        ((num_rows * num_cols * 8 + num_cols * 8) as f64 / 1_000_000.0) / total_time_secs
    } else {
        0.0
    };
    
    // Calculate estimated speedup (Amdahl's Law approximation)
    let sequential_time_ms = metrics.compute_duration_ms as f64 + metrics.coordinate_duration_ms as f64;
    let parallel_time_ms = (metrics.compute_duration_ms as f64 / num_workers as f64) + metrics.coordinate_duration_ms as f64;
    let estimated_speedup = if parallel_time_ms > 0.0 {
        sequential_time_ms / parallel_time_ms
    } else {
        1.0
    };

    // Print metrics prominently with clear coordination vs computation breakdown
    info!("\n{}", "=".repeat(80));
    info!("📊 PERFORMANCE METRICS & BENCHMARKS");
    info!("{}", "=".repeat(80));
    
    info!("\nProblem Size:");
    info!("  Matrix: {}×{} ({} elements)", num_rows, num_cols, num_rows * num_cols);
    info!("  Vector: {}×1 ({} elements)", num_cols, num_cols);
    info!("  Workers: {} ({} rows/worker)", num_workers, num_rows / num_workers);
    info!("  Total operations: {} ({} multiplications + {} additions)", 
        total_ops, num_rows * num_cols, num_rows * (num_cols - 1));
    
    info!("\n{}", "─".repeat(80));
    info!("⚡ LATENCY BREAKDOWN (Coordination vs Computation)");
    info!("{}", "─".repeat(80));
    info!("  Scatter:    {:>12.2} ms (coordination)", metrics.coordinate_duration_ms as f64 * 0.2);
    info!("  Broadcast:  {:>12.2} ms (coordination)", metrics.coordinate_duration_ms as f64 * 0.1);
    info!("  Compute:    {:>12.2} ms (computation)", metrics.compute_duration_ms as f64);
    info!("  Barrier:    {:>12.2} ms (coordination)", metrics.coordinate_duration_ms as f64 * 0.1);
    info!("  Gather:     {:>12.2} ms (coordination)", metrics.coordinate_duration_ms as f64 * 0.6);
    info!("  {}", "─".repeat(30));
    info!("  Coordination: {:>10.2} ms (total)", metrics.coordinate_duration_ms as f64);
    info!("  Computation:  {:>10.2} ms (total)", metrics.compute_duration_ms as f64);
    info!("  Total Time:    {:>10.2} ms ({:.2} seconds)", 
        metrics.total_duration_ms as f64, total_time_secs);
    
    info!("\n{}", "─".repeat(80));
    info!("📈 COORDINATION vs COMPUTATION ANALYSIS");
    info!("{}", "─".repeat(80));
    info!("  Computation time:      {:>12.2} ms", metrics.compute_duration_ms as f64);
    info!("  Coordination time:    {:>12.2} ms", metrics.coordinate_duration_ms as f64);
    info!("  Granularity ratio:     {:>12.2}× (compute/coordinate)", metrics.granularity_ratio);
    info!("  Efficiency:            {:>12.2}% (compute/total)", metrics.efficiency * 100.0);
    info!("  Message count:         {:>12}", metrics.message_count);
    info!("  Barrier count:         {:>12}", metrics.barrier_count);
    
    // Cost analysis - show percentage breakdown
    let coord_cost_pct = if metrics.total_duration_ms > 0 {
        (metrics.coordinate_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
    } else {
        0.0
    };
    let compute_cost_pct = if metrics.total_duration_ms > 0 {
        (metrics.compute_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
    } else {
        0.0
    };
    info!("\n  Cost Breakdown:");
    info!("    Coordination overhead: {:>8.2}% of total time", coord_cost_pct);
    info!("    Computation:           {:>8.2}% of total time", compute_cost_pct);
    
    info!("\n{}", "─".repeat(80));
    info!("🚀 BENCHMARK METRICS");
    info!("{}", "─".repeat(80));
    info!("  Throughput:        {:>12.2} GFLOPS", gflops);
    info!("  Data throughput:   {:>12.2} MB/s", throughput_mb);
    info!("  Operations/sec:    {:>12.2} M ops/s", 
        if total_time_secs > 0.0 { (total_ops as f64 / 1_000_000.0) / total_time_secs } else { 0.0 });
    info!("  Estimated speedup: {:>12.2}× (vs sequential)", estimated_speedup);
    
    info!("\n{}", "─".repeat(80));
    info!("💡 ANALYSIS & RECOMMENDATIONS");
    info!("{}", "─".repeat(80));
    if metrics.granularity_ratio < 10.0 {
        info!("  ⚠️  WARNING: Overhead too high! Consider:");
        info!("     - Larger problem size (more rows/cols)");
        info!("     - Fewer workers (coarser granularity)");
        info!("     - Current ratio: {:.2}× (should be >= 10×)", metrics.granularity_ratio);
    } else if metrics.granularity_ratio < 100.0 {
        info!("  ✓  ACCEPTABLE: Reasonable granularity for this problem size");
        info!("     - Ratio: {:.2}× (good for small-medium problems)", metrics.granularity_ratio);
    } else {
        info!("  ✓  EXCELLENT: Good compute/coordinate ratio");
        info!("     - Ratio: {:.2}× (ideal for parallel efficiency)", metrics.granularity_ratio);
    }
    
    if coord_cost_pct > 20.0 {
        info!("  ⚠️  Coordination overhead is {:.1}% - consider larger problem size", coord_cost_pct);
    } else {
        info!("  ✓  Coordination overhead is {:.1}% - acceptable", coord_cost_pct);
    }
    
    info!("{}", "=".repeat(80));

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
