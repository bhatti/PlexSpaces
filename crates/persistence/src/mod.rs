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

//! Journal module for durable execution
//!
//! Implements event sourcing and journaling inspired by Restate,
//! enabling replay, recovery, and time-travel debugging.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use plexspaces_mailbox::Message;

/// Actor identifier (type alias for String)
pub type ActorId = String;

/// Journal trait for durable execution
#[async_trait]
pub trait Journal: Send + Sync {
    /// Record that a message was received
    async fn record_message_received(&self, message: &Message) -> Result<(), JournalError>;

    /// Record that a message was processed
    async fn record_message_processed(
        &self,
        message: &Message,
        result: &Result<(), String>, // Error as String instead of BehaviorError
    ) -> Result<(), JournalError>;

    /// Record a state change
    async fn record_state_change(
        &self,
        actor_id: &ActorId,
        old_state: Vec<u8>,
        new_state: Vec<u8>,
    ) -> Result<(), JournalError>;

    /// Record a side effect (external call, timer, etc.)
    async fn record_side_effect(
        &self,
        actor_id: &ActorId,
        effect: SideEffect,
    ) -> Result<(), JournalError>;

    /// Record a promise creation (Restate-inspired)
    async fn record_promise_created(
        &self,
        promise_id: &str,
        metadata: PromiseMetadata,
    ) -> Result<(), JournalError>;

    /// Record a promise resolution
    async fn record_promise_resolved(
        &self,
        promise_id: &str,
        result: PromiseResult,
    ) -> Result<(), JournalError>;

    /// Replay journal from a specific point
    async fn replay_from(
        &self,
        actor_id: &ActorId,
        from_sequence: u64,
    ) -> Result<Vec<JournalEntry>, JournalError>;

    /// Get the latest snapshot for an actor
    async fn get_latest_snapshot(
        &self,
        actor_id: &ActorId,
    ) -> Result<Option<Snapshot>, JournalError>;

    /// Save a snapshot
    async fn save_snapshot(&self, snapshot: Snapshot) -> Result<(), JournalError>;

    /// Truncate journal up to a sequence number (after snapshotting)
    async fn truncate_to(&self, actor_id: &ActorId, sequence: u64) -> Result<(), JournalError>;

    /// Append a journal entry directly (for lifecycle events and other direct journal access)
    async fn append(&self, entry: JournalEntry) -> Result<(), JournalError>;

    /// Append multiple journal entries in a single batch operation (for better throughput)
    ///
    /// ## Performance
    /// Batching multiple entries reduces lock contention and I/O overhead.
    /// Provides 3-5x better throughput compared to individual appends.
    ///
    /// ## Atomicity
    /// All entries in the batch are appended atomically. Either all succeed or all fail.
    async fn append_batch(&self, entries: Vec<JournalEntry>) -> Result<(), JournalError>;

    /// Get all journal entries (primarily for testing and debugging)
    async fn get_entries(&self) -> Result<Vec<JournalEntry>, JournalError>;
}

/// Journal entry types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalEntry {
    /// Message received
    MessageReceived {
        /// Sequence number in journal
        sequence: u64,
        /// When the message was received
        timestamp: DateTime<Utc>,
        /// The message that was received
        message: MessageRecord,
    },
    /// Message processed
    MessageProcessed {
        /// Sequence number in journal
        sequence: u64,
        /// When processing completed
        timestamp: DateTime<Utc>,
        /// ID of the message that was processed
        message_id: String,
        /// Processing result (success/error/retry)
        result: ProcessingResult,
    },
    /// State changed
    StateChanged {
        /// Sequence number in journal
        sequence: u64,
        /// When the state changed
        timestamp: DateTime<Utc>,
        /// Hash of old state
        old_hash: String,
        /// Hash of new state
        new_hash: String,
        /// New state data
        state_data: Vec<u8>,
    },
    /// Side effect recorded
    SideEffectRecorded {
        /// Sequence number in journal
        sequence: u64,
        /// When the side effect was recorded
        timestamp: DateTime<Utc>,
        /// The side effect that was executed
        effect: SideEffect,
    },
    /// Promise created (Restate-inspired)
    PromiseCreated {
        /// Sequence number in journal
        sequence: u64,
        /// When the promise was created
        timestamp: DateTime<Utc>,
        /// Unique promise identifier
        promise_id: String,
        /// Promise metadata
        metadata: PromiseMetadata,
    },
    /// Promise resolved
    PromiseResolved {
        /// Sequence number in journal
        sequence: u64,
        /// When the promise was resolved
        timestamp: DateTime<Utc>,
        /// Unique promise identifier
        promise_id: String,
        /// Resolution result (value or error)
        result: PromiseResult,
    },
    /// Lifecycle event (activation, deactivation, timer)
    Lifecycle {
        /// Sequence number in journal
        sequence: u64,
        /// When the lifecycle event occurred
        timestamp: DateTime<Utc>,
        /// Actor that had the lifecycle event
        actor_id: String,
        /// Lifecycle event type
        event: String,
    },
    /// Snapshot taken
    SnapshotTaken {
        /// Sequence number in journal
        sequence: u64,
        /// When the snapshot was taken
        timestamp: DateTime<Utc>,
        /// Unique snapshot identifier
        snapshot_id: String,
    },
}

/// Message record for journaling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    /// Message unique identifier
    pub id: String,
    /// Message payload bytes
    pub payload: Vec<u8>,
    /// Message metadata key-value pairs
    pub metadata: std::collections::HashMap<String, String>,
}

/// Processing result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessingResult {
    /// Processing succeeded
    Success,
    /// Processing failed with error
    Error(String),
    /// Processing should be retried
    Retry(String),
}

/// Side effects that need to be journaled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SideEffect {
    /// External service call
    ExternalCall {
        /// Service being called
        service: String,
        /// Method being invoked
        method: String,
        /// Request payload
        request: Vec<u8>,
        /// Response payload (if completed)
        response: Option<Vec<u8>>,
    },
    /// Timer scheduled
    TimerScheduled {
        /// Timer name
        name: String,
        /// Duration in milliseconds
        duration_ms: u64,
    },
    /// Timer fired
    TimerFired {
        /// Timer name
        name: String,
    },
    /// Sleep/delay
    Sleep {
        /// Sleep duration in milliseconds
        duration_ms: u64,
    },
    /// Random number generated (for deterministic replay)
    RandomGenerated {
        /// Random bytes generated
        value: Vec<u8>,
    },
    /// Current time accessed (for deterministic replay)
    TimeAccessed {
        /// Timestamp when accessed
        timestamp: DateTime<Utc>,
    },
}

/// Promise metadata (Restate-inspired durable promises)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromiseMetadata {
    /// ID of the actor that created the promise
    pub creator_id: String,
    /// Optional timeout in milliseconds
    pub timeout_ms: Option<u64>,
    /// Optional idempotency key for deduplication
    pub idempotency_key: Option<String>,
}

/// Promise result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PromiseResult {
    /// Promise fulfilled with value
    Fulfilled(Vec<u8>),
    /// Promise rejected with error
    Rejected(String),
    /// Promise timed out
    Timeout,
}

/// Compression type for snapshot state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionType {
    /// No compression
    None,
    /// Snappy compression (fast, ~2x compression)
    Snappy,
    /// Zstd compression (balanced, ~3-5x compression)
    Zstd,
}

#[allow(clippy::derivable_impls)]
impl Default for CompressionType {
    fn default() -> Self {
        CompressionType::None
    }
}

/// Encryption type for snapshot state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncryptionType {
    /// No encryption
    None,
    /// AES-256-GCM authenticated encryption
    Aes256Gcm,
}

#[allow(clippy::derivable_impls)]
impl Default for EncryptionType {
    fn default() -> Self {
        EncryptionType::None
    }
}

/// State snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// Actor whose state is snapshotted
    pub actor_id: String,
    /// Sequence number of last applied journal entry
    pub sequence: u64,
    /// When the snapshot was taken
    pub timestamp: DateTime<Utc>,
    /// Serialized actor state (may be compressed/encrypted)
    pub state_data: Vec<u8>,
    /// Compression type applied to state_data
    #[serde(default)]
    pub compression: CompressionType,
    /// Encryption type applied to state_data
    #[serde(default)]
    pub encryption: EncryptionType,
    /// Additional snapshot metadata
    pub metadata: std::collections::HashMap<String, String>,
}

/// Journal errors
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// I/O error during journal operations
    #[error("IO error: {0}")]
    IoError(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Journal sequence number mismatch
    #[error("Sequence mismatch: expected {expected}, got {actual}")]
    SequenceMismatch {
        /// Expected sequence number
        expected: u64,
        /// Actual sequence number
        actual: u64,
    },

    /// Journal entry not found
    #[error("Entry not found")]
    NotFound,

    /// Journal data is corrupted
    #[error("Corrupted journal: {0}")]
    Corrupted(String),
}

