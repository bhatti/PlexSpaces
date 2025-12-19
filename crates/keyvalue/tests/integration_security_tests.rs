// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Security integration tests for KeyValueStore tenant isolation
// These tests verify that unauthenticated or unauthorized access cannot
// access data from other tenants.

#[cfg(feature = "sql-backend")]
mod security_tests {
    use plexspaces_keyvalue::{KeyValueStore, SqliteKVStore};
    use plexspaces_core::RequestContext;

    /// Create RequestContext for tenant1
    fn tenant1_ctx() -> RequestContext {
        RequestContext::new_without_auth("tenant1".to_string(), "default".to_string())
    }

    /// Create RequestContext for tenant2
    fn tenant2_ctx() -> RequestContext {
        RequestContext::new_without_auth("tenant2".to_string(), "default".to_string())
    }

    /// Create RequestContext for tenant3 (unauthorized tenant)
    fn tenant3_ctx() -> RequestContext {
        RequestContext::new_without_auth("tenant3".to_string(), "default".to_string())
    }

    /// Create RequestContext with empty tenant (simulating unauthenticated)
    fn unauthenticated_ctx() -> RequestContext {
        RequestContext::new_without_auth("".to_string(), "default".to_string())
    }

    /// Create RequestContext with malicious tenant_id (header injection attempt)
    fn malicious_ctx() -> RequestContext {
        RequestContext::new_without_auth("tenant1'; DROP TABLE kv_store; --".to_string(), "default".to_string())
    }

    #[tokio::test]
    async fn test_unauthorized_tenant_cannot_access_other_tenant_data() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();
        let ctx3 = tenant3_ctx();

        // Tenant1 stores sensitive data
        kv.put(&ctx1, "secret:key", b"tenant1-secret".to_vec()).await.unwrap();
        kv.put(&ctx1, "config:api_key", b"tenant1-api-key".to_vec()).await.unwrap();

        // Tenant2 stores their own data
        kv.put(&ctx2, "secret:key", b"tenant2-secret".to_vec()).await.unwrap();

        // Tenant3 (unauthorized) tries to access tenant1's data
        // Should get None (data isolation enforced)
        assert_eq!(kv.get(&ctx3, "secret:key").await.unwrap(), None);
        assert_eq!(kv.get(&ctx3, "config:api_key").await.unwrap(), None);

        // Tenant3 should only see their own data (if any)
        kv.put(&ctx3, "secret:key", b"tenant3-secret".to_vec()).await.unwrap();
        assert_eq!(kv.get(&ctx3, "secret:key").await.unwrap(), Some(b"tenant3-secret".to_vec()));

