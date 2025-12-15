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

//! ReplayHandler trait for deterministic message replay
//!
//! ## Purpose
//! Provides a type-safe interface for replaying journaled messages through
//! actor handlers during deterministic replay. This enables state recovery
//! through checkpoint + delta replay (Restate pattern).
//!
//! ## Architecture Context
//! Part of Phase 9.1: Deterministic Replay. The ReplayHandler trait bridges
//! the gap between DurabilityFacet (which has journal entries) and Actor
//! (which has message handlers).
//!
//! ## Design
//! - Type-safe trait (not function pointer) for flexibility
//! - Async support for actor message handling
//! - Error handling with proper error types
//! - Send + Sync for thread-safe usage
//!
//! ## Example
//! ```rust,no_run
//! use plexspaces_journaling::ReplayHandler;
//! use plexspaces_mailbox::Message;
//! use plexspaces_core::ActorContext;
//!
//! struct ActorReplayHandler {
//!     // Actor behavior and context
//! }
//!
//! #[async_trait::async_trait]
//! impl ReplayHandler for ActorReplayHandler {
//!     async fn replay_message(
//!         &self,
//!         message: Message,
//!         context: &ActorContext,
//!     ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//!         // Call actor's handle_message() method
//!         // ExecutionContext will be in REPLAY mode, so side effects are cached
//!         Ok(())
//!     }
//! }
//! ```

use async_trait::async_trait;
use plexspaces_core::ActorContext;
use plexspaces_mailbox::Message;

/// Trait for replaying messages during deterministic replay
///
/// ## Purpose
/// Allows DurabilityFacet to replay journaled messages through the actor's
/// message handler, enabling state recovery through deterministic replay.
///
/// ## Thread Safety
/// Must be Send + Sync for use in async contexts with Arc<RwLock<>>.
///
/// ## Design
/// - Type-safe trait (not function pointer) for flexibility
/// - Async support for actor message handling
/// - Error handling with proper error types
#[async_trait]
pub trait ReplayHandler: Send + Sync {
    /// Replay a message through the actor's handler
    ///
    /// ## Arguments
    /// * `message` - Message to replay (reconstructed from journal entry)
    /// * `context` - Actor context (with ExecutionContext in REPLAY mode)
    ///
    /// ## Returns
    /// `Ok(())` if replay succeeded, error otherwise
    ///
    /// ## Errors
    /// - Actor handler errors (e.g., invalid message format)
    /// - State corruption errors (e.g., incompatible state)
    ///
    /// ## Notes
    /// - ExecutionContext will be in REPLAY mode during this call
    /// - Side effects will be cached (not re-executed)
    /// - State changes will be deterministic (same as original execution)
    async fn replay_message(
        &self,
        message: Message,
        context: &ActorContext,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
