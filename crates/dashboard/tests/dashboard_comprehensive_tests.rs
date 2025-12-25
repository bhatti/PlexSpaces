// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Comprehensive Integration Tests for Dashboard
//
// Tests all dashboard functionality including:
// - Home page data (summary, nodes, applications, actors, workflows)
// - Node page data (metrics, applications, actors, workflows)
// - WASM deployment and verification
// - Data consistency after deployment
//
// To run:
//   cargo test -p plexspaces-dashboard --test dashboard_comprehensive_tests -- --test-threads=1

use plexspaces_dashboard::DashboardServiceImpl;
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_core::{RequestContext, application::{Application, ApplicationError, ApplicationNode}};
use plexspaces_proto::dashboard::v1::{
    dashboard_service_server::DashboardService,
    GetSummaryRequest, GetNodesRequest, GetNodeDashboardRequest, GetApplicationsRequest,
    GetActorsRequest, GetWorkflowsRequest,
};
use std::sync::Arc;
use std::path::Path;
use std::fs;
use tonic::Request;

/// Helper to create a test node
async fn create_test_node(node_id: &str) -> Arc<Node> {
    let node = NodeBuilder::new(node_id).build().await;
    Arc::new(node)
}

/// Helper to create dashboard service from a node
async fn create_dashboard_service(node: Arc<Node>) -> DashboardServiceImpl {
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
    
    DashboardServiceImpl::new(service_locator)
}

/// Get calculator WASM file path
fn get_calculator_wasm_path() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/
    path.pop(); // workspace root
    path.push("examples");
    path.push("simple");
    path.push("wasm_calculator");
    path.push("wasm-modules");
    path.push("calculator_actor.wasm");
    path
}

/// Ensure WASM file exists (build if needed)
fn ensure_wasm_file_exists() -> bool {
    let path = get_calculator_wasm_path();
    if path.exists() && path.is_file() {
        return true;
    }
    
    eprintln!("WASM file not found: {}", path.display());
    eprintln!("Attempting to build WASM file...");
    
    let mut example_dir = path.clone();
    example_dir.pop(); // wasm-modules/
    example_dir.pop(); // wasm_calculator/
    
    let build_script = example_dir.join("scripts").join("build_python_actors.sh");
    
    if !build_script.exists() {
        eprintln!("Build script not found: {}", build_script.display());
        return false;
    }
    
    // Run build script
    use std::process::Command;
    let output = Command::new("bash")
        .arg(build_script)
        .current_dir(&example_dir)
        .output();
    
    match output {
        Ok(output) if output.status.success() => {
            eprintln!("WASM file built successfully");
            path.exists() && path.is_file()
        }
        Ok(output) => {
            eprintln!("Build script failed: {}", String::from_utf8_lossy(&output.stderr));
            false
        }
        Err(e) => {
            eprintln!("Failed to run build script: {}", e);
            false
        }
    }
}

#[tokio::test]
#[ignore] // Requires WASM file and full node setup
async fn test_dashboard_home_page_data() {
    let node = create_test_node("test-node").await;
    let dashboard_service = create_dashboard_service(node.clone()).await;
    
    // Start node
    let node_arc = node.clone();
    tokio::spawn(async move {
        if let Err(e) = node_arc.start().await {
            eprintln!("Node start error: {}", e);
        }
    });
    
    // Wait for node to start
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Test summary
    let summary_req = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let summary_resp = DashboardService::get_summary(&dashboard_service, summary_req).await;
    assert!(summary_resp.is_ok(), "Summary should succeed");
    let summary = summary_resp.unwrap().into_inner();
    assert!(summary.total_nodes >= 1, "Should have at least 1 node");
    
    // Test nodes
    let nodes_req = Request::new(GetNodesRequest {
        tenant_id: String::new(),
        cluster_id: String::new(),
        page: None,
    });
    let nodes_resp = DashboardService::get_nodes(&dashboard_service, nodes_req).await;
    assert!(nodes_resp.is_ok(), "Nodes should succeed");
    let nodes = nodes_resp.unwrap().into_inner();
    assert!(!nodes.nodes.is_empty(), "Should have at least 1 node");
    assert_eq!(nodes.nodes[0].id, "test-node", "Node ID should match");
    
    // Test applications (should be empty initially)
    let apps_req = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });
    let apps_resp = dashboard_service.get_applications(apps_req).await;
    assert!(apps_resp.is_ok(), "Applications should succeed");
    let apps = apps_resp.unwrap().into_inner();
    assert_eq!(apps.applications.len(), 0, "Should have no applications initially");
    
    // Test actors (should be empty initially)
    let actors_req = Request::new(GetActorsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        actor_id_pattern: String::new(),
        actor_group: String::new(),
        actor_type: String::new(),
        status: String::new(),
        since: None,
        page: None,
    });
    let actors_resp = dashboard_service.get_actors(actors_req).await;
    assert!(actors_resp.is_ok(), "Actors should succeed");
    let actors = actors_resp.unwrap().into_inner();
    assert_eq!(actors.actors.len(), 0, "Should have no actors initially");
    
    // Test workflows (should be empty initially)
    let workflows_req = Request::new(GetWorkflowsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        definition_id: String::new(),
        status: 0,
        page: None,
    });
    let workflows_resp = dashboard_service.get_workflows(workflows_req).await;
    assert!(workflows_resp.is_ok(), "Workflows should succeed");
}

