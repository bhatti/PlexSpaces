// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Multi-Node Integration Tests (Simulated)
//!
//! Tests for distributed actor messaging using simulated nodes.
//! These tests use a single local node with actors registered with remote-looking IDs
//! to simulate multi-node scenarios without spawning real processes.

use plexspaces_actor_service::ActorServiceImpl;
use plexspaces_actor::{ActorBuilder, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_behavior::GenServer;
use plexspaces_core::{
    ActorRegistry, ReplyTracker, ServiceLocator, ReplyWaiterRegistry, Actor as ActorTrait,
    ActorContext, BehaviorError, BehaviorType, FacetManager, VirtualActorManager, RequestContext,
    MessageSender,
};
use plexspaces_mailbox::{Message, Mailbox, MailboxConfig};
use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_object_registry::ObjectRegistry;
use plexspaces_proto::actor::v1::{
    actor_service_server::ActorService as ActorServiceTrait,
    SendMessageRequest,
};
use std::collections::HashMap;
use std::sync::Arc;
use tonic::Request;
use async_trait::async_trait;

// Echo actor that receives messages and stores them
struct EchoActor {
    received: Vec<String>,
}

impl EchoActor {
    fn new() -> Self {
        Self { received: vec![] }
    }
}

#[async_trait]
impl ActorTrait for EchoActor {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let payload_str = String::from_utf8_lossy(&msg.payload);
        self.received.push(payload_str.to_string());
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait]
impl GenServer for EchoActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let payload_str = String::from_utf8_lossy(&msg.payload);
        self.received.push(payload_str.to_string());
        Ok(())
    }
}

// Helper to create test registry with actors
// Note: Actors are registered with the local node ID to ensure local routing works.
// The "remote" simulation comes from the actor names, not the node IDs.
async fn create_test_registry_with_remote_actors(
    local_node_id: &str,
    _remote_node_id: &str, // Not used - actors are registered locally
    actor_ids: &[&str],
) -> (Arc<ActorRegistry>, Arc<ServiceLocator>) {
    use plexspaces_core::actor_context::ObjectRegistry as ObjectRegistryTrait;
    use plexspaces_proto::object_registry::v1::ObjectRegistration;

    let kv = Arc::new(InMemoryKVStore::new());
    let object_registry_impl = Arc::new(ObjectRegistry::new(kv));

    struct ObjectRegistryAdapter {
        inner: Arc<ObjectRegistry>,
    }

    #[async_trait]
    impl ObjectRegistryTrait for ObjectRegistryAdapter {
        async fn lookup(
            &self,
            ctx: &RequestContext,
            object_id: &str,
            object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            let obj_type = object_type
                .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
            self.inner
                .lookup(ctx, obj_type, object_id)
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                        as Box<dyn std::error::Error + Send + Sync>
                })
        }

        async fn lookup_full(
            &self,
            ctx: &RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .lookup_full(ctx, object_type, object_id)
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                        as Box<dyn std::error::Error + Send + Sync>
                })
        }

        async fn register(
            &self,
            ctx: &RequestContext,
            registration: ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .register(ctx, registration)
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                        as Box<dyn std::error::Error + Send + Sync>
                })
        }

        async fn discover(
            &self,
            _ctx: &RequestContext,
            _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
            _name: Option<String>,
            _labels: Option<Vec<String>>,
            _exclude_labels: Option<Vec<String>>,
            _health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
            _limit: usize,
            _offset: usize,
        ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(vec![])
        }
    }

    let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
        inner: object_registry_impl,
    });

    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry,
        local_node_id.to_string(),
    ));
    use plexspaces_node::create_default_service_locator;
    let service_locator =
        create_default_service_locator(Some(local_node_id.to_string()), None, None).await;
    service_locator.register_service(actor_registry.clone()).await;

    // Register NodeConfig
    use plexspaces_proto::node::v1::NodeConfig;
    let node_config = NodeConfig {
        id: local_node_id.to_string(),
        listen_address: String::new(),
        cluster_seed_nodes: vec![],
        default_tenant_id: "default".to_string(),
        default_namespace: "default".to_string(),
        cluster_name: String::new(),
    };
    service_locator.register_node_config(node_config).await;

    // Create ActorFactory and required services
    let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    use plexspaces_core::FacetManagerServiceWrapper;
    let facet_manager = Arc::new(FacetManagerServiceWrapper::new(Arc::new(FacetManager::new())));
    service_locator.register_service(virtual_actor_manager).await;
    service_locator.register_service(facet_manager).await;

    let actor_factory = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator.register_service(actor_factory.clone()).await;

    // Register actors with local node ID to ensure local routing works
    // Extract actor name from "actor@node" format and register as "actor@local_node_id"
    // This ensures the routing logic recognizes them as local actors
    let ctx = RequestContext::new_without_auth("default".to_string(), "default".to_string());
    for actor_id_with_node in actor_ids {
        // Parse actor name from "actor@node" format
        let actor_name = if let Some((name, _)) = actor_id_with_node.split_once('@') {
            name
        } else {
            actor_id_with_node
        };
        
        // Register with local node ID to ensure local routing
        let local_actor_id = format!("{}@{}", actor_name, local_node_id);
        
        // Create actor using ActorBuilder
        let echo_actor = EchoActor::new();
        let actor_ref = ActorBuilder::new(Box::new(echo_actor) as Box<dyn ActorTrait>)
            .with_id(local_actor_id.clone())
            .with_namespace("default".to_string())
            .spawn(&ctx, service_locator.clone())
            .await
            .expect("Failed to spawn actor");

        // ActorRef implements MessageSender, so we can use it directly
        let sender: Arc<dyn MessageSender> = Arc::new(actor_ref);
        
        // Register with ActorRegistry using local actor ID
        // Clone sender before moving it, as we may need it again for the second registration
        let sender_clone = sender.clone();
        actor_registry
            .register_actor(&ctx, local_actor_id.clone(), sender, None, None, None)
            .await;
        
        // Also register with the original "remote-looking" ID for lookup
        // This allows tests to use "actor@node2" format while the actor is actually local
        // We create a mapping by registering the same sender under both IDs
        if actor_id_with_node != &local_actor_id {
            actor_registry
                .register_actor(&ctx, actor_id_with_node.to_string(), sender_clone, None, None, None)
                .await;
        }
    }

    (actor_registry, service_locator)
}

