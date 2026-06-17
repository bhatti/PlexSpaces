// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Node-oriented blob startup: default config from environment and SQL-backed service wiring.
//!
//! Keeps object-store connection and [`ServiceLocator`] registration out of the node crate so
//! blob lifecycle stays co-located with the blob implementation.

use std::sync::Arc;

use plexspaces_actor::InitializableServiceLocator;
use plexspaces_proto::storage::v1::BlobConfig as ProtoBlobConfig;
use sqlx::any::AnyPoolOptions;

use crate::embedded_object_store::EmbeddedObjectStore;
use crate::repository::sql::SqlBlobRepository;
use crate::service::BlobService;
use crate::BlobError;

/// Default [`BlobConfig`](plexspaces_proto::storage::v1::BlobConfig) from well-known env vars.
///
/// Used when ReleaseSpec has no `runtime.blob` block (local dev / tests).
/// Defaults to `embedded` backend; if `BLOB_ENDPOINT` is not set, a local
/// S3-compatible object store process is started automatically at service creation time.
pub fn default_blob_config_from_env() -> ProtoBlobConfig {
    use std::env;

    ProtoBlobConfig {
        backend: env::var("BLOB_BACKEND").unwrap_or_else(|_| "embedded".to_string()),
        bucket: env::var("BLOB_BUCKET").unwrap_or_else(|_| "plexspaces".to_string()),
        endpoint: env::var("BLOB_ENDPOINT").ok().unwrap_or_default(),
        region: env::var("BLOB_REGION").ok().unwrap_or_default(),
        access_key_id: env::var("BLOB_ACCESS_KEY_ID")
            .or_else(|_| env::var("AWS_ACCESS_KEY_ID"))
            .ok()
            .unwrap_or_default(),
        secret_access_key: env::var("BLOB_SECRET_ACCESS_KEY")
            .or_else(|_| env::var("AWS_SECRET_ACCESS_KEY"))
            .ok()
            .unwrap_or_default(),
        use_ssl: env::var("BLOB_USE_SSL")
            .unwrap_or_else(|_| "false".to_string())
            .parse()
            .unwrap_or(false),
        prefix: env::var("BLOB_PREFIX").unwrap_or_else(|_| {
            env::var("HOME")
                .map(|h| format!("{}/plexspaces/blob", h))
                .unwrap_or_else(|_| "/tmp/plexspaces/blob".to_string())
        }),
        gcp_service_account_json: env::var("GCP_SERVICE_ACCOUNT_JSON")
            .ok()
            .unwrap_or_default(),
        azure_account_name: env::var("AZURE_ACCOUNT_NAME").ok().unwrap_or_default(),
        azure_account_key: env::var("AZURE_ACCOUNT_KEY").ok().unwrap_or_default(),
    }
}

/// Resolve blob section from an optional [`ReleaseSpec`](plexspaces_proto::node::v1::ReleaseSpec),
/// falling back to [`default_blob_config_from_env`].
pub fn blob_config_from_release_spec(
    release_spec: Option<&plexspaces_proto::node::v1::ReleaseSpec>,
) -> ProtoBlobConfig {
    if let Some(spec) = release_spec {
        if let Some(ref runtime) = spec.runtime {
            if let Some(ref blob) = runtime.blob {
                return blob.clone();
            }
        }
    }
    default_blob_config_from_env()
}

/// Connect SQL pool, construct [`BlobService`], register on `service_locator`, and return the service.
///
/// When the backend is `embedded` and no endpoint is configured, a local S3-compatible
/// object store process is started automatically on `embedded_port`. The returned
/// [`EmbeddedObjectStore`] (if any) must be kept alive for the node's lifetime —
/// drop it to stop the embedded process.
///
/// # Parameters
/// - `embedded_port`: Port for the embedded S3 API when auto-started. Callers should pass the
///   node's existing HTTP port (typically `grpc_port + 100`) so TLS and existing port-management
///   apply uniformly. Pass `0` to fall back to the OS default (not recommended for production).
pub async fn create_and_register_blob_service(
    service_locator: Arc<dyn InitializableServiceLocator + Send + Sync>,
    db_url: &str,
    mut blob_config: ProtoBlobConfig,
    embedded_port: u16,
) -> Result<(Arc<BlobService>, Option<EmbeddedObjectStore>), BlobError> {
    tracing::info!(
        backend = %blob_config.backend,
        bucket = %blob_config.bucket,
        endpoint = %blob_config.endpoint,
        "Initializing blob service"
    );

    // Auto-start embedded object store when backend is "embedded" and no external endpoint given
    let embedded_store = if blob_config.backend == "embedded" && blob_config.endpoint.is_empty() {
        tracing::info!(port = embedded_port, "Auto-starting embedded object store");
        let store = EmbeddedObjectStore::start(embedded_port).await.map_err(|e| {
            BlobError::ConfigError(format!("Failed to start embedded object store: {e}"))
        })?;
        blob_config.endpoint = store.s3_endpoint.clone();
        if !store.access_key.is_empty() {
            blob_config.access_key_id = store.access_key.clone();
            blob_config.secret_access_key = store.secret_key.clone();
        }
        if blob_config.bucket.is_empty() {
            blob_config.bucket = "plexspaces".to_string();
        }
        Some(store)
    } else {
        None
    };

    let pool_options = if db_url.starts_with("sqlite:") {
        AnyPoolOptions::new().max_connections(1)
    } else {
        AnyPoolOptions::new()
    };

    let any_pool = pool_options.connect(db_url).await.map_err(|e| {
        BlobError::ConfigError(format!("Failed to connect to database '{db_url}': {e}"))
    })?;

    let repository = Arc::new(SqlBlobRepository::new(any_pool).await.map_err(|e| {
        BlobError::RepositoryError(format!("Failed to create blob repository: {e}"))
    })?);

    let service = BlobService::new(blob_config.clone(), repository)
        .await
        .map_err(|e| BlobError::ConfigError(format!("Failed to initialize blob service: {e}")))?;

    let service_arc = Arc::new(service);

    let blob_trait: Arc<dyn plexspaces_actor::BlobServiceTrait> = service_arc.clone();
    service_locator.register_blob_service(blob_trait).await;

    tracing::info!(
        backend = %blob_config.backend,
        endpoint = %blob_config.endpoint,
        bucket = %blob_config.bucket,
        "Blob service registered"
    );

    Ok((service_arc, embedded_store))
}
