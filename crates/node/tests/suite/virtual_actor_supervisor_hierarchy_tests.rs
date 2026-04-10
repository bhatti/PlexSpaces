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

//! Unit and integration tests for virtual actors in supervisor hierarchies
//!
//! ## Test Organization
//! - **Unit Tests**: Test virtual actor behavior directly (no WASM, no gRPC server)
//!   - Use `create_test_node()` - no server startup
//!   - Direct actor registration via `get_or_activate_actor_helper`
//!   - Fast, no network dependencies
//!
//! - **Integration Tests**: Test WASM application deployment (minimal)
//!   - Use `create_test_node_with_server()` - with gRPC server
//!   - Test actual application deployment via ApplicationService
//!   - Slower, requires WASM runtime

use async_trait::async_trait;
use plexspaces_actor::{Actor, ActorBuilder};
use plexspaces_behavior::GenServer;
use plexspaces_core::Message;
use plexspaces_core::{service_names, ActorRegistry, RequestContext, ServiceLocator};
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_journaling::VirtualActorFacet;
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, ApplicationSpec, ApplicationType, ChildSpec,
    ChildType, DeployApplicationRequest, RestartPolicy, ShutdownStrategy, SupervisionStrategy,
    SupervisorSpec,
};
use plexspaces_proto::v1::common::Facet;
use plexspaces_proto::wasm::v1::WasmModule;
use plexspaces_services::application_service::ApplicationServiceImpl;
use prost_types::Duration as ProstDuration;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task::yield_now;
use tokio::time::{timeout, Duration};
use tonic::Request;
use wat;

use super::test_helpers::{
    app_request_with_tenant, get_or_activate_actor_helper, lookup_actor_ref, test_runtime_actor_id,
};

