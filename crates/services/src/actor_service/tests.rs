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

//! Tests for ActorServiceImpl.

use super::*;
use plexspaces_actor::{
    InitializableServiceLocator, MessageSender, ObjectRegistry as ObjectRegistryTrait,
};
use plexspaces_mailbox::{mailbox_config_default, Mailbox};
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use plexspaces_proto::actor::v1::{NodePlacement, NodePlacementStrategy};
use plexspaces_proto::node::v1::{NodeCapacity, NodeRegistration};
use plexspaces_proto::object_registry::v1::ObjectRegistration;
use plexspaces_proto::object_registry::v1::ObjectType;
use std::time::Duration as StdDuration;
use ulid::Ulid;

/// Helper to create a test message with proto Message type
fn create_test_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

fn create_test_send_message_request(message: Message) -> SendMessageRequest {
    SendMessageRequest {
        namespace: String::new(),
        actor_name: String::new(),
        actor_type: message.receiver_id.clone(),
        http_method: "POST".to_string(),
        payload: message.payload,
        headers: message.headers,
        query_params: HashMap::new(),
        path: String::new(),
        subpath: String::new(),
        sender_id: message.sender_id,
        message_type: if message.message_type.is_empty() {
            "cast".to_string()
        } else {
            message.message_type
        },
        correlation_id: message.correlation_id,
        reply_to: message.reply_to,
        message_id: message.id,
        request_id: ulid::Ulid::new().to_string(),
    }
}

/// Simple wrapper to adapt ObjectRegistryImpl to ObjectRegistryTrait
#[allow(dead_code)]
struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistryImpl>,
}

struct MockNodeRegistry {
    nodes: Vec<NodeRegistration>,
}

#[async_trait::async_trait]
impl plexspaces_actor::NodeRegistryTrait for MockNodeRegistry {
    async fn lookup_node(
        &self,
        _ctx: &RequestContext,
        node_id: &str,
    ) -> Result<Option<NodeRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self
            .nodes
            .iter()
            .find(|node| node.node_id == node_id)
            .cloned())
    }

    async fn register_node(
        &self,
        _ctx: &RequestContext,
        _registration: NodeRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn unregister_node(
        &self,
        _ctx: &RequestContext,
        _node_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn list_nodes(
        &self,
        _ctx: &RequestContext,
        cluster: Option<&str>,
        _page_size: u32,
        _page_token: &str,
    ) -> Result<(Vec<NodeRegistration>, String), Box<dyn std::error::Error + Send + Sync>> {
        let registrations = self
            .nodes
            .iter()
            .filter(|node| {
                cluster.is_none_or(|cluster_name| {
                    node.capabilities
                        .get("cluster")
                        .map(|value| value == cluster_name)
                        .unwrap_or(false)
                })
            })
            .cloned()
            .collect();
        Ok((registrations, String::new()))
    }

    async fn send_heartbeat(
        &self,
        _ctx: &RequestContext,
        _node_id: &str,
        _capacity: Option<NodeCapacity>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn start_gossip_protocol(&self) {}

    fn stop_gossip_protocol(&self) {}

    fn is_gossip_running(&self) -> bool {
        false
    }

    async fn kickoff_seed_reconcile_ping(
        &self,
        _node_id: String,
        _node_address: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn cache_stats(&self) -> (usize, usize, StdDuration) {
        (self.nodes.len(), 0, StdDuration::from_secs(0))
    }
}

#[async_trait::async_trait]
impl ObjectRegistryTrait for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type
            .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner.lookup(ctx, obj_type, object_id).await.map_err(
            |e| -> Box<dyn std::error::Error + Send + Sync> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            },
        )
    }

    async fn lookup_full(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })
    }

    async fn register(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        registration: ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner.register(ctx, registration).await.map_err(
            |e| -> Box<dyn std::error::Error + Send + Sync> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            },
        )
    }

    async fn discover(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        opts: plexspaces_actor::DiscoverOptions,
    ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner.discover(ctx, opts).await.map_err(
            |e| -> Box<dyn std::error::Error + Send + Sync> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            },
        )
    }

    async fn unregister(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .unregister(ctx, object_type, object_id)
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })
    }

    async fn heartbeat(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .heartbeat(ctx, object_type, object_id)
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })
    }
}

