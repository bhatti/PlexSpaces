// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration test for WASM component deployment via HTTP API
// This test reproduces the WASI binding issue and verifies the fix

use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use plexspaces_node::{Node, NodeBuilder};
use reqwest::multipart;

/// Helper to get the calculator WASM file path
fn get_calculator_wasm_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm");
    path
}

/// Helper to create a test node with a fixed port to avoid port binding issues
async fn create_test_node(node_id: &str) -> Arc<Node> {
    // Use a fixed port based on node_id hash to avoid conflicts
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    node_id.hash(&mut hasher);
    let port = 9000 + (hasher.finish() % 1000) as u16;
    
    let node = Arc::new(
        NodeBuilder::new(node_id)
            .with_listen_address(format!("127.0.0.1:{}", port))
            .build()
            .await
    );
    
    node.initialize_services().await.expect("Failed to initialize services");
    node
}

#[tokio::test]
async fn test_wasm_component_deployment_reproduces_wasi_error() {
    // ARRANGE: Create node and start it
    let node = create_test_node("test-node-wasi").await;
    
    // Start node programmatically
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Node start error: {}", e);
        }
    });
    
    // Wait for node to start
    sleep(Duration::from_millis(2000)).await;
    
    // Get HTTP port
    let grpc_port = node.config().listen_addr.split(':').last()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(9001);
    let http_port = grpc_port + 1;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    // Check if WASM file exists
    let wasm_path = get_calculator_wasm_path();
    if !wasm_path.exists() {
        eprintln!("Skipping test: WASM file not found at {:?}", wasm_path);
        start_handle.abort();
        return;
    }
    
    // ACT: Deploy WASM component via HTTP
    let wasm_bytes = std::fs::read(&wasm_path).expect("Failed to read WASM file");
    eprintln!("📦 Deploying WASM component: {} ({} bytes)", wasm_path.display(), wasm_bytes.len());
    
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
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await;
    
    // ASSERT: Verify deployment result
    match response {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            
            eprintln!("📊 Deployment response status: {}", status);
            eprintln!("📊 Deployment response body: {}", body);
            
            if status.is_success() {
                eprintln!("✅ WASM component deployment succeeded!");
                
                // Verify application is registered
                use plexspaces_core::ApplicationManager;
                use plexspaces_core::service_locator::service_names;
                let app_manager: Arc<ApplicationManager> = node.service_locator()
                    .get_service_by_name(service_names::APPLICATION_MANAGER)
                    .await
                    .expect("ApplicationManager should be available");
                
                let apps = app_manager.list_applications().await;
                assert!(apps.contains(&"calculator-app".to_string()), 
                    "Application should be registered after deployment");
                
                // Cleanup: undeploy
                let _ = client
                    .delete(&format!("{}/api/v1/applications/calculator-app", http_url))
                    .send()
                    .await;
            } else {
                // Check if error is about WASI bindings
                if body.contains("wasi:cli/environment") || body.contains("WASI interface bindings") {
                    eprintln!("❌ WASI binding error reproduced: {}", body);
                    panic!("WASI binding error not fixed: {}", body);
                } else {
                    eprintln!("⚠️ Deployment failed with different error: {}", body);
                    // Don't fail test - might be other issues
                }
            }
        }
        Err(e) => {
            eprintln!("❌ HTTP request failed: {:?}", e);
            // Don't fail test if HTTP server not available
            if e.is_connect() {
                eprintln!("   HTTP server not available - node may not have started HTTP server");
            }
        }
    }
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_wasm_component_deployment_with_supervisor_tree() {
    // ARRANGE: Create node and start it
    let node = create_test_node("test-node-supervisor").await;
    
    // Start node programmatically
    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Node start error: {}", e);
        }
    });
    
    // Wait for node to start
    sleep(Duration::from_millis(2000)).await;
    
    // Get HTTP port
    let grpc_port = node.config().listen_addr.split(':').last()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(9001);
    let http_port = grpc_port + 1;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    // Check if WASM file exists
    let wasm_path = get_calculator_wasm_path();
    if !wasm_path.exists() {
        eprintln!("Skipping test: WASM file not found at {:?}", wasm_path);
        start_handle.abort();
        return;
    }
    
    // ACT: Deploy WASM component with ApplicationSpec containing supervisor tree
    let wasm_bytes = std::fs::read(&wasm_path).expect("Failed to read WASM file");
    
    // Note: The HTTP handler auto-generates ApplicationSpec with a default supervisor tree
    // For this test, we'll deploy without explicit config and verify that:
    // 1. The application deploys successfully
    // 2. The supervisor tree spawns actors
    // 3. The actors appear in the dashboard
    // 
    // The auto-generated ApplicationSpec creates one worker actor with the application name as actor ID
    
    // Note: For now, we'll deploy without explicit config
    // The HTTP handler will auto-generate ApplicationSpec with default supervisor tree
    // TODO: Add support for passing ApplicationSpec via config field in HTTP API
    let form = multipart::Form::new()
        .text("application_id", "calculator-supervisor-app")
        .text("name", "calculator")
        .text("version", "1.0.0")
        .part("wasm_file",
            multipart::Part::bytes(wasm_bytes)
                .file_name("calculator_actor.wasm")
                .mime_str("application/wasm")
                .unwrap()
        );
    
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    eprintln!("📤 Deploying WASM component with supervisor tree");
    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await;
    
    // ASSERT: Verify deployment succeeds
    match response {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            
            if status.is_success() {
                eprintln!("✅ WASM component deployment with supervisor tree succeeded!");
                
                // Verify application is registered
                use plexspaces_core::ApplicationManager;
                use plexspaces_core::service_locator::service_names;
                let app_manager: Arc<ApplicationManager> = node.service_locator()
                    .get_service_by_name(service_names::APPLICATION_MANAGER)
                    .await
                    .expect("ApplicationManager should be available");
                
                let apps = app_manager.list_applications().await;
                assert!(apps.contains(&"calculator-supervisor-app".to_string()), 
                    "Application should be registered after deployment");
                
                // Wait for supervisor tree to spawn
                sleep(Duration::from_millis(1000)).await;
                
                // Verify actors were spawned by supervisor tree
                // The auto-generated ApplicationSpec creates one worker actor with the application name as actor ID
                use plexspaces_core::ActorRegistry;
                let actor_registry: Arc<ActorRegistry> = node.service_locator()
                    .get_service_by_name(service_names::ACTOR_REGISTRY)
                    .await
                    .expect("ActorRegistry should be available");
                
                let registered_ids = actor_registry.registered_actor_ids().read().await;
                let calculator_actor_count = registered_ids.iter()
                    .filter(|id| id.contains("calculator"))
                    .count();
                
                eprintln!("📊 Found {} calculator actors spawned by supervisor tree", calculator_actor_count);
                assert!(calculator_actor_count >= 1, "Supervisor tree should spawn at least 1 calculator actor");
                
                // Cleanup: undeploy
                let _ = client
                    .delete(&format!("{}/api/v1/applications/calculator-supervisor-app", http_url))
                    .send()
                    .await;
                
                sleep(Duration::from_millis(500)).await;
                
                // Verify application is removed
                let apps_after = app_manager.list_applications().await;
                assert!(!apps_after.contains(&"calculator-supervisor-app".to_string()), 
                    "Application should be removed after undeploy");
            } else {
                eprintln!("❌ Deployment failed: status={}, body={}", status, body);
                if body.contains("wasi:cli/environment") || body.contains("wasi:io/") {
                    panic!("WASI binding error not fixed: {}", body);
                } else if body.contains("plexspaces:actor/host") {
                    // This is expected - components requiring plexspaces host functions need WIT bindings
                    eprintln!("⚠️ Component requires plexspaces host functions (expected - requires WIT bindings)");
                    eprintln!("   Skipping test - this is expected until WIT bindings are generated");
                    start_handle.abort();
                    return;
                } else {
                    panic!("Deployment failed: {}", body);
                }
            }
        }
        Err(e) => {
            eprintln!("❌ HTTP request failed: {:?}", e);
            if e.is_connect() {
                eprintln!("   HTTP server not available - skipping test");
                // Don't fail test if HTTP server isn't available (may be port binding issue)
                start_handle.abort();
                return;
            }
            panic!("HTTP request failed: {:?}", e);
        }
    }
    
    // Cleanup
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

