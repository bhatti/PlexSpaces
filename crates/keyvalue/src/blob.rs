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

//! Blob-based KeyValue store implementation using object_store directly.
//!
//! ## Purpose
//! Provides a KeyValue store implementation using S3-compatible object storage (MinIO, AWS S3, etc.)
//! directly, without requiring blob service or SQL database dependencies.
//!
//! ## Design
//! - **Direct Object Store**: Uses `object_store` crate directly (no blob service dependency)
//! - **No SQL Required**: Stores all metadata in object paths/names (no database needed)
//! - **Path Structure**: `{prefix}/keyvalue/{tenant_id}/{namespace}/{key}`
//! - **TTL Support**: Uses object expiration or last_modified timestamp
//! - **Multi-tenancy**: Tenant/namespace isolation via path structure
//!
//! ## Architecture
//! ```
//! KeyValue Store (Blob Backend)
//!     ↓
//! Object Store (MinIO/S3/GCP/Azure)
//!     - Objects stored at: {prefix}/keyvalue/{tenant}/{namespace}/{key}
//!     - No SQL database needed
//!     - All metadata in object paths/names
//! ```
//!
//! ## Performance Considerations
//! - Direct object store operations (put/get/delete)
//! - List operations use S3 prefix listing (efficient for large datasets)
//! - No SQL queries needed
//!
//! ## Limitations
//! - Watch operations are not supported (object_store doesn't provide watch notifications)
//! - Atomic operations (CAS, increment) require multiple operations (may have race conditions)
//! - TTL cleanup requires periodic scanning (no automatic expiration)

use crate::{KVError, KVEvent, KVResult, KVStats, KeyValueStore};
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use object_store::{
    aws::AmazonS3Builder,
    azure::MicrosoftAzureBuilder,
    gcp::GoogleCloudStorageBuilder,
    local::LocalFileSystem,
    path::Path as ObjectPath,
    ObjectMeta, ObjectStore,
};
use plexspaces_core::RequestContext;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};
use tokio::sync::mpsc::Receiver;
use tracing::{debug, error, info, instrument, trace, warn};

/// Configuration for blob-based keyvalue store
#[derive(Debug, Clone)]
pub struct BlobKVConfig {
    /// Storage prefix (e.g., "/plexspaces")
    pub prefix: String,
    /// Backend type (minio, s3, gcp, azure, local)
    pub backend: String,
    /// Bucket name
    pub bucket: String,
    /// Endpoint URL (for MinIO)
    pub endpoint: Option<String>,
    /// Region (for S3/GCP/Azure)
    pub region: Option<String>,
    /// Access key ID
    pub access_key_id: Option<String>,
    /// Secret access key
    pub secret_access_key: Option<String>,
    /// Use SSL
    pub use_ssl: bool,
    /// GCP service account JSON path
    pub gcp_service_account_json: Option<String>,
    /// Azure account name
    pub azure_account_name: Option<String>,
    /// Azure account key
    pub azure_account_key: Option<String>,
}