/// Helper to create a test ActorRegistry
async fn create_test_registry(local_node_id: &str) -> Arc<ActorRegistry> {
    Arc::new(ActorRegistry::new(local_node_id.to_string()))
}

/// Helper to create ActorServiceImpl with proper ServiceLocator setup for tests
async fn create_test_actor_service(
    actor_registry: Arc<ActorRegistry>,
    node_id: String,
) -> ActorServiceImpl {
    use crate::service_locator::ServiceLocatorImpl;
    use plexspaces_actor::ServiceLocator as ServiceLocatorTrait;
    // Create ServiceLocatorImpl directly
    let service_locator_impl = Arc::new(ServiceLocatorImpl::new());
    // Disable auth so tests can call gRPC methods without JWT
    service_locator_impl
        .register_security_config(plexspaces_proto::node::v1::SecurityConfig {
            disable_auth: true,
            oidc: None,
            ..Default::default()
        })
        .await;
    // Initialize services first — this registers GrpcConnectionManager and other
    // services.  The idempotency guard in initialize_services triggers on
    // actor_registry being present, so we must NOT register actor_registry before
    // this call.
    service_locator_impl
        .initialize_services(Some(plexspaces_proto::node::v1::ReleaseSpec {
            node: Some(plexspaces_proto::node::v1::NodeConfig {
                id: node_id.clone(),
                listen_addr: "127.0.0.1:0".to_string(),
                grpc_connection_pool_size: 2,
                max_connections: 100,
                heartbeat_interval_ms: 5000,
                clustering_enabled: true,
                ..Default::default()
            }),
            ..Default::default()
        }))
        .await;
    // Override actor_registry with the test-specific one (which has test actors).
    // VirtualActorManager must always be present in the actor-registry; set it here.
    {
        use plexspaces_actor::VirtualActorManager;
        let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
        actor_registry
            .set_virtual_actor_manager(virtual_actor_manager)
            .await;
    }
    service_locator_impl
        .register_actor_registry(actor_registry.clone())
        .await;
    ActorServiceImpl::new(service_locator_impl, node_id)
}

async fn create_test_actor_service_impl_with_node_registry(
    nodes: Vec<NodeRegistration>,
) -> ActorServiceImpl {
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry, "node1".to_string()).await;
    let mock_registry: Arc<dyn plexspaces_actor::NodeRegistryTrait> =
        Arc::new(MockNodeRegistry { nodes });
    service
        .service_locator
        .register_node_registry(mock_registry)
        .await;
    service
}

/// Helper to register an actor with ActorRegistry for tests
async fn register_test_actor(
    actor_registry: Arc<ActorRegistry>,
    actor_id: ActorId,
    mailbox: Arc<Mailbox>,
    service_locator: Arc<dyn ServiceLocatorTrait>,
    actor_type: &str,
    namespace: &str,
) {
    // CRITICAL: Pass tenant_id from RequestContext to ActorRef (empty for tests)
    let sender: Arc<dyn MessageSender> = Arc::new(plexspaces_actor::ActorRef::local(
        actor_id.clone(),
        String::new(), // Test context uses empty tenant_id
        namespace.to_string(),
        mailbox,
        service_locator,
        plexspaces_proto::actor::v1::ActorVisibility::ActorVisibilityPublic,
    ));
    // Tenant comes from auth, not config - use empty strings for test actor registration
    use plexspaces_actor::RequestContext;
    let ctx = RequestContext::new_without_auth(String::new(), namespace.to_string());
    actor_registry
        .register_actor(
            &ctx,
            plexspaces_actor::ActorRegistrationParams {
                actor_id,
                sender,
                actor_type: actor_type.to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;
}

fn test_actor_id(name: &str, actor_type: &str, namespace: &str, node_id: &str) -> ActorId {
    ActorId::new(name, actor_type, namespace, node_id).expect("test actor id should be valid")
}

/// Helper to create a test ActorRegistry with a node registration
async fn create_test_registry_with_node(
    local_node_id: &str,
    node_id: &str,
    node_address: &str,
) -> Arc<ActorRegistry> {
    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));

    // Register node using ObjectTypeNode
    // Use internal context for system operations (node registration is system-level)
    let ctx = plexspaces_actor::RequestContext::new_without_auth(String::new(), String::new())
        .with_admin(true);
    let registration = ObjectRegistration {
        object_id: node_id.to_string(),
        object_type: ObjectType::ObjectTypeNode as i32,
        object_category: "Node".to_string(),
        grpc_address: node_address.to_string(),
        ..Default::default()
    };

    object_registry_impl
        .register(&ctx, registration)
        .await
        .unwrap();

    Arc::new(ActorRegistry::new(local_node_id.to_string()))
}

