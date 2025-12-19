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

//! SQL-based journal storage implementations (SQLite and PostgreSQL).
//!
//! ## Purpose
//! Provides persistent, durable journal storage using relational databases.
//!
//! ## Features
//! - **Persistent**: Journal survives process restarts
//! - **Transactional**: ACID guarantees for batch operations
//! - **Append-Only**: Immutable entries (never UPDATE or DELETE except truncate)
//! - **JSONB Storage**: Extensible entry_data without schema changes (PostgreSQL)
//! - **Compression Support**: State data compression in checkpoints
//!
//! ## Schema (Optimized for Replay Performance)
//! ```sql
//! -- SQLite/PostgreSQL schema
//! CREATE TABLE journal_entries (
//!     id TEXT PRIMARY KEY,                    -- ULID (time-sortable)
//!     actor_id TEXT NOT NULL,                 -- Actor partitioning key
//!     sequence BIGINT NOT NULL,               -- Monotonic per actor
//!     timestamp BIGINT NOT NULL,              -- Unix timestamp (ms)
//!     correlation_id TEXT,                    -- Link related entries
//!     entry_type TEXT NOT NULL,               -- Entry discriminator
//!     entry_data TEXT NOT NULL,               -- JSON payload (SQLite) / JSONB (PostgreSQL)
//!     UNIQUE(actor_id, sequence)              -- Constraint
//! );
//!
//! CREATE INDEX idx_journal_actor_sequence
//!     ON journal_entries(actor_id, sequence); -- Replay performance
//!
//! CREATE TABLE checkpoints (
//!     actor_id TEXT NOT NULL,
//!     sequence BIGINT NOT NULL,
//!     timestamp BIGINT NOT NULL,
//!     state_data BLOB NOT NULL,               -- Compressed state
//!     compression INTEGER NOT NULL,           -- Compression type
//!     metadata TEXT,                          -- JSON metadata
//!     PRIMARY KEY(actor_id, sequence)
//! );
//!
//! CREATE INDEX idx_checkpoint_latest
//!     ON checkpoints(actor_id, sequence DESC); -- Latest checkpoint lookup
//! ```
//!
//! ## Performance Characteristics
//! - Append entry: O(1) with index update → < 1ms
//! - Replay: O(n) sequential scan from sequence → < 50ms for 10K entries
//! - Checkpoint lookup: O(log n) via PRIMARY KEY → < 1ms
//! - Truncate: O(m) where m = entries to delete → < 100ms for 10K entries

use crate::{Checkpoint, JournalEntry, JournalError, JournalResult, JournalStats, JournalStorage, ActorEvent, ActorHistory};
use crate::storage::{ReminderState, ReminderRegistration};
use async_trait::async_trait;
use plexspaces_proto::common::v1::{PageRequest, PageResponse};
use plexspaces_proto::prost_types;
use prost::Message;
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

// Helper to convert SystemTime to Unix timestamp (milliseconds)
fn system_time_to_unix_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// Helper to convert timestamp proto to Unix ms
fn proto_timestamp_to_unix_ms(ts: &Option<prost_types::Timestamp>) -> i64 {
    if let Some(t) = ts {
        (t.seconds * 1000) + (t.nanos / 1_000_000) as i64
    } else {
        system_time_to_unix_ms(SystemTime::now())
    }
}

// Helper to convert Unix ms to proto timestamp
fn unix_ms_to_proto_timestamp(ms: i64) -> Option<prost_types::Timestamp> {
    Some(prost_types::Timestamp {
        seconds: ms / 1000,
        nanos: ((ms % 1000) * 1_000_000) as i32,
    })
}

// Helper to convert SQL row to ReminderState (PostgreSQL)
#[cfg(feature = "postgres-backend")]
fn row_to_reminder_state_pg(row: &sqlx::postgres::PgRow) -> JournalResult<ReminderState> {
    use sqlx::Row;
    
    let actor_id: String = row.get("actor_id");
    let reminder_name: String = row.get("reminder_name");
    
    let interval_seconds: Option<i64> = row.get("interval_seconds");
    let interval_nanos: Option<i32> = row.get("interval_nanos");
    let interval = if let (Some(secs), Some(nanos)) = (interval_seconds, interval_nanos) {
        Some(prost_types::Duration { seconds: secs, nanos })
    } else {
        None
    };
    
    let first_fire_seconds: Option<i64> = row.get("first_fire_time_seconds");
    let first_fire_nanos: Option<i32> = row.get("first_fire_time_nanos");
    let first_fire_time = if let (Some(secs), Some(nanos)) = (first_fire_seconds, first_fire_nanos) {
        Some(prost_types::Timestamp { seconds: secs, nanos })
    } else {
        None
    };
    
    let callback_data: Vec<u8> = row.get("callback_data");
    let persist_across_activations: bool = row.get("persist_across_activations");
    let max_occurrences: i32 = row.get("max_occurrences");
    
    let last_fired_seconds: Option<i64> = row.get("last_fired_seconds");
    let last_fired_nanos: Option<i32> = row.get("last_fired_nanos");
    let last_fired = if let (Some(secs), Some(nanos)) = (last_fired_seconds, last_fired_nanos) {
        Some(prost_types::Timestamp { seconds: secs, nanos })
    } else {
        None
    };
    
    let next_fire_seconds: Option<i64> = row.get("next_fire_time_seconds");
    let next_fire_nanos: Option<i32> = row.get("next_fire_time_nanos");
    let next_fire_time = if let (Some(secs), Some(nanos)) = (next_fire_seconds, next_fire_nanos) {
        Some(prost_types::Timestamp { seconds: secs, nanos })
    } else {
        None
    };
    
    let fire_count: i32 = row.get("fire_count");
    let is_active: bool = row.get("is_active");
    
    Ok(ReminderState {
        registration: Some(ReminderRegistration {
            actor_id,
            reminder_name,
            interval,
            first_fire_time,
            callback_data,
            persist_across_activations,
            max_occurrences,
        }),
        last_fired,
        next_fire_time,
        fire_count,
        is_active,
    })
}

// Helper to convert SQL row to ReminderState (SQLite)
fn row_to_reminder_state(row: &sqlx::sqlite::SqliteRow) -> JournalResult<ReminderState> {
    use sqlx::Row;
    
    let actor_id: String = row.get("actor_id");
    let reminder_name: String = row.get("reminder_name");
    
    let interval_seconds: Option<i64> = row.get("interval_seconds");
    let interval_nanos: Option<i32> = row.get("interval_nanos");
    let interval = if let (Some(secs), Some(nanos)) = (interval_seconds, interval_nanos) {
        Some(prost_types::Duration { seconds: secs, nanos })
    } else {
        None
    };
    
    let first_fire_seconds: Option<i64> = row.get("first_fire_time_seconds");
    let first_fire_nanos: Option<i32> = row.get("first_fire_time_nanos");
    let first_fire_time = if let (Some(secs), Some(nanos)) = (first_fire_seconds, first_fire_nanos) {
        Some(prost_types::Timestamp { seconds: secs, nanos })
    } else {
        None
    };
    
    let callback_data: Vec<u8> = row.get("callback_data");
    let persist_across_activations: i32 = row.get("persist_across_activations");
    let max_occurrences: i32 = row.get("max_occurrences");
    
    let last_fired_seconds: Option<i64> = row.get("last_fired_seconds");
    let last_fired_nanos: Option<i32> = row.get("last_fired_nanos");
    let last_fired = if let (Some(secs), Some(nanos)) = (last_fired_seconds, last_fired_nanos) {
        Some(prost_types::Timestamp { seconds: secs, nanos })
    } else {
        None
    };
    
    let next_fire_seconds: Option<i64> = row.get("next_fire_time_seconds");
    let next_fire_nanos: Option<i32> = row.get("next_fire_time_nanos");
    let next_fire_time = if let (Some(secs), Some(nanos)) = (next_fire_seconds, next_fire_nanos) {
        Some(prost_types::Timestamp { seconds: secs, nanos })
    } else {
        None
    };
    
    let fire_count: i32 = row.get("fire_count");
    let is_active: i32 = row.get("is_active");
    
    Ok(ReminderState {
        registration: Some(ReminderRegistration {
            actor_id,
            reminder_name,
            interval,
            first_fire_time,
            callback_data,
            persist_across_activations: persist_across_activations != 0,
            max_occurrences,
        }),
        last_fired,
        next_fire_time,
        fire_count,
        is_active: is_active != 0,
    })
}

/// SQLite-based journal storage.
///
/// ## Purpose
/// Persistent journal storage using SQLite, ideal for:
/// - Single-node deployments
/// - Edge computing
/// - Unit/integration tests
/// - Development environments
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_journaling::{JournalStorage, sql::SqliteJournalStorage};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Persistent database
/// let storage = SqliteJournalStorage::new("/tmp/journal.db").await?;
///
/// // In-memory database (for testing) - RECOMMENDED for tests
/// let storage = SqliteJournalStorage::new(":memory:").await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct SqliteJournalStorage {
    pool: Pool<Sqlite>,
    /// Sequence counters by actor_id (cached for performance)
    sequences: Arc<RwLock<HashMap<String, u64>>>,
}

