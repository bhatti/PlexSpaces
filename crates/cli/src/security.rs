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
// but WITHOUT EVEN THE IMPLIED WARRANTY OF MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU Lesser General Public
// License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Security-related CLI commands
//!
//! ## Purpose
//! CLI commands for managing security configuration:
//! - Generate mTLS certificates
//! - Generate default release config
//! - Create JWT tokens for API authentication (tenant_id, roles, groups, is_admin)

use anyhow::{Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Generate mTLS certificates
///
/// ## Purpose
/// Generates CA and server certificates for mTLS authentication.
/// Uses rcgen to create proper X.509 certificates.
///
/// ## Arguments
/// * `output_dir` - Directory to save certificates (default: ./certs)
/// * `ca_common_name` - Common name for CA certificate (default: "PlexSpaces CA")
/// * `server_common_name` - Common name for server certificate (default: "PlexSpaces Server")
/// * `validity_days` - Validity in days for server certificate (default: 90)
pub async fn generate_mtls_certificates(
    output_dir: Option<PathBuf>,
    ca_common_name: Option<String>,
    server_common_name: Option<String>,
    validity_days: Option<u32>,
) -> Result<()> {
    use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
    use time::OffsetDateTime;

    let cert_dir = output_dir.unwrap_or_else(|| PathBuf::from("./certs"));
    let ca_cn = ca_common_name.unwrap_or_else(|| "PlexSpaces CA".to_string());
    let server_cn = server_common_name.unwrap_or_else(|| "PlexSpaces Server".to_string());
    let validity = validity_days.unwrap_or(90);

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&cert_dir).with_context(|| {
        format!(
            "Failed to create certificate directory: {}",
            cert_dir.display()
        )
    })?;

    println!("Generating mTLS certificates in: {}", cert_dir.display());

    // Generate CA certificate
    println!("  → Generating CA certificate...");
    let ca_key_pair = KeyPair::generate().context("Failed to generate CA key pair")?;

    let mut ca_params =
        CertificateParams::new(vec![]).context("Failed to create CA certificate parameters")?;
    ca_params.distinguished_name = DistinguishedName::new();
    ca_params
        .distinguished_name
        .push(DnType::CommonName, &ca_cn);
    ca_params
        .distinguished_name
        .push(DnType::OrganizationName, "PlexSpaces");
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);

    // rcgen uses time crate, not chrono
    let now = OffsetDateTime::now_utc();
    ca_params.not_before = now;
    ca_params.not_after = now + time::Duration::days(365);

    let ca_cert = ca_params
        .self_signed(&ca_key_pair)
        .context("Failed to generate CA certificate")?;

    let ca_cert_path = cert_dir.join("ca.crt");
    let ca_key_path = cert_dir.join("ca.key");

    std::fs::write(&ca_cert_path, ca_cert.pem().as_bytes())
        .with_context(|| format!("Failed to write CA certificate: {}", ca_cert_path.display()))?;
    std::fs::write(&ca_key_path, ca_key_pair.serialize_pem().as_bytes())
        .with_context(|| format!("Failed to write CA key: {}", ca_key_path.display()))?;

    println!("    ✓ CA certificate: {}", ca_cert_path.display());
    println!("    ✓ CA private key: {}", ca_key_path.display());

    // Generate server certificate signed by CA
    println!("  → Generating server certificate...");
    let server_key_pair = KeyPair::generate().context("Failed to generate server key pair")?;

    let mut server_params =
        CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .context("Failed to create server certificate parameters")?;
    server_params.distinguished_name = DistinguishedName::new();
    server_params
        .distinguished_name
        .push(DnType::CommonName, &server_cn);
    server_params
        .distinguished_name
        .push(DnType::OrganizationName, "PlexSpaces");

    // rcgen uses time crate, not chrono
    let now = OffsetDateTime::now_utc();
    server_params.not_before = now;
    server_params.not_after = now + time::Duration::days(validity as i64);

    let server_cert = server_params
        .signed_by(&server_key_pair, &ca_cert, &ca_key_pair)
        .context("Failed to generate server certificate")?;

    let server_cert_path = cert_dir.join("server.crt");
    let server_key_path = cert_dir.join("server.key");

    std::fs::write(&server_cert_path, server_cert.pem().as_bytes()).with_context(|| {
        format!(
            "Failed to write server certificate: {}",
            server_cert_path.display()
        )
    })?;
    std::fs::write(&server_key_path, server_key_pair.serialize_pem().as_bytes())
        .with_context(|| format!("Failed to write server key: {}", server_key_path.display()))?;

    println!("    ✓ Server certificate: {}", server_cert_path.display());
    println!("    ✓ Server private key: {}", server_key_path.display());

    println!("\n✅ mTLS certificates generated successfully!");
    println!("\nTo use these certificates, set environment variables:");
    println!(
        "  export PLEXSPACES_MTLS_CA_CERT=\"{}\"",
        ca_cert_path.display()
    );
    println!(
        "  export PLEXSPACES_MTLS_SERVER_CERT=\"{}\"",
        server_cert_path.display()
    );
    println!(
        "  export PLEXSPACES_MTLS_SERVER_KEY=\"{}\"",
        server_key_path.display()
    );
    println!("\nOr configure in release.yaml:");
    println!("  runtime:");
    println!("    security:");
    println!("      mtls:");
    println!("        enable_mtls: true");
    println!(
        "        ca_certificate_path: \"{}\"",
        ca_cert_path.display()
    );
    println!(
        "        server_certificate_path: \"{}\"",
        server_cert_path.display()
    );
    println!("        server_key_path: \"{}\"", server_key_path.display());

    Ok(())
}

