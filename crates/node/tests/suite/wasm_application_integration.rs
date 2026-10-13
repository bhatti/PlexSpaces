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

//! Integration tests for WASM application deployment
//!
//! ## Purpose
//! Tests WASM application deployment via ApplicationService, including:
//! - Deploying WASM modules
//! - Creating WasmApplication instances
//! - Application lifecycle (start/stop/health)
//! - Error handling (invalid modules, missing runtime, etc.)

use super::test_helpers::app_request_with_tenant;
use async_trait::async_trait;
use plexspaces_actor::JournalStorage as _;
use plexspaces_actor::{
    ActorId, ActorStateHandle, ApplicationManager, Message, MessageSender, RequestContextExt,
    ServiceLocator,
};
use plexspaces_journaling::{virtual_actor_facet_to_lifecycle_facet, VirtualActorFacet};
use plexspaces_node::{Node, NodeId};
use plexspaces_proto::actor::v1::ActorSpawnSpec;
use plexspaces_proto::actor::v1::{
    actor_service_server::ActorService as ActorServiceTrait, AskReplyRequest,
};
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, ApplicationSpec, ApplicationType,
    DeployApplicationRequest, GetApplicationStatusRequest, ListApplicationsRequest,
    ShutdownStrategy,
};
use plexspaces_proto::common::v1::{ActorIdentity, Facet, Metadata};
use plexspaces_proto::supervision::v1::{
    ChildSpec, RestartPolicy, SupervisionStrategy, SupervisorSpec,
};
use plexspaces_proto::v1::journaling::Checkpoint;
use plexspaces_proto::wasm::v1::WasmModule;
use plexspaces_services::actor_service::ActorServiceImpl;
use plexspaces_services::application_service::ApplicationServiceImpl;
use prost_types::Duration as ProstDuration;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tonic::metadata::MetadataValue;
use tonic::Request;

/// WAT source for a minimal WASM actor module that exports memory, init, and handle_message.
/// init(ptr, len) -> 0 (success), handle_message(6 params) -> 0 (success).
const SIMPLE_WASM_WAT: &str = r#"
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

/// Lazy-compiled WASM bytes from WAT source.
fn simple_wasm_bytes() -> Vec<u8> {
    wat::parse_str(SIMPLE_WASM_WAT).expect("Failed to parse SIMPLE_WASM_WAT")
}

/// Convenience alias for places that used the old SIMPLE_WASM constant.
static SIMPLE_WASM_ONCE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
fn get_simple_wasm() -> &'static [u8] {
    SIMPLE_WASM_ONCE.get_or_init(simple_wasm_bytes)
}

async fn create_test_node() -> Arc<Node> {
    use plexspaces_node::NodeBuilder;
    Arc::new(
        NodeBuilder::new("test-node")
            .with_listen_addr("127.0.0.1:0")
            .with_in_memory_backends()
            .with_auth_disabled()
            .build()
            .await,
    )
}

async fn create_test_node_with_service() -> (Arc<Node>, String) {
    // build() calls initialize_services() which registers WASM runtime; set_node_context so start() can spawn actors
    let node = create_test_node().await;
    node.application_manager()
        .set_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;
    // For now, we test directly via ApplicationServiceImpl instead of gRPC (avoids port binding issues)
    (node, String::new())
}

fn app_request_in_scope<T: Send>(body: T, tenant_id: &str, namespace: &str) -> Request<T> {
    let mut request = Request::new(body);
    request
        .metadata_mut()
        .insert("x-tenant-id", MetadataValue::try_from(tenant_id).unwrap());
    request
        .metadata_mut()
        .insert("x-namespace", MetadataValue::try_from(namespace).unwrap());
    request
}

struct TestActorStateHandle {
    stopped: AtomicBool,
}

impl TestActorStateHandle {
    fn new() -> Self {
        Self {
            stopped: AtomicBool::new(false),
        }
    }

