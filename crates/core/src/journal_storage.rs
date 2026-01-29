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

//! Journal storage trait for backend abstraction
//!
//! ## Purpose
//! Provides a unified interface for journal persistence across different storage backends.
//! All backends must implement this trait to be usable with DurabilityFacet.
//!
//! ## Design Notes
//! - All methods are async (compatible with tokio runtime)
//! - Append operations may be buffered (backend-specific)
//! - Replay returns streaming iterator for large journals
//! - Truncate is safe (won't delete if entries are needed for replay)
//!
//! ## Why This Is In Core
//! This trait is moved to `plexspaces-core` to avoid circular dependencies.
//! The `plexspaces-journaling` crate implements this trait, and `plexspaces-actor`
//! uses it via `ServiceLocator` without needing to depend on `plexspaces-journaling`.

use async_trait::async_trait;
use plexspaces_proto::common::v1::{PageRequest, PageResponse};
use std::time::SystemTime;

// Re-export reminder types from proto (used by trait methods)
pub use plexspaces_proto::timer::v1::{ReminderRegistration, ReminderState};

// Re-export types needed by the trait (from journaling proto)
// These are used in method signatures
pub use plexspaces_proto::v1::journaling::{
    ActorEvent, ActorHistory, Checkpoint, JournalEntry, JournalStats,
};

/// Result type for journal operations
pub type JournalResult<T> = Result<T, JournalError>;

/// Error type for journal operations
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// Storage backend error
    #[error("Storage error: {0}")]
    Storage(String),

    /// Entry not found
    #[error("Journal entry not found: actor_id={actor_id}, sequence={sequence}")]
    EntryNotFound {
        /// Actor ID
        actor_id: String,
        /// Sequence number
        sequence: u64,
    },

    /// Checkpoint not found
    #[error("Checkpoint not found: actor_id={0}")]
    CheckpointNotFound(String),

    /// Compression error
    #[error("Compression error: {0}")]
    Compression(String),

    /// Decompression error
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Invalid configuration error (alias for Configuration)
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Replay error
    #[error("Replay error: {0}")]
    Replay(String),

    /// Incompatible checkpoint schema version
    ///
    /// ## Purpose
    /// Prevents loading checkpoints from newer actor versions that may have
    /// incompatible state format.
    ///
    /// ## Why This Exists
    /// - Forward compatibility not guaranteed (newer version may break old code)
    /// - Protects against state corruption from schema mismatches
    /// - Forces explicit migration for breaking schema changes
    ///
    /// ## Example
    /// ```text
    /// Actor v1 creates checkpoint with schema version 1
    /// Actor v2 loads checkpoint → OK (backward compatible)
    ///
    /// Actor v2 creates checkpoint with schema version 2
    /// Actor v1 loads checkpoint → ERROR (forward incompatible)
    /// ```
    #[error("Incompatible checkpoint schema version: checkpoint={checkpoint_version}, current={current_version}, actor_id={actor_id}")]
    IncompatibleSchemaVersion {
        /// Checkpoint schema version
        checkpoint_version: u32,
        /// Current actor schema version
        current_version: u32,
        /// Actor ID
        actor_id: String,
    },
}

/// Journal storage trait for backend abstraction
///
/// ## Purpose
/// Provides a unified interface for journal persistence across different storage backends.
/// All backends must implement this trait to be usable with DurabilityFacet.
///
/// ## Design Notes
/// - All methods are async (compatible with tokio runtime)
/// - Append operations may be buffered (backend-specific)
/// - Replay returns streaming iterator for large journals
/// - Truncate is safe (won't delete if entries are needed for replay)
///
/// ## Example Implementation
/// See `MemoryJournalStorage` in `plexspaces-journaling` for a complete reference implementation.
#[async_trait]
pub trait JournalStorage: Send + Sync {
    /// Append a single journal entry
    ///
    /// ## Arguments
    /// * `entry` - Journal entry to append
    ///
    /// ## Returns
    /// Sequence number assigned to the entry
    ///
    /// ## Errors
    /// - `JournalError::Storage` if append fails
    ///
    /// ## Design Notes
    /// - Backend may buffer this entry (not immediately durable)
    /// - Call `flush()` to ensure durability
    /// - Sequence numbers are monotonically increasing per actor
    async fn append_entry(&self, entry: &JournalEntry) -> JournalResult<u64>;

    /// Append a batch of journal entries atomically
    ///
    /// ## Arguments
    /// * `entries` - Batch of journal entries
    ///
    /// ## Returns
    /// Tuple of (first_sequence, last_sequence, count)
    ///
    /// ## Errors
    /// - `JournalError::Storage` if batch append fails
    ///
    /// ## Design Notes
    /// - All entries written in single transaction (ACID backends)
    /// - More efficient than multiple `append_entry` calls
    /// - Backend may still buffer batch (call `flush()` for durability)
    async fn append_batch(&self, entries: &[JournalEntry]) -> JournalResult<(u64, u64, usize)>;

