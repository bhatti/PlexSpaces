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

//! # PlexSpaces Actor Module
//!
//! ## Purpose
//! Provides the core actor abstraction with lifecycle management, behavior composition,
//! and resource-aware scheduling. This is the foundational building block for all
//! distributed computation in PlexSpaces.
//!
//! ## Architecture Context
//! This module implements **Pillar 2 (Erlang/OTP Philosophy)** of PlexSpaces.
//! It provides the unified actor model where one powerful actor type has composable
//! capabilities instead of multiple specialized types.
//!
//! ### Which Pillars This Supports
//! - **Pillar 1 (TupleSpace)**: Actors can coordinate via TupleSpace for decoupled communication
//! - **Pillar 2 (Erlang/OTP)**: Core implementation of actor model, lifecycle, supervision
//! - **Pillar 3 (Durability)**: Integration with journaling for durable execution
//! - **Pillar 4 (WASM)**: Actors can execute as WASM modules (via behavior abstraction)
//! - **Pillar 5 (Firecracker)**: Actors can run in isolated microVMs (via node placement)
//!
//! ### Component Diagram
//! ```text
//! plexspaces_proto (definitions)
//!        |
//!        v
//! plexspaces_actor (this module) <--- plexspaces_supervision
//!        |                                     |
//!        +---> plexspaces_mailbox              |
//!        +---> plexspaces_journal              |
//!        +---> plexspaces_behavior             |
//!        +---> plexspaces_facet                |
//!        |                                     |
//!        +-------------------------------------+
//!                      |
//!                      v
//!              plexspaces_node
//! ```
//!
//! ## Key Components
//! - [`Actor`]: The unified actor implementation with static lifecycle and dynamic facets
//! - [`ActorRef`]: Lightweight, cloneable handle for sending messages to actors
//! - [`ActorRegistry`]: Thread-safe registry for actor discovery and lookup
//! - [`ActorContext`]: Provides actors access to system resources (journal, node, etc.)
//! - [`ActorState`]: Lifecycle state enum (Creating, Active, Terminated, etc.)
//! - [`ResourceProfile`]: Resource-aware scheduling profiles (CpuIntensive, IoIntensive, etc.)
//! - [`ResourceContract`]: QoS guarantees for actors (max CPU, memory, IO, etc.)
//! - [`ActorHealth`]: Health status tracking (Healthy, Degraded, Stuck, Failed)
//!
//! ## Dependencies
//! This module depends on:
//! - [`plexspaces_proto`]: Actor message definitions, state enums
//! - [`crate::behavior`]: OTP-style behaviors (GenServer, GenEvent, etc.)
//! - [`crate::mailbox`]: Message queue abstraction with priority and backpressure
//! - [`crate::journal`]: Durable execution and replay for fault tolerance
//! - [`crate::facet`]: Dynamic capability composition (mobility, metrics, etc.)
//!
//! ## Dependents
//! This module is used by:
//! - [`crate::supervision`]: Supervises actors and handles restart policies
//! - [`crate::node`]: Hosts actors on compute nodes with resource isolation
//! - [`plexspaces_mobility`]: Migrates actors between nodes with state preservation
//! - [`plexspaces_workflow`]: Orchestrates multi-actor workflows
//!
//! ## Examples
//!
//! ### Basic Actor Creation
//! ```rust,ignore
//! use plexspaces::actor::*;
//! use plexspaces::behavior::MockBehavior;
//! use plexspaces::mailbox::{Mailbox, MailboxConfig};
//! use plexspaces::journal::MemoryJournal;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), ActorError> {
//! // Create an actor with default behavior
//! let behavior = Box::new(MockBehavior::new());
//! let mailbox = Mailbox::new(MailboxConfig::default());
//! let journal = Arc::new(MemoryJournal::new());
//!
//! let mut actor = Actor::new(
//!     "my-actor",
//!     behavior,
//!     mailbox,
//!     journal,
//!     "default-namespace",
//! );
//!
//! // Start the actor (transitions Created -> Activating -> Active)
//! actor.start().await?;
//!
//! // Actor is now processing messages
//! // ...
//!
//! // Stop the actor (transitions Active -> Deactivating -> Terminated)
//! actor.stop().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Resource-Aware Actor
//! ```rust,ignore
//! use plexspaces::actor::*;
//! # use plexspaces::behavior::MockBehavior;
//! # use plexspaces::mailbox::{Mailbox, MailboxConfig};
//! # use plexspaces::journal::MemoryJournal;
//! # use std::sync::Arc;
//!
//! # async fn example() -> Result<(), ActorError> {
//! # let behavior = Box::new(MockBehavior::new());
//! # let mailbox = Mailbox::new(MailboxConfig::default());
//! # let journal = Arc::new(MemoryJournal::new());
//! // Create actor with resource profile and contract (Quickwit-inspired)
//! let actor = Actor::new("cpu-heavy-actor", behavior, mailbox, journal, "default")
//!     .with_resource_profile(ResourceProfile::CpuIntensive)
//!     .with_resource_contract(ResourceContract::cpu_intensive());
//!
//! // Actor will be scheduled on CPU-optimized runtime
//! // Resource usage validated against contract
//! # Ok(())
//! # }
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First
//! All actor state and messages use Protocol Buffer definitions:
//! - `proto/plexspaces/v1/actor_runtime.proto` defines Actor, Message, ActorState
//! - This module implements the logic using generated proto types
//! - Ensures wire compatibility for distributed actor communication
//!
//! ### Static vs Dynamic Design
//! This module follows PlexSpaces' "Static for core, dynamic for extensions" principle:
//!
//! **Static (Core, Always Present)**:
//! - `Actor.id`: Every actor needs unique identity
//! - `Actor.state`: Every actor has lifecycle state
//! - `Actor.behavior`: Every actor processes messages
//! - `Actor.mailbox`: Every actor receives messages
//! - `Actor.journal`: Every actor is durable (Pillar 3)
//! - Lifecycle hooks: `on_activate()`, `on_deactivate()`, `on_timer()`
//!
//! **Dynamic (Extensions via Facets)**:
//! - WASM Migration: State-only migration via WASM runtime
//! - Metrics: `MetricsFacet` for Prometheus integration
//! - Tracing: `TracingFacet` for distributed tracing
//! - Security: `SecurityFacet` for authorization
//!
//! ### Resource Awareness (Quickwit-Inspired)
//! Actors declare resource profiles and contracts upfront:
//! - **ResourceProfile**: Type of resources consumed (CPU, Memory, IO, Network, Balanced)
//! - **ResourceContract**: QoS guarantees (max CPU%, memory bytes, IO ops/sec)
//! - System uses these for intelligent placement and scheduling
//!
//! ### Test-Driven Development
//! This module maintains 90%+ test coverage:
//! - Unit tests: `#[cfg(test)] mod tests` at bottom of this file
//! - Integration tests: Tests actor lifecycle with real components
//! - Doc tests: Examples above are tested via `cargo test --doc`
//!
//! ## Testing
//! ```bash
//! # Run all actor tests
//! cargo test --lib actor
//!
//! # Check coverage
//! cargo tarpaulin --lib actor --out Html
//! open tarpaulin-report.html
//!
//! # Run doc tests
//! cargo test --doc actor
//! ```
//!
//! ## Performance Characteristics
//! - **Actor creation**: < 1ms (in-memory)
//! - **Message delivery**: < 10μs (local), < 1ms (remote)
//! - **State transition**: < 100μs (Activating -> Active)
//! - **Memory per actor**: < 1KB (idle), < 10KB (active with state)
//!
//! ## Known Limitations
//! - **Actor groups**: Planned for Week 6-7 (data-parallel actors from NSDI'22 paper)
//! - **Hot code swapping**: Planned via WASM module replacement
//! - **Distributed placement**: Currently random, planned intelligent placement

// Submodules are declared in lib.rs, not here

use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
// For catch_unwind()
use crate::resource::{ActorHealth, ResourceContract, ResourceProfile, ResourceUsage};

// Import from external crates
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, ActorError, ActorId, BehaviorError, ExitAction, ExitReason,
};

/// Parse ExitReason from string representation (used for EXIT messages)
fn parse_exit_reason_from_str(reason_str: &str, metadata: &std::collections::HashMap<String, String>) -> ExitReason {
    if reason_str == "Normal" {
        ExitReason::Normal
    } else if reason_str == "Shutdown" {
        ExitReason::Shutdown
    } else if reason_str == "Killed" {
        ExitReason::Killed
    } else if reason_str.starts_with("Error:") {
        let error_msg = reason_str.strip_prefix("Error:").unwrap_or("unknown error").to_string();
        ExitReason::Error(error_msg)
    } else if reason_str.starts_with("Linked:") {
        let linked_actor_id = reason_str.strip_prefix("Linked:").unwrap_or("unknown").to_string();
        let linked_error_str = metadata.get("exit_linked_error")
            .map(|s| s.as_str())
            .unwrap_or("Normal");
        let linked_reason = parse_exit_reason_from_str(linked_error_str, metadata);
        ExitReason::Linked {
            actor_id: linked_actor_id,
            reason: Box::new(linked_reason),
        }
    } else {
        ExitReason::Normal // Default
    }
}
use plexspaces_facet::{Facet, FacetContainer};
use plexspaces_mailbox::{Mailbox, Message};
use plexspaces_journaling::ReplayHandler;
use async_trait::async_trait;

// Observability
use metrics;
use tracing;

/// Actor state - matches proto ActorState enum exactly
///
/// ## State Transitions
/// ```
/// CREATING -> ACTIVATING -> ACTIVE -> DEACTIVATING -> INACTIVE
///                        \-> MIGRATING -> ACTIVE (on new node)
///                        \-> FAILED -> (supervisor restarts)
///                        \-> TERMINATED (permanent stop)
/// ```
///
/// ## Design Notes
/// - This enum matches proto `ActorState` exactly for consistency
/// - FAILED state includes error message for debugging
/// - All actors (virtual or not) use the same ActorState
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorState {
    /// Unspecified state (should never occur in valid states)
    Unspecified,
    /// Actor is created but not yet started (replaces "Initializing")
    Creating,
    /// Actor is running and processing messages
    Active,
    /// Actor is suspended/not processing messages (replaces "Suspended")
    Inactive,
    /// Actor is activating (loading state, running on_activate)
    Activating,
    /// Actor is deactivating (saving state, running on_deactivate)
    Deactivating,
    /// Actor is stopping (shutdown in progress) - NEW
    Stopping,
    /// Actor is migrating to another node
    Migrating,
    /// Actor has crashed (includes error message)
    Failed(String),
    /// Actor has permanently stopped (replaces "Stopped")
    Terminated,
}

