// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Integration tests for blob service using embedded S3-compatible object store.
//!
//! Tests start an embedded object store subprocess automatically when `rustfs` is
//! installed. If the binary is not installed and `BLOB_ENDPOINT` is not set, tests
//! are skipped with a warning rather than panicking.
//! Set BLOB_ENDPOINT to use an external endpoint instead.

use plexspaces_actor::{RequestContext, RequestContextExt};
use plexspaces_blob::{
    embedded_object_store::EmbeddedObjectStore,
    repository::sql::SqlBlobRepository,
    repository::ListFilters,
    BlobService,
};
use plexspaces_proto::storage::v1::BlobConfig as ProtoBlobConfig;
use std::sync::Arc;
use std::sync::OnceLock;

/// Shared embedded store + service, started once per test binary.
static TEST_SVC: OnceLock<tokio::sync::Mutex<Option<Arc<BlobService>>>> = OnceLock::new();
/// Keep the manager alive for the test binary's lifetime.
static _EMBEDDED_MGR: OnceLock<EmbeddedObjectStore> = OnceLock::new();

/// Returns `None` when the embedded store binary is absent and no `BLOB_ENDPOINT` is configured,
/// allowing tests to skip gracefully instead of panicking.
async fn get_test_service() -> Option<Arc<BlobService>> {
    let cache = TEST_SVC.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = cache.lock().await;
    if let Some(svc) = guard.as_ref() {
        return Some(svc.clone());
    }

    // Use external endpoint if provided, else try to start embedded store
    let (endpoint, _access_key, _secret_key) =
        if let Ok(ep) = std::env::var("BLOB_ENDPOINT") {
            (
                ep,
                std::env::var("BLOB_ACCESS_KEY_ID").unwrap_or_default(),
                std::env::var("BLOB_SECRET_ACCESS_KEY").unwrap_or_default(),
            )
        } else {
            match EmbeddedObjectStore::start(19200).await {
                Ok(store) => {
                    let ep = store.s3_endpoint.clone();
                    let ak = store.access_key.clone();
                    let sk = store.secret_key.clone();
                    let _ = _EMBEDDED_MGR.set(store);
                    (ep, ak, sk)
                }
                Err(e) => {
                    eprintln!(
                        "⚠️  WARNING: Skipping blob integration tests — embedded object store unavailable: {}\n\
                        Install: cargo install rustfs\n\
                        Or set BLOB_ENDPOINT to an external S3-compatible endpoint.",
                        e
                    );
                    return None;
                }
            }
        };

    sqlx::any::install_default_drivers();

    use sqlx::any::AnyPoolOptions;
    let any_pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory SQLite");

    let repository = Arc::new(
        SqlBlobRepository::new(any_pool)
            .await
            .expect("Failed to create blob repository"),
    );

    let config = ProtoBlobConfig {
        backend: "embedded".to_string(),
        bucket: "plexspaces-integ-test".to_string(),
        endpoint: endpoint.clone(),
        region: String::new(),
        access_key_id: _access_key,
        secret_access_key: _secret_key,
        use_ssl: false,
        prefix: "/plexspaces".to_string(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    let svc = Arc::new(
        BlobService::new(config, repository)
            .await
            .unwrap_or_else(|e| panic!("Failed to initialize BlobService at {}: {}", endpoint, e)),
    );

    // Probe connectivity
    let probe_ctx = create_test_context("__probe__", "__probe__");
    svc.upload_blob(
        &probe_ctx,
        "__connectivity_probe__",
        b"probe".to_vec(),
        None,
        None,
        None,
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("Object store is reachable but S3 operations fail: {}", e));

    tracing::info!(endpoint = %endpoint, "Using object store endpoint for integration tests");

    *guard = Some(svc.clone());
    Some(svc)
}

fn create_test_context(tenant_id: &str, namespace: &str) -> RequestContext {
    RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string())
}

#[tokio::test]
async fn test_upload_and_download_blob() {
    let Some(service) = get_test_service().await else { return; };
    let ctx = create_test_context("tenant-1", "ns-1");
    let data = b"Hello, World!".to_vec();

    let metadata = service
        .upload_blob(
            &ctx,
            "test.txt",
            data.clone(),
            Some("text/plain".to_string()),
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(metadata.tenant_id, "tenant-1");
    assert_eq!(metadata.namespace, "ns-1");
    assert_eq!(metadata.name, "test.txt");
    assert_eq!(metadata.content_length, data.len() as i64);

    let downloaded = service.download_blob(&ctx, &metadata.blob_id).await.unwrap();
    assert_eq!(downloaded, data);
}

#[tokio::test]
async fn test_deduplication() {
    let Some(service) = get_test_service().await else { return; };
    let ctx = create_test_context("tenant-1", "ns-dedup");
    let data = b"Duplicate content".to_vec();

    let metadata1 = service
        .upload_blob(
            &ctx, "file1.txt", data.clone(), None, None, None,
            std::collections::HashMap::new(), std::collections::HashMap::new(), None,
        )
        .await
        .unwrap();

    let metadata2 = service
        .upload_blob(
            &ctx, "file2.txt", data.clone(), None, None, None,
            std::collections::HashMap::new(), std::collections::HashMap::new(), None,
        )
        .await
        .unwrap();

    assert_eq!(metadata1.blob_id, metadata2.blob_id);
    assert_eq!(metadata1.sha256, metadata2.sha256);
}

#[tokio::test]
async fn test_list_and_delete() {
    let Some(service) = get_test_service().await else { return; };
    let ctx = create_test_context("tenant-list", "ns-1");

    for i in 1..=3 {
        service
            .upload_blob(
                &ctx,
                &format!("file{}.txt", i),
                format!("content{}", i).into_bytes(),
                None, None, None,
                std::collections::HashMap::new(), std::collections::HashMap::new(), None,
            )
            .await
            .unwrap();
    }

    let filters = ListFilters::default();
    let (blobs, total) = service.list_blobs(&ctx, &filters, 10, 1).await.unwrap();
    assert!(total >= 3);
    assert!(!blobs.is_empty());

    let blob_id = &blobs[0].blob_id;
    service.delete_blob(&ctx, blob_id).await.unwrap();
    assert!(service.get_metadata(&ctx, blob_id).await.is_err());
}

#[tokio::test]
async fn test_multi_tenancy_isolation() {
    let Some(service) = get_test_service().await else { return; };
    let ctx1 = create_test_context("tenant-iso-1", "ns-1");
    let ctx2 = create_test_context("tenant-iso-2", "ns-1");

    let metadata1 = service
        .upload_blob(
            &ctx1, "file.txt", b"tenant1".to_vec(), None, None, None,
            std::collections::HashMap::new(), std::collections::HashMap::new(), None,
        )
        .await
        .unwrap();

    let metadata2 = service
        .upload_blob(
            &ctx2, "file.txt", b"tenant2".to_vec(), None, None, None,
            std::collections::HashMap::new(), std::collections::HashMap::new(), None,
        )
        .await
        .unwrap();

    let (blobs, _) = service
        .list_blobs(&ctx1, &ListFilters::default(), 10, 1)
        .await
        .unwrap();

    assert!(blobs.iter().any(|b| b.blob_id == metadata1.blob_id));
    assert!(!blobs.iter().any(|b| b.blob_id == metadata2.blob_id));
}

#[cfg(feature = "presigned-urls")]
#[tokio::test]
async fn test_presigned_url_get() {
    let Some(service) = get_test_service().await else { return; };
    let ctx = create_test_context("tenant-1", "ns-presigned");

    let data = b"Test content for presigned URL".to_vec();
    let metadata = service
        .upload_blob(
            &ctx, "presigned-test.txt", data.clone(),
            Some("text/plain".to_string()), None, None,
            std::collections::HashMap::new(), std::collections::HashMap::new(), None,
        )
        .await
        .unwrap();

    use chrono::Duration;
    let presigned_url = service
        .generate_presigned_url(&ctx, &metadata.blob_id, "GET", Duration::hours(1))
        .await
        .unwrap();

    assert!(!presigned_url.is_empty());
    assert!(presigned_url.contains("http://") || presigned_url.contains("https://"));

    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client.get(&presigned_url).send().await.unwrap();
    assert!(response.status().is_success());
    let downloaded_data = response.bytes().await.unwrap();
    assert_eq!(downloaded_data.as_ref(), data.as_slice());
}

#[cfg(feature = "presigned-urls")]
#[tokio::test]
async fn test_presigned_url_invalid_operation() {
    let Some(service) = get_test_service().await else { return; };
    let ctx = create_test_context("tenant-1", "ns-presigned-invalid");

    let metadata = service
        .upload_blob(
            &ctx, "invalid-op-test.txt", b"Test invalid operation".to_vec(),
            Some("text/plain".to_string()), None, None,
            std::collections::HashMap::new(), std::collections::HashMap::new(), None,
        )
        .await
        .unwrap();

    use chrono::Duration;
    let result = service
        .generate_presigned_url(&ctx, &metadata.blob_id, "DELETE", Duration::hours(1))
        .await;

    assert!(result.is_err());
}
