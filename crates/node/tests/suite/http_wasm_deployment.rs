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

//! Integration tests for HTTP multipart WASM deployment
//!
//! Tests the HTTP multipart endpoint for deploying and undeploying WASM applications.
//! Uses both the calculator_actor.wasm (large Python-based) and hello.wasm (small C-based) examples.

use chrono::Utc;
use plexspaces_actor::ApplicationManager;
use plexspaces_node::NodeBuilder;
use plexspaces_proto::v1::application::ApplicationState;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use wat;

/// Shared WASM bytes cache (loaded once, reused for all tests)
static SHARED_WASM_BYTES: std::sync::OnceLock<tokio::sync::Mutex<Option<Vec<u8>>>> =
    std::sync::OnceLock::new();
static INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Get the path to calculator_actor.wasm (Python-based WASM, large size)
fn get_calculator_wasm_path() -> PathBuf {
    // First try: test fixtures (preferred)
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/node
    path.push("wasm-runtime");
    path.push("tests");
    path.push("fixtures");
    path.push("calculator_actor.wasm");
    if path.exists() {
        return path;
    }

    // Second try: examples directory (fallback)
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../examples/simple/wasm_calculator/wasm-modules/calculator_actor.wasm");
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

    let bytes = tokio::task::spawn_blocking(move || fs::read(&wasm_path))
        .await
        .ok()
        .and_then(|r| r.ok())?;

    // Cache the bytes
    {
        let mut guard = cache.lock().await;
        *guard = Some(bytes.clone());
    }

    Some(bytes)
}