#[tokio::test]
#[ignore] // Requires WASM file and full node setup
async fn test_dashboard_node_page_data() {
    let node = create_test_node("test-node").await;
    let dashboard_service = create_dashboard_service(node.clone()).await;
    
    // Start node
    let node_arc = node.clone();
    tokio::spawn(async move {
        if let Err(e) = node_arc.start().await {
            eprintln!("Node start error: {}", e);
        }
    });
    
    // Wait for node to start
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Test node dashboard
    let node_dashboard_req = Request::new(GetNodeDashboardRequest {
        node_id: "test-node".to_string(),
        since: None,
    });
    let node_dashboard_resp = DashboardService::get_node_dashboard(&dashboard_service, node_dashboard_req).await;
    assert!(node_dashboard_resp.is_ok(), "Node dashboard should succeed");
    let dashboard = node_dashboard_resp.unwrap().into_inner();
    
    // Verify node data
    assert!(dashboard.node.is_some(), "Should have node data");
    let node_data = dashboard.node.unwrap();
    assert_eq!(node_data.id, "test-node", "Node ID should match");
    
    // Verify metrics (should not be all zeros after update_metrics_with_system_info)
    assert!(dashboard.node_metrics.is_some(), "Should have node metrics");
    let metrics = dashboard.node_metrics.unwrap();
    assert!(metrics.uptime_seconds > 0 || metrics.memory_available_bytes > 0, 
        "Metrics should have non-zero values (uptime: {}, memory: {})", 
        metrics.uptime_seconds, metrics.memory_available_bytes);
    
    // Verify summary
    assert!(dashboard.summary.is_some(), "Should have summary");
    let summary = dashboard.summary.unwrap();
    assert_eq!(summary.total_applications, 0, "Should have no applications initially");
    assert_eq!(summary.total_tenants, 0, "Should have no tenants initially");
}

