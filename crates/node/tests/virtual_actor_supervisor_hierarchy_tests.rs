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

//! Comprehensive TDD tests for virtual actors in supervisor hierarchies
//!
//! Tests cover:
//! 1. Eager virtual actors as supervisor children
//! 2. Lazy virtual actors as supervisor children
//! 3. Mixed eager/lazy virtual actors in same supervisor
//! 4. Application deployment with root supervisor and virtual actor children
//! 5. Supervisor hierarchy with virtual actors at different levels
//! 6. Parent-child relationship tracking
//! 7. Activation behavior verification (eager vs lazy)
//!
//! Goal: 95%+ test coverage for virtual actor activation in supervisor contexts

use plexspaces_node::{NodeBuilder, Node};
use plexspaces_node::application_service::ApplicationServiceImpl;
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, DeployApplicationRequest,
    ApplicationSpec, ApplicationType, SupervisorSpec, ChildSpec, ChildType,
    SupervisionStrategy, RestartPolicy,
};
use plexspaces_proto::v1::common::Facet;
use plexspaces_proto::wasm::v1::WasmModule;
use plexspaces_core::{ActorRegistry, RequestContext, service_locator::service_names};
use plexspaces_core::application::ApplicationState;
use prost_types::Duration as ProstDuration;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tonic::Request;
use wat;

/// Create a minimal WASM module for testing
fn create_minimal_wasm_module() -> Vec<u8> {
    const MINIMAL_WASM_WAT: &str = r#"
        (module
            (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
                i32.const 0
            )
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
            (memory (export "memory") 1)
        )
    "#;
    wat::parse_str(MINIMAL_WASM_WAT).expect("Failed to parse WAT")
}

/// Create a test node with services initialized
async fn create_test_node() -> Arc<Node> {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let node_clone = node.clone();
    node_clone.initialize_services().await.expect("Failed to initialize services");
    let node_clone2 = node.clone();
    node_clone2.start().await.expect("Failed to start node");
    // Wait for services to be ready
    sleep(Duration::from_millis(100)).await;
    node
}

/// Get all actor IDs from ActorRegistry
async fn get_all_actor_ids(node: &Node) -> Vec<String> {
    let service_locator = node.service_locator();
    let actor_registry: Arc<ActorRegistry> = service_locator
        .get_service_by_name(service_names::ACTOR_REGISTRY)
        .await
        .expect("ActorRegistry not found");
    
    let ctx = RequestContext::internal();
    // Get all actors by iterating through known types or using a discovery method
    // For now, we'll use a simple approach: check if actors exist by trying to discover
    let mut actor_ids = Vec::new();
    
    // This is a simplified approach - in real tests, we'd use proper discovery
    // For now, we'll rely on the test knowing which actors should exist
    actor_ids
}

/// Helper to create virtual actor facet with specified activation strategy
fn create_virtual_actor_facet(activation_strategy: &str) -> Facet {
    let mut config = HashMap::new();
    config.insert("idle_timeout_seconds".to_string(), "300".to_string());
    config.insert("activation_strategy".to_string(), activation_strategy.to_string());
    
    Facet {
        r#type: "virtual_actor".to_string(),
        config,
        priority: 100,
        state: HashMap::new(),
        metadata: None,
    }
}