    fn was_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ActorStateHandle for TestActorStateHandle {
    async fn actor_state(&self) -> plexspaces_proto::v1::actor::ActorState {
        if self.was_stopped() {
            plexspaces_proto::v1::actor::ActorState::ActorStateTerminated
        } else {
            plexspaces_proto::v1::actor::ActorState::ActorStateActive
        }
    }

    async fn stop_actor(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}

struct TestMessageSender {
    actor_id: ActorId,
    tenant_id: String,
    namespace: String,
    actor_type: std::sync::RwLock<Option<String>>,
    local_state_handle: std::sync::RwLock<Option<Arc<dyn ActorStateHandle>>>,
}

impl TestMessageSender {
    fn new(actor_id: ActorId, tenant_id: String, namespace: String) -> Self {
        Self {
            actor_id,
            tenant_id,
            namespace,
            actor_type: std::sync::RwLock::new(None),
            local_state_handle: std::sync::RwLock::new(None),
        }
    }
}

#[async_trait]
impl MessageSender for TestMessageSender {
    async fn tell(
        &self,
        _ctx: &plexspaces_actor::RequestContext,
        _message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn actor_id(&self) -> Option<String> {
        Some(self.actor_id.to_string())
    }

    fn tenant_id(&self) -> Option<&str> {
        Some(&self.tenant_id)
    }

    fn namespace(&self) -> Option<&str> {
        Some(&self.namespace)
    }

    fn actor_type(&self) -> Option<String> {
        self.actor_type.read().ok().and_then(|guard| guard.clone())
    }

    async fn set_actor_type(&self, actor_type: Option<String>) {
        if let Ok(mut guard) = self.actor_type.write() {
            *guard = actor_type;
        }
    }

    fn local_state_handle(&self) -> Option<Arc<dyn ActorStateHandle>> {
        self.local_state_handle
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    async fn set_local_state_handle(&self, handle: Option<Arc<dyn ActorStateHandle>>) {
        if let Ok(mut guard) = self.local_state_handle.write() {
            *guard = handle;
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Create a WASM module with supervisor tree in ApplicationSpec
fn create_wasm_module_with_supervisor_spec() -> (WasmModule, ApplicationSpec) {
    // Create supervisor spec with worker actors
    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: Some(ProstDuration {
            seconds: 5,
            nanos: 0,
        }),
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "worker-1".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 5,
                    nanos: 0,
                }),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "worker-2".to_string(),
                    actor_type: "test_wasm_actor".to_string(),
                }),

                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 5,
                    nanos: 0,
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let app_spec = ApplicationSpec {
        name: "test-app".to_string(),
        tenant_id: String::new(),
        version: "1.0.0".to_string(),
        description: "Test application with supervisor tree".to_string(),
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
        static_mount: String::new(),
    };

    let wasm_module = WasmModule {
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        version_number: 1,
        module_bytes: get_simple_wasm().to_vec(),
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: Some(Metadata {
            labels: HashMap::new(),
            annotations: HashMap::new(),
            create_time: None,
            created_by: String::new(),
            update_time: None,
            updated_by: String::new(),
        }),
        created_at: None,
        size_bytes: get_simple_wasm().len() as u64,
    };

    (wasm_module, app_spec)
}

#[tokio::test]
async fn test_deploy_wasm_application_success() {
    let (node, _) = create_test_node_with_service().await;

    // Create ApplicationService - gets ApplicationManager from ServiceLocator
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);

    // Create WASM module
    let wasm_module = WasmModule {
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        version_number: 1,
        module_bytes: get_simple_wasm().to_vec(),
        module_hash: String::new(), // Will be computed by deployment service
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: None,
        created_at: None,
        size_bytes: get_simple_wasm().len() as u64,
    };

    // Deploy application
    let request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: "test-app-001".to_string(),
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: None,
        initial_state: vec![],
    };

    let response = service
        .deploy_application(app_request_with_tenant(request))
        .await
        .expect("Deploy application should succeed");

    let deploy_response = response.into_inner();
    assert!(deploy_response.success, "Deployment should succeed");
    assert_eq!(
        deploy_response.application_id, "test-app-001",
        "Application ID should match"
    );
}

#[tokio::test]
async fn test_deploy_wasm_application_invalid_module() {
    let (node, _) = create_test_node_with_service().await;

    // Create ApplicationService - gets ApplicationManager from ServiceLocator
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);

    // Create invalid WASM module (empty bytes)
    let wasm_module = WasmModule {
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        version_number: 1,
        module_bytes: vec![], // Invalid: empty WASM
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: None,
        created_at: None,
        size_bytes: 0,
    };

    // Deploy application
    let request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: "test-app-002".to_string(),
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: None,
        initial_state: vec![],
    };

    let response = service
        .deploy_application(app_request_with_tenant(request))
        .await;

    // Should fail with error
    assert!(
        response.is_err(),
        "Deployment should fail with invalid WASM"
    );
    let status = response.unwrap_err();
    assert!(
        status.code() == tonic::Code::Internal || status.code() == tonic::Code::InvalidArgument,
        "Should return Internal or InvalidArgument error: {:?}",
        status
    );
}

#[tokio::test]
async fn test_deploy_wasm_application_missing_fields() {
    let (node, _) = create_test_node_with_service().await;

    // Create ApplicationService - gets ApplicationManager from ServiceLocator
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);

    // Test missing application_id
    let request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: String::new(), // Invalid: empty
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(WasmModule {
            name: "test-app".to_string(),
            version: "1.0.0".to_string(),
            version_number: 1,
            module_bytes: get_simple_wasm().to_vec(),
            module_hash: String::new(),
            wit_interface: String::new(),
            source_languages: vec![],
            metadata: None,
            created_at: None,
            size_bytes: get_simple_wasm().len() as u64,
        }),
        config: None,
        initial_state: vec![],
    };

    let response = service
        .deploy_application(app_request_with_tenant(request))
        .await;
    assert!(response.is_err(), "Should fail with missing application_id");
    let status = response.unwrap_err();
    assert_eq!(
        status.code(),
        tonic::Code::InvalidArgument,
        "Should return InvalidArgument: {:?}",
        status
    );
}

#[tokio::test]
async fn test_get_wasm_application_status() {
    let (node, _) = create_test_node_with_service().await;

    // Create ApplicationService - use same instance for deploy and status
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);

    // Deploy application first
    let wasm_module = WasmModule {
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        version_number: 1,
        module_bytes: get_simple_wasm().to_vec(),
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: None,
        created_at: None,
        size_bytes: get_simple_wasm().len() as u64,
    };

    let deploy_request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: "test-app-003".to_string(),
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: None,
        initial_state: vec![],
    };

    service
        .deploy_application(app_request_with_tenant(deploy_request))
        .await
        .expect("Deploy should succeed");

    // Get application status - ApplicationManager stores by application_id
    let status_service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let status_request = GetApplicationStatusRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: "test-app-003".to_string(),
    };

    let response = status_service
        .get_application_status(Request::new(status_request))
        .await
        .expect("Get status should succeed");

    let status_response = response.into_inner();
    assert!(
        status_response.application.is_some(),
        "Application should be found, error: {:?}",
        status_response.error
    );
    let app = status_response.application.unwrap();
    assert_eq!(app.application_id, "test-app-003");
}

