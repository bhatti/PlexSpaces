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

//! In-Memory TupleSpace Storage Backend
//!
//! ## Purpose
//! Provides fast, in-memory storage for TupleSpace tuples.
//! Ideal for development, testing, and single-process deployments.
//!
//! ## Design
//! - **Storage**: HashMap<TupleId, StoredTuple>
//! - **Pattern Matching**: Linear scan (optimized with early exit)
//! - **Leases**: Background task for cleanup
//! - **Blocking Reads**: Tokio notify for wake-up
//!
//! ## Performance Characteristics
//! - **Write**: O(1) - HashMap insert
//! - **Read/Take**: O(n) - Linear scan with pattern matching
//! - **Count**: O(n) - Linear scan
//! - **Memory**: ~1KB per tuple (depends on field count)
//!
//! ## When to Use
//! - ✅ Development and testing
//! - ✅ Single-process applications
//! - ✅ Low-latency requirements (< 1ms)
//! - ❌ Multi-process distributed systems (use Redis)
//! - ❌ Persistence requirements (use PostgreSQL/SQLite)

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use plexspaces_proto::tuplespace::v1::{MemoryStorageConfig, StorageStats};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, RwLock};

use super::TupleSpaceStorage;
use crate::{Pattern, Tuple, TupleSpaceError};

/// Stored tuple with metadata
#[derive(Debug, Clone)]
struct StoredTuple {
    /// Unique tuple ID (UUID)
    _id: String,

    /// The actual tuple
    tuple: Tuple,

    /// When tuple was created
    _created_at: DateTime<Utc>,

    /// When tuple expires (if has lease)
    expires_at: Option<DateTime<Utc>>,

    /// Is lease renewable?
    renewable: bool,
}

impl StoredTuple {
    /// Check if tuple has expired
    fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }

    /// Check if tuple matches pattern
    fn matches(&self, pattern: &Pattern) -> bool {
        pattern.matches(&self.tuple)
    }
}

/// In-memory TupleSpace storage implementation
///
/// ## Thread Safety
/// - Uses `Arc<RwLock<HashMap>>` for concurrent access
/// - Read operations take read lock (multiple readers)
/// - Write/Take operations take write lock (exclusive)
///
/// ## Lease Management
/// - Background task runs every `cleanup_interval_ms`
/// - Removes expired tuples
/// - Notifies waiting readers on tuple removal
pub struct MemoryStorage {
    /// Tuples stored by ID
    tuples: Arc<RwLock<HashMap<String, StoredTuple>>>,

    /// Notify for blocking reads (wake up when tuples added)
    notify: Arc<Notify>,

    /// Configuration
    config: MemoryStorageConfig,

    /// Statistics
    stats: Arc<RwLock<StorageStats>>,
}

impl MemoryStorage {
    /// Create new in-memory storage
    pub fn new(config: MemoryStorageConfig) -> Self {
        // Check cleanup interval before moving config
        let cleanup_interval = config.cleanup_interval_ms;

        let storage = MemoryStorage {
            tuples: Arc::new(RwLock::new(HashMap::with_capacity(
                config.initial_capacity as usize,
            ))),
            notify: Arc::new(Notify::new()),
            config,
            stats: Arc::new(RwLock::new(StorageStats {
                tuple_count: 0,
                memory_bytes: 0,
                total_operations: 0,
                read_operations: 0,
                write_operations: 0,
                take_operations: 0,
                avg_latency_ms: 0.0,
            })),
        };

        // Start cleanup task if interval configured
        if cleanup_interval > 0 {
            storage.start_cleanup_task();
        }

        storage
    }

    /// Start background cleanup task for expired leases
    fn start_cleanup_task(&self) {
        let tuples = self.tuples.clone();
        let notify = self.notify.clone();
        let interval_ms = self.config.cleanup_interval_ms;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(interval_ms)).await;

                // Remove expired tuples
                let mut tuples_write = tuples.write().await;
                let before_count = tuples_write.len();