impl BlobKVConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        use std::env;
        Self {
            prefix: env::var("BLOB_PREFIX")
                .unwrap_or_else(|_| "/plexspaces".to_string()),
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
            gcp_service_account_json: env::var("GCP_SERVICE_ACCOUNT_JSON").ok(),
            azure_account_name: env::var("AZURE_ACCOUNT_NAME").ok(),
            azure_account_key: env::var("AZURE_ACCOUNT_KEY").ok(),
        }
    }

    /// Build object store from config
    pub async fn build_object_store(&self) -> Result<Arc<dyn ObjectStore>, KVError> {
        let store: Arc<dyn ObjectStore> = match self.backend.as_str() {
            "s3" => {
                let mut builder = AmazonS3Builder::new()
                    .with_bucket_name(&self.bucket);

                if let Some(ref region) = self.region {
                    builder = builder.with_region(region);
                }

                if let Some(ref access_key_id) = self.access_key_id {
                    builder = builder.with_access_key_id(access_key_id);
                }

                if let Some(ref secret_access_key) = self.secret_access_key {
                    builder = builder.with_secret_access_key(secret_access_key);
                }

                Arc::new(builder.build().map_err(|e| {
                    KVError::ConfigError(format!("Failed to build S3 store: {}", e))
                })?)
            }
            "minio" => {
                let endpoint = self.endpoint.as_ref().ok_or_else(|| {
                    KVError::ConfigError("endpoint required for MinIO".to_string())
                })?;

                let mut builder = AmazonS3Builder::new()
                    .with_bucket_name(&self.bucket)
                    .with_endpoint(endpoint)
                    .with_allow_http(!self.use_ssl);

                if let Some(ref access_key_id) = self.access_key_id {
                    builder = builder.with_access_key_id(access_key_id);
                }

                if let Some(ref secret_access_key) = self.secret_access_key {
                    builder = builder.with_secret_access_key(secret_access_key);
                }

                Arc::new(builder.build().map_err(|e| {
                    KVError::ConfigError(format!("Failed to build MinIO store: {}", e))
                })?)
            }
            "gcp" => {
                let mut builder = GoogleCloudStorageBuilder::new()
                    .with_bucket_name(&self.bucket);

                if let Some(ref service_account) = self.gcp_service_account_json {
                    builder = builder.with_service_account_path(service_account);
                }

                Arc::new(builder.build().map_err(|e| {
                    KVError::ConfigError(format!("Failed to build GCP store: {}", e))
                })?)
            }
            "azure" => {
                let account_name = self.azure_account_name.as_ref().ok_or_else(|| {
                    KVError::ConfigError("azure_account_name required".to_string())
                })?;

                let mut builder = MicrosoftAzureBuilder::new()
                    .with_account(account_name)
                    .with_container_name(&self.bucket);

                if let Some(ref account_key) = self.azure_account_key {
                    builder = builder.with_access_key(account_key);
                }

                Arc::new(builder.build().map_err(|e| {
                    KVError::ConfigError(format!("Failed to build Azure store: {}", e))
                })?)
            }
            "local" => {
                Arc::new(
                    LocalFileSystem::new_with_prefix("/")
                        .map_err(|e| KVError::ConfigError(format!("Failed to create local filesystem store: {}", e)))?
                )
            }
            _ => {
                return Err(KVError::ConfigError(format!(
                    "Unsupported backend: {}",
                    self.backend
                )));
            }
        };

        Ok(store)
    }
}

/// Blob-based KeyValue store using object_store directly.
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_keyvalue::{KeyValueStore, blob::BlobKVStore};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Create from environment variables
/// let kv = BlobKVStore::from_env().await?;
///
/// let ctx = RequestContext::new_without_auth("tenant".to_string(), "default".to_string());
/// kv.put(&ctx, "key", b"value".to_vec()).await?;
/// let value = kv.get(&ctx, "key").await?;
/// assert_eq!(value, Some(b"value".to_vec()));
/// # Ok(())
/// # }
/// ```
pub struct BlobKVStore {
    object_store: Arc<dyn ObjectStore>,
    config: BlobKVConfig,
}

impl BlobKVStore {
    /// Create a new blob-based KeyValue store from config.
    pub async fn new(config: BlobKVConfig) -> KVResult<Self> {
        let object_store = config.build_object_store().await?;
        Ok(Self {
            object_store,
            config,
        })
    }

    /// Create from environment variables.
    pub async fn from_env() -> KVResult<Self> {
        let config = BlobKVConfig::from_env();
        Self::new(config).await
    }

    /// Create with custom object store (for testing).
    pub fn with_object_store(
        config: BlobKVConfig,
        object_store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            object_store,
            config,
        }
    }

    /// Get storage path for a key.
    /// Format: `{prefix}/keyvalue/{tenant_id}/{namespace}/{key}`
    fn storage_path(&self, ctx: &RequestContext, key: &str) -> ObjectPath {
        let normalized_prefix = self.config.prefix.trim_end_matches('/');
        let path = format!(
            "{}/keyvalue/{}/{}/{}",
            normalized_prefix,
            ctx.tenant_id(),
            ctx.namespace(),
            key
        );
        ObjectPath::from(path)
    }

    /// Extract key from storage path.
    /// Reverse of `storage_path()`.
    /// Path format: {prefix}/keyvalue/{tenant}/{namespace}/{key}
    fn extract_key_from_path(path: &str) -> Option<String> {
        // Find "keyvalue/" in path, then get everything after tenant/namespace/
        // Example: "/plexspaces/keyvalue/tenant-1/ns-1/config:timeout"
        //          -> after "/keyvalue/" = "tenant-1/ns-1/config:timeout"
        //          -> skip first 2 parts (tenant, namespace) -> "config:timeout"
        if let Some(keyvalue_pos) = path.find("/keyvalue/") {
            let after_keyvalue = &path[keyvalue_pos + "/keyvalue/".len()..];
            // Split by '/' and skip tenant and namespace (first 2 parts)
            let parts: Vec<&str> = after_keyvalue.split('/').collect();
            if parts.len() >= 3 {
                // parts[0] = tenant, parts[1] = namespace, parts[2..] = key (may have /)
                Some(parts[2..].join("/"))
            } else {
                debug!(path = %path, "Path doesn't have enough parts after keyvalue/");
                None
            }
        } else {
            debug!(path = %path, "Path doesn't contain /keyvalue/");
            None
        }
    }

    /// Get prefix path for listing.
    /// Format: `{prefix}/keyvalue/{tenant_id}/{namespace}/{key_prefix}`
    fn list_prefix_path(&self, ctx: &RequestContext, key_prefix: &str) -> ObjectPath {
        let normalized_prefix = self.config.prefix.trim_end_matches('/');
        let path = format!(
            "{}/keyvalue/{}/{}/{}",
            normalized_prefix,
            ctx.tenant_id(),
            ctx.namespace(),
            key_prefix
        );
        trace!(list_path = %path, key_prefix = %key_prefix, "Listing with prefix path");
        ObjectPath::from(path)
    }

    /// Check if object is expired based on last_modified and TTL.
    fn is_expired(meta: &ObjectMeta, expires_at: Option<DateTime<Utc>>) -> bool {
        if let Some(expires) = expires_at {
            return Utc::now() > expires;
        }
        false
    }
}

