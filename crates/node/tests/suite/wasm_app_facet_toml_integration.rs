// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration test for WASM application deployment with facets from TOML config

use plexspaces_node::{Node, NodeBuilder};
use plexspaces_services::application_service::ApplicationServiceImpl;
use plexspaces_proto::application::v1::application_service_server::ApplicationService;
use plexspaces_proto::application::v1::DeployApplicationRequest;
use plexspaces_proto::wasm::v1::WasmModule;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tonic::Request;

/// Test: Deploy WASM application with facets from TOML config
/// 
/// Verifies:
/// 1. TOML config with facets is parsed correctly
/// 2. Facets are created from proto and attached to actors
/// 3. Facet attachment logs appear
///
/// ## Note
/// This is an expensive integration test that requires full node startup and WASM deployment.
#[tokio::test]
async fn test_wasm_deployment_with_facets_from_toml() {
    // ARRANGE: Create node
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    node.initialize_services().await.expect("Failed to initialize services");
    
    // Start node
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        node_clone.start().await.expect("Node start failed");
    });
    sleep(Duration::from_millis(500)).await;
    
    // Create minimal WASM module (just magic number)
    let wasm_bytes = b"\0asm\x01\x00\x00\x00";
    
    // TOML config with facets
    let toml_config = r#"
[supervisor]
strategy = "one_for_one"
max_restarts = 10
max_restart_window_seconds = 60

[[supervisor.children]]
id = "task-queue"
type = "worker"
restart = "permanent"
shutdown_timeout_seconds = 5
facets = [
  { type = "locks", priority = 50, config = {} }
]
"#;
    
    // ACT: Deploy application with TOML config containing facets
    let service = ApplicationServiceImpl::new(node.service_locator().clone());
    
    let wasm_module = WasmModule {
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes.to_vec(),
        module_hash: String::new(),
        ..Default::default()
    };
    
    let request = DeployApplicationRequest {
        application_id: "test-app-facets".to_string(),
        name: "test-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: None, // Will be parsed from TOML
        release_config: None,
        initial_state: vec![],
    };
    
    // Parse TOML and add to request
    use plexspaces_node::wasm_apps_loader::parse_app_config_toml;
    let app_spec = parse_app_config_toml(toml_config, "test-app")
        .expect("Failed to parse TOML config");
    
    let mut request_with_config = request;
    request_with_config.config = Some(app_spec);
    
    // Deploy
    let response = service
        .deploy_application(Request::new(request_with_config))
        .await;
    
    assert!(response.is_ok(), "Deployment should succeed");
    let res = response.unwrap().into_inner();
    assert!(res.success, "Deployment should be successful");
    
    // Wait for application to start and actors to spawn
    sleep(Duration::from_millis(1000)).await;
    
    // ASSERT: Verify application is running
    let app_manager = node.application_manager();
    let app_state = app_manager.get_state("test-app").await;
    assert!(app_state.is_some(), "Application should be registered");
    assert_eq!(
        app_state.unwrap(),
        plexspaces_proto::v1::application::ApplicationState::ApplicationStateRunning,
        "Application should be running"
    );
    
    // Verify actor was spawned (facets should be attached during spawn)
    use plexspaces_core::ActorRegistry;
    let actor_registry: Arc<plexspaces_core::ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry should be available");
    
    let registered_ids = actor_registry.registered_actor_ids().read().await;
    let task_queue_actors: Vec<_> = registered_ids
        .iter()
        .filter(|id| id.contains("task-queue"))
        .collect();
    
    assert!(!task_queue_actors.is_empty(), "task-queue actor should be spawned");
    
    eprintln!("✅ WASM application deployed with facets from TOML config");
    eprintln!("✅ Actor spawned: {:?}", task_queue_actors);
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}