#[tokio::test]
async fn test_list_wasm_applications() {
    let (node, _) = create_test_node_with_service().await;

    // Create ApplicationService
    let application_manager = node.application_manager();
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);

    // Deploy multiple applications
    for i in 0..3 {
        let wasm_module = WasmModule {
            name: format!("test-app-{}", i),
            version: "1.0.0".to_string(),
            version_number: 1,
            module_bytes: get_simple_wasm().to_vec(),
            module_hash: String::new(),
            wit_interface: String::new(),
            source_languages: vec![],
            metadata: None,
            created_at: None,
            size_bytes: get_simple_wasm().len() as u64,
        };

        let deploy_request = DeployApplicationRequest {
            request_id: ulid::Ulid::new().to_string(),
            application_id: format!("test-app-{:03}", i),
            name: format!("test-app-{}", i),
            version: "1.0.0".to_string(),
            wasm_module: Some(wasm_module),
            config: None,
            initial_state: vec![],
        };

        service
            .deploy_application(app_request_with_tenant(deploy_request))
            .await
            .expect("Deploy should succeed");
    }

    // Wait for applications to start
    sleep(Duration::from_millis(300)).await;

    // List applications
    let list_request = ListApplicationsRequest {
        request_id: ulid::Ulid::new().to_string(),
        status_filter: None,
    };
    let response = service
        .list_applications(Request::new(list_request))
        .await
        .expect("List applications should succeed");

    let list_response = response.into_inner();
    assert!(
        list_response.applications.len() >= 3,
        "Should list at least 3 applications, got: {}",
        list_response.applications.len()
    );
}

#[tokio::test]
async fn test_get_nonexistent_wasm_application_status() {
    let (node, _) = create_test_node_with_service().await;

    // Create ApplicationService - gets ApplicationManager from ServiceLocator
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);

    // Get status for non-existent application
    let status_request = GetApplicationStatusRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: "nonexistent-app".to_string(),
    };

    let response = service
        .get_application_status(Request::new(status_request))
        .await
        .expect("Get status should succeed (returns empty)");

    let status_response = response.into_inner();
    assert!(
        status_response.application.is_none(),
        "Non-existent application should return None"
    );
    assert!(
        status_response.error.is_some(),
        "Should include error message"
    );
}

#[tokio::test]
async fn test_deploy_wasm_application_with_supervisor_tree() {
    // Test deploying WASM application with supervisor tree from ApplicationSpec
    let (node, _) = create_test_node_with_service().await;

    let application_manager = node.application_manager();
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);

    // Create WASM module with supervisor spec
    let (wasm_module, app_spec) = create_wasm_module_with_supervisor_spec();

    // Deploy application with supervisor tree
    let request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: "supervisor-app-001".to_string(),
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        initial_state: vec![],
    };

    // Deploy
    let response = service
        .deploy_application(app_request_with_tenant(request))
        .await;
    assert!(
        response.is_ok(),
        "Deployment should succeed: {:?}",
        response.err()
    );
    let res = response.unwrap().into_inner();
    assert!(res.success);
    assert_eq!(res.application_id, "supervisor-app-001");

    // Wait for application to start and actors to spawn
    sleep(Duration::from_millis(500)).await;

    // Verify application is running (ApplicationManager stores by application_id)
    let app_manager = node.application_manager();
    let app_state = app_manager.get_state("supervisor-app-001").await;
    assert!(app_state.is_some());
    assert_eq!(
        app_state.unwrap(),
        plexspaces_proto::v1::application::ApplicationState::ApplicationStateRunning
    );
}

#[tokio::test]
async fn test_undeploy_wasm_application_with_supervisor_tree() {
    // Test undeploying WASM application with supervisor tree (graceful shutdown)
    let (node, _) = create_test_node_with_service().await;

    let application_manager = node.application_manager();
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);

    // Deploy application with supervisor tree
    let (wasm_module, app_spec) = create_wasm_module_with_supervisor_spec();
    let deploy_request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: "shutdown-app-001".to_string(),
        name: "shutdown-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        initial_state: vec![],
    };

    service
        .deploy_application(app_request_with_tenant(deploy_request))
        .await
        .expect("Deploy should succeed");

    // Wait for application to start
    sleep(Duration::from_millis(500)).await;

    // Undeploy application (ApplicationManager stores by application_id)
    let undeploy_request = plexspaces_proto::application::v1::UndeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: "shutdown-app-001".to_string(),
        timeout: None,
    };

    let response = service
        .undeploy_application(app_request_with_tenant(undeploy_request))
        .await;
    assert!(
        response.is_ok(),
        "Undeploy should succeed: {:?}",
        response.err()
    );
    let res = response.unwrap().into_inner();
    assert!(res.success);

    // Wait for shutdown to complete
    sleep(Duration::from_millis(500)).await;

    // Verify application is unregistered (undeploy stops then unregisters)
    let app_manager = node.application_manager();
    let app_state: Option<plexspaces_proto::v1::application::ApplicationState> =
        app_manager.get_state("shutdown-app-001").await;
    assert!(
        app_state.is_none(),
        "Application should be unregistered after undeploy"
    );
}

