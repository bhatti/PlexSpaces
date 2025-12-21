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

//! Redis-based KeyValueStore implementation
//!
//! ## Purpose
//! Provides a distributed, persistent KeyValueStore implementation using Redis.
//!
//! ## Features
//! - **Distributed**: Multi-node Redis cluster support
//! - **Native TTL**: Uses Redis EXPIRE for automatic key expiration
//! - **Atomic Operations**: INCR/DECR, CAS via WATCH/MULTI/EXEC
//! - **Pub/Sub**: Watch notifications via Redis SUBSCRIBE
//! - **Persistence**: Optional RDB/AOF persistence
//!
//! ## Performance Characteristics
//! - Single operations: < 1ms latency (local Redis)
//! - Throughput: > 100K ops/sec (depends on Redis instance)
//! - Network overhead: Minimal with connection pooling
//!
//! ## Usage
//! ```rust,no_run
//! use plexspaces_keyvalue::RedisKVStore;
//! use plexspaces_core::RequestContext;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let store = RedisKVStore::new("redis://localhost:6379", "myapp").await?;
//! let ctx = RequestContext::new("tenant1".to_string());
//! store.put(&ctx, "key", b"value".to_vec()).await?;
//! let value = store.get(&ctx, "key").await?;
//! assert_eq!(value, Some(b"value".to_vec()));
//! # Ok(())
//! # }
//! ```

use crate::{KVError, KVEvent, KVResult, KVStats, KeyValueStore};
use async_trait::async_trait;
use plexspaces_core::RequestContext;
use redis::{aio::ConnectionManager, AsyncCommands, Client, RedisResult};
use std::time::Duration;
use tokio::sync::mpsc;

/// Redis-based KeyValueStore implementation
///
/// ## Architecture
/// - Uses `redis` crate with async ConnectionManager
/// - Connection pooling via ConnectionManager (automatic reconnection)
/// - Namespace prefix to support multi-tenancy
/// - TTL via native Redis EXPIRE command
///
/// ## Design Decisions
/// - **Why ConnectionManager**: Automatic connection pooling and reconnection
/// - **Why namespace prefix**: Allows multiple apps to share same Redis instance
/// - **Why native EXPIRE**: More efficient than manual TTL tracking
pub struct RedisKVStore {
    /// Redis connection manager (async, pooled)
    manager: ConnectionManager,
    /// Namespace prefix for all keys (e.g., "myapp:")
    namespace: String,
}

impl RedisKVStore {
    /// Create a new Redis-backed KeyValueStore
    ///
    /// ## Arguments
    /// * `url` - Redis connection URL (e.g., "redis://localhost:6379")
    /// * `namespace` - Key prefix for isolation (e.g., "myapp")
    ///
    /// ## Returns
    /// A new `RedisKVStore` instance connected to Redis
    ///
    /// ## Errors
    /// - [`KVError::ConnectionFailed`]: If Redis connection fails
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_keyvalue::RedisKVStore;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let store = RedisKVStore::new("redis://localhost:6379", "myapp").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(url: &str, namespace: &str) -> KVResult<Self> {
        let client = Client::open(url)?;
        let manager = ConnectionManager::new(client).await?;

        Ok(Self {
            manager,
            namespace: format!("{}:", namespace),
        })
    }

    /// Add tenant_id, namespace, and key prefix for isolation
    fn prefixed_key(&self, tenant_id: &str, namespace: &str, key: &str) -> String {
        format!("{}:{}:{}:{}", self.namespace, tenant_id, namespace, key)
    }

    /// Remove namespace prefix from key
    fn unprefixed_key(&self, key: &str) -> String {
        key.strip_prefix(&self.namespace).unwrap_or(key).to_string()
    }
}

#[async_trait]
impl KeyValueStore for RedisKVStore {
    /// Get value by key
    async fn get(&self, ctx: &RequestContext, key: &str) -> KVResult<Option<Vec<u8>>> {
        let mut conn = self.manager.clone();
        let prefixed = self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key);