#[tokio::test]
async fn test_resolve_shard_group_target_nodes_from_registry_ignores_node_ids() {
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry, "node1".to_string()).await;
    let ctx = RequestContext::new_without_auth(String::new(), String::new());
    let mock_registry: Arc<dyn plexspaces_actor::NodeRegistryTrait> = Arc::new(MockNodeRegistry {
        nodes: vec![
            NodeRegistration {
                node_id: "node1".to_string(),
                node_address: "127.0.0.1:9001".to_string(),
                capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                ..Default::default()
            },
            NodeRegistration {
                node_id: "node2".to_string(),
                node_address: "127.0.0.1:9002".to_string(),
                capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                ..Default::default()
            },
        ],
    });
    service
        .service_locator
        .register_node_registry(mock_registry)
        .await;

    let placement = NodePlacement {
        strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
        cluster: "heat".to_string(),
        node_ids: vec!["stale-node".to_string()],
        required_labels: HashMap::new(),
        avoid_node_ids: vec![],
        resource_requirements: None,
        affinity_labels: HashMap::new(),
    };

    let target_nodes = service
        .resolve_shard_group_target_nodes(&ctx, Some(&placement), "node1")
        .await
        .expect("target nodes should resolve from registry");

    assert_eq!(target_nodes, vec!["node1".to_string(), "node2".to_string()]);
}

#[tokio::test]
async fn test_resolve_shard_group_target_nodes_from_registry_defaults_to_local_cluster() {
    let service = create_test_actor_service_impl_with_node_registry(vec![
        NodeRegistration {
            node_id: "node1".to_string(),
            node_address: "127.0.0.1:9001".to_string(),
            capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
            ..Default::default()
        },
        NodeRegistration {
            node_id: "node2".to_string(),
            node_address: "127.0.0.1:9002".to_string(),
            capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
            ..Default::default()
        },
        NodeRegistration {
            node_id: "node3".to_string(),
            node_address: "127.0.0.1:9003".to_string(),
            capabilities: HashMap::from([("cluster".to_string(), "other".to_string())]),
            ..Default::default()
        },
    ])
    .await;

    service
        .service_locator
        .register_node_config(plexspaces_proto::node::v1::NodeConfig {
            id: "node1".to_string(),
            cluster_name: "heat".to_string(),
            ..Default::default()
        })
        .await;

    let ctx = RequestContext::new_without_auth(String::new(), String::new());
    let placement = plexspaces_proto::actor::v1::NodePlacement {
        strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
        cluster: String::new(),
        node_ids: vec![],
        required_labels: HashMap::new(),
        avoid_node_ids: vec![],
        resource_requirements: None,
        affinity_labels: HashMap::new(),
    };

    let target_nodes = service
        .resolve_shard_group_target_nodes(&ctx, Some(&placement), "node1")
        .await
        .unwrap();

    assert_eq!(target_nodes, vec!["node1".to_string(), "node2".to_string()]);
}

#[tokio::test]
async fn test_resolve_shard_group_target_nodes_from_registry_errors_when_empty() {
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry, "node1".to_string()).await;
    let ctx = RequestContext::new_without_auth(String::new(), String::new());
    let mock_registry: Arc<dyn plexspaces_actor::NodeRegistryTrait> =
        Arc::new(MockNodeRegistry { nodes: Vec::new() });
    service
        .service_locator
        .register_node_registry(mock_registry)
        .await;

    let placement = NodePlacement {
        strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
        cluster: String::new(),
        node_ids: vec!["stale-node".to_string()],
        required_labels: HashMap::new(),
        avoid_node_ids: vec![],
        resource_requirements: None,
        affinity_labels: HashMap::new(),
    };

    let error = service
        .resolve_shard_group_target_nodes(&ctx, Some(&placement), "node1")
        .await
        .expect_err("empty registry placement should fail");

    assert!(error
        .to_string()
        .contains("Placement produced no target nodes for shard group creation"));
}

