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

//! Integration tests for blob service with MinIO
//!
//! These tests require MinIO to be running. If MinIO is not available,
//! tests will print a warning and skip.

use plexspaces_blob::{BlobService, repository::sql::SqlBlobRepository, repository::ListFilters};
use plexspaces_proto::storage::v1::BlobConfig as ProtoBlobConfig;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Check if MinIO is available
async fn check_minio_available() -> bool {
    use reqwest::Client;
    
    let client = match Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    
    // Try to connect to MinIO
    match timeout(Duration::from_secs(2), client.get("http://localhost:9000/minio/health/live").send()).await {
        Ok(Ok(resp)) => resp.status().is_success(),
        _ => false,
    }
}

async fn create_test_service() -> Option<Arc<BlobService>> {
    if !check_minio_available().await {
        eprintln!("⚠️  MinIO not available at http://localhost:9000. Skipping integration tests.");
        eprintln!("   To run these tests, start MinIO with:");
        eprintln!("   docker run -p 9000:9000 -p 9001:9001 minio/minio server /data --console-address \":9001\"");
        return None;
    }

    // Create SQLite repository
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .ok()?;
    
    let any_pool: sqlx::Pool<sqlx::Any> = pool.into();
    SqlBlobRepository::migrate(&any_pool).await.ok()?;
    let repository = Arc::new(SqlBlobRepository::new(any_pool));

    // Create blob config for MinIO
    let config = ProtoBlobConfig {
        backend: "minio".to_string(),
        bucket: "plexspaces-test".to_string(),
        endpoint: "http://localhost:9000".to_string(),
        region: String::new(),
        access_key_id: "minioadmin_user".to_string(),
        secret_access_key: "minioadmin_pass".to_string(),
        use_ssl: false,
        prefix: "/plexspaces".to_string(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    BlobService::new(config, repository).await.ok().map(Arc::new)
}

#[tokio::test]
async fn test_upload_and_download_blob() {
    let service = match create_test_service().await {
        Some(s) => s,
        None => {
            println!("Skipping test - MinIO not available");
            return;
        }
    };

    let data = b"Hello, World!".to_vec();
    let metadata = service
        .upload_blob(
            "tenant-1",
            "ns-1",
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

    // Download
    let downloaded = service.download_blob(&metadata.blob_id).await.unwrap();
    assert_eq!(downloaded, data);
}

#[tokio::test]
async fn test_deduplication() {
    let service = match create_test_service().await {
        Some(s) => s,
        None => {
            println!("Skipping test - MinIO not available");
            return;
        }
    };

    let data = b"Duplicate content".to_vec();

    // Upload first time
    let metadata1 = service
        .upload_blob(
            "tenant-1",
            "ns-1",
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
            "tenant-1",
            "ns-1",
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
async fn test_list_and_delete() {
    let service = match create_test_service().await {
        Some(s) => s,
        None => {
            println!("Skipping test - MinIO not available");
            return;
        }
    };

    // Upload multiple blobs
    for i in 1..=3 {
        service
            .upload_blob(
                "tenant-1",
                "ns-1",
                &format!("file{}.txt", i),
                format!("content{}", i).into_bytes(),
                None,
                None,
                None,
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                None,
            )
            .await
            .unwrap();
    }

    // List blobs
    let filters = ListFilters::default();
    let (blobs, total) = service
        .list_blobs("tenant-1", "ns-1", &filters, 10, 1)
        .await
        .unwrap();

    assert!(total >= 3);
    assert!(!blobs.is_empty());

    // Delete a blob
    let blob_id = &blobs[0].blob_id;
    service.delete_blob(blob_id).await.unwrap();

    // Verify deleted
    assert!(service.get_metadata(blob_id).await.is_err());
}

#[tokio::test]
async fn test_multi_tenancy_isolation() {
    let service = match create_test_service().await {
        Some(s) => s,
        None => {
            println!("Skipping test - MinIO not available");
            return;
        }
    };

    // Upload to tenant-1
    let metadata1 = service
        .upload_blob(
            "tenant-1",
            "ns-1",
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
            "tenant-2",
            "ns-1",
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
        .list_blobs("tenant-1", "ns-1", &ListFilters::default(), 10, 1)
        .await
        .unwrap();

    // Should only see tenant-1 blob
    assert!(blobs.iter().any(|b| b.blob_id == metadata1.blob_id));
    assert!(!blobs.iter().any(|b| b.blob_id == metadata2.blob_id));
}
