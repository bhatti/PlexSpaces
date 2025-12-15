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

//! Journal storage backends
//!
//! ## Purpose
//! Defines the `JournalStorage` trait and provides multiple backend implementations
//! for persisting journal entries and checkpoints.
//!
//! ## Design Pattern
//! Following the same pattern as TupleSpaceStorage, this module provides:
//! - Trait abstraction for backend-agnostic journal operations
//! - Multiple implementations (Memory, PostgreSQL, Redis, SQLite)
//! - Feature-gated backends for minimal dependencies
//!
//! ## Backends
//! - [`MemoryJournalStorage`]: In-memory HashMap (testing only)
//! - [`PostgresJournalStorage`]: PostgreSQL with auto-migrations (production)
//! - [`RedisJournalStorage`]: Redis (distributed, eventually consistent)
//! - [`SqliteJournalStorage`]: SQLite with WAL mode (edge deployments)

use crate::{Checkpoint, JournalEntry, JournalResult, JournalStats, ActorEvent, ActorHistory};
use async_trait::async_trait;
use plexspaces_proto::common::v1::{PageRequest, PageResponse};
use std::time::SystemTime;
use plexspaces_proto::prost_types;

// Re-export reminder types from proto
pub use plexspaces_proto::timer::v1::{ReminderRegistration, ReminderState};

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
/// See [`MemoryJournalStorage`] for a complete reference implementation.
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
    ///
    /// ## Arguments
    /// * `event` - Event to append
    ///
    /// ## Returns
    /// Sequence number assigned to the event
    ///
    /// ## Errors
    /// - `JournalError::Storage` if append fails
    ///
    /// ## Design Notes
    /// - Events are separate from journal entries (events = state changes)
    /// - Events are immutable (append-only)
    /// - Sequence numbers are monotonically increasing per actor
    /// - Backend may buffer this event (call `flush()` for durability)
    async fn append_event(&self, event: &ActorEvent) -> JournalResult<u64>;

    /// Append a batch of events atomically
    ///
    /// ## Arguments
    /// * `events` - Batch of events
    ///
    /// ## Returns
    /// Tuple of (first_sequence, last_sequence, count)
    ///
    /// ## Errors
    /// - `JournalError::Storage` if batch append fails
    ///
    /// ## Design Notes
    /// - All events written in single transaction (ACID backends)
    /// - More efficient than multiple `append_event` calls
    async fn append_events_batch(&self, events: &[ActorEvent]) -> JournalResult<(u64, u64, usize)>;

    /// Replay events from a specific sequence
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to replay events for
    /// * `from_sequence` - Start sequence (inclusive)
    ///
    /// ## Returns
    /// Vec of events in sequence order
    ///
    /// ## Errors
    /// - `JournalError::Storage` if replay fails
    ///
    /// ## Design Notes
    /// - Events are returned in sequence order (deterministic replay)
    /// - Empty vec if no events exist for actor
    /// - Used for event sourcing (reconstruct state from events)
    /// - **Note**: For large event logs, use `replay_events_from_paginated` instead
    async fn replay_events_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<ActorEvent>>;

    /// Replay events from a specific sequence (paginated, cursor-based)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to replay events for
    /// * `from_sequence` - Start sequence (inclusive)
    /// * `page_request` - Pagination parameters (page_size, page_token)
    ///
    /// ## Returns
    /// Tuple of (events, page_response) where page_response contains next_page_token
    ///
    /// ## Errors
    /// - `JournalError::Storage` if replay fails
    ///
    /// ## Design Notes
    /// - **Cursor-based pagination**: page_token is sequence number as string (O(1) lookup)
    /// - **Efficient**: Only fetches requested page_size events (not all events)
    /// - **Performance**: O(log n) binary search + O(page_size) fetch = efficient for large logs
    /// - **Memory efficient**: Doesn't load entire event log into memory
    /// - page_token format: sequence number as string (e.g., "123") for efficient cursor-based pagination
    /// - Use empty page_token for first page
    /// - Use next_page_token from response for subsequent pages
    ///
    /// ## Example
    /// ```rust
    /// let mut page_token = String::new();
    /// loop {
    ///     let (events, page_response) = storage
    ///         .replay_events_from_paginated(actor_id, from_seq, page_token, 100)
    ///         .await?;
    ///     // Process events...
    ///     if page_response.next_page_token.is_empty() {
    ///         break; // No more pages
    ///     }
    ///     page_token = page_response.next_page_token;
    /// }
    /// ```
    async fn replay_events_from_paginated(
        &self,
        actor_id: &str,
        from_sequence: u64,
        page_request: &PageRequest,
    ) -> JournalResult<(Vec<ActorEvent>, PageResponse)>;

    /// Get complete actor history (all events)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to get history for
    ///
    /// ## Returns
    /// Complete actor history with all events
    ///
    /// ## Errors
    /// - `JournalError::Storage` if retrieval fails
    ///
    /// ## Design Notes
    /// - Returns all events for the actor (for time-travel debugging)
    /// - Events are ordered by sequence
    /// - Used for audit trails and debugging
    /// - **Note**: For large event logs, use `get_actor_history_paginated` instead
    async fn get_actor_history(&self, actor_id: &str) -> JournalResult<ActorHistory>;

    /// Get actor history (paginated, cursor-based)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to get history for
    /// * `page_request` - Pagination parameters (page_size, page_token)
    ///
    /// ## Returns
    /// Actor history with paginated events and page_response
    ///
    /// ## Errors
    /// - `JournalError::Storage` if retrieval fails
    ///
    /// ## Design Notes
    /// - **Cursor-based pagination**: page_token is sequence number as string (O(1) lookup)
    /// - **Efficient**: Only fetches requested page_size events (not all events)
    /// - **Performance**: O(log n) binary search + O(page_size) fetch = efficient for large logs
    /// - **Memory efficient**: Doesn't load entire event log into memory
    /// - page_token format: sequence number as string (e.g., "123") for efficient cursor-based pagination
    /// - Use empty page_token for first page
    /// - Use next_page_token from response for subsequent pages
    ///
    /// ## Example
    /// ```rust
    /// let mut page_token = String::new();
    /// loop {
    ///     let history = storage
    ///         .get_actor_history_paginated(actor_id, page_token, 100)
    ///         .await?;
    ///     // Process events...
    ///     if history.page_response.next_page_token.is_empty() {
    ///         break; // No more pages
    ///     }
    ///     page_token = history.page_response.next_page_token;
    /// }
    /// ```
    async fn get_actor_history_paginated(
        &self,
        actor_id: &str,
        page_request: &PageRequest,
    ) -> JournalResult<ActorHistory>;

    // ==================== Reminder Methods ====================

    /// Register a reminder (persist to storage)
    ///
    /// ## Arguments
    /// * `reminder_state` - Reminder state to persist
    ///
    /// ## Returns
    /// Success or error
    ///
    /// ## Errors
    /// - `JournalError::Storage` if persistence fails
    ///
    /// ## Design Notes
    /// - Reminders are persisted to survive actor deactivation/restart
    /// - Reminder state includes registration, fire_count, next_fire_time
    /// - Backend may buffer this write (call `flush()` for durability)
    async fn register_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()>;

    /// Unregister a reminder (remove from storage)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor that owns the reminder
    /// * `reminder_name` - Name of reminder to unregister
    ///
    /// ## Returns
    /// Success or error
    ///
    /// ## Errors
    /// - `JournalError::Storage` if removal fails
    ///
    /// ## Design Notes
    /// - Removes reminder from persistent storage
    /// - Used when reminder is explicitly unregistered or max_occurrences reached
    async fn unregister_reminder(&self, actor_id: &str, reminder_name: &str) -> JournalResult<()>;

    /// Load all reminders for an actor
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to load reminders for
    ///
    /// ## Returns
    /// Vector of reminder states
    ///
    /// ## Errors
    /// - `JournalError::Storage` if load fails
    ///
    /// ## Design Notes
    /// - Called when ReminderFacet is attached to restore reminders
    /// - Only loads active reminders (is_active = true)
    /// - Used to restore reminders after actor reactivation
    async fn load_reminders(&self, actor_id: &str) -> JournalResult<Vec<ReminderState>>;

    /// Update reminder state (e.g., after firing)
    ///
    /// ## Arguments
    /// * `reminder_state` - Updated reminder state
    ///
    /// ## Returns
    /// Success or error
    ///
    /// ## Errors
    /// - `JournalError::Storage` if update fails
    ///
    /// ## Design Notes
    /// - Updates fire_count, last_fired, next_fire_time after reminder fires
    /// - Backend may buffer this write (call `flush()` for durability)
    async fn update_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()>;

    /// Query reminders that are due to fire
    ///
    /// ## Arguments
    /// * `before_time` - Find reminders with next_fire_time <= before_time
    ///
    /// ## Returns
    /// Vector of reminder states that are due
    ///
    /// ## Errors
    /// - `JournalError::Storage` if query fails
    ///
    /// ## Design Notes
    /// - Used by background task to find reminders that need to fire
    /// - Only returns active reminders (is_active = true)
    /// - Efficient query using index on next_fire_time
    async fn query_due_reminders(&self, before_time: SystemTime) -> JournalResult<Vec<ReminderState>>;
}

// Backend implementations
mod memory;
pub use memory::MemoryJournalStorage;

#[cfg(any(feature = "postgres-backend", feature = "sqlite-backend"))]
pub mod sql;

#[cfg(feature = "sqlite-backend")]
pub use sql::SqliteJournalStorage;

#[cfg(feature = "postgres-backend")]
pub use sql::PostgresJournalStorage;

#[cfg(feature = "redis-backend")]
mod redis;
#[cfg(feature = "redis-backend")]
pub use redis::RedisJournalStorage;