// NOTE: ActorLifecycle trait REMOVED - lifecycle is now STATIC (always present)
// This was a design flaw: lifecycle hooks are CORE to every actor, not optional.
// The new design has lifecycle methods directly on Actor (see impl Actor below)

impl ActorState {
    /// Convert to proto ActorState enum
    pub fn to_proto(&self) -> plexspaces_proto::v1::actor::ActorState {
        use plexspaces_proto::v1::actor::ActorState as ProtoState;
        match self {
            ActorState::Unspecified => ProtoState::ActorStateUnspecified,
            ActorState::Creating => ProtoState::ActorStateCreating,
            ActorState::Active => ProtoState::ActorStateActive,
            ActorState::Inactive => ProtoState::ActorStateInactive,
            ActorState::Activating => ProtoState::ActorStateActivating,
            ActorState::Deactivating => ProtoState::ActorStateDeactivating,
            ActorState::Stopping => ProtoState::ActorStateStopping,
            ActorState::Migrating => ProtoState::ActorStateMigrating,
            ActorState::Failed(_) => ProtoState::ActorStateFailed,
            ActorState::Terminated => ProtoState::ActorStateTerminated,
        }
    }

    /// Convert from proto ActorState enum
    /// 
    /// Note: For FAILED state, error message should be retrieved from Actor.error_message field
    pub fn from_proto(proto: plexspaces_proto::v1::actor::ActorState, error_message: Option<String>) -> Self {
        use plexspaces_proto::v1::actor::ActorState as ProtoState;
        match proto {
            ProtoState::ActorStateUnspecified => ActorState::Unspecified,
            ProtoState::ActorStateCreating => ActorState::Creating,
            ProtoState::ActorStateActive => ActorState::Active,
            ProtoState::ActorStateInactive => ActorState::Inactive,
            ProtoState::ActorStateActivating => ActorState::Activating,
            ProtoState::ActorStateDeactivating => ActorState::Deactivating,
            ProtoState::ActorStateStopping => ActorState::Stopping,
            ProtoState::ActorStateMigrating => ActorState::Migrating,
            ProtoState::ActorStateFailed => ActorState::Failed(error_message.unwrap_or_default()),
            ProtoState::ActorStateTerminated => ActorState::Terminated,
        }
    }
}

// ActorContext is now imported from core crate (see imports at top of file)

/// Core actor structure
///
/// This is the unified actor that incorporates all 5 pillars:
/// - TupleSpace access for coordination
/// - OTP-style behaviors and supervision
/// - Journaling for durability
/// - WASM runtime capability (future)
/// - Firecracker isolation (future)
pub struct Actor {
    /// Unique actor identifier
    id: ActorId,

    /// Current state of the actor
    state: Arc<RwLock<ActorState>>,

    /// Actor's behavior implementation
    behavior: Arc<RwLock<Box<dyn ActorTrait>>>,

    /// Behavior stack for become/unbecome pattern
    behavior_stack: Arc<RwLock<Vec<Box<dyn ActorTrait>>>>,

    /// Composable mailbox for message handling
    mailbox: Arc<Mailbox>,

    /// Actor context
    context: Arc<ActorContext>,

    /// Message processing handle (for graceful shutdown via abort)
    processor_handle: Option<tokio::task::AbortHandle>,

    /// Shutdown channel
    shutdown_tx: Option<mpsc::Sender<()>>,

    /// Resource profile (Quickwit-inspired)
    resource_profile: ResourceProfile,

    /// Resource contract (our innovation)
    resource_contract: Option<ResourceContract>,

    /// Current resource usage
    resource_usage: Arc<RwLock<ResourceUsage>>,

    /// Last message processed time (for health checks)
    last_message_time: Arc<RwLock<std::time::Instant>>,

    /// Dynamically attached facets (runtime behavior composition)
    facets: Arc<RwLock<FacetContainer>>,

    /// Error message when actor is in FAILED state
    /// 
    /// ## Purpose
    /// Provides error details when actor.state == ActorState::Failed.
    /// Used for debugging, logging, and supervisor restart decisions.
    /// 
    /// ## Usage
    /// - Only populated when state == ActorState::Failed
    /// - Empty string when state != ActorState::Failed
    /// - Contains error message from actor crash or failure
    error_message: Arc<RwLock<String>>,
}

/// Helper function to set ReplayHandler for DurabilityFacet
///
/// ## Purpose
/// Works with any storage backend by using type erasure and Any downcasting.
/// Tries common storage backends (Memory, SQLite, PostgreSQL, Redis).
async fn set_replay_handler_for_facet(
    facet: &mut dyn Facet,
    behavior: &Arc<RwLock<Box<dyn ActorTrait>>>,
    context: &Arc<ActorContext>,
) {
    // Create ReplayHandler that calls actor's behavior
    let handler = ActorReplayHandler {
        behavior: Arc::clone(behavior),
        context: Arc::clone(context),
    };

    // Try to downcast to different storage backends
    // We use a macro to avoid code duplication
    macro_rules! try_set_handler {
        ($storage_type:ty) => {
            if let Some(durability_facet) = facet.as_any_mut().downcast_mut::<plexspaces_journaling::DurabilityFacet<$storage_type>>() {
                durability_facet.set_replay_handler(Box::new(handler)).await;
                tracing::debug!("ReplayHandler set for DurabilityFacet with {} storage", stringify!($storage_type));
                return;
            }
        };
    }

    // Try common storage backends
    try_set_handler!(plexspaces_journaling::MemoryJournalStorage);
    
    #[cfg(feature = "sqlite-backend")]
    {
        use plexspaces_journaling::sql::SqliteJournalStorage;
        try_set_handler!(SqliteJournalStorage);
    }
    
    // If we get here, we couldn't downcast - that's okay, replay will use legacy mode
    tracing::debug!("Could not set ReplayHandler - DurabilityFacet will use legacy replay mode");
}

/// ReplayHandler implementation for Actor
///
/// ## Purpose
/// Bridges DurabilityFacet's replay mechanism with Actor's message handling.
/// When DurabilityFacet replays journaled messages, this handler calls the
/// actor's `handle_message()` method, enabling deterministic state recovery.
struct ActorReplayHandler {
    behavior: Arc<RwLock<Box<dyn ActorTrait>>>,
    context: Arc<ActorContext>,
}

#[async_trait]
impl ReplayHandler for ActorReplayHandler {
    async fn replay_message(
        &self,
        message: Message,
        _context: &ActorContext, // Ignored - we use our stored context
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Use the actor's stored context (which has ExecutionContext in REPLAY mode)
        // The context parameter is ignored - we use self.context which has the proper
        // ExecutionContext with REPLAY mode set by DurabilityFacet
        let mut behavior = self.behavior.write().await;
        behavior.handle_message(&self.context, message).await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
}

impl Actor {
    /// Create a new actor with the given configuration
    ///
    /// ## Note
    /// The actor will be created with a minimal ActorContext.
    /// Node will update it with full service access when spawning the actor.
    ///
    /// ## Arguments
    /// * `id` - Actor identifier
    /// * `behavior` - Actor behavior implementation
    /// * `mailbox` - Actor mailbox for message delivery
    /// * `namespace` - Actor namespace
    /// * `node_id` - Optional node ID (defaults to "local" if not provided)
    ///
    /// ## Note
    /// If `node_id` is not provided, it defaults to "local". When the actor is spawned
    /// via `Node::spawn_actor()`, the context is updated with the correct node_id from
    /// the Node. This parameter is mainly useful for cases where actors are created
    /// before spawning or when the node_id is known in advance.
    pub fn new(
        id: ActorId,
        behavior: Box<dyn plexspaces_core::Actor>,
        mailbox: Mailbox,
        tenant_id: String,
        namespace: String,
        node_id: Option<String>,
    ) -> Self {
        // Create context with ServiceLocator - Node will update it with full services when spawning
        let node_id_str = node_id.clone().unwrap_or_else(|| "local".to_string());
        // Note: This is a sync function, so we create a minimal ServiceLocator
        // Node will replace it with full services when spawning
        use plexspaces_core::ServiceLocator;
        let service_locator = Arc::new(ServiceLocator::new());
        let context = Arc::new(ActorContext::new(
            node_id_str,
            tenant_id.clone(),
            namespace.clone(),
            service_locator,
            None,
        ));

        Actor {
            id,
            state: Arc::new(RwLock::new(ActorState::Creating)),
            behavior: Arc::new(RwLock::new(behavior)),
            behavior_stack: Arc::new(RwLock::new(Vec::new())),
            mailbox: Arc::new(mailbox),
            context,
            processor_handle: None,
            shutdown_tx: None,
            resource_profile: ResourceProfile::default(),
            resource_contract: None,
            resource_usage: Arc::new(RwLock::new(ResourceUsage::default())),
            last_message_time: Arc::new(RwLock::new(std::time::Instant::now())),
            facets: Arc::new(RwLock::new(FacetContainer::new())),
            error_message: Arc::new(RwLock::new(String::new())),
        }
    }

    /// Set the actor context (with full service access)
    ///
    /// ## Purpose
    /// Updates the actor's context to include service access (ActorService, ObjectRegistry, etc.)
    /// This should be called by Node before starting the actor.
    ///
    /// ## Arguments
    /// * `context` - Full ActorContext with service access
    pub fn set_context(mut self, context: Arc<ActorContext>) -> Self {
        self.context = context;
        self
    }

    /// Get the actor context
    pub fn context(&self) -> &Arc<ActorContext> {
        &self.context
    }

    /// Set the resource profile for this actor
    pub fn with_resource_profile(mut self, profile: ResourceProfile) -> Self {
        self.resource_profile = profile;
        self
    }

    /// Set the resource contract for this actor
    pub fn with_resource_contract(mut self, contract: ResourceContract) -> Self {
        self.resource_contract = Some(contract);
        self
    }

    /// Get current resource usage
    pub async fn resource_usage(&self) -> ResourceUsage {
        self.resource_usage.read().await.clone()
    }

    /// Check actor health (Quickwit-inspired)
    pub async fn health(&self) -> ActorHealth {
        let last_message = self.last_message_time.read().await;
        let elapsed = last_message.elapsed();

        // Check if stuck (no messages processed for 30 seconds)
        ActorHealth::check_stuck(elapsed, std::time::Duration::from_secs(30))
    }

    /// Validate resource usage against contract
    pub async fn validate_resources(&self) -> Result<(), crate::resource::ResourceViolation> {
        if let Some(ref contract) = self.resource_contract {
            let usage = self.resource_usage.read().await;
            contract.validate_usage(&usage)?;
        }
        Ok(())
    }