impl SqliteJournalStorage {
    /// Create a new SQLite journal storage.
    ///
    /// ## Arguments
    /// - `path`: Database file path (use ":memory:" for in-memory database)
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_journaling::sql::SqliteJournalStorage;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Persistent database
    /// let storage = SqliteJournalStorage::new("/tmp/journal.db").await?;
    ///
    /// // In-memory database (RECOMMENDED for tests)
    /// let storage = SqliteJournalStorage::new(":memory:").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(path: &str) -> JournalResult<Self> {
        // SQLite connection string format for sqlx 0.7:
        // - ":memory:" for in-memory database
        // - "sqlite://path" for file-based (but this has issues on macOS)
        // - Try using SQLite file: URI format as fallback
        let connection_string = if path == ":memory:" {
            ":memory:".to_string() // Try without sqlite: prefix
        } else if path.starts_with("sqlite:") || path.starts_with("file:") {
            // Already has scheme
            path.to_string()
        } else {
            // Use SQLite file: URI format
            // For absolute paths: file:///absolute/path
            // For relative paths: file:relative/path
            if path.starts_with('/') {
                format!("file://{}", path)
            } else {
                format!("file:{}", path)
            }
        };

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&connection_string)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        // Enable WAL mode for better concurrency
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        // Run migrations
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .map_err(|e| JournalError::Storage(format!("Migration failed: {}", e)))?;

        Ok(Self {
            pool,
            sequences: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get next event sequence number for actor (cached)
    async fn next_event_sequence(&self, actor_id: &str) -> JournalResult<u64> {
        let mut sequences = self.sequences.write().await;

        // Use a separate key for event sequences (e.g., "actor_id:events")
        let cache_key = format!("{}:events", actor_id);

        if let Some(seq) = sequences.get_mut(&cache_key) {
            let current = *seq;
            *seq += 1;
            return Ok(current);
        }

        // Cache miss - query database for max sequence
        let row = sqlx::query(
            r#"
            SELECT COALESCE(MAX(sequence), 0) as max_seq
            FROM actor_events
            WHERE actor_id = ?
            "#,
        )
        .bind(actor_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let max_seq: i64 = row.get("max_seq");
        let next = (max_seq + 1) as u64;

        sequences.insert(cache_key, next + 1);
        Ok(next)
    }

    /// Get next sequence number for actor (cached)
    async fn next_sequence(&self, actor_id: &str) -> JournalResult<u64> {
        let mut sequences = self.sequences.write().await;

        if let Some(seq) = sequences.get_mut(actor_id) {
            let current = *seq;
            *seq += 1;
            return Ok(current);
        }

        // Cache miss - query database for max sequence
        let row = sqlx::query(
            r#"
            SELECT COALESCE(MAX(sequence), 0) as max_seq
            FROM journal_entries
            WHERE actor_id = ?
            "#,
        )
        .bind(actor_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let max_seq: i64 = row.get("max_seq");
        let next = (max_seq + 1) as u64;

        sequences.insert(actor_id.to_string(), next + 1);
        Ok(next)
    }
}

#[async_trait]
impl JournalStorage for SqliteJournalStorage {
    async fn append_entry(&self, entry: &JournalEntry) -> JournalResult<u64> {
        let mut entry = entry.clone();

        // Assign sequence if not set, or sync cache if explicit sequence provided
        let sequence = if entry.sequence == 0 {
            let assigned_seq = self.next_sequence(&entry.actor_id).await?;
            // Update entry.sequence so it's serialized correctly
            entry.sequence = assigned_seq;
            assigned_seq
        } else {
            // Entry has explicit sequence - update cache to match to prevent conflicts
            let mut sequences = self.sequences.write().await;
            let next_seq = entry.sequence + 1;
            sequences.insert(entry.actor_id.clone(), next_seq);
            entry.sequence
        };
        let timestamp = proto_timestamp_to_unix_ms(&entry.timestamp);

        // Serialize the entire entry to protobuf bytes
        let mut entry_bytes = Vec::new();
        entry
            .encode(&mut entry_bytes)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;

        // Determine entry_type from oneof
        use plexspaces_proto::v1::journaling::journal_entry::Entry as JournalEntryVariant;
        let entry_type = match &entry.entry {
            Some(JournalEntryVariant::MessageReceived(_)) => "MessageReceived",
            Some(JournalEntryVariant::MessageProcessed(_)) => "MessageProcessed",
            Some(JournalEntryVariant::StateChanged(_)) => "StateChanged",
            Some(JournalEntryVariant::SideEffectExecuted(_)) => "SideEffectExecuted",
            Some(JournalEntryVariant::TimerScheduled(_)) => "TimerScheduled",
            Some(JournalEntryVariant::TimerFired(_)) => "TimerFired",
            Some(JournalEntryVariant::PromiseCreated(_)) => "PromiseCreated",
            Some(JournalEntryVariant::PromiseResolved(_)) => "PromiseResolved",
            None => "Unknown",
        };

        sqlx::query(
            r#"
            INSERT INTO journal_entries (id, actor_id, sequence, timestamp, correlation_id, entry_type, entry_data)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&entry.id)
        .bind(&entry.actor_id)
        .bind(sequence as i64)
        .bind(timestamp)
        .bind(&entry.correlation_id)
        .bind(entry_type)
        .bind(&entry_bytes)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(sequence)
    }

    async fn append_batch(&self, entries: &[JournalEntry]) -> JournalResult<(u64, u64, usize)> {
        if entries.is_empty() {
            return Ok((0, 0, 0));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut first_sequence = 0u64;
        let mut last_sequence = 0u64;

        for (i, entry) in entries.iter().enumerate() {
            let mut entry = entry.clone();

            // Assign sequence if not set, or sync cache if explicit sequence provided
            let seq = if entry.sequence == 0 {
                self.next_sequence(&entry.actor_id).await?
            } else {
                // Entry has explicit sequence - update cache to match to prevent conflicts
                let mut sequences = self.sequences.write().await;
                let next_seq = entry.sequence + 1;
                sequences.insert(entry.actor_id.clone(), next_seq);
                entry.sequence
            };
            entry.sequence = seq;

            if i == 0 {
                first_sequence = entry.sequence;
            }
            last_sequence = entry.sequence;

            let timestamp = proto_timestamp_to_unix_ms(&entry.timestamp);

            // Serialize the entire entry to protobuf bytes
            let mut entry_bytes = Vec::new();
            entry
                .encode(&mut entry_bytes)
                .map_err(|e| JournalError::Serialization(e.to_string()))?;

            use plexspaces_proto::v1::journaling::journal_entry::Entry as JournalEntryVariant;
            let entry_type = match &entry.entry {
                Some(JournalEntryVariant::MessageReceived(_)) => "MessageReceived",
                Some(JournalEntryVariant::MessageProcessed(_)) => "MessageProcessed",
                Some(JournalEntryVariant::StateChanged(_)) => "StateChanged",
                Some(JournalEntryVariant::SideEffectExecuted(_)) => "SideEffectExecuted",
                Some(JournalEntryVariant::TimerScheduled(_)) => "TimerScheduled",
                Some(JournalEntryVariant::TimerFired(_)) => "TimerFired",
                Some(JournalEntryVariant::PromiseCreated(_)) => "PromiseCreated",
                Some(JournalEntryVariant::PromiseResolved(_)) => "PromiseResolved",
                None => "Unknown",
            };

            sqlx::query(
                r#"
                INSERT INTO journal_entries (id, actor_id, sequence, timestamp, correlation_id, entry_type, entry_data)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&entry.id)
            .bind(&entry.actor_id)
            .bind(entry.sequence as i64)
            .bind(timestamp)
            .bind(&entry.correlation_id)
            .bind(entry_type)
            .bind(&entry_bytes)
            .execute(&mut *tx)
            .await
            .map_err(|e| JournalError::Serialization(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok((first_sequence, last_sequence, entries.len()))
    }

    async fn replay_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<JournalEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, timestamp, correlation_id, entry_type, entry_data
            FROM journal_entries
            WHERE actor_id = ? AND sequence >= ?
            ORDER BY sequence ASC
            "#,
        )
        .bind(actor_id)
        .bind(from_sequence as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut entries = Vec::new();

        for row in rows {
            let entry_bytes: Vec<u8> = row.get("entry_data");

            // Deserialize the entire entry from protobuf bytes
            let entry = JournalEntry::decode(&entry_bytes[..])
                .map_err(|e| JournalError::Serialization(e.to_string()))?;

            entries.push(entry);
        }

        Ok(entries)
    }

    async fn get_latest_checkpoint(&self, actor_id: &str) -> JournalResult<Checkpoint> {
        let row = sqlx::query(
            r#"
            SELECT actor_id, sequence, timestamp, state_data, compression, metadata, state_schema_version
            FROM checkpoints
            WHERE actor_id = ?
            ORDER BY sequence DESC
            LIMIT 1
            "#,
        )
        .bind(actor_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if matches!(e, sqlx::Error::RowNotFound) {
                JournalError::CheckpointNotFound(actor_id.to_string())
            } else {
                JournalError::Storage(e.to_string())
            }
        })?;

        let actor_id: String = row.get("actor_id");
        let sequence: i64 = row.get("sequence");
        let timestamp: i64 = row.get("timestamp");
        let state_data: Vec<u8> = row.get("state_data");
        let compression: i32 = row.get("compression");
        let metadata: Option<String> = row.get("metadata");
        // Read state_schema_version (default to 1 if column doesn't exist for backward compatibility)
        let state_schema_version: i32 = row.try_get("state_schema_version").unwrap_or(1);

        let metadata_map: HashMap<String, String> = if let Some(json) = metadata {
            serde_json::from_str(&json).unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Checkpoint {
            actor_id,
            sequence: sequence as u64,
            timestamp: unix_ms_to_proto_timestamp(timestamp),
            state_data,
            compression,
            metadata: metadata_map,
            state_schema_version: state_schema_version as u32,
        })
    }

    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> JournalResult<()> {
        let timestamp = proto_timestamp_to_unix_ms(&checkpoint.timestamp);
        let metadata_json = serde_json::to_string(&checkpoint.metadata)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO checkpoints (actor_id, sequence, timestamp, state_data, compression, metadata, state_schema_version)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&checkpoint.actor_id)
        .bind(checkpoint.sequence as i64)
        .bind(timestamp)
        .bind(&checkpoint.state_data)
        .bind(checkpoint.compression)
        .bind(metadata_json)
        .bind(checkpoint.state_schema_version as i32)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn truncate_to(&self, actor_id: &str, sequence: u64) -> JournalResult<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM journal_entries
            WHERE actor_id = ? AND sequence <= ?
            "#,
        )
        .bind(actor_id)
        .bind(sequence as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(result.rows_affected())
    }

    async fn get_stats(&self, actor_id: Option<&str>) -> JournalResult<JournalStats> {
        let mut stats = JournalStats {
            total_entries: 0,
            total_checkpoints: 0,
            storage_bytes: 0,
            entries_by_actor: HashMap::new(),
            oldest_entry: None,
            newest_entry: None,
        };

        if let Some(aid) = actor_id {
            // Stats for specific actor
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count, MIN(timestamp) as oldest, MAX(timestamp) as newest
                FROM journal_entries
                WHERE actor_id = ?
                "#,
            )
            .bind(aid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            let count: i64 = row.get("count");
            stats.total_entries = count as u64;
            stats.entries_by_actor.insert(aid.to_string(), count as u64);

            // Only get timestamps if there are entries (MIN/MAX return NULL for empty sets)
            if count > 0 {
                if let Ok(oldest) = row.try_get::<i64, _>("oldest") {
                    stats.oldest_entry = unix_ms_to_proto_timestamp(oldest);
                }
                if let Ok(newest) = row.try_get::<i64, _>("newest") {
                    stats.newest_entry = unix_ms_to_proto_timestamp(newest);
                }
            }

            // Checkpoint count
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count
                FROM checkpoints
                WHERE actor_id = ?
                "#,
            )
            .bind(aid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            stats.total_checkpoints = row.get::<i64, _>("count") as u64;
        } else {
            // Global stats
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count, MIN(timestamp) as oldest, MAX(timestamp) as newest
                FROM journal_entries
                "#,
            )
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            let count: i64 = row.get("count");
            stats.total_entries = count as u64;

            // Only get timestamps if there are entries (MIN/MAX return NULL for empty sets)
            if count > 0 {
                if let Ok(oldest) = row.try_get::<i64, _>("oldest") {
                    stats.oldest_entry = unix_ms_to_proto_timestamp(oldest);
                }
                if let Ok(newest) = row.try_get::<i64, _>("newest") {
                    stats.newest_entry = unix_ms_to_proto_timestamp(newest);
                }
            }

            // Per-actor counts
            let rows = sqlx::query(
                r#"
                SELECT actor_id, COUNT(*) as count
                FROM journal_entries
                GROUP BY actor_id
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            for row in rows {
                let actor_id: String = row.get("actor_id");
                let count: i64 = row.get("count");
                stats.entries_by_actor.insert(actor_id, count as u64);
            }

            // Total checkpoints
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count
                FROM checkpoints
                "#,
            )
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            stats.total_checkpoints = row.get::<i64, _>("count") as u64;
        }

        Ok(stats)
    }

    async fn flush(&self) -> JournalResult<()> {
        // SQLite with WAL mode: Ensure all writes are visible
        // For in-memory databases, WAL mode auto-commits, but we ensure consistency
        // Note: wal_checkpoint may fail for in-memory databases, so we ignore errors
        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await;
        Ok(())
    }

    // ==================== Event Sourcing Methods ====================

    async fn append_event(&self, event: &ActorEvent) -> JournalResult<u64> {
        let mut event = event.clone();

        // Assign sequence if not set
        if event.sequence == 0 {
            event.sequence = self.next_event_sequence(&event.actor_id).await?;
        }

        let sequence = event.sequence;
        let timestamp = proto_timestamp_to_unix_ms(&event.timestamp);

        // Serialize event data
        let event_data_bytes = event.event_data.clone();

        // Serialize metadata to JSON
        let metadata_json = serde_json::to_string(&event.metadata)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO actor_events (id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&event.id)
        .bind(&event.actor_id)
        .bind(sequence as i64)
        .bind(&event.event_type)
        .bind(&event_data_bytes)
        .bind(timestamp)
        .bind(&event.caused_by)
        .bind(&metadata_json)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(sequence)
    }

    async fn append_events_batch(&self, events: &[ActorEvent]) -> JournalResult<(u64, u64, usize)> {
        if events.is_empty() {
            return Ok((0, 0, 0));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut first_sequence = 0u64;
        let mut last_sequence = 0u64;

        for (i, event) in events.iter().enumerate() {
            let mut event = event.clone();

            if event.sequence == 0 {
                event.sequence = self.next_event_sequence(&event.actor_id).await?;
            }

            if i == 0 {
                first_sequence = event.sequence;
            }
            last_sequence = event.sequence;

            let timestamp = proto_timestamp_to_unix_ms(&event.timestamp);
            let metadata_json = serde_json::to_string(&event.metadata)
                .map_err(|e| JournalError::Serialization(e.to_string()))?;

            sqlx::query(
                r#"
                INSERT INTO actor_events (id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&event.id)
            .bind(&event.actor_id)
            .bind(event.sequence as i64)
            .bind(&event.event_type)
            .bind(&event.event_data)
            .bind(timestamp)
            .bind(&event.caused_by)
            .bind(&metadata_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok((first_sequence, last_sequence, events.len()))
    }

    async fn replay_events_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<ActorEvent>> {
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = ? AND sequence >= ?
            ORDER BY sequence ASC
            "#,
        )
        .bind(actor_id)
        .bind(from_sequence as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut events = Vec::new();
        for row in rows {
            let metadata_json: String = row.get("metadata");
            let metadata: HashMap<String, String> = serde_json::from_str(&metadata_json)
                .unwrap_or_default();

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence: row.get::<i64, _>("sequence") as u64,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp: unix_ms_to_proto_timestamp(row.get::<i64, _>("timestamp")),
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        Ok(events)
    }

    async fn replay_events_from_paginated(
        &self,
        actor_id: &str,
        from_sequence: u64,
        page_request: &PageRequest,
    ) -> JournalResult<(Vec<ActorEvent>, PageResponse)> {
        // Decode page_token to get starting sequence
        let start_sequence = if page_request.page_token.is_empty() {
            from_sequence
        } else {
            page_request.page_token.parse::<u64>().unwrap_or(from_sequence)
        };

        // Validate and clamp page_size (1-1000)
        let page_size = page_request.page_size.max(1).min(1000) as i64;

        // Fetch page_size + 1 to check if there's more
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = ? AND sequence >= ? AND sequence >= ?
            ORDER BY sequence ASC
            LIMIT ?
            "#,
        )
        .bind(actor_id)
        .bind(start_sequence.max(from_sequence) as i64)
        .bind(from_sequence as i64)
        .bind(page_size + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let has_more = rows.len() > page_size as usize;
        let rows_to_return = rows.iter().take(page_size as usize);

        let mut events = Vec::new();
        for row in rows_to_return {
            let metadata_json: String = row.get("metadata");
            let metadata: HashMap<String, String> = serde_json::from_str(&metadata_json)
                .unwrap_or_default();

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence: row.get::<i64, _>("sequence") as u64,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp: unix_ms_to_proto_timestamp(row.get::<i64, _>("timestamp")),
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        // Generate next_page_token if there are more events
        let next_page_token = if has_more && !events.is_empty() {
            (events.last().unwrap().sequence + 1).to_string()
        } else {
            String::new()
        };

        let page_response = PageResponse {
            next_page_token,
            total_size: 0, // Total size not available without full scan (expensive)
        };

        Ok((events, page_response))
    }

    async fn get_actor_history(&self, actor_id: &str) -> JournalResult<ActorHistory> {
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = ?
            ORDER BY sequence ASC
            "#,
        )
        .bind(actor_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut events = Vec::new();
        let mut latest_sequence = 0u64;
        let mut created_at: Option<prost_types::Timestamp> = None;
        let mut updated_at: Option<prost_types::Timestamp> = None;

        for row in rows {
            let metadata_json: String = row.get("metadata");
            let metadata: HashMap<String, String> = serde_json::from_str(&metadata_json)
                .unwrap_or_default();

            let timestamp = unix_ms_to_proto_timestamp(row.get::<i64, _>("timestamp"));
            if created_at.is_none() {
                created_at = timestamp.clone();
            }
            updated_at = timestamp.clone();

            let sequence = row.get::<i64, _>("sequence") as u64;
            latest_sequence = latest_sequence.max(sequence);

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp,
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        Ok(ActorHistory {
            actor_id: actor_id.to_string(),
            events,
            latest_sequence,
            created_at: created_at.or_else(|| Some(prost_types::Timestamp::from(SystemTime::now()))),
            updated_at: updated_at.or_else(|| Some(prost_types::Timestamp::from(SystemTime::now()))),
            metadata: HashMap::new(),
            page_response: None,
        })
    }

    async fn get_actor_history_paginated(
        &self,
        actor_id: &str,
        page_request: &PageRequest,
    ) -> JournalResult<ActorHistory> {
        // Decode page_token to get starting sequence
        let start_sequence = if page_request.page_token.is_empty() {
            0
        } else {
            page_request.page_token.parse::<u64>().unwrap_or(0)
        };

        // Validate and clamp page_size (1-1000)
        let page_size = page_request.page_size.max(1).min(1000) as i64;

        // Get latest sequence for history metadata
        let latest_row = sqlx::query(
            r#"
            SELECT MAX(sequence) as max_seq, MIN(timestamp) as min_ts, MAX(timestamp) as max_ts
            FROM actor_events
            WHERE actor_id = ?
            "#,
        )
        .bind(actor_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let latest_sequence = latest_row
            .as_ref()
            .and_then(|r| r.get::<Option<i64>, _>("max_seq"))
            .map(|s| s as u64)
            .unwrap_or(0);

        let created_at = latest_row
            .as_ref()
            .and_then(|r| unix_ms_to_proto_timestamp(r.get::<Option<i64>, _>("min_ts").unwrap_or(0)))
            .or_else(|| Some(prost_types::Timestamp::from(SystemTime::now())));

        let updated_at = latest_row
            .as_ref()
            .and_then(|r| unix_ms_to_proto_timestamp(r.get::<Option<i64>, _>("max_ts").unwrap_or(0)))
            .or_else(|| Some(prost_types::Timestamp::from(SystemTime::now())));

        // Fetch page_size + 1 to check if there's more
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = ? AND sequence >= ?
            ORDER BY sequence ASC
            LIMIT ?
            "#,
        )
        .bind(actor_id)
        .bind(start_sequence as i64)
        .bind(page_size + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let has_more = rows.len() > page_size as usize;
        let rows_to_return = rows.iter().take(page_size as usize);

        let mut events = Vec::new();
        for row in rows_to_return {
            let metadata_json: String = row.get("metadata");
            let metadata: HashMap<String, String> = serde_json::from_str(&metadata_json)
                .unwrap_or_default();

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence: row.get::<i64, _>("sequence") as u64,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp: unix_ms_to_proto_timestamp(row.get::<i64, _>("timestamp")),
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        // Generate next_page_token if there are more events
        let next_page_token = if has_more && !events.is_empty() {
            (events.last().unwrap().sequence + 1).to_string()
        } else {
            String::new()
        };

        let page_response = PageResponse {
            next_page_token,
            total_size: 0, // Total size not available without full scan (expensive)
        };

        Ok(ActorHistory {
            actor_id: actor_id.to_string(),
            events,
            latest_sequence,
            created_at,
            updated_at,
            metadata: HashMap::new(),
            page_response: Some(page_response),
        })
    }

    // ==================== Reminder Methods ====================

    async fn register_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()> {
        let now = system_time_to_unix_ms(SystemTime::now());
        let reg = reminder_state.registration.as_ref().ok_or_else(|| {
            JournalError::Configuration("ReminderState must have registration".to_string())
        })?;
        
        let interval_seconds = reg.interval.as_ref().map(|d| d.seconds);
        let interval_nanos = reg.interval.as_ref().map(|d| d.nanos);
        let first_fire_seconds = reg.first_fire_time.as_ref().map(|t| t.seconds);
        let first_fire_nanos = reg.first_fire_time.as_ref().map(|t| t.nanos);
        let last_fired_seconds = reminder_state.last_fired.as_ref().map(|t| t.seconds);
        let last_fired_nanos = reminder_state.last_fired.as_ref().map(|t| t.nanos);
        let next_fire_seconds = reminder_state.next_fire_time.as_ref().map(|t| t.seconds);
        let next_fire_nanos = reminder_state.next_fire_time.as_ref().map(|t| t.nanos);
        
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO reminders (
                actor_id, reminder_name,
                interval_seconds, interval_nanos,
                first_fire_time_seconds, first_fire_time_nanos,
                callback_data,
                persist_across_activations, max_occurrences,
                last_fired_seconds, last_fired_nanos,
                next_fire_time_seconds, next_fire_time_nanos,
                fire_count, is_active,
                created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&reg.actor_id)
        .bind(&reg.reminder_name)
        .bind(interval_seconds)
        .bind(interval_nanos)
        .bind(first_fire_seconds)
        .bind(first_fire_nanos)
        .bind(&reg.callback_data)
        .bind(if reg.persist_across_activations { 1 } else { 0 })
        .bind(reg.max_occurrences)
        .bind(last_fired_seconds)
        .bind(last_fired_nanos)
        .bind(next_fire_seconds)
        .bind(next_fire_nanos)
        .bind(reminder_state.fire_count)
        .bind(if reminder_state.is_active { 1 } else { 0 })
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn unregister_reminder(&self, actor_id: &str, reminder_name: &str) -> JournalResult<()> {
        sqlx::query(
            r#"
            DELETE FROM reminders
            WHERE actor_id = ? AND reminder_name = ?
            "#,
        )
        .bind(actor_id)
        .bind(reminder_name)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn load_reminders(&self, actor_id: &str) -> JournalResult<Vec<ReminderState>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                actor_id, reminder_name,
                interval_seconds, interval_nanos,
                first_fire_time_seconds, first_fire_time_nanos,
                callback_data,
                persist_across_activations, max_occurrences,
                last_fired_seconds, last_fired_nanos,
                next_fire_time_seconds, next_fire_time_nanos,
                fire_count, is_active
            FROM reminders
            WHERE actor_id = ? AND is_active = 1
            "#,
        )
        .bind(actor_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut reminders = Vec::new();
        for row in rows {
            let reminder = row_to_reminder_state(&row)?;
            reminders.push(reminder);
        }

        Ok(reminders)
    }

    async fn update_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()> {
        let now = system_time_to_unix_ms(SystemTime::now());
        let reg = reminder_state.registration.as_ref().ok_or_else(|| {
            JournalError::Configuration("ReminderState must have registration".to_string())
        })?;
        
        let interval_seconds = reg.interval.as_ref().map(|d| d.seconds);
        let interval_nanos = reg.interval.as_ref().map(|d| d.nanos);
        let first_fire_seconds = reg.first_fire_time.as_ref().map(|t| t.seconds);
        let first_fire_nanos = reg.first_fire_time.as_ref().map(|t| t.nanos);
        let last_fired_seconds = reminder_state.last_fired.as_ref().map(|t| t.seconds);
        let last_fired_nanos = reminder_state.last_fired.as_ref().map(|t| t.nanos);
        let next_fire_seconds = reminder_state.next_fire_time.as_ref().map(|t| t.seconds);
        let next_fire_nanos = reminder_state.next_fire_time.as_ref().map(|t| t.nanos);
        
        sqlx::query(
            r#"
            UPDATE reminders SET
                interval_seconds = ?,
                interval_nanos = ?,
                first_fire_time_seconds = ?,
                first_fire_time_nanos = ?,
                callback_data = ?,
                persist_across_activations = ?,
                max_occurrences = ?,
                last_fired_seconds = ?,
                last_fired_nanos = ?,
                next_fire_time_seconds = ?,
                next_fire_time_nanos = ?,
                fire_count = ?,
                is_active = ?,
                updated_at = ?
            WHERE actor_id = ? AND reminder_name = ?
            "#,
        )
        .bind(interval_seconds)
        .bind(interval_nanos)
        .bind(first_fire_seconds)
        .bind(first_fire_nanos)
        .bind(&reg.callback_data)
        .bind(if reg.persist_across_activations { 1 } else { 0 })
        .bind(reg.max_occurrences)
        .bind(last_fired_seconds)
        .bind(last_fired_nanos)
        .bind(next_fire_seconds)
        .bind(next_fire_nanos)
        .bind(reminder_state.fire_count)
        .bind(if reminder_state.is_active { 1 } else { 0 })
        .bind(now)
        .bind(&reg.actor_id)
        .bind(&reg.reminder_name)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn query_due_reminders(&self, before_time: SystemTime) -> JournalResult<Vec<ReminderState>> {
        let before_ms = system_time_to_unix_ms(before_time);
        let before_seconds = before_ms / 1000;
        let before_nanos = ((before_ms % 1000) * 1_000_000) as i32;
        
        let rows = sqlx::query(
            r#"
            SELECT 
                actor_id, reminder_name,
                interval_seconds, interval_nanos,
                first_fire_time_seconds, first_fire_time_nanos,
                callback_data,
                persist_across_activations, max_occurrences,
                last_fired_seconds, last_fired_nanos,
                next_fire_time_seconds, next_fire_time_nanos,
                fire_count, is_active
            FROM reminders
            WHERE is_active = 1
            AND (
                next_fire_time_seconds < ? OR
                (next_fire_time_seconds = ? AND next_fire_time_nanos <= ?)
            )
            ORDER BY next_fire_time_seconds ASC, next_fire_time_nanos ASC
            "#,
        )
        .bind(before_seconds)
        .bind(before_seconds)
        .bind(before_nanos)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut reminders = Vec::new();
        for row in rows {
            let reminder = row_to_reminder_state(&row)?;
            reminders.push(reminder);
        }

        Ok(reminders)
    }
}

/// PostgreSQL-based journal storage.
///
/// ## Purpose
/// Production-grade persistent journal storage using PostgreSQL, ideal for:
/// - Multi-node production deployments
/// - High-volume write workloads
/// - Advanced querying with JSONB
/// - ACID transactions with strong consistency
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_journaling::{JournalStorage, sql::PostgresJournalStorage};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Production database
/// let storage = PostgresJournalStorage::new("postgresql://user:pass@localhost/plexspaces").await?;
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "postgres-backend")]
#[derive(Clone)]
pub struct PostgresJournalStorage {
    pool: Pool<sqlx::Postgres>,
    /// Sequence counters by actor_id (cached for performance)
    sequences: Arc<RwLock<HashMap<String, u64>>>,
}

#[cfg(feature = "postgres-backend")]
impl PostgresJournalStorage {
    /// Create a new PostgreSQL journal storage.
    ///
    /// ## Arguments
    /// - `connection_string`: PostgreSQL connection string (e.g., "postgresql://user:pass@localhost/db")
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_journaling::sql::PostgresJournalStorage;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let storage = PostgresJournalStorage::new("postgresql://localhost/plexspaces").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(connection_string: &str) -> JournalResult<Self> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(20)
            .connect(connection_string)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        // Run migrations
        sqlx::migrate!("./migrations/postgres")
            .run(&pool)
            .await
            .map_err(|e| JournalError::Storage(format!("Migration failed: {}", e)))?;

        Ok(Self {
            pool,
            sequences: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get next sequence number for actor (cached)
    async fn next_sequence(&self, actor_id: &str) -> JournalResult<u64> {
        let mut sequences = self.sequences.write().await;

        if let Some(seq) = sequences.get_mut(actor_id) {
            let current = *seq;
            *seq += 1;
            return Ok(current);
        }

        // Cache miss - query database for max sequence
        let row = sqlx::query(
            r#"
            SELECT COALESCE(MAX(sequence), 0) as max_seq
            FROM journal_entries
            WHERE actor_id = $1
            "#,
        )
        .bind(actor_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let max_seq: i64 = row.get("max_seq");
        let next = (max_seq + 1) as u64;

        sequences.insert(actor_id.to_string(), next + 1);
        Ok(next)
    }

    /// Get next event sequence number for actor (cached)
    async fn next_event_sequence(&self, actor_id: &str) -> JournalResult<u64> {
        let mut sequences = self.sequences.write().await;

        // Use a separate key for event sequences (e.g., "actor_id:events")
        let cache_key = format!("{}:events", actor_id);

        if let Some(seq) = sequences.get_mut(&cache_key) {
            let current = *seq;
            *seq += 1;
            return Ok(current);
        }

        // Cache miss - query database for max sequence
        let row = sqlx::query(
            r#"
            SELECT COALESCE(MAX(sequence), 0) as max_seq
            FROM actor_events
            WHERE actor_id = $1
            "#,
        )
        .bind(actor_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let max_seq: i64 = row.get("max_seq");
        let next = (max_seq + 1) as u64;

        sequences.insert(cache_key, next + 1);
        Ok(next)
    }

    // Reminder methods are implemented in the trait impl above
}

#[cfg(feature = "postgres-backend")]
#[async_trait]
impl JournalStorage for PostgresJournalStorage {
    async fn append_entry(&self, entry: &JournalEntry) -> JournalResult<u64> {
        let mut entry = entry.clone();

        // Assign sequence if not set, or sync cache if explicit sequence provided
        let sequence = if entry.sequence == 0 {
            let assigned_seq = self.next_sequence(&entry.actor_id).await?;
            // Update entry.sequence so it's serialized correctly
            entry.sequence = assigned_seq;
            assigned_seq
        } else {
            // Entry has explicit sequence - update cache to match to prevent conflicts
            let mut sequences = self.sequences.write().await;
            let next_seq = entry.sequence + 1;
            sequences.insert(entry.actor_id.clone(), next_seq);
            entry.sequence
        };
        let timestamp = proto_timestamp_to_unix_ms(&entry.timestamp);

        // Serialize the entire entry to protobuf bytes
        let mut entry_bytes = Vec::new();
        entry
            .encode(&mut entry_bytes)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;

        // Determine entry_type from oneof
        use plexspaces_proto::v1::journaling::journal_entry::Entry as JournalEntryVariant;
        let entry_type = match &entry.entry {
            Some(JournalEntryVariant::MessageReceived(_)) => "MessageReceived",
            Some(JournalEntryVariant::MessageProcessed(_)) => "MessageProcessed",
            Some(JournalEntryVariant::StateChanged(_)) => "StateChanged",
            Some(JournalEntryVariant::SideEffectExecuted(_)) => "SideEffectExecuted",
            Some(JournalEntryVariant::TimerScheduled(_)) => "TimerScheduled",
            Some(JournalEntryVariant::TimerFired(_)) => "TimerFired",
            Some(JournalEntryVariant::PromiseCreated(_)) => "PromiseCreated",
            Some(JournalEntryVariant::PromiseResolved(_)) => "PromiseResolved",
            None => "Unknown",
        };

        sqlx::query(
            r#"
            INSERT INTO journal_entries (id, actor_id, sequence, timestamp, correlation_id, entry_type, entry_data)
            VALUES ($1, $2, $3, to_timestamp($4::double precision / 1000), $5, $6, $7)
            "#,
        )
        .bind(&entry.id)
        .bind(&entry.actor_id)
        .bind(sequence as i64)
        .bind(timestamp as f64)
        .bind(&entry.correlation_id)
        .bind(entry_type)
        .bind(&entry_bytes)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(sequence)
    }

    async fn append_batch(&self, entries: &[JournalEntry]) -> JournalResult<(u64, u64, usize)> {
        if entries.is_empty() {
            return Ok((0, 0, 0));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut first_sequence = 0u64;
        let mut last_sequence = 0u64;

        for (i, entry) in entries.iter().enumerate() {
            let mut entry = entry.clone();

            // Assign sequence if not set, or sync cache if explicit sequence provided
            let seq = if entry.sequence == 0 {
                self.next_sequence(&entry.actor_id).await?
            } else {
                // Entry has explicit sequence - update cache to match to prevent conflicts
                let mut sequences = self.sequences.write().await;
                let next_seq = entry.sequence + 1;
                sequences.insert(entry.actor_id.clone(), next_seq);
                entry.sequence
            };
            entry.sequence = seq;

            if i == 0 {
                first_sequence = entry.sequence;
            }
            last_sequence = entry.sequence;

            let timestamp = proto_timestamp_to_unix_ms(&entry.timestamp);

            // Serialize the entire entry to protobuf bytes
            let mut entry_bytes = Vec::new();
            entry
                .encode(&mut entry_bytes)
                .map_err(|e| JournalError::Serialization(e.to_string()))?;

            use plexspaces_proto::v1::journaling::journal_entry::Entry as JournalEntryVariant;
            let entry_type = match &entry.entry {
                Some(JournalEntryVariant::MessageReceived(_)) => "MessageReceived",
                Some(JournalEntryVariant::MessageProcessed(_)) => "MessageProcessed",
                Some(JournalEntryVariant::StateChanged(_)) => "StateChanged",
                Some(JournalEntryVariant::SideEffectExecuted(_)) => "SideEffectExecuted",
                Some(JournalEntryVariant::TimerScheduled(_)) => "TimerScheduled",
                Some(JournalEntryVariant::TimerFired(_)) => "TimerFired",
                Some(JournalEntryVariant::PromiseCreated(_)) => "PromiseCreated",
                Some(JournalEntryVariant::PromiseResolved(_)) => "PromiseResolved",
                None => "Unknown",
            };

            sqlx::query(
                r#"
                INSERT INTO journal_entries (id, actor_id, sequence, timestamp, correlation_id, entry_type, entry_data)
                VALUES ($1, $2, $3, to_timestamp($4::double precision / 1000), $5, $6, $7)
                "#,
            )
            .bind(&entry.id)
            .bind(&entry.actor_id)
            .bind(entry.sequence as i64)
            .bind(timestamp as f64)
            .bind(&entry.correlation_id)
            .bind(entry_type)
            .bind(&entry_bytes)
            .execute(&mut *tx)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok((first_sequence, last_sequence, entries.len()))
    }

    async fn replay_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<JournalEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT entry_data
            FROM journal_entries
            WHERE actor_id = $1 AND sequence >= $2
            ORDER BY sequence ASC
            "#,
        )
        .bind(actor_id)
        .bind(from_sequence as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut entries = Vec::new();

        for row in rows {
            let entry_bytes: Vec<u8> = row.get("entry_data");

            // Deserialize the entire entry from protobuf bytes
            let entry = JournalEntry::decode(&entry_bytes[..])
                .map_err(|e| JournalError::Serialization(e.to_string()))?;

            entries.push(entry);
        }

        Ok(entries)
    }

    async fn get_latest_checkpoint(&self, actor_id: &str) -> JournalResult<Checkpoint> {
        let row = sqlx::query(
            r#"
            SELECT actor_id, sequence, EXTRACT(EPOCH FROM timestamp)::bigint * 1000 as timestamp_ms,
                   state_data, compression, metadata, state_schema_version
            FROM checkpoints
            WHERE actor_id = $1
            ORDER BY sequence DESC
            LIMIT 1
            "#,
        )
        .bind(actor_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if matches!(e, sqlx::Error::RowNotFound) {
                JournalError::CheckpointNotFound(actor_id.to_string())
            } else {
                JournalError::Storage(e.to_string())
            }
        })?;

        let actor_id: String = row.get("actor_id");
        let sequence: i64 = row.get("sequence");
        let timestamp_ms: i64 = row.get("timestamp_ms");
        let state_data: Vec<u8> = row.get("state_data");
        let compression: i32 = row.get("compression");
        let metadata: Option<serde_json::Value> = row.get("metadata");
        // Read state_schema_version (default to 1 if column doesn't exist for backward compatibility)
        let state_schema_version: i32 = row.try_get("state_schema_version").unwrap_or(1);

        let metadata_map: HashMap<String, String> = if let Some(json) = metadata {
            serde_json::from_value(json).unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Checkpoint {
            actor_id,
            sequence: sequence as u64,
            timestamp: unix_ms_to_proto_timestamp(timestamp_ms),
            state_data,
            compression,
            metadata: metadata_map,
            state_schema_version: state_schema_version as u32,
        })
    }

    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> JournalResult<()> {
        let timestamp_ms = proto_timestamp_to_unix_ms(&checkpoint.timestamp);
        let metadata_json = serde_json::to_value(&checkpoint.metadata)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO checkpoints (actor_id, sequence, timestamp, state_data, compression, metadata, state_schema_version)
            VALUES ($1, $2, to_timestamp($3::double precision / 1000), $4, $5, $6, $7)
            ON CONFLICT (actor_id, sequence) DO UPDATE SET
                timestamp = EXCLUDED.timestamp,
                state_data = EXCLUDED.state_data,
                compression = EXCLUDED.compression,
                metadata = EXCLUDED.metadata,
                state_schema_version = EXCLUDED.state_schema_version
            "#,
        )
        .bind(&checkpoint.actor_id)
        .bind(checkpoint.sequence as i64)
        .bind(timestamp_ms as f64)
        .bind(&checkpoint.state_data)
        .bind(checkpoint.compression)
        .bind(metadata_json)
        .bind(checkpoint.state_schema_version as i32)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn truncate_to(&self, actor_id: &str, sequence: u64) -> JournalResult<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM journal_entries
            WHERE actor_id = $1 AND sequence <= $2
            "#,
        )
        .bind(actor_id)
        .bind(sequence as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(result.rows_affected())
    }

    async fn get_stats(&self, actor_id: Option<&str>) -> JournalResult<JournalStats> {
        let mut stats = JournalStats {
            total_entries: 0,
            total_checkpoints: 0,
            storage_bytes: 0,
            entries_by_actor: HashMap::new(),
            oldest_entry: None,
            newest_entry: None,
        };

        if let Some(aid) = actor_id {
            // Stats for specific actor
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count,
                       MIN(EXTRACT(EPOCH FROM timestamp)::bigint * 1000) as oldest,
                       MAX(EXTRACT(EPOCH FROM timestamp)::bigint * 1000) as newest
                FROM journal_entries
                WHERE actor_id = $1
                "#,
            )
            .bind(aid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            let count: i64 = row.get("count");
            stats.total_entries = count as u64;
            stats.entries_by_actor.insert(aid.to_string(), count as u64);

            // Only get timestamps if there are entries (MIN/MAX return NULL for empty sets)
            if count > 0 {
                if let Ok(oldest) = row.try_get::<i64, _>("oldest") {
                    stats.oldest_entry = unix_ms_to_proto_timestamp(oldest);
                }
                if let Ok(newest) = row.try_get::<i64, _>("newest") {
                    stats.newest_entry = unix_ms_to_proto_timestamp(newest);
                }
            }

            // Checkpoint count
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count
                FROM checkpoints
                WHERE actor_id = $1
                "#,
            )
            .bind(aid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            stats.total_checkpoints = row.get::<i64, _>("count") as u64;
        } else {
            // Global stats
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count,
                       MIN(EXTRACT(EPOCH FROM timestamp)::bigint * 1000) as oldest,
                       MAX(EXTRACT(EPOCH FROM timestamp)::bigint * 1000) as newest
                FROM journal_entries
                "#,
            )
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            let count: i64 = row.get("count");
            stats.total_entries = count as u64;

            // Only get timestamps if there are entries (MIN/MAX return NULL for empty sets)
            if count > 0 {
                if let Ok(oldest) = row.try_get::<i64, _>("oldest") {
                    stats.oldest_entry = unix_ms_to_proto_timestamp(oldest);
                }
                if let Ok(newest) = row.try_get::<i64, _>("newest") {
                    stats.newest_entry = unix_ms_to_proto_timestamp(newest);
                }
            }

            // Per-actor counts
            let rows = sqlx::query(
                r#"
                SELECT actor_id, COUNT(*) as count
                FROM journal_entries
                GROUP BY actor_id
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            for row in rows {
                let actor_id: String = row.get("actor_id");
                let count: i64 = row.get("count");
                stats.entries_by_actor.insert(actor_id, count as u64);
            }

            // Total checkpoints
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count
                FROM checkpoints
                "#,
            )
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            stats.total_checkpoints = row.get::<i64, _>("count") as u64;
        }

        Ok(stats)
    }

    async fn flush(&self) -> JournalResult<()> {
        // PostgreSQL auto-commits by default
        Ok(())
    }

    // ==================== Event Sourcing Methods ====================

    async fn append_event(&self, event: &ActorEvent) -> JournalResult<u64> {
        let mut event = event.clone();

        // Assign sequence if not set
        if event.sequence == 0 {
            event.sequence = self.next_event_sequence(&event.actor_id).await?;
        }

        let sequence = event.sequence;
        let timestamp = event.timestamp.as_ref().map(|ts| {
            chrono::DateTime::<chrono::Utc>::from_timestamp(ts.seconds, ts.nanos as u32)
                .unwrap_or_else(|| chrono::Utc::now())
        }).unwrap_or_else(|| chrono::Utc::now());

        // Serialize metadata to JSONB
        let metadata_json = serde_json::to_value(&event.metadata)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO actor_events (id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(&event.id)
        .bind(&event.actor_id)
        .bind(sequence as i64)
        .bind(&event.event_type)
        .bind(&event.event_data)
        .bind(timestamp)
        .bind(&event.caused_by)
        .bind(metadata_json)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(sequence)
    }

    async fn append_events_batch(&self, events: &[ActorEvent]) -> JournalResult<(u64, u64, usize)> {
        if events.is_empty() {
            return Ok((0, 0, 0));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut first_sequence = 0u64;
        let mut last_sequence = 0u64;

        for (i, event) in events.iter().enumerate() {
            let mut event = event.clone();

            if event.sequence == 0 {
                event.sequence = self.next_event_sequence(&event.actor_id).await?;
            }

            if i == 0 {
                first_sequence = event.sequence;
            }
            last_sequence = event.sequence;

            let timestamp = event.timestamp.as_ref().map(|ts| {
                chrono::DateTime::<chrono::Utc>::from_timestamp(ts.seconds, ts.nanos as u32)
                    .unwrap_or_else(|| chrono::Utc::now())
            }).unwrap_or_else(|| chrono::Utc::now());

            let metadata_json = serde_json::to_value(&event.metadata)
                .map_err(|e| JournalError::Serialization(e.to_string()))?;

            sqlx::query(
                r#"
                INSERT INTO actor_events (id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                "#,
            )
            .bind(&event.id)
            .bind(&event.actor_id)
            .bind(event.sequence as i64)
            .bind(&event.event_type)
            .bind(&event.event_data)
            .bind(timestamp)
            .bind(&event.caused_by)
            .bind(metadata_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok((first_sequence, last_sequence, events.len()))
    }

    async fn replay_events_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<ActorEvent>> {
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = $1 AND sequence >= $2
            ORDER BY sequence ASC
            "#,
        )
        .bind(actor_id)
        .bind(from_sequence as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut events = Vec::new();
        for row in rows {
            let metadata_value: serde_json::Value = row.get("metadata");
            let metadata: HashMap<String, String> = serde_json::from_value(metadata_value)
                .unwrap_or_default();

            let timestamp_pg: chrono::DateTime<chrono::Utc> = row.get("timestamp");
            let timestamp = Some(prost_types::Timestamp {
                seconds: timestamp_pg.timestamp(),
                nanos: timestamp_pg.timestamp_subsec_nanos() as i32,
            });

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence: row.get::<i64, _>("sequence") as u64,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp,
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        Ok(events)
    }

    async fn replay_events_from_paginated(
        &self,
        actor_id: &str,
        from_sequence: u64,
        page_request: &PageRequest,
    ) -> JournalResult<(Vec<ActorEvent>, PageResponse)> {
        // Decode page_token to get starting sequence
        let start_sequence = if page_request.page_token.is_empty() {
            from_sequence
        } else {
            page_request.page_token.parse::<u64>().unwrap_or(from_sequence)
        };

        // Validate and clamp page_size (1-1000)
        let page_size = page_request.page_size.max(1).min(1000) as i64;

        // Fetch page_size + 1 to check if there's more
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = $1 AND sequence >= $2 AND sequence >= $3
            ORDER BY sequence ASC
            LIMIT $4
            "#,
        )
        .bind(actor_id)
        .bind(start_sequence.max(from_sequence) as i64)
        .bind(from_sequence as i64)
        .bind(page_size + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let has_more = rows.len() > page_size as usize;
        let rows_to_return = rows.iter().take(page_size as usize);

        let mut events = Vec::new();
        for row in rows_to_return {
            let metadata_value: serde_json::Value = row.get("metadata");
            let metadata: HashMap<String, String> = serde_json::from_value(metadata_value)
                .unwrap_or_default();

            let timestamp_pg: chrono::DateTime<chrono::Utc> = row.get("timestamp");
            let timestamp = Some(prost_types::Timestamp {
                seconds: timestamp_pg.timestamp(),
                nanos: timestamp_pg.timestamp_subsec_nanos() as i32,
            });

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence: row.get::<i64, _>("sequence") as u64,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp,
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        // Generate next_page_token if there are more events
        let next_page_token = if has_more && !events.is_empty() {
            (events.last().unwrap().sequence + 1).to_string()
        } else {
            String::new()
        };

        let page_response = PageResponse {
            next_page_token,
            total_size: 0, // Total size not available without full scan (expensive)
        };

        Ok((events, page_response))
    }

    async fn get_actor_history(&self, actor_id: &str) -> JournalResult<ActorHistory> {
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = $1
            ORDER BY sequence ASC
            "#,
        )
        .bind(actor_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut events = Vec::new();
        let mut latest_sequence = 0u64;
        let mut created_at: Option<prost_types::Timestamp> = None;
        let mut updated_at: Option<prost_types::Timestamp> = None;

        for row in rows {
            let metadata_value: serde_json::Value = row.get("metadata");
            let metadata: HashMap<String, String> = serde_json::from_value(metadata_value)
                .unwrap_or_default();

            let timestamp_pg: chrono::DateTime<chrono::Utc> = row.get("timestamp");
            let timestamp = Some(prost_types::Timestamp {
                seconds: timestamp_pg.timestamp(),
                nanos: timestamp_pg.timestamp_subsec_nanos() as i32,
            });

            if created_at.is_none() {
                created_at = timestamp.clone();
            }
            updated_at = timestamp.clone();

            let sequence = row.get::<i64, _>("sequence") as u64;
            latest_sequence = latest_sequence.max(sequence);

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp,
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        Ok(ActorHistory {
            actor_id: actor_id.to_string(),
            events,
            latest_sequence,
            created_at: created_at.or_else(|| Some(prost_types::Timestamp::from(SystemTime::now()))),
            updated_at: updated_at.or_else(|| Some(prost_types::Timestamp::from(SystemTime::now()))),
            metadata: HashMap::new(),
            page_response: None,
        })
    }

    async fn get_actor_history_paginated(
        &self,
        actor_id: &str,
        page_request: &PageRequest,
    ) -> JournalResult<ActorHistory> {
        // Decode page_token to get starting sequence
        let start_sequence = if page_request.page_token.is_empty() {
            0
        } else {
            page_request.page_token.parse::<u64>().unwrap_or(0)
        };

        // Validate and clamp page_size (1-1000)
        let page_size = page_request.page_size.max(1).min(1000) as i64;

        // Get latest sequence for history metadata
        let latest_row = sqlx::query(
            r#"
            SELECT MAX(sequence) as max_seq, MIN(timestamp) as min_ts, MAX(timestamp) as max_ts
            FROM actor_events
            WHERE actor_id = $1
            "#,
        )
        .bind(actor_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let latest_sequence = latest_row
            .as_ref()
            .and_then(|r| r.get::<Option<i64>, _>("max_seq"))
            .map(|s| s as u64)
            .unwrap_or(0);

        let created_at = latest_row
            .as_ref()
            .and_then(|r| {
                r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("min_ts")
                    .map(|ts| Some(prost_types::Timestamp {
                        seconds: ts.timestamp(),
                        nanos: ts.timestamp_subsec_nanos() as i32,
                    }))
            })
            .flatten()
            .or_else(|| Some(prost_types::Timestamp::from(SystemTime::now())));

        let updated_at = latest_row
            .as_ref()
            .and_then(|r| {
                r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("max_ts")
                    .map(|ts| Some(prost_types::Timestamp {
                        seconds: ts.timestamp(),
                        nanos: ts.timestamp_subsec_nanos() as i32,
                    }))
            })
            .flatten()
            .or_else(|| Some(prost_types::Timestamp::from(SystemTime::now())));

        // Fetch page_size + 1 to check if there's more
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = $1 AND sequence >= $2
            ORDER BY sequence ASC
            LIMIT $3
            "#,
        )
        .bind(actor_id)
        .bind(start_sequence as i64)
        .bind(page_size + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let has_more = rows.len() > page_size as usize;
        let rows_to_return = rows.iter().take(page_size as usize);

        let mut events = Vec::new();
        for row in rows_to_return {
            let metadata_value: serde_json::Value = row.get("metadata");
            let metadata: HashMap<String, String> = serde_json::from_value(metadata_value)
                .unwrap_or_default();

            let timestamp_pg: chrono::DateTime<chrono::Utc> = row.get("timestamp");
            let timestamp = Some(prost_types::Timestamp {
                seconds: timestamp_pg.timestamp(),
                nanos: timestamp_pg.timestamp_subsec_nanos() as i32,
            });

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence: row.get::<i64, _>("sequence") as u64,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp,
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        // Generate next_page_token if there are more events
        let next_page_token = if has_more && !events.is_empty() {
            (events.last().unwrap().sequence + 1).to_string()
        } else {
            String::new()
        };

        let page_response = PageResponse {
            next_page_token,
            total_size: 0, // Total size not available without full scan (expensive)
        };

        Ok(ActorHistory {
            actor_id: actor_id.to_string(),
            events,
            latest_sequence,
            created_at,
            updated_at,
            metadata: HashMap::new(),
            page_response: Some(page_response),
        })
    }

    // ==================== Reminder Methods ====================

    async fn register_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()> {
        let now = system_time_to_unix_ms(SystemTime::now());
        let reg = reminder_state.registration.as_ref().ok_or_else(|| {
            JournalError::Configuration("ReminderState must have registration".to_string())
        })?;
        
        let interval_seconds = reg.interval.as_ref().map(|d| d.seconds);
        let interval_nanos = reg.interval.as_ref().map(|d| d.nanos);
        let first_fire_seconds = reg.first_fire_time.as_ref().map(|t| t.seconds);
        let first_fire_nanos = reg.first_fire_time.as_ref().map(|t| t.nanos);
        let last_fired_seconds = reminder_state.last_fired.as_ref().map(|t| t.seconds);
        let last_fired_nanos = reminder_state.last_fired.as_ref().map(|t| t.nanos);
        let next_fire_seconds = reminder_state.next_fire_time.as_ref().map(|t| t.seconds);
        let next_fire_nanos = reminder_state.next_fire_time.as_ref().map(|t| t.nanos);
        
        sqlx::query(
            r#"
            INSERT INTO reminders (
                actor_id, reminder_name,
                interval_seconds, interval_nanos,
                first_fire_time_seconds, first_fire_time_nanos,
                callback_data,
                persist_across_activations, max_occurrences,
                last_fired_seconds, last_fired_nanos,
                next_fire_time_seconds, next_fire_time_nanos,
                fire_count, is_active,
                created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            ON CONFLICT (actor_id, reminder_name) DO UPDATE SET
                interval_seconds = EXCLUDED.interval_seconds,
                interval_nanos = EXCLUDED.interval_nanos,
                first_fire_time_seconds = EXCLUDED.first_fire_time_seconds,
                first_fire_time_nanos = EXCLUDED.first_fire_time_nanos,
                callback_data = EXCLUDED.callback_data,
                persist_across_activations = EXCLUDED.persist_across_activations,
                max_occurrences = EXCLUDED.max_occurrences,
                last_fired_seconds = EXCLUDED.last_fired_seconds,
                last_fired_nanos = EXCLUDED.last_fired_nanos,
                next_fire_time_seconds = EXCLUDED.next_fire_time_seconds,
                next_fire_time_nanos = EXCLUDED.next_fire_time_nanos,
                fire_count = EXCLUDED.fire_count,
                is_active = EXCLUDED.is_active,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&reg.actor_id)
        .bind(&reg.reminder_name)
        .bind(interval_seconds)
        .bind(interval_nanos)
        .bind(first_fire_seconds)
        .bind(first_fire_nanos)
        .bind(&reg.callback_data)
        .bind(reg.persist_across_activations)
        .bind(reg.max_occurrences)
        .bind(last_fired_seconds)
        .bind(last_fired_nanos)
        .bind(next_fire_seconds)
        .bind(next_fire_nanos)
        .bind(reminder_state.fire_count)
        .bind(reminder_state.is_active)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn unregister_reminder(&self, actor_id: &str, reminder_name: &str) -> JournalResult<()> {
        sqlx::query(
            r#"
            DELETE FROM reminders
            WHERE actor_id = $1 AND reminder_name = $2
            "#,
        )
        .bind(actor_id)
        .bind(reminder_name)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn load_reminders(&self, actor_id: &str) -> JournalResult<Vec<ReminderState>> {
        let rows = sqlx::query(
            r#"
            SELECT 
                actor_id, reminder_name,
                interval_seconds, interval_nanos,
                first_fire_time_seconds, first_fire_time_nanos,
                callback_data,
                persist_across_activations, max_occurrences,
                last_fired_seconds, last_fired_nanos,
                next_fire_time_seconds, next_fire_time_nanos,
                fire_count, is_active
            FROM reminders
            WHERE actor_id = $1 AND is_active = TRUE
            "#,
        )
        .bind(actor_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut reminders = Vec::new();
        for row in rows {
            let reminder = row_to_reminder_state_pg(&row)?;
            reminders.push(reminder);
        }

        Ok(reminders)
    }

    async fn update_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()> {
        let now = system_time_to_unix_ms(SystemTime::now());
        let reg = reminder_state.registration.as_ref().ok_or_else(|| {
            JournalError::Configuration("ReminderState must have registration".to_string())
        })?;
        
        let interval_seconds = reg.interval.as_ref().map(|d| d.seconds);
        let interval_nanos = reg.interval.as_ref().map(|d| d.nanos);
        let first_fire_seconds = reg.first_fire_time.as_ref().map(|t| t.seconds);
        let first_fire_nanos = reg.first_fire_time.as_ref().map(|t| t.nanos);
        let last_fired_seconds = reminder_state.last_fired.as_ref().map(|t| t.seconds);
        let last_fired_nanos = reminder_state.last_fired.as_ref().map(|t| t.nanos);
        let next_fire_seconds = reminder_state.next_fire_time.as_ref().map(|t| t.seconds);
        let next_fire_nanos = reminder_state.next_fire_time.as_ref().map(|t| t.nanos);
        
        sqlx::query(
            r#"
            UPDATE reminders SET
                interval_seconds = $1,
                interval_nanos = $2,
                first_fire_time_seconds = $3,
                first_fire_time_nanos = $4,
                callback_data = $5,
                persist_across_activations = $6,
                max_occurrences = $7,
                last_fired_seconds = $8,
                last_fired_nanos = $9,
                next_fire_time_seconds = $10,
                next_fire_time_nanos = $11,
                fire_count = $12,
                is_active = $13,
                updated_at = $14
            WHERE actor_id = $15 AND reminder_name = $16
            "#,
        )
        .bind(interval_seconds)
        .bind(interval_nanos)
        .bind(first_fire_seconds)
        .bind(first_fire_nanos)
        .bind(&reg.callback_data)
        .bind(reg.persist_across_activations)
        .bind(reg.max_occurrences)
        .bind(last_fired_seconds)
        .bind(last_fired_nanos)
        .bind(next_fire_seconds)
        .bind(next_fire_nanos)
        .bind(reminder_state.fire_count)
        .bind(reminder_state.is_active)
        .bind(now)
        .bind(&reg.actor_id)
        .bind(&reg.reminder_name)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn query_due_reminders(&self, before_time: SystemTime) -> JournalResult<Vec<ReminderState>> {
        let before_ms = system_time_to_unix_ms(before_time);
        let before_seconds = before_ms / 1000;
        let before_nanos = ((before_ms % 1000) * 1_000_000) as i32;
        
        let rows = sqlx::query(
            r#"
            SELECT 
                actor_id, reminder_name,
                interval_seconds, interval_nanos,
                first_fire_time_seconds, first_fire_time_nanos,
                callback_data,
                persist_across_activations, max_occurrences,
                last_fired_seconds, last_fired_nanos,
                next_fire_time_seconds, next_fire_time_nanos,
                fire_count, is_active
            FROM reminders
            WHERE is_active = TRUE
            AND (
                next_fire_time_seconds < $1 OR
                (next_fire_time_seconds = $1 AND next_fire_time_nanos <= $2)
            )
            ORDER BY next_fire_time_seconds ASC, next_fire_time_nanos ASC
            "#,
        )
        .bind(before_seconds)
        .bind(before_nanos)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut reminders = Vec::new();
        for row in rows {
            let reminder = row_to_reminder_state_pg(&row)?;
            reminders.push(reminder);
        }

        Ok(reminders)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_entry(actor_id: &str, sequence: u64) -> JournalEntry {
        use plexspaces_proto::v1::journaling::{journal_entry, MessageReceived};

        JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            sequence,
            timestamp: Some(prost_types::Timestamp {
                seconds: 1000,
                nanos: 0,
            }),
            correlation_id: String::new(),
            entry: Some(journal_entry::Entry::MessageReceived(MessageReceived {
                message_id: "msg-1".to_string(),
                sender_id: "sender-1".to_string(),
                message_type: "test".to_string(),
                payload: vec![1, 2, 3],
                metadata: HashMap::new(),
            })),
        }
    }

    #[tokio::test]
    async fn test_sqlite_append_and_replay() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        let entry1 = create_test_entry("actor-1", 1).await;
        let entry2 = create_test_entry("actor-1", 2).await;

        storage.append_entry(&entry1).await.unwrap();
        storage.append_entry(&entry2).await.unwrap();

        let entries = storage.replay_from("actor-1", 1).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[1].sequence, 2);
    }

    #[tokio::test]
    async fn test_sqlite_append_batch() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        let entries = vec![
            create_test_entry("actor-1", 1).await,
            create_test_entry("actor-1", 2).await,
            create_test_entry("actor-1", 3).await,
        ];

        let (first, last, count) = storage.append_batch(&entries).await.unwrap();
        assert_eq!(first, 1);
        assert_eq!(last, 3);
        assert_eq!(count, 3);

        let replay = storage.replay_from("actor-1", 1).await.unwrap();
        assert_eq!(replay.len(), 3);
    }

    #[tokio::test]
    async fn test_sqlite_checkpoint() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        let checkpoint = Checkpoint {
            actor_id: "actor-1".to_string(),
            sequence: 100,
            timestamp: Some(prost_types::Timestamp {
                seconds: 2000,
                nanos: 0,
            }),
            state_data: vec![1, 2, 3, 4],
            compression: 0,
            metadata: HashMap::from([("version".to_string(), "1.0".to_string())]),
            state_schema_version: 0,
        };

        storage.save_checkpoint(&checkpoint).await.unwrap();

        let loaded = storage.get_latest_checkpoint("actor-1").await.unwrap();
        assert_eq!(loaded.sequence, 100);
        assert_eq!(loaded.state_data, vec![1, 2, 3, 4]);
        assert_eq!(loaded.metadata.get("version"), Some(&"1.0".to_string()));
    }

    #[tokio::test]
    async fn test_sqlite_truncate() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        for i in 1..=10 {
            let entry = create_test_entry("actor-1", i).await;
            storage.append_entry(&entry).await.unwrap();
        }

        let deleted = storage.truncate_to("actor-1", 5).await.unwrap();
        assert_eq!(deleted, 5);

        let entries = storage.replay_from("actor-1", 1).await.unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].sequence, 6);
    }

    #[tokio::test]
    async fn test_sqlite_stats() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        for i in 1..=5 {
            let entry = create_test_entry("actor-1", i).await;
            storage.append_entry(&entry).await.unwrap();
        }

        for i in 1..=3 {
            let entry = create_test_entry("actor-2", i).await;
            storage.append_entry(&entry).await.unwrap();
        }

        let stats = storage.get_stats(None).await.unwrap();
        assert_eq!(stats.total_entries, 8);
        assert_eq!(stats.entries_by_actor.get("actor-1"), Some(&5));
        assert_eq!(stats.entries_by_actor.get("actor-2"), Some(&3));

        let stats = storage.get_stats(Some("actor-1")).await.unwrap();
        assert_eq!(stats.total_entries, 5);
    }

    #[tokio::test]
    async fn test_sqlite_auto_sequence() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        let entry1 = create_test_entry("actor-1", 0).await; // sequence = 0
        let seq1 = storage.append_entry(&entry1).await.unwrap();
        assert_eq!(seq1, 1);

        let entry2 = create_test_entry("actor-1", 0).await; // sequence = 0, but new ID
        let seq2 = storage.append_entry(&entry2).await.unwrap();
        assert_eq!(seq2, 2);
    }

    // ==================== Additional Edge Case Tests for 95%+ Coverage ====================

    #[tokio::test]
    async fn test_sqlite_empty_batch_append() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // Empty batch should return (0, 0, 0)
        let empty_batch: Vec<JournalEntry> = vec![];
        let (first, last, count) = storage.append_batch(&empty_batch).await.unwrap();
        assert_eq!(first, 0);
        assert_eq!(last, 0);
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_sqlite_replay_no_entries() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // Replay for actor that has no entries
        let entries = storage.replay_from("nonexistent-actor", 1).await.unwrap();
        assert_eq!(entries.len(), 0);

        // Replay from high sequence for actor with entries
        let entry = create_test_entry("actor-1", 1).await;
        storage.append_entry(&entry).await.unwrap();

        let entries = storage.replay_from("actor-1", 100).await.unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[tokio::test]
    async fn test_sqlite_checkpoint_not_found() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // Try to get checkpoint for actor that doesn't exist
        let result = storage.get_latest_checkpoint("nonexistent-actor").await;
        assert!(result.is_err());
        match result {
            Err(JournalError::CheckpointNotFound(actor_id)) => {
                assert_eq!(actor_id, "nonexistent-actor");
            }
            _ => panic!("Expected CheckpointNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_sqlite_large_batch_append() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // Create large batch (1000 entries)
        let mut large_batch = vec![];
        for i in 1..=1000 {
            large_batch.push(create_test_entry("actor-1", i).await);
        }

        let (first, last, count) = storage.append_batch(&large_batch).await.unwrap();
        assert_eq!(first, 1);
        assert_eq!(last, 1000);
        assert_eq!(count, 1000);

        // Verify all entries can be replayed
        let entries = storage.replay_from("actor-1", 1).await.unwrap();
        assert_eq!(entries.len(), 1000);
    }

    #[tokio::test]
    async fn test_sqlite_truncate_no_entries() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // Truncate for actor with no entries should return 0
        let deleted = storage.truncate_to("nonexistent-actor", 100).await.unwrap();
        assert_eq!(deleted, 0);
    }

    #[tokio::test]
    async fn test_sqlite_stats_empty_actor() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // Stats for non-existent actor
        let stats = storage.get_stats(Some("nonexistent-actor")).await.unwrap();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.total_checkpoints, 0);
        assert!(stats.oldest_entry.is_none());
        assert!(stats.newest_entry.is_none());

        // Global stats when DB is empty
        let stats = storage.get_stats(None).await.unwrap();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.total_checkpoints, 0);
    }

    #[tokio::test]
    async fn test_sqlite_concurrent_append() {
        use std::sync::Arc;

        let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());

        // Spawn multiple concurrent append tasks
        let mut handles = vec![];
        for i in 0..10 {
            let storage_clone = Arc::clone(&storage);
            let handle = tokio::spawn(async move {
                let entry = create_test_entry(&format!("actor-{}", i), 0).await;
                storage_clone.append_entry(&entry).await.unwrap()
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all actors have exactly 1 entry
        let stats = storage.get_stats(None).await.unwrap();
        assert_eq!(stats.total_entries, 10);
        assert_eq!(stats.entries_by_actor.len(), 10);
    }

    #[tokio::test]
    async fn test_sqlite_checkpoint_metadata_edge_cases() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // Checkpoint with empty metadata
        let checkpoint_empty = Checkpoint {
            actor_id: "actor-1".to_string(),
            sequence: 1,
            timestamp: Some(prost_types::Timestamp {
                seconds: 1000,
                nanos: 0,
            }),
            state_data: vec![1, 2, 3],
            compression: 0,
            metadata: HashMap::new(),
            state_schema_version: 0,
        };
        storage.save_checkpoint(&checkpoint_empty).await.unwrap();

        let loaded = storage.get_latest_checkpoint("actor-1").await.unwrap();
        assert!(loaded.metadata.is_empty());

        // Checkpoint with complex metadata
        let checkpoint_complex = Checkpoint {
            actor_id: "actor-2".to_string(),
            sequence: 2,
            timestamp: Some(prost_types::Timestamp {
                seconds: 2000,
                nanos: 0,
            }),
            state_data: vec![4, 5, 6],
            compression: 1,
            metadata: HashMap::from([
                ("version".to_string(), "1.0".to_string()),
                ("actor_type".to_string(), "GenServer".to_string()),
                ("timestamp".to_string(), "2025-01-11T12:00:00Z".to_string()),
                ("compressed".to_string(), "true".to_string()),
            ]),
            state_schema_version: 0,
        };
        storage.save_checkpoint(&checkpoint_complex).await.unwrap();

        let loaded = storage.get_latest_checkpoint("actor-2").await.unwrap();
        assert_eq!(loaded.metadata.len(), 4);
        assert_eq!(loaded.metadata.get("version"), Some(&"1.0".to_string()));
        assert_eq!(loaded.compression, 1);
    }

    #[tokio::test]
    async fn test_sqlite_multiple_checkpoints_latest_only() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // Save multiple checkpoints for same actor
        for i in 1..=5 {
            let checkpoint = Checkpoint {
                actor_id: "actor-1".to_string(),
                sequence: i * 10,
                timestamp: Some(prost_types::Timestamp {
                    seconds: 1000 + i as i64,
                    nanos: 0,
                }),
                state_data: vec![i as u8],
                compression: 0,
                metadata: HashMap::from([("seq".to_string(), i.to_string())]),
                state_schema_version: 0,
            };
            storage.save_checkpoint(&checkpoint).await.unwrap();
        }

        // get_latest_checkpoint should return the highest sequence (50)
        let latest = storage.get_latest_checkpoint("actor-1").await.unwrap();
        assert_eq!(latest.sequence, 50);
        assert_eq!(latest.state_data, vec![5]);
    }

    #[tokio::test]
    async fn test_sqlite_replay_partial_range() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // Add entries 1-10
        for i in 1..=10 {
            let entry = create_test_entry("actor-1", i).await;
            storage.append_entry(&entry).await.unwrap();
        }

        // Replay from middle (sequence 5)
        let entries = storage.replay_from("actor-1", 5).await.unwrap();
        assert_eq!(entries.len(), 6); // sequences 5-10
        assert_eq!(entries[0].sequence, 5);
        assert_eq!(entries[5].sequence, 10);

        // Replay from beginning
        let entries = storage.replay_from("actor-1", 0).await.unwrap();
        assert_eq!(entries.len(), 10);
    }

    #[tokio::test]
    async fn test_sqlite_flush() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // Flush should succeed (no-op for SQLite with WAL)
        storage.flush().await.unwrap();

        // Verify data persists after flush
        let entry = create_test_entry("actor-1", 1).await;
        storage.append_entry(&entry).await.unwrap();
        storage.flush().await.unwrap();

        let entries = storage.replay_from("actor-1", 1).await.unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_sqlite_sequence_caching() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        // First append triggers cache miss (queries DB)
        let entry1 = create_test_entry("actor-1", 0).await;
        let seq1 = storage.append_entry(&entry1).await.unwrap();
        assert_eq!(seq1, 1);

        // Second append uses cache (no DB query for sequence)
        let entry2 = create_test_entry("actor-1", 0).await;
        let seq2 = storage.append_entry(&entry2).await.unwrap();
        assert_eq!(seq2, 2);

        // Different actor triggers new cache miss
        let entry3 = create_test_entry("actor-2", 0).await;
        let seq3 = storage.append_entry(&entry3).await.unwrap();
        assert_eq!(seq3, 1);
    }
}