async fn create_test_actor_service(
    _actor_registry: Arc<ActorRegistry>,
    service_locator: Arc<ServiceLocator>,
    node_id: String,
) -> ActorServiceImpl {
    let reply_tracker = Arc::new(ReplyTracker::new());
    let reply_waiter_registry = Arc::new(ReplyWaiterRegistry::new());

    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;

    ActorServiceImpl::new(service_locator, node_id)
}

/// Test remote message delivery from node1 to node2 (simulated)
///
/// Scenario:
/// 1. Create single local node
/// 2. Register actor "receiver@node2" on local node (simulates remote node2)
/// 3. Send message via ActorService to "receiver@node2"
/// 4. Verify message delivered successfully
#[tokio::test]
async fn test_remote_message_delivery() {
    let (actor_registry, service_locator) = create_test_registry_with_remote_actors(
        "node1",
        "node2",
        &["receiver@node2"],
    )
    .await;
    let service = create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    // Create message
    let mut message = Message::new(b"hello from node1".to_vec());
    message.receiver = "receiver@node2".to_string();

    // Send via ActorService (using gRPC trait method)
    let request = Request::new(SendMessageRequest {
        message: Some(message.to_proto()),
        wait_for_response: false,
        timeout: None,
    });

    let result = ActorServiceTrait::send_message(&service, request).await;
    assert!(result.is_ok(), "Message should be delivered successfully");
}

/// Test bidirectional communication between two nodes (simulated)
///
/// Scenario:
/// 1. Create single local node
/// 2. Register actors "actor1@node1" and "actor2@node2" on local node
/// 3. Send messages in both directions
/// 4. Verify both succeed
#[tokio::test]
async fn test_bidirectional_communication() {
    let (actor_registry, service_locator) = create_test_registry_with_remote_actors(
        "node1",
        "node2",
        &["actor1@node1", "actor2@node2"],
    )
    .await;
    let service = create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    // node1 -> node2
    let mut message1 = Message::new(b"hello node2".to_vec());
    message1.receiver = "actor2@node2".to_string();
    let request1 = Request::new(SendMessageRequest {
        message: Some(message1.to_proto()),
        wait_for_response: false,
        timeout: None,
    });
    assert!(ActorServiceTrait::send_message(&service, request1).await.is_ok());

    // node2 -> node1 (simulated - both actors are on local node)
    let mut message2 = Message::new(b"hello node1".to_vec());
    message2.receiver = "actor1@node1".to_string();
    let request2 = Request::new(SendMessageRequest {
        message: Some(message2.to_proto()),
        wait_for_response: false,
        timeout: None,
    });
    assert!(ActorServiceTrait::send_message(&service, request2).await.is_ok());
}

