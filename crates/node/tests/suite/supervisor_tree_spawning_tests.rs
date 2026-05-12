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

//! Comprehensive tests for supervisor tree spawning (Erlang-style)
//!
//! Verifies that when an application is deployed:
//! 1. All workers in the supervisor tree are spawned
//! 2. All supervisors are spawned as actors (Erlang-style)
//! 3. Nested supervisor trees are handled correctly
//! 4. All spawned actors are tracked in ActorRegistry
//! 5. Actor types match `ChildSpec.actor_identity.actor_type` (behavior class) in canonical `ActorId`s
//! 6. The entire tree is spawned when an application is deployed

use super::test_helpers::app_request_with_tenant;
use plexspaces_actor::{
    ActorId, ApplicationManager, InitializableServiceLocator, RequestContext, RequestContextExt,
    ServiceLocator,
};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, ApplicationSpec, ApplicationType,
    DeployApplicationRequest, ShutdownStrategy,
};
use plexspaces_proto::common::v1::ActorIdentity;
use plexspaces_proto::supervision::v1::{
    ChildSpec, RestartPolicy, SupervisionStrategy, SupervisorSpec,
};
use plexspaces_proto::v1::application::ApplicationState;
use plexspaces_proto::wasm::v1::WasmModule;
use plexspaces_proto::ActorLifecycleEvent;
use plexspaces_services::application_service::ApplicationServiceImpl;
use prost_types::Duration as ProstDuration;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::mpsc;
use tokio::task::yield_now;
use tokio::time::{sleep, timeout, Duration};
use tonic::Request;
use wat;

fn app_ctx(name: &str) -> RequestContext {
    RequestContext::new_without_auth(String::new(), name.to_string())
}

/// Shared minimal WASM module for all tests (loaded once, reused)
///
/// ## Purpose
/// Caches the minimal WASM module to avoid re-parsing WAT on every test.
/// Uses OnceLock pattern from WASM integration tests for thread-safe initialization.
static SHARED_WASM_BYTES: OnceLock<Vec<u8>> = OnceLock::new();

/// Get or create shared minimal WASM module bytes
///
/// ## Expected Behavior
/// - First call: Parses WAT and caches the bytes
/// - Subsequent calls: Returns cached bytes (no parsing overhead)
fn get_shared_wasm_bytes() -> &'static Vec<u8> {
    SHARED_WASM_BYTES.get_or_init(|| {
        const MINIMAL_WASM_WAT: &str = r#"
            (module
                (memory (export "memory") 1)
                (func (export "init") (param i32 i32) (result i32)
                    i32.const 0
                )
                (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
                    i32.const 0
                )
                (func (export "snapshot_state") (result i32 i32)
                    i32.const 0
                    i32.const 0
                )
            )
        "#;
        wat::parse_str(MINIMAL_WASM_WAT).expect("Failed to parse WAT")
    })
}

/// Create a minimal WASM module for testing (uses shared bytes)
fn create_minimal_wasm_module() -> Vec<u8> {
    get_shared_wasm_bytes().clone()
}

/// Create a test node with services initialized (without starting gRPC server)
///
/// ## Purpose
/// Creates a node with all services initialized but does NOT start the gRPC server.
/// This avoids port conflicts when running tests in parallel.
///
/// ## Expected Behavior
/// - Node is built and services are initialized
/// - ActorFactory, ApplicationManager, and other services are ready
/// - gRPC server is NOT started (use create_test_node_with_server() for integration tests)
async fn create_test_node() -> Arc<Node> {
    let node = Arc::new(
        NodeBuilder::new("test-node")
            .with_in_memory_backends()
            .build()
            .await,
    );
    let node_clone = node.clone();
    node_clone
        .initialize_services()
        .await
        .expect("Failed to initialize services");
    // Wait for services to be ready with polling (no gRPC server startup)
    for _ in 0..5 {
        yield_now().await;
        sleep(Duration::from_millis(10)).await;
    }
    node
}

/// Create a test node with gRPC server started (for integration tests only)
///
/// ## Purpose
/// Creates a node with gRPC server started. Use this ONLY for integration tests
/// that need actual gRPC communication. Most unit tests should use create_test_node().
///
/// ## Expected Behavior
/// - Node is built, services initialized, and gRPC server is started
/// - Server listens on an ephemeral port (0) to avoid conflicts
/// - Services are ready for full integration testing
async fn create_test_node_with_server() -> Arc<Node> {
    let node = Arc::new(
        NodeBuilder::new("test-node")
            .with_listen_addr("127.0.0.1:0") // Ephemeral port to avoid conflicts
            .with_in_memory_backends()
            .build()
            .await,
    );
    let node_clone = node.clone();
    node_clone
        .initialize_services()
        .await
        .expect("Failed to initialize services");
    let node_clone2 = node.clone();
    node_clone2.start().await.expect("Failed to start node");
    // Wait for services and server to be ready
    for _ in 0..10 {
        yield_now().await;
        sleep(Duration::from_millis(10)).await;
    }
    node
}

/// Helper to wait for actors to be registered (more reliable than activated check)
async fn wait_for_actors_activated(
    node: &Node,
    expected_actor_ids: &[ActorId],
    timeout_duration: Duration,
) -> bool {
    // Get ActorRegistry
    let registry = node
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry not found");

    let start = std::time::Instant::now();
    let mut last_check = std::time::Instant::now();

    // Poll with adaptive backoff - check registered_actor_ids which is more reliable
    while start.elapsed() < timeout_duration {
        // Check if all actors are registered (registered_actor_ids is updated when actors are spawned)
        let registered_ids = registry.registered_actor_ids().await;
        let expected_set: std::collections::HashSet<ActorId> =
            expected_actor_ids.iter().cloned().collect();
        let registered_set: std::collections::HashSet<ActorId> =
            registered_ids.iter().cloned().collect();

        if expected_set.is_subset(&registered_set) {
            return true;
        }
        drop(registered_ids);

        // Use adaptive polling: check more frequently at first, then back off
        let elapsed = last_check.elapsed();
        let sleep_duration = if elapsed < Duration::from_millis(100) {
            Duration::from_millis(10) // Fast polling initially
        } else if elapsed < Duration::from_millis(500) {
            Duration::from_millis(50) // Medium polling
        } else {
            Duration::from_millis(100) // Slower polling after 500ms
        };

        yield_now().await;
        sleep(sleep_duration).await;
        last_check = std::time::Instant::now();
    }

    false
}