                tuples_write.retain(|_, stored| !stored.is_expired());

                let removed_count = before_count - tuples_write.len();
                if removed_count > 0 {
                    drop(tuples_write); // Release write lock
                    notify.notify_waiters(); // Wake up waiting readers
                }
            }
        });
    }

    /// Find tuples matching pattern
    async fn find_matching(
        &self,
        pattern: &Pattern,
        max_results: Option<usize>,
    ) -> Vec<StoredTuple> {
        let tuples = self.tuples.read().await;
        let mut matches = Vec::new();

        for stored in tuples.values() {
            if !stored.is_expired() && stored.matches(pattern) {
                matches.push(stored.clone());

                if let Some(max) = max_results {
                    if matches.len() >= max {
                        break;
                    }
                }
            }
        }

        matches
    }

    /// Update statistics
    async fn update_stats<F>(&self, update_fn: F)
    where
        F: FnOnce(&mut StorageStats),
    {
        let mut stats = self.stats.write().await;
        update_fn(&mut stats);
    }
}

#[async_trait]
impl TupleSpaceStorage for MemoryStorage {
    async fn write(&self, tuple: Tuple) -> Result<String, TupleSpaceError> {
        let start = std::time::Instant::now();

        // Generate tuple ID
        let tuple_id = ulid::Ulid::new().to_string();

        // Extract lease information
        let (expires_at, renewable) = if let Some(lease) = tuple.lease() {
            (Some(lease.expires_at()), lease.is_renewable())
        } else {
            (None, false)
        };

        // Create stored tuple
        let stored = StoredTuple {
            _id: tuple_id.clone(),
            tuple,
            _created_at: Utc::now(),
            expires_at,
            renewable,
        };

        // Insert into storage
        {
            let mut tuples = self.tuples.write().await;
            tuples.insert(tuple_id.clone(), stored);
        }

        // Notify waiting readers
        self.notify.notify_waiters();

        // Update stats
        let elapsed = start.elapsed();
        self.update_stats(|stats| {
            stats.write_operations += 1;
            stats.total_operations += 1;
            stats.tuple_count += 1;

            // Update average latency (exponential moving average)
            let alpha = 0.1;
            stats.avg_latency_ms =
                alpha * elapsed.as_secs_f32() * 1000.0 + (1.0 - alpha) * stats.avg_latency_ms;
        })
        .await;

        Ok(tuple_id)
    }

    async fn write_batch(&self, tuples: Vec<Tuple>) -> Result<Vec<String>, TupleSpaceError> {
        let mut ids = Vec::with_capacity(tuples.len());

        for tuple in tuples {
            let id = self.write(tuple).await?;
            ids.push(id);
        }

        Ok(ids)
    }

    async fn read(
        &self,
        pattern: Pattern,
        timeout: Option<Duration>,
    ) -> Result<Vec<Tuple>, TupleSpaceError> {
        let start = std::time::Instant::now();

        loop {
            // Try to find matching tuples
            let matches = self.find_matching(&pattern, None).await;

            if !matches.is_empty() {
                // Update stats
                let elapsed = start.elapsed();
                self.update_stats(|stats| {
                    stats.read_operations += 1;
                    stats.total_operations += 1;

                    let alpha = 0.1;
                    stats.avg_latency_ms = alpha * elapsed.as_secs_f32() * 1000.0
                        + (1.0 - alpha) * stats.avg_latency_ms;
                })
                .await;

                return Ok(matches.into_iter().map(|s| s.tuple).collect());
            }

            // No matches - check if blocking
            if let Some(timeout_duration) = timeout {
                // Wait for notification or timeout
                if start.elapsed() >= timeout_duration {
                    // Timeout expired
                    return Ok(Vec::new());
                }

                // Wait for notification with remaining timeout
                let remaining = timeout_duration - start.elapsed();
                tokio::select! {
                    _ = self.notify.notified() => {
                        // New tuple added, retry
                        continue;
                    }
                    _ = tokio::time::sleep(remaining) => {
                        // Timeout
                        return Ok(Vec::new());
                    }
                }
            } else {
                // Non-blocking - return empty
                self.update_stats(|stats| {
                    stats.read_operations += 1;
                    stats.total_operations += 1;
                })
                .await;

                return Ok(Vec::new());
            }
        }
    }

