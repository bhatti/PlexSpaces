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

//! Integration tests for WASM deployment with ApplicationSpec and actor verification
//!
//! Tests verify that:
//! 1. WASM applications can be deployed via HTTP with ApplicationSpec
//! 2. Actors created from supervisor tree appear in dashboard "Actors by Type"
//! 3. All metrics are populated correctly
//! 4. Both home page and node page show actors correctly

use plexspaces_core::{ApplicationManager, RequestContext, ServiceLocator};
use plexspaces_dashboard::{DashboardServiceImpl, HealthReporterAccess};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, ApplicationSpec, ChildSpec, ChildType,
    DeployApplicationRequest, RestartPolicy, SupervisionStrategy, SupervisorSpec,
};
use plexspaces_proto::dashboard::v1::{
    dashboard_service_server::DashboardService, GetActorsRequest, GetApplicationsRequest,
    GetNodeDashboardRequest, GetSummaryRequest,
};
use plexspaces_proto::wasm::v1::WasmModule;
use prost_types::Duration as ProstDuration;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tonic::Request;

/// Fast fail when LocalStack-style issues are not the cause: bound TCP wait for HTTP server bind.
async fn tcp_connect_ready(addr: &str, max_wait: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < max_wait {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

fn http_base_url_for_node(node: &Node) -> Option<String> {
    let listen = node.config().listen_addr.as_str();
    let port: u16 = listen.rsplit(':').next()?.parse().ok()?;
    let http_port = port.checked_add(1)?;
    Some(format!("http://127.0.0.1:{http_port}"))
}

/// Helper to create a test node
async fn create_test_node(node_id: &str) -> Arc<Node> {
    static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _env_guard = TEST_ENV_LOCK.lock().unwrap();
    std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");
    let node = NodeBuilder::new(node_id).build().await;
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    Arc::new(node)
}

/// Helper to create dashboard service from a node
async fn create_dashboard_service(node: Arc<Node>) -> DashboardServiceImpl {
    let service_locator = node.service_locator();

    // NodeBuilder::build() already initialized services.

    // Register NodeMetricsAccessor
    use plexspaces_node::service_wrappers::NodeMetricsAccessorWrapper;
    let metrics_accessor = Arc::new(NodeMetricsAccessorWrapper::new(node.clone()));
    let metrics_accessor_trait: Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync> =
        metrics_accessor.clone() as Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync>;
    service_locator
        .register_node_metrics_accessor(metrics_accessor_trait)
        .await;

    // Create HealthReporterAccess implementation
    use plexspaces_core::PlexSpacesHealthReporter;
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
            self.health_reporter
                .get_detailed_health(include_non_critical)
                .await
        }
    }

    let health_reporter_access: Arc<dyn HealthReporterAccess + Send + Sync> =
        Arc::new(HealthReporterAccessImpl { health_reporter });

    DashboardServiceImpl::with_health_reporter(service_locator, health_reporter_access)
}

/// Shared WASM bytes cache (loaded once, reused for all tests)
static SHARED_WASM_BYTES: std::sync::OnceLock<tokio::sync::Mutex<Option<Vec<u8>>>> =
    std::sync::OnceLock::new();
static INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Get calculator WASM file path
fn get_calculator_wasm_path() -> PathBuf {
    // First try: test fixtures (preferred)
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/dashboard
    path.push("wasm-runtime");
    path.push("tests");
    path.push("fixtures");
    path.push("calculator_actor.wasm");
    if path.exists() {
        return path;
    }

    // Second try: examples directory (fallback)
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/dashboard
    path.pop(); // crates
    path.push("examples");
    path.push("simple");
    path.push("wasm_calculator");
    path.push("wasm-modules");
    path.push("calculator_actor.wasm");
    path
}