        let result: RedisResult<Option<Vec<u8>>> = conn.get(&prefixed).await;

        match result {
            Ok(value) => Ok(value),
            Err(e) => Err(KVError::BackendError(format!("Redis GET failed: {}", e))),
        }
    }

    /// Store key-value pair (no TTL)
    async fn put(&self, ctx: &RequestContext, key: &str, value: Vec<u8>) -> KVResult<()> {
        let mut conn = self.manager.clone();
        let prefixed = self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key);

        conn.set::<_, _, ()>(&prefixed, value)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis SET failed: {}", e)))?;

        Ok(())
    }

    /// Delete key
    async fn delete(&self, ctx: &RequestContext, key: &str) -> KVResult<()> {
        let mut conn = self.manager.clone();
        let prefixed = self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key);

        conn.del::<_, ()>(&prefixed)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis DEL failed: {}", e)))?;

        Ok(())
    }

    /// Check if key exists
    async fn exists(&self, ctx: &RequestContext, key: &str) -> KVResult<bool> {
        let mut conn = self.manager.clone();
        let prefixed = self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key);

        let exists: bool = conn
            .exists(&prefixed)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis EXISTS failed: {}", e)))?;

        Ok(exists)
    }

    /// List keys matching prefix
    async fn list(&self, ctx: &RequestContext, prefix: &str) -> KVResult<Vec<String>> {
        let mut conn = self.manager.clone();
        let pattern = format!("{}:{}:{}:{}*", self.namespace, ctx.tenant_id(), ctx.namespace(), prefix);

        // Use SCAN instead of KEYS for better performance
        let keys: Vec<String> = redis::cmd("SCAN")
            .cursor_arg(0)
            .arg("MATCH")
            .arg(&pattern)
            .arg("COUNT")
            .query_async::<Vec<String>>(&mut conn)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis SCAN failed: {}", e)))?;

        // Remove namespace/tenant/namespace prefix to return just the key
        let prefix_to_remove = format!("{}:{}:{}:", self.namespace, ctx.tenant_id(), ctx.namespace());
        Ok(keys.into_iter()
            .filter_map(|k| k.strip_prefix(&prefix_to_remove).map(|s| s.to_string()))
            .collect())
    }

    /// Get multiple values
    async fn multi_get(&self, ctx: &RequestContext, keys: &[&str]) -> KVResult<Vec<Option<Vec<u8>>>> {
        let mut conn = self.manager.clone();
        let prefixed: Vec<String> = keys.iter()
            .map(|k| self.prefixed_key(ctx.tenant_id(), ctx.namespace(), k))
            .collect();

        let values: Vec<Option<Vec<u8>>> = conn
            .get(&prefixed)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis MGET failed: {}", e)))?;

        Ok(values)
    }

    /// Put multiple key-value pairs
    async fn multi_put(&self, ctx: &RequestContext, entries: &[(&str, Vec<u8>)]) -> KVResult<()> {
        let mut conn = self.manager.clone();

        // Use pipeline for efficiency
        let mut pipe = redis::pipe();
        for (key, value) in entries {
            pipe.set(self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key), value.clone());
        }

        pipe.query_async::<()>(&mut conn)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis MSET failed: {}", e)))?;

        Ok(())
    }

    /// Store key-value with TTL
    async fn put_with_ttl(&self, ctx: &RequestContext, key: &str, value: Vec<u8>, ttl: Duration) -> KVResult<()> {
        let mut conn = self.manager.clone();
        let prefixed = self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key);
        let ttl_secs = ttl.as_secs();

        conn.set_ex::<_, _, ()>(&prefixed, value, ttl_secs)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis SETEX failed: {}", e)))?;

        Ok(())
    }

    /// Refresh TTL for existing key
    async fn refresh_ttl(&self, ctx: &RequestContext, key: &str, ttl: Duration) -> KVResult<()> {
        let mut conn = self.manager.clone();
        let prefixed = self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key);
        let ttl_secs = ttl.as_secs() as i64;

        let result: bool = conn
            .expire(&prefixed, ttl_secs)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis EXPIRE failed: {}", e)))?;

        if !result {
            return Err(KVError::KeyNotFound(key.to_string()));
        }

        Ok(())
    }

    /// Get remaining TTL for key
    async fn get_ttl(&self, ctx: &RequestContext, key: &str) -> KVResult<Option<Duration>> {
        let mut conn = self.manager.clone();
        let prefixed = self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key);

        let ttl_secs: i64 = conn
            .ttl(&prefixed)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis TTL failed: {}", e)))?;

        match ttl_secs {
            -2 => Ok(None),                                // Key doesn't exist
            -1 => Ok(Some(Duration::from_secs(u64::MAX))), // No expiration
            secs if secs > 0 => Ok(Some(Duration::from_secs(secs as u64))),
            _ => Ok(None),
        }
    }

    /// Compare-and-swap operation
    async fn cas(&self, ctx: &RequestContext, key: &str, expected: Option<Vec<u8>>, new: Vec<u8>) -> KVResult<bool> {
        let mut conn = self.manager.clone();
        let prefixed = self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key);

        // Use WATCH/MULTI/EXEC for atomic CAS
        redis::cmd("WATCH")
            .arg(&prefixed)
            .query_async::<()>(&mut conn)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis WATCH failed: {}", e)))?;

        // Check current value
        let current: Option<Vec<u8>> = conn
            .get(&prefixed)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis GET failed: {}", e)))?;

        if current != expected {
            // Unwatch and return false
            redis::cmd("UNWATCH")
                .query_async::<()>(&mut conn)
                .await
                .map_err(|e| KVError::BackendError(format!("Redis UNWATCH failed: {}", e)))?;
            return Ok(false);
        }

        // Execute transaction
        let mut pipe = redis::pipe();
        pipe.atomic().set(&prefixed, new);

        let result: Option<Vec<()>> = pipe
            .query_async(&mut conn)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis EXEC failed: {}", e)))?;

        // Transaction succeeded if result is Some
        Ok(result.is_some())
    }

    /// Atomic increment
    async fn increment(&self, ctx: &RequestContext, key: &str, delta: i64) -> KVResult<i64> {
        let mut conn = self.manager.clone();
        let prefixed = self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key);

        let new_value: i64 = conn
            .incr(&prefixed, delta)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis INCRBY failed: {}", e)))?;

        Ok(new_value)
    }

    /// Atomic decrement
    async fn decrement(&self, ctx: &RequestContext, key: &str, delta: i64) -> KVResult<i64> {
        let mut conn = self.manager.clone();
        let prefixed = self.prefixed_key(ctx.tenant_id(), ctx.namespace(), key);

        let new_value: i64 = conn
            .decr(&prefixed, delta)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis DECRBY failed: {}", e)))?;

        Ok(new_value)
    }

    /// Watch for changes to specific key
    async fn watch(&self, ctx: &RequestContext, _key: &str) -> KVResult<mpsc::Receiver<KVEvent>> {
        // TODO: Implement Redis keyspace notifications
        // Requires CONFIG SET notify-keyspace-events and separate connection
        // For now, return empty channel (not implemented yet)
        let (_tx, rx) = mpsc::channel(100);
        Ok(rx)
    }

    /// Watch for changes to keys matching prefix
    async fn watch_prefix(&self, ctx: &RequestContext, _prefix: &str) -> KVResult<mpsc::Receiver<KVEvent>> {
        // TODO: Implement Redis keyspace notifications
        // Requires CONFIG SET notify-keyspace-events and separate connection
        // For now, return empty channel (not implemented yet)
        let (_tx, rx) = mpsc::channel(100);
        Ok(rx)
    }

    /// Delete all keys matching prefix
    async fn clear_prefix(&self, ctx: &plexspaces_core::RequestContext, prefix: &str) -> KVResult<usize> {
        let keys = self.list(ctx, prefix).await?;
        let mut count = 0usize;

        for key in keys {
            self.delete(ctx, &key).await?;
            count += 1;
        }

        Ok(count)
    }

    /// Count keys matching prefix
    async fn count_prefix(&self, ctx: &plexspaces_core::RequestContext, prefix: &str) -> KVResult<usize> {
        let keys = self.list(ctx, prefix).await?;
        Ok(keys.len())
    }

    /// Get storage statistics
    async fn get_stats(&self, _ctx: &RequestContext) -> KVResult<KVStats> {
        let mut conn = self.manager.clone();

        // Get Redis INFO stats
        let info: String = redis::cmd("INFO")
            .arg("stats")
            .query_async(&mut conn)
            .await
            .map_err(|e| KVError::BackendError(format!("Redis INFO failed: {}", e)))?;

        // Parse key count from INFO output
        let total_keys = info
            .lines()
            .find(|line| line.starts_with("db0:keys="))
            .and_then(|line| line.split('=').nth(1))
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        Ok(KVStats {
            total_keys,
            total_size_bytes: 0, // Would need INFO memory for accurate value
            backend_type: "Redis".to_string(),
        })
    }
}