    async fn take(
        &self,
        pattern: Pattern,
        timeout: Option<Duration>,
    ) -> Result<Vec<Tuple>, TupleSpaceError> {
        let start = std::time::Instant::now();

        loop {
            // Try to find and remove matching tuples
            {
                let mut tuples = self.tuples.write().await;
                let mut to_remove = Vec::new();

                // Find matching tuples
                for (id, stored) in tuples.iter() {
                    if !stored.is_expired() && stored.matches(&pattern) {
                        to_remove.push((id.clone(), stored.tuple.clone()));
                    }
                }

                if !to_remove.is_empty() {
                    // Remove tuples
                    for (id, _) in &to_remove {
                        tuples.remove(id);
                    }

                    // Update stats
                    drop(tuples); // Release write lock

                    let elapsed = start.elapsed();
                    self.update_stats(|stats| {
                        stats.take_operations += 1;
                        stats.total_operations += 1;
                        stats.tuple_count -= to_remove.len() as u64;

                        let alpha = 0.1;
                        stats.avg_latency_ms = alpha * elapsed.as_secs_f32() * 1000.0
                            + (1.0 - alpha) * stats.avg_latency_ms;
                    })
                    .await;

                    return Ok(to_remove.into_iter().map(|(_, tuple)| tuple).collect());
                }
            }

            // No matches - check if blocking
            if let Some(timeout_duration) = timeout {
                if start.elapsed() >= timeout_duration {
                    return Ok(Vec::new());
                }

                let remaining = timeout_duration - start.elapsed();
                tokio::select! {
                    _ = self.notify.notified() => {
                        continue;
                    }
                    _ = tokio::time::sleep(remaining) => {
                        return Ok(Vec::new());
                    }
                }
            } else {
                self.update_stats(|stats| {
                    stats.take_operations += 1;
                    stats.total_operations += 1;
                })
                .await;

                return Ok(Vec::new());
            }
        }
    }

    async fn count(&self, pattern: Pattern) -> Result<usize, TupleSpaceError> {
        let matches = self.find_matching(&pattern, None).await;
        Ok(matches.len())
    }

    async fn exists(&self, pattern: Pattern) -> Result<bool, TupleSpaceError> {
        let matches = self.find_matching(&pattern, Some(1)).await;
        Ok(!matches.is_empty())
    }

    async fn renew_lease(
        &self,
        tuple_id: &str,
        new_ttl: Option<Duration>,
    ) -> Result<DateTime<Utc>, TupleSpaceError> {
        let mut tuples = self.tuples.write().await;

        let stored = tuples.get_mut(tuple_id).ok_or(TupleSpaceError::NotFound)?;

        if !stored.renewable {
            return Err(TupleSpaceError::LeaseError(
                "Lease is not renewable".to_string(),
            ));
        }

        let extension = if let Some(ttl) = new_ttl {
            chrono::Duration::milliseconds(ttl.as_millis() as i64)
        } else if let Some(lease) = stored.tuple.lease() {
            chrono::Duration::seconds(lease.ttl_seconds() as i64)
        } else {
            return Err(TupleSpaceError::LeaseError(
                "No lease associated with tuple".to_string(),
            ));
        };

        let new_expires = Utc::now() + extension;
        stored.expires_at = Some(new_expires);

        Ok(new_expires)
    }

