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
// but WITHOUT EVEN THE IMPLIED WARRANTY OF MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU Lesser General Public
// License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Security Configuration Validator
//!
//! ## Purpose
//! Validates security configuration to ensure production-grade security:
//! - JWT secrets are provided when JWT auth is enabled
//! - mTLS keys are available when mTLS is enabled
//! - Supports environment variables for secrets
//! - Supports auto-generation of mTLS keys for development
//!
//! ## Security Best Practices
//! - Secrets must come from environment variables, never hardcoded
//! - Fatal errors if auth is enabled but keys are missing
//! - Auto-generation only for development/testing
//! - Production must use proper key management

use plexspaces_proto::node::v1::SecurityConfig;
use plexspaces_proto::security::v1::{JwtConfig, MtlsConfig};
use std::fs;
use std::path::Path;

use crate::config_manager::{
    get_env, get_env_bool, ENV_DISABLE_AUTH, ENV_JWT_SECRET, ENV_MTLS_CERT_DIR,
};

/// Security configuration validation errors
#[derive(Debug, thiserror::Error)]
pub enum SecurityValidationError {
    #[error("JWT authentication is enabled but secret is missing. Required: Set PLEXSPACES_JWT_SECRET environment variable, or configure SecurityConfig.jwt.secret in release config")]
    MissingJwtSecret,

    #[error("mTLS is enabled but CA certificate is missing. Required: Set PLEXSPACES_MTLS_CA_CERT environment variable (file path), or configure SecurityConfig.mtls.ca_cert_path in release config. Current value: {0}")]
    MissingCaCertificate(String),

    #[error("mTLS is enabled but server certificate is missing. Required: Set PLEXSPACES_MTLS_SERVER_CERT environment variable (file path), or configure SecurityConfig.mtls.server_cert_path in release config. Current value: {0}")]
    MissingServerCertificate(String),

    #[error("mTLS is enabled but server key is missing. Required: Set PLEXSPACES_MTLS_SERVER_KEY environment variable (file path), or configure SecurityConfig.mtls.server_key_path in release config. Current value: {0}")]
    MissingServerKey(String),

    #[error("Failed to read certificate file {0}: {1}")]
    CertificateReadError(String, std::io::Error),

    #[error("Failed to create certificate directory {0}: {1}. Required: Set PLEXSPACES_MTLS_CERT_DIR environment variable (writable directory path), or configure SecurityConfig.mtls.cert_dir in release config. For testing, set PLEXSPACES_DISABLE_AUTH=1")]
    CertificateDirError(String, std::io::Error),

    #[error("Auto-generation of mTLS certificates failed: {0}")]
    AutoGenerationFailed(String),
}

/// Validate security configuration
///
/// ## Purpose
/// Ensures that when authentication is enabled, all required keys/secrets are available.
/// Throws fatal errors if auth is enabled but keys are missing.
///
/// ## Arguments
/// * `config` - SecurityConfig to validate
///
/// ## Returns
/// * `Ok(())` - Configuration is valid
/// * `Err(SecurityValidationError)` - Configuration is invalid (fatal)
///
/// ## Behavior
/// - If auth is disabled (via PLEXSPACES_DISABLE_AUTH or disable_auth=true), validation is skipped
/// - If JWT is enabled, JWT secret must be available (from env var or config)
/// - If mTLS is enabled, certificate files must exist (or auto-generation must be enabled)
pub async fn validate_security_config(
    config: &SecurityConfig,
) -> Result<(), SecurityValidationError> {
    // Check if auth is disabled via env variable (for testing)
    if get_env_bool(ENV_DISABLE_AUTH) {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                "Auth disabled via {} - skipping security validation",
                ENV_DISABLE_AUTH
            );
        }
        return Ok(());
    }

    // Check if auth is disabled in config
    if config.disable_auth {
        tracing::debug!("Auth disabled in SecurityConfig - skipping security validation");
        return Ok(());
    }

    // Validate JWT config if enabled
    // Note: JWT is typically for public APIs, mTLS for node-to-node
    // Both can be enabled simultaneously
    if let Some(ref jwt) = config.jwt {
        if jwt.enable_jwt {
            validate_jwt_config(jwt)?;
        }
    } else {
        // If JWT is not configured but we're not in disable mode, that's OK
        // (might be using mTLS only, or auth disabled via other means)
        tracing::debug!("JWT config not provided - assuming not needed");
    }

    // Validate mTLS config if enabled
    // Note: mTLS is for node-to-node authentication
    if let Some(ref mtls) = config.mtls {
        if mtls.enable_mtls {
            validate_mtls_config(mtls).await?;
        }
    } else {
        // If mTLS is not configured but we're not in disable mode, that's OK
        // (might be using JWT only, or auth disabled via other means)
        tracing::debug!("mTLS config not provided - assuming not needed");
    }

    Ok(())
}