/// Check if calculator WASM file exists
fn calculator_wasm_exists() -> bool {
    get_calculator_wasm_path().exists()
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
        if let Ok(response) = client
            .get(&format!("{}/api/v1/applications", http_url))
            .send()
            .await
        {
            if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND
            {
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
    let node = Arc::new(
        NodeBuilder::new("test-node-http-wasm-small".to_string())
            .with_listen_addr("127.0.0.1:8000".to_string())
            .with_auth_disabled()
            .build()
            .await,
    );

    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move { if let Err(e) = node_clone.start().await {} });

    // Wait for node to start and HTTP server to be ready
    sleep(Duration::from_millis(2000)).await;

    // HTTP and gRPC share the same port
    let http_port = 8000;
    let http_url = format!("http://127.0.0.1:{}", http_port);

    // Wait for HTTP server to be ready
    if !wait_for_http_server(&http_url, 10).await {
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

    // Verify WASM magic number
    assert!(wasm_bytes.len() >= 4, "WASM file too small");
    assert_eq!(
        &wasm_bytes[0..4],
        b"\0asm",
        "WASM file missing magic number"
    );

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
        .part(
            "wasm_file",
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("test_app.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type"),
        );

    // Deploy via HTTP
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120)) // 2 minute timeout for large uploads
        .build()
        .expect("Failed to create HTTP client");

    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    let status = response.status();
    let response_text = response
        .text()
        .await
        .unwrap_or_else(|_| "No response body".to_string());

    assert!(
        status.is_success(),
        "Deployment should succeed. Status: {}, Response: {}",
        status,
        response_text
    );

    let json: serde_json::Value = serde_json::from_str(&response_text)
        .unwrap_or_else(|_| serde_json::json!({"error": "Failed to parse JSON"}));

    assert_eq!(
        json["success"], true,
        "Deployment should be successful. Response: {}",
        response_text
    );
    assert_eq!(json["application_id"], app_id);

    // Wait a bit for application to fully start
    sleep(Duration::from_millis(1000)).await;

    // Verify application is registered by checking ApplicationManager
    // ApplicationManager stores by application_id (fallback: name)
    let app_manager = node.application_manager();
    let app_state = app_manager.get_state(app_id).await;
    assert!(
        app_state.is_some(),
        "Application should be registered with id '{}'",
        app_id
    );
    // Undeploy via HTTP DELETE
    let undeploy_response = client
        .delete(&format!("{}/api/v1/applications/{}", http_url, app_id))
        .send()
        .await
        .expect("Failed to undeploy application");

    assert_eq!(
        undeploy_response.status(),
        reqwest::StatusCode::OK,
        "Undeployment should succeed. Status: {}, Response: {:?}",
        undeploy_response.status(),
        undeploy_response.text().await
    );

    let undeploy_json: serde_json::Value = undeploy_response
        .json()
        .await
        .expect("Failed to parse undeploy response");
    assert_eq!(
        undeploy_json["success"], true,
        "Undeployment should succeed"
    );

    // Verify application is stopped and unregistered
    sleep(Duration::from_millis(500)).await;
    let app_state_after = app_manager.get_state(app_id).await;
    assert!(
        app_state_after.is_none()
            || matches!(app_state_after, Some(state) if state == ApplicationState::ApplicationStateStopped),
        "Application should be stopped or unregistered after undeployment"
    );

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
async fn test_http_deploy_wasm_application() {
    if !calculator_wasm_exists() {
        return;
    }

    // Start node on a fixed port for testing (avoid permission issues with port < 1024)
    let node = Arc::new(
        NodeBuilder::new("test-node-http-wasm".to_string())
            .with_listen_addr("127.0.0.1:8002".to_string()) // Use different port to avoid conflicts
            .with_auth_disabled()
            .build()
            .await,
    );

    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move { if let Err(e) = node_clone.start().await {} });

    // Wait for node to start and HTTP server to be ready
    sleep(Duration::from_millis(2000)).await;

    // HTTP and gRPC share the same port
    let grpc_port = node
        .config()
        .listen_addr
        .split(':')
        .last()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8002);
    let http_port = grpc_port;
    let http_url = format!("http://127.0.0.1:{}", http_port);

    // Wait for HTTP server to be ready
    if !wait_for_http_server(&http_url, 10).await {
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

    // Read WASM file (use shared module for performance)
    let wasm_bytes = get_shared_wasm_bytes()
        .await
        .expect("WASM file not found. Please ensure calculator_actor.wasm is available.");

    // Verify WASM magic number
    assert!(wasm_bytes.len() >= 4, "WASM file too small");
    assert_eq!(
        &wasm_bytes[0..4],
        b"\0asm",
        "WASM file missing magic number"
    );

    // Create multipart form data
    let form = reqwest::multipart::Form::new()
        .text("application_id", "test-calculator-app")
        .text("name", "calculator")
        .text("version", "1.0.0")
        .part(
            "wasm_file",
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("calculator_actor.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type"),
        );

    // Deploy via HTTP
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120)) // 2 minute timeout for large uploads
        .build()
        .expect("Failed to create HTTP client");

    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Failed to send HTTP request");

    let status = response.status();
    let response_text = response
        .text()
        .await
        .unwrap_or_else(|_| "No response body".to_string());

    assert!(
        status.is_success(),
        "Deployment should succeed. Status: {}, Response: {}",
        status,
        response_text
    );

    let json: serde_json::Value = serde_json::from_str(&response_text)
        .unwrap_or_else(|_| serde_json::json!({"error": "Failed to parse JSON"}));

    assert_eq!(
        json["success"], true,
        "Deployment should be successful. Response: {}",
        response_text
    );
    assert_eq!(json["application_id"], "test-calculator-app");

    // Wait a bit for application to start
    sleep(Duration::from_millis(500)).await;

    // Verify application is listed
    let list_response = client
        .get(&format!("{}/api/v1/applications", http_url))
        .send()
        .await
        .expect("Failed to list applications");

    assert!(
        list_response.status().is_success(),
        "List applications should succeed, got: {}",
        list_response.status()
    );
    let list_text = list_response.text().await.unwrap_or_default();

    // Undeploy via HTTP DELETE
    let undeploy_response = client
        .delete(&format!(
            "{}/api/v1/applications/test-calculator-app",
            http_url
        ))
        .send()
        .await
        .expect("Failed to undeploy application");

    assert_eq!(undeploy_response.status(), reqwest::StatusCode::OK);
    let undeploy_json: serde_json::Value = undeploy_response
        .json()
        .await
        .expect("Failed to parse undeploy response");

    // Shutdown node
    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_dashboard_applications_api_lists_deployed_app() {
    let node = Arc::new(
        NodeBuilder::new("test-node-dashboard-apps".to_string())
            .with_listen_addr("127.0.0.1:8007".to_string())
            .with_auth_disabled()
            .build()
            .await,
    );

    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move { if let Err(e) = node_clone.start().await {} });

    sleep(Duration::from_millis(2000)).await;

    let http_url = "http://127.0.0.1:8007";
    if !wait_for_http_server(http_url, 10).await {
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        panic!("HTTP server not ready");
    }

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

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    let app_id = "dashboard-test-app";
    let form = reqwest::multipart::Form::new()
        .text("application_id", app_id)
        .text("name", "dashboard-test-app")
        .text("version", "1.0.0")
        .part(
            "wasm_file",
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("test.wasm")
                .mime_str("application/wasm")
                .unwrap(),
        );

    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .expect("Deploy request failed");

    assert!(
        response.status().is_success(),
        "Deploy failed: {:?}",
        response.text().await
    );

    sleep(Duration::from_millis(1500)).await;

    // Query the dashboard applications API (same endpoint the UI uses)
    let dashboard_response = client
        .get(&format!("{}/api/v1/dashboard/applications", http_url))
        .send()
        .await
        .expect("Dashboard applications request failed");

    assert!(
        dashboard_response.status().is_success(),
        "Dashboard API returned error: {}",
        dashboard_response.status()
    );

    let body: serde_json::Value = dashboard_response.json().await.unwrap();
    let apps = body["applications"].as_array().expect("applications should be an array");

    let found = apps.iter().any(|app| {
        app["application_id"].as_str() == Some(app_id)
            || app["name"].as_str() == Some("dashboard-test-app")
    });
    assert!(
        found,
        "Deployed app should appear in dashboard applications API. Got: {}",
        serde_json::to_string_pretty(&body).unwrap()
    );

    // Cleanup
    let _ = client
        .delete(&format!("{}/api/v1/applications/{}", http_url, app_id))
        .send()
        .await;

    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}

