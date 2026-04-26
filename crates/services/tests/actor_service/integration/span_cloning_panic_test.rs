// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Integration tests covering tracing/span safety for AskReply.

use super::TestHarness;
use plexspaces_proto::actor::v1::AskReplyRequest;
use std::collections::HashMap;
use std::time::Duration;
use tonic::Request;

#[tokio::test]
async fn test_ask_reply_does_not_panic_after_handler_span_closes() {
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
    let span = tracing::info_span!(
        "test_grpc_handler",
        tenant_id = "test-tenant",
        namespace = "test-namespace",
        actor_type = "counter"
    );
    let guard = span.enter();

    let request = Request::new(AskReplyRequest {
        namespace: "test-namespace".to_string(),
        actor_type: "counter".to_string(),
        actor_name: String::new(),
        http_method: "GET".to_string(),
        payload: serde_json::json!({ "action": "get"}).to_string().into_bytes(),
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path: "/api/v1/actors/test-namespace/counter".to_string(),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "call".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
        timeout: None,
    });

    let response = client.ask_reply(request).await;
    drop(guard);

    tokio::time::sleep(Duration::from_millis(250)).await;

    match response {
        Ok(_) => {}
        Err(e) => {
            if e.code() != tonic::Code::NotFound {
                panic!("ask_reply failed: {:?}", e);
            }
        }
    }

    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    harness.shutdown().await;
}

#[tokio::test]
async fn test_concurrent_ask_reply_requests_do_not_panic() {
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

    let client = harness.get_node("node1").unwrap().client.clone();
    let span = tracing::info_span!(
        "test_grpc_handler_concurrent",
        tenant_id = "test-tenant",
        namespace = "test-namespace"
    );
    let guard = span.enter();

    let mut handles = Vec::new();
    for i in 0..10 {
        let mut client_clone = client.clone();
        let request = Request::new(AskReplyRequest {
            namespace: "test-namespace".to_string(),
            actor_type: "counter".to_string(),
            actor_name: String::new(),
            http_method: "GET".to_string(),
            payload: serde_json::json!({ "action": "get", "id": i })
                .to_string()
                .into_bytes(),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            path: format!("/api/v1/actors/test-namespace/counter-{i}"),
            subpath: String::new(),
            sender_id: String::new(),
            message_type: "call".to_string(),
            correlation_id: String::new(),
            reply_to: String::new(),
            message_id: String::new(),
            timeout: None,
        });
        handles.push(tokio::spawn(async move { client_clone.ask_reply(request).await }));
    }

    drop(guard);

    for handle in handles {
        match tokio::time::timeout(Duration::from_secs(5), handle).await {
            Ok(Ok(Ok(_))) | Ok(Ok(Err(_))) => {}
            Ok(Err(join_err)) => panic!("Task panicked: {:?}", join_err),
            Err(_) => panic!("Request timed out"),
        }
    }

    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    harness.shutdown().await;
}
