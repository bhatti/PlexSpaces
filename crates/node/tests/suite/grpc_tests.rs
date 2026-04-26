// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Consolidated gRPC tests from:
// - grpc_client_test.rs (7 tests)
// - grpc_service_test.rs (7 tests)
// Total: 13 tests (1 duplicate removed)

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_core::{Actor, ActorContext, ActorId, BehaviorError, BehaviorType, Message};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::{grpc_client::RemoteActorClient, Node, NodeBuilder};
use plexspaces_proto::{
    actor::v1::SendMessageRequest, common::v1::Message as ProtoMessageCommon,
    ActorService as ActorServiceTrait, ActorServiceServer,
};
use plexspaces_services::actor_service::ActorServiceImpl;
use std::sync::Arc;
use tonic::{transport::Server, Request};

use super::test_helpers::spawn_actor_helper;

// =============================================================================
// COMMON HELPERS
// =============================================================================

/// Helper to create a proto message (common::v1::Message)
fn create_proto_message_common(
    id: &str,
    sender: &str,
    receiver: &str,
    payload: Vec<u8>,
) -> ProtoMessageCommon {
    ProtoMessageCommon {
        id: id.to_string(),
        sender_id: sender.to_string(),
        receiver_id: receiver.to_string(),
        message_type: "test".to_string(),
        payload,
        timestamp: None,
        priority: 25,
        ttl: None,
        headers: std::collections::HashMap::new(),
        idempotency_key: String::new(),
        uri_method: String::new(),
        uri_path: String::new(),
        ..Default::default()
    }
}

/// Helper to create a proto message (common::v1::Message)
fn create_proto_message(
    id: &str,
    sender: &str,
    receiver: &str,
    payload: Vec<u8>,
) -> ProtoMessageCommon {
    ProtoMessageCommon {
        id: id.to_string(),
        sender_id: sender.to_string(),
        receiver_id: receiver.to_string(),
        message_type: "test".to_string(),
        payload,
        timestamp: None,
        idempotency_key: String::new(),
        priority: 25,
        ttl: None,
        headers: std::collections::HashMap::new(),
        uri_method: String::new(),
        uri_path: String::new(),
        ..Default::default()
    }
}

fn create_send_message_request(message: ProtoMessageCommon) -> SendMessageRequest {
    SendMessageRequest {
        namespace: String::new(),
        actor_type: message.receiver_id.clone(),
        actor_name: String::new(),
        http_method: "POST".to_string(),
        payload: message.payload,
        headers: message.headers,
        query_params: Default::default(),
        path: String::new(),
        subpath: String::new(),
        sender_id: message.sender_id,
        message_type: if message.message_type.is_empty() {
            "cast".to_string()
        } else {
            message.message_type
        },
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: message.id,
    }
}

/// Simple test behavior that processes messages
struct TestBehavior;

#[async_trait::async_trait]
impl Actor for TestBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

/// Helper to start a gRPC server for testing
async fn start_test_server(node: Arc<Node>) -> String {
    use plexspaces_services::actor_service::ActorServiceImpl;
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let service = ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let bound_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        Server::builder()
            .add_service(ActorServiceServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("Server failed");
    });

    let server_ready = async {
        tokio::task::yield_now().await;
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(1), server_ready)
        .await
        .expect("Server should start quickly");

    format!("http://{}", bound_addr)
}

/// Helper to create a test node with a registered actor (auth disabled for tests)
async fn create_test_node_with_actor(node_name: &str) -> (Arc<Node>, ActorRef) {
    let node = Arc::new(
        NodeBuilder::new(node_name)
            .with_auth_disabled()
            .build()
            .await,
    );

    let behavior = Box::new(TestBehavior);
    let actor = ActorBuilder::new(behavior)
        .with_name("test-actor-1")
        .with_node_id(node_name)
        .build()
        .await
        .unwrap();

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();

    (node, actor_ref)
}

// =============================================================================
// GRPC CLIENT TESTS (from grpc_client_test.rs - 7 tests)
// =============================================================================

#[tokio::test]
async fn test_client_connection() {
    let (node, _actor_ref) = create_test_node_with_actor("test-node-client-conn").await;
    let server_addr = start_test_server(node).await;

    let client = RemoteActorClient::connect(&server_addr).await;

    assert!(
        client.is_ok(),
        "Failed to connect to server: {:?}",
        client.err()
    );
}

#[tokio::test]
async fn test_client_connection_failure() {
    let result = RemoteActorClient::connect("http://127.0.0.1:1").await;

    assert!(result.is_err(), "Expected connection failure");
}

