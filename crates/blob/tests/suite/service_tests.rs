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

//! Unit tests for blob service using local filesystem backend

use object_store::local::LocalFileSystem;
use plexspaces_blob::{
    repository::sql::SqlBlobRepository, repository::ListFilters, BlobError, BlobService,
};
use plexspaces_core::RequestContext;
use plexspaces_proto::storage::v1::BlobConfig as ProtoBlobConfig;
use std::sync::Arc;
use std::sync::Once;
use tempfile::TempDir;

static INIT: Once = Once::new();

fn init_sqlx_drivers() {
    INIT.call_once(|| {
        // Install sqlx::any default drivers before any database operations
        // This is required for AnyPool to work with sqlite
        // Using Once ensures it's only called once, even in parallel test scenarios
        sqlx::any::install_default_drivers();
    });
}

async fn create_test_service() -> (Arc<BlobService>, TempDir) {
    // Ensure sqlx drivers are installed (idempotent, safe for parallel tests)
    init_sqlx_drivers();

    // Create temp directory for local filesystem
    let temp_dir = TempDir::new().unwrap();
    let local_store = Arc::new(LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap());

    // Use in-memory SQLite database for tests (fast, isolated, no file cleanup needed)
    use sqlx::any::AnyPoolOptions;
    use sqlx::AnyPool;

    // Use in-memory database for tests (not recovery-related, so memory is appropriate)
    // For in-memory SQLite, use max_connections=1 to ensure all operations share the same database
    let db_url = "sqlite::memory:";

    let any_pool = AnyPoolOptions::new()
        .max_connections(1)
        .connect(db_url)
        .await
        .unwrap();

    // Migrations are auto-applied in new()
    let repository = Arc::new(SqlBlobRepository::new(any_pool).await.unwrap());

    // Create blob config (not used when using with_object_store, but needed for type)
    let config = ProtoBlobConfig {
        backend: "local".to_string(),
        bucket: "test".to_string(),
        endpoint: String::new(),
        region: String::new(),
        access_key_id: String::new(),
        secret_access_key: String::new(),
        use_ssl: false,
        prefix: "/plexspaces".to_string(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    // Create service with custom object store (for testing)
    let service = BlobService::with_object_store(config, local_store, repository);
    (Arc::new(service), temp_dir)
}

fn create_test_context(tenant_id: &str, namespace: &str) -> RequestContext {
    RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string())
}

#[tokio::test]
async fn test_upload_and_download() {
    let (service, _temp_dir) = create_test_service().await;
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
    assert!(!metadata.blob_id.is_empty());
    assert_eq!(metadata.sha256.len(), 64);

    // Download
    let downloaded = service
        .download_blob(&ctx, &metadata.blob_id)
        .await
        .unwrap();
    assert_eq!(downloaded, data);
}

#[tokio::test]
async fn test_deduplication() {
    let (service, _temp_dir) = create_test_service().await;
    let ctx = create_test_context("tenant-1", "ns-1");

    let data = b"Duplicate content".to_vec();

    // Upload first time
    let metadata1 = service
        .upload_blob(
            &ctx,
            "file1.txt",
            data.clone(),
            None,
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    // Upload same content with different name
    let metadata2 = service
        .upload_blob(
            &ctx,
            "file2.txt",
            data.clone(),
            None,
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    // Should return same blob (deduplication)
    assert_eq!(metadata1.blob_id, metadata2.blob_id);
    assert_eq!(metadata1.sha256, metadata2.sha256);
}

#[tokio::test]
async fn test_get_metadata() {
    let (service, _temp_dir) = create_test_service().await;
    let ctx = create_test_context("tenant-1", "ns-1");

    let metadata = service
        .upload_blob(
            &ctx,
            "test.txt",
            b"content".to_vec(),
            Some("text/plain".to_string()),
            Some("group1".to_string()),
            Some("ARTIFACTS".to_string()),
            {
                let mut m = std::collections::HashMap::new();
                m.insert("key1".to_string(), "value1".to_string());
                m
            },
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    let retrieved = service.get_metadata(&ctx, &metadata.blob_id).await.unwrap();
    assert_eq!(retrieved.blob_id, metadata.blob_id);
    assert_eq!(retrieved.content_type, "text/plain");
    assert_eq!(retrieved.blob_group, "group1");
    assert_eq!(retrieved.kind, "ARTIFACTS");
    assert_eq!(retrieved.metadata.get("key1"), Some(&"value1".to_string()));
}

#[tokio::test]
async fn test_list_blobs() {
    let (service, _temp_dir) = create_test_service().await;
    let ctx = create_test_context("tenant-1", "ns-1");

    // Upload multiple blobs
    for i in 1..=5 {
        service
            .upload_blob(
                &ctx,
                &format!("file{}.txt", i),
                format!("content{}", i).into_bytes(),
                None,
                if i % 2 == 0 {
                    Some("even".to_string())
                } else {
                    Some("odd".to_string())
                },
                None,
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                None,
            )
            .await
            .unwrap();
    }

    // List all
    let (blobs, total) = service
        .list_blobs(&ctx, &ListFilters::default(), 10, 1)
        .await
        .unwrap();

    assert_eq!(total, 5);
    assert_eq!(blobs.len(), 5);

    // Filter by blob_group
    let filters = ListFilters {
        blob_group: Some("even".to_string()),
        ..Default::default()
    };
    let (blobs, total) = service.list_blobs(&ctx, &filters, 10, 1).await.unwrap();

    assert_eq!(total, 2);
    assert_eq!(blobs.len(), 2);
}

#[tokio::test]
async fn test_delete_blob() {
    let (service, _temp_dir) = create_test_service().await;
    let ctx = create_test_context("tenant-1", "ns-1");

    let metadata = service
        .upload_blob(
            &ctx,
            "test.txt",
            b"content".to_vec(),
            None,
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    // Verify exists
    assert!(service.get_metadata(&ctx, &metadata.blob_id).await.is_ok());

    // Delete
    service.delete_blob(&ctx, &metadata.blob_id).await.unwrap();

    // Verify deleted
    assert!(service.get_metadata(&ctx, &metadata.blob_id).await.is_err());
    assert!(service
        .download_blob(&ctx, &metadata.blob_id)
        .await
        .is_err());
}

#[tokio::test]
async fn test_empty_data_error() {
    let (service, _temp_dir) = create_test_service().await;
    let ctx = create_test_context("tenant-1", "ns-1");

    let result = service
        .upload_blob(
            &ctx,
            "empty.txt",
            vec![],
            None,
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        plexspaces_blob::BlobError::InvalidInput(_)
    ));
}

#[tokio::test]
async fn test_not_found_error() {
    let (service, _temp_dir) = create_test_service().await;
    let ctx = create_test_context("tenant-1", "ns-1");

    assert!(service.get_metadata(&ctx, "nonexistent").await.is_err());
    assert!(service.download_blob(&ctx, "nonexistent").await.is_err());
    assert!(service.delete_blob(&ctx, "nonexistent").await.is_err());
}

#[tokio::test]
async fn test_expiration() {
    let (service, _temp_dir) = create_test_service().await;
    let ctx = create_test_context("tenant-1", "ns-1");

    use chrono::Duration;
    let metadata = service
        .upload_blob(
            &ctx,
            "test.txt",
            b"content".to_vec(),
            None,
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            Some(Duration::hours(1)),
        )
        .await
        .unwrap();

    assert!(metadata.expires_at.is_some());

    // Find expired (should be empty)
    let expired = service.find_expired(&ctx, 10).await.unwrap();
    assert!(expired.is_empty());
}

#[tokio::test]
async fn test_multi_tenancy_isolation() {
    let (service, _temp_dir) = create_test_service().await;
    let ctx1 = create_test_context("tenant-1", "ns-1");
    let ctx2 = create_test_context("tenant-2", "ns-1");

    // Upload to tenant-1
    let metadata1 = service
        .upload_blob(
            &ctx1,
            "file.txt",
            b"tenant1".to_vec(),
            None,
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    // Upload to tenant-2
    let metadata2 = service
        .upload_blob(
            &ctx2,
            "file.txt",
            b"tenant2".to_vec(),
            None,
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    // List tenant-1 blobs
    let (blobs, _) = service
        .list_blobs(&ctx1, &ListFilters::default(), 10, 1)
        .await
        .unwrap();

    // Should only see tenant-1 blob
    assert!(blobs.iter().any(|b| b.blob_id == metadata1.blob_id));
    assert!(!blobs.iter().any(|b| b.blob_id == metadata2.blob_id));

    // List tenant-2 blobs
    let (blobs, _) = service
        .list_blobs(&ctx2, &ListFilters::default(), 10, 1)
        .await
        .unwrap();

    // Should only see tenant-2 blob
    assert!(blobs.iter().any(|b| b.blob_id == metadata2.blob_id));
    assert!(!blobs.iter().any(|b| b.blob_id == metadata1.blob_id));

    // Try to access tenant-1 blob from tenant-2 context (should fail)
    assert!(service
        .get_metadata(&ctx2, &metadata1.blob_id)
        .await
        .is_err());
    assert!(service
        .download_blob(&ctx2, &metadata1.blob_id)
        .await
        .is_err());
}

// =============================================================================
// Tests merged from error_tests.rs (3 tests)
// =============================================================================

#[test]
fn test_error_from_str() {
    let err: BlobError = "test error".into();
    match err {
        BlobError::InternalError(msg) => assert_eq!(msg, "test error"),
        _ => panic!("Expected InternalError"),
    }
}

#[test]
fn test_error_from_string() {
    let err: BlobError = "test error".to_string().into();
    match err {
        BlobError::InternalError(msg) => assert_eq!(msg, "test error"),
        _ => panic!("Expected InternalError"),
    }
}

#[test]
fn test_error_display() {
    let err = BlobError::ConfigError("invalid config".to_string());
    assert!(format!("{}", err).contains("invalid config"));

    let err = BlobError::NotFound("blob-1".to_string());
    assert!(format!("{}", err).contains("blob-1"));

    let err = BlobError::InvalidInput("empty data".to_string());
    assert!(format!("{}", err).contains("empty data"));
}

// =============================================================================
// Tests for stale metadata cleanup (blob exists in DB but not in object store)
// =============================================================================

/// Test that stale metadata (blob in DB but not in object store) is detected
/// and cleaned up during deduplication, allowing fresh upload to proceed.
///
/// This simulates the scenario where:
/// 1. A blob is uploaded and exists in both DB and object store
/// 2. The object store is cleared (e.g., MinIO restart) but DB metadata remains
/// 3. A new upload with the same content should detect the stale metadata,
///    clean it up, and proceed with a fresh upload
#[tokio::test]
async fn test_stale_metadata_cleanup_during_deduplication() {
    let (service, temp_dir) = create_test_service().await;
    let ctx = create_test_context("tenant-1", "ns-1");
    let data = b"Content for stale metadata test".to_vec();

    // Step 1: Upload a blob
    let metadata1 = service
        .upload_blob(
            &ctx,
            "stale_test.txt",
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

    let original_blob_id = metadata1.blob_id.clone();

    // Verify blob exists and can be downloaded
    let downloaded = service
        .download_blob(&ctx, &original_blob_id)
        .await
        .unwrap();
    assert_eq!(downloaded, data);

    // Step 2: Delete the blob file from object store DIRECTLY (simulating MinIO restart)
    // The metadata still exists in the database (stale state)
    let storage_path = format!(
        "/plexspaces/{}/{}/{}",
        metadata1.tenant_id, metadata1.namespace, metadata1.blob_id
    );
    let full_path = temp_dir.path().join(storage_path.trim_start_matches('/'));
    if full_path.exists() {
        std::fs::remove_file(&full_path).expect("Failed to delete blob file");
    }

    // Verify blob file is gone but metadata still exists
    let metadata_exists = service.get_metadata(&ctx, &original_blob_id).await.is_ok();
    assert!(metadata_exists, "Metadata should still exist in DB");

    // Download should now fail (blob missing from object store)
    let download_result = service.download_blob(&ctx, &original_blob_id).await;
    assert!(
        download_result.is_err(),
        "Download should fail - blob missing from object store"
    );

    // Step 3: Upload same content again - should detect stale metadata and clean up
    let metadata2 = service
        .upload_blob(
            &ctx,
            "stale_test_new.txt",
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

    // Should get a NEW blob_id (not the stale one)
    assert_ne!(
        metadata2.blob_id, original_blob_id,
        "Should get new blob_id after stale metadata cleanup"
    );

    // Step 4: Verify the new blob works correctly
    let downloaded2 = service
        .download_blob(&ctx, &metadata2.blob_id)
        .await
        .unwrap();
    assert_eq!(downloaded2, data);

    // Step 5: Verify the stale metadata was cleaned up
    let stale_metadata_result = service.get_metadata(&ctx, &original_blob_id).await;
    assert!(
        stale_metadata_result.is_err(),
        "Stale metadata should have been cleaned up"
    );
}

/// Test download_blob_by_name and delete_blob_by_name work correctly
#[tokio::test]
async fn test_blob_operations_by_name() {
    let (service, _temp_dir) = create_test_service().await;
    let ctx = create_test_context("tenant-1", "ns-1");

    let data = b"Content for name-based test".to_vec();
    let blob_name = "assets/images/logo.png";

    // Upload blob
    let metadata = service
        .upload_blob(
            &ctx,
            blob_name,
            data.clone(),
            Some("image/png".to_string()),
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(metadata.name, blob_name);

    // Download by name
    let downloaded = service
        .download_blob_by_name(&ctx, blob_name)
        .await
        .unwrap();
    assert_eq!(downloaded, data);

    // Get metadata by name
    let found_metadata = service.get_metadata_by_name(&ctx, blob_name).await.unwrap();
    assert!(found_metadata.is_some());
    assert_eq!(found_metadata.unwrap().blob_id, metadata.blob_id);

    // Delete by name
    service.delete_blob_by_name(&ctx, blob_name).await.unwrap();

    // Verify deleted
    let deleted_metadata = service.get_metadata_by_name(&ctx, blob_name).await.unwrap();
    assert!(deleted_metadata.is_none());
}