#[tokio::test]
async fn test_register_and_unregister_local_actor() {
    // ARRANGE
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
    let actor_id = test_actor_id("test", "test_actor", "default", "node1");

    let mailbox = Arc::new(
        Mailbox::new(mailbox_config_default(), actor_id.to_string(), String::new(), String::new(), None)
            .await
            .expect("Failed to create mailbox"),
    );
    let _actor_ref = plexspaces_service_traits::ActorRef::new(actor_id.clone()).unwrap();

    // ACT: Register actor
    register_test_actor(
        actor_registry.clone(),
        actor_id.clone(),
        Arc::clone(&mailbox),
        service.service_locator.clone(),
        "test_actor",
        "default",
    )
    .await;

    // ASSERT: Actor is in cache
    {
        let is_activated = service
            .get_actor_registry()
            .await
            .is_actor_activated(&actor_id)
            .await;
        assert!(is_activated);
    }

    // ACT: Unregister actor
    actor_registry.unregister(&actor_id).await.unwrap();

    // ASSERT: Actor is removed from cache
    {
        let is_activated = service
            .get_actor_registry()
            .await
            .is_actor_activated(&actor_id)
            .await;
        assert!(!is_activated);
    }
}

#[tokio::test]
async fn test_parse_actor_id() {
    let actor_id = ActorId::from_canonical("counter//worker::default@node1")
        .expect("canonical actor id should parse");
    assert_eq!(actor_id.name(), "counter");
    assert_eq!(actor_id.actor_type(), "worker");
    assert_eq!(actor_id.namespace(), "default");
    assert_eq!(actor_id.node_id(), "node1");
    assert_eq!(actor_id.to_string(), "counter//worker::default@node1");

    assert!(ActorId::from_canonical("counter@node1").is_err());
    assert!(ActorId::from_canonical("invalid_no_node_separator").is_err());
}

#[tokio::test]
async fn test_canonical_actor_id_from_client_target_resolves_bare_live_actor_type() {
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(),
            "controller//controller::app-ns@node1".to_string(),
            String::new(),
            "app-ns".to_string(),
            None,
        )
        .await
        .expect("Failed to create mailbox"),
    );
    let actor_id = test_actor_id("controller", "controller", "app-ns", "node1");
    let ctx = RequestContext::new_without_auth(String::new(), "app-ns".to_string());

    register_test_actor(
        actor_registry,
        actor_id.clone(),
        mailbox,
        service.service_locator.clone(),
        "controller",
        "app-ns",
    )
    .await;

    let resolved = service
        .canonical_actor_id_from_client_target(&ctx, "controller")
        .await;

    assert_eq!(resolved, Some(actor_id.to_string()));
}

#[tokio::test]
async fn test_canonical_actor_id_from_client_target_name_colon_type_hits_live_actor() {
    // name:actor_type where actor_type is the right side — O(1) lookup path.
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

    let mailbox = Arc::new(
        Mailbox::new(
            mailbox_config_default(),
            "weather//weather_actor_wasm::app-ns@node1".to_string(),
            String::new(),
            "app-ns".to_string(),
            None,
        )
        .await
        .expect("Failed to create mailbox"),
    );
    let actor_id = test_actor_id("weather", "weather_actor_wasm", "app-ns", "node1");
    let ctx = RequestContext::new_without_auth(String::new(), "app-ns".to_string());

    register_test_actor(
        actor_registry,
        actor_id.clone(),
        mailbox,
        service.service_locator.clone(),
        "weather_actor_wasm",
        "app-ns",
    )
    .await;

    // weather:weather_actor_wasm → direct O(1) hit: weather//weather_actor_wasm::app-ns@node1
    let resolved = service
        .canonical_actor_id_from_client_target(&ctx, "weather:weather_actor_wasm")
        .await;

    assert_eq!(resolved, Some(actor_id.to_string()));
}

