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

//! Integration tests for DynamoDB BlobRepository backend.
//!
//! ## TDD Approach
//! These tests are written FIRST before implementation (RED phase).

#[cfg(feature = "ddb-backend")]
mod ddb_tests {
    use plexspaces_blob::{BlobRepository, DynamoDBBlobRepository, ListFilters};
    use plexspaces_core::RequestContext;
    use plexspaces_proto::storage::v1::BlobMetadata;
    use prost_types::Timestamp;

    /// Helper to create DynamoDB repository for testing
    async fn create_ddb_repository() -> DynamoDBBlobRepository {
        let endpoint = std::env::var("DYNAMODB_ENDPOINT_URL")
            .or_else(|_| std::env::var("PLEXSPACES_DDB_ENDPOINT_URL"))
            .unwrap_or_else(|_| "http://localhost:8000".to_string());
        
        DynamoDBBlobRepository::new(
            "us-east-1".to_string(),
            "plexspaces-blob-test".to_string(),
            Some(endpoint),
        )
        .await
        .expect("Failed to create DynamoDB blob repository")
    }

    /// Helper to create test RequestContext with unique tenant IDs for test isolation
    fn tenant1_ctx() -> RequestContext {
        use ulid::Ulid;
        let unique_id = Ulid::new().to_string();
        RequestContext::new_without_auth(format!("tenant1-{}", unique_id), "default".to_string())
    }

    fn tenant2_ctx() -> RequestContext {
        use ulid::Ulid;
        let unique_id = Ulid::new().to_string();
        RequestContext::new_without_auth(format!("tenant2-{}", unique_id), "default".to_string())
    }

    fn create_test_metadata(ctx: &RequestContext, blob_id: &str, sha256: &str) -> BlobMetadata {
        BlobMetadata {
            blob_id: blob_id.to_string(),
            tenant_id: ctx.tenant_id().to_string(),
            namespace: ctx.namespace().to_string(),
            name: format!("file-{}.txt", blob_id),
            sha256: sha256.to_string(),
            content_type: "text/plain".to_string(),
            content_length: 100,
            etag: "etag-1".to_string(),
            blob_group: String::new(),
            kind: String::new(),
            metadata: std::collections::HashMap::new(),
            tags: std::collections::HashMap::new(),
            expires_at: None,
            created_at: Some(Timestamp {
                seconds: 1000,
                nanos: 0,
            }),
            updated_at: Some(Timestamp {
                seconds: 1000,
                nanos: 0,
            }),
        }
    }

    // =========================================================================
    // Core Operations Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_save_and_get() {
        let repo = create_ddb_repository().await;
        let ctx = tenant1_ctx();

        let metadata = create_test_metadata(&ctx, "blob-1", "sha256-hash-1");
        repo.save(&ctx, &metadata).await.unwrap();