/// Validate JWT configuration
///
/// ## Requirements
/// - JWT secret must be available (from env var PLEXSPACES_JWT_SECRET or config.secret)
/// - If using JWKS (RS256), jwks_url must be provided
fn validate_jwt_config(jwt: &JwtConfig) -> Result<(), SecurityValidationError> {
    // If JWT is disabled, no validation needed
    if !jwt.enable_jwt {
        tracing::debug!("JWT authentication is disabled - skipping validation");
        return Ok(());
    }

    // Check if using JWKS (RS256) - no secret needed
    if !jwt.jwks_url.is_empty() {
        tracing::debug!("JWT using JWKS (RS256) - no secret required");
        return Ok(());
    }

    // For HS256, we need a secret
    // Priority: 1. Env var PLEXSPACES_JWT_SECRET, 2. Config.secret
    let secret = get_env(ENV_JWT_SECRET).or_else(|| {
        if !jwt.secret.is_empty() {
            Some(jwt.secret.clone())
        } else {
            None
        }
    });

    if secret.is_none() || secret.as_ref().unwrap().is_empty() {
        return Err(SecurityValidationError::MissingJwtSecret);
    }

    tracing::debug!("JWT secret validated (from env var or config)");
    Ok(())
}

/// Validate mTLS configuration
///
/// ## Requirements
/// - If auto_generate is true, certificates will be generated
/// - Otherwise, certificate files must exist at specified paths
/// - Supports environment variables for paths (PLEXSPACES_MTLS_CA_CERT, etc.)
async fn validate_mtls_config(mtls: &MtlsConfig) -> Result<(), SecurityValidationError> {
    // If mTLS is disabled, no validation needed
    if !mtls.enable_mtls {
        tracing::debug!("mTLS authentication is disabled - skipping validation");
        return Ok(());
    }

    // If auto-generation is enabled, generate certificates
    if mtls.auto_generate {
        return generate_mtls_certificates(mtls).await;
    }

    // Otherwise, validate that certificate files exist
    // Support env variables for paths
    let ca_cert_path = resolve_path(&mtls.ca_certificate_path, "PLEXSPACES_MTLS_CA_CERT")?;
    let server_cert_path =
        resolve_path(&mtls.server_certificate_path, "PLEXSPACES_MTLS_SERVER_CERT")?;
    let server_key_path = resolve_path(&mtls.server_key_path, "PLEXSPACES_MTLS_SERVER_KEY")?;

    // Check CA certificate
    if ca_cert_path.is_empty() {
        return Err(SecurityValidationError::MissingCaCertificate(
            "path is empty".to_string(),
        ));
    }
    if !Path::new(&ca_cert_path).exists() {
        return Err(SecurityValidationError::MissingCaCertificate(ca_cert_path));
    }

    // Check server certificate
    if server_cert_path.is_empty() {
        return Err(SecurityValidationError::MissingServerCertificate(
            "path is empty".to_string(),
        ));
    }
    if !Path::new(&server_cert_path).exists() {
        return Err(SecurityValidationError::MissingServerCertificate(
            server_cert_path,
        ));
    }

    // Check server key
    if server_key_path.is_empty() {
        return Err(SecurityValidationError::MissingServerKey(
            "path is empty".to_string(),
        ));
    }
    if !Path::new(&server_key_path).exists() {
        return Err(SecurityValidationError::MissingServerKey(server_key_path));
    }

    tracing::debug!(
        ca_cert = %ca_cert_path,
        server_cert = %server_cert_path,
        server_key = %server_key_path,
        "mTLS certificate files validated"
    );

    Ok(())
}

