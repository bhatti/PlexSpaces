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

//! Core types and traits for PlexSpaces
//!
//! This crate contains the fundamental types shared between actor and behavior modules
//! to break circular dependencies.

#![warn(missing_docs)]
#![warn(clippy::all)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// Re-export types from dependencies
pub use plexspaces_mailbox::{Mailbox, Message};

// Public modules
pub mod application;
pub mod behavior_factory;
// registry module removed - replaced by object-registry
pub mod actor_context;
pub mod actor_registry;
// Service wrappers moved to node crate to avoid circular dependencies
// Only TupleSpaceProviderWrapper remains here since TupleSpace is in core
pub mod service_wrappers;
pub mod service_locator;
pub mod patterns;
pub mod actor_trait;
pub mod virtual_actor_manager;
pub mod facet_manager;
pub mod monitoring;
pub mod message_metrics;
pub mod reply_waiter;
pub use monitoring::NodeMetricsUpdater;
pub use message_metrics::{ActorMetrics, ActorMetricsHandle, ActorMetricsExt, new_actor_metrics};

// Re-export enhanced ActorContext
pub use actor_context::{
    ActorContext, ActorService, ChannelService, FacetService, ObjectRegistry, ProcessGroupService, TupleSpaceProvider,
};
// ObjectRegistration is re-exported from proto via actor_context module
pub use actor_context::ObjectRegistration;
// Re-export ActorRegistry and related types
pub use actor_registry::{ActorRegistry, ActorRegistryError, ActorRoutingInfo, MonitorLink, VirtualActorMetadata, TemporarySenderEntry};
// Re-export VirtualActorManager
pub use virtual_actor_manager::{VirtualActorManager, VirtualActorError};
// Re-export FacetManager
pub use facet_manager::FacetManager;
// Re-export ServiceLocator
pub use service_locator::{Service, ServiceLocator};
// Re-export MessageSender trait (for sending messages to actors)
pub use actor_trait::MessageSender;
// Re-export ReplyWaiter and related types
pub use reply_waiter::{ReplyWaiter, ReplyWaiterRegistry, ReplyWaiterError};

/// Actor ID type (String for simplicity and flexibility)
pub type ActorId = String;

// ActorContext is now in actor_context module with full service access
// See actor_context.rs for the enhanced version with ActorService, ObjectRegistry, etc.

use std::collections::HashMap;
use tokio::sync::{oneshot, RwLock};

/// Tracks pending ask() requests for reply routing
///
/// ## Purpose
/// Stores correlation_id -> reply channel mappings for the ask() pattern.
/// When a reply arrives with a matching correlation_id, it's routed to the waiting caller.
#[derive(Clone, Debug)]
pub struct ReplyTracker {
    /// Map of correlation_id -> oneshot::Sender for pending replies
    pending: Arc<RwLock<HashMap<String, oneshot::Sender<Message>>>>,
}

impl ReplyTracker {
    /// Create a new ReplyTracker
    pub fn new() -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a pending reply request
    pub async fn register(&self, correlation_id: String, reply_tx: oneshot::Sender<Message>) {
        let mut pending = self.pending.write().await;
        pending.insert(correlation_id, reply_tx);
    }

    /// Try to route a reply to a waiting caller
    ///
    /// Returns true if reply was routed, false if no pending request found
    pub async fn route_reply(&self, correlation_id: &str, reply: Message) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(reply_tx) = pending.remove(correlation_id) {
            // Ignore send error (caller may have timed out)
            let _ = reply_tx.send(reply);
            true
        } else {
            false
        }
    }

    /// Remove a pending reply (for cleanup on timeout/error)
    pub async fn remove(&self, correlation_id: &str) {
        let mut pending = self.pending.write().await;
        pending.remove(correlation_id);
    }
}

impl Default for ReplyTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl service_locator::Service for ReplyTracker {}

/// Lightweight actor reference - pure data, no methods, no service dependencies
///
/// ## Design Principles
/// - **Pure Data**: Just identity, no methods, no service references
/// - **Lightweight**: ~32 bytes (just identity)
/// - **Erlang/Akka/Orleans Inspired**: Follows proven patterns from industry leaders
///
/// ## Actor ID Format
/// Format: "actor_name@node_id"
/// Examples: "counter@node1", "user-session-123@prod-5"
///
/// ## Usage
/// All messaging goes through ActorService:
/// ```rust,ignore
/// let actor_ref = ActorRef::new("counter@node1".to_string())?;
/// actor_service.send(&actor_ref, message).await?;
/// ```
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorRef {
    /// Actor ID in format "actor_name@node_id"
    pub id: ActorId,
    /// Actor name (parsed from ID, cached for performance)
    pub actor_name: String,
    /// Node ID (parsed from ID, cached for performance)
    pub node_id: String,
}