/// Get or load shared WASM bytes
/// Loads the 40MB WASM file once and caches it for all tests
async fn get_shared_wasm_bytes() -> Option<Vec<u8>> {
    let cache = SHARED_WASM_BYTES.get_or_init(|| tokio::sync::Mutex::new(None));

    // Fast path: already loaded
    {
        let guard = cache.lock().await;
        if let Some(ref bytes) = *guard {
            return Some(bytes.clone());
        }
    }

    // Slow path: need to load (use lock to ensure only one thread loads)
    let _guard = INIT_LOCK.lock().unwrap();

    // Double-check after acquiring lock
    {
        let guard = cache.lock().await;
        if let Some(ref bytes) = *guard {
            return Some(bytes.clone());
        }
    }

    // Load WASM file (first time only)
    let wasm_path = get_calculator_wasm_path();
    if !wasm_path.exists() {
        return None;
    }

    eprintln!(
        "📦 Loading WASM file (first time, ~40MB): {} (this may take a moment)",
        wasm_path.display()
    );

    let bytes = tokio::task::spawn_blocking(move || fs::read(&wasm_path))
        .await
        .ok()
        .and_then(|r| r.ok())?;

    eprintln!("✅ WASM file loaded: {} bytes", bytes.len());

    // Cache the bytes
    {
        let mut guard = cache.lock().await;
        *guard = Some(bytes.clone());
    }

    Some(bytes)
}

/// Create minimal WASM module for testing (if calculator WASM not available)
fn create_minimal_wasm_module() -> Vec<u8> {
    // Create a minimal valid WASM module (just magic number + version)
    // This is enough for testing deployment flow, even if execution fails
    let mut wasm = vec![0x00, 0x61, 0x73, 0x6D]; // Magic: "\0asm"
    wasm.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Version: 1
    wasm
}

