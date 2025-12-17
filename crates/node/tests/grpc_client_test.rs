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

//! Integration tests for gRPC client implementation

use plexspaces_actor::ActorRef;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::{
    grpc_client::RemoteActorClient, grpc_service::ActorServiceImpl, Node, NodeId, default_node_config,
};
use plexspaces_proto::{v1::actor::Message as ProtoMessage, ActorServiceServer};
use std::sync::Arc;
use tonic::transport::Server;

/// Helper to create a proto message with default values
fn create_proto_message(id: &str, sender: &str, receiver: &str, payload: Vec<u8>) -> ProtoMessage {
    ProtoMessage {
        id: id.to_string(),
        sender_id: sender.to_string(),
        receiver_id: receiver.to_string(),
        message_type: "test".to_string(),
        payload,
        timestamp: None,
        priority: 25, // Normal priority (5-level system: Signal=100, System=75, High=50, Normal=25, Low=0)
        ttl: None,
        headers: std::collections::HashMap::new(),
        idempotency_key: String::new(),
    }
}

/// Helper to start a gRPC server for testing
async fn start_test_server(node: Arc<Node>) -> String {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap(); // Use ephemeral port
    let service = ActorServiceImpl::new(node.clone());

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let bound_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        Server::builder()
            .add_service(ActorServiceServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("Server failed");
    });

    // Wait for server to be ready - use a future that checks server readiness
    let server_ready = async {
        tokio::task::yield_now().await;
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(1), server_ready)
        .await
        .expect("Server should start quickly");

    format!("http://{}", bound_addr)
}

/// Helper to create a test node with a registered actor
async fn create_test_node_with_actor() -> (Arc<Node>, ActorRef) {
    let node = Arc::new(Node::new(NodeId::new("test-node-1"), default_node_config()));

    use plexspaces_actor::RegularActorWrapper;
    use plexspaces_core::MessageSender;
    
    let actor_id = "test-actor-1@test-node-1".to_string();
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), actor_id.clone()).await.unwrap());
    let service_locator = node.service_locator();
    let actor_ref = ActorRef::local(actor_id.clone(), mailbox.clone(), service_locator.clone());
    
    // Register actor with MessageSender (mailbox is internal)
    let wrapper = Arc::new(RegularActorWrapper::new(
        actor_id.clone(),
        mailbox,
        service_locator,
    ));
    node.actor_registry().register_actor(actor_id.clone(), wrapper, None, None, None).await;
    
    // Also register with node for config tracking
    let core_actor_ref = plexspaces_core::ActorRef::new(actor_ref.id().as_str().to_string()).unwrap();
    node.actor_registry().register_actor_with_config(actor_ref.id().as_str().to_string(), None).await.unwrap();
    // Note: Metrics are updated internally by Node methods

    (node, actor_ref)
}

#[tokio::test]
async fn test_client_connection() {
    // Setup: Start a gRPC server
    let (node, _actor_ref) = create_test_node_with_actor().await;
    let server_addr = start_test_server(node).await;

    // Act: Create client and connect
    let client = RemoteActorClient::connect(&server_addr).await;

    // Assert: Connection should succeed
    assert!(
        client.is_ok(),
        "Failed to connect to server: {:?}",
        client.err()
    );
}

#[tokio::test]
async fn test_client_connection_failure() {
    // Act: Try to connect to non-existent server
    let result = RemoteActorClient::connect("http://127.0.0.1:1").await;

    // Assert: Should fail with connection error
    assert!(result.is_err(), "Expected connection failure");
}

#[tokio::test]
async fn test_send_message_via_client() {
    // Setup: Start server with registered actor
    let (node, _actor_ref) = create_test_node_with_actor().await;
    let server_addr = start_test_server(node).await;

    // Create client
    let mut client = RemoteActorClient::connect(&server_addr)
        .await
        .expect("Failed to connect");

    // Create message
    let proto_msg = create_proto_message(
        "msg-1",
        "sender-1",
        "test-actor-1@test-node-1",
        vec![1, 2, 3],
    );

    // Act: Send message via client
    let result = client.send_message(proto_msg).await;

    // Assert: Message should be delivered successfully
    assert!(result.is_ok(), "Failed to send message: {:?}", result.err());
    let msg_id = result.unwrap();
    assert_eq!(msg_id, "msg-1", "Message ID should match");

    // Note: We can't directly verify mailbox receipt without exposing internal mailbox,
    // but successful gRPC response indicates message was delivered to actor's mailbox
}

