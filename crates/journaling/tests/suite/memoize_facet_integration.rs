// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for MemoizeFacet using the full ServiceLocator stack.

use async_trait::async_trait;
use plexspaces_actor::{
    InitializableServiceLocator, JournalStorage, KeyValueStore, KeyValueStoreError, LockManager,
    RequestContext, ServiceLocator,
};
use plexspaces_facet::{Facet, FacetFactory, InterceptResult};
use plexspaces_journaling::{
    MemoizeFacet, MemoizeFacetFactory, SqliteJournalStorage, MEMOIZE_FACET_DEFAULT_PRIORITY,
};
use plexspaces_locks::sql::SqliteLockManager;
use plexspaces_services::ServiceLocatorImpl;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Minimal in-process KeyValueStore
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
        _: &RequestContext,
        key: &str,
    ) -> Result<Option<Vec<u8>>, KeyValueStoreError> {
        Ok(self.data.lock().unwrap().get(key).cloned())
    }
    async fn put(
        &self,
        _: &RequestContext,
        key: &str,
        value: Vec<u8>,
    ) -> Result<(), KeyValueStoreError> {
        self.data.lock().unwrap().insert(key.to_string(), value);
        Ok(())
    }
    async fn put_with_ttl(
        &self,
        _: &RequestContext,
        key: &str,
        value: Vec<u8>,
        _: std::time::Duration,
    ) -> Result<(), KeyValueStoreError> {
        self.data.lock().unwrap().insert(key.to_string(), value);
        Ok(())
    }
    async fn delete(&self, _: &RequestContext, key: &str) -> Result<(), KeyValueStoreError> {
        self.data.lock().unwrap().remove(key);
        Ok(())
    }
    async fn exists(&self, _: &RequestContext, key: &str) -> Result<bool, KeyValueStoreError> {
        Ok(self.data.lock().unwrap().contains_key(key))
    }
    async fn list_keys(
        &self,
        _: &RequestContext,
        prefix: &str,
    ) -> Result<Vec<String>, KeyValueStoreError> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }
    async fn cas(
        &self,
        _: &RequestContext,
        key: &str,
        expected: Option<Vec<u8>>,
        new_value: Vec<u8>,
    ) -> Result<bool, KeyValueStoreError> {
        let mut data = self.data.lock().unwrap();
        if data.get(key).cloned() == expected {
            data.insert(key.to_string(), new_value);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    async fn increment(
        &self,
        _: &RequestContext,
        key: &str,
        delta: i64,
    ) -> Result<i64, KeyValueStoreError> {
        let mut data = self.data.lock().unwrap();
        let v: i64 = data
            .get(key)
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
            + delta;
        data.insert(key.to_string(), v.to_string().into_bytes());
        Ok(v)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
async fn build_service_locator() -> Arc<dyn ServiceLocator> {
    let sl = Arc::new(ServiceLocatorImpl::new());

    let journal: Arc<dyn JournalStorage> =
        Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    sl.register_journal_storage(journal).await;

    let lock_mgr: Arc<dyn LockManager + Send + Sync> =
        Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
    sl.register_lock_manager(lock_mgr).await;

    let kv: Arc<dyn KeyValueStore> = MemKv::new();
    sl.register_keyvalue_store(kv).await;

    sl as Arc<dyn ServiceLocator>
}

fn make_facet(kv: Arc<dyn KeyValueStore>, ttl: u32, max: u32, handlers: Vec<&str>) -> MemoizeFacet {
    MemoizeFacet::new(
        kv,
        serde_json::json!({
            "ttl_seconds": ttl,
            "max_entries": max,
            "handler_names": handlers,
        }),
        MEMOIZE_FACET_DEFAULT_PRIORITY,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Factory creates a working memoize facet from ServiceLocator.
#[tokio::test]
async fn test_memoize_factory_roundtrip_via_service_locator() {
    let sl = build_service_locator().await;
    let factory = MemoizeFacetFactory::new(sl);

    let mut facet = factory
        .create(serde_json::json!({ "ttl_seconds": 0, "max_entries": 0 }))
        .await
        .unwrap();

    assert_eq!(facet.facet_type(), "memoize");

    facet
        .on_attach("actor-sl-1", serde_json::json!({}))
        .await
        .unwrap();

    // Miss
    assert!(matches!(
        facet
            .before_method("process", b"data", &HashMap::new())
            .await
            .unwrap(),
        InterceptResult::Continue
    ));

    // Store result
    facet
        .after_method("process", b"data", b"computed-result", &HashMap::new())
        .await
        .unwrap();

    // Hit
    let hit = facet
        .before_method("process", b"data", &HashMap::new())
        .await
        .unwrap();
    assert!(matches!(&hit, InterceptResult::ShortCircuit(r) if r == b"computed-result"));
}

/// Verify payload-only cache key — message ID is not part of args.
#[tokio::test]
async fn test_memoize_payload_only_not_message_id() {
    let kv: Arc<dyn KeyValueStore> = MemKv::new();
    let mut facet = make_facet(kv, 0, 0, vec![]);
    facet
        .on_attach("actor-payload-1", serde_json::json!({}))
        .await
        .unwrap();

    let payload = b"stable-payload";
    facet
        .after_method("compute", payload, b"result-v1", &HashMap::new())
        .await
        .unwrap();

    // Same payload → hit regardless of conceptual "message ID"
    let hit = facet
        .before_method("compute", payload, &HashMap::new())
        .await
        .unwrap();
    assert!(
        matches!(&hit, InterceptResult::ShortCircuit(r) if r == b"result-v1"),
        "Expected cache hit for identical payload"
    );
}

/// For tell/cast: ShortCircuit is returned; framework decides about reply.
#[tokio::test]
async fn test_memoize_tell_pattern_short_circuits_handler() {
    let kv: Arc<dyn KeyValueStore> = MemKv::new();
    let mut facet = make_facet(kv, 0, 0, vec![]);
    facet
        .on_attach("actor-tell-1", serde_json::json!({}))
        .await
        .unwrap();

    facet
        .after_method("ingest", b"event-data", b"ok", &HashMap::new())
        .await
        .unwrap();

    let res = facet
        .before_method("ingest", b"event-data", &HashMap::new())
        .await
        .unwrap();
    // ShortCircuit skips the actor; for tell the framework won't send a reply.
    assert!(matches!(res, InterceptResult::ShortCircuit(_)));
}

/// Handler filter: only listed handlers are intercepted.
#[tokio::test]
async fn test_memoize_handler_filter_integration() {
    let kv: Arc<dyn KeyValueStore> = MemKv::new();
    let mut facet = make_facet(kv, 0, 0, vec!["allowed_handler"]);
    facet
        .on_attach("actor-filter-1", serde_json::json!({}))
        .await
        .unwrap();

    // Prime cache for both
    facet
        .after_method("allowed_handler", b"x", b"cached-allowed", &HashMap::new())
        .await
        .unwrap();
    facet
        .after_method("blocked_handler", b"x", b"cached-blocked", &HashMap::new())
        .await
        .unwrap();

    // allowed_handler: hit
    assert!(matches!(
        facet
            .before_method("allowed_handler", b"x", &HashMap::new())
            .await
            .unwrap(),
        InterceptResult::ShortCircuit(_)
    ));

    // blocked_handler: NOT intercepted → always miss
    assert!(matches!(
        facet
            .before_method("blocked_handler", b"x", &HashMap::new())
            .await
            .unwrap(),
        InterceptResult::Continue
    ));
}

/// max_entries: oldest entry is evicted when capacity is reached.
#[tokio::test]
async fn test_memoize_max_entries_eviction_integration() {
    let kv: Arc<dyn KeyValueStore> = MemKv::new();
    let mut facet = make_facet(kv, 0, 2, vec![]);
    facet
        .on_attach("actor-evict-1", serde_json::json!({}))
        .await
        .unwrap();

    facet
        .after_method("op", b"a", b"va", &HashMap::new())
        .await
        .unwrap();
    facet
        .after_method("op", b"b", b"vb", &HashMap::new())
        .await
        .unwrap();
    facet
        .after_method("op", b"c", b"vc", &HashMap::new())
        .await
        .unwrap();

    // "a" should have been evicted
    assert!(matches!(
        facet
            .before_method("op", b"a", &HashMap::new())
            .await
            .unwrap(),
        InterceptResult::Continue
    ));

    // "b" and "c" should still be cached
    assert!(matches!(
        facet
            .before_method("op", b"b", &HashMap::new())
            .await
            .unwrap(),
        InterceptResult::ShortCircuit(_)
    ));
    assert!(matches!(
        facet
            .before_method("op", b"c", &HashMap::new())
            .await
            .unwrap(),
        InterceptResult::ShortCircuit(_)
    ));
}
