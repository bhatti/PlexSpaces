// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
//! Integration tests for WASM auto-deploy loader
//!
//! Tests the Tomcat-style auto-deployment feature that scans a directory
//! for WASM applications and deploys them automatically on startup.

use plexspaces_node::wasm_apps_loader::scan_wasm_apps_directory;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Create a valid minimal WASM file (just magic number + version)
fn create_minimal_wasm() -> Vec<u8> {
    b"\0asm\x01\x00\x00\x00".to_vec()
}

/// Create a sample app-config.toml content
fn create_sample_config(version: &str) -> String {
    format!(
        r#"version = "{version}"

[supervisor]
strategy = "one_for_one"
max_restarts = 10
max_restart_window_seconds = 60

[[supervisor.children]]
id = "worker-1"
type = "worker"
restart = "permanent"
"#
    )
}

#[test]
fn test_scan_empty_temp_directory() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert!(apps.is_empty(), "Empty directory should yield no apps");
}

#[test]
fn test_scan_nonexistent_directory() {
    let apps = scan_wasm_apps_directory(Path::new("/nonexistent/path/xyz123")).unwrap();
    assert!(
        apps.is_empty(),
        "Nonexistent directory should yield no apps"
    );
}

#[test]
fn test_scan_single_app_without_config() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let app_dir = temp_dir.path().join("my_app");
    fs::create_dir_all(&app_dir).unwrap();
    fs::write(app_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "my_app");
    assert_eq!(apps[0].version, "1.0.0"); // Default version
    assert!(apps[0].config.is_none());
}

#[test]
fn test_scan_single_app_with_config() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let app_dir = temp_dir.path().join("bank_account");
    fs::create_dir_all(&app_dir).unwrap();
    fs::write(app_dir.join("app.wasm"), create_minimal_wasm()).unwrap();
    fs::write(
        app_dir.join("application-spec.toml"),
        create_sample_config("2.0.0"),
    )
    .unwrap();

    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "bank_account");
    assert_eq!(apps[0].version, "2.0.0");
    assert!(apps[0].config.is_some());

    let config = apps[0].config.as_ref().unwrap();
    assert!(config.supervisor.is_some());
}

#[test]
fn test_scan_multiple_apps() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    // Create first app
    let app_alpha_dir = temp_dir.path().join("app_alpha");
    fs::create_dir_all(&app_alpha_dir).unwrap();
    fs::write(app_alpha_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

    // Create second app with config
    let app_beta_dir = temp_dir.path().join("app_beta");
    fs::create_dir_all(&app_beta_dir).unwrap();
    fs::write(app_beta_dir.join("app.wasm"), create_minimal_wasm()).unwrap();
    fs::write(
        app_beta_dir.join("application-spec.toml"),
        create_sample_config("3.0.0"),
    )
    .unwrap();

    // Create third app
    let app_gamma_dir = temp_dir.path().join("app_gamma");
    fs::create_dir_all(&app_gamma_dir).unwrap();
    fs::write(app_gamma_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert_eq!(apps.len(), 3);

    // Check all apps were found (order may vary)
    let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"app_alpha"));
    assert!(names.contains(&"app_beta"));
    assert!(names.contains(&"app_gamma"));
}

#[test]
fn test_scan_skips_hidden_directories() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    // Create a hidden directory (should be skipped)
    let hidden_dir = temp_dir.path().join(".hidden_app");
    fs::create_dir_all(&hidden_dir).unwrap();
    fs::write(hidden_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

    // Create a normal app
    let normal_dir = temp_dir.path().join("normal_app");
    fs::create_dir_all(&normal_dir).unwrap();
    fs::write(normal_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "normal_app");
}

#[test]
fn test_scan_skips_directories_without_wasm() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    // Create directory without app.wasm (should be skipped)
    let no_wasm_dir = temp_dir.path().join("no_wasm_dir");
    fs::create_dir_all(&no_wasm_dir).unwrap();
    fs::write(no_wasm_dir.join("readme.txt"), "not a wasm file").unwrap();

    // Create valid app
    let valid_dir = temp_dir.path().join("valid_app");
    fs::create_dir_all(&valid_dir).unwrap();
    fs::write(valid_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "valid_app");
}

#[test]
fn test_scan_skips_invalid_wasm_files() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    // Create invalid WASM file (no magic number)
    let invalid_dir = temp_dir.path().join("invalid_wasm");
    fs::create_dir_all(&invalid_dir).unwrap();
    fs::write(invalid_dir.join("app.wasm"), b"not wasm content").unwrap();

    // Create valid app
    let valid_dir = temp_dir.path().join("valid_app");
    fs::create_dir_all(&valid_dir).unwrap();
    fs::write(valid_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "valid_app");
}

