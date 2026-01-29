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

//! WASM Applications Auto-Deploy Loader
//!
//! ## Purpose
//! Provides Tomcat-style auto-deployment for WASM applications. On node startup,
//! scans a configured directory and automatically deploys all valid WASM applications.
//!
//! ## Directory Structure
//! ```text
//! wasm_apps/                    # Configured via wasm_apps_directory
//!   bank_account/               # Application name (directory name)
//!     app.wasm                  # Required: WASM module
//!     app-config.toml           # Optional: ApplicationSpec (supervisor tree, etc.)
//!   feature_flags/
//!     app.wasm
//!     app-config.toml
//! ```
//!
//! ## Architecture Context
//! - Called during `Node::start()` after WASM runtime is initialized
//! - Uses `WasmDeploymentService` to deploy modules
//! - Uses `ApplicationManager` to register and start applications
//! - Errors are logged but don't prevent node startup (best-effort deployment)

use plexspaces_application::ApplicationSpec;
use plexspaces_core::ServiceLocator;
use plexspaces_proto::application::v1::SupervisorSpec;
use std::path::Path;
use std::sync::Arc;

/// Information about a WASM application found in the apps directory
#[derive(Debug)]
pub struct WasmAppInfo {
    /// Application name (derived from directory name)
    pub name: String,
    /// Application version (from config or "1.0.0")
    pub version: String,
    /// WASM module bytes
    pub wasm_bytes: Vec<u8>,
    /// Parsed ApplicationSpec (if app-config.toml exists)
    pub config: Option<ApplicationSpec>,
}

/// Error type for WASM apps loader
#[derive(Debug, thiserror::Error)]
pub enum WasmAppsLoaderError {
    /// IO error reading files
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML parsing error
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
    /// Deployment error
    #[error("Deployment error: {0}")]
    Deployment(String),
    /// Invalid WASM file
    #[error("Invalid WASM file: {0}")]
    InvalidWasm(String),
}

/// Scan the wasm_apps_directory for applications
///
/// Returns a list of valid application directories (those containing app.wasm)
pub fn scan_wasm_apps_directory(base_path: &Path) -> Result<Vec<WasmAppInfo>, WasmAppsLoaderError> {
    let mut apps = Vec::new();

    if !base_path.exists() {
        tracing::warn!(
            path = %base_path.display(),
            "WASM apps directory does not exist, skipping auto-deploy"
        );
        return Ok(apps);
    }

    if !base_path.is_dir() {
        tracing::warn!(
            path = %base_path.display(),
            "WASM apps path is not a directory, skipping auto-deploy"
        );
        return Ok(apps);
    }

    tracing::info!(
        path = %base_path.display(),
        "Scanning WASM apps directory for auto-deploy"
    );

    for entry in std::fs::read_dir(base_path)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let app_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        // Skip hidden directories
        if app_name.starts_with('.') {
            continue;
        }

        match load_wasm_app(&path, &app_name) {
            Ok(app_info) => {
                tracing::info!(
                    app_name = %app_info.name,
                    version = %app_info.version,
                    wasm_size = app_info.wasm_bytes.len(),
                    has_config = app_info.config.is_some(),
                    "Found WASM application"
                );
                apps.push(app_info);
            }
            Err(e) => {
                tracing::warn!(
                    app_name = %app_name,
                    error = %e,
                    "Skipping application directory due to error"
                );
            }
        }
    }

    tracing::info!(
        count = apps.len(),
        "Found {} WASM applications to auto-deploy",
        apps.len()
    );

    Ok(apps)
}

/// Load a WASM application from a directory
///
/// Expects:
/// - app.wasm (required)
/// - app-config.toml (optional)
fn load_wasm_app(app_dir: &Path, app_name: &str) -> Result<WasmAppInfo, WasmAppsLoaderError> {
    let wasm_path = app_dir.join("app.wasm");
    let config_path = app_dir.join("app-config.toml");

    // Check for app.wasm
    if !wasm_path.exists() {
        return Err(WasmAppsLoaderError::InvalidWasm(format!(
            "app.wasm not found in {}",
            app_dir.display()
        )));
    }

    // Read WASM bytes
    let wasm_bytes = std::fs::read(&wasm_path)?;

    // Validate WASM magic number
    if wasm_bytes.len() < 4 || &wasm_bytes[0..4] != b"\0asm" {
        return Err(WasmAppsLoaderError::InvalidWasm(format!(
            "Invalid WASM file (missing magic number): {}",
            wasm_path.display()
        )));
    }

    // Try to read config
    let (config, version) = if config_path.exists() {
        let config_str = std::fs::read_to_string(&config_path)?;
        let parsed = parse_app_config_toml(&config_str, app_name)?;
        let version = parsed.version.clone();
        (Some(parsed), version)
    } else {
        (None, "1.0.0".to_string())
    };

    Ok(WasmAppInfo {
        name: app_name.to_string(),
        version,
        wasm_bytes,
        config,
    })
}