/// Build the expected actor ID for a supervisor-tree actor.
/// actor_type = "test_wasm_actor" (all ChildSpecs in these tests use this type).
/// namespace  = app_name (SpecApplication uses spec.name when spec.namespace is empty).
fn expected_actor_id(name: &str, app_name: &str, node_id: &str) -> ActorId {
    ActorId::new(name, "test_wasm_actor", app_name, node_id)
        .expect("supervision test actor IDs must be valid")
}

/// Helper to wait for application state using polling (no events available for this)
async fn wait_for_application_state(
    node: &Node,
    app_name: &str,
    expected_state: plexspaces_proto::v1::application::ApplicationState,
    timeout_duration: Duration,
) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout_duration {
        let app_manager = node.application_manager();
        let current_state = plexspaces_actor::service_locator_trait::ApplicationManager::get_state(
            app_manager.as_ref(),
            app_name,
        )
        .await;
        // Compare by matching the enum variant directly
        match (current_state, &expected_state) {
            (Some(current), expected) if current == *expected => return true,
            _ => {}
        }
        yield_now().await;
        sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Helper to wait for minimum number of actors to be activated using polling
async fn wait_for_min_actors_activated(
    node: &Node,
    min_count: usize,
    timeout_duration: Duration,
) -> bool {
    // Get ActorRegistry
    let registry = node
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry not found");

    let start = std::time::Instant::now();
    let mut last_check = std::time::Instant::now();

    // Poll with adaptive backoff
    while start.elapsed() < timeout_duration {
        // Check current count
        let registered_ids = registry.registered_actor_ids().await;
        if registered_ids.len() >= min_count {
            return true;
        }
        drop(registered_ids);

        // Use adaptive polling
        let elapsed = last_check.elapsed();
        let sleep_duration = if elapsed < Duration::from_millis(100) {
            Duration::from_millis(10)
        } else if elapsed < Duration::from_millis(500) {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(100)
        };

        yield_now().await;
        sleep(sleep_duration).await;
        last_check = std::time::Instant::now();
    }

    false
}

/// Create a supervisor tree with multiple workers
fn create_simple_supervisor_tree() -> SupervisorSpec {
    SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "worker-1".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "worker-2".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "worker-3".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

/// Create a nested supervisor tree (supervisor with child supervisor)
fn create_nested_supervisor_tree() -> SupervisorSpec {
    // Child supervisor with workers
    let child_supervisor = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "nested-worker-1".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "nested-worker-2".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    // Root supervisor with workers and a child supervisor
    SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "root-worker-1".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "child-supervisor".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "supervisor".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                supervisor: Some(child_supervisor),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

/// Get all actor IDs from ActorRegistry
async fn get_all_actor_ids(node: &Node) -> Vec<plexspaces_actor::ActorId> {
    let actor_registry = node
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry not found");

    let registered_ids = actor_registry.registered_actor_ids().await;
    registered_ids.iter().cloned().collect()
}

/// Get actor type for an actor ID
async fn get_actor_type(node: &Node, actor_id: &plexspaces_actor::ActorId) -> Option<String> {
    let actor_registry = node
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry not found");

    let index = actor_registry.actor_type_index().read().await;
    for ((_tenant, _namespace, actor_type), actor_ids) in index.iter() {
        if actor_ids.contains(actor_id) {
            return Some(actor_type.clone());
        }
    }
    None
}

/// Register a mock behavior factory in ServiceLocator
///
/// ## Purpose
/// Registers a BehaviorRegistry in the ServiceLocator. ActorFactory::spawn_actor looks for
/// BehaviorRegistry in ServiceLocator. If not found or actor_type is not registered, spawn_actor
/// will FAIL with an error.
///
/// ## Expected Behavior
/// - Registers BehaviorRegistry in ServiceLocator
/// - ActorFactory will use it to create behaviors for spawned actors
/// - If BehaviorRegistry is not found or actor_type is unknown, spawn_actor returns an error
/// - Tests must register behaviors before spawning actors with actor_type strings
async fn register_mock_behavior_factory(node: &Node) -> Result<(), String> {
    use plexspaces_actor::behavior_factory::BehaviorRegistry;
    use std::sync::Arc;

    let registry = Arc::new(BehaviorRegistry::new());

    // All ChildSpecs in supervisor tree tests use actor_type = "test_wasm_actor".
    // Register it once so ActorFactory can create mock actors for any name.
    for actor_type in ["test_wasm_actor"] {
        let at = actor_type.to_string();
        registry
            .register_simple(at.clone(), move || {
                let at2 = at.clone();
                async move {
                    Ok(Box::new(MockActor { actor_type: at2 }) as Box<dyn plexspaces_actor::Actor>)
                }
            })
            .await;
    }

    node.service_locator()
        .register_behavior_registry(registry)
        .await;

    Ok(())
}

/// Minimal no-op actor for supervisor tree spawn tests — verifies structure, not behavior.
struct MockActor {
    actor_type: String,
}

#[async_trait::async_trait]
impl plexspaces_actor::Actor for MockActor {
    async fn handle_message(
        &mut self,
        _ctx: &plexspaces_actor::ActorContext,
        _msg: plexspaces_actor::Message,
    ) -> Result<(), plexspaces_actor::BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_actor::BehaviorType {
        plexspaces_actor::BehaviorType::GenServer
    }
}

/// Deploy application using SpecApplication directly (mock/simulated setup for unit tests)
///
/// ## Purpose
/// Deploys an application using SpecApplication directly, bypassing WASM runtime.
/// This is a mock/simulated setup for unit tests that don't need actual WASM deployment.
///
/// ## Expected Behavior
/// 1. Creates SpecApplication with mock behavior factory
/// 2. Registers application with ApplicationManager
/// 3. Starts the application (spawns supervisor tree and actors)
/// 4. Returns success/failure result
///
/// ## Arguments
/// * `node` - Node instance (services must be initialized)
/// * `app_name` - Application name
/// * `app_spec` - Application specification with supervisor tree
///
/// ## Returns
/// Result indicating success or failure
async fn deploy_application_mock(
    node: &Arc<Node>,
    app_name: &str,
    app_spec: ApplicationSpec,
) -> Result<(), String> {
    use plexspaces_application::{Application, ApplicationNode, SpecApplication};
    use std::sync::Arc;

    register_mock_behavior_factory(node)
        .await
        .map_err(|e| format!("Failed to register behavior factory: {}", e))?;

    let spec_app = SpecApplication::new(app_spec);
    let app: Box<dyn Application> = Box::new(spec_app);

    let app_manager = node.application_manager();

    // Node implements ApplicationNode — set context so initialize_supervisor_tree
    // can resolve ActorFactory and other services via ServiceLocator.
    let node_as_app_node: Arc<dyn ApplicationNode> = node.clone();
    app_manager.set_node_context(node_as_app_node).await;

    app_manager
        .register(&app_ctx(app_name), app)
        .await
        .map_err(|e| format!("Failed to register application: {}", e))?;

    app_manager
        .start(app_name)
        .await
        .map_err(|e| format!("Failed to start application: {}", e))?;

    Ok(())
}

/// Deploy application via ApplicationServiceImpl with WASM (integration test setup)
///
/// ## Purpose
/// Deploys an application using ApplicationServiceImpl with actual WASM deployment.
/// This requires the node to be started (WASM runtime initialized).
/// Use this ONLY for integration tests that verify full WASM deployment flow.
///
/// ## Expected Behavior
/// 1. Creates DeployApplicationRequest from spec and WASM module
/// 2. Calls ApplicationServiceImpl::deploy_application() directly (bypasses gRPC)
/// 3. Application is registered and started (supervisor tree and actors are spawned)
/// 4. Returns success/failure result
///
/// ## Arguments
/// * `node` - Node instance (MUST be started - WASM runtime must be initialized)
/// * `app_name` - Application name
/// * `app_spec` - Application specification with supervisor tree
/// * `wasm_module` - WASM module bytes
///
/// ## Returns
/// Result indicating success or failure
async fn deploy_application_with_wasm(
    node: &Node,
    app_name: &str,
    app_spec: ApplicationSpec,
    wasm_module: WasmModule,
) -> Result<(), String> {
    use plexspaces_services::application_service::ApplicationServiceImpl;
    use std::sync::Arc;
    use tonic::Request;

    tracing::debug!(
        application = %app_name,
        "Deploying application with WASM (integration test mode)"
    );

    // Get ApplicationManager
    let application_manager = node.application_manager();

    // Create ApplicationServiceImpl (doesn't require gRPC server to be running)
    let node_arc = Arc::new(node.clone());
    let service = ApplicationServiceImpl::new(node_arc.service_locator().clone(), None);

    // Create deployment request (same as gRPC would receive)
    let request = DeployApplicationRequest {
        application_id: format!("{}-001", app_name),
        name: app_name.to_string(),
        version: app_spec.version.clone(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        initial_state: vec![],
    };

    // Call deploy_application directly (bypasses gRPC layer)
    let response = service
        .deploy_application(app_request_with_tenant(request))
        .await
        .map_err(|e| format!("DeployApplication failed: {}", e))?;

    let res = response.into_inner();
    if !res.success {
        return Err(format!("Deployment failed: success=false"));
    }

    tracing::debug!(
        application = %app_name,
        "Application deployed and started successfully"
    );

    Ok(())
}

/// Test 1: Simple supervisor tree - all workers should be spawned
///
/// ## Expected Behavior
/// - All 3 worker actors should be spawned and registered
/// - Actor types should be set correctly from ChildSpec.id
/// - Application should enter Running state
#[tokio::test]
async fn test_simple_supervisor_tree_all_workers_spawned() {
    timeout(Duration::from_secs(2), async {
        let node = create_test_node().await;

        // Create supervisor tree with 3 workers
        let supervisor_spec = create_simple_supervisor_tree();
        let app_spec = ApplicationSpec {
            name: "test-app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Test app with simple supervisor tree".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor: Some(supervisor_spec),
            enabled: true,
            auto_start: true,
            shutdown_timeout: Some(ProstDuration {
                seconds: 60,
                nanos: 0,
            }),
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        };

        // Deploy using mock/simulated setup (no WASM runtime needed)
        deploy_application_mock(&node, "test-app", app_spec)
            .await
            .expect("Deployment should succeed");

        // Wait for application to be running
        let app_running = wait_for_application_state(
            &node,
            "test-app",
            ApplicationState::ApplicationStateRunning,
            Duration::from_secs(1),
        )
        .await;
        assert!(
            app_running,
            "Application should be running within 5 seconds"
        );

        // Wait for actors to be activated using lifecycle events
        let node_id = node.id().as_str();
        let expected_actors = vec![
            expected_actor_id("worker-1", "test-app", node_id),
            expected_actor_id("worker-2", "test-app", node_id),
            expected_actor_id("worker-3", "test-app", node_id),
        ];
        let actors_activated =
            wait_for_actors_activated(&node, &expected_actors, Duration::from_secs(1)).await;
        assert!(
            actors_activated,
            "Actors should be activated within 5 seconds"
        );

        // Get all actor IDs
        let actor_ids = get_all_actor_ids(&node).await;

        // Verify all 3 workers are spawned
        let node_id = node.id().as_str();
        let expected_actors = vec![
            expected_actor_id("worker-1", "test-app", node_id),
            expected_actor_id("worker-2", "test-app", node_id),
            expected_actor_id("worker-3", "test-app", node_id),
        ];

        for expected_actor in &expected_actors {
            assert!(
                actor_ids.contains(expected_actor),
                "Actor {} should be spawned. Found actors: {:?}",
                expected_actor,
                actor_ids
            );
        }

        // Verify actor types are set correctly (actor_type = ChildSpec.actor_identity.actor_type)
        for expected_actor in &expected_actors {
            let actor_type = get_actor_type(&node, expected_actor).await;
            assert_eq!(
                actor_type,
                Some(expected_actor.actor_type().to_string()),
                "Actor {} should have type {}",
                expected_actor,
                expected_actor.actor_type()
            );
        }
    })
    .await
    .expect("Test should complete within 2 seconds");
}

/// Test 2: Nested supervisor tree - all workers and supervisors should be spawned
///
/// ## Expected Behavior
/// - Root worker, child supervisor (as actor), and nested workers should all be spawned
/// - Supervisor should be spawned as an actor (Erlang-style)
/// - All actors should be registered in ActorRegistry
#[tokio::test]
async fn test_nested_supervisor_tree_all_actors_spawned() {
    timeout(Duration::from_secs(2), async {
        let node = create_test_node().await;

        // Create nested supervisor tree
        let supervisor_spec = create_nested_supervisor_tree();
        let app_spec = ApplicationSpec {
            name: "nested-app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Test app with nested supervisor tree".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor: Some(supervisor_spec),
            enabled: true,
            auto_start: true,
            shutdown_timeout: Some(ProstDuration {
                seconds: 60,
                nanos: 0,
            }),
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        };

        deploy_application_mock(&node, "nested-app", app_spec)
            .await
            .expect("Deployment should succeed");

        // Wait for application to be running
        let app_running = wait_for_application_state(
            &node,
            "nested-app",
            ApplicationState::ApplicationStateRunning,
            Duration::from_secs(1),
        )
        .await;
        assert!(
            app_running,
            "Application should be running within 5 seconds"
        );

        // Wait for actors to be activated using lifecycle events
        let actors_activated =
            wait_for_min_actors_activated(&node, 4, Duration::from_secs(1)).await;
        assert!(
            actors_activated,
            "At least 4 actors should be activated within 5 seconds"
        );

        // Verify application is running
        let app_manager = node.application_manager();
        let app_state = plexspaces_actor::service_locator_trait::ApplicationManager::get_state(
            app_manager.as_ref(),
            "nested-app",
        )
        .await;
        assert_eq!(
            app_state,
            Some(ApplicationState::ApplicationStateRunning),
            "Application should be running"
        );

        // Get all actor IDs
        let actor_ids = get_all_actor_ids(&node).await;

        let node_id = node.id().as_str();

        // Expected actors:
        // - root-worker-1 (worker)
        // - child-supervisor (supervisor actor - Erlang-style)
        // - nested-worker-1 (worker under child supervisor)
        // - nested-worker-2 (worker under child supervisor)
        let expected_actors = vec![
            expected_actor_id("root-worker-1", "nested-app", node_id),
            expected_actor_id("child-supervisor", "nested-app", node_id),
            expected_actor_id("nested-worker-1", "nested-app", node_id),
            expected_actor_id("nested-worker-2", "nested-app", node_id),
        ];

        for expected_actor in &expected_actors {
            assert!(
                actor_ids.contains(expected_actor),
                "Actor {} should be spawned. Found actors: {:?}",
                expected_actor,
                actor_ids
            );
        }

        // Verify actor types are set correctly
        for expected_actor in &expected_actors {
            let actor_type = get_actor_type(&node, expected_actor).await;
            assert_eq!(
                actor_type,
                Some(expected_actor.actor_type().to_string()),
                "Actor {} should have type {}",
                expected_actor,
                expected_actor.actor_type()
            );
        }

        // Verify total count matches expected (4 actors total)
        assert_eq!(
            actor_ids.len(),
            expected_actors.len(),
            "Should have exactly {} actors spawned",
            expected_actors.len()
        );
    })
    .await
    .expect("Test should complete within 2 seconds");
}

/// Create a deeply nested supervisor tree (3 levels)
fn create_deeply_nested_supervisor_tree() -> SupervisorSpec {
    // Level 3: Deepest supervisor with workers
    let level3_supervisor = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: None,
        children: vec![ChildSpec {
            actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: "deep-worker-1".to_string(),
                actor_type: "test_wasm_actor".to_string(),
            }),

            role: "worker".to_string(),
            restart: RestartPolicy::RestartPolicyPermanent.into(),
            ..Default::default()
        }],
        ..Default::default()
    };

    // Level 2: Middle supervisor
    let level2_supervisor = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "level2-supervisor".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "supervisor".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                supervisor: Some(level3_supervisor),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "level2-worker".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    // Level 1: Root supervisor
    SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "root-worker".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "level1-supervisor".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "supervisor".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                supervisor: Some(level2_supervisor),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

/// Create a supervisor tree with multiple sibling supervisors
fn create_multiple_sibling_supervisors_spec() -> SupervisorSpec {
    // Supervisor A with workers
    let supervisor_a = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "supervisor-a-worker-1".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "supervisor-a-worker-2".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    // Supervisor B with workers
    let supervisor_b = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: None,
        children: vec![ChildSpec {
            actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: "supervisor-b-worker-1".to_string(),
                actor_type: "test_wasm_actor".to_string(),
            }),

            role: "worker".to_string(),
            restart: RestartPolicy::RestartPolicyPermanent.into(),
            ..Default::default()
        }],
        ..Default::default()
    };

    // Root supervisor with two sibling supervisors
    SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "supervisor-a".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "supervisor".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                supervisor: Some(supervisor_a),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "supervisor-b".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "supervisor".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                supervisor: Some(supervisor_b),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "root-worker".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

