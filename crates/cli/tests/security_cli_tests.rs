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

//! CLI tests for security commands

use tempfile::TempDir;

#[path = "../src/security.rs"]
mod security;

#[tokio::test]
async fn test_generate_mtls_certificates() {
    let temp_dir = TempDir::new().unwrap();
    let cert_dir = temp_dir.path();

    // Test certificate generation
    let result = security::generate_mtls_certificates(
        Some(cert_dir.to_path_buf()),
        Some("Test CA".to_string()),
        Some("Test Server".to_string()),
        Some(90),
    )
    .await;

    assert!(result.is_ok(), "Certificate generation should succeed");

    // Verify certificates were created
    let ca_cert = cert_dir.join("ca.crt");
    let ca_key = cert_dir.join("ca.key");
    let server_cert = cert_dir.join("server.crt");
    let server_key = cert_dir.join("server.key");

    assert!(ca_cert.exists(), "CA certificate should exist");
    assert!(ca_key.exists(), "CA key should exist");
    assert!(server_cert.exists(), "Server certificate should exist");
    assert!(server_key.exists(), "Server key should exist");

    // Verify certificates are valid PEM
    let ca_cert_content = std::fs::read_to_string(&ca_cert).unwrap();
    assert!(ca_cert_content.contains("BEGIN CERTIFICATE"));
    assert!(ca_cert_content.contains("END CERTIFICATE"));

    let server_cert_content = std::fs::read_to_string(&server_cert).unwrap();
    assert!(server_cert_content.contains("BEGIN CERTIFICATE"));
    assert!(server_cert_content.contains("END CERTIFICATE"));
}

#[tokio::test]
async fn test_generate_release_config() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test-release.yaml");

    // Test release config generation
    let result = security::generate_release_config(
        Some(output_path.clone()),
        Some("test-cluster".to_string()),
        Some("1.0.0".to_string()),
        Some("test-node".to_string()),
        Some("0.0.0.0:8000".to_string()),
    )
    .await;

    assert!(result.is_ok(), "Release config generation should succeed");

    // Verify file was created
    assert!(output_path.exists(), "Release config file should exist");

    // Verify content
    let content = std::fs::read_to_string(&output_path).unwrap();
    assert!(content.contains("name: test-cluster"));
    assert!(content.contains("version: 1.0.0"));
    assert!(content.contains("id: test-node"));
    assert!(content.contains("listen_addr: 0.0.0.0:8000"));
    assert!(content.contains("security:"));
    assert!(content.contains("mtls:"));
    assert!(content.contains("jwt:"));
}