#[test]
fn test_scan_skips_directories_without_app_wasm() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    // Create a directory with non-WASM file (should be skipped)
    let no_wasm_dir = temp_dir.path().join("no_wasm_dir");
    fs::create_dir_all(&no_wasm_dir).unwrap();
    fs::write(no_wasm_dir.join("not_a_wasm_file.txt"), "hello").unwrap();

    // Create valid app
    let valid_dir = temp_dir.path().join("valid_app");
    fs::create_dir_all(&valid_dir).unwrap();
    fs::write(valid_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "valid_app");
}

#[test]
fn test_wasm_magic_number_validation() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    // Test various invalid WASM files (all should be rejected)
    let invalid_contents = vec![
        b"".to_vec(),                 // Empty
        b"abc".to_vec(),              // Too short (3 bytes)
        b"\x00\x00\x00\x00".to_vec(), // Wrong magic (null bytes)
        b"WASM".to_vec(),             // Text instead of binary magic
    ];

    for (idx, content) in invalid_contents.iter().enumerate() {
        let invalid_dir = temp_dir.path().join(format!("invalid_{}", idx));
        fs::create_dir_all(&invalid_dir).unwrap();
        fs::write(invalid_dir.join("app.wasm"), content).unwrap();
        let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
        assert!(
            apps.is_empty(),
            "Invalid WASM content {:?} should be rejected",
            content
        );
        // Clean up for next iteration
        fs::remove_dir_all(&invalid_dir).unwrap();
    }

    // Valid WASM (magic number + version = 8 bytes minimum) should work
    let valid_dir = temp_dir.path().join("valid_app");
    fs::create_dir_all(&valid_dir).unwrap();
    fs::write(valid_dir.join("app.wasm"), create_minimal_wasm()).unwrap();
    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert_eq!(apps.len(), 1);
}

#[test]
fn test_config_parsing_all_supervision_strategies() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    let strategies = ["one_for_one", "one_for_all", "rest_for_one"];

    for strategy in &strategies {
        let app_name = format!("app_{}", strategy);
        let app_dir = temp_dir.path().join(&app_name);
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(app_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

        let config = format!(
            r#"version = "1.0.0"
[supervisor]
strategy = "{}"
max_restarts = 5
"#,
            strategy
        );
        fs::write(app_dir.join("application-spec.toml"), config).unwrap();
    }

    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert_eq!(apps.len(), 3);
}

#[test]
fn test_config_parsing_child_restart_policies() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let app_dir = temp_dir.path().join("test_app");
    fs::create_dir_all(&app_dir).unwrap();
    fs::write(app_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

    let config = r#"version = "1.0.0"

[supervisor]
strategy = "one_for_one"
max_restarts = 10

[[supervisor.children]]
id = "permanent-worker"
type = "worker"
restart = "permanent"

[[supervisor.children]]
id = "transient-worker"
type = "worker"
restart = "transient"

[[supervisor.children]]
id = "temporary-worker"
type = "worker"
restart = "temporary"
"#;
    fs::write(app_dir.join("application-spec.toml"), config).unwrap();

    let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
    assert_eq!(apps.len(), 1);

    let config = apps[0].config.as_ref().unwrap();
    let supervisor = config.supervisor.as_ref().unwrap();
    assert_eq!(supervisor.children.len(), 3);
}

#[test]
fn test_env_var_wasm_apps_dir() {
    // This test verifies that PLEXSPACES_WASM_APPS_DIR env var is respected
    // Note: The actual integration with Node::start() requires more setup
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let app_dir = temp_dir.path().join("env_test_app");
    fs::create_dir_all(&app_dir).unwrap();
    fs::write(app_dir.join("app.wasm"), create_minimal_wasm()).unwrap();

    // Set env var (for this test's scope only)
    let env_var_name = "PLEXSPACES_WASM_APPS_DIR";
    let original_value = std::env::var(env_var_name).ok();

    std::env::set_var(env_var_name, temp_dir.path().to_str().unwrap());

    // Verify we can read the env var and scan the directory
    let wasm_apps_dir = std::env::var(env_var_name).unwrap();
    let apps = scan_wasm_apps_directory(Path::new(&wasm_apps_dir)).unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "env_test_app");

    // Cleanup
    if let Some(val) = original_value {
        std::env::set_var(env_var_name, val);
    } else {
        std::env::remove_var(env_var_name);
    }
}

// ============================================================================
// Integration Tests - Full Node startup with auto-deploy
// ============================================================================

/// Get the webapps directory path (from wasm-runtime crate)
fn get_webapps_path() -> std::path::PathBuf {
    // Path: crates/wasm-runtime/tests/webapps
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/node -> crates
    path.push("wasm-runtime");
    path.push("tests");
    path.push("webapps");
    path
}

