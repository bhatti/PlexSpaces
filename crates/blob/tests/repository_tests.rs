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
    use chrono::Utc;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::sync::Arc;

    async fn create_test_repository() -> Arc<SqlBlobRepository> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        
        let any_pool: sqlx::Pool<sqlx::Any> = pool.into();
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

    #[tokio::test]
    async fn test_save_and_get() {
        let repo = create_test_repository().await;
        let metadata = create_test_metadata("blob-1", "tenant-1", "ns-1");

        repo.save(&metadata).await.unwrap();
        let retrieved = repo.get("blob-1").await.unwrap().unwrap();

        assert_eq!(retrieved.blob_id, "blob-1");
        assert_eq!(retrieved.tenant_id, "tenant-1");
        assert_eq!(retrieved.namespace, "ns-1");
    }

    #[tokio::test]
    async fn test_get_by_sha256() {
        let repo = create_test_repository().await;
        let sha256 = "b".repeat(64);
        let mut metadata = create_test_metadata("blob-1", "tenant-1", "ns-1");
        metadata.sha256 = sha256.clone();

        repo.save(&metadata).await.unwrap();
        let retrieved = repo.get_by_sha256("tenant-1", "ns-1", &sha256).await.unwrap().unwrap();

        assert_eq!(retrieved.blob_id, "blob-1");
        assert_eq!(retrieved.sha256, sha256);
    }

    #[tokio::test]
    async fn test_list_with_filters() {
        let repo = create_test_repository().await;
        
        let mut metadata1 = create_test_metadata("blob-1", "tenant-1", "ns-1");
        metadata1.name = "file1.txt".to_string();
        metadata1.blob_group = "group1".to_string();
        repo.save(&metadata1).await.unwrap();

        let mut metadata2 = create_test_metadata("blob-2", "tenant-1", "ns-1");
        metadata2.name = "file2.txt".to_string();
        metadata2.blob_group = "group2".to_string();
        repo.save(&metadata2).await.unwrap();

        let filters = ListFilters {
            blob_group: Some("group1".to_string()),
            ..Default::default()
        };
        let (results, total) = repo.list("tenant-1", "ns-1", &filters, 10, 0).await.unwrap();

        assert_eq!(total, 1);
        assert_eq!(results[0].blob_id, "blob-1");
    }

    #[tokio::test]
    async fn test_find_expired() {
        let repo = create_test_repository().await;
        
        let mut metadata = create_test_metadata("blob-1", "tenant-1", "ns-1");
        metadata.expires_at = Some(datetime_to_timestamp(Utc::now() - chrono::Duration::hours(1)));
        repo.save(&metadata).await.unwrap();

        let expired = repo.find_expired(None, 10).await.unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].blob_id, "blob-1");
    }

    #[tokio::test]
    async fn test_delete() {
        let repo = create_test_repository().await;
        let metadata = create_test_metadata("blob-1", "tenant-1", "ns-1");

        repo.save(&metadata).await.unwrap();
        assert!(repo.get("blob-1").await.unwrap().is_some());

        repo.delete("blob-1").await.unwrap();
        assert!(repo.get("blob-1").await.unwrap().is_none());
    }
}
