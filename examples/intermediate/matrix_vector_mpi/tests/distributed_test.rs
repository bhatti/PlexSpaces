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

//! Distributed Multi-Node Tests
//!
//! ## Purpose
//! Test matrix-vector multiplication across multiple processes/nodes
//! to validate distributed TupleSpace coordination.
//!
//! ## Test Scenario
//! - Node 1 (Master): Orchestrates computation, scatter, gather
//! - Node 2-N (Workers): Perform local computation
//! - Distributed TupleSpace: Coordinates via network
//!
//! ## Running
//! These tests are marked #[ignore] and require manual setup:
//! ```bash
//! # Terminal 1 - Start master node
//! cargo test --test distributed_test test_distributed_master -- --ignored --nocapture
//!
//! # Terminal 2 - Start worker nodes (in parallel)
//! cargo test --test distributed_test test_distributed_worker_0 -- --ignored --nocapture
//! cargo test --test distributed_test test_distributed_worker_1 -- --ignored --nocapture
//! ```

use matrix_vector_mpi::*;
use plexspaces_tuplespace::TupleSpace;
use plexspaces_node::{Node, NodeId, NodeConfig};
use std::sync::Arc;
use std::time::Instant;
use std::collections::HashMap;

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

/// Compute sequential result for verification
fn compute_sequential(matrix: &[Vec<f64>], vector: &[f64]) -> Vec<f64> {
    matrix.iter().map(|row| {
        row.iter().zip(vector)
            .map(|(a, b)| a * b)
            .sum()
    }).collect()
}

/// Helper to create a test node with proper configuration
fn create_test_node(id: &str, port: u16) -> Node {
    Node::new(
        NodeId::new(id),
        NodeConfig {
            listen_addr: format!("127.0.0.1:{}", port),
            max_connections: 100,
            heartbeat_interval_ms: 5000,
            clustering_enabled: true,
            metadata: HashMap::new(),
        }
    )
}

#[tokio::test]
#[ignore] // Requires manual multi-process setup
async fn test_distributed_master() {
    println!("\n=== MASTER NODE: Distributed Matrix-Vector Multiplication ===\n");

    let num_workers = 2;
    let num_rows = 1000;
    let num_cols = 500;

    // Create distributed node
    let node = create_test_node("master", 9001);
    let space = node.tuplespace(); // Returns Arc<TupleSpace>

    println!("Master node started at 127.0.0.1:9001");
    println!("Waiting for workers to connect...\n");

    // Give workers time to start
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    let matrix = create_test_matrix(num_rows, num_cols);
    let vector = create_test_vector(num_cols);

    let mut metrics = ComputeMetrics::new(num_rows, num_cols, num_workers);

    println!("Configuration:");
    println!("  Matrix: {}×{}", num_rows, num_cols);
    println!("  Workers: {}", num_workers);
    println!("  Rows per worker: {}\n", num_rows / num_workers);

    // Phase 1: Scatter (distributed)
    println!("=== Phase 1: Scatter (Distributed) ===");
    let scatter_start = Instant::now();
    scatter_rows(&space, &matrix, num_workers).await.unwrap();
    metrics.record_scatter(scatter_start.elapsed());
    println!("  Scattered {} rows to {} workers", num_rows, num_workers);

    // Phase 2: Broadcast
    println!("\n=== Phase 2: Broadcast (Distributed) ===");
    let broadcast_start = Instant::now();
    broadcast_vector(&space, &vector).await.unwrap();
    metrics.record_broadcast(broadcast_start.elapsed());
    println!("  Broadcast vector to all workers");

    // Phase 3: Workers compute (on remote nodes)
    println!("\n=== Phase 3: Remote Computation ===");
    println!("  Workers computing on remote nodes...");
    let compute_start = Instant::now();

    // Wait for workers to compute (they will write barrier tuples)
    // We don't spawn local tasks - workers are on remote nodes
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    metrics.record_compute(compute_start.elapsed());

    // Phase 4: Barrier (distributed synchronization)
    println!("\n=== Phase 4: Distributed Barrier ===");
    let barrier_start = Instant::now();
    barrier_sync(&space, num_workers).await.unwrap();
    metrics.record_barrier(barrier_start.elapsed());

    // Phase 5: Gather (distributed)
    println!("\n=== Phase 5: Gather (Distributed) ===");
    let gather_start = Instant::now();
    let result = gather_results(&space, num_workers, num_rows).await.unwrap();
    metrics.record_gather(gather_start.elapsed());
    println!("  Gathered {} results from {} workers", result.len(), num_workers);

    // Verify
    let expected = compute_sequential(&matrix, &vector);
    assert_eq!(result, expected, "Distributed result must match sequential");

    println!("\n✅ Distributed computation successful!");
    println!("\n");
    metrics.print_detailed();

    // Distributed-specific metrics
    println!("\n=== Distributed Performance ===");
    println!("  Network coordination time: {:.3}ms",
        metrics.total_coordination_time().as_secs_f64() * 1000.0);
    println!("  Remote compute time: {:.3}ms",
        metrics.compute_time().as_secs_f64() * 1000.0);

    println!("\nMaster node test complete. Press Ctrl+C to stop.");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
}

