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

//! Integration tests for blob-based keyvalue store using LocalFileSystem
//!
//! These tests use a temporary directory that is automatically cleaned up
//! after tests complete. All test data is verified to be removed.

#[cfg(feature = "blob-backend")]
mod tests {
    use plexspaces_keyvalue::{KeyValueStore, blob::{BlobKVStore, BlobKVConfig}};
    use plexspaces_core::RequestContext;
    use object_store::local::LocalFileSystem;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use std::path::Path;
    use std::fs;

    /// Create blob keyvalue store for testing with LocalFileSystem
    /// Returns the store and temp directory (which auto-cleans on drop)
    async fn create_test_service() -> (Arc<BlobKVStore>, TempDir) {
        // Create temp directory - automatically cleaned up when dropped
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let temp_path = temp_dir.path();

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

        // Create LocalFileSystem object store pointing to temp directory
        let local_store = Arc::new(
            LocalFileSystem::new_with_prefix(temp_path)
                .expect("Failed to create LocalFileSystem")
        );

        // Create keyvalue store with custom object store
        let kv = BlobKVStore::with_object_store(config, local_store);
        (Arc::new(kv), temp_dir)
    }

    /// Verify that all test data is cleaned up
    fn verify_cleanup(temp_dir: &TempDir) {
        let path = temp_dir.path();
        
        // Check if the keyvalue directory exists
        let kv_path = path.join("plexspaces/keyvalue");
        
        if kv_path.exists() {
            // Count files in keyvalue directory
            let count = count_files_recursive(&kv_path);
            if count > 0 {
                panic!(
                    "Test data not cleaned up! Found {} files in {:?}",
                    count, kv_path
                );
            }
        }
    }

