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
        use sqlx::{sqlite::SqlitePoolOptions, AnyPool};
        use tempfile::NamedTempFile;
        
        // Workaround for sqlx 0.7 AnyPool driver registration issue in tests
        // AnyPool requires the sqlite driver to be registered at runtime, which
        // doesn't happen in test environment. We use a file-based database as a workaround.
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap();
        
        // Try AnyPool first (works in production)
        let any_pool = match AnyPool::connect(&format!("sqlite:{}", db_path)).await {
            Ok(pool) => pool,
            Err(_) => {
                // Fallback: Use SqlitePool and convert (if possible)
                // Note: This is a workaround - ideally AnyPool would work in tests
                let sqlite_pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect(&format!("sqlite:{}", db_path))
                    .await
                    .unwrap();
                // Unfortunately, we can't convert SqlitePool to AnyPool directly
                // So we'll just panic with a helpful message
                panic!("AnyPool driver not available in test environment. This is a known sqlx 0.7 limitation. Consider using integration tests or a different test setup.");
            }
        };
        
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
        RequestContext::new(tenant_id.to_string())
            .with_namespace(namespace.to_string())
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
}
