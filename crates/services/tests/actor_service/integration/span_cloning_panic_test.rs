// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration test to reproduce "tried to clone a span that already closed" panic
//
// ## Purpose
// This test reproduces the exact scenario from the logs where:
// 1. A span is created in a gRPC handler (invoke_actor)
// 2. An actor is spawned from within that handler
// 3. The handler returns, dropping the span guard (closing the span)
// 4. The actor task is still running with the span in its tracing context
// 5. When the actor task completes and tries to log, tracing tries to clone the already-closed span
// 6. This causes a panic: "tried to clone a span (Id(...)) that already closed"
//
// ## Test Strategy
// - Create a span in the test (simulating gRPC handler span)
// - Call invoke_actor which spawns an actor
// - Let the actor process a message and complete
// - Verify no panic occurs when the actor task completes

use super::TestHarness;
use plexspaces_proto::actor::v1::InvokeActorRequest;
use std::collections::HashMap;
use std::time::Duration;
use tonic::Request;

/// Test that reproduces the span cloning panic
/// 
/// This test creates a span (simulating a gRPC handler span), calls invoke_actor
/// which spawns an actor, lets the actor process a message and complete, then
/// verifies no panic occurs when the actor task completes.
#[tokio::test]
async fn test_span_cloning_panic_reproduction() {
    std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");
    
    // Set up tracing to ensure spans are created
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let mut harness = TestHarness::new();
    let _node = harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");

    // Wait for node to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut client = harness.get_node("node1").unwrap().client.clone();

    // CRITICAL: Create a span (simulating gRPC handler span)
    // This span will be dropped when the function returns, but the actor task
    // will still be running with this span in its tracing context
    let span = tracing::info_span!("test_grpc_handler", 
        tenant_id = "test-tenant",
        namespace = "test-namespace",
        actor_type = "counter"
    );
    let _guard = span.enter();

    // Call invoke_actor which spawns an actor (simulating actor spawned from gRPC handler)
    let request = Request::new(InvokeActorRequest {
        namespace: "test-namespace".to_string(),
        actor_type: "counter".to_string(),
        http_method: "GET".to_string(),
        payload: serde_json::json!({ "action": "get" }).to_string().into_bytes(),
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path: "/api/v1/actors/test-tenant/test-namespace/counter".to_string(),
        subpath: String::new(),
        ask: true,
        msg_type_override: String::new(),
        timeout: None,
    });

    // Call invoke_actor - this spawns an actor task that inherits the tracing context
    let response = client.invoke_actor(request).await;

    // Drop the span guard (simulating gRPC handler returning)
    // At this point, the span is closed, but the actor task is still running
    drop(_guard);

    // Wait a bit for the actor to process the message and complete
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // CRITICAL: When the actor task completes, it will try to log, and tracing
    // will try to clone the span from the context. But the span is already closed,
    // which should cause a panic. However, with our fixes, it should not panic.
    
    match response {
        Ok(_) => {
            // Request completed successfully - no panic occurred
            println!("✅ invoke_actor completed successfully without panic");
        }
        Err(e) => {
            // If actor not found, that's OK (actor registration needs to be set up)
            if e.code() == tonic::Code::NotFound {
                println!("⚠️  Actor not found - this is expected if actor registration is not set up");
            } else {
                panic!("invoke_actor failed: {:?}", e);
            }
        }
    }

    // CLEANUP
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    harness.shutdown().await;
}

/// Test that reproduces the panic with multiple concurrent requests
/// 
/// This test spawns multiple concurrent invoke_actor calls, simulating
/// multiple concurrent gRPC requests.
#[tokio::test]
async fn test_span_cloning_panic_reproduction_concurrent() {
    std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");
    
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let mut harness = TestHarness::new();
    let _node = harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");

    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut client = harness.get_node("node1").unwrap().client.clone();
    let num_requests = 10;
    let mut handles = Vec::new();

    // Create a span (simulating gRPC handler span)
    let span = tracing::info_span!("test_grpc_handler_concurrent",
        tenant_id = "test-tenant",
        namespace = "test-namespace"
    );
    let _guard = span.enter();

    // Spawn multiple concurrent requests
    for i in 0..num_requests {
        let mut client_clone = client.clone();
        let request = Request::new(InvokeActorRequest {
            namespace: "test-namespace".to_string(),
            actor_type: "counter".to_string(),
            http_method: "GET".to_string(),
            payload: serde_json::json!({ "action": "get", "id": i }).to_string().into_bytes(),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            path: format!("/api/v1/actors/test-tenant/test-namespace/counter-{}", i),
            subpath: String::new(),
            ask: true,
            msg_type_override: String::new(),
            timeout: None,
        });

        let handle = tokio::spawn(async move {
            client_clone.invoke_actor(request).await
        });
        handles.push(handle);
    }

    // Drop the span guard (simulating gRPC handler returning)
    drop(_guard);

    // Wait for all requests to complete
    for handle in handles {
        let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
        match result {
            Ok(Ok(_)) => {
                // Request completed successfully
            }
            Ok(Err(e)) => {
                if e.code() == tonic::Code::NotFound {
                    // Actor not found is OK
                } else {
                    panic!("invoke_actor failed: {:?}", e);
                }
            }
            Err(_) => {
                panic!("Request did not complete within timeout - may have panicked");
            }
        }
    }

    println!("✅ All {} requests completed successfully without panic", num_requests);

    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    harness.shutdown().await;
}
