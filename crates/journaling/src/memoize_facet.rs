// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Memoize facet for transparent result caching
//!
//! ## Purpose
//! Caches actor handler results in KV storage keyed by `(actor_id, handler, args)`.
//! On a cache hit the actor handler is skipped entirely via `InterceptResult::ShortCircuit`.
//!
//! ## Architecture Context
//! Part of the PlexSpaces facet system. Priority 700 — runs after metrics (~800) but
//! before domain logic (~100-500), so the actor itself is never called on a hit.
//!
//! ## Cache Key Format
//! `<key_prefix><hex(hash(actor_id + ":" + handler + ":" + hex(args)))>`
//!
//! ## TTL / max_entries
//! TTL envelope: `{"v":"<hex(result)>","exp":<unix_ms_or_0>}` stored in KV.
//! LRU key list kept at `<key_prefix>__lru` for max_entries eviction.

use async_trait::async_trait;
use plexspaces_common::KeyValueStore;
use plexspaces_common::{RequestContext, RequestContextExt};
use plexspaces_facet::{ErrorHandling, Facet, FacetError, InterceptResult};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Default priority for MemoizeFacet — between metrics (800) and domain (~100-500)
pub const MEMOIZE_FACET_DEFAULT_PRIORITY: i32 = 700;

/// Memoize facet — caches handler results in KV to skip actor execution on hits.
pub struct MemoizeFacet {
    config_value: Value,
    priority: i32,

    /// Actor ID, tenant_id, and namespace set on `on_attach` (populated by the facet system).
    actor_id: Arc<RwLock<Option<String>>>,
    /// Tenant ID from the actor's RequestContext — used to scope KV operations.
    tenant_id: Arc<RwLock<String>>,
    /// Namespace from the actor's RequestContext.
    namespace: Arc<RwLock<String>>,

    /// KV store for caching
    kv_store: Arc<dyn KeyValueStore>,

    /// Resolved config fields (from proto MemoizeFacetConfig)
    key_prefix: String,
    ttl_seconds: u32,
    max_entries: u32,
    /// Empty = intercept all handlers
    handler_names: Vec<String>,

    /// Hit / miss counters (in-process; also emitted via metrics crate)
    hits: Arc<RwLock<u64>>,
    misses: Arc<RwLock<u64>>,
    evictions: Arc<RwLock<u64>>,
}

