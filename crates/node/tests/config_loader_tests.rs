// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
//! Tests for config loader with environment variable precedence and security validation

use plexspaces_node::config_loader::{ConfigLoader, ConfigLoaderError};
use plexspaces_proto::node::v1::ReleaseSpec;
use std::env;
use tempfile::NamedTempFile;
use std::io::Write;

#[tokio::test]
async fn test_load_release_spec_from_yaml() {
    let yaml_content = r#"
name: "test-release"
version: "1.0.0"
description: "Test release"
node:
  id: "test-node"
  listen_address: "0.0.0.0:8000"
  cluster_seed_nodes: []
runtime:
  grpc:
    enabled: true
    address: "0.0.0.0:8000"
    max_connections: 100
    keepalive_interval_seconds: 30
  health:
    heartbeat_interval_seconds: 10
    heartbeat_timeout_seconds: 5
    registry_url: "http://localhost:8000"
applications: []
env: {}
shutdown:
  global_timeout_seconds: 30
  grace_period_seconds: 5
  grpc_drain_timeout_seconds: 10
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(yaml_content.as_bytes()).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let loader = ConfigLoader::new();
    let spec = loader.load_release_spec(&path).await.unwrap();

    assert_eq!(spec.name, "test-release");
    assert_eq!(spec.version, "1.0.0");
    assert!(spec.node.is_some());
    assert_eq!(spec.node.as_ref().unwrap().id, "test-node");
}

#[tokio::test]
async fn test_env_var_substitution() {
    env::set_var("TEST_NODE_ID", "env-node-123");
    env::set_var("TEST_GRPC_ADDR", "0.0.0.0:9999");

    let yaml_content = r#"
name: "test-release"
version: "1.0.0"
description: "Test release"
node:
  id: "${TEST_NODE_ID}"
  listen_address: "${TEST_GRPC_ADDR}"
  cluster_seed_nodes: []
runtime:
  grpc:
    enabled: true
    address: "${TEST_GRPC_ADDR}"
    max_connections: 100
    keepalive_interval_seconds: 30
  health:
    heartbeat_interval_seconds: 10
    heartbeat_timeout_seconds: 5
    registry_url: "http://localhost:8000"
applications: []
env: {}
shutdown:
  global_timeout_seconds: 30
  grace_period_seconds: 5
  grpc_drain_timeout_seconds: 10
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(yaml_content.as_bytes()).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let loader = ConfigLoader::new();
    let spec = loader.load_release_spec(&path).await.unwrap();

    assert_eq!(spec.node.as_ref().unwrap().id, "env-node-123");
    assert_eq!(spec.node.as_ref().unwrap().listen_address, "0.0.0.0:9999");
    assert_eq!(spec.runtime.as_ref().unwrap().grpc.as_ref().unwrap().address, "0.0.0.0:9999");

    env::remove_var("TEST_NODE_ID");
    env::remove_var("TEST_GRPC_ADDR");
}

#[tokio::test]
async fn test_env_var_with_default() {
    // Ensure env var is not set - should use default
    env::remove_var("TEST_NODE_ID");
    
    let yaml_content = r#"
name: "test-release"
version: "1.0.0"
description: "Test release"
node:
  id: "${TEST_NODE_ID:-default-node}"
  listen_address: "0.0.0.0:8000"
  cluster_seed_nodes: []
runtime:
  grpc:
    enabled: true
    address: "0.0.0.0:8000"
    max_connections: 100
    keepalive_interval_seconds: 30
  health:
    heartbeat_interval_seconds: 10
    heartbeat_timeout_seconds: 5
    registry_url: "http://localhost:8000"
applications: []
env: {}
shutdown:
  global_timeout_seconds: 30
  grace_period_seconds: 5
  grpc_drain_timeout_seconds: 10
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(yaml_content.as_bytes()).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let loader = ConfigLoader::new();
    let spec = loader.load_release_spec(&path).await.unwrap();

    assert_eq!(spec.node.as_ref().unwrap().id, "default-node");
    
    // Clean up
    env::remove_var("TEST_NODE_ID");
}

