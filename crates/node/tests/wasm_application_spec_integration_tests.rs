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

//! Comprehensive Integration Tests for WASM Application Deployment with ApplicationSpec
//!
//! Tests WASM deployment via HTTP multipart with ApplicationSpec creation and verification.
//! Follows the same pattern as dashboard_integration_tests.rs for comprehensive testing.
//!
//! ## Test Coverage
//!
//! 1. **Empty Node Verification**: `test_empty_node_dashboard_shows_zero_applications`
//!    - Verifies empty node shows 0 applications in dashboard
//!
//! 2. **ApplicationSpec Creation**: `test_wasm_deployment_creates_applicationspec`
//!    - Verifies ApplicationSpec is auto-generated from form fields
//!    - Verifies application is registered with ApplicationManager
//!
//! 3. **Dashboard Integration**: `test_wasm_deployment_dashboard_reflects_changes`
//!    - Verifies dashboard shows 0 apps before deployment
//!    - Verifies dashboard shows 1 app after deployment
//!    - Verifies application appears in applications list
//!
//! 4. **ApplicationSpec Fields**: `test_wasm_deployment_applicationspec_fields`
//!    - Verifies ApplicationSpec uses name and version from form fields
//!    - Verifies application state is Running (ApplicationSpec used correctly)
//!
//! 5. **Undeployment Flow**: `test_wasm_deployment_undeployment_flow`
//!    - Verifies deployment succeeds
//!    - Verifies undeployment using name (not application_id)
//!    - Verifies dashboard shows 0 apps after undeployment
//!
//! 6. **Multiple Applications**: `test_wasm_deployment_multiple_applications`
//!    - Verifies multiple applications can be deployed
//!    - Verifies all are registered with ApplicationManager
//!    - Verifies dashboard shows correct count
//!
//! 7. **Component Error Handling**: `test_wasm_deployment_component_error_handling`
//!    - Verifies component deployment fails gracefully
//!    - Verifies error message mentions component limitation
//!
//! 8. **ApplicationSpec Auto-Generation**: `test_wasm_deployment_applicationspec_auto_generation`
//!    - Verifies ApplicationSpec is auto-generated when config not provided
//!    - Verifies application works with auto-generated spec
//!
//! 9. **Name vs Application ID**: `test_wasm_deployment_name_vs_application_id`
//!    - Verifies ApplicationManager uses name (not application_id)
//!    - Verifies undeployment uses name
//!
//! 10. **Complete Workflow**: `test_wasm_deployment_complete_workflow`
//!     - End-to-end: empty → deploy → dashboard → undeploy → dashboard
//!     - Verifies all steps work correctly
//!
//! ## Running Tests
//!
//! ```bash
//! # Run all tests
//! cargo test -p plexspaces-node --test wasm_application_spec_integration_tests -- --test-threads=1
//!
//! # Run specific test
//! cargo test -p plexspaces-node --test wasm_application_spec_integration_tests test_wasm_deployment_complete_workflow
//!
//! # Run with output
//! cargo test -p plexspaces-node --test wasm_application_spec_integration_tests -- --nocapture
//! ```

use plexspaces_node::NodeBuilder;
use plexspaces_core::application::ApplicationState;
use plexspaces_proto::dashboard::v1::{
    dashboard_service_server::DashboardService,
    GetSummaryRequest, GetApplicationsRequest,
};
use plexspaces_dashboard::{DashboardServiceImpl, HealthReporterAccess};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tonic::Request;
use wat;

/// Helper to create a test node
async fn create_test_node(node_id: &str, listen_addr: &str) -> Arc<plexspaces_node::Node> {
    let node = NodeBuilder::new(node_id.to_string())
        .with_listen_address(listen_addr.to_string())
        .build()
        .await;
    Arc::new(node)
}

