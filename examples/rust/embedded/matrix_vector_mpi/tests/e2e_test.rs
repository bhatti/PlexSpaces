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

//! End-to-End Tests for Matrix-Vector Multiplication
//!
//! ## Purpose
//! Test the complete MPI-style matrix-vector multiplication pipeline
//! with detailed performance metrics.

use matrix_vector_mpi::*;
use plexspaces_tuplespace::TupleSpace;
use std::sync::Arc;
use std::time::Instant;

/// Create test matrix (identity-based for easy verification)
fn create_test_matrix(rows: usize, cols: usize) -> Vec<Vec<f64>> {
    (0..rows).map(|i| {
        (0..cols).map(|j| (i * cols + j + 1) as f64).collect()
    }).collect()
}

/// Create test vector
fn create_test_vector(size: usize) -> Vec<f64> {
    (1..=size).map(|i| i as f64).collect()
}

/// Compute sequential result for verification
fn compute_sequential(matrix: &[Vec<f64>], vector: &[f64]) -> Vec<f64> {
    matrix.iter().map(|row| {
        row.iter().zip(vector)
            .map(|(a, b)| a * b)
            .sum()
    }).collect()
}

#[tokio::test]
async fn test_matrix_vector_2x2_workers() {
    println!("\n=== E2E Test: 2 Workers, 1000×500 Matrix ===\n");

    let num_workers = 2;
    let num_rows = 1000;
    let num_cols = 500;

    let matrix = create_test_matrix(num_rows, num_cols);
    let vector = create_test_vector(num_cols);
    let space = Arc::new(TupleSpace::with_tenant_namespace("internal", "system"));

    let mut metrics = ComputeMetrics::new(num_rows, num_cols, num_workers);

    // Phase 1: Scatter
    println!("Phase 1: Scatter");
    let scatter_start = Instant::now();
    scatter_rows(&space, &matrix, num_workers).await.unwrap();
    metrics.record_scatter(scatter_start.elapsed());

    // Phase 2: Broadcast
    println!("Phase 2: Broadcast");
    let broadcast_start = Instant::now();
    broadcast_vector(&space, &vector).await.unwrap();
    metrics.record_broadcast(broadcast_start.elapsed());

    // Phase 3: Compute
    println!("Phase 3: Compute");
    let compute_start = Instant::now();
    let mut worker_handles = Vec::new();
    for worker_id in 0..num_workers {
        let space_clone = space.clone();
        let handle = tokio::spawn(async move {
            worker_compute(space_clone, worker_id, num_cols).await
        });
        worker_handles.push(handle);
    }
    for handle in worker_handles {
        handle.await.unwrap().unwrap();
    }
    metrics.record_compute(compute_start.elapsed());

    // Phase 4: Barrier
    println!("Phase 4: Barrier");
    let barrier_start = Instant::now();
    barrier_sync(&space, num_workers).await.unwrap();
    metrics.record_barrier(barrier_start.elapsed());

    // Phase 5: Gather
    println!("Phase 5: Gather");
    let gather_start = Instant::now();
    let result = gather_results(&space, num_workers, num_rows).await.unwrap();
    metrics.record_gather(gather_start.elapsed());

    // Verify
    let expected = compute_sequential(&matrix, &vector);
    assert_eq!(result, expected, "Parallel result must match sequential");

    // Print detailed metrics
    println!("\n");
    metrics.print_detailed();

    // Assertions on performance characteristics
    assert!(metrics.granularity_ratio() > 0.1,
        "Granularity ratio too low: {}", metrics.granularity_ratio());
    assert!(metrics.total_time().as_millis() < 10000,
        "Total time too high: {}ms", metrics.total_time().as_millis());
}