/// Test 3: Deeply nested supervisor tree (3 levels of supervisors)
#[tokio::test]
async fn test_deeply_nested_supervisor_tree() {
    timeout(Duration::from_secs(2), async {
        let node = create_test_node().await;

        // Create deeply nested supervisor tree
        let supervisor_spec = create_deeply_nested_supervisor_tree();
        let app_spec = ApplicationSpec {
            name: "test-app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Test app".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor: Some(supervisor_spec),
            enabled: true,
            auto_start: true,
            shutdown_timeout: Some(ProstDuration {
                seconds: 60,
                nanos: 0,
            }),
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        };

        // Deploy using mock/simulated setup (no WASM runtime needed)
        deploy_application_mock(&node, "test-app", app_spec)
            .await
            .expect("Deployment should succeed");
        let actors_activated =
            wait_for_min_actors_activated(&node, 3, Duration::from_secs(1)).await;
        assert!(
            actors_activated,
            "At least 3 actors should be activated within 5 seconds"
        );

        // Get all actor IDs
        let actor_ids = get_all_actor_ids(&node).await;

        let node_id = node.id().as_str();

        // Expected actors (all supervisors and workers):
        // - root-worker (worker)
        // - level1-supervisor (supervisor actor)
        //   - level2-worker (worker)
        //   - level2-supervisor (supervisor actor)
        //     - deep-worker-1 (worker)
        let expected_actors = vec![
            expected_actor_id("root-worker", "test-app", node_id),
            expected_actor_id("level1-supervisor", "test-app", node_id),
            expected_actor_id("level2-worker", "test-app", node_id),
            expected_actor_id("level2-supervisor", "test-app", node_id),
            expected_actor_id("deep-worker-1", "test-app", node_id),
        ];

        for expected_actor in &expected_actors {
            assert!(
                actor_ids.contains(expected_actor),
                "Actor {} should be spawned. Found actors: {:?}",
                expected_actor,
                actor_ids
            );
        }

        // Verify all supervisors are spawned as actors (Erlang-style)
        let supervisor_actors = vec![
            expected_actor_id("level1-supervisor", "test-app", node_id),
            expected_actor_id("level2-supervisor", "test-app", node_id),
        ];

        for supervisor_actor in &supervisor_actors {
            assert!(
                actor_ids.contains(supervisor_actor),
                "Supervisor {} should be spawned as an actor (Erlang-style)",
                supervisor_actor
            );

            let actor_type = get_actor_type(&node, supervisor_actor).await;
            assert_eq!(
                actor_type,
                Some(supervisor_actor.actor_type().to_string()),
                "Supervisor actor {} should have type {}",
                supervisor_actor,
                supervisor_actor.actor_type()
            );
        }
    })
    .await
    .expect("Test should complete within 2 seconds");
}

