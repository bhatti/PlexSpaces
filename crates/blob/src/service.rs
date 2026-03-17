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
use chrono::{Duration, Utc};
use object_store::{
    aws::AmazonS3Builder, azure::MicrosoftAzureBuilder, gcp::GoogleCloudStorageBuilder,
    local::LocalFileSystem, path::Path as ObjectPath, ObjectStore,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use ulid::Ulid;

use crate::{
    helpers::{datetime_to_timestamp, get_storage_path},
    repository::ListFilters,
    BlobConfigExt, BlobError, BlobRepository, BlobResult,
};
use plexspaces_core::RequestContext;
use plexspaces_proto::storage::v1::{BlobConfig, BlobMetadata};

/// Ensures the S3/MinIO bucket exists, creating it if necessary.
/// This is called during BlobService initialization for s3/minio backends.
#[cfg(feature = "s3-backend")]
async fn ensure_bucket_exists(config: &BlobConfig) -> BlobResult<()> {
    use aws_sdk_s3::config::{Credentials, Region};
    use aws_sdk_s3::Client;

    let access_key = config.get_access_key_id().ok_or_else(|| {
        BlobError::ConfigError("access_key_id required for bucket creation".to_string())
    })?;
    let secret_key = config.get_secret_access_key().ok_or_else(|| {
        BlobError::ConfigError("secret_access_key required for bucket creation".to_string())
    })?;

    let credentials = Credentials::new(access_key, secret_key, None, None, "plexspaces-blob");

    let region = if config.region.is_empty() {
        "us-east-1".to_string()
    } else {
        config.region.clone()
    };

    let mut s3_config_builder = aws_sdk_s3::Config::builder()
        .credentials_provider(credentials)
        .region(Region::new(region.clone()))
        .behavior_version_latest();

    // For MinIO or custom endpoints, set the endpoint URL
    if !config.endpoint.is_empty() {
        s3_config_builder = s3_config_builder
            .endpoint_url(&config.endpoint)
            .force_path_style(true);
    }

    let s3_config = s3_config_builder.build();
    let client = Client::from_conf(s3_config);

    // Check if bucket exists by trying to head it
    match client.head_bucket().bucket(&config.bucket).send().await {
        Ok(_) => {
            tracing::debug!(bucket = %config.bucket, "Bucket already exists");
            Ok(())
        }
        Err(e) => {
            // Check if the error is "bucket not found" (404)
            let is_not_found = e
                .raw_response()
                .map(|r| r.status().as_u16() == 404)
                .unwrap_or(false);

            if is_not_found {
                tracing::info!(bucket = %config.bucket, "Bucket does not exist, creating it");

                // Create the bucket
                let create_result = if region == "us-east-1" {
                    // us-east-1 doesn't need location constraint
                    client.create_bucket().bucket(&config.bucket).send().await
                } else {
                    // Other regions need location constraint
                    use aws_sdk_s3::types::{BucketLocationConstraint, CreateBucketConfiguration};
                    let constraint = BucketLocationConstraint::from(region.as_str());
                    let cfg = CreateBucketConfiguration::builder()
                        .location_constraint(constraint)
                        .build();
                    client
                        .create_bucket()
                        .bucket(&config.bucket)
                        .create_bucket_configuration(cfg)
                        .send()
                        .await
                };

                match create_result {
                    Ok(_) => {
                        tracing::info!(bucket = %config.bucket, "Bucket created successfully");
                        Ok(())
                    }
                    Err(create_err) => {
                        // Check if bucket was created by another process (race condition)
                        let already_exists =
                            create_err.to_string().contains("BucketAlreadyOwnedByYou")
                                || create_err.to_string().contains("BucketAlreadyExists");
                        if already_exists {
                            tracing::debug!(bucket = %config.bucket, "Bucket already exists (race condition)");
                            Ok(())
                        } else {
                            Err(BlobError::StorageError(format!(
                                "Failed to create bucket '{}': {}",
                                config.bucket, create_err
                            )))
                        }
                    }
                }
            } else {
                // Some other error (permissions, etc.)
                Err(BlobError::StorageError(format!(
                    "Failed to check bucket '{}': {}",
                    config.bucket, e
                )))
            }
        }
    }
}

/// No-op bucket creation when s3-backend feature is not enabled
#[cfg(not(feature = "s3-backend"))]
async fn ensure_bucket_exists(_config: &BlobConfig) -> BlobResult<()> {
    // Without s3-backend feature, we can't create buckets programmatically
    // The bucket must exist beforehand
    Ok(())
}

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
    pub async fn new(config: BlobConfig, repository: Arc<dyn BlobRepository>) -> BlobResult<Self> {
        config.validate()?;

        // Set default prefix if empty
        let prefix = if config.prefix.is_empty() {
            "/plexspaces".to_string()
        } else {
            config.prefix.clone()
        };

        let object_store: Arc<dyn ObjectStore> = match config.backend.as_str() {
            "s3" => {
                let mut builder = AmazonS3Builder::new().with_bucket_name(&config.bucket);

                if !config.region.is_empty() {
                    builder = builder.with_region(&config.region);
                }

                if let Some(access_key_id) = config.get_access_key_id() {
                    builder = builder.with_access_key_id(&access_key_id);
                }

                if let Some(secret_access_key) = config.get_secret_access_key() {
                    builder = builder.with_secret_access_key(&secret_access_key);
                }

                Arc::new(builder.build().map_err(|e| {
                    BlobError::ConfigError(format!("Failed to build S3 store: {}", e))
                })?)
            }
            "minio" => {
                if config.endpoint.is_empty() {
                    return Err(BlobError::ConfigError(
                        "endpoint required for MinIO".to_string(),
                    ));
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

                Arc::new(builder.build().map_err(|e| {
                    BlobError::ConfigError(format!("Failed to build MinIO store: {}", e))
                })?)
            }
            "gcp" => {
                let mut builder = GoogleCloudStorageBuilder::new().with_bucket_name(&config.bucket);

                if !config.gcp_service_account_json.is_empty() {
                    builder = builder.with_service_account_path(&config.gcp_service_account_json);
                }

                Arc::new(builder.build().map_err(|e| {
                    BlobError::ConfigError(format!("Failed to build GCP store: {}", e))
                })?)
            }
            "azure" => {
                if config.azure_account_name.is_empty() {
                    return Err(BlobError::ConfigError(
                        "azure_account_name required".to_string(),
                    ));
                }

                let mut builder = MicrosoftAzureBuilder::new()
                    .with_account(&config.azure_account_name)
                    .with_container_name(&config.bucket);

                if !config.azure_account_key.is_empty() {
                    builder = builder.with_access_key(&config.azure_account_key);
                }

                Arc::new(builder.build().map_err(|e| {
                    BlobError::ConfigError(format!("Failed to build Azure store: {}", e))
                })?)
            }
            "local" => {
                // Local filesystem for testing
                Arc::new(LocalFileSystem::new_with_prefix("/").map_err(|e| {
                    BlobError::ConfigError(format!(
                        "Failed to create local filesystem store: {}",
                        e
                    ))
                })?)
            }
            _ => {
                return Err(BlobError::ConfigError(format!(
                    "Unsupported backend: {}",
                    config.backend
                )))
            }
        };

        // For S3/MinIO backends, ensure the bucket exists (auto-create if needed)
        if config.backend == "s3" || config.backend == "minio" {
            ensure_bucket_exists(&config).await?;
        }

        // Log blob service configuration
        let endpoint_display = if config.endpoint.is_empty() {
            "(default)".to_string()
        } else {
            config.endpoint.clone()
        };

        tracing::info!(
            backend = %config.backend,
            bucket = %config.bucket,
            endpoint = %endpoint_display,
            prefix = %prefix,
            use_ssl = config.use_ssl,
            "Blob storage initialized"
        );

        let mut config_with_prefix = config;
        config_with_prefix.prefix = prefix;

        Ok(Self {
            config: config_with_prefix,
            object_store,
            repository,
        })
    }

    /// Upload a blob
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `name` - Blob name
    /// * `data` - Blob content
    /// * `content_type` - Optional content type
    /// * `blob_group` - Optional blob group
    /// * `kind` - Optional blob kind
    /// * `metadata` - Optional metadata map
    /// * `tags` - Optional tags map
    /// * `expires_after` - Optional expiration duration
    pub async fn upload_blob(
        &self,
        ctx: &RequestContext,
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

        // Check if blob with same SHA256 already exists (content deduplication)
        if let Some(existing) = self.repository.get_by_sha256(ctx, &sha256).await? {
            // Verify the blob still exists in object store before returning deduped metadata
            let storage_path = get_storage_path(&existing, &self.config.prefix);
            let path = ObjectPath::from(storage_path.clone());
            match self.object_store.head(&path).await {
                Ok(_) => {
                    // Blob exists, return existing metadata (deduplication)
                    tracing::debug!(
                        blob_id = %existing.blob_id,
                        sha256 = %sha256,
                        storage_path = %storage_path,
                        "Returning deduplicated blob"
                    );
                    return Ok(existing);
                }
                Err(_) => {
                    // Blob doesn't exist in object store - stale metadata
                    // Delete the stale metadata and proceed with new upload
                    tracing::warn!(
                        blob_id = %existing.blob_id,
                        sha256 = %sha256,
                        storage_path = %storage_path,
                        "Stale blob metadata found (blob missing from object store), cleaning up"
                    );
                    // Delete stale metadata so we can upload fresh
                    let _ = self.repository.delete(ctx, &existing.blob_id).await;
                }
            }
        }

        // Check if a blob with the same name already exists
        // If so, delete it first (upsert behavior for name-based access)
        if let Some(existing) = self.get_metadata_by_name(ctx, name).await? {
            // Delete the old blob to allow the new one with the same name
            self.delete_blob(ctx, &existing.blob_id).await?;
        }

        // Generate blob ID (ULID) - this is the internal identifier
        let blob_id = Ulid::new().to_string();

        // Create metadata using proto type
        // Note: blob_id is returned in the metadata for callers who need it
        let now = Utc::now();
        let expires_at = expires_after.map(|d| datetime_to_timestamp(now + d));

        let blob_metadata = BlobMetadata {
            blob_id: blob_id.clone(),
            tenant_id: ctx.tenant_id().to_string(),
            namespace: ctx.namespace().to_string(),
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
        self.object_store
            .put(&path, bytes.into())
            .await
            .map_err(|e| BlobError::StorageError(format!("Failed to upload blob: {}", e)))?;

        // Get ETag from object store (if available)
        // Note: object_store doesn't directly return ETag, so we'll skip it for now
        // blob_metadata.etag = Some(etag);

        // Save metadata to repository
        self.repository.save(ctx, &blob_metadata).await?;

        Ok(blob_metadata)
    }

    /// Download a blob
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `blob_id` - Blob identifier
    pub async fn download_blob(&self, ctx: &RequestContext, blob_id: &str) -> BlobResult<Vec<u8>> {
        // Get metadata (automatically filtered by tenant_id and namespace)
        let metadata = self
            .repository
            .get(ctx, blob_id)
            .await?
            .ok_or_else(|| BlobError::NotFound(blob_id.to_string()))?;

        // Get storage path
        let storage_path = get_storage_path(&metadata, &self.config.prefix);
        let path = ObjectPath::from(storage_path);

        // Download from object store
        let result = self
            .object_store
            .get(&path)
            .await
            .map_err(|e| BlobError::StorageError(format!("Failed to download blob: {}", e)))?;
        let bytes = result
            .bytes()
            .await
            .map_err(|e| BlobError::StorageError(format!("Failed to read blob bytes: {}", e)))?;

        Ok(bytes.to_vec())
    }

    /// Get blob metadata
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `blob_id` - Blob identifier
    pub async fn get_metadata(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> BlobResult<BlobMetadata> {
        self.repository
            .get(ctx, blob_id)
            .await?
            .ok_or_else(|| BlobError::NotFound(blob_id.to_string()))
    }

    /// Get blob metadata by name (exact match)
    ///
    /// This function finds a blob by its user-friendly name (e.g., "assets/images/logo.png").
    /// It's useful for WASM actors that use paths as blob identifiers.
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `name` - Blob name (exact match)
    pub async fn get_metadata_by_name(
        &self,
        ctx: &RequestContext,
        name: &str,
    ) -> BlobResult<Option<BlobMetadata>> {
        // Use list with name_prefix filter to find exact match
        let filters = ListFilters {
            name_prefix: Some(name.to_string()),
            ..Default::default()
        };
        let (blobs, _count) = self.repository.list(ctx, &filters, 100, 0).await?;

        // Find exact name match (prefix filter might return more)
        Ok(blobs.into_iter().find(|b| b.name == name))
    }

    /// Download a blob by name (exact match)
    ///
    /// This function finds a blob by its user-friendly name and downloads it.
    /// It's useful for WASM actors that use paths as blob identifiers.
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `name` - Blob name (exact match)
    pub async fn download_blob_by_name(
        &self,
        ctx: &RequestContext,
        name: &str,
    ) -> BlobResult<Vec<u8>> {
        // Find the blob by name
        let metadata = self
            .get_metadata_by_name(ctx, name)
            .await?
            .ok_or_else(|| BlobError::NotFound(name.to_string()))?;

        // Download using the actual blob_id
        self.download_blob(ctx, &metadata.blob_id).await
    }

    /// Delete a blob by name (exact match)
    ///
    /// This function finds a blob by its user-friendly name and deletes it.
    /// It's useful for WASM actors that use paths as blob identifiers.
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `name` - Blob name (exact match)
    pub async fn delete_blob_by_name(&self, ctx: &RequestContext, name: &str) -> BlobResult<()> {
        // Find the blob by name
        let metadata = self
            .get_metadata_by_name(ctx, name)
            .await?
            .ok_or_else(|| BlobError::NotFound(name.to_string()))?;

        // Delete using the actual blob_id
        self.delete_blob(ctx, &metadata.blob_id).await
    }

    /// List blobs
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `filters` - Filter criteria
    /// * `page_size` - Number of results per page
    /// * `page` - Page number (1-indexed)
    pub async fn list_blobs(
        &self,
        ctx: &RequestContext,
        filters: &ListFilters,
        page_size: i64,
        page: i64,
    ) -> BlobResult<(Vec<BlobMetadata>, i64)> {
        let offset = (page - 1) * page_size;
        self.repository.list(ctx, filters, page_size, offset).await
    }

    /// Delete a blob
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `blob_id` - Blob identifier
    pub async fn delete_blob(&self, ctx: &RequestContext, blob_id: &str) -> BlobResult<()> {
        // Get metadata (automatically filtered by tenant_id and namespace)
        let metadata = self
            .repository
            .get(ctx, blob_id)
            .await?
            .ok_or_else(|| BlobError::NotFound(blob_id.to_string()))?;

        // Delete from object store
        let storage_path = get_storage_path(&metadata, &self.config.prefix);
        let path = ObjectPath::from(storage_path);
        self.object_store
            .delete(&path)
            .await
            .map_err(|e| BlobError::StorageError(format!("Failed to delete blob: {}", e)))?;

        // Delete metadata
        self.repository.delete(ctx, blob_id).await?;

        Ok(())
    }

    /// Generate presigned URL (for direct client access)
    /// Requires 'presigned-urls' feature to be enabled
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `blob_id` - Blob identifier
    /// * `operation` - Operation type ("GET" or "PUT")
    /// * `expires_after` - Expiration duration
    pub async fn generate_presigned_url(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
        operation: &str,
        expires_after: Duration,
    ) -> BlobResult<String> {
        // Get metadata (automatically filtered by tenant_id and namespace)
        let metadata = self
            .repository
            .get(ctx, blob_id)
            .await?
            .ok_or_else(|| BlobError::NotFound(blob_id.to_string()))?;

        // Get storage path
        let storage_path = crate::helpers::get_storage_path(&metadata, &self.config.prefix);

        // Generate presigned URL
        crate::presigned::generate_presigned_url(
            &self.config,
            &storage_path,
            operation,
            expires_after,
        )
        .await
    }

    /// Find expired blobs
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `limit` - Maximum number of results
    pub async fn find_expired(
        &self,
        ctx: &RequestContext,
        limit: i64,
    ) -> BlobResult<Vec<BlobMetadata>> {
        self.repository.find_expired(ctx, limit).await
    }
}

// Implement Service trait for ServiceLocator registration
impl plexspaces_core::Service for BlobService {
    fn service_name(&self) -> String {
        "BlobService".to_string()
    }
}

// Implement BlobServiceTrait for ServiceLocator access
#[async_trait::async_trait]
impl plexspaces_core::BlobServiceTrait for BlobService {
    async fn upload(
        &self,
        ctx: &RequestContext,
        name: &str,
        data: Vec<u8>,
        content_type: Option<String>,
        metadata: std::collections::HashMap<String, String>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Use the full name/path as provided
        let blob_metadata = self
            .upload_blob(
                ctx,
                name,
                data,
                content_type,
                None, // blob_group
                None, // kind
                metadata,
                std::collections::HashMap::new(), // tags
                None,                             // expires_after
            )
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        Ok(blob_metadata.blob_id)
    }

    async fn download(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let data = self
            .download_blob(ctx, blob_id)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        Ok(data)
    }

    async fn download_by_name(
        &self,
        ctx: &RequestContext,
        name: &str,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        match self.download_blob_by_name(ctx, name).await {
            Ok(data) => Ok(Some(data)),
            Err(BlobError::NotFound(_)) => Ok(None),
            Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    async fn delete(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.delete_blob(ctx, blob_id)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        Ok(())
    }

    async fn delete_by_name(
        &self,
        ctx: &RequestContext,
        name: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        match self.delete_blob_by_name(ctx, name).await {
            Ok(()) => Ok(true),
            Err(BlobError::NotFound(_)) => Ok(false),
            Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    async fn exists(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        match self.get_metadata(ctx, blob_id).await {
            Ok(_) => Ok(true),
            Err(BlobError::NotFound(_)) => Ok(false),
            Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    async fn list(
        &self,
        ctx: &RequestContext,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let filters = ListFilters {
            name_prefix: Some(prefix.to_string()),
            ..Default::default()
        };
        let (blobs, _total) = self
            .list_blobs(ctx, &filters, limit as i64, 1)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        // Return names (paths), not blob_ids
        Ok(blobs.into_iter().map(|b| b.name).collect())
    }

    fn as_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync> {
        self
    }
}