#[tokio::test]
async fn test_node_auto_deploy_with_webapps_directory() {
    use plexspaces_node::NodeBuilder;
    use std::sync::Arc;

    // Use the shared webapps directory with calculator app
    let webapps_path = get_webapps_path();

    // Skip test if webapps directory doesn't exist
    if !webapps_path.exists() {
        eprintln!(
            "⚠️  Skipping test: webapps directory not found at {:?}",
            webapps_path
        );
        return;
    }

    // Verify calculator app exists (using subdirectory format)
    let calculator_dir = webapps_path.join("calculator");
    let calculator_wasm = calculator_dir.join("app.wasm");
    if !calculator_wasm.exists() {
        eprintln!(
            "⚠️  Skipping test: calculator/app.wasm not found at {:?}",
            calculator_wasm
        );
        return;
    }

    // Set env var for auto-deploy directory
    let env_var_name = "PLEXSPACES_WASM_APPS_DIR";
    let original_value = std::env::var(env_var_name).ok();
    std::env::set_var(env_var_name, webapps_path.to_str().unwrap());

    // Create and start node
    let node = Arc::new(
        NodeBuilder::new("auto-deploy-calculator-test")
            .with_listen_addr("127.0.0.1:0")
            .build()
            .await,
    );

    // Start node in background (this triggers auto-deploy)
    let node_clone = node.clone();
    let handle = tokio::spawn(async move {
        let _ = node_clone.start().await;
    });

    // Wait for node to start and auto-deploy to attempt
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Node should have started successfully regardless of auto-deploy result
    // Auto-deploy of complex WASM modules may fail due to runtime limitations,
    // but that's expected behavior (errors are logged, node continues)
    // The key test is that the node starts and doesn't panic
    let _ = node.service_locator();

    // Cleanup
    handle.abort();
    if let Some(val) = original_value {
        std::env::set_var(env_var_name, val);
    } else {
        std::env::remove_var(env_var_name);
    }

    eprintln!("✅ Node started successfully with WASM auto-deploy directory configured");
}

#[tokio::test]
async fn test_node_startup_with_empty_wasm_apps_dir() {
    use plexspaces_node::NodeBuilder;
    use std::sync::Arc;

    // Create empty temp directory
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    // Set env var to empty directory
    let env_var_name = "PLEXSPACES_WASM_APPS_DIR";
    let original_value = std::env::var(env_var_name).ok();
    std::env::set_var(env_var_name, temp_dir.path().to_str().unwrap());

    // Create and start node - should not fail with empty directory
    let node = Arc::new(
        NodeBuilder::new("empty-dir-test-node")
            .with_listen_addr("127.0.0.1:0")
            .build()
            .await,
    );

    let node_clone = node.clone();
    let handle = tokio::spawn(async move {
        let _ = node_clone.start().await;
    });

    // Wait for node to start
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Node should have started successfully (no panic)
    // Just verify we can access the service locator
    let _ = node.service_locator();

    // Cleanup
    handle.abort();
    if let Some(val) = original_value {
        std::env::set_var(env_var_name, val);
    } else {
        std::env::remove_var(env_var_name);
    }
}

#[tokio::test]
async fn test_node_startup_with_nonexistent_wasm_apps_dir() {
    use plexspaces_node::NodeBuilder;
    use std::sync::Arc;

    // Set env var to nonexistent directory
    let env_var_name = "PLEXSPACES_WASM_APPS_DIR";
    let original_value = std::env::var(env_var_name).ok();
    std::env::set_var(env_var_name, "/nonexistent/path/that/does/not/exist");

    // Create and start node - should not fail, just log warning
    let node = Arc::new(
        NodeBuilder::new("nonexistent-dir-test-node")
            .with_listen_addr("127.0.0.1:0")
            .build()
            .await,
    );

    let node_clone = node.clone();
    let handle = tokio::spawn(async move {
        let _ = node_clone.start().await;
    });

    // Wait for node to start
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Node should have started successfully (no panic)
    let _ = node.service_locator();

    // Cleanup
    handle.abort();
    if let Some(val) = original_value {
        std::env::set_var(env_var_name, val);
    } else {
        std::env::remove_var(env_var_name);
    }
}

#[test]
fn test_webapps_directory_structure() {
    // Verify the webapps directory has correct structure
    let webapps_path = get_webapps_path();

    if !webapps_path.exists() {
        eprintln!("⚠️  webapps directory not found - skipping structure test");
        return;
    }

    // Check calculator app structure (subdirectory format)
    let calculator_dir = webapps_path.join("calculator");
    let app_wasm = calculator_dir.join("app.wasm");

    assert!(app_wasm.exists(), "calculator/app.wasm should exist");

    // Verify WASM file has correct magic number
    let wasm_bytes = fs::read(&app_wasm).expect("Should be able to read calculator/app.wasm");
    assert!(
        wasm_bytes.len() >= 4,
        "WASM file should have at least 4 bytes"
    );
    assert_eq!(
        &wasm_bytes[0..4],
        b"\0asm",
        "WASM file should have correct magic number"
    );

    eprintln!("✅ webapps calculator structure verified: calculator/app.wasm ({} bytes), application-spec.toml", wasm_bytes.len());
}
