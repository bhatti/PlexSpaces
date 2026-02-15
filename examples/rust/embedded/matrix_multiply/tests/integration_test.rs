// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Integration Tests for Matrix Multiplication Example
//!
//! Tests the MatrixWorker actor using SDK patterns:
//! - SDK spawn helpers: `spawn()` for actor creation
//! - SDK message helpers: `GenServerRef.cast()` and `GenServerRef.call()`
//! - Scatter-gather pattern verification

use matrix_multiply::MatrixWorker;
use plexspaces_sdk::{spawn, GenServerRef, RequestContext, json};
use plexspaces_node::NodeBuilder;
use plexspaces_core::ActorId;
use std::time::Duration;

#[tokio::test]
async fn test_matrix_worker_spawn_and_compute() {
    // Setup: Create node and context
    let node = NodeBuilder::new("test-node".to_string())
        .build()
        .await;
    let service_locator = node.service_locator().clone();
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-ns".to_string());
    
    // Spawn MatrixWorker actor using SDK helper
    let actor_id = ActorId::from("worker-0@test-node");
    let worker = MatrixWorker::new(0);
    let actor_ref = spawn(&ctx, service_locator, actor_id.clone(), "test-ns", worker).await
        .expect("Failed to spawn MatrixWorker");
    let worker_ref = GenServerRef::new(actor_ref);
    
    // Test compute_rows via call (request-reply)
    let matrix_a = vec![
        vec![1.0, 2.0],
        vec![3.0, 4.0],
    ];
    let matrix_b = vec![
        vec![5.0, 6.0],
        vec![7.0, 8.0],
    ];
    
    let compute_request = json!({
        "start_row": 0,
        "end_row": 2,
        "matrix_a": matrix_a,
        "matrix_b": matrix_b,
    });
    
    let result: serde_json::Value = worker_ref.call("compute_rows", &compute_request).await
        .expect("compute_rows call failed");
    
    // Verify result
    assert_eq!(result["start_row"].as_u64().unwrap(), 0);
    let rows: Vec<Vec<f64>> = serde_json::from_value(result["rows"].clone()).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], 19.0);
    assert_eq!(rows[0][1], 22.0);
    assert_eq!(rows[1][0], 43.0);
    assert_eq!(rows[1][1], 50.0);
    
    node.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[tokio::test]
async fn test_matrix_worker_scatter_gather_pattern() {
    // Test scatter-gather pattern: cast for distribution, call for collection
    let node = NodeBuilder::new("test-node".to_string())
        .build()
        .await;
    let service_locator = node.service_locator().clone();
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-ns".to_string());
    
    // Spawn worker
    let actor_id = ActorId::from("worker-1@test-node");
    let worker = MatrixWorker::new(1);
    let actor_ref = spawn(&ctx, service_locator.clone(), actor_id.clone(), "test-ns", worker).await
        .expect("Failed to spawn MatrixWorker");
    let worker_ref = GenServerRef::new(actor_ref);
    
    // SCATTER: Use cast (fire-and-forget) to distribute work
    let matrix_a = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
    let matrix_b = vec![vec![5.0, 6.0], vec![7.0, 8.0]];
    
    let compute_request = json!({
        "start_row": 0,
        "end_row": 2,
        "matrix_a": matrix_a,
        "matrix_b": matrix_b,
    });
    
    worker_ref.cast("compute_rows", &compute_request).await
        .expect("cast failed");
    
    // GATHER: Use call to retrieve result
    let get_result_request = json!({});
    let result: serde_json::Value = worker_ref.call("get_result", &get_result_request).await
        .expect("get_result call failed");
    
    // Verify result was stored and retrieved correctly
    assert_eq!(result["start_row"].as_u64().unwrap(), 0);
    let rows: Vec<Vec<f64>> = serde_json::from_value(result["rows"].clone()).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], 19.0);
    
    node.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[tokio::test]
async fn test_matrix_worker_multiple_workers() {
    // Test multiple workers computing different row ranges
    let node = NodeBuilder::new("test-node".to_string())
        .build()
        .await;
    let service_locator = node.service_locator().clone();
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-ns".to_string());
    
    let matrix_a = vec![
        vec![1.0, 2.0],
        vec![3.0, 4.0],
        vec![5.0, 6.0],
    ];
    let matrix_b = vec![
        vec![1.0, 0.0],
        vec![0.0, 1.0],
    ];
    
    // Spawn two workers
    let worker0_id = ActorId::from("worker-0@test-node");
    let worker0_ref = GenServerRef::new(
        spawn(&ctx, service_locator.clone(), worker0_id.clone(), "test-ns", MatrixWorker::new(0)).await
            .expect("Failed to spawn worker 0")
    );
    
    let worker1_id = ActorId::from("worker-1@test-node");
    let worker1_ref = GenServerRef::new(
        spawn(&ctx, service_locator.clone(), worker1_id.clone(), "test-ns", MatrixWorker::new(1)).await
            .expect("Failed to spawn worker 1")
    );
    
    // Worker 0 computes rows 0-1
    worker0_ref.cast("compute_rows", &json!({
        "start_row": 0,
        "end_row": 1,
        "matrix_a": matrix_a.clone(),
        "matrix_b": matrix_b.clone(),
    })).await.expect("cast failed");
    
    // Worker 1 computes rows 1-3
    worker1_ref.cast("compute_rows", &json!({
        "start_row": 1,
        "end_row": 3,
        "matrix_a": matrix_a.clone(),
        "matrix_b": matrix_b.clone(),
    })).await.expect("cast failed");
    
    // Gather results
    let result0: serde_json::Value = worker0_ref.call("get_result", &json!({})).await
        .expect("get_result failed");
    let result1: serde_json::Value = worker1_ref.call("get_result", &json!({})).await
        .expect("get_result failed");
    
    // Verify results
    let rows0: Vec<Vec<f64>> = serde_json::from_value(result0["rows"].clone()).unwrap();
    let rows1: Vec<Vec<f64>> = serde_json::from_value(result1["rows"].clone()).unwrap();
    
    assert_eq!(rows0.len(), 1);
    assert_eq!(rows1.len(), 2);
    assert_eq!(rows0[0][0], 1.0);
    assert_eq!(rows0[0][1], 2.0);
    assert_eq!(rows1[0][0], 3.0);
    assert_eq!(rows1[0][1], 4.0);
    
    node.shutdown(Duration::from_secs(1)).await.unwrap();
}