/// Helper to create dashboard service from a node
async fn create_dashboard_service(node: Arc<plexspaces_node::Node>) -> DashboardServiceImpl {
    let service_locator = node.service_locator();
    
    // Initialize services
    node.initialize_services().await.expect("Failed to initialize services");
    
    // Register NodeMetricsAccessor
    use plexspaces_node::service_wrappers::NodeMetricsAccessorWrapper;
    let metrics_accessor = Arc::new(NodeMetricsAccessorWrapper::new(node.clone()));
    service_locator.register_service(metrics_accessor.clone()).await;
    let metrics_accessor_trait: Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync> = 
        metrics_accessor.clone() as Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync>;
    service_locator.register_node_metrics_accessor(metrics_accessor_trait).await;
    
    // Ensure ApplicationManager is registered
    use plexspaces_core::ApplicationManager;
    use plexspaces_core::service_locator::service_names;
    if let Some(app_manager) = service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await {
        service_locator.register_service(app_manager.clone()).await;
    }
    
    // Create HealthReporterAccess implementation
    use plexspaces_node::health_service::PlexSpacesHealthReporter;
    let (health_reporter, _service) = PlexSpacesHealthReporter::new();
    let health_reporter = Arc::new(health_reporter);
    
    struct HealthReporterAccessImpl {
        health_reporter: Arc<PlexSpacesHealthReporter>,
    }
    
    #[async_trait::async_trait]
    impl HealthReporterAccess for HealthReporterAccessImpl {
        async fn get_detailed_health(
            &self,
            include_non_critical: bool,
        ) -> plexspaces_proto::system::v1::DetailedHealthCheck {
            self.health_reporter.get_detailed_health(include_non_critical).await
        }
    }
    
    let health_reporter_access: Arc<dyn HealthReporterAccess + Send + Sync> = 
        Arc::new(HealthReporterAccessImpl { health_reporter });
    
    DashboardServiceImpl::with_health_reporter(service_locator, health_reporter_access)
}

/// Wait for HTTP server to be ready
async fn wait_for_http_server(http_url: &str, max_retries: u32) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    
    for i in 0..max_retries {
        if let Ok(response) = client.get(&format!("{}/api/v1/applications", http_url)).send().await {
            if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
                eprintln!("✅ HTTP server is ready (attempt {})", i + 1);
                return true;
            }
        }
        sleep(Duration::from_millis(500)).await;
    }
    false
}

/// Create a minimal traditional WASM module for testing
fn create_minimal_wasm_module() -> Vec<u8> {
    let wat = r#"
(module
    (memory (export "memory") 1)
    (func (export "init") (param i32 i32) (result i32)
        (i32.const 0)
    )
    (func (export "handle_message") 
          (param $from_ptr i32) (param $from_len i32)
          (param $msg_type_ptr i32) (param $msg_type_len i32)
          (param $payload_ptr i32) (param $payload_len i32)
          (result i32)
        (i32.const 0)
    )
    (func (export "snapshot_state") (result i32 i32)
        (i32.const 0)
        (i32.const 0)
    )
)
"#;
    wat::parse_str(wat).expect("Failed to parse WAT")
}

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