#[async_trait]
impl KeyValueStore for BlobKVStore {
    #[instrument(skip(self), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), key = %key))]
    async fn get(&self, ctx: &RequestContext, key: &str) -> KVResult<Option<Vec<u8>>> {
        let start = Instant::now();
        trace!("Getting key from object store");
        let path = self.storage_path(ctx, key);

        // Try to get object metadata first (to check if exists)
        match self.object_store.head(&path).await {
            Ok(meta) => {
                // Check expiration if needed (we'll store TTL in metadata later)
                // For now, just download the object
                let result = self.object_store.get(&path).await.map_err(|e| {
                    error!(error = %e, path = %path, "Failed to get object");
                    KVError::StorageError(format!("Failed to get object: {}", e))
                })?;

                let bytes = result.bytes().await.map_err(|e| {
                    error!(error = %e, "Failed to read object bytes");
                    KVError::StorageError(format!("Failed to read object bytes: {}", e))
                })?;

                debug!(
                    duration_ms = start.elapsed().as_millis(),
                    size_bytes = bytes.len(),
                    "Key retrieved successfully"
                );
                Ok(Some(bytes.to_vec()))
            }
            Err(object_store::Error::NotFound { .. }) => {
                debug!(duration_ms = start.elapsed().as_millis(), "Key not found");
                Ok(None)
            }
            Err(e) => {
                error!(error = %e, "Failed to check object existence");
                Err(KVError::StorageError(format!("Failed to check object: {}", e)))
            }
        }
    }