/// Integration test: WASM undeploy cleans up instances and evicts module from cache.
///
/// Verifies that on undeploy:
/// 1. Application is stopped and unregistered (get_state returns None, not in list).
/// 2. WASM instances are dropped (Drop decrements plexspaces_wasm_active_instances).
/// 3. Compiled module is evicted from runtime cache (no memory leak).
///
/// When a WASM actor handle() fails, the runtime logs SIMPLE_ACTOR_HANDLE_FAILED_LOG_MESSAGE
/// (error_first_line only; full backtrace only at DEBUG). Instance cleanup (Drop) still runs
/// after such errors and on undeploy.
#[tokio::test]
async fn test_wasm_undeploy_cleanup_instances_and_module() {
    // Test checks for the canonical "Simple actor handle() call failed" message: integration
    // tests and log parsing can rely on this constant.
    assert_eq!(
        plexspaces_wasm_runtime::SIMPLE_ACTOR_HANDLE_FAILED_LOG_MESSAGE,
        "Simple actor handle() call failed"
    );
    let (node, _) = create_test_node_with_service().await;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let app_name = "cleanup-test-app";

    // Deploy WASM application (ApplicationManager stores by application_id)
    let app_id = format!("{}-001", app_name);
    let (wasm_module, app_spec) = create_wasm_module_with_supervisor_spec();
    let wasm_bytes = wasm_module.module_bytes.clone();
    let deploy_request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: app_id.clone(),
        name: app_name.to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        initial_state: vec![],
    };

    service
        .deploy_application(app_request_with_tenant(deploy_request))
        .await
        .expect("Deploy should succeed");

    sleep(Duration::from_millis(500)).await;

    // Compute module hash (same as deployment service) to verify eviction later
    let module_hash =
        plexspaces_wasm_runtime::deployment_service::WasmDeploymentService::compute_hash(
            &wasm_bytes,
        );

    // Verify module is in cache after deploy
    let wasm_runtime = node
        .service_locator()
        .get_wasm_runtime()
        .await
        .expect("WASM runtime");
    let cached_before: Option<Arc<dyn std::any::Any + Send + Sync>> =
        wasm_runtime.get_module(&module_hash).await;
    assert!(
        cached_before.is_some(),
        "Module should be in cache after deploy"
    );

    // Undeploy (use application_id = app_id, same as deploy)
    let undeploy_request = plexspaces_proto::application::v1::UndeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: app_id.clone(),
        timeout: None,
    };
    let response = service
        .undeploy_application(app_request_with_tenant(undeploy_request))
        .await;
    assert!(
        response.is_ok(),
        "Undeploy should succeed: {:?}",
        response.err()
    );
    assert!(response.unwrap().into_inner().success);

    sleep(Duration::from_millis(500)).await;

    // 1. Application must be unregistered (not in list, get_state None)
    let app_manager = node.application_manager();
    let app_state = app_manager.get_state(&app_id).await;
    assert!(
        app_state.is_none(),
        "Application should be unregistered after undeploy"
    );
    let list = app_manager.list_applications().await;
    assert!(
        !list.contains(&app_id),
        "Application should not be in list after undeploy"
    );

    // 2. Module must be evicted from cache (no leak)
    let cached_after: Option<Arc<dyn std::any::Any + Send + Sync>> =
        wasm_runtime.get_module(&module_hash).await;
    assert!(
        cached_after.is_none(),
        "Module should be evicted from cache after undeploy (cleanup)"
    );
}

// ============================================================================
// Integration Tests for WASM Supervisor Restart Functionality
// ============================================================================

fn build_go_abstractions_example_wasm() -> Vec<u8> {
    let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let example_dir = repo_root.join("examples/go/apps/abstractions");
    let output_path = repo_root.join("target/examples/go/abstractions/abstractions_actor.wasm");

    if !output_path.exists() {
        let status = Command::new("bash")
            .arg("build.sh")
            .current_dir(&example_dir)
            .env("GOCACHE", "/tmp/plexspaces-go-cache")
            .status()
            .expect("Go abstractions build.sh should start");
        assert!(status.success(), "Go abstractions build.sh should succeed");
    }

    std::fs::read(&output_path).unwrap_or_else(|e| {
        panic!(
            "Failed to load built Go example wasm {}: {}",
            output_path.display(),
            e
        )
    })
}

fn build_go_web_crawl_example_wasm() -> Vec<u8> {
    let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let example_dir = repo_root.join("examples/go/apps/web_crawl");
    let output_path = repo_root.join("target/examples/go/web_crawl/web_crawl_actor.wasm");

    // Always rebuild to pick up source changes
    let status = Command::new("bash")
        .arg("build.sh")
        .current_dir(&example_dir)
        .env("GOCACHE", "/tmp/plexspaces-go-cache")
        .status()
        .expect("Go web_crawl build.sh should start");
    assert!(status.success(), "Go web_crawl build.sh should succeed");

    std::fs::read(&output_path).unwrap_or_else(|e| {
        panic!(
            "Failed to load built Go web_crawl wasm {}: {}",
            output_path.display(),
            e
        )
    })
}

fn build_python_abstractions_example_wasm() -> Vec<u8> {
    let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let example_dir = repo_root.join("examples/python/apps/abstractions");
    let output_path = repo_root.join("target/examples/python/abstractions/abstractions_actor.wasm");

    if !output_path.exists() {
        let status = Command::new("bash")
            .arg("build.sh")
            .current_dir(&example_dir)
            .status()
            .expect("Python abstractions build.sh should start");
        assert!(
            status.success(),
            "Python abstractions build.sh should succeed"
        );
    }

    std::fs::read(&output_path).unwrap_or_else(|e| {
        panic!(
            "Failed to load built Python example wasm {}: {}",
            output_path.display(),
            e
        )
    })
}

async fn actor_ask_json(
    actor_service: &ActorServiceImpl,
    tenant_id: &str,
    namespace: &str,
    actor_type: &str,
    payload: serde_json::Value,
) -> serde_json::Value {
    let request = AskReplyRequest {
        request_id: ulid::Ulid::new().to_string(),
        actor_type: actor_type.to_string(),
        actor_name: String::new(),
        namespace: namespace.to_string(),
        http_method: "POST".to_string(),
        payload: serde_json::to_vec(&payload).expect("payload JSON should serialize"),
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path: format!("/api/v1/actors/{}/{}/ask", namespace, actor_type),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "call".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
        timeout: None,
    };

    let response = ActorServiceTrait::ask_reply(
        actor_service,
        app_request_in_scope(request, tenant_id, namespace),
    )
    .await
    .expect("actor ask should succeed")
    .into_inner();

    assert!(
        response.success,
        "actor ask should succeed, got error_message={}",
        response.error_message
    );

    serde_json::from_slice(&response.payload).expect("actor payload should be valid JSON")
}