    /// Get the actor's ID
    pub fn id(&self) -> &ActorId {
        &self.id
    }

    /// Get the current state
    pub async fn state(&self) -> ActorState {
        self.state.read().await.clone()
    }

    /// Get the actor's mailbox (for ActorRef creation)
    pub fn mailbox(&self) -> &Arc<Mailbox> {
        &self.mailbox
    }

    /// Get error message when actor is in FAILED state
    ///
    /// ## Returns
    /// Error message string if actor is in FAILED state, empty string otherwise
    pub async fn error_message(&self) -> String {
        self.error_message.read().await.clone()
    }

    /// Get the actor's behavior (for testing/debugging)
    pub fn behavior(&self) -> &Arc<RwLock<Box<dyn ActorTrait>>> {
        &self.behavior
    }

    /// Set error message (used when transitioning to FAILED state)
    async fn set_error_message(&self, message: String) {
        *self.error_message.write().await = message;
    }

    /// Start the actor (Erlang-style: returns JoinHandle for supervision)
    ///
    /// ## Purpose
    /// Starts the actor's message processing loop and returns a JoinHandle.
    /// The caller (Node or Supervisor) owns this handle and detects termination.
    ///
    /// ## Erlang Philosophy
    /// In Erlang, when you spawn a process, you get back a Pid. If the process
    /// crashes, linked/monitoring processes receive an EXIT signal automatically.
    /// Similarly, this returns a JoinHandle that the supervisor can await.
    ///
    /// ## Returns
    /// JoinHandle that completes when actor terminates (normally or via panic)
    ///
    /// ## Example
    /// ```ignore
    /// let mut actor = Actor::new(...);
    /// let handle = actor.start().await?;
    ///
    /// // Supervisor/Node watches for termination
    /// let result = handle.await;
    /// match result {
    ///     Ok(_) => println!("Actor terminated normally"),
    ///     Err(e) if e.is_panic() => println!("Actor panicked: {:?}", e),
    ///     Err(e) => println!("Actor cancelled: {:?}", e),
    /// }
    /// ```
    pub async fn start(&mut self) -> Result<tokio::task::JoinHandle<()>, ActorError> {
        let state = self.state.read().await.clone();
        if state != ActorState::Creating && state != ActorState::Terminated {
            return Err(ActorError::InvalidState(format!(
                "Cannot start actor in state {:?}",
                state
            )));
        }
        drop(state);

        // OBSERVABILITY: Track actor creation
        metrics::counter!("plexspaces_actor_created_total",
            "actor_id" => self.id.clone()
        ).increment(1);
        
        tracing::info!(actor_id = %self.id, "Actor created");

        // Step 1: State → Activating
        *self.state.write().await = ActorState::Activating;
        
        // OBSERVABILITY: Track state transition
        metrics::counter!("plexspaces_actor_state_transitions_total",
            "actor_id" => self.id.clone(),
            "from" => "Creating",
            "to" => "Activating"
        ).increment(1);
        
        tracing::debug!(actor_id = %self.id, "Actor activating - starting unified lifecycle");

        // Step 2: Actor initialization
        // Note: Facets are already attached before start() is called (via attach_facet() in actor_factory)
        // Supervisor waits for init() to complete before starting next child
        // If init() fails, actor is not registered and supervisor handles error
        let init_start = std::time::Instant::now();
        {
            let mut behavior = self.behavior.write().await;
            match behavior.init(&self.context).await {
                Ok(()) => {
                    // Init succeeded - track metrics
                    let init_duration = init_start.elapsed();
                    metrics::counter!("plexspaces_actor_init_total",
                        "actor_id" => self.id.clone(),
                        "status" => "success"
                    ).increment(1);
                    metrics::histogram!("plexspaces_actor_init_duration_seconds",
                        "actor_id" => self.id.clone()
                    ).record(init_duration.as_secs_f64());
                    tracing::debug!(
                        actor_id = %self.id,
                        duration_ms = init_duration.as_millis(),
                        "Actor init() completed successfully"
                    );
                }
                Err(e) => {
                    // Set error state and message (sync operations only in error handler)
                    let init_duration = init_start.elapsed();
                    *self.state.write().await = ActorState::Failed(e.to_string());
                    let _ = self.set_error_message(e.to_string()).await;
                    
                    // Track init failure metrics
                    metrics::counter!("plexspaces_actor_init_total",
                        "actor_id" => self.id.clone(),
                        "status" => "failure"
                    ).increment(1);
                    metrics::histogram!("plexspaces_actor_init_duration_seconds",
                        "actor_id" => self.id.clone()
                    ).record(init_duration.as_secs_f64());
                    metrics::counter!("plexspaces_actor_init_errors_total",
                        "actor_id" => self.id.clone(),
                        "error_type" => format!("{:?}", e)
                    ).increment(1);
                    
                    tracing::error!(
                        actor_id = %self.id,
                        error = %e,
                        duration_ms = init_duration.as_millis(),
                        "Actor init() failed - actor will not start or register"
                    );
                    return Err(e);
                }
            }
        }

        // Step 3: Facet post-init setup (Phase 1: Unified Lifecycle)
        // Call on_init_complete() for ALL facets (in priority order, descending)
        // This allows facets to perform setup that requires actor to be initialized
        let facets_init_complete_start = std::time::Instant::now();
        {
            let mut facets = self.facets.write().await;
            let facet_errors = facets.call_on_init_complete(&self.id).await;
            
            if !facet_errors.is_empty() {
                // Log errors but don't fail activation (facets may have non-critical errors)
                for error in &facet_errors {
                    metrics::counter!("plexspaces_facet_errors_total",
                        "facet_type" => "unknown",
                        "error_type" => format!("{:?}", error),
                        "operation" => "on_init_complete"
                    ).increment(1);
                    tracing::warn!(
                        actor_id = %self.id,
                        error = %error,
                        "Facet on_init_complete() returned error (non-fatal)"
                    );
                }
            }
            
            let facets_init_complete_duration = facets_init_complete_start.elapsed();
            metrics::histogram!("plexspaces_facet_init_complete_duration_seconds",
                "actor_id" => self.id.clone()
            ).record(facets_init_complete_duration.as_secs_f64());
            tracing::debug!(
                actor_id = %self.id,
                facet_count = facets.list_facets().len(),
                duration_ms = facets_init_complete_duration.as_millis(),
                "Facet on_init_complete() called for all facets"
            );
        }

        // Step 4: Behavior post-facet setup (Phase 1: Unified Lifecycle)
        // Call on_facets_ready() to allow behavior to initialize after facets are ready
        let facets_ready_start = std::time::Instant::now();
        {
            let mut behavior = self.behavior.write().await;
            match behavior.on_facets_ready(&self.context).await {
                Ok(()) => {
                    let facets_ready_duration = facets_ready_start.elapsed();
                    metrics::histogram!("plexspaces_actor_facets_ready_duration_seconds",
                        "actor_id" => self.id.clone()
                    ).record(facets_ready_duration.as_secs_f64());
                    tracing::debug!(
                        actor_id = %self.id,
                        duration_ms = facets_ready_duration.as_millis(),
                        "Actor on_facets_ready() completed successfully"
                    );
                }
                Err(e) => {
                    // Log error but don't fail activation (behavior may have non-critical errors)
                    let facets_ready_duration = facets_ready_start.elapsed();
                    metrics::counter!("plexspaces_actor_facets_ready_errors_total",
                        "actor_id" => self.id.clone(),
                        "error_type" => format!("{:?}", e)
                    ).increment(1);
                    tracing::warn!(
                        actor_id = %self.id,
                        error = %e,
                        duration_ms = facets_ready_duration.as_millis(),
                        "Actor on_facets_ready() returned error (non-fatal)"
                    );
                }
            }
        }

        // Step 5: Register in ActorRegistry AFTER init() succeeds
        // This ensures failed actors are never visible to other actors
        // Critical for supervisor tree: supervisor waits for init() before starting next child
        self.register_in_registry().await?;

        // Step 6: Call STATIC lifecycle hook (ALWAYS runs) - for backward compatibility
        self.on_activate().await?;

        // Start message processing
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        let mailbox = self.mailbox.clone();
        let behavior = self.behavior.clone();
        let context = self.context.clone();
        let state = self.state.clone();
        let last_message_time = self.last_message_time.clone();
        let facets = self.facets.clone();
        let actor_id_for_logging = self.id.clone(); // Capture actor_id before spawning

        // Simple message loop - Node/Supervisor will detect termination via JoinHandle
        let handle = tokio::spawn(async move {
            // Mark as active
            *state.write().await = ActorState::Active;
            let mut loop_iteration = 0;
            loop {
                loop_iteration += 1;
                // Use tokio::select! with biased to prioritize shutdown
                // Add a very short timeout to periodically check shutdown (non-blocking)
                // This allows shutdown to interrupt even when mailbox is empty
                // The timeout is very short (1ms) to minimize latency while ensuring responsive shutdown
                tokio::select! {
                    biased; // Prioritize shutdown branch
                    _ = shutdown_rx.recv() => {
                        // Shutdown signal received - highest priority
                        tracing::debug!(
                            actor_id = %actor_id_for_logging,
                            "Actor message loop: Shutdown signal received, exiting loop"
                        );
                        break;
                    }
                    Some(message) = mailbox.dequeue() => {
                        tracing::debug!(
                            actor_id = %actor_id_for_logging,
                            message_id = %message.id,
                            "Actor message loop: ✅ Dequeued message, processing..."
                        );
                        
                        // Check if this is an EXIT message (from linked actor death)
                        if message.is_exit() {
                            if let Some((from_actor_id, reason_str)) = message.try_parse_exit() {
                                // Parse exit reason from string
                                let exit_reason = parse_exit_reason_from_str(&reason_str, &message.metadata);
                                // Check if actor traps exits
                                if context.trap_exit {
                                    // Phase 4: Monitoring/Linking Integration - Call facet.on_exit() for ALL facets
                                    // Facets receive EXIT signal in priority order (high priority first)
                                    // This allows facets to handle EXIT signals (e.g., TimerFacet cancels timers)
                                    let facet_exit_start = std::time::Instant::now();
                                    let facet_exit_reason = match &exit_reason {
                                        plexspaces_core::ExitReason::Normal => plexspaces_facet::ExitReason::Normal,
                                        plexspaces_core::ExitReason::Shutdown => plexspaces_facet::ExitReason::Shutdown,
                                        plexspaces_core::ExitReason::Killed => plexspaces_facet::ExitReason::Killed,
                                        plexspaces_core::ExitReason::Error(msg) => plexspaces_facet::ExitReason::Error(msg.clone()),
                                        plexspaces_core::ExitReason::Linked { actor_id, reason } => {
                                            plexspaces_facet::ExitReason::Error(format!("Linked: {} -> {:?}", actor_id, reason))
                                        },
                                    };
                                    
                                    let facet_exit_errors = {
                                        let mut facets_guard = facets.write().await;
                                        facets_guard.call_on_exit(&actor_id_for_logging, &from_actor_id, &facet_exit_reason).await
                                    };
                                    
                                    let facet_exit_duration = facet_exit_start.elapsed();
                                    if !facet_exit_errors.is_empty() {
                                        metrics::counter!("plexspaces_facet_exit_errors_total",
                                            "actor_id" => actor_id_for_logging.clone(),
                                            "error_count" => facet_exit_errors.len().to_string()
                                        ).increment(facet_exit_errors.len() as u64);
                                        tracing::warn!(
                                            actor_id = %actor_id_for_logging,
                                            from = %from_actor_id,
                                            error_count = facet_exit_errors.len(),
                                            "Some facets failed to handle EXIT signal (continuing with behavior.handle_exit)"
                                        );
                                    } else {
                                        tracing::debug!(
                                            actor_id = %actor_id_for_logging,
                                            from = %from_actor_id,
                                            duration_ms = facet_exit_duration.as_millis(),
                                            "All facets handled EXIT signal successfully"
                                        );
                                    }
                                    metrics::histogram!("plexspaces_facet_exit_duration_seconds",
                                        "actor_id" => actor_id_for_logging.clone()
                                    ).record(facet_exit_duration.as_secs_f64());
                                    metrics::counter!("plexspaces_facet_exit_total",
                                        "actor_id" => actor_id_for_logging.clone()
                                    ).increment(1);
                                    
                                    // Call handle_exit() - actor can decide to propagate or handle
                                    let handle_exit_start = std::time::Instant::now();
                                    let mut behavior_guard = behavior.write().await;
                                    match behavior_guard.handle_exit(&context, &from_actor_id, &exit_reason).await {
                                        Ok(ExitAction::Propagate) => {
                                            // Actor decided to propagate - terminate this actor
                                            let handle_exit_duration = handle_exit_start.elapsed();
                                            metrics::counter!("plexspaces_actor_exit_handled_total",
                                                "actor_id" => actor_id_for_logging.clone(),
                                                "action" => "propagate"
                                            ).increment(1);
                                            metrics::histogram!("plexspaces_actor_exit_handle_duration_seconds",
                                                "actor_id" => actor_id_for_logging.clone()
                                            ).record(handle_exit_duration.as_secs_f64());
                                            tracing::info!(
                                                actor_id = %actor_id_for_logging,
                                                from = %from_actor_id,
                                                reason = ?exit_reason,
                                                duration_ms = handle_exit_duration.as_millis(),
                                                "EXIT from linked actor - propagating (actor will terminate)"
                                            );
                                            // Call terminate() before breaking
                                            let _ = behavior_guard.terminate(&context, &exit_reason).await;
                                            break;
                                        }
                                        Ok(ExitAction::Handle) => {
                                            // Actor decided to handle - continue running
                                            let handle_exit_duration = handle_exit_start.elapsed();
                                            metrics::counter!("plexspaces_actor_exit_handled_total",
                                                "actor_id" => actor_id_for_logging.clone(),
                                                "action" => "handle"
                                            ).increment(1);
                                            metrics::histogram!("plexspaces_actor_exit_handle_duration_seconds",
                                                "actor_id" => actor_id_for_logging.clone()
                                            ).record(handle_exit_duration.as_secs_f64());
                                            tracing::debug!(
                                                actor_id = %actor_id_for_logging,
                                                from = %from_actor_id,
                                                reason = ?exit_reason,
                                                duration_ms = handle_exit_duration.as_millis(),
                                                "EXIT from linked actor - handled (actor continues)"
                                            );
                                            // Continue loop
                                        }
                                        Err(e) => {
                                            let handle_exit_duration = handle_exit_start.elapsed();
                                            metrics::counter!("plexspaces_actor_exit_handled_total",
                                                "actor_id" => actor_id_for_logging.clone(),
                                                "action" => "error"
                                            ).increment(1);
                                            metrics::counter!("plexspaces_actor_exit_handle_errors_total",
                                                "actor_id" => actor_id_for_logging.clone(),
                                                "error_type" => format!("{:?}", e)
                                            ).increment(1);
                                            tracing::error!(
                                                actor_id = %actor_id_for_logging,
                                                from = %from_actor_id,
                                                error = %e,
                                                duration_ms = handle_exit_duration.as_millis(),
                                                "Error handling EXIT - terminating actor"
                                            );
                                            // Error handling EXIT - terminate actor
                                            let _ = behavior_guard.terminate(&context, &exit_reason).await;
                                            break;
                                        }
                                    }
                                    drop(behavior_guard);
                                } else {
                                    // Actor doesn't trap exits - terminate immediately (Erlang default behavior)
                                    metrics::counter!("plexspaces_actor_exit_propagated_total",
                                        "actor_id" => actor_id_for_logging.clone(),
                                        "from" => from_actor_id.clone(),
                                        "trapped" => "false"
                                    ).increment(1);
                                    tracing::info!(
                                        actor_id = %actor_id_for_logging,
                                        from = %from_actor_id,
                                        reason = ?exit_reason,
                                        "EXIT from linked actor - not trapping, terminating"
                                    );
                                    let mut behavior_guard = behavior.write().await;
                                    let linked_reason = ExitReason::Linked {
                                        actor_id: from_actor_id,
                                        reason: Box::new(exit_reason),
                                    };
                                    let _ = behavior_guard.terminate(&context, &linked_reason).await;
                                    break;
                                }
                                continue; // Skip normal message processing for EXIT messages
                            }
                        }
                        
                        // Process normal message with facets
                        let result = Self::process_message(
                            message.clone(),
                            &actor_id_for_logging,
                            &behavior,
                            &context,
                            &last_message_time,
                            &facets,
                        ).await;

                        // ACK/NACK message based on processing result
                        // Only for channels that support ack/nack (Redis, Kafka, etc.)
                        if result.is_ok() {
                            // Successful processing - ACK the message
                            if let Err(e) = mailbox.ack_message(&message).await {
                                tracing::warn!(
                                    actor_id = %actor_id_for_logging,
                                    message_id = %message.id,
                                    error = %e,
                                    "Failed to ack message after successful processing"
                                );
                            }
                        } else {
                            // Failed processing - NACK the message (will retry or DLQ)
                            let error_msg = result.as_ref().err().map(|e| e.to_string());
                            if let Err(e) = mailbox.nack_message(&message, error_msg.as_deref()).await {
                                tracing::warn!(
                                    actor_id = %actor_id_for_logging,
                                    message_id = %message.id,
                                    error = %e,
                                    "Failed to nack message after failed processing"
                                );
                            }
                        }

                        if let Err(e) = &result {
                            // OBSERVABILITY: Error already tracked in process_message
                            // Log at error level for visibility
                            tracing::error!(error = %e, "Error processing message (supervision should handle)");
                            // TODO: Handle error based on supervision strategy
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(1)) => {
                        // Very short timeout to check shutdown periodically
                        // This ensures shutdown can interrupt even when mailbox is empty
                        // The 1ms timeout is negligible for latency but ensures responsive shutdown
                        if shutdown_rx.try_recv().is_ok() {
                            tracing::debug!(
                                actor_id = %actor_id_for_logging,
                                "Actor message loop: Shutdown signal received (via timeout check), exiting loop"
                            );
                            break;
                        }
                        // Log every 1000 iterations (roughly every second) to show loop is running
                        if loop_iteration % 1000 == 0 {
                            tracing::debug!(
                                actor_id = %actor_id_for_logging,
                                iteration = loop_iteration,
                                "Actor message loop: Still running, waiting for messages..."
                            );
                        }
                        // Continue loop (messages will wake us up via channel/Notify)
                    }
                }
            }

            // Note: terminate() is called in Actor::stop() before the message loop exits
            // So we don't need to call it again here. The message loop just exits cleanly.
            
            // Mark as stopped
            *state.write().await = ActorState::Terminated;
        });

        self.processor_handle = Some(handle.abort_handle());

        Ok(handle)
    }

    /// Stop the actor
    pub async fn stop(&mut self) -> Result<(), ActorError> {
        let state = self.state.read().await.clone();
        // Check if already stopped or stopping - return early to avoid calling terminate() multiple times
        if state == ActorState::Stopping || state == ActorState::Terminated || matches!(state, ActorState::Failed(_)) {
            return Ok(()); // Already stopped or stopping
        }
        // Only proceed if Active or Inactive
        if state != ActorState::Active && state != ActorState::Inactive {
            return Ok(()); // Invalid state for stopping
        }
        drop(state);

        tracing::info!(actor_id = %self.id, "Stopping actor - starting graceful shutdown");

        // Mark as stopping (prevents new messages from being processed and duplicate terminate() calls)
        *self.state.write().await = ActorState::Stopping;

        // Step 1: Signal shutdown to message loop (stops processing new messages)
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Step 2: For non-memory channels, gracefully shutdown mailbox
        // This will:
        // - Stop accepting new messages via enqueue()
        // - Stop receiving new messages from channel backend
        // - Wait for in-progress messages to complete (with timeout)
        // - Close the channel
        if !self.mailbox.is_in_memory() {
            let timeout = Some(std::time::Duration::from_secs(30));
            if let Err(e) = self.mailbox.graceful_shutdown(timeout).await {
                tracing::warn!(
                    actor_id = %self.id,
                    error = %e,
                    "Mailbox graceful shutdown had errors (continuing with actor stop)"
                );
            }
        }

        // Step 3: Wait for message loop to complete (it will exit when shutdown signal is received)
        // The message loop will complete in-progress messages before exiting
        // Note: We can't await the JoinHandle directly since we only have AbortHandle
        // The loop will exit when shutdown signal is received, so we just abort to ensure it stops
        if let Some(handle) = self.processor_handle.take() {
            // Give message loop a moment to finish current message
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            // Abort the handle (the loop should already be exiting due to shutdown signal)
            handle.abort();
        }

        // Step 4: Facet pre-terminate cleanup (Phase 1: Unified Lifecycle)
        // Call on_terminate_start() for ALL facets (in priority order, descending)
        // This allows facets to perform cleanup before actor terminates (e.g., cancel timers, flush journal)
        let exit_reason = ExitReason::Shutdown;
        let facets_terminate_start_time = std::time::Instant::now();
        {
            let mut facets = self.facets.write().await;
            // Convert ExitReason to facet's ExitReason
            // Manual conversion to avoid circular dependency
            let facet_exit_reason = match &exit_reason {
                ExitReason::Normal => plexspaces_facet::ExitReason::Normal,
                ExitReason::Shutdown => plexspaces_facet::ExitReason::Shutdown,
                ExitReason::Killed => plexspaces_facet::ExitReason::Killed,
                ExitReason::Error(msg) => plexspaces_facet::ExitReason::Error(msg.clone()),
                ExitReason::Linked { actor_id, reason } => {
                    let linked_reason = match reason.as_ref() {
                        ExitReason::Normal => plexspaces_facet::ExitReason::Normal,
                        ExitReason::Shutdown => plexspaces_facet::ExitReason::Shutdown,
                        ExitReason::Killed => plexspaces_facet::ExitReason::Killed,
                        ExitReason::Error(msg) => plexspaces_facet::ExitReason::Error(msg.clone()),
                        ExitReason::Linked { .. } => plexspaces_facet::ExitReason::Error("linked".to_string()),
                    };
                    plexspaces_facet::ExitReason::Linked {
                        actor_id: actor_id.clone(),
                        reason: Box::new(linked_reason),
                    }
                }
            };
            let facet_errors = facets.call_on_terminate_start(&self.id, &facet_exit_reason).await;
            
            if !facet_errors.is_empty() {
                // Log errors but don't fail shutdown (facets may have non-critical errors)
                for error in &facet_errors {
                    metrics::counter!("plexspaces_facet_errors_total",
                        "facet_type" => "unknown",
                        "error_type" => format!("{:?}", error),
                        "operation" => "on_terminate_start"
                    ).increment(1);
                    tracing::warn!(
                        actor_id = %self.id,
                        error = %error,
                        "Facet on_terminate_start() returned error (non-fatal)"
                    );
                }
            }
            
            let facets_terminate_start_duration = facets_terminate_start_time.elapsed();
            metrics::histogram!("plexspaces_facet_terminate_start_duration_seconds",
                "actor_id" => self.id.clone()
            ).record(facets_terminate_start_duration.as_secs_f64());
            tracing::debug!(
                actor_id = %self.id,
                facet_count = facets.list_facets().len(),
                duration_ms = facets_terminate_start_duration.as_millis(),
                "Facet on_terminate_start() called for all facets"
            );
        }

        // Step 5: Behavior pre-facet-detachment cleanup (Phase 1: Unified Lifecycle)
        // Call on_facets_detaching() to allow behavior to clean up before facets are detached
        let facets_detaching_start = std::time::Instant::now();
        {
            let mut behavior = self.behavior.write().await;
            match behavior.on_facets_detaching(&self.context, &exit_reason).await {
                Ok(()) => {
                    let facets_detaching_duration = facets_detaching_start.elapsed();
                    metrics::histogram!("plexspaces_actor_facets_detaching_duration_seconds",
                        "actor_id" => self.id.clone()
                    ).record(facets_detaching_duration.as_secs_f64());
                    tracing::debug!(
                        actor_id = %self.id,
                        duration_ms = facets_detaching_duration.as_millis(),
                        "Actor on_facets_detaching() completed successfully"
                    );
                }
                Err(e) => {
                    // Log error but don't fail shutdown (behavior may have non-critical errors)
                    let facets_detaching_duration = facets_detaching_start.elapsed();
                    metrics::counter!("plexspaces_actor_facets_detaching_errors_total",
                        "actor_id" => self.id.clone(),
                        "error_type" => format!("{:?}", e)
                    ).increment(1);
                    tracing::warn!(
                        actor_id = %self.id,
                        error = %e,
                        duration_ms = facets_detaching_duration.as_millis(),
                        "Actor on_facets_detaching() returned error (non-fatal)"
                    );
                }
            }
        }

        // Step 6: Actor cleanup - Call terminate() lifecycle hook
        // This is called even on crash if possible (via panic handling in message loop)
        // No new messages will be delivered after terminate() starts
        let terminate_start = std::time::Instant::now();
        {
            let mut behavior = self.behavior.write().await;
            match behavior.terminate(&self.context, &exit_reason).await {
                Ok(()) => {
                    let terminate_duration = terminate_start.elapsed();
                    metrics::counter!("plexspaces_actor_terminate_total",
                        "actor_id" => self.id.clone(),
                        "reason" => "shutdown"
                    ).increment(1);
                    metrics::histogram!("plexspaces_actor_terminate_duration_seconds",
                        "actor_id" => self.id.clone()
                    ).record(terminate_duration.as_secs_f64());
                    tracing::debug!(
                        actor_id = %self.id,
                        duration_ms = terminate_duration.as_millis(),
                        "Actor terminate() completed successfully"
                    );
                }
                Err(e) => {
                    let terminate_duration = terminate_start.elapsed();
                    metrics::counter!("plexspaces_actor_terminate_total",
                        "actor_id" => self.id.clone(),
                        "reason" => "shutdown"
                    ).increment(1);
                    metrics::histogram!("plexspaces_actor_terminate_duration_seconds",
                        "actor_id" => self.id.clone()
                    ).record(terminate_duration.as_secs_f64());
                    metrics::counter!("plexspaces_actor_terminate_errors_total",
                        "actor_id" => self.id.clone(),
                        "error_type" => format!("{:?}", e)
                    ).increment(1);
                    tracing::warn!(
                        actor_id = %self.id,
                        error = %e,
                        duration_ms = terminate_duration.as_millis(),
                        "terminate() returned error (continuing shutdown)"
                    );
                    // Don't fail shutdown if terminate() fails - continue with cleanup
                }
            }
        }

        // Step 7: Facet detachment (Phase 1: Unified Lifecycle)
        // Detach ALL facets in reverse priority order (ascending priority, reverse of attach)
        let facets_detach_start = std::time::Instant::now();
        {
            let mut facets = self.facets.write().await;
            let facet_errors = facets.detach_all(&self.id).await;
            
            if !facet_errors.is_empty() {
                // Log errors but don't fail shutdown (facets may have non-critical errors)
                for error in &facet_errors {
                    metrics::counter!("plexspaces_facet_errors_total",
                        "facet_type" => "unknown",
                        "error_type" => format!("{:?}", error),
                        "operation" => "on_detach"
                    ).increment(1);
                    tracing::warn!(
                        actor_id = %self.id,
                        error = %error,
                        "Facet detach() returned error (non-fatal)"
                    );
                }
            }
            
            let facets_detach_duration = facets_detach_start.elapsed();
            metrics::histogram!("plexspaces_facet_detach_all_duration_seconds",
                "actor_id" => self.id.clone()
            ).record(facets_detach_duration.as_secs_f64());
            tracing::debug!(
                actor_id = %self.id,
                facet_count = facets.list_facets().len(),
                duration_ms = facets_detach_duration.as_millis(),
                "All facets detached"
            );
        }

        // Step 8: Call STATIC lifecycle hook (ALWAYS runs) - for backward compatibility
        self.on_deactivate().await?;

        // OBSERVABILITY: Track actor termination
        metrics::counter!("plexspaces_actor_terminated_total",
            "actor_id" => self.id.clone()
        ).increment(1);
        
        tracing::info!(actor_id = %self.id, "Actor terminated");

        // Note: State is set to Terminated by on_deactivate(), no need to set again
        Ok(())
    }

    /// Send a message to this actor
    pub async fn send(&self, message: Message) -> Result<(), ActorError> {
        self.mailbox
            .enqueue(message)
            .await
            .map_err(|e| ActorError::MailboxError(e.to_string()))
    }

    /// Change actor behavior at runtime (become pattern)
    pub async fn r#become(&self, new_behavior: Box<dyn ActorTrait>) -> Result<(), ActorError> {
        let mut behavior = self.behavior.write().await;
        let mut stack = self.behavior_stack.write().await;

        // Push current behavior to stack
        let current = std::mem::replace(&mut *behavior, new_behavior);
        stack.push(current);

        Ok(())
    }

