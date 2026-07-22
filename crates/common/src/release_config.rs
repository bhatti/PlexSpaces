// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT EVEN THE IMPLIED WARRANTY OF MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public
// License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Default Release Configuration Generator
//!
//! ## Purpose
//! Generates default ReleaseSpec configuration based on proto/plexspaces/v1/node/release.proto.
//! Provides sensible defaults for development and production deployments.
//!
//! ## Usage
//! ```rust
//! use plexspaces_common::release_config::create_default_release_config;
//!
//! let config = create_default_release_config(
//!     "my-cluster".to_string(),
//!     "1.0.0".to_string(),
//!     "node-1".to_string(),
//!     "0.0.0.0:8000".to_string(),
//! ).await?;
//! ```

use plexspaces_proto::node::v1::{
    GrpcConfig, HealthConfig, NodeConfig, ReleaseSpec, RuntimeConfig, SecurityConfig,
    ShutdownConfig,
};
use plexspaces_proto::security::v1::{JwtConfig, MtlsConfig};
use prost_types::Duration;

use crate::config_manager::{get_env, get_env_or, ENV_JWT_SECRET, ENV_MTLS_CERT_DIR};

/// Create default release configuration
///
/// ## Purpose
/// Generates a ReleaseSpec with sensible defaults for development and production.
/// Includes default security configuration with auto-generated mTLS certificates.
///
/// ## Arguments
/// * `release_name` - Release name (e.g., "plexspaces-cluster")
/// * `release_version` - Release version (e.g., "1.0.0")
/// * `node_id` - Node ID (e.g., "node-1")
/// * `listen_addr` - gRPC listen address (e.g., "0.0.0.0:8000")
///
/// ## Returns
/// ReleaseSpec with default configuration
///
/// ## Defaults
/// - JWT: Enabled with secret from PLEXSPACES_JWT_SECRET env var
/// - mTLS: Enabled with auto-generation for development
/// - gRPC: Standard configuration
/// - Health: Default heartbeat intervals
pub async fn create_default_release_config(
    release_name: String,
    release_version: String,
    node_id: String,
    listen_addr: String,
) -> ReleaseSpec {
    // Get JWT secret from env var (empty if not set - will use JWKS or fail validation)
    let jwt_secret = get_env(ENV_JWT_SECRET).unwrap_or_default();

    // Get cert directory from env var or use default
    let cert_dir = get_env_or(ENV_MTLS_CERT_DIR, "/app/certs");

    // Create default security config
    let security = Some(SecurityConfig {
        service_identity: None,
        mtls: Some(MtlsConfig {
            enable_mtls: true,
            ca_certificate_path: format!("{}/ca.crt", cert_dir),
            server_certificate_path: format!("{}/server.crt", cert_dir),
            server_key_path: format!("{}/server.key", cert_dir),
            auto_generate: true, // Auto-generate for development
            cert_dir: cert_dir.clone(),
            certificate_rotation_interval: None, // TODO: Certificate rotation
            trusted_services: vec![],
        }),
        jwt: Some(JwtConfig {
            enable_jwt: true,
            secret: jwt_secret,
            issuer: String::new(),
            jwks_url: String::new(),
            allowed_audiences: vec!["plexspaces-api".to_string()],
            token_ttl: Some(Duration {
                seconds: 900,
                nanos: 0,
            }),
            refresh_token_ttl: Some(Duration {
                seconds: 604800,
                nanos: 0,
            }),
            tenant_id_claim: "tenant_id".to_string(),
            user_id_claim: "sub".to_string(),
            algorithm: "ES256".to_string(),
            private_key_pem: String::new(),
            private_key_file: String::new(),
            auto_generate_key: true,
        }),
        api_keys: vec![],
        disable_auth: false,
        oidc: None,
    });

    // Create default node config
    // NOTE: default_tenant_id and default_namespace have been removed.
    // Tenant-id comes from auth (JWT/mTLS); namespace from application/actor.
    let node = NodeConfig {
        id: node_id.clone(),
        listen_addr: listen_addr.clone(),
        cluster_seed_nodes: vec![],
        cluster_name: String::new(),
        max_connections: 100,
        heartbeat_interval_ms: 5000,
        clustering_enabled: false, // Disabled by default
        grpc_connection_pool_size: 2,
        metadata: std::collections::HashMap::new(),
        node_registry: None,
        grpc_address: crate::dialable_node_address(&listen_addr),
        blob_http_port: 0,
    };

    // Create default gRPC config
    let grpc = GrpcConfig {
        enabled: true,
        address: listen_addr.clone(),
        max_connections: 100,
        keepalive_interval_seconds: 30,
        middleware: vec![],
    };

    // Create default health config
    let health = HealthConfig {
        heartbeat_interval_seconds: 5,
        heartbeat_timeout_seconds: 30,
        registry_url: String::new(),
    };

    // Create default runtime config
    // Note: base_dir, wasm_apps_directory, db, channel_provider, mailbox_provider
    // are set by config_manager::initialize()
    let runtime = RuntimeConfig {
        save_wasm_apps: false, // Default: disabled (only for testing)
        grpc: Some(grpc),
        health: Some(health),
        security,
        blob: None,
        db: None, // Set by config_manager::initialize
        locks_provider: None,
        channel_provider: 0, // ChannelProvider::ChannelProviderInMemory - set by config_manager::initialize
        mailbox_provider: 0, // ChannelProvider::ChannelProviderInMemory - set by config_manager::initialize
        framework_info: None,
        base_dir: String::new(), // Set by config_manager::initialize
        wasm_apps_directory: String::new(), // Set by config_manager::initialize
        default_virtual_actor_config: None, // Defaults applied in code when None (5m, pool 100, lazy)
        service_links: vec![],
        default_outbound_client_policy: None,
        outbound_policy_templates: std::collections::HashMap::new(),
        static_dirs: vec![], // Set by config_manager::initialize from PLEXSPACES_STATIC_DIRS
    };

    // Create default shutdown config
    let shutdown = ShutdownConfig {
        global_timeout_seconds: 30,
        grace_period_seconds: 5,
        grpc_drain_timeout_seconds: 10,
    };

    ReleaseSpec {
        name: release_name,
        version: release_version,
        description: format!("PlexSpaces release for node {}", node_id),
        node: Some(node),
        runtime: Some(runtime),
        system_applications: vec![], // System apps are always included
        applications: vec![],        // User applications can be added via config
        env: std::collections::HashMap::new(),
        shutdown: Some(shutdown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_default_release_config() {
        let config = create_default_release_config(
            "test-cluster".to_string(),
            "1.0.0".to_string(),
            "test-node".to_string(),
            "0.0.0.0:8000".to_string(),
        )
        .await;

        assert_eq!(config.name, "test-cluster");
        assert_eq!(config.version, "1.0.0");
        assert!(config.node.is_some());
        assert!(config.runtime.is_some());
        assert!(config.shutdown.is_some());

        let node = config.node.as_ref().unwrap();
        assert_eq!(node.id, "test-node");
        assert_eq!(node.listen_addr, "0.0.0.0:8000");

        let runtime = config.runtime.as_ref().unwrap();
        assert!(runtime.security.is_some());

        let security = runtime.security.as_ref().unwrap();
        assert!(security.mtls.is_some());
        assert!(security.jwt.is_some());

        let mtls = security.mtls.as_ref().unwrap();
        assert!(mtls.enable_mtls);
        assert!(mtls.auto_generate);
    }
}