impl ActorRef {
    /// Create ActorRef from identity
    ///
    /// # Arguments
    /// * `id` - Actor ID in format "actor_name@node_id"
    ///
    /// # Example
    /// ```ignore
    /// let actor_ref = ActorRef::new("counter@node1".to_string())?;
    /// ```
    pub fn new(id: ActorId) -> Result<Self, ActorError> {
        let (actor_name, node_id) = Self::parse_actor_id(&id)?;
        Ok(ActorRef {
            id,
            actor_name,
            node_id,
        })
    }

    /// Check if an actor is remote (different node) - static helper
    ///
    /// # Arguments
    /// * `actor_id` - Actor ID in format "actor@node"
    /// * `current_node` - Current node ID
    ///
    /// # Returns
    /// True if actor is on a different node
    ///
    /// # Example
    /// ```
    /// # use plexspaces_core::ActorRef;
    /// assert!(ActorRef::is_remote_actor("actor@node2", "node1"));
    /// assert!(!ActorRef::is_remote_actor("actor@node1", "node1"));
    /// ```
    pub fn is_remote_actor(actor_id: &str, current_node: &str) -> bool {
        if let Ok((_, node_id)) = Self::parse_actor_id(actor_id) {
            node_id != current_node
        } else {
            false
        }
    }

    /// Parse actor_id format: "actor_name@node_id"
    ///
    /// # Examples
    /// ```ignore
    /// let (name, node) = ActorRef::parse_actor_id("counter@node1")?;
    /// assert_eq!(name, "counter");
    /// assert_eq!(node, "node1");
    /// ```
    pub fn parse_actor_id(actor_id: &str) -> Result<(String, String), ActorError> {
        actor_id
            .split_once('@')
            .map(|(name, node)| (name.to_string(), node.to_string()))
            .ok_or_else(|| {
                ActorError::InvalidState(format!(
                    "Invalid actor ID format: '{}' (expected 'actor@node')",
                    actor_id
                ))
            })
    }

    /// Check if actor is on remote node
    ///
    /// # Arguments
    /// * `current_node_id` - Current node ID
    ///
    /// # Returns
    /// True if actor is on a different node
    ///
    /// # Example
    /// ```
    /// # use plexspaces_core::ActorRef;
    /// let actor_ref = ActorRef::new("actor@node2".to_string()).unwrap();
    /// assert!(actor_ref.is_remote("node1"));
    /// assert!(!actor_ref.is_remote("node2"));
    /// ```
    pub fn is_remote(&self, current_node_id: &str) -> bool {
        self.node_id != current_node_id
    }
    
    /// Get actor ID
    pub fn id(&self) -> &ActorId {
        &self.id
    }

    /// Get actor name (without node ID)
    pub fn actor_name(&self) -> &str {
        &self.actor_name
    }

    /// Get node ID
    pub fn node_id(&self) -> &str {
        &self.node_id
    }
}

/// Context passed to behavior when handling a message
pub struct BehaviorContext {
    /// Reference to actor context
    pub actor_context: Arc<ActorContext>,
    /// Current message being processed
    pub message: Message,
    /// Sender of the message - essential for request-reply patterns
    pub sender: Option<ActorRef>,
    /// Correlation ID for distributed tracing
    pub correlation_id: Option<String>,
    /// Span for distributed tracing
    #[cfg(feature = "tracing")]
    pub span: Option<tracing::Span>,
}

/// Actor trait - what you implement to create an actor
///
/// ## Purpose
/// This is the trait you implement to create an actor. It defines how the actor handles messages.
/// This is the "receiver" side - actors implement this to process messages.
///
/// ## Go-Style Context First Parameter
/// Following Go language conventions, context is the first parameter to all methods.
/// This makes it clear that context is always available and is the primary way to access services.
///
/// ## Note
/// This is different from `MessageSender`:
/// - `Actor` (this trait): What you implement to create an actor (handles messages)
/// - `MessageSender`: What you use to send messages to an actor
///
/// ## Previous Name
/// This was previously called `ActorBehavior` but renamed to `Actor` for clarity.
#[async_trait]
pub trait Actor: Send + Sync {
    /// Handle an incoming message
    ///
    /// ## Go-Style Signature
    /// Context is the first parameter (Go convention), followed by the message.
    /// This makes it clear that context provides all services needed.
    /// To send replies, use ActorRef::send_reply() via ctx.service_locator.
    ///
    /// ## Arguments
    /// * `ctx` - Actor context with all services
    /// * `msg` - Message to handle
    ///
    /// ## Sending Replies
    /// To send a reply, use ActorRef::send_reply() via ctx.service_locator:
    /// ```rust,ignore
    /// if let Some(sender_id) = &msg.sender {
    ///     let actor_ref = ActorRef::remote(sender_id.clone(), node_id, ctx.service_locator().clone());
    ///     let reply = Message::new(b"response".to_vec());
    ///     ActorRef::send_reply(sender_id, msg.correlation_id.as_deref(), reply, ctx.service_locator().clone(), ctx.actor_id().clone()).await?;
    /// }
    /// ```
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError>;

    /// Get the behavior type
    fn behavior_type(&self) -> BehaviorType;
}