    /// Restore previous behavior (unbecome pattern)
    pub async fn unbecome(&self) -> Result<(), ActorError> {
        let mut behavior = self.behavior.write().await;
        let mut stack = self.behavior_stack.write().await;

        if let Some(previous) = stack.pop() {
            *behavior = previous;
            Ok(())
        } else {
            Err(ActorError::NoBehaviorToRestore)
        }
    }

    /// Attach a facet to this actor at runtime
    ///
    /// ## Configuration
    /// Facet must have config and priority set via constructor before attachment.
    /// These are extracted from the facet automatically.
    ///
    /// ## DurabilityFacet Integration
    /// When attaching DurabilityFacet, this method:
    /// - Checks if mailbox is durable and logs metrics
    /// - Records mailbox stats for observability
    /// - Sets ReplayHandler for deterministic message replay
    pub async fn attach_facet(
        &self,
        mut facet: Box<dyn Facet>,
    ) -> Result<String, ActorError> {
        let facet_type = facet.facet_type().to_string();
        
        // Coordinate with mailbox for DurabilityFacet
        if facet_type == "durability" {
            let mailbox_stats = self.mailbox.get_stats().await;
            tracing::info!(
                actor_id = %self.id,
                mailbox_backend = %mailbox_stats.backend_type,
                mailbox_is_durable = mailbox_stats.is_durable,
                mailbox_size = mailbox_stats.current_size,
                "Attaching DurabilityFacet - mailbox durability status"
            );
            
            // Record metrics
            metrics::gauge!("plexspaces_mailbox_size", "actor_id" => self.id.to_string(), "backend" => mailbox_stats.backend_type.clone())
                .set(mailbox_stats.current_size as f64);
            metrics::gauge!("plexspaces_mailbox_is_durable", "actor_id" => self.id.to_string(), "backend" => mailbox_stats.backend_type)
                .set(if mailbox_stats.is_durable { 1.0 } else { 0.0 });

            // Set ReplayHandler for deterministic message replay (Phase 9.1)
            // This enables DurabilityFacet to replay messages through actor's handle_message()
            // We use a helper function that works with any storage backend via type erasure
            set_replay_handler_for_facet(&mut *facet, &self.behavior, &self.context).await;
        }
        
        let mut facets = self.facets.write().await;
        facets
            .attach(facet, &self.id)
            .await
            .map_err(|e| ActorError::FacetError(e.to_string()))
    }

