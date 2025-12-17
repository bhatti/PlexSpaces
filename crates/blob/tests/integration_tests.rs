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
use plexspaces_core::RequestContext;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Get MinIO endpoint (checks which port is available)
async fn get_minio_endpoint() -> Option<String> {
    use reqwest::Client;
    
    let client = match Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };
    
    // Try port 9001 first, then 9000
    let ports = ["9001", "9000"];
    for port in &ports {
        let url = format!("http://localhost:{}/minio/health/live", port);
        match timeout(Duration::from_secs(2), client.get(&url).send()).await {
            Ok(Ok(resp)) if resp.status().is_success() => {
                return Some(format!("http://localhost:{}", port));
            }
            _ => continue,
        }
    }
    None
}

async fn create_test_service() -> Option<Arc<BlobService>> {
    let endpoint = match get_minio_endpoint().await {
        Some(e) => e,
        None => {
            eprintln!("⚠️  MinIO not available at http://localhost:9001 or http://localhost:9000. Skipping integration tests.");
            eprintln!("   To run these tests, start MinIO with:");
            eprintln!("   docker run -p 9001:9000 -p 9002:9001 -e MINIO_ROOT_USER=minioadmin_user -e MINIO_ROOT_PASSWORD=minioadmin_pass minio/minio server /data --console-address \":9002\"");
            return None;
        }
    };

    // Create SQLite repository using AnyPool (like the node does)
    // Use sqlite::memory: format (not sqlite://:memory:)
    use sqlx::AnyPool;
    let any_pool = AnyPool::connect("sqlite::memory:").await.ok()?;
    SqlBlobRepository::migrate(&any_pool).await.ok()?;
    let repository = Arc::new(SqlBlobRepository::new(any_pool));

    // Create blob config for MinIO
    let config = ProtoBlobConfig {
        backend: "minio".to_string(),
        bucket: "plexspaces-test".to_string(),
        endpoint: endpoint.clone(),
        region: String::new(),
        access_key_id: "minioadmin_user".to_string(),
        secret_access_key: "minioadmin_pass".to_string(),
        use_ssl: false,
        prefix: "/plexspaces".to_string(),
        gcp_service_account_json: String::new(),
        azure_account_name: String::new(),
        azure_account_key: String::new(),
    };

    eprintln!("Using MinIO endpoint: {}", endpoint);
    BlobService::new(config, repository).await.ok().map(Arc::new)
}

