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

//! In-memory KeyValue store implementation.
//!
//! ## Purpose
//! Provides a HashMap-based implementation for testing and single-process scenarios.
//!
//! ## Features
//! - Fast in-memory operations
//! - TTL support with background cleanup
//! - Watch notifications
//! - Atomic CAS and increment operations
//!
//! ## Limitations
//! - Not persistent (data lost on restart)
//! - Not distributed (single process only)
//! - Limited scalability (all data in RAM)

use crate::{KVError, KVEvent, KVEventType, KVResult, KVStats, KeyValueStore};
use async_trait::async_trait;
use plexspaces_common::RequestContext;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

/// Entry in the in-memory store with optional TTL.
#[derive(Debug, Clone)]
struct Entry {
    value: Vec<u8>,
    expires_at: Option<Instant>,
}

impl Entry {
    fn new(value: Vec<u8>) -> Self {
        Self {
            value,
            expires_at: None,
        }
    }

    fn new_with_ttl(value: Vec<u8>, ttl: Duration) -> Self {
        Self {
            value,
            expires_at: Some(Instant::now() + ttl),
        }
    }

    fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|exp| Instant::now() >= exp)
    }

    fn ttl_remaining(&self) -> Option<Duration> {
        self.expires_at
            .and_then(|exp| exp.checked_duration_since(Instant::now()))
    }
}

/// Watch subscription for a specific key or prefix.
#[derive(Debug)]
struct Watch {
    pattern: String,
    sender: mpsc::Sender<KVEvent>,
    is_prefix: bool,
}

impl Watch {
    fn matches(&self, key: &str) -> bool {
        if self.is_prefix {
            key.starts_with(&self.pattern)
        } else {
            key == self.pattern
        }
    }
}

/// In-memory KeyValue store implementation.
///
/// ## Example
/// ```rust
/// use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let kv = InMemoryKVStore::new();
///
/// kv.put("key", b"value".to_vec()).await?;
/// let value = kv.get("key").await?;
/// assert_eq!(value, Some(b"value".to_vec()));
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct InMemoryKVStore {
    data: Arc<RwLock<HashMap<String, Entry>>>,
    watches: Arc<RwLock<Vec<Watch>>>,
}

impl InMemoryKVStore {
    /// Create a new in-memory KeyValue store.
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            watches: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Notify watchers of a change.
    async fn notify_watchers(&self, key: &str, event: KVEvent) {
        let watches = self.watches.read().await;
        for watch in watches.iter() {
            if watch.matches(key) {
                // Best-effort notification (ignore send errors)
                let _ = watch.sender.send(event.clone()).await;
            }
        }
    }

    /// Clean up expired entries.
    #[allow(dead_code)]
    async fn cleanup_expired(&self) -> usize {
        let mut data = self.data.write().await;
        let expired_keys: Vec<String> = data
            .iter()
            .filter(|(_, entry)| entry.is_expired())
            .map(|(key, _)| key.clone())
            .collect();

        for key in &expired_keys {
            if let Some(entry) = data.remove(key) {
                // Notify watchers of expiration
                let event = KVEvent {
                    key: key.clone(),
                    event_type: KVEventType::Expire,
                    old_value: Some(entry.value),
                    new_value: None,
                };
                drop(data); // Release write lock before notifying
                self.notify_watchers(key, event).await;
                data = self.data.write().await; // Re-acquire lock
            }
        }

        expired_keys.len()
    }

    /// Decode i64 from bytes (big-endian).
    fn decode_i64(bytes: &[u8]) -> KVResult<i64> {
        if bytes.len() != 8 {
            return Err(KVError::InvalidValue(format!(
                "Expected 8 bytes for i64, got {}",
                bytes.len()
            )));
        }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(bytes);
        Ok(i64::from_be_bytes(arr))
    }

    /// Encode i64 to bytes (big-endian).
    fn encode_i64(value: i64) -> Vec<u8> {
        value.to_be_bytes().to_vec()
    }

    /// Create composite key from tenant_id, namespace, and key.
    fn composite_key(tenant_id: &str, namespace: &str, key: &str) -> String {
        format!("{}:{}:{}", tenant_id, namespace, key)
    }
}

