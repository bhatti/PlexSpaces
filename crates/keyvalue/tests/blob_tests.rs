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

//! Unit tests for blob-based keyvalue store using local filesystem backend

#[cfg(feature = "blob-backend")]
mod tests {
    use plexspaces_keyvalue::{KeyValueStore, blob::{BlobKVStore, BlobKVConfig}};
    use plexspaces_core::RequestContext;
    use object_store::local::LocalFileSystem;
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn create_test_service() -> (Arc<BlobKVStore>, TempDir) {
        // Create temp directory for local filesystem
        let temp_dir = TempDir::new().unwrap();
        let local_store = Arc::new(LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap());

        // Create blob config for local filesystem
        let config = BlobKVConfig {
            prefix: "/plexspaces".to_string(),
            backend: "local".to_string(),
            bucket: "test".to_string(),
            endpoint: None,
            region: None,
            access_key_id: None,
            secret_access_key: None,
            use_ssl: false,
            gcp_service_account_json: None,
            azure_account_name: None,
            azure_account_key: None,
        };

        // Create keyvalue store with custom object store (for testing)
        let kv = BlobKVStore::with_object_store(config, local_store);
        (Arc::new(kv), temp_dir)
    }

    fn create_test_context(tenant_id: &str, namespace: &str) -> RequestContext {
        RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string())
    }

    #[tokio::test]
    async fn test_get_and_put() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        let data = b"Hello, World!".to_vec();
        kv.put(&ctx, "key1", data.clone()).await.unwrap();

        let retrieved = kv.get(&ctx, "key1").await.unwrap();
        assert_eq!(retrieved, Some(data));
    }

    #[tokio::test]
    async fn test_put_overwrites() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx, "key1", b"value2".to_vec()).await.unwrap();

        let retrieved = kv.get(&ctx, "key1").await.unwrap();
        assert_eq!(retrieved, Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        let retrieved = kv.get(&ctx, "nonexistent").await.unwrap();
        assert_eq!(retrieved, None);
    }

    #[tokio::test]
    async fn test_delete() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.delete(&ctx, "key1").await.unwrap();

        let retrieved = kv.get(&ctx, "key1").await.unwrap();
        assert_eq!(retrieved, None);
    }

    #[tokio::test]
    async fn test_delete_nonexistent() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // Should not error
        kv.delete(&ctx, "nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn test_exists() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        assert!(!kv.exists(&ctx, "key1").await.unwrap());
        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        assert!(kv.exists(&ctx, "key1").await.unwrap());
    }

    #[tokio::test]
    async fn test_list() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put(&ctx, "config:timeout", b"30s".to_vec()).await.unwrap();
        kv.put(&ctx, "config:max_size", b"10000".to_vec()).await.unwrap();
        kv.put(&ctx, "other:key", b"value".to_vec()).await.unwrap();

        let keys = kv.list(&ctx, "config:").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"config:timeout".to_string()));
        assert!(keys.contains(&"config:max_size".to_string()));
    }

    #[tokio::test]
    async fn test_multi_get() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put(&ctx, "k1", b"v1".to_vec()).await.unwrap();
        kv.put(&ctx, "k2", b"v2".to_vec()).await.unwrap();

        let values = kv.multi_get(&ctx, &["k1", "k2", "k3"]).await.unwrap();
        assert_eq!(values[0], Some(b"v1".to_vec()));
        assert_eq!(values[1], Some(b"v2".to_vec()));
        assert_eq!(values[2], None);
    }

    #[tokio::test]
    async fn test_multi_put() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.multi_put(
            &ctx,
            &[("k1", b"v1".to_vec()), ("k2", b"v2".to_vec())],
        )
        .await
        .unwrap();

        assert_eq!(kv.get(&ctx, "k1").await.unwrap(), Some(b"v1".to_vec()));
        assert_eq!(kv.get(&ctx, "k2").await.unwrap(), Some(b"v2".to_vec()));
    }

    #[tokio::test]
    async fn test_put_with_ttl() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // TTL is best-effort without metadata storage
        kv.put_with_ttl(&ctx, "session:123", b"data".to_vec(), std::time::Duration::from_secs(60))
            .await
            .unwrap();

        let value = kv.get(&ctx, "session:123").await.unwrap();
        assert_eq!(value, Some(b"data".to_vec()));
    }

    #[tokio::test]
    async fn test_refresh_ttl() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put_with_ttl(&ctx, "session:123", b"data".to_vec(), std::time::Duration::from_secs(30))
            .await
            .unwrap();

        // Refresh TTL (re-uploads to update last_modified)
        kv.refresh_ttl(&ctx, "session:123", std::time::Duration::from_secs(120))
            .await
            .unwrap();

        // Verify still exists
        assert!(kv.exists(&ctx, "session:123").await.unwrap());
    }

    #[tokio::test]
    async fn test_cas() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // CAS on non-existent key (expected = None)
        let acquired = kv
            .cas(&ctx, "lock:resource", None, b"node1".to_vec())
            .await
            .unwrap();
        assert!(acquired);

        // Try again (should fail)
        let acquired2 = kv
            .cas(&ctx, "lock:resource", None, b"node2".to_vec())
            .await
            .unwrap();
        assert!(!acquired2);

        // CAS with correct expected value
        let acquired3 = kv
            .cas(&ctx, "lock:resource", Some(b"node1".to_vec()), b"node2".to_vec())
            .await
            .unwrap();
        assert!(acquired3);
    }

    #[tokio::test]
    async fn test_increment() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        let count = kv.increment(&ctx, "counter", 1).await.unwrap();
        assert_eq!(count, 1);

        let count = kv.increment(&ctx, "counter", 5).await.unwrap();
        assert_eq!(count, 6);
    }

    #[tokio::test]
    async fn test_decrement() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.increment(&ctx, "counter", 10).await.unwrap();
        let count = kv.decrement(&ctx, "counter", 3).await.unwrap();
        assert_eq!(count, 7);
    }

    #[tokio::test]
    async fn test_clear_prefix() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put(&ctx, "temp:1", b"a".to_vec()).await.unwrap();
        kv.put(&ctx, "temp:2", b"b".to_vec()).await.unwrap();
        kv.put(&ctx, "other:key", b"c".to_vec()).await.unwrap();

        let deleted = kv.clear_prefix(&ctx, "temp:").await.unwrap();
        assert_eq!(deleted, 2);

        assert!(!kv.exists(&ctx, "temp:1").await.unwrap());
        assert!(!kv.exists(&ctx, "temp:2").await.unwrap());
        assert!(kv.exists(&ctx, "other:key").await.unwrap());
    }

    #[tokio::test]
    async fn test_count_prefix() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put(&ctx, "actor:a", b"1".to_vec()).await.unwrap();
        kv.put(&ctx, "actor:b", b"2".to_vec()).await.unwrap();

        let count = kv.count_prefix(&ctx, "actor:").await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_get_stats() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx, "key2", b"value2".to_vec()).await.unwrap();

        let stats = kv.get_stats(&ctx).await.unwrap();
        assert_eq!(stats.backend_type, "Blob");
        assert!(stats.total_keys >= 2);
        assert!(stats.total_size_bytes > 0);
    }

    #[tokio::test]
    async fn test_tenant_isolation() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-2", "ns-1");

        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        assert_eq!(kv.get(&ctx1, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(kv.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_namespace_isolation() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-1", "ns-2");

        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        assert_eq!(kv.get(&ctx1, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(kv.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_watch_not_supported() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        let result = kv.watch(&ctx, "key1").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not supported"));
    }

    #[tokio::test]
    async fn test_watch_prefix_not_supported() {
        let (kv, _temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        let result = kv.watch_prefix(&ctx, "prefix:").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not supported"));
    }
}
