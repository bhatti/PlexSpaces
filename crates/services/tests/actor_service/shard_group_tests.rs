// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Unit and Integration Tests for ShardGroup RPCs (data-parallel sharding)
//!
//! Tests ShardGroup functionality via ActorService:
//! - CreateShardGroup: Create group with N shards
//! - DeleteShardGroup: Delete group and all shards
//! - GetShardGroup: Retrieve group metadata
//! - ListShardGroups: List groups with filtering
//! - SendToShard: Route message to specific shard by partition key
//! - ScatterGather: Query all shards in parallel, aggregate results
//!
//! TDD: Tests written first, will fail until implementation is added.

use async_trait::async_trait;
use plexspaces_behavior::GenServer;
use plexspaces_core::Message;
use plexspaces_core::{
    actor_context::ObjectRegistry as ObjectRegistryTrait, behavior_factory::BehaviorFactory,
    Actor as ActorTrait, ActorContext, ActorRegistry, BehaviorError, BehaviorType,
    NodeRegistryTrait, RequestContext, ServiceLocator,
};
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use plexspaces_proto::actor::v1::{
    actor_service_server::ActorService as ActorServiceTrait, CreateShardGroupRequest,
    DataParallelConfig, DeleteShardGroupRequest, GetShardGroupRequest, ListShardGroupsRequest,
    MapShardGroupRequest, PartitionStrategy, RebalancePolicy, ScatterGatherRequest,
    SendToShardRequest, ShardGroupAggregationStrategy, ShardGroupState, SpawnActorRequest,
};
use plexspaces_proto::common::v1::{Empty, Message as ProtoMessage};
use plexspaces_proto::node::v1::{NodeCapacity, NodeRegistration};
use plexspaces_services::actor_service::{ActorServiceImpl, ActorServiceWrapper};
use std::collections::HashMap;
use std::sync::Arc;
use tonic::Request;
use ulid::Ulid;

/// Build CreateShardGroupRequest with unified DataParallelConfig.
/// Labels go in config.placement.required_labels for scheduler node matching.
fn new_create_shard_group_request(
    group_id: &str,
    actor_type: &str,
    shard_count: u32,
    labels: HashMap<String, String>,
) -> CreateShardGroupRequest {
    use plexspaces_proto::actor::v1::{NodePlacement, NodePlacementStrategy};
    CreateShardGroupRequest {
        config: Some(DataParallelConfig {
            group_id: group_id.to_string(),
            shard_count,
            partition_strategy: PartitionStrategy::PartitionStrategyHash as i32,
            rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
            placement: if labels.is_empty() {
                None
            } else {
                Some(NodePlacement {
                    strategy: NodePlacementStrategy::NodePlacementStrategyUnspecified as i32,
                    cluster: String::new(),
                    node_ids: vec![],
                    required_labels: labels,
                    avoid_node_ids: vec![],
                    resource_requirements: None,
                    affinity_labels: HashMap::new(),
                })
            },
        }),
        actor_type: actor_type.to_string(),
        shard_config: None,
        initial_state: Vec::new(),
        metadata: HashMap::new(),
    }
}

/// Build CreateShardGroupRequest with explicit node_ids for multi-node placement.
/// Used by integration tests that spawn shards across multiple nodes.
fn new_create_shard_group_request_with_node_ids(
    group_id: &str,
    actor_type: &str,
    shard_count: u32,
    node_ids: Vec<String>,
) -> CreateShardGroupRequest {
    use plexspaces_proto::actor::v1::{NodePlacement, NodePlacementStrategy};
    CreateShardGroupRequest {
        config: Some(DataParallelConfig {
            group_id: group_id.to_string(),
            shard_count,
            partition_strategy: PartitionStrategy::PartitionStrategyHash as i32,
            rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
            placement: Some(NodePlacement {
                strategy: NodePlacementStrategy::NodePlacementStrategyNodeIds as i32,
                cluster: String::new(),
                node_ids,
                required_labels: HashMap::new(),
                avoid_node_ids: vec![],
                resource_requirements: None,
                affinity_labels: HashMap::new(),
            }),
        }),
        actor_type: actor_type.to_string(),
        shard_config: None,
        initial_state: Vec::new(),
        metadata: HashMap::new(),
    }
}

/// Build CreateShardGroupRequest with registry-based placement.
/// Strategy must ignore `node_ids` and use NodeRegistry membership instead.
fn new_create_shard_group_request_from_registry(
    group_id: &str,
    actor_type: &str,
    shard_count: u32,
    cluster: &str,
    node_ids: Vec<String>,
) -> CreateShardGroupRequest {
    use plexspaces_proto::actor::v1::{NodePlacement, NodePlacementStrategy};
    CreateShardGroupRequest {
        config: Some(DataParallelConfig {
            group_id: group_id.to_string(),
            shard_count,
            partition_strategy: PartitionStrategy::PartitionStrategyHash as i32,
            rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
            placement: Some(NodePlacement {
                strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
                cluster: cluster.to_string(),
                node_ids,
                required_labels: HashMap::new(),
                avoid_node_ids: vec![],
                resource_requirements: None,
                affinity_labels: HashMap::new(),
            }),
        }),
        actor_type: actor_type.to_string(),
        shard_config: None,
        initial_state: Vec::new(),
        metadata: HashMap::new(),
    }
}

// Helper to create test ProtoMessage
fn create_test_proto_message(payload: Vec<u8>) -> ProtoMessage {
    ProtoMessage {
        id: Ulid::new().to_string(),
        sender_id: "test-client".to_string(),
        receiver_id: String::new(),
        channel: String::new(),
        message_type: String::new(),
        payload,
        headers: HashMap::new(),
        timestamp: None,
        priority: 0,
        ttl: None,
        delivery_count: 0,
        idempotency_key: String::new(),
        correlation_id: String::new(),
        reply_to: String::new(),
        partition_key: String::new(),
        uri_path: String::new(),
        uri_method: String::new(),
    }
}

// Simple counter actor for testing
struct CounterActor {
    count: i64,
}

impl CounterActor {
    fn new() -> Self {
        Self { count: 0 }
    }
}

#[async_trait]
impl ActorTrait for CounterActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait]
impl GenServer for CounterActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        self.count += 1;
        Ok(())
    }
}

// Helper adapter for ObjectRegistry
struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistryImpl>,
}

struct MockNodeRegistry {
    nodes: Vec<NodeRegistration>,
}

#[async_trait::async_trait]
impl NodeRegistryTrait for MockNodeRegistry {
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

    async fn cache_stats(&self) -> (usize, usize, std::time::Duration) {
        (self.nodes.len(), 0, std::time::Duration::from_secs(0))
    }
}

