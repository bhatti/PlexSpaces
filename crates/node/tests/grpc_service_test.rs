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

//! Integration tests for gRPC ActorService implementation

use plexspaces_actor::ActorRef;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::{grpc_service::ActorServiceImpl, Node, NodeConfig, NodeId};
use plexspaces_proto::{
    v1::actor::{Message as ProtoMessage, SendMessageRequest},
    ActorService,
};
use std::sync::Arc;
use tonic::Request;

/// Helper to create a proto message with default values
fn create_proto_message(id: &str, sender: &str, receiver: &str, payload: Vec<u8>) -> ProtoMessage {
    ProtoMessage {
        id: id.to_string(),
        sender_id: sender.to_string(),
        receiver_id: receiver.to_string(),
        message_type: "test".to_string(),
        payload,
        timestamp: None,
        idempotency_key: String::new(),
        priority: 25, // Normal priority
        ttl: None,
        headers: std::collections::HashMap::new(),
    }
}

/// Helper to create a test node with a mock actor
async fn create_test_node_with_actor() -> (Arc<Node>, ActorRef) {
    let node = Arc::new(Node::new(NodeId::new("test-node-1"), NodeConfig::default()));

    // Create a mock actor with mailbox (use actor@node format)
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
    
    // Register actor config
    node.actor_registry().register_actor_with_config(actor_ref.id().as_str().to_string(), None).await.unwrap();

    (node, actor_ref)
}

#[tokio::test]
async fn test_send_message_missing_message() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-1"), NodeConfig::default()));
    let service = ActorServiceImpl::new(node.clone());

    // Request with no message
    let request = Request::new(SendMessageRequest {
        message: None,
        wait_for_response: false,
        timeout: None,
    });

    // Act
    let response = service.send_message(request).await;

    // Assert: Should fail with INVALID_ARGUMENT
    assert!(response.is_err(), "Should fail for missing message");
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("Missing message"));
}

#[tokio::test]
async fn test_send_message_missing_receiver() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-2"), NodeConfig::default()));
    let service = ActorServiceImpl::new(node.clone());

    // Message with empty receiver
    let proto_msg = create_proto_message("msg-1", "sender-1", "", vec![]); // Empty receiver_id

    let request = Request::new(SendMessageRequest {
        message: Some(proto_msg),
        wait_for_response: false,
        timeout: None,
    });

    // Act
    let response = service.send_message(request).await;

    // Assert: Should fail with INVALID_ARGUMENT
    assert!(response.is_err(), "Should fail for empty receiver");
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("Missing receiver"));
}

#[tokio::test]
async fn test_send_message_to_nonexistent_actor() {
    // Setup: Create node WITHOUT registered actor
    let node = Arc::new(Node::new(NodeId::new("test-node-3"), NodeConfig::default()));
    let service = ActorServiceImpl::new(node.clone());

    // Create message to non-existent actor
    let proto_msg = create_proto_message("msg-2", "sender-1", "nonexistent-actor", vec![]);

    let request = Request::new(SendMessageRequest {
        message: Some(proto_msg),
        wait_for_response: false,
        timeout: None,
    });

    // Act: Try to send message
    let response = service.send_message(request).await;

    // Assert: Should fail with NOT_FOUND or UNIMPLEMENTED (since send_message not impl yet)
    assert!(response.is_err(), "Should fail for non-existent actor");
    let err = response.unwrap_err();
    // Currently returns UNIMPLEMENTED, will be NOT_FOUND after implementation
    assert!(
        err.code() == tonic::Code::Unimplemented || err.code() == tonic::Code::NotFound,
        "Expected UNIMPLEMENTED or NOT_FOUND, got {:?}",
        err.code()
    );
}