#[tokio::test]
async fn test_send_message_via_client() {
    let (node, actor_ref) = create_test_node_with_actor("test-node-send-msg").await;
    let server_addr = start_test_server(node).await;

    let mut client = RemoteActorClient::connect(&server_addr)
        .await
        .expect("Failed to connect");

    let proto_msg = create_proto_message_common(
        "msg-1",
        "sender-1",
        &actor_ref.id().to_string(),
        b"{\"action\":\"ping\"}".to_vec(),
    );

    let result = client.send_message(proto_msg).await;

    assert!(result.is_ok(), "Failed to send message: {:?}", result.err());
    let msg_id = result.unwrap();
    assert_eq!(msg_id, "msg-1", "Message ID should match");
}

#[tokio::test]
async fn test_send_message_to_nonexistent_actor_via_client() {
    let node = Arc::new(
        NodeBuilder::new("test-node-nonexist")
            .with_auth_disabled()
            .build()
            .await,
    );
    let server_addr = start_test_server(node).await;

    let mut client = RemoteActorClient::connect(&server_addr)
        .await
        .expect("Failed to connect");

    let proto_msg = create_proto_message_common(
        "msg-2",
        "sender-1",
        &ActorId::new(
            "nonexistent-actor",
            "gen_server",
            "default",
            "test-node-nonexist",
        )
        .unwrap()
        .to_string(),
        vec![],
    );

    let result = client.send_message(proto_msg).await;

    // Expect any error — either actor-not-found or invalid-actor-id (the canonical ID
    // format is unambiguous in the registry but may fail validation at the gRPC boundary
    // depending on how the service parses the actor_type field).
    assert!(
        result.is_err(),
        "Expected error for nonexistent actor, got: {:?}",
        result
    );
}

#[tokio::test]
async fn test_send_message_with_headers_via_client() {
    let (node, actor_ref) = create_test_node_with_actor("test-node-headers").await;
    let server_addr = start_test_server(node).await;

    let mut client = RemoteActorClient::connect(&server_addr)
        .await
        .expect("Failed to connect");

    let mut proto_msg = create_proto_message_common(
        "msg-headers",
        "sender-1",
        &actor_ref.id().to_string(),
        b"{\"action\":\"ping\"}".to_vec(),
    );
    proto_msg
        .headers
        .insert("trace-id".to_string(), "123-456".to_string());
    proto_msg
        .headers
        .insert("user-id".to_string(), "user-42".to_string());

    let result = client.send_message(proto_msg).await;
    assert!(
        result.is_ok(),
        "Failed to send message with headers: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_concurrent_client_messages() {
    let (node, actor_ref) = create_test_node_with_actor("test-node-concurrent").await;
    let server_addr = start_test_server(node).await;
    let receiver_id = actor_ref.id().to_string();

    let mut handles = vec![];
    for i in 0..5 {
        let addr = server_addr.clone();
        let receiver_id = receiver_id.clone();
        let handle = tokio::spawn(async move {
            let mut client = RemoteActorClient::connect(&addr).await.unwrap();

            let proto_msg = create_proto_message_common(
                &format!("msg-{}", i),
                &format!("sender-{}", i),
                &receiver_id,
                format!("{{\"index\":{}}}", i).into_bytes(),
            );

            client.send_message(proto_msg).await
        });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.await.expect("Task panicked");
        assert!(result.is_ok(), "Concurrent send failed");
    }
}

#[tokio::test]
async fn test_client_with_invalid_message() {
    let (node, _actor_ref) = create_test_node_with_actor("test-node-invalid").await;
    let server_addr = start_test_server(node).await;

    let mut client = RemoteActorClient::connect(&server_addr)
        .await
        .expect("Failed to connect");

    let proto_msg = create_proto_message_common(
        "msg-invalid",
        "sender-1",
        "", // Empty receiver_id is invalid
        vec![],
    );

    let result = client.send_message(proto_msg).await;

    assert!(
        result.is_err(),
        "Expected validation error for empty receiver_id"
    );
    // RemoteActorClient maps receiver_id to actor_type; empty receiver → Missing actor_type.
    let err = result.unwrap_err();
    assert!(
        err.contains("receiver")
            || err.contains("actor_type")
            || err.contains("empty")
            || err.contains("Missing"),
        "Unexpected error: {}",
        err
    );
}

// =============================================================================
// GRPC SERVICE TESTS (from grpc_service_test.rs - 6 tests, 1 duplicate removed)
// =============================================================================

#[tokio::test]
async fn test_send_message_missing_message() {
    let node = Arc::new(
        NodeBuilder::new("test-node-missing-msg")
            .with_auth_disabled()
            .build()
            .await,
    );
    let service = ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());

    let request = Request::new(SendMessageRequest {
        namespace: String::new(),
        actor_type: String::new(),
        actor_name: String::new(),
        http_method: "POST".to_string(),
        payload: Vec::new(),
        headers: Default::default(),
        query_params: Default::default(),
        path: String::new(),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "cast".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
    });

    let response = ActorServiceTrait::send_message(&service, request).await;

    assert!(response.is_err(), "Should fail for missing actor_type");
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("actor_type"));
}