/// Helper to create a test message
fn create_test_message(payload: Vec<u8>) -> plexspaces_core::Message {
    plexspaces_core::Message {
        id: ulid::Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

// ============================================================================
// TEST ACTOR BEHAVIOR
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
enum TestMessage {
    Ping,
    Pong(String),
}

struct TestActor;

#[async_trait]
impl ActorTrait for TestActor {
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
impl GenServer for TestActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let test_msg: TestMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        let reply_msg = match test_msg {
            TestMessage::Ping => create_test_message(
                serde_json::to_vec(&TestMessage::Pong("pong".to_string())).unwrap(),
            ),
            _ => {
                return Err(BehaviorError::ProcessingError(
                    "Unknown message".to_string(),
                ))
            }
        };

        if !msg.sender_id.is_empty() {
            let correlation_id = if msg.correlation_id.is_empty() {
                None
            } else {
                Some(msg.correlation_id.as_str())
            };
            ctx.send_reply(
                correlation_id,
                &msg.sender_id,
                ActorId::from_canonical(&msg.receiver_id).map_err(|e| {
                    BehaviorError::ProcessingError(format!(
                        "Failed to parse sender actor id for reply: {}",
                        e
                    ))
                })?,
                reply_msg,
            )
            .await
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
        }
        Ok(())
    }
}

// ============================================================================
// TEST HELPERS
// ============================================================================

/// Create a test node WITHOUT server (for unit tests)
/// - Services initialized but no gRPC server
/// - Fast, no network dependencies
async fn create_test_node() -> Arc<Node> {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let node_clone = node.clone();
    node_clone
        .initialize_services()
        .await
        .expect("Failed to initialize services");
    // Wait for services to be ready
    for _ in 0..5 {
        yield_now().await;
    }
    node
}

/// Create a test node WITH server (for integration tests only)
/// - Services initialized AND gRPC server started
/// - Requires WASM runtime
/// - Use ONLY for tests that need actual application deployment
async fn create_test_node_with_server() -> Arc<Node> {
    let node: Arc<Node> = Arc::new(
        NodeBuilder::new("test-node")
            .with_listen_addr("127.0.0.1:0") // Ephemeral port (HTTP gateway auto-uses 0 too)
            .build()
            .await,
    );
    let node_clone = node.clone();
    node_clone
        .initialize_services()
        .await
        .expect("Failed to initialize services");
    // Start node in background (start() blocks forever)
    let node_clone2 = node.clone();
    let start_error = std::sync::Arc::new(tokio::sync::Mutex::new(None::<String>));
    let start_error_clone = start_error.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone2.start().await {
            let error_msg = format!("Node start failed: {:?}", e);
            eprintln!("🔴 [TEST] {}", error_msg);
            *start_error_clone.lock().await = Some(error_msg);
        }
    });
    // Wait for WASM runtime to be initialized (required for application deployment)
    // start() initializes WASM runtime early, but it's async so we need to poll
    // WasmRuntime::new() can take time (Engine::new() is a blocking call that runs in spawn_blocking)
    let start = std::time::Instant::now();
    let mut attempts = 0;
    let timeout = Duration::from_secs(30); // Increased timeout for WASM runtime initialization
    while start.elapsed() < timeout {
        // Check if start() failed
        if let Some(error) = start_error.lock().await.as_ref() {
            start_handle.abort();
            panic!(
                "WASM runtime not initialized - {} - cannot run integration test",
                error
            );
        }

        if node.service_locator().get_wasm_runtime().await.is_some() {
            eprintln!(
                "🟢 [TEST] WASM runtime initialized after {} attempts (elapsed: {:?})",
                attempts,
                start.elapsed()
            );
            break;
        }
        attempts += 1;
        if attempts % 100 == 0 {
            eprintln!(
                "🔵 [TEST] Waiting for WASM runtime... (attempt {}, elapsed: {:?})",
                attempts,
                start.elapsed()
            );
        }
        // Use small sleep to allow start() to make progress
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Verify WASM runtime is initialized
    if node.service_locator().get_wasm_runtime().await.is_none() {
        let error_msg = start_error.lock().await.clone();
        start_handle.abort();
        if let Some(error) = error_msg {
            panic!(
                "WASM runtime not initialized after {} attempts - {} - cannot run integration test",
                attempts, error
            );
        } else {
            panic!("WASM runtime not initialized after {} attempts (elapsed: {:?}) - cannot run integration test", attempts, start.elapsed());
        }
    }

    // Additional wait for services to be fully ready
    for _ in 0..20 {
        yield_now().await;
    }
    node
}

/// Helper to wait for actors to be registered (yield-based polling, no sleep)
async fn wait_for_actors_registered(
    node: &Node,
    expected_actor_ids: &[ActorId],
    timeout_duration: Duration,
) -> bool {
    let registry = node
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry not found");

    let start = std::time::Instant::now();
    let mut attempts = 0;

    while start.elapsed() < timeout_duration {
        let registered_ids = registry.registered_actor_ids().await;
        let expected_set: std::collections::HashSet<ActorId> =
            expected_actor_ids.iter().cloned().collect();
        let registered_set: std::collections::HashSet<ActorId> =
            registered_ids.iter().cloned().collect();

        if expected_set.is_subset(&registered_set) {
            return true;
        }
        drop(registered_ids);

        attempts += 1;
        if attempts > 1000 {
            break; // Prevent infinite loops
        }

        yield_now().await;
    }

    false
}

/// Helper to wait for eager actors to be active
async fn wait_for_eager_actors_active(
    node: &Node,
    expected_actor_ids: &[ActorId],
    timeout_duration: Duration,
) -> bool {
    let registry = node
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry not found");

    let start = std::time::Instant::now();
    let mut attempts = 0;

    while start.elapsed() < timeout_duration {
        let mut all_active = true;
        for actor_id in expected_actor_ids {
            if !registry.is_actor_activated(actor_id).await {
                all_active = false;
                break;
            }
        }

        if all_active {
            return true;
        }

        attempts += 1;
        if attempts > 1000 {
            break;
        }

        yield_now().await;
    }

    false
}

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