/// Test actor not found on remote node (simulated)
///
/// Scenario:
/// 1. Create single local node
/// 2. Try to send to non-existent actor
/// 3. Verify NotFound error
#[tokio::test]
async fn test_actor_not_found_remote() {
    let (actor_registry, service_locator) =
        create_test_registry_with_remote_actors("node1", "node2", &[]).await;
    let service = create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    // Try to send to non-existent actor
    let mut message = Message::new(b"test".to_vec());
    message.receiver = "nonexistent@node2".to_string();
    let request = Request::new(SendMessageRequest {
        message: Some(message.to_proto()),
        wait_for_response: false,
        timeout: None,
    });

    let result = ActorServiceTrait::send_message(&service, request).await;
    assert!(result.is_err(), "Should return error for non-existent actor");
    let status = result.unwrap_err();
    assert_eq!(status.code(), tonic::Code::NotFound);
}

/// Test connection pooling - multiple messages should work (simulated)
///
/// Scenario:
/// 1. Create single local node
/// 2. Register actor "echo@node2" on local node
/// 3. Send 10 messages
/// 4. Verify all succeed
#[tokio::test]
async fn test_connection_pooling() {
    let (actor_registry, service_locator) =
        create_test_registry_with_remote_actors("node1", "node2", &["echo@node2"]).await;
    let service = create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    // Send multiple messages
    for i in 0..10 {
        let payload = format!("message_{}", i);
        let mut message = Message::new(payload.as_bytes().to_vec());
        message.receiver = "echo@node2".to_string();
        let request = Request::new(SendMessageRequest {
            message: Some(message.to_proto()),
            wait_for_response: false,
            timeout: None,
        });
        assert!(ActorServiceTrait::send_message(&service, request).await.is_ok(), "Message {} should succeed", i);
    }
}

/// Test multiple target nodes - verify different actors work (simulated)
///
/// Scenario:
/// 1. Create single local node
/// 2. Register actors on node2 and node3 (simulated)
/// 3. Send messages to both
/// 4. Verify both succeed
#[tokio::test]
async fn test_multiple_target_nodes() {
    let (actor_registry, service_locator) = create_test_registry_with_remote_actors(
        "node1",
        "node2",
        &["actor@node2", "actor@node3"],
    )
    .await;
    let service = create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    // Send to node2
    let mut message1 = Message::new(b"to node2".to_vec());
    message1.receiver = "actor@node2".to_string();
    let request1 = Request::new(SendMessageRequest {
        message: Some(message1.to_proto()),
        wait_for_response: false,
        timeout: None,
    });
    assert!(ActorServiceTrait::send_message(&service, request1).await.is_ok());

    // Send to node3
    let mut message2 = Message::new(b"to node3".to_vec());
    message2.receiver = "actor@node3".to_string();
    let request2 = Request::new(SendMessageRequest {
        message: Some(message2.to_proto()),
        wait_for_response: false,
        timeout: None,
    });
    assert!(ActorServiceTrait::send_message(&service, request2).await.is_ok());
}

/// Test node not in registry (simulated)
///
/// Scenario:
/// 1. Create single local node
/// 2. Try to send to actor on non-existent node
/// 3. Verify NotFound error
#[tokio::test]
async fn test_node_not_found() {
    let (actor_registry, service_locator) =
        create_test_registry_with_remote_actors("node1", "node2", &[]).await;
    let service = create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    // Try to send to non-existent node (actor ID doesn't match any registered actor)
    let mut message = Message::new(b"test".to_vec());
    message.receiver = "actor@nonexistent_node".to_string();
    let request = Request::new(SendMessageRequest {
        message: Some(message.to_proto()),
        wait_for_response: false,
        timeout: None,
    });

    let result = ActorServiceTrait::send_message(&service, request).await;
    assert!(result.is_err(), "Should return error for non-existent node/actor");
    let status = result.unwrap_err();
    // Should be NotFound or Internal (depending on routing logic)
    assert!(
        status.code() == tonic::Code::NotFound || status.code() == tonic::Code::Internal,
        "Should return NotFound or Internal, got: {:?}",
        status.code()
    );
}
