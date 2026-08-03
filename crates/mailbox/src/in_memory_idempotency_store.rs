// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! In-memory idempotency store with per-(tenant, namespace) LRU buckets.
//!
//! # Purpose
//! Default zero-setup backend for exactly-once message deduplication.
//! Each (tenant_id, namespace) pair gets its own bounded LRU bucket so one
//! noisy tenant cannot evict another tenant's dedup entries.
//!
//! # Atomicity
//! `check_and_record` uses a per-key `Mutex<EntryState>` stored in a
//! `DashMap`.  The mutex is held only for the state transition, not for I/O,
//! so contention is minimal. Concurrent retries for the same key will have
//! exactly one winner that gets `FirstSeen`; all others see `Duplicate` or
//! `InFlight`.
//!
//! # Eviction
//! Entries are evicted lazily on `check_and_record` (TTL check) and eagerly
//! via `cleanup_expired`. When a bucket exceeds `capacity_per_bucket` the
//! oldest entry (by insertion order) is dropped.

use async_trait::async_trait;
use bytes::Bytes;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use dashmap::DashMap;
use plexspaces_service_traits::{IdempotencyError, IdempotencyOutcome, IdempotencyResult, IdempotencyStore};

/// State of a single idempotency entry.
#[derive(Debug, Clone)]
enum EntryState {
    /// Request received, processing in progress.
    InFlight { inserted_at: Instant },
    /// Processing complete; response cached.
    Complete {
        inserted_at: Instant,
        response: Option<Bytes>,
    },
}

impl EntryState {
    fn inserted_at(&self) -> Instant {
        match self {
            EntryState::InFlight { inserted_at } => *inserted_at,
            EntryState::Complete { inserted_at, .. } => *inserted_at,
        }
    }
}

/// A single LRU bucket for one (tenant_id, namespace) pair.
struct Bucket {
    entries: HashMap<String, Arc<Mutex<EntryState>>>,
    /// Insertion-order queue for LRU eviction.
    order: VecDeque<String>,
    capacity: usize,
    ttl: Duration,
}

impl Bucket {
    fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity.min(1024)),
            order: VecDeque::with_capacity(capacity.min(1024)),
            capacity,
            ttl,
        }
    }

    /// Returns the entry for `key` if it exists and is not expired.
    fn get_valid(&mut self, key: &str) -> Option<Arc<Mutex<EntryState>>> {
        if let Some(entry) = self.entries.get(key) {
            let inserted_at = entry.lock().unwrap().inserted_at();
            if inserted_at.elapsed() > self.ttl {
                self.entries.remove(key);
                self.order.retain(|k| k != key);
                return None;
            }
            return Some(entry.clone());
        }
        None
    }

    /// Insert a new in-flight entry, evicting the oldest if over capacity.
    fn insert_in_flight(&mut self, key: String) -> Arc<Mutex<EntryState>> {
        let state = Arc::new(Mutex::new(EntryState::InFlight {
            inserted_at: Instant::now(),
        }));
        if self.entries.len() >= self.capacity {
            // Evict oldest
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, state.clone());
        state
    }

    /// Evict all entries older than TTL. Returns number evicted.
    fn evict_expired(&mut self) -> usize {
        let ttl = self.ttl;
        let before = self.entries.len();
        let expired_keys: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, v)| v.lock().unwrap().inserted_at().elapsed() > ttl)
            .map(|(k, _)| k.clone())
            .collect();
        for k in &expired_keys {
            self.entries.remove(k);
            self.order.retain(|o| o != k);
        }
        before - self.entries.len()
    }
}

/// In-memory idempotency store.
///
/// ## Construction
///
/// ```rust,no_run
/// use plexspaces_mailbox::InMemoryIdempotencyStore;
///
/// let store = InMemoryIdempotencyStore::new(10_000, std::time::Duration::from_secs(300));
/// ```
pub struct InMemoryIdempotencyStore {
    /// Buckets keyed by `"tenant_id\x00namespace"`.
    buckets: DashMap<String, Mutex<Bucket>>,
    capacity_per_bucket: usize,
    ttl: Duration,
}

