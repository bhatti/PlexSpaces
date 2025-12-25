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

//! Supervision module for fault tolerance
//!
//! Implements Erlang/OTP-inspired supervision trees with
//! elevated abstractions for adaptive recovery.
//!
//! ## Proto-First Design
//! All data models and errors are defined in `proto/plexspaces/v1/supervision.proto`:
//! - `SupervisionStrategy`, `RestartStrategy`, `ChildType` (enums)
//! - `ChildSpec`, `SupervisorConfig`, `SupervisorState` (messages)
//! - `SupervisionErrorCode`, `SupervisionError` (error types)

use async_trait::async_trait;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout as tokio_timeout;
use tracing::{debug, error, info, instrument, trace, warn};
use metrics;

use plexspaces_actor::{Actor, ActorRef as ActorActorRef};
use plexspaces_core::{ActorError, ActorId, ActorRef, ServiceLocator};

// Import proto types
use plexspaces_proto::supervision::v1::{SupervisionError as ProtoError, SupervisorStats};

// ============================================================================
// Link Provider Trait (Phase 8.5: Link Semantics Integration)
// ============================================================================

/// Trait for providing link semantics to supervisors
///
/// ## Purpose
/// Allows Supervisor to use links internally without creating circular dependencies.
/// Node implements this trait to provide link/unlink functionality.
///
/// ## Erlang Philosophy
/// Supervision uses links internally (Erlang/OTP pattern). When a supervisor
/// adds a child, it links to the child so cascading failures work correctly.
#[async_trait::async_trait]
pub trait LinkProvider: Send + Sync {
    /// Link two actors (bidirectional death propagation)
    ///
    /// ## Arguments
    /// * `actor_id` - First actor in the link
    /// * `linked_actor_id` - Second actor in the link
    ///
    /// ## Returns
    /// Success or error
    async fn link(&self, actor_id: &ActorId, linked_actor_id: &ActorId) -> Result<(), String>;

    /// Unlink two actors
    ///
    /// ## Arguments
    /// * `actor_id` - First actor in the link
    /// * `linked_actor_id` - Second actor in the link
    ///
    /// ## Returns
    /// Success or error
    async fn unlink(&self, actor_id: &ActorId, linked_actor_id: &ActorId) -> Result<(), String>;
}

// ============================================================================
// Supervised Child Trait (Rust-side interface, uses proto errors)
// ============================================================================

/// Supervised child trait - unified interface for actors and supervisors
///
/// ## Erlang/OTP Equivalent
/// In Erlang, both workers and supervisors implement behaviors (gen_server, supervisor)
/// that provide common lifecycle functions:
/// - `start_link/1` - Start the process
/// - `init/1` - Initialize state
/// - `terminate/2` - Cleanup on shutdown
///
/// ## PlexSpaces Design
/// This trait provides the same unified interface so supervisors can manage
/// both actors and other supervisors uniformly, enabling hierarchical supervision trees.
///
/// ## Implementation
/// - `Actor` implements this trait (workers)
/// - `Supervisor` implements this trait (supervisors supervising supervisors)
#[async_trait]
pub trait SupervisedChild: Send + Sync {
    /// Start the child
    ///
    /// ## Behavior
    /// - For actors: Spawns the actor's message processing loop
    /// - For supervisors: Starts all children and begins monitoring
    ///
    /// ## Returns
    /// JoinHandle for monitoring child termination
    async fn start(&mut self) -> Result<tokio::task::JoinHandle<()>, ProtoError>;

    /// Stop the child gracefully with timeout
    ///
    /// ## Arguments
    /// * `timeout` - Maximum time to wait for graceful shutdown
    ///   - None = infinity (wait indefinitely, for supervisors)
    ///   - Some(Duration::ZERO) = brutal_kill (immediate abort)
    ///   - Some(duration) = graceful with timeout
    ///
    /// ## Erlang/OTP Equivalent
    /// Maps to shutdown spec in child_spec (brutal_kill | Timeout | infinity)
    async fn stop(&mut self, timeout: Option<Duration>) -> Result<(), ProtoError>;

    /// Check if child is alive
    fn is_alive(&self) -> bool;

    /// Get child identifier
    fn id(&self) -> &str;
}

/// Supervisor for managing actor lifecycle and fault tolerance
pub struct Supervisor {
    /// Supervisor ID
    id: String,
    /// Supervision strategy (wrapped in Arc<RwLock> for adaptive strategies)
    strategy: Arc<RwLock<SupervisionStrategy>>,
    /// Child actors (IndexMap preserves insertion order for RestForOne)
    children: Arc<RwLock<IndexMap<ActorId, SupervisedActor>>>,
    /// Child supervisors (for hierarchical supervision trees)
    /// IndexMap preserves insertion order for RestForOne strategy
    child_supervisors: Arc<RwLock<IndexMap<String, SupervisedSupervisor>>>,
    /// Parent supervisor (if any)
    parent: Option<Arc<Supervisor>>,
    /// Restart statistics
    stats: Arc<RwLock<SupervisorStats>>,
    /// Event channel for notifications
    event_tx: mpsc::Sender<SupervisorEvent>,
    /// Shutdown signal
    _shutdown_rx: Option<mpsc::Receiver<()>>,
    /// Node reference for link semantics (Phase 8.5: Erlang link/1 pattern)
    /// When provided, supervisor uses links internally for cascading failures
    /// If None, supervisor works standalone without link semantics
    node: Option<Arc<dyn LinkProvider + Send + Sync>>,
    /// ServiceLocator for creating ActorRefs with service access
    /// Required for creating ActorRefs (both local and remote need ServiceLocator)
    service_locator: Option<Arc<ServiceLocator>>,
}

/// Supervised actor wrapper
struct SupervisedActor {
    /// The actual actor instance
    actor: Arc<RwLock<Actor>>,
    /// Reference to the actor
    actor_ref: ActorRef,
    /// Actor task handle (for monitoring termination)
    handle: Option<tokio::task::JoinHandle<()>>,
    /// Restart count (total)
    restart_count: u32,
    /// Last restart time
    last_restart: Option<tokio::time::Instant>,
    /// Restart history for intensity tracking (timestamp of each restart)
    restart_history: Vec<tokio::time::Instant>,
    /// Actor specification for restarts
    spec: ActorSpec,
    /// Facet configurations from ChildSpec (Phase 1: Unified Lifecycle)
    /// Stored separately from ActorSpec to preserve facet configs during restart
    facets: Vec<plexspaces_proto::common::v1::Facet>,
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
struct SupervisedSupervisor {
    /// The child supervisor instance
    supervisor: Arc<RwLock<Supervisor>>,
    /// Supervisor ID (for identification)
    id: String,
    /// Supervisor task handle (for monitoring termination)
    handle: Option<tokio::task::JoinHandle<()>>,
    /// Restart count (total)
    restart_count: u32,
    /// Last restart time
    last_restart: Option<tokio::time::Instant>,
    /// Restart history for intensity tracking
    restart_history: Vec<tokio::time::Instant>,
    /// Restart policy (from spec)
    restart: RestartPolicy,
    /// Shutdown timeout in milliseconds
    shutdown_timeout_ms: Option<u64>,
}

/// Actor specification for creating/restarting actors
///
/// ## Erlang/OTP Equivalent
/// This maps to Erlang's child_spec:
/// ```erlang
/// #{id => ChildId,
///   start => {Module, Function, Args},
///   restart => permanent | temporary | transient,
///   shutdown => brutal_kill | Timeout | infinity,
///   type => worker | supervisor,
///   modules => [Module]}
/// ```
#[derive(Clone)]
pub struct ActorSpec {
    /// Actor ID (Erlang: id)
    pub id: ActorId,
    /// Factory function to create the actor (Erlang: start MFA)
    pub factory: Arc<dyn Fn() -> Result<Actor, ActorError> + Send + Sync>,
    /// Restart policy (Erlang: restart)
    pub restart: RestartPolicy,
    /// Child type - worker or supervisor (Erlang: type)
    pub child_type: ChildType,
    /// Shutdown timeout in milliseconds (Erlang: shutdown)
    /// - None = infinity (for supervisors)
    /// - Some(0) = brutal_kill
    /// - Some(ms) = graceful shutdown with timeout
    pub shutdown_timeout_ms: Option<u64>,
}

/// Supervision strategy (Erlang-inspired but elevated)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupervisionStrategy {
    /// One-for-one: restart only the failed actor
    OneForOne {
        max_restarts: u32,
        within_seconds: u64,
    },
    /// One-for-all: restart all actors if one fails
    OneForAll {
        max_restarts: u32,
        within_seconds: u64,
    },
    /// Rest-for-one: restart failed actor and all started after it
    RestForOne {
        max_restarts: u32,
        within_seconds: u64,
    },
    /// Adaptive: Learn from failures and adapt strategy
    Adaptive {
        initial_strategy: Box<SupervisionStrategy>,
        learning_rate: f64,
    },
    /// Custom strategy with callback
    Custom { name: String },
}

/// Restart policy for individual actors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RestartPolicy {
    /// Always restart
    Permanent,
    /// Restart only on abnormal exit
    Transient,
    /// Never restart
    Temporary,
    /// Exponential backoff
    ExponentialBackoff {
        initial_delay_ms: u64,
        max_delay_ms: u64,
        factor: f64,
    },
}

/// Child type (Erlang-inspired)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChildType {
    /// Always running, restart if terminated
    Worker,
    /// Another supervisor
    Supervisor,
}

/// Supervisor events
#[derive(Debug, Clone)]
pub enum SupervisorEvent {
    /// Child started
    ChildStarted(ActorId),
    /// Child stopped
    ChildStopped(ActorId),
    /// Child restarted
    ChildRestarted(ActorId, u32), // (id, restart_count)
    /// Child failed
    ChildFailed(ActorId, String),
    /// Max restarts exceeded
    MaxRestartsExceeded(ActorId),
    /// Strategy adapted
    StrategyAdapted(SupervisionStrategy),
}

impl Supervisor {
    /// Create a new supervisor
    pub fn new(
        id: String,
        strategy: SupervisionStrategy,
    ) -> (Self, mpsc::Receiver<SupervisorEvent>) {
        let (event_tx, event_rx) = mpsc::channel(100);
        let (_shutdown_tx, shutdown_rx) = mpsc::channel(1);

        let supervisor = Supervisor {
            id,
            strategy: Arc::new(RwLock::new(strategy)),
            children: Arc::new(RwLock::new(IndexMap::new())),
            child_supervisors: Arc::new(RwLock::new(IndexMap::new())),
            parent: None,
            stats: Arc::new(RwLock::new(SupervisorStats::default())),
            event_tx,
            _shutdown_rx: Some(shutdown_rx),
            node: None, // No Node by default (standalone mode)
            service_locator: None, // No ServiceLocator by default
        };

        (supervisor, event_rx)
    }

    /// Set parent supervisor (for supervision trees)
    pub fn with_parent(mut self, parent: Arc<Supervisor>) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Set Node for link semantics (Phase 8.5: Erlang link/1 pattern)
    ///
    /// ## Purpose
    /// When Node is provided, supervisor uses links internally for cascading failures.
    /// This enables the Erlang/OTP pattern where supervision uses links.
    ///
    /// ## Arguments
    /// * `node` - Node that implements LinkProvider trait
    ///
    /// ## Returns
    /// Self for method chaining
    pub fn with_node(mut self, node: Arc<dyn LinkProvider + Send + Sync>) -> Self {
        self.node = Some(node);
        self
    }

    /// Set ServiceLocator for creating ActorRefs
    ///
    /// ## Purpose
    /// ServiceLocator is required for creating ActorRefs (both local and remote need it).
    /// This should be set when Supervisor is created via Node.
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator for service access
    ///
    /// ## Returns
    /// Self for method chaining
    pub fn with_service_locator(mut self, service_locator: Arc<ServiceLocator>) -> Self {
        self.service_locator = Some(service_locator);
        self
    }