#[tokio::test]
async fn test_security_validation_rejects_secrets_in_config() {
    let yaml_content = r#"
name: "test-release"
version: "1.0.0"
description: "Test release"
node:
  id: "test-node"
  listen_address: "0.0.0.0:8000"
  cluster_seed_nodes: []
runtime:
  security:
    authn_config:
      jwt_config:
        enable_jwt: true
        secret: "hardcoded-secret-key"  # Should fail validation
        issuer: "https://auth.example.com"
  grpc:
    enabled: true
    address: "0.0.0.0:8000"
    max_connections: 100
    keepalive_interval_seconds: 30
  health:
    heartbeat_interval_seconds: 10
    heartbeat_timeout_seconds: 5
    registry_url: "http://localhost:8000"
applications: []
env: {}
shutdown:
  global_timeout_seconds: 30
  grace_period_seconds: 5
  grpc_drain_timeout_seconds: 10
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(yaml_content.as_bytes()).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let loader = ConfigLoader::new();
    let result = loader.load_release_spec(&path).await;

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("secret") || error_msg.contains("Security"));
}

#[tokio::test]
async fn test_security_validation_accepts_env_var_references() {
    env::set_var("JWT_SECRET", "secret-from-env");

    let yaml_content = r#"
name: "test-release"
version: "1.0.0"
description: "Test release"
node:
  id: "test-node"
  listen_address: "0.0.0.0:8000"
  cluster_seed_nodes: []
runtime:
  security:
    authn_config:
      jwt_config:
        enable_jwt: true
        secret: "${JWT_SECRET}"  # Should pass validation
        issuer: "https://auth.example.com"
  grpc:
    enabled: true
    address: "0.0.0.0:8000"
    max_connections: 100
    keepalive_interval_seconds: 30
  health:
    heartbeat_interval_seconds: 10
    heartbeat_timeout_seconds: 5
    registry_url: "http://localhost:8000"
applications: []
env: {}
shutdown:
  global_timeout_seconds: 30
  grace_period_seconds: 5
  grpc_drain_timeout_seconds: 10
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(yaml_content.as_bytes()).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let loader = ConfigLoader::new();
    let result = loader.load_release_spec(&path).await;

    // Clean up env var
    env::remove_var("JWT_SECRET");
    
    match result {
        Ok(spec) => {
            assert!(spec.runtime.as_ref().unwrap().security.is_some());
        }
        Err(e) => {
            panic!("Expected success but got error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_env_var_precedence_over_file() {
    env::set_var("PLEXSPACES_NODE_ID", "env-override-node");

    let yaml_content = r#"
name: "test-release"
version: "1.0.0"
description: "Test release"
node:
  id: "file-node"
  listen_address: "0.0.0.0:8000"
  cluster_seed_nodes: []
runtime:
  grpc:
    enabled: true
    address: "0.0.0.0:8000"
    max_connections: 100
    keepalive_interval_seconds: 30
  health:
    heartbeat_interval_seconds: 10
    heartbeat_timeout_seconds: 5
    registry_url: "http://localhost:8000"
applications: []
env: {}
shutdown:
  global_timeout_seconds: 30
  grace_period_seconds: 5
  grpc_drain_timeout_seconds: 10
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(yaml_content.as_bytes()).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let loader = ConfigLoader::new();
    let spec = loader.load_release_spec_with_env_precedence(&path).await.unwrap();

    // Env var should override file config
    assert_eq!(spec.node.as_ref().unwrap().id, "env-override-node");

    env::remove_var("PLEXSPACES_NODE_ID");
}

#[tokio::test]
async fn test_missing_file_returns_error() {
    let loader = ConfigLoader::new();
    let result = loader.load_release_spec("/nonexistent/path/release.yaml").await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_invalid_yaml_returns_error() {
    let invalid_yaml = r#"
name: "test-release"
version: "1.0.0"
invalid: [unclosed
"#;

    let mut file = NamedTempFile::new().unwrap();
    file.write_all(invalid_yaml.as_bytes()).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let loader = ConfigLoader::new();
    let result = loader.load_release_spec(&path).await;

    assert!(result.is_err());
}