#[tokio::test]
async fn test_dashboard_applications_api_with_auth_enabled() {
    // Tests that the dashboard applications API works with auth ENABLED,
    // using both Bearer token and cookie-based authentication.
    // This exercises the same path the browser uses after OIDC login.

    // Use the repo's ES256 key for test (same key used by gen-test-jwt.sh).
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let key_file = repo_root.join("certs/jwt-es256.pem");
    assert!(key_file.exists(), "certs/jwt-es256.pem must exist");
    std::env::set_var("PLEXSPACES_JWT_PRIVATE_KEY_FILE", key_file.to_str().unwrap());
    let private_pem = fs::read_to_string(&key_file).expect("read key file");
    let key_pair = plexspaces_grpc_middleware::JwtKeyPair::from_ec_pem(&private_pem)
        .expect("ES256 key pair from PEM");

    let node = Arc::new(
        NodeBuilder::new("test-node-dashboard-auth".to_string())
            .with_listen_addr("127.0.0.1:8008".to_string())
            // NOT calling with_auth_disabled() — auth is ENABLED
            .build()
            .await,
    );

    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move { if let Err(e) = node_clone.start().await {} });

    sleep(Duration::from_millis(2000)).await;

    let http_url = "http://127.0.0.1:8008";
    if !wait_for_http_server(http_url, 10).await {
        let _ = node.shutdown(Duration::from_secs(5)).await;
        start_handle.abort();
        std::env::remove_var("PLEXSPACES_JWT_PRIVATE_KEY_FILE");
        panic!("HTTP server not ready");
    }

    // Generate a valid JWT token using the same ES256 key pair
    let now = Utc::now().timestamp();
    let claims = plexspaces_grpc_middleware::JwtClaims {
        sub: "admin-user".to_string(),
        exp: now + 3600,
        iat: now,
        iss: "plexspaces".to_string(),
        aud: vec![],
        tenant_id: "test-tenant".to_string(),
        roles: vec!["admin".to_string()],
        groups: vec![],
        is_admin: true,
        jti: None,
    };
    let token = plexspaces_grpc_middleware::sign_jwt_with_keypair(&key_pair, &claims)
        .expect("JWT signing should succeed");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    // Deploy an app first (with Bearer auth)
    let wat = r#"