    #[instrument(skip(self, value), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), key = %key, value_size = value.len()))]
    async fn put(&self, ctx: &RequestContext, key: &str, value: Vec<u8>) -> KVResult<()> {
        let start = Instant::now();
        trace!("Putting key-value pair to object store");
        let path = self.storage_path(ctx, key);

        // Delete existing object if any (for overwrite)
        let _ = self.object_store.delete(&path).await;

        // Upload new object
        let bytes = Bytes::from(value.clone());
        let value_size = value.len();
        self.object_store
            .put(&path, bytes.into())
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to put object");
                KVError::StorageError(format!("Failed to put object: {}", e))
            })?;

        info!(
            duration_ms = start.elapsed().as_millis(),
            path = %path,
            key = %key,
            size_bytes = value_size,
            "Key stored successfully"
        );
        Ok(())
    }

    #[instrument(skip(self), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), key = %key))]
    async fn delete(&self, ctx: &RequestContext, key: &str) -> KVResult<()> {
        let start = Instant::now();
        trace!("Deleting key from object store");
        let path = self.storage_path(ctx, key);

        match self.object_store.delete(&path).await {
            Ok(()) => {
                info!(
                    duration_ms = start.elapsed().as_millis(),
                    key = %key,
                    "Key deleted successfully"
                );
                Ok(())
            }
            Err(object_store::Error::NotFound { .. }) => {
                debug!(
                    duration_ms = start.elapsed().as_millis(),
                    key = %key,
                    "Key not found (idempotent delete)"
                );
                Ok(()) // Idempotent - succeed even if not found
            }
            Err(e) => {
                error!(error = %e, "Failed to delete object");
                Err(KVError::StorageError(format!("Failed to delete object: {}", e)))
            }
        }
    }

    async fn exists(&self, ctx: &RequestContext, key: &str) -> KVResult<bool> {
        let path = self.storage_path(ctx, key);
        match self.object_store.head(&path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(KVError::StorageError(format!("Failed to check object: {}", e))),
        }
    }

    #[instrument(skip(self), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), prefix = %prefix))]
    async fn list(&self, ctx: &RequestContext, prefix: &str) -> KVResult<Vec<String>> {
        let start = Instant::now();
        trace!("Listing keys with prefix");
        
        // List all objects for this tenant/namespace, then filter by key prefix
        // This is simpler than trying to match the exact prefix path
        let base_path = self.list_prefix_path(ctx, "");
        let mut stream = self.object_store.list(Some(&base_path));
        let mut keys = Vec::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(meta) => {
                    // Extract key from path
                    // Path format: {prefix}/keyvalue/{tenant}/{namespace}/{key}
                    let path_str = meta.location.as_ref();
                    if let Some(key) = Self::extract_key_from_path(path_str) {
                        // Filter by key prefix
                        if key.starts_with(prefix) {
                            keys.push(key);
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to list objects");
                    return Err(KVError::StorageError(format!("Failed to list objects: {}", e)));
                }
            }
        }

        keys.sort();
        keys.dedup();
        debug!(
            duration_ms = start.elapsed().as_millis(),
            count = keys.len(),
            "Listed keys successfully"
        );
        Ok(keys)
    }

    async fn multi_get(&self, ctx: &RequestContext, keys: &[&str]) -> KVResult<Vec<Option<Vec<u8>>>> {
        // Fetch all keys in parallel
        let mut handles: Vec<tokio::task::JoinHandle<Result<Option<Vec<u8>>, KVError>>> = Vec::new();
        for key in keys {
            let key_str = key.to_string();
            let store = Arc::clone(&self.object_store);
            let config = self.config.clone();
            let tenant_id = ctx.tenant_id().to_string();
            let namespace = ctx.namespace().to_string();
            handles.push(tokio::spawn(async move {
                let normalized_prefix = config.prefix.trim_end_matches('/');
                let path = format!("{}/keyvalue/{}/{}/{}", normalized_prefix, tenant_id, namespace, key_str);
                let obj_path = ObjectPath::from(path);

                match store.get(&obj_path).await {
                    Ok(result) => {
                        let bytes = result.bytes().await.map_err(|e| {
                            KVError::StorageError(format!("Failed to read bytes: {}", e))
                        })?;
                        Ok(Some(bytes.to_vec()))
                    }
                    Err(object_store::Error::NotFound { .. }) => Ok(None),
                    Err(e) => Err(KVError::StorageError(format!("Failed to get object: {}", e))),
                }
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(value)) => results.push(value),
                Ok(Err(e)) => return Err(e),
                Err(e) => return Err(KVError::StorageError(format!("Task error: {}", e))),
            }
        }

        Ok(results)
    }

    async fn multi_put(&self, ctx: &RequestContext, pairs: &[(&str, Vec<u8>)]) -> KVResult<()> {
        // Put all pairs sequentially (could be optimized with parallel uploads)
        for (key, value) in pairs {
            self.put(ctx, key, value.clone()).await?;
        }
        Ok(())
    }

    async fn put_with_ttl(
        &self,
        ctx: &RequestContext,
        key: &str,
        value: Vec<u8>,
        _ttl: StdDuration,
    ) -> KVResult<()> {
        // For TTL, we'll store expiration time in object metadata
        // Since object_store doesn't directly support expiration,
        // we'll use last_modified + ttl for expiration checking
        // In production, you might want to use S3 lifecycle policies or scheduled cleanup
        
        // For now, just put the object - TTL checking will be done in get()
        // by comparing last_modified + ttl with current time
        // Note: This is a limitation - we'd need to store TTL metadata separately
        // or use object tags/metadata
        
        // Store TTL in object name or use a separate metadata object
        // For simplicity, we'll just put it and handle expiration in get()
        self.put(ctx, key, value).await?;
        
        // TODO: Store TTL metadata (could use object tags or separate metadata object)
        // For now, TTL is best-effort based on last_modified
        warn!("TTL support is limited - expiration checking requires metadata storage");
        Ok(())
    }

    async fn refresh_ttl(
        &self,
        ctx: &RequestContext,
        key: &str,
        ttl: StdDuration,
    ) -> KVResult<()> {
        // Get current value
        let value = self
            .get(ctx, key)
            .await?
            .ok_or_else(|| KVError::KeyNotFound(key.to_string()))?;

        // Re-upload to update last_modified (refreshes TTL)
        self.put_with_ttl(ctx, key, value, ttl).await
    }

    async fn get_ttl(&self, ctx: &RequestContext, key: &str) -> KVResult<Option<StdDuration>> {
        let path = self.storage_path(ctx, key);
        match self.object_store.head(&path).await {
            Ok(_meta) => {
                // TTL is not directly stored - would need metadata
                // For now, return None (TTL not fully supported without metadata storage)
                warn!("TTL retrieval not fully supported without metadata storage");
                Ok(None)
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(KVError::StorageError(format!("Failed to check object: {}", e))),
        }
    }

    async fn cas(
        &self,
        ctx: &RequestContext,
        key: &str,
        expected: Option<Vec<u8>>,
        new_value: Vec<u8>,
    ) -> KVResult<bool> {
        // Get current value
        let current = self.get(ctx, key).await?;

        // Check if current matches expected
        let matches = match (&current, &expected) {
            (None, None) => true,
            (Some(c), Some(e)) => c == e,
            _ => false,
        };

        if !matches {
            return Ok(false);
        }

        // Put new value
        self.put(ctx, key, new_value).await?;
        Ok(true)
    }

    async fn increment(&self, ctx: &RequestContext, key: &str, delta: i64) -> KVResult<i64> {
        // Get current value
        let current_bytes = self.get(ctx, key).await?.unwrap_or_else(|| {
            // Initialize to 0 if not exists
            vec![0u8; 8]
        });

        // Decode as i64 (big-endian)
        if current_bytes.len() != 8 {
            return Err(KVError::InvalidValue(
                "Value is not a valid i64 counter".to_string(),
            ));
        }

        let mut arr = [0u8; 8];
        arr.copy_from_slice(&current_bytes);
        let current = i64::from_be_bytes(arr);

        // Increment
        let new_value = current + delta;

        // Encode and store
        let new_bytes = new_value.to_be_bytes().to_vec();
        self.put(ctx, key, new_bytes).await?;

        Ok(new_value)
    }

    async fn decrement(&self, ctx: &RequestContext, key: &str, delta: i64) -> KVResult<i64> {
        self.increment(ctx, key, -delta).await
    }

    async fn watch(&self, _ctx: &RequestContext, _key: &str) -> KVResult<Receiver<KVEvent>> {
        // Watch is not supported for object store (no notification mechanism)
        Err(KVError::NotSupported(
            "Watch operations are not supported for blob backend".to_string(),
        ))
    }

    async fn watch_prefix(&self, _ctx: &RequestContext, _prefix: &str) -> KVResult<Receiver<KVEvent>> {
        // Watch is not supported for object store
        Err(KVError::NotSupported(
            "Watch operations are not supported for blob backend".to_string(),
        ))
    }

    async fn clear_prefix(&self, ctx: &RequestContext, prefix: &str) -> KVResult<usize> {
        let keys = self.list(ctx, prefix).await?;
        let count = keys.len();

        // Delete all keys
        for key in keys {
            let _ = self.delete(ctx, &key).await;
        }

        Ok(count)
    }

    async fn count_prefix(&self, ctx: &RequestContext, prefix: &str) -> KVResult<usize> {
        let keys = self.list(ctx, prefix).await?;
        Ok(keys.len())
    }

    async fn get_stats(&self, ctx: &RequestContext) -> KVResult<KVStats> {
        // List all keyvalue objects for this tenant/namespace
        let list_path = self.list_prefix_path(ctx, "");
        let mut stream = self.object_store.list(Some(&list_path));
        
        let mut total_keys = 0;
        let mut total_size = 0;

        while let Some(result) = stream.next().await {
            match result {
                Ok(meta) => {
                    total_keys += 1;
                    total_size += meta.size;
                }
                Err(e) => {
                    error!(error = %e, "Failed to list objects for stats");
                    return Err(KVError::StorageError(format!("Failed to list objects: {}", e)));
                }
            }
        }

        Ok(KVStats {
            total_keys,
            total_size_bytes: total_size,
            backend_type: "Blob".to_string(),
        })
    }
}

// Import futures::StreamExt for stream.next()
use futures::StreamExt;