    /// Add a child actor
    #[instrument(skip(self, spec), fields(supervisor_id = %self.id, child_id = %spec.id))]
    pub async fn add_child(&self, spec: ActorSpec) -> Result<ActorActorRef, SupervisorError> {
        debug!(
            supervisor_id = %self.id,
            child_id = %spec.id,
            restart_policy = ?spec.restart,
            child_type = ?spec.child_type,
            "Adding child to supervisor"
        );

        // Create the actor via factory
        let mut actor =
            (spec.factory)().map_err(|e| {
                error!(
                    supervisor_id = %self.id,
                    child_id = %spec.id,
                    error = %e,
                    "Failed to create actor via factory"
                );
                SupervisorError::ActorCreationFailed(e.to_string())
            })?;

        // Get mailbox reference before starting actor
        let mailbox = actor.mailbox().clone();

        // Get ServiceLocator (required for ActorRef creation)
        let service_locator = self.service_locator.as_ref()
            .ok_or_else(|| SupervisorError::ActorCreationFailed(
                "ServiceLocator not set on Supervisor. Call with_service_locator() when creating Supervisor.".to_string()
            ))?
            .clone();

        // Create ActorRef from the actor crate (has tell() method)
        let actor_ref = ActorActorRef::local(spec.id.clone(), mailbox, service_locator);
        
        // Also create core ActorRef for internal storage
        let core_actor_ref = ActorRef::new(spec.id.clone())
            .map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?;

        // Start the actor (spawns message loop)
        let handle = actor
            .start()
            .await
            .map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?;

        let supervised = SupervisedActor {
            actor: Arc::new(RwLock::new(actor)),
            actor_ref: core_actor_ref.clone(),
            handle: Some(handle),
            restart_count: 0,
            last_restart: None,
            restart_history: Vec::new(),
            spec: spec.clone(),
            facets: Vec::new(), // Facets not stored in ActorSpec - would need to be added
        };

        // Add to children
        let mut children = self.children.write().await;
        children.insert(spec.id.clone(), supervised);
        drop(children);

        // Phase 3: Register parent-child relationship in ActorRegistry
        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.get_service::<plexspaces_core::ActorRegistry>().await {
                let supervisor_id = ActorId::from(self.id.clone());
                registry.register_parent_child(&supervisor_id, &spec.id).await;
                
                // OBSERVABILITY: Log parent-child registration
                debug!(
                    supervisor_id = %self.id,
                    child_id = %spec.id,
                    "Registered parent-child relationship in ActorRegistry"
                );
            }
        }

        // Phase 8.5: Link Semantics - Link supervisor to child
        // This enables cascading failures (Erlang/OTP pattern)
        if let Some(node) = &self.node {
            let supervisor_id = ActorId::from(self.id.clone());
            if let Err(e) = node.link(&supervisor_id, &spec.id).await {
                // Log error but don't fail - supervision can work without links
                warn!(
                    supervisor_id = %self.id,
                    child_id = %spec.id,
                    error = %e,
                    "Failed to link supervisor to child (supervision will continue without links)"
                );
            } else {
                debug!(
                    supervisor_id = %self.id,
                    child_id = %spec.id,
                    "Linked supervisor to child for cascading failures"
                );
            }
        }

        // OBSERVABILITY: Record metrics for child started (Phase 8)
        metrics::counter!("plexspaces_supervisor_child_started_total",
            "supervisor_id" => self.id.clone(),
            "child_id" => spec.id.clone()
        ).increment(1);

        // Send event
        let _ = self
            .event_tx
            .send(SupervisorEvent::ChildStarted(spec.id))
            .await;