fn create_test_context(tenant_id: &str, namespace: &str) -> RequestContext {
    RequestContext::new(tenant_id.to_string())
        .with_namespace(namespace.to_string())
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

    // Download
    let downloaded = service.download_blob(&ctx, &metadata.blob_id).await.unwrap();
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
async fn test_list_and_delete() {
    let service = match create_test_service().await {
        Some(s) => s,
        None => {
            println!("Skipping test - MinIO not available");
            return;
        }
    };

    let ctx = create_test_context("tenant-1", "ns-1");
    // Upload multiple blobs
    for i in 1..=3 {
        service
            .upload_blob(
                &ctx,
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
        .list_blobs(&ctx, &filters, 10, 1)
        .await
        .unwrap();

    assert!(total >= 3);
    assert!(!blobs.is_empty());

    // Delete a blob
    let blob_id = &blobs[0].blob_id;
    service.delete_blob(&ctx, blob_id).await.unwrap();

    // Verify deleted
    assert!(service.get_metadata(&ctx, blob_id).await.is_err());
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
}

#[cfg(feature = "presigned-urls")]
#[tokio::test]
async fn test_presigned_url_get() {
    let service = match create_test_service().await {
        Some(s) => s,
        None => {
            println!("Skipping test - MinIO not available");
            return;
        }
    };

    let ctx = create_test_context("tenant-1", "ns-1");
    // Upload a blob first
    let data = b"Test content for presigned URL".to_vec();
    let metadata = service
        .upload_blob(
            &ctx,
            "presigned-test.txt",
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

    // Generate presigned URL for GET
    use chrono::Duration;
    let presigned_url = service
        .generate_presigned_url(&ctx, &metadata.blob_id, "GET", Duration::hours(1))
        .await
        .unwrap();

    // Verify URL is not empty and contains expected components
    assert!(!presigned_url.is_empty());
    assert!(presigned_url.contains("http://") || presigned_url.contains("https://"));
    
    // Try to download using presigned URL
    use reqwest::Client;
    let client = Client::new();
    let response = client.get(&presigned_url).send().await.unwrap();
    assert!(response.status().is_success());
    
    let downloaded_data = response.bytes().await.unwrap();
    assert_eq!(downloaded_data.as_ref(), data.as_slice());
}

#[cfg(feature = "presigned-urls")]
#[tokio::test]
async fn test_presigned_url_put() {
    let service = match create_test_service().await {
        Some(s) => s,
        None => {
            println!("Skipping test - MinIO not available");
            return;
        }
    };

    let ctx = create_test_context("tenant-1", "ns-1");
    // Upload a blob first to get a valid blob_id and path
    let data = b"Initial content".to_vec();
    let metadata = service
        .upload_blob(
            &ctx,
            "presigned-put-test.txt",
            data,
            Some("text/plain".to_string()),
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    // Generate presigned URL for PUT
    use chrono::Duration;
    let presigned_url = service
        .generate_presigned_url(&ctx, &metadata.blob_id, "PUT", Duration::hours(1))
        .await
        .unwrap();

    // Verify URL is not empty
    assert!(!presigned_url.is_empty());
    assert!(presigned_url.contains("http://") || presigned_url.contains("https://"));
    
    // Try to upload using presigned URL
    use reqwest::Client;
    let client = Client::new();
    let new_data = b"Updated content via presigned URL";
    let response = client
        .put(&presigned_url)
        .body(new_data.to_vec())
        .send()
        .await
        .unwrap();
    
    // PUT should succeed (200 or 204)
    assert!(response.status().is_success() || response.status().as_u16() == 200 || response.status().as_u16() == 204);
}

#[cfg(feature = "presigned-urls")]
#[tokio::test]
async fn test_presigned_url_expiration() {
    let service = match create_test_service().await {
        Some(s) => s,
        None => {
            println!("Skipping test - MinIO not available");
            return;
        }
    };

    let ctx = create_test_context("tenant-1", "ns-1");
    // Upload a blob first
    let data = b"Test expiration".to_vec();
    let metadata = service
        .upload_blob(
            &ctx,
            "expiration-test.txt",
            data,
            Some("text/plain".to_string()),
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    // Generate presigned URL with very short expiration (1 second)
    use chrono::Duration;
    let presigned_url = service
        .generate_presigned_url(&ctx, &metadata.blob_id, "GET", Duration::seconds(1))
        .await
        .unwrap();

    // Wait for expiration
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Try to use expired URL - should fail
    use reqwest::Client;
    let client = Client::new();
    let response = client.get(&presigned_url).send().await.unwrap();
    // Expired URLs typically return 403 Forbidden
    assert!(!response.status().is_success());
}

#[cfg(feature = "presigned-urls")]
#[tokio::test]
async fn test_presigned_url_invalid_operation() {
    let service = match create_test_service().await {
        Some(s) => s,
        None => {
            println!("Skipping test - MinIO not available");
            return;
        }
    };

    let ctx = create_test_context("tenant-1", "ns-1");
    // Upload a blob first
    let data = b"Test invalid operation".to_vec();
    let metadata = service
        .upload_blob(
            &ctx,
            "invalid-op-test.txt",
            data,
            Some("text/plain".to_string()),
            None,
            None,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            None,
        )
        .await
        .unwrap();

    // Try invalid operation
    use chrono::Duration;
    let result = service
        .generate_presigned_url(&ctx, &metadata.blob_id, "DELETE", Duration::hours(1))
        .await;

    // Should fail with invalid operation error
    assert!(result.is_err());
}
