// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Remote Actor Spawning Tests
//!
//! Tests for Erlang-style spawn/4 equivalent remote actor spawning.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use plexspaces_core::{ActorId, ServiceLocator};
use plexspaces_node::{default_node_config, Node, NodeId};
use plexspaces_proto::{
    actor::v1::{
        ActorConfig as ProtoActorConfig, ActorSpawnSpec, ActorVisibility, SpawnActorRequest,
    },
    common::v1::ActorIdentity,
    ActorService as ActorServiceTrait, ActorServiceServer,
};
use tonic::Request;

fn spawn_actor_grpc_request(
    namespace: &str,
    actor_type: &str,
    role: &str,
    instance_name: &str,
) -> SpawnActorRequest {
    SpawnActorRequest {
        spec: Some(ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: instance_name.to_string(),
                actor_type: actor_type.to_string(),
            }),
            role: role.to_string(),
            namespace: String::new(),
            tenant_id: String::new(),
            visibility: 0,
            behavior_kind: String::new(),
            args: HashMap::new(),
            facets: vec![],
            config: None,
            labels: HashMap::new(),
        }),
        namespace: namespace.to_string(),
        instances_count: 1,
    }
}

/// Helper to create a test node
fn create_test_node(id: &str, port: u16) -> Node {
    let mut config = default_node_config();
    config.listen_addr = format!("127.0.0.1:{}", port);
    config.heartbeat_interval_ms = 100;

    Node::new(NodeId::new(id), config)
}

#[tokio::test]
async fn test_spawn_actor_basic() {
    // Create a node and initialize services (required for ActorFactory to be available)
    let node = Arc::new(create_test_node("test-node", 9501));
    node.initialize_services().await.unwrap();

    // Create gRPC service
    let service = plexspaces_services::actor_service::ActorServiceImpl::new(
        node.service_locator(),
        node.id().as_str().to_string(),
    );

    // Create SpawnActorRequest (target node is implicit from gRPC endpoint)
    let request = Request::new(spawn_actor_grpc_request("", "test_actor", "test_actor", ""));

    // Spawn actor via gRPC
    let response = ActorServiceTrait::spawn_actor(&service, request).await;

    // Should succeed
    assert!(response.is_ok(), "spawn_actor should succeed");

    let resp = response.unwrap().into_inner();

    // Should return canonical actor_ref with the target node encoded in the ID
    let actor_id = ActorId::from_canonical(&resp.actor_ref).expect("spawn should return ActorId");
    assert!(
        actor_id.node_id() == "test-node",
        "actor_ref should target test-node"
    );

    // Should return actor details
    assert!(resp.actor.is_some(), "actor details should be present");
    let actor = resp.actor.unwrap();
    assert_eq!(actor.actor_type, "test_actor");
    assert_eq!(actor.node_id, "test-node");
}

#[tokio::test]
async fn test_spawn_remote_actor_missing_target_node() {
    let node = Arc::new(create_test_node("test-node", 9502));
    let service = plexspaces_services::actor_service::ActorServiceImpl::new(
        node.service_locator(),
        node.id().as_str().to_string(),
    );

    // Missing actor_type (should fail)
    let request = Request::new(spawn_actor_grpc_request("", "", "", ""));

    let response = ActorServiceTrait::spawn_actor(&service, request).await;

    // Should fail with invalid_argument
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(
        err.message().contains("actor_type")
            || err.message().contains("Missing")
            || err.message().contains("spec.identity")
    );
}

#[tokio::test]
async fn test_spawn_remote_actor_missing_actor_type() {
    let node = Arc::new(create_test_node("test-node", 9503));
    let service = plexspaces_services::actor_service::ActorServiceImpl::new(
        node.service_locator(),
        node.id().as_str().to_string(),
    );

    // Missing actor_type
    let request = Request::new(spawn_actor_grpc_request("default", "", "", ""));

    let response = ActorServiceTrait::spawn_actor(&service, request).await;

    // Should fail with invalid_argument
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(
        err.message().contains("actor_type")
            || err.message().contains("Missing")
            || err.message().contains("spec.identity")
    );
}