#[tokio::test]
async fn test_undeploy_missing_application_still_cleans_namespace_state() {
    let (node, _) = create_test_node_with_service().await;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let service_locator = node.service_locator();
    let namespace = "abstractions-rust";
    let actor_type = "abstractions";
    let actor_id = ActorId::new("cart-1", actor_type, namespace, "test-node")
        .expect("test actor id should be valid");

    let virtual_actor_manager = service_locator
        .virtual_actor_manager()
        .await
        .expect("VirtualActorManager should be registered");
    virtual_actor_manager
        .register_virtual_actor_type(
            actor_type.to_string(),
            None,
            namespace.to_string(),
            serde_json::json!({
                "virtual_actor": {
                    "idle_timeout": "10m",
                    "activation_strategy": "lazy"
                },
                "durability": {
                    "checkpoint_interval": 5
                }
            }),
            None,
            None,
        )
        .await
        .expect("type registration should succeed");
    virtual_actor_manager
        .register(
            actor_id.clone(),
            Arc::new(tokio::sync::RwLock::new(
                virtual_actor_facet_to_lifecycle_facet(VirtualActorFacet::new(
                    serde_json::json!({
                        "idle_timeout": "10m",
                        "activation_strategy": "lazy"
                    }),
                    100,
                )),
            )),
            ActorSpawnSpec {
                identity: Some(ActorIdentity {
                    name: actor_id.name().to_string(),
                    actor_type: actor_type.to_string(),
                }),
                tenant_id: String::new(),
                namespace: namespace.to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("instance registration should succeed");

    let journal_storage = service_locator
        .get_journal_storage()
        .await
        .expect("JournalStorage should be registered");
    journal_storage
        .save_checkpoint(&Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 2,
            timestamp: Some(prost_types::Timestamp {
                seconds: 1,
                nanos: 0,
            }),
            state_data: br#"{"count":2}"#.to_vec(),
            compression: 0,
            metadata: HashMap::new(),
            state_schema_version: 1,
        })
        .await
        .expect("checkpoint should be saved");

    let undeploy_request = plexspaces_proto::application::v1::UndeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: namespace.to_string(),
        timeout: None,
    };
    let response = service
        .undeploy_application(app_request_with_tenant(undeploy_request))
        .await
        .expect("stateless undeploy cleanup should succeed");
    assert!(response.into_inner().success);

    assert!(
        virtual_actor_manager
            .get_virtual_actor_type(actor_type)
            .await
            .is_none(),
        "virtual actor type should be removed by stateless undeploy cleanup"
    );
    assert!(
        virtual_actor_manager
            .get_metadata(&actor_id)
            .await
            .is_none(),
        "virtual actor instance metadata should be removed by stateless undeploy cleanup"
    );
    assert!(
        journal_storage
            .get_latest_checkpoint(&actor_id.to_string())
            .await
            .is_err(),
        "checkpoint should be purged by stateless undeploy cleanup"
    );
}

#[tokio::test]
#[ignore = "requires Go toolchain and external WASM build"]
async fn test_go_wasm_role_dispatch_virtual_actor_reactivation() {
    // Mimics web_crawl pattern: actor_type doesn't match any factory key,
    // but role matches. Verifies state persists across re-instantiation
    // when dispatch resolves via role (not actor_type).
    let (node, _) = create_test_node_with_service().await;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let actor_service =
        ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());
    let app_id = "role-dispatch-go-sdk-it";
    let tenant_id = "test-tenant";

    let wasm_bytes = build_go_abstractions_example_wasm();
    let wasm_module = WasmModule {
        name: app_id.to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes,
        module_hash: String::new(),
        ..Default::default()
    };

    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: Some(ProstDuration {
            seconds: 60,
            nanos: 0,
        }),
        children: vec![ChildSpec {
            actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: "ephemeral".to_string(),
                actor_type: "abstractions_wasm".to_string(), // Does NOT match factory key
            }),
            role: "ephemeral".to_string(), // Matches factory key via role dispatch
            args: HashMap::from([
                ("role".to_string(), "ephemeral".to_string()),
                ("initial_count".to_string(), "5".to_string()),
            ]),
            restart: RestartPolicy::RestartPolicyPermanent.into(),
            shutdown_timeout: Some(ProstDuration {
                seconds: 5,
                nanos: 0,
            }),
            facets: vec![Facet {
                r#type: "virtual_actor".to_string(),
                config: HashMap::from([
                    ("idle_timeout".to_string(), "10m".to_string()),
                    ("activation_strategy".to_string(), "lazy".to_string()),
                ]),
                priority: 0,
                state: HashMap::new(),
                metadata: None,
            }],
            behavior_kind: Some("GenServer".to_string()),
            ..Default::default()
        }],
        ..Default::default()
    };

    let app_spec = ApplicationSpec {
        name: app_id.to_string(),
        tenant_id: String::new(),
        version: "1.0.0".to_string(),
        description: "Go SDK role-dispatch reactivation test".to_string(),
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
        static_mount: String::new(),
    };

    let deploy_request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: app_id.to_string(),
        name: app_id.to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        initial_state: vec![],
    };
    let deploy_response = service
        .deploy_application(app_request_in_scope(deploy_request, tenant_id, app_id))
        .await
        .expect("deployment should succeed");
    assert!(deploy_response.into_inner().success);

    // First message: activates the actor (dispatches via role since actor_type doesn't match)
    let initial_status = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "session-1:ephemeral",
        serde_json::json!({ "op": "status" }),
    )
    .await;
    assert_eq!(initial_status["count"], serde_json::json!(5));
    assert_eq!(initial_status["role"], serde_json::json!("ephemeral"));

    // Second message: tests re-instantiation state restore (set_state must work)
    let incremented = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "session-1:ephemeral",
        serde_json::json!({ "op": "increment", "amount": 3 }),
    )
    .await;
    assert_eq!(
        incremented["count"],
        serde_json::json!(8),
        "State must persist across re-instantiation. Got: {:?}",
        incremented
    );

    // Third message: further state persistence
    let third = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "session-1:ephemeral",
        serde_json::json!({ "op": "status" }),
    )
    .await;
    assert_eq!(
        third["count"],
        serde_json::json!(8),
        "State must persist across multiple re-instantiations. Got: {:?}",
        third
    );
}

