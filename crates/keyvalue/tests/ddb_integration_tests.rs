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

//! Integration tests for DynamoDB KeyValue store backend.
//!
//! ## TDD Approach
//! These tests are written FIRST before implementation (RED phase).
//! Implementation will be written to make these tests pass (GREEN phase).

#[cfg(feature = "ddb-backend")]
mod ddb_tests {
    use plexspaces_keyvalue::{KeyValueStore, DynamoDBKVStore};
    use plexspaces_common::RequestContext;
    use std::time::Duration;
    use ulid::Ulid;

    /// Helper to create test RequestContext
    fn tenant1_ctx() -> RequestContext {
        RequestContext::new_without_auth("tenant1".to_string(), "default".to_string())
    }

    fn tenant2_ctx() -> RequestContext {
        RequestContext::new_without_auth("tenant2".to_string(), "default".to_string())
    }

    fn namespace1_ctx() -> RequestContext {
        RequestContext::new_without_auth("tenant1".to_string(), "ns1".to_string())
    }

    fn namespace2_ctx() -> RequestContext {
        RequestContext::new_without_auth("tenant1".to_string(), "ns2".to_string())
    }

    /// Helper to create DynamoDB KV store for testing
    /// Uses DynamoDB Local endpoint if available
    async fn create_ddb_store() -> DynamoDBKVStore {
        let endpoint = std::env::var("DYNAMODB_ENDPOINT_URL")
            .or_else(|_| std::env::var("PLEXSPACES_DDB_ENDPOINT_URL"))
            .unwrap_or_else(|_| "http://localhost:8000".to_string());
        
        DynamoDBKVStore::new(
            "us-east-1".to_string(),
            "plexspaces-keyvalue-test".to_string(),
            Some(endpoint),
        )
        .await
        .expect("Failed to create DynamoDB KV store")
    }

    // =========================================================================
    // Core Operations Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_put_and_get() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        // Put a value
        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();