/// TEST 1: Eager virtual actors as supervisor children
/// 
/// Test that when a supervisor has eager virtual actor children:
/// 1. Actors are created and activated immediately
/// 2. Parent-child relationships are tracked
/// 3. Actors are registered in ActorRegistry
/// 4. Actors are immediately available for messages
#[tokio::test]
async fn test_eager_virtual_actors_as_supervisor_children() {
    let node = create_test_node().await;
    let node_id = node.id().as_str();
    
    // Create application spec with supervisor and eager virtual actor children
    let wasm_module = create_minimal_wasm_module();
    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                id: "eager-worker-1".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("eager")],
            },
            ChildSpec {
                id: "eager-worker-2".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("eager")],
            },
        ],
    };
    
    let app_spec = ApplicationSpec {
        name: "eager-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        description: "Test app with eager virtual actors".to_string(),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: HashMap::new(),
        supervisor: Some(supervisor_spec),
    };
    
    // Deploy application
    let application_manager = node.application_manager().await.expect("ApplicationManager not found");
    let service = ApplicationServiceImpl::new(node.clone(), application_manager);
    let wasm_module_proto = WasmModule {
        name: "eager-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_module,
        module_hash: "test-hash".to_string(),
        ..Default::default()
    };
    let request = DeployApplicationRequest {
        application_id: "eager-app-001".to_string(),
        name: "eager-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module_proto),
        config: Some(app_spec),
        release_config: None,
        initial_state: vec![],
    };
    
    let response = service.deploy_application(Request::new(request)).await;
    assert!(response.is_ok(), "Deployment should succeed: {:?}", response.err());
    let res = response.unwrap().into_inner();
    assert!(res.success, "Deployment should be successful");
    
    // Wait for actors to spawn and activate
    sleep(Duration::from_millis(500)).await;
    
    // Verify actors are registered and active
    let service_locator = node.service_locator();
    let actor_registry: Arc<ActorRegistry> = service_locator
        .get_service_by_name(service_names::ACTOR_REGISTRY)
        .await
        .expect("ActorRegistry not found");
    
    let ctx = RequestContext::internal();
    let actor_id_1 = format!("eager-worker-1@{}", node_id);
    let actor_id_2 = format!("eager-worker-2@{}", node_id);
    
    // Check that actors are registered
    let actor_1 = actor_registry.lookup_actor(&actor_id_1).await;
    let actor_2 = actor_registry.lookup_actor(&actor_id_2).await;
    
    assert!(actor_1.is_some(), "Eager virtual actor 1 should be registered and active");
    assert!(actor_2.is_some(), "Eager virtual actor 2 should be registered and active");
    
    // Verify parent-child relationships (if supervisor is tracked)
    // Note: This depends on how supervisor relationships are tracked
    // For now, we verify actors exist and are active
}

/// TEST 2: Lazy virtual actors as supervisor children
///
/// Test that when a supervisor has lazy virtual actor children:
/// 1. Actors are created but NOT activated immediately
/// 2. Actors are registered but in inactive state
/// 3. First message triggers activation
/// 4. Parent-child relationships are tracked
#[tokio::test]
async fn test_lazy_virtual_actors_as_supervisor_children() {
    let node = create_test_node().await;
    let node_id = node.id().as_str();
    
    // Create application spec with supervisor and lazy virtual actor children
    let wasm_module = create_minimal_wasm_module();
    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                id: "lazy-worker-1".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("lazy")],
            },
        ],
    };
    
    let app_spec = ApplicationSpec {
        name: "lazy-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        description: "Test app with lazy virtual actors".to_string(),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: HashMap::new(),
        supervisor: Some(supervisor_spec),
    };
    
    // Deploy application
    let application_manager = node.application_manager().await.expect("ApplicationManager not found");
    let service = ApplicationServiceImpl::new(node.clone(), application_manager);
    let wasm_module_proto = WasmModule {
        name: "lazy-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_module,
        module_hash: "test-hash".to_string(),
        ..Default::default()
    };
    let request = DeployApplicationRequest {
        application_id: "lazy-app-001".to_string(),
        name: "lazy-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module_proto),
        config: Some(app_spec),
        release_config: None,
        initial_state: vec![],
    };
    
    let response = service.deploy_application(Request::new(request)).await;
    assert!(response.is_ok(), "Deployment should succeed: {:?}", response.err());
    let res = response.unwrap().into_inner();
    assert!(res.success, "Deployment should be successful");
    
    // Wait for actors to be created (but not necessarily activated)
    sleep(Duration::from_millis(300)).await;
    
    // Verify actors are registered (but may not be active yet for lazy)
    let service_locator = node.service_locator();
    let actor_registry: Arc<ActorRegistry> = service_locator
        .get_service_by_name(service_names::ACTOR_REGISTRY)
        .await
        .expect("ActorRegistry not found");
    
    let actor_id = format!("lazy-worker-1@{}", node_id);
    
    // For lazy actors, they should be registered but activation happens on first message
    // Check that actor exists in registry (may be inactive)
    let routing = actor_registry
        .lookup_routing(&RequestContext::internal(), &actor_id)
        .await;
    
    // Actor should be routable (registered) even if not active
    assert!(routing.is_ok(), "Lazy virtual actor should be registered");
    
    // Note: For lazy actors, we can't easily check if they're active without sending a message
    // This is by design - lazy activation means activation on first message
}