impl From<std::io::Error> for JournalError {
    fn from(error: std::io::Error) -> Self {
        JournalError::IoError(error.to_string())
    }
}

/// Checkpoint retention configuration
#[derive(Debug, Clone)]
pub struct RetentionConfig {
    /// Number of old snapshots to retain (0 = keep all)
    pub retention_count: u32,
    /// Automatically truncate journal entries after snapshot
    pub auto_truncate: bool,
}

#[allow(clippy::derivable_impls)]
impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            retention_count: 0, // Keep all by default
            auto_truncate: false,
        }
    }
}

/// In-memory journal implementation for testing
pub struct MemoryJournal {
    entries: Arc<tokio::sync::RwLock<Vec<JournalEntry>>>,
    snapshots: Arc<tokio::sync::RwLock<Vec<Snapshot>>>,
    next_sequence: Arc<tokio::sync::RwLock<u64>>,
    retention_config: Arc<tokio::sync::RwLock<RetentionConfig>>,
}

impl Default for MemoryJournal {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryJournal {
    /// Creates a new empty in-memory journal with default retention config
    pub fn new() -> Self {
        Self::with_retention(RetentionConfig::default())
    }

    /// Creates a new in-memory journal with custom retention configuration
    pub fn with_retention(retention_config: RetentionConfig) -> Self {
        MemoryJournal {
            entries: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            snapshots: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            next_sequence: Arc::new(tokio::sync::RwLock::new(1)),
            retention_config: Arc::new(tokio::sync::RwLock::new(retention_config)),
        }
    }

    /// Creates a new in-memory journal from a full JournalConfig
    ///
    /// ## Example
    /// ```
    /// use plexspaces_persistence::*;
    /// use plexspaces_persistence::config::*;
    ///
    /// # async fn example() {
    /// // Use production config
    /// let config = JournalConfig::production();
    /// let journal = MemoryJournal::with_config(&config);
    ///
    /// // Or custom config
    /// let config = JournalConfig::builder()
    ///     .compression(CompressionType::Zstd)
    ///     .retention_count(5)
    ///     .build();
    /// let journal = MemoryJournal::with_config(&config);
    /// # }
    /// ```
    ///
    /// ## Note
    /// Currently only retention settings are used.
    /// Future: compression, encryption, batching will be used by backend implementations.
    pub fn with_config(config: &crate::config::JournalConfig) -> Self {
        let retention_config = RetentionConfig {
            retention_count: config.retention.retention_count,
            auto_truncate: config.snapshot.auto_truncate,
        };
        Self::with_retention(retention_config)
    }

    /// Lists all snapshots for an actor (sorted by sequence, descending)
    pub async fn list_snapshots(&self, actor_id: &ActorId) -> Result<Vec<Snapshot>, JournalError> {
        let snapshots = self.snapshots.read().await;
        let mut actor_snapshots: Vec<Snapshot> = snapshots
            .iter()
            .filter(|s| s.actor_id == actor_id.as_str())
            .cloned()
            .collect();

        // Sort by sequence descending (newest first)
        actor_snapshots.sort_by(|a, b| b.sequence.cmp(&a.sequence));

        Ok(actor_snapshots)
    }

    /// Enforces snapshot retention policy for an actor
    ///
    /// ## Purpose
    /// Deletes old snapshots beyond retention_count to prevent unbounded storage growth.
    ///
    /// ## Retention Rules
    /// - Keep last N snapshots (configured by retention_count)
    /// - Always keep at least 1 snapshot (safety)
    /// - Delete oldest snapshots first
    ///
    /// ## Returns
    /// Number of snapshots deleted
    pub async fn enforce_retention(&self, actor_id: &ActorId) -> Result<u64, JournalError> {
        let retention_count = self.retention_config.read().await.retention_count;

        if retention_count == 0 {
            return Ok(0); // Keep all snapshots
        }

        // List all snapshots for actor (sorted by sequence, newest first)
        let all_snapshots = self.list_snapshots(actor_id).await?;

        // Keep last N snapshots
        let to_keep = retention_count as usize;
        if all_snapshots.len() <= to_keep {
            return Ok(0); // Nothing to delete
        }

        // Identify snapshots to delete (oldest ones)
        let to_delete = &all_snapshots[to_keep..];
        let sequences_to_delete: Vec<u64> = to_delete.iter().map(|s| s.sequence).collect();

        // Delete old snapshots
        let mut snapshots = self.snapshots.write().await;
        snapshots.retain(|s| {
            s.actor_id != actor_id.as_str() || !sequences_to_delete.contains(&s.sequence)
        });

        Ok(sequences_to_delete.len() as u64)
    }

    /// Creates a snapshot with automatic retention enforcement
    ///
    /// ## Purpose
    /// Creates snapshot and optionally enforces retention policy and truncates journal.
    ///
    /// ## Workflow
    /// 1. Save snapshot
    /// 2. If auto_truncate enabled, delete old journal entries
    /// 3. If retention_count > 0, delete old snapshots
    ///
    /// ## Returns
    /// The created snapshot
    pub async fn save_snapshot_with_retention(
        &self,
        snapshot: Snapshot,
    ) -> Result<Snapshot, JournalError> {
        let actor_id = snapshot.actor_id.clone();
        let sequence = snapshot.sequence;

        // Save snapshot
        self.save_snapshot(snapshot.clone()).await?;

        // Get retention config
        let config = self.retention_config.read().await.clone();

        // Truncate journal if configured
        if config.auto_truncate {
            self.truncate_to(&actor_id, sequence).await?;
        }

        // Enforce retention policy if configured
        if config.retention_count > 0 {
            self.enforce_retention(&actor_id).await?;
        }

        Ok(snapshot)
    }

    async fn next_sequence(&self) -> u64 {
        let mut seq = self.next_sequence.write().await;
        let current = *seq;
        *seq += 1;
        current
    }
}

#[async_trait]
impl Journal for MemoryJournal {
    async fn record_message_received(&self, message: &Message) -> Result<(), JournalError> {
        let sequence = self.next_sequence().await;
        let entry = JournalEntry::MessageReceived {
            sequence,
            timestamp: Utc::now(),
            message: MessageRecord {
                id: message.id().to_string(),
                payload: message.payload().to_vec(),
                metadata: Default::default(),
            },
        };
        self.entries.write().await.push(entry);
        Ok(())
    }

    async fn record_message_processed(
        &self,
        message: &Message,
        result: &Result<(), String>, // Error as String instead of BehaviorError
    ) -> Result<(), JournalError> {
        let sequence = self.next_sequence().await;
        let processing_result = match result {
            Ok(()) => ProcessingResult::Success,
            Err(e) => ProcessingResult::Error(e.clone()), // Already a String
        };
        let entry = JournalEntry::MessageProcessed {
            sequence,
            timestamp: Utc::now(),
            message_id: message.id().to_string(),
            result: processing_result,
        };
        self.entries.write().await.push(entry);
        Ok(())
    }

    async fn record_state_change(
        &self,
        _actor_id: &ActorId,
        old_state: Vec<u8>,
        new_state: Vec<u8>,
    ) -> Result<(), JournalError> {
        let sequence = self.next_sequence().await;
        let old_hash = format!("{:x}", md5::compute(&old_state));
        let new_hash = format!("{:x}", md5::compute(&new_state));
        let entry = JournalEntry::StateChanged {
            sequence,
            timestamp: Utc::now(),
            old_hash,
            new_hash,
            state_data: new_state,
        };
        self.entries.write().await.push(entry);
        Ok(())
    }

    async fn record_side_effect(
        &self,
        _actor_id: &ActorId,
        effect: SideEffect,
    ) -> Result<(), JournalError> {
        let sequence = self.next_sequence().await;
        let entry = JournalEntry::SideEffectRecorded {
            sequence,
            timestamp: Utc::now(),
            effect,
        };
        self.entries.write().await.push(entry);
        Ok(())
    }

    async fn record_promise_created(
        &self,
        promise_id: &str,
        metadata: PromiseMetadata,
    ) -> Result<(), JournalError> {
        let sequence = self.next_sequence().await;
        let entry = JournalEntry::PromiseCreated {
            sequence,
            timestamp: Utc::now(),
            promise_id: promise_id.to_string(),
            metadata,
        };
        self.entries.write().await.push(entry);
        Ok(())
    }

