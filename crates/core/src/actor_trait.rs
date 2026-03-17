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

//! MessageSender trait for sending messages to actors
//!
//! ## Purpose
//! Provides a simple trait-based interface for sending messages to actors, inspired by Orleans grains.
//! Virtual actors can wrap this trait to automatically handle activation on `tell()`.
//!
//! ## Design (Orleans-Inspired)
//! - **Always Addressable**: MessageSender trait objects are always available via ActorRegistry
//! - **Automatic Activation**: Virtual actor wrapper activates on first `tell()` call
//! - **Simple API**: Just `tell()` method - activation is transparent
//!
//! ## Usage
//! ```rust,ignore
//! // Regular actor wrapper implements MessageSender trait
//! let regular_sender = ActorRef::local(actor_id, mailbox, service_locator);
//! regular_sender.tell(message).await?; // Sends to mailbox
//!
//! // Virtual actor wrapper automatically handles activation
//! let virtual_sender = VirtualActorWrapper::new(actor_id, node);
//! virtual_sender.tell(message).await?; // Activates if needed, then forwards message
//! ```

use async_trait::async_trait;
use plexspaces_proto::common::v1::Message;

/// MessageSender trait - interface for sending messages to actors
///
/// ## Purpose
/// Trait for sending messages to actors. Virtual actors can wrap this to handle activation automatically.
/// This is the "sender" side - use this to send messages to actors.
///
/// ## Design (Orleans-Inspired)
/// - **Always Addressable**: Trait objects stored in ActorRegistry
/// - **Automatic Activation**: Virtual actor wrapper activates on first `tell()` if needed
/// - **Simple**: Just one method - `tell()`
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
pub trait MessageSender: Send + Sync {
    /// Send a message to the actor (fire-and-forget)
    ///
    /// ## Purpose
    /// Erlang-style `!` operator - sends message to actor's mailbox.
    /// For virtual actors, this automatically activates the actor if needed.
    ///
    /// ## Arguments
    /// * `message` - Message to send
    ///
    /// ## Returns
    /// Ok(()) if message was sent successfully
    ///
    /// ## Behavior
    /// - **Regular actors**: Uses ActorRef::tell() which sends to mailbox
    /// - **Virtual actors**: Activates if needed, then uses ActorRef::tell()
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
}
