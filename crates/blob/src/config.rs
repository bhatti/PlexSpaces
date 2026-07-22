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
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Blob storage configuration

use serde::{Deserialize, Serialize};
use std::env;

/// Blob storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobConfig {
    /// Backend type (s3, embedded, gcp, azure)
    pub backend: String,

    /// Bucket name
    pub bucket: String,

    /// Endpoint URL for embedded or custom S3-compatible store (e.g., "http://127.0.0.1:9100")
    pub endpoint: Option<String>,

    /// Region (for S3/GCP/Azure)
    pub region: Option<String>,

    /// Access key ID (can be from env var)
    pub access_key_id: Option<String>,

    /// Secret access key (should be from env var, not config file)
    pub secret_access_key: Option<String>,

    /// Use SSL/TLS
    pub use_ssl: bool,

    /// Path prefix for all blobs (default: /plexspaces)
    pub prefix: String,

    /// GCP-specific: Service account JSON (base64 encoded)
    pub gcp_service_account_json: Option<String>,

    /// Azure-specific: Account name
    pub azure_account_name: Option<String>,

    /// Azure-specific: Account key
    pub azure_account_key: Option<String>,
}

/// Resolve the default blob prefix: `$HOME/plexspaces/blob`.
/// Falls back to `/tmp/plexspaces/blob` if `$HOME` is unset.
fn default_blob_prefix() -> String {
    match env::var("HOME") {
        Ok(home) => format!("{}/plexspaces/blob", home),
        Err(_) => "/tmp/plexspaces/blob".to_string(),
    }
}

impl Default for BlobConfig {
    fn default() -> Self {
        Self {
            backend: "embedded".to_string(),
            bucket: "plexspaces".to_string(),
            endpoint: None,
            region: None,
            access_key_id: None,
            secret_access_key: None,
            use_ssl: false,
            prefix: default_blob_prefix(),
            gcp_service_account_json: None,
            azure_account_name: None,
            azure_account_key: None,
        }
    }
}

impl BlobConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        Self {
            backend: env::var("BLOB_BACKEND")
                .unwrap_or_else(|_| "embedded".to_string()),
            bucket: env::var("BLOB_BUCKET")
                .unwrap_or_else(|_| "plexspaces".to_string()),
            endpoint: env::var("BLOB_ENDPOINT").ok(),
            region: env::var("BLOB_REGION").ok(),
            access_key_id: env::var("BLOB_ACCESS_KEY_ID")
                .or_else(|_| env::var("AWS_ACCESS_KEY_ID"))
                .ok(),
            secret_access_key: env::var("BLOB_SECRET_ACCESS_KEY")
                .or_else(|_| env::var("AWS_SECRET_ACCESS_KEY"))
                .ok(),
            use_ssl: env::var("BLOB_USE_SSL")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            prefix: env::var("BLOB_PREFIX")
                .unwrap_or_else(|_| default_blob_prefix()),
            gcp_service_account_json: env::var("GCP_SERVICE_ACCOUNT_JSON").ok(),
            azure_account_name: env::var("AZURE_ACCOUNT_NAME").ok(),
            azure_account_key: env::var("AZURE_ACCOUNT_KEY").ok(),
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        match self.backend.as_str() {
            "s3" | "embedded" | "local" | "gcp" | "azure" => {}
            _ => return Err(format!("Invalid backend: {}", self.backend)),
        }

        if self.bucket.is_empty() && self.backend != "embedded" {
            return Err("bucket is required".to_string());
        }

        if self.backend == "s3" && self.region.is_none() {
            return Err("region is required for S3 backend".to_string());
        }

        Ok(())
    }

    /// Get access key ID (from config or env)
    pub fn get_access_key_id(&self) -> Option<String> {
        self.access_key_id
            .clone()
            .or_else(|| env::var("BLOB_ACCESS_KEY_ID").ok())
            .or_else(|| env::var("AWS_ACCESS_KEY_ID").ok())
    }

    /// Get secret access key (from config or env)
    pub fn get_secret_access_key(&self) -> Option<String> {
        self.secret_access_key
            .clone()
            .or_else(|| env::var("BLOB_SECRET_ACCESS_KEY").ok())
            .or_else(|| env::var("AWS_SECRET_ACCESS_KEY").ok())
    }
}