/// Parse app-config.toml into ApplicationSpec
///
/// This is the public entry point for parsing TOML config files into ApplicationSpec.
/// Used by both WASM auto-deploy and HTTP multipart deploy.
pub fn parse_app_config_toml(toml_str: &str, app_name: &str) -> Result<ApplicationSpec, WasmAppsLoaderError> {
    // Parse the TOML file - it should match our ApplicationSpec structure
    let parsed: toml::Value = toml::from_str(toml_str)?;

    // Extract version
    let version = parsed
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("1.0.0")
        .to_string();

    // Extract supervisor configuration
    let supervisor = if let Some(sup_table) = parsed.get("supervisor") {
        Some(parse_supervisor_spec(sup_table)?)
    } else {
        None
    };

    Ok(ApplicationSpec {
        name: app_name.to_string(),
        version,
        supervisor,
        ..Default::default()
    })
}

/// Parse supervisor specification from TOML
///
/// ## Purpose
/// Converts a TOML supervisor table to proto `SupervisorSpec`, including parsing children with facets.
/// Follows proto-first design: TOML → Proto structures directly.
///
/// ## Arguments
/// * `value` - The supervisor table from TOML (`[supervisor]`)
///
/// ## Returns
/// `Ok(SupervisorSpec)` with parsed children and their facets, `Err(WasmAppsLoaderError)` if parsing fails
fn parse_supervisor_spec(value: &toml::Value) -> Result<SupervisorSpec, WasmAppsLoaderError> {
    use plexspaces_proto::application::v1::SupervisionStrategy;

    let strategy_str = value
        .get("strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("one_for_one");

    let strategy = match strategy_str.to_lowercase().as_str() {
        "one_for_one" => SupervisionStrategy::SupervisionStrategyOneForOne,
        "one_for_all" => SupervisionStrategy::SupervisionStrategyOneForAll,
        "rest_for_one" => SupervisionStrategy::SupervisionStrategyRestForOne,
        _ => SupervisionStrategy::SupervisionStrategyOneForOne,
    };

    let max_restarts = value
        .get("max_restarts")
        .and_then(|v| v.as_integer())
        .unwrap_or(10) as u32;

    let max_restart_window_secs = value
        .get("max_restart_window_seconds")
        .and_then(|v| v.as_integer())
        .unwrap_or(60) as i64;

    // Parse children
    // Facets are parsed from inside each child table (see parse_child_spec)
    // The correct TOML structure is:
    //   [[supervisor.children]]
    //   id = "actor-id"
    //   facets = [{ type = "locks", priority = 50, config = {} }]
    let children = if let Some(children_arr) = value.get("children").and_then(|v| v.as_array()) {
        children_arr
            .iter()
            .filter_map(|child| parse_child_spec(child).ok())
            .collect()
    } else {
        vec![]
    };

    Ok(SupervisorSpec {
        strategy: strategy as i32,
        max_restarts,
        max_restart_window: Some(prost_types::Duration {
            seconds: max_restart_window_secs,
            nanos: 0,
        }),
        children,
    })
}

/// Parse a child spec from TOML
///
/// ## Purpose
/// Converts a TOML child table to proto `ChildSpec`, including parsing facets array.
/// Follows proto-first design: TOML → Proto structures directly.
///
/// ## Arguments
/// * `value` - TOML value representing a child (table from `[[supervisor.children]]`)
///
/// ## Returns
/// `Ok(ChildSpec)` with parsed facets, `Err(WasmAppsLoaderError)` if parsing fails
///
/// ## TOML Structure
/// ```toml
/// [[supervisor.children]]
/// id = "task-queue"
/// type = "worker"
/// facets = [{ type = "locks", priority = 50, config = {} }]
/// ```
///
/// ## Error Handling
/// - Invalid facets are logged as warnings but don't fail the entire child parsing
/// - This allows graceful degradation: one bad facet doesn't prevent other facets from being attached
fn parse_child_spec(value: &toml::Value) -> Result<plexspaces_proto::application::v1::ChildSpec, WasmAppsLoaderError> {
    use plexspaces_proto::application::v1::{ChildSpec, ChildType, RestartPolicy};
    use plexspaces_proto::common::v1::Facet;
    use std::collections::HashMap;

    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("worker")
        .to_string();

    let child_type_str = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("worker");

    let child_type = match child_type_str.to_lowercase().as_str() {
        "supervisor" => ChildType::ChildTypeSupervisor,
        _ => ChildType::ChildTypeWorker,
    };

    let restart_str = value
        .get("restart")
        .and_then(|v| v.as_str())
        .unwrap_or("permanent");

    let restart = match restart_str.to_lowercase().as_str() {
        "permanent" => RestartPolicy::RestartPolicyPermanent,
        "transient" => RestartPolicy::RestartPolicyTransient,
        "temporary" => RestartPolicy::RestartPolicyTemporary,
        _ => RestartPolicy::RestartPolicyPermanent,
    };

    let shutdown_timeout_secs = value
        .get("shutdown_timeout_seconds")
        .and_then(|v| v.as_integer())
        .unwrap_or(5) as i64;

    // Parse facets from TOML
    // Facets are defined as an array of inline tables: facets = [{ type = "locks", priority = 50, config = {} }]
    let facets = if let Some(facets_arr) = value.get("facets").and_then(|v| v.as_array()) {
        let mut parsed_facets = Vec::new();
        for (idx, facet_value) in facets_arr.iter().enumerate() {
            match parse_facet_from_toml(facet_value) {
                Ok(facet) => parsed_facets.push(facet),
                Err(e) => {
                    // Log warning but continue parsing other facets (graceful degradation)
                    tracing::warn!(
                        child_id = %id,
                        facet_index = idx,
                        error = %e,
                        "Failed to parse facet from TOML (skipping this facet, continuing with others)"
                    );
                }
            }
        }
        parsed_facets
    } else {
        vec![]
    };

    Ok(ChildSpec {
        id,
        r#type: child_type as i32,
        restart: restart as i32,
        shutdown_timeout: Some(prost_types::Duration {
            seconds: shutdown_timeout_secs,
            nanos: 0,
        }),
        facets,
        ..Default::default()
    })
}

/// Parse a facet from TOML inline table
///
/// ## Purpose
/// Converts a TOML inline table to proto `Facet` structure, matching the proto-first design.
/// This function handles the conversion from TOML's rich value types to proto's `map<string, string>` config.
///
/// ## Arguments
/// * `value` - TOML value representing a facet (inline table)
///
/// ## Returns
/// `Ok(Facet)` if parsing succeeds, `Err(WasmAppsLoaderError)` if required fields are missing
///
/// ## TOML Structure
/// ```toml
/// { type = "locks", priority = 50, config = { key1 = "value1" } }
/// ```
///
/// ## Proto Alignment
/// Directly creates `plexspaces_proto::common::v1::Facet` matching the proto definition:
/// - `type` (string) → `Facet.r#type`
/// - `priority` (int32) → `Facet.priority`
/// - `config` (table) → `Facet.config` (map<string, string>)
fn parse_facet_from_toml(value: &toml::Value) -> Result<plexspaces_proto::common::v1::Facet, WasmAppsLoaderError> {
    use plexspaces_proto::common::v1::Facet;
    use std::collections::HashMap;

    // Facet type is required
    let facet_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| WasmAppsLoaderError::Deployment("Facet 'type' is required".to_string()))?
        .to_string();

    // Priority defaults to 50 if not specified
    let priority = value
        .get("priority")
        .and_then(|v| v.as_integer())
        .unwrap_or(50) as i32;

    // Parse config map: convert TOML table to proto map<string, string>
    // Proto requires all config values to be strings, so we convert TOML values to strings
    let mut config_map = HashMap::new();
    if let Some(config_table) = value.get("config").and_then(|v| v.as_table()) {
        for (key, val) in config_table {
            // Convert TOML value to string (proto config is map<string, string>)
            let val_str = match val {
                toml::Value::String(s) => s.clone(),
                toml::Value::Integer(i) => i.to_string(),
                toml::Value::Float(f) => f.to_string(),
                toml::Value::Boolean(b) => b.to_string(),
                toml::Value::Array(arr) => {
                    // Convert array to JSON string for complex values
                    serde_json::to_string(arr).unwrap_or_else(|_| "[]".to_string())
                }
                toml::Value::Table(t) => {
                    // Convert nested table to JSON string for complex values
                    serde_json::to_string(t).unwrap_or_else(|_| "{}".to_string())
                }
                toml::Value::Datetime(dt) => dt.to_string(),
            };
            config_map.insert(key.clone(), val_str);
        }
    }

    Ok(Facet {
        r#type: facet_type,
        config: config_map,
        priority,
        ..Default::default()
    })
}