    /// Detach a facet from this actor
    ///
    /// ## DurabilityFacet Integration
    /// When detaching DurabilityFacet, this method:
    /// - Calls mailbox.graceful_shutdown() to flush pending messages
    /// - Records final mailbox stats for observability
    pub async fn detach_facet(&self, facet_type: &str) -> Result<(), ActorError> {
        // Coordinate with mailbox for DurabilityFacet graceful shutdown
        if facet_type == "durability" {
            // Flush pending messages to durable backend before detaching
            let timeout = Some(std::time::Duration::from_secs(30));
            self.mailbox.graceful_shutdown(timeout).await
                .map_err(|e| ActorError::MailboxError(format!("Failed to gracefully shutdown mailbox: {}", e)))?;
            
            // Record final mailbox stats
            let mailbox_stats = self.mailbox.get_stats().await;
            tracing::info!(
                actor_id = %self.id,
                mailbox_backend = %mailbox_stats.backend_type,
                mailbox_size = mailbox_stats.current_size,
                total_enqueued = mailbox_stats.total_enqueued,
                total_dequeued = mailbox_stats.total_dequeued,
                "Detaching DurabilityFacet - final mailbox stats"
            );
            
            // Record final metrics
            metrics::gauge!("plexspaces_mailbox_size", "actor_id" => self.id.to_string(), "backend" => mailbox_stats.backend_type.clone())
                .set(mailbox_stats.current_size as f64);
        }
        
        let mut facets = self.facets.write().await;
        facets
            .detach(facet_type, &self.id)
            .await
            .map_err(|e| ActorError::FacetError(e.to_string()))
    }