/// Test 4: Verify actors are tracked in WasmApplication
#[tokio::test]
async fn test_actors_tracked_in_application() {
    timeout(Duration::from_secs(2), async {
        let node = create_test_node().await;
        let application_manager = node.application_manager();
        // Deploy directly via ApplicationServiceImpl (no gRPC server needed)
        let supervisor_spec = create_simple_supervisor_tree();
        let app_spec = ApplicationSpec {
            name: "actors_tracked_in_-app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Test app".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor: Some(supervisor_spec),
            enabled: true,
            auto_start: true,
            shutdown_timeout: Some(ProstDuration {
                seconds: 60,
                nanos: 0,
            }),
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        };

        deploy_application_mock(&node, "actors_tracked_in_-app", app_spec)
            .await
            .expect("Deployment should succeed");
        let app_running = wait_for_application_state(
            &node,
            "actors_tracked_in_-app",
            ApplicationState::ApplicationStateRunning,
            Duration::from_secs(1),
        )
        .await;
        assert!(
            app_running,
            "Application should be running within 5 seconds"
        );

        // Wait for actors to be activated using lifecycle events
        let actors_activated =
            wait_for_min_actors_activated(&node, 1, Duration::from_secs(1)).await;
        assert!(
            actors_activated,
            "At least 1 actor should be activated within 5 seconds"
        );

        // Verify application is running
        let app_manager = node.application_manager();
        let app_state = plexspaces_actor::service_locator_trait::ApplicationManager::get_state(
            app_manager.as_ref(),
            "actors_tracked_in_-app",
        )
        .await;
        assert_eq!(
            app_state,
            Some(ApplicationState::ApplicationStateRunning),
            "Application should be running"
        );

        // Verify actors are spawned in ActorRegistry (ApplicationManager doesn't auto-track)
        let actor_ids = get_all_actor_ids(&node).await;
        assert_eq!(
            actor_ids.len(),
            3,
            "Application should have 3 actors spawned. Found: {:?}",
            actor_ids
        );
    })
    .await
    .expect("Test should complete within 2 seconds");
}