/// Resolve certificate path (env var or config)
fn resolve_path(config_path: &str, env_var: &str) -> Result<String, SecurityValidationError> {
    // Priority: 1. Env var, 2. Config path
    if let Some(env_path) = get_env(env_var) {
        return Ok(env_path);
    }

    if !config_path.is_empty() {
        return Ok(config_path.to_string());
    }

    Ok(String::new())
}

/// Auto-generate mTLS certificates
///
/// ## Purpose
/// Generates self-signed certificates for development/testing.
/// Production should use proper certificate management (cert-manager, Vault, etc.).
///
/// ## Implementation
/// Uses rcgen to generate proper X.509 certificates.
/// Saves them to cert_dir (default: /app/certs).
///
/// ## TODO: Certificate Rotation
/// - [ ] Implement automatic certificate rotation based on
///   `certificate_rotation_interval` in `MtlsConfig`
/// - [ ] Add support for renewing certificates before expiration
/// - [ ] Add background task to monitor certificate expiration
/// - [ ] Implement graceful rotation (generate new cert, update config, restart connections)
/// - [ ] Add metrics for certificate rotation events
async fn generate_mtls_certificates(mtls: &MtlsConfig) -> Result<(), SecurityValidationError> {
    // Resolve cert_dir: Priority: 1. Env var PLEXSPACES_MTLS_CERT_DIR, 2. Config cert_dir, 3. Default
    let cert_dir = get_env(ENV_MTLS_CERT_DIR).unwrap_or_else(|| {
        if !mtls.cert_dir.is_empty() {
            mtls.cert_dir.clone()
        } else {
            "/app/certs".to_string()
        }
    });

    // Create cert directory if it doesn't exist
    fs::create_dir_all(&cert_dir)
        .map_err(|e| SecurityValidationError::CertificateDirError(cert_dir.clone(), e))?;

    // Check if certificates already exist
    let ca_cert_path = format!("{}/ca.crt", cert_dir);
    let ca_key_path = format!("{}/ca.key", cert_dir);
    let server_cert_path = format!("{}/server.crt", cert_dir);
    let server_key_path = format!("{}/server.key", cert_dir);

    if Path::new(&ca_cert_path).exists()
        && Path::new(&ca_key_path).exists()
        && Path::new(&server_cert_path).exists()
        && Path::new(&server_key_path).exists()
    {
        tracing::info!(
            cert_dir = %cert_dir,
            "mTLS certificates already exist, skipping auto-generation"
        );
        return Ok(());
    }

    tracing::info!(
        cert_dir = %cert_dir,
        "Auto-generating mTLS certificates (development mode)"
    );

    // Generate CA certificate
    let ca_cert = generate_ca_certificate()?;
    fs::write(&ca_cert_path, ca_cert.cert_pem.as_bytes())
        .map_err(|e| SecurityValidationError::CertificateReadError(ca_cert_path.clone(), e))?;
    fs::write(&ca_key_path, ca_cert.key_pem.as_bytes())
        .map_err(|e| SecurityValidationError::CertificateReadError(ca_key_path.clone(), e))?;

    // Generate server certificate signed by CA
    let server_cert = generate_server_certificate(&ca_cert)?;
    fs::write(&server_cert_path, server_cert.cert_pem.as_bytes())
        .map_err(|e| SecurityValidationError::CertificateReadError(server_cert_path.clone(), e))?;
    fs::write(&server_key_path, server_cert.key_pem.as_bytes())
        .map_err(|e| SecurityValidationError::CertificateReadError(server_key_path.clone(), e))?;

    tracing::info!(
        cert_dir = %cert_dir,
        ca_cert = %ca_cert_path,
        server_cert = %server_cert_path,
        "mTLS certificates generated successfully"
    );

    Ok(())
}

/// CA certificate and key (PEM format)
struct CertificatePair {
    cert_pem: String,
    key_pem: String,
    // Store the Certificate object for signing server certs
    // (rcgen 0.13 doesn't support parsing from PEM)
    cert: Option<rcgen::Certificate>,
}

