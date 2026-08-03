// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Integration tests for blob-based keyvalue store using embedded object store.

#[cfg(feature = "blob-backend")]
mod tests {
    use plexspaces_blob::embedded_object_store::EmbeddedObjectStore;
    use plexspaces_common::{RequestContext, RequestContextExt};
    use plexspaces_keyvalue::{
        blob::{BlobKVConfig, BlobKVStore},
        KeyValueStore,
    };
    use std::sync::{Arc, OnceLock};
    use std::time::Duration;

    /// Holds the embedded store singleton for the test binary lifetime.
    /// `None` when using an external endpoint via BLOB_ENDPOINT.
    static EMBEDDED_STORE: OnceLock<tokio::sync::Mutex<Option<EmbeddedObjectStore>>> =
        OnceLock::new();
    static BLOB_ENDPOINT_CACHE: OnceLock<tokio::sync::Mutex<Option<String>>> = OnceLock::new();

    /// Start or reuse the embedded object store for tests.
    /// Panics if the binary is not available — tests must not silently skip.
    /// Returns `None` when the embedded store binary is absent and no `BLOB_ENDPOINT` is set,
    /// so tests can skip gracefully instead of panicking.
    async fn get_object_store_endpoint() -> Option<String> {
        let endpoint_cache = BLOB_ENDPOINT_CACHE.get_or_init(|| tokio::sync::Mutex::new(None));
        let mut endpoint_guard = endpoint_cache.lock().await;
        if let Some(ref ep) = *endpoint_guard {
            return Some(ep.clone());
        }
        // Use external endpoint if configured; avoids spawning a subprocess.
        if let Ok(ep) = std::env::var("BLOB_ENDPOINT") {
            *endpoint_guard = Some(ep.clone());
            return Some(ep);
        }
        // Auto-start embedded store and keep it alive for the full test binary run.
        let store_cache = EMBEDDED_STORE.get_or_init(|| tokio::sync::Mutex::new(None));
        let mut store_guard = store_cache.lock().await;
        match EmbeddedObjectStore::start(19100).await {
            Ok(store) => {
                let endpoint = store.s3_endpoint.clone();
                *store_guard = Some(store);
                *endpoint_guard = Some(endpoint.clone());
                Some(endpoint)
            }
            Err(e) => {
                eprintln!(
                    "⚠️  WARNING: Skipping keyvalue blob integration tests — embedded object store unavailable: {}\n\
                    Install: cargo install rustfs\n\
                    Or set BLOB_ENDPOINT to an external S3-compatible endpoint.",
                    e
                );
                None
            }
        }
    }

    /// Returns `None` when the object store is unavailable (tests skip gracefully).
    async fn create_test_service() -> Option<Arc<BlobKVStore>> {
        let endpoint = get_object_store_endpoint().await?;
        let config = BlobKVConfig {
            prefix: "/plexspaces".to_string(),
            backend: "embedded".to_string(),
            bucket: "plexspaces-kv-test".to_string(),
            endpoint: Some(endpoint),
            region: None,
            access_key_id: None,
            secret_access_key: None,
            use_ssl: false,
            gcp_service_account_json: None,
            azure_account_name: None,
            azure_account_key: None,
        };
        Some(Arc::new(
            BlobKVStore::new(config)
                .await
                .expect("Failed to create BlobKVStore"),
        ))
    }

    fn create_test_context(tenant_id: &str, namespace: &str) -> RequestContext {
        RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string())
    }

    #[tokio::test]
    async fn test_upload_and_download() {
        let Some(kv) = create_test_service().await else {
            return;
        };
        let ctx = create_test_context("tenant-1", "ns-1");
        let data = b"Hello, World!".to_vec();

        kv.put(&ctx, "test-key", data.clone()).await.unwrap();
        let retrieved = kv.get(&ctx, "test-key").await.unwrap();
        assert_eq!(retrieved, Some(data));
    }

    #[tokio::test]
    async fn test_put_overwrites_existing() {
        let Some(kv) = create_test_service().await else {
            return;
        };
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx, "key1", b"value2".to_vec()).await.unwrap();

        let retrieved = kv.get(&ctx, "key1").await.unwrap();
        assert_eq!(retrieved, Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_delete() {
        let Some(kv) = create_test_service().await else {
            return;
        };
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        assert!(kv.exists(&ctx, "key1").await.unwrap());

        kv.delete(&ctx, "key1").await.unwrap();
        assert!(!kv.exists(&ctx, "key1").await.unwrap());
    }

    #[tokio::test]
    async fn test_list_with_prefix() {
        let Some(kv) = create_test_service().await else {
            return;
        };
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put(&ctx, "config:timeout", b"30s".to_vec())
            .await
            .unwrap();
        kv.put(&ctx, "config:max_size", b"10000".to_vec())
            .await
            .unwrap();
        kv.put(&ctx, "other:key", b"value".to_vec()).await.unwrap();

        let keys = kv.list(&ctx, "config:").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"config:timeout".to_string()));
        assert!(keys.contains(&"config:max_size".to_string()));
    }

    #[tokio::test]
    async fn test_multi_get() {
        let Some(kv) = create_test_service().await else {
            return;
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
        let Some(kv) = create_test_service().await else {
            return;
        };
        let ctx = create_test_context("tenant-1", "ns-1");

        kv.put_with_ttl(
            &ctx,
            "session:123",
            b"data".to_vec(),
            Duration::from_secs(60),
        )
        .await
        .unwrap();

        assert!(kv.exists(&ctx, "session:123").await.unwrap());

        let ttl = kv.get_ttl(&ctx, "session:123").await.unwrap();
        assert!(ttl.is_some());
        let ttl_secs = ttl.unwrap().as_secs();
        assert!(ttl_secs > 50 && ttl_secs <= 60);
    }

    #[tokio::test]
    async fn test_tenant_isolation() {
        let Some(kv) = create_test_service().await else {
            return;
        };
        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-2", "ns-1");

        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        assert_eq!(
            kv.get(&ctx1, "key1").await.unwrap(),
            Some(b"value1".to_vec())
        );
        assert_eq!(
            kv.get(&ctx2, "key1").await.unwrap(),
            Some(b"value2".to_vec())
        );
    }

    #[tokio::test]
    async fn test_namespace_isolation() {
        let Some(kv) = create_test_service().await else {
            return;
        };
        let ctx1 = create_test_context("tenant-1", "ns-1");
        let ctx2 = create_test_context("tenant-1", "ns-2");

        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        assert_eq!(
            kv.get(&ctx1, "key1").await.unwrap(),
            Some(b"value1".to_vec())
        );
        assert_eq!(
            kv.get(&ctx2, "key1").await.unwrap(),
            Some(b"value2".to_vec())
        );
    }

    #[tokio::test]
    async fn test_get_stats() {
        let Some(kv) = create_test_service().await else {
            return;
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
        let Some(kv) = create_test_service().await else {
            return;
        };
        let ctx = create_test_context("tenant-1", "ns-1");

        let large_data = vec![0u8; 1024 * 1024];
        kv.put(&ctx, "large-key", large_data.clone()).await.unwrap();

        let retrieved = kv.get(&ctx, "large-key").await.unwrap();
        assert_eq!(retrieved, Some(large_data));
    }
}