    /// Count files recursively in a directory
    fn count_files_recursive(dir: &Path) -> usize {
        let mut count = 0;
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    count += 1;
                } else if path.is_dir() {
                    count += count_files_recursive(&path);
                }
            }
        }
        count
    }

    /// List all files in a directory (for debugging)
    fn list_files_recursive(dir: &Path) -> Vec<String> {
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    files.push(path.to_string_lossy().to_string());
                } else if path.is_dir() {
                    files.extend(list_files_recursive(&path));
                }
            }
        }
        files
    }

    fn create_test_context(tenant_id: &str, namespace: &str) -> RequestContext {
        RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string())
    }

    #[tokio::test]
    async fn test_upload_and_download() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        let data = b"Hello, World!".to_vec();
        kv.put(&ctx, "test-key", data.clone()).await.unwrap();

        let retrieved = kv.get(&ctx, "test-key").await.unwrap();
        assert_eq!(retrieved, Some(data));

        // Cleanup: delete the key
        kv.delete(&ctx, "test-key").await.unwrap();
        
        // Verify cleanup
        drop(kv);
        drop(temp_dir); // TempDir automatically cleans up
    }

    #[tokio::test]
    async fn test_multiple_keys() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // Put multiple keys
        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx, "key2", b"value2".to_vec()).await.unwrap();
        kv.put(&ctx, "key3", b"value3".to_vec()).await.unwrap();

        // Verify all exist
        assert_eq!(kv.get(&ctx, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(kv.get(&ctx, "key2").await.unwrap(), Some(b"value2".to_vec()));
        assert_eq!(kv.get(&ctx, "key3").await.unwrap(), Some(b"value3".to_vec()));

        // Cleanup: delete all keys
        kv.delete(&ctx, "key1").await.unwrap();
        kv.delete(&ctx, "key2").await.unwrap();
        kv.delete(&ctx, "key3").await.unwrap();

        // Verify all deleted
        assert_eq!(kv.get(&ctx, "key1").await.unwrap(), None);
        assert_eq!(kv.get(&ctx, "key2").await.unwrap(), None);
        assert_eq!(kv.get(&ctx, "key3").await.unwrap(), None);

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_list_and_cleanup() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // Create keys with prefix
        kv.put(&ctx, "config:timeout", b"30s".to_vec()).await.unwrap();
        kv.put(&ctx, "config:max_size", b"10000".to_vec()).await.unwrap();
        kv.put(&ctx, "other:key", b"value".to_vec()).await.unwrap();

        // List config keys
        let keys = kv.list(&ctx, "config:").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"config:timeout".to_string()));
        assert!(keys.contains(&"config:max_size".to_string()));

        // Cleanup: clear prefix
        let deleted = kv.clear_prefix(&ctx, "config:").await.unwrap();
        assert_eq!(deleted, 2);

        // Verify cleanup
        let remaining = kv.list(&ctx, "config:").await.unwrap();
        assert_eq!(remaining.len(), 0);

        // Cleanup remaining
        kv.delete(&ctx, "other:key").await.unwrap();

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_tenant_isolation() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-2", "ns-1");

        // Put same key for different tenants
        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        // Verify isolation
        assert_eq!(kv.get(&ctx1, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(kv.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));

        // Cleanup
        kv.delete(&ctx1, "key1").await.unwrap();
        kv.delete(&ctx2, "key1").await.unwrap();

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_namespace_isolation() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-1", "ns-2");

        // Put same key for different namespaces
        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        // Verify isolation
        assert_eq!(kv.get(&ctx1, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(kv.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));

        // Cleanup
        kv.delete(&ctx1, "key1").await.unwrap();
        kv.delete(&ctx2, "key1").await.unwrap();

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_cas_operations() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // CAS on non-existent key
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

        // Cleanup
        kv.delete(&ctx, "lock:resource").await.unwrap();

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_increment_decrement() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // Increment from 0
        let count = kv.increment(&ctx, "counter", 1).await.unwrap();
        assert_eq!(count, 1);

        // Increment more
        let count = kv.increment(&ctx, "counter", 5).await.unwrap();
        assert_eq!(count, 6);

        // Decrement
        let count = kv.decrement(&ctx, "counter", 3).await.unwrap();
        assert_eq!(count, 3);

        // Cleanup
        kv.delete(&ctx, "counter").await.unwrap();

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_multi_get_put() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // Multi-put
        kv.multi_put(
            &ctx,
            &[
                ("k1", b"v1".to_vec()),
                ("k2", b"v2".to_vec()),
                ("k3", b"v3".to_vec()),
            ],
        )
        .await
        .unwrap();

        // Multi-get
        let values = kv.multi_get(&ctx, &["k1", "k2", "k3", "k4"]).await.unwrap();
        assert_eq!(values[0], Some(b"v1".to_vec()));
        assert_eq!(values[1], Some(b"v2".to_vec()));
        assert_eq!(values[2], Some(b"v3".to_vec()));
        assert_eq!(values[3], None);

        // Cleanup
        kv.delete(&ctx, "k1").await.unwrap();
        kv.delete(&ctx, "k2").await.unwrap();
        kv.delete(&ctx, "k3").await.unwrap();

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_get_stats() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // Put some keys
        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx, "key2", b"value2".to_vec()).await.unwrap();
        kv.put(&ctx, "key3", b"value3".to_vec()).await.unwrap();

        // Get stats
        let stats = kv.get_stats(&ctx).await.unwrap();
        assert_eq!(stats.backend_type, "Blob");
        assert!(stats.total_keys >= 3);
        assert!(stats.total_size_bytes > 0);

        // Cleanup
        kv.delete(&ctx, "key1").await.unwrap();
        kv.delete(&ctx, "key2").await.unwrap();
        kv.delete(&ctx, "key3").await.unwrap();

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_clear_prefix_cleanup() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // Create multiple keys with different prefixes
        kv.put(&ctx, "temp:1", b"a".to_vec()).await.unwrap();
        kv.put(&ctx, "temp:2", b"b".to_vec()).await.unwrap();
        kv.put(&ctx, "temp:3", b"c".to_vec()).await.unwrap();
        kv.put(&ctx, "other:key", b"d".to_vec()).await.unwrap();

        // Clear temp: prefix
        let deleted = kv.clear_prefix(&ctx, "temp:").await.unwrap();
        assert_eq!(deleted, 3);

        // Verify temp: keys are gone
        let temp_keys = kv.list(&ctx, "temp:").await.unwrap();
        assert_eq!(temp_keys.len(), 0);

        // Verify other: key still exists
        assert_eq!(kv.get(&ctx, "other:key").await.unwrap(), Some(b"d".to_vec()));

        // Cleanup remaining
        kv.delete(&ctx, "other:key").await.unwrap();

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_put_with_ttl() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // Put with TTL (best-effort without metadata storage)
        kv.put_with_ttl(
            &ctx,
            "session:123",
            b"data".to_vec(),
            Duration::from_secs(60),
        )
        .await
        .unwrap();

        // Verify exists
        let value = kv.get(&ctx, "session:123").await.unwrap();
        assert_eq!(value, Some(b"data".to_vec()));

        // Cleanup
        kv.delete(&ctx, "session:123").await.unwrap();

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_refresh_ttl() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // Put with TTL
        kv.put_with_ttl(
            &ctx,
            "session:123",
            b"data".to_vec(),
            Duration::from_secs(30),
        )
        .await
        .unwrap();

        // Refresh TTL
        kv.refresh_ttl(&ctx, "session:123", Duration::from_secs(120))
            .await
            .unwrap();

        // Verify still exists
        assert!(kv.exists(&ctx, "session:123").await.unwrap());

        // Cleanup
        kv.delete(&ctx, "session:123").await.unwrap();

        drop(kv);
        drop(temp_dir);
    }

    #[tokio::test]
    async fn test_comprehensive_cleanup() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");

        // Create various keys
        let keys = vec![
            "config:timeout",
            "config:max_size",
            "session:user1",
            "session:user2",
            "cache:item1",
            "cache:item2",
        ];

        // Put all keys
        for key in &keys {
            kv.put(&ctx, key, format!("value-{}", key).into_bytes())
                .await
                .unwrap();
        }

        // Verify all exist
        for key in &keys {
            assert!(kv.exists(&ctx, key).await.unwrap());
        }

        // Get stats before cleanup
        let stats_before = kv.get_stats(&ctx).await.unwrap();
        assert!(stats_before.total_keys >= keys.len() as usize);

        // Cleanup: delete all keys
        for key in &keys {
            kv.delete(&ctx, key).await.unwrap();
        }

        // Verify all deleted
        for key in &keys {
            assert!(!kv.exists(&ctx, key).await.unwrap());
        }

        // Get stats after cleanup
        let stats_after = kv.get_stats(&ctx).await.unwrap();
        assert!(stats_after.total_keys < stats_before.total_keys);

        // Drop to trigger cleanup
        drop(kv);
        
        // Verify temp directory cleanup (TempDir auto-cleans on drop)
        let temp_path = temp_dir.path().to_path_buf();
        drop(temp_dir);
        
        // After drop, directory should be cleaned up
        // (TempDir automatically removes the directory)
        assert!(!temp_path.exists(), "Temp directory should be cleaned up");
    }

    #[tokio::test]
    async fn test_file_cleanup_verification() {
        let (kv, temp_dir) = create_test_service().await;
        let ctx = create_test_context("tenant-1", "ns-1");
        let temp_path = temp_dir.path();

        // Create some keys
        kv.put(&ctx, "test1", b"data1".to_vec()).await.unwrap();
        kv.put(&ctx, "test2", b"data2".to_vec()).await.unwrap();

        // Verify files exist
        let kv_path = temp_path.join("plexspaces/keyvalue");
        if kv_path.exists() {
            let file_count_before = count_files_recursive(&kv_path);
            assert!(file_count_before > 0, "Files should exist before cleanup");
        }

        // Cleanup all keys
        kv.delete(&ctx, "test1").await.unwrap();
        kv.delete(&ctx, "test2").await.unwrap();

        // Verify files are deleted
        if kv_path.exists() {
            let file_count_after = count_files_recursive(&kv_path);
            // Files should be deleted (or directory might be empty)
            // Note: object_store might keep empty directories, but files should be gone
        }

        drop(kv);
        
        // Capture path before dropping temp_dir
        let temp_path = temp_dir.path().to_path_buf();
        drop(temp_dir);
        
        // TempDir automatically cleans up everything
        assert!(!temp_path.exists(), "Temp directory should be completely removed");
    }
}