        Ok(actor_ref)
    }

    /// Remove a child actor
    #[instrument(skip(self), fields(supervisor_id = %self.id, child_id = %id))]
    pub async fn remove_child(&self, id: &ActorId) -> Result<(), SupervisorError> {
        debug!(
            supervisor_id = %self.id,
            child_id = %id,
            "Removing child from supervisor"
        );
        // Phase 8.5: Unlink supervisor from child before removing
        if let Some(node) = &self.node {
            let supervisor_id = ActorId::from(self.id.clone());
            let _ = node.unlink(&supervisor_id, id).await; // Ignore errors (idempotent)
        }

        // Phase 3: Unregister parent-child relationship in ActorRegistry
        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.get_service::<plexspaces_core::ActorRegistry>().await {
                let supervisor_id = ActorId::from(self.id.clone());
                registry.unregister_parent_child(&supervisor_id, id).await;
                
                // OBSERVABILITY: Log parent-child unregistration
                debug!(
                    supervisor_id = %self.id,
                    child_id = %id,
                    "Unregistered parent-child relationship in ActorRegistry"
                );
            }
        }

        let mut children = self.children.write().await;

        if let Some(child) = children.shift_remove(id) {
            // Stop the actor gracefully
            if let Some(handle) = child.handle {
                handle.abort();
            }
            // Also call actor.stop() for proper cleanup
            if let Ok(mut actor) = child.actor.try_write() {
                let _ = actor.stop().await;
            }
            let _ = self
                .event_tx
                .send(SupervisorEvent::ChildStopped(id.clone()))
                .await;
            Ok(())
        } else {
            Err(SupervisorError::ChildNotFound(id.clone()))
        }
    }

    /// Add a child supervisor (for hierarchical supervision trees)
    ///
    /// ## Purpose
    /// Adds a child supervisor to this parent supervisor, creating a hierarchical
    /// supervision tree (Erlang/OTP-style). Events from the child supervisor are
    /// forwarded to the parent according to the event propagation policy.
    ///
    /// ## Arguments
    /// * `child_supervisor` - The child supervisor instance (with its own event receiver)
    /// * `child_event_rx` - Event receiver from child supervisor
    /// * `event_propagation` - How child events propagate to parent (proto-defined policy)
    /// * `restart` - Restart policy for this child supervisor
    /// * `shutdown_timeout_ms` - Shutdown timeout for graceful stop
    ///
    /// ## Event Forwarding (Proto-First Design)
    /// This method spawns a task to forward events from child to parent:
    /// ```text
    /// ChildSupervisor -> ForwardingTask -> ParentSupervisor
    /// ```
    /// The forwarding task is NOT stored in SupervisedSupervisor struct (proto-first principle).
    /// When channel abstraction arrives, we replace mpsc with Channel, no refactoring needed.
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_supervisor::*;
    /// use plexspaces_proto::supervision::v1::EventPropagation;
    ///
    /// # async fn example() -> Result<(), SupervisorError> {
    /// let (parent, _) = Supervisor::new(
    ///     "parent".to_string(),
    ///     SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 60 }
    /// );
    ///
    /// let (child, child_rx) = Supervisor::new(
    ///     "child".to_string(),
    ///     SupervisionStrategy::OneForAll { max_restarts: 3, within_seconds: 60 }
    /// );
    ///
    /// parent.add_supervisor_child(
    ///     child,
    ///     child_rx,
    ///     EventPropagation::EventPropagationForwardAll,
    ///     RestartPolicy::Permanent,
    ///     Some(5000)  // 5 second shutdown timeout
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add_supervisor_child(
        &self,
        child_supervisor: Supervisor,
        mut child_event_rx: mpsc::Receiver<SupervisorEvent>,
        event_propagation: plexspaces_proto::supervision::v1::EventPropagation,
        restart: RestartPolicy,
        shutdown_timeout_ms: Option<u64>,
    ) -> Result<(), SupervisorError> {
        let child_id = child_supervisor.id.clone();

        // Start the child supervisor (implements SupervisedChild trait)
        let mut child = child_supervisor;
        let handle = child.start().await.map_err(|e| {
            SupervisorError::ActorCreationFailed(format!(
                "Failed to start child supervisor: {}",
                e.message
            ))
        })?;

        // Spawn event forwarding task (proto-first event propagation)
        // This implements the behavioral pattern defined in EventPropagation proto enum
        let parent_tx = self.event_tx.clone();
        let _forward_child_id = child_id.clone();
        tokio::spawn(async move {
            while let Some(event) = child_event_rx.recv().await {
                // Apply event propagation policy
                let should_forward = match event_propagation {
                    plexspaces_proto::supervision::v1::EventPropagation::EventPropagationForwardAll => true,
                    plexspaces_proto::supervision::v1::EventPropagation::EventPropagationFilterCritical => {
                        // Only forward failures, max restarts exceeded
                        matches!(
                            event,
                            SupervisorEvent::ChildFailed(_, _)
                                | SupervisorEvent::MaxRestartsExceeded(_)
                        )
                    }
                    plexspaces_proto::supervision::v1::EventPropagation::EventPropagationNone => false,
                };

                if should_forward {
                    // Forward child event to parent
                    let _ = parent_tx.send(event).await;
                }
            }
        });

        // Create supervised supervisor wrapper
        let supervised = SupervisedSupervisor {
            supervisor: Arc::new(RwLock::new(child)),
            id: child_id.clone(),
            handle: Some(handle),
            restart_count: 0,
            last_restart: None,
            restart_history: Vec::new(),
            restart: restart,
            shutdown_timeout_ms,
        };

        // Add to child supervisors
        let mut child_supervisors = self.child_supervisors.write().await;
        child_supervisors.insert(child_id.clone(), supervised);
        drop(child_supervisors);

        // Phase 3: Register parent-child relationship in ActorRegistry
        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.get_service::<plexspaces_core::ActorRegistry>().await {
                let supervisor_id = ActorId::from(self.id.clone());
                let child_supervisor_id = ActorId::from(child_id.clone());
                registry.register_parent_child(&supervisor_id, &child_supervisor_id).await;
                
                // OBSERVABILITY: Log parent-child registration
                debug!(
                    supervisor_id = %self.id,
                    child_supervisor_id = %child_id,
                    "Registered parent-child relationship for supervisor child in ActorRegistry"
                );
            }
        }

        // Phase 8.5: Link Semantics - Link parent supervisor to child supervisor
        // This enables cascading failures in supervision trees
        if let Some(node) = &self.node {
            let supervisor_id = ActorId::from(self.id.clone());
            let child_supervisor_id = ActorId::from(child_id.clone());
            if let Err(e) = node.link(&supervisor_id, &child_supervisor_id).await {
                warn!(
                    supervisor_id = %self.id,
                    child_supervisor_id = %child_id,
                    error = %e,
                    "Failed to link supervisor to child supervisor (supervision will continue without links)"
                );
            } else {
                debug!(
                    supervisor_id = %self.id,
                    child_supervisor_id = %child_id,
                    "Linked supervisor to child supervisor for cascading failures"
                );
            }
        }

        // Send event (child supervisor started)
        let _ = self
            .event_tx
            .send(SupervisorEvent::ChildStarted(child_id))
            .await;

        Ok(())
    }

    /// Remove a child supervisor from this supervisor
    ///
    /// ## Purpose
    /// Stops and removes a child supervisor, cleaning up parent-child relationships
    /// and unregistering from ActorRegistry.
    ///
    /// ## Arguments
    /// * `supervisor_id` - ID of the child supervisor to remove
    ///
    /// ## Returns
    /// Success or error if supervisor not found
    pub async fn remove_supervisor_child(&self, supervisor_id: &str) -> Result<(), SupervisorError> {
        debug!(
            supervisor_id = %self.id,
            child_supervisor_id = %supervisor_id,
            "Removing child supervisor from supervisor"
        );
        
        // Phase 8.5: Unlink supervisor from child supervisor before removing
        if let Some(node) = &self.node {
            let supervisor_id_actor = ActorId::from(self.id.clone());
            let child_supervisor_id_actor = ActorId::from(supervisor_id.to_string());
            let _ = node.unlink(&supervisor_id_actor, &child_supervisor_id_actor).await; // Ignore errors (idempotent)
        }

        // Phase 3: Unregister parent-child relationship in ActorRegistry
        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.get_service::<plexspaces_core::ActorRegistry>().await {
                let supervisor_id_actor = ActorId::from(self.id.clone());
                let child_supervisor_id_actor = ActorId::from(supervisor_id.to_string());
                registry.unregister_parent_child(&supervisor_id_actor, &child_supervisor_id_actor).await;
                
                // OBSERVABILITY: Log parent-child unregistration
                debug!(
                    supervisor_id = %self.id,
                    child_supervisor_id = %supervisor_id,
                    "Unregistered parent-child relationship for supervisor child in ActorRegistry"
                );
            }
        }

        let mut child_supervisors = self.child_supervisors.write().await;

        if let Some(mut supervised_supervisor) = child_supervisors.shift_remove(supervisor_id) {
            // Stop the child supervisor gracefully
            if let Some(handle) = supervised_supervisor.handle.take() {
                handle.abort();
            }
            // Also call shutdown on the supervisor for proper cleanup
            if let Ok(mut child_supervisor) = supervised_supervisor.supervisor.try_write() {
                let _ = child_supervisor.shutdown().await;
            }
            let _ = self
                .event_tx
                .send(SupervisorEvent::ChildStopped(supervisor_id.to_string().into()))
                .await;
            Ok(())
        } else {
            Err(SupervisorError::ChildNotFound(supervisor_id.to_string()))
        }
    }

    /// Handle child failure
    #[instrument(skip(self), fields(supervisor_id = %self.id, child_id = %id, reason = %reason))]
    pub async fn handle_failure(
        &self,
        id: &ActorId,
        reason: String,
    ) -> Result<(), SupervisorError> {
        warn!(
            supervisor_id = %self.id,
            child_id = %id,
            reason = %reason,
            "Handling child failure"
        );
        // Record failure pattern (in a separate scope to release lock immediately)
        {
            let mut stats = self.stats.write().await;
            *stats.failure_patterns.entry(reason.clone()).or_insert(0) += 1;
            let failure_count = stats.failure_patterns.get(&reason).copied().unwrap_or(0);
            trace!(
                supervisor_id = %self.id,
                child_id = %id,
                reason = %reason,
                failure_count = failure_count,
                "Recorded failure pattern"
            );
        } // Drop stats lock here

        // Send failure event
        let _ = self
            .event_tx
            .send(SupervisorEvent::ChildFailed(id.clone(), reason.clone()))
            .await;

        // Apply supervision strategy
        // NOTE: restart_* methods will acquire their own locks, so we must NOT hold any locks here
        // Read strategy and clone the relevant parts to avoid holding the lock
        let strategy = self.strategy.read().await.clone();
        match strategy {
            SupervisionStrategy::OneForOne {
                max_restarts,
                within_seconds,
            } => {
                self.restart_one(id, max_restarts, within_seconds).await?;
            }
            SupervisionStrategy::OneForAll {
                max_restarts,
                within_seconds,
            } => {
                self.restart_all(max_restarts, within_seconds).await?;
            }
            SupervisionStrategy::RestForOne {
                max_restarts,
                within_seconds,
            } => {
                self.restart_rest_for_one(id, max_restarts, within_seconds)
                    .await?;
            }
            SupervisionStrategy::Adaptive {
                initial_strategy,
                learning_rate,
            } => {
                // Apply adaptive strategy based on failure patterns
                self.apply_adaptive_strategy(id, &reason, &initial_strategy, learning_rate)
                    .await?;
            }
            SupervisionStrategy::Custom { name } => {
                // TODO: Call custom strategy handler
                todo!("Custom strategy: {}", name);
            }
        }

        Ok(())
    }

    /// Restart a single child supervisor (one-for-one)
    ///
    /// ## Purpose
    /// Restarts a failed child supervisor, tracking restart intensity to prevent
    /// infinite restart loops. If max_restarts is exceeded, escalates to parent.
    ///
    /// ## Failure Escalation
    /// When a child supervisor exceeds max_restarts:
    /// 1. Emit MaxRestartsExceeded event
    /// 2. If parent supervisor exists, notify parent
    /// 3. Parent applies its own supervision strategy
    ///
    /// ## Arguments
    /// * `supervisor_id` - ID of the child supervisor to restart
    /// * `max_restarts` - Maximum restarts within time window
    /// * `within_seconds` - Time window for restart intensity tracking
    async fn restart_supervisor(
        &self,
        supervisor_id: &str,
        max_restarts: u32,
        within_seconds: u64,
    ) -> Result<(), SupervisorError> {
        let mut child_supervisors = self.child_supervisors.write().await;

        if let Some(supervised_supervisor) = child_supervisors.get_mut(supervisor_id) {
            // Track restart intensity using restart_history
            let now = tokio::time::Instant::now();
            let window_duration = Duration::from_secs(within_seconds);

            // Remove old restarts outside the time window
            supervised_supervisor
                .restart_history
                .retain(|&restart_time| now.duration_since(restart_time) < window_duration);

            // Check if we've exceeded max_restarts within the time window
            if supervised_supervisor.restart_history.len() >= max_restarts as usize {
                let _ = self
                    .event_tx
                    .send(SupervisorEvent::MaxRestartsExceeded(
                        supervisor_id.to_string(),
                    ))
                    .await;

                // Escalate to parent supervisor if it exists
                if let Some(parent) = &self.parent {
                    let _ = parent
                        .handle_failure(
                            &self.id,
                            format!("Child supervisor {} exceeded max restarts", supervisor_id),
                        )
                        .await;
                }

                return Err(SupervisorError::MaxRestartsExceeded);
            }

            // Record this restart attempt in history
            supervised_supervisor.restart_history.push(now);

            // Apply restart policy
            match supervised_supervisor.restart {
                RestartPolicy::Permanent => {
                    self.perform_supervisor_restart(supervised_supervisor)
                        .await?;
                }
                RestartPolicy::Transient => {
                    // Only restart on abnormal exit
                    // TODO: Check if exit was abnormal
                    self.perform_supervisor_restart(supervised_supervisor)
                        .await?;
                }
                RestartPolicy::Temporary => {
                    // Don't restart
                    return Ok(());
                }
                RestartPolicy::ExponentialBackoff {
                    initial_delay_ms,
                    max_delay_ms,
                    factor,
                } => {
                    // Calculate delay
                    let delay = calculate_backoff_delay(
                        supervised_supervisor.restart_count,
                        initial_delay_ms,
                        max_delay_ms,
                        factor,
                    );

                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    self.perform_supervisor_restart(supervised_supervisor)
                        .await?;
                }
            }

            // Send restart event
            let _ = self
                .event_tx
                .send(SupervisorEvent::ChildRestarted(
                    supervisor_id.to_string(),
                    supervised_supervisor.restart_count,
                ))
                .await;
        }

        Ok(())
    }

    /// Perform the actual supervisor restart
    ///
    /// ## Purpose
    /// Restarts a child supervisor by stopping it gracefully and creating a new instance.
    ///
    /// ## Design Note
    /// Unlike actor restarts, supervisor restarts require recreating the supervisor instance
    /// and re-adding its children. This is similar to Erlang/OTP supervisor restart behavior.
    async fn perform_supervisor_restart(
        &self,
        supervised_supervisor: &mut SupervisedSupervisor,
    ) -> Result<(), SupervisorError> {
        // Abort the old supervisor's task handle
        if let Some(handle) = supervised_supervisor.handle.take() {
            handle.abort();
        }

        // Stop the old supervisor gracefully
        if let Ok(mut supervisor) = supervised_supervisor.supervisor.try_write() {
            let _ = Box::pin(supervisor.shutdown()).await;
        }

        // TODO: Recreate supervisor instance and re-add children
        // For now, we just mark it as restarted. Full implementation requires:
        // 1. Supervisor factory function (similar to ActorSpec)
        // 2. Child spec storage for re-adding children
        // 3. Recursive supervisor tree reconstruction

        // Update restart tracking
        supervised_supervisor.restart_count += 1;
        supervised_supervisor.last_restart = Some(tokio::time::Instant::now());

        Ok(())
    }

    /// Restart a single actor (one-for-one)
    async fn restart_one(
        &self,
        id: &ActorId,
        max_restarts: u32,
        within_seconds: u64,
    ) -> Result<(), SupervisorError> {
        let mut children = self.children.write().await;
        let mut stats = self.stats.write().await;

        if let Some(child) = children.get_mut(id) {
            // Track restart intensity using restart_history
            let now = tokio::time::Instant::now();
            let window_duration = Duration::from_secs(within_seconds);

            // Remove old restarts outside the time window
            child
                .restart_history
                .retain(|&restart_time| now.duration_since(restart_time) < window_duration);

            // Check if we've exceeded max_restarts within the time window
            if child.restart_history.len() >= max_restarts as usize {
                error!(
                    supervisor_id = %self.id,
                    child_id = %id,
                    restart_count = child.restart_history.len(),
                    max_restarts = max_restarts,
                    within_seconds = within_seconds,
                    "Max restarts exceeded for child"
                );
                let _ = self
                    .event_tx
                    .send(SupervisorEvent::MaxRestartsExceeded(id.clone()))
                    .await;
                return Err(SupervisorError::MaxRestartsExceeded);
            }

            // Record this restart attempt in history
            child.restart_history.push(now);

            // Apply restart policy
            match child.spec.restart {
                RestartPolicy::Permanent => {
                    self.perform_restart(child, &mut stats).await?;
                }
                RestartPolicy::Transient => {
                    // Only restart on abnormal exit
                    // TODO: Check if exit was abnormal
                    self.perform_restart(child, &mut stats).await?;
                }
                RestartPolicy::Temporary => {
                    // Don't restart
                    return Ok(());
                }
                RestartPolicy::ExponentialBackoff {
                    initial_delay_ms,
                    max_delay_ms,
                    factor,
                } => {
                    // Calculate delay
                    let delay = calculate_backoff_delay(
                        child.restart_count,
                        initial_delay_ms,
                        max_delay_ms,
                        factor,
                    );

                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    self.perform_restart(child, &mut stats).await?;
                }
            }

            // Send restart event
            info!(
                supervisor_id = %self.id,
                child_id = %id,
                restart_count = child.restart_count,
                restart_policy = ?child.spec.restart,
                "Child restarted successfully"
            );
            let _ = self
                .event_tx
                .send(SupervisorEvent::ChildRestarted(
                    id.clone(),
                    child.restart_count,
                ))
                .await;
        }

        Ok(())
    }

    /// Restart all actors (one-for-all)
    async fn restart_all(
        &self,
        max_restarts: u32,
        within_seconds: u64,
    ) -> Result<(), SupervisorError> {
        let children = self.children.read().await;
        let ids: Vec<ActorId> = children.keys().cloned().collect();
        drop(children);

        for id in ids {
            self.restart_one(&id, max_restarts, within_seconds).await?;
        }

        Ok(())
    }

    /// Restart failed actor and all started after it (rest-for-one)
    async fn restart_rest_for_one(
        &self,
        failed_id: &ActorId,
        max_restarts: u32,
        within_seconds: u64,
    ) -> Result<(), SupervisorError> {
        // IndexMap preserves insertion order, so we can find position of failed actor
        // and restart all actors from that position onwards

        let children = self.children.read().await;

        // Find the index of the failed actor
        let failed_index = children.get_index_of(failed_id);

        if failed_index.is_none() {
            drop(children);
            return Err(SupervisorError::ChildNotFound(failed_id.to_string()));
        }

        let failed_idx = failed_index.unwrap();

        // Collect IDs of failed actor + all actors started after it
        let ids_to_restart: Vec<ActorId> = children
            .iter()
            .skip(failed_idx) // Skip actors before failed one
            .map(|(id, _)| id.clone())
            .collect();

        drop(children);

        // Restart all actors in order (failed + rest)
        for id in ids_to_restart {
            self.restart_one(&id, max_restarts, within_seconds).await?;
        }

        Ok(())
    }

    /// Apply adaptive supervision strategy
    ///
    /// ## Adaptation Logic
    /// - If failed_restarts > successful_restarts * 2: Switch to OneForAll (more conservative)
    /// - Otherwise: Use initial strategy
    /// - Emit StrategyAdapted event when strategy changes
    async fn apply_adaptive_strategy(
        &self,
        id: &ActorId,
        _reason: &str,
        initial_strategy: &SupervisionStrategy,
        _learning_rate: f64,
    ) -> Result<(), SupervisorError> {
        // Check stats to determine if we should adapt strategy
        let (should_adapt, new_strategy) = {
            let stats = self.stats.read().await;
            let should_be_conservative = stats.failed_restarts > stats.successful_restarts * 2;

            if should_be_conservative {
                // Adapt to more conservative strategy (OneForAll)
                match initial_strategy {
                    SupervisionStrategy::OneForOne {
                        max_restarts,
                        within_seconds,
                    } => {
                        // Switch from OneForOne to OneForAll
                        let new_strat = SupervisionStrategy::OneForAll {
                            max_restarts: *max_restarts,
                            within_seconds: *within_seconds,
                        };
                        (true, new_strat)
                    }
                    other => (false, other.clone()),
                }
            } else {
                // Keep initial strategy
                (false, initial_strategy.clone())
            }
        }; // Drop stats lock

        // If strategy changed, update it and emit event
        if should_adapt {
            {
                let mut strategy = self.strategy.write().await;
                *strategy = SupervisionStrategy::Adaptive {
                    initial_strategy: Box::new(new_strategy.clone()),
                    learning_rate: _learning_rate,
                };
            } // Drop strategy lock

            // Increment strategy_adaptations counter
            {
                let mut stats = self.stats.write().await;
                stats.strategy_adaptations += 1;
            }

            // Emit StrategyAdapted event
            let _ = self
                .event_tx
                .send(SupervisorEvent::StrategyAdapted(new_strategy.clone()))
                .await;
        }

        // Apply the strategy (adapted or original)
        match new_strategy {
            SupervisionStrategy::OneForOne {
                max_restarts,
                within_seconds,
            } => {
                self.restart_one(id, max_restarts, within_seconds).await?;
            }
            SupervisionStrategy::OneForAll {
                max_restarts,
                within_seconds,
            } => {
                self.restart_all(max_restarts, within_seconds).await?;
            }
            SupervisionStrategy::RestForOne {
                max_restarts,
                within_seconds,
            } => {
                self.restart_rest_for_one(id, max_restarts, within_seconds)
                    .await?;
            }
            _ => {
                // Fallback: use initial strategy
                if let SupervisionStrategy::OneForOne {
                    max_restarts,
                    within_seconds,
                } = initial_strategy
                {
                    self.restart_one(id, *max_restarts, *within_seconds).await?;
                }
            }
        }

        Ok(())
    }

    /// Perform the actual restart
    #[instrument(skip(self, child, stats), fields(supervisor_id = %self.id, child_id = %child.spec.id))]
    async fn perform_restart(
        &self,
        child: &mut SupervisedActor,
        stats: &mut SupervisorStats,
    ) -> Result<(), SupervisorError> {
        debug!(
            supervisor_id = %self.id,
            child_id = %child.spec.id,
            "Performing child restart"
        );
        stats.total_restarts += 1;

        // Stop the old actor if it's still running
        if let Some(handle) = child.handle.take() {
            handle.abort();
        }

        // Create new actor via factory
        let mut new_actor = (child.spec.factory)().map_err(|e| {
            stats.failed_restarts += 1;
            error!(
                supervisor_id = %self.id,
                child_id = %child.spec.id,
                error = %e,
                "Failed to create actor during restart"
            );
            SupervisorError::RestartFailed(e.to_string())
        })?;

        // Phase 1: Unified Lifecycle - Restore facets from stored facet configs during restart
        // Facets are stored in SupervisedActor and restored here to ensure they're reattached
        if !child.facets.is_empty() {
            // Get FacetRegistry from ServiceLocator to create facets from proto
            if let Some(service_locator) = &self.service_locator {
                use plexspaces_core::service_locator::service_names;
                if let Some(facet_registry_wrapper) = service_locator.get_service_by_name::<plexspaces_core::FacetRegistryServiceWrapper>(service_names::FACET_REGISTRY).await {
                    let facet_registry = facet_registry_wrapper.inner_clone();
                    // Use facet_helpers to create facets from proto
                    use crate::create_facets_from_proto;
                    let facets = create_facets_from_proto(&child.facets, &facet_registry).await;
                    
                    // Attach facets to the new actor before starting
                    // Phase 2: Supervisor Facet Metrics - Record metrics for facet restoration
                    let facet_restore_start = std::time::Instant::now();
                    let mut restored_count = 0;
                    
                    for mut facet in facets {
                        if let Err(e) = new_actor.attach_facet(facet).await {
                            warn!(
                                supervisor_id = %self.id,
                                child_id = %child.spec.id,
                                error = %e,
                                "Failed to attach facet during restart (continuing with other facets)"
                            );
                            metrics::counter!("plexspaces_supervisor_facet_restore_errors_total",
                                "supervisor_id" => self.id.clone(),
                                "child_id" => child.spec.id.clone()
                            ).increment(1);
                        } else {
                            restored_count += 1;
                        }
                    }
                    
                    let facet_restore_duration = facet_restore_start.elapsed();
                    metrics::histogram!("plexspaces_supervisor_facet_restore_duration_seconds",
                        "supervisor_id" => self.id.clone(),
                        "child_id" => child.spec.id.clone()
                    ).record(facet_restore_duration.as_secs_f64());
                    metrics::counter!("plexspaces_supervisor_facets_restored_total",
                        "supervisor_id" => self.id.clone(),
                        "child_id" => child.spec.id.clone()
                    ).increment(restored_count);
                    
                    debug!(
                        supervisor_id = %self.id,
                        child_id = %child.spec.id,
                        facet_count = child.facets.len(),
                        restored_count = restored_count,
                        duration_ms = facet_restore_duration.as_millis(),
                        "Restored facets from stored facet configs during restart"
                    );
                } else {
                    debug!(
                        supervisor_id = %self.id,
                        child_id = %child.spec.id,
                        facet_count = child.facets.len(),
                        "FacetRegistry not available - facets not restored (graceful degradation)"
                    );
                }
            }
        }
        
        // Start the new actor
        // Facets are already attached, so facet lifecycle hooks will be called during start()
        let handle = new_actor.start().await.map_err(|e| {
            stats.failed_restarts += 1;
            error!(
                supervisor_id = %self.id,
                child_id = %child.spec.id,
                error = %e,
                "Failed to start actor during restart"
            );
            SupervisorError::RestartFailed(e.to_string())
        })?;

        // Update child state
        child.actor = Arc::new(RwLock::new(new_actor));
        child.handle = Some(handle);
        child.restart_count += 1;
        child.last_restart = Some(tokio::time::Instant::now());
        stats.successful_restarts += 1;

        // OBSERVABILITY: Record metrics for child restarted (Phase 8)
        metrics::counter!("plexspaces_supervisor_child_restarted_total",
            "supervisor_id" => self.id.clone(),
            "child_id" => child.spec.id.clone()
        ).increment(1);

        debug!(
            supervisor_id = %self.id,
            child_id = %child.spec.id,
            restart_count = child.restart_count,
            "Child restart completed successfully"
        );

        Ok(())
    }

    /// Shutdown all children gracefully (cascading shutdown for supervision trees)
    ///
    /// ## Erlang/OTP Shutdown Order
    /// Follows Erlang/OTP convention for graceful shutdown:
    /// 1. Shutdown child supervisors first (in reverse start order)
    ///    - Each child supervisor shuts down its own children recursively
    ///    - Wait for child supervisor shutdown to complete
    /// 2. Then shutdown child actors (in reverse start order)
    ///    - Enforce shutdown timeouts per actor
    ///
    /// ## Cascading Behavior
    /// When a parent supervisor shuts down, the shutdown cascades down the entire
    /// supervision tree:
    /// ```text
    /// RootSupervisor.shutdown()
    ///   - MidSupervisor1.shutdown()
    ///     - Actor1.stop()
    ///     - Actor2.stop()
    ///   - MidSupervisor2.shutdown()
    ///     - Actor3.stop()
    ///     - Actor4.stop()
    /// ```
    ///
    /// ## Error Handling
    /// Shutdown continues even if some children fail to stop gracefully.
    /// All errors are logged but don't prevent other children from stopping.
    #[instrument(skip(self), fields(supervisor_id = %self.id))]
    pub async fn shutdown(&mut self) -> Result<(), SupervisorError> {
        info!(
            supervisor_id = %self.id,
            "Starting supervisor shutdown"
        );
        // Phase 1: Shutdown child supervisors first (they shutdown their children recursively)
        // Reverse order to shutdown in opposite order of start (Erlang/OTP convention)
        let mut child_supervisors = self.child_supervisors.write().await;

        // Collect IDs in reverse order
        let supervisor_ids: Vec<String> = child_supervisors.keys().rev().cloned().collect();

        for id in supervisor_ids {
            if let Some(mut supervised_supervisor) = child_supervisors.shift_remove(&id) {
                // Phase 3: Unregister parent-child relationship in ActorRegistry
                if let Some(service_locator) = &self.service_locator {
                    if let Some(registry) = service_locator.get_service::<plexspaces_core::ActorRegistry>().await {
                        let supervisor_id = ActorId::from(self.id.clone());
                        let child_supervisor_id = ActorId::from(id.clone());
                        registry.unregister_parent_child(&supervisor_id, &child_supervisor_id).await;
                    }
                }

                // Abort the supervisor's task handle
                if let Some(handle) = supervised_supervisor.handle.take() {
                    handle.abort();
                }

                // Call shutdown on the child supervisor (recursive!)
                if let Ok(mut child_supervisor) = supervised_supervisor.supervisor.try_write() {
                    let _timeout = supervised_supervisor
                        .shutdown_timeout_ms
                        .map(Duration::from_millis);

                    // Recursive call: child supervisor shuts down its own children
                    // Use Box::pin to enable recursion in async fn
                    let _ = Box::pin(child_supervisor.shutdown()).await;
                }

                // Emit ChildStopped event for child supervisor
                let _ = self.event_tx.send(SupervisorEvent::ChildStopped(ActorId::from(id))).await;
            }
        }

        drop(child_supervisors); // Release lock before shutting down actors

        // Phase 2: Shutdown child actors (in reverse start order)
        // Phase 3: Unregister parent-child relationships for actors
        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.get_service::<plexspaces_core::ActorRegistry>().await {
                let supervisor_id = ActorId::from(self.id.clone());
                let children = self.children.read().await;
                let child_ids: Vec<ActorId> = children.keys().cloned().collect();
                drop(children);
                
                for child_id in child_ids {
                    registry.unregister_parent_child(&supervisor_id, &child_id).await;
                }
            }
        }

        let children = self.children.read().await;
        let actor_count = children.len();
        debug!(
            supervisor_id = %self.id,
            child_actor_count = actor_count,
            "Shutting down child actors (reverse start order)"
        );

        for (id, child) in children.iter().rev() {
            // Phase 4: Enforce shutdown spec (BrutalKill, Timeout, Infinity)
            let shutdown_timeout = child.spec.shutdown_timeout_ms;
            
            match shutdown_timeout {
                Some(0) => {
                    // BrutalKill: Immediate abort
                    debug!(
                        supervisor_id = %self.id,
                        child_id = %id,
                        "BrutalKill: Aborting child immediately"
                    );
                    if let Some(handle) = &child.handle {
                        handle.abort();
                    }
                }
                Some(timeout_ms) => {
                    // Timeout: Graceful shutdown with timeout
                    debug!(
                        supervisor_id = %self.id,
                        child_id = %id,
                        timeout_ms = timeout_ms,
                        "Graceful shutdown with timeout"
                    );
                    // Phase 1: Unified Lifecycle - Graceful shutdown with facet lifecycle hooks
                    // actor.stop() will trigger:
                    // 1. facet.on_terminate_start() for all facets (priority order)
                    // 2. actor.on_facets_detaching()
                    // 3. actor.terminate()
                    // 4. facet.on_detach() for all facets (reverse priority order)
                    let stop_future = async {
                        if let Ok(mut actor) = child.actor.try_write() {
                            // OBSERVABILITY: Record metrics for graceful shutdown
                            let shutdown_start = std::time::Instant::now();
                            metrics::counter!("plexspaces_supervisor_child_shutdown_total",
                                "supervisor_id" => self.id.clone(),
                                "child_id" => id.clone()
                            ).increment(1);
                            
                            let result = actor.stop().await;
                            
                            let shutdown_duration = shutdown_start.elapsed();
                            metrics::histogram!("plexspaces_supervisor_child_shutdown_duration_seconds",
                                "supervisor_id" => self.id.clone(),
                                "child_id" => id.clone()
                            ).record(shutdown_duration.as_secs_f64());
                            
                            if result.is_err() {
                                metrics::counter!("plexspaces_supervisor_child_shutdown_errors_total",
                                    "supervisor_id" => self.id.clone(),
                                    "child_id" => id.clone()
                                ).increment(1);
                                warn!(
                                    supervisor_id = %self.id,
                                    child_id = %id,
                                    error = ?result.as_ref().err(),
                                    "Child shutdown failed (continuing with other children)"
                                );
                            } else {
                                debug!(
                                    supervisor_id = %self.id,
                                    child_id = %id,
                                    duration_ms = shutdown_duration.as_millis(),
                                    "Child shutdown completed (facet lifecycle hooks executed)"
                                );
                            }
                        }
                        if let Some(handle) = &child.handle {
                            handle.abort();
                        }
                    };
                    
                    // Enforce timeout
                    if let Err(_) = tokio_timeout(Duration::from_millis(timeout_ms), stop_future).await {
                        warn!(
                            supervisor_id = %self.id,
                            child_id = %id,
                            timeout_ms = timeout_ms,
                            "Child shutdown exceeded timeout, aborting"
                        );
                        if let Some(handle) = &child.handle {
                            handle.abort();
                        }
                    }
                }
                None => {
                    // Infinity: Wait indefinitely (for supervisors)
                    debug!(
                        supervisor_id = %self.id,
                        child_id = %id,
                        "Infinity: Waiting indefinitely for child shutdown"
                    );
                    if let Ok(mut actor) = child.actor.try_write() {
                        let _ = actor.stop().await;
                    }
                    if let Some(handle) = &child.handle {
                        handle.abort();
                    }
                }
            }
            
            let _ = self
                .event_tx
                .send(SupervisorEvent::ChildStopped(id.clone()))
                .await;
        }

        info!(
            supervisor_id = %self.id,
            "Supervisor shutdown completed"
        );

        Ok(())
    }

    /// Get supervisor statistics
    pub async fn stats(&self) -> SupervisorStats {
        let guard = self.stats.read().await;
        guard.clone()
    }
}