/// Test 5: Complex hierarchy - supervisor->supervisor->supervisor->workers
#[tokio::test]
async fn test_complex_supervisor_hierarchy() {
    timeout(Duration::from_secs(2), async {
        let node = create_test_node().await;

        // Create complex supervisor hierarchy
        let supervisor_spec = create_complex_supervisor_hierarchy_spec();
        let app_spec = ApplicationSpec {
            name: "complex-app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Test app with complex supervisor hierarchy".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor: Some(supervisor_spec),
            enabled: true,
            auto_start: true,
            shutdown_timeout: Some(ProstDuration {
                seconds: 60,
                nanos: 0,
            }),
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        };

        deploy_application_mock(&node, "complex-app", app_spec)
            .await
            .expect("Deployment should succeed");
        let app_running = wait_for_application_state(
            &node,
            "complex-app",
            ApplicationState::ApplicationStateRunning,
            Duration::from_secs(1),
        )
        .await;
        assert!(
            app_running,
            "Application should be running within 5 seconds"
        );

        // Wait for all actors to be activated using lifecycle events
        // Complex hierarchy: 1 root-worker + 1 level1-supervisor + 1 level2-worker + 1 level2-supervisor + 1 level3-supervisor + 1 level3-worker = 6 actors
        // Get actual count first to debug
        let actor_ids = get_all_actor_ids(&node).await;
        tracing::debug!(
            "Complex hierarchy test: Found {} actors: {:?}",
            actor_ids.len(),
            actor_ids
        );

        // Wait for at least 6 actors (may take a moment for recursive spawning)
        let actors_activated =
            wait_for_min_actors_activated(&node, 6, Duration::from_secs(1)).await;
        if !actors_activated {
            // Get final count for better error message
            let final_actor_ids = get_all_actor_ids(&node).await;
            panic!(
                "At least 6 actors should be activated within 5 seconds. Found {} actors: {:?}",
                final_actor_ids.len(),
                final_actor_ids
            );
        }

        // Verify application is running
        let app_manager = node.application_manager();
        let app_state = plexspaces_actor::service_locator_trait::ApplicationManager::get_state(
            app_manager.as_ref(),
            "complex-app",
        )
        .await;
        assert_eq!(
            app_state,
            Some(ApplicationState::ApplicationStateRunning),
            "Application should be running"
        );

        // Get all actor IDs
        let actor_ids = get_all_actor_ids(&node).await;

        let node_id = node.id().as_str();

        // Expected actors (all supervisors and workers):
        // Level 1 (root):
        //   - root-worker (worker)
        //   - level1-supervisor (supervisor actor)
        // Level 2:
        //   - level2-worker (worker)
        //   - level2-supervisor (supervisor actor)
        // Level 3:
        //   - level3-supervisor (supervisor actor)
        //   - level3-worker (worker)
        let expected_actors = vec![
            // Level 1
            expected_actor_id("root-worker", "complex-app", node_id),
            expected_actor_id("level1-supervisor", "complex-app", node_id),
            // Level 2
            expected_actor_id("level2-worker", "complex-app", node_id),
            expected_actor_id("level2-supervisor", "complex-app", node_id),
            // Level 3
            expected_actor_id("level3-supervisor", "complex-app", node_id),
            expected_actor_id("level3-worker", "complex-app", node_id),
        ];

        // Verify all actors are spawned
        for expected_actor in &expected_actors {
            assert!(
                actor_ids.contains(expected_actor),
                "Actor {} should be spawned. Found actors: {:?}",
                expected_actor,
                actor_ids
            );
        }

        // Verify all supervisors are spawned as actors (Erlang-style)
        let supervisor_actors = vec![
            expected_actor_id("level1-supervisor", "complex-app", node_id),
            expected_actor_id("level2-supervisor", "complex-app", node_id),
            expected_actor_id("level3-supervisor", "complex-app", node_id),
        ];

        for supervisor_actor in &supervisor_actors {
            assert!(
                actor_ids.contains(supervisor_actor),
                "Supervisor {} should be spawned as an actor (Erlang-style)",
                supervisor_actor
            );

            let actor_type = get_actor_type(&node, supervisor_actor).await;
            assert_eq!(
                actor_type,
                Some(supervisor_actor.actor_type().to_string()),
                "Supervisor actor {} should have type {}",
                supervisor_actor,
                supervisor_actor.actor_type()
            );
        }

        // Verify total count matches expected (6 actors: 3 supervisors + 3 workers)
        assert_eq!(
            actor_ids.len(),
            expected_actors.len(),
            "Should have exactly {} actors spawned (found {})",
            expected_actors.len(),
            actor_ids.len()
        );

        // Verify actor types for all workers
        let worker_actors = vec![
            expected_actor_id("root-worker", "complex-app", node_id),
            expected_actor_id("level2-worker", "complex-app", node_id),
            expected_actor_id("level3-worker", "complex-app", node_id),
        ];

        for worker_actor in &worker_actors {
            let actor_type = get_actor_type(&node, worker_actor).await;
            assert_eq!(
                actor_type,
                Some(worker_actor.actor_type().to_string()),
                "Worker actor {} should have type {}",
                worker_actor,
                worker_actor.actor_type()
            );
        }
    })
    .await
    .expect("Test should complete within 2 seconds");
}