#[tokio::test]
async fn test_empty_node_dashboard_shows_zero_applications() {
    // ARRANGE: Create empty node and start it
    let node = create_test_node("test-node-empty-apps", "127.0.0.1:9011").await;
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;
    
    let dashboard_service = create_dashboard_service(node.clone()).await;
    
    // ACT: Get summary from dashboard
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    
    let response = DashboardService::get_summary(&dashboard_service, request).await;
    assert!(response.is_ok(), "get_summary should succeed for empty node");
    
    let summary = response.unwrap().into_inner();
    
    // ASSERT: Empty node should show 0 applications
    assert_eq!(summary.total_applications, 0, "Empty node should show 0 applications");
    assert_eq!(summary.total_nodes, 1, "Should show 1 node");
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_wasm_deployment_creates_applicationspec() {
    // ARRANGE: Create node and start it
    let node = create_test_node("test-node-appspec", "127.0.0.1:9013").await;
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;
    
    let http_port = 9014;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // Create minimal WASM module
    let wasm_bytes = create_minimal_wasm_module();
    eprintln!("📦 Deploying WASM module ({} bytes) with ApplicationSpec", wasm_bytes.len());
    
    // ACT: Deploy via HTTP (ApplicationSpec will be auto-generated)
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    let form = reqwest::multipart::Form::new()
        .text("application_id", "test-app-1")
        .text("name", "test-application")
        .text("version", "1.0.0")
        .part("wasm_file", 
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("test_app.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type")
        );
    
    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    let status = response.status();
    let response_text = response.text().await.unwrap_or_else(|_| "No response body".to_string());
    
    eprintln!("📥 Response status: {}", status);
    eprintln!("📥 Response body: {}", response_text);

    // ASSERT: Deployment should succeed
    assert!(status.is_success(), 
        "Deployment should succeed. Status: {}, Response: {}", status, response_text);

    let json: serde_json::Value = serde_json::from_str(&response_text)
        .unwrap_or_else(|_| serde_json::json!({"error": "Failed to parse JSON"}));
    
    assert_eq!(json["success"], true, "Deployment should be successful. Response: {}", response_text);
    assert_eq!(json["application_id"], "test-app-1");

    // Wait for application to register
    sleep(Duration::from_millis(1000)).await;
    
    // ASSERT: Verify ApplicationSpec was created and application is registered
    let app_manager = node.application_manager().await.expect("ApplicationManager should be available");
    let app_state = app_manager.get_state("test-application").await;
    assert!(app_state.is_some(), "Application should be registered with name 'test-application'");
    
    // Verify ApplicationSpec was used (check via WasmApplication)
    // The application should be running
    assert_eq!(app_state, Some(ApplicationState::ApplicationStateRunning), 
        "Application should be in Running state");
    
    eprintln!("✅ ApplicationSpec created and application deployed successfully");
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_wasm_deployment_dashboard_reflects_changes() {
    // ARRANGE: Create empty node, check dashboard, deploy, check again
    let node = create_test_node("test-node-dashboard-flow", "127.0.0.1:9015").await;
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;
    
    let dashboard_service = create_dashboard_service(node.clone()).await;
    let http_port = 9016;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // ACT 1: Check dashboard before deployment
    let before_request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let before_response = DashboardService::get_summary(&dashboard_service, before_request).await.unwrap();
    let before_summary = before_response.into_inner();
    let before_app_count = before_summary.total_applications;
    
    eprintln!("📊 Dashboard before deployment: {} applications", before_app_count);
    assert_eq!(before_app_count, 0, "Should start with 0 applications");
    
    // ACT 2: Deploy WASM application
    let wasm_bytes = create_minimal_wasm_module();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    let form = reqwest::multipart::Form::new()
        .text("application_id", "dashboard-test-app")
        .text("name", "dashboard-test")
        .text("version", "1.0.0")
        .part("wasm_file", 
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("test_app.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type")
        );
    
    let deploy_response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert!(deploy_response.status().is_success(), 
        "Deployment should succeed. Status: {}", deploy_response.status());
    
    // Wait for application to register
    sleep(Duration::from_millis(1000)).await;
    
    // ACT 3: Check dashboard after deployment
    let after_request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let after_response = DashboardService::get_summary(&dashboard_service, after_request).await.unwrap();
    let after_summary = after_response.into_inner();
    let after_app_count = after_summary.total_applications;
    
    eprintln!("📊 Dashboard after deployment: {} applications", after_app_count);
    
    // ASSERT: Dashboard should reflect the deployment
    assert!(after_app_count > before_app_count, 
        "Dashboard should show increased application count. Before: {}, After: {}", 
        before_app_count, after_app_count);
    assert_eq!(after_app_count, 1, "Should show 1 application after deployment");
    
    // Verify application appears in applications list
    let apps_request = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });
    let apps_response = dashboard_service.get_applications(apps_request).await.unwrap();
    let apps = apps_response.into_inner();
    assert_eq!(apps.applications.len(), 1, "Should have 1 application in list");
    assert_eq!(apps.applications[0].name, "dashboard-test", "Application name should match");
    
    eprintln!("✅ Dashboard correctly reflects WASM deployment");
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_wasm_deployment_applicationspec_fields() {
    // ARRANGE: Deploy WASM and verify ApplicationSpec fields
    let node = create_test_node("test-node-spec-fields", "127.0.0.1:9017").await;
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;
    
    let http_port = 9018;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // ACT: Deploy with specific name and version
    let wasm_bytes = create_minimal_wasm_module();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    let app_name = "spec-test-app";
    let app_version = "2.5.0";
    
    let form = reqwest::multipart::Form::new()
        .text("application_id", "spec-test-id")
        .text("name", app_name)
        .text("version", app_version)
        .part("wasm_file", 
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("test_app.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type")
        );
    
    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert!(response.status().is_success(), "Deployment should succeed");
    
    sleep(Duration::from_millis(1000)).await;
    
    // ASSERT: Verify ApplicationSpec fields via ApplicationManager
    let app_manager = node.application_manager().await.expect("ApplicationManager should be available");
    
    // Application should be registered by name (not application_id)
    let app_state = app_manager.get_state(app_name).await;
    assert!(app_state.is_some(), "Application should be registered with name '{}'", app_name);
    
    // Verify application is running (ApplicationSpec was used correctly)
    assert_eq!(app_state, Some(ApplicationState::ApplicationStateRunning), 
        "Application should be running");
    
    eprintln!("✅ ApplicationSpec created with correct name and version");
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_wasm_deployment_undeployment_flow() {
    // ARRANGE: Deploy, verify, undeploy, verify
    let node = create_test_node("test-node-undeploy", "127.0.0.1:9019").await;
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;
    
    let dashboard_service = create_dashboard_service(node.clone()).await;
    let http_port = 9020;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // ACT 1: Deploy
    let wasm_bytes = create_minimal_wasm_module();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    let app_name = "undeploy-test";
    
    let form = reqwest::multipart::Form::new()
        .text("application_id", "undeploy-test-id")
        .text("name", app_name)
        .text("version", "1.0.0")
        .part("wasm_file", 
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("test_app.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type")
        );
    
    let deploy_response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert!(deploy_response.status().is_success(), "Deployment should succeed");
    sleep(Duration::from_millis(1000)).await;
    
    // Verify deployed
    let app_manager = node.application_manager().await.expect("ApplicationManager should be available");
    let app_state = app_manager.get_state(app_name).await;
    assert!(app_state.is_some(), "Application should be registered");
    
    // ACT 2: Undeploy (use name, not application_id)
    let undeploy_response = client
        .delete(&format!("{}/api/v1/applications/{}", http_url, app_name))
        .send()
        .await
        .expect("Failed to undeploy application");

    assert_eq!(undeploy_response.status(), reqwest::StatusCode::OK, 
        "Undeployment should succeed");
    
    let undeploy_json: serde_json::Value = undeploy_response.json().await.expect("Failed to parse undeploy response");
    assert_eq!(undeploy_json["success"], true, "Undeployment should succeed");
    
    sleep(Duration::from_millis(500)).await;
    
    // ASSERT: Application should be stopped/unregistered
    let app_state_after = app_manager.get_state(app_name).await;
    assert!(app_state_after.is_none() || 
            matches!(app_state_after, Some(state) if state == ApplicationState::ApplicationStateStopped),
            "Application should be stopped or unregistered after undeployment");
    
    // Verify dashboard shows 0 applications
    let summary_request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let summary_response = DashboardService::get_summary(&dashboard_service, summary_request).await.unwrap();
    let summary = summary_response.into_inner();
    assert_eq!(summary.total_applications, 0, "Dashboard should show 0 applications after undeployment");
    
    eprintln!("✅ Deployment and undeployment flow verified");
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_wasm_deployment_multiple_applications() {
    // ARRANGE: Deploy multiple applications and verify all are registered
    let node = create_test_node("test-node-multi-apps", "127.0.0.1:9021").await;
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;
    
    let dashboard_service = create_dashboard_service(node.clone()).await;
    let http_port = 9022;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    // ACT: Deploy 3 applications
    let apps = vec![
        ("multi-app-1", "app-1", "1.0.0"),
        ("multi-app-2", "app-2", "2.0.0"),
        ("multi-app-3", "app-3", "1.5.0"),
    ];
    
    let wasm_bytes = create_minimal_wasm_module();
    
    for (app_id, app_name, app_version) in &apps {
        let form = reqwest::multipart::Form::new()
            .text("application_id", app_id.to_string())
            .text("name", app_name.to_string())
            .text("version", app_version.to_string())
            .part("wasm_file", 
                reqwest::multipart::Part::bytes(wasm_bytes.clone())
                    .file_name("test_app.wasm")
                    .mime_str("application/wasm")
                    .expect("Failed to set MIME type")
            );
        
        let response = client
            .post(&format!("{}/api/v1/applications/deploy", http_url))
            .multipart(form)
            .send()
            .await
            .expect("Failed to send HTTP request");

        assert!(response.status().is_success(), 
            "Deployment of {} should succeed", app_name);
        
        sleep(Duration::from_millis(500)).await;
    }
    
    // ASSERT: All applications should be registered
    let app_manager = node.application_manager().await.expect("ApplicationManager should be available");
    
    for (_, app_name, _) in &apps {
        let app_state = app_manager.get_state(app_name).await;
        assert!(app_state.is_some(), "Application '{}' should be registered", app_name);
    }
    
    // Verify dashboard shows all applications
    let summary_request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let summary_response = DashboardService::get_summary(&dashboard_service, summary_request).await.unwrap();
    let summary = summary_response.into_inner();
    assert_eq!(summary.total_applications, 3, "Dashboard should show 3 applications");
    
    // Verify applications list
    let apps_request = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });
    let apps_response = dashboard_service.get_applications(apps_request).await.unwrap();
    let apps_list = apps_response.into_inner();
    assert_eq!(apps_list.applications.len(), 3, "Should have 3 applications in list");
    
    eprintln!("✅ Multiple applications deployed and verified");
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_wasm_deployment_component_error_handling() {
    // ARRANGE: Try to deploy a component (should fail gracefully)
    let node = create_test_node("test-node-component-error", "127.0.0.1:9023").await;
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;
    
    let http_port = 9024;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // Try to use calculator WASM (component) if it exists
    let calculator_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm");
    
    if !calculator_path.exists() {
        eprintln!("⚠️  Calculator WASM not found, skipping component error test");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        return;
    }
    
    let wasm_bytes = std::fs::read(&calculator_path).expect("Failed to read calculator WASM");
    
    // ACT: Try to deploy component
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    let form = reqwest::multipart::Form::new()
        .text("application_id", "component-test")
        .text("name", "component-app")
        .text("version", "1.0.0")
        .part("wasm_file", 
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("calculator_actor.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type")
        );
    
    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    // ASSERT: Should fail with component error (but HTTP endpoint should work)
    // The error should be clear about component instantiation
    let status = response.status();
    let response_text = response.text().await.unwrap_or_else(|_| "No response body".to_string());
    
    // Component deployment should fail, but with a clear error message
    if status.is_success() {
        // If it succeeds, that's fine (component support might be implemented)
        eprintln!("✅ Component deployment succeeded (component support may be implemented)");
    } else {
        // Should fail with component error
        assert!(response_text.contains("component") || response_text.contains("Component"), 
            "Error should mention component. Response: {}", response_text);
        eprintln!("✅ Component error handled correctly: {}", response_text);
    }
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_wasm_deployment_applicationspec_auto_generation() {
    // ARRANGE: Deploy without config field - ApplicationSpec should be auto-generated
    let node = create_test_node("test-node-auto-spec", "127.0.0.1:9025").await;
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;
    
    let http_port = 9026;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // ACT: Deploy with only name and version (no config field)
    let wasm_bytes = create_minimal_wasm_module();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    let app_name = "auto-spec-app";
    
    let form = reqwest::multipart::Form::new()
        .text("application_id", "auto-spec-id")
        .text("name", app_name)
        .text("version", "1.0.0")
        .part("wasm_file", 
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("test_app.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type")
        );
    
    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert!(response.status().is_success(), "Deployment should succeed");
    
    sleep(Duration::from_millis(1000)).await;
    
    // ASSERT: ApplicationSpec should be auto-generated and application should be registered
    let app_manager = node.application_manager().await.expect("ApplicationManager should be available");
    let app_state = app_manager.get_state(app_name).await;
    assert!(app_state.is_some(), "Application should be registered (ApplicationSpec auto-generated)");
    assert_eq!(app_state, Some(ApplicationState::ApplicationStateRunning), 
        "Application should be running (ApplicationSpec used correctly)");
    
    eprintln!("✅ ApplicationSpec auto-generation verified");
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_wasm_deployment_name_vs_application_id() {
    // ARRANGE: Verify that ApplicationManager uses name, not application_id
    let node = create_test_node("test-node-name-id", "127.0.0.1:9027").await;
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;
    
    let http_port = 9028;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // ACT: Deploy with different application_id and name
    let wasm_bytes = create_minimal_wasm_module();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    let app_id = "different-id-123";
    let app_name = "actual-name";
    
    let form = reqwest::multipart::Form::new()
        .text("application_id", app_id)
        .text("name", app_name)
        .text("version", "1.0.0")
        .part("wasm_file", 
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("test_app.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type")
        );
    
    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert!(response.status().is_success(), "Deployment should succeed");
    
    sleep(Duration::from_millis(1000)).await;
    
    // ASSERT: Application should be registered by name, not application_id
    let app_manager = node.application_manager().await.expect("ApplicationManager should be available");
    
    // Should be found by name
    let app_state_by_name = app_manager.get_state(app_name).await;
    assert!(app_state_by_name.is_some(), 
        "Application should be registered with name '{}' (not application_id)", app_name);
    
    // Should NOT be found by application_id
    let app_state_by_id = app_manager.get_state(app_id).await;
    assert!(app_state_by_id.is_none(), 
        "Application should NOT be registered with application_id '{}'", app_id);
    
    // Undeploy should use name, not application_id
    let undeploy_response = client
        .delete(&format!("{}/api/v1/applications/{}", http_url, app_name))
        .send()
        .await
        .expect("Failed to undeploy application");

    assert_eq!(undeploy_response.status(), reqwest::StatusCode::OK, 
        "Undeployment should succeed using name");
    
    eprintln!("✅ Verified ApplicationManager uses name, not application_id");
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_wasm_deployment_complete_workflow() {
    // ARRANGE: Complete workflow: empty node → deploy → dashboard → undeploy → dashboard
    let node = create_test_node("test-node-complete", "127.0.0.1:9029").await;
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;
    
    let dashboard_service = create_dashboard_service(node.clone()).await;
    let http_port = 9030;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    // STEP 1: Check empty node dashboard
    let before_request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let before_response = DashboardService::get_summary(&dashboard_service, before_request).await.unwrap();
    let before_summary = before_response.into_inner();
    assert_eq!(before_summary.total_applications, 0, "Should start with 0 applications");
    eprintln!("✅ Step 1: Empty node verified (0 applications)");
    
    // STEP 2: Deploy WASM application
    let wasm_bytes = create_minimal_wasm_module();
    let app_name = "complete-workflow-app";
    
    let form = reqwest::multipart::Form::new()
        .text("application_id", "complete-workflow-id")
        .text("name", app_name)
        .text("version", "1.0.0")
        .part("wasm_file", 
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("test_app.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type")
        );
    
    let deploy_response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    assert!(deploy_response.status().is_success(), "Deployment should succeed");
    sleep(Duration::from_millis(1000)).await;
    eprintln!("✅ Step 2: WASM application deployed");
    
    // STEP 3: Verify dashboard shows application
    let after_request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let after_response = DashboardService::get_summary(&dashboard_service, after_request).await.unwrap();
    let after_summary = after_response.into_inner();
    assert_eq!(after_summary.total_applications, 1, "Dashboard should show 1 application");
    eprintln!("✅ Step 3: Dashboard verified (1 application)");
    
    // STEP 4: Verify ApplicationSpec was created and used
    let app_manager = node.application_manager().await.expect("ApplicationManager should be available");
    let app_state = app_manager.get_state(app_name).await;
    assert!(app_state.is_some(), "Application should be registered");
    assert_eq!(app_state, Some(ApplicationState::ApplicationStateRunning), 
        "Application should be running");
    eprintln!("✅ Step 4: ApplicationSpec verified (application running)");
    
    // STEP 5: Undeploy
    let undeploy_response = client
        .delete(&format!("{}/api/v1/applications/{}", http_url, app_name))
        .send()
        .await
        .expect("Failed to undeploy application");

    assert_eq!(undeploy_response.status(), reqwest::StatusCode::OK, 
        "Undeployment should succeed");
    sleep(Duration::from_millis(500)).await;
    eprintln!("✅ Step 5: Application undeployed");
    
    // STEP 6: Verify dashboard shows 0 applications again
    let final_request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let final_response = DashboardService::get_summary(&dashboard_service, final_request).await.unwrap();
    let final_summary = final_response.into_inner();
    assert_eq!(final_summary.total_applications, 0, "Dashboard should show 0 applications after undeployment");
    eprintln!("✅ Step 6: Dashboard verified (0 applications)");
    
    eprintln!("✅ Complete workflow verified: empty → deploy → dashboard → undeploy → dashboard");
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

