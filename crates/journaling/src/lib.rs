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

//! # PlexSpaces Journaling
//!
//! ## Purpose
//! Provides durable execution and event sourcing for PlexSpaces actors, enabling exactly-once
//! semantics, deterministic replay, and time-travel debugging through RESTATE-inspired journaling.
//!
//! ## Architecture Context
//! This crate implements **Pillar 3: Durability** (Restate-inspired) of the PlexSpaces architecture.
//! It is **100% optional** via the DurabilityFacet pattern (Static vs Dynamic design principle).
//!
//! ### Component Diagram
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ Actor (plexspaces-actor)                                        │
//! │   └─ DurabilityFacet ──┐                                        │
//! └─────────────────────────│────────────────────────────────────────┘
//!                           │
//!                           v
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ JournalStorage Trait (this crate)                              │
//! │   ├─ MemoryJournalStorage   (testing)                          │
//! │   ├─ PostgresJournalStorage (production)                       │
//! │   ├─ RedisJournalStorage    (distributed)                      │
//! │   └─ SqliteJournalStorage   (edge)                             │
//! └─────────────────────────────────────────────────────────────────┘
//!                           │
//!                           v
//! ┌─────────────────────────────────────────────────────────────────┐
//! │ Journal Table (PostgreSQL/SQLite) or Redis                     │
//! │   ├─ journal_entries   (append-only)                           │
//! │   └─ checkpoints       (periodic snapshots)                    │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Components
//! - [`JournalStorage`]: Trait for pluggable journal backends
//! - [`ExecutionContext`]: Tracks replay mode and caches side effects
//! - [`CheckpointManager`]: Periodic snapshots for fast recovery (90%+ faster)
//! - [`DurabilityFacet`]: Optional actor capability for durable execution
//!
//! ## Dependencies
//! This crate depends on:
//! - [`plexspaces_core`]: Common types (ActorId, ActorContext, errors)
//! - [`plexspaces_proto`]: Protocol buffer definitions for journal entry types
//! - [`plexspaces_mailbox`]: Message types for journaling
//! - [`sqlx`]: PostgreSQL and SQLite backends (optional, feature-gated)
//! - [`redis`]: Redis backend (optional, feature-gated)
//! - [`zstd`]: Checkpoint compression (optional, feature-gated)
//!
//! ## Dependents
//! This crate is used by:
//! - [`plexspaces_actor`]: Via DurabilityFacet for optional durability
//! - [`plexspaces_supervisor`]: For crash recovery and replay
//!
//! ## Examples
//!
//! ### Basic Usage (Memory Backend)
//! ```rust,no_run
//! use plexspaces_journaling::*;
//! use plexspaces_journaling::journal_entry;
//! use plexspaces_proto::prost_types;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create in-memory journal (for testing)
//! let storage = MemoryJournalStorage::new();
//!
//! // Create journal entry with MessageReceived
//! let entry = JournalEntry {
//!     id: ulid::Ulid::new().to_string(),
//!     actor_id: "actor-123".to_string(),
//!     sequence: 1,
//!     timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
//!     correlation_id: "corr-1".to_string(),
//!     entry: Some(journal_entry::Entry::MessageReceived(MessageReceived {
//!         message_id: "msg-1".to_string(),
//!         sender_id: "sender-123".to_string(),
//!         message_type: "test".to_string(),
//!         payload: vec![1, 2, 3],
//!         metadata: Default::default(),
//!     })),
//! };
//! storage.append_entry(&entry).await?;
//!
//! // Replay from sequence
//! let entries = storage.replay_from("actor-123", 0).await?;
//! for entry in entries {
//!     // Process entry for deterministic replay
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ### Production Usage (SQLite Backend)
//! ```rust,no_run
//! use plexspaces_journaling::*;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create SQLite journal for production (persistent storage)
//! let storage = SqliteJournalStorage::new("/var/lib/plexspaces/journal.db").await?;
//!
//! // Configure durability with checkpointing
//! let config = DurabilityConfig {
//!     backend: JournalBackend::JournalBackendSqlite as i32,
//!     checkpoint_interval: 1000,  // Checkpoint every 1000 messages
//!     checkpoint_timeout: None,
//!     replay_on_activation: true,
//!     cache_side_effects: true,
//!     compression: CompressionType::CompressionTypeZstd as i32,  // Enable compression
//!     state_schema_version: 1,
//!     backend_config: None,
//! };
//!
//! let facet = DurabilityFacet::new(storage, config);
//! // Attach to actor for durable execution
//! # Ok(())
//! # }
//! ```
//!
//! ### Attaching Durability to Actor
//! ```rust,no_run
//! use plexspaces_journaling::*;
//! use plexspaces_facet::Facet;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create storage backend
//! let storage = SqliteJournalStorage::new(":memory:").await?;
//!
//! // Configure durability
//! let config = DurabilityConfig {
//!     backend: JournalBackend::JournalBackendSqlite as i32,
//!     checkpoint_interval: 100,  // Checkpoint every 100 messages
//!     checkpoint_timeout: None,
//!     replay_on_activation: true,  // Replay journal on restart
//!     cache_side_effects: true,    // Cache external calls during replay
//!     compression: CompressionType::CompressionTypeNone as i32,
//!     state_schema_version: 1,
//!     backend_config: None,
//! };
//!
//! // Create durability facet
//! let mut facet = DurabilityFacet::new(storage, config);
//!
//! // Attach to actor (via Facet trait)
//! facet.on_attach("actor-123", serde_json::json!({})).await?;
//!
//! // Actor is now durable - all operations journaled
//! # Ok(())
//! # }
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First
//! All journal entry types are defined in `proto/plexspaces/v1/journaling.proto`:
//! - `JournalEntry` with 8 entry type variants (MessageReceived, MessageProcessed, etc.)
//! - `Checkpoint` for periodic snapshots
//! - `DurabilityConfig` for backend configuration
//! - `JournalService` gRPC service for distributed journaling
//!
//! ### Static vs Dynamic
//! Durability is **100% dynamic** (optional facet, not core):
//! - Actors without DurabilityFacet have zero journaling overhead
//! - Facet can be attached/detached at runtime
//! - Different actors can use different backends (memory, postgres, redis, sqlite)
//!
//! ### Test-Driven
//! - 90%+ test coverage target
//! - Integration tests for each backend
//! - Byzantine Generals example validates full system with replay
//! - Property-based tests for deterministic replay
//!
//! ## Testing
//! ```bash
//! # Run all tests (memory backend only)
//! cargo test -p plexspaces-journaling
//!
//! # Test with PostgreSQL backend
//! cargo test -p plexspaces-journaling --features postgres-backend
//!
//! # Test with all backends
//! cargo test -p plexspaces-journaling --all-features
//!
//! # Check coverage
//! cargo tarpaulin -p plexspaces-journaling --all-features
//! ```
//!
//! ## Performance Characteristics
//!
//! ### Write Performance
//! - **Batch Writes**: 1000 entries buffered before flush (configurable)
//! - **Flush Interval**: Max 1s latency before forced flush
//! - **Throughput**: 100K+ writes/sec with batching (PostgreSQL)
//! - **Compression**: 3-5x size reduction with zstd (checkpoints)
//!
//! ### Read Performance (Replay)
//! - **With Checkpoints**: 90%+ faster recovery (load checkpoint, replay delta)
//! - **Sequential Scan**: Optimized for time-ordered replay
//! - **Streaming**: Replay uses streaming to avoid loading full journal in memory
//!
//! ### Storage
//! - **JSONB Columns**: Extensible without schema changes (PostgreSQL)
//! - **Immutable Entries**: Append-only, never UPDATE or DELETE
//! - **Auto-Cleanup**: Truncate old entries after checkpoint (configurable)
//!
//! ## Known Limitations
//!
//! ### Current Implementation
//! - Redis backend: Eventually consistent (not ACID like PostgreSQL/SQLite)
//! - Side effect caching: Requires deterministic external APIs
//! - Time-travel debugging: Not yet implemented (planned)
//! - Distributed journaling: gRPC service not yet implemented (planned)
//!
//! ### Future Improvements
//! - Distributed consensus for multi-node journaling
//! - Compression for journal entries (currently only checkpoints)
//! - Incremental checkpoints (currently full-state snapshots)
//! - Schema versioning for journal entry evolution