/// Create virtual actor facet with specified activation strategy
fn create_virtual_actor_facet(activation_strategy: &str) -> Facet {
    let mut config = HashMap::new();
    config.insert("idle_timeout_seconds".to_string(), "300".to_string());
    config.insert(
        "activation_strategy".to_string(),
        activation_strategy.to_string(),
    );

    Facet {
        r#type: "virtual_actor".to_string(),
        config,
        priority: 100,
        state: HashMap::new(),
        metadata: None,
    }
}

// ============================================================================
// UNIT TESTS - Direct Actor Registration (No WASM, No Server)
// ============================================================================

/// UNIT TEST: Eager virtual actors should activate immediately
#[tokio::test]
async fn test_eager_virtual_actors_activation() {
    timeout(Duration::from_secs(3), async {
        let node = create_test_node().await;
        let node_id = node.id().as_str();

        // Register eager virtual actor directly
        let actor_id_1 = test_runtime_actor_id("eager-worker-1", node_id);
        let _actor_ref_1 = get_or_activate_actor_helper(&node, actor_id_1.clone(), || async {
            let behavior = Box::new(TestActor);
            let actor = ActorBuilder::new(behavior)
                .with_id(actor_id_1.clone())
                .build()
                .await
                .map_err(|e| {
                    plexspaces_node::NodeError::ActorRegistrationFailed(
                        actor_id_1.clone().into(),
                        format!("Failed to build actor: {}", e),
                    )
                })?;

            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "eager"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config, 100));
            actor.attach_facet(virtual_facet).await.map_err(|e| {
                plexspaces_node::NodeError::ActorRegistrationFailed(
                    actor_id_1.clone().into(),
                    format!("Failed to attach VirtualActorFacet: {}", e),
                )
            })?;

            Ok(actor)
        })
        .await
        .unwrap();

        // Wait for actor to be registered and active
        let registered =
            wait_for_actors_registered(&node, &[actor_id_1.clone()], Duration::from_secs(1)).await;
        assert!(registered, "Eager virtual actor should be registered");

        // Check if eager actor is active (should activate immediately)
        let registry = node
            .service_locator()
            .actor_registry()
            .await
            .expect("ActorRegistry not found");

        let active = registry.is_actor_activated(&actor_id_1).await;
        assert!(active, "Eager virtual actor should be active");

        // Verify actor is accessible
        let actor_ref = lookup_actor_ref(&node, &actor_id_1).await;
        assert!(
            actor_ref.is_ok() && actor_ref.unwrap().is_some(),
            "Eager virtual actor should be accessible"
        );
    })
    .await
    .expect("Test should complete within 3 seconds");
}

/// UNIT TEST: Lazy virtual actors should be registered but not active until first message
#[tokio::test]
async fn test_lazy_virtual_actors_registration() {
    timeout(Duration::from_secs(3), async {
        let node = create_test_node().await;
        let node_id = node.id().as_str();

        // Register lazy virtual actor directly
        let actor_id = test_runtime_actor_id("lazy-worker-1", node_id);
        let _actor_ref = get_or_activate_actor_helper(&node, actor_id.clone(), || async {
            let behavior = Box::new(TestActor);
            let actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await
                .map_err(|e| {
                    plexspaces_node::NodeError::ActorRegistrationFailed(
                        actor_id.clone().into(),
                        format!("Failed to build actor: {}", e),
                    )
                })?;

            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config, 100));
            actor.attach_facet(virtual_facet).await.map_err(|e| {
                plexspaces_node::NodeError::ActorRegistrationFailed(
                    actor_id.clone().into(),
                    format!("Failed to attach VirtualActorFacet: {}", e),
                )
            })?;

            Ok(actor)
        })
        .await
        .unwrap();

        // Wait for actor to be registered
        let registered =
            wait_for_actors_registered(&node, &[actor_id.clone()], Duration::from_secs(1)).await;
        assert!(registered, "Lazy virtual actor should be registered");

        // Lazy actor should be routable (registered) even if not active
        let registry = node
            .service_locator()
            .actor_registry()
            .await
            .expect("ActorRegistry not found");

        assert!(
            registry.registered_actor_ids().await.contains(&actor_id),
            "Lazy virtual actor should remain registered"
        );

        // Actor should be accessible via lookup_actor_ref even before first activation.
        let actor_ref = lookup_actor_ref(&node, &actor_id).await;
        assert!(
            actor_ref.is_ok() && actor_ref.unwrap().is_some(),
            "Lazy virtual actor should be accessible"
        );
    })
    .await
    .expect("Test should complete within 3 seconds");
}

