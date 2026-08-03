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

//! Integration tests for security configuration validation

use plexspaces_proto::node::v1::SecurityConfig;
use plexspaces_proto::security::v1::{JwtConfig, MtlsConfig};
use plexspaces_services::service_locator::ServiceLocatorImpl;
use tempfile::TempDir;

#[tokio::test]
async fn test_security_config_validation_jwt_missing_secret() {
    std::env::remove_var("PLEXSPACES_JWT_SECRET");
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");

    let config = SecurityConfig {
        jwt: Some(JwtConfig {
            enable_jwt: true,
            secret: String::new(),
            jwks_url: String::new(),
            ..Default::default()
        }),
        disable_auth: false,
        oidc: None,
        ..Default::default()
    };

    let service_locator = ServiceLocatorImpl::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            service_locator.register_security_config(config).await;
        });
    }));
    assert!(result.is_err(), "Should panic when JWT secret is missing");
}

#[tokio::test]
async fn test_security_config_validation_mtls_missing_certs() {
    std::env::remove_var("PLEXSPACES_MTLS_CA_CERT");
    std::env::remove_var("PLEXSPACES_MTLS_SERVER_CERT");
    std::env::remove_var("PLEXSPACES_MTLS_SERVER_KEY");
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");

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
        oidc: None,
        ..Default::default()
    };

    let service_locator = ServiceLocatorImpl::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            service_locator.register_security_config(config).await;
        });
    }));
    assert!(
        result.is_err(),
        "Should panic when mTLS certificates are missing"
    );
}

#[tokio::test]
async fn test_security_config_validation_mtls_auto_generate() {
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");

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
        oidc: None,
        ..Default::default()
    };

    let service_locator = ServiceLocatorImpl::new();
    service_locator.register_security_config(config).await;

    assert!(std::path::Path::new(&format!("{}/ca.crt", cert_dir)).exists());
    assert!(std::path::Path::new(&format!("{}/server.crt", cert_dir)).exists());
    assert!(std::path::Path::new(&format!("{}/server.key", cert_dir)).exists());

    let ca_cert = std::fs::read_to_string(format!("{}/ca.crt", cert_dir)).unwrap();
    assert!(ca_cert.contains("BEGIN CERTIFICATE"));
}

#[tokio::test]
async fn test_security_config_validation_jwt_jwks() {
    std::env::remove_var("PLEXSPACES_JWT_SECRET");
    std::env::remove_var("PLEXSPACES_DISABLE_AUTH");

    let config = SecurityConfig {
        jwt: Some(JwtConfig {
            enable_jwt: true,
            secret: String::new(),
            jwks_url: "https://auth.example.com/.well-known/jwks.json".to_string(),
            ..Default::default()
        }),
        disable_auth: false,
        oidc: None,
        ..Default::default()
    };

    let service_locator = ServiceLocatorImpl::new();
    service_locator.register_security_config(config).await;
}