#![warn(missing_docs)]
#![warn(clippy::all)]

// Re-export proto types for convenience
pub use plexspaces_proto::journaling::v1::{
    journal_entry,
    journal_entry::Entry,
    ActorEvent,
    ActorHistory,
    AppendBatchRequest,
    AppendBatchResponse,
    AppendRequest,
    AppendResponse,
    Checkpoint,
    CheckpointConfig,
    // Phase 3: Checkpoint Manager
    CheckpointManagerStats,
    CompressionType,
    DurabilityConfig,
    // Phase 2: Execution Context (RESTATE-inspired deterministic replay)
    ExecutionContext,
    ExecutionMode,
    EventSourcingConfig,
    GetLatestCheckpointRequest,
    GetStatsRequest,
    JournalBackend,
    JournalEntry,
    JournalStats,
    MessageProcessed,
    MessageReceived,
    PostgresJournalConfig,
    ProcessingResult,
    PromiseCreated,
    PromiseResolved,
    RedisJournalConfig,
    ReplayFromRequest,
    SaveCheckpointRequest,
    SaveCheckpointResponse,
    SideEffectEntry,
    SideEffectExecuted,
    SideEffectType,
    SqliteJournalConfig,
    StateChanged,
    TimerFired,
    TimerScheduled,
    TruncateToRequest,
    TruncateToResponse,
};

// Re-export timer types
pub use plexspaces_proto::timer::v1::{
    ReminderRegistration,
    ReminderState,
    TimerRegistration,
};

// Core modules
mod storage;
pub use storage::*;

// Phase 2: Execution context for deterministic replay
mod execution_context;
pub use execution_context::ExecutionContextImpl;

// Phase 3: Checkpoint manager for periodic snapshots
mod checkpoint_manager;
pub use checkpoint_manager::CheckpointManager;

// Phase 4: Durability facet for optional actor durability
mod durability_facet;
pub use durability_facet::DurabilityFacet;

// Phase 9.1: Replay handler for deterministic replay
mod replay_handler;
pub use replay_handler::ReplayHandler;

// Phase 9.1: State loader for automatic checkpoint loading
mod state_loader;
pub use state_loader::StateLoader;

// Phase 8.5: Virtual Actor facet for Orleans-style lifecycle
mod virtual_actor_facet;
pub use virtual_actor_facet::{ActivationStrategy, VirtualActorFacet, VirtualActorLifecycleState};

// Phase 8.5: Event Sourcing facet for Temporal-inspired event sourcing
mod event_sourcing_facet;
pub use event_sourcing_facet::EventSourcingFacet;

// Phase 8.5: Timer facet for Orleans-style non-durable timers
mod timer_facet;
pub use timer_facet::{TimerError, TimerFacet};
// TimerRegistration is re-exported from proto (above in timer module re-exports)

// Phase 8.5: Reminder facet for Orleans-style durable reminders
mod reminder_facet;
pub use reminder_facet::{ActivationProvider, ReminderError, ReminderFacet};

/// Journaling errors
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

/// Result type for journaling operations
pub type JournalResult<T> = Result<T, JournalError>;
