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

//! Repository tests for blob metadata storage

#[cfg(feature = "sql-backend")]
mod sql_tests {
    use plexspaces_blob::{BlobRepository, repository::sql::SqlBlobRepository, repository::ListFilters};
    use plexspaces_proto::storage::v1::BlobMetadata;
    use plexspaces_blob::helpers::datetime_to_timestamp;
    use plexspaces_core::RequestContext;
    use chrono::Utc;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    async fn create_test_repository() -> Arc<SqlBlobRepository> {
        use sqlx::AnyPool;
        
        // Use in-memory database to prevent concurrency issues
        let any_pool = AnyPool::connect("sqlite::memory:").await.unwrap();
        
        SqlBlobRepository::migrate(&any_pool).await.unwrap();
        Arc::new(SqlBlobRepository::new(any_pool))
    }

    fn create_test_metadata(blob_id: &str, tenant_id: &str, namespace: &str) -> BlobMetadata {
        let now = Utc::now();
        BlobMetadata {
            blob_id: blob_id.to_string(),
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
            name: "test.txt".to_string(),
            sha256: "a".repeat(64),
            content_type: "text/plain".to_string(),
            content_length: 100,
            etag: String::new(),
            blob_group: String::new(),
            kind: String::new(),
            metadata: std::collections::HashMap::new(),
            tags: std::collections::HashMap::new(),
            expires_at: None,
            created_at: Some(datetime_to_timestamp(now)),
            updated_at: Some(datetime_to_timestamp(now)),
        }
    }

