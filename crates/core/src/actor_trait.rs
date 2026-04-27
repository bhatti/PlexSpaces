// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! MessageSender trait for sending messages to actors
//!
//! ## Purpose
//! Provides a simple trait-based interface for sending messages to actors, inspired by Orleans grains.
//! Local virtual activation is owned by `ActorRegistry`, while concrete senders focus on delivery.
//!
//! ## Design (Orleans-Inspired)
//! - **Always Addressable**: Active senders are tracked in `ActorRegistry`
//! - **Automatic Activation**: `ActorRegistry::tell()` and `ActorRegistry::ask()` activate virtual actors on demand
//! - **Simple API**: Concrete senders only perform delivery
//!
//! ## Usage
//! ```rust,ignore
//! // Concrete actor sender implements MessageSender
//! let regular_sender = ActorRef::local(actor_id, mailbox, service_locator);
//! regular_sender.tell(message).await?; // Sends to mailbox
//!
//! // ActorRegistry handles local activation for virtual actors
//! registry.tell(&actor_id, message).await?;
//! ```

use crate::ActorStateHandle;
use async_trait::async_trait;
use plexspaces_proto::common::v1::Message;
use std::any::Any;
use std::sync::Arc;

/// MessageSender trait - interface for sending messages to actors
///
/// ## Purpose
/// Trait for sending messages to actors.
/// This is the "sender" side - use this to send messages to actors.
///
/// ## Design (Orleans-Inspired)
/// - **Always Addressable**: Active senders are stored in `ActorRegistry`
/// - **Automatic Activation**: Registry-owned local routing activates virtual actors when needed
/// - **Simple**: Senders only implement delivery semantics
///
/// ## Comparison to Other Frameworks
/// - **Orleans**: Grain references always available, method calls activate automatically
/// - **Erlang**: PIDs always addressable, `!` operator sends messages
/// - **Akka**: ActorRef always addressable, `tell()` sends messages
///
/// ## Note
/// This is different from `ActorBehavior` (renamed to `Actor`):
/// - `Actor` (ActorBehavior): What you implement to create an actor (handles messages)
/// - `MessageSender`: What you use to send messages to an actor
#[async_trait]
pub trait MessageSender: Send + Sync + Any {
    /// Send a message to the actor (fire-and-forget)
    ///
    /// ## Purpose
    /// Erlang-style `!` operator - sends message to actor's mailbox.
    /// Local virtual activation is handled by `ActorRegistry` before this sender is called.
    ///
    /// ## Arguments
    /// * `message` - Message to send
    ///
    /// ## Returns
    /// Ok(()) if message was sent successfully
    ///
    /// ## Behavior
    /// - **Regular actors**: `ActorRef::tell()` sends directly to the mailbox
    /// - **Virtual actors**: `ActorRegistry::tell()` activates if needed, then calls the sender
    async fn tell(&self, message: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Send a message and wait for a reply (request-reply pattern)
    ///
    /// ## Purpose
    /// Erlang-style call/ask pattern. Sends a message and waits for the actor
    /// to reply within the specified timeout.
    ///
    /// ## Arguments
    /// * `message` - Message to send (must have sender_id set for reply routing)
    /// * `timeout` - Maximum time to wait for reply
    ///
    /// ## Returns
    /// Reply message from the actor
    ///
    /// ## Default Implementation
    /// Returns an error by default. Implementations like `ActorRef` override this
    /// with proper ask semantics (correlation-based reply routing).
    async fn ask(
        &self,
        _message: Message,
        _timeout: std::time::Duration,
    ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
        Err("ask() not supported by this MessageSender implementation".into())
    }

    /// Returns the actor ID when this sender is an actor reference (e.g. from spawn).
    /// Used by callers that need the canonical ID without a registry lookup.
    fn actor_id(&self) -> Option<String> {
        None
    }

    /// Returns the tenant_id carried by this sender when available.
    ///
    /// Local and remote `ActorRef` values return their configured tenant scope.
    fn tenant_id(&self) -> Option<&str> {
        None
    }

    /// Returns the namespace carried by this sender when available.
    ///
    /// Local and remote `ActorRef` values return their configured namespace scope.
    fn namespace(&self) -> Option<&str> {
        None
    }

    /// Returns the actor type carried by this sender when available.
    fn actor_type(&self) -> Option<String> {
        None
    }

    /// Updates the actor type carried by this sender when supported.
    async fn set_actor_type(&self, _actor_type: Option<String>) {}

    /// Returns the runtime behavior kind carried by this sender when available.
    fn behavior_kind(&self) -> Option<String> {
        None
    }

    /// Updates the runtime behavior kind carried by this sender when supported.
    async fn set_behavior_kind(&self, _behavior_kind: Option<String>) {}

    /// Returns local lifecycle/state access when this sender points to a local actor runtime.
    ///
    /// Remote senders always return `None`.
    fn local_state_handle(&self) -> Option<Arc<dyn ActorStateHandle>> {
        None
    }

    /// Updates the local lifecycle/state handle carried by this sender when supported.
    async fn set_local_state_handle(&self, _handle: Option<Arc<dyn ActorStateHandle>>) {}

    /// Returns a reference to `self` as `dyn Any`.
    ///
    /// Enables checked downcast to concrete types (e.g., `ActorRef`) without a
    /// registry lookup. Every concrete `MessageSender` implementation must return
    /// `self` here. The `Any` supertrait bound guarantees this is safe.
    ///
    /// Prefer `actor_ref_from_sender` in the SDK rather than calling this directly.
    fn as_any(&self) -> &dyn Any;
}