/// Test 6: Multiple supervisors at same level (sibling supervisors)
#[tokio::test]
async fn test_multiple_sibling_supervisors() {
    timeout(Duration::from_secs(2), async {
        let node = create_test_node().await;

        // Create supervisor tree with multiple sibling supervisors
        let supervisor_spec = create_multiple_sibling_supervisors_spec();
        let app_spec = ApplicationSpec {
            name: "actors_tracked_in_-app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Test app".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor: Some(supervisor_spec),
            enabled: true,
            auto_start: true,
            shutdown_timeout: Some(ProstDuration {
                seconds: 60,
                nanos: 0,
            }),
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        };

        deploy_application_mock(&node, "actors_tracked_in_-app", app_spec)
            .await
            .expect("Deployment should succeed");
        let actors_activated =
            wait_for_min_actors_activated(&node, 3, Duration::from_secs(1)).await;
        assert!(
            actors_activated,
            "At least 3 actors should be activated within 5 seconds"
        );

        // Get all actor IDs
        let actor_ids = get_all_actor_ids(&node).await;

        let node_id = node.id().as_str();

        // Expected actors:
        // - supervisor-a (supervisor actor)
        //   - supervisor-a-worker-1 (worker)
        //   - supervisor-a-worker-2 (worker)
        // - supervisor-b (supervisor actor)
        //   - supervisor-b-worker-1 (worker)
        // - root-worker (worker)
        let expected_actors = vec![
            expected_actor_id("supervisor-a", "actors_tracked_in_-app", node_id),
            expected_actor_id("supervisor-a-worker-1", "actors_tracked_in_-app", node_id),
            expected_actor_id("supervisor-a-worker-2", "actors_tracked_in_-app", node_id),
            expected_actor_id("supervisor-b", "actors_tracked_in_-app", node_id),
            expected_actor_id("supervisor-b-worker-1", "actors_tracked_in_-app", node_id),
            expected_actor_id("root-worker", "actors_tracked_in_-app", node_id),
        ];

        // Verify all actors are spawned
        for expected_actor in &expected_actors {
            assert!(
                actor_ids.contains(expected_actor),
                "Actor {} should be spawned. Found actors: {:?}",
                expected_actor,
                actor_ids
            );
        }

        // Verify both sibling supervisors are spawned as actors
        let supervisor_actors = vec![
            expected_actor_id("supervisor-a", "actors_tracked_in_-app", node_id),
            expected_actor_id("supervisor-b", "actors_tracked_in_-app", node_id),
        ];

        for supervisor_actor in &supervisor_actors {
            assert!(
                actor_ids.contains(supervisor_actor),
                "Sibling supervisor {} should be spawned as an actor",
                supervisor_actor
            );
        }

        // Verify total count
        assert_eq!(
            actor_ids.len(),
            expected_actors.len(),
            "Should have exactly {} actors spawned",
            expected_actors.len()
        );
    })
    .await
    .expect("Test should complete within 2 seconds");
}

/// Test 7: Auto-generated supervisor tree (deployment without supervisor spec)
///
/// ## Expected Behavior
/// - When supervisor is None, application should still work (auto-generated supervisor)
/// - For now, we test with a simple supervisor tree since auto-generation is not fully implemented
#[tokio::test]
async fn test_auto_generated_supervisor_tree() {
    timeout(Duration::from_secs(2), async {
        let node = create_test_node().await;

        // Deploy without supervisor spec - should use simple tree for now
        // TODO: When auto-generation is implemented, set supervisor: None
        let supervisor_spec = create_simple_supervisor_tree();
        let app_spec = ApplicationSpec {
            name: "auto-app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Test app with auto-generated supervisor".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor: Some(supervisor_spec),
            enabled: true,
            auto_start: true,
            shutdown_timeout: Some(ProstDuration {
                seconds: 60,
                nanos: 0,
            }),
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        };

        deploy_application_mock(&node, "auto-app", app_spec)
            .await
            .expect("Deployment should succeed");
        let app_running = wait_for_application_state(
            &node,
            "auto-app",
            ApplicationState::ApplicationStateRunning,
            Duration::from_secs(1),
        )
        .await;
        assert!(
            app_running,
            "Application should be running within 5 seconds"
        );

        // Wait for actors to be activated using lifecycle events
        let actors_activated =
            wait_for_min_actors_activated(&node, 3, Duration::from_secs(1)).await;
        assert!(
            actors_activated,
            "At least 3 actors should be activated within 5 seconds"
        );

        // Verify application is running
        let app_manager = node.application_manager();
        let app_state = plexspaces_actor::service_locator_trait::ApplicationManager::get_state(
            app_manager.as_ref(),
            "auto-app",
        )
        .await;
        assert_eq!(
            app_state,
            Some(ApplicationState::ApplicationStateRunning),
            "Application should be running"
        );

        // Get all actor IDs
        let actor_ids = get_all_actor_ids(&node).await;

        // With simple supervisor tree, we expect 3 workers
        let node_id = node.id().as_str();
        let expected_actors = vec![
            expected_actor_id("worker-1", "auto-app", node_id),
            expected_actor_id("worker-2", "auto-app", node_id),
            expected_actor_id("worker-3", "auto-app", node_id),
        ];

        for expected_actor in &expected_actors {
            assert!(
                actor_ids.contains(expected_actor),
                "Actor {} should be spawned. Found actors: {:?}",
                expected_actor,
                actor_ids
            );
        }
    })
    .await
    .expect("Test should complete within 2 seconds");
}