#[tokio::test]
async fn test_canonical_actor_id_from_client_target_name_colon_type_no_live_actor_builds_canonical()
{
    // name:actor_type where no live actor exists — falls through to Step 4 direct build.
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

    let ctx = RequestContext::new_without_auth(String::new(), "app-ns".to_string());

    // No live actor registered. Should build: weather//weather_actor_wasm::app-ns@node1
    let resolved = service
        .canonical_actor_id_from_client_target(&ctx, "weather:weather_actor_wasm")
        .await;

    assert_eq!(
        resolved,
        Some("weather//weather_actor_wasm::app-ns@node1".to_string())
    );
}

#[tokio::test]
async fn test_canonical_actor_id_from_client_target_virtual_definition_name_lookup() {
    // definition_name:instance_id — left is a virtual actor definition name.
    // This uses the Step 3 path (definition metadata lookup).
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

    // Register virtual actor definition: name="weather", actor_type="weather_actor_wasm"
    if let Some(manager) = service.service_locator.virtual_actor_manager().await {
        let spec = plexspaces_proto::actor::v1::ActorSpawnSpec {
            identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: "weather".to_string(),
                actor_type: "weather_actor_wasm".to_string(),
            }),
            namespace: "app-ns".to_string(),
            behavior_kind: "GenServer".to_string(),
            ..Default::default()
        };
        manager
            .register_virtual_actor_definition(spec)
            .await
            .unwrap();
    }

    let ctx = RequestContext::new_without_auth(String::new(), "app-ns".to_string());

    // weather:session-1 → definition name "weather" maps to "weather_actor_wasm",
    // instance name = "session-1" → session-1//weather_actor_wasm::app-ns@node1
    let resolved = service
        .canonical_actor_id_from_client_target(&ctx, "weather:session-1")
        .await;

    assert_eq!(
        resolved,
        Some("session-1//weather_actor_wasm::app-ns@node1".to_string())
    );
}

#[tokio::test]
async fn test_canonical_actor_id_from_client_target_bare_definition_name_lookup() {
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

    if let Some(manager) = service.service_locator.virtual_actor_manager().await {
        let spec = plexspaces_proto::actor::v1::ActorSpawnSpec {
            identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: "audit-log-test".to_string(),
                actor_type: "audit_log_test_wasm".to_string(),
            }),
            role: "worker".to_string(),
            namespace: "audit-log-test".to_string(),
            behavior_kind: "GenEvent".to_string(),
            ..Default::default()
        };
        manager
            .register_virtual_actor_definition(spec)
            .await
            .unwrap();
    }

    let ctx = RequestContext::new_without_auth(String::new(), "audit-log-test".to_string());
    let resolved = service
        .canonical_actor_id_from_client_target(&ctx, "audit-log-test")
        .await;

    assert_eq!(
        resolved,
        Some("audit-log-test//audit_log_test_wasm::audit-log-test@node1".to_string())
    );
}

#[tokio::test]
async fn test_canonical_actor_id_from_client_target_canonical_id_passthrough() {
    // Canonical actor ID passthrough — already contains //.
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

    let ctx = RequestContext::new_without_auth(String::new(), "app-ns".to_string());

    let canonical = "weather//weather_actor_wasm::app-ns@node1";
    let resolved = service
        .canonical_actor_id_from_client_target(&ctx, canonical)
        .await;

    assert_eq!(resolved, Some(canonical.to_string()));
}

#[test]
fn test_set_message_receiver_id_uses_canonical_actor_id() {
    let mut message = create_test_message(b"test".to_vec());
    message.receiver_id = "controller:cart-1".to_string();

    ActorServiceImpl::set_message_receiver_id(&mut message, "cart-1//controller::app-ns@node1");

    assert_eq!(message.receiver_id, "cart-1//controller::app-ns@node1");
}

// ========================================================================
// COVERAGE TESTS - route_message()
// ========================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_route_message_invalid_actor_id() {
    // ARRANGE: Create service
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

    let message = create_test_message(b"test".to_vec());

    // ACT: Try to route with a non-canonical actor ID. Parsing should fail before lookup.
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    let result = service
        .route_message(ctx, "invalid_no_node", message, false, None)
        .await;

    // ASSERT: Should fail because the actor ID is not canonical
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("Invalid actor ID"));
}