// ActorBehavior has been renamed to Actor for clarity
// - Actor: What you implement to create an actor (handles messages)
// - MessageSender: What you use to send messages to an actor

/// Types of behaviors (OTP-inspired)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BehaviorType {
    /// GenServer-like request/reply behavior
    GenServer,
    /// GenEvent-like event handling
    GenEvent,
    /// GenStateMachine-like FSM
    GenStateMachine,
    /// Workflow-like orchestration
    Workflow,
    /// Custom behavior type
    Custom(String),
}

/// Behavior errors
#[derive(Debug, thiserror::Error)]
pub enum BehaviorError {
    /// Handler not found
    #[error("Handler not found: {0}")]
    HandlerNotFound(String),

    /// Unsupported message type
    #[error("Unsupported message type")]
    UnsupportedMessage,

    /// State transition failed
    #[error("State transition failed: {0}")]
    TransitionFailed(String),

    /// Processing error
    #[error("Processing error: {0}")]
    ProcessingError(String),
}

/// Actor errors
#[derive(Debug, thiserror::Error)]
pub enum ActorError {
    /// Mailbox error
    #[error("Mailbox error: {0}")]
    MailboxError(String),

    /// Behavior error
    #[error("Behavior error: {0}")]
    BehaviorError(String),

    /// Journal error
    #[error("Journal error: {0}")]
    JournalError(String),

    /// Actor not found
    ///
    /// ## Context
    /// The requested actor does not exist (local or remote).
    ///
    /// ## Suggestions
    /// - Verify the actor ID is correct (format: "name@node_id")
    /// - Check that the actor has been spawned
    /// - For remote actors, ensure the node is reachable
    #[error("Actor not found: {0}. Hint: Verify the actor ID is correct and the actor has been spawned.")]
    NotFound(String),

    /// Actor already exists
    #[error("Actor already exists: {0}")]
    AlreadyExists(String),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// No behavior to restore
    #[error("No behavior to restore")]
    NoBehaviorToRestore,

    /// Request timeout
    ///
    /// ## Context
    /// An ask() request timed out waiting for a reply.
    ///
    /// ## Suggestions
    /// - Increase the timeout duration
    /// - Check that the actor is processing messages
    /// - Verify network connectivity for remote actors
    #[error("Request timeout. Hint: The actor may be overloaded or unreachable. Consider increasing the timeout or checking actor health.")]
    Timeout,

    /// Actor terminated
    #[error("Actor terminated")]
    ActorTerminated,

    /// Facet error
    #[error("Facet error: {0}")]
    FacetError(String),
}

// Conversion from JournalError to ActorError
impl From<plexspaces_persistence::JournalError> for ActorError {
    fn from(e: plexspaces_persistence::JournalError) -> Self {
        ActorError::JournalError(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_actor_id_parsing() {
        // Test valid actor IDs
        let mailbox = Arc::new(Mailbox::new(Default::default(), "counter@node1".to_string()).await.expect("Failed to create mailbox"));

        let actor_ref = ActorRef::new("counter@node1".to_string()).unwrap();
        assert_eq!(actor_ref.id(), "counter@node1");
        assert_eq!(actor_ref.actor_name(), "counter");
        assert_eq!(actor_ref.node_id(), "node1");

        // Test complex actor ID
        let actor_ref2 = ActorRef::new("user-session-123@prod-5".to_string()).unwrap();
        assert_eq!(actor_ref2.actor_name(), "user-session-123");
        assert_eq!(actor_ref2.node_id(), "prod-5");

        // Test invalid actor ID (missing @)
        let result = ActorRef::new("invalid_actor_id".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_actor_id_static() {
        // Test static method for parsing actor IDs
        let (name, node) = ActorRef::parse_actor_id("counter@node1").unwrap();
        assert_eq!(name, "counter");
        assert_eq!(node, "node1");

        let (name, node) = ActorRef::parse_actor_id("user-123@prod-5").unwrap();
        assert_eq!(name, "user-123");
        assert_eq!(node, "prod-5");

        // Invalid format
        assert!(ActorRef::parse_actor_id("invalid").is_err());
    }

    #[tokio::test]
    async fn test_local_tell() {
        use plexspaces_mailbox::mailbox_config_default;
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@localhost".to_string()).await.expect("Failed to create mailbox"));
        let actor_ref = ActorRef::new("test@localhost".to_string()).unwrap();

        // Send a message directly to mailbox (ActorRef no longer has tell method)
        let message = Message::new(vec![1, 2, 3]);
        let result = mailbox.send(message).await;
        assert!(result.is_ok());

        // Verify message was delivered to mailbox
        let received = mailbox.dequeue().await;
        assert!(received.is_some());
        assert_eq!(received.unwrap().payload(), &vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_tell_and_ask_metrics() {
        // NOTE: This test is disabled because ActorRef is now pure data (no tell() method).
        // Metrics testing should be done at the ActorService level instead.
        // This test mainly ensured the metrics code doesn't panic, which is now handled
        // by ActorService metrics.
    }
}
