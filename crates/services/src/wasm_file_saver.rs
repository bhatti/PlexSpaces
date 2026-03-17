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

//! Atomic WASM File Saver
//!
//! ## Purpose
//! Provides atomic file operations for saving WASM applications to disk.
//! Uses temp file + atomic move pattern to prevent half-completed files.
//!
//! ## Design
//! - Writes to temp file first (e.g., app.wasm.tmp)
//! - Atomically moves temp file to final location (e.g., app.wasm)
//! - Creates directory structure if needed
//! - Saves both WASM file and ApplicationSpec config
//! - Only saves if save_wasm_apps is enabled
//!
//! ## File Structure
//! ```
//! wasm_apps_dir/
//!   app-name/
//!     app.wasm              # WASM module
//!     application-spec.toml  # ApplicationSpec config
//! ```

use plexspaces_proto::application::v1::{
    ApplicationSpec, ChildType, RestartPolicy, SupervisionStrategy,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Save WASM module and ApplicationSpec to apps directory atomically
///
/// ## Arguments
/// * `wasm_apps_dir` - Base directory for WASM apps (e.g., "/app/data/apps")
/// * `app_name` - Application name (used as subdirectory name)
/// * `wasm_bytes` - WASM module bytes
/// * `application_spec` - ApplicationSpec to save as TOML config
/// * `save_wasm_apps` - Whether to save (from RuntimeConfig.save_wasm_apps)
///
/// ## Returns
/// Ok(()) if saved successfully, Err if save failed (non-fatal - deployment continues)
///
/// ## File Structure
/// ```
/// wasm_apps_dir/
///   app-name/
///     app.wasm              # WASM file (atomically moved from app.wasm.tmp)
///     application-spec.toml # ApplicationSpec config (atomically moved from application-spec.toml.tmp)
/// ```
pub fn save_wasm_app_atomically(
    wasm_apps_dir: &str,
    app_name: &str,
    wasm_bytes: &[u8],
    application_spec: &ApplicationSpec,
    save_wasm_apps: bool,
) -> Result<(), String> {
    // Only save if enabled
    if !save_wasm_apps {
        return Ok(());
    }

    if wasm_apps_dir.is_empty() {
        return Err("wasm_apps_directory is empty".to_string());
    }

    let base_path = Path::new(wasm_apps_dir);

    // Ensure base directory exists
    if let Err(e) = fs::create_dir_all(&base_path) {
        return Err(format!(
            "Failed to create apps directory {}: {}",
            base_path.display(),
            e
        ));
    }

    // Create app subdirectory: wasm_apps_dir/app_name/
    let app_dir = base_path.join(app_name);
    if let Err(e) = fs::create_dir_all(&app_dir) {
        return Err(format!(
            "Failed to create app directory {}: {}",
            app_dir.display(),
            e
        ));
    }

    // Save WASM file atomically
    let wasm_temp = app_dir.join("app.wasm.tmp");
    let wasm_final = app_dir.join("app.wasm");

    {
        let mut file = fs::File::create(&wasm_temp).map_err(|e| {
            format!(
                "Failed to create WASM temp file {}: {}",
                wasm_temp.display(),
                e
            )
        })?;

        file.write_all(wasm_bytes)
            .map_err(|e| format!("Failed to write WASM bytes: {}", e))?;

        file.sync_all()
            .map_err(|e| format!("Failed to sync WASM temp file: {}", e))?;
    }

    fs::rename(&wasm_temp, &wasm_final).map_err(|e| {
        format!(
            "Failed to atomically move WASM file {} to {}: {}",
            wasm_temp.display(),
            wasm_final.display(),
            e
        )
    })?;

    // Save ApplicationSpec as TOML atomically
    let spec_toml = serialize_application_spec_to_toml(application_spec)?;
    let spec_temp = app_dir.join("application-spec.toml.tmp");
    let spec_final = app_dir.join("application-spec.toml");

    {
        let mut file = fs::File::create(&spec_temp).map_err(|e| {
            format!(
                "Failed to create config temp file {}: {}",
                spec_temp.display(),
                e
            )
        })?;

        file.write_all(spec_toml.as_bytes())
            .map_err(|e| format!("Failed to write config TOML: {}", e))?;

        file.sync_all()
            .map_err(|e| format!("Failed to sync config temp file: {}", e))?;
    }

    fs::rename(&spec_temp, &spec_final).map_err(|e| {
        format!(
            "Failed to atomically move config file {} to {}: {}",
            spec_temp.display(),
            spec_final.display(),
            e
        )
    })?;

    tracing::info!(
        app_name = %app_name,
        wasm_size = wasm_bytes.len(),
        app_dir = %app_dir.display(),
        "Atomically saved WASM application and config to disk"
    );

    Ok(())
}

/// Serialize ApplicationSpec to TOML format
///
/// This creates a TOML representation that matches what `parse_app_config_toml` expects.
/// Only serializes fields that are relevant for deployment config (version, namespace, seed_nodes, supervisor).
fn serialize_application_spec_to_toml(spec: &ApplicationSpec) -> Result<String, String> {
    let mut toml_lines = Vec::new();

    // Version
    toml_lines.push(format!("version = \"{}\"", spec.version));

    // Namespace (if set)
    if !spec.namespace.is_empty() {
        toml_lines.push(format!("namespace = \"{}\"", spec.namespace));
    }

    if !spec.seed_nodes.is_empty() {
        let seed_nodes = spec
            .seed_nodes
            .iter()
            .map(|seed| format!("\"{}\"", seed.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(", ");
        toml_lines.push(format!("seed_nodes = [{}]", seed_nodes));
    }

    // Supervisor (if present)
    if let Some(supervisor) = &spec.supervisor {
        toml_lines.push(String::from("\n[supervisor]"));

        // Strategy
        let strategy_str = match SupervisionStrategy::try_from(supervisor.strategy) {
            Ok(SupervisionStrategy::SupervisionStrategyOneForAll) => "one_for_all",
            Ok(SupervisionStrategy::SupervisionStrategyRestForOne) => "rest_for_one",
            _ => "one_for_one",
        };
        toml_lines.push(format!("strategy = \"{}\"", strategy_str));

        // Max restarts
        toml_lines.push(format!("max_restarts = {}", supervisor.max_restarts));

        // Max restart window
        if let Some(window) = &supervisor.max_restart_window {
            let seconds = window.seconds;
            toml_lines.push(format!("max_restart_window_seconds = {}", seconds));
        }

        // Children
        if !supervisor.children.is_empty() {
            for child in &supervisor.children {
                toml_lines.push(String::from("\n[[supervisor.children]]"));
                toml_lines.push(format!("id = \"{}\"", child.id));

                let child_type_str = match ChildType::try_from(child.r#type) {
                    Ok(ChildType::ChildTypeSupervisor) => "supervisor",
                    _ => "worker",
                };
                toml_lines.push(format!("type = \"{}\"", child_type_str));

                let restart_str = match RestartPolicy::try_from(child.restart) {
                    Ok(RestartPolicy::RestartPolicyTransient) => "transient",
                    Ok(RestartPolicy::RestartPolicyTemporary) => "temporary",
                    _ => "permanent",
                };
                toml_lines.push(format!("restart = \"{}\"", restart_str));

                if let Some(timeout) = &child.shutdown_timeout {
                    toml_lines.push(format!("shutdown_timeout_seconds = {}", timeout.seconds));
                }

                if !child.args.is_empty() {
                    let args = child
                        .args
                        .iter()
                        .map(|(key, value)| {
                            format!("{} = \"{}\"", key, value.replace('"', "\\\""))
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    toml_lines.push(format!("args = {{ {} }}", args));
                }

                if let Some(behavior_kind) = child
                    .behavior_kind
                    .as_deref()
                    .filter(|behavior_kind| !behavior_kind.is_empty())
                {
                    toml_lines.push(format!(
                        "behavior_kind = \"{}\"",
                        behavior_kind.replace('"', "\\\"")
                    ));
                }

                // Facets
                if !child.facets.is_empty() {
                    toml_lines.push(String::from("facets = ["));
                    for facet in &child.facets {
                        let mut facet_parts = Vec::new();
                        facet_parts.push(format!("type = \"{}\"", facet.r#type));
                        facet_parts.push(format!("priority = {}", facet.priority));

                        if !facet.config.is_empty() {
                            let config_parts: Vec<String> = facet
                                .config
                                .iter()
                                .map(|(k, v)| format!("{} = \"{}\"", k, v.replace('"', "\\\"")))
                                .collect();
                            facet_parts.push(format!("config = {{ {} }}", config_parts.join(", ")));
                        } else {
                            facet_parts.push(String::from("config = {}"));
                        }

                        toml_lines.push(format!("  {{ {} }},", facet_parts.join(", ")));
                    }
                    toml_lines.push(String::from("]"));
                }
            }
        }
    }

    Ok(toml_lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::application::v1::{
        ApplicationSpec, ChildSpec, ChildType, RestartPolicy, SupervisionStrategy, SupervisorSpec,
    };
    use tempfile::TempDir;

    fn create_test_spec() -> ApplicationSpec {
        ApplicationSpec {
            name: "test-app".to_string(),
            version: "1.0.0".to_string(),
            namespace: "test-namespace".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_save_wasm_app_atomically_disabled() {
        let temp_dir = TempDir::new().unwrap();
        let wasm_bytes = b"\0asm\x01\0\0\0"; // Minimal WASM file
        let spec = create_test_spec();

        // Should succeed but not save when disabled
        let result = save_wasm_app_atomically(
            temp_dir.path().to_str().unwrap(),
            "test-app",
            wasm_bytes,
            &spec,
            false, // save_wasm_apps = false
        );
        assert!(result.is_ok());

        // Directory should not exist
        let app_dir = temp_dir.path().join("test-app");
        assert!(!app_dir.exists());
    }

    #[test]
    fn test_save_wasm_app_atomically_enabled() {
        let temp_dir = TempDir::new().unwrap();
        let wasm_bytes = b"\0asm\x01\0\0\0"; // Minimal WASM file
        let spec = create_test_spec();

        // Should save when enabled
        let result = save_wasm_app_atomically(
            temp_dir.path().to_str().unwrap(),
            "test-app",
            wasm_bytes,
            &spec,
            true, // save_wasm_apps = true
        );
        assert!(result.is_ok());

        // App directory should exist
        let app_dir = temp_dir.path().join("test-app");
        assert!(app_dir.exists(), "App directory should exist");

        // WASM file should exist in subdirectory
        let wasm_file = app_dir.join("app.wasm");
        assert!(wasm_file.exists(), "WASM file should exist");

        // Config file should exist
        let config_file = app_dir.join("application-spec.toml");
        assert!(config_file.exists(), "Config file should exist");

        // Temp files should not exist (moved atomically)
        let wasm_temp = app_dir.join("app.wasm.tmp");
        let config_temp = app_dir.join("application-spec.toml.tmp");
        assert!(
            !wasm_temp.exists(),
            "WASM temp file should not exist after atomic move"
        );
        assert!(
            !config_temp.exists(),
            "Config temp file should not exist after atomic move"
        );

        // Verify WASM file contents
        let saved_bytes = fs::read(&wasm_file).unwrap();
        assert_eq!(saved_bytes, wasm_bytes);

        // Verify config file contents
        let config_str = fs::read_to_string(&config_file).unwrap();
        assert!(config_str.contains("version = \"1.0.0\""));
        assert!(config_str.contains("namespace = \"test-namespace\""));
    }

    #[test]
    fn test_save_wasm_app_atomically_empty_dir() {
        let wasm_bytes = b"\0asm\x01\0\0\0";
        let spec = create_test_spec();

        // Should fail gracefully when directory is empty
        let result = save_wasm_app_atomically("", "test-app", wasm_bytes, &spec, true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("wasm_apps_directory is empty"));
    }

    #[test]
    fn test_serialize_application_spec_round_trips_seed_nodes_and_child_args() {
        let spec = ApplicationSpec {
            name: "heat-diffusion-rust".to_string(),
            version: "1.0.0".to_string(),
            namespace: "heat-diffusion-rust".to_string(),
            seed_nodes: vec!["localhost:8091".to_string(), "localhost:8093".to_string()],
            supervisor: Some(SupervisorSpec {
                strategy: SupervisionStrategy::SupervisionStrategyOneForOne as i32,
                max_restarts: 10,
                max_restart_window: Some(prost_types::Duration {
                    seconds: 60,
                    nanos: 0,
                }),
                children: vec![
                    ChildSpec {
                        id: "leader".to_string(),
                        r#type: ChildType::ChildTypeWorker as i32,
                        restart: RestartPolicy::RestartPolicyPermanent as i32,
                        shutdown_timeout: Some(prost_types::Duration {
                            seconds: 10,
                            nanos: 0,
                        }),
                        args: std::collections::HashMap::from([(
                            "role".to_string(),
                            "leader".to_string(),
                        )]),
                        ..Default::default()
                    },
                    ChildSpec {
                        id: "worker".to_string(),
                        r#type: ChildType::ChildTypeWorker as i32,
                        restart: RestartPolicy::RestartPolicyPermanent as i32,
                        shutdown_timeout: Some(prost_types::Duration {
                            seconds: 10,
                            nanos: 0,
                        }),
                        args: std::collections::HashMap::from([(
                            "role".to_string(),
                            "worker".to_string(),
                        )]),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let toml = serialize_application_spec_to_toml(&spec).unwrap();
        assert!(toml.contains("seed_nodes = [\"localhost:8091\", \"localhost:8093\"]"));
        assert!(toml.contains("args = { role = \"leader\" }"));
        assert!(toml.contains("args = { role = \"worker\" }"));

        let reparsed =
            plexspaces_node::wasm_apps_loader::parse_app_config_toml(&toml, "heat-diffusion-rust")
                .unwrap();
        assert_eq!(reparsed.seed_nodes, spec.seed_nodes);
        let supervisor = reparsed.supervisor.expect("supervisor");
        assert_eq!(
            supervisor.children[0].args.get("role"),
            Some(&"leader".to_string())
        );
        assert_eq!(
            supervisor.children[1].args.get("role"),
            Some(&"worker".to_string())
        );
    }
}