/// Implementation of SupervisedChild trait for Supervisor
///
/// This enables supervisors to be children of other supervisors, creating
/// hierarchical supervision trees (Erlang/OTP-style supervision hierarchies).
#[async_trait]
impl SupervisedChild for Supervisor {
    /// Start the supervisor and all its children (Phase 4: Bottom-up startup)
    ///
    /// ## Behavior
    /// - Starts all child actors/supervisors in order (bottom-up)
    /// - Each child must complete init() before next starts
    /// - If any child fails, rollback already-started children in reverse order
    /// - Spawns monitoring task for child health
    /// - Returns JoinHandle for supervisor termination
    ///
    /// ## Phase 4: Bottom-Up Startup with Rollback
    /// This method implements proper bottom-up startup:
    /// 1. Start children in spec order
    /// 2. Wait for each child's init() to complete before starting next
    /// 3. If any child fails, rollback all previously started children
    ///
    /// ## Returns
    /// JoinHandle that completes when supervisor stops
    async fn start(&mut self) -> Result<tokio::task::JoinHandle<()>, ProtoError> {
        // OBSERVABILITY: Record metrics for supervisor startup (Phase 8)
        let startup_start = std::time::Instant::now();
        
        // Phase 4: Bottom-up startup with rollback
        // Start nested supervisors first (bottom-up), then verify actors are started
        // If any child fails, rollback all previously started children in reverse order
        
        let mut started_supervisor_ids: Vec<String> = Vec::new();
        let mut started_actor_ids: Vec<ActorId> = Vec::new();
        let mut rollback_needed = false;
        let mut failed_child_id: Option<String> = None;
        
        // Step 1: Start nested supervisors first (bottom-up ordering)
        {
            let child_supervisors = self.child_supervisors.read().await;
            let supervisor_ids: Vec<String> = child_supervisors.keys().cloned().collect();
            drop(child_supervisors);
            
            for supervisor_id in supervisor_ids {
                let needs_start = {
                    let child_supervisors = self.child_supervisors.read().await;
                    if let Some(supervised_supervisor) = child_supervisors.get(&supervisor_id) {
                        // Check if supervisor is already started
                        supervised_supervisor.handle.is_none()
                    } else {
                        false
                    }
                };
                
                if needs_start {
                    // Recursively start the nested supervisor
                    let supervisor_arc = {
                        let child_supervisors = self.child_supervisors.read().await;
                        if let Some(supervised_supervisor) = child_supervisors.get(&supervisor_id) {
                            Some(supervised_supervisor.supervisor.clone())
                        } else {
                            None
                        }
                    };
                    
                    if let Some(supervisor_arc) = supervisor_arc {
                        if let Ok(mut child_supervisor) = supervisor_arc.try_write() {
                            match child_supervisor.start().await {
                                Ok(handle) => {
                                    // Update the handle in the supervised supervisor
                                    let mut child_supervisors = self.child_supervisors.write().await;
                                    if let Some(supervised) = child_supervisors.get_mut(&supervisor_id) {
                                        supervised.handle = Some(handle);
                                    }
                                    drop(child_supervisors);
                                    started_supervisor_ids.push(supervisor_id.clone());
                                }
                                Err(e) => {
                                    rollback_needed = true;
                                    failed_child_id = Some(format!("supervisor:{}", supervisor_id));
                                    error!(
                                        supervisor_id = %self.id,
                                        child_supervisor_id = %supervisor_id,
                                        error = %e.message,
                                        "Failed to start nested supervisor"
                                    );
                                    break;
                                }
                            }
                        } else {
                            rollback_needed = true;
                            failed_child_id = Some(format!("supervisor:{}", supervisor_id));
                            break;
                        }
                    } else {
                        rollback_needed = true;
                        failed_child_id = Some(format!("supervisor:{}", supervisor_id));
                        break;
                    }
                } else {
                    // Already started or doesn't exist
                    started_supervisor_ids.push(supervisor_id.clone());
                }
            }
        }
        
        // Step 2: Verify actors are started (they should be started via add_child())
        if !rollback_needed {
            let children = self.children.read().await;
            
            for (id, child) in children.iter() {
                // Verify child is started
                if !child.handle.is_some() {
                    rollback_needed = true;
                    failed_child_id = Some(format!("actor:{}", id));
                    break;
                }
                started_actor_ids.push(id.clone());
            }
        }
        
        // Step 3: Rollback if needed (in reverse order)
        if rollback_needed {
            // Rollback actors first (in reverse order)
            for child_id in started_actor_ids.iter().rev() {
                if let Err(e) = self.remove_child(child_id).await {
                    warn!(
                        supervisor_id = %self.id,
                        child_id = %child_id,
                        error = %e,
                        "Failed to rollback child actor during startup"
                    );
                }
            }
            
            // Rollback nested supervisors (in reverse order)
            for supervisor_id in started_supervisor_ids.iter().rev() {
                if let Err(e) = self.remove_supervisor_child(supervisor_id.as_str()).await {
                    warn!(
                        supervisor_id = %self.id,
                        child_supervisor_id = %supervisor_id,
                        error = %e,
                        "Failed to rollback child supervisor during startup"
                    );
                }
            }
            
            return Err(ProtoError {
                code: plexspaces_proto::supervision::v1::SupervisionErrorCode::ChildStartFailed
                    as i32,
                message: format!(
                    "Child {:?} not started, rolled back {} supervisors and {} actors",
                    failed_child_id,
                    started_supervisor_ids.len(),
                    started_actor_ids.len()
                ),
                context: Default::default(),
                timestamp: None,
            });
        }

        // OBSERVABILITY: Record metrics for supervisor startup duration (Phase 8)
        let startup_duration = startup_start.elapsed();
        metrics::histogram!("plexspaces_supervisor_startup_duration_seconds",
            "supervisor_id" => self.id.clone()
        ).record(startup_duration.as_secs_f64());

        // Spawn supervisor monitoring task
        let supervisor_id = self.id.clone();
        let handle = tokio::spawn(async move {
            // Supervisor runs indefinitely until shutdown
            // In a full implementation, this would monitor children and handle events
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                // TODO: Monitor children health, handle restart requests
            }
        });