#[tokio::test]
async fn test_spawn_remote_actor_wrong_node() {
    let node = Arc::new(create_test_node("node1", 9504));
    node.initialize_services().await.unwrap();
    let service = plexspaces_services::actor_service::ActorServiceImpl::new(
        node.service_locator(),
        node.id().as_str().to_string(),
    );

    // gRPC spawn_actor always spawns on the node receiving the request
    // The test for "wrong node" doesn't make sense anymore since target is implicit
    // This test is now redundant - spawn_actor always succeeds on the receiving node
    let request = Request::new(spawn_actor_grpc_request(
        "default",
        "test_actor",
        "test_actor",
        "",
    ));

    let response = ActorServiceTrait::spawn_actor(&service, request).await;

    // Should succeed - spawns on node1 (the node receiving the request)
    assert!(
        response.is_ok(),
        "spawn_actor should succeed on receiving node"
    );
}

#[tokio::test]
async fn test_spawn_multiple_remote_actors() {
    let node = Arc::new(create_test_node("test-node", 9505));
    node.initialize_services().await.unwrap();
    let service = plexspaces_services::actor_service::ActorServiceImpl::new(
        node.service_locator(),
        node.id().as_str().to_string(),
    );

    // Spawn 3 actors
    for i in 0..3 {
        let request = Request::new(spawn_actor_grpc_request(
            "",
            &format!("test_actor_{}", i),
            &format!("test_actor_{}", i),
            "",
        ));

        let response = ActorServiceTrait::spawn_actor(&service, request).await;
        assert!(response.is_ok(), "spawn {} should succeed", i);

        let resp = response.unwrap().into_inner();
        let actor_id =
            ActorId::from_canonical(&resp.actor_ref).expect("spawn should return ActorId");
        assert_eq!(actor_id.node_id(), "test-node");
    }

    // All 3 should be registered with node
    // (We'd need node.list_actors() to verify, but testing spawn succeeded is enough)
}

#[tokio::test]
async fn test_spawn_remote_actor_via_grpc() {
    // Start node with gRPC server
    let node = Arc::new(create_test_node("node1", 9601));

    // Start gRPC server in background
    let node_clone = node.clone();
    let server_handle = tokio::spawn(async move { node_clone.start().await });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Connect gRPC client
    use plexspaces_proto::ActorServiceClient;
    let mut client = ActorServiceClient::connect("http://127.0.0.1:9601")
        .await
        .expect("should connect");

    // Spawn actor via gRPC using RemoteActorClient
    use plexspaces_node::grpc_client::RemoteActorClient;
    let mut remote_client = RemoteActorClient::connect("http://127.0.0.1:9601")
        .await
        .expect("should connect");
    let response = remote_client
        .spawn_actor(
            "node1",
            "remote_test_actor",
            std::collections::HashMap::new(),
            None,
            std::collections::HashMap::new(),
        )
        .await;
    assert!(response.is_ok(), "gRPC spawn should succeed");

    let actor_ref = response.unwrap();
    assert_eq!(actor_ref.id().node_id(), "node1");

    // Cleanup
    server_handle.abort();
}

// =============================================================================
// Tests merged from remote_routing.rs (5 tests)
// =============================================================================

use plexspaces_actor::ActorRef;
use plexspaces_core::{Message, MessageSender};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_services::actor_service::ActorServiceImpl;
use tonic::transport::Server;

use super::test_helpers::{lookup_actor_ref, test_runtime_actor_id};