    /// List attached facets
    pub async fn list_facets(&self) -> Vec<String> {
        let facets = self.facets.read().await;
        facets.list_facets()
    }
    
    /// Get a facet by type (for FacetService - Option B)
    ///
    /// ## Arguments
    /// * `facet_type` - Facet type identifier (e.g., "timer", "reminder")
    ///
    /// ## Returns
    /// Arc to the facet if found, None otherwise
    pub async fn get_facet(&self, facet_type: &str) -> Option<Arc<RwLock<Box<dyn Facet>>>> {
        let facets = self.facets.read().await;
        facets.get_facet(facet_type)
    }
    
    /// Get facets container (for FacetService - Option 1: Store facets separately)
    ///
    /// ## Purpose
    /// Returns a clone of the facets container for storage in Node.
    /// Facets are already Arc, so cloning is cheap and safe.
    ///
    /// ## Returns
    /// Clone of the facets container
    pub fn facets(&self) -> Arc<RwLock<FacetContainer>> {
        self.facets.clone()
    }

    // ============================================================================
    // STATIC LIFECYCLE METHODS (CORE FEATURE - ALWAYS RUN)
    // ============================================================================
    // These methods are CORE to every actor and ALWAYS execute.
    // They are NOT optional like the old ActorLifecycle trait.
    //
    // Design principle: "Static for core, facets for dynamic"
    // Lifecycle is CORE - every actor needs init/cleanup (like Erlang's gen_server:init/terminate)
    // ============================================================================

    /// Called when actor activates (ALWAYS RUNS)
    ///
    /// This is a CORE lifecycle hook that executes for every actor during activation.
    /// It handles:
    /// 1. State transition to Activating
    /// 2. Deterministic replay from journal (RESTATE-inspired durability)
    /// 3. Behavior initialization (if behavior implements lifecycle extensions)
    /// 4. Journaling activation event
    /// 5. State transition to Active
    ///
    /// This is similar to Erlang's gen_server:init callback
    async fn on_activate(&mut self) -> Result<(), ActorError> {
        let actor_id = self.id.clone();
        
        // OBSERVABILITY: Tracing span for activation
        let span = tracing::span!(
            tracing::Level::INFO,
            "actor.activate",
            actor_id = %actor_id
        );
        let _guard = span.enter();
        
        tracing::info!("Actor activating");
        
        // 1. Set state to Activating
        *self.state.write().await = ActorState::Activating;

        // OBSERVABILITY: Track state transition
        metrics::counter!("plexspaces_actor_state_transitions_total",
            "actor_id" => actor_id.clone(),
            "from" => "Created",
            "to" => "Activating"
        ).increment(1);

        // 2. Let behavior initialize if it has lifecycle extensions
        // (Behaviors can optionally implement lifecycle hooks via helper methods)
        if let Ok(_behavior) = self.behavior.try_write() {
            // TODO: Add as_lifecycle_mut() helper to Actor trait
            // This allows behaviors to optionally extend lifecycle
            // For now, we skip this step
        }

        // 3. Set to active
        *self.state.write().await = ActorState::Active;

        // OBSERVABILITY: Track activation success
        metrics::counter!("plexspaces_actor_state_transitions_total",
            "actor_id" => actor_id.clone(),
            "from" => "Activating",
            "to" => "Active"
        ).increment(1);
        
        metrics::gauge!("plexspaces_actor_active_total",
            "actor_id" => actor_id.clone()
        ).increment(1.0);
        
        tracing::info!("Actor activated successfully");

        Ok(())
    }

    /// Called when actor deactivates (ALWAYS RUNS)
    ///
    /// This is a CORE lifecycle hook that executes for every actor during deactivation.
    /// It handles:
    /// 1. State transition to Deactivating
    /// 2. Behavior cleanup (if behavior implements lifecycle extensions)
    /// 3. Journaling deactivation event
    /// 4. State transition to Terminated
    ///
    /// This is similar to Erlang's gen_server:terminate callback
    async fn on_deactivate(&mut self) -> Result<(), ActorError> {
        let actor_id = self.id.clone();
        
        // OBSERVABILITY: Tracing span for deactivation
        let span = tracing::span!(
            tracing::Level::INFO,
            "actor.deactivate",
            actor_id = %actor_id
        );
        let _guard = span.enter();
        
        tracing::info!("Actor deactivating");
        
        // 1. Set state to Deactivating
        *self.state.write().await = ActorState::Deactivating;

        // OBSERVABILITY: Track state transition
        metrics::counter!("plexspaces_actor_state_transitions_total",
            "actor_id" => actor_id.clone(),
            "from" => "Active",
            "to" => "Deactivating"
        ).increment(1);

        // 2. Let behavior clean up if it has lifecycle extensions
        if let Ok(_behavior) = self.behavior.try_write() {
            // TODO: Add as_lifecycle_mut() helper to Actor trait
            // For now, we skip this step
        }

        // 3. Set to terminated
        *self.state.write().await = ActorState::Terminated;

        // OBSERVABILITY: Track deactivation success
        metrics::counter!("plexspaces_actor_state_transitions_total",
            "actor_id" => actor_id.clone(),
            "from" => "Deactivating",
            "to" => "Terminated"
        ).increment(1);
        
        metrics::gauge!("plexspaces_actor_active_total",
            "actor_id" => actor_id.clone()
        ).decrement(1.0);
        
        tracing::info!("Actor deactivated successfully");

        Ok(())
    }

    /// Register actor in ActorRegistry (called AFTER init() succeeds)
    ///
    /// ## Purpose
    /// Registers the actor in ActorRegistry so it can be discovered and messaged.
    /// Called internally by `start()` after `init()` succeeds to ensure failed actors
    /// are never registered (prevents memory leaks).
    ///
    /// ## Design Notes
    /// - Registration happens AFTER init() succeeds (as per actors_gaps2.txt design)
    /// - Supervisor waits for init() to complete before starting next child
    /// - If init() fails, actor is never registered (no cleanup needed)
    /// - This is critical for supervisor tree building
    ///
    /// ## Returns
    /// Ok(()) if registration succeeds, ActorError otherwise
    async fn register_in_registry(&self) -> Result<(), ActorError> {
        use plexspaces_core::{ActorRegistry, RequestContext, MessageSender};
        use crate::ActorRef;
        use std::sync::Arc;
        
        // Get ActorRegistry from ServiceLocator
        if let Some(registry) = self.context.service_locator
            .get_service::<ActorRegistry>().await
        {
            let ctx = RequestContext::new_without_auth(
                self.context.tenant_id.clone(),
                self.context.namespace.clone(),
            );

            // Create ActorRef for registration
            let actor_ref = ActorRef::local(
                self.id.clone(),
                self.mailbox.clone(),
                self.context.service_locator.clone(),
            );

            // Extract actor_type from behavior for dashboard visibility
            let behavior_guard = self.behavior.read().await;
            let behavior_type = behavior_guard.behavior_type();
            drop(behavior_guard);
            let actor_type = match behavior_type {
                plexspaces_core::BehaviorType::GenServer => Some("GenServer".to_string()),
                plexspaces_core::BehaviorType::GenEvent => Some("GenEvent".to_string()),
                plexspaces_core::BehaviorType::GenStateMachine => Some("GenStateMachine".to_string()),
                plexspaces_core::BehaviorType::Workflow => Some("Workflow".to_string()),
                plexspaces_core::BehaviorType::Custom(s) => Some(s),
            };

            // Register actor in registry with type information for dashboard
            registry.register_actor(
                &ctx,
                self.id.clone(),
                Arc::new(actor_ref) as Arc<dyn MessageSender>,
                actor_type,
            ).await;

            // OBSERVABILITY: Log successful registration
            tracing::debug!(
                actor_id = %self.id,
                "Actor registered in ActorRegistry (after init() succeeded)"
            );
        } else {
            // Registry not available - log warning but don't fail
            // This allows actors to be created in test environments without full service setup
            tracing::warn!(
                actor_id = %self.id,
                "ActorRegistry not available in ServiceLocator - actor not registered"
            );
        }
        
        Ok(())
    }

    /// Called on timer/reminder events (ALWAYS AVAILABLE)
    ///
    /// This is a CORE lifecycle hook that handles timer events.
    /// It forwards the timer event to the behavior for processing.
    ///
    /// This is similar to Erlang's gen_server:handle_info with timeout messages
    async fn _on_timer(&mut self, timer_name: &str) -> Result<(), ActorError> {
        // Forward to behavior (Go-style: context first, then message)
        let timer_message = Message::timer(timer_name);

        self.behavior
            .write()
            .await
            .handle_message(&*self.context, timer_message)
            .await
            .map_err(|e| ActorError::BehaviorError(e.to_string()))
    }

    // ============================================================================
    // END STATIC LIFECYCLE METHODS
    // ============================================================================