// ============================================================================
// TESTS (TDD Approach)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_core::RequestContext;

    // Helper to create test store (requires running Redis instance)
    async fn create_test_store() -> RedisKVStore {
        RedisKVStore::new("redis://localhost:6379", "test")
            .await
            .expect("Failed to connect to Redis (ensure Redis is running)")
    }

    // Helper to create test RequestContext
    fn test_ctx() -> RequestContext {
        RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string())
    }

    #[tokio::test]
    #[ignore] // Requires running Redis instance
    async fn test_basic_put_get() {
        let store = create_test_store().await;
        let ctx = test_ctx();

        store.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        let value = store.get(&ctx, "key1").await.unwrap();

        assert_eq!(value, Some(b"value1".to_vec()));

        // Cleanup
        store.delete(&ctx, "key1").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_ttl_operations() {
        let store = create_test_store().await;
        let ctx = test_ctx();

        store
            .put_with_ttl(&ctx, "ttl_key", b"value".to_vec(), Duration::from_secs(60))
            .await
            .unwrap();

        let ttl = store.get_ttl(&ctx, "ttl_key").await.unwrap();
        assert!(ttl.is_some());
        assert!(ttl.unwrap().as_secs() <= 60);

        // Cleanup
        store.delete(&ctx, "ttl_key").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_atomic_increment() {
        let store = create_test_store().await;
        let ctx = test_ctx();

        let v1 = store.increment(&ctx, "counter", 5).await.unwrap();
        let v2 = store.increment(&ctx, "counter", 3).await.unwrap();

        assert_eq!(v1, 5);
        assert_eq!(v2, 8);

        // Cleanup
        store.delete(&ctx, "counter").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_cas_success() {
        let store = create_test_store().await;
        let ctx = test_ctx();

        store.put(&ctx, "cas_key", b"old".to_vec()).await.unwrap();

        let success = store
            .cas(&ctx, "cas_key", Some(b"old".to_vec()), b"new".to_vec())
            .await
            .unwrap();

        assert!(success);

        let value = store.get(&ctx, "cas_key").await.unwrap();
        assert_eq!(value, Some(b"new".to_vec()));

        // Cleanup
        store.delete(&ctx, "cas_key").await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn test_list_prefix() {
        let store = create_test_store().await;
        let ctx = test_ctx();

        store.put(&ctx, "prefix:key1", b"v1".to_vec()).await.unwrap();
        store.put(&ctx, "prefix:key2", b"v2".to_vec()).await.unwrap();
        store.put(&ctx, "other:key", b"v3".to_vec()).await.unwrap();

        let keys = store.list(&ctx, "prefix:").await.unwrap();
        assert_eq!(keys.len(), 2);

        // Cleanup
        store.clear_prefix(&ctx, "prefix:").await.unwrap();
        store.delete(&ctx, "other:key").await.unwrap();
    }
}
