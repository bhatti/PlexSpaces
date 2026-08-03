// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

use super::*;
use crate::cluster::{ClusterConfig, ClusterManager};
use plexspaces_actor::ActorId;
use plexspaces_actor::ActorRef;
use plexspaces_mailbox::{mailbox_config_default, Mailbox};
use plexspaces_proto::actor::v1::ActorVisibility;

fn test_runtime_actor_id(name: &str, node_id: &str) -> ActorId {
    ActorId::new(name, "gen_server", "default", node_id).expect("test actor IDs must be valid")
}

// Helper functions for tests (defined inline since we can't import from tests/ directory)
async fn lookup_actor_ref_helper(
    node: &Node,
    actor_id: &ActorId,
) -> Result<Option<ActorRef>, NodeError> {
    use plexspaces_actor::ActorRegistry;
    use std::sync::Arc;

    // Get ActorRegistry
    let actor_registry: Arc<ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| NodeError::ConfigError("ActorRegistry not found".to_string()))?;

    // Check if actor exists
    if let Some(_actor_trait) = actor_registry.lookup_actor(actor_id).await {
        Ok(Some(ActorRef::remote(
            actor_id.clone(),
            "".to_string(), // tenant_id
            "".to_string(), // TODO: get namespace from context
            node.id().as_str().to_string(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        )))
    } else {
        if actor_id.node_id() != node.id().as_str() {
            Ok(Some(ActorRef::remote(
                actor_id.clone(),
                "".to_string(),
                "".to_string(),
                actor_id.node_id().to_string(),
                node.service_locator().clone(),
                ActorVisibility::ActorVisibilityPublic,
            )))
        } else {
            Ok(None)
        }
    }
}

// Alias for consistency with test files
use lookup_actor_ref_helper as lookup_actor_ref;

// Import NodeBuilder for tests
use crate::NodeBuilder;

// Helper to get ActorRegistry from service_locator
async fn get_actor_registry(node: &Node) -> Arc<ActorRegistry> {
    // Ensure services are initialized
    node.initialize_services().await.unwrap();
    node.actor_registry().await.unwrap()
}

// Helper to register actor with MessageSender (replaces register_local)
// Test helper function - registering test actors
// This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
async fn register_actor_for_test(node: &Node, actor_id: &ActorId, mailbox: Arc<Mailbox>) {
    let wrapper = Arc::new(ActorRef::local(
        actor_id.clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox,
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(node).await;
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor_id.clone(),
                sender: wrapper,
                actor_type: actor_id.actor_type().to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;
}

#[tokio::test]
async fn test_node_creation() {
    let node = NodeBuilder::new("test-node").build().await;
    assert_eq!(node.id().as_str(), "test-node");
}

#[tokio::test]
async fn test_actor_registration() {
    use std::sync::Arc;

    let node = NodeBuilder::new("test-node").build().await;

    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_ref = ActorRef::local(
        test_runtime_actor_id("test-actor", "test-node"),
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );

    // Register with ActorRegistry first
    // Register actor with MessageSender (mailbox is internal)

    let wrapper = Arc::new(ActorRef::local(
        actor_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    // Test code - registering and looking up test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    // Register actor (idempotent - can be called multiple times)
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor_ref.id().clone(),
                sender: wrapper,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Should find local actor via ActorRegistry
    assert!(actor_registry.lookup_actor(actor_ref.id()).await.is_some());
}

#[tokio::test]
async fn test_tuplespace_integration() {
    let _node = NodeBuilder::new("test-node").build().await;
}

#[tokio::test]
async fn test_actor_unregistration() {
    use std::sync::Arc;

    let node = NodeBuilder::new("test-node").build().await;

    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_id = test_runtime_actor_id("test-actor", "test-node");
    let actor_ref = ActorRef::local(
        actor_id,
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );

    // Register with ActorRegistry first
    // Register actor with MessageSender (mailbox is internal)

    let wrapper = Arc::new(ActorRef::local(
        actor_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    // Test code - registering test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor_ref.id().clone(),
                sender: wrapper,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Update actor registration with config (idempotent - actor already registered)
    let actor_registry = get_actor_registry(&node).await;
    if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
    }
    // Test code - looking up test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    assert!(actor_registry.lookup_actor(actor_ref.id()).await.is_some());

    // Unregister
    actor_registry
        .unregister_with_cleanup(actor_ref.id())
        .await
        .unwrap();
    // After unregistering, the local sender should no longer be discoverable.
    let lookup_result = actor_registry.lookup_actor(actor_ref.id()).await;
    assert!(
        lookup_result.is_none(),
        "Actor should not be found after unregistering"
    );
}

#[tokio::test]
async fn test_duplicate_actor_registration() {
    use std::sync::Arc;

    let node = NodeBuilder::new("test-node").build().await;

    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_id = test_runtime_actor_id("test-actor", "test-node");
    let actor_ref = ActorRef::local(
        actor_id,
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );

    // Register with ActorRegistry first (using MessageSender)
    register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

    // First registration should succeed
    let actor_registry = get_actor_registry(&node).await;
    if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
    }

    // Second registration should also succeed (idempotent - safe to call multiple times)
    // This is a production-grade design: register_actor is idempotent
    // to allow safe retries and prevent errors from duplicate calls
    let actor_registry = get_actor_registry(&node).await;
    if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
    }
}

#[tokio::test]
async fn test_route_message_local() {
    let node_arc = Arc::new(NodeBuilder::new("test-node").build().await);

    let node = node_arc.as_ref();

    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_id = test_runtime_actor_id("test-actor", "test-node");
    let actor_ref = ActorRef::local(
        actor_id,
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );

    // Register with ActorRegistry (using MessageSender)
    register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

    let actor_registry = get_actor_registry(&node).await;
    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let actor_id = actor_ref.id().clone();
    let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref.clone());
    actor_registry
        .register_actor(
            &ctx,
            ActorRegistrationParams {
                actor_id,
                sender,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Send message via ActorRef directly to local mailbox
    let message = Message {
        id: ulid::Ulid::new().to_string(),
        payload: vec![1, 2, 3],
        ..Default::default()
    };
    actor_ref.tell(&ctx, message).await.unwrap();

    // Verify message was delivered to the mailbox
    // ActorRef::tell() enqueues directly into the mailbox; routing metrics are
    // only updated when messages flow through Node::route_message() / routing.rs.
    let msg = mailbox
        .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(200)))
        .await;
    assert!(msg.is_some(), "message should be delivered to mailbox");
    assert_eq!(msg.unwrap().payload, vec![1, 2, 3]);
}

#[tokio::test]
async fn test_route_message_actor_not_found() {
    let node = NodeBuilder::new("test-node").build().await;

    // Initialize services
    node.initialize_services().await.unwrap();

    let message = Message {
        id: ulid::Ulid::new().to_string(),
        payload: vec![1, 2, 3],
        ..Default::default()
    };

    // Try to send to non-existent actor
    // lookup_actor_ref returns Ok(None) for local actors that don't exist
    let tell_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let result = lookup_actor_ref(&node, &test_runtime_actor_id("nonexistent", "test-node")).await;
    let result = match result {
        Ok(Some(actor_ref)) => actor_ref
            .tell(&tell_ctx, message)
            .await
            .map_err(|e| NodeError::DeliveryFailed(format!("{}", e))),
        Ok(None) => {
            // Local actor not found - try to send anyway to get ActorNotFound error
            // Or return ActorNotFound directly
            Err(NodeError::ActorNotFound(
                "nonexistent@test-node".to_string(),
            ))
        }
        Err(e) => Err(e),
    };
    assert!(result.is_err());
    match result {
        Err(NodeError::ActorNotFound(_)) => {} // Expected
        _ => panic!("Expected ActorNotFound error, got: {:?}", result),
    }
}

#[tokio::test]
async fn test_node_announcement() {
    let _node = NodeBuilder::new("test-node").build().await;
}

#[tokio::test]
async fn test_cluster_manager_join() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let cluster_config = ClusterConfig {
        name: "test-cluster".to_string(),
        seed_nodes: vec![
            (NodeId::new("node1"), "localhost:8000".to_string()),
            (NodeId::new("node2"), "localhost:8001".to_string()),
        ],
        min_nodes: 2,
        auto_discovery: true,
    };

    let manager = ClusterManager::new(node.clone(), cluster_config);

    // Join cluster
    manager.join().await.unwrap();
}

#[tokio::test]
async fn test_cluster_manager_leave() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let cluster_config = ClusterConfig {
        name: "test-cluster".to_string(),
        seed_nodes: vec![],
        min_nodes: 1,
        auto_discovery: false,
    };

    let manager = ClusterManager::new(node.clone(), cluster_config);

    // Join then leave
    manager.join().await.unwrap();
    manager.leave().await.unwrap();
}

