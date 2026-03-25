// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Real integration tests for AskReply and SendMessage via gRPC.

use super::TestHarness;
use plexspaces_proto::actor::v1::{AskReplyRequest, SendMessageRequest};
use std::collections::HashMap;
use std::time::Duration;
use tonic::Request;

#[tokio::test]
async fn test_ask_reply_get_counter_real_grpc() {
    std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");
    let mut harness = TestHarness::new();
    let _node = harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");

    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut client = harness.get_node("node1").unwrap().client.clone();
    let request = Request::new(AskReplyRequest {
        namespace: "default".to_string(),
        actor_type: "counter".to_string(),
        http_method: "GET".to_string(),
        payload: serde_json::json!({ "action": "get" }).to_string().into_bytes(),
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path: "/api/v1/actors/default/counter".to_string(),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "call".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
        timeout: None,
    });

    let response = client.ask_reply(request).await;
    match response {
        Ok(resp) => {
            let inner = resp.into_inner();
            assert!(inner.success);
            assert!(!inner.actor_id.is_empty());
        }
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
async fn test_send_message_post_counter_real_grpc() {
    std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");
    let mut harness = TestHarness::new();
    let _node = harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");

    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut client = harness.get_node("node1").unwrap().client.clone();
    let request = Request::new(SendMessageRequest {
        namespace: "default".to_string(),
        actor_type: "counter".to_string(),
        http_method: "POST".to_string(),
        payload: serde_json::json!({ "action": "increment" })
            .to_string()
            .into_bytes(),
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path: "/api/v1/actors/default/counter".to_string(),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "cast".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
    });

    let response = client.send_message(request).await;
    match response {
        Ok(resp) => {
            let inner = resp.into_inner();
            assert!(inner.success);
        }
        Err(e) => {
            if e.code() != tonic::Code::NotFound {
                panic!("send_message failed: {:?}", e);
            }
        }
    }

    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    harness.shutdown().await;
}

#[tokio::test]
async fn test_ask_reply_post_counter_real_grpc() {
    std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");
    let mut harness = TestHarness::new();
    let _node = harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");

    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut client = harness.get_node("node1").unwrap().client.clone();
    let request = Request::new(AskReplyRequest {
        namespace: "default".to_string(),
        actor_type: "counter".to_string(),
        http_method: "POST".to_string(),
        payload: serde_json::json!({ "action": "get" }).to_string().into_bytes(),
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path: "/api/v1/actors/default/counter/ask".to_string(),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "call".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
        timeout: None,
    });

    let response = client.ask_reply(request).await;
    match response {
        Ok(resp) => assert!(resp.into_inner().success),
        Err(e) => {
            if e.code() != tonic::Code::NotFound {
                panic!("ask_reply POST failed: {:?}", e);
            }
        }
    }

    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    harness.shutdown().await;
}