/// Generate default release configuration
///
/// ## Purpose
/// Generates a default ReleaseSpec configuration file in YAML format that can be customized.
///
/// ## Arguments
/// * `output_path` - Path to save release config (default: release.yaml)
/// * `release_name` - Release name (default: "plexspaces-cluster")
/// * `release_version` - Release version (default: "1.0.0")
/// * `node_id` - Node ID (default: "node-1")
/// * `listen_addr` - Listen address (default: "0.0.0.0:8000")
pub async fn generate_release_config(
    output_path: Option<PathBuf>,
    release_name: Option<String>,
    release_version: Option<String>,
    node_id: Option<String>,
    listen_addr: Option<String>,
) -> Result<()> {
    use plexspaces_common::release_config::create_default_release_config;

    let output = output_path.unwrap_or_else(|| PathBuf::from("release.yaml"));
    let name = release_name.unwrap_or_else(|| "plexspaces-cluster".to_string());
    let version = release_version.unwrap_or_else(|| "1.0.0".to_string());
    let node = node_id.unwrap_or_else(|| "node-1".to_string());
    let addr = listen_addr.unwrap_or_else(|| "0.0.0.0:8000".to_string());

    println!("Generating default release configuration...");
    println!("  Release: {} v{}", name, version);
    println!("  Node ID: {}", node);
    println!("  Listen Address: {}", addr);

    let release_spec =
        create_default_release_config(name.clone(), version.clone(), node.clone(), addr.clone())
            .await;

    // Generate YAML manually (proto types don't serialize directly to YAML)
    let cert_dir =
        std::env::var("PLEXSPACES_MTLS_CERT_DIR").unwrap_or_else(|_| "/app/certs".to_string());

    let yaml = build_release_config_yaml(&release_spec, &node, &addr, &cert_dir);

    std::fs::write(&output, yaml.as_bytes())
        .with_context(|| format!("Failed to write release config: {}", output.display()))?;

    println!("✅ Release configuration generated: {}", output.display());
    println!("\nTo use this configuration:");
    println!(
        "  plexspaces start --node-id {} --listen-addr {} --release-config {}",
        node,
        addr,
        output.display()
    );
    println!("\nTo customize security settings:");
    println!(
        "  1. Edit {} and modify the security section",
        output.display()
    );
    println!("  2. Set PLEXSPACES_JWT_SECRET env var for JWT authentication");
    println!("  3. For mTLS, either:");
    println!("     - Set auto_generate_certs: true (development)");
    println!("     - Or set certificate paths and use plexspaces generate-mtls to create them");

    Ok(())
}