// ============================================================================
// spawn_actor() Tests (Erlang-style supervision)
// ============================================================================

#[tokio::test]
async fn test_spawn_actor_creates_and_returns_ref() {
    use plexspaces_actor::behavior::MockBehavior;
    use std::sync::Arc;

    let node = NodeBuilder::new("test-node").build().await;

    // Initialize services (registers all services including ActorFactory)
    node.initialize_services().await.unwrap();

    // Register a BehaviorRegistry so spawn_actor can create "test" actors
    {
        use plexspaces_actor::behavior_factory::BehaviorRegistry;
        let registry = BehaviorRegistry::new();
        registry
            .register_simple("test", || {
                Box::pin(async move {
                    Ok(Box::new(MockBehavior::new()) as Box<dyn plexspaces_actor::Actor>)
                })
            })
            .await;
        node.service_locator()
            .register_behavior_registry(Arc::new(registry))
            .await;
    }

    // Get ActorFactory from ServiceLocator using extension trait
    let service_locator = node.service_locator();
    let actor_factory = service_locator
        .get_actor_factory()
        .await
        .expect("ActorFactoryImpl should be registered after initialize_services()");

    // Test code - spawn with explicit tenant/namespace per Rule #6
    let spawn_ctx = plexspaces_actor::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "default".to_string(),
    );
    // Build actor_id to match spawn spec: name="test-actor", type="test", namespace="default", node="test-node"
    let actor_id =
        ActorId::new("test-actor", "test", "default", "test-node").expect("valid actor id");
    let _message_sender = actor_factory
        .spawn_actor(
            &spawn_ctx,
            &plexspaces_actor::ActorSpawnSpec {
                identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: actor_id.name().to_string(),
                    actor_type: actor_id.actor_type().to_string(),
                }),
                namespace: actor_id.namespace().to_string(),
                tenant_id: "test-tenant".to_string(),
                ..Default::default()
            },
            vec![],
        )
        .await
        .unwrap();

    // Verify actor registered in ActorRegistry — lookup by canonical ActorId
    let actor_registry = get_actor_registry(&node).await;
    assert!(
        actor_registry.lookup_actor(&actor_id).await.is_some(),
        "spawned actor must appear in registry"
    );
}

// Known issue: Actors don't terminate automatically - they require explicit stop.
// Dropping actor_ref doesn't trigger termination notification.
// TODO: Implement proper actor lifecycle with explicit stop_actor() method
// that triggers termination notifications to monitors.

#[tokio::test]
async fn test_spawn_actor_detects_panic() {
    use std::sync::Arc;

    // NOTE: Panics in actor behavior's handle_message() are caught by Rust
    // and converted to BehaviorError. To truly test panic detection, we'd
    // need the panic to happen outside the async function (e.g., in actor loop).
    // For now, this test verifies graceful shutdown detection.
    //
    // True panic detection would require:
    // 1. Panic in tokio::spawn closure (outside process_message)
    // 2. Or explicit panic!() in actor loop
    //
    // This is a known limitation of the current Erlang-simple approach.
    // Actual panics would be caught by JoinHandle and classified as "panic"
    // in spawn_actor's watcher task.

    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Initialize services
    node.initialize_services().await.unwrap();

    // Register a BehaviorRegistry so spawn_actor can create "test" actors
    {
        use plexspaces_actor::behavior_factory::BehaviorRegistry;
        let registry = BehaviorRegistry::new();
        registry
            .register_simple("test", || {
                Box::pin(async move {
                    Ok(Box::new(plexspaces_actor::behavior::MockBehavior::new())
                        as Box<dyn plexspaces_actor::Actor>)
                })
            })
            .await;
        node.service_locator()
            .register_behavior_registry(Arc::new(registry))
            .await;
    }

    // Get ActorFactory from ServiceLocator using extension trait
    let service_locator = node.service_locator();
    let actor_factory = service_locator
        .get_actor_factory()
        .await
        .expect("ActorFactory should be registered after initialize_services()");

    // Test code - spawn with explicit tenant/namespace per Rule #6
    let spawn_ctx = plexspaces_actor::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "default".to_string(),
    );
    // actor_id must match spawn spec: type="test", namespace="default"
    let actor_id =
        ActorId::new("test-actor", "test", "default", "test-node").expect("valid actor id");
    let _message_sender = actor_factory
        .spawn_actor(
            &spawn_ctx,
            &plexspaces_actor::ActorSpawnSpec {
                identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: actor_id.name().to_string(),
                    actor_type: actor_id.actor_type().to_string(),
                }),
                namespace: actor_id.namespace().to_string(),
                tenant_id: "test-tenant".to_string(),
                ..Default::default()
            },
            vec![],
        )
        .await
        .unwrap();

    // Establish monitoring link — supervisor just needs to be registered to receive __DOWN__.
    let supervisor_id = test_runtime_actor_id("supervisor", "test-node");
    register_actor_for_test(node.as_ref(), &supervisor_id, {
        Arc::new(
            Mailbox::new(
                mailbox_config_default(), format!("sup-{}", ulid::Ulid::new()), String::new(), String::new(), None,
            )
            .await
            .unwrap(),
        )
    })
    .await;
    node.monitor(&spawn_ctx, &actor_id, &supervisor_id)
        .await
        .unwrap();

    // Verify the spawned actor is in the registry (monitoring only works on activated actors)
    let actor_registry = get_actor_registry(node.as_ref()).await;
    assert!(
        actor_registry.lookup_actor(&actor_id).await.is_some(),
        "spawned actor must appear in registry before monitoring"
    );
}

// ============================================================================
// Remote Messaging Tests (ActorRef::tell() for remote actors, gRPC client pooling)
// ============================================================================

#[tokio::test]
async fn test_tell_to_remote_node() {
    // Create two nodes
    let node1 = Arc::new(
        NodeBuilder::new("node1")
            .with_in_memory_backends()
            .build()
            .await,
    );

    let node2 = Arc::new(
        NodeBuilder::new("node2")
            .with_in_memory_backends()
            .build()
            .await,
    );

    // Note: We no longer use register_remote_node - node discovery goes through ObjectRegistry/NodeRegistry
    // The registration happens below via ObjectRegistry.register()

    // Register actor on node2
    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let remote_actor_id = test_runtime_actor_id("test-actor", "node2");
    let actor_ref = ActorRef::remote(
        remote_actor_id.clone(),
        "".to_string(),
        "".to_string(),
        "node2",
        node2.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );

    // Register actor with ActorRegistry on node2
    register_actor_for_test(&node2, actor_ref.id(), mailbox.clone()).await;
    let actor_registry2 = get_actor_registry(&node2).await;
    let ctx = node2
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let actor_id = actor_ref.id().clone();
    let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
    actor_registry2
        .register_actor(
            &ctx,
            ActorRegistrationParams {
                actor_id,
                sender,
                actor_type: "gen_server".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Register node2 in ObjectRegistry on node1 (so node1 can find it)
    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    // Tenant comes from auth, not config
    let ctx = RequestContext::new_without_auth(String::new(), String::new());
    let registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeNode as i32,
        object_id: "node2".to_string(),
        grpc_address: "http://localhost:9999".to_string(),
        object_category: "Node".to_string(),
        ..Default::default()
    };
    let object_registry = node1.service_locator.get_object_registry().await.unwrap();
    object_registry.register(&ctx, registration).await.unwrap();

    // Try to route message from node1 to actor on node2
    let message = Message {
        id: ulid::Ulid::new().to_string(),
        payload: vec![1, 2, 3],
        ..Default::default()
    };

    // This will fail because we don't have a real gRPC server running
    // But it exercises the remote routing code path via ActorRef::tell()
    // Initialize services on node1
    node1.initialize_services().await.unwrap();
    let tell_ctx = node1
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let actor_ref = lookup_actor_ref(&node1, &remote_actor_id).await;
    let result = match actor_ref {
        Ok(Some(actor_ref)) => actor_ref
            .tell(&tell_ctx, message)
            .await
            .map_err(|e| NodeError::DeliveryFailed(format!("{}", e))),
        Ok(None) => Err(NodeError::ActorNotFound(remote_actor_id.to_string())),
        Err(e) => Err(e),
    };

    // Should fail with network error (no server listening)
    // The error could be NetworkError, DeliveryFailed, or ActorNotFound (if routing fails before network)
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(
            err,
            NodeError::NetworkError(_) | NodeError::DeliveryFailed(_) | NodeError::ActorNotFound(_)
        ),
        "Expected NetworkError, DeliveryFailed, or ActorNotFound, got: {:?}",
        err
    );
}

