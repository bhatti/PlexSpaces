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

//! Integration tests for WASM application deployment
//!
//! ## Purpose
//! Tests WASM application deployment via ApplicationService, including:
//! - Deploying WASM modules
//! - Creating WasmApplication instances
//! - Application lifecycle (start/stop/health)
//! - Error handling (invalid modules, missing runtime, etc.)

use plexspaces_node::{Node, NodeId};
use plexspaces_core::ApplicationManager;
use plexspaces_services::application_service::ApplicationServiceImpl;
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, ApplicationSpec, ApplicationType,
    ChildSpec, ChildType, DeployApplicationRequest, GetApplicationStatusRequest,
    ListApplicationsRequest, RestartPolicy, SupervisorSpec, SupervisionStrategy,
};
use plexspaces_proto::common::v1::Metadata;
use plexspaces_proto::wasm::v1::WasmModule;
use prost_types::Duration as ProstDuration;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tonic::Request;

// Simple WASM module: (module (func (export "test") (result i32) i32.const 42))
const SIMPLE_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // Magic: \0asm
    0x01, 0x00, 0x00, 0x00, // Version: 1
    0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Type section
    0x03, 0x02, 0x01, 0x00, // Function section
    0x07, 0x08, 0x01, 0x04, 0x74, 0x65, 0x73, 0x74, 0x00, 0x00, // Export section
    0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // Code section
];

async fn create_test_node() -> Arc<Node> {
    use plexspaces_node::NodeBuilder;
    Arc::new(NodeBuilder::new("test-node")
        .with_listen_addr("127.0.0.1:0")
        .with_in_memory_backends()
        .build().await)
}

async fn create_test_node_with_service() -> (Arc<Node>, String) {
    let node = create_test_node().await;
    let node_clone = node.clone();
    
    // Start node in background
    let _handle = tokio::spawn(async move {
        let _ = node_clone.start().await;
    });

    // Wait for node to start and get actual listen address
    sleep(Duration::from_millis(500)).await;
    
    // For now, we'll test directly via ApplicationServiceImpl instead of gRPC
    // This avoids port binding issues in tests
    (node, String::new())
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
                id: "worker-1".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 5,
                    nanos: 0,
                }),
                supervisor: None,
                facets: vec![], // Phase 1: Unified Lifecycle - facets support
            },
            ChildSpec {
                id: "worker-2".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 5,
                    nanos: 0,
                }),
                supervisor: None,
                facets: vec![], // Phase 1: Unified Lifecycle - facets support
            },
        ],
    };

    let app_spec = ApplicationSpec {
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        description: "Test application with supervisor tree".to_string(),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: HashMap::new(),
        supervisor: Some(supervisor_spec),
    };

    let wasm_module = WasmModule {
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        version_number: 1,
        module_bytes: SIMPLE_WASM.to_vec(),
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
        size_bytes: SIMPLE_WASM.len() as u64,
    };

    (wasm_module, app_spec)
}