#[tokio::test]
#[ignore = "requires Go toolchain and external WASM build"]
async fn test_go_wasm_web_crawl_reactivation() {
    // Tests the actual web_crawl WASM binary with the exact dispatch pattern from app-config.toml:
    // actor_type="web_crawl_wasm" (doesn't match factory), role="orchestrator" (matches factory).
    // This is the test that exercises the real web_crawl code path.
    let (node, _) = create_test_node_with_service().await;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let actor_service =
        ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());
    let app_id = "go-web-crawl-it";
    let tenant_id = "test-tenant";

    let wasm_bytes = build_go_web_crawl_example_wasm();
    let wasm_module = WasmModule {
        name: app_id.to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes,
        module_hash: String::new(),
        ..Default::default()
    };

    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: Some(ProstDuration {
            seconds: 60,
            nanos: 0,
        }),
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "orchestrator".to_string(),
                    actor_type: "web_crawl_wasm".to_string(),
                }),
                role: "orchestrator".to_string(),
                args: HashMap::from([("role".to_string(), "orchestrator".to_string())]),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 30,
                    nanos: 0,
                }),
                behavior_kind: Some("GenServer".to_string()),
                facets: vec![Facet {
                    r#type: "virtual_actor".to_string(),
                    config: HashMap::from([
                        ("idle_timeout".to_string(), "30m".to_string()),
                        ("activation_strategy".to_string(), "lazy".to_string()),
                    ]),
                    priority: 100,
                    state: HashMap::new(),
                    metadata: None,
                }],
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "fetcher-0".to_string(),
                    actor_type: "web_crawl_wasm".to_string(),
                }),
                role: "fetcher".to_string(),
                args: HashMap::from([
                    ("role".to_string(), "fetcher".to_string()),
                    ("pool_slot".to_string(), "0".to_string()),
                ]),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 10,
                    nanos: 0,
                }),
                behavior_kind: Some("GenServer".to_string()),
                facets: vec![Facet {
                    r#type: "virtual_actor".to_string(),
                    config: HashMap::from([
                        ("idle_timeout".to_string(), "5m".to_string()),
                        ("activation_strategy".to_string(), "lazy".to_string()),
                    ]),
                    priority: 100,
                    state: HashMap::new(),
                    metadata: None,
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let app_spec = ApplicationSpec {
        name: app_id.to_string(),
        tenant_id: String::new(),
        version: "1.0.0".to_string(),
        description: "Go web_crawl integration test".to_string(),
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
        static_mount: String::new(),
    };

    let deploy_request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: app_id.to_string(),
        name: app_id.to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        initial_state: vec![],
    };
    let deploy_response = service
        .deploy_application(app_request_in_scope(deploy_request, tenant_id, app_id))
        .await
        .expect("deployment should succeed");
    assert!(deploy_response.into_inner().success);

    // Send crawl command to orchestrator
    let crawl_result = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "orchestrator",
        serde_json::json!({
            "op": "crawl",
            "seed_urls": ["https://example.com"],
            "max_pages": 5,
            "max_depth": 1
        }),
    )
    .await;
    assert_eq!(
        crawl_result["status"],
        serde_json::json!("ok"),
        "Crawl should succeed. Got: {:?}",
        crawl_result
    );

    // Second message to orchestrator: status (tests re-instantiation state restore)
    let status_result = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "orchestrator",
        serde_json::json!({ "op": "status" }),
    )
    .await;
    assert!(
        status_result.get("error").is_none()
            || status_result["error"].as_str() != Some("no active actor (init not called)"),
        "Orchestrator must stay active across re-instantiation. Got: {:?}",
        status_result
    );
    assert_eq!(
        status_result["role"],
        serde_json::json!("orchestrator"),
        "Orchestrator role must persist. Got: {:?}",
        status_result
    );

    // Third message: another status (third re-instantiation cycle)
    let status_result2 = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "orchestrator",
        serde_json::json!({ "op": "status" }),
    )
    .await;
    assert!(
        status_result2.get("error").is_none()
            || status_result2["error"].as_str() != Some("no active actor (init not called)"),
        "Orchestrator must stay active across multiple re-instantiations. Got: {:?}",
        status_result2
    );

    // Test fetcher too
    let fetch_result = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "fetcher-0",
        serde_json::json!({ "op": "fetch", "url": "https://example.com" }),
    )
    .await;
    assert_eq!(
        fetch_result["status"],
        serde_json::json!("ok"),
        "Fetch should succeed. Got: {:?}",
        fetch_result
    );

    // Fetcher status after re-instantiation
    let fetcher_status = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "fetcher-0",
        serde_json::json!({ "op": "status" }),
    )
    .await;
    assert!(
        fetcher_status.get("error").is_none()
            || fetcher_status["error"].as_str() != Some("no active actor (init not called)"),
        "Fetcher must stay active across re-instantiation. Got: {:?}",
        fetcher_status
    );
}

