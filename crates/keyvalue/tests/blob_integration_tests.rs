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

//! Integration tests for blob-based keyvalue store with MinIO
//!
//! These tests require MinIO to be running. If MinIO is not available,
//! tests will print a warning and skip.

#[cfg(feature = "blob-backend")]
mod tests {
    use plexspaces_keyvalue::{KeyValueStore, blob::{BlobKVStore, BlobKVConfig}};
    use plexspaces_core::RequestContext;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::timeout;

    /// Get MinIO endpoint (checks which port is available)
    async fn get_minio_endpoint() -> Option<String> {
        use reqwest::Client;
        
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .ok()?;

        // Try port 9000 first (default MinIO API)
        let endpoints = vec![
            "http://localhost:9000",
            "http://localhost:9001",
        ];

        for endpoint in endpoints {
            let url = format!("{}/minio/health/live", endpoint);
            if client.get(&url).send().await.is_ok() {
                return Some(endpoint.to_string());
            }
        }

        None
    }

    /// Create blob keyvalue store for testing with MinIO
    async fn create_test_service() -> Option<Arc<BlobKVStore>> {
        let endpoint = get_minio_endpoint().await?;

        // Create blob config for MinIO (uses object_store directly, no SQL needed)
        let config = BlobKVConfig {
            prefix: "/plexspaces".to_string(),
            backend: "minio".to_string(),
            bucket: "plexspaces-test".to_string(),
            endpoint: Some(endpoint.clone()),
            region: None,
            access_key_id: Some("minioadmin_user".to_string()),
            secret_access_key: Some("minioadmin_pass".to_string()),
            use_ssl: false,
            gcp_service_account_json: None,
            azure_account_name: None,
            azure_account_key: None,
        };

        eprintln!("Using MinIO endpoint: {}", endpoint);
        let kv = BlobKVStore::new(config).await.ok()?;
        Some(Arc::new(kv))
    }

    fn create_test_context(tenant_id: &str, namespace: &str) -> RequestContext {
        RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string())
    }

    #[tokio::test]
    async fn test_upload_and_download() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx = create_test_context("tenant-1", "ns-1");
        let data = b"Hello, World!".to_vec();
        
        kv.put(&ctx, "test-key", data.clone()).await.unwrap();

        let retrieved = kv.get(&ctx, "test-key").await.unwrap();
        assert_eq!(retrieved, Some(data));
    }

    #[tokio::test]
    async fn test_put_overwrites_existing() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx = create_test_context("tenant-1", "ns-1");
        
        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx, "key1", b"value2".to_vec()).await.unwrap();

        let retrieved = kv.get(&ctx, "key1").await.unwrap();
        assert_eq!(retrieved, Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_delete() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx = create_test_context("tenant-1", "ns-1");
        
        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        assert!(kv.exists(&ctx, "key1").await.unwrap());
        
        kv.delete(&ctx, "key1").await.unwrap();
        assert!(!kv.exists(&ctx, "key1").await.unwrap());
    }

    #[tokio::test]
    async fn test_list_with_prefix() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

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
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx = create_test_context("tenant-1", "ns-1");
        
        kv.put(&ctx, "k1", b"v1".to_vec()).await.unwrap();
        kv.put(&ctx, "k2", b"v2".to_vec()).await.unwrap();

        let values = kv.multi_get(&ctx, &["k1", "k2", "k3"]).await.unwrap();
        assert_eq!(values[0], Some(b"v1".to_vec()));
        assert_eq!(values[1], Some(b"v2".to_vec()));
        assert_eq!(values[2], None);
    }

    #[tokio::test]
    async fn test_ttl_operations() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx = create_test_context("tenant-1", "ns-1");
        
        // Put with TTL
        kv.put_with_ttl(&ctx, "session:123", b"data".to_vec(), Duration::from_secs(60))
            .await
            .unwrap();

        // Verify it exists
        assert!(kv.exists(&ctx, "session:123").await.unwrap());

        // Get TTL
        let ttl = kv.get_ttl(&ctx, "session:123").await.unwrap();
        assert!(ttl.is_some());
        let ttl_secs = ttl.unwrap().as_secs();
        assert!(ttl_secs > 50 && ttl_secs <= 60);

        // Refresh TTL
        kv.refresh_ttl(&ctx, "session:123", Duration::from_secs(120))
            .await
            .unwrap();

        let ttl = kv.get_ttl(&ctx, "session:123").await.unwrap();
        assert!(ttl.is_some());
        let ttl_secs = ttl.unwrap().as_secs();
        assert!(ttl_secs > 115 && ttl_secs <= 120);
    }

    #[tokio::test]
    async fn test_cas_operation() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

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
    }

    #[tokio::test]
    async fn test_increment_decrement() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx = create_test_context("tenant-1", "ns-1");
        
        let count = kv.increment(&ctx, "counter", 1).await.unwrap();
        assert_eq!(count, 1);

        let count = kv.increment(&ctx, "counter", 5).await.unwrap();
        assert_eq!(count, 6);

        let count = kv.decrement(&ctx, "counter", 2).await.unwrap();
        assert_eq!(count, 4);
    }

    #[tokio::test]
    async fn test_tenant_isolation() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-2", "ns-1");

        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        assert_eq!(kv.get(&ctx1, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(kv.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_namespace_isolation() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-1", "ns-2");

        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        assert_eq!(kv.get(&ctx1, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(kv.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_clear_prefix() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

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
    async fn test_get_stats() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx = create_test_context("tenant-1", "ns-1");
        
        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx, "key2", b"value2".to_vec()).await.unwrap();

        let stats = kv.get_stats(&ctx).await.unwrap();
        assert_eq!(stats.backend_type, "Blob");
        assert!(stats.total_keys >= 2);
        assert!(stats.total_size_bytes > 0);
    }

    #[tokio::test]
    async fn test_large_value() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx = create_test_context("tenant-1", "ns-1");
        
        // Test with 1MB value
        let large_data = vec![0u8; 1024 * 1024];
        kv.put(&ctx, "large-key", large_data.clone()).await.unwrap();

        let retrieved = kv.get(&ctx, "large-key").await.unwrap();
        assert_eq!(retrieved, Some(large_data));
    }

    #[tokio::test]
    async fn test_concurrent_operations() {
        let kv = match create_test_service().await {
            Some(s) => s,
            None => {
                println!("Skipping test - MinIO not available");
                return;
            }
        };

        let ctx = create_test_context("tenant-1", "ns-1");
        
        // Concurrent puts
        let mut handles = Vec::new();
        for i in 0..10 {
            let kv_clone = Arc::clone(&kv);
            let ctx_clone = ctx.clone();
            handles.push(tokio::spawn(async move {
                kv_clone
                    .put(&ctx_clone, &format!("key{}", i), format!("value{}", i).into_bytes())
                    .await
            }));
        }

        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        // Verify all keys exist
        for i in 0..10 {
            let value = kv.get(&ctx, &format!("key{}", i)).await.unwrap();
            assert_eq!(value, Some(format!("value{}", i).into_bytes()));
        }
    }
}
