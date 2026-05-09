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

use plexspaces_actor::{ActorId, ServiceLocator, ServiceLocatorBase, RequestContextExt};
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

fn grpc_request_with_ctx<T>(body: T, namespace: &str) -> Request<T> {
    let mut req = Request::new(body);
    req.metadata_mut()
        .insert("x-tenant-id", "test-tenant".parse().unwrap());
    req.metadata_mut()
        .insert("x-namespace", namespace.parse().unwrap());
    req
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

    // Register a behavior so ActorFactory can create "test_actor" type actors
    use plexspaces_actor::{behavior_factory::BehaviorRegistry, InitializableServiceLocator};
    use plexspaces_behavior::MockBehavior;
    let behavior_registry = BehaviorRegistry::new();
    behavior_registry
        .register_simple("test_actor", || {
            Box::pin(async move { Ok(Box::new(MockBehavior::new()) as Box<dyn plexspaces_actor::Actor>) })
        })
        .await;
    node.service_locator()
        .register_behavior_registry(Arc::new(behavior_registry))
        .await;

    // Create gRPC service
    let service = plexspaces_services::actor_service::ActorServiceImpl::new(
        node.service_locator(),
        node.id().as_str().to_string(),
    );

    // Create SpawnActorRequest (target node is implicit from gRPC endpoint)
    let request = grpc_request_with_ctx(
        spawn_actor_grpc_request("default", "test_actor", "test_actor", ""),
        "default",
    );

    // Spawn actor via gRPC
    let response = ActorServiceTrait::spawn_actor(&service, request).await;

    // Should succeed
    assert!(response.is_ok(), "spawn_actor should succeed: {:?}", response.as_ref().err());

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

    // Register behavior so ActorFactory can create "test_actor" type actors
    use plexspaces_actor::{behavior_factory::BehaviorRegistry, InitializableServiceLocator};
    use plexspaces_behavior::MockBehavior;
    let behavior_registry = BehaviorRegistry::new();
    behavior_registry
        .register_simple("test_actor", || {
            Box::pin(async move { Ok(Box::new(MockBehavior::new()) as Box<dyn plexspaces_actor::Actor>) })
        })
        .await;
    node.service_locator()
        .register_behavior_registry(Arc::new(behavior_registry))
        .await;

    let service = plexspaces_services::actor_service::ActorServiceImpl::new(
        node.service_locator(),
        node.id().as_str().to_string(),
    );

    // gRPC spawn_actor always spawns on the node receiving the request
    // The test for "wrong node" doesn't make sense anymore since target is implicit
    // This test is now redundant - spawn_actor always succeeds on the receiving node
    let request = grpc_request_with_ctx(
        spawn_actor_grpc_request("default", "test_actor", "test_actor", ""),
        "default",
    );

    let response = ActorServiceTrait::spawn_actor(&service, request).await;

    // Should succeed - spawns on node1 (the node receiving the request)
    assert!(
        response.is_ok(),
        "spawn_actor should succeed on receiving node: {:?}", response.as_ref().err()
    );
}