#[tokio::test]
async fn test_matrix_vector_4_workers() {
    println!("\n=== E2E Test: 4 Workers, 2000×1000 Matrix ===\n");

    let num_workers = 4;
    let num_rows = 2000;
    let num_cols = 1000;

    let matrix = create_test_matrix(num_rows, num_cols);
    let vector = create_test_vector(num_cols);
    let space = Arc::new(TupleSpace::with_tenant_namespace("internal", "system"));

    let mut metrics = ComputeMetrics::new(num_rows, num_cols, num_workers);

    // Execute all phases
    let scatter_start = Instant::now();
    scatter_rows(&space, &matrix, num_workers).await.unwrap();
    metrics.record_scatter(scatter_start.elapsed());

    let broadcast_start = Instant::now();
    broadcast_vector(&space, &vector).await.unwrap();
    metrics.record_broadcast(broadcast_start.elapsed());

    let compute_start = Instant::now();
    let mut worker_handles = Vec::new();
    for worker_id in 0..num_workers {
        let space_clone = space.clone();
        let handle = tokio::spawn(async move {
            worker_compute(space_clone, worker_id, num_cols).await
        });
        worker_handles.push(handle);
    }
    for handle in worker_handles {
        handle.await.unwrap().unwrap();
    }
    metrics.record_compute(compute_start.elapsed());

    let barrier_start = Instant::now();
    barrier_sync(&space, num_workers).await.unwrap();
    metrics.record_barrier(barrier_start.elapsed());

    let gather_start = Instant::now();
    let result = gather_results(&space, num_workers, num_rows).await.unwrap();
    metrics.record_gather(gather_start.elapsed());

    // Verify
    let expected = compute_sequential(&matrix, &vector);
    assert_eq!(result, expected);

    println!("\n");
    metrics.print_detailed();

    // Check scaling characteristics
    println!("\nScaling Analysis:");
    println!("  Workers: {}", num_workers);
    println!("  Rows per worker: {}", num_rows / num_workers);
    println!("  Granularity ratio: {:.2}×", metrics.granularity_ratio());
    println!("  Efficiency: {:.1}%", metrics.efficiency() * 100.0);
}

#[tokio::test]
async fn test_matrix_vector_large_problem() {
    println!("\n=== E2E Test: 8 Workers, 5000×2000 Matrix (Large Problem) ===\n");

    let num_workers = 8;
    let num_rows = 5000;
    let num_cols = 2000;

    let matrix = create_test_matrix(num_rows, num_cols);
    let vector = create_test_vector(num_cols);
    let space = Arc::new(TupleSpace::with_tenant_namespace("internal", "system"));

    let mut metrics = ComputeMetrics::new(num_rows, num_cols, num_workers);

    // Execute all phases
    let scatter_start = Instant::now();
    scatter_rows(&space, &matrix, num_workers).await.unwrap();
    metrics.record_scatter(scatter_start.elapsed());

    let broadcast_start = Instant::now();
    broadcast_vector(&space, &vector).await.unwrap();
    metrics.record_broadcast(broadcast_start.elapsed());

    let compute_start = Instant::now();
    let mut worker_handles = Vec::new();
    for worker_id in 0..num_workers {
        let space_clone = space.clone();
        let handle = tokio::spawn(async move {
            worker_compute(space_clone, worker_id, num_cols).await
        });
        worker_handles.push(handle);
    }
    for handle in worker_handles {
        handle.await.unwrap().unwrap();
    }
    metrics.record_compute(compute_start.elapsed());

    let barrier_start = Instant::now();
    barrier_sync(&space, num_workers).await.unwrap();
    metrics.record_barrier(barrier_start.elapsed());

    let gather_start = Instant::now();
    let result = gather_results(&space, num_workers, num_rows).await.unwrap();
    metrics.record_gather(gather_start.elapsed());

    // Verify
    let expected = compute_sequential(&matrix, &vector);
    assert_eq!(result, expected);

    println!("\n");
    metrics.print_detailed();

    // For large problems, granularity ratio should be better than small problems
    // Note: In-memory TupleSpace is so fast that even 100×50 doesn't show huge gains
    // In real distributed scenarios with network latency, this would be much better
    assert!(metrics.granularity_ratio() > 0.5,
        "Large problem should have reasonable granularity: {:.2}×",
        metrics.granularity_ratio());

    println!("\nLarge Problem Analysis:");
    println!("  Compute operations: {}", metrics.compute_operations());
    println!("  Coordination operations: {}", metrics.coordination_operations());
    println!("  Ratio: {:.2}× (compute/coord)",
        metrics.compute_operations() as f64 / metrics.coordination_operations() as f64);
}

