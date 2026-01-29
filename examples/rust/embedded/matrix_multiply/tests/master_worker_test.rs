// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Master and Worker Integration Tests
//!
//! Tests the full master-worker flow with TupleSpace coordination

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use plexspaces_node::{Node, NodeId, NodeConfig};

use matrix_multiply::master::MasterActor;
use matrix_multiply::worker::WorkerActor;

#[tokio::test]
async fn test_master_worker_single_block() {
    // Test with 2×2 matrix and 2×2 block (single work item)
    let node = Arc::new(Node::new(
        NodeId::new("test-node"),
        NodeConfig::default(),
    ));

    // Spawn one worker
    let worker = WorkerActor::new("worker-0".to_string(), node.tuplespace().clone());
    let worker_handle = tokio::spawn(async move {
        // Run worker in background but limit iterations
        for _ in 0..10 {
            if let Err(e) = tokio::time::timeout(
                Duration::from_millis(500),
                worker.run()
            ).await {
                // Timeout is expected when no more work
                break;
            }
        }
    });

    // Small delay to let worker start
    sleep(Duration::from_millis(50)).await;

    // Create and run master
    let master = MasterActor::new(
        "master".to_string(),
        node.tuplespace().clone(),
        2, // matrix_size
        2, // block_size (single 2×2 block)
    );

    // Run master - should complete successfully
    let result = master.run().await;
    assert!(result.is_ok(), "Master should complete successfully");

    // Cleanup
    worker_handle.abort();
}

#[tokio::test]
async fn test_master_worker_multiple_blocks() {
    // Test with 4×4 matrix and 2×2 blocks (4 work items)
    let node = Arc::new(Node::new(
        NodeId::new("test-node"),
        NodeConfig::default(),
    ));

    // Spawn 2 workers
    let mut worker_handles = Vec::new();
    for i in 0..2 {
        let worker_id = format!("worker-{}", i);
        let worker = WorkerActor::new(worker_id, node.tuplespace().clone());

        let handle = tokio::spawn(async move {
            // Run worker with timeout
            for _ in 0..10 {
                if tokio::time::timeout(
                    Duration::from_millis(500),
                    worker.run()
                ).await.is_err() {
                    break;
                }
            }
        });
        worker_handles.push(handle);
    }

    sleep(Duration::from_millis(50)).await;

    // Create and run master
    let master = MasterActor::new(
        "master".to_string(),
        node.tuplespace().clone(),
        4, // matrix_size
        2, // block_size (4 blocks total)
    );

    let result = master.run().await;
    assert!(result.is_ok(), "Master should complete successfully with 4 blocks");

    // Cleanup
    for handle in worker_handles {
        handle.abort();
    }
}

#[tokio::test]
async fn test_master_worker_8x8_matrix() {
    // Test with 8×8 matrix and 2×2 blocks (16 work items)
    let node = Arc::new(Node::new(
        NodeId::new("test-node"),
        NodeConfig::default(),
    ));

    // Spawn 4 workers
    let mut worker_handles = Vec::new();
    for i in 0..4 {
        let worker_id = format!("worker-{}", i);
        let worker = WorkerActor::new(worker_id, node.tuplespace().clone());

        let handle = tokio::spawn(async move {
            for _ in 0..20 {
                if tokio::time::timeout(
                    Duration::from_millis(500),
                    worker.run()
                ).await.is_err() {
                    break;
                }
            }
        });
        worker_handles.push(handle);
    }

    sleep(Duration::from_millis(100)).await;

    // Create and run master
    let master = MasterActor::new(
        "master".to_string(),
        node.tuplespace().clone(),
        8, // matrix_size
        2, // block_size (16 blocks total)
    );

    let result = master.run().await;
    assert!(result.is_ok(), "Master should complete successfully with 16 blocks");

    // Cleanup
    for handle in worker_handles {
        handle.abort();
    }
}

#[tokio::test]
async fn test_worker_handles_invalid_payload() {
    // Test that worker gracefully handles invalid tuple payloads
    let node = Arc::new(Node::new(
        NodeId::new("test-node"),
        NodeConfig::default(),
    ));

    // Write an invalid work tuple (wrong payload format)
    use plexspaces_tuplespace::{Tuple, TupleField};

    let invalid_tuple = Tuple::new(vec![
        TupleField::String("work".to_string()),
        TupleField::Integer(0),
        TupleField::Integer(0),
        TupleField::Binary(vec![1, 2, 3]), // Invalid payload
    ]);

    node.tuplespace().write(invalid_tuple).await.expect("Should write tuple");

    // Spawn worker
    let worker = WorkerActor::new("worker-0".to_string(), node.tuplespace().clone());

    // Run worker - should handle invalid payload gracefully
    let worker_handle = tokio::spawn(async move {
        tokio::time::timeout(
            Duration::from_millis(200),
            worker.run()
        ).await.ok();
    });

    sleep(Duration::from_millis(300)).await;

    worker_handle.abort();
    // Test passes if no panic occurred
}

#[tokio::test]
async fn test_master_with_no_workers() {
    // Test that master timeout/fails gracefully when no workers available
    let node = Arc::new(Node::new(
        NodeId::new("test-node"),
        NodeConfig::default(),
    ));

    let master = MasterActor::new(
        "master".to_string(),
        node.tuplespace().clone(),
        2,
        2,
    );

    // Run master with very short timeout expectation
    // This will hang waiting for workers, so we timeout the test
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        master.run()
    ).await;

    // Should timeout since no workers are processing
    assert!(result.is_err(), "Master should timeout when no workers available");
}