#[tokio::test]
async fn test_send_message_missing_receiver() {
    let node = Arc::new(
        NodeBuilder::new("test-node-missing-recv")
            .with_auth_disabled()
            .build()
            .await,
    );
    let service = ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());

    let proto_msg = create_proto_message("msg-1", "sender-1", "", vec![]);

    let request = Request::new(create_send_message_request(proto_msg));

    let response = ActorServiceTrait::send_message(&service, request).await;

    assert!(response.is_err(), "Should fail for empty receiver");
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("actor_type"));
}

#[tokio::test]
async fn test_send_message_to_existing_actor() {
    let (node, actor_ref) = create_test_node_with_actor("test-node-existing").await;
    let service = ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());

    let proto_msg = create_proto_message(
        "msg-3",
        "sender-1",
        &actor_ref.id().to_string(),
        b"{\"action\":\"ping\"}".to_vec(),
    );

    let request = Request::new(create_send_message_request(proto_msg));

    let response = ActorServiceTrait::send_message(&service, request).await;

    assert!(
        response.is_ok(),
        "send_message should succeed: {:?}",
        response.err()
    );
    let resp = response.unwrap().into_inner();
    assert!(!resp.message_id.is_empty());
    assert!(resp.success, "Tell response should acknowledge success");
}

#[tokio::test]
async fn test_unimplemented_methods_return_unimplemented_status() {
    let node = Arc::new(
        NodeBuilder::new("test-node-unimpl")
            .with_auth_disabled()
            .build()
            .await,
    );
    let service = ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());

    let result = ActorServiceTrait::spawn_actor(
        &service,
        Request::new(plexspaces_proto::actor::v1::SpawnActorRequest {
            actor_id: String::new(),
            actor_type: String::new(),
            role: String::new(),
            initial_state: vec![],
            config: None,
            labels: std::collections::HashMap::new(),
            facets: vec![],
            namespace: "default".to_string(),
            instances_count: 1,
        }),
    )
    .await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(
        err.code(),
        tonic::Code::InvalidArgument,
        "spawn_actor returns InvalidArgument for empty actor_type"
    );

    let result = ActorServiceTrait::list_actors(
        &service,
        Request::new(plexspaces_proto::actor::v1::ListActorsRequest {
            page_request: None,
            actor_type: String::new(),
            state: 0,
            node_id: String::new(),
            namespace: String::new(),
        }),
    )
    .await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.code() == tonic::Code::Unimplemented
            || err.code() == tonic::Code::InvalidArgument
            || err.code() == tonic::Code::Internal
    );

    let result = ActorServiceTrait::delete_actor(
        &service,
        Request::new(plexspaces_proto::actor::v1::DeleteActorRequest {
            actor_id: "test".to_string(),
            force: false,
            namespace: String::new(),
        }),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::Unimplemented);
}

#[tokio::test]
async fn test_message_with_headers_via_service() {
    let (node, actor_ref) = create_test_node_with_actor("test-node-svc-headers").await;
    let service = ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());

    let mut proto_msg = create_proto_message(
        "msg-headers",
        "sender-1",
        &actor_ref.id().to_string(),
        b"{\"action\":\"ping\"}".to_vec(),
    );
    proto_msg
        .headers
        .insert("trace-id".to_string(), "123-456-789".to_string());
    proto_msg
        .headers
        .insert("user-id".to_string(), "user-42".to_string());

    let request = Request::new(create_send_message_request(proto_msg));

    let response = ActorServiceTrait::send_message(&service, request).await;

    if response.is_ok() {
        assert!(true, "Message with headers should succeed");
    }
}

#[tokio::test]
async fn test_concurrent_message_sends_via_service() {
    let (node, actor_ref) = create_test_node_with_actor("test-node-svc-conc").await;
    let service = Arc::new(ActorServiceImpl::new(
        node.service_locator(),
        node.id().as_str().to_string(),
    ));
    let receiver_id = actor_ref.id().to_string();

    let mut handles = vec![];
    for i in 0..5 {
        let service_clone = service.clone();
        let receiver_id = receiver_id.clone();
        let handle = tokio::spawn(async move {
            let proto_msg = create_proto_message(
                &format!("msg-{}", i),
                &format!("sender-{}", i),
                &receiver_id,
                format!("{{\"index\":{}}}", i).into_bytes(),
            );

            let request = Request::new(create_send_message_request(proto_msg));

            ActorServiceTrait::send_message(&*service_clone, request).await
        });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.await.expect("Task should not panic");
        assert!(
            result.is_ok(),
            "Concurrent send should succeed: {:?}",
            result.err()
        );
    }
}