    fn create_test_context(tenant_id: &str, namespace: &str) -> RequestContext {
        RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string())
    }

    #[tokio::test]
    async fn test_save_and_get() {
        let repo = create_test_repository().await;
        let ctx = create_test_context("tenant-1", "ns-1");
        let metadata = create_test_metadata("blob-1", "tenant-1", "ns-1");

        repo.save(&ctx, &metadata).await.unwrap();
        let retrieved = repo.get(&ctx, "blob-1").await.unwrap().unwrap();

        assert_eq!(retrieved.blob_id, "blob-1");
        assert_eq!(retrieved.tenant_id, "tenant-1");
        assert_eq!(retrieved.namespace, "ns-1");
    }

    #[tokio::test]
    async fn test_get_by_sha256() {
        let repo = create_test_repository().await;
        let ctx = create_test_context("tenant-1", "ns-1");
        let sha256 = "b".repeat(64);
        let mut metadata = create_test_metadata("blob-1", "tenant-1", "ns-1");
        metadata.sha256 = sha256.clone();

        repo.save(&ctx, &metadata).await.unwrap();
        let retrieved = repo.get_by_sha256(&ctx, &sha256).await.unwrap().unwrap();

        assert_eq!(retrieved.blob_id, "blob-1");
        assert_eq!(retrieved.sha256, sha256);
    }

    #[tokio::test]
    async fn test_list_with_filters() {
        let repo = create_test_repository().await;
        let ctx = create_test_context("tenant-1", "ns-1");
        
        let mut metadata1 = create_test_metadata("blob-1", "tenant-1", "ns-1");
        metadata1.name = "file1.txt".to_string();
        metadata1.blob_group = "group1".to_string();
        repo.save(&ctx, &metadata1).await.unwrap();

        let mut metadata2 = create_test_metadata("blob-2", "tenant-1", "ns-1");
        metadata2.name = "file2.txt".to_string();
        metadata2.blob_group = "group2".to_string();
        repo.save(&ctx, &metadata2).await.unwrap();

        let filters = ListFilters {
            blob_group: Some("group1".to_string()),
            ..Default::default()
        };
        let (results, total) = repo.list(&ctx, &filters, 10, 0).await.unwrap();

        assert_eq!(total, 1);
        assert_eq!(results[0].blob_id, "blob-1");
    }

    #[tokio::test]
    async fn test_find_expired() {
        let repo = create_test_repository().await;
        let ctx = create_test_context("tenant-1", "ns-1");
        
        let mut metadata = create_test_metadata("blob-1", "tenant-1", "ns-1");
        metadata.expires_at = Some(datetime_to_timestamp(Utc::now() - chrono::Duration::hours(1)));
        repo.save(&ctx, &metadata).await.unwrap();

        let expired = repo.find_expired(&ctx, 10).await.unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].blob_id, "blob-1");
    }

    #[tokio::test]
    async fn test_delete() {
        let repo = create_test_repository().await;
        let ctx = create_test_context("tenant-1", "ns-1");
        let metadata = create_test_metadata("blob-1", "tenant-1", "ns-1");

        repo.save(&ctx, &metadata).await.unwrap();
        assert!(repo.get(&ctx, "blob-1").await.unwrap().is_some());

        repo.delete(&ctx, "blob-1").await.unwrap();
        assert!(repo.get(&ctx, "blob-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_tenant_isolation() {
        let repo = create_test_repository().await;
        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-2", "ns-1");
        
        let metadata = create_test_metadata("blob-1", "tenant-1", "ns-1");
        repo.save(&ctx1, &metadata).await.unwrap();

        // Should be able to get from tenant-1
        assert!(repo.get(&ctx1, "blob-1").await.unwrap().is_some());

        // Should NOT be able to get from tenant-2 (tenant isolation)
        assert!(repo.get(&ctx2, "blob-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_tenant_validation_on_save() {
        let repo = create_test_repository().await;
        let ctx = create_test_context("tenant-1", "ns-1");
        
        // Try to save metadata with mismatched tenant_id
        let metadata = create_test_metadata("blob-1", "tenant-2", "ns-1");
        let result = repo.save(&ctx, &metadata).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("tenant_id"));
    }

    #[tokio::test]
    async fn test_namespace_validation_on_save() {
        let repo = create_test_repository().await;
        let ctx = create_test_context("tenant-1", "ns-1");
        
        // Try to save metadata with mismatched namespace
        let metadata = create_test_metadata("blob-1", "tenant-1", "ns-2");
        let result = repo.save(&ctx, &metadata).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("namespace"));
    }

    #[tokio::test]
    async fn test_namespace_isolation() {
        let repo = create_test_repository().await;
        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-1", "ns-2");
        
        let metadata = create_test_metadata("blob-1", "tenant-1", "ns-1");
        repo.save(&ctx1, &metadata).await.unwrap();

        // Should be able to get from ns-1
        assert!(repo.get(&ctx1, "blob-1").await.unwrap().is_some());

        // Should NOT be able to get from ns-2 (namespace isolation)
        assert!(repo.get(&ctx2, "blob-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_tenant_and_namespace_isolation_list() {
        let repo = create_test_repository().await;
        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-2", "ns-1");
        let ctx3 = create_test_context("tenant-1", "ns-2");
        
        // Create blobs in different tenants/namespaces
        let metadata1 = create_test_metadata("blob-1", "tenant-1", "ns-1");
        repo.save(&ctx1, &metadata1).await.unwrap();
        
        let metadata2 = create_test_metadata("blob-2", "tenant-2", "ns-1");
        repo.save(&ctx2, &metadata2).await.unwrap();
        
        let metadata3 = create_test_metadata("blob-3", "tenant-1", "ns-2");
        repo.save(&ctx3, &metadata3).await.unwrap();

        // Each context should only see its own blobs
        let (results1, total1) = repo.list(&ctx1, &ListFilters::default(), 10, 0).await.unwrap();
        assert_eq!(total1, 1);
        assert_eq!(results1[0].blob_id, "blob-1");

        let (results2, total2) = repo.list(&ctx2, &ListFilters::default(), 10, 0).await.unwrap();
        assert_eq!(total2, 1);
        assert_eq!(results2[0].blob_id, "blob-2");

        let (results3, total3) = repo.list(&ctx3, &ListFilters::default(), 10, 0).await.unwrap();
        assert_eq!(total3, 1);
        assert_eq!(results3[0].blob_id, "blob-3");
    }

    #[tokio::test]
    async fn test_empty_namespace_list_returns_all_namespaces() {
        let repo = create_test_repository().await;
        let ctx_ns1 = create_test_context("tenant-1", "ns-1");
        let ctx_ns2 = create_test_context("tenant-1", "ns-2");
        let ctx_empty = create_test_context("tenant-1", "");

        // Create blobs in different namespaces
        let metadata1 = create_test_metadata("blob-1", "tenant-1", "ns-1");
        repo.save(&ctx_ns1, &metadata1).await.unwrap();

        let metadata2 = create_test_metadata("blob-2", "tenant-1", "ns-2");
        repo.save(&ctx_ns2, &metadata2).await.unwrap();

        // List with empty namespace should return blobs from all namespaces
        let (results, total) = repo.list(&ctx_empty, &ListFilters::default(), 10, 0).await.unwrap();
        assert_eq!(total, 2);
        assert!(results.iter().any(|b| b.blob_id == "blob-1"));
        assert!(results.iter().any(|b| b.blob_id == "blob-2"));

        // List with specific namespace should only return that namespace's blobs
        let (results_ns1, total_ns1) = repo.list(&ctx_ns1, &ListFilters::default(), 10, 0).await.unwrap();
        assert_eq!(total_ns1, 1);
        assert_eq!(results_ns1[0].blob_id, "blob-1");

        let (results_ns2, total_ns2) = repo.list(&ctx_ns2, &ListFilters::default(), 10, 0).await.unwrap();
        assert_eq!(total_ns2, 1);
        assert_eq!(results_ns2[0].blob_id, "blob-2");
    }

    #[tokio::test]
    async fn test_empty_namespace_get_by_sha256_returns_any_namespace() {
        let repo = create_test_repository().await;
        let ctx_ns1 = create_test_context("tenant-1", "ns-1");
        let ctx_ns2 = create_test_context("tenant-1", "ns-2");
        let ctx_empty = create_test_context("tenant-1", "");

        let sha256 = "c".repeat(64);
        
        // Create blob in ns-1 with specific sha256
        let mut metadata1 = create_test_metadata("blob-1", "tenant-1", "ns-1");
        metadata1.sha256 = sha256.clone();
        repo.save(&ctx_ns1, &metadata1).await.unwrap();

        // Create blob in ns-2 with different sha256
        let mut metadata2 = create_test_metadata("blob-2", "tenant-1", "ns-2");
        metadata2.sha256 = "d".repeat(64);
        repo.save(&ctx_ns2, &metadata2).await.unwrap();

        // get_by_sha256 with empty namespace should find blob from any namespace
        let retrieved = repo.get_by_sha256(&ctx_empty, &sha256).await.unwrap().unwrap();
        assert_eq!(retrieved.blob_id, "blob-1");
        assert_eq!(retrieved.namespace, "ns-1");

        // get_by_sha256 with specific namespace should only find in that namespace
        let retrieved_ns1 = repo.get_by_sha256(&ctx_ns1, &sha256).await.unwrap().unwrap();
        assert_eq!(retrieved_ns1.blob_id, "blob-1");

        // get_by_sha256 with different namespace should not find it
        let retrieved_ns2 = repo.get_by_sha256(&ctx_ns2, &sha256).await.unwrap();
        assert!(retrieved_ns2.is_none());
    }

    #[tokio::test]
    async fn test_empty_namespace_find_expired_returns_all_namespaces() {
        let repo = create_test_repository().await;
        let ctx_ns1 = create_test_context("tenant-1", "ns-1");
        let ctx_ns2 = create_test_context("tenant-1", "ns-2");
        let ctx_empty = create_test_context("tenant-1", "");

        // Create expired blob in ns-1
        let mut metadata1 = create_test_metadata("blob-1", "tenant-1", "ns-1");
        metadata1.expires_at = Some(datetime_to_timestamp(Utc::now() - chrono::Duration::hours(1)));
        repo.save(&ctx_ns1, &metadata1).await.unwrap();

        // Create expired blob in ns-2
        let mut metadata2 = create_test_metadata("blob-2", "tenant-1", "ns-2");
        metadata2.expires_at = Some(datetime_to_timestamp(Utc::now() - chrono::Duration::hours(1)));
        repo.save(&ctx_ns2, &metadata2).await.unwrap();

        // find_expired with empty namespace should return expired blobs from all namespaces
        let expired = repo.find_expired(&ctx_empty, 10).await.unwrap();
        assert_eq!(expired.len(), 2);
        assert!(expired.iter().any(|b| b.blob_id == "blob-1"));
        assert!(expired.iter().any(|b| b.blob_id == "blob-2"));

        // find_expired with specific namespace should only return that namespace's expired blobs
        let expired_ns1 = repo.find_expired(&ctx_ns1, 10).await.unwrap();
        assert_eq!(expired_ns1.len(), 1);
        assert_eq!(expired_ns1[0].blob_id, "blob-1");

        let expired_ns2 = repo.find_expired(&ctx_ns2, 10).await.unwrap();
        assert_eq!(expired_ns2.len(), 1);
        assert_eq!(expired_ns2[0].blob_id, "blob-2");
    }
}