/// Helper to create a test message
fn create_routing_test_message(payload: Vec<u8>) -> plexspaces_core::Message {
    plexspaces_core::Message {
        id: ulid::Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

/// Helper to start a gRPC server for testing
async fn start_test_server(node: Arc<Node>) -> String {
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

    tokio::task::yield_now().await;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    bound_addr.to_string()
}

#[tokio::test]
async fn test_node_route_local_message() {
    use plexspaces_node::NodeBuilder;

    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let actor_id = test_runtime_actor_id("test-actor", "node1");
    let mut mailbox_config = MailboxConfig::default();
    mailbox_config.capacity = 1000;
    let mailbox = Arc::new(
        Mailbox::new(mailbox_config, actor_id.to_string())
            .await
            .unwrap(),
    );
    let service_locator = node.service_locator().clone();
    let actor_ref = ActorRef::local(
        actor_id.clone(),
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        service_locator.clone(),
        ActorVisibility::ActorVisibilityPublic,
    );

    let wrapper = Arc::new(ActorRef::local(
        actor_id.clone(),
        "".to_string(),
        "".to_string(), // test namespace
        mailbox.clone(),
        service_locator.clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry: Arc<plexspaces_core::ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })
        .unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    actor_registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            wrapper,
            "gen_server".to_string(),
            None,
            None,
            None,
        )
        .await;

    let message = create_routing_test_message(vec![1, 2, 3]);
    let result = actor_ref.tell(&ctx, message).await;

    assert!(result.is_ok(), "Local routing should succeed");
    let received = mailbox.dequeue().await;
    assert!(received.is_some(), "Message should be in mailbox");
}

#[tokio::test]
async fn test_node_route_remote_message() {
    use plexspaces_node::NodeBuilder;

    let node1 = Arc::new(NodeBuilder::new("node1").build().await);
    let node2 = Arc::new(NodeBuilder::new("node2").build().await);

    let actor_id = test_runtime_actor_id("remote-actor", "node2");
    let node2_address = start_test_server(node2.clone()).await;

    let mut mailbox_config2 = MailboxConfig::default();
    mailbox_config2.capacity = 1000;
    let mailbox2 = Arc::new(
        Mailbox::new(mailbox_config2, actor_id.to_string())
            .await
            .unwrap(),
    );
    let service_locator2 = node2.service_locator().clone();
    let actor_ref2 = ActorRef::local(
        actor_id.clone(),
        "".to_string(),
        "".to_string(),
        mailbox2.clone(),
        service_locator2.clone(),
        ActorVisibility::ActorVisibilityPublic,
    );

    let wrapper2 = Arc::new(ActorRef::local(
        actor_id.clone(),
        "".to_string(),
        "".to_string(), // test namespace
        mailbox2.clone(),
        service_locator2.clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry2: Arc<plexspaces_core::ActorRegistry> = node2
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })
        .unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    actor_registry2
        .register_actor(
            &ctx,
            actor_id.clone(),
            wrapper2,
            "gen_server".to_string(),
            None,
            None,
            None,
        )
        .await;

    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    let remote_actor_id = actor_ref2.id().clone();
    let sender: Arc<dyn plexspaces_core::MessageSender> = Arc::new(actor_ref2.clone());
    actor_registry2
        .register_actor(
            &ctx,
            remote_actor_id.clone(),
            sender,
            "TestActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    let object_registry: Arc<dyn plexspaces_core::ObjectRegistry> =
        node1.service_locator().get_object_registry().await.unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    let grpc_address = if node2_address.starts_with("http://") {
        node2_address.strip_prefix("http://").unwrap().to_string()
    } else {
        node2_address.clone()
    };
    let node_registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeNode as i32,
        object_id: "node2".to_string(),
        grpc_address,
        object_category: "Node".to_string(),
        ..Default::default()
    };
    object_registry
        .register(&ctx, node_registration)
        .await
        .unwrap();

    tokio::task::yield_now().await;
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let service_locator1 = node1.service_locator().clone();
    let remote_actor_ref = ActorRef::remote(
        remote_actor_id,
        "".to_string(),
        "".to_string(),
        "node2".to_string(),
        service_locator1,
        ActorVisibility::ActorVisibilityPublic,
    );
    let message = create_routing_test_message(vec![4, 5, 6]);
    let result = remote_actor_ref.tell(&ctx, message).await;

    assert!(result.is_ok(), "Remote routing should succeed");
    let received_opt = mailbox2
        .dequeue_with_timeout(Some(tokio::time::Duration::from_secs(5)))
        .await;
    let received = received_opt.expect("Message should arrive within 5 seconds");
    assert_eq!(received.payload, vec![4, 5, 6]);
}

#[tokio::test]
async fn test_node_route_to_unregistered_remote() {
    use plexspaces_node::NodeBuilder;

    let node = Arc::new(NodeBuilder::new("node1").build().await);
    let missing_actor_id = test_runtime_actor_id("actor", "node999");

    let message = create_routing_test_message(vec![7, 8, 9]);
    let tell_ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    let result = match lookup_actor_ref(&node, &missing_actor_id).await {
        Ok(Some(actor_ref)) => actor_ref
            .tell(&tell_ctx, message)
            .await
            .map_err(|e| plexspaces_node::NodeError::DeliveryFailed(format!("{}", e))),
        Ok(None) => Err(plexspaces_node::NodeError::ActorNotFound(
            missing_actor_id.to_string(),
        )),
        Err(e) => Err(e),
    };

    assert!(result.is_err(), "Should fail for unregistered node");
}

#[tokio::test]
async fn test_connection_pooling() {
    use plexspaces_node::NodeBuilder;

    let node1 = Arc::new(NodeBuilder::new("node1").build().await);
    let node2 = Arc::new(NodeBuilder::new("node2").build().await);

    let actor_id = test_runtime_actor_id("pooled-actor", "node2");
    let node2_address = start_test_server(node2.clone()).await;

    let mut mailbox_config2 = MailboxConfig::default();
    mailbox_config2.capacity = 1000;
    let mailbox2 = Arc::new(
        Mailbox::new(mailbox_config2, actor_id.to_string())
            .await
            .unwrap(),
    );
    let service_locator2 = node2.service_locator().clone();
    let actor_ref2 = ActorRef::local(
        actor_id.clone(),
        "".to_string(),
        "".to_string(),
        mailbox2.clone(),
        service_locator2.clone(),
        ActorVisibility::ActorVisibilityPublic,
    );

    let wrapper_pooled = Arc::new(ActorRef::local(
        actor_id.clone(),
        "".to_string(),
        "".to_string(), // test namespace
        mailbox2.clone(),
        service_locator2.clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry2: Arc<plexspaces_core::ActorRegistry> = node2
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })
        .unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    actor_registry2
        .register_actor(
            &ctx,
            actor_id.clone(),
            wrapper_pooled,
            "gen_server".to_string(),
            None,
            None,
            None,
        )
        .await;

    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    let remote_actor_id = actor_ref2.id().clone();
    let sender: Arc<dyn plexspaces_core::MessageSender> = Arc::new(actor_ref2.clone());
    actor_registry2
        .register_actor(
            &ctx,
            remote_actor_id.clone(),
            sender,
            "TestActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    let object_registry: Arc<dyn plexspaces_core::ObjectRegistry> =
        node1.service_locator().get_object_registry().await.unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    let grpc_address = if node2_address.starts_with("http://") {
        node2_address.strip_prefix("http://").unwrap().to_string()
    } else {
        node2_address.clone()
    };
    let node_registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeNode as i32,
        object_id: "node2".to_string(),
        grpc_address,
        object_category: "Node".to_string(),
        ..Default::default()
    };
    object_registry
        .register(&ctx, node_registration)
        .await
        .unwrap();

    tokio::task::yield_now().await;
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let service_locator1 = node1.service_locator().clone();
    let remote_actor_ref = ActorRef::remote(
        remote_actor_id,
        "".to_string(),
        "".to_string(),
        "node2".to_string(),
        service_locator1,
        ActorVisibility::ActorVisibilityPublic,
    );

    for i in 0..5 {
        let message = create_routing_test_message(vec![i]);
        let result = remote_actor_ref.tell(&ctx, message).await;
        assert!(result.is_ok(), "Message {} should succeed", i);
    }

    let mut count = 0;
    for _ in 0..5 {
        if let Some(_) = mailbox2
            .dequeue_with_timeout(Some(tokio::time::Duration::from_secs(5)))
            .await
        {
            count += 1;
        }
    }
    assert_eq!(count, 5, "All 5 messages should have been delivered");
}

#[tokio::test]
async fn test_node_discovery() {
    use plexspaces_node::NodeBuilder;

    let node1 = Arc::new(NodeBuilder::new("node1").build().await);
    let node2 = Arc::new(NodeBuilder::new("node2").build().await);

    let node2_address = start_test_server(node2.clone()).await;

    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    let ctx = node1
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeNode as i32,
        object_id: "node2".to_string(),
        grpc_address: node2_address,
        object_category: "Node".to_string(),
        ..Default::default()
    };
    let object_registry: Arc<dyn plexspaces_core::ObjectRegistry> =
        node1.service_locator().object_registry().await.unwrap();
    object_registry.register(&ctx, registration).await.unwrap();

    let lookup_result = object_registry.lookup(&ctx, "node2", None).await;
    assert!(
        lookup_result.is_ok() && lookup_result.unwrap().is_some(),
        "node2 should be registered in ObjectRegistry"
    );
}