#[tokio::test]
async fn test_find_actor_remote_via_node_id() {
    let node = NodeBuilder::new("node1").build().await;

    // Register remote node via NodeRegistry so it ends up in the in-memory cache,
    // which lookup_node checks even when use_shared_db=false.
    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let node_registry = node
        .service_locator
        .get_node_registry()
        .await
        .expect("NodeRegistry should be registered");
    node_registry
        .register_node(
            &ctx,
            plexspaces_proto::node::v1::NodeRegistration {
                node_id: "node2".to_string(),
                node_address: "http://localhost:9999".to_string(),
                status: plexspaces_proto::node::v1::NodeStatus::NodeStatusReady as i32,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let result = node.lookup_node_address(&crate::NodeId::new("node2")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_find_actor_remote_not_found() {
    let node = NodeBuilder::new("node1").build().await;

    let result = node
        .lookup_node_address(&crate::NodeId::new("unknown-node"))
        .await;
    assert!(result.is_err(), "Should fail for non-existent remote node");
}

#[tokio::test]
async fn test_find_actor_via_tuplespace() {
    let node1 = NodeBuilder::new("node1").build().await;

    let node2 = NodeBuilder::new("node2").build().await;

    // Register actor on node2 (this writes to node2's TupleSpace)
    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let remote_actor_id = test_runtime_actor_id("test-actor", "node2");
    let actor_ref = ActorRef::remote(
        remote_actor_id.clone(),
        "".to_string(),
        "".to_string(),
        "node2",
        node2.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );

    // Register actor with ActorRegistry on node2
    register_actor_for_test(&node2, actor_ref.id(), mailbox.clone()).await;
    let actor_registry2 = get_actor_registry(&node2).await;
    let ctx = node2
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let actor_id = actor_ref.id().clone();
    let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
    actor_registry2
        .register_actor(
            &ctx,
            ActorRegistrationParams {
                actor_id,
                sender,
                actor_type: "gen_server".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Register node2 in NodeRegistry on node1 so remote node resolution can succeed
    // (lookup_node checks the in-memory cache, populated by register_node).
    let ctx = node1
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let node_registry1 = node1
        .service_locator
        .get_node_registry()
        .await
        .expect("NodeRegistry should be registered");
    node_registry1
        .register_node(
            &ctx,
            plexspaces_proto::node::v1::NodeRegistration {
                node_id: "node2".to_string(),
                node_address: "http://localhost:9999".to_string(),
                status: plexspaces_proto::node::v1::NodeStatus::NodeStatusReady as i32,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let result = node1
        .lookup_node_address(&crate::NodeId::new("node2"))
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_send_to_remote_node_not_found() {
    let node = NodeBuilder::new("node1").build().await;

    let message = Message {
        id: ulid::Ulid::new().to_string(),
        payload: vec![1, 2, 3],
        ..Default::default()
    };

    // Lookup returns Ok(Some(remote_ref)) even when node is unregistered;
    // the failure surfaces at send time (connection refused / gRPC error).
    let actor_ref =
        lookup_actor_ref(&node, &test_runtime_actor_id("test-actor", "unknown-node")).await;
    assert!(
        actor_ref.is_ok(),
        "lookup should succeed (returns remote ref)"
    );
    let actor_ref = actor_ref.unwrap();
    assert!(
        actor_ref.is_some(),
        "lookup should return a remote actor ref"
    );

    // Sending to an unregistered node fails at delivery time
    let tell_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let send_result = actor_ref.unwrap().tell(&tell_ctx, message).await;
    assert!(send_result.is_err(), "tell to unknown node should fail");
}

// ============================================================================
// Monitoring Infrastructure Tests (monitor, handle_actor_termination)
// ============================================================================

#[tokio::test]
async fn test_monitor_local_actor() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Register a local actor
    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let monitored_actor_id = test_runtime_actor_id("monitored-actor", "test-node");
    let supervisor_id = test_runtime_actor_id("supervisor", "test-node");
    let actor_ref = ActorRef::local(
        monitored_actor_id.clone(),
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );

    // Register with ActorRegistry first (using MessageSender)
    register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

    let actor_registry = get_actor_registry(&node).await;
    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let actor_id = actor_ref.id().clone();
    let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
    actor_registry
        .register_actor(
            &ctx,
            ActorRegistrationParams {
                actor_id,
                sender,
                actor_type: "gen_server".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Register supervisor so it has a mailbox to receive __DOWN__ messages.
    let sup_mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("sup-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    register_actor_for_test(&node, &supervisor_id, sup_mailbox.clone()).await;

    // Monitor the actor
    let monitor_ref = node
        .monitor(&ctx, &monitored_actor_id, &supervisor_id)
        .await
        .unwrap();

    // Verify monitor_ref is a ULID (26 characters)
    assert_eq!(monitor_ref.len(), 26);

    // Notify actor down (via ActorRegistry)
    let actor_registry = get_actor_registry(&node).await;
    actor_registry
        .handle_actor_termination(
            &monitored_actor_id,
            ExitReason::Error("test reason".to_string()),
        )
        .await;

    // Supervisor's mailbox should receive __DOWN__ message.
    let mut down_msg: Option<Message> = None;
    for _ in 0..10 {
        if let Some(msg) = sup_mailbox
            .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(100)))
            .await
        {
            if msg.message_type == "__DOWN__" {
                down_msg = Some(msg);
                break;
            }
        }
    }
    let msg = down_msg.expect("Supervisor must receive __DOWN__ message");
    assert_eq!(
        msg.headers.get("down_from").map(|s| s.as_str()),
        Some(monitored_actor_id.to_string().as_str())
    );
    assert_eq!(
        msg.headers.get("down_reason").map(|s| s.as_str()),
        Some("test reason")
    );
}

#[tokio::test]
async fn test_monitor_local_actor_not_found() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Initialize services
    node.initialize_services().await.unwrap();

    // Try to monitor non-existent actor
    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let result = node
        .monitor(
            &ctx,
            &test_runtime_actor_id("nonexistent", "test-node"),
            &test_runtime_actor_id("supervisor", "test-node"),
        )
        .await;

    // Should fail with ActorNotFound
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), NodeError::ActorNotFound(_)));
}

#[tokio::test]
async fn test_monitor_remote_actor_node_not_connected() {
    let node = Arc::new(NodeBuilder::new("monitor-test-local").build().await);

    // Try to monitor actor on a node that was never registered anywhere
    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let result = node
        .monitor(
            &ctx,
            &test_runtime_actor_id("test-actor", "nonexistent-remote-node"),
            &test_runtime_actor_id("supervisor", "monitor-test-local"),
        )
        .await;

    // Should fail with NodeNotConnected
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        NodeError::NodeNotConnected(_)
    ));
}

#[tokio::test]
async fn test_notify_actor_down_no_monitors() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Notify for actor with no monitors (should not panic)
    let actor_registry = node.actor_registry().await.unwrap();
    actor_registry
        .handle_actor_termination(
            &test_runtime_actor_id("unmonitored-actor", "test-node"),
            ExitReason::Error("reason".to_string()),
        )
        .await;

    // Should succeed (no-op) - handle_actor_termination doesn't return Result, it's void
}

#[tokio::test]
async fn test_handle_actor_termination_multiple_monitors() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Register a local actor
    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let watched_actor_id = test_runtime_actor_id("watched-actor", "test-node");
    let actor_ref = ActorRef::local(
        watched_actor_id.clone(),
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );

    // Register with ActorRegistry first (using MessageSender)
    register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

    let actor_registry = get_actor_registry(&node).await;
    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let actor_id = actor_ref.id().clone();
    let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
    actor_registry
        .register_actor(
            &ctx,
            ActorRegistrationParams {
                actor_id,
                sender,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Register 3 supervisor actors with mailboxes.
    let sup1_id = test_runtime_actor_id("sup1", "test-node");
    let sup2_id = test_runtime_actor_id("sup2", "test-node");
    let sup3_id = test_runtime_actor_id("sup3", "test-node");

    let sup1_mbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("sup1-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let sup2_mbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("sup2-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let sup3_mbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("sup3-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    register_actor_for_test(&node, &sup1_id, sup1_mbox.clone()).await;
    register_actor_for_test(&node, &sup2_id, sup2_mbox.clone()).await;
    register_actor_for_test(&node, &sup3_id, sup3_mbox.clone()).await;

    node.monitor(&ctx, &watched_actor_id, &sup1_id)
        .await
        .unwrap();
    node.monitor(&ctx, &watched_actor_id, &sup2_id)
        .await
        .unwrap();
    node.monitor(&ctx, &watched_actor_id, &sup3_id)
        .await
        .unwrap();

    // Notify actor down
    let actor_registry = node.actor_registry().await.unwrap();
    actor_registry
        .handle_actor_termination(&watched_actor_id, ExitReason::Error("crashed".to_string()))
        .await;

    // All 3 supervisor mailboxes should receive __DOWN__ messages.
    for (sup_mbox, sup_name) in [
        (&sup1_mbox, "sup1"),
        (&sup2_mbox, "sup2"),
        (&sup3_mbox, "sup3"),
    ] {
        let mut found = false;
        for _ in 0..10 {
            if let Some(msg) = sup_mbox
                .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(100)))
                .await
            {
                if msg.message_type == "__DOWN__" {
                    assert_eq!(
                        msg.headers.get("down_reason").map(|s| s.as_str()),
                        Some("crashed"),
                        "{} should get 'crashed' reason",
                        sup_name
                    );
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "{} must receive __DOWN__", sup_name);
    }
}

// ============================================================================
// Lifecycle Event Tests (subscribe/unsubscribe/publish)
// ============================================================================

#[tokio::test]
async fn test_lifecycle_event_subscription() {
    use tokio::sync::mpsc;

    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Initialize services
    node.initialize_services().await.unwrap();

    // Create subscription channel
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Subscribe
    node.subscribe_lifecycle_events(tx).await;

    // Publish a test event
    let event = plexspaces_proto::ActorLifecycleEvent {
        actor_id: "test-actor@test-node".to_string(),
        timestamp: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        }),
        event_type: Some(plexspaces_proto::actor_lifecycle_event::EventType::Created(
            plexspaces_proto::v1::actor::ActorCreated {},
        )),
    };

    let actor_registry = get_actor_registry(&node).await;
    actor_registry.publish_lifecycle_event(event.clone()).await;

    // Should receive event
    let received = tokio::time::timeout(tokio::time::Duration::from_millis(500), rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(received.actor_id, "test-actor@test-node");
}

#[tokio::test]
async fn test_lifecycle_event_unsubscribe() {
    use tokio::sync::mpsc;

    let node = Arc::new(
        NodeBuilder::new("test-node")
            .with_in_memory_backends()
            .build()
            .await,
    );
    node.initialize_services().await.unwrap();

    // Create subscription channel
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Subscribe
    node.subscribe_lifecycle_events(tx).await;

    // Publish one event and confirm subscription is active first.
    let first_event = plexspaces_proto::ActorLifecycleEvent {
        actor_id: "test-actor@test-node".to_string(),
        timestamp: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        }),
        event_type: Some(plexspaces_proto::actor_lifecycle_event::EventType::Created(
            plexspaces_proto::v1::actor::ActorCreated {},
        )),
    };
    let actor_registry = get_actor_registry(&node).await;
    actor_registry.publish_lifecycle_event(first_event).await;
    let received = tokio::time::timeout(tokio::time::Duration::from_millis(500), rx.recv())
        .await
        .unwrap();
    assert!(
        received.is_some(),
        "subscriber should receive event before unsubscribe"
    );

    // Unsubscribe
    node.unsubscribe_lifecycle_events().await;

    // Publish another event after unsubscribe.
    let event = plexspaces_proto::ActorLifecycleEvent {
        actor_id: "test-actor@test-node".to_string(),
        timestamp: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        }),
        event_type: Some(plexspaces_proto::actor_lifecycle_event::EventType::Created(
            plexspaces_proto::v1::actor::ActorCreated {},
        )),
    };

    actor_registry.publish_lifecycle_event(event).await;

    // After unsubscribe, no new event should be delivered.
    let after_unsubscribe =
        tokio::time::timeout(tokio::time::Duration::from_millis(100), rx.recv()).await;
    assert!(
        !matches!(after_unsubscribe, Ok(Some(_))),
        "subscriber should not receive events after unsubscribe"
    );
}