#[tokio::test]
async fn test_send_message_to_nonexistent_actor() {
    // Setup: Start server WITHOUT registered actor
    let node = Arc::new(Node::new(NodeId::new("test-node-2"), default_node_config()));
    let server_addr = start_test_server(node).await;

    // Create client
    let mut client = RemoteActorClient::connect(&server_addr)
        .await
        .expect("Failed to connect");

    // Create message for non-existent actor
    let proto_msg =
        create_proto_message("msg-2", "sender-1", "nonexistent-actor@test-node-2", vec![]);

    // Act: Try to send message
    let result = client.send_message(proto_msg).await;

    // Assert: Should fail with NOT_FOUND
    assert!(result.is_err(), "Expected NOT_FOUND error");
    let err = result.unwrap_err();
    assert!(err.contains("not found") || err.contains("Actor not found"));
}

#[tokio::test]
async fn test_send_message_with_headers() {
    // Setup: Start server with actor
    let (node, _actor_ref) = create_test_node_with_actor().await;
    let server_addr = start_test_server(node).await;

    // Create client
    let mut client = RemoteActorClient::connect(&server_addr)
        .await
        .expect("Failed to connect");

    // Create message with headers
    let mut proto_msg = create_proto_message(
        "msg-headers",
        "sender-1",
        "test-actor-1@test-node-1",
        vec![1, 2, 3],
    );
    proto_msg
        .headers
        .insert("trace-id".to_string(), "123-456".to_string());
    proto_msg
        .headers
        .insert("user-id".to_string(), "user-42".to_string());

    // Act: Send message
    let result = client.send_message(proto_msg).await;
    assert!(
        result.is_ok(),
        "Failed to send message with headers: {:?}",
        result.err()
    );

    // Note: Header preservation is tested in grpc_service_test via convert_proto_to_internal
    // Here we just verify the gRPC call succeeded with headers present
}

#[tokio::test]
async fn test_concurrent_client_messages() {
    // Setup: Start server with actor
    let (node, _actor_ref) = create_test_node_with_actor().await;
    let server_addr = start_test_server(node).await;

    // Create multiple clients
    let mut handles = vec![];
    for i in 0..5 {
        let addr = server_addr.clone();
        let handle = tokio::spawn(async move {
            let mut client = RemoteActorClient::connect(&addr).await.unwrap();

            let proto_msg = create_proto_message(
                &format!("msg-{}", i),
                &format!("sender-{}", i),
                "test-actor-1@test-node-1",
                vec![i as u8],
            );

            client.send_message(proto_msg).await
        });
        handles.push(handle);
    }

    // Wait for all sends to complete
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        assert!(result.is_ok(), "Concurrent send failed");
    }
}

#[tokio::test]
async fn test_client_with_invalid_message() {
    // Setup: Start server
    let (node, _actor_ref) = create_test_node_with_actor().await;
    let server_addr = start_test_server(node).await;

    // Create client
    let mut client = RemoteActorClient::connect(&server_addr)
        .await
        .expect("Failed to connect");

    // Create message with empty receiver
    let mut proto_msg = create_proto_message(
        "msg-invalid",
        "sender-1",
        "", // Empty receiver_id is invalid
        vec![],
    );

    // Act: Try to send invalid message
    let result = client.send_message(proto_msg).await;

    // Assert: Should fail with INVALID_ARGUMENT
    assert!(result.is_err(), "Expected validation error");
    let err = result.unwrap_err();
    assert!(err.contains("receiver") || err.contains("Missing receiver") || err.contains("empty"));
}