/// Generate CA certificate and private key
///
/// ## Returns
/// CertificatePair with CA certificate and key in PEM format
fn generate_ca_certificate() -> Result<CertificatePair, SecurityValidationError> {
    use rcgen::{CertificateParams, DistinguishedName, DnType};

    // Create CA certificate parameters
    let mut params = CertificateParams::new(vec![]).map_err(|e| {
        SecurityValidationError::AutoGenerationFailed(format!(
            "Failed to create CA certificate parameters: {}",
            e
        ))
    })?;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "PlexSpaces CA");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "PlexSpaces");
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);

    // Set validity (1 year for CA)
    // rcgen uses time crate, not chrono
    use time::OffsetDateTime;
    let now = OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(365);

    // Generate CA key pair
    let key_pair = rcgen::KeyPair::generate().map_err(|e| {
        SecurityValidationError::AutoGenerationFailed(format!(
            "Failed to generate CA key pair: {}",
            e
        ))
    })?;

    // Generate self-signed CA certificate
    let cert = params.self_signed(&key_pair).map_err(|e| {
        SecurityValidationError::AutoGenerationFailed(format!(
            "Failed to generate CA certificate: {}",
            e
        ))
    })?;

    Ok(CertificatePair {
        cert_pem: cert.pem(),
        key_pem: key_pair.serialize_pem(),
        cert: Some(cert),
    })
}

