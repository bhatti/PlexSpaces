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

//! Supervision module for fault tolerance
//!
//! Implements Erlang/OTP-inspired supervision trees with
//! elevated abstractions for adaptive recovery.
//!
//! ## Proto-First Design
//! All data models and errors are defined in `proto/plexspaces/v1/supervision.proto`:
//! - `SupervisionStrategy`, `ChildType` (enums)
//! - `ChildSpec`, `SupervisorSpec`, `SupervisorState` (messages)
//! - `SupervisionErrorCode`, `SupervisionError` (error types)

pub mod builder;
pub mod restart;
pub mod tree;

#[cfg(test)]
mod tests;

pub use builder::{SupervisorBuilder, SupervisorError};
pub use restart::RestartPolicy;
pub use tree::SupervisedChild;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

use crate::core::{ActorId, ServiceLocator as ServiceLocatorTrait};
use crate::ActorInstance;

// Import proto types
use plexspaces_proto::supervision::v1::SupervisorStats;

// ============================================================================
// Link Provider Trait (Phase 8.5: Link Semantics Integration)
// ============================================================================
pub use crate::core::LinkProvider;

// ============================================================================
// Supervised Child Trait (Rust-side interface, uses proto errors)
// ============================================================================

/// Supervisor events — canonical definition lives in proto.
pub use plexspaces_proto::supervision::v1::SupervisorEvent;

pub use plexspaces_proto::supervision::v1::ChildCount;
pub use plexspaces_proto::supervision::v1::ChildInfo;
pub use plexspaces_proto::supervision::v1::SupervisorEventType;

/// Supervision strategy (Erlang-inspired but elevated)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupervisionStrategy {
    /// One-for-one: restart only the failed actor
    OneForOne {
        /// Maximum number of restarts allowed within the time window.
        max_restarts: u32,
        /// Time window in seconds for counting restarts.
        within_seconds: u64,
    },
    /// One-for-all: restart all actors if one fails
    OneForAll {
        /// Maximum number of restarts allowed within the time window.
        max_restarts: u32,
        /// Time window in seconds for counting restarts.
        within_seconds: u64,
    },
    /// Rest-for-one: restart failed actor and all started after it
    RestForOne {
        /// Maximum number of restarts allowed within the time window.
        max_restarts: u32,
        /// Time window in seconds for counting restarts.
        within_seconds: u64,
    },
    /// Adaptive: Learn from failures and adapt strategy
    Adaptive {
        /// Initial supervision strategy before learning kicks in.
        initial_strategy: Box<SupervisionStrategy>,
        /// Learning rate for adapting the strategy (0.0–1.0).
        learning_rate: f64,
    },
    /// Custom strategy with callback
    Custom {
        /// Name identifying the custom strategy implementation.
        name: String,
    },
}

/// Supervisor for managing actor lifecycle and fault tolerance
pub struct Supervisor {
    /// Supervisor ID
    pub(crate) id: String,
    /// Supervision strategy (wrapped in Arc<RwLock> for adaptive strategies)
    pub(crate) strategy: Arc<RwLock<SupervisionStrategy>>,
    /// Child actors (IndexMap preserves insertion order for RestForOne)
    pub(crate) children: Arc<RwLock<IndexMap<ActorId, SupervisedActor>>>,
    /// Child supervisors (for hierarchical supervision trees)
    /// IndexMap preserves insertion order for RestForOne strategy
    pub(crate) child_supervisors: Arc<RwLock<IndexMap<String, SupervisedSupervisor>>>,
    /// Parent supervisor (if any)
    pub(crate) parent: Option<Arc<Supervisor>>,
    /// Restart statistics
    pub(crate) stats: Arc<RwLock<SupervisorStats>>,
    /// Event channel for notifications
    pub(crate) event_tx: mpsc::Sender<SupervisorEvent>,
    /// Shutdown signal
    pub(crate) _shutdown_rx: Option<mpsc::Receiver<()>>,
    /// Node reference for link semantics (Phase 8.5: Erlang link/1 pattern)
    /// When provided, supervisor uses links internally for cascading failures
    /// If None, supervisor works standalone without link semantics
    pub(crate) node: Option<Arc<dyn LinkProvider + Send + Sync>>,
    /// ServiceLocator for creating ActorRefs with service access
    /// Required for creating ActorRefs (both local and remote need ServiceLocator)
    pub(crate) service_locator: Option<Arc<dyn ServiceLocatorTrait>>,
    /// Default shutdown timeout for "infinity" shutdowns (prevents deadlocks)
    /// None = use default (1 second), Some(duration) = use custom timeout
    /// This is configurable for testing purposes
    pub(crate) default_shutdown_timeout: Option<Duration>,
}

/// Supervised actor wrapper
pub(crate) struct SupervisedActor {
    /// The actual actor instance
    pub(crate) actor: Arc<RwLock<ActorInstance>>,
    /// Actor task handle (for monitoring termination)
    pub(crate) handle: Option<tokio::task::JoinHandle<()>>,
    /// Restart count (total)
    pub(crate) restart_count: u32,
    /// Last restart time
    pub(crate) last_restart: Option<tokio::time::Instant>,
    /// Restart history for intensity tracking (timestamp of each restart)
    pub(crate) restart_timestamps: Vec<tokio::time::Instant>,
    /// Child specification for restarts (proto-first, includes facets)
    pub(crate) spec: crate::ChildSpec,
}

/// Supervised supervisor wrapper (for hierarchical supervision trees)
///
/// ## Purpose
/// Wraps a child supervisor with restart tracking and lifecycle management,
/// enabling supervisors to supervise other supervisors (Erlang/OTP-style).
///
/// ## Design (Proto-First Event Forwarding)
/// Event propagation is handled behaviorally (spawned task during add_child),
/// not stored as state. This enables:
/// - Future channel abstraction (no refactoring needed)
/// - Proto-first design (event propagation defined in proto)
/// - Clean separation (receiver is implementation detail)
///
/// ## Event Flow
/// ```text
/// ChildSupervisor -> ForwardingTask -> ParentSupervisor
/// ```
/// Event forwarding task is spawned when child supervisor is added,
/// not stored in this struct.
pub(crate) struct SupervisedSupervisor {
    /// The child supervisor instance
    pub(crate) supervisor: Arc<RwLock<Supervisor>>,
    /// Supervisor task handle (for monitoring termination)
    pub(crate) handle: Option<tokio::task::JoinHandle<()>>,
    /// Restart count (total)
    pub(crate) restart_count: u32,
    /// Last restart time
    pub(crate) last_restart: Option<tokio::time::Instant>,
    /// Restart history for intensity tracking
    pub(crate) restart_timestamps: Vec<tokio::time::Instant>,
    /// Restart policy (from spec)
    pub(crate) restart: RestartPolicy,
    /// Shutdown timeout in milliseconds
    pub(crate) shutdown_timeout_ms: Option<u64>,
}

/// Collected info about a child supervisor for batch shutdown.
pub(crate) type SupervisorShutdownInfo = Vec<(
    String,
    Arc<RwLock<Supervisor>>,
    Option<tokio::task::JoinHandle<()>>,
    Option<u64>,
)>;

/// Collected info about a child actor for batch shutdown.
pub(crate) type ActorShutdownInfo = Vec<(
    ActorId,
    Arc<RwLock<ActorInstance>>,
    Option<tokio::task::JoinHandle<()>>,
    Option<u64>,
)>;
