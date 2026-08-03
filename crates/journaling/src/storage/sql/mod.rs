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

use crate::storage::{ReminderRegistration, ReminderState};
use crate::{ActorEvent, ActorHistory, Checkpoint, JournalEntry, JournalStats};
use async_trait::async_trait;
use plexspaces_proto::common::v1::{PageRequest, PageResponse};
use plexspaces_proto::prost_types;
use plexspaces_service_traits::{JournalError, JournalResult, JournalStorage};
use prost::Message;
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

// Helper to convert SystemTime to Unix timestamp (milliseconds)
pub(super) fn system_time_to_unix_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// Helper to convert timestamp proto to Unix ms
pub(super) fn proto_timestamp_to_unix_ms(ts: &Option<prost_types::Timestamp>) -> i64 {
    if let Some(t) = ts {
        (t.seconds * 1000) + (t.nanos / 1_000_000) as i64
    } else {
        system_time_to_unix_ms(SystemTime::now())
    }
}

// Helper to convert Unix ms to proto timestamp
pub(super) fn unix_ms_to_proto_timestamp(ms: i64) -> Option<prost_types::Timestamp> {
    Some(prost_types::Timestamp {
        seconds: ms / 1000,
        nanos: ((ms % 1000) * 1_000_000) as i32,
    })
}

// Helper to convert SQL row to ReminderState (PostgreSQL)
#[cfg(feature = "postgres-backend")]
pub(super) fn row_to_reminder_state_pg(
    row: &sqlx::postgres::PgRow,
) -> JournalResult<ReminderState> {
    use sqlx::Row;

    let actor_id: String = row.get("actor_id");
    let reminder_name: String = row.get("reminder_name");

    let interval_seconds: Option<i64> = row.get("interval_seconds");
    let interval_nanos: Option<i32> = row.get("interval_nanos");
    let interval = if let (Some(secs), Some(nanos)) = (interval_seconds, interval_nanos) {
        Some(prost_types::Duration {
            seconds: secs,
            nanos,
        })
    } else {
        None
    };

    let first_fire_seconds: Option<i64> = row.get("first_fire_time_seconds");
    let first_fire_nanos: Option<i32> = row.get("first_fire_time_nanos");
    let first_fire_time = if let (Some(secs), Some(nanos)) = (first_fire_seconds, first_fire_nanos)
    {
        Some(prost_types::Timestamp {
            seconds: secs,
            nanos,
        })
    } else {
        None
    };

    let callback_data: Vec<u8> = row.get("callback_data");
    let persist_across_activations: bool = row.get("persist_across_activations");
    let max_occurrences: i32 = row.get("max_occurrences");

    let last_fired_seconds: Option<i64> = row.get("last_fired_seconds");
    let last_fired_nanos: Option<i32> = row.get("last_fired_nanos");
    let last_fired = if let (Some(secs), Some(nanos)) = (last_fired_seconds, last_fired_nanos) {
        Some(prost_types::Timestamp {
            seconds: secs,
            nanos,
        })
    } else {
        None
    };

    let next_fire_seconds: Option<i64> = row.get("next_fire_time_seconds");
    let next_fire_nanos: Option<i32> = row.get("next_fire_time_nanos");
    let next_fire_time = if let (Some(secs), Some(nanos)) = (next_fire_seconds, next_fire_nanos) {
        Some(prost_types::Timestamp {
            seconds: secs,
            nanos,
        })
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
pub(super) fn row_to_reminder_state(row: &sqlx::sqlite::SqliteRow) -> JournalResult<ReminderState> {
    use sqlx::Row;

    let actor_id: String = row.get("actor_id");
    let reminder_name: String = row.get("reminder_name");

    let interval_seconds: Option<i64> = row.get("interval_seconds");
    let interval_nanos: Option<i32> = row.get("interval_nanos");
    let interval = if let (Some(secs), Some(nanos)) = (interval_seconds, interval_nanos) {
        Some(prost_types::Duration {
            seconds: secs,
            nanos,
        })
    } else {
        None
    };

    let first_fire_seconds: Option<i64> = row.get("first_fire_time_seconds");
    let first_fire_nanos: Option<i32> = row.get("first_fire_time_nanos");
    let first_fire_time = if let (Some(secs), Some(nanos)) = (first_fire_seconds, first_fire_nanos)
    {
        Some(prost_types::Timestamp {
            seconds: secs,
            nanos,
        })
    } else {
        None
    };

    let callback_data: Vec<u8> = row.get("callback_data");
    let persist_across_activations: i32 = row.get("persist_across_activations");
    let max_occurrences: i32 = row.get("max_occurrences");

    let last_fired_seconds: Option<i64> = row.get("last_fired_seconds");
    let last_fired_nanos: Option<i32> = row.get("last_fired_nanos");
    let last_fired = if let (Some(secs), Some(nanos)) = (last_fired_seconds, last_fired_nanos) {
        Some(prost_types::Timestamp {
            seconds: secs,
            nanos,
        })
    } else {
        None
    };

    let next_fire_seconds: Option<i64> = row.get("next_fire_time_seconds");
    let next_fire_nanos: Option<i32> = row.get("next_fire_time_nanos");
    let next_fire_time = if let (Some(secs), Some(nanos)) = (next_fire_seconds, next_fire_nanos) {
        Some(prost_types::Timestamp {
            seconds: secs,
            nanos,
        })
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

#[cfg(feature = "sqlite-backend")]
mod sqlite;

#[cfg(feature = "sqlite-backend")]
pub use sqlite::SqliteJournalStorage;

#[cfg(feature = "postgres-backend")]
mod postgres;

#[cfg(feature = "postgres-backend")]
pub use postgres::PostgresJournalStorage;

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
                ("actor_type".to_string(), "gen_server".to_string()),
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
    async fn test_sqlite_purge_namespace_removes_matching_actor_state() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();

        let matching_checkpoint = Checkpoint {
            actor_id: "cart-1//abstractions::abstractions-rust@test-node".to_string(),
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
        let non_matching_checkpoint = Checkpoint {
            actor_id: "cart-1//abstractions::other-app@test-node".to_string(),
            sequence: 1,
            timestamp: Some(prost_types::Timestamp {
                seconds: 1001,
                nanos: 0,
            }),
            state_data: vec![4, 5, 6],
            compression: 0,
            metadata: HashMap::new(),
            state_schema_version: 0,
        };

        storage.save_checkpoint(&matching_checkpoint).await.unwrap();
        storage
            .save_checkpoint(&non_matching_checkpoint)
            .await
            .unwrap();

        let deleted = storage.purge_namespace("abstractions-rust").await.unwrap();
        assert!(deleted >= 1);

        assert!(matches!(
            storage
                .get_latest_checkpoint("cart-1//abstractions::abstractions-rust@test-node")
                .await,
            Err(JournalError::CheckpointNotFound(_))
        ));

        let remaining = storage
            .get_latest_checkpoint("cart-1//abstractions::other-app@test-node")
            .await
            .unwrap();
        assert_eq!(remaining.state_data, vec![4, 5, 6]);
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