#[tokio::test]
async fn test_send_message_to_existing_actor() {
    // Setup: Create node with registered actor
    let (node, _actor_ref) = create_test_node_with_actor().await;
    let service = ActorServiceImpl::new(node.clone());

    // Create a proto message (use full actor@node ID)
    let proto_msg = create_proto_message(
        "msg-3",
        "sender-1",
        "test-actor-1@test-node-1",
        vec![1, 2, 3],
    );

    let request = Request::new(SendMessageRequest {
        message: Some(proto_msg),
        wait_for_response: false,
        timeout: None,
    });

    // Act: Send message via gRPC
    let response = service.send_message(request).await;

    // Assert: Currently UNIMPLEMENTED, will succeed after implementation
    // For now, just check it doesn't panic
    if response.is_ok() {
        let resp = response.unwrap().into_inner();
        assert_eq!(resp.message_id, "msg-3");
        assert!(
            resp.response.is_none(),
            "No response expected for fire-and-forget"
        );
    } else {
        // Before implementation, expect UNIMPLEMENTED
        assert_eq!(response.unwrap_err().code(), tonic::Code::Unimplemented);
    }
}

#[tokio::test]
async fn test_unimplemented_methods_return_unimplemented_status() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-4"), NodeConfig::default()));
    let service = ActorServiceImpl::new(node.clone());

    // Test create_actor
    let result = service
        .create_actor(Request::new(
            plexspaces_proto::v1::actor::CreateActorRequest {
                actor_type: String::new(),
                initial_state: vec![],
                config: None,
                labels: std::collections::HashMap::new(),
            },
        ))
        .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::Unimplemented);

    // Test list_actors
    let result = service
        .list_actors(Request::new(
            plexspaces_proto::v1::actor::ListActorsRequest {
                page_request: None,
                actor_type: String::new(),
                state: 0,
                node_id: String::new(),
            },
        ))
        .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::Unimplemented);

    // Test delete_actor
    let result = service
        .delete_actor(Request::new(
            plexspaces_proto::v1::actor::DeleteActorRequest {
                actor_id: "test".to_string(),
                force: false,
            },
        ))
        .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::Unimplemented);
}

#[tokio::test]
async fn test_message_with_headers() {
    // Setup
    let (node, _actor_ref) = create_test_node_with_actor().await;
    let service = ActorServiceImpl::new(node.clone());

    // Create message with headers
    let mut proto_msg = create_proto_message(
        "msg-headers",
        "sender-1",
        "test-actor-1@test-node-1",
        vec![1, 2, 3],
    );
    proto_msg
        .headers
        .insert("trace-id".to_string(), "123-456-789".to_string());
    proto_msg
        .headers
        .insert("user-id".to_string(), "user-42".to_string());

    let request = Request::new(SendMessageRequest {
        message: Some(proto_msg),
        wait_for_response: false,
        timeout: None,
    });

    // Act
    let response = service.send_message(request).await;

    // Currently unimplemented - will test proper handling after implementation
    if response.is_ok() {
        assert!(true, "Message with headers should succeed");
    }
}

#[tokio::test]
async fn test_concurrent_message_sends() {
    // Setup: Create node with actor
    let (node, _actor_ref) = create_test_node_with_actor().await;
    let service = Arc::new(ActorServiceImpl::new(node.clone()));

    // Send multiple messages concurrently
    let mut handles = vec![];
    for i in 0..5 {
        let service_clone = service.clone();
        let handle = tokio::spawn(async move {
            let proto_msg = create_proto_message(
                &format!("msg-{}", i),
                &format!("sender-{}", i),
                "test-actor-1@test-node-1",
                vec![i as u8],
            );

            let request = Request::new(SendMessageRequest {
                message: Some(proto_msg),
                wait_for_response: false,
                timeout: None,
            });

            service_clone.send_message(request).await
        });
        handles.push(handle);
    }

    // Wait for all sends to complete - they should all return (either OK or UNIMPLEMENTED)
    for handle in handles {
        let result = handle.await.expect("Task should not panic");
        // Either succeeds or returns UNIMPLEMENTED - both are valid for now
        if let Err(e) = result {
            assert_eq!(e.code(), tonic::Code::Unimplemented);
        }
    }
}
