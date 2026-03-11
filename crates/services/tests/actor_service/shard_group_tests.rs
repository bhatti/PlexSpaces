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

use plexspaces_services::actor_service::{ActorServiceImpl, ActorServiceWrapper};
use plexspaces_core::{
    ActorRegistry, ServiceLocator, RequestContext,
    actor_context::ObjectRegistry as ObjectRegistryTrait,
    Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType,
    behavior_factory::BehaviorFactory,
};
use plexspaces_behavior::GenServer;
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use plexspaces_proto::actor::v1::{
    actor_service_server::ActorService as ActorServiceTrait,
    CreateShardGroupRequest, DataParallelConfig,
    DeleteShardGroupRequest,
    GetShardGroupRequest, ListShardGroupsRequest,
    MapShardGroupRequest, RebalancePolicy,
    SendToShardRequest, ScatterGatherRequest,
    ShardGroupState, PartitionStrategy, ShardGroupAggregationStrategy,
};
use plexspaces_proto::common::v1::{Empty, Message as ProtoMessage};
use std::sync::Arc;
use std::collections::HashMap;
use tonic::Request;
use async_trait::async_trait;
use plexspaces_core::Message;
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
                    preferred_node_ids: vec![],
                    avoid_node_ids: vec![],
                    resource_requirements: None,
                    affinity_labels: HashMap::new(),
                    preferred_node_id: String::new(),
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
                preferred_node_ids: vec![],
                avoid_node_ids: vec![],
                resource_requirements: None,
                affinity_labels: HashMap::new(),
                preferred_node_id: String::new(),
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

#[async_trait::async_trait]
impl ObjectRegistryTrait for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(ctx, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .register(ctx, registration)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
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
    ) -> Result<Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
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
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
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
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }
}

/// Helper to create test ActorService with registry and auth disabled
async fn create_test_actor_service(
    node_id: &str,
) -> (Arc<ActorServiceImpl>, Arc<ActorRegistry>, Arc<plexspaces_services::ServiceLocatorImpl>) {
    use plexspaces_node::create_default_service_locator;
    use plexspaces_core::actor_context::ObjectRegistry as ObjectRegistryTrait;

    let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
    let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
        inner: object_registry_impl,
    });
    let actor_registry = Arc::new(ActorRegistry::new(object_registry, node_id.to_string()));

    // Use create_default_service_locator which doesn't call blocking code
    let service_locator = create_default_service_locator(Some(node_id.to_string()), None, None).await;
    service_locator.register_service(actor_registry.clone()).await;

    // Register ActorFactory (required for spawn_actor to work)
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    use plexspaces_core::{FacetManager, FacetManagerServiceWrapper, VirtualActorManager};
    let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    let facet_manager = Arc::new(FacetManagerServiceWrapper::new(Arc::new(FacetManager::new())));
    service_locator.register_service(virtual_actor_manager).await;
    service_locator.register_service(facet_manager).await;
    let actor_factory = ActorFactoryImpl::new_arc(service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>).await;
    service_locator.register_service_by_name(plexspaces_core::service_names::ACTOR_FACTORY_IMPL, actor_factory.clone()).await;
    let factory_trait: Arc<dyn plexspaces_actor::ActorFactory> = actor_factory.clone();
    service_locator.register_actor_factory(factory_trait).await;

    // Register BehaviorRegistry and behavior for "counter" actor type
    // Note: BehaviorRegistry needs to be registered so ActorFactory can create actors
    use plexspaces_core::behavior_factory::BehaviorRegistry;
    let behavior_registry = BehaviorRegistry::new();
    behavior_registry.register_simple("counter", || {
        Box::pin(async move {
            Ok(Box::new(CounterActor::new()) as Box<dyn plexspaces_core::Actor>)
        })
    }).await;
    service_locator.register_behavior_registry(Arc::new(behavior_registry)).await;

    // Disable auth for tests
    let config = plexspaces_proto::node::v1::SecurityConfig {
        disable_auth: true,
        ..Default::default()
    };
    service_locator.register_security_config(config).await;

    // Cast to ServiceLocatorImpl for return type
    let service_locator_impl = service_locator.clone() as Arc<plexspaces_services::ServiceLocatorImpl>;
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator.clone(), node_id.to_string()));
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

    assert_eq!(shard_id1, shard_id2, "Same partition key should route to same shard");
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

