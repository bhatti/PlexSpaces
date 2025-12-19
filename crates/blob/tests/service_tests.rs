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

use plexspaces_blob::{BlobService, BlobRepository, repository::sql::SqlBlobRepository, repository::ListFilters};
use plexspaces_proto::storage::v1::BlobConfig as ProtoBlobConfig;
use plexspaces_core::RequestContext;
use object_store::local::LocalFileSystem;
use std::sync::Arc;
use tempfile::{TempDir, NamedTempFile};

async fn create_test_service() -> (Arc<BlobService>, TempDir) {
    // Create temp directory for local filesystem
    let temp_dir = TempDir::new().unwrap();
    let local_store = Arc::new(LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap());

    // Create SQLite repository - use in-memory to prevent concurrency issues
    use sqlx::AnyPool;
    let any_pool = AnyPool::connect("sqlite::memory:").await.unwrap();
    SqlBlobRepository::migrate(&any_pool).await.unwrap();
    let repository = Arc::new(SqlBlobRepository::new(any_pool));

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
    RequestContext::new(tenant_id.to_string())
        .with_namespace(namespace.to_string())
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
    let downloaded = service.download_blob(&ctx, &metadata.blob_id).await.unwrap();
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
    let (blobs, total) = service
        .list_blobs(&ctx, &filters, 10, 1)
        .await
        .unwrap();

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
    assert!(service.download_blob(&ctx, &metadata.blob_id).await.is_err());
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
    assert!(matches!(result.unwrap_err(), plexspaces_blob::BlobError::InvalidInput(_)));
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
    assert!(service.get_metadata(&ctx2, &metadata1.blob_id).await.is_err());
    assert!(service.download_blob(&ctx2, &metadata1.blob_id).await.is_err());
}
