// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for tenant and namespace isolation in KeyValueStore

#[cfg(feature = "sql-backend")]
mod sqlite_tests {
    use plexspaces_keyvalue::{KeyValueStore, SqliteKVStore};
    use plexspaces_core::RequestContext;
    use std::time::Duration;

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

    fn empty_namespace_ctx() -> RequestContext {
        RequestContext::new_without_auth("tenant1".to_string(), "".to_string())
    }

    #[tokio::test]
    async fn test_tenant_isolation_basic_operations() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
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
    async fn test_namespace_isolation_basic_operations() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
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
    async fn test_tenant_isolation_list_operations() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        // Put keys with same prefix for different tenants
        kv.put(&ctx1, "actor:alice", b"ref1".to_vec()).await.unwrap();
        kv.put(&ctx1, "actor:bob", b"ref2".to_vec()).await.unwrap();
        kv.put(&ctx2, "actor:charlie", b"ref3".to_vec()).await.unwrap();

        // Each tenant should only see their own keys
        let keys1 = kv.list(&ctx1, "actor:").await.unwrap();
        assert_eq!(keys1.len(), 2);
        assert!(keys1.contains(&"actor:alice".to_string()));
        assert!(keys1.contains(&"actor:bob".to_string()));

        let keys2 = kv.list(&ctx2, "actor:").await.unwrap();
        assert_eq!(keys2.len(), 1);
        assert!(keys2.contains(&"actor:charlie".to_string()));
    }

    #[tokio::test]
    async fn test_namespace_isolation_list_operations() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = namespace1_ctx();
        let ctx2 = namespace2_ctx();

        // Put keys with same prefix for different namespaces
        kv.put(&ctx1, "config:timeout", b"30s".to_vec()).await.unwrap();
        kv.put(&ctx2, "config:timeout", b"60s".to_vec()).await.unwrap();

        // Each namespace should only see their own keys
        let keys1 = kv.list(&ctx1, "config:").await.unwrap();
        assert_eq!(keys1.len(), 1);
        assert_eq!(kv.get(&ctx1, "config:timeout").await.unwrap(), Some(b"30s".to_vec()));

        let keys2 = kv.list(&ctx2, "config:").await.unwrap();
        assert_eq!(keys2.len(), 1);
        assert_eq!(kv.get(&ctx2, "config:timeout").await.unwrap(), Some(b"60s".to_vec()));
    }

    #[tokio::test]
    async fn test_tenant_isolation_cas_operations() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        // Acquire lock in tenant1
        let acquired1 = kv.cas(&ctx1, "lock:resource", None, b"node1".to_vec()).await.unwrap();
        assert!(acquired1);

        // Tenant2 should be able to acquire same lock (different tenant)
        let acquired2 = kv.cas(&ctx2, "lock:resource", None, b"node2".to_vec()).await.unwrap();
        assert!(acquired2);

        // But tenant1 should not be able to acquire again
        let acquired3 = kv.cas(&ctx1, "lock:resource", None, b"node3".to_vec()).await.unwrap();
        assert!(!acquired3);
    }

    #[tokio::test]
    async fn test_tenant_isolation_increment_operations() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        // Increment counter in tenant1
        let count1 = kv.increment(&ctx1, "counter", 5).await.unwrap();
        assert_eq!(count1, 5);

        // Increment counter in tenant2 (should start from 0)
        let count2 = kv.increment(&ctx2, "counter", 3).await.unwrap();
        assert_eq!(count2, 3);

        // Each tenant's counter should be independent
        assert_eq!(kv.increment(&ctx1, "counter", 1).await.unwrap(), 6);
        assert_eq!(kv.increment(&ctx2, "counter", 1).await.unwrap(), 4);
    }

    #[tokio::test]
    async fn test_tenant_isolation_ttl_operations() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        // Put with TTL in tenant1
        kv.put_with_ttl(&ctx1, "session:abc", b"data1".to_vec(), Duration::from_secs(1))
            .await
            .unwrap();

        // Put with longer TTL in tenant2
        kv.put_with_ttl(&ctx2, "session:abc", b"data2".to_vec(), Duration::from_secs(10))
            .await
            .unwrap();

        // Both should exist initially
        assert!(kv.exists(&ctx1, "session:abc").await.unwrap());
        assert!(kv.exists(&ctx2, "session:abc").await.unwrap());

        // Wait for tenant1's TTL to expire
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Tenant1's key should be expired, tenant2's should still exist
        assert!(!kv.exists(&ctx1, "session:abc").await.unwrap());
        assert!(kv.exists(&ctx2, "session:abc").await.unwrap());
    }

    #[tokio::test]
    async fn test_tenant_isolation_stats() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        // Put keys in both tenants
        kv.put(&ctx1, "k1", b"v1".to_vec()).await.unwrap();
        kv.put(&ctx1, "k2", b"v2".to_vec()).await.unwrap();
        kv.put(&ctx2, "k3", b"v3".to_vec()).await.unwrap();

        // Stats should be scoped to tenant
        let stats1 = kv.get_stats(&ctx1).await.unwrap();
        assert_eq!(stats1.total_keys, 2);

        let stats2 = kv.get_stats(&ctx2).await.unwrap();
        assert_eq!(stats2.total_keys, 1);
    }

    #[tokio::test]
    async fn test_tenant_isolation_clear_prefix() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        // Put keys with same prefix in both tenants
        kv.put(&ctx1, "temp:1", b"a".to_vec()).await.unwrap();
        kv.put(&ctx1, "temp:2", b"b".to_vec()).await.unwrap();
        kv.put(&ctx2, "temp:1", b"c".to_vec()).await.unwrap();

        // Clear prefix in tenant1
        let deleted = kv.clear_prefix(&ctx1, "temp:").await.unwrap();
        assert_eq!(deleted, 2);

        // Tenant1's keys should be gone, tenant2's should remain
        assert!(!kv.exists(&ctx1, "temp:1").await.unwrap());
        assert!(!kv.exists(&ctx1, "temp:2").await.unwrap());
        assert!(kv.exists(&ctx2, "temp:1").await.unwrap());
    }

    #[tokio::test]
    async fn test_tenant_isolation_multi_operations() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        // Multi-put in tenant1
        kv.multi_put(&ctx1, &[
            ("k1", b"v1".to_vec()),
            ("k2", b"v2".to_vec()),
        ]).await.unwrap();

        // Multi-put in tenant2
        kv.multi_put(&ctx2, &[
            ("k1", b"v3".to_vec()),
            ("k2", b"v4".to_vec()),
        ]).await.unwrap();

        // Multi-get should return tenant-specific values
        let values1 = kv.multi_get(&ctx1, &["k1", "k2"]).await.unwrap();
        assert_eq!(values1[0], Some(b"v1".to_vec()));
        assert_eq!(values1[1], Some(b"v2".to_vec()));

        let values2 = kv.multi_get(&ctx2, &["k1", "k2"]).await.unwrap();
        assert_eq!(values2[0], Some(b"v3".to_vec()));
        assert_eq!(values2[1], Some(b"v4".to_vec()));
    }

    #[tokio::test]
    async fn test_empty_namespace_list_returns_all_namespaces() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx_ns1 = namespace1_ctx();
        let ctx_ns2 = namespace2_ctx();
        let ctx_empty = empty_namespace_ctx();

        // Put keys in different namespaces
        kv.put(&ctx_ns1, "key:ns1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx_ns2, "key:ns2", b"value2".to_vec()).await.unwrap();

        // List with empty namespace should return keys from all namespaces
        let keys = kv.list(&ctx_empty, "key:").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"key:ns1".to_string()));
        assert!(keys.contains(&"key:ns2".to_string()));

        // List with specific namespace should only return that namespace's keys
        let keys_ns1 = kv.list(&ctx_ns1, "key:").await.unwrap();
        assert_eq!(keys_ns1.len(), 1);
        assert!(keys_ns1.contains(&"key:ns1".to_string()));

        let keys_ns2 = kv.list(&ctx_ns2, "key:").await.unwrap();
        assert_eq!(keys_ns2.len(), 1);
        assert!(keys_ns2.contains(&"key:ns2".to_string()));
    }

    #[tokio::test]
    async fn test_empty_namespace_list_with_multiple_tenants() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1_ns1 = namespace1_ctx();
        let ctx1_empty = empty_namespace_ctx();
        let ctx2_ns1 = tenant2_ctx(); // tenant2, namespace "default"

        // Put keys in tenant1, namespace1
        kv.put(&ctx1_ns1, "key:1", b"value1".to_vec()).await.unwrap();
        
        // Put keys in tenant2, namespace "default"
        kv.put(&ctx2_ns1, "key:2", b"value2".to_vec()).await.unwrap();

        // Empty namespace in tenant1 should only see tenant1's keys across all namespaces
        let keys = kv.list(&ctx1_empty, "key:").await.unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys.contains(&"key:1".to_string()));

        // Tenant2 should only see their own keys
        let keys2 = kv.list(&ctx2_ns1, "key:").await.unwrap();
        assert_eq!(keys2.len(), 1);
        assert!(keys2.contains(&"key:2".to_string()));
    }
}