/// UNIT TEST: Mixed eager and lazy virtual actors
#[tokio::test]
async fn test_mixed_eager_lazy_virtual_actors() {
    timeout(Duration::from_secs(3), async {
        let node = create_test_node().await;
        let node_id = node.id().as_str();

        // Register eager virtual actor
        let eager_id = test_runtime_actor_id("eager-mixed-1", node_id);
        let _eager_ref = get_or_activate_actor_helper(&node, eager_id.clone(), || async {
            let behavior = Box::new(TestActor);
            let actor = ActorBuilder::new(behavior)
                .with_id(eager_id.clone())
                .build()
                .await
                .map_err(|e| {
                    plexspaces_node::NodeError::ActorRegistrationFailed(
                        eager_id.clone().into(),
                        format!("Failed to build actor: {}", e),
                    )
                })?;

            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "eager"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config, 100));
            actor.attach_facet(virtual_facet).await.map_err(|e| {
                plexspaces_node::NodeError::ActorRegistrationFailed(
                    eager_id.clone().into(),
                    format!("Failed to attach VirtualActorFacet: {}", e),
                )
            })?;

            Ok(actor)
        })
        .await
        .unwrap();

        // Register lazy virtual actor
        let lazy_id = test_runtime_actor_id("lazy-mixed-1", node_id);
        let _lazy_ref = get_or_activate_actor_helper(&node, lazy_id.clone(), || async {
            let behavior = Box::new(TestActor);
            let actor = ActorBuilder::new(behavior)
                .with_id(lazy_id.clone())
                .build()
                .await
                .map_err(|e| {
                    plexspaces_node::NodeError::ActorRegistrationFailed(
                        lazy_id.clone().into(),
                        format!("Failed to build actor: {}", e),
                    )
                })?;

            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config, 100));
            actor.attach_facet(virtual_facet).await.map_err(|e| {
                plexspaces_node::NodeError::ActorRegistrationFailed(
                    lazy_id.clone().into(),
                    format!("Failed to attach VirtualActorFacet: {}", e),
                )
            })?;

            Ok(actor)
        })
        .await
        .unwrap();

        // Wait for both actors to be registered
        let expected_actors = vec![eager_id.clone(), lazy_id.clone()];
        let registered =
            wait_for_actors_registered(&node, &expected_actors, Duration::from_secs(1)).await;
        assert!(registered, "Mixed virtual actors should be registered");

        // Check if eager actor is active (should activate immediately)
        let registry = node
            .service_locator()
            .actor_registry()
            .await
            .expect("ActorRegistry not found");

        let eager_active = registry.is_actor_activated(&eager_id).await;
        assert!(eager_active, "Eager virtual actor should be active");

        // Lazy actor should be registered but not active
        assert!(
            registry.registered_actor_ids().await.contains(&lazy_id),
            "Lazy virtual actor should be registered"
        );

        // Verify both are accessible
        let eager_ref = lookup_actor_ref(&node, &eager_id).await;
        let lazy_ref = lookup_actor_ref(&node, &lazy_id).await;
        assert!(
            eager_ref.is_ok() && eager_ref.unwrap().is_some(),
            "Eager virtual actor should be accessible"
        );
        assert!(
            lazy_ref.is_ok() && lazy_ref.unwrap().is_some(),
            "Lazy virtual actor should be accessible"
        );
    })
    .await
    .expect("Test should complete within 3 seconds");
}