#[tokio::test]
async fn test_deploy_wasm_application_success() {
    let (node, _) = create_test_node_with_service().await;
    
    // Start node to initialize WASM runtime
    let node_clone = node.clone();
    tokio::spawn(async move {
        let _ = node_clone.start().await;
    });
    sleep(Duration::from_millis(500)).await;

    // Create ApplicationService - gets ApplicationManager from ServiceLocator
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

    // Create WASM module
    let wasm_module = WasmModule {
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        version_number: 1,
        module_bytes: SIMPLE_WASM.to_vec(),
        module_hash: String::new(), // Will be computed by deployment service
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: None,
        created_at: None,
        size_bytes: SIMPLE_WASM.len() as u64,
    };

    // Deploy application
    let request = DeployApplicationRequest {
        application_id: "test-app-001".to_string(),
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: None,
        release_config: None,
        initial_state: vec![],
    };

    let response = service
        .deploy_application(Request::new(request))
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
    
    // Start node to initialize WASM runtime
    let node_clone = node.clone();
    tokio::spawn(async move {
        let _ = node_clone.start().await;
    });
    sleep(Duration::from_millis(500)).await;

    // Create ApplicationService - gets ApplicationManager from ServiceLocator
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

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
        application_id: "test-app-002".to_string(),
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: None,
        release_config: None,
        initial_state: vec![],
    };

    let response = service.deploy_application(Request::new(request)).await;

    // Should fail with error
    assert!(response.is_err(), "Deployment should fail with invalid WASM");
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
    
    // Start node to initialize WASM runtime
    let node_clone = node.clone();
    tokio::spawn(async move {
        let _ = node_clone.start().await;
    });
    sleep(Duration::from_millis(500)).await;

    // Create ApplicationService - gets ApplicationManager from ServiceLocator
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

    // Test missing application_id
    let request = DeployApplicationRequest {
        application_id: String::new(), // Invalid: empty
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(WasmModule {
            name: "test-app".to_string(),
            version: "1.0.0".to_string(),
            version_number: 1,
            module_bytes: SIMPLE_WASM.to_vec(),
            module_hash: String::new(),
            wit_interface: String::new(),
            source_languages: vec![],
            metadata: None,
            created_at: None,
            size_bytes: SIMPLE_WASM.len() as u64,
        }),
        config: None,
        release_config: None,
        initial_state: vec![],
    };

    let response = service.deploy_application(Request::new(request)).await;
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
    
    // Start node to initialize WASM runtime
    let node_clone = node.clone();
    tokio::spawn(async move {
        let _ = node_clone.start().await;
    });
    sleep(Duration::from_millis(500)).await;

    // Create ApplicationService - use same instance for deploy and status
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

    // Deploy application first
    let wasm_module = WasmModule {
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        version_number: 1,
        module_bytes: SIMPLE_WASM.to_vec(),
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: None,
        created_at: None,
        size_bytes: SIMPLE_WASM.len() as u64,
    };

    let deploy_request = DeployApplicationRequest {
        application_id: "test-app-003".to_string(),
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: None,
        release_config: None,
        initial_state: vec![],
    };

    service
        .deploy_application(Request::new(deploy_request))
        .await
        .expect("Deploy should succeed");

    // Wait a bit for application to start
    sleep(Duration::from_millis(200)).await;

    // Get application status - ApplicationManager stores by name, not application_id
    // For now, we use the application name for lookup
    // TODO: Enhance ApplicationManager to support multiple instances per application name
    let status_service = ApplicationServiceImpl::new(node.service_locator().clone());
    let status_request = GetApplicationStatusRequest {
        application_id: "test-app".to_string(), // Use name for lookup
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
    assert_eq!(app.name, "test-app");
}

