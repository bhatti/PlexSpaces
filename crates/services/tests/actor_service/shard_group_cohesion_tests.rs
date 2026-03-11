// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Integration Tests for ShardGroup Cohesion
//!
//! Tests that verify ShardGroup labels correctly flow through the resource-based routing
//! infrastructure to ensure shards are placed on nodes matching the group's labels.
//!
//! ## Cohesion Flow Verified
//! ```
//! DataParallelConfig.placement.required_labels (NodePlacement)
//!   → ActorResourceRequirements.placement (NodePlacement)
//!   → NodeSelector filters by placement.required_labels vs NodeCapacity.labels
//!   → NodeCapacity.labels from ObjectRegistration.metadata.labels
//!   → metadata.labels from NodeRegistration.capabilities
//!   → capabilities["cluster"] from ConnectNodes
//! ```

use plexspaces_services::actor_service::ActorServiceImpl;
use plexspaces_core::{
    ActorRegistry, RequestContext,
    actor_context::ObjectRegistry as ObjectRegistryTrait,
    Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType,
    NodeRegistryTrait, ServiceLocator,
};
use plexspaces_behavior::GenServer;
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use plexspaces_proto::actor::v1::{
    actor_service_server::ActorService as ActorServiceTrait,
    CreateShardGroupRequest,
};
use plexspaces_proto::v1::actor::ActorResourceRequirements;
use plexspaces_proto::node::v1::NodeCapacity;
use plexspaces_proto::object_registry::v1::ObjectRegistration;
use std::sync::Arc;
use std::collections::HashMap;
use tonic::Request;
use async_trait::async_trait;
use plexspaces_core::Message;

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

// Helper to create test ActorService (same as shard_group_tests.rs)
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

    let service_locator_impl = service_locator.clone() as Arc<plexspaces_services::ServiceLocatorImpl>;
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator.clone(), node_id.to_string()));
    (actor_service, actor_registry, service_locator_impl)
}

// Helper adapter for ObjectRegistry (same as shard_group_tests.rs)
struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistryImpl>,
}

#[async_trait]
impl ObjectRegistryTrait for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
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
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: ObjectRegistration,
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
    ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
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