    async fn clear(&self) -> Result<(), TupleSpaceError> {
        let mut tuples = self.tuples.write().await;
        tuples.clear();

        self.update_stats(|stats| {
            stats.tuple_count = 0;
        })
        .await;

        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, TupleSpaceError> {
        let stats = self.stats.read().await;
        Ok(stats.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lease, PatternField, TupleField};

    #[tokio::test]
    async fn test_memory_storage_basic() {
        let config = MemoryStorageConfig {
            initial_capacity: 100,
            cleanup_interval_ms: 1000,
        };

        let storage = MemoryStorage::new(config);

        // Write tuple
        let tuple = Tuple::new(vec![
            TupleField::String("test".to_string()),
            TupleField::Integer(42),
        ]);

        let tuple_id = storage.write(tuple.clone()).await.unwrap();
        assert!(!tuple_id.is_empty());

        // Read tuple
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("test".to_string())),
            PatternField::Wildcard,
        ]);

        let results = storage.read(pattern.clone(), None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields(), tuple.fields());

        // Take tuple
        let taken = storage.take(pattern, None).await.unwrap();
        assert_eq!(taken.len(), 1);

        // Should be gone
        let pattern2 = Pattern::new(vec![
            PatternField::Exact(TupleField::String("test".to_string())),
            PatternField::Wildcard,
        ]);
        let empty = storage.read(pattern2, None).await.unwrap();
        assert_eq!(empty.len(), 0);
    }

    #[tokio::test]
    async fn test_memory_storage_lease() {
        let config = MemoryStorageConfig {
            initial_capacity: 100,
            cleanup_interval_ms: 100, // Fast cleanup for testing
        };

        let storage = MemoryStorage::new(config);

        // Write tuple with short lease
        let tuple = Tuple::new(vec![TupleField::String("expiring".to_string())])
            .with_lease(Lease::new(chrono::Duration::milliseconds(200)));

        storage.write(tuple).await.unwrap();

        // Should exist immediately
        let pattern = Pattern::new(vec![PatternField::Exact(TupleField::String(
            "expiring".to_string(),
        ))]);

        let results = storage.read(pattern.clone(), None).await.unwrap();
        assert_eq!(results.len(), 1);

        // Wait for expiry + cleanup
        tokio::time::sleep(Duration::from_millis(400)).await;

        // Should be gone
        let empty = storage.read(pattern, None).await.unwrap();
        assert_eq!(empty.len(), 0);
    }

    #[tokio::test]
    async fn test_memory_storage_blocking_read() {
        let config = MemoryStorageConfig {
            initial_capacity: 100,
            cleanup_interval_ms: 1000,
        };

        let storage = Arc::new(MemoryStorage::new(config));
        let storage_clone = storage.clone();

        // Spawn task to write tuple after delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;

            let tuple = Tuple::new(vec![TupleField::String("delayed".to_string())]);

            storage_clone.write(tuple).await.unwrap();
        });

        // Blocking read should wait for tuple
        let pattern = Pattern::new(vec![PatternField::Exact(TupleField::String(
            "delayed".to_string(),
        ))]);

        let start = std::time::Instant::now();
        let results = storage
            .read(pattern, Some(Duration::from_secs(1)))
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 1);
        assert!(elapsed >= Duration::from_millis(100));
        assert!(elapsed < Duration::from_millis(500));
    }

    #[tokio::test]
    async fn test_memory_storage_stats() {
        let config = MemoryStorageConfig {
            initial_capacity: 100,
            cleanup_interval_ms: 1000,
        };

        let storage = MemoryStorage::new(config);

        // Write some tuples
        for i in 0..5 {
            let tuple = Tuple::new(vec![TupleField::Integer(i)]);
            storage.write(tuple).await.unwrap();
        }

        // Read and take
        let pattern = Pattern::new(vec![PatternField::Wildcard]);
        storage.read(pattern.clone(), None).await.unwrap();
        storage.take(pattern, None).await.unwrap();

        // Check stats
        let stats = storage.stats().await.unwrap();
        assert_eq!(stats.write_operations, 5);
        assert_eq!(stats.read_operations, 1);
        assert_eq!(stats.take_operations, 1);
        assert_eq!(stats.total_operations, 7);
        assert_eq!(stats.tuple_count, 0); // All taken
    }
}