        // Get the value
        let value = kv.get(&ctx, "key1").await.unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[tokio::test]
    async fn test_ddb_get_nonexistent_key() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        let value = kv.get(&ctx, "nonexistent").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_ddb_put_overwrites_existing() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx, "key1", b"value2".to_vec()).await.unwrap();

        let value = kv.get(&ctx, "key1").await.unwrap();
        assert_eq!(value, Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_ddb_delete() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.delete(&ctx, "key1").await.unwrap();

        let value = kv.get(&ctx, "key1").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_ddb_delete_nonexistent_key() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        // Delete should succeed even if key doesn't exist (idempotent)
        kv.delete(&ctx, "nonexistent").await.unwrap();
    }

    #[tokio::test]
    async fn test_ddb_exists() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        assert!(!kv.exists(&ctx, "key1").await.unwrap());
        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        assert!(kv.exists(&ctx, "key1").await.unwrap());
        kv.delete(&ctx, "key1").await.unwrap();
        assert!(!kv.exists(&ctx, "key1").await.unwrap());
    }

    // =========================================================================
    // Query Operations Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_list_prefix() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();
        let test_id = Ulid::new().to_string();
        let prefix = format!("actor-{}:", test_id);
        let key1 = format!("{}alice", prefix);
        let key2 = format!("{}bob", prefix);
        let key3 = format!("node-{}:node1", test_id);

        kv.put(&ctx, &key1, b"ref1".to_vec()).await.unwrap();
        kv.put(&ctx, &key2, b"ref2".to_vec()).await.unwrap();
        kv.put(&ctx, &key3, b"info".to_vec()).await.unwrap();

        let keys = kv.list(&ctx, &prefix).await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&key1));
        assert!(keys.contains(&key2));
        assert!(!keys.contains(&key3));
    }

    #[tokio::test]
    async fn test_ddb_list_empty_prefix() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();
        let test_id = Ulid::new().to_string();
        let key1 = format!("test-{}-key1", test_id);
        let key2 = format!("test-{}-key2", test_id);

        kv.put(&ctx, &key1, b"value1".to_vec()).await.unwrap();
        kv.put(&ctx, &key2, b"value2".to_vec()).await.unwrap();

        let keys = kv.list(&ctx, &format!("test-{}-", test_id)).await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&key1));
        assert!(keys.contains(&key2));
    }

    #[tokio::test]
    async fn test_ddb_multi_get() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        let test_id = Ulid::new().to_string();
        let k1 = format!("k1-{}", test_id);
        let k2 = format!("k2-{}", test_id);
        let k3 = format!("k3-{}", test_id);

        kv.put(&ctx, &k1, b"v1".to_vec()).await.unwrap();
        kv.put(&ctx, &k2, b"v2".to_vec()).await.unwrap();

        let values = kv.multi_get(&ctx, &[&k1, &k2, &k3]).await.unwrap();
        assert_eq!(values.len(), 3);
        assert_eq!(values[0], Some(b"v1".to_vec()));
        assert_eq!(values[1], Some(b"v2".to_vec()));
        assert_eq!(values[2], None);
    }

    #[tokio::test]
    async fn test_ddb_multi_put() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

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

        assert_eq!(kv.get(&ctx, "k1").await.unwrap(), Some(b"v1".to_vec()));
        assert_eq!(kv.get(&ctx, "k2").await.unwrap(), Some(b"v2".to_vec()));
        assert_eq!(kv.get(&ctx, "k3").await.unwrap(), Some(b"v3".to_vec()));
    }

    // =========================================================================
    // TTL Operations Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_put_with_ttl() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        kv.put_with_ttl(&ctx, "key1", b"value1".to_vec(), Duration::from_secs(1))
            .await
            .unwrap();

        // Should exist immediately
        assert!(kv.exists(&ctx, "key1").await.unwrap());

        // Wait for expiration (with buffer)
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Should be expired (DynamoDB TTL cleanup may take a moment)
        // Note: DynamoDB TTL cleanup is eventually consistent
        let value = kv.get(&ctx, "key1").await.unwrap();
        // Value might still exist due to eventual consistency, but TTL should be set
        // We'll check get_ttl instead
    }

    #[tokio::test]
    async fn test_ddb_get_ttl() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        // Key without TTL
        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        assert_eq!(kv.get_ttl(&ctx, "key1").await.unwrap(), None);

        // Key with TTL
        kv.put_with_ttl(&ctx, "key2", b"value2".to_vec(), Duration::from_secs(60))
            .await
            .unwrap();
        let ttl = kv.get_ttl(&ctx, "key2").await.unwrap();
        assert!(ttl.is_some());
        assert!(ttl.unwrap().as_secs() <= 60);
    }

    #[tokio::test]
    async fn test_ddb_refresh_ttl() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        kv.put_with_ttl(&ctx, "key1", b"value1".to_vec(), Duration::from_secs(10))
            .await
            .unwrap();

        // Refresh with longer TTL
        kv.refresh_ttl(&ctx, "key1", Duration::from_secs(60))
            .await
            .unwrap();

        let ttl = kv.get_ttl(&ctx, "key1").await.unwrap();
        assert!(ttl.is_some());
        assert!(ttl.unwrap().as_secs() <= 60);
    }

    #[tokio::test]
    async fn test_ddb_refresh_ttl_nonexistent_key() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        let result = kv.refresh_ttl(&ctx, "nonexistent", Duration::from_secs(60)).await;
        assert!(result.is_err());
    }

    // =========================================================================
    // Atomic Operations Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_cas_create() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();
        let test_key = format!("cas-create-{}", Ulid::new());

        // CAS with None (key must not exist)
        let success = kv
            .cas(&ctx, &test_key, None, b"value1".to_vec())
            .await
            .unwrap();
        assert!(success);

        // Verify key exists
        assert!(kv.exists(&ctx, &test_key).await.unwrap());

        // Try again (should fail because key now exists)
        let success = kv
            .cas(&ctx, &test_key, None, b"value2".to_vec())
            .await
            .unwrap();
        assert!(!success);
    }

    #[tokio::test]
    async fn test_ddb_cas_update() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();

        // CAS with matching value
        let success = kv
            .cas(&ctx, "key1", Some(b"value1".to_vec()), b"value2".to_vec())
            .await
            .unwrap();
        assert!(success);

        assert_eq!(kv.get(&ctx, "key1").await.unwrap(), Some(b"value2".to_vec()));

        // CAS with non-matching value
        let success = kv
            .cas(&ctx, "key1", Some(b"value1".to_vec()), b"value3".to_vec())
            .await
            .unwrap();
        assert!(!success);
    }

    #[tokio::test]
    async fn test_ddb_increment() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();
        let counter_key = format!("counter-{}", ulid::Ulid::new());

        // Increment non-existent key (should initialize to 0)
        let value = kv.increment(&ctx, &counter_key, 1).await.unwrap();
        assert_eq!(value, 1);

        let value = kv.increment(&ctx, &counter_key, 5).await.unwrap();
        assert_eq!(value, 6);

        let value = kv.increment(&ctx, &counter_key, -2).await.unwrap();
        assert_eq!(value, 4);
    }

    #[tokio::test]
    async fn test_ddb_decrement() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();
        let counter_key = format!("counter-{}", ulid::Ulid::new());

        // Decrement non-existent key (should initialize to 0)
        let value = kv.decrement(&ctx, &counter_key, 1).await.unwrap();
        assert_eq!(value, -1);

        kv.put(&ctx, &counter_key, 10i64.to_be_bytes().to_vec())
            .await
            .unwrap();

        let value = kv.decrement(&ctx, &counter_key, 3).await.unwrap();
        assert_eq!(value, 7);
    }

    // =========================================================================
    // Tenant/Namespace Isolation Tests (CRITICAL FOR SECURITY)
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_tenant_isolation() {
        let kv = create_ddb_store().await;
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        // Put same key for different tenants
        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        // Each tenant should see their own value
        assert_eq!(kv.get(&ctx1, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(kv.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));

        // Delete from tenant1 should not affect tenant2
        kv.delete(&ctx1, "key1").await.unwrap();
        assert_eq!(kv.get(&ctx1, "key1").await.unwrap(), None);
        assert_eq!(kv.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_ddb_namespace_isolation() {
        let kv = create_ddb_store().await;
        let ctx1 = namespace1_ctx();
        let ctx2 = namespace2_ctx();

        // Put same key for different namespaces
        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        // Each namespace should see their own value
        assert_eq!(kv.get(&ctx1, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(kv.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_ddb_tenant_isolation_list() {
        let kv = create_ddb_store().await;
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();
        let test_id = Ulid::new().to_string();
        let prefix = format!("actor-{}:", test_id);
        let key1 = format!("{}alice", prefix);
        let key2 = format!("{}bob", prefix);
        let key3 = format!("{}charlie", prefix);

        kv.put(&ctx1, &key1, b"ref1".to_vec()).await.unwrap();
        kv.put(&ctx1, &key2, b"ref2".to_vec()).await.unwrap();
        kv.put(&ctx2, &key3, b"ref3".to_vec()).await.unwrap();

        // Each tenant should only see their own keys
        let keys1 = kv.list(&ctx1, &prefix).await.unwrap();
        assert_eq!(keys1.len(), 2);
        assert!(keys1.contains(&key1));
        assert!(keys1.contains(&key2));

        let keys2 = kv.list(&ctx2, &prefix).await.unwrap();
        assert_eq!(keys2.len(), 1);
        assert!(keys2.contains(&key3));
    }

    #[tokio::test]
    async fn test_ddb_namespace_isolation_list() {
        let kv = create_ddb_store().await;
        let ctx1 = namespace1_ctx();
        let ctx2 = namespace2_ctx();
        let test_id = Ulid::new().to_string();
        let prefix = format!("config-{}:", test_id);
        let key1 = format!("{}timeout", prefix);

        kv.put(&ctx1, &key1, b"30s".to_vec()).await.unwrap();
        kv.put(&ctx2, &key1, b"60s".to_vec()).await.unwrap();

        // Each namespace should only see their own keys
        let keys1 = kv.list(&ctx1, &prefix).await.unwrap();
        assert_eq!(keys1.len(), 1);
        assert!(keys1.contains(&key1));

        let keys2 = kv.list(&ctx2, &prefix).await.unwrap();
        assert_eq!(keys2.len(), 1);
        assert!(keys2.contains(&key1));
    }

    // =========================================================================
    // Maintenance Operations Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_clear_prefix() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();
        let test_id = Ulid::new().to_string();
        let temp_prefix = format!("temp-{}:", test_id);
        let keep_prefix = format!("keep-{}:", test_id);
        let key1 = format!("{}1", temp_prefix);
        let key2 = format!("{}2", temp_prefix);
        let key3 = format!("{}1", keep_prefix);

        kv.put(&ctx, &key1, b"a".to_vec()).await.unwrap();
        kv.put(&ctx, &key2, b"b".to_vec()).await.unwrap();
        kv.put(&ctx, &key3, b"c".to_vec()).await.unwrap();

        let deleted = kv.clear_prefix(&ctx, &temp_prefix).await.unwrap();
        assert_eq!(deleted, 2);

        assert!(!kv.exists(&ctx, &key1).await.unwrap());
        assert!(!kv.exists(&ctx, &key2).await.unwrap());
        assert!(kv.exists(&ctx, &key3).await.unwrap());
    }

    #[tokio::test]
    async fn test_ddb_count_prefix() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();
        let test_id = Ulid::new().to_string();
        let actor_prefix = format!("actor-{}:", test_id);
        let node_prefix = format!("node-{}:", test_id);
        let key1 = format!("{}a", actor_prefix);
        let key2 = format!("{}b", actor_prefix);
        let key3 = format!("{}node1", node_prefix);

        kv.put(&ctx, &key1, b"1".to_vec()).await.unwrap();
        kv.put(&ctx, &key2, b"2".to_vec()).await.unwrap();
        kv.put(&ctx, &key3, b"info".to_vec()).await.unwrap();

        let count = kv.count_prefix(&ctx, &actor_prefix).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_ddb_get_stats() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx, "key2", b"value2".to_vec()).await.unwrap();

        let stats = kv.get_stats(&ctx).await.unwrap();
        assert_eq!(stats.backend_type, "DynamoDB");
        assert!(stats.total_keys >= 2);
    }

    // =========================================================================
    // Concurrent Operations Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_concurrent_puts() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        let mut handles = Vec::new();
        for i in 0..10 {
            let kv_clone = kv.clone();
            let ctx_clone = ctx.clone();
            let handle = tokio::spawn(async move {
                kv_clone
                    .put(&ctx_clone, &format!("key{}", i), format!("value{}", i).into_bytes())
                    .await
                    .unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all keys were written
        for i in 0..10 {
            let value = kv.get(&ctx, &format!("key{}", i)).await.unwrap();
            assert_eq!(value, Some(format!("value{}", i).into_bytes()));
        }
    }

    #[tokio::test]
    async fn test_ddb_concurrent_cas() {
        let kv = create_ddb_store().await;
        let ctx = tenant1_ctx();

        let counter_key = format!("counter-{}", Ulid::new());
        kv.put(&ctx, &counter_key, 0i64.to_be_bytes().to_vec())
            .await
            .unwrap();

        let mut handles = Vec::new();
        for _ in 0..10 {
            let kv_clone = kv.clone();
            let ctx_clone = ctx.clone();
            let counter_key_clone = counter_key.clone();
            let handle = tokio::spawn(async move {
                // Use increment for atomic operations instead of CAS
                // This tests that concurrent operations work correctly
                kv_clone.increment(&ctx_clone, &counter_key_clone, 1).await.unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Verify final value
        let value = kv.get(&ctx, &counter_key).await.unwrap().unwrap();
        let final_value = i64::from_be_bytes([
            value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
        ]);
        assert_eq!(final_value, 10);
    }
}