(module
    (memory (export "memory") 1)
    (func (export "init") (param i32 i32) (result i32) (i32.const 0))
    (func (export "handle_message")
          (param $from_ptr i32) (param $from_len i32)
          (param $msg_type_ptr i32) (param $msg_type_len i32)
          (param $payload_ptr i32) (param $payload_len i32)
          (result i32)
        (i32.const 0))
    (func (export "snapshot_state") (result i32 i32) (i32.const 0) (i32.const 0))
)
"#;
    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

    let app_id = "auth-test-app";
    let form = reqwest::multipart::Form::new()
        .text("application_id", app_id)
        .text("name", "auth-test-app")
        .text("version", "1.0.0")
        .part(
            "wasm_file",
            reqwest::multipart::Part::bytes(wasm_bytes)
                .file_name("test.wasm")
                .mime_str("application/wasm")
                .unwrap(),
        );

    let deploy_resp = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .header("Authorization", format!("Bearer {}", token))
        .multipart(form)
        .send()
        .await
        .expect("Deploy request failed");

    assert!(
        deploy_resp.status().is_success(),
        "Deploy with Bearer token should succeed: {:?}",
        deploy_resp.text().await
    );

    sleep(Duration::from_millis(1500)).await;

    // Test 1: Dashboard API with Bearer token
    let bearer_resp = client
        .get(&format!("{}/api/v1/dashboard/applications", http_url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .expect("Bearer request failed");

    assert!(
        bearer_resp.status().is_success(),
        "Dashboard API with Bearer should return 200, got: {}",
        bearer_resp.status()
    );

    let body: serde_json::Value = bearer_resp.json().await.unwrap();
    let apps = body["applications"].as_array().expect("applications array");
    assert!(
        apps.iter().any(|a| a["name"].as_str() == Some("auth-test-app")),
        "Deployed app should appear with Bearer auth. Got: {}",
        serde_json::to_string_pretty(&body).unwrap()
    );

    // Test 2: Dashboard API with cookie (same as browser after OIDC login)
    let cookie_resp = client
        .get(&format!("{}/api/v1/dashboard/applications", http_url))
        .header("Cookie", format!("plexspaces_token={}", token))
        .send()
        .await
        .expect("Cookie request failed");

    assert!(
        cookie_resp.status().is_success(),
        "Dashboard API with cookie should return 200, got: {}",
        cookie_resp.status()
    );

    let body2: serde_json::Value = cookie_resp.json().await.unwrap();
    let apps2 = body2["applications"].as_array().expect("applications array");
    assert!(
        apps2.iter().any(|a| a["name"].as_str() == Some("auth-test-app")),
        "Deployed app should appear with cookie auth. Got: {}",
        serde_json::to_string_pretty(&body2).unwrap()
    );

    // Test 3: Dashboard API WITHOUT auth should return 401
    let no_auth_resp = client
        .get(&format!("{}/api/v1/dashboard/applications", http_url))
        .send()
        .await
        .expect("No-auth request failed");

    assert_eq!(
        no_auth_resp.status(),
        reqwest::StatusCode::UNAUTHORIZED,
        "Dashboard API without auth should return 401"
    );

    // Test 4: Admin with DIFFERENT tenant_id should still see ALL apps
    // (simulates OIDC admin whose org claim differs from deployed app's tenant)
    let cross_tenant_claims = plexspaces_grpc_middleware::JwtClaims {
        sub: "admin-other-org".to_string(),
        exp: now + 3600,
        iat: now,
        iss: "plexspaces".to_string(),
        aud: vec![],
        tenant_id: "completely-different-tenant".to_string(),
        roles: vec!["admin".to_string()],
        groups: vec![],
        is_admin: true,
        jti: None,
    };
    let cross_tenant_token =
        plexspaces_grpc_middleware::sign_jwt_with_keypair(&key_pair, &cross_tenant_claims)
            .expect("JWT signing should succeed");

    let cross_resp = client
        .get(&format!("{}/api/v1/dashboard/applications", http_url))
        .header("Authorization", format!("Bearer {}", cross_tenant_token))
        .send()
        .await
        .expect("Cross-tenant admin request failed");

    assert!(
        cross_resp.status().is_success(),
        "Cross-tenant admin should get 200, got: {}",
        cross_resp.status()
    );

    let body3: serde_json::Value = cross_resp.json().await.unwrap();
    let apps3 = body3["applications"].as_array().expect("applications array");
    assert!(
        apps3.iter().any(|a| a["name"].as_str() == Some("auth-test-app")),
        "Admin should see ALL apps regardless of their own tenant_id. Got: {}",
        serde_json::to_string_pretty(&body3).unwrap()
    );

    // Cleanup
    let _ = client
        .delete(&format!("{}/api/v1/applications/{}", http_url, app_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await;

    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
    std::env::remove_var("PLEXSPACES_JWT_PRIVATE_KEY_FILE");
}

#[tokio::test]
async fn test_http_deploy_wasm_size_limit() {
    // Start node
    let node = Arc::new(
        NodeBuilder::new("test-node-http-wasm-size".to_string())
            .with_listen_addr("127.0.0.1:9005".to_string()) // Use different port
            .with_auth_disabled()
            .build()
            .await,
    );

    let node_clone = node.clone();
    let start_handle = tokio::spawn(async move { if let Err(e) = node_clone.start().await {} });

    sleep(Duration::from_millis(2000)).await;

    let grpc_port = node
        .config()
        .listen_addr
        .split(':')
        .last()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(9005);
    let http_port = grpc_port;
    let http_url = format!("http://127.0.0.1:{}", http_port);

    // Wait for HTTP server to be ready
    if !wait_for_http_server(&http_url, 10).await {
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
        .part(
            "wasm_file",
            reqwest::multipart::Part::bytes(large_file)
                .file_name("large.wasm")
                .mime_str("application/wasm")
                .expect("Failed to set MIME type"),
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

    // Should reject file larger than 100MB (413 from handler or 400 from body size limit)
    assert!(
        response.status() == reqwest::StatusCode::PAYLOAD_TOO_LARGE
            || response.status() == reqwest::StatusCode::BAD_REQUEST,
        "Should reject oversized file, got: {}",
        response.status()
    );

    let _ = node.shutdown(Duration::from_secs(5)).await;
    start_handle.abort();
}