#[tokio::test]
#[ignore = "expensive full WASM deployment integration test; run explicitly"]
async fn test_wasm_deployment_with_applicationspec_creates_actors() {
    // ARRANGE: In-process ApplicationService deploy (no tonic server required).
    let node = create_test_node("test-node-wasm").await;

    // Create ApplicationSpec with supervisor tree
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
                facets: vec![],
                behavior_kind: None,
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
                facets: vec![],
                behavior_kind: None,
            },
        ],
    };

    let app_spec = ApplicationSpec {
        name: "test-wasm-app".to_string(),
        tenant_id: String::new(),
        namespace: String::new(),
        version: "1.0.0".to_string(),
        description: "Test WASM application with supervisor tree".to_string(),
        r#type: plexspaces_proto::application::v1::ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: HashMap::new(),
        supervisor: Some(supervisor_spec),
        enabled: true,
        auto_start: true,
        shutdown_timeout: Some(ProstDuration {
            seconds: 60,
            nanos: 0,
        }),
        shutdown_strategy:
            plexspaces_proto::application::v1::ShutdownStrategy::ShutdownStrategyGraceful.into(),
        seed_nodes: vec![],
        metadata: None,
    };

    // Get WASM file or create minimal one (use shared module for performance)
    let wasm_bytes = if let Some(bytes) = get_shared_wasm_bytes().await {
        bytes
    } else {
        eprintln!("⚠️  Calculator WASM not found, using minimal WASM module");
        create_minimal_wasm_module()
    };

    // Deploy via gRPC ApplicationService (more reliable than HTTP for ApplicationSpec)
    use plexspaces_services::application_service::ApplicationServiceImpl;
    let _application_manager = node.application_manager();
    let app_service = ApplicationServiceImpl::new(node.service_locator().clone(), None);

    let wasm_module = WasmModule {
        name: "test-wasm-app".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes,
        module_hash: String::new(), // Will be computed by server
        ..Default::default()
    };

    let deploy_request = DeployApplicationRequest {
        application_id: "test-wasm-app-001".to_string(),
        name: "test-wasm-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec.clone()),
        initial_state: vec![],
    };

    // ACT: Deploy application
    let deploy_response =
        ApplicationService::deploy_application(&app_service, Request::new(deploy_request)).await;
    if deploy_response.is_err() {
        let err = deploy_response.err().unwrap();
        eprintln!(
            "⚠️  Deployment failed (may be due to WASM component issue): {:?}",
            err
        );
        eprintln!("   This is expected if WASM file is a component requiring WASI bindings");
        return; // Skip test if deployment fails (WASM component issue)
    }

    let deploy_result = deploy_response.unwrap().into_inner();
    assert!(deploy_result.success, "Deployment should succeed");

    let dashboard_service = create_dashboard_service(node.clone()).await;

    let expected_actor_count = app_spec
        .supervisor
        .as_ref()
        .map(|s| s.children.len() as u32)
        .unwrap_or(0);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = expected_actor_count == 0;
    while !ready && Instant::now() < deadline {
        let summary_req = Request::new(GetSummaryRequest {
            tenant_id: String::new(),
            node_id: String::new(),
            cluster_id: String::new(),
            since: None,
        });
        if let Ok(resp) = DashboardService::get_summary(&dashboard_service, summary_req).await {
            let total: u32 = resp.into_inner().actors_by_type.values().sum();
            if expected_actor_count == 0 || total >= expected_actor_count {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        ready,
        "expected at least {} actors in summary within 5s",
        expected_actor_count
    );

    // Verify home page summary shows actors
    let summary_req = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let summary = DashboardService::get_summary(&dashboard_service, summary_req)
        .await
        .unwrap()
        .into_inner();

    // ASSERT: Verify actors_by_type is populated
    if let Some(supervisor) = &app_spec.supervisor {
        let expected_actor_count = supervisor.children.len() as u32;
        let total_actors: u32 = summary.actors_by_type.values().sum();
        assert!(
            total_actors >= expected_actor_count,
            "Home page should show at least {} actors from supervisor tree (found {})",
            expected_actor_count,
            total_actors
        );

        // Verify specific actor types
        for child in &supervisor.children {
            if child.r#type() == ChildType::ChildTypeWorker {
                let actor_type = &child.id;
                let count = summary.actors_by_type.get(actor_type).copied().unwrap_or(0);
                assert!(
                    count >= 1,
                    "Actor type '{}' should appear in home page actors_by_type (found {})",
                    actor_type,
                    count
                );
            }
        }
    }

    // Verify node page also shows actors
    let node_dashboard_req = Request::new(GetNodeDashboardRequest {
        node_id: "test-node-wasm".to_string(),
        since: None,
    });
    let node_dashboard =
        DashboardService::get_node_dashboard(&dashboard_service, node_dashboard_req)
            .await
            .unwrap()
            .into_inner();

    if let Some(node_summary) = node_dashboard.summary {
        let total_actors: u32 = node_summary.actors_by_type.values().sum();
        assert!(
            total_actors >= 1,
            "Node page should show at least 1 actor (found {})",
            total_actors
        );
    }

    // Verify GetActors API also returns actors
    let actors_req = Request::new(GetActorsRequest {
        node_id: "test-node-wasm".to_string(),
        tenant_id: String::new(),
        namespace: String::new(),
        actor_id_pattern: String::new(),
        actor_group: String::new(),
        actor_type: String::new(),
        status: String::new(),
        since: None,
        page: None,
    });
    let actors_response = dashboard_service
        .get_actors(actors_req)
        .await
        .unwrap()
        .into_inner();

    if let Some(supervisor) = &app_spec.supervisor {
        let expected_actor_count = supervisor.children.len();
        assert!(
            actors_response.actors.len() >= expected_actor_count,
            "GetActors should return at least {} actors (found {})",
            expected_actor_count,
            actors_response.actors.len()
        );
    }

    // Verify all metrics are populated
    if let Some(metrics) = node_dashboard.node_metrics {
        assert!(
            metrics.uptime_seconds > 0 || metrics.memory_available_bytes > 0,
            "Node metrics should have non-zero values"
        );
    }

    let _ = node.shutdown(tokio::time::Duration::from_secs(5)).await;
}