    async fn record_promise_resolved(
        &self,
        promise_id: &str,
        result: PromiseResult,
    ) -> Result<(), JournalError> {
        let sequence = self.next_sequence().await;
        let entry = JournalEntry::PromiseResolved {
            sequence,
            timestamp: Utc::now(),
            promise_id: promise_id.to_string(),
            result,
        };
        self.entries.write().await.push(entry);
        Ok(())
    }

    async fn replay_from(
        &self,
        _actor_id: &ActorId,
        from_sequence: u64,
    ) -> Result<Vec<JournalEntry>, JournalError> {
        let entries = self.entries.read().await;
        Ok(entries
            .iter()
            .filter(|e| match e {
                JournalEntry::MessageReceived { sequence, .. }
                | JournalEntry::MessageProcessed { sequence, .. }
                | JournalEntry::StateChanged { sequence, .. }
                | JournalEntry::SideEffectRecorded { sequence, .. }
                | JournalEntry::PromiseCreated { sequence, .. }
                | JournalEntry::PromiseResolved { sequence, .. }
                | JournalEntry::Lifecycle { sequence, .. }
                | JournalEntry::SnapshotTaken { sequence, .. } => *sequence >= from_sequence,
            })
            .cloned()
            .collect())
    }

    async fn get_latest_snapshot(
        &self,
        actor_id: &ActorId,
    ) -> Result<Option<Snapshot>, JournalError> {
        let snapshots = self.snapshots.read().await;
        Ok(snapshots
            .iter()
            .filter(|s| s.actor_id == actor_id.as_str())
            .max_by_key(|s| s.sequence)
            .cloned())
    }

    async fn save_snapshot(&self, snapshot: Snapshot) -> Result<(), JournalError> {
        self.snapshots.write().await.push(snapshot);
        Ok(())
    }

    async fn truncate_to(&self, _actor_id: &ActorId, sequence: u64) -> Result<(), JournalError> {
        let mut entries = self.entries.write().await;
        entries.retain(|e| match e {
            JournalEntry::MessageReceived { sequence: s, .. }
            | JournalEntry::MessageProcessed { sequence: s, .. }
            | JournalEntry::StateChanged { sequence: s, .. }
            | JournalEntry::SideEffectRecorded { sequence: s, .. }
            | JournalEntry::PromiseCreated { sequence: s, .. }
            | JournalEntry::PromiseResolved { sequence: s, .. }
            | JournalEntry::Lifecycle { sequence: s, .. }
            | JournalEntry::SnapshotTaken { sequence: s, .. } => *s > sequence,
        });
        Ok(())
    }

    async fn append(&self, entry: JournalEntry) -> Result<(), JournalError> {
        self.entries.write().await.push(entry);
        Ok(())
    }

    async fn append_batch(&self, entries: Vec<JournalEntry>) -> Result<(), JournalError> {
        if entries.is_empty() {
            return Ok(());
        }

        // Acquire write lock once for the entire batch (reduces lock contention)
        let mut journal_entries = self.entries.write().await;
        journal_entries.extend(entries);
        Ok(())
    }

    async fn get_entries(&self) -> Result<Vec<JournalEntry>, JournalError> {
        Ok(self.entries.read().await.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_journal() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();

        // Test recording and replay
        let message = Message::new(b"test".to_vec());
        journal.record_message_received(&message).await.unwrap();

        let entries = journal.replay_from(&actor_id, 1).await.unwrap();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            JournalEntry::MessageReceived { message: msg, .. } => {
                assert_eq!(msg.payload, b"test");
            }
            _ => panic!("Wrong entry type"),
        }
    }

    #[tokio::test]
    async fn test_snapshot() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();

        let snapshot = Snapshot {
            actor_id: actor_id.as_str().to_string(),
            sequence: 10,
            timestamp: Utc::now(),
            state_data: b"state".to_vec(),
            metadata: Default::default(),
            compression: CompressionType::None,
            encryption: EncryptionType::None,
        };