impl MemoizeFacet {
    /// Create a new `MemoizeFacet`.
    ///
    /// ## Arguments
    /// * `kv_store` - KV store backend used for cached results
    /// * `config`   - Facet config (fields from proto `MemoizeFacetConfig`)
    /// * `priority` - Facet priority (default: 700)
    pub fn new(kv_store: Arc<dyn KeyValueStore>, config: Value, priority: i32) -> Self {
        let key_prefix = config
            .get("key_prefix")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let ttl_seconds = config
            .get("ttl_seconds")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(0);

        let max_entries = config
            .get("max_entries")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(0);

        let handler_names = config
            .get("handler_names")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            config_value: config,
            priority,
            actor_id: Arc::new(RwLock::new(None)),
            tenant_id: Arc::new(RwLock::new(String::new())),
            namespace: Arc::new(RwLock::new(String::new())),
            kv_store,
            key_prefix,
            ttl_seconds,
            max_entries,
            handler_names,
            hits: Arc::new(RwLock::new(0)),
            misses: Arc::new(RwLock::new(0)),
            evictions: Arc::new(RwLock::new(0)),
        }
    }

    /// Returns whether `handler` should be intercepted by this facet.
    fn should_intercept(&self, handler: &str) -> bool {
        self.handler_names.is_empty() || self.handler_names.iter().any(|h| h == handler)
    }

    /// Builds the effective key prefix for `actor_id`.
    fn effective_prefix(&self, actor_id: &str) -> String {
        if self.key_prefix.is_empty() {
            format!("memo:{}:", actor_id)
        } else {
            self.key_prefix.clone()
        }
    }

    /// Computes the cache key.
    ///
    /// If the caller set `headers["memo_key"]` the value is used verbatim (with the prefix),
    /// giving callers an explicit stable key that survives per-request nonces inside the payload.
    /// Otherwise the key is derived from `hash(actor_id + ":" + handler + ":" + args)`.
    fn cache_key(
        &self,
        actor_id: &str,
        handler: &str,
        args: &[u8],
        headers: &std::collections::HashMap<String, String>,
    ) -> String {
        let prefix = self.effective_prefix(actor_id);
        if let Some(explicit) = headers.get("memo_key") {
            if !explicit.is_empty() {
                return format!("{}{}", prefix, explicit);
            }
        }
        let mut hasher = DefaultHasher::new();
        actor_id.hash(&mut hasher);
        ":".hash(&mut hasher);
        handler.hash(&mut hasher);
        ":".hash(&mut hasher);
        args.hash(&mut hasher);
        let hash = hasher.finish();
        format!("{}{:016x}", prefix, hash)
    }

    /// Key for the LRU list (insertion-ordered list of cache keys).
    fn lru_list_key(&self, actor_id: &str) -> String {
        format!("{}__lru", self.effective_prefix(actor_id))
    }

    /// Current unix milliseconds.
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Read cached result bytes if present and not expired. Returns `None` on miss.
    async fn read_cache(&self, key: &str, ctx: &RequestContext) -> Option<Vec<u8>> {
        let raw = self.kv_store.get(ctx, key).await.ok()??;
        let envelope: Value = serde_json::from_slice(&raw).ok()?;
        let exp = envelope.get("exp").and_then(|v| v.as_u64()).unwrap_or(0);
        if exp > 0 && Self::now_ms() > exp {
            // Expired — treat as miss; background eviction ok to skip for correctness
            return None;
        }
        let hex_v = envelope.get("v").and_then(|v| v.as_str())?;
        hex_decode(hex_v)
    }

    /// Write result bytes to cache with TTL / max_entries enforcement.
    async fn write_cache(&self, key: &str, result: &[u8], actor_id: &str, ctx: &RequestContext) {
        let exp = if self.ttl_seconds > 0 {
            Self::now_ms() + (self.ttl_seconds as u64) * 1000
        } else {
            0
        };

        let envelope = serde_json::json!({
            "v": hex_encode(result),
            "exp": exp
        });

        let envelope_bytes = match serde_json::to_vec(&envelope) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("memoize: failed to serialize cache envelope: {}", e);
                return;
            }
        };

        if let Err(e) = self.kv_store.put(ctx, key, envelope_bytes).await {
            tracing::warn!("memoize: KV put failed for key {}: {}", key, e);
            return;
        }

        // Update LRU list and enforce max_entries
        if self.max_entries > 0 {
            self.enforce_max_entries(key, actor_id, ctx).await;
        }
    }

    /// Append `key` to LRU list and evict oldest if over `max_entries`.
    async fn enforce_max_entries(&self, key: &str, actor_id: &str, ctx: &RequestContext) {
        let lru_key = self.lru_list_key(actor_id);

        let mut list: Vec<String> = match self.kv_store.get(ctx, &lru_key).await {
            Ok(Some(bytes)) => serde_json::from_slice(&bytes).unwrap_or_default(),
            _ => vec![],
        };

        // Remove existing occurrence of key (move-to-back behaviour)
        list.retain(|k| k != key);
        list.push(key.to_string());

        // Evict from front while over limit
        while list.len() > self.max_entries as usize {
            let oldest = list.remove(0);
            let _ = self.kv_store.delete(ctx, &oldest).await;
            *self.evictions.write().await += 1;
            metrics::counter!("plexspaces_memoize_evictions_total",
                "actor_id" => actor_id.to_string()
            )
            .increment(1);
            tracing::debug!(actor_id = %actor_id, key = %oldest, "memoize: evicted cache entry");
        }

        // Persist updated list
        if let Ok(bytes) = serde_json::to_vec(&list) {
            let _ = self.kv_store.put(ctx, &lru_key, bytes).await;
        }
    }
}

/// Encode bytes to lowercase hex string.
fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|byte| format!("{:02x}", byte)).collect()
}