impl InMemoryIdempotencyStore {
    /// Create a new in-memory idempotency store.
    ///
    /// - `capacity_per_bucket`: max entries per (tenant, namespace) LRU bucket.
    /// - `ttl`: how long a processed key is remembered.
    pub fn new(capacity_per_bucket: usize, ttl: Duration) -> Self {
        Self {
            buckets: DashMap::new(),
            capacity_per_bucket,
            ttl,
        }
    }

    fn bucket_key(tenant_id: &str, namespace: &str) -> String {
        format!("{}\x00{}", tenant_id, namespace)
    }

    fn get_or_create_bucket(&self, tenant_id: &str, namespace: &str) -> dashmap::mapref::one::RefMut<'_, String, Mutex<Bucket>> {
        let key = Self::bucket_key(tenant_id, namespace);
        self.buckets
            .entry(key)
            .or_insert_with(|| Mutex::new(Bucket::new(self.capacity_per_bucket, self.ttl)))
    }
}

#[async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn check_and_record(
        &self,
        tenant_id: &str,
        namespace: &str,
        key: &str,
    ) -> IdempotencyResult<IdempotencyOutcome> {
        let bucket_ref = self.get_or_create_bucket(tenant_id, namespace);
        let mut bucket = bucket_ref.lock().map_err(|e| {
            IdempotencyError::Storage(format!("bucket lock poisoned: {}", e))
        })?;

        // Check existing entry first
        if let Some(entry_arc) = bucket.get_valid(key) {
            let state = entry_arc.lock().map_err(|e| {
                IdempotencyError::Storage(format!("entry lock poisoned: {}", e))
            })?;
            return Ok(match &*state {
                EntryState::InFlight { .. } => IdempotencyOutcome::InFlight,
                EntryState::Complete { response, .. } => {
                    IdempotencyOutcome::Duplicate(response.clone())
                }
            });
        }

        // First time seen — insert in-flight marker
        bucket.insert_in_flight(key.to_string());
        Ok(IdempotencyOutcome::FirstSeen)
    }

    async fn complete_record(
        &self,
        tenant_id: &str,
        namespace: &str,
        key: &str,
        response: Option<Bytes>,
    ) -> IdempotencyResult<()> {
        let bucket_ref = self.get_or_create_bucket(tenant_id, namespace);
        let mut bucket = bucket_ref.lock().map_err(|e| {
            IdempotencyError::Storage(format!("bucket lock poisoned: {}", e))
        })?;

        if let Some(entry_arc) = bucket.entries.get(key) {
            let mut state = entry_arc.lock().map_err(|e| {
                IdempotencyError::Storage(format!("entry lock poisoned: {}", e))
            })?;
            let inserted_at = state.inserted_at();
            *state = EntryState::Complete {
                inserted_at,
                response,
            };
        }
        // If entry was evicted between check_and_record and complete_record (unlikely but
        // possible under extreme LRU pressure), silently ignore — the next retry will
        // get FirstSeen again, which is safe.
        Ok(())
    }

    async fn cleanup_expired(&self) -> IdempotencyResult<usize> {
        let mut total = 0usize;
        for entry in self.buckets.iter() {
            if let Ok(mut bucket) = entry.value().lock() {
                total += bucket.evict_expired();
            }
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_service_traits::IdempotencyOutcome;

    fn store() -> InMemoryIdempotencyStore {
        InMemoryIdempotencyStore::new(100, Duration::from_secs(60))
    }

    #[tokio::test]
    async fn first_call_returns_first_seen() {
        let s = store();
        let outcome = s.check_and_record("t1", "ns", "key1").await.unwrap();
        assert!(matches!(outcome, IdempotencyOutcome::FirstSeen));
    }

    #[tokio::test]
    async fn in_flight_before_complete() {
        let s = store();
        s.check_and_record("t1", "ns", "key1").await.unwrap();
        let outcome = s.check_and_record("t1", "ns", "key1").await.unwrap();
        assert!(matches!(outcome, IdempotencyOutcome::InFlight));
    }

    #[tokio::test]
    async fn duplicate_after_complete() {
        let s = store();
        s.check_and_record("t1", "ns", "key1").await.unwrap();
        let payload = Bytes::from_static(b"hello");
        s.complete_record("t1", "ns", "key1", Some(payload.clone()))
            .await
            .unwrap();
        let outcome = s.check_and_record("t1", "ns", "key1").await.unwrap();
        match outcome {
            IdempotencyOutcome::Duplicate(Some(r)) => assert_eq!(r, payload),
            other => panic!("expected Duplicate, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn duplicate_without_response() {
        let s = store();
        s.check_and_record("t1", "ns", "key1").await.unwrap();
        s.complete_record("t1", "ns", "key1", None).await.unwrap();
        let outcome = s.check_and_record("t1", "ns", "key1").await.unwrap();
        assert!(matches!(outcome, IdempotencyOutcome::Duplicate(None)));
    }

    #[tokio::test]
    async fn tenant_isolation() {
        let s = store();
        // Same key, different tenants — independent
        s.check_and_record("tenant-a", "ns", "key1").await.unwrap();
        s.complete_record("tenant-a", "ns", "key1", None)
            .await
            .unwrap();

        let outcome = s.check_and_record("tenant-b", "ns", "key1").await.unwrap();
        assert!(
            matches!(outcome, IdempotencyOutcome::FirstSeen),
            "Different tenant should see FirstSeen"
        );
    }

    #[tokio::test]
    async fn namespace_isolation() {
        let s = store();
        s.check_and_record("t1", "ns-a", "key1").await.unwrap();
        s.complete_record("t1", "ns-a", "key1", None).await.unwrap();

        let outcome = s.check_and_record("t1", "ns-b", "key1").await.unwrap();
        assert!(
            matches!(outcome, IdempotencyOutcome::FirstSeen),
            "Different namespace should see FirstSeen"
        );
    }

    #[tokio::test]
    async fn lru_eviction_at_capacity() {
        let s = InMemoryIdempotencyStore::new(3, Duration::from_secs(60));
        // Fill to capacity
        for i in 0..3usize {
            s.check_and_record("t", "ns", &format!("key{}", i))
                .await
                .unwrap();
            s.complete_record("t", "ns", &format!("key{}", i), None)
                .await
                .unwrap();
        }
        // Insert one more — oldest (key0) should be evicted
        s.check_and_record("t", "ns", "key_new").await.unwrap();
        // key0 was evicted — next call should be FirstSeen again
        let outcome = s.check_and_record("t", "ns", "key0").await.unwrap();
        assert!(
            matches!(outcome, IdempotencyOutcome::FirstSeen),
            "Evicted key should be FirstSeen again"
        );
    }

    #[tokio::test]
    async fn expired_entries_cleaned() {
        let s = InMemoryIdempotencyStore::new(100, Duration::from_millis(50));
        s.check_and_record("t", "ns", "key1").await.unwrap();
        s.complete_record("t", "ns", "key1", None).await.unwrap();

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_millis(100)).await;

        let evicted = s.cleanup_expired().await.unwrap();
        assert!(evicted > 0, "Should evict expired entry");

        // After cleanup, key should be seen fresh
        let outcome = s.check_and_record("t", "ns", "key1").await.unwrap();
        assert!(matches!(outcome, IdempotencyOutcome::FirstSeen));
    }

    #[tokio::test]
    async fn cleanup_expired_returns_count() {
        let s = InMemoryIdempotencyStore::new(100, Duration::from_millis(50));
        for i in 0..5usize {
            s.check_and_record("t", "ns", &format!("k{}", i))
                .await
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        let evicted = s.cleanup_expired().await.unwrap();
        assert_eq!(evicted, 5);
    }
}
