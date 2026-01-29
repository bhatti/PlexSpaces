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

//! Integration tests for security validation
//!
//! ## Purpose
//! Tests security configuration validation in realistic scenarios:
//! - JWT secret validation
//! - mTLS certificate validation
//! - Auto-generation of certificates
//! - Environment variable support
//! - Error handling when keys are missing

use plexspaces_proto::node::v1::SecurityConfig;
use plexspaces_proto::security::v1::{JwtConfig, MtlsConfig};
use plexspaces_services::service_locator::ServiceLocatorImpl;
use tempfile::TempDir;

#[tokio::test]
async fn test_security_config_validation_jwt_missing_secret() {
    // Test that JWT validation fails when secret is missing
    let config = SecurityConfig {
        jwt: Some(JwtConfig {
            enable_jwt: true,
            secret: String::new(),
            jwks_url: String::new(),
            ..Default::default()
        }),
        disable_auth: false,
        ..Default::default()
    };
    
    std::env::remove_var("PLEXSPACES_JWT_SECRET");
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    
    let service_locator = ServiceLocatorImpl::new();
    
    // Should panic with clear error message
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            service_locator.register_security_config(config).await;
        });
    }));
    
    assert!(result.is_err(), "Should panic when JWT secret is missing");
}

#[tokio::test]
async fn test_security_config_validation_jwt_from_env() {
    // Test that JWT validation passes when secret is in env var
    let config = SecurityConfig {
        jwt: Some(JwtConfig {
            enable_jwt: true,
            secret: String::new(),
            jwks_url: String::new(),
            ..Default::default()
        }),
        disable_auth: false,
        ..Default::default()
    };
    
    std::env::set_var("PLEXSPACES_JWT_SECRET", "test-secret-12345");
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    
    let service_locator = ServiceLocatorImpl::new();
    
    // Should not panic
    service_locator.register_security_config(config).await;
    
    std::env::remove_var("PLEXSPACES_JWT_SECRET");
}

#[tokio::test]
async fn test_security_config_validation_mtls_missing_certs() {
    // Test that mTLS validation fails when certificates are missing
    let config = SecurityConfig {
        mtls: Some(MtlsConfig {
            enable_mtls: true,
            auto_generate: false,
            ca_certificate_path: "/nonexistent/ca.crt".to_string(),
            server_certificate_path: "/nonexistent/server.crt".to_string(),
            server_key_path: "/nonexistent/server.key".to_string(),
            ..Default::default()
        }),
        disable_auth: false,
        ..Default::default()
    };
    
    std::env::remove_var("PLEXSPACES_MTLS_CA_CERT");
    std::env::remove_var("PLEXSPACES_MTLS_SERVER_CERT");
    std::env::remove_var("PLEXSPACES_MTLS_SERVER_KEY");
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    
    let service_locator = ServiceLocatorImpl::new();
    
    // Should panic with clear error message
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            service_locator.register_security_config(config).await;
        });
    }));
    
    assert!(result.is_err(), "Should panic when mTLS certificates are missing");
}

#[tokio::test]
async fn test_security_config_validation_mtls_auto_generate() {
    // Test that mTLS auto-generation works
    let temp_dir = TempDir::new().unwrap();
    let cert_dir = temp_dir.path().to_str().unwrap();
    
    let config = SecurityConfig {
        mtls: Some(MtlsConfig {
            enable_mtls: true,
            auto_generate: true,
            cert_dir: cert_dir.to_string(),
            ..Default::default()
        }),
        disable_auth: false,
        ..Default::default()
    };
    
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    
    let service_locator = ServiceLocatorImpl::new();
    
    // Should not panic - certificates will be auto-generated
    service_locator.register_security_config(config).await;
    
    // Verify certificates were generated
    let ca_cert_path = format!("{}/ca.crt", cert_dir);
    let server_cert_path = format!("{}/server.crt", cert_dir);
    let server_key_path = format!("{}/server.key", cert_dir);
    
    assert!(std::path::Path::new(&ca_cert_path).exists());
    assert!(std::path::Path::new(&server_cert_path).exists());
    assert!(std::path::Path::new(&server_key_path).exists());
    
    // Verify certificates are valid PEM
    let ca_cert_content = std::fs::read_to_string(&ca_cert_path).unwrap();
    assert!(ca_cert_content.contains("BEGIN CERTIFICATE"));
    
    let server_cert_content = std::fs::read_to_string(&server_cert_path).unwrap();
    assert!(server_cert_content.contains("BEGIN CERTIFICATE"));
}

#[tokio::test]
async fn test_security_config_validation_disabled_via_env() {
    // Test that validation is skipped when PLEXSPACES_DISABLE_AUTH is set
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
        disable_auth: false,
        ..Default::default()
    };
    
    std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");
    std::env::remove_var("PLEXSPACES_JWT_SECRET");
    
    let service_locator = ServiceLocatorImpl::new();
    
    // Should not panic - validation is disabled
    service_locator.register_security_config(config).await;
    
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
}

#[tokio::test]
async fn test_security_config_validation_jwt_jwks() {
    // Test that JWT validation passes when using JWKS (no secret needed)
    let config = SecurityConfig {
        jwt: Some(JwtConfig {
            enable_jwt: true,
            secret: String::new(),
            jwks_url: "https://auth.example.com/.well-known/jwks.json".to_string(),
            ..Default::default()
        }),
        disable_auth: false,
        ..Default::default()
    };
    
    std::env::remove_var("PLEXSPACES_JWT_SECRET");
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");
    
    let service_locator = ServiceLocatorImpl::new();
    
    // Should not panic - JWKS doesn't require secret
    service_locator.register_security_config(config).await;
}