        // Verify tenant1's data is still isolated
        assert_eq!(kv.get(&ctx1, "secret:key").await.unwrap(), Some(b"tenant1-secret".to_vec()));
        assert_eq!(kv.get(&ctx2, "secret:key").await.unwrap(), Some(b"tenant2-secret".to_vec()));
    }

    #[tokio::test]
    async fn test_unauthenticated_request_cannot_access_any_tenant_data() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let unauthenticated = unauthenticated_ctx();

        // Tenant1 stores data
        kv.put(&ctx1, "user:alice", b"alice-data".to_vec()).await.unwrap();
        kv.put(&ctx1, "user:bob", b"bob-data".to_vec()).await.unwrap();

        // Unauthenticated request (empty tenant_id) should not access tenant1's data
        // Note: Empty tenant_id may be rejected by validation, but if it passes,
        // it should be isolated from tenant1's data
        let keys = kv.list(&unauthenticated, "user:").await.unwrap();
        assert_eq!(keys.len(), 0, "Unauthenticated request should not see tenant1's data");

        // Unauthenticated can only see their own empty tenant namespace
        kv.put(&unauthenticated, "temp:data", b"temp".to_vec()).await.unwrap();
        let keys = kv.list(&unauthenticated, "temp:").await.unwrap();
        assert_eq!(keys.len(), 1);

        // Tenant1's data should still be isolated
        let keys = kv.list(&ctx1, "user:").await.unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[tokio::test]
    async fn test_tenant_isolation_enforced_at_storage_layer() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();

        // Store same keys for different tenants
        for i in 0..100 {
            let key = format!("item:{}", i);
            kv.put(&ctx1, &key, format!("tenant1-value-{}", i).into_bytes()).await.unwrap();
            kv.put(&ctx2, &key, format!("tenant2-value-{}", i).into_bytes()).await.unwrap();
        }

        // Verify complete isolation - tenant1 cannot see tenant2's data
        for i in 0..100 {
            let key = format!("item:{}", i);
            let value = kv.get(&ctx1, &key).await.unwrap().unwrap();
            let value_str = String::from_utf8(value).unwrap();
            assert!(value_str.starts_with("tenant1-value-"), 
                "Tenant1 should only see their own data, got: {}", value_str);
        }

        // Verify tenant2's isolation
        for i in 0..100 {
            let key = format!("item:{}", i);
            let value = kv.get(&ctx2, &key).await.unwrap().unwrap();
            let value_str = String::from_utf8(value).unwrap();
            assert!(value_str.starts_with("tenant2-value-"),
                "Tenant2 should only see their own data, got: {}", value_str);
        }
    }

    #[tokio::test]
    async fn test_sql_injection_attempt_via_tenant_id_is_isolated() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let malicious = malicious_ctx();

        // Tenant1 stores data
        kv.put(&ctx1, "data:1", b"tenant1-data".to_vec()).await.unwrap();

        // Malicious tenant_id (SQL injection attempt) should be treated as a different tenant
        // and should not access tenant1's data
        assert_eq!(kv.get(&malicious, "data:1").await.unwrap(), None);

        // Malicious tenant can only access their own namespace
        kv.put(&malicious, "data:1", b"malicious-data".to_vec()).await.unwrap();
        assert_eq!(kv.get(&malicious, "data:1").await.unwrap(), Some(b"malicious-data".to_vec()));

        // Tenant1's data should still be safe
        assert_eq!(kv.get(&ctx1, "data:1").await.unwrap(), Some(b"tenant1-data".to_vec()));
    }

    #[tokio::test]
    async fn test_list_operations_respect_tenant_isolation() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();
        let ctx3 = tenant3_ctx();

        // Store keys with same prefix in different tenants
        for i in 0..50 {
            let key = format!("actor:{}", i);
            kv.put(&ctx1, &key, format!("tenant1-actor-{}", i).into_bytes()).await.unwrap();
        }

        for i in 0..30 {
            let key = format!("actor:{}", i);
            kv.put(&ctx2, &key, format!("tenant2-actor-{}", i).into_bytes()).await.unwrap();
        }

        // Tenant3 (unauthorized) should see no keys
        let keys3 = kv.list(&ctx3, "actor:").await.unwrap();
        assert_eq!(keys3.len(), 0, "Unauthorized tenant should not see any keys");

        // Tenant1 should only see their own keys
        let keys1 = kv.list(&ctx1, "actor:").await.unwrap();
        assert_eq!(keys1.len(), 50);
        for key in &keys1 {
            assert!(key.starts_with("actor:"));
        }

        // Tenant2 should only see their own keys
        let keys2 = kv.list(&ctx2, "actor:").await.unwrap();
        assert_eq!(keys2.len(), 30);
        for key in &keys2 {
            assert!(key.starts_with("actor:"));
        }
    }

    #[tokio::test]
    async fn test_multi_get_respects_tenant_isolation() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();
        let ctx3 = tenant3_ctx();

        // Store same keys in different tenants
        kv.put(&ctx1, "k1", b"tenant1-v1".to_vec()).await.unwrap();
        kv.put(&ctx1, "k2", b"tenant1-v2".to_vec()).await.unwrap();
        kv.put(&ctx2, "k1", b"tenant2-v1".to_vec()).await.unwrap();
        kv.put(&ctx2, "k2", b"tenant2-v2".to_vec()).await.unwrap();

        // Tenant3 (unauthorized) should get None for all keys
        let values3 = kv.multi_get(&ctx3, &["k1", "k2"]).await.unwrap();
        assert_eq!(values3[0], None);
        assert_eq!(values3[1], None);

        // Tenant1 should get their own values
        let values1 = kv.multi_get(&ctx1, &["k1", "k2"]).await.unwrap();
        assert_eq!(values1[0], Some(b"tenant1-v1".to_vec()));
        assert_eq!(values1[1], Some(b"tenant1-v2".to_vec()));

        // Tenant2 should get their own values
        let values2 = kv.multi_get(&ctx2, &["k1", "k2"]).await.unwrap();
        assert_eq!(values2[0], Some(b"tenant2-v1".to_vec()));
        assert_eq!(values2[1], Some(b"tenant2-v2".to_vec()));
    }

    #[tokio::test]
    async fn test_delete_operations_respect_tenant_isolation() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();
        let ctx3 = tenant3_ctx();

        // Store same key in multiple tenants
        kv.put(&ctx1, "shared:key", b"tenant1-value".to_vec()).await.unwrap();
        kv.put(&ctx2, "shared:key", b"tenant2-value".to_vec()).await.unwrap();

        // Tenant3 tries to delete (should not affect other tenants)
        kv.delete(&ctx3, "shared:key").await.unwrap(); // Should succeed but do nothing

        // Verify tenant1 and tenant2's data is still intact
        assert_eq!(kv.get(&ctx1, "shared:key").await.unwrap(), Some(b"tenant1-value".to_vec()));
        assert_eq!(kv.get(&ctx2, "shared:key").await.unwrap(), Some(b"tenant2-value".to_vec()));

        // Tenant1 deletes their own key
        kv.delete(&ctx1, "shared:key").await.unwrap();

        // Tenant1's key should be gone, tenant2's should remain
        assert_eq!(kv.get(&ctx1, "shared:key").await.unwrap(), None);
        assert_eq!(kv.get(&ctx2, "shared:key").await.unwrap(), Some(b"tenant2-value".to_vec()));
    }

    #[tokio::test]
    async fn test_stats_respect_tenant_isolation() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();
        let ctx3 = tenant3_ctx();

        // Store different amounts of data in each tenant
        for i in 0..100 {
            kv.put(&ctx1, &format!("key:{}", i), b"value".to_vec()).await.unwrap();
        }
        for i in 0..50 {
            kv.put(&ctx2, &format!("key:{}", i), b"value".to_vec()).await.unwrap();
        }

        // Tenant3 should see zero stats (no data)
        let stats3 = kv.get_stats(&ctx3).await.unwrap();
        assert_eq!(stats3.total_keys, 0);

        // Tenant1 should see their own stats
        let stats1 = kv.get_stats(&ctx1).await.unwrap();
        assert_eq!(stats1.total_keys, 100);

        // Tenant2 should see their own stats
        let stats2 = kv.get_stats(&ctx2).await.unwrap();
        assert_eq!(stats2.total_keys, 50);
    }

    #[tokio::test]
    async fn test_clear_prefix_respects_tenant_isolation() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();
        let ctx3 = tenant3_ctx();

        // Store keys with same prefix in different tenants
        for i in 0..20 {
            kv.put(&ctx1, &format!("temp:{}", i), b"data".to_vec()).await.unwrap();
        }
        for i in 0..10 {
            kv.put(&ctx2, &format!("temp:{}", i), b"data".to_vec()).await.unwrap();
        }

        // Tenant3 tries to clear prefix (should not affect other tenants)
        let deleted3 = kv.clear_prefix(&ctx3, "temp:").await.unwrap();
        assert_eq!(deleted3, 0);

        // Verify tenant1 and tenant2's data is still intact
        let keys1 = kv.list(&ctx1, "temp:").await.unwrap();
        assert_eq!(keys1.len(), 20);
        let keys2 = kv.list(&ctx2, "temp:").await.unwrap();
        assert_eq!(keys2.len(), 10);

        // Tenant1 clears their prefix
        let deleted1 = kv.clear_prefix(&ctx1, "temp:").await.unwrap();
        assert_eq!(deleted1, 20);

        // Tenant1's keys should be gone, tenant2's should remain
        let keys1 = kv.list(&ctx1, "temp:").await.unwrap();
        assert_eq!(keys1.len(), 0);
        let keys2 = kv.list(&ctx2, "temp:").await.unwrap();
        assert_eq!(keys2.len(), 10);
    }

    #[tokio::test]
    async fn test_cas_operations_respect_tenant_isolation() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();
        let ctx3 = tenant3_ctx();

        // Tenant1 acquires lock
        let acquired1 = kv.cas(&ctx1, "lock:resource", None, b"tenant1-lock".to_vec()).await.unwrap();
        assert!(acquired1);

        // Tenant2 should be able to acquire same lock (different tenant, different isolation)
        let acquired2 = kv.cas(&ctx2, "lock:resource", None, b"tenant2-lock".to_vec()).await.unwrap();
        assert!(acquired2);

        // Tenant3 (unauthorized) should also be able to acquire (different tenant)
        let acquired3 = kv.cas(&ctx3, "lock:resource", None, b"tenant3-lock".to_vec()).await.unwrap();
        assert!(acquired3);

        // But each tenant should not be able to acquire again in their own tenant
        let acquired1_again = kv.cas(&ctx1, "lock:resource", None, b"tenant1-lock2".to_vec()).await.unwrap();
        assert!(!acquired1_again);

        // Verify each tenant has their own lock value
        assert_eq!(kv.get(&ctx1, "lock:resource").await.unwrap(), Some(b"tenant1-lock".to_vec()));
        assert_eq!(kv.get(&ctx2, "lock:resource").await.unwrap(), Some(b"tenant2-lock".to_vec()));
        assert_eq!(kv.get(&ctx3, "lock:resource").await.unwrap(), Some(b"tenant3-lock".to_vec()));
    }

    #[tokio::test]
    async fn test_increment_operations_respect_tenant_isolation() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();
        let ctx1 = tenant1_ctx();
        let ctx2 = tenant2_ctx();
        let ctx3 = tenant3_ctx();

        // Each tenant increments their own counter
        let count1 = kv.increment(&ctx1, "counter", 10).await.unwrap();
        assert_eq!(count1, 10);

        let count2 = kv.increment(&ctx2, "counter", 5).await.unwrap();
        assert_eq!(count2, 5);

        // Tenant3 (unauthorized) increments their own counter
        let count3 = kv.increment(&ctx3, "counter", 3).await.unwrap();
        assert_eq!(count3, 3);

        // Each tenant's counter should be independent
        assert_eq!(kv.increment(&ctx1, "counter", 1).await.unwrap(), 11);
        assert_eq!(kv.increment(&ctx2, "counter", 1).await.unwrap(), 6);
        assert_eq!(kv.increment(&ctx3, "counter", 1).await.unwrap(), 4);

        // Verify values are isolated by checking increment results directly
        // (increment stores values as binary i64, not strings)
        assert_eq!(kv.increment(&ctx1, "counter", 0).await.unwrap(), 11);
        assert_eq!(kv.increment(&ctx2, "counter", 0).await.unwrap(), 6);
        assert_eq!(kv.increment(&ctx3, "counter", 0).await.unwrap(), 4);
    }
}