        Ok(handle)
    }

    /// Stop the supervisor and all its children gracefully (Phase 4: Top-down shutdown)
    ///
    /// ## Arguments
    /// * `timeout` - Maximum time to wait for graceful shutdown
    ///   - None = wait indefinitely (Erlang/OTP infinity)
    ///   - Some(Duration::ZERO) = brutal_kill
    ///   - Some(duration) = graceful with timeout
    ///
    /// ## Phase 4: Top-Down Shutdown
    /// This method implements proper top-down shutdown:
    /// 1. Stop child supervisors first (they shutdown their children recursively)
    /// 2. Stop child actors in reverse start order
    /// 3. Enforce shutdown specs (BrutalKill, Timeout, Infinity) for each child
    ///
    /// ## Behavior
    /// - Stops all children in reverse start order (Erlang/OTP convention)
    /// - Waits for each child to stop before stopping the next
    /// - Enforces timeout for each child according to its shutdown spec
    async fn stop(&mut self, _timeout: Option<Duration>) -> Result<(), ProtoError> {
        // Phase 4: Top-down shutdown is implemented in shutdown() method
        // which already handles:
        // - Child supervisors first (recursive)
        // - Child actors in reverse order
        // - Shutdown spec enforcement (BrutalKill, Timeout, Infinity)
        self.shutdown().await.map_err(|e| ProtoError {
            code: plexspaces_proto::supervision::v1::SupervisionErrorCode::ChildStopFailed as i32,
            message: format!("Supervisor shutdown failed: {}", e),
            context: Default::default(),
            timestamp: None,
        })
    }

    /// Check if supervisor is alive
    ///
    /// ## Returns
    /// true if supervisor has active children, false otherwise
    fn is_alive(&self) -> bool {
        // Supervisor is alive if it has any children
        // Use try_read to avoid blocking
        if let Ok(children) = self.children.try_read() {
            !children.is_empty()
        } else {
            // If we can't acquire lock, assume alive (conservative)
            true
        }
    }

    /// Get supervisor identifier
    fn id(&self) -> &str {
        &self.id
    }
}

// ============================================================================
// Phase 4: Enhanced Supervisor Lifecycle Methods
// ============================================================================

impl Supervisor {
    /// Start a child dynamically (Phase 4)
    ///
    /// ## Purpose
    /// Dynamically adds and starts a new child to a running supervisor.
    /// This enables runtime child management without supervisor restart.
    ///
    /// ## Arguments
    /// * `spec` - ChildSpec defining the child to start
    ///
    /// ## Returns
    /// ActorId of the started child
    ///
    /// ## Example
    /// ```rust,ignore
    /// let spec = ChildSpec::worker("worker1", "worker1@node1", start_fn);
    /// let child_id = supervisor.start_child(spec).await?;
    /// ```
    #[instrument(skip(self, spec), fields(supervisor_id = %self.id, child_id = %spec.child_id))]
    pub async fn start_child(&mut self, spec: crate::child_spec::ChildSpec) -> Result<ActorId, SupervisorError> {
        use crate::child_spec::{StartedChild, ChildType};
        
        debug!(
            supervisor_id = %self.id,
            child_id = %spec.child_id,
            child_type = ?spec.child_type,
            "Starting child dynamically"
        );

        // Call start function to create/start the child
        let started = (spec.start_fn)().await
            .map_err(|e| SupervisorError::ActorCreationFailed(format!(
                "Failed to start child: {}", e
            )))?;

        match started {
            StartedChild::Worker { mut actor, actor_ref } => {
                // Phase 1: Unified Lifecycle - Attach facets from ChildSpec before starting actor
                // Facets are attached in priority order (high priority first)
                // This ensures facets are ready before actor.init() is called
                if !spec.facets.is_empty() {
                    // Get FacetRegistry from ServiceLocator to create facets from proto
                    if let Some(service_locator) = &self.service_locator {
                        use plexspaces_core::service_locator::service_names;
                        if let Some(facet_registry_wrapper) = service_locator.get_service_by_name::<plexspaces_core::FacetRegistryServiceWrapper>(service_names::FACET_REGISTRY).await {
                            let facet_registry = facet_registry_wrapper.inner_clone();
                            // Use facet_helpers to create facets from proto
                            use crate::create_facets_from_proto;
                            let facets = create_facets_from_proto(&spec.facets, &facet_registry).await;
                            
                            // Attach facets to the actor before starting
                            // Phase 2: Supervisor Facet Metrics - Record metrics for facet attachment
                            let facet_attach_start = std::time::Instant::now();
                            let mut attached_count = 0;
                            
                            for mut facet in facets {
                                if let Err(e) = actor.attach_facet(facet).await {
                                    warn!(
                                        supervisor_id = %self.id,
                                        child_id = %spec.child_id,
                                        error = %e,
                                        "Failed to attach facet from ChildSpec (continuing with other facets)"
                                    );
                                    metrics::counter!("plexspaces_supervisor_facet_attach_errors_total",
                                        "supervisor_id" => self.id.clone(),
                                        "child_id" => spec.child_id.clone()
                                    ).increment(1);
                                } else {
                                    attached_count += 1;
                                }
                            }
                            
                            let facet_attach_duration = facet_attach_start.elapsed();
                            metrics::histogram!("plexspaces_supervisor_facet_attach_duration_seconds",
                                "supervisor_id" => self.id.clone(),
                                "child_id" => spec.child_id.clone()
                            ).record(facet_attach_duration.as_secs_f64());
                            metrics::counter!("plexspaces_supervisor_facets_attached_total",
                                "supervisor_id" => self.id.clone(),
                                "child_id" => spec.child_id.clone()
                            ).increment(attached_count);
                            
                            debug!(
                                supervisor_id = %self.id,
                                child_id = %spec.child_id,
                                facet_count = spec.facets.len(),
                                attached_count = attached_count,
                                duration_ms = facet_attach_duration.as_millis(),
                                "Attached facets from ChildSpec before starting actor"
                            );
                        } else {
                            debug!(
                                supervisor_id = %self.id,
                                child_id = %spec.child_id,
                                facet_count = spec.facets.len(),
                                "FacetRegistry not available - facets not attached (graceful degradation)"
                            );
                        }
                    }
                }
                
                // Start the actor (calls init() and registers in ActorRegistry)
                // Facets are already attached, so facet lifecycle hooks will be called during start()
                let handle = actor.start().await
                    .map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?;

                // Convert ChildSpec to ActorSpec for compatibility
                let actor_spec = ActorSpec {
                    id: spec.actor_or_supervisor_id.clone(),
                    factory: Arc::new(move || {
                        // This factory won't be used since actor is already created
                        Err(ActorError::InvalidState("Actor already created".to_string()))
                    }),
                    restart: match spec.restart_strategy {
                        crate::child_spec::RestartStrategy::Permanent => RestartPolicy::Permanent,
                        crate::child_spec::RestartStrategy::Transient => RestartPolicy::Transient,
                        crate::child_spec::RestartStrategy::Temporary => RestartPolicy::Temporary,
                    },
                    child_type: match spec.child_type {
                        crate::child_spec::ChildType::Actor => super::ChildType::Worker,
                        crate::child_spec::ChildType::Supervisor => super::ChildType::Supervisor,
                    },
                    shutdown_timeout_ms: spec.shutdown_timeout.map(|d| d.as_millis() as u64),
                };

                let supervised = SupervisedActor {
                    actor: Arc::new(RwLock::new(actor)),
                    actor_ref: ActorRef::new(spec.actor_or_supervisor_id.clone())
                        .map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?,
                    handle: Some(handle),
                    restart_count: 0,
                    last_restart: None,
                    restart_history: Vec::new(),
                    spec: actor_spec,
                    facets: spec.facets.clone(), // Store facets for restart (Phase 1: Unified Lifecycle)
                };

                // Add to children
                let mut children = self.children.write().await;
                children.insert(spec.actor_or_supervisor_id.clone(), supervised);
                drop(children);

                // Phase 3: Register parent-child relationship
                if let Some(service_locator) = &self.service_locator {
                    if let Some(registry) = service_locator.get_service::<plexspaces_core::ActorRegistry>().await {
                        let supervisor_id = ActorId::from(self.id.clone());
                        registry.register_parent_child(&supervisor_id, &spec.actor_or_supervisor_id).await;
                    }
                }

                // Send event
                let _ = self.event_tx.send(SupervisorEvent::ChildStarted(spec.actor_or_supervisor_id.clone())).await;

                Ok(spec.actor_or_supervisor_id)
            }
            StartedChild::Supervisor { supervisor } => {
                // For supervisor children, we need to add them via add_supervisor_child
                // This is a simplified version - full implementation would handle event_rx
                Err(SupervisorError::ActorCreationFailed(
                    "Supervisor children must be added via add_supervisor_child()".to_string()
                ))
            }
        }
    }