#[tokio::test]
#[ignore = "requires Go toolchain and external WASM build"]
async fn test_go_wasm_nondurable_virtual_actor_reactivation() {
    let (node, _) = create_test_node_with_service().await;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let actor_service =
        ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());
    let app_id = "abstractions-go-sdk-it";
    let tenant_id = "test-tenant";

    let wasm_bytes = build_go_abstractions_example_wasm();
    let wasm_module = WasmModule {
        name: app_id.to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes,
        module_hash: String::new(),
        ..Default::default()
    };

    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: Some(ProstDuration {
            seconds: 60,
            nanos: 0,
        }),
        children: vec![
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "controller".to_string(),
                    actor_type: "controller".to_string(),
                }),

                role: String::new(),
                args: HashMap::from([("role".to_string(), "controller".to_string())]),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 5,
                    nanos: 0,
                }),
                behavior_kind: Some("GenServer".to_string()),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "ephemeral".to_string(),
                    actor_type: "ephemeral".to_string(),
                }),

                role: String::new(),
                args: HashMap::from([
                    ("role".to_string(), "ephemeral".to_string()),
                    ("initial_count".to_string(), "5".to_string()),
                ]),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 5,
                    nanos: 0,
                }),
                facets: vec![Facet {
                    r#type: "virtual_actor".to_string(),
                    config: HashMap::from([
                        ("idle_timeout".to_string(), "10m".to_string()),
                        ("activation_strategy".to_string(), "lazy".to_string()),
                    ]),
                    priority: 0,
                    state: HashMap::new(),
                    metadata: None,
                }],
                behavior_kind: Some("GenServer".to_string()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let app_spec = ApplicationSpec {
        name: app_id.to_string(),
        tenant_id: String::new(),
        version: "1.0.0".to_string(),
        description: "Go SDK controller stop integration".to_string(),
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
        static_mount: String::new(),
    };

    let deploy_request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: app_id.to_string(),
        name: app_id.to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        initial_state: vec![],
    };
    let deploy_response = service
        .deploy_application(app_request_in_scope(deploy_request, tenant_id, app_id))
        .await
        .expect("deployment should succeed");
    assert!(deploy_response.into_inner().success);

    let initial_status = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "session-1:ephemeral",
        serde_json::json!({ "op": "status" }),
    )
    .await;
    assert_eq!(initial_status["count"], serde_json::json!(5));
    assert_eq!(initial_status["role"], serde_json::json!("ephemeral"));

    let incremented = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "session-1:ephemeral",
        serde_json::json!({ "op": "increment", "amount": 2 }),
    )
    .await;
    assert_eq!(incremented["count"], serde_json::json!(7));

    let session_id_str = initial_status["self_id"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            plexspaces_actor::ActorId::new(
                "session-1",
                "abstractions_wasm",
                app_id,
                node.id().as_str(),
            )
            .unwrap()
            .to_string()
        });
    let session_actor_id = plexspaces_actor::ActorId::from_canonical(&session_id_str)
        .expect("self_id must be a valid canonical actor ID");
    let stop_ctx =
        plexspaces_actor::RequestContext::new_without_auth(String::new(), app_id.to_string());
    node.service_locator()
        .get_actor_factory()
        .await
        .expect("ActorFactory must be available")
        .stop_actor(&stop_ctx, &session_actor_id)
        .await
        .expect("stop_actor must succeed");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let reactivated = actor_ask_json(
            &actor_service,
            tenant_id,
            app_id,
            "session-1:ephemeral",
            serde_json::json!({ "op": "status" }),
        )
        .await;
        if reactivated["count"] == serde_json::json!(5) {
            assert_eq!(reactivated["role"], serde_json::json!("ephemeral"));
            break;
        }
        assert!(
            std::time::Instant::now() <= deadline,
            "reactivation bug: expected count=5 after stop, got count={}",
            reactivated["count"]
        );
        sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[ignore = "requires Python toolchain and external WASM build"]
async fn test_python_wasm_nondurable_virtual_actor_reactivation() {
    let (node, _) = create_test_node_with_service().await;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let actor_service =
        ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());
    let app_id = "abstractions-python-sdk-it";
    let tenant_id = "test-tenant";

    let wasm_bytes = build_python_abstractions_example_wasm();
    let wasm_module = WasmModule {
        name: app_id.to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes,
        module_hash: String::new(),
        ..Default::default()
    };

    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: Some(ProstDuration {
            seconds: 60,
            nanos: 0,
        }),
        children: vec![ChildSpec {
            actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: "ephemeral".to_string(),
                actor_type: "abstractions_wasm".to_string(),
            }),

            role: "worker".to_string(),
            args: HashMap::from([
                ("role".to_string(), "ephemeral".to_string()),
                ("initial_count".to_string(), "5".to_string()),
            ]),
            restart: RestartPolicy::RestartPolicyPermanent.into(),
            shutdown_timeout: Some(ProstDuration {
                seconds: 5,
                nanos: 0,
            }),
            facets: vec![Facet {
                r#type: "virtual_actor".to_string(),
                config: HashMap::from([
                    ("idle_timeout".to_string(), "10m".to_string()),
                    ("activation_strategy".to_string(), "lazy".to_string()),
                ]),
                priority: 0,
                state: HashMap::new(),
                metadata: None,
            }],
            behavior_kind: Some("GenServer".to_string()),
            ..Default::default()
        }],
        ..Default::default()
    };

    let app_spec = ApplicationSpec {
        name: app_id.to_string(),
        tenant_id: String::new(),
        version: "1.0.0".to_string(),
        description: "Python SDK nondurable virtual actor reactivation".to_string(),
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
        static_mount: String::new(),
    };

    let deploy_request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: app_id.to_string(),
        name: app_id.to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        initial_state: vec![],
    };
    let deploy_response = service
        .deploy_application(app_request_in_scope(deploy_request, tenant_id, app_id))
        .await
        .expect("deployment should succeed");
    assert!(deploy_response.into_inner().success);

    let initial_status = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "session-1:ephemeral",
        serde_json::json!({ "op": "status" }),
    )
    .await;
    assert_eq!(initial_status["count"], serde_json::json!(5));
    assert_eq!(initial_status["role"], serde_json::json!("ephemeral"));

    let incremented = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "session-1:ephemeral",
        serde_json::json!({ "op": "increment", "amount": 2 }),
    )
    .await;
    assert_eq!(incremented["count"], serde_json::json!(7));

    let session_id_str = initial_status["self_id"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            plexspaces_actor::ActorId::new(
                "session-1",
                "abstractions_wasm",
                app_id,
                node.id().as_str(),
            )
            .unwrap()
            .to_string()
        });
    let session_actor_id = plexspaces_actor::ActorId::from_canonical(&session_id_str)
        .expect("self_id must be a valid canonical actor ID");
    let stop_ctx =
        plexspaces_actor::RequestContext::new_without_auth(String::new(), app_id.to_string());
    node.service_locator()
        .get_actor_factory()
        .await
        .expect("ActorFactory must be available")
        .stop_actor(&stop_ctx, &session_actor_id)
        .await
        .expect("stop_actor must succeed");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let reactivated = actor_ask_json(
            &actor_service,
            tenant_id,
            app_id,
            "session-1:ephemeral",
            serde_json::json!({ "op": "status" }),
        )
        .await;
        if reactivated["count"] == serde_json::json!(5) {
            assert_eq!(reactivated["role"], serde_json::json!("ephemeral"));
            break;
        }
        assert!(
            std::time::Instant::now() <= deadline,
            "reactivation bug: expected count=5 after stop, got count={}",
            reactivated["count"]
        );
        sleep(Duration::from_millis(100)).await;
    }
}

