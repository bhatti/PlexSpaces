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

//! Extension traits for proto-generated config types

use std::env;
use crate::BlobError;
use plexspaces_proto::storage::v1::BlobConfig;

/// Extension trait for BlobConfig to add helper methods
pub trait BlobConfigExt {
    /// Validate configuration
    fn validate(&self) -> Result<(), BlobError>;

    /// Get access key ID (from config or env)
    fn get_access_key_id(&self) -> Option<String>;

    /// Get secret access key (from config or env)
    fn get_secret_access_key(&self) -> Option<String>;
}

impl BlobConfigExt for BlobConfig {
    fn validate(&self) -> Result<(), BlobError> {
        match self.backend.as_str() {
            "s3" | "minio" | "gcp" | "azure" | "local" => {}
            _ => return Err(BlobError::ConfigError(format!("Invalid backend: {}", self.backend))),
        }

        if self.bucket.is_empty() && self.backend != "local" {
            return Err(BlobError::ConfigError("bucket is required".to_string()));
        }

        if self.backend == "minio" && self.endpoint.is_empty() {
            return Err(BlobError::ConfigError("endpoint is required for MinIO backend".to_string()));
        }

        if self.backend == "s3" && self.region.is_empty() {
            return Err(BlobError::ConfigError("region is required for S3 backend".to_string()));
        }

        Ok(())
    }

    fn get_access_key_id(&self) -> Option<String> {
        if !self.access_key_id.is_empty() {
            Some(self.access_key_id.clone())
        } else {
            env::var("BLOB_ACCESS_KEY_ID")
                .or_else(|_| env::var("AWS_ACCESS_KEY_ID"))
                .ok()
        }
    }

    fn get_secret_access_key(&self) -> Option<String> {
        if !self.secret_access_key.is_empty() {
            Some(self.secret_access_key.clone())
        } else {
            env::var("BLOB_SECRET_ACCESS_KEY")
                .or_else(|_| env::var("AWS_SECRET_ACCESS_KEY"))
                .ok()
        }
    }
}