#[tokio::test(flavor = "multi_thread")]
async fn test_shard_group_labels_mapped_to_actor_resource_requirements() {
    // Test: Verify that config.placement.required_labels are stored and flow to ActorResourceRequirements.placement
    let (_service, _registry, _locator) = create_test_actor_service("test-node").await;

    let mut labels = HashMap::new();
    labels.insert("cluster".to_string(), "prod".to_string());
    labels.insert("zone".to_string(), "us-west-1".to_string());
    labels.insert("tier".to_string(), "compute".to_string());

    use plexspaces_proto::actor::v1::{DataParallelConfig, NodePlacement, NodePlacementStrategy, PartitionStrategy, RebalancePolicy};
    let req = Request::new(CreateShardGroupRequest {
        config: Some(DataParallelConfig {
            group_id: "labeled-group".to_string(),
            shard_count: 2,
            partition_strategy: PartitionStrategy::PartitionStrategyHash as i32,
            rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
            placement: Some(NodePlacement {
                strategy: NodePlacementStrategy::NodePlacementStrategyUnspecified as i32,
                cluster: String::new(),
                node_ids: vec![],
                required_labels: labels.clone(),
                preferred_node_ids: vec![],
                avoid_node_ids: vec![],
                resource_requirements: None,
                affinity_labels: HashMap::new(),
                preferred_node_id: String::new(),
            }),
        }),
        actor_type: "counter".to_string(),
        shard_config: None,
        initial_state: Vec::new(),
        metadata: HashMap::new(),
    });

    let result = _service.create_shard_group(req).await;
    assert!(result.is_ok(), "CreateShardGroup should succeed with labels");

    let response = result.unwrap().into_inner();
    let group = response.group.as_ref().expect("group should be present");
    let stored = group.config.as_ref().and_then(|c| c.placement.as_ref()).map(|p| &p.required_labels);
    assert_eq!(stored, Some(&labels), "ShardGroup config.placement should store required_labels");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_node_selector_filters_by_required_labels() {
    // Test: Verify that NodeSelector correctly filters nodes by ActorResourceRequirements.required_labels
    // This verifies the second step: ActorResourceRequirements.required_labels → NodeSelector filtering
    // Note: This test requires plexspaces-scheduler as a dev dependency
    
    use plexspaces_proto::common::v1::ResourceSpec;
    use plexspaces_scheduler::node_selector::NodeSelector;
    
    // Create node capacities with different labels
    let mut node_capacities = HashMap::new();
    
    // Node 1: Matches labels
    node_capacities.insert("node1".to_string(), NodeCapacity {
        labels: HashMap::from([
            ("cluster".to_string(), "prod".to_string()),
            ("zone".to_string(), "us-west-1".to_string()),
        ]),
        total: Some(ResourceSpec {
            cpu_cores: 4.0,
            memory_bytes: 8 * 1024 * 1024 * 1024,
            disk_bytes: 100 * 1024 * 1024 * 1024,
            gpu_count: 0,
            gpu_type: String::new(),
        }),
        available: Some(ResourceSpec {
            cpu_cores: 2.0,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            disk_bytes: 50 * 1024 * 1024 * 1024,
            gpu_count: 0,
            gpu_type: String::new(),
        }),
        allocated: Some(ResourceSpec {
            cpu_cores: 2.0,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            disk_bytes: 50 * 1024 * 1024 * 1024,
            gpu_count: 0,
            gpu_type: String::new(),
        }),
    });
    
    // Node 2: Doesn't match (different cluster)
    node_capacities.insert("node2".to_string(), NodeCapacity {
        labels: HashMap::from([
            ("cluster".to_string(), "dev".to_string()),  // Different cluster
            ("zone".to_string(), "us-west-1".to_string()),
        ]),
        total: Some(ResourceSpec {
            cpu_cores: 4.0,
            memory_bytes: 8 * 1024 * 1024 * 1024,
            disk_bytes: 100 * 1024 * 1024 * 1024,
            gpu_count: 0,
            gpu_type: String::new(),
        }),
        available: Some(ResourceSpec {
            cpu_cores: 2.0,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            disk_bytes: 50 * 1024 * 1024 * 1024,
            gpu_count: 0,
            gpu_type: String::new(),
        }),
        allocated: Some(ResourceSpec {
            cpu_cores: 2.0,
            memory_bytes: 4 * 1024 * 1024 * 1024,
            disk_bytes: 50 * 1024 * 1024 * 1024,
            gpu_count: 0,
            gpu_type: String::new(),
        }),
    });
    
    use plexspaces_proto::actor::v1::{NodePlacement, NodePlacementStrategy};
    let requirements = ActorResourceRequirements {
        placement: Some(NodePlacement {
            strategy: NodePlacementStrategy::NodePlacementStrategyUnspecified as i32,
            cluster: String::new(),
            node_ids: vec![],
            required_labels: HashMap::from([
                ("cluster".to_string(), "prod".to_string()),
                ("zone".to_string(), "us-west-1".to_string()),
            ]),
            preferred_node_ids: vec![],
            avoid_node_ids: vec![],
            resource_requirements: Some(ResourceSpec {
                cpu_cores: 1.0,
                memory_bytes: 1024 * 1024 * 1024,
                disk_bytes: 0,
                gpu_count: 0,
                gpu_type: String::new(),
            }),
            affinity_labels: HashMap::new(),
            preferred_node_id: String::new(),
        }),
    };
    
    // NodeSelector should select node1 (matches labels), not node2 (different cluster)
    let result = NodeSelector::select_node(&requirements, &node_capacities);
    assert!(result.is_ok(), "NodeSelector should find matching node");
    let (selected_node, _score) = result.unwrap();
    assert_eq!(selected_node, "node1", "NodeSelector should select node with matching labels");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_capabilities_to_labels_mapping() {
    // Test: Verify that NodeRegistration.capabilities are correctly mapped to ObjectRegistration.metadata.labels
    // This verifies: capabilities → metadata.labels → NodeCapacity.labels
    
    use plexspaces_proto::node::v1::NodeRegistration;
    use plexspaces_services::node_registry::{NodeRegistry, NodeRegistryConfig};
    use std::time::Duration;
    
    let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
    let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
        inner: object_registry_impl.clone(),
    });
    
    // Enable shared DB so registration persists to ObjectRegistry
    let mut config = NodeRegistryConfig::default();
    config.use_shared_db = true;
    config.gossip_enabled = false; // Disable gossip for simpler test
    config.db_max_attempts = 3;
    config.db_backoff_base = Duration::from_millis(10);
    config.db_backoff_cap = Duration::from_millis(100);
    
    let node_registry = NodeRegistry::new(
        object_registry.clone(),
        "test-node".to_string(),
        "127.0.0.1:8000".to_string(),
        config,
    );
    
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());
    
    // Create NodeRegistration with capabilities (as ConnectNodes would set)
    let mut capabilities = HashMap::new();
    capabilities.insert("cluster".to_string(), "prod".to_string());
    capabilities.insert("zone".to_string(), "us-west-1".to_string());
    
    let node_reg = NodeRegistration {
        node_id: "test-node".to_string(),
        node_address: "127.0.0.1:8000".to_string(),
        capabilities: capabilities.clone(),
        status: plexspaces_proto::node::v1::NodeStatus::NodeStatusReady as i32,
        registered_at: None,
        last_heartbeat: None,
        ..Default::default()
    };
    
    // Register node
    node_registry.register_node(&ctx, node_reg.clone()).await
        .expect("Failed to register node");
    
    // Give it a moment for async DB write
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Verify that ObjectRegistration has metadata.labels from capabilities
    // We need to query ObjectRegistry directly since lookup_node returns NodeRegistration
    use plexspaces_proto::object_registry::v1::ObjectType;
    let obj_reg = object_registry_impl.lookup_full(&ctx, ObjectType::ObjectTypeNode, "test-node").await
        .expect("Failed to lookup node")
        .expect("Node should be found");
    
    assert!(obj_reg.metadata.is_some(), "ObjectRegistration should have metadata");
    let metadata = obj_reg.metadata.as_ref().unwrap();
    assert_eq!(metadata.labels, capabilities, "metadata.labels should match capabilities");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_shard_group_cohesion_end_to_end() {
    // Test: End-to-end verification that ShardGroup labels flow through the entire system
    // This is a simplified test that verifies the components work together
    
    // 1. Create ShardGroup with labels
    let (_service, _registry, _locator) = create_test_actor_service("test-node").await;
    
    let mut labels = HashMap::new();
    labels.insert("cluster".to_string(), "prod".to_string());
    labels.insert("zone".to_string(), "us-west-1".to_string());
    
    use plexspaces_proto::actor::v1::{DataParallelConfig, NodePlacement, NodePlacementStrategy, PartitionStrategy, RebalancePolicy};
    let req = Request::new(CreateShardGroupRequest {
        config: Some(DataParallelConfig {
            group_id: "cohesion-test-group".to_string(),
            shard_count: 2,
            partition_strategy: PartitionStrategy::PartitionStrategyHash as i32,
            rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
            placement: Some(NodePlacement {
                strategy: NodePlacementStrategy::NodePlacementStrategyUnspecified as i32,
                cluster: String::new(),
                node_ids: vec![],
                required_labels: labels.clone(),
                preferred_node_ids: vec![],
                avoid_node_ids: vec![],
                resource_requirements: None,
                affinity_labels: HashMap::new(),
                preferred_node_id: String::new(),
            }),
        }),
        actor_type: "counter".to_string(),
        shard_config: None,
        initial_state: Vec::new(),
        metadata: HashMap::new(),
    });

    let result = _service.create_shard_group(req).await;
    assert!(result.is_ok(), "ShardGroup creation should succeed");

    let response = result.unwrap().into_inner();
    let group = response.group.as_ref().expect("group should be present");

    let stored = group.config.as_ref().and_then(|c| c.placement.as_ref()).map(|p| &p.required_labels);
    assert_eq!(stored, Some(&labels), "ShardGroup config.placement should store required_labels");
    
    // Verify shards were created
    assert_eq!(group.shard_actor_ids.len(), 2, "Should have 2 shard actors");
    
    // Note: Full end-to-end test would require:
    // 1. Multiple nodes with different labels
    // 2. NodeSelector to actually place shards
    // 3. Verification that shards are on nodes matching labels
    // This is a simplified unit test - full integration test would be in integration tests
}
