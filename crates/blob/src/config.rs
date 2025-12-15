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

//! Blob storage configuration

use serde::{Deserialize, Serialize};
use std::env;

/// Blob storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobConfig {
    /// Backend type (s3, minio, gcp, azure)
    pub backend: String,

    /// Bucket name
    pub bucket: String,

    /// Endpoint URL (for MinIO or custom S3-compatible)
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

impl Default for BlobConfig {
    fn default() -> Self {
        Self {
            backend: "minio".to_string(),
            bucket: "plexspaces".to_string(),
            endpoint: Some("http://localhost:9000".to_string()),
            region: None,
            access_key_id: None,
            secret_access_key: None,
            use_ssl: false,
            prefix: "/plexspaces".to_string(),
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
                .unwrap_or_else(|_| "minio".to_string()),
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
                .unwrap_or_else(|_| "/plexspaces".to_string()),
            gcp_service_account_json: env::var("GCP_SERVICE_ACCOUNT_JSON").ok(),
            azure_account_name: env::var("AZURE_ACCOUNT_NAME").ok(),
            azure_account_key: env::var("AZURE_ACCOUNT_KEY").ok(),
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        match self.backend.as_str() {
            "s3" | "minio" | "gcp" | "azure" => {}
            _ => return Err(format!("Invalid backend: {}", self.backend)),
        }

        if self.bucket.is_empty() {
            return Err("bucket is required".to_string());
        }

        if self.backend == "minio" && self.endpoint.is_none() {
            return Err("endpoint is required for MinIO backend".to_string());
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