#[tokio::test]
async fn test_granularity_comparison() {
    println!("\n=== Granularity Comparison: Different Problem Sizes ===\n");

    let test_cases = vec![
        ("Small (500×250, 2 workers)", 500, 250, 2),
        ("Medium (1500×750, 4 workers)", 1500, 750, 4),
        ("Large (3000×1500, 8 workers)", 3000, 1500, 8),
    ];

    for (name, num_rows, num_cols, num_workers) in test_cases {
        println!("\n--- {} ---", name);

        let matrix = create_test_matrix(num_rows, num_cols);
        let vector = create_test_vector(num_cols);
        let space = Arc::new(TupleSpace::with_tenant_namespace("internal", "system"));

        let mut metrics = ComputeMetrics::new(num_rows, num_cols, num_workers);

        // Execute pipeline
        let scatter_start = Instant::now();
        scatter_rows(&space, &matrix, num_workers).await.unwrap();
        metrics.record_scatter(scatter_start.elapsed());

        let broadcast_start = Instant::now();
        broadcast_vector(&space, &vector).await.unwrap();
        metrics.record_broadcast(broadcast_start.elapsed());

        let compute_start = Instant::now();
        let mut worker_handles = Vec::new();
        for worker_id in 0..num_workers {
            let space_clone = space.clone();
            let handle = tokio::spawn(async move {
                worker_compute(space_clone, worker_id, num_cols).await
            });
            worker_handles.push(handle);
        }
        for handle in worker_handles {
            handle.await.unwrap().unwrap();
        }
        metrics.record_compute(compute_start.elapsed());

        let barrier_start = Instant::now();
        barrier_sync(&space, num_workers).await.unwrap();
        metrics.record_barrier(barrier_start.elapsed());

        let gather_start = Instant::now();
        let result = gather_results(&space, num_workers, num_rows).await.unwrap();
        metrics.record_gather(gather_start.elapsed());

        // Verify
        let expected = compute_sequential(&matrix, &vector);
        assert_eq!(result, expected);

        // Print compact metrics
        metrics.print_summary();
    }

    println!("\n=== Key Insight ===");
    println!("Larger problems have better compute/coordination ratios");
    println!("This demonstrates the importance of granularity in parallel computing");
}

#[tokio::test]
async fn test_latency_breakdown() {
    println!("\n=== Latency Breakdown Test ===\n");

    let num_workers = 4;
    let num_rows = 1000;
    let num_cols = 500;

    let matrix = create_test_matrix(num_rows, num_cols);
    let vector = create_test_vector(num_cols);
    let space = Arc::new(TupleSpace::with_tenant_namespace("internal", "system"));

    let mut metrics = ComputeMetrics::new(num_rows, num_cols, num_workers);

    // Measure each phase separately
    println!("Measuring individual phase latencies...\n");

    let scatter_start = Instant::now();
    scatter_rows(&space, &matrix, num_workers).await.unwrap();
    let scatter_time = scatter_start.elapsed();
    metrics.record_scatter(scatter_time);
    println!("  Scatter latency: {:.3}ms", scatter_time.as_secs_f64() * 1000.0);

    let broadcast_start = Instant::now();
    broadcast_vector(&space, &vector).await.unwrap();
    let broadcast_time = broadcast_start.elapsed();
    metrics.record_broadcast(broadcast_time);
    println!("  Broadcast latency: {:.3}ms", broadcast_time.as_secs_f64() * 1000.0);

    let compute_start = Instant::now();
    let mut worker_handles = Vec::new();
    for worker_id in 0..num_workers {
        let space_clone = space.clone();
        let handle = tokio::spawn(async move {
            worker_compute(space_clone, worker_id, num_cols).await
        });
        worker_handles.push(handle);
    }
    for handle in worker_handles {
        handle.await.unwrap().unwrap();
    }
    let compute_time = compute_start.elapsed();
    metrics.record_compute(compute_time);
    println!("  Compute latency: {:.3}ms", compute_time.as_secs_f64() * 1000.0);

    let barrier_start = Instant::now();
    barrier_sync(&space, num_workers).await.unwrap();
    let barrier_time = barrier_start.elapsed();
    metrics.record_barrier(barrier_time);
    println!("  Barrier latency: {:.3}ms", barrier_time.as_secs_f64() * 1000.0);

    let gather_start = Instant::now();
    let result = gather_results(&space, num_workers, num_rows).await.unwrap();
    let gather_time = gather_start.elapsed();
    metrics.record_gather(gather_time);
    println!("  Gather latency: {:.3}ms", gather_time.as_secs_f64() * 1000.0);

    // Verify
    let expected = compute_sequential(&matrix, &vector);
    assert_eq!(result, expected);

    println!("\n");
    metrics.print_detailed();

    // Latency assertions - adjusted for realistic problem sizes
    assert!(scatter_time.as_millis() < 5000, "Scatter too slow: {}ms", scatter_time.as_millis());
    assert!(broadcast_time.as_millis() < 100, "Broadcast too slow: {}ms", broadcast_time.as_millis());
    assert!(compute_time.as_millis() < 5000, "Compute too slow: {}ms", compute_time.as_millis());
    assert!(barrier_time.as_millis() < 100, "Barrier too slow: {}ms", barrier_time.as_millis());
    assert!(gather_time.as_millis() < 100, "Gather too slow: {}ms", gather_time.as_millis());
}