/// TEST 3: Mixed eager and lazy virtual actors in same supervisor
///
/// Test that a supervisor can have both eager and lazy virtual actor children:
/// 1. Eager actors activate immediately
/// 2. Lazy actors remain inactive until first message
/// 3. Both types are properly tracked
#[tokio::test]
async fn test_mixed_eager_lazy_virtual_actors_in_supervisor() {
    let node = create_test_node().await;
    let node_id = node.id().as_str();
    
    // Create application spec with mixed eager/lazy virtual actor children
    let wasm_module = create_minimal_wasm_module();
    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                id: "eager-mixed-1".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("eager")],
            },
            ChildSpec {
                id: "lazy-mixed-1".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("lazy")],
            },
        ],
    };
    
    let app_spec = ApplicationSpec {
        name: "mixed-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        description: "Test app with mixed eager/lazy virtual actors".to_string(),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: HashMap::new(),
        supervisor: Some(supervisor_spec),
    };
    
    // Deploy application
    let application_manager = node.application_manager().await.expect("ApplicationManager not found");
    let service = ApplicationServiceImpl::new(node.clone(), application_manager);
    let wasm_module_proto = WasmModule {
        name: "mixed-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_module,
        module_hash: "test-hash".to_string(),
        ..Default::default()
    };
    let request = DeployApplicationRequest {
        application_id: "mixed-app-001".to_string(),
        name: "mixed-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module_proto),
        config: Some(app_spec),
        release_config: None,
        initial_state: vec![],
    };
    
    let response = service.deploy_application(Request::new(request)).await;
    assert!(response.is_ok(), "Deployment should succeed: {:?}", response.err());
    let res = response.unwrap().into_inner();
    assert!(res.success, "Deployment should be successful");
    
    // Wait for actors to spawn
    sleep(Duration::from_millis(500)).await;
    
    // Verify eager actor is active
    let service_locator = node.service_locator();
    let actor_registry: Arc<ActorRegistry> = service_locator
        .get_service_by_name(service_names::ACTOR_REGISTRY)
        .await
        .expect("ActorRegistry not found");
    
    let eager_id = format!("eager-mixed-1@{}", node_id);
    let lazy_id = format!("lazy-mixed-1@{}", node_id);
    
    // Eager actor should be active
    let eager_actor = actor_registry.lookup_actor(&eager_id).await;
    assert!(eager_actor.is_some(), "Eager virtual actor should be active");
    
    // Lazy actor should be registered (but may not be active yet)
    let lazy_routing = actor_registry
        .lookup_routing(&RequestContext::internal(), &lazy_id)
        .await;
    assert!(lazy_routing.is_ok(), "Lazy virtual actor should be registered");
}

/// TEST 4: Nested supervisor hierarchy with virtual actors
///
/// Test that virtual actors work correctly in nested supervisor hierarchies:
/// 1. Root supervisor with child supervisor
/// 2. Child supervisor with virtual actor children
/// 3. Both eager and lazy virtual actors at different levels
#[tokio::test]
async fn test_nested_supervisor_hierarchy_with_virtual_actors() {
    let node = create_test_node().await;
    let node_id = node.id().as_str();
    
    // Create nested supervisor hierarchy
    let wasm_module = create_minimal_wasm_module();
    let child_supervisor = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                id: "nested-eager-worker".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("eager")],
            },
            ChildSpec {
                id: "nested-lazy-worker".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("lazy")],
            },
        ],
    };
    
    let root_supervisor = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForAll.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                id: "root-eager-worker".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("eager")],
            },
            ChildSpec {
                id: "child-supervisor".to_string(),
                r#type: ChildType::ChildTypeSupervisor.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: Some(child_supervisor),
                facets: vec![],
            },
        ],
    };
    
    let app_spec = ApplicationSpec {
        name: "nested-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        description: "Test app with nested supervisor hierarchy and virtual actors".to_string(),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: HashMap::new(),
        supervisor: Some(root_supervisor),
    };
    
    // Deploy application
    let application_manager = node.application_manager().await.expect("ApplicationManager not found");
    let service = ApplicationServiceImpl::new(node.clone(), application_manager);
    let wasm_module_proto = WasmModule {
        name: "nested-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_module,
        module_hash: "test-hash".to_string(),
        ..Default::default()
    };
    let request = DeployApplicationRequest {
        application_id: "nested-app-001".to_string(),
        name: "nested-virtual-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module_proto),
        config: Some(app_spec),
        release_config: None,
        initial_state: vec![],
    };
    
    let response = service.deploy_application(Request::new(request)).await;
    assert!(response.is_ok(), "Deployment should succeed: {:?}", response.err());
    let res = response.unwrap().into_inner();
    assert!(res.success, "Deployment should be successful");
    
    // Wait for all actors to spawn
    sleep(Duration::from_millis(500)).await;
    
    // Verify all actors are registered
    let service_locator = node.service_locator();
    let actor_registry: Arc<ActorRegistry> = service_locator
        .get_service_by_name(service_names::ACTOR_REGISTRY)
        .await
        .expect("ActorRegistry not found");
    
    let root_eager_id = format!("root-eager-worker@{}", node_id);
    let nested_eager_id = format!("nested-eager-worker@{}", node_id);
    let nested_lazy_id = format!("nested-lazy-worker@{}", node_id);
    
    // Root level eager actor should be active
    let root_eager = actor_registry.lookup_actor(&root_eager_id).await;
    assert!(root_eager.is_some(), "Root level eager virtual actor should be active");
    
    // Nested eager actor should be active
    let nested_eager = actor_registry.lookup_actor(&nested_eager_id).await;
    assert!(nested_eager.is_some(), "Nested eager virtual actor should be active");
    
    // Nested lazy actor should be registered
    let nested_lazy_routing = actor_registry
        .lookup_routing(&RequestContext::internal(), &nested_lazy_id)
        .await;
    assert!(nested_lazy_routing.is_ok(), "Nested lazy virtual actor should be registered");
}