        journal.save_snapshot(snapshot.clone()).await.unwrap();
        let loaded = journal.get_latest_snapshot(&actor_id).await.unwrap();

        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().sequence, 10);
    }

    /// Test record_message_processed with all ProcessingResult variants
    #[tokio::test]
    async fn test_record_message_processed() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();
        let message = Message::new(b"test".to_vec());

        // Test Success result
        journal
            .record_message_processed(&message, &Ok(()))
            .await
            .unwrap();

        // Test Error result
        journal
            .record_message_processed(&message, &Err("error occurred".to_string()))
            .await
            .unwrap();

        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 2);

        // Verify Success entry
        match &entries[0] {
            JournalEntry::MessageProcessed { result, .. } => {
                assert!(matches!(result, ProcessingResult::Success));
            }
            _ => panic!("Expected MessageProcessed entry"),
        }

        // Verify Error entry
        match &entries[1] {
            JournalEntry::MessageProcessed {
                result, message_id, ..
            } => {
                match result {
                    ProcessingResult::Error(err) => assert_eq!(err, "error occurred"),
                    _ => panic!("Expected Error result"),
                }
                assert_eq!(message_id, message.id());
            }
            _ => panic!("Expected MessageProcessed entry"),
        }
    }

    /// Test record_state_change with MD5 hashing
    #[tokio::test]
    async fn test_record_state_change() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();

        let old_state = b"old state".to_vec();
        let new_state = b"new state".to_vec();

        journal
            .record_state_change(&actor_id, old_state.clone(), new_state.clone())
            .await
            .unwrap();

        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            JournalEntry::StateChanged {
                old_hash,
                new_hash,
                state_data,
                ..
            } => {
                // Verify MD5 hashes
                let expected_old_hash = format!("{:x}", md5::compute(&old_state));
                let expected_new_hash = format!("{:x}", md5::compute(&new_state));

                assert_eq!(old_hash, &expected_old_hash);
                assert_eq!(new_hash, &expected_new_hash);
                assert_eq!(state_data, &new_state);
            }
            _ => panic!("Expected StateChanged entry"),
        }
    }

    /// Test record_side_effect with all SideEffect variants
    #[tokio::test]
    async fn test_record_side_effect() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();

        // Test ExternalCall
        journal
            .record_side_effect(
                &actor_id,
                SideEffect::ExternalCall {
                    service: "auth-service".to_string(),
                    method: "validate".to_string(),
                    request: b"request".to_vec(),
                    response: Some(b"response".to_vec()),
                },
            )
            .await
            .unwrap();

        // Test TimerScheduled
        journal
            .record_side_effect(
                &actor_id,
                SideEffect::TimerScheduled {
                    name: "reminder".to_string(),
                    duration_ms: 5000,
                },
            )
            .await
            .unwrap();

        // Test TimerFired
        journal
            .record_side_effect(
                &actor_id,
                SideEffect::TimerFired {
                    name: "reminder".to_string(),
                },
            )
            .await
            .unwrap();

        // Test Sleep
        journal
            .record_side_effect(&actor_id, SideEffect::Sleep { duration_ms: 1000 })
            .await
            .unwrap();

        // Test RandomGenerated
        journal
            .record_side_effect(
                &actor_id,
                SideEffect::RandomGenerated {
                    value: vec![1, 2, 3, 4],
                },
            )
            .await
            .unwrap();

        // Test TimeAccessed
        journal
            .record_side_effect(
                &actor_id,
                SideEffect::TimeAccessed {
                    timestamp: Utc::now(),
                },
            )
            .await
            .unwrap();

        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 6);

        // Verify all are SideEffectRecorded entries
        for entry in &entries {
            assert!(matches!(entry, JournalEntry::SideEffectRecorded { .. }));
        }

        // Verify specific side effects
        match &entries[0] {
            JournalEntry::SideEffectRecorded { effect, .. } => match effect {
                SideEffect::ExternalCall {
                    service, method, ..
                } => {
                    assert_eq!(service, "auth-service");
                    assert_eq!(method, "validate");
                }
                _ => panic!("Expected ExternalCall"),
            },
            _ => panic!("Expected SideEffectRecorded"),
        }
    }

    /// Test record_promise_created with metadata
    #[tokio::test]
    async fn test_record_promise_created() {
        let journal = MemoryJournal::new();

        // Test with timeout
        let metadata_with_timeout = PromiseMetadata {
            creator_id: "actor-1".to_string(),
            timeout_ms: Some(5000),
            idempotency_key: None,
        };
        journal
            .record_promise_created("promise-1", metadata_with_timeout)
            .await
            .unwrap();

        // Test with idempotency key
        let metadata_with_key = PromiseMetadata {
            creator_id: "actor-2".to_string(),
            timeout_ms: None,
            idempotency_key: Some("idempotent-key".to_string()),
        };
        journal
            .record_promise_created("promise-2", metadata_with_key)
            .await
            .unwrap();

        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 2);

        // Verify first promise with timeout
        match &entries[0] {
            JournalEntry::PromiseCreated {
                promise_id,
                metadata,
                ..
            } => {
                assert_eq!(promise_id, "promise-1");
                assert_eq!(metadata.creator_id, "actor-1");
                assert_eq!(metadata.timeout_ms, Some(5000));
                assert_eq!(metadata.idempotency_key, None);
            }
            _ => panic!("Expected PromiseCreated entry"),
        }

        // Verify second promise with idempotency key
        match &entries[1] {
            JournalEntry::PromiseCreated {
                promise_id,
                metadata,
                ..
            } => {
                assert_eq!(promise_id, "promise-2");
                assert_eq!(metadata.creator_id, "actor-2");
                assert_eq!(metadata.timeout_ms, None);
                assert_eq!(metadata.idempotency_key, Some("idempotent-key".to_string()));
            }
            _ => panic!("Expected PromiseCreated entry"),
        }
    }

    /// Test record_promise_resolved with all PromiseResult variants
    #[tokio::test]
    async fn test_record_promise_resolved() {
        let journal = MemoryJournal::new();

        // Test Fulfilled result
        journal
            .record_promise_resolved(
                "promise-1",
                PromiseResult::Fulfilled(b"success value".to_vec()),
            )
            .await
            .unwrap();

        // Test Rejected result
        journal
            .record_promise_resolved(
                "promise-2",
                PromiseResult::Rejected("error message".to_string()),
            )
            .await
            .unwrap();

        // Test Timeout result
        journal
            .record_promise_resolved("promise-3", PromiseResult::Timeout)
            .await
            .unwrap();

        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 3);

        // Verify Fulfilled
        match &entries[0] {
            JournalEntry::PromiseResolved {
                promise_id, result, ..
            } => {
                assert_eq!(promise_id, "promise-1");
                match result {
                    PromiseResult::Fulfilled(value) => assert_eq!(value, b"success value"),
                    _ => panic!("Expected Fulfilled result"),
                }
            }
            _ => panic!("Expected PromiseResolved entry"),
        }

        // Verify Rejected
        match &entries[1] {
            JournalEntry::PromiseResolved {
                promise_id, result, ..
            } => {
                assert_eq!(promise_id, "promise-2");
                match result {
                    PromiseResult::Rejected(err) => assert_eq!(err, "error message"),
                    _ => panic!("Expected Rejected result"),
                }
            }
            _ => panic!("Expected PromiseResolved entry"),
        }

        // Verify Timeout
        match &entries[2] {
            JournalEntry::PromiseResolved {
                promise_id, result, ..
            } => {
                assert_eq!(promise_id, "promise-3");
                assert!(matches!(result, PromiseResult::Timeout));
            }
            _ => panic!("Expected PromiseResolved entry"),
        }
    }

    /// Test truncate_to filtering logic
    #[tokio::test]
    async fn test_truncate_to() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();
        let message = Message::new(b"test".to_vec());

        // Add 10 entries
        for _ in 0..10 {
            journal.record_message_received(&message).await.unwrap();
        }

        let entries_before = journal.get_entries().await.unwrap();
        assert_eq!(entries_before.len(), 10);

        // Truncate to sequence 5 (should keep entries > 5, remove entries <= 5)
        journal.truncate_to(&actor_id, 5).await.unwrap();

        let entries_after = journal.get_entries().await.unwrap();
        assert_eq!(entries_after.len(), 5);

        // Verify entries 6-10 remain
        for (i, entry) in entries_after.iter().enumerate() {
            match entry {
                JournalEntry::MessageReceived { sequence, .. } => {
                    assert_eq!(*sequence, (i + 6) as u64);
                }
                _ => panic!("Expected MessageReceived entry"),
            }
        }

        // Truncate to 0 should keep all remaining entries
        journal.truncate_to(&actor_id, 0).await.unwrap();
        let entries_final = journal.get_entries().await.unwrap();
        assert_eq!(entries_final.len(), 5);
    }

    /// Test append and get_entries
    #[tokio::test]
    async fn test_append_and_get_entries() {
        let journal = MemoryJournal::new();

        // Append entries directly
        journal
            .append(JournalEntry::MessageReceived {
                sequence: 1,
                timestamp: Utc::now(),
                message: MessageRecord {
                    id: "msg-1".to_string(),
                    payload: b"payload-1".to_vec(),
                    metadata: Default::default(),
                },
            })
            .await
            .unwrap();

        journal
            .append(JournalEntry::StateChanged {
                sequence: 2,
                timestamp: Utc::now(),
                old_hash: "hash1".to_string(),
                new_hash: "hash2".to_string(),
                state_data: b"state".to_vec(),
            })
            .await
            .unwrap();

        journal
            .append(JournalEntry::SnapshotTaken {
                sequence: 3,
                timestamp: Utc::now(),
                snapshot_id: "snap-1".to_string(),
            })
            .await
            .unwrap();

        // Get all entries
        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 3);

        // Verify ordering
        match &entries[0] {
            JournalEntry::MessageReceived { sequence, .. } => assert_eq!(*sequence, 1),
            _ => panic!("Expected MessageReceived"),
        }
        match &entries[1] {
            JournalEntry::StateChanged { sequence, .. } => assert_eq!(*sequence, 2),
            _ => panic!("Expected StateChanged"),
        }
        match &entries[2] {
            JournalEntry::SnapshotTaken { sequence, .. } => assert_eq!(*sequence, 3),
            _ => panic!("Expected SnapshotTaken"),
        }
    }

    /// Test replay_from with all entry types
    #[tokio::test]
    async fn test_replay_from_advanced() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();
        let message = Message::new(b"test".to_vec());

        // Add one of each entry type
        journal.record_message_received(&message).await.unwrap(); // seq 1
        journal
            .record_message_processed(&message, &Ok(()))
            .await
            .unwrap(); // seq 2
        journal
            .record_state_change(&actor_id, b"old".to_vec(), b"new".to_vec())
            .await
            .unwrap(); // seq 3
        journal
            .record_side_effect(&actor_id, SideEffect::Sleep { duration_ms: 100 })
            .await
            .unwrap(); // seq 4
        journal
            .record_promise_created(
                "p1",
                PromiseMetadata {
                    creator_id: "a1".to_string(),
                    timeout_ms: None,
                    idempotency_key: None,
                },
            )
            .await
            .unwrap(); // seq 5
        journal
            .record_promise_resolved("p1", PromiseResult::Fulfilled(b"result".to_vec()))
            .await
            .unwrap(); // seq 6
        journal
            .append(JournalEntry::Lifecycle {
                sequence: 7,
                timestamp: Utc::now(),
                actor_id: actor_id.clone(),
                event: "activated".to_string(),
            })
            .await
            .unwrap();
        journal
            .append(JournalEntry::SnapshotTaken {
                sequence: 8,
                timestamp: Utc::now(),
                snapshot_id: "snap-1".to_string(),
            })
            .await
            .unwrap();

        // Replay from sequence 1 (should get all 8 entries)
        let all_entries = journal.replay_from(&actor_id, 1).await.unwrap();
        assert_eq!(all_entries.len(), 8);

        // Replay from sequence 5 (should get entries 5-8)
        let partial_entries = journal.replay_from(&actor_id, 5).await.unwrap();
        assert_eq!(partial_entries.len(), 4);

        // Verify correct entries returned
        match &partial_entries[0] {
            JournalEntry::PromiseCreated { sequence, .. } => assert_eq!(*sequence, 5),
            _ => panic!("Expected PromiseCreated"),
        }
        match &partial_entries[1] {
            JournalEntry::PromiseResolved { sequence, .. } => assert_eq!(*sequence, 6),
            _ => panic!("Expected PromiseResolved"),
        }
        match &partial_entries[2] {
            JournalEntry::Lifecycle { sequence, .. } => assert_eq!(*sequence, 7),
            _ => panic!("Expected Lifecycle"),
        }
        match &partial_entries[3] {
            JournalEntry::SnapshotTaken { sequence, .. } => assert_eq!(*sequence, 8),
            _ => panic!("Expected SnapshotTaken"),
        }

        // Replay from sequence 100 (should get no entries)
        let no_entries = journal.replay_from(&actor_id, 100).await.unwrap();
        assert_eq!(no_entries.len(), 0);
    }

    /// Test get_latest_snapshot returns most recent when multiple snapshots exist
    #[tokio::test]
    async fn test_multiple_snapshots_returns_latest() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();

        // Create 3 snapshots with increasing sequence numbers
        let snapshot1 = Snapshot {
            actor_id: actor_id.clone(),
            sequence: 10,
            timestamp: Utc::now(),
            state_data: b"state-1".to_vec(),
            metadata: Default::default(),
            compression: CompressionType::None,
            encryption: EncryptionType::None,
        };

        let snapshot2 = Snapshot {
            actor_id: actor_id.clone(),
            sequence: 20,
            timestamp: Utc::now(),
            state_data: b"state-2".to_vec(),
            metadata: Default::default(),
            compression: CompressionType::None,
            encryption: EncryptionType::None,
        };

        let snapshot3 = Snapshot {
            actor_id: actor_id.clone(),
            sequence: 30,
            timestamp: Utc::now(),
            state_data: b"state-3".to_vec(),
            metadata: Default::default(),
            compression: CompressionType::None,
            encryption: EncryptionType::None,
        };

        // Save all 3 snapshots
        journal.save_snapshot(snapshot1).await.unwrap();
        journal.save_snapshot(snapshot2).await.unwrap();
        journal.save_snapshot(snapshot3).await.unwrap();

        // get_latest_snapshot should return the one with highest sequence (30)
        let latest = journal.get_latest_snapshot(&actor_id).await.unwrap();
        assert!(latest.is_some());
        let snapshot = latest.unwrap();
        assert_eq!(snapshot.sequence, 30);
        assert_eq!(snapshot.state_data, b"state-3");
    }

    /// Test Lifecycle entry recording
    #[tokio::test]
    async fn test_lifecycle_entry() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();

        // Append Lifecycle entry directly (simulating activation)
        journal
            .append(JournalEntry::Lifecycle {
                sequence: 1,
                timestamp: Utc::now(),
                actor_id: actor_id.clone(),
                event: "activated".to_string(),
            })
            .await
            .unwrap();

        // Append deactivation event
        journal
            .append(JournalEntry::Lifecycle {
                sequence: 2,
                timestamp: Utc::now(),
                actor_id: actor_id.clone(),
                event: "deactivated".to_string(),
            })
            .await
            .unwrap();

        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 2);

        // Verify first lifecycle event
        match &entries[0] {
            JournalEntry::Lifecycle {
                sequence,
                actor_id: id,
                event,
                ..
            } => {
                assert_eq!(*sequence, 1);
                assert_eq!(id, &actor_id);
                assert_eq!(event, "activated");
            }
            _ => panic!("Expected Lifecycle entry"),
        }

        // Verify second lifecycle event
        match &entries[1] {
            JournalEntry::Lifecycle {
                sequence, event, ..
            } => {
                assert_eq!(*sequence, 2);
                assert_eq!(event, "deactivated");
            }
            _ => panic!("Expected Lifecycle entry"),
        }
    }

    /// Test SnapshotTaken entry recording
    #[tokio::test]
    async fn test_snapshot_taken_entry() {
        let journal = MemoryJournal::new();

        // Append SnapshotTaken entry
        journal
            .append(JournalEntry::SnapshotTaken {
                sequence: 1,
                timestamp: Utc::now(),
                snapshot_id: "snapshot-12345".to_string(),
            })
            .await
            .unwrap();

        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            JournalEntry::SnapshotTaken {
                sequence,
                snapshot_id,
                ..
            } => {
                assert_eq!(*sequence, 1);
                assert_eq!(snapshot_id, "snapshot-12345");
            }
            _ => panic!("Expected SnapshotTaken entry"),
        }
    }

    /// Test record_message_processed with Retry result variant
    #[tokio::test]
    async fn test_record_message_processed_retry() {
        let journal = MemoryJournal::new();
        let message = Message::new(b"test".to_vec());

        // Note: Current implementation doesn't support Retry variant directly
        // This test documents expected behavior for future implementation

        // For now, test that Error variant can represent retry scenarios
        journal
            .record_message_processed(&message, &Err("retry: temporary failure".to_string()))
            .await
            .unwrap();

        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 1);

        match &entries[0] {
            JournalEntry::MessageProcessed { result, .. } => match result {
                ProcessingResult::Error(err) => {
                    assert!(err.starts_with("retry:"));
                }
                _ => panic!("Expected Error result with retry marker"),
            },
            _ => panic!("Expected MessageProcessed entry"),
        }
    }

    /// Integration test: Full actor lifecycle journaling
    #[tokio::test]
    async fn test_full_actor_lifecycle_journaling() {
        let journal = MemoryJournal::new();
        let actor_id = "lifecycle-actor".to_string();
        let message = Message::new(b"test message".to_vec());

        // 1. Actor activation (Lifecycle entry)
        journal
            .append(JournalEntry::Lifecycle {
                sequence: 1,
                timestamp: Utc::now(),
                actor_id: actor_id.clone(),
                event: "activated".to_string(),
            })
            .await
            .unwrap();

        // 2. Receive message
        journal.record_message_received(&message).await.unwrap();

        // 3. Process message successfully
        journal
            .record_message_processed(&message, &Ok(()))
            .await
            .unwrap();

        // 4. State change after processing
        journal
            .record_state_change(&actor_id, b"initial".to_vec(), b"updated".to_vec())
            .await
            .unwrap();

        // 5. Take snapshot at sequence 4
        let snapshot = Snapshot {
            actor_id: actor_id.clone(),
            sequence: 4,
            timestamp: Utc::now(),
            state_data: b"updated".to_vec(),
            metadata: Default::default(),
            compression: CompressionType::None,
            encryption: EncryptionType::None,
        };
        journal.save_snapshot(snapshot).await.unwrap();

        // 6. Record snapshot taken
        journal
            .append(JournalEntry::SnapshotTaken {
                sequence: 5,
                timestamp: Utc::now(),
                snapshot_id: "snap-1".to_string(),
            })
            .await
            .unwrap();

        // 7. Actor deactivation
        journal
            .append(JournalEntry::Lifecycle {
                sequence: 6,
                timestamp: Utc::now(),
                actor_id: actor_id.clone(),
                event: "deactivated".to_string(),
            })
            .await
            .unwrap();

        // Verify complete lifecycle
        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 6);

        // Verify sequence
        assert!(matches!(entries[0], JournalEntry::Lifecycle { .. }));
        assert!(matches!(entries[1], JournalEntry::MessageReceived { .. }));
        assert!(matches!(entries[2], JournalEntry::MessageProcessed { .. }));
        assert!(matches!(entries[3], JournalEntry::StateChanged { .. }));
        assert!(matches!(entries[4], JournalEntry::SnapshotTaken { .. }));
        assert!(matches!(entries[5], JournalEntry::Lifecycle { .. }));

        // Verify snapshot was saved
        let latest_snapshot = journal.get_latest_snapshot(&actor_id).await.unwrap();
        assert!(latest_snapshot.is_some());
        assert_eq!(latest_snapshot.unwrap().sequence, 4);
    }

    /// Integration test: Promise workflow (creation → resolution)
    #[tokio::test]
    async fn test_promise_workflow_integration() {
        let journal = MemoryJournal::new();
        let promise_id = "promise-123";

        // 1. Create promise
        let metadata = PromiseMetadata {
            creator_id: "actor-1".to_string(),
            timeout_ms: Some(5000),
            idempotency_key: Some("idempotent-1".to_string()),
        };
        journal
            .record_promise_created(promise_id, metadata)
            .await
            .unwrap();

        // 2. Simulate some work (other journal entries)
        let message = Message::new(b"work".to_vec());
        journal.record_message_received(&message).await.unwrap();
        journal
            .record_message_processed(&message, &Ok(()))
            .await
            .unwrap();

        // 3. Resolve promise
        journal
            .record_promise_resolved(promise_id, PromiseResult::Fulfilled(b"result".to_vec()))
            .await
            .unwrap();

        // Verify workflow
        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 4);

        // First entry: PromiseCreated
        match &entries[0] {
            JournalEntry::PromiseCreated {
                promise_id: id,
                metadata,
                ..
            } => {
                assert_eq!(id, "promise-123");
                assert_eq!(metadata.creator_id, "actor-1");
                assert_eq!(metadata.timeout_ms, Some(5000));
                assert_eq!(metadata.idempotency_key, Some("idempotent-1".to_string()));
            }
            _ => panic!("Expected PromiseCreated"),
        }

        // Last entry: PromiseResolved
        match &entries[3] {
            JournalEntry::PromiseResolved {
                promise_id: id,
                result,
                ..
            } => {
                assert_eq!(id, "promise-123");
                match result {
                    PromiseResult::Fulfilled(value) => assert_eq!(value, b"result"),
                    _ => panic!("Expected Fulfilled result"),
                }
            }
            _ => panic!("Expected PromiseResolved"),
        }
    }

    /// Integration test: Snapshot + truncation workflow
    #[tokio::test]
    async fn test_snapshot_truncation_workflow() {
        let journal = MemoryJournal::new();
        let actor_id = "snapshot-actor".to_string();
        let message = Message::new(b"test".to_vec());

        // 1. Generate 100 journal entries
        for _ in 0..100 {
            journal.record_message_received(&message).await.unwrap();
        }

        let entries_before = journal.get_entries().await.unwrap();
        assert_eq!(entries_before.len(), 100);

        // 2. Take snapshot at sequence 50
        let snapshot = Snapshot {
            actor_id: actor_id.clone(),
            sequence: 50,
            timestamp: Utc::now(),
            state_data: b"state-at-50".to_vec(),
            metadata: Default::default(),
            compression: CompressionType::None,
            encryption: EncryptionType::None,
        };
        journal.save_snapshot(snapshot).await.unwrap();

        // 3. Truncate journal up to sequence 50 (remove entries <= 50)
        journal.truncate_to(&actor_id, 50).await.unwrap();

        // 4. Verify only entries > 50 remain
        let entries_after = journal.get_entries().await.unwrap();
        assert_eq!(entries_after.len(), 50); // Entries 51-100

        // 5. Verify snapshot is still available
        let latest_snapshot = journal.get_latest_snapshot(&actor_id).await.unwrap();
        assert!(latest_snapshot.is_some());
        let snapshot = latest_snapshot.unwrap();
        assert_eq!(snapshot.sequence, 50);
        assert_eq!(snapshot.state_data, b"state-at-50");

        // 6. Replay from snapshot onwards should get entries 51-100
        let replay_entries = journal.replay_from(&actor_id, 51).await.unwrap();
        assert_eq!(replay_entries.len(), 50);
    }

    /// Test snapshot metadata preservation
    #[tokio::test]
    async fn test_snapshot_metadata_preservation() {
        let journal = MemoryJournal::new();
        let actor_id = "metadata-actor".to_string();

        // Create snapshot with custom metadata
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("version".to_string(), "1.2.3".to_string());
        metadata.insert("checkpoint".to_string(), "after_migration".to_string());
        metadata.insert("node_id".to_string(), "node-5".to_string());

        let snapshot = Snapshot {
            actor_id: actor_id.clone(),
            sequence: 42,
            timestamp: Utc::now(),
            state_data: b"state with metadata".to_vec(),
            compression: CompressionType::None,
            encryption: EncryptionType::None,
            metadata: metadata.clone(),
        };

        journal.save_snapshot(snapshot).await.unwrap();

        // Retrieve and verify metadata preserved
        let retrieved = journal.get_latest_snapshot(&actor_id).await.unwrap();
        assert!(retrieved.is_some());
        let snapshot = retrieved.unwrap();

        assert_eq!(snapshot.metadata.len(), 3);
        assert_eq!(snapshot.metadata.get("version"), Some(&"1.2.3".to_string()));
        assert_eq!(
            snapshot.metadata.get("checkpoint"),
            Some(&"after_migration".to_string())
        );
        assert_eq!(
            snapshot.metadata.get("node_id"),
            Some(&"node-5".to_string())
        );
    }

    /// Test sequence number generation is monotonically increasing
    #[tokio::test]
    async fn test_sequence_number_monotonicity() {
        let journal = MemoryJournal::new();
        let message = Message::new(b"test".to_vec());

        // Record multiple entries
        for _ in 0..10 {
            journal.record_message_received(&message).await.unwrap();
        }

        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 10);

        // Verify sequence numbers are monotonically increasing: 1, 2, 3, ..., 10
        for (i, entry) in entries.iter().enumerate() {
            match entry {
                JournalEntry::MessageReceived { sequence, .. } => {
                    assert_eq!(*sequence, (i + 1) as u64);
                }
                _ => panic!("Expected MessageReceived entry"),
            }
        }
    }

    /// Test get_latest_snapshot with no snapshots returns None
    #[tokio::test]
    async fn test_get_latest_snapshot_empty() {
        let journal = MemoryJournal::new();
        let actor_id = "no-snapshots-actor".to_string();

        let result = journal.get_latest_snapshot(&actor_id).await.unwrap();
        assert!(result.is_none());
    }

    /// Test get_latest_snapshot filters by actor_id correctly
    #[tokio::test]
    async fn test_get_latest_snapshot_actor_isolation() {
        let journal = MemoryJournal::new();

        // Create snapshots for different actors
        let snapshot1 = Snapshot {
            actor_id: "actor-1".to_string(),
            sequence: 10,
            timestamp: Utc::now(),
            state_data: b"state-1".to_vec(),
            metadata: Default::default(),
            compression: CompressionType::None,
            encryption: EncryptionType::None,
        };

        let snapshot2 = Snapshot {
            actor_id: "actor-2".to_string(),
            sequence: 20,
            timestamp: Utc::now(),
            state_data: b"state-2".to_vec(),
            metadata: Default::default(),
            compression: CompressionType::None,
            encryption: EncryptionType::None,
        };

        journal.save_snapshot(snapshot1).await.unwrap();
        journal.save_snapshot(snapshot2).await.unwrap();

        // get_latest_snapshot for actor-1 should only return actor-1's snapshot
        let result1 = journal
            .get_latest_snapshot(&"actor-1".to_string())
            .await
            .unwrap();
        assert!(result1.is_some());
        let snap1 = result1.unwrap();
        assert_eq!(snap1.actor_id, "actor-1");
        assert_eq!(snap1.sequence, 10);

        // get_latest_snapshot for actor-2 should only return actor-2's snapshot
        let result2 = journal
            .get_latest_snapshot(&"actor-2".to_string())
            .await
            .unwrap();
        assert!(result2.is_some());
        let snap2 = result2.unwrap();
        assert_eq!(snap2.actor_id, "actor-2");
        assert_eq!(snap2.sequence, 20);
    }

    /// Test replay_from with empty journal returns empty vec
    #[tokio::test]
    async fn test_replay_from_empty_journal() {
        let journal = MemoryJournal::new();
        let actor_id = "empty-actor".to_string();

        let entries = journal.replay_from(&actor_id, 1).await.unwrap();
        assert_eq!(entries.len(), 0);
    }

    /// Test truncate_to with sequence 0 keeps all entries
    #[tokio::test]
    async fn test_truncate_to_zero_keeps_all() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();
        let message = Message::new(b"test".to_vec());

        // Add 5 entries
        for _ in 0..5 {
            journal.record_message_received(&message).await.unwrap();
        }

        // Truncate to 0 should keep all entries (only removes entries <= 0)
        journal.truncate_to(&actor_id, 0).await.unwrap();

        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 5);
    }

    /// Test concurrent journal operations (thread safety)
    #[tokio::test]
    async fn test_concurrent_journal_operations() {
        use std::sync::Arc;

        let journal = Arc::new(MemoryJournal::new());
        let message = Message::new(b"concurrent".to_vec());

        // Spawn 10 tasks that concurrently record messages
        let mut handles = vec![];
        for _ in 0..10 {
            let journal = journal.clone();
            let message = message.clone();
            let handle = tokio::spawn(async move {
                journal.record_message_received(&message).await.unwrap();
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all 10 entries were recorded
        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 10);

        // Verify all have unique sequence numbers
        let mut sequences = vec![];
        for entry in &entries {
            match entry {
                JournalEntry::MessageReceived { sequence, .. } => {
                    sequences.push(*sequence);
                }
                _ => panic!("Expected MessageReceived entry"),
            }
        }

        sequences.sort();
        // Should be 1, 2, 3, ..., 10 (no duplicates)
        for (i, seq) in sequences.iter().enumerate() {
            assert_eq!(*seq, (i + 1) as u64);
        }
    }

    /// Test Default trait implementation for MemoryJournal
    #[tokio::test]
    async fn test_memory_journal_default() {
        // Test that Default::default() works correctly
        let journal = MemoryJournal::default();

        // Should be empty initially
        assert_eq!(journal.get_entries().await.unwrap().len(), 0);

        // Should be functional
        let message = Message::new(b"test".to_vec());
        journal.record_message_received(&message).await.unwrap();

        assert_eq!(journal.get_entries().await.unwrap().len(), 1);
    }

    // ========================================================================
    // Checkpoint Retention Tests (Phase 3.2)
    // ========================================================================

    #[tokio::test]
    async fn test_retention_keeps_last_n_snapshots() {
        let config = RetentionConfig {
            retention_count: 3, // Keep last 3
            auto_truncate: false,
        };
        let journal = MemoryJournal::with_retention(config);
        let actor_id = "actor-123".to_string();

        // Create 5 snapshots with increasing sequence numbers
        for seq in [10, 20, 30, 40, 50] {
            let snapshot = Snapshot {
                actor_id: actor_id.clone(),
                sequence: seq,
                timestamp: Utc::now(),
                state_data: vec![seq as u8], // Different data for each
                metadata: Default::default(),
                compression: CompressionType::None,
                encryption: EncryptionType::None,
            };
            journal.save_snapshot(snapshot).await.unwrap();
        }

        // Verify all 5 snapshots exist
        let all_snapshots = journal.list_snapshots(&actor_id).await.unwrap();
        assert_eq!(all_snapshots.len(), 5);

        // Enforce retention
        let deleted = journal.enforce_retention(&actor_id).await.unwrap();

        assert_eq!(deleted, 2); // Deleted seq 10, 20

        // Verify only last 3 snapshots remain
        let remaining = journal.list_snapshots(&actor_id).await.unwrap();
        assert_eq!(remaining.len(), 3);
        assert_eq!(remaining[0].sequence, 50); // Newest first
        assert_eq!(remaining[1].sequence, 40);
        assert_eq!(remaining[2].sequence, 30);
    }

    #[tokio::test]
    async fn test_retention_keeps_all_when_count_zero() {
        let config = RetentionConfig {
            retention_count: 0, // Keep all
            auto_truncate: false,
        };
        let journal = MemoryJournal::with_retention(config);
        let actor_id = "actor-123".to_string();

        // Create 5 snapshots
        for seq in [10, 20, 30, 40, 50] {
            let snapshot = Snapshot {
                actor_id: actor_id.clone(),
                sequence: seq,
                timestamp: Utc::now(),
                state_data: vec![seq as u8],
                metadata: Default::default(),
                compression: CompressionType::None,
                encryption: EncryptionType::None,
            };
            journal.save_snapshot(snapshot).await.unwrap();
        }

        // Enforce retention
        let deleted = journal.enforce_retention(&actor_id).await.unwrap();

        assert_eq!(deleted, 0); // Nothing deleted

        // Verify all 5 snapshots still exist
        let snapshots = journal.list_snapshots(&actor_id).await.unwrap();
        assert_eq!(snapshots.len(), 5);
    }

    #[tokio::test]
    async fn test_retention_no_deletion_when_under_limit() {
        let config = RetentionConfig {
            retention_count: 5, // Keep last 5
            auto_truncate: false,
        };
        let journal = MemoryJournal::with_retention(config);
        let actor_id = "actor-123".to_string();

        // Create only 3 snapshots (less than retention limit)
        for seq in [10, 20, 30] {
            let snapshot = Snapshot {
                actor_id: actor_id.clone(),
                sequence: seq,
                timestamp: Utc::now(),
                state_data: vec![seq as u8],
                metadata: Default::default(),
                compression: CompressionType::None,
                encryption: EncryptionType::None,
            };
            journal.save_snapshot(snapshot).await.unwrap();
        }

        // Enforce retention
        let deleted = journal.enforce_retention(&actor_id).await.unwrap();

        assert_eq!(deleted, 0); // Nothing deleted (under limit)

        // Verify all 3 snapshots still exist
        let snapshots = journal.list_snapshots(&actor_id).await.unwrap();
        assert_eq!(snapshots.len(), 3);
    }

    #[tokio::test]
    async fn test_retention_with_multiple_actors() {
        let config = RetentionConfig {
            retention_count: 2, // Keep last 2 per actor
            auto_truncate: false,
        };
        let journal = MemoryJournal::with_retention(config);

        // Create snapshots for actor-1
        for seq in [10, 20, 30] {
            let snapshot = Snapshot {
                actor_id: "actor-1".to_string(),
                sequence: seq,
                timestamp: Utc::now(),
                state_data: vec![seq as u8],
                metadata: Default::default(),
                compression: CompressionType::None,
                encryption: EncryptionType::None,
            };
            journal.save_snapshot(snapshot).await.unwrap();
        }

        // Create snapshots for actor-2
        for seq in [5, 15, 25, 35] {
            let snapshot = Snapshot {
                actor_id: "actor-2".to_string(),
                sequence: seq,
                timestamp: Utc::now(),
                state_data: vec![seq as u8],
                metadata: Default::default(),
                compression: CompressionType::None,
                encryption: EncryptionType::None,
            };
            journal.save_snapshot(snapshot).await.unwrap();
        }

        // Enforce retention for actor-1
        let deleted_1 = journal
            .enforce_retention(&"actor-1".to_string())
            .await
            .unwrap();
        assert_eq!(deleted_1, 1); // Deleted seq 10 for actor-1

        // Enforce retention for actor-2
        let deleted_2 = journal
            .enforce_retention(&"actor-2".to_string())
            .await
            .unwrap();
        assert_eq!(deleted_2, 2); // Deleted seq 5, 15 for actor-2

        // Verify actor-1 has 2 snapshots (20, 30)
        let snapshots_1 = journal
            .list_snapshots(&"actor-1".to_string())
            .await
            .unwrap();
        assert_eq!(snapshots_1.len(), 2);
        assert_eq!(snapshots_1[0].sequence, 30);
        assert_eq!(snapshots_1[1].sequence, 20);

        // Verify actor-2 has 2 snapshots (25, 35)
        let snapshots_2 = journal
            .list_snapshots(&"actor-2".to_string())
            .await
            .unwrap();
        assert_eq!(snapshots_2.len(), 2);
        assert_eq!(snapshots_2[0].sequence, 35);
        assert_eq!(snapshots_2[1].sequence, 25);
    }

    #[tokio::test]
    async fn test_save_snapshot_with_retention() {
        let config = RetentionConfig {
            retention_count: 2, // Keep last 2
            auto_truncate: false,
        };
        let journal = MemoryJournal::with_retention(config);
        let actor_id = "actor-123".to_string();

        // Create snapshots using save_snapshot_with_retention
        for seq in [10, 20, 30, 40] {
            let snapshot = Snapshot {
                actor_id: actor_id.clone(),
                sequence: seq,
                timestamp: Utc::now(),
                state_data: vec![seq as u8],
                metadata: Default::default(),
                compression: CompressionType::None,
                encryption: EncryptionType::None,
            };
            journal
                .save_snapshot_with_retention(snapshot)
                .await
                .unwrap();
        }

        // Verify only last 2 snapshots remain (retention enforced automatically)
        let snapshots = journal.list_snapshots(&actor_id).await.unwrap();
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].sequence, 40);
        assert_eq!(snapshots[1].sequence, 30);
    }

    #[tokio::test]
    async fn test_save_snapshot_with_retention_and_auto_truncate() {
        let config = RetentionConfig {
            retention_count: 2,
            auto_truncate: true, // Enable journal truncation
        };
        let journal = MemoryJournal::with_retention(config);
        let actor_id = "actor-123".to_string();
        let message = Message::new(b"test".to_vec());

        // Add journal entries
        for _ in 0..10 {
            journal.record_message_received(&message).await.unwrap();
        }

        // Verify 10 journal entries exist
        let entries_before = journal.get_entries().await.unwrap();
        assert_eq!(entries_before.len(), 10);

        // Create snapshot at sequence 5 with auto-truncation
        let snapshot = Snapshot {
            actor_id: actor_id.clone(),
            sequence: 5,
            timestamp: Utc::now(),
            state_data: b"state".to_vec(),
            metadata: Default::default(),
            compression: CompressionType::None,
            encryption: EncryptionType::None,
        };
        journal
            .save_snapshot_with_retention(snapshot)
            .await
            .unwrap();

        // Verify journal truncated (entries <= 5 removed, entries > 5 remain)
        let entries_after = journal.get_entries().await.unwrap();
        assert_eq!(entries_after.len(), 5); // Entries 6-10 remain
    }

    #[tokio::test]
    async fn test_list_snapshots_sorted_descending() {
        let journal = MemoryJournal::new();
        let actor_id = "actor-123".to_string();

        // Create snapshots out of order
        for seq in [30, 10, 50, 20, 40] {
            let snapshot = Snapshot {
                actor_id: actor_id.clone(),
                sequence: seq,
                timestamp: Utc::now(),
                state_data: vec![seq as u8],
                metadata: Default::default(),
                compression: CompressionType::None,
                encryption: EncryptionType::None,
            };
            journal.save_snapshot(snapshot).await.unwrap();
        }

        // List should be sorted descending by sequence
        let snapshots = journal.list_snapshots(&actor_id).await.unwrap();
        assert_eq!(snapshots.len(), 5);
        assert_eq!(snapshots[0].sequence, 50); // Newest first
        assert_eq!(snapshots[1].sequence, 40);
        assert_eq!(snapshots[2].sequence, 30);
        assert_eq!(snapshots[3].sequence, 20);
        assert_eq!(snapshots[4].sequence, 10); // Oldest last
    }

    /// Test truncate_to with all 8 JournalEntry types to ensure pattern match coverage
    #[tokio::test]
    async fn test_truncate_to_all_entry_types() {
        let journal = MemoryJournal::new();
        let actor_id = "test-actor".to_string();
        let message = Message::new(b"test".to_vec());

        // Create one of EACH entry type with different sequence numbers
        // Seq 1: MessageReceived
        journal.record_message_received(&message).await.unwrap();

        // Seq 2: MessageProcessed
        journal
            .record_message_processed(&message, &Ok(()))
            .await
            .unwrap();

        // Seq 3: StateChanged
        journal
            .record_state_change(&actor_id, b"old".to_vec(), b"new".to_vec())
            .await
            .unwrap();

        // Seq 4: SideEffectRecorded
        journal
            .record_side_effect(&actor_id, SideEffect::Sleep { duration_ms: 100 })
            .await
            .unwrap();

        // Seq 5: PromiseCreated
        journal
            .record_promise_created(
                "p1",
                PromiseMetadata {
                    creator_id: "a1".to_string(),
                    timeout_ms: None,
                    idempotency_key: None,
                },
            )
            .await
            .unwrap();

        // Seq 6: PromiseResolved
        journal
            .record_promise_resolved("p1", PromiseResult::Fulfilled(b"result".to_vec()))
            .await
            .unwrap();

        // Seq 7: Lifecycle
        journal
            .append(JournalEntry::Lifecycle {
                sequence: 7,
                timestamp: Utc::now(),
                actor_id: actor_id.clone(),
                event: "activated".to_string(),
            })
            .await
            .unwrap();

        // Seq 8: SnapshotTaken
        journal
            .append(JournalEntry::SnapshotTaken {
                sequence: 8,
                timestamp: Utc::now(),
                snapshot_id: "snap-1".to_string(),
            })
            .await
            .unwrap();

        // Verify we have all 8 entry types
        let entries_before = journal.get_entries().await.unwrap();
        assert_eq!(entries_before.len(), 8);

        // Truncate to sequence 4 (should remove entries 1-4, keep 5-8)
        journal.truncate_to(&actor_id, 4).await.unwrap();

        let entries_after = journal.get_entries().await.unwrap();
        assert_eq!(entries_after.len(), 4);

        // Verify correct entries remain (5-8)
        match &entries_after[0] {
            JournalEntry::PromiseCreated { sequence, .. } => assert_eq!(*sequence, 5),
            _ => panic!("Expected PromiseCreated"),
        }
        match &entries_after[1] {
            JournalEntry::PromiseResolved { sequence, .. } => assert_eq!(*sequence, 6),
            _ => panic!("Expected PromiseResolved"),
        }
        match &entries_after[2] {
            JournalEntry::Lifecycle { sequence, .. } => assert_eq!(*sequence, 7),
            _ => panic!("Expected Lifecycle"),
        }
        match &entries_after[3] {
            JournalEntry::SnapshotTaken { sequence, .. } => assert_eq!(*sequence, 8),
            _ => panic!("Expected SnapshotTaken"),
        }
    }

    #[tokio::test]
    async fn test_append_batch() {
        let journal = MemoryJournal::new();

        // Create a batch of journal entries
        let entries = vec![
            JournalEntry::Lifecycle {
                actor_id: "test-actor".to_string(),
                sequence: 1,
                timestamp: Utc::now(),
                event: "activated".to_string(),
            },
            JournalEntry::Lifecycle {
                actor_id: "test-actor".to_string(),
                sequence: 2,
                timestamp: Utc::now(),
                event: "started".to_string(),
            },
            JournalEntry::Lifecycle {
                actor_id: "test-actor".to_string(),
                sequence: 3,
                timestamp: Utc::now(),
                event: "running".to_string(),
            },
        ];

        // Append batch
        journal.append_batch(entries).await.unwrap();

        // Verify all entries appended
        let stored = journal.get_entries().await.unwrap();
        assert_eq!(stored.len(), 3);

        // Verify order preserved
        match &stored[0] {
            JournalEntry::Lifecycle {
                event, sequence, ..
            } => {
                assert_eq!(event, "activated");
                assert_eq!(*sequence, 1);
            }
            _ => panic!("Expected Lifecycle entry"),
        }
        match &stored[2] {
            JournalEntry::Lifecycle {
                event, sequence, ..
            } => {
                assert_eq!(event, "running");
                assert_eq!(*sequence, 3);
            }
            _ => panic!("Expected Lifecycle entry"),
        }
    }

    #[tokio::test]
    async fn test_append_batch_empty() {
        let journal = MemoryJournal::new();

        // Append empty batch (should not error)
        journal.append_batch(vec![]).await.unwrap();

        // Verify nothing appended
        let stored = journal.get_entries().await.unwrap();
        assert_eq!(stored.len(), 0);
    }

    #[tokio::test]
    async fn test_append_batch_performance() {
        let journal = MemoryJournal::new();

        // Create a large batch of 100 entries
        let mut batch = Vec::new();
        for i in 0..100 {
            batch.push(JournalEntry::Lifecycle {
                actor_id: "test-actor".to_string(),
                sequence: i,
                timestamp: Utc::now(),
                event: format!("event-{}", i),
            });
        }

        // Append batch
        journal.append_batch(batch).await.unwrap();

        // Verify all 100 entries appended
        let stored = journal.get_entries().await.unwrap();
        assert_eq!(stored.len(), 100);

        // Verify first and last entries
        match &stored[0] {
            JournalEntry::Lifecycle { sequence, .. } => assert_eq!(*sequence, 0),
            _ => panic!("Expected Lifecycle entry"),
        }
        match &stored[99] {
            JournalEntry::Lifecycle { sequence, .. } => assert_eq!(*sequence, 99),
            _ => panic!("Expected Lifecycle entry"),
        }
    }

    #[tokio::test]
    async fn test_append_batch_mixed_with_single_appends() {
        let journal = MemoryJournal::new();

        // Single append
        journal
            .append(JournalEntry::Lifecycle {
                actor_id: "test-actor".to_string(),
                sequence: 1,
                timestamp: Utc::now(),
                event: "single-1".to_string(),
            })
            .await
            .unwrap();

        // Batch append
        journal
            .append_batch(vec![
                JournalEntry::Lifecycle {
                    actor_id: "test-actor".to_string(),
                    sequence: 2,
                    timestamp: Utc::now(),
                    event: "batch-1".to_string(),
                },
                JournalEntry::Lifecycle {
                    actor_id: "test-actor".to_string(),
                    sequence: 3,
                    timestamp: Utc::now(),
                    event: "batch-2".to_string(),
                },
            ])
            .await
            .unwrap();

        // Another single append
        journal
            .append(JournalEntry::Lifecycle {
                actor_id: "test-actor".to_string(),
                sequence: 4,
                timestamp: Utc::now(),
                event: "single-2".to_string(),
            })
            .await
            .unwrap();

        // Verify all entries appended in correct order
        let stored = journal.get_entries().await.unwrap();
        assert_eq!(stored.len(), 4);

        match &stored[0] {
            JournalEntry::Lifecycle { event, .. } => assert_eq!(event, "single-1"),
            _ => panic!("Expected Lifecycle entry"),
        }
        match &stored[1] {
            JournalEntry::Lifecycle { event, .. } => assert_eq!(event, "batch-1"),
            _ => panic!("Expected Lifecycle entry"),
        }
        match &stored[2] {
            JournalEntry::Lifecycle { event, .. } => assert_eq!(event, "batch-2"),
            _ => panic!("Expected Lifecycle entry"),
        }
        match &stored[3] {
            JournalEntry::Lifecycle { event, .. } => assert_eq!(event, "single-2"),
            _ => panic!("Expected Lifecycle entry"),
        }
    }
}
#[tokio::test]
async fn test_memory_journal_with_config() {
    use crate::config::*;

    // Test with production config
    let config = JournalConfig::production();
    let journal = MemoryJournal::with_config(&config);

    // Verify retention settings applied
    let retention = journal.retention_config.read().await;
    assert_eq!(retention.retention_count, 10);
    assert!(!retention.auto_truncate); // Production config doesn't set auto_truncate

    // Test with custom config
    let config = JournalConfig::builder()
        .retention_count(5)
        .snapshot_auto_truncate(true)
        .build();
    let journal = MemoryJournal::with_config(&config);

    let retention = journal.retention_config.read().await;
    assert_eq!(retention.retention_count, 5);
    assert!(retention.auto_truncate);
}
