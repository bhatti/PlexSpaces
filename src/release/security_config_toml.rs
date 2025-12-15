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

//! TOML structures for SecurityConfig (proto-first design)

use serde::Deserialize;
use std::collections::HashMap;

/// Security configuration in TOML format
#[derive(Debug, Deserialize, Default)]
pub struct SecurityConfigToml {
    /// Service identity for this node (mTLS)
    #[serde(default)]
    pub service_identity: Option<ServiceIdentityToml>,

    /// mTLS configuration
    #[serde(default)]
    pub mtls: Option<MtlsConfigToml>,

    /// JWT configuration for public APIs
    #[serde(default)]
    pub jwt: Option<JwtConfigToml>,

    /// API keys for service authentication
    #[serde(default)]
    pub api_keys: Vec<ApiKeyToml>,
}

/// Service identity in TOML format
#[derive(Debug, Deserialize)]
pub struct ServiceIdentityToml {
    /// Service ID (e.g., "actor-service")
    pub service_id: String,

    /// Path to certificate file (PEM) or PEM content
    pub certificate: String,

    /// Path to private key file (PEM) or PEM content
    pub private_key: String,

    /// Certificate expiration timestamp (ISO 8601 or Unix timestamp)
    #[serde(default)]
    pub expires_at: Option<String>,

    /// Allowed services this identity can communicate with
    #[serde(default)]
    pub allowed_services: Vec<String>,
}

/// mTLS configuration in TOML format
#[derive(Debug, Deserialize)]
pub struct MtlsConfigToml {
    /// Enable mTLS
    #[serde(default = "default_true")]
    pub enable_mtls: bool,

    /// CA certificate path or PEM content
    pub ca_certificate: String,

    /// Trusted services (service IDs)
    #[serde(default)]
    pub trusted_services: Vec<String>,

    /// Certificate rotation interval (seconds)
    #[serde(default)]
    pub certificate_rotation_interval_seconds: Option<u64>,
}

fn default_true() -> bool {
    true
}

/// JWT configuration in TOML format
#[derive(Debug, Deserialize)]
pub struct JwtConfigToml {
    /// Enable JWT authentication
    #[serde(default = "default_true")]
    pub enable_jwt: bool,

    /// JWT issuer (e.g., "https://auth.example.com")
    pub issuer: String,

    /// JWKS URL for public key discovery
    pub jwks_url: String,

    /// Allowed audiences
    #[serde(default)]
    pub allowed_audiences: Vec<String>,

    /// Token TTL (seconds)
    #[serde(default)]
    pub token_ttl_seconds: Option<u64>,
}

/// API key in TOML format
#[derive(Debug, Deserialize)]
pub struct ApiKeyToml {
    /// API key ID
    pub id: String,

    /// API key name
    pub name: String,

    /// Optional description
    #[serde(default)]
    pub description: Option<String>,

    /// Key hash (e.g., "sha256:...")
    pub key_hash: String,

    /// Scopes (e.g., ["read", "write"])
    #[serde(default)]
    pub scopes: Vec<String>,

    /// Expiration timestamp (ISO 8601 or Unix timestamp)
    #[serde(default)]
    pub expires_at: Option<String>,

    /// Optional metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

