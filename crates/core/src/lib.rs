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

// Re-export Message from proto (unified message type)
pub use plexspaces_proto::common::v1::Message;

// Public modules
pub mod behavior_factory;
pub use behavior_factory::{BehaviorFactory, BehaviorFactoryError, BehaviorRegistry};
// registry module removed - replaced by object-registry
pub mod actor_context;
pub use actor_context::{LinkProvider, ActivationProvider};
pub mod actor_registry;
pub mod service_wrappers;
pub mod service_trait;
pub use service_trait::{Service, service_names};
pub mod service_locator_trait;
pub use service_locator_trait::{ServiceLocator, ApplicationManager, WasmRuntimeTrait, BlobServiceTrait, NodeRegistryTrait};
pub mod service_locator;
pub mod keyvalue_store;
pub use keyvalue_store::KeyValueStore;
// LockManager trait is in plexspaces-locks crate - re-export for convenience
pub use plexspaces_locks::{LockManager, LockError, LockResult};
pub use plexspaces_proto::locks::prv::{Lock, AcquireLockOptions, RenewLockOptions, ReleaseLockOptions};
pub use service_locator::request_context_from_grpc_request;
pub mod application_node_trait;
pub use application_node_trait::ApplicationNode;
pub mod grpc_connection_manager;
pub use grpc_connection_manager::{GrpcConnectionManager, ServiceType};
pub mod object_registry_helpers;
pub mod actor_trait;
pub mod exit_reason;
pub mod virtual_actor_manager;
pub mod actor_state_checker;
pub use actor_state_checker::ActorStateFetcher;
// FacetManager moved to plexspaces-facet crate to break circular dependency
pub mod facet_service_wrapper;
pub use facet_service_wrapper::{FacetRegistryServiceWrapper, FacetManagerServiceWrapper};
pub mod monitoring;
pub mod message_metrics;
pub mod reply_waiter;
pub use monitoring::{NodeMetricsAccessor, NodeConnectionInfo};
pub use message_metrics::{ActorMetrics, ActorMetricsHandle, ActorMetricsExt, new_actor_metrics};
pub mod journal_storage;
pub use journal_storage::{JournalStorage, JournalError, JournalResult};
/// Health module - consolidated health checking, reporting, and service functionality.
///
/// This module provides:
/// - `HealthChecker`: Run health checks on components
/// - `HealthReporter`: Report health status
/// - `PlexSpacesHealthReporter`: Tonic-integrated health service
pub mod health;
pub use health::reporter::HealthReporter;
pub use health::checker::{HealthChecker, HealthCheckContext, HealthCheckError, HealthCheckResult, run_health_check};
pub use health::service::PlexSpacesHealthReporter;
// Backward compatibility aliases
pub use health::reporter as health_reporter;
pub use health::checker as health_checker;
pub use health::service as health_service;
pub mod secret_masker;
pub use secret_masker::{SecretMasker, mask_release_spec, mask_map_secrets, DEFAULT_MASK};
pub mod actor_factory;
pub use actor_factory::ActorFactory;
pub mod constants;
pub use constants::TEMP_SENDER_PREFIX;

// Re-export enhanced ActorContext
pub use actor_context::{
    ActorContext, ActorService, ChannelService, FacetService, ObjectRegistry, ProcessGroupService, TupleSpaceProvider,
};
// Re-export ExitReason and ExitAction
pub use exit_reason::{ExitAction, ExitReason};
// ObjectRegistration is re-exported from proto via actor_context module
pub use actor_context::ObjectRegistration;
// Re-export ActorRegistry and related types
pub use actor_registry::{ActorRegistry, ActorRegistryError, ActorRoutingInfo, MonitorLink, TemporarySenderEntry};
// Re-export VirtualActorManager and VirtualActorMetadata (source of truth for virtual actors)
pub use virtual_actor_manager::{VirtualActorManager, VirtualActorError, VirtualActorMetadata};
// FacetManager re-exported from plexspaces-facet crate (for backward compatibility)
pub use plexspaces_facet::FacetManager;
// Re-export MessageSender trait (for sending messages to actors)
pub use actor_trait::MessageSender;
// Re-export ReplyWaiter and related types
pub use reply_waiter::{ReplyWaiter, ReplyWaiterRegistry, ReplyWaiterError};
// Re-export RequestContext from common crate
pub use plexspaces_common::{RequestContext, RequestContextError};

/// Actor ID type (String for simplicity and flexibility)
pub type ActorId = String;