#[tokio::test]
#[ignore] // Requires WASM file and full node setup
async fn test_dashboard_wasm_deployment_flow() {
    // Ensure WASM file exists
    if !ensure_wasm_file_exists() {
        eprintln!("Skipping test: WASM file not available");
        return;
    }
    
    let node = create_test_node("test-node").await;
    let dashboard_service = create_dashboard_service(node.clone()).await;
    
    // Start node and get HTTP port
    let node_arc = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_arc.start().await {
            eprintln!("Node start error: {}", e);
        }
    });
    
    // Wait for node to start
    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
    
    // Get HTTP port from node config
    let grpc_port = node.config().listen_addr.split(':').last()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(9001);
    let http_port = grpc_port + 1;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    // Get initial state
    let initial_apps_req = Request::new(GetApplicationsRequest {
        node_id: "test-node".to_string(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });
    let initial_apps = dashboard_service.get_applications(initial_apps_req).await
        .unwrap().into_inner();
    let initial_app_count = initial_apps.applications.len();
    
    // Deploy WASM application via HTTP
    let wasm_path = get_calculator_wasm_path();
    let wasm_bytes = fs::read(&wasm_path).expect("Failed to read WASM file");
    eprintln!("📦 Deploying WASM file: {} ({} bytes)", wasm_path.display(), wasm_bytes.len());
    
    use reqwest::multipart;
    let form = multipart::Form::new()
        .text("application_id", "calculator-app")
        .text("name", "calculator")
        .text("version", "1.0.0")
        .part("wasm_file",
            multipart::Part::bytes(wasm_bytes)
                .file_name("calculator_actor.wasm")
                .mime_str("application/wasm")
                .unwrap()
        );
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120)) // 2 minute timeout for large uploads
        .build()
        .expect("Failed to create HTTP client");
    
    eprintln!("📤 Sending deployment request to {}", http_url);
    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await;
    
    if response.is_err() {
        let err = response.err().unwrap();
        eprintln!("❌ HTTP deployment failed: {:?}", err);
        eprintln!("   Node may not be running or HTTP server not started");
        eprintln!("   Check if node started successfully");
        start_handle.abort();
        return; // Skip test if node HTTP server not available
    }
    
    let response = response.unwrap();
    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!("❌ Deployment failed with status {}: {}", status, error_text);
        panic!("Deployment should succeed, got status: {} - {}", status, error_text);
    }
    
    eprintln!("✅ Deployment successful");
    
    // Wait for deployment to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Verify application appears in dashboard
    let apps_req = Request::new(GetApplicationsRequest {
        node_id: "test-node".to_string(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });
    let apps = dashboard_service.get_applications(apps_req).await
        .unwrap().into_inner();
    assert_eq!(apps.applications.len(), initial_app_count + 1, 
        "Should have one more application after deployment");
    
    let deployed_app = apps.applications.iter()
        .find(|app| app.application_id == "calculator-app");
    assert!(deployed_app.is_some(), "Deployed application should be in list");
    let app = deployed_app.unwrap();
    assert_eq!(app.name, "calculator", "Application name should match");
    assert_eq!(app.version, "1.0.0", "Application version should match");
    
    // Verify node dashboard shows the application
    let node_dashboard_req = Request::new(GetNodeDashboardRequest {
        node_id: "test-node".to_string(),
        since: None,
    });
    let node_dashboard = DashboardService::get_node_dashboard(&dashboard_service, node_dashboard_req).await
        .unwrap().into_inner();
    
    if let Some(summary) = node_dashboard.summary {
        assert!(summary.total_applications >= 1, 
            "Node dashboard should show at least 1 application");
    }
    
    // Verify metrics are updated
    if let Some(metrics) = node_dashboard.node_metrics {
        assert!(metrics.uptime_seconds > 0, "Uptime should be > 0");
        assert!(metrics.memory_available_bytes > 0, "Memory should be > 0");
    }
    
    // Undeploy application
    let undeploy_response = client
        .delete(&format!("{}/api/v1/applications/calculator-app", http_url))
        .send()
        .await;
    
    if undeploy_response.is_ok() {
        let undeploy_resp = undeploy_response.unwrap();
        if undeploy_resp.status().is_success() {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            
            // Verify application is removed
            let final_apps = dashboard_service.get_applications(
                Request::new(GetApplicationsRequest {
                    node_id: "test-node".to_string(),
                    tenant_id: String::new(),
                    namespace: String::new(),
                    name_pattern: String::new(),
                    page: None,
                })
            ).await.unwrap().into_inner();
            
            assert_eq!(final_apps.applications.len(), initial_app_count,
                "Application count should return to initial value after undeploy");
        }
    }
    
    // Shutdown node
    let _ = node.shutdown(tokio::time::Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
#[ignore] // Requires full node setup
async fn test_dashboard_metrics_not_zero() {
    let node = create_test_node("test-node").await;
    let dashboard_service = create_dashboard_service(node.clone()).await;
    
    // Start node
    let node_arc = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_arc.start().await {
            eprintln!("Node start error: {}", e);
        }
    });
    
    // Wait for node to start and metrics to update
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    
    // Get node dashboard
    let node_dashboard_req = Request::new(GetNodeDashboardRequest {
        node_id: "test-node".to_string(),
        since: None,
    });
    let node_dashboard = DashboardService::get_node_dashboard(&dashboard_service, node_dashboard_req).await
        .unwrap().into_inner();
    
    // Verify metrics are not all zeros
    if let Some(metrics) = node_dashboard.node_metrics {
        // At least one of these should be non-zero
        assert!(
            metrics.uptime_seconds > 0 || 
            metrics.memory_available_bytes > 0 || 
            metrics.cpu_usage_percent > 0.0,
            "Metrics should not all be zero (uptime: {}, memory: {}, cpu: {})",
            metrics.uptime_seconds, metrics.memory_available_bytes, metrics.cpu_usage_percent
        );
    } else {
        panic!("Node metrics should be present");
    }
}