#[tokio::test]
async fn test_handle_lifecycle_event_terminated() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Register actor and monitor it
    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let monitored_actor_id = test_runtime_actor_id("test-actor", "test-node");
    let actor_ref = ActorRef::local(
        monitored_actor_id.clone(),
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );
    // Register actor with MessageSender (mailbox is internal)

    let wrapper = Arc::new(ActorRef::local(
        actor_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    // Test code - registering test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor_ref.id().clone(),
                sender: wrapper,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Register with ActorRegistry first (using MessageSender)
    register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

    let actor_registry = get_actor_registry(&node).await;
    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let actor_id = actor_ref.id().clone();
    let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
    actor_registry
        .register_actor(
            &ctx,
            ActorRegistrationParams {
                actor_id,
                sender,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    let supervisor_id = test_runtime_actor_id("supervisor", "test-node");
    let sup_mbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("sup-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    register_actor_for_test(&node, &supervisor_id, sup_mbox.clone()).await;
    node.monitor(&ctx, &monitored_actor_id, &supervisor_id)
        .await
        .unwrap();

    // Create Terminated event
    let event = plexspaces_proto::ActorLifecycleEvent {
        actor_id: monitored_actor_id.as_str().to_string(),
        timestamp: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        }),
        event_type: Some(
            plexspaces_proto::actor_lifecycle_event::EventType::Terminated(
                plexspaces_proto::v1::actor::ActorTerminated {
                    reason: "normal".to_string(),
                },
            ),
        ),
    };

    // Handle the event
    node.handle_lifecycle_event(event).await.unwrap();

    // Supervisor mailbox should receive __DOWN__ message.
    let mut down: Option<Message> = None;
    for _ in 0..10 {
        if let Some(msg) = sup_mbox
            .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(100)))
            .await
        {
            if msg.message_type == "__DOWN__" {
                down = Some(msg);
                break;
            }
        }
    }
    let msg = down.expect("Supervisor must receive __DOWN__");
    assert_eq!(
        msg.headers.get("down_reason").map(|s| s.as_str()),
        Some("normal")
    );
}

