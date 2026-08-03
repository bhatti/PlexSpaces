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

//! Supervision tree construction, traversal, and lifecycle management.

use async_trait::async_trait;
use indexmap::IndexMap;
use metrics;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout as tokio_timeout;
use tracing::{debug, error, info, instrument, trace, warn};

use crate::core::{
    ActorContext, ActorError, ActorId, ActorRef, ServiceLocator as ServiceLocatorTrait,
};
use crate::ActorRef as ActorActorRef;
use plexspaces_proto::actor::v1::ActorVisibility;

use plexspaces_proto::supervision::v1::{SupervisionError as ProtoError, SupervisorStats};

use super::{
    ActorShutdownInfo, RestartPolicy, SupervisedActor, SupervisedSupervisor, SupervisionStrategy,
    Supervisor, SupervisorError, SupervisorEvent, SupervisorShutdownInfo,
};

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

impl Supervisor {
    /// Create a new supervisor with required ServiceLocator
    ///
    /// ## Arguments
    /// * `supervisor_label` - Opaque handle for this supervisor (logging / metrics / local tables).
    ///   This is **not** an [`ActorId`] and must **not** be reused as a supervised child's
    ///   instance `name` or as `ActorIdentity.name` in application specs — workers and nested
    ///   supervisors each have their own canonical [`ActorId`] via [`crate::ChildSpec::actor_id`].
    /// * `strategy` - Supervision strategy (OneForOne, OneForAll, RestForOne)
    /// * `service_locator` - ServiceLocator for service access (required for ActorRef creation)
    ///
    /// ## Returns
    /// Tuple of (Supervisor, event receiver channel)
    pub fn new(
        supervisor_label: String,
        strategy: SupervisionStrategy,
        service_locator: Arc<dyn ServiceLocatorTrait>,
    ) -> (Self, mpsc::Receiver<SupervisorEvent>) {
        let (event_tx, event_rx) = mpsc::channel(100);
        let (_shutdown_tx, shutdown_rx) = mpsc::channel(1);

        let supervisor = Supervisor {
            id: supervisor_label,
            strategy: Arc::new(RwLock::new(strategy)),
            children: Arc::new(RwLock::new(IndexMap::new())),
            child_supervisors: Arc::new(RwLock::new(IndexMap::new())),
            parent: None,
            stats: Arc::new(RwLock::new(SupervisorStats::default())),
            event_tx,
            _shutdown_rx: Some(shutdown_rx),
            node: None, // No Node by default (standalone mode)
            service_locator: Some(service_locator),
            default_shutdown_timeout: None, // Use default 1 second for tests
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
    /// When LinkProvider is provided, supervisor uses links internally for cascading failures.
    /// This enables the Erlang/OTP pattern where supervision uses links.
    /// Pass an [`ActorRegistry`] (as `dyn LinkProvider`) or another implementation that
    /// establishes links with the same tenant-scoped [`RequestContext`] you use elsewhere.
    ///
    /// ## Arguments
    /// * `link_provider` - LinkProvider implementation (typically ActorRegistry)
    ///
    /// ## Returns
    /// Self for method chaining
    ///
    /// ## Example
    /// ```rust,ignore
    /// use crate::core::{ActorRegistry, ServiceLocator};
    /// let actor_registry: Arc<ActorRegistry> = service_locator.actor_registry().await.unwrap();
    /// supervisor.with_link_provider(actor_registry as Arc<dyn LinkProvider + Send + Sync>);
    /// ```
    ///
    pub fn with_link_provider(
        mut self,
        link_provider: Arc<dyn super::LinkProvider + Send + Sync>,
    ) -> Self {
        self.node = Some(link_provider);
        self
    }

    /// Add a child actor using ChildSpec (proto-first design)
    ///
    /// ## Arguments
    /// * `spec` - ChildSpec defining the child (uses async factory, supports facets)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let child_actor_id = ActorId::new("worker1", "worker", "ns", "node1").unwrap();
    /// let spec = ChildSpec::worker_sync(child_actor_id, Arc::new(|| Ok(actor)), actor_ref);
    /// supervisor.add_child(spec).await?;
    /// ```
    #[instrument(skip(self, spec), fields(supervisor_id = %self.id, child_id = %spec.actor_id))]
    pub async fn add_child(
        &self,
        spec: crate::ChildSpec,
    ) -> Result<ActorActorRef, SupervisorError> {
        use crate::child_spec::StartedChild;

        // Record facets on span for observability (which facets are attached when creating actor)
        let facet_count = spec.proto.facets.len();
        let facet_list: String = spec
            .proto
            .facets
            .iter()
            .map(|f| f.r#type.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        tracing::Span::current().record("facet_count", facet_count);
        if !facet_list.is_empty() {
            tracing::Span::current().record("facets", facet_list.as_str());
        }

        if tracing::enabled!(tracing::Level::TRACE) {
            trace!(
                supervisor_id = %self.id,
                child_id = %spec.actor_id,
                restart_policy = ?spec.restart_policy(),
                role = %spec.role(),
                facet_count = spec.proto.facets.len(),
                facets = %facet_list,
                "Adding child to supervisor"
            );
        }

        let child_id = spec.actor_id.clone();

        // Create the actor via async factory
        let started_child = (spec.start_fn)().await.map_err(|e| {
            error!(
                supervisor_id = %self.id,
                child_id = %child_id,
                error = %e,
                "Failed to create actor via factory"
            );
            SupervisorError::ActorCreationFailed(e.to_string())
        })?;

        // Extract actor from StartedChild (must be Worker type for add_child)
        let actor = match started_child {
            StartedChild::Worker { actor, .. } => actor,
            StartedChild::Supervisor { .. } => {
                return Err(SupervisorError::ActorCreationFailed(
                    "add_child expects Worker child, use add_supervisor for Supervisor children"
                        .to_string(),
                ));
            }
        };

        // Get mailbox reference before starting actor
        let mailbox = actor.mailbox().clone();

        // Get ServiceLocator (required for ActorRef creation and registry injection)
        let service_locator = self.service_locator.as_ref()
            .ok_or_else(|| SupervisorError::ActorCreationFailed(
                "ServiceLocator not set on Supervisor. Call with_service_locator() when creating Supervisor.".to_string()
            ))?
            .clone();

        // Inject the supervisor's service_locator into the actor's context so the
        // started actor can be registered in ActorRegistry afterward. Actor::new() always
        // starts with a TestServiceLocatorStub; we replace it here with the real one.
        let existing_ctx = actor.context().clone();
        // Create ActorRef from the actor crate (has tell() method) for return value.
        let actor_ref = ActorActorRef::local(
            child_id.clone(),
            existing_ctx.tenant_id.clone(),
            existing_ctx.namespace.clone(),
            mailbox,
            service_locator.clone(),
            ActorVisibility::ActorVisibilityPublic,
        );
        let self_ref = crate::core::ActorRef::new(child_id.clone())
            .map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?;
        let new_ctx = Arc::new(
            ActorContext::new(
                existing_ctx.node_id.clone(),
                existing_ctx.tenant_id.clone(),
                existing_ctx.namespace.clone(),
                service_locator.clone(),
                existing_ctx.config.clone(),
            )
            .with_self_ref(self_ref),
        );
        let mut actor = actor.set_context(new_ctx);

        // Create core ActorRef for internal storage
        let _core_actor_ref = ActorRef::new(child_id.clone())
            .map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?;

        // Start the actor (spawns message loop)
        let handle = actor
            .start()
            .await
            .map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?;

        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.actor_registry().await {
                actor
                    .register_started(&registry, ActorVisibility::ActorVisibilityPublic)
                    .await;
            }
        }

        let supervised = SupervisedActor {
            actor: Arc::new(RwLock::new(actor)),
            handle: Some(handle),
            restart_count: 0,
            last_restart: None,
            restart_timestamps: Vec::new(),
            spec: spec.clone(),
        };

        // Add to children
        let mut children = self.children.write().await;
        children.insert(child_id.clone(), supervised);
        drop(children);

        // Phase 3: Register parent-child relationship in ActorRegistry
        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.actor_registry().await {
                let supervisor_id = ActorId::from(self.id.clone());
                registry
                    .register_parent_child(&supervisor_id, &child_id)
                    .await;

                // OBSERVABILITY: Log parent-child registration
                trace!(
                    supervisor_id = %self.id,
                    child_id = %child_id,
                    "Registered parent-child relationship in ActorRegistry"
                );
            }
        }

        // Phase 8.5: Link Semantics - Link supervisor to child
        // This enables cascading failures (Erlang/OTP pattern)
        // System supervision operation - linking supervisor to child for fault tolerance
        // Use NodeConfig defaults for system operations
        if let Some(node) = &self.node {
            use crate::core::{RequestContext, RequestContextExt};
            let supervisor_id = ActorId::from(self.id.clone());
            // Tenant comes from auth, not config
            let ctx =
                RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);
            if let Err(e) = node.link(&ctx, &supervisor_id, &child_id).await {
                // Log error but don't fail - supervision can work without links
                warn!(
                    supervisor_id = %self.id,
                    child_id = %child_id,
                    error = %e,
                    "Failed to link supervisor to child (supervision will continue without links)"
                );
            } else {
                debug!(
                    supervisor_id = %self.id,
                    child_id = %child_id,
                    "Linked supervisor to child for cascading failures"
                );
            }
        }

        // OBSERVABILITY: Record metrics for child started (Phase 8)
        metrics::counter!("plexspaces_supervisor_child_started_total",
            "supervisor_id" => self.id.clone(),
            "child_id" => child_id.to_string()
        )
        .increment(1);

        // Send event
        let _ = self
            .event_tx
            .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildStarted as i32,
                actor_id: child_id.to_string(),
                ..Default::default()
            })
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
            // System supervision operation - unlinking supervisor from child
            // Use NodeConfig defaults for system operations
            use crate::core::{RequestContext, RequestContextExt};
            // Tenant comes from auth, not config
            let ctx =
                RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);
            let _ = node.unlink(&ctx, &supervisor_id, id).await; // Ignore errors (idempotent)
        }

        // Phase 3: Unregister parent-child relationship in ActorRegistry
        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.actor_registry().await {
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
                .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                    event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildStopped as i32,
                    actor_id: id.to_string(),
                    ..Default::default()
                })
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
    /// ```rust,ignore
    /// use plexspaces_supervisor::*;
    /// use plexspaces_proto::supervision::v1::EventPropagation;
    /// use std::sync::Arc;
    ///
    /// # async fn example(service_locator: Arc<dyn crate::core::ServiceLocator>) -> Result<(), SupervisorError> {
    /// let (parent, _) = Supervisor::new(
    ///     "parent".to_string(),
    ///     SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 60 },
    ///     service_locator.clone(),
    /// );
    ///
    /// let (child, child_rx) = Supervisor::new(
    ///     "child".to_string(),
    ///     SupervisionStrategy::OneForAll { max_restarts: 3, within_seconds: 60 },
    ///     service_locator.clone(),
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

        // Forward child supervisor events to the parent according to the propagation policy.
        // The forwarding task is lightweight and exits when the child's event channel closes.
        let parent_tx = self.event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = child_event_rx.recv().await {
                let should_forward = match event_propagation {
                    plexspaces_proto::supervision::v1::EventPropagation::EventPropagationForwardAll => true,
                    plexspaces_proto::supervision::v1::EventPropagation::EventPropagationFilterCritical => {
                        event.event_type == plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildFailed as i32
                            || event.event_type == plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventMaxRestartsExceeded as i32
                    }
                    plexspaces_proto::supervision::v1::EventPropagation::EventPropagationNone => false,
                };
                if should_forward {
                    let _ = parent_tx.send(event).await;
                }
            }
        });

        // Store the child supervisor's task handle directly.
        // TODO: Add automatic restart of child supervisors — when the child supervisor's task
        //       exits, detect it and call restart_supervisor_with_state(). This requires either
        //       a dedicated monitor task (like the old code did) or integrating with the
        //       existing handle_failure() pathway. Keeping it simple for now; callers can
        //       manually call restart_supervisor() if needed.
        let supervised = SupervisedSupervisor {
            supervisor: Arc::new(RwLock::new(child)),
            handle: Some(handle),
            restart_count: 0,
            last_restart: None,
            restart_timestamps: Vec::new(),
            restart,
            shutdown_timeout_ms,
        };

        // Add to child supervisors
        let mut child_supervisors = self.child_supervisors.write().await;
        child_supervisors.insert(child_id.clone(), supervised);
        drop(child_supervisors);

        // Phase 3: Register parent-child relationship in ActorRegistry
        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.actor_registry().await {
                let supervisor_id = ActorId::from(self.id.clone());
                let child_supervisor_id = ActorId::from(child_id.clone());
                registry
                    .register_parent_child(&supervisor_id, &child_supervisor_id)
                    .await;

                // OBSERVABILITY: Log parent-child registration
                trace!(
                    supervisor_id = %self.id,
                    child_supervisor_id = %child_id,
                    "Registered parent-child relationship for supervisor child in ActorRegistry"
                );
            }
        }

        // Phase 8.5: Link Semantics - Link parent supervisor to child supervisor
        // This enables cascading failures in supervision trees
        // System supervision operation - linking supervisor to child supervisor for fault tolerance
        // Use NodeConfig defaults for system operations
        if let Some(node) = &self.node {
            use crate::core::{RequestContext, RequestContextExt};
            let supervisor_id = ActorId::from(self.id.clone());
            let child_supervisor_id = ActorId::from(child_id.clone());
            // Tenant comes from auth, not config
            let ctx =
                RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);
            if let Err(e) = node.link(&ctx, &supervisor_id, &child_supervisor_id).await {
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
            .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildStarted as i32,
                actor_id: child_id.to_string(),
                ..Default::default()
            })
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
    pub async fn remove_supervisor_child(
        &self,
        supervisor_id: &str,
    ) -> Result<(), SupervisorError> {
        debug!(
            supervisor_id = %self.id,
            child_supervisor_id = %supervisor_id,
            "Removing child supervisor from supervisor"
        );

        // Phase 8.5: Unlink supervisor from child supervisor before removing
        if let Some(node) = &self.node {
            // System supervision operation - unlinking supervisor from child supervisor
            // Use NodeConfig defaults for system operations
            use crate::core::{RequestContext, RequestContextExt};
            let supervisor_id_actor = ActorId::from(self.id.clone());
            let child_supervisor_id_actor = ActorId::from(supervisor_id.to_string());
            // Tenant comes from auth, not config
            let ctx =
                RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);
            let _ = node
                .unlink(&ctx, &supervisor_id_actor, &child_supervisor_id_actor)
                .await; // Ignore errors (idempotent)
        }

        // Phase 3: Unregister parent-child relationship in ActorRegistry
        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.actor_registry().await {
                let supervisor_id_actor = ActorId::from(self.id.clone());
                let child_supervisor_id_actor = ActorId::from(supervisor_id.to_string());
                registry
                    .unregister_parent_child(&supervisor_id_actor, &child_supervisor_id_actor)
                    .await;

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
                .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                    event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildStopped as i32,
                    actor_id: supervisor_id.to_string(),
                    ..Default::default()
                })
                .await;
            Ok(())
        } else {
            Err(SupervisorError::ChildNotFound(
                supervisor_id.to_string().into(),
            ))
        }
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
        if tracing::enabled!(tracing::Level::TRACE) {
            let child_count = self.children.read().await.len();
            tracing::trace!(
                supervisor_id = %self.id,
                child_count,
                "Starting supervisor shutdown"
            );
        }

        // Phase 1: Shutdown child supervisors first (they shutdown their children recursively)
        // Reverse order to shutdown in opposite order of start (Erlang/OTP convention)
        // CRITICAL: Collect all supervisor info first, then release lock before recursive calls
        //
        // Expected Behavior:
        // 1. Child supervisors are shut down first (in reverse start order)
        //    - Each child supervisor recursively shuts down its own children
        //    - Parent-child relationships are unregistered from ActorRegistry
        //    - Supervisor task handles are aborted
        // 2. Then child actors are shut down (in reverse start order)
        //    - Shutdown timeout is enforced per actor (BrutalKill, Timeout, Infinity)
        //    - Facet lifecycle hooks are executed (on_terminate_start, on_detach)
        //    - Parent-child relationships are unregistered
        // 3. All children are removed from internal maps
        // 4. Supervisor enters stopped state
        //
        // Deadlock Prevention:
        // - All child info is collected BEFORE any await points
        // - Locks are released BEFORE recursive shutdown() calls
        // - This prevents deadlocks when children try to acquire their own locks
        let supervisor_info: SupervisorShutdownInfo = {
            let mut child_supervisors = self.child_supervisors.write().await;
            let mut info = Vec::new();
            let ids: Vec<String> = child_supervisors.keys().rev().cloned().collect();
            for id in ids {
                if let Some(mut supervised) = child_supervisors.shift_remove(&id) {
                    let handle = supervised.handle.take();
                    info.push((
                        id.clone(),
                        supervised.supervisor.clone(),
                        handle,
                        supervised.shutdown_timeout_ms,
                    ));
                }
            }
            info
        };

        for (id, supervisor_arc, handle, shutdown_timeout) in supervisor_info {
            // Phase 3: Unregister parent-child relationship in ActorRegistry
            if let Some(service_locator) = &self.service_locator {
                if let Some(registry) = service_locator.actor_registry().await {
                    let supervisor_id = ActorId::from(self.id.clone());
                    let child_supervisor_id = ActorId::from(id.clone());
                    registry
                        .unregister_parent_child(&supervisor_id, &child_supervisor_id)
                        .await;
                }
            }

            // Abort the supervisor's task handle
            if let Some(handle) = handle {
                handle.abort();
            }

            // Call shutdown on the child supervisor (recursive!)
            // CRITICAL: No locks held here, preventing deadlock
            let shutdown_future = async {
                let mut child_supervisor = supervisor_arc.write().await;
                Box::pin(child_supervisor.shutdown()).await
            };

            if let Some(timeout) = shutdown_timeout {
                let _ = tokio_timeout(Duration::from_millis(timeout), shutdown_future).await;
            } else {
                // Use configurable safety timeout to prevent deadlocks
                let safety_timeout = self
                    .default_shutdown_timeout
                    .unwrap_or(Duration::from_secs(1));
                let _ = tokio_timeout(safety_timeout, shutdown_future).await;
            }

            // Emit ChildStopped event for child supervisor
            let _ = self
                .event_tx
                .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                    event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildStopped as i32,
                    actor_id: id.to_string(),
                    ..Default::default()
                })
                .await;
        }

        // Phase 2: Shutdown child actors (in reverse start order)
        // Phase 3: Unregister parent-child relationships for actors
        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.actor_registry().await {
                let supervisor_id = ActorId::from(self.id.clone());
                let children = self.children.read().await;
                let child_ids: Vec<ActorId> = children.keys().cloned().collect();
                drop(children);

                for child_id in child_ids {
                    registry
                        .unregister_parent_child(&supervisor_id, &child_id)
                        .await;
                }
            }
        }

        // CRITICAL: Collect child info first, then release lock before async operations
        // This prevents deadlocks when actors try to acquire locks during shutdown
        let child_info: ActorShutdownInfo = {
            let mut children = self.children.write().await;
            let mut info = Vec::new();
            let ids: Vec<ActorId> = children.keys().rev().cloned().collect();
            for id in ids {
                if let Some(mut child) = children.shift_remove(&id) {
                    let handle = child.handle.take();
                    // Convert ChildSpec's Duration to Option<u64> milliseconds
                    let shutdown_timeout_ms =
                        child.spec.shutdown_timeout().map(|d| d.as_millis() as u64);
                    info.push((id.clone(), child.actor.clone(), handle, shutdown_timeout_ms));
                }
            }
            info
        };

        let actor_count = child_info.len();

        for (id, actor_arc, handle, shutdown_timeout) in child_info {
            // Phase 4: Enforce shutdown spec (BrutalKill, Timeout, Infinity)
            match shutdown_timeout {
                Some(0) => {
                    // BrutalKill: Immediate abort
                    debug!(
                        supervisor_id = %self.id,
                        child_id = %id,
                        "BrutalKill: Aborting child immediately"
                    );
                    if let Some(handle) = handle {
                        handle.abort();
                    }
                }
                Some(timeout_ms) => {
                    // Timeout: Graceful shutdown with timeout
                    info!(
                        supervisor_id = %self.id,
                        child_id = %id,
                        timeout_ms = timeout_ms,
                        child_actor_count = actor_count,
                        shutdown_children_order = "reverse_start",
                        "Graceful shutdown with timeout"
                    );
                    // Phase 1: Unified Lifecycle - Graceful shutdown with facet lifecycle hooks
                    // actor.stop() will trigger:
                    // 1. facet.on_terminate_start() for all facets (priority order)
                    // 2. actor.on_facets_detaching()
                    // 3. actor.terminate()
                    // 4. facet.on_detach() for all facets (reverse priority order)
                    let stop_future = async {
                        let mut actor = actor_arc.write().await;
                        // OBSERVABILITY: Record metrics for graceful shutdown
                        let shutdown_start = std::time::Instant::now();
                        metrics::counter!("plexspaces_supervisor_child_shutdown_total",
                            "supervisor_id" => self.id.clone(),
                            "child_id" => id.to_string()
                        )
                        .increment(1);

                        let result = actor.stop().await;

                        let shutdown_duration = shutdown_start.elapsed();
                        metrics::histogram!("plexspaces_supervisor_child_shutdown_duration_seconds",
                            "supervisor_id" => self.id.clone(),
                            "child_id" => id.to_string()
                        ).record(shutdown_duration.as_secs_f64());

                        if result.is_err() {
                            metrics::counter!("plexspaces_supervisor_child_shutdown_errors_total",
                                "supervisor_id" => self.id.clone(),
                                "child_id" => id.to_string()
                            )
                            .increment(1);
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

                        if let Some(handle) = &handle {
                            handle.abort();
                        }
                    };

                    // Enforce timeout
                    if tokio_timeout(Duration::from_millis(timeout_ms), stop_future)
                        .await
                        .is_err()
                    {
                        warn!(
                            supervisor_id = %self.id,
                            child_id = %id,
                            timeout_ms = timeout_ms,
                            "Child shutdown exceeded timeout, aborting"
                        );
                        if let Some(handle) = &handle {
                            handle.abort();
                        }
                    }
                }
                None => {
                    // Infinity: Wait indefinitely (for supervisors) - but with a configurable timeout to prevent deadlocks
                    let safety_timeout = self
                        .default_shutdown_timeout
                        .unwrap_or(Duration::from_secs(1));
                    debug!(
                        supervisor_id = %self.id,
                        child_id = %id,
                        timeout_secs = safety_timeout.as_secs(),
                        "Infinity: Waiting for child shutdown (with configurable safety timeout)"
                    );
                    let stop_future = async {
                        let mut actor = actor_arc.write().await;
                        let _ = actor.stop().await;
                    };
                    // Use configurable safety timeout to prevent deadlocks
                    if tokio_timeout(safety_timeout, stop_future).await.is_err() {
                        warn!(
                            supervisor_id = %self.id,
                            child_id = %id,
                            timeout_secs = safety_timeout.as_secs(),
                            "Child shutdown exceeded safety timeout, aborting"
                        );
                    }
                    // Abort handle after timeout check
                    if let Some(handle) = &handle {
                        handle.abort();
                    }
                }
            }

            let _ = self
                .event_tx
                .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                    event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildStopped as i32,
                    actor_id: id.to_string(),
                    ..Default::default()
                })
                .await;
        }

        // Remove all children from the map after shutdown
        {
            let mut children = self.children.write().await;
            children.clear();
        }

        if tracing::enabled!(tracing::Level::TRACE) {
            trace!(
                supervisor_id = %self.id,
                "Supervisor shutdown completed"
            );
        }

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

        info!(
            supervisor_id = %self.id,
            "Starting supervisor (Phase 4: Bottom-up startup with rollback)"
        );

        // Phase 4: Bottom-up startup with rollback
        // Start nested supervisors first (bottom-up), then verify actors are started
        // If any child fails, rollback all previously started children in reverse order
        //
        // Expected Behavior:
        // 1. Nested supervisors are started first (bottom-up ordering)
        // 2. Then child actors are started
        // 3. If any child fails, all previously started children are rolled back
        // 4. Supervisor enters running state only if all children start successfully

        let mut started_supervisor_ids: Vec<String> = Vec::new();
        let mut started_actor_ids: Vec<ActorId> = Vec::new();
        let mut rollback_needed = false;
        let mut failed_child_id: Option<String> = None;

        // Step 1: Start nested supervisors first (bottom-up ordering)
        {
            let child_supervisors = self.child_supervisors.read().await;
            let supervisor_ids: Vec<String> = child_supervisors.keys().cloned().collect();
            let supervisor_count = supervisor_ids.len();
            drop(child_supervisors);

            debug!(
                supervisor_id = %self.id,
                child_supervisor_count = supervisor_count,
                "Starting {} nested supervisor(s) in bottom-up order",
                supervisor_count
            );

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
                        child_supervisors
                            .get(&supervisor_id)
                            .map(|s| s.supervisor.clone())
                    };

                    if let Some(supervisor_arc) = supervisor_arc {
                        // CRITICAL: Release lock before recursive start to prevent deadlock
                        let start_result = {
                            let mut child_supervisor = supervisor_arc.write().await;
                            child_supervisor.start().await
                        };

                        match start_result {
                            Ok(handle) => {
                                // Update the handle in the supervised supervisor
                                let mut child_supervisors = self.child_supervisors.write().await;
                                if let Some(supervised) = child_supervisors.get_mut(&supervisor_id)
                                {
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
                if child.handle.is_none() {
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
        )
        .record(startup_duration.as_secs_f64());

        // Spawn the supervisor's lifetime task.
        // The task parks with `pending()` consuming no CPU; it is aborted by `shutdown()`.
        // Worker health monitoring is event-driven (callers invoke handle_failure() when
        // an actor task exits); no polling is needed here.
        // TODO: If active health polling is ever needed, replace pending() with a select
        //       over a Notify channel and a health-check interval.
        let handle = tokio::spawn(std::future::pending::<()>());

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
    /// let child_actor_id = ActorId::new("worker1", "worker", "ns", "node1").unwrap();
    /// let spec = ChildSpec::worker(child_actor_id, start_fn);
    /// let child_id = supervisor.start_child(spec).await?;
    /// ```
    #[instrument(skip(self, spec), fields(supervisor_id = %self.id, child_id = %spec.actor_id))]
    pub async fn start_child(
        &mut self,
        spec: crate::child_spec::ChildSpec,
    ) -> Result<ActorId, SupervisorError> {
        use crate::child_spec::StartedChild;

        debug!(
            supervisor_id = %self.id,
            child_id = %spec.actor_id,
            role = %spec.role(),
            "Starting child dynamically"
        );

        // Call start function to create/start the child
        let started = (spec.start_fn)().await.map_err(|e| {
            SupervisorError::ActorCreationFailed(format!("Failed to start child: {}", e))
        })?;

        match started {
            StartedChild::Worker {
                mut actor,
                actor_ref: _,
            } => {
                // Phase 1: Unified Lifecycle - Attach facets from ChildSpec before starting actor
                // Facets are attached in priority order (high priority first)
                // This ensures facets are ready before actor.init() is called
                if !spec.proto.facets.is_empty() {
                    // Get FacetRegistry from ServiceLocator to create facets from proto
                    if let Some(service_locator) = &self.service_locator {
                        if let Some(facet_registry_wrapper) =
                            service_locator.get_facet_registry().await
                        {
                            let facet_registry = facet_registry_wrapper.inner_clone();
                            // Use facet_helpers to create facets from proto
                            use crate::create_facets_from_proto;
                            let facets =
                                create_facets_from_proto(&spec.proto.facets, &facet_registry).await;

                            // Attach facets to the actor before starting
                            let mut attached_count = 0;

                            for facet in facets {
                                if let Err(e) = actor.attach_facet(facet).await {
                                    warn!(
                                        supervisor_id = %self.id,
                                        child_id = %spec.actor_id,
                                        error = %e,
                                        "Failed to attach facet from ChildSpec (continuing with other facets)"
                                    );
                                    metrics::counter!("plexspaces_supervisor_facet_attach_errors_total",
                                        "supervisor_id" => self.id.clone(),
                                        "child_id" => spec.actor_id.to_string()
                                    ).increment(1);
                                } else {
                                    attached_count += 1;
                                }
                            }

                            metrics::counter!("plexspaces_supervisor_facets_attached_total",
                                "supervisor_id" => self.id.clone(),
                                "child_id" => spec.actor_id.to_string()
                            )
                            .increment(attached_count);

                            debug!(
                                supervisor_id = %self.id,
                                child_id = %spec.actor_id,
                                facet_count = spec.proto.facets.len(),
                                attached_count = attached_count,
                                "Attached facets from ChildSpec before starting actor"
                            );
                        } else {
                            debug!(
                                supervisor_id = %self.id,
                                child_id = %spec.actor_id,
                                facet_count = spec.proto.facets.len(),
                                "FacetRegistry not available - facets not attached (graceful degradation)"
                            );
                        }
                    }
                }

                // Start the actor (calls init() and registers in ActorRegistry)
                // Facets are already attached, so facet lifecycle hooks will be called during start()
                let handle = actor
                    .start()
                    .await
                    .map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?;

                // Store ChildSpec directly (proto-first design with facets support)
                let supervised = SupervisedActor {
                    actor: Arc::new(RwLock::new(actor)),
                    handle: Some(handle),
                    restart_count: 0,
                    last_restart: None,
                    restart_timestamps: Vec::new(),
                    spec: spec.clone(),
                };

                // Add to children
                let mut children = self.children.write().await;
                children.insert(spec.actor_id.clone(), supervised);
                drop(children);

                // Phase 3: Register parent-child relationship
                if let Some(service_locator) = &self.service_locator {
                    if let Some(registry) = service_locator.actor_registry().await {
                        let supervisor_id = ActorId::from(self.id.clone());
                        let child_id = spec.actor_id.clone();
                        registry
                            .register_parent_child(&supervisor_id, &child_id)
                            .await;
                    }
                }

                // Send event
                let _ = self
                    .event_tx
                    .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                        event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildStarted as i32,
                        actor_id: spec.actor_id.to_string(),
                        ..Default::default()
                    })
                    .await;

                Ok(spec.actor_id.clone())
            }
            StartedChild::Supervisor { supervisor: _ } => {
                // For supervisor children, we need to add them via add_supervisor_child
                // This is a simplified version - full implementation would handle event_rx
                Err(SupervisorError::ActorCreationFailed(
                    "Supervisor children must be added via add_supervisor_child()".to_string(),
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
        self.remove_child(&ActorId::from(child_id.to_string()))
            .await
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

            // Recreate actor using the spec's async start_fn
            let spec = child.spec.clone();
            drop(children);

            // Create new actor via async factory
            use crate::child_spec::StartedChild;
            let started_child = (spec.start_fn)()
                .await
                .map_err(|e| SupervisorError::ActorCreationFailed(e.to_string()))?;

            let mut new_actor = match started_child {
                StartedChild::Worker { actor, .. } => actor,
                StartedChild::Supervisor { .. } => {
                    return Err(SupervisorError::ActorCreationFailed(
                        "restart_child: Expected worker, got supervisor".to_string(),
                    ));
                }
            };

            let handle = new_actor
                .start()
                .await
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
    pub async fn which_children(&self) -> Vec<super::ChildInfo> {
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

            result.push(super::ChildInfo {
                child_id: id.to_string(),
                role: child.spec.role().to_string(),
                status: status as i32,
                restart_count: child.restart_count,
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

            result.push(super::ChildInfo {
                child_id: id.clone(),
                role: "supervisor".to_string(),
                status: status as i32,
                restart_count: supervised.restart_count,
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
    pub async fn count_children(&self) -> super::ChildCount {
        let children = self.children.read().await;
        let child_supervisors = self.child_supervisors.read().await;

        super::ChildCount {
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

        // Check actor children - ChildSpec is stored directly
        let children = self.children.read().await;
        if let Some(child) = children.get(&actor_id) {
            return Some(child.spec.clone());
        }
        drop(children);

        // Check supervisor children
        let child_supervisors = self.child_supervisors.read().await;
        if child_supervisors.contains_key(child_id) {
            // For supervisor children, we don't have full ChildSpec stored
            // Return a minimal spec
            let actor_id = ActorId::from_canonical(child_id).unwrap_or_else(|_| {
                ActorId::new(child_id, "supervisor", "default", "localhost")
                    .expect("placeholder supervisor child id")
            });
            return Some(crate::child_spec::ChildSpec::supervisor(
                actor_id,
                Arc::new(|| {
                    Box::pin(async move {
                        Err(ActorError::InvalidState(
                            "get_childspec() returns read-only spec".to_string(),
                        ))
                    })
                }),
            ));
        }

        None
    }

    /// Build supervisor from SupervisorSpec
    ///
    /// ## Purpose
    /// Creates a supervisor from a SupervisorSpec proto message.
    /// This enables declarative supervisor creation from configuration.
    pub async fn from_config(
        supervisor_id: String,
        config: plexspaces_proto::supervision::v1::SupervisorSpec,
        service_locator: Arc<dyn ServiceLocatorTrait>,
    ) -> Result<(Self, mpsc::Receiver<SupervisorEvent>), SupervisorError> {
        use plexspaces_proto::supervision::v1::SupervisionStrategy as ProtoStrategy;
        let window_secs = config
            .max_restart_window
            .as_ref()
            .map(|d| d.seconds as u64)
            .unwrap_or(60);
        let strategy = match ProtoStrategy::try_from(config.strategy)
            .unwrap_or(ProtoStrategy::SupervisionStrategyUnspecified)
        {
            ProtoStrategy::SupervisionStrategyUnspecified
            | ProtoStrategy::SupervisionStrategyOneForOne
            | ProtoStrategy::SupervisionStrategySimpleOneForOne => SupervisionStrategy::OneForOne {
                max_restarts: config.max_restarts,
                within_seconds: window_secs,
            },
            ProtoStrategy::SupervisionStrategyOneForAll => SupervisionStrategy::OneForAll {
                max_restarts: config.max_restarts,
                within_seconds: window_secs,
            },
            ProtoStrategy::SupervisionStrategyRestForOne => SupervisionStrategy::RestForOne {
                max_restarts: config.max_restarts,
                within_seconds: window_secs,
            },
            ProtoStrategy::SupervisionStrategyAdaptive => SupervisionStrategy::Adaptive {
                initial_strategy: Box::new(SupervisionStrategy::OneForOne {
                    max_restarts: config.max_restarts,
                    within_seconds: window_secs,
                }),
                learning_rate: config
                    .adaptive
                    .as_ref()
                    .map(|a| a.learning_rate)
                    .unwrap_or(0.1),
            },
        };

        // Create supervisor
        let (supervisor, event_rx) = Supervisor::new(supervisor_id, strategy, service_locator);

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
            let Some(id) = child_spec_proto.actor_identity.as_ref() else {
                return Err(SupervisorError::ActorCreationFailed(format!(
                    "Child at index {} is missing actor_identity",
                    idx
                )));
            };
            if id.name.is_empty() || id.actor_type.is_empty() {
                return Err(SupervisorError::ActorCreationFailed(format!(
                    "Child at index {} has empty name or actor_type in actor_identity",
                    idx
                )));
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
