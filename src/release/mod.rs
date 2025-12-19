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

//! # PlexSpaces Release Module
//!
//! ## Purpose
//! Manages PlexSpaces releases (Erlang/OTP-inspired). A release is a complete
//! packaged system containing multiple applications, runtime configuration,
//! and deployment settings.
//!
//! ## Architecture Context
//! Implements the release design from PLEXSPACES_RELEASE_DESIGN.md:
//! - One Node = One Release (Erlang/OTP model)
//! - Release = Multiple Applications (system + user apps)
//! - TOML configuration parsing → Proto types
//! - Dependency resolution (topological sort)
//! - Application startup/shutdown ordering
//!
//! ## Proto-First Design
//! This module uses proto-generated types from `plexspaces_proto::node::v1`:
//! - `ReleaseSpec` - Main release configuration
//! - `NodeConfig` - Node identity and clustering
//! - `RuntimeConfig` - gRPC and health monitoring
//! - `ApplicationConfig` - Application metadata
//!
//! TOML is used for human-friendly configuration, but internally everything
//! is proto-typed for type safety and cross-language compatibility.
//!
//! ## Examples
//!
//! ### Loading a Release
//! ```rust,no_run
//! use plexspaces::release::Release;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let release = Release::from_toml_file("release.toml").await?;
//! println!("Release: {} v{}", release.name(), release.version());
//! # Ok(())
//! # }
//! ```

mod security_config_toml;

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

// Import proto-generated types (Proto-First Design)
pub use plexspaces_proto::application::v1::{
    ChildSpec, ChildType, RestartPolicy, SupervisionStrategy, SupervisorSpec,
};
pub use plexspaces_proto::node::v1::{
    ApplicationConfig,
    GrpcConfig, HealthConfig, MiddlewareConfig, NodeConfig, ReleaseSpec,
    RuntimeConfig, SecurityConfig, ShutdownConfig, ShutdownStrategy,
};
pub use plexspaces_proto::security::v1::{ApiKey, JwtConfig, MtlsConfig, ServiceIdentity};
pub use plexspaces_proto::prost_types;

use security_config_toml::{
    SecurityConfigToml,
};