/// Decode lowercase hex string to bytes.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[async_trait]
impl Facet for MemoizeFacet {
    fn facet_type(&self) -> &str {
        "memoize"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, config: Value) -> Result<(), FacetError> {
        *self.actor_id.write().await = Some(actor_id.to_string());
        // _tenant_id and _namespace are injected by the facet system from the actor's
        // RequestContext, ensuring KV operations are properly tenant-scoped.
        if let Some(tid) = config.get("_tenant_id").and_then(|v| v.as_str()) {
            *self.tenant_id.write().await = tid.to_string();
        }
        if let Some(ns) = config.get("_namespace").and_then(|v| v.as_str()) {
            *self.namespace.write().await = ns.to_string();
        }
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        let mut aid = self.actor_id.write().await;
        *aid = None;
        Ok(())
    }

    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
        headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        if !self.should_intercept(method) {
            return Ok(InterceptResult::Continue);
        }

        let actor_id = {
            let guard = self.actor_id.read().await;
            match guard.as_ref() {
                Some(id) => id.clone(),
                None => return Ok(InterceptResult::Continue),
            }
        };
        let tenant_id = self.tenant_id.read().await.clone();
        let namespace = self.namespace.read().await.clone();
        let ctx = RequestContext::new_without_auth(tenant_id, namespace);
        let key = self.cache_key(&actor_id, method, args, headers);

        if let Some(cached) = self.read_cache(&key, &ctx).await {
            let mut hits = self.hits.write().await;
            *hits += 1;
            tracing::debug!(
                actor_id = %actor_id,
                method = %method,
                key = %key,
                "memoize: cache hit"
            );
            metrics::counter!("plexspaces_memoize_hits_total",
                "actor_id" => actor_id.clone(),
                "method" => method.to_string()
            )
            .increment(1);
            return Ok(InterceptResult::ShortCircuit(cached));
        }

        let mut misses = self.misses.write().await;
        *misses += 1;
        tracing::debug!(
            actor_id = %actor_id,
            method = %method,
            key = %key,
            "memoize: cache miss"
        );
        metrics::counter!("plexspaces_memoize_misses_total",
            "actor_id" => actor_id.clone(),
            "method" => method.to_string()
        )
        .increment(1);

