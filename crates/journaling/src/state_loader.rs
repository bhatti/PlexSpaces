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

//! StateLoader trait for automatic checkpoint state deserialization
//!
//! ## Purpose
//! Provides automatic checkpoint state loading and deserialization for actors
//! that support it. This enables Azure Durable Functions-style automatic state
//! restoration, while still allowing manual override (Restate-style) for
//! complex actors with schema migrations.
//!
//! ## Architecture Context
//! Part of Phase 9.1: Deterministic Replay. The StateLoader trait enables
//! hybrid state loading: automatic by default, manual override available.
//!
//! ## Design
//! - Optional trait (actors can implement for automatic loading)
//! - If not implemented, actor must manually call `get_latest_checkpoint()`
//! - Schema version validation happens automatically (framework-level)
//! - Actor controls deserialization format (flexible)
//!
//! ## Example
//! ```rust,no_run
//! use plexspaces_journaling::StateLoader;
//! use plexspaces_journaling::JournalResult;
//! use serde_json::Value;
//!
//! struct CounterActor {
//!     count: u64,
//! }
//!
//! #[async_trait::async_trait]
//! impl StateLoader for CounterActor {
//!     fn deserialize(&self, state_data: &[u8]) -> JournalResult<Value> {
//!         // Deserialize from protobuf/JSON/etc.
//!         let state: CounterState = bincode::deserialize(state_data)?;
//!         Ok(serde_json::to_value(state)?)
//!     }
//!
//!     async fn restore_state(&self, state: &Value) -> JournalResult<()> {
//!         let counter: CounterState = serde_json::from_value(state.clone())?;
//!         self.count = counter.count;
//!         Ok(())
//!     }
//!
//!     fn schema_version(&self) -> u32 {
//!         1  // Current schema version
//!     }
//! }
//! ```

use async_trait::async_trait;
use crate::{JournalError, JournalResult};
use serde_json::Value;

/// Trait for automatic checkpoint state loading
///
/// ## Purpose
/// Enables automatic state deserialization and restoration for actors that
/// support it. This provides Azure Durable Functions-style simplicity while
/// maintaining Restate-style flexibility (manual override available).
///
/// ## Thread Safety
/// Must be Send + Sync for use in async contexts with Arc<RwLock<>>.
///
/// ## Design
/// - Optional trait (actors can implement for automatic loading)
/// - If not implemented, actor must manually call `get_latest_checkpoint()`
/// - Schema version validation happens automatically (framework-level)
/// - Actor controls deserialization format (flexible)
#[async_trait]
pub trait StateLoader: Send + Sync {
    /// Deserialize checkpoint state data
    ///
    /// ## Arguments
    /// * `state_data` - Serialized state bytes from checkpoint
    ///
    /// ## Returns
    /// Deserialized state as JSON Value (flexible format)
    ///
    /// ## Errors
    /// - Deserialization errors (e.g., invalid format)
    /// - Corrupted state data
    fn deserialize(&self, state_data: &[u8]) -> JournalResult<Value>;

    /// Restore state to actor
    ///
    /// ## Arguments
    /// * `state` - Deserialized state (from `deserialize()`)
    ///
    /// ## Returns
    /// `Ok(())` if restoration succeeded, error otherwise
    ///
    /// ## Errors
    /// - State restoration errors (e.g., invalid state)
    /// - Type conversion errors
    ///
    /// ## Notes
    /// - Called after `deserialize()` during checkpoint loading
    /// - Actor should restore its internal state from the Value
    /// - This happens BEFORE delta replay starts
    async fn restore_state(&self, state: &Value) -> JournalResult<()>;

    /// Get current state schema version
    ///
    /// ## Returns
    /// Current schema version number (used for validation)
    ///
    /// ## Notes
    /// - Framework validates checkpoint schema version against this
    /// - Prevents loading incompatible checkpoints
    /// - Increment when making breaking state format changes
    fn schema_version(&self) -> u32;
}