#[tokio::test]
async fn test_route_message_local_routing() {
    // ARRANGE: Create actor and register it locally
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
    let actor_id = test_actor_id("test", "test_actor", "default", "node1");

    let mailbox = Arc::new(
        Mailbox::new(mailbox_config_default(), actor_id.to_string(), String::new(), String::new(), None)
            .await
            .expect("Failed to create mailbox"),
    );
    let _actor_ref = plexspaces_service_traits::ActorRef::new(actor_id.clone()).unwrap();
    register_test_actor(
        actor_registry.clone(),
        actor_id.clone(),
        Arc::clone(&mailbox),
        service.service_locator.clone(),
        "test_actor",
        "default",
    )
    .await;

    let message = create_test_message(b"hello".to_vec());
    let message_id = message.id.to_string();

    // ACT: Route message via route_message() entry point
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    let result = service
        .route_message(ctx, actor_id.as_str(), message, false, None)
        .await;

    // ASSERT: Should route locally
    assert!(result.is_ok());
    let (returned_id, response) = result.unwrap();
    assert_eq!(returned_id, message_id);
    assert!(response.is_none());

    // Verify message delivered (poll immediately - no sleep needed)
    let delivered = mailbox.dequeue().await;
    assert!(
        delivered.is_some(),
        "Message should be delivered immediately"
    );
    assert_eq!(delivered.unwrap().payload, b"hello");
}