// ============================================================================
// INTEGRATION TESTS - WASM Application Deployment (Minimal)
// ============================================================================

/// INTEGRATION TEST: Application deployment with eager virtual actors via WASM
/// This is the ONLY integration test - tests actual WASM application deployment
#[tokio::test]
async fn test_application_deployment_with_eager_virtual_actors() {
    // Increased timeout to account for WASM runtime initialization, deployment, and actor activation
    // WASM deployment can take time due to module compilation and actor spawning
    timeout(Duration::from_secs(60), async {
        let node = create_test_node_with_server().await;
        let node_id = node.id().as_str();

        // Create application spec with supervisor and eager virtual actor children
        let wasm_module = create_minimal_wasm_module();
        let supervisor_spec = SupervisorSpec {
            strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
            max_restarts: 5,
            max_restart_window: None,
            children: vec![ChildSpec {
                id: "eager-worker-1".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: None,
                supervisor: None,
                facets: vec![create_virtual_actor_facet("eager")],
                behavior_kind: None,
            }],
        };

        let app_spec = ApplicationSpec {
            name: "eager-virtual-app".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            version: "1.0.0".to_string(),
            description: "Test app with eager virtual actors".to_string(),
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

        // Deploy application
        let application_manager = node.application_manager();
        let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
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
            initial_state: vec![],
        };

        eprintln!("🔵 [TEST] Starting application deployment...");
        let deploy_start = std::time::Instant::now();
        let response = tokio::time::timeout(
            Duration::from_secs(20),
            service.deploy_application(app_request_with_tenant(request)),
        )
        .await;

        match response {
            Ok(Ok(resp)) => {
                eprintln!(
                    "✅ [TEST] Deployment response received in {:?}",
                    deploy_start.elapsed()
                );
                let res = resp.into_inner();
                assert!(res.success, "Deployment should be successful: {:?}", res);
                eprintln!("✅ [TEST] Application deployed successfully");
            }
            Ok(Err(e)) => {
                panic!("Deployment failed: {:?}", e);
            }
            Err(_) => {
                panic!("Deployment timed out after 20 seconds");
            }
        }

        // Wait for actor to be registered (with longer timeout for WASM deployment)
        let actor_id = test_runtime_actor_id("eager-worker-1", node_id);
        eprintln!("🔵 [TEST] Waiting for actor to be registered: {}", actor_id);
        let registered =
            wait_for_actors_registered(&node, &[actor_id.clone()], Duration::from_secs(10)).await;
        assert!(
            registered,
            "Eager virtual actor should be registered: {}",
            actor_id
        );
        eprintln!("✅ [TEST] Actor registered: {}", actor_id);

        // Check if eager actor is active (should activate immediately)
        let registry = node
            .service_locator()
            .actor_registry()
            .await
            .expect("ActorRegistry not found");

        eprintln!("🔵 [TEST] Checking if actor is active: {}", actor_id);
        let active = registry.is_actor_activated(&actor_id).await;
        assert!(active, "Eager virtual actor should be active: {}", actor_id);
        eprintln!("✅ [TEST] Actor is active: {}", actor_id);

        // Verify actor is accessible
        eprintln!("🔵 [TEST] Verifying actor is accessible: {}", actor_id);
        let actor_ref = lookup_actor_ref(&node, &actor_id).await;
        assert!(
            actor_ref.is_ok() && actor_ref.unwrap().is_some(),
            "Eager virtual actor should be accessible: {}",
            actor_id
        );
        eprintln!("✅ [TEST] Actor is accessible: {}", actor_id);
    })
    .await
    .expect("Test should complete within 60 seconds");
}