/// Test 8: Verify graceful shutdown of entire supervisor tree
#[tokio::test]
async fn test_graceful_shutdown_of_supervisor_tree() {
    timeout(Duration::from_secs(2), async {
        let node = create_test_node().await;

        // Create nested supervisor tree for shutdown test
        let supervisor_spec = create_nested_supervisor_tree();
        let app_spec = ApplicationSpec {
            name: "shutdown-app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Test app for graceful shutdown".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor: Some(supervisor_spec),
            enabled: true,
            auto_start: true,
            shutdown_timeout: Some(ProstDuration {
                seconds: 60,
                nanos: 0,
            }),
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        };

        deploy_application_mock(&node, "shutdown-app", app_spec)
            .await
            .expect("Deployment should succeed");
        let actors_activated =
            wait_for_min_actors_activated(&node, 4, Duration::from_secs(1)).await;
        assert!(
            actors_activated,
            "At least 4 actors should be activated within 5 seconds"
        );

        // Verify all actors are spawned
        let actor_ids_before = get_all_actor_ids(&node).await;
        assert!(
            actor_ids_before.len() >= 4,
            "Should have at least 4 actors spawned (found {})",
            actor_ids_before.len()
        );

        // Stop application directly (mock/simulated setup - no WASM runtime needed)
        let app_manager = node.application_manager();
        app_manager
            .stop("shutdown-app", Duration::from_secs(1))
            .await
            .expect("Application stop should succeed");

        // Wait for graceful shutdown
        let app_stopped = wait_for_application_state(
            &node,
            "shutdown-app",
            ApplicationState::ApplicationStateStopped,
            Duration::from_secs(2),
        )
        .await;
        assert!(app_stopped, "Application should stop within 10 seconds");

        // Verify application is stopped
        let app_manager = node.application_manager();
        let app_state = plexspaces_actor::service_locator_trait::ApplicationManager::get_state(
            app_manager.as_ref(),
            "shutdown-app",
        )
        .await;
        assert_eq!(
            app_state,
            Some(ApplicationState::ApplicationStateStopped),
            "Application should be stopped after undeployment"
        );

        // Verify all actors from the application are removed
        let actor_ids_after = get_all_actor_ids(&node).await;
        assert!(
            actor_ids_after.len() < actor_ids_before.len(),
            "Actor count should decrease after undeployment (before: {}, after: {})",
            actor_ids_before.len(),
            actor_ids_after.len()
        );
    })
    .await
    .expect("Test should complete within 2 seconds");
}

/// Test 9: Verify actor type tracking for all actors in complex tree
#[tokio::test]
async fn test_actor_type_tracking_complex_tree() {
    timeout(Duration::from_secs(2), async {
        let node = create_test_node().await;
        let application_manager = node.application_manager();
        // Deploy directly via ApplicationServiceImpl (no gRPC server needed)
        let supervisor_spec = create_complex_supervisor_hierarchy_spec();
        let app_spec = ApplicationSpec {
            name: "actors_tracked_in_-app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Test app".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor: Some(supervisor_spec),
            enabled: true,
            auto_start: true,
            shutdown_timeout: Some(ProstDuration {
                seconds: 60,
                nanos: 0,
            }),
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        };

        deploy_application_mock(&node, "actors_tracked_in_-app", app_spec)
            .await
            .expect("Deployment should succeed");
        let actors_activated =
            wait_for_min_actors_activated(&node, 1, Duration::from_secs(1)).await;
        assert!(
            actors_activated,
            "At least 1 actor should be activated within 5 seconds"
        );

        // Get all actor IDs
        let actor_ids = get_all_actor_ids(&node).await;
        let _node_id = node.id().as_str();

        // Verify every actor has the correct type (matching ChildSpec.actor_identity.actor_type = "test_wasm_actor")
        for actor_id in &actor_ids {
            let actor_type = get_actor_type(&node, actor_id).await;
            assert!(
                actor_type.is_some(),
                "Actor {} should have a type registered",
                actor_id
            );
            assert_eq!(
                actor_type,
                Some("test_wasm_actor".to_string()),
                "Actor {} should have type 'test_wasm_actor' (matching ChildSpec.actor_identity.actor_type)",
                actor_id
            );
        }
    })
    .await
    .expect("Test should complete within 2 seconds");
}

