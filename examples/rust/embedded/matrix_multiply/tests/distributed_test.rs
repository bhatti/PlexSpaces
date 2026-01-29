// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Distributed Multi-Node Tests
//!
//! Tests matrix multiplication with multiple nodes communicating via gRPC

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use plexspaces_node::{Node, NodeId, NodeConfig};

use matrix_multiply::master::MasterActor;
use matrix_multiply::worker::WorkerActor;

/// Helper to create a test node with specific port
fn create_test_node(id: &str, port: u16) -> Node {
    let mut config = NodeConfig::default();
    config.listen_addr = format!("127.0.0.1:{}", port);
    config.heartbeat_interval_ms = 100; // Fast heartbeat for tests

    Node::new(NodeId::new(id), config)
}

#[tokio::test]
#[ignore] // Ignored by default - requires running gRPC servers
async fn test_distributed_matrix_multiply_two_nodes() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("matrix_multiply=debug")
        .try_init();

    println!("\n🌐 Distributed Test: Matrix Multiply Across 2 Nodes");
    println!("{}", "=".repeat(60));

    // Create 2 nodes
    let node1 = Arc::new(create_test_node("node1", 9101));
    let node2 = Arc::new(create_test_node("node2", 9102));

    // Start node1 in background
    let node1_clone = node1.clone();
    let node1_handle = tokio::spawn(async move {
        if let Err(e) = node1_clone.start().await {
            eprintln!("Node1 error: {:?}", e);
        }
    });

    // Start node2 in background
    let node2_clone = node2.clone();
    let node2_handle = tokio::spawn(async move {
        if let Err(e) = node2_clone.start().await {
            eprintln!("Node2 error: {:?}", e);
        }
    });

    // Wait for nodes to start
    sleep(Duration::from_millis(500)).await;
    println!("✓ Nodes started");

    // Register node2 as remote in node1's registry
    node1.register_remote_node(
        NodeId::new("node2"),
        "http://127.0.0.1:9102".to_string()
    ).await.expect("Failed to register node2");

    println!("✓ Nodes connected");

    // Configuration
    let matrix_size = 8;
    let block_size = 2;
    let workers_per_node = 2;

    println!("\nConfiguration:");
    println!("  Matrix: {}×{}", matrix_size, matrix_size);
    println!("  Blocks: {}×{}", block_size, block_size);
    println!("  Workers per node: {}", workers_per_node);
    println!("  Total workers: {}", workers_per_node * 2);

    // Spawn workers on node1
    let mut node1_workers = Vec::new();
    for i in 0..workers_per_node {
        let worker_id = format!("worker-node1-{}", i);
        let worker = WorkerActor::new(worker_id.clone(), node1.tuplespace().clone());
        println!("  Spawning worker: {}", worker_id);

        let handle = tokio::spawn(async move {
            if let Err(e) = worker.run().await {
                eprintln!("Worker error: {:?}", e);
            }
        });
        node1_workers.push(handle);
    }

    // Spawn workers on node2
    let mut node2_workers = Vec::new();
    for i in 0..workers_per_node {
        let worker_id = format!("worker-node2-{}", i);
        let worker = WorkerActor::new(worker_id.clone(), node2.tuplespace().clone());
        println!("  Spawning worker: {}", worker_id);

        let handle = tokio::spawn(async move {
            if let Err(e) = worker.run().await {
                eprintln!("Worker error: {:?}", e);
            }
        });
        node2_workers.push(handle);
    }

    sleep(Duration::from_millis(200)).await;
    println!("✓ Workers spawned");

    // Create master on node1
    let master = MasterActor::new(
        "master@node1".to_string(),
        node1.tuplespace().clone(),
        matrix_size,
        block_size,
    );

    println!("\n▶ Running distributed matrix multiplication...");

    // Run multiplication
    master.run().await.expect("Multiplication should succeed");

    println!("✓ Matrix multiplication completed successfully");

    // Cleanup
    println!("\nCleaning up...");
    for handle in node1_workers {
        handle.abort();
    }
    for handle in node2_workers {
        handle.abort();
    }

    node1_handle.abort();
    node2_handle.abort();

    println!("✓ Test complete");
    println!("{}", "=".repeat(60));
}

#[tokio::test]
#[ignore] // Ignored by default - requires running gRPC servers
async fn test_distributed_matrix_multiply_three_nodes() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("matrix_multiply=debug")
        .try_init();

    println!("\n🌐 Distributed Test: Matrix Multiply Across 3 Nodes");
    println!("{}", "=".repeat(60));

    // Create 3 nodes
    let node1 = Arc::new(create_test_node("node1", 9201));
    let node2 = Arc::new(create_test_node("node2", 9202));
    let node3 = Arc::new(create_test_node("node3", 9203));

    // Start nodes in background
    let node1_clone = node1.clone();
    let node1_handle = tokio::spawn(async move {
        if let Err(e) = node1_clone.start().await {
            eprintln!("Node1 error: {:?}", e);
        }
    });

    let node2_clone = node2.clone();
    let node2_handle = tokio::spawn(async move {
        if let Err(e) = node2_clone.start().await {
            eprintln!("Node2 error: {:?}", e);
        }
    });

    let node3_clone = node3.clone();
    let node3_handle = tokio::spawn(async move {
        if let Err(e) = node3_clone.start().await {
            eprintln!("Node3 error: {:?}", e);
        }
    });

    // Wait for nodes to start
    sleep(Duration::from_millis(500)).await;
    println!("✓ 3 Nodes started");

    // Register remote nodes
    node1.register_remote_node(NodeId::new("node2"), "http://127.0.0.1:9202".to_string())
        .await.expect("Failed to register node2");
    node1.register_remote_node(NodeId::new("node3"), "http://127.0.0.1:9203".to_string())
        .await.expect("Failed to register node3");

    println!("✓ Nodes connected");

    // Configuration - larger matrix for 3 nodes
    let matrix_size = 12;
    let block_size = 3;
    let workers_per_node = 3;

    println!("\nConfiguration:");
    println!("  Matrix: {}×{}", matrix_size, matrix_size);
    println!("  Blocks: {}×{}", block_size, block_size);
    println!("  Workers per node: {}", workers_per_node);
    println!("  Total workers: {}", workers_per_node * 3);

    // Spawn workers on all nodes
    let mut all_worker_handles = Vec::new();

    for (node, node_name) in [(&node1, "node1"), (&node2, "node2"), (&node3, "node3")] {
        for i in 0..workers_per_node {
            let worker_id = format!("worker-{}-{}", node_name, i);
            let worker = WorkerActor::new(worker_id.clone(), node.tuplespace().clone());
            println!("  Spawning worker: {}", worker_id);

            let handle = tokio::spawn(async move {
                if let Err(e) = worker.run().await {
                    eprintln!("Worker error: {:?}", e);
                }
            });
            all_worker_handles.push(handle);
        }
    }

    sleep(Duration::from_millis(300)).await;
    println!("✓ {} workers spawned across 3 nodes", workers_per_node * 3);

    // Create master on node1
    let master = MasterActor::new(
        "master@node1".to_string(),
        node1.tuplespace().clone(),
        matrix_size,
        block_size,
    );

    println!("\n▶ Running distributed matrix multiplication...");

    // Run multiplication
    master.run().await.expect("Multiplication should succeed");

    println!("✓ Matrix multiplication completed successfully");

    // Cleanup
    println!("\nCleaning up...");
    for handle in all_worker_handles {
        handle.abort();
    }

    node1_handle.abort();
    node2_handle.abort();
    node3_handle.abort();

    println!("✓ Test complete");
    println!("{}", "=".repeat(60));
}
