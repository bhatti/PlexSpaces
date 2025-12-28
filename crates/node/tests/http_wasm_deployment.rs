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

//! Integration tests for HTTP multipart WASM deployment
//!
//! Tests the HTTP multipart endpoint for deploying and undeploying WASM applications.
//! Uses both the calculator_actor.wasm (large Python-based) and hello.wasm (small C-based) examples.

use plexspaces_node::NodeBuilder;
use std::path::PathBuf;
use std::fs;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use wat;
use plexspaces_core::application::ApplicationState;

/// Get the path to calculator_actor.wasm (Python-based WASM, large size)
fn get_calculator_wasm_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm");
    path
}

/// Check if calculator WASM file exists
fn calculator_wasm_exists() -> bool {
    let path = get_calculator_wasm_path();
    path.exists() && path.is_file()
}


/// Wait for HTTP server to be ready by checking if port is listening
async fn wait_for_http_server(http_url: &str, max_retries: u32) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    
    for i in 0..max_retries {
        // Try to connect to the HTTP server
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

#[tokio::test]
async fn test_http_deploy_wasm_application_small() {
    // Deploy a PlexSpaces application (not arbitrary WASM module)
    // Using a minimal traditional WASM module that the runtime can handle
    // The HTTP handler creates ApplicationSpec with name and version
    // Following the pattern from examples/simple/wasm_calculator/src/main.rs

    // Start node on a fixed port for testing (avoid permission issues with port < 1024)
    let node = Arc::new(NodeBuilder::new("test-node-http-wasm-small".to_string())
        .with_listen_address("127.0.0.1:8000".to_string())
        .build()
        .await);

    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    // Wait for node to start and HTTP server to be ready
    sleep(Duration::from_millis(2000)).await;
    
    // Use fixed HTTP port (gRPC port + 1)
    let http_port = 8001;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    // Wait for HTTP server to be ready
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready in time");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // Create a minimal traditional WASM module (not a component) for testing
    // Pattern from examples/wasm_showcase/src/main.rs - minimal valid WASM module
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
    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");
    eprintln!("📦 Deploying minimal WASM module ({} bytes) as PlexSpaces application", wasm_bytes.len());
    
    // Verify WASM magic number
    assert!(wasm_bytes.len() >= 4, "WASM file too small");
    assert_eq!(&wasm_bytes[0..4], b"\0asm", "WASM file missing magic number");

    // Create multipart form data - deploying a PlexSpaces application (not arbitrary WASM)
    // The HTTP handler creates ApplicationSpec with name and version
    // Following the pattern from examples/simple/wasm_calculator/src/main.rs
    // Note: ApplicationManager stores by name, not application_id
    let app_name = "calculator";
    let app_id = "test-calculator-app";
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

    // Deploy via HTTP
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120)) // 2 minute timeout for large uploads
        .build()
        .expect("Failed to create HTTP client");
    
    eprintln!("📤 Sending deployment request to {}", http_url);
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

    assert!(status.is_success(), 
        "Deployment should succeed. Status: {}, Response: {}", status, response_text);

    let json: serde_json::Value = serde_json::from_str(&response_text)
        .unwrap_or_else(|_| serde_json::json!({"error": "Failed to parse JSON"}));
    
    assert_eq!(json["success"], true, "Deployment should be successful. Response: {}", response_text);
    assert_eq!(json["application_id"], app_id);

    eprintln!("✅ Deployment successful: {:?}", json);
    
    // Wait a bit for application to fully start
    sleep(Duration::from_millis(1000)).await;
    
    // Verify application is registered by checking ApplicationManager
    // ApplicationManager stores by name, not application_id
    let app_manager = node.application_manager().await.expect("ApplicationManager should be available");
    let app_state = app_manager.get_state(app_name).await;
    assert!(app_state.is_some(), "Application should be registered with name '{}'", app_name);
    eprintln!("✅ Application registered and running - state: {:?}", app_state);
    
    // Undeploy via HTTP DELETE (following Erlang application model)
    // Note: ApplicationManager stores by name, but HTTP handler uses application_id from path
    // The ApplicationService should handle the lookup, but for now we'll use the name
    // since that's what ApplicationManager uses
    eprintln!("🛑 Undeploying application: {} (name: {})", app_id, app_name);
    let undeploy_response = client
        .delete(&format!("{}/api/v1/applications/{}", http_url, app_name))
        .send()
        .await
        .expect("Failed to undeploy application");

    assert_eq!(undeploy_response.status(), reqwest::StatusCode::OK, 
        "Undeployment should succeed. Status: {}, Response: {:?}", 
        undeploy_response.status(), undeploy_response.text().await);
    
    let undeploy_json: serde_json::Value = undeploy_response.json().await.expect("Failed to parse undeploy response");
    eprintln!("🛑 Undeployment successful: {:?}", undeploy_json);
    assert_eq!(undeploy_json["success"], true, "Undeployment should succeed");
    
    // Verify application is stopped and unregistered
    sleep(Duration::from_millis(500)).await;
    let app_state_after = app_manager.get_state(app_name).await;
    assert!(app_state_after.is_none() || 
            matches!(app_state_after, Some(state) if state == ApplicationState::ApplicationStateStopped),
            "Application should be stopped or unregistered after undeployment");
    eprintln!("✅ Application undeployed successfully");
    
    // Verify the application follows Erlang application model:
    // - Application is the unit of deployment (entire application, not individual actors)
    // - Application::start() is called during deployment (spawns actors/supervisors)
    // - Application::stop() is called during undeployment (graceful shutdown)
    // - WASM applications implement Application trait and integrate with ApplicationManager

    // Shutdown node
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
#[ignore] // Requires WASM file to be built first
async fn test_http_deploy_wasm_application() {
    if !calculator_wasm_exists() {
        eprintln!("Skipping test: calculator_actor.wasm not found.");
        eprintln!("To build manually: cd examples/simple/wasm_calculator && ./scripts/build_python_actors.sh");
        return;
    }

    // Start node on a fixed port for testing (avoid permission issues with port < 1024)
    let node = Arc::new(NodeBuilder::new("test-node-http-wasm".to_string())
        .with_listen_address("127.0.0.1:8002".to_string()) // Use different port to avoid conflicts
        .build()
        .await);

    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    // Wait for node to start and HTTP server to be ready
    sleep(Duration::from_millis(2000)).await;

    // Get HTTP port (gRPC port + 1)
    let grpc_port = node.config().listen_addr.split(':').last()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8002);
    let http_port = grpc_port + 1;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    // Wait for HTTP server to be ready
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready in time");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // Read WASM file
    let wasm_path = get_calculator_wasm_path();
    let wasm_bytes = fs::read(&wasm_path).expect("Failed to read WASM file");
    eprintln!("📦 Deploying WASM file: {} ({} bytes)", wasm_path.display(), wasm_bytes.len());
    
    // Verify WASM magic number
    assert!(wasm_bytes.len() >= 4, "WASM file too small");
    assert_eq!(&wasm_bytes[0..4], b"\0asm", "WASM file missing magic number");

    // Create multipart form data
    let form = reqwest::multipart::Form::new()
        .text("application_id", "test-calculator-app")
        .text("name", "calculator")
        .text("version", "1.0.0")
        .part("wasm_file", 
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("calculator_actor.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type")
        );

    // Deploy via HTTP
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120)) // 2 minute timeout for large uploads
        .build()
        .expect("Failed to create HTTP client");
    
    eprintln!("📤 Sending deployment request to {}", http_url);
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

    assert!(status.is_success(), 
        "Deployment should succeed. Status: {}, Response: {}", status, response_text);

    let json: serde_json::Value = serde_json::from_str(&response_text)
        .unwrap_or_else(|_| serde_json::json!({"error": "Failed to parse JSON"}));
    
    assert_eq!(json["success"], true, "Deployment should be successful. Response: {}", response_text);
    assert_eq!(json["application_id"], "test-calculator-app");

    eprintln!("✅ Deployment successful: {:?}", json);

    // Wait a bit for application to start
    sleep(Duration::from_millis(500)).await;

    // Verify application is listed
    let list_response = client
        .get(&format!("{}/api/v1/applications", http_url))
        .send()
        .await
        .expect("Failed to list applications");

    assert_eq!(list_response.status(), reqwest::StatusCode::OK);
    let list_json: serde_json::Value = list_response.json().await.expect("Failed to parse list response");
    eprintln!("📋 Applications: {:?}", list_json);

    // Undeploy via HTTP DELETE
    let undeploy_response = client
        .delete(&format!("{}/api/v1/applications/test-calculator-app", http_url))
        .send()
        .await
        .expect("Failed to undeploy application");

    assert_eq!(undeploy_response.status(), reqwest::StatusCode::OK);
    let undeploy_json: serde_json::Value = undeploy_response.json().await.expect("Failed to parse undeploy response");
    eprintln!("🛑 Undeployment successful: {:?}", undeploy_json);

    // Shutdown node
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
#[ignore] // Requires WASM file to be built first
async fn test_http_deploy_wasm_size_limit() {
    // Start node
    let node = Arc::new(NodeBuilder::new("test-node-http-wasm-size".to_string())
        .with_listen_address("127.0.0.1:9005".to_string()) // Use different port
        .build()
        .await);

    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Failed to start node: {}", e);
        }
    });

    sleep(Duration::from_millis(2000)).await;

    let grpc_port = node.config().listen_addr.split(':').last()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(9005);
    let http_port = grpc_port + 1;
    let http_url = format!("http://127.0.0.1:{}", http_port);
    
    // Wait for HTTP server to be ready
    if !wait_for_http_server(&http_url, 10).await {
        eprintln!("❌ HTTP server did not become ready in time");
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // Create a file larger than 100MB
    let large_file = vec![0u8; 101 * 1024 * 1024]; // 101MB

    let form = reqwest::multipart::Form::new()
        .text("application_id", "test-large-app")
        .text("name", "large")
        .text("version", "1.0.0")
        .part("wasm_file",
            reqwest::multipart::Part::bytes(large_file)
                .file_name("large.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type")
        );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client");
    
    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    // Should reject file larger than 100MB
    assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE,
        "Should reject file larger than 100MB");

    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