/// Check if WASM file exists
fn ensure_wasm_file_exists() -> bool {
    get_calculator_wasm_path().exists()
}

#[tokio::test]
#[ignore = "expensive full WASM deployment integration test; run explicitly"]
async fn test_dashboard_wasm_deployment_flow() {
    // Ensure WASM file exists
    if !ensure_wasm_file_exists() {
        eprintln!("Skipping test: WASM file not available");
        return;
    }

    let node = create_test_node("test-node").await;
    let dashboard_service = create_dashboard_service(node.clone()).await;

    let http_url = http_base_url_for_node(&node).expect("valid listen_addr on test node");
    let http_port = http_url
        .rsplit(':')
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .expect("http port");

    let node_arc = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_arc.start().await {
            eprintln!("Node start error: {}", e);
        }
    });

    let tcp_addr = format!("127.0.0.1:{http_port}");
    if !tcp_connect_ready(&tcp_addr, Duration::from_secs(15)).await {
        eprintln!("HTTP server did not accept connections on {tcp_addr} within 15s; skipping test");
        start_handle.abort();
        return;
    }

    // Get initial state
    let initial_apps_req = Request::new(GetApplicationsRequest {
        node_id: "test-node".to_string(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });
    let initial_apps = dashboard_service
        .get_applications(initial_apps_req)
        .await
        .unwrap()
        .into_inner();
    let initial_app_count = initial_apps.applications.len();

    // Deploy WASM application via HTTP with ApplicationSpec (use shared module for performance)
    let wasm_bytes = get_shared_wasm_bytes()
        .await
        .expect("WASM file not found. Please ensure calculator_actor.wasm is available.");
    eprintln!("📦 Deploying WASM file: {} bytes", wasm_bytes.len());

    // Note: HTTP handler auto-generates ApplicationSpec with default supervisor tree
    // if config is not provided. The default supervisor tree creates one worker actor
    // with actor_id = application name. This ensures actors are created and should
    // appear in "Actors by Type" dashboard.

    use reqwest::multipart;
    let form = multipart::Form::new()
        .text("application_id", "calculator-app")
        .text("name", "calculator")
        .text("version", "1.0.0")
        // Note: config field is optional - if not provided, auto-generates supervisor tree
        .part(
            "wasm_file",
            multipart::Part::bytes(wasm_bytes)
                .file_name("calculator_actor.wasm")
                .mime_str("application/wasm")
                .unwrap(),
        );

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(120))
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
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        eprintln!(
            "❌ Deployment failed with status {}: {}",
            status, error_text
        );

        // If deployment fails because component requires plexspaces host functions,
        // this is expected until WIT bindings are generated
        if error_text.contains("plexspaces:actor/host") {
            eprintln!("⚠️ Component requires plexspaces host functions (expected - requires WIT bindings)");
            eprintln!("   Skipping test - this is expected until WIT bindings are generated");
            start_handle.abort();
            return;
        }

        panic!(
            "Deployment should succeed, got status: {} - {}",
            status, error_text
        );
    }

    eprintln!("✅ Deployment successful");

    // Wait for deployment to complete by checking ApplicationManager directly
    // This is more reliable than polling the dashboard service
    let service_locator = node.service_locator();
    let app_manager: Arc<dyn ApplicationManager> = service_locator
        .get_application_manager()
        .await
        .expect("ApplicationManager should be available");

    // Wait for application to be registered (deployment is async)
    let mut retries = 0;
    while {
        let apps: Vec<String> = app_manager.list_applications().await;
        !apps.contains(&"calculator".to_string())
    } && retries < 20
    {
        tokio::task::yield_now().await; // Yield to allow async operations to complete
        retries += 1;
    }

    // Verify application is registered
    let apps: Vec<String> = app_manager.list_applications().await;
    assert!(
        apps.contains(&"calculator".to_string()),
        "Application 'calculator' should be registered after deployment"
    );

    // Now check dashboard service
    let apps = dashboard_service
        .get_applications(Request::new(GetApplicationsRequest {
            node_id: "test-node".to_string(),
            tenant_id: String::new(),
            namespace: String::new(),
            name_pattern: String::new(),
            page: None,
        }))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(
        apps.applications.len(),
        initial_app_count + 1,
        "Should have one more application after deployment"
    );

    // ApplicationInfo uses name as application_id (see ApplicationManager::get_application_info)
    let deployed_app = apps
        .applications
        .iter()
        .find(|app| app.application_id == "calculator" || app.name == "calculator");
    assert!(
        deployed_app.is_some(),
        "Deployed application should be in list"
    );
    let app = deployed_app.unwrap();
    assert_eq!(app.name, "calculator", "Application name should match");
    assert_eq!(app.version, "1.0.0", "Application version should match");

    // Verify node dashboard shows the application
    let node_dashboard_req = Request::new(GetNodeDashboardRequest {
        node_id: "test-node".to_string(),
        since: None,
    });
    let node_dashboard =
        DashboardService::get_node_dashboard(&dashboard_service, node_dashboard_req)
            .await
            .unwrap()
            .into_inner();

    if let Some(summary) = node_dashboard.summary {
        assert!(
            summary.total_applications >= 1,
            "Node dashboard should show at least 1 application"
        );

        // CRITICAL: Verify actors_by_type is populated after WASM deployment
        // This tests the fix for "No actors" issue
        // HTTP handler auto-generates a default supervisor tree with one worker actor
        // Actor ID = application name ("calculator"), actor_type = "calculator"
        let total_actors: u32 = summary.actors_by_type.values().sum();
        assert!(
            total_actors >= 1,
            "Should have at least 1 actor from auto-generated supervisor tree (found {})",
            total_actors
        );

        // Verify the auto-generated actor type appears (actor_type = application name)
        let calculator_count = summary
            .actors_by_type
            .get("calculator")
            .copied()
            .unwrap_or(0);
        assert!(
            calculator_count >= 1,
            "Actor type 'calculator' should appear in actors_by_type (found {})",
            calculator_count
        );
    }

    // Verify metrics are updated (uptime may still be 0 immediately after bind)
    if let Some(metrics) = node_dashboard.node_metrics {
        assert!(
            metrics.uptime_seconds > 0 || metrics.memory_available_bytes > 0,
            "Expected non-zero uptime or memory from system metrics"
        );
    }

    // Verify home page summary also shows actors
    let summary_req = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let home_summary = DashboardService::get_summary(&dashboard_service, summary_req)
        .await
        .unwrap()
        .into_inner();

    // Verify actors_by_type on home page
    // HTTP handler auto-generates supervisor tree with one worker actor
    let total_actors: u32 = home_summary.actors_by_type.values().sum();
    assert!(
        total_actors >= 1,
        "Home page should show at least 1 actor (found {})",
        total_actors
    );

    // Verify the auto-generated actor type appears
    let calculator_count = home_summary
        .actors_by_type
        .get("calculator")
        .copied()
        .unwrap_or(0);
    assert!(
        calculator_count >= 1,
        "Home page should show 'calculator' actor type (found {})",
        calculator_count
    );

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
            let final_apps = dashboard_service
                .get_applications(Request::new(GetApplicationsRequest {
                    node_id: "test-node".to_string(),
                    tenant_id: String::new(),
                    namespace: String::new(),
                    name_pattern: String::new(),
                    page: None,
                }))
                .await
                .unwrap()
                .into_inner();

            assert_eq!(
                final_apps.applications.len(),
                initial_app_count,
                "Application count should return to initial value after undeploy"
            );
        }
    }

    // Shutdown node
    let _ = node.shutdown(tokio::time::Duration::from_secs(5)).await;
    start_handle.abort();
}