#[tokio::test]
async fn test_spawn_multiple_remote_actors() {
    let node = Arc::new(create_test_node("test-node", 9505));
    node.initialize_services().await.unwrap();

    // Register behavior so ActorFactory can create each actor type
    use plexspaces_actor::{behavior_factory::BehaviorRegistry, InitializableServiceLocator};
    use plexspaces_behavior::MockBehavior;
    let behavior_registry = BehaviorRegistry::new();
    for i in 0..3 {
        let type_name = format!("test_actor_{}", i);
        behavior_registry
            .register_simple(&type_name, || {
                Box::pin(async move { Ok(Box::new(MockBehavior::new()) as Box<dyn plexspaces_actor::Actor>) })
            })
            .await;
    }
    node.service_locator()
        .register_behavior_registry(Arc::new(behavior_registry))
        .await;

    let service = plexspaces_services::actor_service::ActorServiceImpl::new(
        node.service_locator(),
        node.id().as_str().to_string(),
    );

    // Spawn 3 actors
    for i in 0..3 {
        let request = grpc_request_with_ctx(
            spawn_actor_grpc_request("default", &format!("test_actor_{}", i), &format!("test_actor_{}", i), ""),
            "default",
        );

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
    use plexspaces_actor::{behavior_factory::BehaviorRegistry, InitializableServiceLocator};
    use plexspaces_behavior::MockBehavior;
    use plexspaces_node::NodeBuilder;
    use plexspaces_services::actor_service::ActorServiceImpl;
    use tonic::transport::Server;

    let node = Arc::new(NodeBuilder::new("node1").with_auth_disabled().build().await);

    // Register a behavior so spawn can succeed
    let behavior_registry = BehaviorRegistry::new();
    behavior_registry
        .register_simple("remote_test_actor", || {
            Box::pin(async move {
                Ok(Box::new(MockBehavior::new()) as Box<dyn plexspaces_actor::Actor>)
            })
        })
        .await;
    node.service_locator()
        .register_behavior_registry(Arc::new(behavior_registry))
        .await;

    // Start gRPC server on a random port
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let service = ActorServiceImpl::new(node.service_locator(), "node1".to_string());
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let bound_addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", bound_addr);
    tokio::spawn(async move {
        Server::builder()
            .add_service(ActorServiceServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    tokio::task::yield_now().await;
    sleep(Duration::from_millis(100)).await;

    // Spawn actor via gRPC using direct proto client with namespace headers
    use plexspaces_proto::ActorServiceClient;
    let mut grpc_client = ActorServiceClient::connect(server_url.clone())
        .await
        .expect("should connect");
    let spawn_req = spawn_actor_grpc_request("default", "remote_test_actor", "", "test-actor-1");
    let response = grpc_client
        .spawn_actor(grpc_request_with_ctx(spawn_req, "default"))
        .await;
    assert!(response.is_ok(), "gRPC spawn should succeed: {:?}", response.err());

    let resp = response.unwrap().into_inner();
    assert!(!resp.actor_ref.is_empty(), "actor_ref should not be empty");
    let actor_id = ActorId::from_canonical(&resp.actor_ref).expect("valid actor id");
    assert_eq!(actor_id.node_id(), "node1");
}

// =============================================================================
// Tests merged from remote_routing.rs (5 tests)
// =============================================================================

use plexspaces_actor::ActorRef;
use plexspaces_actor::{Message, MessageSender};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_services::actor_service::ActorServiceImpl;
use tonic::transport::Server;

use super::test_helpers::{lookup_actor_ref, test_runtime_actor_id};

/// Helper to create a test message
fn create_routing_test_message(payload: Vec<u8>) -> plexspaces_actor::Message {
    plexspaces_actor::Message {
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
    let actor_registry: Arc<plexspaces_actor::ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })
        .unwrap();
    let ctx = plexspaces_actor::RequestContext::new_without_auth(
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
    let actor_registry2: Arc<plexspaces_actor::ActorRegistry> = node2
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })
        .unwrap();
    let ctx = plexspaces_actor::RequestContext::new_without_auth(
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

    let remote_actor_id = actor_ref2.id().clone();

    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    let object_registry: Arc<dyn plexspaces_actor::ObjectRegistry> =
        node1.service_locator().get_object_registry().await.unwrap();
    // Use empty tenant/namespace so the system context lookup in get_actor_service_client
    // (which uses request_context_for_system_operations with empty tenant) can find it.
    let reg_ctx =
        plexspaces_actor::RequestContext::new_without_auth(String::new(), String::new());
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
        .register(&reg_ctx, node_registration)
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
    let payload = b"{\"data\":\"routing-test\"}".to_vec();
    let message = create_routing_test_message(payload.clone());
    let tell_ctx = plexspaces_actor::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    let result = remote_actor_ref.tell(&tell_ctx, message).await;

    assert!(result.is_ok(), "Remote routing should succeed: {:?}", result.err());
    let received_opt = mailbox2
        .dequeue_with_timeout(Some(tokio::time::Duration::from_secs(5)))
        .await;
    let received = received_opt.expect("Message should arrive within 5 seconds");
    assert_eq!(received.payload, payload);
}

#[tokio::test]
async fn test_node_route_to_unregistered_remote() {
    use plexspaces_node::NodeBuilder;

    let node = Arc::new(NodeBuilder::new("node1").build().await);
    let missing_actor_id = test_runtime_actor_id("actor", "node999");

    let message = create_routing_test_message(vec![7, 8, 9]);
    let tell_ctx = plexspaces_actor::RequestContext::new_without_auth(
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
    let actor_registry2: Arc<plexspaces_actor::ActorRegistry> = node2
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })
        .unwrap();
    let ctx = plexspaces_actor::RequestContext::new_without_auth(
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

    let remote_actor_id = actor_ref2.id().clone();

    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    let object_registry: Arc<dyn plexspaces_actor::ObjectRegistry> =
        node1.service_locator().get_object_registry().await.unwrap();
    // Use empty tenant/namespace so the system context lookup in get_actor_service_client
    // (which uses request_context_for_system_operations with empty tenant) can find it.
    let reg_ctx =
        plexspaces_actor::RequestContext::new_without_auth(String::new(), String::new());
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
        .register(&reg_ctx, node_registration)
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

    let tell_ctx = plexspaces_actor::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    for i in 0..5 {
        let payload = format!("{{\"seq\":{}}}", i).into_bytes();
        let message = create_routing_test_message(payload);
        let result = remote_actor_ref.tell(&tell_ctx, message).await;
        assert!(result.is_ok(), "Message {} should succeed: {:?}", i, result.err());
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
    let object_registry: Arc<dyn plexspaces_actor::ObjectRegistry> =
        node1.service_locator().object_registry().await.unwrap();
    object_registry.register(&ctx, registration).await.unwrap();

    let lookup_result = object_registry.lookup(&ctx, "node2", None).await;
    assert!(
        lookup_result.is_ok() && lookup_result.unwrap().is_some(),
        "node2 should be registered in ObjectRegistry"
    );
}
