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

//! ReplayHandler trait for deterministic message replay.
//!
//! # Purpose
//! Provides a type-safe interface for replaying journaled messages through
//! actor handlers during deterministic replay. This enables state recovery
//! through checkpoint + delta replay (Restate pattern).
//!
//! # Architecture Context
//! Part of Phase 9.1: Deterministic Replay. The `ReplayHandler` trait bridges
//! the gap between `DurabilityFacet` (which has journal entries) and `Actor`
//! (which has message handlers).
//!
//! # Design
//! - Type-safe trait (not function pointer) for flexibility
//! - Async support for actor message handling
//! - Error handling with proper error types
//! - Send + Sync for thread-safe usage
//! - **No `&ActorContext` parameter** — implementations store whatever context
//!   they need internally. This removes the dependency on `plexspaces-core`'s
//!   `ActorContext` type from the trait signature.

use async_trait::async_trait;
use plexspaces_proto::common::v1::Message;

/// Trait for replaying messages during deterministic replay.
///
/// # Purpose
/// Allows `DurabilityFacet` to replay journaled messages through the actor's
/// message handler, enabling state recovery through deterministic replay.
///
/// # Thread Safety
/// Must be `Send + Sync` for use in async contexts with `Arc<RwLock<>>`.
///
/// # Design
/// - Type-safe trait (not function pointer) for flexibility
/// - Async support for actor message handling
/// - Error handling with proper error types
/// - Implementations carry their own context (e.g. `Arc<ActorContext>` stored
///   during construction), keeping this trait free of `plexspaces-core` types.
#[async_trait]
pub trait ReplayHandler: Send + Sync {
    /// Replay a message through the actor's handler.
    ///
    /// # Arguments
    /// * `message` - Message to replay (reconstructed from journal entry)
    ///
    /// # Returns
    /// `Ok(())` if replay succeeded, error otherwise.
    ///
    /// # Errors
    /// - Actor handler errors (e.g., invalid message format)
    /// - State corruption errors (e.g., incompatible state)
    ///
    /// # Notes
    /// - The `ExecutionContext` will be in REPLAY mode during this call
    /// - Side effects will be cached (not re-executed)
    /// - State changes will be deterministic (same as original execution)
    /// - Implementations are responsible for storing any context they need
    ///   (e.g., an `Arc<ActorContext>`) during construction
    async fn replay_message(
        &self,
        message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Called before replay begins.
    /// Implementations should suppress side effects (timers, outbound messages)
    /// to prevent reentrance into the execution context during replay.
    async fn on_replay_start(&self) {}

    /// Called after replay completes (success or failure).
    /// Implementations should re-enable side effects.
    async fn on_replay_end(&self) {}
}