    /// Stop and remove a child dynamically (Phase 4)
    ///
    /// ## Purpose
    /// Stops and removes a child from a running supervisor.
    /// This is an alias for `remove_child()` for consistency with Erlang/OTP naming.
    ///
    /// ## Arguments
    /// * `child_id` - ID of the child to delete
    ///
    /// ## Returns
    /// Ok(()) on success, SupervisorError otherwise
    #[instrument(skip(self), fields(supervisor_id = %self.id, child_id = %child_id))]
    pub async fn delete_child(&mut self, child_id: &str) -> Result<(), SupervisorError> {
        self.remove_child(&ActorId::from(child_id.to_string())).await
    }

    /// Restart a specific child (Phase 4)
    ///
    /// ## Purpose
    /// Restarts a child that has failed or needs to be restarted.
    /// Uses the child's original spec to recreate it.
    ///
    /// ## Arguments
    /// * `child_id` - ID of the child to restart
    ///
    /// ## Returns
    /// Ok(()) on success, SupervisorError otherwise
    #[instrument(skip(self), fields(supervisor_id = %self.id, child_id = %child_id))]
    pub async fn restart_child(&mut self, child_id: &str) -> Result<(), SupervisorError> {
        let actor_id = ActorId::from(child_id.to_string());
        // Use existing restart_actor method (it's private, so we'll make it public or use a different approach)
        // For now, we'll implement restart logic inline
        let mut children = self.children.write().await;
        if let Some(child) = children.get_mut(&actor_id) {
            // Stop the current actor
            if let Some(handle) = child.handle.take() {
                handle.abort();
            }
            if let Ok(mut actor) = child.actor.try_write() {
                let _ = actor.stop().await;
            }
            
            // Recreate actor using the spec
            let spec = child.spec.clone();
            drop(children);
            
            // Create new actor
            let mut new_actor = (spec.factory)().map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?;
            let handle = new_actor.start().await
                .map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?;
            
            // Update child
            let mut children = self.children.write().await;
            if let Some(child) = children.get_mut(&actor_id) {
                child.actor = Arc::new(RwLock::new(new_actor));
                child.handle = Some(handle);
                child.restart_count += 1;
                child.last_restart = Some(tokio::time::Instant::now());
            }
            
            Ok(())
        } else {
            Err(SupervisorError::ChildNotFound(actor_id))
        }
    }

    /// List all children (Phase 4)
    ///
    /// ## Purpose
    /// Returns information about all children managed by this supervisor.
    /// This is the Erlang/OTP `supervisor:which_children/1` equivalent.
    ///
    /// ## Returns
    /// Vector of child information (ID, type, status)
    pub async fn which_children(&self) -> Vec<ChildInfo> {
        use plexspaces_proto::supervision::v1::ChildStatus;
        
        let mut result = Vec::new();
        
        // Get actor children
        let children = self.children.read().await;
        for (id, child) in children.iter() {
            let status = if child.handle.is_some() {
                ChildStatus::ChildStatusRunning
            } else {
                ChildStatus::ChildStatusStopped
            };
            
            result.push(ChildInfo {
                child_id: id.clone(),
                child_type: match child.spec.child_type {
                    ChildType::Worker => plexspaces_proto::supervision::v1::ChildType::ChildTypeActor,
                    ChildType::Supervisor => plexspaces_proto::supervision::v1::ChildType::ChildTypeSupervisor,
                } as i32,
                status: status as i32,
                restart_count: child.restart_count,
                pid: id.clone(), // In Erlang, this would be the Pid
            });
        }
        drop(children);
        
        // Get supervisor children
        let child_supervisors = self.child_supervisors.read().await;
        for (id, supervised) in child_supervisors.iter() {
            let status = if supervised.handle.is_some() {
                ChildStatus::ChildStatusRunning
            } else {
                ChildStatus::ChildStatusStopped
            };
            
            result.push(ChildInfo {
                child_id: id.clone(),
                child_type: plexspaces_proto::supervision::v1::ChildType::ChildTypeSupervisor as i32,
                status: status as i32,
                restart_count: supervised.restart_count,
                pid: id.clone(),
            });
        }
        
        result
    }

    /// Count children by type (Phase 4)
    ///
    /// ## Purpose
    /// Returns counts of children grouped by type.
    /// This is the Erlang/OTP `supervisor:count_children/1` equivalent.
    ///
    /// ## Returns
    /// ChildCount with actor and supervisor counts
    pub async fn count_children(&self) -> ChildCount {
        let children = self.children.read().await;
        let child_supervisors = self.child_supervisors.read().await;
        
        ChildCount {
            actors: children.len() as u32,
            supervisors: child_supervisors.len() as u32,
            total: (children.len() + child_supervisors.len()) as u32,
        }
    }

    /// Get child specification (Phase 4)
    ///
    /// ## Purpose
    /// Returns the ChildSpec for a given child ID.
    /// This is the Erlang/OTP `supervisor:get_childspec/2` equivalent.
    ///
    /// ## Arguments
    /// * `child_id` - ID of the child
    ///
    /// ## Returns
    /// Some(ChildSpec) if child exists, None otherwise
    pub async fn get_childspec(&self, child_id: &str) -> Option<crate::child_spec::ChildSpec> {
        let actor_id = ActorId::from(child_id.to_string());
        
        // Check actor children
        let children = self.children.read().await;
        if let Some(child) = children.get(&actor_id) {
            // Extract short child_id from actor_id (remove "@node" part if present)
            // This is a heuristic - ideally we'd store child_id separately in ActorSpec
            let short_child_id = if let Some(at_pos) = child.spec.id.to_string().find('@') {
                child.spec.id.to_string()[..at_pos].to_string()
            } else {
                child.spec.id.to_string()
            };
            
            // Convert ActorSpec to ChildSpec
            let spec = crate::child_spec::ChildSpec {
                child_id: short_child_id,
                actor_or_supervisor_id: child.spec.id.clone(),
                restart_strategy: match child.spec.restart {
                    RestartPolicy::Permanent => crate::child_spec::RestartStrategy::Permanent,
                    RestartPolicy::Transient => crate::child_spec::RestartStrategy::Transient,
                    RestartPolicy::Temporary => crate::child_spec::RestartStrategy::Temporary,
                    RestartPolicy::ExponentialBackoff { .. } => crate::child_spec::RestartStrategy::Permanent, // Default
                },
                shutdown_timeout: child.spec.shutdown_timeout_ms.map(|ms| Duration::from_millis(ms)),
                child_type: match child.spec.child_type {
                    super::ChildType::Worker => crate::child_spec::ChildType::Actor,
                    super::ChildType::Supervisor => crate::child_spec::ChildType::Supervisor,
                },
                metadata: std::collections::HashMap::new(),
                facets: Vec::new(), // Facets not stored in ActorSpec - would need to be added
                start_fn: Arc::new(|| {
                    // Return error - this is just for inspection, not for restarting
                    Box::pin(async move {
                        Err(ActorError::InvalidState("get_childspec() returns read-only spec".to_string()))
                    })
                }),
            };
            return Some(spec);
        }
        drop(children);
        
        // Check supervisor children
        let child_supervisors = self.child_supervisors.read().await;
        if child_supervisors.contains_key(child_id) {
            // For supervisor children, we don't have full ChildSpec stored
            // Return a minimal spec
            return Some(crate::child_spec::ChildSpec {
                child_id: child_id.to_string(),
                actor_or_supervisor_id: child_id.to_string(),
                restart_strategy: crate::child_spec::RestartStrategy::Permanent,
                shutdown_timeout: None,
                child_type: crate::child_spec::ChildType::Supervisor,
                metadata: std::collections::HashMap::new(),
                facets: Vec::new(), // Facets not stored in SupervisedSupervisor - would need to be added
                start_fn: Arc::new(|| {
                    Box::pin(async move {
                        Err(ActorError::InvalidState("get_childspec() returns read-only spec".to_string()))
                    })
                }),
            });
        }
        
        None
    }

    /// Build supervisor from SupervisorConfig (Phase 4)
    ///
    /// ## Purpose
    /// Creates a supervisor from a SupervisorConfig proto message.
    /// This enables declarative supervisor creation from configuration.
    ///
    /// ## Arguments
    /// * `supervisor_id` - ID for the supervisor
    /// * `config` - SupervisorConfig proto message
    /// * `service_locator` - ServiceLocator for service access
    ///
    /// ## Returns
    /// Supervisor instance with all children configured
    ///
    /// ## Example
    /// ```rust,ignore
    /// let config = SupervisorConfig {
    ///     strategy: SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 60 },
    ///     max_restarts: 3,
    ///     within_period: Some(Duration::from_secs(60)),
    ///     children: vec![...],
    ///     metadata: HashMap::new(),
    /// };
    /// let supervisor = Supervisor::from_config("my-supervisor", config, service_locator).await?;
    /// ```
    pub async fn from_config(
        supervisor_id: String,
        config: plexspaces_proto::supervision::v1::SupervisorConfig,
        service_locator: Arc<ServiceLocator>,
    ) -> Result<(Self, mpsc::Receiver<SupervisorEvent>), SupervisorError> {
        // Convert proto SupervisionStrategy to Rust SupervisionStrategy
        // Proto enum: ONE_FOR_ONE = 1, ONE_FOR_ALL = 2, REST_FOR_ONE = 3
        let strategy = match config.strategy() as i32 {
            1 => { // ONE_FOR_ONE
                SupervisionStrategy::OneForOne {
                    max_restarts: config.max_restarts as u32,
                    within_seconds: config.within_period
                        .as_ref()
                        .map(|d| d.seconds as u64)
                        .unwrap_or(60),
                }
            }
            2 => { // ONE_FOR_ALL
                SupervisionStrategy::OneForAll {
                    max_restarts: config.max_restarts as u32,
                    within_seconds: config.within_period
                        .as_ref()
                        .map(|d| d.seconds as u64)
                        .unwrap_or(60),
                }
            }
            3 => { // REST_FOR_ONE
                SupervisionStrategy::RestForOne {
                    max_restarts: config.max_restarts as u32,
                    within_seconds: config.within_period
                        .as_ref()
                        .map(|d| d.seconds as u64)
                        .unwrap_or(60),
                }
            }
            _ => {
                return Err(SupervisorError::ActorCreationFailed(
                    format!("Unsupported supervision strategy: {:?}", config.strategy())
                ));
            }
        };

        // Create supervisor
        let (mut supervisor, event_rx) = Supervisor::new(supervisor_id, strategy);
        supervisor = supervisor.with_service_locator(service_locator);

        // Add children from config
        // NOTE: ChildSpec proto cannot fully reconstruct Rust ChildSpec because
        // the `start_fn` field is not serializable. Children must be added via
        // `start_child()` with proper start functions, or via a factory registry
        // that can create start functions from metadata (e.g., "start_module").
        //
        // For now, we validate the config but don't add children automatically.
        // The caller should:
        // 1. Use `from_config()` to create the supervisor structure
        // 2. For each child in config.children, call `start_child()` with a proper ChildSpec
        //    that includes the start_fn factory function.
        //
        // Future enhancement: Add a ChildFactoryRegistry that can create start functions
        // from metadata (start_module, start_function) for dynamic child creation.
        
        // Validate children config (but don't add them yet)
        for (idx, child_spec_proto) in config.children.iter().enumerate() {
            if child_spec_proto.child_id.is_empty() {
                return Err(SupervisorError::ActorCreationFailed(
                    format!("Child at index {} has empty child_id", idx)
                ));
            }
            if child_spec_proto.actor_or_supervisor_id.is_empty() {
                return Err(SupervisorError::ActorCreationFailed(
                    format!("Child {} has empty actor_or_supervisor_id", child_spec_proto.child_id)
                ));
            }
            // Validate child type
            let child_type = plexspaces_proto::supervision::v1::ChildType::try_from(child_spec_proto.child_type)
                .map_err(|_| SupervisorError::ActorCreationFailed(
                    format!("Child {} has invalid child_type: {}", child_spec_proto.child_id, child_spec_proto.child_type)
                ))?;
            if child_type == plexspaces_proto::supervision::v1::ChildType::ChildTypeUnspecified {
                return Err(SupervisorError::ActorCreationFailed(
                    format!("Child {} has unspecified child_type", child_spec_proto.child_id)
                ));
            }
        }
        
        debug!(
            supervisor_id = %supervisor.id,
            child_count = config.children.len(),
            "Created supervisor from config (children must be added via start_child())"
        );
        
        Ok((supervisor, event_rx))
    }
}