fn load_dispatch_test_wasm() -> Vec<u8> {
    let fixture_path = format!(
        "{}/../wasm-runtime/tests/fixtures/dispatch_test/dispatch_test_actor.wasm",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&fixture_path)
        .unwrap_or_else(|e| panic!("Failed to load dispatch_test_actor.wasm: {}", e))
}

#[tokio::test]
async fn test_multi_actor_dispatch_by_actor_type() {
    let (node, _) = create_test_node_with_service().await;
    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let actor_service =
        ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());

    let app_id = "dispatch-test-it";
    let tenant_id = "test-tenant";

    let wasm_bytes = load_dispatch_test_wasm();

    // Build ApplicationSpec inline — avoids any seed_nodes or cluster config.
    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: Some(ProstDuration {
            seconds: 60,
            nanos: 0,
        }),
        children: vec![
            ChildSpec {
                actor_identity: Some(ActorIdentity {
                    name: "ping".to_string(),
                    actor_type: "PingActor".to_string(),
                }),
                role: "ping".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 5,
                    nanos: 0,
                }),
                facets: vec![Facet {
                    r#type: "virtual_actor".to_string(),
                    config: HashMap::from([
                        ("idle_timeout".to_string(), "10m".to_string()),
                        ("activation_strategy".to_string(), "lazy".to_string()),
                    ]),
                    priority: 0,
                    state: HashMap::new(),
                    metadata: None,
                }],
                behavior_kind: Some("GenServer".to_string()),
                ..Default::default()
            },
            ChildSpec {
                actor_identity: Some(ActorIdentity {
                    name: "echo".to_string(),
                    actor_type: "EchoActor".to_string(),
                }),
                role: "echo".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 5,
                    nanos: 0,
                }),
                facets: vec![Facet {
                    r#type: "virtual_actor".to_string(),
                    config: HashMap::from([
                        ("idle_timeout".to_string(), "10m".to_string()),
                        ("activation_strategy".to_string(), "lazy".to_string()),
                    ]),
                    priority: 0,
                    state: HashMap::new(),
                    metadata: None,
                }],
                behavior_kind: Some("GenServer".to_string()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let app_spec = ApplicationSpec {
        name: app_id.to_string(),
        tenant_id: String::new(),
        version: "1.0.0".to_string(),
        description: "Multi-actor dispatch integration test".to_string(),
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
        static_mount: String::new(),
    };

    service
        .deploy_application(app_request_in_scope(
            DeployApplicationRequest {
                request_id: ulid::Ulid::new().to_string(),
                application_id: app_id.to_string(),
                name: app_id.to_string(),
                version: "1.0.0".to_string(),
                wasm_module: Some(WasmModule {
                    name: app_id.to_string(),
                    version: "1.0.0".to_string(),
                    module_bytes: wasm_bytes,
                    module_hash: String::new(),
                    ..Default::default()
                }),
                config: Some(app_spec),
                initial_state: vec![],
            },
            tenant_id,
            app_id,
        ))
        .await
        .expect("deployment should succeed");

    // PingActor receives "ping" — handler unique to PingActor.
    let ping_reply = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "PingActor:ping",
        serde_json::json!({ "op": "ping" }),
    )
    .await;
    assert!(
        ping_reply.get("error").is_none(),
        "PingActor should handle 'ping'; wrong dispatch returns an error. Got: {:?}",
        ping_reply
    );
    assert_eq!(
        ping_reply["pong"],
        serde_json::json!(true),
        "PingActor 'ping' must return pong=true, got: {:?}",
        ping_reply
    );

    // EchoActor receives "echo" — handler unique to EchoActor.
    let echo_reply = actor_ask_json(
        &actor_service,
        tenant_id,
        app_id,
        "EchoActor:echo",
        serde_json::json!({ "op": "echo", "message": "hello" }),
    )
    .await;
    assert!(
        echo_reply.get("error").is_none(),
        "EchoActor should handle 'echo'; wrong dispatch returns an error. Got: {:?}",
        echo_reply
    );
    assert_eq!(
        echo_reply["echoed"],
        serde_json::json!("hello"),
        "EchoActor 'echo' must return echoed=hello, got: {:?}",
        echo_reply
    );
}