/// Release errors
#[derive(Debug, Error)]
pub enum ReleaseError {
    /// IO error reading release file
    #[error("Failed to read release file: {0}")]
    IoError(#[from] std::io::Error),

    /// TOML parsing error
    #[error("Failed to parse TOML: {0}")]
    TomlError(#[from] toml::de::Error),

    /// Circular dependency detected
    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    /// Missing dependency
    #[error("Application '{application}' depends on '{dependency}' which is not in the release")]
    MissingDependency {
        /// Application with missing dependency
        application: String,
        /// Missing dependency name
        dependency: String,
    },

    /// Missing application
    #[error("Application not found: {0}")]
    ApplicationNotFound(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Intermediate TOML representation (for parsing)
///
/// This is only used during TOML parsing, then converted to proto ReleaseSpec.
/// Keeps TOML parsing separate from proto types.
#[derive(Debug, Deserialize)]
struct ReleaseToml {
    release: ReleaseMetadata,
    node: NodeConfigToml,
    runtime: RuntimeConfigToml,
    #[serde(default)]
    system_applications: SystemApplicationsToml,
    #[serde(default)]
    applications: Vec<ApplicationConfigToml>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    shutdown: ShutdownConfigToml,
}

#[derive(Debug, Deserialize)]
struct ReleaseMetadata {
    name: String,
    version: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct NodeConfigToml {
    id: String,
    listen_address: String,
    #[serde(default)]
    cluster_seed_nodes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RuntimeConfigToml {
    grpc: GrpcConfigToml,
    health: HealthConfigToml,
    #[serde(default)]
    security: SecurityConfigToml,
}

#[derive(Debug, Deserialize)]
struct GrpcConfigToml {
    enabled: bool,
    address: String,
    max_connections: u32,
    keepalive_interval_seconds: u64,
    #[serde(default)]
    middleware: Vec<MiddlewareConfigToml>,
}

#[derive(Debug, Deserialize)]
struct MiddlewareConfigToml {
    #[serde(rename = "type")]
    type_: String,
    enabled: bool,
    #[serde(flatten)]
    config: HashMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
struct HealthConfigToml {
    heartbeat_interval_seconds: u64,
    heartbeat_timeout_seconds: u64,
    registry_url: String,
}

#[derive(Debug, Deserialize)]
struct SystemApplicationsToml {
    #[serde(default = "default_system_apps")]
    included: Vec<String>,
}

fn default_system_apps() -> Vec<String> {
    vec![
        "plexspaces-core".to_string(),
        "plexspaces-tuplespace".to_string(),
        "plexspaces-actor".to_string(),
        "plexspaces-supervisor".to_string(),
    ]
}

impl Default for SystemApplicationsToml {
    fn default() -> Self {
        Self {
            included: default_system_apps(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApplicationConfigToml {
    name: String,
    version: String,
    config_path: String,
    enabled: bool,
    auto_start: bool,
    shutdown_timeout_seconds: u64,
    #[serde(default)]
    shutdown_strategy: String,
    #[serde(default)]
    dependencies: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ShutdownConfigToml {
    #[serde(default = "default_global_timeout")]
    global_timeout_seconds: u64,
    #[serde(default = "default_grace_period")]
    grace_period_seconds: u64,
    #[serde(default = "default_grpc_drain_timeout")]
    grpc_drain_timeout_seconds: u64,
}

fn default_global_timeout() -> u64 {
    300
}
fn default_grace_period() -> u64 {
    10
}
fn default_grpc_drain_timeout() -> u64 {
    30
}

impl Default for ShutdownConfigToml {
    fn default() -> Self {
        Self {
            global_timeout_seconds: default_global_timeout(),
            grace_period_seconds: default_grace_period(),
            grpc_drain_timeout_seconds: default_grpc_drain_timeout(),
        }
    }
}

/// Release convenience wrapper around proto ReleaseSpec
///
/// Provides helper methods for working with ReleaseSpec from proto.
pub struct Release {
    spec: ReleaseSpec,
}

impl Release {
    /// Create release from proto ReleaseSpec directly
    ///
    /// ## Arguments
    /// * `spec` - Proto ReleaseSpec
    ///
    /// ## Returns
    /// Release wrapping the spec
    ///
    /// ## Design Notes
    /// - Useful for testing
    /// - Bypasses TOML parsing
    pub fn from_spec(spec: ReleaseSpec) -> Self {
        Self { spec }
    }

    /// Load release from TOML file
    ///
    /// ## Arguments
    /// * `path` - Path to release.toml file
    ///
    /// ## Returns
    /// Parsed release (proto ReleaseSpec internally)
    ///
    /// ## Errors
    /// - `ReleaseError::IoError` if file cannot be read
    /// - `ReleaseError::TomlError` if TOML parsing fails
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces::release::Release;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let release = Release::from_toml_file("release.toml").await?;
    /// assert_eq!(release.name(), "plexspaces-cluster");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_toml_file(path: impl AsRef<Path>) -> Result<Self, ReleaseError> {
        let contents = tokio::fs::read_to_string(path).await?;
        Self::from_toml_str(&contents)
    }

    /// Parse release from TOML string
    ///
    /// ## Arguments
    /// * `contents` - TOML string contents
    ///
    /// ## Returns
    /// Parsed release (proto ReleaseSpec internally)
    pub fn from_toml_str(contents: &str) -> Result<Self, ReleaseError> {
        // Parse TOML into intermediate representation
        let toml_release: ReleaseToml = toml::from_str(contents)?;

        // Convert to proto ReleaseSpec
        let spec = convert_toml_to_proto(toml_release)?;

        Ok(Release { spec })
    }

    /// Get release name
    pub fn name(&self) -> &str {
        &self.spec.name
    }

    /// Get release version
    pub fn version(&self) -> &str {
        &self.spec.version
    }

    /// Get release description
    pub fn description(&self) -> &str {
        &self.spec.description
    }

    /// Get proto ReleaseSpec
    pub fn spec(&self) -> &ReleaseSpec {
        &self.spec
    }

    /// Get mutable proto ReleaseSpec
    pub fn spec_mut(&mut self) -> &mut ReleaseSpec {
        &mut self.spec
    }

    /// Get applications in dependency order (for startup)
    ///
    /// ## Purpose
    /// Performs topological sort to determine correct startup order.
    /// Dependencies start before dependents.
    ///
    /// ## Returns
    /// Applications in startup order
    ///
    /// ## Errors
    /// - `ReleaseError::CircularDependency` if cycle detected
    /// - `ReleaseError::MissingDependency` if dependency not found
    pub fn get_applications_in_start_order(&self) -> Result<Vec<&ApplicationConfig>, ReleaseError> {
        use std::collections::{HashMap, HashSet};

        // Build dependency graph
        let mut app_map: HashMap<String, &ApplicationConfig> = HashMap::new();
        for app in &self.spec.applications {
            app_map.insert(app.name.clone(), app);
        }

        // Topological sort using Kahn's algorithm
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();

        // Initialize in-degree and adjacency list
        for app in &self.spec.applications {
            in_degree.insert(app.name.clone(), 0);
            adjacency.insert(app.name.clone(), Vec::new());
        }

        // Build graph: for each app, add edges from dependencies to this app
        for app in &self.spec.applications {
            for dep_name in &app.dependencies {
                // Verify dependency exists
                if !app_map.contains_key(dep_name) {
                    return Err(ReleaseError::MissingDependency {
                        application: app.name.clone(),
                        dependency: dep_name.clone(),
                    });
                }

                // Add edge: dep_name -> app.name
                adjacency.get_mut(dep_name).unwrap().push(app.name.clone());
                *in_degree.get_mut(&app.name).unwrap() += 1;
            }
        }

        // Kahn's algorithm: repeatedly remove nodes with in-degree 0
        let mut queue: Vec<String> = Vec::new();
        for (app_name, &degree) in &in_degree {
            if degree == 0 {
                queue.push(app_name.clone());
            }
        }

        let mut sorted: Vec<&ApplicationConfig> = Vec::new();

        while let Some(app_name) = queue.pop() {
            sorted.push(app_map[&app_name]);

            // Decrease in-degree for dependents
            if let Some(dependents) = adjacency.get(&app_name) {
                for dependent in dependents {
                    let degree = in_degree.get_mut(dependent).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push(dependent.clone());
                    }
                }
            }
        }

        // Check for circular dependencies
        if sorted.len() != self.spec.applications.len() {
            // Find cycle for error message
            let sorted_names: HashSet<String> = sorted.iter().map(|app| app.name.clone()).collect();
            let cycle_apps: Vec<String> = self
                .spec
                .applications
                .iter()
                .filter(|app| !sorted_names.contains(&app.name))
                .map(|app| app.name.clone())
                .collect();

            return Err(ReleaseError::CircularDependency(cycle_apps.join(" -> ")));
        }

        Ok(sorted)
    }

    /// Get applications in reverse dependency order (for shutdown)
    ///
    /// ## Purpose
    /// Returns applications in shutdown order (reverse of startup).
    /// Children stop before parents (Erlang/OTP pattern).
    pub fn get_applications_in_shutdown_order(
        &self,
    ) -> Result<Vec<&ApplicationConfig>, ReleaseError> {
        let mut start_order = self.get_applications_in_start_order()?;
        start_order.reverse();
        Ok(start_order)
    }
}

/// Convert TOML representation to proto ReleaseSpec
fn convert_toml_to_proto(toml: ReleaseToml) -> Result<ReleaseSpec, ReleaseError> {
    Ok(ReleaseSpec {
        name: toml.release.name,
        version: toml.release.version,
        description: toml.release.description,
        node: Some(NodeConfig {
            id: toml.node.id,
            listen_address: toml.node.listen_address,
            cluster_seed_nodes: toml.node.cluster_seed_nodes,
            default_tenant_id: "internal".to_string(), // Default for local development
            default_namespace: "system".to_string(), // Default for local development
        }),
        runtime: Some(RuntimeConfig {
            grpc: Some(GrpcConfig {
                enabled: toml.runtime.grpc.enabled,
                address: toml.runtime.grpc.address,
                max_connections: toml.runtime.grpc.max_connections,
                keepalive_interval_seconds: toml.runtime.grpc.keepalive_interval_seconds,
                middleware: toml
                    .runtime
                    .grpc
                    .middleware
                    .into_iter()
                    .map(|m| MiddlewareConfig {
                        r#type: m.type_,
                        enabled: m.enabled,
                        config: m
                            .config
                            .into_iter()
                            .map(|(k, v)| (k, v.to_string()))
                            .collect(),
                    })
                    .collect(),
            }),
            health: Some(HealthConfig {
                heartbeat_interval_seconds: toml.runtime.health.heartbeat_interval_seconds,
                heartbeat_timeout_seconds: toml.runtime.health.heartbeat_timeout_seconds,
                registry_url: toml.runtime.health.registry_url,
            }),
            security: convert_security_config(&toml.runtime.security)?,
            blob: None,
            shared_database: None,
            locks_provider: None,
            channel_provider: None,
            tuplespace_provider: None,
            framework_info: None,
            ..Default::default() // Include mailbox_provider default until proto is regenerated
        }),
        system_applications: toml.system_applications.included,
        applications: toml
            .applications
            .into_iter()
            .map(|app| {
                let strategy = match app.shutdown_strategy.as_str() {
                    "brutal_kill" => ShutdownStrategy::ShutdownStrategyBrutalKill as i32,
                    "infinity" => ShutdownStrategy::ShutdownStrategyInfinity as i32,
                    _ => ShutdownStrategy::ShutdownStrategyGraceful as i32,
                };

                ApplicationConfig {
                    name: app.name,
                    version: app.version,
                    config_path: app.config_path,
                    enabled: app.enabled,
                    auto_start: app.auto_start,
                    shutdown_timeout_seconds: app.shutdown_timeout_seconds as i64,
                    shutdown_strategy: strategy,
                    dependencies: app.dependencies,
                }
            })
            .collect(),
        env: toml.env,
        shutdown: Some(ShutdownConfig {
            global_timeout_seconds: toml.shutdown.global_timeout_seconds,
            grace_period_seconds: toml.shutdown.grace_period_seconds,
            grpc_drain_timeout_seconds: toml.shutdown.grpc_drain_timeout_seconds,
        }),
    })
}

/// Convert TOML SecurityConfig to proto SecurityConfig
fn convert_security_config(
    toml: &SecurityConfigToml,
) -> Result<Option<SecurityConfig>, ReleaseError> {
    // If no security config provided, return None
    if toml.service_identity.is_none()
        && toml.mtls.is_none()
        && toml.jwt.is_none()
        && toml.api_keys.is_empty()
    {
        return Ok(None);
    }

    let mut security = SecurityConfig {
        service_identity: None,
        mtls: None,
        jwt: None,
        api_keys: vec![],
        allow_disable_auth: false,
        disable_auth: false,
    };

    // Convert service identity
    if let Some(si) = &toml.service_identity {
        // Read certificate and key from file if path, otherwise use as content
        let certificate = if si.certificate.starts_with("-----BEGIN") {
            si.certificate.as_bytes().to_vec()
        } else {
            std::fs::read(&si.certificate).map_err(|e| {
                ReleaseError::InvalidConfig(format!(
                    "Failed to read certificate file {}: {}",
                    si.certificate, e
                ))
            })?
        };

        let private_key = if si.private_key.starts_with("-----BEGIN") {
            si.private_key.as_bytes().to_vec()
        } else {
            std::fs::read(&si.private_key).map_err(|e| {
                ReleaseError::InvalidConfig(format!(
                    "Failed to read private key file {}: {}",
                    si.private_key, e
                ))
            })?
        };

        security.service_identity = Some(ServiceIdentity {
            service_id: si.service_id.clone(),
            certificate,
            private_key,
            expires_at: si
                .expires_at
                .as_ref()
                .and_then(|s| parse_timestamp(s).ok()),
            allowed_services: si.allowed_services.clone(),
        });
    }

    // Convert mTLS config
    if let Some(mtls) = &toml.mtls {
        let ca_cert = if mtls.ca_certificate.starts_with("-----BEGIN") {
            mtls.ca_certificate.clone()
        } else {
            std::fs::read_to_string(&mtls.ca_certificate).map_err(|e| {
                ReleaseError::InvalidConfig(format!(
                    "Failed to read CA certificate file {}: {}",
                    mtls.ca_certificate, e
                ))
            })?
        };

        security.mtls = Some(MtlsConfig {
            enable_mtls: mtls.enable_mtls,
            ca_certificate_path: ca_cert,
            server_certificate_path: String::new(),  // Not in TOML
            server_key_path: String::new(),  // Not in TOML
            auto_generate: false,  // Not in TOML
            cert_dir: "/app/certs".to_string(),  // Default
            certificate_rotation_interval: mtls
                .certificate_rotation_interval_seconds
                .map(|s| prost_types::Duration {
                    seconds: s as i64,
                    nanos: 0,
                }),
            trusted_services: mtls.trusted_services.clone(),
        });
    }

    // Convert JWT config
    if let Some(jwt) = &toml.jwt {
        // JWT secret should come from environment variable
        let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| String::new());
        security.jwt = Some(JwtConfig {
            enable_jwt: jwt.enable_jwt,
            secret,
            issuer: jwt.issuer.clone(),
            jwks_url: jwt.jwks_url.clone(),
            allowed_audiences: jwt.allowed_audiences.clone(),
            token_ttl: jwt.token_ttl_seconds.map(|s| prost_types::Duration {
                seconds: s as i64,
                nanos: 0,
            }),
            refresh_token_ttl: None,  // Not in TOML
            tenant_id_claim: "tenant_id".to_string(),  // Default
            user_id_claim: "sub".to_string(),  // Default
        });
    }

    // Convert API keys
    for key in &toml.api_keys {
        security.api_keys.push(ApiKey {
            id: key.id.clone(),
            name: key.name.clone(),
            description: key.description.clone().unwrap_or_default(),
            key_hash: key.key_hash.clone(),
            scopes: key.scopes.clone(),
            expires_at: key
                .expires_at
                .as_ref()
                .and_then(|s| parse_timestamp(s).ok()),
            last_used: None,
            metadata: if key.metadata.is_empty() {
                None
            } else {
                // Convert HashMap to Metadata proto type
                let mut labels = std::collections::HashMap::new();
                for (k, v) in &key.metadata {
                    labels.insert(k.clone(), v.clone());
                }
                Some(plexspaces_proto::common::v1::Metadata {
                    create_time: None,
                    update_time: None,
                    created_by: String::new(),
                    updated_by: String::new(),
                    labels,
                    annotations: std::collections::HashMap::new(),
                })
            },
        });
    }

    Ok(Some(security))
}

/// Parse timestamp from string (Unix timestamp or ISO 8601)
fn parse_timestamp(s: &str) -> Result<prost_types::Timestamp, ReleaseError> {
    // Try Unix timestamp first
    if let Ok(secs) = s.parse::<i64>() {
        return Ok(prost_types::Timestamp {
            seconds: secs,
            nanos: 0,
        });
    }

    // Try ISO 8601 (simplified - just parse as Unix timestamp for now)
    // Full ISO 8601 parsing would require chrono dependency
    Err(ReleaseError::InvalidConfig(format!(
        "Invalid timestamp format: {} (expected Unix timestamp)",
        s
    )))
}

// ============================================================================
// TESTS (TDD Approach)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test: Parse minimal release.toml
    #[test]
    fn test_parse_minimal_release() {
        let toml = r#"
            [release]
            name = "test-release"
            version = "1.0.0"
            description = "Test release"

            [node]
            id = "node1"
            listen_address = "0.0.0.0:9001"

            [runtime.grpc]
            enabled = true
            address = "0.0.0.0:9001"
            max_connections = 1000
            keepalive_interval_seconds = 30

            [runtime.health]
            heartbeat_interval_seconds = 2
            heartbeat_timeout_seconds = 10
            registry_url = "http://localhost:9000"
        "#;

        let release = Release::from_toml_str(toml).expect("Failed to parse TOML");

        assert_eq!(release.name(), "test-release");
        assert_eq!(release.version(), "1.0.0");
        assert_eq!(release.spec().node.as_ref().unwrap().id, "node1");
        assert_eq!(
            release
                .spec()
                .runtime
                .as_ref()
                .unwrap()
                .grpc
                .as_ref()
                .unwrap()
                .enabled,
            true
        );
        assert_eq!(
            release
                .spec()
                .runtime
                .as_ref()
                .unwrap()
                .grpc
                .as_ref()
                .unwrap()
                .max_connections,
            1000
        );
        assert_eq!(
            release
                .spec()
                .runtime
                .as_ref()
                .unwrap()
                .health
                .as_ref()
                .unwrap()
                .heartbeat_interval_seconds,
            2
        );
    }

    /// Test: Parse release with applications
    #[test]
    fn test_parse_release_with_applications() {
        let toml = r#"
            [release]
            name = "test-release"
            version = "1.0.0"
            description = "Test release"

            [node]
            id = "node1"
            listen_address = "0.0.0.0:9001"

            [runtime.grpc]
            enabled = true
            address = "0.0.0.0:9001"
            max_connections = 1000
            keepalive_interval_seconds = 30

            [runtime.health]
            heartbeat_interval_seconds = 2
            heartbeat_timeout_seconds = 10
            registry_url = "http://localhost:9000"

            [[applications]]
            name = "app1"
            version = "0.1.0"
            config_path = "apps/app1.toml"
            enabled = true
            auto_start = true
            shutdown_timeout_seconds = 30
            shutdown_strategy = "graceful"

            [[applications]]
            name = "app2"
            version = "0.1.0"
            config_path = "apps/app2.toml"
            enabled = true
            auto_start = false
            shutdown_timeout_seconds = 60
            shutdown_strategy = "brutal_kill"
        "#;

        let release = Release::from_toml_str(toml).expect("Failed to parse TOML");

        assert_eq!(release.spec().applications.len(), 2);
        assert_eq!(release.spec().applications[0].name, "app1");
        assert_eq!(release.spec().applications[0].shutdown_timeout_seconds, 30);
        assert_eq!(
            release.spec().applications[0].shutdown_strategy,
            ShutdownStrategy::ShutdownStrategyGraceful as i32
        );

        assert_eq!(release.spec().applications[1].name, "app2");
        assert_eq!(release.spec().applications[1].auto_start, false);
        assert_eq!(
            release.spec().applications[1].shutdown_strategy,
            ShutdownStrategy::ShutdownStrategyBrutalKill as i32
        );
    }

    /// Test: Parse release with shutdown config
    #[test]
    fn test_parse_shutdown_config() {
        let toml = r#"
            [release]
            name = "test-release"
            version = "1.0.0"
            description = "Test release"

            [node]
            id = "node1"
            listen_address = "0.0.0.0:9001"

            [runtime.grpc]
            enabled = true
            address = "0.0.0.0:9001"
            max_connections = 1000
            keepalive_interval_seconds = 30

            [runtime.health]
            heartbeat_interval_seconds = 2
            heartbeat_timeout_seconds = 10
            registry_url = "http://localhost:9000"

            [shutdown]
            global_timeout_seconds = 300
            grace_period_seconds = 10
            grpc_drain_timeout_seconds = 30
        "#;

        let release = Release::from_toml_str(toml).expect("Failed to parse TOML");

        let shutdown = release.spec().shutdown.as_ref().unwrap();
        assert_eq!(shutdown.global_timeout_seconds, 300);
        assert_eq!(shutdown.grace_period_seconds, 10);
        assert_eq!(shutdown.grpc_drain_timeout_seconds, 30);
    }

    /// Test: Default shutdown config
    #[test]
    fn test_default_shutdown_config() {
        let toml = r#"
            [release]
            name = "test-release"
            version = "1.0.0"
            description = "Test release"

            [node]
            id = "node1"
            listen_address = "0.0.0.0:9001"

            [runtime.grpc]
            enabled = true
            address = "0.0.0.0:9001"
            max_connections = 1000
            keepalive_interval_seconds = 30

            [runtime.health]
            heartbeat_interval_seconds = 2
            heartbeat_timeout_seconds = 10
            registry_url = "http://localhost:9000"
        "#;

        let release = Release::from_toml_str(toml).expect("Failed to parse TOML");

        // Should have default values
        let shutdown = release.spec().shutdown.as_ref().unwrap();
        assert_eq!(shutdown.global_timeout_seconds, 300);
        assert_eq!(shutdown.grace_period_seconds, 10);
        assert_eq!(shutdown.grpc_drain_timeout_seconds, 30);
    }

    /// Test: Parse release with middleware
    #[test]
    fn test_parse_middleware() {
        let toml = r#"
            [release]
            name = "test-release"
            version = "1.0.0"
            description = "Test release"

            [node]
            id = "node1"
            listen_address = "0.0.0.0:9001"

            [runtime.grpc]
            enabled = true
            address = "0.0.0.0:9001"
            max_connections = 1000
            keepalive_interval_seconds = 30

            [[runtime.grpc.middleware]]
            type = "metrics"
            enabled = true

            [[runtime.grpc.middleware]]
            type = "tracing"
            enabled = true

            [runtime.health]
            heartbeat_interval_seconds = 2
            heartbeat_timeout_seconds = 10
            registry_url = "http://localhost:9000"
        "#;

        let release = Release::from_toml_str(toml).expect("Failed to parse TOML");

        let middleware = &release
            .spec()
            .runtime
            .as_ref()
            .unwrap()
            .grpc
            .as_ref()
            .unwrap()
            .middleware;
        assert_eq!(middleware.len(), 2);
        assert_eq!(middleware[0].r#type, "metrics");
        assert_eq!(middleware[0].enabled, true);
        assert_eq!(middleware[1].r#type, "tracing");
    }

    /// Test: Get applications in start order
    #[test]
    fn test_get_applications_in_start_order() {
        let toml = r#"
            [release]
            name = "test-release"
            version = "1.0.0"
            description = "Test release"

            [node]
            id = "node1"
            listen_address = "0.0.0.0:9001"

            [runtime.grpc]
            enabled = true
            address = "0.0.0.0:9001"
            max_connections = 1000
            keepalive_interval_seconds = 30

            [runtime.health]
            heartbeat_interval_seconds = 2
            heartbeat_timeout_seconds = 10
            registry_url = "http://localhost:9000"

            [[applications]]
            name = "app1"
            version = "0.1.0"
            config_path = "apps/app1.toml"
            enabled = true
            auto_start = true
            shutdown_timeout_seconds = 30

            [[applications]]
            name = "app2"
            version = "0.1.0"
            config_path = "apps/app2.toml"
            enabled = true
            auto_start = true
            shutdown_timeout_seconds = 30
        "#;

        let release = Release::from_toml_str(toml).expect("Failed to parse TOML");
        let start_order = release
            .get_applications_in_start_order()
            .expect("Failed to get start order");

        // When there are no dependencies, order is non-deterministic
        // Just verify both apps are present
        assert_eq!(start_order.len(), 2);
        let names: Vec<&str> = start_order.iter().map(|app| app.name.as_str()).collect();
        assert!(names.contains(&"app1"));
        assert!(names.contains(&"app2"));
    }

    /// Test: Get applications in shutdown order (reverse)
    #[test]
    fn test_get_applications_in_shutdown_order() {
        // Test with dependencies to ensure deterministic order
        use plexspaces_proto::node::v1::{
            ApplicationConfig, NodeConfig, ReleaseSpec, RuntimeConfig,
        };

        let spec = ReleaseSpec {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            node: Some(NodeConfig {
                id: "node1".to_string(),
                listen_address: "0.0.0.0:9001".to_string(),
                cluster_seed_nodes: vec![],
                default_tenant_id: "internal".to_string(),
                default_namespace: "system".to_string(),
            }),
            runtime: Some(RuntimeConfig {
                grpc: None,
                health: None,
                security: None,
                blob: None,
                shared_database: None,
                locks_provider: None,
                channel_provider: None,
                tuplespace_provider: None,
                framework_info: None,
                ..Default::default() // Include mailbox_provider default until proto is regenerated
            }),
            system_applications: vec![],
            applications: vec![
                ApplicationConfig {
                    name: "app-b".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/b.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec!["app-a".to_string()],
                },
                ApplicationConfig {
                    name: "app-a".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/a.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec![],
                },
            ],
            env: std::collections::HashMap::new(),
            shutdown: None,
        };

        let release = Release { spec };
        let start_order = release.get_applications_in_start_order().unwrap();
        let shutdown_order = release.get_applications_in_shutdown_order().unwrap();

        // With dependencies: app-a starts first, app-b starts second
        assert_eq!(start_order.len(), 2);
        assert_eq!(start_order[0].name, "app-a");
        assert_eq!(start_order[1].name, "app-b");

        // Shutdown should be reversed: app-b stops first, app-a stops second
        assert_eq!(shutdown_order.len(), 2);
        assert_eq!(shutdown_order[0].name, "app-b");
        assert_eq!(shutdown_order[1].name, "app-a");
    }

    /// Test: Environment variables
    #[test]
    fn test_environment_variables() {
        let toml = r#"
            [release]
            name = "test-release"
            version = "1.0.0"
            description = "Test release"

            [node]
            id = "node1"
            listen_address = "0.0.0.0:9001"

            [runtime.grpc]
            enabled = true
            address = "0.0.0.0:9001"
            max_connections = 1000
            keepalive_interval_seconds = 30

            [runtime.health]
            heartbeat_interval_seconds = 2
            heartbeat_timeout_seconds = 10
            registry_url = "http://localhost:9000"

            [env]
            LOG_LEVEL = "info"
            TUPLESPACE_BACKEND = "redis"
            REDIS_URL = "redis://localhost:6379"
        "#;

        let release = Release::from_toml_str(toml).expect("Failed to parse TOML");

        assert_eq!(
            release.spec().env.get("LOG_LEVEL"),
            Some(&"info".to_string())
        );
        assert_eq!(
            release.spec().env.get("TUPLESPACE_BACKEND"),
            Some(&"redis".to_string())
        );
        assert_eq!(
            release.spec().env.get("REDIS_URL"),
            Some(&"redis://localhost:6379".to_string())
        );
    }

    /// Test: Dependency resolution - simple chain
    #[test]
    fn test_dependency_resolution_simple_chain() {
        use plexspaces_proto::node::v1::{
            ApplicationConfig, NodeConfig, ReleaseSpec, RuntimeConfig,
        };

        // Create: app-c depends on app-b, app-b depends on app-a
        let spec = ReleaseSpec {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            node: Some(NodeConfig {
                id: "node1".to_string(),
                listen_address: "0.0.0.0:9001".to_string(),
                cluster_seed_nodes: vec![],
                default_tenant_id: "internal".to_string(),
                default_namespace: "system".to_string(),
            }),
            runtime: Some(RuntimeConfig {
                grpc: None,
                health: None,
                security: None,
                blob: None,
                shared_database: None,
                locks_provider: None,
                channel_provider: None,
                tuplespace_provider: None,
                framework_info: None,
                ..Default::default() // Include mailbox_provider default until proto is regenerated
            }),
            system_applications: vec![],
            applications: vec![
                ApplicationConfig {
                    name: "app-c".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/c.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec!["app-b".to_string()],
                },
                ApplicationConfig {
                    name: "app-a".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/a.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec![],
                },
                ApplicationConfig {
                    name: "app-b".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/b.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec!["app-a".to_string()],
                },
            ],
            env: std::collections::HashMap::new(),
            shutdown: None,
        };

        let release = Release { spec };
        let ordered = release.get_applications_in_start_order().unwrap();

        // Should be: app-a, app-b, app-c
        assert_eq!(ordered.len(), 3);
        assert_eq!(ordered[0].name, "app-a");
        assert_eq!(ordered[1].name, "app-b");
        assert_eq!(ordered[2].name, "app-c");
    }

    /// Test: Dependency resolution - multiple dependencies
    #[test]
    fn test_dependency_resolution_multiple_deps() {
        use plexspaces_proto::node::v1::{
            ApplicationConfig, NodeConfig, ReleaseSpec, RuntimeConfig,
        };

        // Create: app-d depends on app-b AND app-c
        let spec = ReleaseSpec {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            node: Some(NodeConfig {
                id: "node1".to_string(),
                listen_address: "0.0.0.0:9001".to_string(),
                cluster_seed_nodes: vec![],
                default_tenant_id: "internal".to_string(),
                default_namespace: "system".to_string(),
            }),
            runtime: Some(RuntimeConfig {
                grpc: None,
                health: None,
                security: None,
                blob: None,
                shared_database: None,
                locks_provider: None,
                channel_provider: None,
                tuplespace_provider: None,
                framework_info: None,
                ..Default::default() // Include mailbox_provider default until proto is regenerated
            }),
            system_applications: vec![],
            applications: vec![
                ApplicationConfig {
                    name: "app-d".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/d.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec!["app-b".to_string(), "app-c".to_string()],
                },
                ApplicationConfig {
                    name: "app-a".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/a.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec![],
                },
                ApplicationConfig {
                    name: "app-b".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/b.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec!["app-a".to_string()],
                },
                ApplicationConfig {
                    name: "app-c".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/c.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec!["app-a".to_string()],
                },
            ],
            env: std::collections::HashMap::new(),
            shutdown: None,
        };

        let release = Release { spec };
        let ordered = release.get_applications_in_start_order().unwrap();

        // Should start: app-a first, then app-b and app-c (order undefined), then app-d
        assert_eq!(ordered.len(), 4);
        assert_eq!(ordered[0].name, "app-a");
        assert_eq!(ordered[3].name, "app-d");
        // app-b and app-c can be in either order
        assert!(ordered[1].name == "app-b" || ordered[1].name == "app-c");
        assert!(ordered[2].name == "app-b" || ordered[2].name == "app-c");
    }

    /// Test: Circular dependency detection
    #[test]
    fn test_circular_dependency_detection() {
        use plexspaces_proto::node::v1::{
            ApplicationConfig, NodeConfig, ReleaseSpec, RuntimeConfig,
        };

        // Create: app-a depends on app-b, app-b depends on app-a (circular!)
        let spec = ReleaseSpec {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            node: Some(NodeConfig {
                id: "node1".to_string(),
                listen_address: "0.0.0.0:9001".to_string(),
                cluster_seed_nodes: vec![],
                default_tenant_id: "internal".to_string(),
                default_namespace: "system".to_string(),
            }),
            runtime: Some(RuntimeConfig {
                grpc: None,
                health: None,
                security: None,
                blob: None,
                shared_database: None,
                locks_provider: None,
                channel_provider: None,
                tuplespace_provider: None,
                framework_info: None,
                ..Default::default() // Include mailbox_provider default until proto is regenerated
            }),
            system_applications: vec![],
            applications: vec![
                ApplicationConfig {
                    name: "app-a".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/a.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec!["app-b".to_string()],
                },
                ApplicationConfig {
                    name: "app-b".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/b.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec!["app-a".to_string()],
                },
            ],
            env: std::collections::HashMap::new(),
            shutdown: None,
        };

        let release = Release { spec };
        let result = release.get_applications_in_start_order();

        assert!(result.is_err());
        match result.unwrap_err() {
            ReleaseError::CircularDependency(_) => (), // Expected
            other => panic!("Expected CircularDependency, got: {:?}", other),
        }
    }

    /// Test: Missing dependency detection
    #[test]
    fn test_missing_dependency_detection() {
        use plexspaces_proto::node::v1::{
            ApplicationConfig, NodeConfig, ReleaseSpec, RuntimeConfig,
        };

        // Create: app-a depends on "nonexistent" which doesn't exist
        let spec = ReleaseSpec {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            node: Some(NodeConfig {
                id: "node1".to_string(),
                listen_address: "0.0.0.0:9001".to_string(),
                cluster_seed_nodes: vec![],
                default_tenant_id: "internal".to_string(),
                default_namespace: "system".to_string(),
            }),
            runtime: Some(RuntimeConfig {
                grpc: None,
                health: None,
                security: None,
                blob: None,
                shared_database: None,
                locks_provider: None,
                channel_provider: None,
                tuplespace_provider: None,
                framework_info: None,
                ..Default::default() // Include mailbox_provider default until proto is regenerated
            }),
            system_applications: vec![],
            applications: vec![ApplicationConfig {
                name: "app-a".to_string(),
                version: "1.0.0".to_string(),
                config_path: "apps/a.toml".to_string(),
                enabled: true,
                auto_start: true,
                shutdown_timeout_seconds: 30,
                shutdown_strategy: 1,
                dependencies: vec!["nonexistent".to_string()],
            }],
            env: std::collections::HashMap::new(),
            shutdown: None,
        };

        let release = Release { spec };
        let result = release.get_applications_in_start_order();

        assert!(result.is_err());
        match result.unwrap_err() {
            ReleaseError::MissingDependency {
                application,
                dependency,
            } => {
                assert_eq!(application, "app-a");
                assert_eq!(dependency, "nonexistent");
            }
            other => panic!("Expected MissingDependency, got: {:?}", other),
        }
    }

    /// Test: No dependencies (should return apps in original order)
    #[test]
    fn test_no_dependencies() {
        use plexspaces_proto::node::v1::{
            ApplicationConfig, NodeConfig, ReleaseSpec, RuntimeConfig,
        };

        let spec = ReleaseSpec {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            node: Some(NodeConfig {
                id: "node1".to_string(),
                listen_address: "0.0.0.0:9001".to_string(),
                cluster_seed_nodes: vec![],
                default_tenant_id: "internal".to_string(),
                default_namespace: "system".to_string(),
            }),
            runtime: Some(RuntimeConfig {
                grpc: None,
                health: None,
                security: None,
                blob: None,
                shared_database: None,
                locks_provider: None,
                channel_provider: None,
                tuplespace_provider: None,
                framework_info: None,
                ..Default::default() // Include mailbox_provider default until proto is regenerated
            }),
            system_applications: vec![],
            applications: vec![
                ApplicationConfig {
                    name: "app-a".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/a.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec![],
                },
                ApplicationConfig {
                    name: "app-b".to_string(),
                    version: "1.0.0".to_string(),
                    config_path: "apps/b.toml".to_string(),
                    enabled: true,
                    auto_start: true,
                    shutdown_timeout_seconds: 30,
                    shutdown_strategy: 1,
                    dependencies: vec![],
                },
            ],
            env: std::collections::HashMap::new(),
            shutdown: None,
        };

        let release = Release { spec };
        let ordered = release.get_applications_in_start_order().unwrap();

        // With no dependencies, any order is valid (apps are independent)
        assert_eq!(ordered.len(), 2);
    }

    /// Test: Parse release.toml with security configuration
    #[test]
    fn test_parse_release_with_security_config() {
        let toml = r#"
            [release]
            name = "test-release"
            version = "1.0.0"
            description = "Test release with security"

            [node]
            id = "node1"
            listen_address = "0.0.0.0:9001"

            [runtime.grpc]
            enabled = true
            address = "0.0.0.0:9001"
            max_connections = 1000
            keepalive_interval_seconds = 30

            [runtime.health]
            heartbeat_interval_seconds = 10
            heartbeat_timeout_seconds = 30
            registry_url = "http://localhost:8080"

            [runtime.security.service_identity]
            service_id = "actor-service"
            certificate = "-----BEGIN CERTIFICATE-----\ntest-cert\n-----END CERTIFICATE-----"
            private_key = "-----BEGIN PRIVATE KEY-----\ntest-key\n-----END PRIVATE KEY-----"
            allowed_services = ["scheduler-service"]

            [runtime.security.mtls]
            enable_mtls = true
            ca_certificate = "-----BEGIN CERTIFICATE-----\nca-cert\n-----END CERTIFICATE-----"
            trusted_services = ["scheduler-service", "node-service"]
            certificate_rotation_interval_seconds = 2592000

            [runtime.security.jwt]
            enable_jwt = true
            issuer = "https://auth.example.com"
            jwks_url = "https://auth.example.com/.well-known/jwks.json"
            allowed_audiences = ["plexspaces-api"]
            token_ttl_seconds = 3600

            [[runtime.security.api_keys]]
            id = "key-123"
            name = "service-api-key"
            description = "API key for service authentication"
            key_hash = "sha256:abc123"
            scopes = ["read", "write"]
        "#;

        let release = Release::from_toml_str(toml).unwrap();
        let spec = release.spec();

        // Verify security config is parsed
        assert!(spec.runtime.is_some());
        let runtime = spec.runtime.as_ref().unwrap();
        assert!(runtime.security.is_some());
        let security = runtime.security.as_ref().unwrap();

        // Verify service identity
        assert!(security.service_identity.is_some());
        let si = security.service_identity.as_ref().unwrap();
        assert_eq!(si.service_id, "actor-service");
        assert_eq!(si.allowed_services.len(), 1);

        // Verify mTLS config
        assert!(security.mtls.is_some());
        let mtls = security.mtls.as_ref().unwrap();
        assert!(mtls.enable_mtls);
        assert_eq!(mtls.trusted_services.len(), 2);

        // Verify JWT config
        assert!(security.jwt.is_some());
        let jwt = security.jwt.as_ref().unwrap();
        assert!(jwt.enable_jwt);
        assert_eq!(jwt.issuer, "https://auth.example.com");

        // Verify API keys
        assert_eq!(security.api_keys.len(), 1);
        assert_eq!(security.api_keys[0].id, "key-123");
    }
}