/// Child information for which_children() (Phase 4)
#[derive(Debug, Clone)]
pub struct ChildInfo {
    pub child_id: String,
    pub child_type: i32, // Proto ChildType enum value
    pub status: i32,     // Proto ChildStatus enum value
    pub restart_count: u32,
    pub pid: String,    // Process ID (actor/supervisor ID)
}

/// Child count for count_children() (Phase 4)
#[derive(Debug, Clone)]
pub struct ChildCount {
    pub actors: u32,
    pub supervisors: u32,
    pub total: u32,
}

/// Calculate exponential backoff delay
fn calculate_backoff_delay(
    restart_count: u32,
    initial_delay_ms: u64,
    max_delay_ms: u64,
    factor: f64,
) -> u64 {
    let delay = initial_delay_ms as f64 * factor.powi(restart_count as i32);
    delay.min(max_delay_ms as f64) as u64
}

/// Supervisor errors
///
/// ## Error Types
/// All errors are returned when supervisor operations fail.
/// Errors are designed to be actionable and include context.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    /// Actor creation failed
    ///
    /// ## When This Occurs
    /// - Factory function returns an error
    /// - Actor initialization fails
    ///
    /// ## Context
    /// The error message includes the original error from the factory.
    #[error("Actor creation failed: {0}")]
    ActorCreationFailed(String),

    /// Child not found
    ///
    /// ## When This Occurs
    /// - Attempting to remove a child that doesn't exist
    /// - Attempting to restart a child that doesn't exist
    ///
    /// ## Context
    /// The `ActorId` of the missing child is included.
    #[error("Child not found: {0:?}")]
    ChildNotFound(ActorId),

    /// Maximum restarts exceeded
    ///
    /// ## When This Occurs
    /// - Child has been restarted more than `max_restarts` times
    /// - Restarts occurred within the `within_seconds` time window
    ///
    /// ## Behavior
    /// When this error occurs, the supervisor stops attempting to restart
    /// the child and may escalate to the parent supervisor (if present).
    #[error("Max restarts exceeded")]
    MaxRestartsExceeded,

    /// Restart failed
    ///
    /// ## When This Occurs
    /// - Actor factory fails during restart
    /// - Actor start fails during restart
    ///
    /// ## Context
    /// The error message includes the original error from the restart attempt.
    #[error("Restart failed: {0}")]
    RestartFailed(String),

    /// Invalid supervision strategy
    ///
    /// ## When This Occurs
    /// - Unknown strategy type is provided
    /// - Strategy configuration is invalid
    ///
    /// ## Context
    /// The error message includes the invalid strategy identifier or description.
    #[error("Invalid strategy: {0}")]
    InvalidStrategy(String),
}

/// Supervisor builder for fluent API
pub struct SupervisorBuilder {
    id: String,
    strategy: SupervisionStrategy,
    children: Vec<ActorSpec>,
    parent: Option<Arc<Supervisor>>,
}

impl SupervisorBuilder {
    /// Create a new supervisor builder
    pub fn new(id: String) -> Self {
        SupervisorBuilder {
            id,
            strategy: SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
            children: Vec::new(),
            parent: None,
        }
    }

    /// Set supervision strategy
    pub fn with_strategy(mut self, strategy: SupervisionStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Add a child specification
    pub fn add_child(mut self, spec: ActorSpec) -> Self {
        self.children.push(spec);
        self
    }

    /// Set parent supervisor
    pub fn with_parent(mut self, parent: Arc<Supervisor>) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Build the supervisor
    pub async fn build(
        self,
    ) -> Result<(Supervisor, mpsc::Receiver<SupervisorEvent>), SupervisorError> {
        let (mut supervisor, event_rx) = Supervisor::new(self.id, self.strategy);
        
        // Set ServiceLocator for the supervisor (required for add_child)
        // For tests, create a minimal ServiceLocator
        // Tests that need full services should register them explicitly
        let service_locator = Arc::new(ServiceLocator::new());
        supervisor = supervisor.with_service_locator(service_locator);

        if let Some(parent) = self.parent {
            supervisor = supervisor.with_parent(parent);
        }

        // Add all children
        for spec in self.children {
            supervisor.add_child(spec).await?;
        }

        Ok((supervisor, event_rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_behavior::MockBehavior;
    use plexspaces_mailbox::{Mailbox, MailboxConfig};

    /// Helper function to create an ActorSpec with a mailbox created asynchronously
    /// This avoids the need to use block_on() inside factory closures
    async fn create_actor_spec(
        id: String,
        restart: RestartPolicy,
    ) -> ActorSpec {
        let mailbox = Mailbox::new(MailboxConfig::default(), id.clone())
            .await
            .expect("Failed to create mailbox");
        
        ActorSpec {
            id: id.clone(),
            factory: Arc::new(move || {
                // Create a new mailbox for each factory call
                // Note: This still requires async, but we'll handle it differently
                // by creating the mailbox in the test and cloning the config
                let mailbox = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(
                        Mailbox::new(MailboxConfig::default(), id.clone())
                    )
                }).expect("Failed to create mailbox in factory");
                Ok(Actor::new(
                    id.clone(),
                    Box::new(MockBehavior::new()),
                    mailbox,
                    "test-tenant".to_string(),
                    "test".to_string(),
                    None,
                ))
            }),
            restart,
            child_type: ChildType::Worker,
            shutdown_timeout_ms: Some(5000),
        }
    }

    /// Helper function to create a test supervisor with ServiceLocator
    fn create_test_supervisor(
        id: String,
        strategy: SupervisionStrategy,
    ) -> (Supervisor, mpsc::Receiver<SupervisorEvent>) {
        let (supervisor, event_rx) = Supervisor::new(id, strategy);
        let service_locator = Arc::new(ServiceLocator::new());
        (supervisor.with_service_locator(service_locator), event_rx)
    }

    /// Helper function to create an ActorSpec with mailbox created synchronously
    /// This uses a separate thread to avoid blocking the async runtime
    fn create_actor_spec_sync(
        id: String,
        restart: RestartPolicy,
    ) -> ActorSpec {
        let id_clone = id.clone();
        ActorSpec {
            id: id.clone(),
            factory: Arc::new(move || {
                let actor_id = id_clone.clone();
                // Create a new runtime on a separate thread to avoid blocking async runtime
                let mailbox = std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to create runtime for mailbox");
                    rt.block_on(
                        Mailbox::new(MailboxConfig::default(), actor_id.clone())
                    )
                })
                .join()
                .expect("Thread panicked")
                .expect("Failed to create mailbox in factory");
                Ok(Actor::new(
                    id_clone.clone(),
                    Box::new(MockBehavior::new()),
                    mailbox,
                    "test-tenant".to_string(),
                    "test".to_string(),
                    None,
                ))
            }),
            restart,
            child_type: ChildType::Worker,
            shutdown_timeout_ms: Some(5000),
        }
    }