#[tokio::test]
async fn test_handle_lifecycle_event_failed() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Register actor and monitor it
    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let monitored_actor_id = test_runtime_actor_id("test-actor", "test-node");
    let actor_ref = ActorRef::local(
        monitored_actor_id.clone(),
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );
    // Register actor with MessageSender (mailbox is internal)

    let wrapper = Arc::new(ActorRef::local(
        actor_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    // Test code - registering test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor_ref.id().clone(),
                sender: wrapper,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Register with ActorRegistry first (using MessageSender)
    register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

    let actor_registry = get_actor_registry(&node).await;
    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let actor_id = actor_ref.id().clone();
    let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
    actor_registry
        .register_actor(
            &ctx,
            ActorRegistrationParams {
                actor_id,
                sender,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    let supervisor_id = test_runtime_actor_id("supervisor", "test-node");
    let sup_mbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("sup-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    register_actor_for_test(&node, &supervisor_id, sup_mbox.clone()).await;
    node.monitor(&ctx, &monitored_actor_id, &supervisor_id)
        .await
        .unwrap();

    // Create Failed event
    let event = plexspaces_proto::ActorLifecycleEvent {
        actor_id: monitored_actor_id.as_str().to_string(),
        timestamp: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        }),
        event_type: Some(plexspaces_proto::actor_lifecycle_event::EventType::Failed(
            plexspaces_proto::v1::actor::ActorFailed {
                error: "panic: index out of bounds".to_string(),
                stack_trace: String::new(),
            },
        )),
    };

    // Handle the event
    node.handle_lifecycle_event(event).await.unwrap();

    // Supervisor mailbox should receive __DOWN__ message.
    let mut down: Option<Message> = None;
    for _ in 0..10 {
        if let Some(msg) = sup_mbox
            .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(100)))
            .await
        {
            if msg.message_type == "__DOWN__" {
                down = Some(msg);
                break;
            }
        }
    }
    let msg = down.expect("Supervisor must receive __DOWN__");
    assert_eq!(
        msg.headers.get("down_reason").map(|s| s.as_str()),
        Some("panic: index out of bounds")
    );
}

#[tokio::test]
async fn test_handle_lifecycle_event_other() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Create Starting event (non-terminal event)
    let event = plexspaces_proto::ActorLifecycleEvent {
        actor_id: "test-actor@test-node".to_string(),
        timestamp: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        }),
        event_type: Some(
            plexspaces_proto::actor_lifecycle_event::EventType::Starting(
                plexspaces_proto::v1::actor::ActorStarting {},
            ),
        ),
    };

    // Handle the event (should be no-op for non-terminal events)
    let result = node.handle_lifecycle_event(event).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_stats_tracking() {
    let node_arc = Arc::new(NodeBuilder::new("test-node").build().await);

    let node = node_arc.as_ref();

    // Initial stats
    let node_metrics = node.metrics().await;
    assert_eq!(node_metrics.messages_routed, 0);
    assert_eq!(node_metrics.local_deliveries, 0);
    assert_eq!(node_metrics.active_actors, 0);

    // Register an actor
    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_id = test_runtime_actor_id("test-actor", "test-node");
    let actor_ref = ActorRef::local(
        actor_id,
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );

    // Register with ActorRegistry (using MessageSender)
    register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

    let actor_registry = get_actor_registry(&node).await;
    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let actor_id = actor_ref.id().clone();
    let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref.clone());
    actor_registry
        .register_actor(
            &ctx,
            ActorRegistrationParams {
                actor_id,
                sender,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // active_actors is only updated when actors are spawned via ActorFactory, not when registered
    // So we check that the actor is registered instead
    let actor_registry = get_actor_registry(&node).await;
    let lookup_result = actor_registry.lookup_actor(actor_ref.id()).await;
    assert!(lookup_result.is_some(), "Actor should be registered");

    // Send local message via ActorRef directly to mailbox
    let message = Message {
        id: ulid::Ulid::new().to_string(),
        payload: vec![1, 2, 3],
        ..Default::default()
    };
    actor_ref.tell(&ctx, message).await.unwrap();

    // Verify delivery: ActorRef::tell() enqueues into the mailbox.
    // Routing counters (messages_routed, local_deliveries) are only updated
    // when messages flow through Node::route_message() / routing.rs — not here.
    let delivered = mailbox
        .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(200)))
        .await;
    assert!(
        delivered.is_some(),
        "message should be delivered to mailbox"
    );

    // Unregister actor
    let actor_registry = get_actor_registry(&node).await;
    actor_registry
        .unregister_with_cleanup(actor_ref.id())
        .await
        .unwrap();

    // Check stats updated
    let node_metrics = node.metrics().await;
    assert_eq!(node_metrics.active_actors, 0);
}

// ============================================================================
// Phase 3: Actor Resource Requirements Tests
// ============================================================================

/// Helper to create an actor config with resource requirements
fn create_actor_config_with_resources(
    cpu_cores: f64,
    memory_bytes: u64,
    disk_bytes: u64,
    gpu_count: u32,
) -> plexspaces_proto::v1::actor::ActorConfig {
    use plexspaces_proto::{
        common::v1::ResourceSpec,
        v1::actor::{ActorConfig, ActorResourceRequirements, NodePlacement, NodePlacementStrategy},
    };

    let resource_requirements = ActorResourceRequirements {
        placement: Some(NodePlacement {
            strategy: NodePlacementStrategy::NodePlacementStrategyUnspecified as i32,
            cluster: String::new(),
            node_ids: vec![],
            required_labels: std::collections::HashMap::new(),
            avoid_node_ids: vec![],
            resource_requirements: Some(ResourceSpec {
                cpu_cores,
                memory_bytes,
                disk_bytes,
                gpu_count,
                gpu_type: String::new(),
            }),
            affinity_labels: std::collections::HashMap::new(),
        }),
    };

    ActorConfig {
        mailbox_timeout: None,
        max_mailbox_size: 1000,
        enable_persistence: false,
        checkpoint_interval: None,
        restart_policy: None,
        supervision_strategy: 0,
        properties: std::collections::HashMap::new(),
        stateless_worker_config: None,
        data_parallel_config: None,
        state_management_mode: 0,
        consistency_level: 0,
        resource_requirements: Some(resource_requirements),
        actor_groups: vec![],
        config_schema_version: 1,
    }
}