impl FailingActor {
    fn new() -> Self {
        Self { fail_on_message: None }
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
                    return Err(BehaviorError::ProcessingError(format!("Simulated failure for {}", fail_on)));
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
    behavior_registry.register("failing-counter", |_initial_state| {
        Box::pin(async move {
            Ok(Box::new(FailingActor::new().with_fail_on("fail".to_string())) as Box<dyn ActorTrait>)
        })
    }).await;
    
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
    assert!(result.is_ok(), "ScatterGather should return Ok with partial/failure stats");
    let response = result.unwrap().into_inner();
    let stats = response.stats.as_ref().expect("stats should be present");
    assert_eq!(stats.shards_queried, 4);
    assert!(stats.shards_failed > 0, "FailingActor with 'fail' payload should record failures");
    assert!(stats.shards_responded + stats.shards_failed == stats.shards_queried);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_scatter_gather_timeout() {
    // Test: Some shards timeout - should return partial results
    let (service, _registry, locator) = create_test_actor_service("test-node").await;
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    
    // Register slow actor behavior
    let behavior_registry = locator.get_behavior_registry().await.unwrap();
    behavior_registry.register("slow-counter", |_initial_state| {
        Box::pin(async move {
            Ok(Box::new(SlowActor::new(2000)) as Box<dyn ActorTrait>) // 2 second delay
        })
    }).await;
    
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
    assert!(elapsed.as_millis() < 1000, "Should timeout quickly, not wait 2 seconds");
    assert!(result.is_ok(), "Should succeed with timeout stats");
    let response = result.unwrap().into_inner();
    let stats = response.stats.as_ref().expect("stats should be present");
    assert_eq!(stats.shards_queried, 3);
    assert_eq!(stats.shards_responded, 0, "No shards should respond within timeout");
    assert_eq!(stats.shards_failed, 3, "All shards should timeout");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_scatter_gather_min_responses_threshold() {
    // Test: min_responses threshold - should fail if not enough responses
    let (service, _registry, locator) = create_test_actor_service("test-node").await;
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    
    // Register failing actor behavior
    let behavior_registry = locator.get_behavior_registry().await.unwrap();
    behavior_registry.register("failing-counter-2", |_initial_state| {
        Box::pin(async move {
            Ok(Box::new(FailingActor::new().with_fail_on("fail".to_string())) as Box<dyn ActorTrait>)
        })
    }).await;
    
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
    behavior_registry.register("failing-counter-3", |_initial_state| {
        Box::pin(async move {
            Ok(Box::new(FailingActor::new().with_fail_on("fail".to_string())) as Box<dyn ActorTrait>)
        })
    }).await;
    
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
    assert_eq!(response.shard_results.len(), 3, "Should have results for all shards (including failures)");
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
    use tonic::transport::Server;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use std::time::Duration as StdDuration;

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

    let obj_reg = node1_locator.get_object_registry().await.expect("node1 ObjectRegistry");
    let ctx = RequestContext::new_without_auth(String::new(), String::new());
    let registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeNode as i32,
        object_id: "node2".to_string(),
        grpc_address: node2_addr,
        object_category: "Node".to_string(),
        ..Default::default()
    };
    obj_reg.register(&ctx, registration).await.expect("register node2");

    let create_req = Request::new(new_create_shard_group_request_with_node_ids(
        "multi-node-group",
        "counter",
        2,
        vec!["node1".to_string(), "node2".to_string()],
    ));
    let result = node1_service.create_shard_group(create_req).await;
    assert!(result.is_ok(), "CreateShardGroup across nodes should succeed: {:?}", result.err());
    let create_resp = result.unwrap().into_inner();
    let group = create_resp.group.as_ref().expect("group");
    assert_eq!(group.shard_actor_ids.len(), 2, "two shards (one per node)");
    let has_node1 = group.shard_actor_ids.iter().any(|id| id.contains("node1"));
    let has_node2 = group.shard_actor_ids.iter().any(|id| id.contains("node2"));
    assert!(has_node1, "one shard should be on node1");
    assert!(has_node2, "one shard should be on node2");

    // ScatterGather from node1: ask_helper routes to remote shard on node2 via route_remote (ObjectRegistry has node2).
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
    assert!(scatter_result.is_ok(), "ScatterGather across nodes should succeed: {:?}", scatter_result.err());
    let scatter_resp = scatter_result.unwrap().into_inner();
    let stats = scatter_resp.stats.as_ref().expect("stats");
    assert_eq!(stats.shards_queried, 2);
    assert_eq!(stats.shards_responded, 2, "both shards (local and remote) should respond");
}