    /// Replay journal entries from a specific sequence
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to replay journal for
    /// * `from_sequence` - Start sequence (inclusive)
    ///
    /// ## Returns
    /// Vec of journal entries in sequence order
    ///
    /// ## Errors
    /// - `JournalError::Storage` if replay fails
    ///
    /// ## Design Notes
    /// - Entries are returned in sequence order (deterministic replay)
    /// - Empty vec if no entries exist for actor
    /// - Use checkpoint to skip replaying old entries (performance)
    async fn replay_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<JournalEntry>>;

    /// Get the latest checkpoint for an actor
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to get checkpoint for
    ///
    /// ## Returns
    /// Latest checkpoint if exists
    ///
    /// ## Errors
    /// - `JournalError::CheckpointNotFound` if no checkpoint exists
    ///
    /// ## Design Notes
    /// - Returns most recent checkpoint (highest sequence number)
    /// - Checkpoint contains full actor state snapshot
    /// - Use to avoid replaying full journal from beginning
    async fn get_latest_checkpoint(&self, actor_id: &str) -> JournalResult<Checkpoint>;

    /// Save a checkpoint
    ///
    /// ## Arguments
    /// * `checkpoint` - Checkpoint to save
    ///
    /// ## Returns
    /// Success or error
    ///
    /// ## Errors
    /// - `JournalError::Storage` if save fails
    ///
    /// ## Design Notes
    /// - Checkpoint represents actor state at specific sequence number
    /// - Allows truncating journal entries before checkpoint (cleanup)
    /// - Compress state_data with zstd for 3-5x size reduction
    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> JournalResult<()>;

    /// Truncate journal entries up to a sequence number
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to truncate journal for
    /// * `sequence` - Truncate entries up to this sequence (inclusive)
    ///
    /// ## Returns
    /// Number of entries deleted
    ///
    /// ## Errors
    /// - `JournalError::Storage` if truncation fails
    ///
    /// ## Design Notes
    /// - Safe to call after saving checkpoint
    /// - Entries <= sequence are deleted
    /// - Entries > sequence are kept for replay
    /// - Used for cleanup to prevent unbounded journal growth
    async fn truncate_to(&self, actor_id: &str, sequence: u64) -> JournalResult<u64>;

    /// Get journal statistics
    ///
    /// ## Arguments
    /// * `actor_id` - Optional actor ID to filter stats (None = global stats)
    ///
    /// ## Returns
    /// Journal statistics
    ///
    /// ## Design Notes
    /// - Provides observability metrics
    /// - Used for monitoring journal health
    /// - Backend-specific implementation
    async fn get_stats(&self, actor_id: Option<&str>) -> JournalResult<JournalStats>;

    /// Flush any buffered entries to durable storage
    ///
    /// ## Returns
    /// Success or error
    ///
    /// ## Errors
    /// - `JournalError::Storage` if flush fails
    ///
    /// ## Design Notes
    /// - Backends may buffer writes for performance
    /// - Call this to ensure durability (e.g., before checkpoint)
    /// - Some backends (Memory) may no-op this
    async fn flush(&self) -> JournalResult<()>;

    // ==================== Event Sourcing Methods ====================

    /// Append a single event to the event log
    async fn append_event(&self, event: &ActorEvent) -> JournalResult<u64>;

    /// Append a batch of events atomically
    async fn append_events_batch(&self, events: &[ActorEvent]) -> JournalResult<(u64, u64, usize)>;

    /// Replay events from a specific sequence
    async fn replay_events_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<ActorEvent>>;

    /// Replay events from a specific sequence (paginated, cursor-based)
    async fn replay_events_from_paginated(
        &self,
        actor_id: &str,
        from_sequence: u64,
        page_request: &PageRequest,
    ) -> JournalResult<(Vec<ActorEvent>, PageResponse)>;

    /// Get complete actor history (all events)
    async fn get_actor_history(&self, actor_id: &str) -> JournalResult<ActorHistory>;

    /// Get actor history (paginated, cursor-based)
    async fn get_actor_history_paginated(
        &self,
        actor_id: &str,
        page_request: &PageRequest,
    ) -> JournalResult<ActorHistory>;

    // ==================== Reminder Methods ====================

    /// Register a reminder (persist to storage)
    async fn register_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()>;

    /// Unregister a reminder (remove from storage)
    async fn unregister_reminder(&self, actor_id: &str, reminder_name: &str) -> JournalResult<()>;

    /// Load all reminders for an actor
    async fn load_reminders(&self, actor_id: &str) -> JournalResult<Vec<ReminderState>>;

    /// Update reminder state (e.g., after firing)
    async fn update_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()>;

    /// Query reminders that are due to fire
    async fn query_due_reminders(&self, before_time: SystemTime) -> JournalResult<Vec<ReminderState>>;
}