/// Generate server certificate signed by CA
///
/// ## Arguments
/// * `ca_cert` - CA certificate pair
///
/// ## Returns
/// CertificatePair with server certificate and key in PEM format
fn generate_server_certificate(
    ca_cert: &CertificatePair,
) -> Result<CertificatePair, SecurityValidationError> {
    use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};

    // Parse CA certificate and key from PEM
    // Note: rcgen 0.13 uses Certificate::from_pem, but we need to verify the exact API
    // For now, we'll regenerate the CA cert params from the PEM and extract the key
    let ca_key_pair = KeyPair::from_pem(&ca_cert.key_pem).map_err(|e| {
        SecurityValidationError::AutoGenerationFailed(format!("Failed to parse CA key: {}", e))
    })?;

    // Create server certificate parameters
    let mut params = CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])
        .map_err(|e| {
            SecurityValidationError::AutoGenerationFailed(format!(
                "Failed to create server certificate parameters: {}",
                e
            ))
        })?;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "PlexSpaces Server");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "PlexSpaces");

    // Set validity (90 days for server cert)
    // rcgen uses time crate, not chrono
    use time::OffsetDateTime;
    let now = OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + time::Duration::days(90);

    // Generate server key pair
    let server_key_pair = KeyPair::generate().map_err(|e| {
        SecurityValidationError::AutoGenerationFailed(format!(
            "Failed to generate server key pair: {}",
            e
        ))
    })?;

    // Get CA certificate object (rcgen 0.13 doesn't support parsing from PEM)
    let ca_cert_obj = ca_cert.cert.as_ref().ok_or_else(|| {
        SecurityValidationError::AutoGenerationFailed(
            "CA certificate object not available (cannot parse from PEM in rcgen 0.13)".to_string(),
        )
    })?;

    // Generate certificate signed by CA
    let server_cert = params
        .signed_by(&server_key_pair, ca_cert_obj, &ca_key_pair)
        .map_err(|e| {
            SecurityValidationError::AutoGenerationFailed(format!(
                "Failed to sign server certificate: {}",
                e
            ))
        })?;

    Ok(CertificatePair {
        cert_pem: server_cert.pem(),
        key_pem: server_key_pair.serialize_pem(),
        cert: Some(server_cert),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::security::v1::{JwtConfig, MtlsConfig};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_validate_jwt_config_missing_secret() {
        let jwt = JwtConfig {
            enable_jwt: true,
            secret: String::new(),
            jwks_url: String::new(),
            ..Default::default()
        };

        // Clear env var for test
        std::env::remove_var("PLEXSPACES_JWT_SECRET");

        let result = validate_jwt_config(&jwt);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SecurityValidationError::MissingJwtSecret
        ));
    }

    #[tokio::test]
    async fn test_validate_jwt_config_from_env() {
        let jwt = JwtConfig {
            enable_jwt: true,
            secret: String::new(),
            jwks_url: String::new(),
            ..Default::default()
        };

        std::env::set_var("PLEXSPACES_JWT_SECRET", "test-secret");
        let result = validate_jwt_config(&jwt);
        std::env::remove_var("PLEXSPACES_JWT_SECRET");

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_jwt_config_from_config() {
        let jwt = JwtConfig {
            enable_jwt: true,
            secret: "config-secret".to_string(),
            jwks_url: String::new(),
            ..Default::default()
        };

        std::env::remove_var("PLEXSPACES_JWT_SECRET");
        let result = validate_jwt_config(&jwt);

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_jwt_config_jwks() {
        let jwt = JwtConfig {
            enable_jwt: true,
            secret: String::new(),
            jwks_url: "https://auth.example.com/.well-known/jwks.json".to_string(),
            ..Default::default()
        };

        std::env::remove_var("PLEXSPACES_JWT_SECRET");
        let result = validate_jwt_config(&jwt);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_jwt_config_disabled() {
        let jwt = JwtConfig {
            enable_jwt: false,
            secret: String::new(),
            jwks_url: String::new(),
            ..Default::default()
        };

        std::env::remove_var("PLEXSPACES_JWT_SECRET");
        let result = validate_jwt_config(&jwt);
        assert!(result.is_ok()); // Should pass when disabled
    }

    #[tokio::test]
    async fn test_validate_mtls_config_missing_files() {
        let mtls = MtlsConfig {
            enable_mtls: true,
            auto_generate: false,
            ca_certificate_path: "/nonexistent/ca.crt".to_string(),
            server_certificate_path: "/nonexistent/server.crt".to_string(),
            server_key_path: "/nonexistent/server.key".to_string(),
            ..Default::default()
        };

        std::env::remove_var("PLEXSPACES_MTLS_CA_CERT");
        std::env::remove_var("PLEXSPACES_MTLS_SERVER_CERT");
        std::env::remove_var("PLEXSPACES_MTLS_SERVER_KEY");

        let result = validate_mtls_config(&mtls).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SecurityValidationError::MissingCaCertificate(_) => {}
            SecurityValidationError::MissingServerCertificate(_) => {}
            SecurityValidationError::MissingServerKey(_) => {}
            e => panic!("Unexpected error: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_validate_mtls_config_from_env() {
        let temp_dir = TempDir::new().unwrap();
        let ca_cert = temp_dir.path().join("ca.crt");
        let server_cert = temp_dir.path().join("server.crt");
        let server_key = temp_dir.path().join("server.key");

        // Create dummy certificate files
        std::fs::write(&ca_cert, "dummy ca cert").unwrap();
        std::fs::write(&server_cert, "dummy server cert").unwrap();
        std::fs::write(&server_key, "dummy server key").unwrap();

        std::env::set_var("PLEXSPACES_MTLS_CA_CERT", ca_cert.to_str().unwrap());
        std::env::set_var("PLEXSPACES_MTLS_SERVER_CERT", server_cert.to_str().unwrap());
        std::env::set_var("PLEXSPACES_MTLS_SERVER_KEY", server_key.to_str().unwrap());

        let mtls = MtlsConfig {
            enable_mtls: true,
            auto_generate: false,
            ca_certificate_path: String::new(),
            server_certificate_path: String::new(),
            server_key_path: String::new(),
            ..Default::default()
        };

        let result = validate_mtls_config(&mtls).await;

        std::env::remove_var("PLEXSPACES_MTLS_CA_CERT");
        std::env::remove_var("PLEXSPACES_MTLS_SERVER_CERT");
        std::env::remove_var("PLEXSPACES_MTLS_SERVER_KEY");

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_mtls_config_auto_generate() {
        let temp_dir = TempDir::new().unwrap();
        let cert_dir = temp_dir.path().to_str().unwrap();

        let mtls = MtlsConfig {
            enable_mtls: true,
            auto_generate: true,
            cert_dir: cert_dir.to_string(),
            ..Default::default()
        };

        let result = validate_mtls_config(&mtls).await;
        assert!(result.is_ok());

        // Verify certificates were generated
        let ca_cert_path = format!("{}/ca.crt", cert_dir);
        let server_cert_path = format!("{}/server.crt", cert_dir);
        let server_key_path = format!("{}/server.key", cert_dir);

        assert!(Path::new(&ca_cert_path).exists());
        assert!(Path::new(&server_cert_path).exists());
        assert!(Path::new(&server_key_path).exists());

        // Verify certificates are valid PEM
        let ca_cert_content = std::fs::read_to_string(&ca_cert_path).unwrap();
        assert!(ca_cert_content.contains("BEGIN CERTIFICATE"));
        assert!(ca_cert_content.contains("END CERTIFICATE"));

        let server_cert_content = std::fs::read_to_string(&server_cert_path).unwrap();
        assert!(server_cert_content.contains("BEGIN CERTIFICATE"));
        assert!(server_cert_content.contains("END CERTIFICATE"));

        let server_key_content = std::fs::read_to_string(&server_key_path).unwrap();
        assert!(
            server_key_content.contains("BEGIN PRIVATE KEY")
                || server_key_content.contains("BEGIN EC PRIVATE KEY")
        );
    }

    #[tokio::test]
    async fn test_validate_mtls_config_disabled() {
        let mtls = MtlsConfig {
            enable_mtls: false,
            auto_generate: false,
            ..Default::default()
        };

        let result = validate_mtls_config(&mtls).await;
        assert!(result.is_ok()); // Should pass when disabled
    }

    #[tokio::test]
    async fn test_validate_security_config_disabled_via_env() {
        std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");

        let config = SecurityConfig {
            jwt: Some(JwtConfig {
                enable_jwt: true,
                secret: String::new(),
                ..Default::default()
            }),
            mtls: Some(MtlsConfig {
                enable_mtls: true,
                auto_generate: false,
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = validate_security_config(&config).await;
        std::env::remove_var("PLEXSPACES_DISABLE_AUTH");

        assert!(result.is_ok()); // Should pass when disabled via env
    }

    #[tokio::test]
    async fn test_validate_security_config_disabled_in_config() {
        let config = SecurityConfig {
            disable_auth: true,
            jwt: Some(JwtConfig {
                enable_jwt: true,
                secret: String::new(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = validate_security_config(&config).await;
        assert!(result.is_ok()); // Should pass when disabled in config
    }

    #[tokio::test]
    async fn test_resolve_path_env_var() {
        std::env::set_var("TEST_ENV_VAR", "/test/path");
        let result = resolve_path("", "TEST_ENV_VAR").unwrap();
        std::env::remove_var("TEST_ENV_VAR");

        assert_eq!(result, "/test/path");
    }

    #[tokio::test]
    async fn test_resolve_path_config() {
        std::env::remove_var("TEST_ENV_VAR");
        let result = resolve_path("/config/path", "TEST_ENV_VAR").unwrap();

        assert_eq!(result, "/config/path");
    }

    #[tokio::test]
    async fn test_resolve_path_empty() {
        std::env::remove_var("TEST_ENV_VAR");
        let result = resolve_path("", "TEST_ENV_VAR").unwrap();

        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn test_generate_ca_certificate() {
        let result = generate_ca_certificate();
        assert!(result.is_ok());

        let cert_pair = result.unwrap();
        assert!(cert_pair.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(cert_pair.cert_pem.contains("END CERTIFICATE"));
        assert!(cert_pair.key_pem.contains("BEGIN") && cert_pair.key_pem.contains("PRIVATE KEY"));
    }

    #[tokio::test]
    async fn test_generate_server_certificate() {
        let ca_cert = generate_ca_certificate().unwrap();
        let result = generate_server_certificate(&ca_cert);

        assert!(result.is_ok());

        let server_cert = result.unwrap();
        assert!(server_cert.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(server_cert.cert_pem.contains("END CERTIFICATE"));
        assert!(
            server_cert.key_pem.contains("BEGIN") && server_cert.key_pem.contains("PRIVATE KEY")
        );
    }
}