impl Default for InMemoryKVStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl KeyValueStore for InMemoryKVStore {
    async fn get(&self, ctx: &RequestContext, key: &str) -> KVResult<Option<Vec<u8>>> {
        let composite = Self::composite_key(ctx.tenant_id(), ctx.namespace(), key);
        let data = self.data.read().await;
        match data.get(&composite) {
            Some(entry) if !entry.is_expired() => Ok(Some(entry.value.clone())),
            _ => Ok(None),
        }
    }

    async fn put(&self, ctx: &RequestContext, key: &str, value: Vec<u8>) -> KVResult<()> {
        let composite = Self::composite_key(ctx.tenant_id(), ctx.namespace(), key);
        let mut data = self.data.write().await;
        let old_value = data.get(&composite).map(|e| e.value.clone());
        data.insert(composite.clone(), Entry::new(value.clone()));

        drop(data); // Release write lock

        let event = KVEvent {
            key: key.to_string(),
            event_type: KVEventType::Put,
            old_value,
            new_value: Some(value),
        };
        self.notify_watchers(key, event).await;

        Ok(())
    }

    async fn delete(&self, ctx: &RequestContext, key: &str) -> KVResult<()> {
        let composite = Self::composite_key(ctx.tenant_id(), ctx.namespace(), key);
        let mut data = self.data.write().await;
        let old_value = data.remove(&composite).map(|e| e.value);

        if let Some(old_val) = old_value {
            drop(data); // Release write lock

            let event = KVEvent {
                key: key.to_string(),
                event_type: KVEventType::Delete,
                old_value: Some(old_val),
                new_value: None,
            };
            self.notify_watchers(key, event).await;
        }

        Ok(())
    }

    async fn exists(&self, ctx: &RequestContext, key: &str) -> KVResult<bool> {
        let composite = Self::composite_key(ctx.tenant_id(), ctx.namespace(), key);
        let data = self.data.read().await;
        Ok(data.get(&composite).is_some_and(|e| !e.is_expired()))
    }

    async fn list(&self, ctx: &RequestContext, prefix: &str) -> KVResult<Vec<String>> {
        let tenant_prefix = Self::composite_key(ctx.tenant_id(), ctx.namespace(), prefix);
        let data = self.data.read().await;
        let keys: Vec<String> = data
            .iter()
            .filter(|(key, entry)| key.starts_with(&tenant_prefix) && !entry.is_expired())
            .map(|(key, _)| {
                // Extract original key from composite key (tenant:namespace:key)
                key.strip_prefix(&format!("{}:{}:", ctx.tenant_id(), ctx.namespace()))
                    .unwrap_or(key)
                    .to_string()
            })
            .collect();
        Ok(keys)
    }

    async fn multi_get(&self, ctx: &RequestContext, keys: &[&str]) -> KVResult<Vec<Option<Vec<u8>>>> {
        let data = self.data.read().await;
        let results: Vec<Option<Vec<u8>>> = keys
            .iter()
            .map(|key| {
                let composite = Self::composite_key(ctx.tenant_id(), ctx.namespace(), key);
                data.get(&composite)
                    .filter(|e| !e.is_expired())
                    .map(|e| e.value.clone())
            })
            .collect();
        Ok(results)
    }

    async fn multi_put(&self, ctx: &RequestContext, pairs: &[(&str, Vec<u8>)]) -> KVResult<()> {
        for (key, value) in pairs {
            self.put(ctx, key, value.clone()).await?;
        }
        Ok(())
    }

    async fn put_with_ttl(&self, ctx: &RequestContext, key: &str, value: Vec<u8>, ttl: Duration) -> KVResult<()> {
        let composite = Self::composite_key(ctx.tenant_id(), ctx.namespace(), key);
        let mut data = self.data.write().await;
        let old_value = data.get(&composite).map(|e| e.value.clone());
        data.insert(composite.clone(), Entry::new_with_ttl(value.clone(), ttl));

        drop(data); // Release write lock

        let event = KVEvent {
            key: key.to_string(),
            event_type: KVEventType::Put,
            old_value,
            new_value: Some(value),
        };
        self.notify_watchers(key, event).await;

        Ok(())
    }

    async fn refresh_ttl(&self, ctx: &RequestContext, key: &str, ttl: Duration) -> KVResult<()> {
        let composite = Self::composite_key(ctx.tenant_id(), ctx.namespace(), key);
        let mut data = self.data.write().await;
        match data.get_mut(&composite) {
            Some(entry) if !entry.is_expired() => {
                entry.expires_at = Some(Instant::now() + ttl);
                Ok(())
            }
            _ => Err(KVError::KeyNotFound(key.to_string())),
        }
    }