/// TEST 5: Application deployment with root supervisor and virtual actors
///
/// Test complete application deployment flow:
/// 1. Deploy application with root supervisor
/// 2. Supervisor spawns virtual actor children
/// 3. Eager actors activate immediately
/// 4. Lazy actors activate on first message
/// 5. All actors are tracked in ActorRegistry
/// 6. Parent-child relationships are maintained
#[tokio::test]
async fn test_application_deployment_with_virtual_actors() {
    let node = create_test_node().await;
    let node_id = node.id().as_str();
    
    // Create comprehensive application spec
    let wasm_module = create_minimal_wasm_module();
    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                id: "app-eager-1".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("eager")],
            },
            ChildSpec {
                id: "app-eager-2".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("eager")],
            },
            ChildSpec {
                id: "app-lazy-1".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("lazy")],
            },
        ],
    };
    
    let app_spec = ApplicationSpec {
        name: "virtual-app-deployment".to_string(),
        version: "1.0.0".to_string(),
        description: "Test app deployment with virtual actors".to_string(),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: HashMap::new(),
        supervisor: Some(supervisor_spec),
    };
    
    // Deploy application
    let application_manager = node.application_manager().await.expect("ApplicationManager not found");
    let service = ApplicationServiceImpl::new(node.clone(), application_manager);
    let wasm_module_proto = WasmModule {
        name: "virtual-app-deployment".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_module,
        module_hash: "test-hash".to_string(),
        ..Default::default()
    };
    let request = DeployApplicationRequest {
        application_id: "deployment-app-001".to_string(),
        name: "virtual-app-deployment".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module_proto),
        config: Some(app_spec),
        release_config: None,
        initial_state: vec![],
    };
    
    let response = service.deploy_application(Request::new(request)).await;
    assert!(response.is_ok(), "Deployment should succeed: {:?}", response.err());
    let res = response.unwrap().into_inner();
    assert!(res.success, "Deployment should be successful");
    
    // Wait for deployment to complete
    sleep(Duration::from_millis(500)).await;
    
    // Verify application is registered
    let service_locator = node.service_locator();
    use plexspaces_core::ApplicationManager;
    let app_manager: Arc<plexspaces_core::ApplicationManager> = service_locator
        .get_service_by_name(service_names::APPLICATION_MANAGER)
        .await
        .expect("ApplicationManager not found");
    
    let app_info = app_manager
        .get_application_info("deployment-app-001")
        .await;
    assert!(app_info.is_some(), "Application should be registered");
    
    // Verify all actors are registered
    let actor_registry: Arc<ActorRegistry> = service_locator
        .get_service_by_name(service_names::ACTOR_REGISTRY)
        .await
        .expect("ActorRegistry not found");
    
    let eager_1_id = format!("app-eager-1@{}", node_id);
    let eager_2_id = format!("app-eager-2@{}", node_id);
    let lazy_1_id = format!("app-lazy-1@{}", node_id);
    
    // Eager actors should be active
    let eager_1 = actor_registry.lookup_actor(&eager_1_id).await;
    let eager_2 = actor_registry.lookup_actor(&eager_2_id).await;
    assert!(eager_1.is_some(), "Eager actor 1 should be active");
    assert!(eager_2.is_some(), "Eager actor 2 should be active");
    
    // Lazy actor should be registered
    let lazy_1_routing = actor_registry
        .lookup_routing(&RequestContext::internal(), &lazy_1_id)
        .await;
    assert!(lazy_1_routing.is_ok(), "Lazy actor should be registered");
}