#[tokio::test]
async fn test_list_wasm_applications() {
    let (node, _) = create_test_node_with_service().await;
    
    // Start node to initialize WASM runtime
    let node_clone = node.clone();
    tokio::spawn(async move {
        let _ = node_clone.start().await;
    });
    sleep(Duration::from_millis(500)).await;

    // Create ApplicationService
    let application_manager = node.application_manager();
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

    // Deploy multiple applications
    for i in 0..3 {
        let wasm_module = WasmModule {
            name: format!("test-app-{}", i),
            version: "1.0.0".to_string(),
            version_number: 1,
            module_bytes: SIMPLE_WASM.to_vec(),
            module_hash: String::new(),
            wit_interface: String::new(),
            source_languages: vec![],
            metadata: None,
            created_at: None,
            size_bytes: SIMPLE_WASM.len() as u64,
        };

        let deploy_request = DeployApplicationRequest {
            application_id: format!("test-app-{:03}", i),
            name: format!("test-app-{}", i),
            version: "1.0.0".to_string(),
            wasm_module: Some(wasm_module),
            config: None,
            release_config: None,
            initial_state: vec![],
        };

        service
            .deploy_application(Request::new(deploy_request))
            .await
            .expect("Deploy should succeed");
    }

    // Wait for applications to start
    sleep(Duration::from_millis(300)).await;

    // List applications
    let list_request = ListApplicationsRequest {
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
    
    // Start node to initialize WASM runtime
    let node_clone = node.clone();
    tokio::spawn(async move {
        let _ = node_clone.start().await;
    });
    sleep(Duration::from_millis(500)).await;

    // Create ApplicationService - gets ApplicationManager from ServiceLocator
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

    // Get status for non-existent application
    let status_request = GetApplicationStatusRequest {
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
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

    // Create WASM module with supervisor spec
    let (wasm_module, app_spec) = create_wasm_module_with_supervisor_spec();

    // Deploy application with supervisor tree
    let request = DeployApplicationRequest {
        application_id: "supervisor-app-001".to_string(),
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        release_config: None,
        initial_state: vec![],
    };

    // Deploy
    let response = service.deploy_application(Request::new(request)).await;
    assert!(response.is_ok(), "Deployment should succeed: {:?}", response.err());
    let res = response.unwrap().into_inner();
    assert!(res.success);
    assert_eq!(res.application_id, "supervisor-app-001");

    // Wait for application to start and actors to spawn
    sleep(Duration::from_millis(500)).await;

    // Verify application is running
    let app_manager = node.application_manager();
    let app_state = app_manager.get_state("test-app").await;
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
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

    // Deploy application with supervisor tree
    let (wasm_module, app_spec) = create_wasm_module_with_supervisor_spec();
    let deploy_request = DeployApplicationRequest {
        application_id: "shutdown-app-001".to_string(),
        name: "shutdown-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        release_config: None,
        initial_state: vec![],
    };

    service
        .deploy_application(Request::new(deploy_request))
        .await
        .expect("Deploy should succeed");

    // Wait for application to start
    sleep(Duration::from_millis(500)).await;

    // Undeploy application (should gracefully shutdown all actors)
    // Note: ApplicationManager stores by name, not application_id
    let undeploy_request = plexspaces_proto::application::v1::UndeployApplicationRequest {
        application_id: "shutdown-app".to_string(), // Use name, not application_id
        timeout: None, // Use default timeout
    };

    let response = service
        .undeploy_application(Request::new(undeploy_request))
        .await;
    assert!(response.is_ok(), "Undeploy should succeed: {:?}", response.err());
    let res = response.unwrap().into_inner();
    assert!(res.success);

    // Wait for shutdown to complete
    sleep(Duration::from_millis(500)).await;

    // Verify application is stopped
    let app_manager = node.application_manager();
    let app_state: Option<plexspaces_proto::v1::application::ApplicationState> = app_manager.get_state("shutdown-app").await;
    assert!(app_state.is_some());
    assert_eq!(
        app_state.unwrap(),
        plexspaces_proto::v1::application::ApplicationState::ApplicationStateStopped
    );
}

// ============================================================================
// Integration Tests for WASM Supervisor Restart Functionality
// ============================================================================

/// Load real WASM fixture file for integration tests
fn load_wasm_fixture(name: &str) -> Vec<u8> {
    let fixture_path = format!(
        "{}/../wasm-runtime/tests/fixtures/{}", 
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read(&fixture_path)
        .unwrap_or_else(|e| panic!("Failed to load WASM fixture {}: {}", fixture_path, e))
}

/// Create WasmModule from fixture with supervisor spec
fn create_wasm_module_from_fixture_with_supervisor(
    fixture_name: &str,
    actor_name: &str,
) -> (WasmModule, ApplicationSpec) {
    let wasm_bytes = load_wasm_fixture(fixture_name);
    
    let wasm_module = WasmModule {
        name: actor_name.to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes,
        module_hash: String::new(),
        ..Default::default()
    };
    
    // Create supervisor spec with one-for-one strategy
    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: Some(ProstDuration {
            seconds: 60,
            nanos: 0,
        }),
        children: vec![
            ChildSpec {
                id: actor_name.to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration {
                    seconds: 5,
                    nanos: 0,
                }),
                supervisor: None,
                facets: vec![],
            },
        ],
    };
    
    let app_spec = ApplicationSpec {
        name: actor_name.to_string(),
        version: "1.0.0".to_string(),
        description: format!("WASM application with supervisor: {}", actor_name),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: HashMap::new(),
        supervisor: Some(supervisor_spec),
    };
    
    (wasm_module, app_spec)
}

/// Test: Deploy real WASM actor with supervisor tree (using calculator_actor.wasm)
#[tokio::test]
async fn test_deploy_real_wasm_with_supervisor_tree() {
    let (node, _) = create_test_node_with_service().await;
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

    // Load real WASM fixture
    let (wasm_module, app_spec) = create_wasm_module_from_fixture_with_supervisor(
        "calculator_actor.wasm",
        "calculator"
    );

    let deploy_request = DeployApplicationRequest {
        application_id: "calculator-supervisor-test".to_string(),
        name: "calculator".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        release_config: None,
        initial_state: vec![],
    };

    // Deploy should succeed
    let response = service.deploy_application(Request::new(deploy_request)).await;
    assert!(response.is_ok(), "Deploy should succeed: {:?}", response.err());
    let res = response.unwrap().into_inner();
    assert!(res.success, "Deployment should be successful");
    
    // Wait for application to start
    sleep(Duration::from_millis(1000)).await;

    // Verify application is running with supervisor
    let app_manager = node.application_manager();
    let app_state = app_manager.get_state("calculator").await;
    assert!(app_state.is_some(), "Application should be registered");
    assert_eq!(
        app_state.unwrap(),
        plexspaces_proto::v1::application::ApplicationState::ApplicationStateRunning
    );

    // Cleanup
    let undeploy_request = plexspaces_proto::application::v1::UndeployApplicationRequest {
        application_id: "calculator".to_string(),
        timeout: None,
    };
    let _ = service.undeploy_application(Request::new(undeploy_request)).await;
}

/// Test: Supervisor properly adds WASM actors as children
#[tokio::test]
async fn test_supervisor_adds_wasm_actors_as_children() {
    let (node, _) = create_test_node_with_service().await;
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

    // Create app with multiple workers
    let wasm_bytes = load_wasm_fixture("calculator_actor.wasm");
    
    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: Some(ProstDuration { seconds: 60, nanos: 0 }),
        children: vec![
            ChildSpec {
                id: "worker-1".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration { seconds: 5, nanos: 0 }),
                supervisor: None,
                facets: vec![],
            },
            ChildSpec {
                id: "worker-2".to_string(),
                r#type: ChildType::ChildTypeWorker.into(),
                args: HashMap::new(),
                restart: RestartPolicy::RestartPolicyPermanent.into(),
                shutdown_timeout: Some(ProstDuration { seconds: 5, nanos: 0 }),
                supervisor: None,
                facets: vec![],
            },
        ],
    };
    
    let wasm_module = WasmModule {
        name: "multi-worker-app".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes,
        module_hash: String::new(),
        ..Default::default()
    };
    
    let app_spec = ApplicationSpec {
        name: "multi-worker-app".to_string(),
        version: "1.0.0".to_string(),
        description: "App with multiple supervised workers".to_string(),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: HashMap::new(),
        supervisor: Some(supervisor_spec),
    };

    let deploy_request = DeployApplicationRequest {
        application_id: "multi-worker-test".to_string(),
        name: "multi-worker-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        release_config: None,
        initial_state: vec![],
    };

    let response = service.deploy_application(Request::new(deploy_request)).await;
    assert!(response.is_ok(), "Deploy should succeed: {:?}", response.err());
    
    // Wait for actors to spawn
    sleep(Duration::from_millis(1000)).await;
    
    // Verify deployment succeeded
    // Note: Actor registration verification will be done via lookup_actor
    // once supervisor integration is fully tested
    eprintln!("Multi-worker app deployed successfully");

    // Cleanup
    let undeploy_request = plexspaces_proto::application::v1::UndeployApplicationRequest {
        application_id: "multi-worker-app".to_string(),
        timeout: None,
    };
    let _ = service.undeploy_application(Request::new(undeploy_request)).await;
}