fn build_release_config_yaml(
    release_spec: &plexspaces_proto::node::v1::ReleaseSpec,
    node: &str,
    addr: &str,
    cert_dir: &str,
) -> String {
    let base_dir = "/var/lib/plexspaces";

    format!(
        r#"# PlexSpaces Release Configuration
# Generated by: plexspaces generate-release-config
# 
# This file defines the complete system configuration for a PlexSpaces node.
# Customize as needed for your deployment.

name: {}
version: {}
description: "PlexSpaces release for node {}"

node:
  id: {}
  listen_addr: {}
  cluster_seed_nodes: []

runtime:
  base_dir: "{}"
  wasm_apps_directory: "{}/apps"

  db:
    connection_string: "sqlite://{}/db/plexspaces.db?mode=rwc"
    pool_size: 10
    auto_migrate: true

  grpc:
    enabled: true
    address: {}
    max_connections: 100
    keepalive_interval_seconds: 30
    middleware: []
  
  health:
    heartbeat_interval_seconds: 5
    heartbeat_timeout_seconds: 30
    registry_url: ""
  
  security:
    jwt:
      enable_jwt: true
      # JWT secret should be set via PLEXSPACES_JWT_SECRET env var
      secret: ""  # Leave empty - will be read from PLEXSPACES_JWT_SECRET
      issuer: ""
      jwks_url: ""  # For RS256, set this instead of secret
      allowed_audiences:
        - "plexspaces-api"
      tenant_id_claim: "tenant_id"
      user_id_claim: "sub"
    
    mtls:
      enable_mtls: true
      auto_generate_certs: true  # Auto-generate for development
      cert_dir: "{}"
      # Or specify paths for production:
      # ca_certificate: "/certs/ca.crt"
      # client_certificate: "/certs/server.crt"
      # client_private_key: "/certs/server.key"
    
    disable_auth: false  # Production: false, testing: can be enabled via PLEXSPACES_DISABLE_AUTH env var

system_applications: []

applications: []

env: {{}}

shutdown:
  graceful_timeout_seconds: 30
  force_timeout_seconds: 10
"#,
        release_spec.name,
        release_spec.version,
        node,
        node,
        addr,
        base_dir,
        base_dir,
        base_dir,
        addr,
        cert_dir
    )
}

#[cfg(test)]
mod tests {
    use super::build_release_config_yaml;

    #[tokio::test]
    async fn generated_release_config_uses_base_dir_shared_db_defaults() {
        let release_spec = plexspaces_common::release_config::create_default_release_config(
            "test-release".to_string(),
            "1.0.0".to_string(),
            "node-1".to_string(),
            "0.0.0.0:8000".to_string(),
        )
        .await;

        let yaml = build_release_config_yaml(&release_spec, "node-1", "0.0.0.0:8000", "/app/certs");

        assert!(yaml.contains("base_dir: \"/var/lib/plexspaces\""));
        assert!(yaml.contains("wasm_apps_directory: \"/var/lib/plexspaces/apps\""));
        assert!(yaml.contains(
            "connection_string: \"sqlite:///var/lib/plexspaces/db/plexspaces.db?mode=rwc\""
        ));
        assert!(!yaml.contains("sqlite:///tmp/plexspaces"));
        assert!(!yaml.contains("node:\n  wasm_apps_directory"));
    }
}

/// JWT claims for API authentication (tenant_id, roles, groups, is_admin)
#[derive(Debug, Serialize, Deserialize)]
struct JwtCreateClaims {
    sub: String,
    exp: i64,
    iat: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    iss: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    aud: Vec<String>,
    tenant_id: String,
    roles: Vec<String>,
    groups: Vec<String>,
    is_admin: bool,
}

/// Create a JWT token for API authentication
///
/// ## Purpose
/// Generates a JWT with tenant_id, roles, groups, and is_admin for use with
/// HTTP (Authorization: Bearer) or gRPC metadata. Token is validated by
/// grpc-middleware AuthInterceptor and must be signed with the same secret
/// as configured on the node (PLEXSPACES_JWT_SECRET or release security.jwt.secret).
///
/// ## Arguments
/// * `tenant_id` - Tenant ID (required)
/// * `sub` - Subject / user ID
/// * `roles` - List of roles (e.g. admin, user)
/// * `groups` - List of groups
/// * `is_admin` - Admin flag
/// * `exp_hours` - Validity in hours
/// * `secret` - JWT secret (from --secret or PLEXSPACES_JWT_SECRET)
pub async fn create_jwt_token(
    tenant_id: String,
    sub: String,
    roles: Vec<String>,
    groups: Vec<String>,
    is_admin: bool,
    exp_hours: u32,
    secret: Option<String>,
) -> Result<()> {
    let secret = secret.ok_or_else(|| {
        anyhow::anyhow!(
            "JWT secret required. Set --secret or PLEXSPACES_JWT_SECRET environment variable."
        )
    })?;

    let now = chrono::Utc::now();
    let exp = now + chrono::Duration::hours(exp_hours as i64);
    let claims = JwtCreateClaims {
        sub: sub.clone(),
        exp: exp.timestamp(),
        iat: now.timestamp(),
        iss: String::new(),
        aud: vec![],
        tenant_id: tenant_id.clone(),
        roles,
        groups,
        is_admin,
    };

    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .context("Failed to encode JWT")?;

    println!("{}", token);
    eprintln!(
        "Use with HTTP: Authorization: Bearer <token>\n\
         Tenant: {}  Sub: {}  Admin: {}",
        tenant_id, sub, is_admin
    );
    Ok(())
}