    #[tokio::test]
    async fn test_supervisor_creation() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
        );

        // Add a child
        let spec = create_actor_spec_sync(
            "test-child@localhost".to_string(),
            RestartPolicy::Permanent,
        );

        let actor_ref = supervisor.add_child(spec).await.unwrap();
        assert_eq!(actor_ref.id().as_str(), "test-child@localhost");

        // Check event
        if let Some(event) = event_rx.recv().await {
            match event {
                SupervisorEvent::ChildStarted(id) => {
                    assert_eq!(id.as_str(), "test-child@localhost");
                }
                _ => panic!("Unexpected event"),
            }
        }
    }

    #[test]
    fn test_backoff_calculation() {
        assert_eq!(calculate_backoff_delay(0, 100, 10000, 2.0), 100);
        assert_eq!(calculate_backoff_delay(1, 100, 10000, 2.0), 200);
        assert_eq!(calculate_backoff_delay(2, 100, 10000, 2.0), 400);
        assert_eq!(calculate_backoff_delay(10, 100, 10000, 2.0), 10000); // Capped at max
    }

    #[tokio::test]
    async fn test_supervisor_builder() {
        let spec = create_actor_spec_sync(
            "worker-1@localhost".to_string(),
            RestartPolicy::Transient,
        );

        let (_supervisor, _event_rx) = SupervisorBuilder::new("root".to_string())
            .with_strategy(SupervisionStrategy::OneForAll {
                max_restarts: 5,
                within_seconds: 30,
            })
            .add_child(spec)
            .build()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_remove_child() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
        );

        // Add a child
        let spec = create_actor_spec_sync(
            "removable-child@localhost".to_string(),
            RestartPolicy::Permanent,
        );

        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // Consume ChildStarted event

        // Remove the child
        supervisor
            .remove_child(&"removable-child@localhost".to_string())
            .await
            .unwrap();

        // Check event
        if let Some(event) = event_rx.recv().await {
            match event {
                SupervisorEvent::ChildStopped(id) => {
                    assert_eq!(id.as_str(), "removable-child@localhost");
                }
                _ => panic!("Expected ChildStopped event"),
            }
        }
    }

    #[tokio::test]
    async fn test_remove_nonexistent_child() {
        let (supervisor, _event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
        );

        // Try to remove a child that doesn't exist
        let result = supervisor
            .remove_child(&"nonexistent@localhost".to_string())
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SupervisorError::ChildNotFound(_) => (),
            _ => panic!("Expected ChildNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_handle_failure_one_for_one() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
        );

        // Add a child
        let spec = create_actor_spec_sync(
            "failing-child@localhost".to_string(),
            RestartPolicy::Permanent,
        );

        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // Consume ChildStarted

        // Handle failure
        supervisor
            .handle_failure(
                &"failing-child@localhost".to_string(),
                "test error".to_string(),
            )
            .await
            .unwrap();

        // Check for ChildFailed event
        let event = event_rx.recv().await.unwrap();
        match event {
            SupervisorEvent::ChildFailed(id, reason) => {
                assert_eq!(id.as_str(), "failing-child@localhost");
                assert_eq!(reason, "test error");
            }
            _ => panic!("Expected ChildFailed event, got {:?}", event),
        }

        // Check for ChildRestarted event
        let event = event_rx.recv().await.unwrap();
        match event {
            SupervisorEvent::ChildRestarted(id, count) => {
                assert_eq!(id.as_str(), "failing-child@localhost");
                assert_eq!(count, 1); // First restart
            }
            _ => panic!("Expected ChildRestarted event, got {:?}", event),
        }
    }

    #[tokio::test]
    async fn test_temporary_restart_policy() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
        );

        // Add a temporary child (should not restart)
        let spec = create_actor_spec_sync(
            "temp-child@localhost".to_string(),
            RestartPolicy::Temporary,
        );

        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // Consume ChildStarted

        // Handle failure
        supervisor
            .handle_failure(
                &"temp-child@localhost".to_string(),
                "test error".to_string(),
            )
            .await
            .unwrap();

        // Should get ChildFailed but NOT ChildRestarted
        let event = event_rx.recv().await.unwrap();
        match event {
            SupervisorEvent::ChildFailed(_, _) => (),
            _ => panic!("Expected ChildFailed event"),
        }

        // No restart event should follow for Temporary (try_recv should fail immediately)
        assert!(
            event_rx.try_recv().is_err(),
            "Should not restart temporary actor"
        );
    }

    #[tokio::test]
    async fn test_max_restarts_exceeded() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 2, // Low limit to test
                within_seconds: 60,
            },
        );

        let spec = create_actor_spec_sync(
            "crash-child@localhost".to_string(),
            RestartPolicy::Permanent,
        );

        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // ChildStarted

        // Trigger failures until max restarts exceeded
        for i in 0..3 {
            let result = supervisor
                .handle_failure(&"crash-child@localhost".to_string(), format!("crash {}", i))
                .await;

            let _ = event_rx.recv().await; // ChildFailed

            if i < 2 {
                // Should succeed
                assert!(result.is_ok());
                let _ = event_rx.recv().await; // ChildRestarted
            } else {
                // Third failure should exceed limit
                assert!(result.is_err());
                match result.unwrap_err() {
                    SupervisorError::MaxRestartsExceeded => (),
                    e => panic!("Expected MaxRestartsExceeded, got {:?}", e),
                }

                // Should get MaxRestartsExceeded event
                let event = event_rx.recv().await.unwrap();
                match event {
                    SupervisorEvent::MaxRestartsExceeded(id) => {
                        assert_eq!(id.as_str(), "crash-child@localhost");
                    }
                    _ => panic!("Expected MaxRestartsExceeded event"),
                }
            }
        }
    }

    #[tokio::test]
    async fn test_supervisor_stats() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 5,
                within_seconds: 60,
            },
        );

        let spec = create_actor_spec_sync(
            "stats-child@localhost".to_string(),
            RestartPolicy::Permanent,
        );

        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // ChildStarted

        // Trigger a failure and restart
        supervisor
            .handle_failure(
                &"stats-child@localhost".to_string(),
                "test error".to_string(),
            )
            .await
            .unwrap();

        let _ = event_rx.recv().await; // ChildFailed
        let _ = event_rx.recv().await; // ChildRestarted

        // Check stats
        let stats = supervisor.stats().await;
        assert_eq!(stats.total_restarts, 1);
        assert_eq!(stats.successful_restarts, 1);
        assert_eq!(stats.failed_restarts, 0);
        assert_eq!(stats.failure_patterns.get("test error"), Some(&1));
    }

    #[tokio::test]
    async fn test_shutdown() {
        let (mut supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
        );

        // Add multiple children (reduced to 2 for faster tests)
        for i in 0..2 {
            let id = format!("child-{}@localhost", i);
            let spec = create_actor_spec_sync(id.clone(), RestartPolicy::Permanent);

            supervisor.add_child(spec).await.unwrap();
            let _ = event_rx.recv().await; // ChildStarted
        }

        // Shutdown all children
        supervisor.shutdown().await.unwrap();

        // Should get ChildStopped for all 2 children
        for _ in 0..2 {
            let event = event_rx.recv().await.unwrap();
            match event {
                SupervisorEvent::ChildStopped(_) => (),
                _ => panic!("Expected ChildStopped event"),
            }
        }
    }

    #[test]
    fn test_supervision_strategy_serialization() {
        // Test OneForOne
        let strategy = SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        };
        let json = serde_json::to_string(&strategy).unwrap();
        let deserialized: SupervisionStrategy = serde_json::from_str(&json).unwrap();
        match deserialized {
            SupervisionStrategy::OneForOne {
                max_restarts,
                within_seconds,
            } => {
                assert_eq!(max_restarts, 3);
                assert_eq!(within_seconds, 60);
            }
            _ => panic!("Wrong strategy type"),
        }

        // Test OneForAll
        let strategy = SupervisionStrategy::OneForAll {
            max_restarts: 5,
            within_seconds: 30,
        };
        let json = serde_json::to_string(&strategy).unwrap();
        let _: SupervisionStrategy = serde_json::from_str(&json).unwrap();

        // Test RestForOne
        let strategy = SupervisionStrategy::RestForOne {
            max_restarts: 2,
            within_seconds: 120,
        };
        let json = serde_json::to_string(&strategy).unwrap();
        let _: SupervisionStrategy = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn test_restart_policy_serialization() {
        // Test all restart policy variants
        let policies = vec![
            RestartPolicy::Permanent,
            RestartPolicy::Transient,
            RestartPolicy::Temporary,
            RestartPolicy::ExponentialBackoff {
                initial_delay_ms: 100,
                max_delay_ms: 10000,
                factor: 2.0,
            },
        ];

        for policy in policies {
            let json = serde_json::to_string(&policy).unwrap();
            let _: RestartPolicy = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_child_type_serialization() {
        let worker = ChildType::Worker;
        let json = serde_json::to_string(&worker).unwrap();
        let _: ChildType = serde_json::from_str(&json).unwrap();

        let supervisor = ChildType::Supervisor;
        let json = serde_json::to_string(&supervisor).unwrap();
        let _: ChildType = serde_json::from_str(&json).unwrap();
    }

    #[tokio::test]
    async fn test_one_for_all_strategy() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForAll {
                max_restarts: 3,
                within_seconds: 60,
            },
        );

        // Add 3 children
        for i in 0..3 {
            let id = format!("child-{}@localhost", i);
            let spec = create_actor_spec_sync(id.clone(), RestartPolicy::Permanent);
            supervisor.add_child(spec).await.unwrap();
            let _ = event_rx.recv().await; // Consume ChildStarted
        }

        // Trigger failure on child-1
        supervisor
            .handle_failure(&"child-1@localhost".to_string(), "test error".to_string())
            .await
            .unwrap();

        // Should get ChildFailed for child-1
        let event = event_rx.recv().await.unwrap();
        match event {
            SupervisorEvent::ChildFailed(id, _) => {
                assert_eq!(id.as_str(), "child-1@localhost");
            }
            _ => panic!("Expected ChildFailed event"),
        }

        // OneForAll should restart ALL children (0, 1, 2)
        // Collect all restart events
        let mut restarted_ids = Vec::new();
        for _ in 0..3 {
            let event = event_rx.recv().await.unwrap();
            match event {
                SupervisorEvent::ChildRestarted(id, _) => {
                    restarted_ids.push(id.as_str().to_string());
                }
                _ => panic!("Expected ChildRestarted event"),
            }
        }

        // All 3 children should be restarted
        assert_eq!(restarted_ids.len(), 3);
        assert!(restarted_ids.contains(&"child-0@localhost".to_string()));
        assert!(restarted_ids.contains(&"child-1@localhost".to_string()));
        assert!(restarted_ids.contains(&"child-2@localhost".to_string()));
    }

    #[tokio::test]
    async fn test_rest_for_one_strategy() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::RestForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
        );

        // Add 3 children
        for i in 0..3 {
            let id = format!("child-{}@localhost", i);
            let spec = create_actor_spec_sync(id.clone(), RestartPolicy::Permanent);
            supervisor.add_child(spec).await.unwrap();
            let _ = event_rx.recv().await; // Consume ChildStarted
        }

        // Trigger failure on child-1
        supervisor
            .handle_failure(&"child-1@localhost".to_string(), "test error".to_string())
            .await
            .unwrap();

        // Should get ChildFailed for child-1
        let event = event_rx.recv().await.unwrap();
        match event {
            SupervisorEvent::ChildFailed(id, _) => {
                assert_eq!(id.as_str(), "child-1@localhost");
            }
            _ => panic!("Expected ChildFailed event"),
        }

        // RestForOne should restart the failed child (for now, until we implement child ordering)
        // TODO: When child ordering (Vec) is implemented, this will restart child-1 and all after it
        let event = event_rx.recv().await.unwrap();
        match event {
            SupervisorEvent::ChildRestarted(id, count) => {
                assert_eq!(id.as_str(), "child-1@localhost");
                assert_eq!(count, 1);
            }
            _ => panic!("Expected ChildRestarted event"),
        }
    }

    #[tokio::test]
    async fn test_adaptive_strategy() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::Adaptive {
                initial_strategy: Box::new(SupervisionStrategy::OneForOne {
                    max_restarts: 5,
                    within_seconds: 60,
                }),
                learning_rate: 0.1,
            },
        );

        // Add a child
        let spec = create_actor_spec_sync(
            "adaptive-child@localhost".to_string(),
            RestartPolicy::Permanent,
        );

        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // Consume ChildStarted

        // Trigger failure with adaptive strategy
        supervisor
            .handle_failure(
                &"adaptive-child@localhost".to_string(),
                "test error".to_string(),
            )
            .await
            .unwrap();

        // Should get ChildFailed
        let event = event_rx.recv().await.unwrap();
        match event {
            SupervisorEvent::ChildFailed(_, _) => (),
            _ => panic!("Expected ChildFailed event"),
        }

        // Should get ChildRestarted (adaptive strategy uses initial OneForOne)
        let event = event_rx.recv().await.unwrap();
        match event {
            SupervisorEvent::ChildRestarted(id, count) => {
                assert_eq!(id.as_str(), "adaptive-child@localhost");
                assert_eq!(count, 1);
            }
            _ => panic!("Expected ChildRestarted event"),
        }
    }

    #[tokio::test]
    async fn test_transient_restart_normal_exit() {
        // TODO: Implement when we can distinguish normal vs abnormal exit
        // For now, transient policy restarts on all failures
    }

    #[tokio::test]
    async fn test_exponential_backoff_policy() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 5,
                within_seconds: 60,
            },
        );

        // Add child with exponential backoff
        let id = "backoff-child@localhost".to_string();
        let spec = ActorSpec {
            id: id.clone(),
            factory: Arc::new(move || {
                let actor_id = id.clone();
                // Create a new runtime on a separate thread to avoid blocking async runtime
                let mailbox = std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to create runtime for mailbox");
                    rt.block_on(
                        Mailbox::new(MailboxConfig::default(), actor_id.clone())
                    )
                })
                .join()
                .expect("Thread panicked")
                .expect("Failed to create mailbox in factory");
                Ok(Actor::new(
                    id.clone(),
                    Box::new(MockBehavior::new()),
                    mailbox,
                    "test-tenant".to_string(),
                    "test".to_string(),
                    None,
                ))
            }),
            restart: RestartPolicy::ExponentialBackoff {
                initial_delay_ms: 10, // Short delay for test
                max_delay_ms: 100,
                factor: 2.0,
            },
            child_type: ChildType::Worker,
            shutdown_timeout_ms: Some(5000),
        };

        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // Consume ChildStarted

        // Measure restart with backoff
        let start = tokio::time::Instant::now();

        supervisor
            .handle_failure(
                &"backoff-child@localhost".to_string(),
                "test error".to_string(),
            )
            .await
            .unwrap();

        let _ = event_rx.recv().await; // ChildFailed
        let _ = event_rx.recv().await; // ChildRestarted

        let elapsed = start.elapsed();

        // Should have waited at least initial_delay_ms (10ms)
        // Using 5ms threshold to account for timing variations
        assert!(
            elapsed.as_millis() >= 5,
            "Expected backoff delay, got {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_with_parent_supervisor() {
        let (parent, _parent_rx) = create_test_supervisor(
            "parent-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
        );

        let parent = Arc::new(parent);

        let (mut child_supervisor, _child_rx) = create_test_supervisor(
            "child-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
        );

        child_supervisor = child_supervisor.with_parent(parent.clone());

        // Verify parent is set (we can't directly access it due to privacy, but test compiles)
        let strategy = child_supervisor.strategy.read().await.clone();
        match strategy {
            SupervisionStrategy::OneForOne {
                max_restarts,
                within_seconds,
            } => {
                assert_eq!(max_restarts, 3);
                assert_eq!(within_seconds, 60);
            }
            _ => panic!("Wrong strategy type"),
        }
    }

    #[tokio::test]
    async fn test_restart_intensity_window_reset() {
        let (supervisor, mut event_rx) = create_test_supervisor(
            "test-supervisor".to_string(),
            SupervisionStrategy::OneForOne {
                max_restarts: 2,
                within_seconds: 1, // 1 second window
            },
        );

        let spec = create_actor_spec_sync(
            "window-child@localhost".to_string(),
            RestartPolicy::Permanent,
        );

        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // ChildStarted

        // First restart
        supervisor
            .handle_failure(&"window-child@localhost".to_string(), "error 1".to_string())
            .await
            .unwrap();
        let _ = event_rx.recv().await; // ChildFailed
        let _ = event_rx.recv().await; // ChildRestarted

        // Second restart (within window)
        supervisor
            .handle_failure(&"window-child@localhost".to_string(), "error 2".to_string())
            .await
            .unwrap();
        let _ = event_rx.recv().await; // ChildFailed
        let _ = event_rx.recv().await; // ChildRestarted

        // Wait for window to expire
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Third restart (outside window - should reset counter)
        let result = supervisor
            .handle_failure(&"window-child@localhost".to_string(), "error 3".to_string())
            .await;

        // Should succeed because counter was reset
        assert!(
            result.is_ok(),
            "Expected restart to succeed after window reset"
        );
        let _ = event_rx.recv().await; // ChildFailed
        let _ = event_rx.recv().await; // ChildRestarted
    }
}