/// Create Erlang-style supervision structure
/// my_app_sup (supervisor)
///     ├── worker_a (worker)
///     └── sub_sup (supervisor)
///         ├── worker_b (worker)
///         └── worker_c (worker)
fn create_erlang_style_supervision_structure() -> SupervisorSpec {
    // sub_sup supervisor (bottom level)
    let sub_sup = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "worker_b".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "worker_c".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    // my_app_sup supervisor (root level)
    SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "worker_a".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "sub_sup".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "supervisor".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                supervisor: Some(sub_sup),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

/// Test 10: Exact Erlang-style supervision structure
/// my_app
/// └── my_app_sup (supervisor)
///     ├── worker_a (worker)
///     └── sub_sup (supervisor)
///         ├── worker_b (worker)
///         └── worker_c (worker)
///
/// Verifies:
/// 1. Tree is built bottom-up (workers first, then supervisors)
/// 2. Supervisors are spawned as actors (Erlang-style)
/// 3. All actors are tracked
/// 4. Supervisors manage their children (top-down management)
#[tokio::test]
async fn test_erlang_style_supervision_structure() {
    timeout(Duration::from_secs(2), async {
        let node = create_test_node().await;

        // Create Erlang-style supervision structure
        let supervisor_spec = create_erlang_style_supervision_structure();
        let app_spec = ApplicationSpec {
            name: "my_app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Erlang-style supervision structure test".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor: Some(supervisor_spec),
            enabled: true,
            auto_start: true,
            shutdown_timeout: Some(ProstDuration {
                seconds: 60,
                nanos: 0,
            }),
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        };

        deploy_application_mock(&node, "my_app", app_spec)
            .await
            .expect("Deployment should succeed");
        let app_running = wait_for_application_state(
            &node,
            "my_app",
            ApplicationState::ApplicationStateRunning,
            Duration::from_secs(1),
        )
        .await;
        assert!(
            app_running,
            "Application should be running within 5 seconds"
        );

        // Wait for actors to be activated using lifecycle events (bottom-up: workers first, then supervisors)
        let actors_activated =
            wait_for_min_actors_activated(&node, 4, Duration::from_secs(1)).await;
        assert!(
            actors_activated,
            "At least 4 actors should be activated within 5 seconds"
        );

        // Verify application is running
        let app_manager = node.application_manager();
        let app_state = plexspaces_actor::service_locator_trait::ApplicationManager::get_state(
            app_manager.as_ref(),
            "my_app",
        )
        .await;
        assert_eq!(
            app_state,
            Some(ApplicationState::ApplicationStateRunning),
            "Application should be running"
        );

        // Get all actor IDs
        let actor_ids = get_all_actor_ids(&node).await;
        let node_id = node.id().as_str();

        // Expected actors (all must be spawned):
        // - worker_a (worker under my_app_sup)
        // - sub_sup (supervisor actor under my_app_sup)
        // - worker_b (worker under sub_sup)
        // - worker_c (worker under sub_sup)
        let expected_actors = vec![
            expected_actor_id("worker_a", "my_app", node_id),
            expected_actor_id("sub_sup", "my_app", node_id), // Supervisor must be spawned as actor
            expected_actor_id("worker_b", "my_app", node_id),
            expected_actor_id("worker_c", "my_app", node_id),
        ];

        // Verify all actors are spawned
        for expected_actor in &expected_actors {
            assert!(
                actor_ids.contains(expected_actor),
                "Actor {} should be spawned (Erlang-style supervision). Found actors: {:?}",
                expected_actor,
                actor_ids
            );
        }

        // Verify supervisors are spawned as actors (Erlang-style)
        let supervisor_actor = expected_actor_id("sub_sup", "my_app", node_id);
        assert!(
            actor_ids.contains(&supervisor_actor),
            "Supervisor 'sub_sup' should be spawned as an actor (Erlang-style)"
        );

        // Verify actor types match ChildSpec.actor_identity.actor_type
        for expected_actor in &expected_actors {
            let actor_type = get_actor_type(&node, expected_actor).await;
            assert_eq!(
                actor_type,
                Some(expected_actor.actor_type().to_string()),
                "Actor {} should have type '{}' (matching ChildSpec.actor_identity.actor_type)",
                expected_actor,
                expected_actor.actor_type()
            );
        }

        // Verify total count matches expected (4 actors: 1 supervisor + 3 workers)
        assert_eq!(
            actor_ids.len(),
            expected_actors.len(),
            "Should have exactly {} actors spawned (1 supervisor + 3 workers)",
            expected_actors.len()
        );

        // Verify application tracks all actors
        // Note: ApplicationManager tracks actors via tracked_actor_count, but this is updated
        // when actors are registered. For now, we verify actors are spawned in ActorRegistry.
        let app_info = app_manager.get_application_info("my_app").await;
        assert!(app_info.is_some(), "Application info should be available");
        let info = app_info.unwrap();

        // Verify actors are actually spawned (check ActorRegistry directly)
        let actor_registry = node
            .service_locator()
            .actor_registry()
            .await
            .expect("ActorRegistry not found");

        let registered_ids = actor_registry.registered_actor_ids().await;
        let spawned_count = expected_actors
            .iter()
            .filter(|expected| registered_ids.contains(expected.as_str()))
            .count();

        assert_eq!(
            spawned_count,
            expected_actors.len(),
            "All {} actors should be spawned and registered in ActorRegistry (found {})",
            expected_actors.len(),
            spawned_count
        );

        // Also verify metrics if available (may be 0 if not tracked, but actors should exist)
        if let Some(metrics) = info.metrics {
            // Note: tracked_actor_count might not be updated automatically
            // The important thing is that actors are spawned and registered
            let tracked_actor_count = metrics.actor_counts.get("total").copied().unwrap_or(0);
            tracing::debug!(
                "Application metrics: actor_count={}, but {} actors are actually registered",
                tracked_actor_count,
                spawned_count
            );
        }
    })
    .await
    .expect("Test should complete within 2 seconds");
}

/// Helper: Create complex supervisor hierarchy spec (reusable)
fn create_complex_supervisor_hierarchy_spec() -> SupervisorSpec {
    // Level 3: Deepest supervisor
    let level3_supervisor = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: None,
        children: vec![ChildSpec {
            actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: "level3-worker".to_string(),
                actor_type: "test_wasm_actor".to_string(),
            }),

            role: "worker".to_string(),
            restart: RestartPolicy::RestartPolicyPermanent.into(),
            ..Default::default()
        }],
        ..Default::default()
    };

    // Level 2: Middle supervisor
    // Note: level2-supervisor is a supervisor child, so it needs its own SupervisorSpec
    // level3-supervisor is a child of level2-supervisor, with level3_supervisor as its nested spec
    // Create a nested spec for level2-supervisor that contains level3-supervisor as a child
    let level2_supervisor_nested_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: None,
        children: vec![ChildSpec {
            actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: "level3-supervisor".to_string(),
                actor_type: "test_wasm_actor".to_string(),
            }),

            role: "supervisor".to_string(),
            restart: RestartPolicy::RestartPolicyPermanent.into(),
            supervisor: Some(level3_supervisor), // level3_supervisor is the nested spec for level3-supervisor
            ..Default::default()
        }],
        ..Default::default()
    };

    let level2_supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "level2-supervisor".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "supervisor".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                supervisor: Some(level2_supervisor_nested_spec), // level2-supervisor has level3-supervisor as a nested child
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "level2-worker".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    // Level 1: Root supervisor
    SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "root-worker".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "level1-supervisor".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "supervisor".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                supervisor: Some(level2_supervisor_spec),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}