// ActorContext is now in actor_context module with full service access
// See actor_context.rs for the enhanced version with ActorService, ObjectRegistry, etc.


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
    /// Initialize actor before entering message loop
    ///
    /// ## Purpose
    /// Called ONCE before any messages are processed. Supervisor waits for init() to complete
    /// before starting next child. If init() fails, actor is not registered and supervisor
    /// handles error.
    ///
    /// ## Erlang Equivalent
    /// Maps to gen_server:init/1
    ///
    /// ## Guarantees
    /// - Called ONCE before any messages are processed
    /// - Supervisor waits for init() to complete before starting next child
    /// - If init() fails, actor is not registered and supervisor handles error
    ///
    /// ## Default
    /// Returns Ok(()) - most actors don't need custom init
    async fn init(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
        Ok(())
    }

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
    /// if !msg.sender_id.is_empty() {
    ///     let actor_ref = ActorRef::remote(msg.sender_id.clone(), node_id, ctx.service_locator().clone());
    ///     let reply = Message { payload: b"response".to_vec(), ..Default::default() };
    ///     ActorRef::send_reply(&msg.sender_id, Some(&msg.correlation_id), reply, ctx.service_locator().clone(), ctx.actor_id().clone()).await?;
    /// }
    /// ```
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError>;

    /// Handle EXIT from linked actor (only if trap_exit = true)
    ///
    /// ## Purpose
    /// Called when a linked actor exits. Only called if ActorContext.trap_exit == true.
    /// Otherwise, linked actor death causes this actor to die too (default Erlang behavior).
    ///
    /// ## Erlang Equivalent
    /// Receiving {'EXIT', Pid, Reason} when process_flag(trap_exit, true)
    ///
    /// ## When Called
    /// Only called if ActorContext.trap_exit == true. Otherwise, linked
    /// actor death causes this actor to die too (default Erlang behavior).
    ///
    /// ## Returns
    /// - ExitAction::Propagate: crash this actor too (default link behavior)
    /// - ExitAction::Handle: absorb the exit, continue running
    ///
    /// ## Default
    /// Propagates the exit (actor will terminate)
    async fn handle_exit(
        &mut self,
        _ctx: &ActorContext,
        _from: &ActorId,
        _reason: &ExitReason,
    ) -> Result<ExitAction, ActorError> {
        Ok(ExitAction::Propagate)
    }

    /// Cleanup before actor stops
    ///
    /// ## Purpose
    /// Called when actor is stopping (graceful or crash). No new messages will be delivered
    /// after terminate() starts. In-flight message completes before terminate() is called.
    ///
    /// ## Erlang Equivalent
    /// Maps to gen_server:terminate/2
    ///
    /// ## Guarantees
    /// - Called ONCE when actor is stopping (graceful or crash)
    /// - No new messages will be delivered after terminate() starts
    /// - In-flight message completes before terminate() is called
    ///
    /// ## Default
    /// Returns Ok(()) - most actors don't need custom cleanup
    async fn terminate(
        &mut self,
        _ctx: &ActorContext,
        _reason: &ExitReason,
    ) -> Result<(), ActorError> {
        Ok(())
    }

    /// Called after all facets are attached and initialized (for behavior-specific initialization)
    ///
    /// ## Purpose
    /// This allows behaviors to initialize after facets are ready.
    /// Called after:
    /// 1. All facets are attached (facet.on_attach() called for all)
    /// 2. Actor.init() completes
    /// 3. All facets receive on_init_complete()
    ///
    /// ## When Called
    /// - After all facets are attached and initialized
    /// - Before actor enters message loop
    /// - Only if facets are present (otherwise not called)
    ///
    /// ## Default
    /// Returns Ok(()) - most behaviors don't need post-facet initialization
    async fn on_facets_ready(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
        Ok(())
    }

    /// Called before facets are detached (for behavior-specific cleanup)
    ///
    /// ## Purpose
    /// This allows behaviors to clean up before facets are detached.
    /// Called before:
    /// 1. Facets receive on_terminate_start()
    /// 2. Actor.terminate() is called
    /// 3. Facets are detached (facet.on_detach() called for all)
    ///
    /// ## When Called
    /// - Before facets are detached
    /// - After actor receives stop signal
    /// - Only if facets are present (otherwise not called)
    ///
    /// ## Default
    /// Returns Ok(()) - most behaviors don't need pre-facet-detachment cleanup
    async fn on_facets_detaching(&mut self, _ctx: &ActorContext, _reason: &ExitReason) -> Result<(), ActorError> {
        Ok(())
    }

    /// Get the behavior type (used for dashboard/actor_type index; Custom(name) for WASM actors).
    fn behavior_type(&self) -> BehaviorType;

    /// Get the OTP-style behavior kind for logging (GenServer, GenEvent, etc.).
    /// Defaults to behavior_type(); WASM actors override to report spec'd kind so spans show "GenEvent" not "SensorStream".
    fn behavior_kind(&self) -> BehaviorType {
        self.behavior_type()
    }
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

// Conversion from core's JournalError to ActorError
impl From<crate::JournalError> for ActorError {
    fn from(e: crate::JournalError) -> Self {
        ActorError::JournalError(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actor_id_parsing() {
        // Test valid actor IDs
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

    // test_local_tell moved to plexspaces-mailbox crate tests
    // It tests mailbox send/receive which is mailbox functionality
}