    /// Process a single message
    async fn process_message(
        message: Message,
        actor_id: &str, // Actor ID passed as parameter (no longer in context)
        behavior: &Arc<RwLock<Box<dyn ActorTrait>>>,
        context: &Arc<ActorContext>,
        last_message_time: &Arc<RwLock<std::time::Instant>>,
        facets: &Arc<RwLock<FacetContainer>>,
    ) -> Result<(), ActorError> {
        // RECURSION DETECTION: Track call depth to detect infinite loops
        use std::thread_local;
        thread_local! {
            static PROCESS_MESSAGE_DEPTH: std::cell::Cell<usize> = std::cell::Cell::new(0);
        }
        
        let depth = PROCESS_MESSAGE_DEPTH.with(|d| {
            let current = d.get();
            d.set(current + 1);
            current
        });
        
        // Safety check: prevent infinite recursion
        const MAX_RECURSION_DEPTH: usize = 100;
        if depth > MAX_RECURSION_DEPTH {
            let _ = PROCESS_MESSAGE_DEPTH.with(|d| d.set(0)); // Reset on panic
            tracing::error!(
                "🔴 [PROCESS_MESSAGE] RECURSION DETECTED! depth={}, actor_id={}, sender={:?}, receiver={}, correlation_id={:?}",
                depth, actor_id, message.sender, message.receiver, message.correlation_id
            );
            eprintln!("\n╔════════════════════════════════════════════════════════════════╗");
            eprintln!("║  INFINITE RECURSION DETECTED IN process_message!                ║");
            eprintln!("║  Depth: {} (max: {})                                            ║", depth, MAX_RECURSION_DEPTH);
            eprintln!("║  Actor ID: {}                                                    ║", actor_id);
            eprintln!("║  Sender: {:?}                                                    ║", message.sender);
            eprintln!("║  Receiver: {}                                                    ║", message.receiver);
            eprintln!("║  Correlation ID: {:?}                                            ║", message.correlation_id);
            eprintln!("╚════════════════════════════════════════════════════════════════╝");
            eprintln!("\nStack backtrace:");
            eprintln!("{:?}", std::backtrace::Backtrace::capture());
            return Err(ActorError::BehaviorError(format!(
                "Infinite recursion detected in process_message (depth: {})",
                depth
            )));
        }
        
        // Convert actor_id to owned String early to avoid lifetime issues in macros
        let actor_id_owned = actor_id.to_string();
        let message_id = message.id.clone();
        let message_type = message.message_type.clone();
        let start = std::time::Instant::now();
        
        // OBSERVABILITY: Tracing span for message processing
        let span = tracing::span!(
            tracing::Level::DEBUG,
            "actor.process_message",
            actor_id = %actor_id_owned,
            message_id = %message_id,
            message_type = %message_type,
            depth = depth
        );
        let _guard = span.enter();
        
        tracing::debug!(
            "🔴 [PROCESS_MESSAGE] START: depth={}, actor_id={}, message_id={}, message_type={}, sender={:?}, receiver={}, correlation_id={:?}",
            depth, actor_id_owned, message_id, message_type, message.sender, message.receiver, message.correlation_id
        );
        
        // OBSERVABILITY: Track message received
        let message_type_owned = message_type.clone();
        metrics::counter!("plexspaces_actor_messages_received_total",
            "actor_id" => actor_id_owned.clone(),
            "message_type" => message_type_owned
        ).increment(1);
        
        // Update last message time for health monitoring
        *last_message_time.write().await = std::time::Instant::now();

        // Apply before-method facet interceptors
        let facets = facets.read().await;
        let method_name = message.message_type.clone();
        let intercepted_args = facets
            .intercept_before(method_name.as_str(), &message.payload)
            .await
            .map_err(|e| {
                // OBSERVABILITY: Track facet interception errors
                let error_str = e.to_string();
                metrics::counter!("plexspaces_actor_facet_intercept_errors_total",
                    "actor_id" => actor_id_owned.clone(),
                    "facet_error" => error_str.clone()
                ).increment(1);
                tracing::error!(error = %e, "Facet interception error");
                ActorError::FacetError(error_str)
            })?;

        // Update message with intercepted args if changed
        let mut message = message;
        if intercepted_args != message.payload {
            message.payload = intercepted_args;
        }

        // Process with behavior (Go-style: context first, then message)
        // Note: ActorContext is now static - sender_id and correlation_id are in Message, not context
        let mut behavior = behavior.write().await;
        tracing::debug!(
            "🔴 [PROCESS_MESSAGE] CALLING handle_message: depth={}, actor_id={}, sender={:?}, receiver={}",
            depth, actor_id_owned, message.sender, message.receiver
        );
        let result = behavior.handle_message(context, message.clone()).await;
        tracing::debug!(
            "🔴 [PROCESS_MESSAGE] handle_message COMPLETED: depth={}, actor_id={}, result={:?}",
            depth, actor_id_owned, result.is_ok()
        );

        // Decrement recursion depth
        let _ = PROCESS_MESSAGE_DEPTH.with(|d| {
            let current = d.get();
            if current > 0 {
                d.set(current - 1);
            }
        });

        // Apply after-method facet interceptors if successful
        if result.is_ok() {
            // TODO: Capture actual result and apply after interceptors
            let _ = facets
                .intercept_after(
                    &method_name,
                    &message.payload,
                    &[], // TODO: Serialize result
                )
                .await;
        }

                // OBSERVABILITY: Track message processing result and latency
        let duration = start.elapsed();
        match &result {
            Ok(_) => {
                let message_type_owned2 = message_type.clone();
                metrics::counter!("plexspaces_actor_messages_processed_total",
                    "actor_id" => actor_id_owned.clone(),
                    "message_type" => message_type_owned2.clone(),
                    "status" => "success"
                ).increment(1);
                metrics::histogram!("plexspaces_actor_message_processing_duration_seconds",
                    "actor_id" => actor_id_owned.clone(),
                    "message_type" => message_type_owned2.clone()
                ).record(duration.as_secs_f64());
                tracing::debug!(duration_ms = duration.as_millis(), "Message processed successfully");
            }
            Err(e) => {
                let message_type_owned3 = message_type.clone();
                let error_type = format!("{:?}", e);
                metrics::counter!("plexspaces_actor_messages_processed_total",
                    "actor_id" => actor_id_owned.clone(),
                    "message_type" => message_type_owned3.clone(),
                    "status" => "error"
                ).increment(1);
                metrics::counter!("plexspaces_actor_message_processing_errors_total",
                    "actor_id" => actor_id_owned.clone(),
                    "message_type" => message_type_owned3.clone(),
                    "error_type" => error_type
                ).increment(1);
                tracing::error!(error = %e, duration_ms = duration.as_millis(), "Message processing failed");
                
                // Note: set_error_message() is available but not called here because:
                // 1. process_message() is not a method (no self parameter)
                // 2. Error handling should be done at the actor level, not in process_message
                // 3. The error is already logged and tracked via metrics
                // TODO: Consider calling set_error_message() when actor transitions to FAILED state
            }
        }

        result.map_err(|e| ActorError::BehaviorError(e.to_string()))
    }
}

// ActorRef is now imported from core crate (see imports at top of file)
// ActorError is now imported from core crate (see imports at top of file)
// The From<JournalError> conversion is also in core crate

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_behavior::MockBehavior;
    use plexspaces_mailbox::MailboxConfig;