        Ok(InterceptResult::Continue)
    }

    async fn after_method(
        &self,
        method: &str,
        args: &[u8],
        result: &[u8],
        headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        if !self.should_intercept(method) {
            return Ok(InterceptResult::Continue);
        }

        let actor_id = {
            let guard = self.actor_id.read().await;
            match guard.as_ref() {
                Some(id) => id.clone(),
                None => return Ok(InterceptResult::Continue),
            }
        };
        let tenant_id = self.tenant_id.read().await.clone();
        let namespace = self.namespace.read().await.clone();
        let ctx = RequestContext::new_without_auth(tenant_id, namespace);
        let key = self.cache_key(&actor_id, method, args, headers);

        self.write_cache(&key, result, &actor_id, &ctx).await;

        Ok(InterceptResult::Continue)
    }

    async fn on_error(&self, _method: &str, _error: &str) -> Result<ErrorHandling, FacetError> {
        Ok(ErrorHandling::Propagate)
    }

    fn get_config(&self) -> Value {
        self.config_value.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_actor::KeyValueStoreError;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // ---------------------------------------------------------------------------
    // Minimal in-process KeyValueStore for unit tests
    // ---------------------------------------------------------------------------
    struct MemKv {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemKv {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                data: Mutex::new(HashMap::new()),
            })
        }
    }

    #[async_trait]
    impl KeyValueStore for MemKv {
        async fn get(
            &self,
            _ctx: &RequestContext,
            key: &str,
        ) -> Result<Option<Vec<u8>>, KeyValueStoreError> {
            Ok(self.data.lock().unwrap().get(key).cloned())
        }

        async fn put(
            &self,
            _ctx: &RequestContext,
            key: &str,
            value: Vec<u8>,
        ) -> Result<(), KeyValueStoreError> {
            self.data.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }

        async fn put_with_ttl(
            &self,
            _ctx: &RequestContext,
            key: &str,
            value: Vec<u8>,
            _ttl: std::time::Duration,
        ) -> Result<(), KeyValueStoreError> {
            self.data.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }

        async fn delete(&self, _ctx: &RequestContext, key: &str) -> Result<(), KeyValueStoreError> {
            self.data.lock().unwrap().remove(key);
            Ok(())
        }

        async fn exists(
            &self,
            _ctx: &RequestContext,
            key: &str,
        ) -> Result<bool, KeyValueStoreError> {
            Ok(self.data.lock().unwrap().contains_key(key))
        }

        async fn list_keys(
            &self,
            _ctx: &RequestContext,
            prefix: &str,
        ) -> Result<Vec<String>, KeyValueStoreError> {
            let keys: Vec<String> = self
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect();
            Ok(keys)
        }

        async fn cas(
            &self,
            _ctx: &RequestContext,
            key: &str,
            expected: Option<Vec<u8>>,
            new_value: Vec<u8>,
        ) -> Result<bool, KeyValueStoreError> {
            let mut data = self.data.lock().unwrap();
            let current = data.get(key).cloned();
            if current == expected {
                data.insert(key.to_string(), new_value);
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn increment(
            &self,
            _ctx: &RequestContext,
            key: &str,
            delta: i64,
        ) -> Result<i64, KeyValueStoreError> {
            let mut data = self.data.lock().unwrap();
            let current: i64 = data
                .get(key)
                .and_then(|b| std::str::from_utf8(b).ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let new_val = current + delta;
            data.insert(key.to_string(), new_val.to_string().into_bytes());
            Ok(new_val)
        }
    }

    fn make_facet(
        kv: Arc<dyn KeyValueStore>,
        ttl: u32,
        max: u32,
        handlers: Vec<&str>,
    ) -> MemoizeFacet {
        let config = serde_json::json!({
            "ttl_seconds": ttl,
            "max_entries": max,
            "handler_names": handlers,
        });
        MemoizeFacet::new(kv, config, MEMOIZE_FACET_DEFAULT_PRIORITY)
    }

    #[tokio::test]
    async fn test_facet_type() {
        let kv = MemKv::new();
        let facet = make_facet(kv, 0, 0, vec![]);
        assert_eq!(facet.facet_type(), "memoize");
    }

    #[tokio::test]
    async fn test_default_priority() {
        let kv = MemKv::new();
        let facet = make_facet(kv, 0, 0, vec![]);
        assert_eq!(facet.get_priority(), MEMOIZE_FACET_DEFAULT_PRIORITY);
    }

    #[tokio::test]
    async fn test_cache_miss_then_hit() {
        let kv = MemKv::new();
        let mut facet = make_facet(kv, 0, 0, vec![]);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        let args = b"payload";
        let result = b"result-data";

        // First call: miss → Continue
        let before = facet
            .before_method("compute", args, &std::collections::HashMap::new())
            .await
            .unwrap();
        assert!(matches!(before, InterceptResult::Continue));

        // Store result
        facet
            .after_method("compute", args, result, &std::collections::HashMap::new())
            .await
            .unwrap();

        // Second call: hit → ShortCircuit with cached result
        let before2 = facet
            .before_method("compute", args, &std::collections::HashMap::new())
            .await
            .unwrap();
        match before2 {
            InterceptResult::ShortCircuit(cached) => {
                assert_eq!(cached, result);
            }
            other => panic!("Expected ShortCircuit, got {:?}", other),
        }

        assert_eq!(*facet.hits.read().await, 1);
        assert_eq!(*facet.misses.read().await, 1);
    }

    #[tokio::test]
    async fn test_different_args_different_keys() {
        let kv = MemKv::new();
        let mut facet = make_facet(kv, 0, 0, vec![]);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        facet
            .after_method(
                "op",
                b"args-a",
                b"result-a",
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();
        facet
            .after_method(
                "op",
                b"args-b",
                b"result-b",
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        let hit_a = facet
            .before_method("op", b"args-a", &std::collections::HashMap::new())
            .await
            .unwrap();
        let hit_b = facet
            .before_method("op", b"args-b", &std::collections::HashMap::new())
            .await
            .unwrap();

        assert!(matches!(&hit_a, InterceptResult::ShortCircuit(r) if r == b"result-a"));
        assert!(matches!(&hit_b, InterceptResult::ShortCircuit(r) if r == b"result-b"));
    }

    #[tokio::test]
    async fn test_handler_filter_skips_unlisted() {
        let kv = MemKv::new();
        let mut facet = make_facet(kv, 0, 0, vec!["allowed"]);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Put something in cache for "blocked"
        facet
            .after_method(
                "blocked",
                b"a",
                b"cached",
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // "blocked" should not be intercepted → always Continue
        let res = facet
            .before_method("blocked", b"a", &std::collections::HashMap::new())
            .await
            .unwrap();
        assert!(matches!(res, InterceptResult::Continue));
    }

    #[tokio::test]
    async fn test_handler_filter_intercepts_listed() {
        let kv = MemKv::new();
        let mut facet = make_facet(kv, 0, 0, vec!["allowed"]);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        facet
            .after_method("allowed", b"x", b"y", &std::collections::HashMap::new())
            .await
            .unwrap();

        let res = facet
            .before_method("allowed", b"x", &std::collections::HashMap::new())
            .await
            .unwrap();
        assert!(matches!(res, InterceptResult::ShortCircuit(_)));
    }

    #[tokio::test]
    async fn test_max_entries_eviction() {
        let kv = MemKv::new();
        let kv_ref = kv.clone();
        let mut facet = make_facet(kv, 0, 2, vec![]);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Insert 3 entries with distinct args
        facet
            .after_method("op", b"a", b"va", &std::collections::HashMap::new())
            .await
            .unwrap();
        facet
            .after_method("op", b"b", b"vb", &std::collections::HashMap::new())
            .await
            .unwrap();
        facet
            .after_method("op", b"c", b"vc", &std::collections::HashMap::new())
            .await
            .unwrap();

        // After 3rd insert, first entry should have been evicted
        assert_eq!(*facet.evictions.read().await, 1);

        // First entry should be a miss now
        let res_a = facet
            .before_method("op", b"a", &std::collections::HashMap::new())
            .await
            .unwrap();
        assert!(matches!(res_a, InterceptResult::Continue));

        // Last two should still be hits
        let res_b = facet
            .before_method("op", b"b", &std::collections::HashMap::new())
            .await
            .unwrap();
        let res_c = facet
            .before_method("op", b"c", &std::collections::HashMap::new())
            .await
            .unwrap();
        assert!(matches!(res_b, InterceptResult::ShortCircuit(_)));
        assert!(matches!(res_c, InterceptResult::ShortCircuit(_)));

        // Suppress unused variable warning
        let _ = kv_ref;
    }

    #[tokio::test]
    async fn test_ttl_expired_entry_is_miss() {
        let kv = MemKv::new();
        let mut facet = make_facet(kv, 0, 0, vec![]);

        // Manually inject an already-expired envelope into cache
        let actor_id = "actor-ttl";
        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        let key = facet.cache_key(actor_id, "op", b"args", &std::collections::HashMap::new());
        let expired_envelope = serde_json::json!({
            "v": hex_encode(b"stale"),
            "exp": 1u64  // timestamp in the past
        });

        let ctx = RequestContext::new_without_auth(actor_id.to_string(), "memoize".to_string());
        facet
            .kv_store
            .put(&ctx, &key, serde_json::to_vec(&expired_envelope).unwrap())
            .await
            .unwrap();

        // Should be treated as miss
        let res = facet
            .before_method("op", b"args", &std::collections::HashMap::new())
            .await
            .unwrap();
        assert!(matches!(res, InterceptResult::Continue));
    }

    #[tokio::test]
    async fn test_before_without_attach_is_continue() {
        let kv = MemKv::new();
        let facet = make_facet(kv, 0, 0, vec![]);
        // No on_attach called
        let res = facet
            .before_method("op", b"", &std::collections::HashMap::new())
            .await
            .unwrap();
        assert!(matches!(res, InterceptResult::Continue));
    }

    #[tokio::test]
    async fn test_on_detach_clears_actor_id() {
        let kv = MemKv::new();
        let mut facet = make_facet(kv, 0, 0, vec![]);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();
        facet.on_detach("actor-1").await.unwrap();
        assert!(facet.actor_id.read().await.is_none());
    }

    #[tokio::test]
    async fn test_hex_encode_decode_roundtrip() {
        let data = b"\x00\x01\xff\xab\xcd";
        let encoded = hex_encode(data);
        let decoded = hex_decode(&encoded).unwrap();
        assert_eq!(&decoded, data);
    }
}