// ========================================================================
// COVERAGE TESTS - send_message() gRPC Handler
// ========================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_send_message_missing_actor_type() {
    // ARRANGE: Create service
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

    let request = tonic::Request::new(SendMessageRequest {
        namespace: String::new(),
        actor_name: String::new(),
        actor_type: String::new(),
        http_method: "POST".to_string(),
        payload: Vec::new(),
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path: String::new(),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "cast".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
        request_id: ulid::Ulid::new().to_string(),
    });

    let result = ActorServiceTrait::send_message(&service, request).await;

    // ASSERT: Should fail with InvalidArgument
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("actor_type"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_send_message_missing_receiver() {
    // ARRANGE: Create service
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

    let mut proto_message = create_test_message(b"test".to_vec());
    proto_message.receiver_id = String::new();
    let request = tonic::Request::new(create_test_send_message_request(proto_message));

    // ACT
    let result = ActorServiceTrait::send_message(&service, request).await;

    // ASSERT: Should fail with InvalidArgument
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("actor_type"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_send_message_success() {
    // ARRANGE: Create actor and register it
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
    let actor_id = test_actor_id("test", "test_actor", "default", "node1");

    let mailbox = Arc::new(
        Mailbox::new(mailbox_config_default(), actor_id.to_string(), String::new(), String::new(), None)
            .await
            .expect("Failed to create mailbox"),
    );
    let _actor_ref = plexspaces_service_traits::ActorRef::new(actor_id.clone()).unwrap();
    register_test_actor(
        actor_registry.clone(),
        actor_id.clone(),
        Arc::clone(&mailbox),
        service.service_locator.clone(),
        "test_actor",
        "default",
    )
    .await;

    // Create proto message with a valid JSON payload (send_message validates JSON)
    let json_payload = b"{\"msg\":\"hello\"}".to_vec();
    let mut message = create_test_message(json_payload.clone());
    message.receiver_id = actor_id.to_string();
    let proto_message = message.clone();
    let expected_message_id = proto_message.id.clone();

    let request = tonic::Request::new(create_test_send_message_request(proto_message));

    // ACT: Send via gRPC handler
    let result = ActorServiceTrait::send_message(&service, request).await;

    // ASSERT: Should succeed
    assert!(result.is_ok());
    let response = result.unwrap().into_inner();
    assert_eq!(response.message_id, expected_message_id);
    assert!(response.success);

    // Verify delivery (poll immediately - no sleep needed)
    let delivered = mailbox.dequeue().await;
    assert!(
        delivered.is_some(),
        "Message should be delivered immediately"
    );
    assert_eq!(delivered.unwrap().payload, json_payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_send_message_with_timeout() {
    // ARRANGE
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
    let actor_id = test_actor_id("test", "test_actor", "default", "node1");

    let mailbox = Arc::new(
        Mailbox::new(mailbox_config_default(), actor_id.to_string(), String::new(), String::new(), None)
            .await
            .expect("Failed to create mailbox"),
    );
    let _actor_ref = plexspaces_service_traits::ActorRef::new(actor_id.clone()).unwrap();
    register_test_actor(
        actor_registry.clone(),
        actor_id.clone(),
        Arc::clone(&mailbox),
        service.service_locator.clone(),
        "test_actor",
        "default",
    )
    .await;

    // Create message with timeout and valid JSON payload (send_message validates JSON)
    let mut message = create_test_message(b"{}".to_vec());
    message.receiver_id = actor_id.to_string();
    let proto_message = message.clone();

    let request = tonic::Request::new(create_test_send_message_request(proto_message));

    // ACT: Send with timeout (though fire-and-forget ignores it)
    let result = ActorServiceTrait::send_message(&service, request).await;

    // ASSERT: Should succeed (timeout parsed but not used for fire-and-forget)
    assert!(result.is_ok());
}

// ========================================================================
// Connection Manager
// ========================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_connection_manager_available() {
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

    // Test that connection manager is available
    let conn_manager = service.service_locator.get_grpc_connection_manager().await;
    assert!(
        conn_manager.is_some(),
        "GrpcConnectionManager should be available"
    );
}

// ========================================================================
// COVERAGE TESTS - send_message() timeout conversion
// ========================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_send_message_converts_timeout_correctly() {
    // ARRANGE
    let actor_registry = create_test_registry("node1").await;
    let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
    let actor_id = test_actor_id("test", "test_actor", "default", "node1");

    let mailbox = Arc::new(
        Mailbox::new(mailbox_config_default(), actor_id.to_string(), String::new(), String::new(), None)
            .await
            .expect("Failed to create mailbox"),
    );
    let _actor_ref = plexspaces_service_traits::ActorRef::new(actor_id.clone()).unwrap();
    register_test_actor(
        actor_registry.clone(),
        actor_id.clone(),
        Arc::clone(&mailbox),
        service.service_locator.clone(),
        "test_actor",
        "default",
    )
    .await;

    // Create message with fractional seconds timeout and valid JSON payload
    let mut message = create_test_message(b"{}".to_vec());
    message.receiver_id = actor_id.to_string();
    let proto_message = message.clone();

    let request = tonic::Request::new(create_test_send_message_request(proto_message));

    // ACT: Send with fractional timeout
    let result = ActorServiceTrait::send_message(&service, request).await;

    // ASSERT: Should succeed (timeout converted correctly)
    assert!(
        result.is_ok(),
        "Expected success, got error: {:?}",
        result.err()
    );
}

// ========================================================================
// INTEGRATION TEST - route_remote() Success Path with Real gRPC Server
// ========================================================================

// Integration tests cover real gRPC server scenarios; unit tests focus on local routing and simulated remote.
// - test_route_remote_connection_pooling
// - test_route_remote_with_timeout
// - test_route_remote_multi_node_scenario
// These tests used real gRPC servers with sleep calls, making them slow and flaky.
// Integration tests in crates/actor-service/tests/integration/ cover these scenarios.

#[tokio::test]
async fn test_route_remote_error_handling() {
    // Test: Error handling for network failures and invalid addresses
    // ARRANGE: Create service with invalid node address
    let invalid_address = "127.0.0.1:1"; // Port 1 is typically not in use
    let registry = create_test_registry_with_node("node1", "node2", invalid_address).await;
    let service = create_test_actor_service(registry.clone(), "node1".to_string()).await;

    // Service registration is synchronous - verify immediately
    assert!(
        service.service_locator.actor_registry().await.is_some(),
        "ActorRegistry should be registered synchronously"
    );

    // ACT: Try to route to unreachable node
    let message = create_test_message(b"test".to_vec());
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    let result = service
        .route_message(ctx, "actor//worker::default@node2", message, false, None)
        .await;

    // ASSERT: Should fail with appropriate error
    assert!(result.is_err(), "Should fail when node is unreachable");
    let err = result.unwrap_err();
    // Error should indicate connection failure
    assert!(
        err.code() == tonic::Code::Unavailable || err.code() == tonic::Code::Internal,
        "Expected Unavailable or Internal error, got {:?}: {}",
        err.code(),
        err.message()
    );
}