#[tokio::test]
async fn test_register_actor_with_config() {
    use std::sync::Arc;
    let node = NodeBuilder::new("test-node").build().await;

    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_id = test_runtime_actor_id("test-actor", "test-node");
    let actor_ref = ActorRef::local(
        actor_id,
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );
    // Register actor with MessageSender (mailbox is internal)

    let wrapper = Arc::new(ActorRef::local(
        actor_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    // Test code - registering test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor_ref.id().clone(),
                sender: wrapper,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    let config = create_actor_config_with_resources(2.0, 1024 * 1024 * 512, 1024 * 1024 * 1024, 0);

    // Update actor registration with config (idempotent - actor already registered)
    let actor_registry = get_actor_registry(&node).await;
    if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: Some(config.clone()),
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
    }

    // Verify config is stored
    let actor_configs_arc = get_actor_registry(&node).await.actor_configs().clone();
    let actor_configs = actor_configs_arc.read().await;
    assert!(actor_configs.contains_key(actor_ref.id()));
    let stored_config = actor_configs.get(actor_ref.id()).unwrap();
    assert_eq!(
        stored_config
            .resource_requirements
            .as_ref()
            .unwrap()
            .placement
            .as_ref()
            .unwrap()
            .resource_requirements
            .as_ref()
            .unwrap()
            .cpu_cores,
        2.0
    );
}

#[tokio::test]
async fn test_register_actor_without_config() {
    use std::sync::Arc;
    let node = NodeBuilder::new("test-node").build().await;

    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_id = test_runtime_actor_id("test-actor", "test-node");
    let actor_ref = ActorRef::local(
        actor_id,
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );
    // Register actor with MessageSender (mailbox is internal)

    let wrapper = Arc::new(ActorRef::local(
        actor_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    // Test code - registering test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor_ref.id().clone(),
                sender: wrapper,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Actor already registered - no need to update config
    let _actor_registry = get_actor_registry(&node).await;

    // Verify config is not stored
    let actor_configs_arc = get_actor_registry(&node).await.actor_configs().clone();
    let actor_configs = actor_configs_arc.read().await;
    assert!(!actor_configs.contains_key(actor_ref.id()));
}

#[tokio::test]
async fn test_unregister_actor_removes_config() {
    use std::sync::Arc;
    let node = NodeBuilder::new("test-node").build().await;

    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_id = test_runtime_actor_id("test-actor", "test-node");
    let actor_ref = ActorRef::local(
        actor_id,
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );
    // Register actor with MessageSender (mailbox is internal)

    let wrapper = Arc::new(ActorRef::local(
        actor_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    // Test code - registering test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor_ref.id().clone(),
                sender: wrapper,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    let config = create_actor_config_with_resources(1.0, 1024 * 1024 * 256, 0, 0);

    // Register actor with config
    let actor_registry = get_actor_registry(&node).await;
    if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: Some(config),
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
    }

    // Verify config is stored
    {
        let actor_configs_arc = get_actor_registry(&node).await.actor_configs().clone();
        let actor_configs = actor_configs_arc.read().await;
        assert!(actor_configs.contains_key(actor_ref.id()));
    }

    // Unregister actor
    let actor_registry = get_actor_registry(&node).await;
    actor_registry
        .unregister_with_cleanup(actor_ref.id())
        .await
        .unwrap();

    // Verify config is removed
    let actor_configs_arc = get_actor_registry(&node).await.actor_configs().clone();
    let actor_configs = actor_configs_arc.read().await;
    assert!(!actor_configs.contains_key(actor_ref.id()));
}

#[tokio::test]
async fn test_calculate_node_capacity_with_actors() {
    use std::sync::Arc;

    let node = NodeBuilder::new("test-node").build().await;

    // Register first actor with resources
    let mailbox1 = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-1-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor1_id = test_runtime_actor_id("actor-1", "test-node");
    let actor1_ref = ActorRef::local(
        actor1_id,
        "".to_string(),
        "".to_string(),
        mailbox1.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );
    // Register actor with MessageSender (mailbox is internal)

    let wrapper1 = Arc::new(ActorRef::local(
        actor1_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox1.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor1_ref.id().clone(),
                sender: wrapper1,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;
    let config1 = create_actor_config_with_resources(2.0, 1024 * 1024 * 512, 1024 * 1024 * 1024, 0);
    let actor_registry = get_actor_registry(&node).await;
    if let Some(sender1) = actor_registry.lookup_actor(&actor1_ref.id().clone()).await {
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id: actor1_ref.id().clone(),
                    sender: sender1,
                    actor_type: "test_actor".to_string(),
                    config: Some(config1),
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
    }

    // Register second actor with resources
    let mailbox2 = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-2-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor2_id = test_runtime_actor_id("actor-2", "test-node");
    let actor2_ref = ActorRef::local(
        actor2_id,
        "".to_string(),
        "".to_string(),
        mailbox2.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );
    let config2 = create_actor_config_with_resources(1.5, 1024 * 1024 * 256, 512 * 1024 * 1024, 1);
    // Register actor2 with MessageSender first
    let wrapper2 = Arc::new(ActorRef::local(
        actor2_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox2.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor2_ref.id().clone(),
                sender: wrapper2,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;
    let actor_registry = get_actor_registry(&node).await;
    if let Some(sender2) = actor_registry.lookup_actor(&actor2_ref.id().clone()).await {
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id: actor2_ref.id().clone(),
                    sender: sender2,
                    actor_type: "test_actor".to_string(),
                    config: Some(config2),
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
    }

    // Calculate capacity
    let capacity = node.calculate_node_capacity().await;

    // Verify allocated resources are summed correctly
    let allocated = capacity.allocated.as_ref().unwrap();
    assert_eq!(allocated.cpu_cores, 3.5); // 2.0 + 1.5
    assert_eq!(
        allocated.memory_bytes,
        1024 * 1024 * 512 + 1024 * 1024 * 256
    ); // 512MB + 256MB
    assert_eq!(allocated.disk_bytes, 1024 * 1024 * 1024 + 512 * 1024 * 1024); // 1GB + 512MB
    assert_eq!(allocated.gpu_count, 1); // 0 + 1

    // Verify available resources are calculated correctly
    let available = capacity.available.as_ref().unwrap();
    let total = capacity.total.as_ref().unwrap();
    assert_eq!(available.cpu_cores, total.cpu_cores - allocated.cpu_cores);
    assert_eq!(
        available.memory_bytes,
        total.memory_bytes - allocated.memory_bytes
    );
}

#[tokio::test]
async fn test_calculate_node_capacity_without_actors() {
    let node = NodeBuilder::new("test-node").build().await;

    // Calculate capacity with no actors
    let capacity = node.calculate_node_capacity().await;

    // Verify allocated resources are zero
    let allocated = capacity.allocated.as_ref().unwrap();
    assert_eq!(allocated.cpu_cores, 0.0);
    assert_eq!(allocated.memory_bytes, 0);
    assert_eq!(allocated.disk_bytes, 0);
    assert_eq!(allocated.gpu_count, 0);

    // Verify available equals total
    let available = capacity.available.as_ref().unwrap();
    let total = capacity.total.as_ref().unwrap();
    assert_eq!(available.cpu_cores, total.cpu_cores);
    assert_eq!(available.memory_bytes, total.memory_bytes);
}

#[tokio::test]
async fn test_calculate_node_capacity_with_actor_without_resources() {
    let node = NodeBuilder::new("test-node").build().await;

    // Register actor without resource requirements
    let mailbox = Arc::new(
        Mailbox::new(
            plexspaces_mailbox::mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_id = test_runtime_actor_id("actor-1", "test-node");
    let actor_ref = ActorRef::local(
        actor_id,
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );
    // Register actor with MessageSender (mailbox is internal)

    let wrapper = Arc::new(ActorRef::local(
        actor_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    // Test code - registering test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor_ref.id().clone(),
                sender: wrapper,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;
    let mut config = plexspaces_proto::v1::actor::ActorConfig::default();
    config.resource_requirements = None; // No resource requirements
    config.config_schema_version = 1;
    let actor_registry = get_actor_registry(&node).await;
    if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: Some(config),
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
    }

    // Calculate capacity
    let capacity = node.calculate_node_capacity().await;

    // Verify allocated resources are still zero (actor has no requirements)
    let allocated = capacity.allocated.as_ref().unwrap();
    assert_eq!(allocated.cpu_cores, 0.0);
    assert_eq!(allocated.memory_bytes, 0);
}

#[tokio::test]
async fn test_calculate_node_capacity_after_unregister() {
    use std::sync::Arc;
    let node = NodeBuilder::new("test-node").build().await;

    // Register actor with resources
    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_id = test_runtime_actor_id("actor-1", "test-node");
    let actor_ref = ActorRef::local(
        actor_id,
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );
    let config = create_actor_config_with_resources(2.0, 1024 * 1024 * 512, 0, 0);
    let actor_registry = get_actor_registry(&node).await;

    // Register actor first
    // Tenant comes from auth, not config
    let ctx = RequestContext::new_without_auth(String::new(), String::new());
    let actor_id = actor_ref.id().clone();
    let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref.clone());
    actor_registry
        .register_actor(
            &ctx,
            ActorRegistrationParams {
                actor_id: actor_id.clone(),
                sender: sender.clone(),
                actor_type: "test_actor".to_string(),
                config: Some(config),
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

    // Verify allocated resources
    let capacity = node.calculate_node_capacity().await;
    let allocated = capacity.allocated.as_ref().unwrap();
    assert_eq!(allocated.cpu_cores, 2.0);

    // Unregister actor
    let actor_registry = get_actor_registry(&node).await;
    actor_registry
        .unregister_with_cleanup(actor_ref.id())
        .await
        .unwrap();

    // Verify allocated resources are back to zero
    let capacity = node.calculate_node_capacity().await;
    let allocated = capacity.allocated.as_ref().unwrap();
    assert_eq!(allocated.cpu_cores, 0.0);
    assert_eq!(allocated.memory_bytes, 0);
}

#[tokio::test]
async fn test_calculate_node_capacity_with_partial_resource_spec() {
    let node = NodeBuilder::new("test-node").build().await;

    // Create config with only CPU specified (no memory/disk)
    use plexspaces_proto::{
        common::v1::ResourceSpec,
        v1::actor::{ActorConfig, ActorResourceRequirements},
    };

    let resources = ResourceSpec {
        cpu_cores: 1.0,
        memory_bytes: 0,
        disk_bytes: 0,
        gpu_count: 0,
        gpu_type: String::new(),
    };

    let resource_requirements = ActorResourceRequirements {
        placement: Some(plexspaces_proto::v1::actor::NodePlacement {
            strategy:
                plexspaces_proto::v1::actor::NodePlacementStrategy::NodePlacementStrategyUnspecified
                    as i32,
            cluster: String::new(),
            node_ids: vec![],
            required_labels: std::collections::HashMap::new(),
            avoid_node_ids: vec![],
            resource_requirements: Some(resources),
            affinity_labels: std::collections::HashMap::new(),
        }),
    };

    let mut config = ActorConfig::default();
    config.resource_requirements = Some(resource_requirements);
    config.config_schema_version = 1;

    use std::sync::Arc;
    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None,
        )
        .await
        .unwrap(),
    );
    let actor_id = test_runtime_actor_id("actor-1", "test-node");
    let actor_ref = ActorRef::local(
        actor_id,
        "".to_string(),
        "".to_string(),
        mailbox.clone(),
        node.service_locator(),
        ActorVisibility::ActorVisibilityPublic,
    );
    // Register actor with MessageSender (mailbox is internal)

    let wrapper = Arc::new(ActorRef::local(
        actor_ref.id().clone(),
        "".to_string(), // test tenant
        "".to_string(), // test namespace
        mailbox.clone(),
        node.service_locator().clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let actor_registry = get_actor_registry(&node).await;
    // Test code - registering test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    let internal_ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;
    actor_registry
        .register_actor(
            &internal_ctx,
            ActorRegistrationParams {
                actor_id: actor_ref.id().clone(),
                sender: wrapper,
                actor_type: "test_actor".to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;
    let actor_registry = get_actor_registry(&node).await;
    if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: Some(config),
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
    }

    // Calculate capacity
    let capacity = node.calculate_node_capacity().await;
    let allocated = capacity.allocated.as_ref().unwrap();

    // Verify only CPU is allocated
    assert_eq!(allocated.cpu_cores, 1.0);
    assert_eq!(allocated.memory_bytes, 0);
    assert_eq!(allocated.disk_bytes, 0);
}

// ============================================================================
// Application Lifecycle Tests (Erlang/OTP-style)
// ============================================================================

use async_trait::async_trait;
use plexspaces_application::{Application, ApplicationError, ApplicationNode};
use plexspaces_common::{RequestContext, RequestContextExt};
use plexspaces_proto::v1::application::{ApplicationState, HealthStatus};

fn app_ctx(name: &str) -> RequestContext {
    RequestContext::new_without_auth(String::new(), name.to_string())
}

// Mock application for testing
struct MockTestApplication {
    name: String,
    should_fail_start: bool,
    should_fail_stop: bool,
    start_called: Arc<RwLock<bool>>,
    stop_called: Arc<RwLock<bool>>,
}

impl MockTestApplication {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            start_called: Arc::new(RwLock::new(false)),
            stop_called: Arc::new(RwLock::new(false)),
        }
    }

    fn new_failing_start(name: &str) -> Self {
        Self {
            name: name.to_string(),
            should_fail_start: true,
            should_fail_stop: false,
            start_called: Arc::new(RwLock::new(false)),
            stop_called: Arc::new(RwLock::new(false)),
        }
    }

    fn new_failing_stop(name: &str) -> Self {
        Self {
            name: name.to_string(),
            should_fail_start: false,
            should_fail_stop: true,
            start_called: Arc::new(RwLock::new(false)),
            stop_called: Arc::new(RwLock::new(false)),
        }
    }
}

#[async_trait]
impl Application for MockTestApplication {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        *self.start_called.write().await = true;
        tracing::warn!("MockApp '{}' starting on node: {}", self.name, node.id());
        if self.should_fail_start {
            Err(ApplicationError::StartupFailed("mock failure".to_string()))
        } else {
            Ok(())
        }
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        *self.stop_called.write().await = true;
        tracing::warn!("MockApp '{}' stopping", self.name);
        if self.should_fail_stop {
            Err(ApplicationError::ShutdownFailed("mock failure".to_string()))
        } else {
            Ok(())
        }
    }

    async fn health_check(&self) -> HealthStatus {
        HealthStatus::HealthStatusHealthy
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[tokio::test]
async fn test_register_application() {
    let node = NodeBuilder::new("test-node").build().await;

    let app = Box::new(MockTestApplication::new("test-app"));
    node.application_manager()
        .register(&app_ctx("test-app"), app)
        .await
        .unwrap();

    // Verify application is registered
    let state = node.application_manager().get_state("test-app").await;
    assert_eq!(state, Some(ApplicationState::ApplicationStateCreated));
}

#[tokio::test]
async fn test_register_duplicate_application() {
    let node = NodeBuilder::new("test-node").build().await;

    let app1 = Box::new(MockTestApplication::new("test-app"));
    node.application_manager()
        .register(&app_ctx("test-app"), app1)
        .await
        .unwrap();

    let app2 = Box::new(MockTestApplication::new("test-app"));
    let result = node
        .application_manager()
        .register(&app_ctx("test-app"), app2)
        .await;

    // Should fail with duplicate error
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("already registered"));
}

#[tokio::test]
async fn test_start_application() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    let app = Box::new(MockTestApplication::new("test-app"));
    let start_called = app.start_called.clone();

    node.application_manager()
        .register(&app_ctx("test-app"), app)
        .await
        .unwrap();
    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    node.application_manager().start("test-app").await.unwrap();

    // Verify application started
    let state = node.application_manager().get_state("test-app").await;
    assert_eq!(state, Some(ApplicationState::ApplicationStateRunning));

    // Verify start was called
    assert!(*start_called.read().await);
}

#[tokio::test]
async fn test_start_application_failure() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    let app = Box::new(MockTestApplication::new_failing_start("test-app"));
    node.application_manager()
        .register(&app_ctx("test-app"), app)
        .await
        .unwrap();

    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    let result = node.application_manager().start("test-app").await;

    // Should fail
    assert!(result.is_err());

    // State should be Failed
    let state = node.application_manager().get_state("test-app").await;
    assert_eq!(state, Some(ApplicationState::ApplicationStateFailed));
}

#[tokio::test]
async fn test_stop_application() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    let app = Box::new(MockTestApplication::new("test-app"));
    let stop_called = app.stop_called.clone();

    node.application_manager()
        .register(&app_ctx("test-app"), app)
        .await
        .unwrap();
    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    node.application_manager().start("test-app").await.unwrap();
    node.application_manager()
        .stop("test-app", tokio::time::Duration::from_secs(5))
        .await
        .unwrap();

    // Verify application stopped
    let state = node.application_manager().get_state("test-app").await;
    assert_eq!(state, Some(ApplicationState::ApplicationStateStopped));

    // Verify stop was called
    assert!(*stop_called.read().await);
}

#[tokio::test]
async fn test_stop_application_failure() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    let app = Box::new(MockTestApplication::new_failing_stop("test-app"));
    node.application_manager()
        .register(&app_ctx("test-app"), app)
        .await
        .unwrap();
    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    node.application_manager().start("test-app").await.unwrap();

    let result = node
        .application_manager()
        .stop("test-app", tokio::time::Duration::from_secs(5))
        .await;

    // Should fail
    assert!(result.is_err());

    // State should be Failed
    let state = node.application_manager().get_state("test-app").await;
    assert_eq!(state, Some(ApplicationState::ApplicationStateFailed));
}

#[tokio::test]
async fn test_shutdown_multiple_applications() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Register and start 3 applications
    let apps = vec![
        Box::new(MockTestApplication::new("app1")) as Box<dyn Application>,
        Box::new(MockTestApplication::new("app2")) as Box<dyn Application>,
        Box::new(MockTestApplication::new("app3")) as Box<dyn Application>,
    ];

    for app in apps {
        node.application_manager()
            .register(&app_ctx(app.name()), app)
            .await
            .unwrap();
    }

    node.application_manager()
        .ensure_node_context(node.clone())
        .await;
    node.application_manager().start("app1").await.unwrap();
    node.application_manager()
        .ensure_node_context(node.clone())
        .await;
    node.application_manager().start("app2").await.unwrap();
    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    node.application_manager().start("app3").await.unwrap();

    // Shutdown all applications
    node.shutdown(tokio::time::Duration::from_secs(10))
        .await
        .unwrap();

    // Verify all applications stopped
    assert_eq!(
        node.application_manager().get_state("app1").await,
        Some(ApplicationState::ApplicationStateStopped)
    );
    assert_eq!(
        node.application_manager().get_state("app2").await,
        Some(ApplicationState::ApplicationStateStopped)
    );
    assert_eq!(
        node.application_manager().get_state("app3").await,
        Some(ApplicationState::ApplicationStateStopped)
    );

    // Verify shutdown flag set
    assert!(node.is_shutdown_requested().await);
}

#[tokio::test]
async fn test_application_node_trait_implementation() {
    use crate::NodeBuilder;
    let node = NodeBuilder::new("test-node")
        .with_listen_addr("0.0.0.0:9999")
        .build()
        .await;

    // Test ApplicationNode trait methods (uses trait methods, not Node methods)
    let node_ref: &dyn ApplicationNode = &node;
    assert_eq!(node_ref.id(), "test-node");
    assert_eq!(node_ref.listen_addr(), "0.0.0.0:9999");
}

#[tokio::test]
async fn test_start_nonexistent_application() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    let result = node.application_manager().start("nonexistent").await;

    // Should fail with not found error
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn test_stop_nonexistent_application() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    let result = node
        .application_manager()
        .stop("nonexistent", tokio::time::Duration::from_secs(5))
        .await;

    // Should fail with not found error
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn test_application_lifecycle_full_cycle() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    let app = Box::new(MockTestApplication::new("lifecycle-test"));
    let start_called = app.start_called.clone();
    let stop_called = app.stop_called.clone();

    // Full lifecycle: register -> start -> stop
    node.application_manager()
        .register(&app_ctx("lifecycle-test"), app)
        .await
        .unwrap();
    assert_eq!(
        node.application_manager().get_state("lifecycle-test").await,
        Some(ApplicationState::ApplicationStateCreated)
    );

    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    node.application_manager()
        .start("lifecycle-test")
        .await
        .unwrap();
    assert_eq!(
        node.application_manager().get_state("lifecycle-test").await,
        Some(ApplicationState::ApplicationStateRunning)
    );
    assert!(*start_called.read().await);

    node.application_manager()
        .stop("lifecycle-test", tokio::time::Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(
        node.application_manager().get_state("lifecycle-test").await,
        Some(ApplicationState::ApplicationStateStopped)
    );
    assert!(*stop_called.read().await);
}

#[tokio::test]
async fn test_shutdown_with_partial_failure() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Register 2 apps: one normal, one failing to stop
    let app1 = Box::new(MockTestApplication::new("good-app")) as Box<dyn Application>;
    let app2 = Box::new(MockTestApplication::new_failing_stop("bad-app")) as Box<dyn Application>;

    node.application_manager()
        .register(&app_ctx("good-app"), app1)
        .await
        .unwrap();
    node.application_manager()
        .register(&app_ctx("bad-app"), app2)
        .await
        .unwrap();

    node.application_manager()
        .ensure_node_context(node.clone())
        .await;
    node.application_manager().start("good-app").await.unwrap();
    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    node.application_manager().start("bad-app").await.unwrap();

    // Shutdown should fail due to bad-app
    let result = node.shutdown(tokio::time::Duration::from_secs(5)).await;
    assert!(result.is_err());

    // good-app should still be stopped
    assert_eq!(
        node.application_manager().get_state("good-app").await,
        Some(ApplicationState::ApplicationStateStopped)
    );

    // bad-app should be in Failed state
    assert_eq!(
        node.application_manager().get_state("bad-app").await,
        Some(ApplicationState::ApplicationStateFailed)
    );
}

#[tokio::test]
async fn test_shutdown_request_flag() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Initially not requested
    assert!(!node.is_shutdown_requested().await);

    // Register and start an app
    let app = Box::new(MockTestApplication::new("test-app"));
    node.application_manager()
        .register(&app_ctx("test-app"), app)
        .await
        .unwrap();
    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    node.application_manager().start("test-app").await.unwrap();

    // Shutdown
    node.shutdown(tokio::time::Duration::from_secs(5))
        .await
        .unwrap();

    // Now shutdown is requested
    assert!(node.is_shutdown_requested().await);
}

#[tokio::test]
async fn test_shutdown_with_no_applications() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Initialize services
    node.initialize_services().await.unwrap();

    // Shutdown with no apps should succeed
    let result = node.shutdown(tokio::time::Duration::from_secs(5)).await;
    assert!(result.is_ok());

    // Shutdown flag should be set
    assert!(node.is_shutdown_requested().await);
}

#[tokio::test]
async fn test_application_manager_accessor() {
    let node = NodeBuilder::new("test-node").build().await;

    // Get application manager reference
    let manager = node.application_manager();

    // Verify it's the same manager (returns empty list initially)
    let apps = manager.list_applications().await;
    assert_eq!(apps.len(), 0);
}

#[tokio::test]
async fn test_multiple_start_attempts() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    let app = Box::new(MockTestApplication::new("test-app"));
    node.application_manager()
        .register(&app_ctx("test-app"), app)
        .await
        .unwrap();

    // First start succeeds
    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    node.application_manager().start("test-app").await.unwrap();
    assert_eq!(
        node.application_manager().get_state("test-app").await,
        Some(ApplicationState::ApplicationStateRunning)
    );

    // Second start should fail (not in Created state)
    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    let result = node.application_manager().start("test-app").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("state"));
}

#[tokio::test]
async fn test_stop_already_stopped_application() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    let app = Box::new(MockTestApplication::new("test-app"));
    node.application_manager()
        .register(&app_ctx("test-app"), app)
        .await
        .unwrap();
    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    node.application_manager().start("test-app").await.unwrap();

    // First stop succeeds
    node.application_manager()
        .stop("test-app", tokio::time::Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(
        node.application_manager().get_state("test-app").await,
        Some(ApplicationState::ApplicationStateStopped)
    );

    // Second stop should succeed (already stopped)
    let result = node
        .application_manager()
        .stop("test-app", tokio::time::Duration::from_secs(5))
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_shutdown_stops_all_applications() {
    let node = Arc::new(
        NodeBuilder::new("test-node")
            .with_in_memory_backends()
            .build()
            .await,
    );

    // Track which apps were stopped
    let stopped_apps = Arc::new(RwLock::new(Vec::new()));

    // Create apps that record when they stop
    struct StopTrackingApp {
        name: String,
        stopped_apps: Arc<RwLock<Vec<String>>>,
    }

    #[async_trait]
    impl Application for StopTrackingApp {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            "0.1.0"
        }

        async fn start(&mut self, _node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
            Ok(())
        }

        async fn stop(&mut self) -> Result<(), ApplicationError> {
            self.stopped_apps.write().await.push(self.name.clone());
            Ok(())
        }

        async fn health_check(&self) -> HealthStatus {
            HealthStatus::HealthStatusHealthy
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    // Register and start multiple apps
    for i in 1..=3 {
        let app = Box::new(StopTrackingApp {
            name: format!("app{}", i),
            stopped_apps: stopped_apps.clone(),
        }) as Box<dyn Application>;
        node.application_manager()
            .register(&app_ctx(&format!("app{}", i)), app)
            .await
            .unwrap();
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager()
            .start(&format!("app{}", i))
            .await
            .unwrap();
    }

    // Shutdown
    node.shutdown(tokio::time::Duration::from_secs(10))
        .await
        .unwrap();

    // Verify all apps were stopped (order not guaranteed due to HashMap)
    let stopped = stopped_apps.read().await;
    assert_eq!(stopped.len(), 3);
    assert!(stopped.contains(&"app1".to_string()));
    assert!(stopped.contains(&"app2".to_string()));
    assert!(stopped.contains(&"app3".to_string()));
}

#[tokio::test]
async fn test_shutdown_stops_manual_applications_with_runtime_only_release_spec() {
    let node = Arc::new(
        NodeBuilder::new("test-node")
            .with_in_memory_backends()
            .build()
            .await,
    );

    let app = Box::new(MockTestApplication::new("test-app"));
    let stop_called = app.stop_called.clone();

    node.application_manager()
        .register(&app_ctx("test-app"), app)
        .await
        .unwrap();
    node.application_manager()
        .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    node.application_manager().start("test-app").await.unwrap();

    node.shutdown(tokio::time::Duration::from_secs(5))
        .await
        .unwrap();

    assert_eq!(
        node.application_manager().get_state("test-app").await,
        Some(ApplicationState::ApplicationStateStopped)
    );
    assert!(
        *stop_called.read().await,
        "shutdown should stop manually registered applications even when the node only carries runtime configuration in ReleaseSpec"
    );
}
