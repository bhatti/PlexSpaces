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

//! Presigned URL generation for blob storage

use crate::{BlobConfig, BlobConfigExt, BlobError, BlobResult};
use chrono::Duration;
use std::time::Duration as StdDuration;

#[cfg(feature = "presigned-urls")]
pub async fn generate_presigned_url(
    config: &BlobConfig,
    blob_path: &str,
    operation: &str,
    expires_after: Duration,
) -> BlobResult<String> {
    use aws_sdk_s3::{
        config::{Builder, Credentials, Region},
        presigning::PresigningConfig,
        Client,
    };

    // Validate operation
    let operation = operation.to_uppercase();
    if operation != "GET" && operation != "PUT" {
        return Err(BlobError::InvalidInput(format!(
            "Invalid operation: {}. Must be 'GET' or 'PUT'",
            operation
        )));
    }

    // Get credentials
    let access_key_id = config
        .get_access_key_id()
        .ok_or_else(|| BlobError::ConfigError("Access key ID is required".to_string()))?;
    let secret_access_key = config
        .get_secret_access_key()
        .ok_or_else(|| BlobError::ConfigError("Secret access key is required".to_string()))?;

    // Create credentials
    let credentials = Credentials::new(
        &access_key_id,
        &secret_access_key,
        None,
        None,
        "static",
    );

    // Build S3 config
    let mut builder = Builder::new()
        .credentials_provider(credentials)
        .force_path_style(true); // Required for MinIO

    // Set region (required by SDK, but MinIO doesn't use it)
    let region = if !config.region.is_empty() {
        Region::new(config.region.clone())
    } else {
        Region::new("us-east-1") // Default for MinIO
    };
    builder = builder.region(region);

    // Set endpoint for MinIO or custom S3-compatible storage
    if config.backend == "minio" || !config.endpoint.is_empty() {
        let endpoint = if !config.endpoint.is_empty() {
            &config.endpoint
        } else {
            return Err(BlobError::ConfigError(
                "Endpoint is required for MinIO backend".to_string(),
            ));
        };
        builder = builder.endpoint_url(endpoint);
    }

    // Build config and create client
    let s3_config = builder.build();
    let client = Client::from_conf(s3_config);

    // Convert chrono::Duration to std::time::Duration
    let expires_secs = expires_after.num_seconds();
    if expires_secs <= 0 || expires_secs > 604800 {
        // Max 7 days (AWS S3 limit)
        return Err(BlobError::InvalidInput(
            "Expiration must be between 1 second and 7 days".to_string(),
        ));
    }
    let expires_in = StdDuration::from_secs(expires_secs as u64);

    // Create presigning config
    let presigning_config = PresigningConfig::builder()
        .expires_in(expires_in)
        .build()
        .map_err(|e| BlobError::InternalError(format!("Failed to create presigning config: {}", e)))?;

    // Generate presigned URL based on operation
    let presigned_url = match operation.as_str() {
        "GET" => {
            client
                .get_object()
                .bucket(&config.bucket)
                .key(blob_path)
                .presigned(presigning_config)
                .await
                .map_err(|e| BlobError::InternalError(format!("Failed to generate presigned URL: {}", e)))?
                .uri()
                .to_string()
        }
        "PUT" => {
            client
                .put_object()
                .bucket(&config.bucket)
                .key(blob_path)
                .presigned(presigning_config)
                .await
                .map_err(|e| BlobError::InternalError(format!("Failed to generate presigned URL: {}", e)))?
                .uri()
                .to_string()
        }
        _ => unreachable!(), // Already validated above
    };

    Ok(presigned_url)
}

#[cfg(not(feature = "presigned-urls"))]
pub async fn generate_presigned_url(
    _config: &BlobConfig,
    _blob_path: &str,
    _operation: &str,
    _expires_after: Duration,
) -> BlobResult<String> {
    Err(BlobError::InternalError(
        "Presigned URLs require the 'presigned-urls' feature to be enabled".to_string()
    ))
}