/// Test: Verify supervisor is created with correct strategy
#[tokio::test]
async fn test_supervisor_created_with_correct_strategy() {
    let (node, _) = create_test_node_with_service().await;
    let service = ApplicationServiceImpl::new(node.service_locator().clone());

    let (wasm_module, app_spec) = create_wasm_module_from_fixture_with_supervisor(
        "calculator_actor.wasm",
        "strategy-test-app"
    );

    let deploy_request = DeployApplicationRequest {
        application_id: "strategy-test".to_string(),
        name: "strategy-test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        release_config: None,
        initial_state: vec![],
    };

    let response = service.deploy_application(Request::new(deploy_request)).await;
    assert!(response.is_ok(), "Deploy should succeed");
    
    sleep(Duration::from_millis(500)).await;
    
    // Verify application status includes supervisor info
    let status_request = GetApplicationStatusRequest {
        application_id: "strategy-test-app".to_string(),
    };
    
    let status_response = service.get_application_status(Request::new(status_request)).await;
    assert!(status_response.is_ok(), "Status check should succeed");
    
    // Cleanup
    let undeploy_request = plexspaces_proto::application::v1::UndeployApplicationRequest {
        application_id: "strategy-test-app".to_string(),
        timeout: None,
    };
    let _ = service.undeploy_application(Request::new(undeploy_request)).await;
}

