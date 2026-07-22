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

//! Journal storage trait for backend abstraction.
//!
//! # Purpose
//! Provides a unified interface for journal persistence across different storage backends.
//! All backends must implement this trait to be usable with DurabilityFacet.
//!
//! # Architecture Context
//! Moved to `plexspaces-service-traits` so that `plexspaces-journaling` can depend on it
//! without creating a cycle through `plexspaces-core`.

use async_trait::async_trait;
use plexspaces_proto::common::v1::{PageRequest, PageResponse};
use std::time::SystemTime;

// Re-export reminder types from proto (used by trait methods)
pub use plexspaces_proto::timer::v1::{ReminderRegistration, ReminderState};

// Re-export types needed by the trait (from journaling proto)
pub use plexspaces_proto::v1::journaling::{
    ActorEvent, ActorHistory, Checkpoint, JournalEntry, JournalStats,
};

/// Result type for journal operations.
pub type JournalResult<T> = Result<T, JournalError>;

/// Error type for journal operations.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// Storage backend error.
    #[error("Storage error: {0}")]
    Storage(String),

    /// Entry not found.
    #[error("Journal entry not found: actor_id={actor_id}, sequence={sequence}")]
    EntryNotFound {
        /// Actor ID
        actor_id: String,
        /// Sequence number
        sequence: u64,
    },

    /// Checkpoint not found.
    #[error("Checkpoint not found: actor_id={0}")]
    CheckpointNotFound(String),

    /// Compression error.
    #[error("Compression error: {0}")]
    Compression(String),

    /// Decompression error.
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Invalid configuration error (alias for Configuration).
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Replay error.
    #[error("Replay error: {0}")]
    Replay(String),

    /// Incompatible checkpoint schema version.
    ///
    /// Prevents loading checkpoints from newer actor versions that may have
    /// incompatible state format.
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

impl JournalError {
    /// Returns the proto error code for this error variant.
    pub fn code(&self) -> plexspaces_proto::journaling::v1::JournalErrorCode {
        use plexspaces_proto::journaling::v1::JournalErrorCode;
        match self {
            JournalError::Storage(_) => JournalErrorCode::JournalErrorStorage,
            JournalError::EntryNotFound { .. } => JournalErrorCode::JournalErrorEntryNotFound,
            JournalError::CheckpointNotFound(_) => JournalErrorCode::JournalErrorCheckpointNotFound,
            JournalError::Compression(_) | JournalError::Decompression(_) => {
                JournalErrorCode::JournalErrorCompression
            }
            JournalError::Serialization(_) => JournalErrorCode::JournalErrorSerialization,
            JournalError::Configuration(_) | JournalError::InvalidConfiguration(_) => {
                JournalErrorCode::JournalErrorStorage
            }
            JournalError::Replay(_) => JournalErrorCode::JournalErrorStorage,
            JournalError::IncompatibleSchemaVersion { .. } => {
                JournalErrorCode::JournalErrorConflict
            }
        }
    }
}

/// Journal storage trait for backend abstraction.
///
/// # Purpose
/// Provides a unified interface for journal persistence across different storage backends.
/// All backends must implement this trait to be usable with DurabilityFacet.
#[async_trait]
pub trait JournalStorage: Send + Sync {
    /// Append a single journal entry.
    async fn append_entry(&self, entry: &JournalEntry) -> JournalResult<u64>;

    /// Append a batch of journal entries atomically.
    async fn append_batch(&self, entries: &[JournalEntry]) -> JournalResult<(u64, u64, usize)>;

    /// Replay journal entries from a specific sequence.
    async fn replay_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<JournalEntry>>;

    /// Get the latest checkpoint for an actor.
    async fn get_latest_checkpoint(&self, actor_id: &str) -> JournalResult<Checkpoint>;

    /// Save a checkpoint.
    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> JournalResult<()>;

    /// Truncate journal entries up to a sequence number.
    async fn truncate_to(&self, actor_id: &str, sequence: u64) -> JournalResult<u64>;

    /// Get journal statistics.
    async fn get_stats(&self, actor_id: Option<&str>) -> JournalResult<JournalStats>;

    /// Flush any buffered entries to durable storage.
    async fn flush(&self) -> JournalResult<()>;

    /// Purge all persisted state for a single actor.
    async fn purge_actor(&self, actor_id: &str) -> JournalResult<u64> {
        Err(JournalError::Configuration(format!(
            "purge_actor is not implemented for actor_id={}",
            actor_id
        )))
    }

    /// Purge all persisted state for actors in a namespace.
    async fn purge_namespace(&self, namespace: &str) -> JournalResult<u64> {
        Err(JournalError::Configuration(format!(
            "purge_namespace is not implemented for namespace={}",
            namespace
        )))
    }

    // ==================== Event Sourcing Methods ====================

    /// Append a single event to the event log.
    async fn append_event(&self, event: &ActorEvent) -> JournalResult<u64>;

    /// Append a batch of events atomically.
    async fn append_events_batch(&self, events: &[ActorEvent]) -> JournalResult<(u64, u64, usize)>;

    /// Replay events from a specific sequence.
    async fn replay_events_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<ActorEvent>>;

    /// Replay events from a specific sequence (paginated, cursor-based).
    async fn replay_events_from_paginated(
        &self,
        actor_id: &str,
        from_sequence: u64,
        page_request: &PageRequest,
    ) -> JournalResult<(Vec<ActorEvent>, PageResponse)>;

    /// Get complete actor history (all events).
    async fn get_actor_history(&self, actor_id: &str) -> JournalResult<ActorHistory>;

    /// Get actor history (paginated, cursor-based).
    async fn get_actor_history_paginated(
        &self,
        actor_id: &str,
        page_request: &PageRequest,
    ) -> JournalResult<ActorHistory>;

    // ==================== Reminder Methods ====================

    /// Register a reminder (persist to storage).
    async fn register_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()>;

    /// Unregister a reminder (remove from storage).
    async fn unregister_reminder(&self, actor_id: &str, reminder_name: &str) -> JournalResult<()>;

    /// CAS-style alarm delete: removes the reminder only if its next_fire_time matches
    /// expected_next_fire_ms (Unix milliseconds). Returns Ok(true) if deleted, Ok(false) if
    /// the reminder was not found or the timestamp did not match.
    async fn unregister_reminder_if_matches(
        &self,
        actor_id: &str,
        reminder_name: &str,
        expected_next_fire_ms: u64,
    ) -> JournalResult<bool>;

    /// Load all reminders for an actor.
    async fn load_reminders(&self, actor_id: &str) -> JournalResult<Vec<ReminderState>>;

    /// Update reminder state (e.g., after firing).
    async fn update_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()>;

    /// Query reminders that are due to fire.
    async fn query_due_reminders(
        &self,
        before_time: SystemTime,
    ) -> JournalResult<Vec<ReminderState>>;
}