        let retrieved = repo.get(&ctx, "blob-1").await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.blob_id, "blob-1");
        assert_eq!(retrieved.sha256, "sha256-hash-1");
    }

    #[tokio::test]
    async fn test_ddb_get_nonexistent() {
        let repo = create_ddb_repository().await;
        let ctx = tenant1_ctx();

        let retrieved = repo.get(&ctx, "nonexistent").await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_ddb_get_by_sha256() {
        let repo = create_ddb_repository().await;
        let ctx = tenant1_ctx();

        let metadata = create_test_metadata(&ctx, "blob-1", "sha256-hash-1");
        repo.save(&ctx, &metadata).await.unwrap();

        let retrieved = repo.get_by_sha256(&ctx, "sha256-hash-1").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().blob_id, "blob-1");
    }

    #[tokio::test]
    async fn test_ddb_update() {
        let repo = create_ddb_repository().await;
        let ctx = tenant1_ctx();

        let mut metadata = create_test_metadata(&ctx, "blob-1", "sha256-hash-1");
        repo.save(&ctx, &metadata).await.unwrap();

        metadata.content_length = 200;
        repo.update(&ctx, &metadata).await.unwrap();

        let retrieved = repo.get(&ctx, "blob-1").await.unwrap().unwrap();
        assert_eq!(retrieved.content_length, 200);
    }

    #[tokio::test]
    async fn test_ddb_delete() {
        let repo = create_ddb_repository().await;
        let ctx = tenant1_ctx();

        let metadata = create_test_metadata(&ctx, "blob-1", "sha256-hash-1");
        repo.save(&ctx, &metadata).await.unwrap();

        repo.delete(&ctx, "blob-1").await.unwrap();

        let retrieved = repo.get(&ctx, "blob-1").await.unwrap();
        assert!(retrieved.is_none());
    }

    // =========================================================================
    // List Operations Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_list() {
        let repo = create_ddb_repository().await;
        let ctx = tenant1_ctx();

        for i in 1..=5 {
            let metadata = create_test_metadata(&ctx, &format!("blob-{}", i), &format!("sha256-{}", i));
            repo.save(&ctx, &metadata).await.unwrap();
        }

        let (results, total) = repo.list(&ctx, &ListFilters::default(), 10, 0).await.unwrap();
        assert_eq!(results.len(), 5);
        assert_eq!(total, 5);
    }

    #[tokio::test]
    async fn test_ddb_list_with_name_prefix() {
        let repo = create_ddb_repository().await;
        let ctx = tenant1_ctx();

        let mut metadata1 = create_test_metadata(&ctx, "blob-1", "sha256-1");
        metadata1.name = "prefix-file1.txt".to_string();
        repo.save(&ctx, &metadata1).await.unwrap();

        let mut metadata2 = create_test_metadata(&ctx, "blob-2", "sha256-2");
        metadata2.name = "other-file2.txt".to_string();
        repo.save(&ctx, &metadata2).await.unwrap();

        let filters = ListFilters {
            name_prefix: Some("prefix-".to_string()),
            ..Default::default()
        };
        let (results, _) = repo.list(&ctx, &filters, 10, 0).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "prefix-file1.txt");
    }

    #[tokio::test]
    async fn test_ddb_list_with_blob_group() {
        let repo = create_ddb_repository().await;
        let ctx = tenant1_ctx();

        let mut metadata1 = create_test_metadata(&ctx, "blob-1", "sha256-1");
        metadata1.blob_group = "group1".to_string();
        repo.save(&ctx, &metadata1).await.unwrap();

        let mut metadata2 = create_test_metadata(&ctx, "blob-2", "sha256-2");
        metadata2.blob_group = "group2".to_string();
        repo.save(&ctx, &metadata2).await.unwrap();

        let filters = ListFilters {
            blob_group: Some("group1".to_string()),
            ..Default::default()
        };
        let (results, _) = repo.list(&ctx, &filters, 10, 0).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].blob_group, "group1".to_string());
    }

    #[tokio::test]
    async fn test_ddb_find_expired() {
        let repo = create_ddb_repository().await;
        let ctx = tenant1_ctx();

        let mut metadata1 = create_test_metadata(&ctx, "blob-1", "sha256-1");
        metadata1.expires_at = Some(Timestamp {
            seconds: 100, // Expired (in the past)
            nanos: 0,
        });
        repo.save(&ctx, &metadata1).await.unwrap();

        let mut metadata2 = create_test_metadata(&ctx, "blob-2", "sha256-2");
        metadata2.expires_at = Some(Timestamp {
            seconds: chrono::Utc::now().timestamp() + 3600, // Not expired (1 hour from now)
            nanos: 0,
        });
        repo.save(&ctx, &metadata2).await.unwrap();

        let expired = repo.find_expired(&ctx, 10).await.unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].blob_id, "blob-1");
    }

    // =========================================================================
    // Tenant Isolation Tests (CRITICAL FOR SECURITY)
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_tenant_isolation() {
        let repo = create_ddb_repository().await;
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        let metadata1 = create_test_metadata(&ctx1, "blob-1", "sha256-1");
        repo.save(&ctx1, &metadata1).await.unwrap();

        let metadata2 = create_test_metadata(&ctx2, "blob-1", "sha256-2"); // Same blob_id, different tenant
        repo.save(&ctx2, &metadata2).await.unwrap();

        // Each tenant should see their own blob
        let retrieved1 = repo.get(&ctx1, "blob-1").await.unwrap();
        assert!(retrieved1.is_some());
        assert_eq!(retrieved1.unwrap().sha256, "sha256-1");

        let retrieved2 = repo.get(&ctx2, "blob-1").await.unwrap();
        assert!(retrieved2.is_some());
        assert_eq!(retrieved2.unwrap().sha256, "sha256-2");

        // Tenant1 should not see tenant2's blob when listing
        let (results, _) = repo.list(&ctx1, &ListFilters::default(), 10, 0).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].sha256, "sha256-1");
    }

    #[tokio::test]
    async fn test_ddb_tenant_isolation_delete() {
        let repo = create_ddb_repository().await;
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        let metadata1 = create_test_metadata(&ctx1, "blob-1", "sha256-1");
        repo.save(&ctx1, &metadata1).await.unwrap();

        let metadata2 = create_test_metadata(&ctx2, "blob-1", "sha256-2");
        repo.save(&ctx2, &metadata2).await.unwrap();

        // Delete from tenant1
        repo.delete(&ctx1, "blob-1").await.unwrap();

        // Tenant1's blob should be gone
        assert!(repo.get(&ctx1, "blob-1").await.unwrap().is_none());

        // Tenant2's blob should still exist
        assert!(repo.get(&ctx2, "blob-1").await.unwrap().is_some());
    }
}