/// Deploy all WASM applications from a directory
///
/// This is the main entry point called from Node::start()
pub async fn deploy_all_from_directory(
    base_path: &Path,
    service_locator: Arc<dyn ServiceLocator>,
) -> Result<Vec<String>, WasmAppsLoaderError> {
    let apps = scan_wasm_apps_directory(base_path)?;

    if apps.is_empty() {
        tracing::info!("No WASM applications found for auto-deploy");
        return Ok(vec![]);
    }

    let mut deployed = Vec::new();

    for app in apps {
        match deploy_wasm_app(&app, service_locator.clone()).await {
            Ok(app_id) => {
                tracing::info!(
                    app_name = %app.name,
                    app_id = %app_id,
                    "Successfully auto-deployed WASM application"
                );
                deployed.push(app_id);
            }
            Err(e) => {
                tracing::error!(
                    app_name = %app.name,
                    error = %e,
                    "Failed to auto-deploy WASM application"
                );
                // Continue with other apps - don't fail the whole deployment
            }
        }
    }

    tracing::info!(
        deployed_count = deployed.len(),
        "Auto-deploy complete: {} applications deployed",
        deployed.len()
    );

    Ok(deployed)
}

/// Deploy a single WASM application
async fn deploy_wasm_app(
    app: &WasmAppInfo,
    service_locator: Arc<dyn ServiceLocator>,
) -> Result<String, WasmAppsLoaderError> {
    use plexspaces_proto::application::v1::DeployApplicationRequest;
    use plexspaces_proto::wasm::v1::WasmModule;
    use plexspaces_services::application_service::ApplicationServiceImpl;
    use plexspaces_proto::application::v1::application_service_server::ApplicationService;

    // Create WasmModule
    let wasm_module = WasmModule {
        name: app.name.clone(),
        version: app.version.clone(),
        module_bytes: app.wasm_bytes.clone(),
        module_hash: String::new(), // Will be computed by server
        ..Default::default()
    };

    // Use config from file or create default
    let config = app.config.clone().unwrap_or_else(|| {
        plexspaces_services::create_default_application_spec(&app.name, &app.version)
    });

    // Create deploy request
    let request = DeployApplicationRequest {
        application_id: app.name.clone(),
        name: app.name.clone(),
        version: app.version.clone(),
        wasm_module: Some(wasm_module),
        config: Some(config),
        initial_state: vec![],
    };

    // Deploy using ApplicationService
    let app_service = ApplicationServiceImpl::new(service_locator);
    let grpc_request = tonic::Request::new(request);

    let response = app_service
        .deploy_application(grpc_request)
        .await
        .map_err(|e| WasmAppsLoaderError::Deployment(e.to_string()))?;

    let inner = response.into_inner();

    if inner.success {
        Ok(inner.application_id)
    } else {
        Err(WasmAppsLoaderError::Deployment(
            inner.error.unwrap_or_else(|| "Unknown deployment error".to_string()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_scan_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let apps = scan_wasm_apps_directory(temp_dir.path()).unwrap();
        assert!(apps.is_empty());
    }

    #[test]
    fn test_scan_nonexistent_directory() {
        let apps = scan_wasm_apps_directory(Path::new("/nonexistent/path")).unwrap();
        assert!(apps.is_empty());
    }

    #[test]
    fn test_parse_supervisor_spec() {
        use plexspaces_proto::application::v1::SupervisionStrategy;
        
        let toml_str = r#"
[supervisor]
strategy = "one_for_one"
max_restarts = 10
max_restart_window_seconds = 60

[[supervisor.children]]
id = "worker-1"
type = "worker"
restart = "permanent"
"#;
        let parsed: toml::Value = toml::from_str(toml_str).unwrap();
        let supervisor = parse_supervisor_spec(parsed.get("supervisor").unwrap()).unwrap();
        
        assert_eq!(supervisor.strategy, SupervisionStrategy::SupervisionStrategyOneForOne as i32);
        assert_eq!(supervisor.max_restarts, 10);
        assert_eq!(supervisor.children.len(), 1);
        assert_eq!(supervisor.children[0].id, "worker-1");
    }

    #[test]
    fn test_parse_supervisor_spec_with_facets() {
        use plexspaces_proto::application::v1::SupervisionStrategy;
        
        let toml_str = r#"
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
        let parsed: toml::Value = toml::from_str(toml_str).unwrap();
        let supervisor = parse_supervisor_spec(parsed.get("supervisor").unwrap()).unwrap();
        
        assert_eq!(supervisor.strategy, SupervisionStrategy::SupervisionStrategyOneForOne as i32);
        assert_eq!(supervisor.max_restarts, 10);
        assert_eq!(supervisor.children.len(), 1);
        assert_eq!(supervisor.children[0].id, "task-queue");
        
        // Verify facets were parsed
        assert_eq!(supervisor.children[0].facets.len(), 1, "Should have 1 facet");
        assert_eq!(supervisor.children[0].facets[0].r#type, "locks");
        assert_eq!(supervisor.children[0].facets[0].priority, 50);
        assert!(supervisor.children[0].facets[0].config.is_empty(), "Config should be empty map");
    }

    #[test]
    fn test_parse_facet_from_toml() {
        let toml_str = r#"
type = "locks"
priority = 50
config = { key1 = "value1", key2 = "value2" }
"#;
        let parsed: toml::Value = toml::from_str(toml_str).unwrap();
        let facet = parse_facet_from_toml(&parsed).unwrap();
        
        assert_eq!(facet.r#type, "locks");
        assert_eq!(facet.priority, 50);
        assert_eq!(facet.config.len(), 2);
        assert_eq!(facet.config.get("key1"), Some(&"value1".to_string()));
        assert_eq!(facet.config.get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_load_wasm_app_missing_wasm() {
        let temp_dir = TempDir::new().unwrap();
        let app_dir = temp_dir.path().join("test_app");
        fs::create_dir(&app_dir).unwrap();

        let result = load_wasm_app(&app_dir, "test_app");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_wasm_app_invalid_wasm() {
        let temp_dir = TempDir::new().unwrap();
        let app_dir = temp_dir.path().join("test_app");
        fs::create_dir(&app_dir).unwrap();
        fs::write(app_dir.join("app.wasm"), b"not wasm").unwrap();

        let result = load_wasm_app(&app_dir, "test_app");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("magic number"));
    }

    #[test]
    fn test_load_wasm_app_valid() {
        let temp_dir = TempDir::new().unwrap();
        let app_dir = temp_dir.path().join("test_app");
        fs::create_dir(&app_dir).unwrap();
        
        // Minimal valid WASM (just magic number + version)
        let wasm_bytes = b"\0asm\x01\x00\x00\x00";
        fs::write(app_dir.join("app.wasm"), wasm_bytes).unwrap();

        let result = load_wasm_app(&app_dir, "test_app").unwrap();
        assert_eq!(result.name, "test_app");
        assert_eq!(result.version, "1.0.0");
        assert!(result.config.is_none());
    }

    #[test]
    fn test_load_wasm_app_with_config() {
        let temp_dir = TempDir::new().unwrap();
        let app_dir = temp_dir.path().join("test_app");
        fs::create_dir(&app_dir).unwrap();
        
        // Minimal valid WASM
        let wasm_bytes = b"\0asm\x01\x00\x00\x00";
        fs::write(app_dir.join("app.wasm"), wasm_bytes).unwrap();

        // Config file
        let config = r#"
version = "2.0.0"

[supervisor]
strategy = "one_for_one"
max_restarts = 5

[[supervisor.children]]
id = "account"
type = "worker"
restart = "permanent"
"#;
        fs::write(app_dir.join("app-config.toml"), config).unwrap();

        let result = load_wasm_app(&app_dir, "test_app").unwrap();
        assert_eq!(result.name, "test_app");
        assert_eq!(result.version, "2.0.0");
        assert!(result.config.is_some());
        
        let config = result.config.unwrap();
        assert!(config.supervisor.is_some());
    }
}