    async fn get_ttl(&self, ctx: &RequestContext, key: &str) -> KVResult<Option<Duration>> {
        let composite = Self::composite_key(ctx.tenant_id(), ctx.namespace(), key);
        let data = self.data.read().await;
        Ok(data
            .get(&composite)
            .filter(|e| !e.is_expired())
            .and_then(|e| e.ttl_remaining()))
    }

    async fn cas(
        &self,
        ctx: &RequestContext,
        key: &str,
        expected: Option<Vec<u8>>,
        new_value: Vec<u8>,
    ) -> KVResult<bool> {
        let composite = Self::composite_key(ctx.tenant_id(), ctx.namespace(), key);
        let mut data = self.data.write().await;

        let current = data.get(&composite).filter(|e| !e.is_expired()).map(|e| &e.value);

        let matches = match (current, &expected) {
            (None, None) => true,                   // Key doesn't exist, expected None
            (Some(curr), Some(exp)) => curr == exp, // Key exists, values match
            _ => false,                             // Mismatch
        };

        if matches {
            let old_value = data.get(&composite).map(|e| e.value.clone());
            data.insert(composite.clone(), Entry::new(new_value.clone()));

            drop(data); // Release write lock

            let event = KVEvent {
                key: key.to_string(),
                event_type: KVEventType::Put,
                old_value,
                new_value: Some(new_value),
            };
            self.notify_watchers(key, event).await;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn increment(&self, ctx: &RequestContext, key: &str, delta: i64) -> KVResult<i64> {
        let composite = Self::composite_key(ctx.tenant_id(), ctx.namespace(), key);
        let mut data = self.data.write().await;

        let current = data
            .get(&composite)
            .filter(|e| !e.is_expired())
            .map(|e| Self::decode_i64(&e.value))
            .transpose()?
            .unwrap_or(0);

        let new_value = current + delta;
        let encoded = Self::encode_i64(new_value);

        data.insert(composite.clone(), Entry::new(encoded.clone()));

        drop(data); // Release write lock

        let event = KVEvent {
            key: key.to_string(),
            event_type: KVEventType::Put,
            old_value: Some(Self::encode_i64(current)),
            new_value: Some(encoded),
        };
        self.notify_watchers(key, event).await;

        Ok(new_value)
    }

    async fn decrement(&self, ctx: &RequestContext, key: &str, delta: i64) -> KVResult<i64> {
        self.increment(ctx, key, -delta).await
    }

    async fn watch(&self, ctx: &RequestContext, key: &str) -> KVResult<mpsc::Receiver<KVEvent>> {
        let (tx, rx) = mpsc::channel(100);
        let watch = Watch {
            pattern: key.to_string(),
            sender: tx,
            is_prefix: false,
        };

        let mut watches = self.watches.write().await;
        watches.push(watch);

        Ok(rx)
    }

    async fn watch_prefix(&self, ctx: &RequestContext, prefix: &str) -> KVResult<mpsc::Receiver<KVEvent>> {
        let (tx, rx) = mpsc::channel(100);
        let watch = Watch {
            pattern: prefix.to_string(),
            sender: tx,
            is_prefix: true,
        };

        let mut watches = self.watches.write().await;
        watches.push(watch);

        Ok(rx)
    }

    async fn clear_prefix(&self, ctx: &RequestContext, prefix: &str) -> KVResult<usize> {
        let tenant_prefix = Self::composite_key(ctx.tenant_id(), ctx.namespace(), prefix);
        let mut data = self.data.write().await;
        let keys_to_delete: Vec<String> = data
            .keys()
            .filter(|key| key.starts_with(&tenant_prefix))
            .cloned()
            .collect();

        for key in &keys_to_delete {
            if let Some(entry) = data.remove(key) {
                let event = KVEvent {
                    key: key.clone(),
                    event_type: KVEventType::Delete,
                    old_value: Some(entry.value),
                    new_value: None,
                };
                drop(data); // Release write lock
                self.notify_watchers(key, event).await;
                data = self.data.write().await; // Re-acquire lock
            }
        }

        Ok(keys_to_delete.len())
    }

    async fn count_prefix(&self, ctx: &RequestContext, prefix: &str) -> KVResult<usize> {
        let tenant_prefix = Self::composite_key(ctx.tenant_id(), ctx.namespace(), prefix);
        let data = self.data.read().await;
        let count = data
            .iter()
            .filter(|(key, entry)| key.starts_with(&tenant_prefix) && !entry.is_expired())
            .count();
        Ok(count)
    }

    async fn get_stats(&self, ctx: &RequestContext) -> KVResult<KVStats> {
        let tenant_prefix = format!("{}:{}:", ctx.tenant_id(), ctx.namespace());
        let data = self.data.read().await;
        let total_keys = data.iter()
            .filter(|(k, e)| k.starts_with(&tenant_prefix) && !e.is_expired())
            .count();
        let total_size_bytes: usize = data
            .iter()
            .filter(|(k, e)| k.starts_with(&tenant_prefix) && !e.is_expired())
            .map(|(k, e)| k.len() + e.value.len())
            .sum();

        Ok(KVStats {
            total_keys,
            total_size_bytes,
            backend_type: "InMemory".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> RequestContext {
        plexspaces_common::RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string())
    }

    #[tokio::test]
    async fn test_basic_operations() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        // Put and get
        kv.put(&ctx, "key1", b"value1".to_vec()).await.unwrap();
        let value = kv.get(&ctx, "key1").await.unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Exists
        assert!(kv.exists(&ctx, "key1").await.unwrap());
        assert!(!kv.exists(&ctx, "nonexistent").await.unwrap());

        // Delete
        kv.delete(&ctx, "key1").await.unwrap();
        assert!(!kv.exists(&ctx, "key1").await.unwrap());
    }

    #[tokio::test]
    async fn test_tenant_isolation() {
        let kv = InMemoryKVStore::new();
        let ctx1 = RequestContext::new_without_auth("tenant1".to_string(), "default".to_string());
        let ctx2 = plexspaces_common::RequestContext::new_without_auth("tenant2".to_string(), "default".to_string());

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
    async fn test_namespace_isolation() {
        let kv = InMemoryKVStore::new();
        let ctx1 = RequestContext::new_without_auth("tenant1".to_string(), "ns1".to_string());
        let ctx2 = plexspaces_common::RequestContext::new_without_auth("tenant1".to_string(), "ns2".to_string());

        // Put same key for different namespaces
        kv.put(&ctx1, "key1", b"value1".to_vec()).await.unwrap();
        kv.put(&ctx2, "key1", b"value2".to_vec()).await.unwrap();

        // Each namespace should see their own value
        assert_eq!(kv.get(&ctx1, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(kv.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_list_prefix() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        kv.put(&ctx, "actor:alice", b"ref1".to_vec()).await.unwrap();
        kv.put(&ctx, "actor:bob", b"ref2".to_vec()).await.unwrap();
        kv.put(&ctx, "node:node1", b"info".to_vec()).await.unwrap();

        let actors = kv.list(&ctx, "actor:").await.unwrap();
        assert_eq!(actors.len(), 2);
        assert!(actors.contains(&"actor:alice".to_string()));
        assert!(actors.contains(&"actor:bob".to_string()));
    }

    #[tokio::test]
    async fn test_multi_get() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        kv.put(&ctx, "k1", b"v1".to_vec()).await.unwrap();
        kv.put(&ctx, "k2", b"v2".to_vec()).await.unwrap();

        let values = kv.multi_get(&ctx, &["k1", "k2", "k3"]).await.unwrap();
        assert_eq!(values[0], Some(b"v1".to_vec()));
        assert_eq!(values[1], Some(b"v2".to_vec()));
        assert_eq!(values[2], None);
    }

    #[tokio::test]
    async fn test_multi_put() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        kv.multi_put(&ctx, &[("k1", b"v1".to_vec()), ("k2", b"v2".to_vec())])
            .await
            .unwrap();

        assert_eq!(kv.get(&ctx, "k1").await.unwrap(), Some(b"v1".to_vec()));
        assert_eq!(kv.get(&ctx, "k2").await.unwrap(), Some(b"v2".to_vec()));
    }

    #[tokio::test]
    async fn test_ttl() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        kv.put_with_ttl(&ctx, "key", b"value".to_vec(), Duration::from_secs(1))
            .await
            .unwrap();

        // Should exist immediately
        assert!(kv.exists(&ctx, "key").await.unwrap());

        // Check TTL
        let ttl = kv.get_ttl(&ctx, "key").await.unwrap();
        assert!(ttl.is_some());
        assert!(ttl.unwrap() <= Duration::from_secs(1));

        // Wait for expiry
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(!kv.exists(&ctx, "key").await.unwrap());
    }

    #[tokio::test]
    async fn test_refresh_ttl() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        kv.put_with_ttl(&ctx, "key", b"value".to_vec(), Duration::from_secs(1))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Refresh TTL
        kv.refresh_ttl(&ctx, "key", Duration::from_secs(2)).await.unwrap();

        // Should still exist after original TTL
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(kv.exists(&ctx, "key").await.unwrap());
    }

    #[tokio::test]
    async fn test_cas() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        // Acquire lock (only if not held)
        let acquired = kv
            .cas(&ctx, "lock:resource", None, b"node1".to_vec())
            .await
            .unwrap();
        assert!(acquired);

        // Try to acquire again (should fail)
        let acquired2 = kv
            .cas(&ctx, "lock:resource", None, b"node2".to_vec())
            .await
            .unwrap();
        assert!(!acquired2);

        // Release lock (expected value matches)
        let released = kv
            .cas(&ctx, "lock:resource", Some(b"node1".to_vec()), b"node2".to_vec())
            .await
            .unwrap();
        assert!(released);
    }

    #[tokio::test]
    async fn test_increment_decrement() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        // Increment from 0
        let count = kv.increment(&ctx, "counter", 1).await.unwrap();
        assert_eq!(count, 1);

        let count = kv.increment(&ctx, "counter", 5).await.unwrap();
        assert_eq!(count, 6);

        // Decrement
        let count = kv.decrement(&ctx, "counter", 3).await.unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_watch() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();
        let mut watcher = kv.watch(&ctx, "config:timeout").await.unwrap();

        // Put value
        kv.put(&ctx, "config:timeout", b"30s".to_vec()).await.unwrap();

        // Should receive event
        let event = tokio::time::timeout(Duration::from_secs(1), watcher.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(event.key, "config:timeout");
        assert_eq!(event.event_type, KVEventType::Put);
        assert_eq!(event.new_value, Some(b"30s".to_vec()));
    }

    #[tokio::test]
    async fn test_watch_prefix() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();
        let mut watcher = kv.watch_prefix(&ctx, "config:actor.").await.unwrap();

        // Put value
        kv.put(&ctx, "config:actor.timeout", b"30s".to_vec())
            .await
            .unwrap();

        // Should receive event
        let event = tokio::time::timeout(Duration::from_secs(1), watcher.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(event.key, "config:actor.timeout");
        assert_eq!(event.event_type, KVEventType::Put);
    }

    #[tokio::test]
    async fn test_clear_prefix() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        kv.put(&ctx, "temp:1", b"a".to_vec()).await.unwrap();
        kv.put(&ctx, "temp:2", b"b".to_vec()).await.unwrap();
        kv.put(&ctx, "keep:1", b"c".to_vec()).await.unwrap();

        let deleted = kv.clear_prefix(&ctx, "temp:").await.unwrap();
        assert_eq!(deleted, 2);

        assert!(!kv.exists(&ctx, "temp:1").await.unwrap());
        assert!(!kv.exists(&ctx, "temp:2").await.unwrap());
        assert!(kv.exists(&ctx, "keep:1").await.unwrap());
    }

    #[tokio::test]
    async fn test_count_prefix() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        kv.put(&ctx, "actor:a", b"1".to_vec()).await.unwrap();
        kv.put(&ctx, "actor:b", b"2".to_vec()).await.unwrap();
        kv.put(&ctx, "node:n1", b"3".to_vec()).await.unwrap();

        let count = kv.count_prefix(&ctx, "actor:").await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_stats() {
        let kv = InMemoryKVStore::new();
        let ctx = test_ctx();

        kv.put(&ctx, "k1", b"v1".to_vec()).await.unwrap();
        kv.put(&ctx, "k2", b"v2".to_vec()).await.unwrap();

        let stats = kv.get_stats(&ctx).await.unwrap();
        assert_eq!(stats.total_keys, 2);
        assert_eq!(stats.backend_type, "InMemory");
        assert!(stats.total_size_bytes > 0);
    }
}