#[async_trait::async_trait]
impl ObjectRegistryTrait for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<
        Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let obj_type = object_type
            .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(ctx, obj_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<
        Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner.register(ctx, registration).await.map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )) as Box<dyn std::error::Error + Send + Sync>
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
    ) -> Result<
        Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Ok(vec![])
    }

    async fn unregister(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .unregister(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn heartbeat(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .heartbeat(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }
}

/// Helper to create test ActorService with registry and auth disabled
async fn create_test_actor_service(
    node_id: &str,
) -> (
    Arc<ActorServiceImpl>,
    Arc<ActorRegistry>,
    Arc<plexspaces_services::ServiceLocatorImpl>,
) {
    use plexspaces_core::actor_context::ObjectRegistry as ObjectRegistryTrait;
    use plexspaces_node::create_default_service_locator;

    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
    let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
        inner: object_registry_impl,
    });
    let actor_registry = Arc::new(ActorRegistry::new(object_registry, node_id.to_string()));

    // Use create_default_service_locator which doesn't call blocking code
    let service_locator =
        create_default_service_locator(Some(node_id.to_string()), None).await;
    service_locator
        .register_service(actor_registry.clone())
        .await;

    // Register ActorFactory (required for spawn_actor to work)
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    use plexspaces_core::{FacetManager, FacetManagerServiceWrapper, VirtualActorManager};
    let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    let facet_manager = Arc::new(FacetManagerServiceWrapper::new(Arc::new(
        FacetManager::new(),
    )));
    service_locator
        .register_service(virtual_actor_manager)
        .await;
    service_locator.register_service(facet_manager).await;
    let actor_factory = ActorFactoryImpl::new_arc(
        service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>
    )
    .await;
    service_locator
        .register_service_by_name(
            plexspaces_core::service_names::ACTOR_FACTORY_IMPL,
            actor_factory.clone(),
        )
        .await;
    let factory_trait: Arc<dyn plexspaces_actor::ActorFactory> = actor_factory.clone();
    service_locator.register_actor_factory(factory_trait).await;

    // Register BehaviorRegistry and behavior for "counter" actor type
    // Note: BehaviorRegistry needs to be registered so ActorFactory can create actors
    use plexspaces_core::behavior_factory::BehaviorRegistry;
    let behavior_registry = BehaviorRegistry::new();
    behavior_registry
        .register_simple("counter", || {
            Box::pin(
                async move { Ok(Box::new(CounterActor::new()) as Box<dyn plexspaces_core::Actor>) },
            )
        })
        .await;
    service_locator
        .register_behavior_registry(Arc::new(behavior_registry))
        .await;

    // Disable auth for tests
    let config = plexspaces_proto::node::v1::SecurityConfig {
        disable_auth: true,
        ..Default::default()
    };
    service_locator.register_security_config(config).await;

    // Cast to ServiceLocatorImpl for return type
    let service_locator_impl =
        service_locator.clone() as Arc<plexspaces_services::ServiceLocatorImpl>;
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        node_id.to_string(),
    ));
    (actor_service, actor_registry, service_locator_impl)
}

