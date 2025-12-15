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

//! Blob storage service implementation

use bytes::Bytes;
use chrono::{DateTime, Duration, Utc};
use object_store::{
    aws::AmazonS3Builder,
    azure::MicrosoftAzureBuilder,
    gcp::GoogleCloudStorageBuilder,
    local::LocalFileSystem,
    path::Path as ObjectPath,
    ObjectStore,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use ulid::Ulid;

use crate::{
    BlobConfigExt, BlobError, BlobResult, BlobRepository,
    repository::ListFilters,
    helpers::{datetime_to_timestamp, get_storage_path, timestamp_to_datetime},
};
use plexspaces_proto::storage::v1::{BlobConfig, BlobMetadata};
use prost_types::Timestamp;

/// Blob storage service
pub struct BlobService {
    config: BlobConfig,
    object_store: Arc<dyn ObjectStore>,
    repository: Arc<dyn BlobRepository>,
}

impl BlobService {
    /// Create new blob service with custom object store (for testing)
    pub fn with_object_store(
        config: BlobConfig,
        object_store: Arc<dyn ObjectStore>,
        repository: Arc<dyn BlobRepository>,
    ) -> Self {
        let prefix = if config.prefix.is_empty() {
            "/plexspaces".to_string()
        } else {
            config.prefix.clone()
        };
        let mut config_with_prefix = config;
        config_with_prefix.prefix = prefix;
        
        Self {
            config: config_with_prefix,
            object_store,
            repository,
        }
    }

    /// Create new blob service
    pub async fn new(
        config: BlobConfig,
        repository: Arc<dyn BlobRepository>,
    ) -> BlobResult<Self> {
        config.validate()?;
        
        // Set default prefix if empty
        let prefix = if config.prefix.is_empty() {
            "/plexspaces".to_string()
        } else {
            config.prefix.clone()
        };

        let object_store: Arc<dyn ObjectStore> = match config.backend.as_str() {
            "s3" => {
                let mut builder = AmazonS3Builder::new()
                    .with_bucket_name(&config.bucket);

                if !config.region.is_empty() {
                    builder = builder.with_region(&config.region);
                }

                if let Some(access_key_id) = config.get_access_key_id() {
                    builder = builder.with_access_key_id(&access_key_id);
                }

                if let Some(secret_access_key) = config.get_secret_access_key() {
                    builder = builder.with_secret_access_key(&secret_access_key);
                }

                Arc::new(builder.build().map_err(|e| BlobError::ConfigError(format!("Failed to build S3 store: {}", e)))?)
            }
            "minio" => {
                if config.endpoint.is_empty() {
                    return Err(BlobError::ConfigError("endpoint required for MinIO".to_string()));
                }

                let mut builder = AmazonS3Builder::new()
                    .with_bucket_name(&config.bucket)
                    .with_endpoint(&config.endpoint)
                    .with_allow_http(!config.use_ssl);

                if let Some(access_key_id) = config.get_access_key_id() {
                    builder = builder.with_access_key_id(&access_key_id);
                }

                if let Some(secret_access_key) = config.get_secret_access_key() {
                    builder = builder.with_secret_access_key(&secret_access_key);
                }

                Arc::new(builder.build().map_err(|e| BlobError::ConfigError(format!("Failed to build MinIO store: {}", e)))?)
            }
            "gcp" => {
                let mut builder = GoogleCloudStorageBuilder::new()
                    .with_bucket_name(&config.bucket);

                if !config.gcp_service_account_json.is_empty() {
                    builder = builder.with_service_account_path(&config.gcp_service_account_json);
                }

                Arc::new(builder.build().map_err(|e| BlobError::ConfigError(format!("Failed to build GCP store: {}", e)))?)
            }
            "azure" => {
                if config.azure_account_name.is_empty() {
                    return Err(BlobError::ConfigError("azure_account_name required".to_string()));
                }

                let mut builder = MicrosoftAzureBuilder::new()
                    .with_account(&config.azure_account_name)
                    .with_container_name(&config.bucket);

                if !config.azure_account_key.is_empty() {
                    builder = builder.with_access_key(&config.azure_account_key);
                }

                Arc::new(builder.build().map_err(|e| BlobError::ConfigError(format!("Failed to build Azure store: {}", e)))?)
            }
            "local" => {
                // Local filesystem for testing
                Arc::new(
                    LocalFileSystem::new_with_prefix("/")
                        .map_err(|e| BlobError::ConfigError(format!("Failed to create local filesystem store: {}", e)))?
                )
            }
            _ => return Err(BlobError::ConfigError(format!("Unsupported backend: {}", config.backend))),
        };

        let mut config_with_prefix = config;
        config_with_prefix.prefix = prefix;
        
        Ok(Self {
            config: config_with_prefix,
            object_store,
            repository,
        })
    }

    /// Upload a blob
    pub async fn upload_blob(
        &self,
        tenant_id: &str,
        namespace: &str,
        name: &str,
        data: Vec<u8>,
        content_type: Option<String>,
        blob_group: Option<String>,
        kind: Option<String>,
        metadata: std::collections::HashMap<String, String>,
        tags: std::collections::HashMap<String, String>,
        expires_after: Option<Duration>,
    ) -> BlobResult<BlobMetadata> {
        if data.is_empty() {
            return Err(BlobError::InvalidInput("data cannot be empty".to_string()));
        }

        // Calculate SHA256
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let sha256 = hex::encode(hasher.finalize());

        // Check if blob with same SHA256 already exists
        if let Some(existing) = self.repository.get_by_sha256(tenant_id, namespace, &sha256).await? {
            // Return existing metadata (deduplication)
            return Ok(existing);
        }

        // Generate blob ID (ULID)
        let blob_id = Ulid::new().to_string();

        // Create metadata using proto type
        let now = Utc::now();
        let expires_at = expires_after.map(|d| datetime_to_timestamp(now + d));
        
        let mut blob_metadata = BlobMetadata {
            blob_id: blob_id.clone(),
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
            name: name.to_string(),
            sha256: sha256.clone(),
            content_type: content_type.unwrap_or_default(),
            content_length: data.len() as i64,
            etag: String::new(),
            blob_group: blob_group.unwrap_or_default(),
            kind: kind.unwrap_or_default(),
            metadata: metadata,
            tags: tags,
            expires_at: expires_at,
            created_at: Some(datetime_to_timestamp(now)),
            updated_at: Some(datetime_to_timestamp(now)),
        };

        // Get storage path
        let storage_path = get_storage_path(&blob_metadata, &self.config.prefix);
        let path = ObjectPath::from(storage_path);

        // Upload to object store
        let bytes = Bytes::from(data);
        self.object_store.put(&path, bytes.into()).await
            .map_err(|e| BlobError::StorageError(format!("Failed to upload blob: {}", e)))?;

        // Get ETag from object store (if available)
        // Note: object_store doesn't directly return ETag, so we'll skip it for now
        // blob_metadata.etag = Some(etag);

        // Save metadata to repository
        self.repository.save(&blob_metadata).await?;

        Ok(blob_metadata)
    }

    /// Download a blob
    pub async fn download_blob(&self, blob_id: &str) -> BlobResult<Vec<u8>> {
        // Get metadata
        let metadata = self.repository.get(blob_id).await?
            .ok_or_else(|| BlobError::NotFound(blob_id.to_string()))?;

        // Get storage path
        let storage_path = get_storage_path(&metadata, &self.config.prefix);
        let path = ObjectPath::from(storage_path);

        // Download from object store
        let result = self.object_store.get(&path).await
            .map_err(|e| BlobError::StorageError(format!("Failed to download blob: {}", e)))?;
        let bytes = result.bytes().await
            .map_err(|e| BlobError::StorageError(format!("Failed to read blob bytes: {}", e)))?;

        Ok(bytes.to_vec())
    }

    /// Get blob metadata
    pub async fn get_metadata(&self, blob_id: &str) -> BlobResult<BlobMetadata> {
        self.repository.get(blob_id).await?
            .ok_or_else(|| BlobError::NotFound(blob_id.to_string()))
    }

    /// List blobs
    pub async fn list_blobs(
        &self,
        tenant_id: &str,
        namespace: &str,
        filters: &ListFilters,
        page_size: i64,
        page: i64,
    ) -> BlobResult<(Vec<BlobMetadata>, i64)> {
        let offset = (page - 1) * page_size;
        self.repository.list(tenant_id, namespace, filters, page_size, offset).await
    }

    /// Delete a blob
    pub async fn delete_blob(&self, blob_id: &str) -> BlobResult<()> {
        // Get metadata
        let metadata = self.repository.get(blob_id).await?
            .ok_or_else(|| BlobError::NotFound(blob_id.to_string()))?;

        // Delete from object store
        let storage_path = get_storage_path(&metadata, &self.config.prefix);
        let path = ObjectPath::from(storage_path);
        self.object_store.delete(&path).await
            .map_err(|e| BlobError::StorageError(format!("Failed to delete blob: {}", e)))?;

        // Delete metadata
        self.repository.delete(blob_id).await?;

        Ok(())
    }

    /// Generate presigned URL (for direct client access)
    /// Requires 'presigned-urls' feature to be enabled
    pub async fn generate_presigned_url(
        &self,
        blob_id: &str,
        operation: &str,
        expires_after: Duration,
    ) -> BlobResult<String> {
        // Get metadata
        let metadata = self.repository.get(blob_id).await?
            .ok_or_else(|| BlobError::NotFound(blob_id.to_string()))?;

        // Get storage path
        let storage_path = crate::helpers::get_storage_path(&metadata, &self.config.prefix);
        
        // Generate presigned URL
        crate::presigned::generate_presigned_url(
            &self.config,
            &storage_path,
            operation,
            expires_after,
        ).await
    }

    /// Find expired blobs
    pub async fn find_expired(
        &self,
        tenant_id: Option<&str>,
        limit: i64,
    ) -> BlobResult<Vec<BlobMetadata>> {
        self.repository.find_expired(tenant_id, limit).await
    }
}

// Implement Service trait for ServiceLocator registration
impl plexspaces_core::Service for BlobService {}