#[tokio::test]
#[ignore] // Requires manual multi-process setup
async fn test_distributed_worker_0() {
    println!("\n=== WORKER 0: Waiting for tasks ===\n");

    // Connect to master
    let node = create_test_node("worker0", 9002);
    let space = node.tuplespace(); // Returns Arc<TupleSpace>

    // Connect to master node
    node.connect_to(
        NodeId::new("master"),
        "http://127.0.0.1:9001".to_string()
    ).await.unwrap();

    println!("Worker 0 connected to master at 127.0.0.1:9001");
    println!("Listening on 127.0.0.1:9002\n");

    // Wait for computation task
    println!("Waiting for scatter...");
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Execute worker computation
    let worker_id = 0;
    let num_cols = 500; // Must match master

    println!("Executing worker_compute for worker_id={}", worker_id);
    worker_compute(space.clone(), worker_id, num_cols).await.unwrap(); // space is already Arc<TupleSpace>

    println!("✅ Worker 0 computation complete");
    println!("Waiting for gather...");

    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    println!("Worker 0 test complete.");
}

#[tokio::test]
#[ignore] // Requires manual multi-process setup
async fn test_distributed_worker_1() {
    println!("\n=== WORKER 1: Waiting for tasks ===\n");

    // Connect to master
    let node = create_test_node("worker1", 9003);
    let space = node.tuplespace(); // Returns Arc<TupleSpace>

    // Connect to master node
    node.connect_to(
        NodeId::new("master"),
        "http://127.0.0.1:9001".to_string()
    ).await.unwrap();

    println!("Worker 1 connected to master at 127.0.0.1:9001");
    println!("Listening on 127.0.0.1:9003\n");

    // Wait for computation task
    println!("Waiting for scatter...");
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Execute worker computation
    let worker_id = 1;
    let num_cols = 500; // Must match master

    println!("Executing worker_compute for worker_id={}", worker_id);
    worker_compute(space.clone(), worker_id, num_cols).await.unwrap(); // space is already Arc<TupleSpace>

    println!("✅ Worker 1 computation complete");
    println!("Waiting for gather...");

    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    println!("Worker 1 test complete.");
}

#[tokio::test]
#[ignore] // Requires manual setup
async fn test_distributed_4_workers() {
    println!("\n=== MASTER NODE: 4-Worker Distributed Test ===\n");

    let num_workers = 4;
    let num_rows = 2000;
    let num_cols = 1000;

    let node = create_test_node("master", 9001);
    let space = node.tuplespace(); // Returns Arc<TupleSpace>

    println!("Master node started. Waiting for 4 workers...\n");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    let matrix = create_test_matrix(num_rows, num_cols);
    let vector = create_test_vector(num_cols);

    let mut metrics = ComputeMetrics::new(num_rows, num_cols, num_workers);

    println!("Configuration:");
    println!("  Matrix: {}×{}", num_rows, num_cols);
    println!("  Workers: {}", num_workers);
    println!("  Rows per worker: {}\n", num_rows / num_workers);

    // Execute distributed pipeline
    let scatter_start = Instant::now();
    scatter_rows(&space, &matrix, num_workers).await.unwrap();
    metrics.record_scatter(scatter_start.elapsed());

    let broadcast_start = Instant::now();
    broadcast_vector(&space, &vector).await.unwrap();
    metrics.record_broadcast(broadcast_start.elapsed());

    println!("Workers computing remotely...");
    let compute_start = Instant::now();
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
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

    println!("\n✅ 4-worker distributed computation successful!");
    println!("\n");
    metrics.print_detailed();

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
}

#[tokio::test]
async fn test_distributed_simulation() {
    println!("\n=== Simulated Distributed Test (Single Process) ===\n");
    println!("NOTE: This simulates distributed behavior in a single process");
    println!("      For real distributed tests, use the --ignored tests\n");

    let num_workers = 3;
    let num_rows = 1500;
    let num_cols = 750;

    // Simulate distributed TupleSpace (shared in-memory)
    let space = Arc::new(TupleSpace::new());

    let matrix = create_test_matrix(num_rows, num_cols);
    let vector = create_test_vector(num_cols);

    let mut metrics = ComputeMetrics::new(num_rows, num_cols, num_workers);

    println!("Simulated Configuration:");
    println!("  Nodes: 1 master + {} workers", num_workers);
    println!("  Matrix: {}×{}", num_rows, num_cols);
    println!("  Rows per worker: {}\n", num_rows / num_workers);

    // Scatter
    let scatter_start = Instant::now();
    scatter_rows(&space, &matrix, num_workers).await.unwrap();
    metrics.record_scatter(scatter_start.elapsed());

    // Broadcast
    let broadcast_start = Instant::now();
    broadcast_vector(&space, &vector).await.unwrap();
    metrics.record_broadcast(broadcast_start.elapsed());

    // Workers compute (simulated as concurrent tasks)
    let compute_start = Instant::now();
    let mut worker_handles = Vec::new();
    for worker_id in 0..num_workers {
        let space_clone = space.clone();
        let handle = tokio::spawn(async move {
            // Simulate network latency
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            worker_compute(space_clone, worker_id, num_cols).await
        });
        worker_handles.push(handle);
    }
    for handle in worker_handles {
        handle.await.unwrap().unwrap();
    }
    metrics.record_compute(compute_start.elapsed());

    // Barrier
    let barrier_start = Instant::now();
    barrier_sync(&space, num_workers).await.unwrap();
    metrics.record_barrier(barrier_start.elapsed());

    // Gather
    let gather_start = Instant::now();
    let result = gather_results(&space, num_workers, num_rows).await.unwrap();
    metrics.record_gather(gather_start.elapsed());

    // Verify
    let expected = compute_sequential(&matrix, &vector);
    assert_eq!(result, expected);

    println!("✅ Simulated distributed test passed");
    println!("\n");
    metrics.print_detailed();
}