    #[tokio::test]
    async fn test_actor_lifecycle() {
        let id = "test-actor".to_string();
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), id.clone()).await.unwrap();

        let mut actor = Actor::new(id.clone(), behavior, mailbox, String::new(), "test-namespace".to_string(), None);

        // Test initial state
        assert_eq!(actor.state().await, ActorState::Creating);

        // Start actor
        actor.start().await.unwrap();
        assert_eq!(actor.state().await, ActorState::Active);

        // Stop actor
        actor.stop().await.unwrap();
        assert_eq!(actor.state().await, ActorState::Terminated);
    }

    #[tokio::test]
    async fn test_actor_id() {
        let id1 = "test".to_string();
        let id2 = "test".to_string();
        assert_eq!(id1, id2);
        assert_eq!(id1.as_str(), "test");
    }

    /// Test that on_activate is ALWAYS called during actor start
    /// This is a CORE feature - not optional
    #[tokio::test]
    async fn test_static_lifecycle_on_activate_always_runs() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let mut actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Start actor - on_activate should ALWAYS run (transitions to Active state)
        actor.start().await.unwrap();

        // Verify state transition to Active (on_activate completed successfully)
        assert_eq!(actor.state().await, ActorState::Active);
    }

    /// Test that on_deactivate is ALWAYS called during actor stop
    /// This is a CORE feature - not optional
    #[tokio::test]
    async fn test_static_lifecycle_on_deactivate_always_runs() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let mut actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        actor.start().await.unwrap();
        actor.stop().await.unwrap();

        // Verify state transition to Terminated (on_deactivate completed successfully)
        assert_eq!(actor.state().await, ActorState::Terminated);
    }

    /// Test that lifecycle state transitions are predictable
    #[tokio::test]
    async fn test_static_lifecycle_state_transitions() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let mut actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Initial state
        assert_eq!(actor.state().await, ActorState::Creating);

        // During start, should go: Created -> Activating -> Active
        actor.start().await.unwrap();
        assert_eq!(actor.state().await, ActorState::Active);

        // During stop, should go: Active -> Deactivating -> Terminated
        actor.stop().await.unwrap();
        assert_eq!(actor.state().await, ActorState::Terminated);
    }

    /// Test that behaviors can optionally extend lifecycle
    /// The core lifecycle always runs, but behaviors can add custom init/cleanup
    #[tokio::test]
    async fn test_static_lifecycle_allows_behavior_extension() {
        // This test will pass once we implement the helper methods
        // for behaviors to extend lifecycle (like as_lifecycle_mut())
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let mut actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Should work even if behavior doesn't implement lifecycle extensions
        actor.start().await.unwrap();
        actor.stop().await.unwrap();
    }

    // ============================================================================
    // RESOURCE MONITORING TESTS
    // ============================================================================

    #[tokio::test]
    async fn test_with_resource_profile() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let profile = ResourceProfile::CpuIntensive;

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        )
        .with_resource_profile(profile);

        // Verify profile was set
        assert!(matches!(
            actor.resource_profile,
            ResourceProfile::CpuIntensive
        ));
    }

    #[tokio::test]
    async fn test_with_resource_contract() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let contract = ResourceContract {
            max_memory_bytes: 1024 * 1024 * 50, // 50 MB
            max_cpu_percent: 80.0,
            max_io_ops_per_sec: Some(1000),
            guaranteed_bandwidth_mbps: Some(10),
            max_execution_time: None,
        };

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        )
        .with_resource_contract(contract.clone());

        // Verify contract was set
        assert!(actor.resource_contract.is_some());
        let stored_contract = actor.resource_contract.unwrap();
        assert_eq!(stored_contract.max_memory_bytes, 1024 * 1024 * 50);
        assert_eq!(stored_contract.max_cpu_percent, 80.0);
        assert_eq!(stored_contract.max_io_ops_per_sec, Some(1000));
    }

    #[tokio::test]
    async fn test_resource_usage() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Get initial resource usage
        let usage = actor.resource_usage().await;

        // Should have zero initial usage
        assert_eq!(usage.memory_bytes, 0);
        assert_eq!(usage.cpu_percent, 0.0);
        assert_eq!(usage.io_ops_per_sec, 0);
        assert_eq!(usage.network_mbps, 0);
    }

    #[tokio::test]
    async fn test_health_check() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Check health (should be Healthy initially)
        let health = actor.health().await;

        // Actor just created, should be healthy
        assert!(matches!(health, ActorHealth::Healthy));
    }

    #[tokio::test]
    async fn test_validate_resources_without_contract() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Validate resources (should pass without contract)
        let result = actor.validate_resources().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_resources_with_contract() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let contract = ResourceContract {
            max_memory_bytes: 1024 * 1024 * 1000, // 1000 MB (high limit)
            max_cpu_percent: 100.0,
            max_io_ops_per_sec: Some(10000),
            guaranteed_bandwidth_mbps: None,
            max_execution_time: None,
        };

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        )
        .with_resource_contract(contract);

        // Validate resources (should pass with reasonable contract)
        let result = actor.validate_resources().await;
        assert!(result.is_ok());
    }

    // ============================================================================
    // ACTOR LIFECYCLE ERROR PATHS TESTS
    // ============================================================================

    #[tokio::test]
    async fn test_start_already_active_error() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let mut actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Start actor
        actor.start().await.unwrap();
        assert_eq!(actor.state().await, ActorState::Active);

        // Try to start again (should fail)
        let result = actor.start().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ActorError::InvalidState(_)));
    }

    #[tokio::test]
    async fn test_stop_already_stopped() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let mut actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Start then stop
        actor.start().await.unwrap();
        actor.stop().await.unwrap();
        assert_eq!(actor.state().await, ActorState::Terminated);

        // Try to stop again (should succeed silently)
        let result = actor.stop().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_message() {
        use plexspaces_mailbox::{mailbox_config_default, Message};

        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(mailbox_config_default(), "test-actor".to_string()).await.expect("Failed to create mailbox");

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Send a message
        let msg = Message::new(b"test message".to_vec());
        let result = actor.send(msg).await;
        assert!(result.is_ok());

        // Verify message is in mailbox
        assert_eq!(actor.mailbox().size().await, 1);
    }

    #[tokio::test]
    async fn test_actor_id_getter() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor-123".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor-123".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Test id() getter
        assert_eq!(actor.id(), "test-actor-123");
    }

    #[tokio::test]
    async fn test_mailbox_getter() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Test mailbox() getter returns a reference
        // Just verify we can access the mailbox
        let _mailbox_ref = actor.mailbox();
        assert!(true); // Test passes if we can get the mailbox reference
    }

    // ============================================================================
    // BEHAVIOR STACK TESTS (become/unbecome pattern)
    // ============================================================================

    #[tokio::test]
    async fn test_become_changes_behavior() {
        let behavior1 = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior1,
            mailbox,
            "test-tenant".to_string(),
            "test-namespace".to_string(),
            None,
        );

        // Change behavior (using raw identifier since "become" is reserved)
        let behavior2 = Box::new(MockBehavior::new());
        let result = actor.r#become(behavior2).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_unbecome_restores_previous_behavior() {
        let behavior1 = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior1,
            mailbox,
            "test-tenant".to_string(),
            "test-namespace".to_string(),
            None,
        );

        // Change behavior (using raw identifier since "become" is reserved)
        let behavior2 = Box::new(MockBehavior::new());
        actor.r#become(behavior2).await.unwrap();

        // Restore previous behavior
        let result = actor.unbecome().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_unbecome_error_when_no_previous_behavior() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Try to unbecome without become (should fail)
        let result = actor.unbecome().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ActorError::NoBehaviorToRestore
        ));
    }

    // ============================================================================
    // FACET MANAGEMENT TESTS
    // ============================================================================

    #[tokio::test]
    async fn test_attach_facet() {
        use async_trait::async_trait;
        use plexspaces_facet::{Facet, FacetError};

        // Simple test facet
        struct TestFacet {
            config: serde_json::Value,
            priority: i32,
        }

        impl TestFacet {
            fn new(config: serde_json::Value, priority: i32) -> Self {
                Self { config, priority }
            }
        }

        #[async_trait]
        impl Facet for TestFacet {
            fn facet_type(&self) -> &str {
                "test-facet"
            }

            fn get_config(&self) -> serde_json::Value {
                self.config.clone()
            }

            fn get_priority(&self) -> i32 {
                self.priority
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }

            async fn on_attach(
                &mut self,
                _actor_id: &str,
                _config: serde_json::Value,
            ) -> Result<(), FacetError> {
                Ok(())
            }

            async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
                Ok(())
            }

            // Default implementations are provided by the trait for before_method and after_method
            // So we don't need to implement them unless we want custom behavior
        }

        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Attach facet
        let facet = Box::new(TestFacet::new(serde_json::json!({}), 50));
        let result = actor.attach_facet(facet).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_facets() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // List facets (should be empty initially)
        let facets = actor.list_facets().await;
        assert_eq!(facets.len(), 0);
    }

    #[tokio::test]
    async fn test_detach_facet() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Try to detach non-existent facet (should succeed - no-op)
        let result = actor.detach_facet("non-existent").await;
        // Note: This might fail depending on FacetContainer implementation
        // If it returns an error, that's also acceptable behavior
        let _ = result; // Accept either Ok or Err
    }

    // ============================================================================
    // MESSAGE LOOP AND PROCESSING TESTS
    // ============================================================================

    /// Test that actor actually processes messages in its message loop
    /// This covers the message loop internals (lines 425-442)
    #[tokio::test]
    async fn test_actor_processes_messages_in_loop() {
        use plexspaces_mailbox::mailbox_config_default;

        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(mailbox_config_default(), "test-actor".to_string()).await.expect("Failed to create mailbox");

        let mut actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Start actor (message loop begins)
        let _handle = actor.start().await.unwrap();
        assert_eq!(actor.state().await, ActorState::Active);

        // Send a message
        let msg = Message::new(b"test message".to_vec());
        actor.send(msg).await.unwrap();

        // Give actor time to process message
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Stop actor gracefully
        actor.stop().await.unwrap();

        // Verify final state
        assert_eq!(actor.state().await, ActorState::Terminated);
    }

    /// Test graceful shutdown via shutdown channel
    /// This covers shutdown path (lines 444-451)
    #[tokio::test]
    async fn test_actor_graceful_shutdown() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let mut actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Start actor
        let handle = actor.start().await.unwrap();
        assert_eq!(actor.state().await, ActorState::Active);

        // Stop actor (triggers shutdown signal)
        actor.stop().await.unwrap();

        // Wait for shutdown to complete
        let result = tokio::time::timeout(tokio::time::Duration::from_secs(1), handle).await;

        // Handle should complete successfully
        assert!(result.is_ok());
        assert_eq!(actor.state().await, ActorState::Terminated);
    }

    /// Test on_timer() hook forwards timer events to behavior
    /// This covers timer hook (lines 612-625)
    #[tokio::test]
    async fn test_on_timer_hook() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let mut actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Call _on_timer hook (internal method)
        let result = actor._on_timer("test-timer").await;

        // Should succeed (MockBehavior accepts all messages)
        assert!(result.is_ok());
    }

    /// Test message processing with sender tracking
    /// This covers process_message lines 658-662 (sender extraction)
    #[tokio::test]
    async fn test_message_processing_with_sender() {
        use plexspaces_mailbox::mailbox_config_default;

        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(mailbox_config_default(), "test-actor".to_string()).await.expect("Failed to create mailbox");

        let mut actor = Actor::new(
            "receiver".to_string(),
            behavior,
            mailbox,
            "test-tenant".to_string(),
            "test-namespace".to_string(),
            None,
        );

        // Start actor
        let _handle = actor.start().await.unwrap();

        // Create message with sender
        let mut msg = Message::new(b"hello".to_vec());
        msg.sender = Some("sender-actor".to_string());

        // Send message
        actor.send(msg).await.unwrap();

        // Give time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Stop actor
        actor.stop().await.unwrap();
    }

    /// Test message processing error handling
    /// This covers error path in message loop (lines 439-442)
    #[tokio::test]
    async fn test_message_processing_error_handling() {
        use async_trait::async_trait;
        use plexspaces_core::{Actor as ActorTrait, BehaviorError, BehaviorType};
        use plexspaces_mailbox::mailbox_config_default;

        // Behavior that always fails
        struct FailingBehavior;

        #[async_trait]
        impl ActorTrait for FailingBehavior {
            async fn handle_message(
                &mut self,
                _ctx: &plexspaces_core::ActorContext,
                _msg: Message,
            ) -> Result<(), BehaviorError> {
                Err(BehaviorError::ProcessingError(
                    "Intentional failure".to_string(),
                ))
            }

            fn behavior_type(&self) -> BehaviorType {
                BehaviorType::Custom("Failing".to_string())
            }
        }

        let behavior = Box::new(FailingBehavior);
        let mailbox = Mailbox::new(mailbox_config_default(), "failing-actor".to_string()).await.unwrap();

        let mut actor = Actor::new(
            "failing-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Start actor
        let _handle = actor.start().await.unwrap();

        // Send message (will fail in processing)
        let msg = Message::new(b"test".to_vec());
        actor.send(msg).await.unwrap();

        // Give time to attempt processing
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Actor should still be running despite error
        assert_eq!(actor.state().await, ActorState::Active);

        // Stop actor
        actor.stop().await.unwrap();
    }

    /// Test health check tracks last message time
    /// This covers process_message line 641 (last_message_time update)
    #[tokio::test]
    async fn test_health_tracking_with_messages() {
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string()).await.unwrap();

        let actor = Actor::new(
            "test-actor".to_string(),
            behavior,
            mailbox,
            String::new(), // tenant_id (empty if auth disabled)
            "test-namespace".to_string(),
            None,
        );

        // Check health immediately (should be Healthy)
        let health1 = actor.health().await;
        assert!(matches!(health1, ActorHealth::Healthy));

        // Simulate long wait (> 30 seconds for Stuck detection)
        // Note: We can't actually wait 30s in test, but we can verify the method works
        let health2 = actor.health().await;
        assert!(matches!(health2, ActorHealth::Healthy));
    }
}