// ========================================================================
// CreateShardGroup Tests
// ========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_create_shard_group_success() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(new_create_shard_group_request(
        "test-group",
        "counter",
        4,
        HashMap::new(),
    ));

    let result = service.create_shard_group(req).await;
    assert!(result.is_ok(), "CreateShardGroup should succeed");
    let response = result.unwrap().into_inner();
    let group = response.group.as_ref().expect("group should be present");
    let cfg = group.config.as_ref().expect("config required");
    assert_eq!(cfg.group_id, "test-group");
    assert_eq!(cfg.shard_count, 4);
    assert_eq!(group.shard_actor_ids.len(), 4);
    assert_eq!(group.state, ShardGroupState::ShardGroupStateActive as i32);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_create_shard_group_with_labels() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let mut labels = HashMap::new();
    labels.insert("zone".to_string(), "us-west-1".to_string());
    labels.insert("tier".to_string(), "compute".to_string());

    let req = Request::new(new_create_shard_group_request(
        "labeled-group",
        "counter",
        2,
        labels.clone(),
    ));

    let result = service.create_shard_group(req).await;
    assert!(result.is_ok());
    let response = result.unwrap().into_inner();
    let group = response.group.as_ref().expect("group should be present");
    let placement_labels = group
        .config
        .as_ref()
        .and_then(|c| c.placement.as_ref())
        .map(|p| &p.required_labels);
    assert_eq!(placement_labels, Some(&labels));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_create_shard_group_duplicate_id() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req1 = Request::new(new_create_shard_group_request(
        "duplicate-group",
        "counter",
        2,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(req1).await;

    let req2 = Request::new(new_create_shard_group_request(
        "duplicate-group",
        "counter",
        2,
        HashMap::new(),
    ));
    let result = service.create_shard_group(req2).await;
    assert!(result.is_err(), "Duplicate group_id should fail");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_create_shard_group_invalid_shard_count() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(CreateShardGroupRequest {
        config: Some(DataParallelConfig {
            group_id: "invalid-group".to_string(),
            shard_count: 0, // Invalid: must be >= 1
            partition_strategy: PartitionStrategy::PartitionStrategyHash as i32,
            rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
            placement: None,
        }),
        actor_type: "counter".to_string(),
        shard_config: None,
        initial_state: Vec::new(),
        metadata: HashMap::new(),
    });

    let result = service.create_shard_group(req).await;
    assert!(result.is_err(), "shard_count=0 should fail");
}

// ========================================================================
// GetShardGroup Tests
// ========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_get_shard_group_success() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    // Create group first
    let create_req = Request::new(new_create_shard_group_request(
        "get-test-group",
        "counter",
        3,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    // Get group
    let get_req = Request::new(GetShardGroupRequest {
        group_id: "get-test-group".to_string(),
    });
    let result = service.get_shard_group(get_req).await;
    assert!(result.is_ok());
    let response = result.unwrap().into_inner();
    let group = response.group.as_ref().expect("group should be present");
    let cfg = group.config.as_ref().expect("config required");
    assert_eq!(cfg.group_id, "get-test-group");
    assert_eq!(cfg.shard_count, 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_get_shard_group_not_found() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(GetShardGroupRequest {
        group_id: "nonexistent-group".to_string(),
    });
    let result = service.get_shard_group(req).await;
    assert!(result.is_err(), "Non-existent group should return error");
}

// ========================================================================
// ListShardGroups Tests
// ========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_list_shard_groups_empty() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(ListShardGroupsRequest {
        actor_type: String::new(),
        state: ShardGroupState::ShardGroupStateUnspecified as i32,
        page: Some(plexspaces_proto::common::v1::PageRequest {
            offset: 0,
            limit: 100,
            filter: String::new(),
            order_by: String::new(),
        }),
    });

    let result = service.list_shard_groups(req).await;
    assert!(result.is_ok());
    let response = result.unwrap().into_inner();
    assert!(response.groups.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_list_shard_groups_with_filter() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    // Create two groups with different actor types
    let req1 = Request::new(new_create_shard_group_request(
        "group-counter",
        "counter",
        2,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(req1).await;

    let req2 = Request::new(new_create_shard_group_request(
        "group-cache",
        "cache",
        2,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(req2).await;

    // List filtered by actor_type
    let list_req = Request::new(ListShardGroupsRequest {
        actor_type: "counter".to_string(),
        state: ShardGroupState::ShardGroupStateUnspecified as i32,
        page: Some(plexspaces_proto::common::v1::PageRequest {
            offset: 0,
            limit: 100,
            filter: String::new(),
            order_by: String::new(),
        }),
    });
    let result = service.list_shard_groups(list_req).await;
    assert!(result.is_ok());
    let response = result.unwrap().into_inner();
    assert_eq!(response.groups.len(), 1);
    assert_eq!(response.groups.len(), 1);
    assert_eq!(response.groups[0].actor_type, "counter");
}

// ========================================================================
// DeleteShardGroup Tests
// ========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_delete_shard_group_success() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    // Create group
    let create_req = Request::new(new_create_shard_group_request(
        "delete-test-group",
        "counter",
        2,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    // Delete group
    let delete_req = Request::new(DeleteShardGroupRequest {
        group_id: "delete-test-group".to_string(),
        force: false,
        shutdown_timeout: None,
    });
    let result = service.delete_shard_group(delete_req).await;
    assert!(result.is_ok());

    // Verify deleted
    let get_req = Request::new(GetShardGroupRequest {
        group_id: "delete-test-group".to_string(),
    });
    let result = service.get_shard_group(get_req).await;
    assert!(result.is_err(), "Deleted group should not be found");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_delete_shard_group_not_found() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(DeleteShardGroupRequest {
        group_id: "nonexistent-group".to_string(),
        force: false,
        shutdown_timeout: None,
    });
    let result = service.delete_shard_group(req).await;
    // Should be idempotent (succeed even if not found)
    assert!(result.is_ok());
}

// ========================================================================
// SendToShard Tests
// ========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_send_to_shard_success() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    // Create group
    let create_req = Request::new(new_create_shard_group_request(
        "send-test-group",
        "counter",
        4,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    // Send message to shard
    let send_req = Request::new(SendToShardRequest {
        group_id: "send-test-group".to_string(),
        partition_key: b"user-123".to_vec(),
        message: Some(create_test_proto_message(b"increment".to_vec())),
        wait_for_response: false,
        timeout: None,
    });

    let result = service.send_to_shard(send_req).await;
    assert!(result.is_ok());
    let response = result.unwrap().into_inner();
    assert!(response.shard_id < 4);
    assert!(!response.shard_actor_id.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_send_to_shard_same_key_routes_to_same_shard() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    // Create group
    let create_req = Request::new(new_create_shard_group_request(
        "consistent-routing-group",
        "counter",
        4,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    let partition_key = b"user-456".to_vec();
    let send_req1 = Request::new(SendToShardRequest {
        group_id: "consistent-routing-group".to_string(),
        partition_key: partition_key.clone(),
        message: Some(create_test_proto_message(b"test".to_vec())),
        wait_for_response: false,
        timeout: None,
    });
    let result1 = service.send_to_shard(send_req1).await.unwrap().into_inner();
    let shard_id1 = result1.shard_id;

    let send_req2 = Request::new(SendToShardRequest {
        group_id: "consistent-routing-group".to_string(),
        partition_key: partition_key.clone(),
        message: Some(create_test_proto_message(b"test".to_vec())),
        wait_for_response: false,
        timeout: None,
    });
    let result2 = service.send_to_shard(send_req2).await.unwrap().into_inner();
    let shard_id2 = result2.shard_id;

    assert_eq!(
        shard_id1, shard_id2,
        "Same partition key should route to same shard"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_send_to_shard_group_not_found() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(SendToShardRequest {
        group_id: "nonexistent-group".to_string(),
        partition_key: b"key".to_vec(),
        message: Some(create_test_proto_message(b"test".to_vec())),
        wait_for_response: false,
        timeout: None,
    });

    let result = service.send_to_shard(req).await;
    assert!(result.is_err(), "Non-existent group should fail");
}

// ========================================================================
// ScatterGather Tests
// ========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_scatter_gather_success() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    // Create group
    let create_req = Request::new(new_create_shard_group_request(
        "scatter-test-group",
        "counter",
        3,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    // Scatter-gather query
    let scatter_req = Request::new(ScatterGatherRequest {
        group_id: "scatter-test-group".to_string(),
        query: Some(create_test_proto_message(b"get_count".to_vec())),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        aggregation: ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32,
        min_responses: 0,
    });

    let result = service.scatter_gather(scatter_req).await;
    assert!(result.is_ok());
    let response = result.unwrap().into_inner();
    let stats = response.stats.as_ref().expect("stats should be present");
    assert_eq!(stats.shards_queried, 3);
    assert_eq!(response.shard_responses.len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_scatter_gather_group_not_found() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(ScatterGatherRequest {
        group_id: "nonexistent-group".to_string(),
        query: Some(create_test_proto_message(b"test".to_vec())),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        aggregation: ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32,
        min_responses: 0,
    });

    let result = service.scatter_gather(req).await;
    assert!(result.is_err(), "Non-existent group should fail");
}

// ========================================================================
// Edge Case Tests for Parallel Map/Reduce Operations
// ========================================================================

// Actor that can simulate failures based on message content
struct FailingActor {
    fail_on_message: Option<String>,
}

struct InitAwareMetricsActor {
    initialized: bool,
}

impl InitAwareMetricsActor {
    fn new() -> Self {
        Self { initialized: false }
    }
}

#[async_trait]
impl ActorTrait for InitAwareMetricsActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait]
impl GenServer for InitAwareMetricsActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload).map_err(|e| {
            BehaviorError::ProcessingError(format!("Failed to parse metrics payload: {}", e))
        })?;
        let op = payload
            .get("op")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        let reply_payload = match op {
            "init" => {
                self.initialized = true;
                serde_json::json!({
                    "status": "ok",
                    "initialized": true,
                    "node_id": ctx.node_id,
                })
            }
            "compute" if self.initialized => serde_json::json!({
                "status": "ok",
                "role": "worker",
                "node_id": ctx.node_id,
                "compute_time_ms": 7,
                "coordination_time_ms": 3,
                "tuple_operations": 4,
                "messages_processed": 1,
                "errors": 0,
            }),
            "compute" => serde_json::json!({
                "error": "worker not initialized",
                "node_id": ctx.node_id,
            }),
            _ => serde_json::json!({
                "error": format!("unsupported op '{}'", op),
                "node_id": ctx.node_id,
            }),
        };

        if !msg.sender_id.is_empty() {
            let reply_msg =
                create_test_proto_message(serde_json::to_vec(&reply_payload).map_err(|e| {
                    BehaviorError::ProcessingError(format!(
                        "Failed to serialize metrics payload: {}",
                        e
                    ))
                })?);
            ctx.send_reply(
                if msg.correlation_id.is_empty() {
                    None
                } else {
                    Some(msg.correlation_id.as_str())
                },
                &msg.sender_id,
                msg.receiver_id.clone(),
                reply_msg,
            )
            .await
            .map_err(|e| {
                BehaviorError::ProcessingError(format!("Failed to send metrics reply: {}", e))
            })?;
        }

        Ok(())
    }
}

impl FailingActor {
    fn new() -> Self {
        Self {
            fail_on_message: None,
        }
    }

    fn with_fail_on(mut self, message: String) -> Self {
        self.fail_on_message = Some(message);
        self
    }
}

#[async_trait]
impl ActorTrait for FailingActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait]
impl GenServer for FailingActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        if let Ok(payload_str) = String::from_utf8(msg.payload.clone()) {
            if let Some(fail_on) = &self.fail_on_message {
                if payload_str.contains(fail_on) {
                    return Err(BehaviorError::ProcessingError(format!(
                        "Simulated failure for {}",
                        fail_on
                    )));
                }
            }
        }
        Ok(())
    }
}

// Actor that simulates slow responses
struct SlowActor {
    delay_ms: u64,
}

impl SlowActor {
    fn new(delay_ms: u64) -> Self {
        Self { delay_ms }
    }
}

#[async_trait]
impl ActorTrait for SlowActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait]
impl GenServer for SlowActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        tokio::time::sleep(tokio::time::Duration::from_millis(self.delay_ms)).await;
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_scatter_gather_partial_failures() {
    // Test: Some shards succeed, some fail - should still return partial results
    let (service, registry, locator) = create_test_actor_service("test-node").await;
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());

    // Register failing actor behavior
    let behavior_registry = locator.get_behavior_registry().await.unwrap();
    behavior_registry
        .register("failing-counter", |_initial_state| {
            Box::pin(async move {
                Ok(
                    Box::new(FailingActor::new().with_fail_on("fail".to_string()))
                        as Box<dyn ActorTrait>,
                )
            })
        })
        .await;

    // Create group with 4 shards
    let create_req = Request::new(CreateShardGroupRequest {
        config: Some(DataParallelConfig {
            group_id: "partial-fail-group".to_string(),
            shard_count: 4,
            partition_strategy: PartitionStrategy::PartitionStrategyHash as i32,
            rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
            placement: None,
        }),
        actor_type: "failing-counter".to_string(),
        shard_config: None,
        initial_state: Vec::new(),
        metadata: HashMap::new(),
    });
    let _ = service.create_shard_group(create_req).await;

    // Scatter-gather with query "fail" so all FailingActor shards fail
    let scatter_req = Request::new(ScatterGatherRequest {
        group_id: "partial-fail-group".to_string(),
        query: Some(create_test_proto_message(b"fail".to_vec())),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        aggregation: ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32,
        min_responses: 0, // Accept when all fail
    });

    let result = service.scatter_gather(scatter_req).await;
    assert!(
        result.is_ok(),
        "ScatterGather should return Ok with partial/failure stats"
    );
    let response = result.unwrap().into_inner();
    let stats = response.stats.as_ref().expect("stats should be present");
    assert_eq!(stats.shards_queried, 4);
    assert!(
        stats.shards_failed > 0,
        "FailingActor with 'fail' payload should record failures"
    );
    assert!(stats.shards_responded + stats.shards_failed == stats.shards_queried);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_scatter_gather_timeout() {
    // Test: Some shards timeout - should return partial results
    let (service, _registry, locator) = create_test_actor_service("test-node").await;
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());

    // Register slow actor behavior
    let behavior_registry = locator.get_behavior_registry().await.unwrap();
    behavior_registry
        .register("slow-counter", |_initial_state| {
            Box::pin(async move {
                Ok(Box::new(SlowActor::new(2000)) as Box<dyn ActorTrait>) // 2 second delay
            })
        })
        .await;

    // Create group
    let create_req = Request::new(CreateShardGroupRequest {
        config: Some(DataParallelConfig {
            group_id: "timeout-group".to_string(),
            shard_count: 3,
            partition_strategy: PartitionStrategy::PartitionStrategyHash as i32,
            rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
            placement: None,
        }),
        actor_type: "slow-counter".to_string(),
        shard_config: None,
        initial_state: Vec::new(),
        metadata: HashMap::new(),
    });
    let _ = service.create_shard_group(create_req).await;

    // Scatter-gather with short timeout (500ms)
    let scatter_req = Request::new(ScatterGatherRequest {
        group_id: "timeout-group".to_string(),
        query: Some(create_test_proto_message(b"get_count".to_vec())),
        timeout: Some(prost_types::Duration {
            seconds: 0,
            nanos: 500_000_000, // 500ms
        }),
        aggregation: ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32,
        min_responses: 0, // Accept no responses
    });

    let start = std::time::Instant::now();
    let result = service.scatter_gather(scatter_req).await;
    let elapsed = start.elapsed();

    // Should complete quickly (timeout), not wait for slow actors
    assert!(
        elapsed.as_millis() < 1000,
        "Should timeout quickly, not wait 2 seconds"
    );
    assert!(result.is_ok(), "Should succeed with timeout stats");
    let response = result.unwrap().into_inner();
    let stats = response.stats.as_ref().expect("stats should be present");
    assert_eq!(stats.shards_queried, 3);
    assert_eq!(
        stats.shards_responded, 0,
        "No shards should respond within timeout"
    );
    assert_eq!(stats.shards_failed, 3, "All shards should timeout");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_scatter_gather_min_responses_threshold() {
    // Test: min_responses threshold - should fail if not enough responses
    let (service, _registry, locator) = create_test_actor_service("test-node").await;
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());

    // Register failing actor behavior
    let behavior_registry = locator.get_behavior_registry().await.unwrap();
    behavior_registry
        .register("failing-counter-2", |_initial_state| {
            Box::pin(async move {
                Ok(
                    Box::new(FailingActor::new().with_fail_on("fail".to_string()))
                        as Box<dyn ActorTrait>,
                )
            })
        })
        .await;

    // Create group with 4 shards
    let create_req = Request::new(new_create_shard_group_request(
        "min-responses-group",
        "failing-counter-2",
        4,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    // Scatter-gather with min_responses = 4 (all must succeed)
    let scatter_req = Request::new(ScatterGatherRequest {
        group_id: "min-responses-group".to_string(),
        query: Some(create_test_proto_message(b"fail".to_vec())), // All will fail
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        aggregation: ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32,
        min_responses: 4, // Require all 4 responses
    });

    let result = service.scatter_gather(scatter_req).await;
    // When min_responses=4 and all fail, implementation may return Ok with stats or Err
    match result {
        Ok(resp) => {
            let inner = resp.into_inner();
            let stats = inner.stats.as_ref().expect("stats");
            assert_eq!(stats.shards_queried, 4);
            assert_eq!(stats.shards_failed, 4, "All shards should fail");
            assert_eq!(stats.shards_responded, 0);
        }
        Err(_) => {
            // Acceptable when min_responses not met and all fail
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_map_shard_group_partial_failures() {
    // Test: Map operation with partial failures
    let (service, _registry, locator) = create_test_actor_service("test-node").await;
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());

    // Register failing actor behavior
    let behavior_registry = locator.get_behavior_registry().await.unwrap();
    behavior_registry
        .register("failing-counter-3", |_initial_state| {
            Box::pin(async move {
                Ok(
                    Box::new(FailingActor::new().with_fail_on("fail".to_string()))
                        as Box<dyn ActorTrait>,
                )
            })
        })
        .await;

    // Create group
    let create_req = Request::new(new_create_shard_group_request(
        "map-partial-fail-group",
        "failing-counter-3",
        3,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    // Map query
    let map_req = Request::new(MapShardGroupRequest {
        group_id: "map-partial-fail-group".to_string(),
        map_function: Some(create_test_proto_message(b"fail".to_vec())), // All will fail
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        min_responses: 0,
    });

    let result = service.map_shard_group(map_req).await;
    assert!(result.is_ok(), "Should succeed even with failures");
    let response = result.unwrap().into_inner();
    let stats = response.stats.as_ref().expect("stats should be present");
    assert_eq!(stats.shards_queried, 3);
    assert_eq!(stats.shards_failed, 3, "All shards should fail");
    assert_eq!(stats.shards_responded, 0);
    assert_eq!(
        response.shard_results.len(),
        3,
        "Should have results for all shards (including failures)"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_scatter_gather_concat_aggregation_edge_cases() {
    // Test: Concatenated JSON parsing edge cases
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    // Create group with counter actors
    let create_req = Request::new(new_create_shard_group_request(
        "concat-edge-group",
        "counter",
        2,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    // Scatter-gather with Concat aggregation
    let scatter_req = Request::new(ScatterGatherRequest {
        group_id: "concat-edge-group".to_string(),
        query: Some(create_test_proto_message(b"get_count".to_vec())),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        aggregation: ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32,
        min_responses: 0,
    });

    let result = service.scatter_gather(scatter_req).await;
    assert!(result.is_ok(), "Should succeed");
    let response = result.unwrap().into_inner();

    // Verify we got a result when aggregation is Concat (payload may be empty depending on handler)
    assert!(response.stats.is_some(), "Stats should be present");
    assert_eq!(response.stats.as_ref().unwrap().shards_queried, 2);
}

// ========================================================================
// Multi-Node ShardGroup Integration Tests (in-process two nodes)
// ========================================================================

/// In-process two-node test: CreateShardGroup with node_ids places one shard on node1 and one on node2.
/// Node2 runs on a local gRPC server; node1's ObjectRegistry is updated so get_actor_service_client("node2")
/// resolves and remote SpawnActor succeeds. Validates multi-node spawn and that shard_actor_ids reflect both nodes.
#[tokio::test(flavor = "multi_thread")]
async fn test_create_shard_group_multi_node_scatter_gather() {
    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    use plexspaces_proto::ActorServiceServer;
    use std::time::Duration as StdDuration;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;

    let (node2_service, _node2_registry, _node2_locator) = create_test_actor_service("node2").await;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    let node2_addr = format!("http://127.0.0.1:{}", port);

    tokio::spawn({
        let svc = ActorServiceWrapper::new(node2_service.clone());
        async move {
            Server::builder()
                .add_service(ActorServiceServer::new(svc))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .expect("node2 server");
        }
    });

    tokio::time::sleep(StdDuration::from_millis(300)).await;

    let (node1_service, _node1_registry, node1_locator) = create_test_actor_service("node1").await;

    let obj_reg = node1_locator
        .get_object_registry()
        .await
        .expect("node1 ObjectRegistry");
    let ctx = RequestContext::new_without_auth(String::new(), String::new());
    let registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeNode as i32,
        object_id: "node2".to_string(),
        grpc_address: node2_addr,
        object_category: "Node".to_string(),
        ..Default::default()
    };
    obj_reg
        .register(&ctx, registration)
        .await
        .expect("register node2");

    let create_req = Request::new(new_create_shard_group_request_with_node_ids(
        "multi-node-group",
        "counter",
        2,
        vec!["node1".to_string(), "node2".to_string()],
    ));
    let result = node1_service.create_shard_group(create_req).await;
    assert!(
        result.is_ok(),
        "CreateShardGroup across nodes should succeed: {:?}",
        result.err()
    );
    let create_resp = result.unwrap().into_inner();
    let group = create_resp.group.as_ref().expect("group");
    assert_eq!(group.shard_actor_ids.len(), 2, "two shards (one per node)");
    let has_node1 = group.shard_actor_ids.iter().any(|id| id.contains("node1"));
    let has_node2 = group.shard_actor_ids.iter().any(|id| id.contains("node2"));
    assert!(has_node1, "one shard should be on node1");
    assert!(has_node2, "one shard should be on node2");

    // ScatterGather from node1 must collect replies from both local and remote shards.
    let scatter_req = Request::new(ScatterGatherRequest {
        group_id: "multi-node-group".to_string(),
        query: Some(create_test_proto_message(b"get_count".to_vec())),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        aggregation: ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32,
        min_responses: 2,
    });
    let scatter_result = node1_service.scatter_gather(scatter_req).await;
    assert!(
        scatter_result.is_ok(),
        "ScatterGather should succeed: {:?}",
        scatter_result.err()
    );
    let scatter_resp = scatter_result.unwrap().into_inner();
    let stats = scatter_resp.stats.as_ref().expect("stats");
    assert_eq!(stats.shards_queried, 2);
    assert_eq!(stats.shards_responded, 2, "both shards should reply");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_remote_spawn_actor_uses_request_namespace_for_actor_id() {
    use plexspaces_proto::ActorServiceServer;
    use std::time::Duration as StdDuration;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;

    let (node2_service, node2_registry, _node2_locator) = create_test_actor_service("node2").await;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("local_addr").port();

    tokio::spawn({
        let svc = ActorServiceWrapper::new(node2_service.clone());
        async move {
            Server::builder()
                .add_service(ActorServiceServer::new(svc))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .expect("node2 server");
        }
    });

    tokio::time::sleep(StdDuration::from_millis(300)).await;

    let channel = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{}", port))
        .expect("endpoint")
        .connect()
        .await
        .expect("connect node2");
    let mut client = plexspaces_proto::ActorServiceClient::new(channel);

    let requested_base_id = "remote-counter".to_string();
    let response = client
        .spawn_actor(Request::new(SpawnActorRequest {
            actor_type: "counter".to_string(),
            actor_id: requested_base_id.clone(),
            initial_state: Vec::new(),
            config: None,
            labels: HashMap::new(),
            facets: vec![],
            namespace: "heat-diffusion-rust".to_string(),
            instances_count: 1,
        }))
        .await
        .expect("remote spawn should succeed")
        .into_inner();

    assert_eq!(
        response.actor.as_ref().expect("actor response").actor_type,
        "counter"
    );
    assert_ne!(
        response.actor_ref, requested_base_id,
        "remote spawn must return the canonical actor id built on the receiving node"
    );
    assert!(
        response
            .actor_ref
            .contains("//counter::heat-diffusion-rust@node2"),
        "remote spawn should return canonical actor id with namespace, got {}",
        response.actor_ref
    );
    assert_eq!(
        response.actor.as_ref().expect("actor response").actor_id,
        response.actor_ref
    );
    assert_eq!(
        response.actor.as_ref().expect("actor response").namespace,
        "heat-diffusion-rust"
    );

    let discover_ctx =
        RequestContext::new_without_auth(String::new(), "heat-diffusion-rust".to_string());
    let discovered = node2_registry
        .discover_actors_by_type(&discover_ctx, "counter")
        .await;
    assert!(
        discovered
            .iter()
            .any(|actor_id| actor_id == &response.actor_ref),
        "spawned actor should be discoverable in the requested namespace"
    );
}

/// In-process two-node test: registry placement must ignore `node_ids` when strategy is FROM_REGISTRY.
#[tokio::test(flavor = "multi_thread")]
async fn test_create_shard_group_from_registry_ignores_stale_node_ids() {
    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    use plexspaces_proto::ActorServiceServer;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;

    let (node2_service, _node2_registry, _node2_locator) = create_test_actor_service("node2").await;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    let node2_addr = format!("http://127.0.0.1:{}", port);

    tokio::spawn({
        let svc = ActorServiceWrapper::new(node2_service.clone());
        async move {
            Server::builder()
                .add_service(ActorServiceServer::new(svc))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .expect("node2 server");
        }
    });

    let (node1_service, _node1_registry, node1_locator) = create_test_actor_service("node1").await;
    let ctx = RequestContext::new_without_auth(String::new(), String::new());
    node1_locator
        .get_object_registry()
        .await
        .expect("node1 ObjectRegistry")
        .register(
            &ctx,
            ObjectRegistration {
                object_type: ObjectType::ObjectTypeNode as i32,
                object_id: "node2".to_string(),
                grpc_address: node2_addr,
                object_category: "Node".to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("register node2 object registration");

    let mock_registry: Arc<dyn NodeRegistryTrait> = Arc::new(MockNodeRegistry {
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
    node1_locator.register_node_registry(mock_registry).await;

    let create_req = Request::new(new_create_shard_group_request_from_registry(
        "from-registry-group",
        "counter",
        2,
        "heat",
        vec!["stale-node".to_string()],
    ));
    let result = node1_service.create_shard_group(create_req).await;
    assert!(
        result.is_ok(),
        "CreateShardGroup should resolve nodes from registry, not stale node_ids: {:?}",
        result.err()
    );

    let group = result
        .unwrap()
        .into_inner()
        .group
        .expect("group should be returned");
    assert_eq!(
        group.shard_actor_ids.len(),
        2,
        "two shards should be spawned"
    );
    assert!(
        group.shard_actor_ids.iter().any(|id| id.contains("node1")),
        "registry placement should include node1"
    );
    assert!(
        group.shard_actor_ids.iter().any(|id| id.contains("node2")),
        "registry placement should include node2"
    );

    let scatter_req = Request::new(ScatterGatherRequest {
        group_id: "from-registry-group".to_string(),
        query: Some(create_test_proto_message(b"get_count".to_vec())),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        aggregation: ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32,
        min_responses: 0,
    });
    let scatter_result = node1_service.scatter_gather(scatter_req).await;
    assert!(
        scatter_result.is_ok(),
        "ScatterGather should succeed for registry placement: {:?}",
        scatter_result.err()
    );
    let stats = scatter_result.unwrap().into_inner().stats.expect("stats");
    assert_eq!(stats.shards_queried, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_bulk_update_initializes_remote_shards_before_scatter_gather() {
    use plexspaces_proto::actor::v1::BulkUpdateShardGroupRequest;
    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    use plexspaces_proto::v1::actor::ConsistencyLevel;
    use plexspaces_proto::ActorServiceServer;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::transport::Server;

    let (node2_service, _node2_registry, node2_locator) = create_test_actor_service("node2").await;
    node2_locator
        .get_behavior_registry()
        .await
        .expect("node2 behavior registry")
        .register("init-aware-metrics", |_initial_state| {
            Box::pin(
                async move { Ok(Box::new(InitAwareMetricsActor::new()) as Box<dyn ActorTrait>) },
            )
        })
        .await;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    let node2_addr = format!("http://127.0.0.1:{}", port);

    tokio::spawn({
        let svc = ActorServiceWrapper::new(node2_service.clone());
        async move {
            Server::builder()
                .add_service(ActorServiceServer::new(svc))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .expect("node2 server");
        }
    });

    let (node1_service, _node1_registry, node1_locator) = create_test_actor_service("node1").await;
    node1_locator
        .get_behavior_registry()
        .await
        .expect("node1 behavior registry")
        .register("init-aware-metrics", |_initial_state| {
            Box::pin(
                async move { Ok(Box::new(InitAwareMetricsActor::new()) as Box<dyn ActorTrait>) },
            )
        })
        .await;

    let ctx = RequestContext::new_without_auth(String::new(), String::new());
    node1_locator
        .get_object_registry()
        .await
        .expect("node1 ObjectRegistry")
        .register(
            &ctx,
            ObjectRegistration {
                object_type: ObjectType::ObjectTypeNode as i32,
                object_id: "node2".to_string(),
                grpc_address: node2_addr,
                object_category: "Node".to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("register node2");

    let create_req = Request::new(new_create_shard_group_request_with_node_ids(
        "bulk-update-remote-init-group",
        "init-aware-metrics",
        2,
        vec!["node1".to_string(), "node2".to_string()],
    ));
    let create_resp = node1_service
        .create_shard_group(create_req)
        .await
        .expect("create shard group")
        .into_inner();
    let group = create_resp.group.expect("group");
    assert_eq!(group.shard_actor_ids.len(), 2);

    let init_message = |region_id: usize| {
        let mut msg = create_test_proto_message(
            serde_json::to_vec(&serde_json::json!({
                "op": "init",
                "region_id": region_id,
            }))
            .expect("serialize init payload"),
        );
        msg.message_type = "call".to_string();
        msg
    };
    let mut updates = HashMap::new();
    updates.insert("0".to_string(), init_message(0));
    updates.insert("1".to_string(), init_message(1));

    let bulk_resp = node1_service
        .bulk_update_shard_group(Request::new(BulkUpdateShardGroupRequest {
            group_id: "bulk-update-remote-init-group".to_string(),
            updates,
            consistency_level: ConsistencyLevel::ConsistencyLevelLinearizable as i32,
            timeout: Some(prost_types::Duration {
                seconds: 5,
                nanos: 0,
            }),
            wait_for_responses: true,
        }))
        .await
        .expect("bulk update")
        .into_inner();
    assert_eq!(
        bulk_resp.updates_failed, 0,
        "all init updates should succeed"
    );

    let scatter_resp = node1_service
        .scatter_gather(Request::new(ScatterGatherRequest {
            group_id: "bulk-update-remote-init-group".to_string(),
            query: Some(create_test_proto_message(
                serde_json::to_vec(&serde_json::json!({ "op": "compute" }))
                    .expect("serialize compute payload"),
            )),
            timeout: Some(prost_types::Duration {
                seconds: 5,
                nanos: 0,
            }),
            aggregation: ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32,
            min_responses: 2,
        }))
        .await
        .expect("scatter gather")
        .into_inner();

    let mut payloads = Vec::new();
    for shard in scatter_resp.shard_responses {
        let response = shard.response.expect("shard response");
        let payload: serde_json::Value =
            serde_json::from_slice(&response.payload).expect("json payload");
        payloads.push(payload);
    }

    assert_eq!(payloads.len(), 2);
    assert!(payloads
        .iter()
        .all(|payload| payload.get("error").is_none()));
    assert!(payloads
        .iter()
        .all(|payload| payload.get("compute_time_ms").and_then(|v| v.as_u64()) == Some(7)));
    assert!(payloads
        .iter()
        .all(|payload| payload.get("coordination_time_ms").and_then(|v| v.as_u64()) == Some(3)));
    assert!(payloads
        .iter()
        .all(|payload| payload.get("tuple_operations").and_then(|v| v.as_u64()) == Some(4)));
    let node_ids = payloads
        .iter()
        .filter_map(|payload| payload.get("node_id").and_then(|value| value.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(node_ids.contains("node1"));
    assert!(node_ids.contains("node2"));
}

// ========================================================================
// Collective Operation Tests (Broadcast, Reduce, AllReduce, Barrier, SpawnActors)
// ========================================================================

use plexspaces_proto::actor::v1::{
    AllReduceShardGroupRequest, BarrierShardGroupRequest, BroadcastShardGroupRequest,
    CollectiveReduction, ReduceShardGroupRequest, SpawnActorsRequest,
};

#[tokio::test(flavor = "multi_thread")]
async fn test_broadcast_shard_group_success() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let create_req = Request::new(new_create_shard_group_request(
        "broadcast-group",
        "counter",
        3,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    let req = Request::new(BroadcastShardGroupRequest {
        group_id: "broadcast-group".to_string(),
        message: Some(create_test_proto_message(
            serde_json::to_vec(&serde_json::json!({"action": "ping"})).unwrap(),
        )),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        min_acks: 0,
    });

    let result = service.broadcast_shard_group(req).await;
    assert!(
        result.is_ok(),
        "Broadcast should succeed: {:?}",
        result.err()
    );
    let resp = result.unwrap().into_inner();
    let stats = resp.stats.as_ref().expect("stats should be present");
    assert_eq!(stats.shards_queried, 3);
    assert_eq!(resp.shard_responses.len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_broadcast_shard_group_not_found() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(BroadcastShardGroupRequest {
        group_id: "nonexistent-group".to_string(),
        message: Some(create_test_proto_message(b"test".to_vec())),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        min_acks: 0,
    });

    let result = service.broadcast_shard_group(req).await;
    assert!(result.is_err(), "Non-existent group should fail");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_reduce_shard_group_no_reducible_values() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let create_req = Request::new(new_create_shard_group_request(
        "reduce-group",
        "counter",
        3,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    // CounterActor's handle_request returns Ok(()) without an explicit reply payload,
    // so reduce cannot extract numeric values from shard responses.
    let req = Request::new(ReduceShardGroupRequest {
        group_id: "reduce-group".to_string(),
        map_function: Some(create_test_proto_message(
            serde_json::to_vec(&serde_json::json!({"action": "get_count"})).unwrap(),
        )),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        min_responses: 0,
        reduction: CollectiveReduction::CollectiveReductionSum as i32,
        target: None,
    });

    let result = service.reduce_shard_group(req).await;
    // Reduce correctly fails when shard responses don't contain reducible values
    assert!(
        result.is_err(),
        "Reduce should fail when actors return empty payloads"
    );
    let status = result.unwrap_err();
    assert!(
        status
            .message()
            .contains("No values available for reduction")
            || status.message().contains("reduction"),
        "Error should indicate reduction failure: {}",
        status.message()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_reduce_shard_group_not_found() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(ReduceShardGroupRequest {
        group_id: "nonexistent-group".to_string(),
        map_function: Some(create_test_proto_message(b"test".to_vec())),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        min_responses: 0,
        reduction: CollectiveReduction::CollectiveReductionSum as i32,
        target: None,
    });

    let result = service.reduce_shard_group(req).await;
    assert!(result.is_err(), "Non-existent group should fail");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_all_reduce_shard_group_no_reducible_values() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let create_req = Request::new(new_create_shard_group_request(
        "allreduce-group",
        "counter",
        2,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    // All-reduce fails at the reduce step when actors don't return reducible payloads
    let req = Request::new(AllReduceShardGroupRequest {
        group_id: "allreduce-group".to_string(),
        map_function: Some(create_test_proto_message(
            serde_json::to_vec(&serde_json::json!({"action": "get_count"})).unwrap(),
        )),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        min_responses: 0,
        reduction: CollectiveReduction::CollectiveReductionSum as i32,
        target: None,
    });

    let result = service.all_reduce_shard_group(req).await;
    assert!(
        result.is_err(),
        "AllReduce should fail when actors return empty payloads"
    );
    let status = result.unwrap_err();
    assert!(
        status.message().contains("reduction") || status.message().contains("reduce"),
        "Error should indicate reduction failure: {}",
        status.message()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_all_reduce_shard_group_not_found() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(AllReduceShardGroupRequest {
        group_id: "nonexistent-group".to_string(),
        map_function: Some(create_test_proto_message(b"test".to_vec())),
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        min_responses: 0,
        reduction: CollectiveReduction::CollectiveReductionSum as i32,
        target: None,
    });

    let result = service.all_reduce_shard_group(req).await;
    assert!(result.is_err(), "Non-existent group should fail");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_barrier_shard_group_success() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let create_req = Request::new(new_create_shard_group_request(
        "barrier-group",
        "counter",
        3,
        HashMap::new(),
    ));
    let _ = service.create_shard_group(create_req).await;

    let req = Request::new(BarrierShardGroupRequest {
        group_id: "barrier-group".to_string(),
        barrier_id: "barrier-1".to_string(),
        round: 1,
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        min_acks: 0,
    });

    let result = service.barrier_shard_group(req).await;
    assert!(result.is_ok(), "Barrier should succeed: {:?}", result.err());
    let resp = result.unwrap().into_inner();
    let stats = resp.stats.as_ref().expect("stats should be present");
    assert_eq!(stats.shards_queried, 3);
    assert_eq!(resp.shard_responses.len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_barrier_shard_group_not_found() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(BarrierShardGroupRequest {
        group_id: "nonexistent-group".to_string(),
        barrier_id: "b1".to_string(),
        round: 1,
        timeout: Some(prost_types::Duration {
            seconds: 5,
            nanos: 0,
        }),
        min_acks: 0,
    });

    let result = service.barrier_shard_group(req).await;
    assert!(result.is_err(), "Non-existent group should fail");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_spawn_actors_success() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(SpawnActorsRequest {
        requests: vec![
            SpawnActorRequest {
                actor_type: "counter".to_string(),
                namespace: "default".to_string(),
                ..Default::default()
            },
            SpawnActorRequest {
                actor_type: "counter".to_string(),
                namespace: "default".to_string(),
                ..Default::default()
            },
        ],
    });

    let result = service.spawn_actors(req).await;
    assert!(
        result.is_ok(),
        "SpawnActors should succeed: {:?}",
        result.err()
    );
    let resp = result.unwrap().into_inner();
    assert_eq!(resp.results.len(), 2);
    assert!(resp.results.iter().all(|r| r.success));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_spawn_actors_empty_request() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    let req = Request::new(SpawnActorsRequest { requests: vec![] });

    let result = service.spawn_actors(req).await;
    assert!(result.is_ok(), "Empty SpawnActors should succeed");
    let resp = result.unwrap().into_inner();
    assert_eq!(resp.results.len(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_spawn_actors_instances_count_replicas() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    // Spawn 3 replicas of the same actor type with a single request
    let req = Request::new(SpawnActorsRequest {
        requests: vec![SpawnActorRequest {
            actor_type: "counter".to_string(),
            namespace: "default".to_string(),
            actor_id: "worker".to_string(),
            instances_count: 3,
            ..Default::default()
        }],
    });

    let result = service.spawn_actors(req).await;
    assert!(
        result.is_ok(),
        "SpawnActors with instances_count=3 should succeed: {:?}",
        result.err()
    );
    let resp = result.unwrap().into_inner();
    assert_eq!(
        resp.results.len(),
        3,
        "Should have 3 results for 3 replicas"
    );
    assert!(resp.results.iter().all(|r| r.success));

    // Verify actor IDs are prefixed correctly: worker-0, worker-1, worker-2
    let actor_refs: Vec<&str> = resp
        .results
        .iter()
        .filter_map(|r| r.response.as_ref())
        .map(|r| r.actor_ref.as_str())
        .collect();
    assert_eq!(actor_refs.len(), 3);
    assert!(actor_refs[0].starts_with("worker-0"));
    assert!(actor_refs[1].starts_with("worker-1"));
    assert!(actor_refs[2].starts_with("worker-2"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_spawn_actors_instances_count_zero_spawns_one() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    // instances_count=0 should behave as 1
    let req = Request::new(SpawnActorsRequest {
        requests: vec![SpawnActorRequest {
            actor_type: "counter".to_string(),
            namespace: "default".to_string(),
            instances_count: 0,
            ..Default::default()
        }],
    });

    let result = service.spawn_actors(req).await;
    assert!(result.is_ok());
    let resp = result.unwrap().into_inner();
    assert_eq!(
        resp.results.len(),
        1,
        "instances_count=0 should spawn 1 actor"
    );
    assert!(resp.results[0].success);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_spawn_actors_instances_count_auto_id() {
    let (service, _registry, _locator) = create_test_actor_service("test-node").await;

    // No actor_id + instances_count=2 should generate ULID-based IDs
    let req = Request::new(SpawnActorsRequest {
        requests: vec![SpawnActorRequest {
            actor_type: "counter".to_string(),
            namespace: "default".to_string(),
            instances_count: 2,
            ..Default::default()
        }],
    });

    let result = service.spawn_actors(req).await;
    assert!(result.is_ok());
    let resp = result.unwrap().into_inner();
    assert_eq!(resp.results.len(), 2);
    assert!(resp.results.iter().all(|r| r.success));

    // IDs should be auto-generated (not empty)
    for r in &resp.results {
        let actor_ref = &r.response.as_ref().unwrap().actor_ref;
        assert!(
            !actor_ref.is_empty(),
            "Auto-generated ID should not be empty"
        );
    }
}
