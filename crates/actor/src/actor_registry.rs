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

//! Actor registry for local actor lifecycle, lookup, and message delivery.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

use crate::actor_context::ActorService;
use crate::service_locator_trait::ServiceLocator;
use crate::ActorFactory;
use crate::Service;
use crate::{
    ActorId, ExitReason, MessageSender, ReplyWaiter, ReplyWaiterRegistry, RequestContext,
    RequestContextExt, VirtualActorManager, TEMP_SENDER_ACTOR_TYPE,
};
use plexspaces_common::ServiceNameExt;
use plexspaces_facet::{ExitReason as FacetExitReason, FacetManager};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::ActorLifecycleEvent;
use ulid::Ulid;

// Observability
use metrics;
use tracing;

// Re-export from actor_monitor for use within this module.
use crate::actor_monitor::{create_down_message, create_exit_message, exit_reason_to_string};
// MonitorLink is now defined in actor_monitor and re-exported from crate root.
pub use crate::actor_monitor::MonitorLink;

/// Type index key: (tenant_id, namespace, actor_type) mapped to actor IDs.
type ActorTypeIndex = Arc<RwLock<HashMap<(String, String, String), Vec<ActorId>>>>;

/// Error types for ActorRegistry operations
#[derive(Debug, thiserror::Error)]
pub enum ActorRegistryError {
    /// Actor was not found in the registry
    #[error("Actor not found: {0}")]
    ActorNotFound(String),

    /// Failed to lookup actor location
    #[error("Lookup failed: {0}")]
    LookupFailed(String),

    /// Failed to register actor
    #[error("Registration failed: {0}")]
    RegistrationFailed(String),

    /// Failed to unregister actor
    #[error("Unregistration failed: {0}")]
    UnregistrationFailed(String),

    /// Message send failed
    #[error("Send failed: {0}")]
    SendFailed(String),

    /// Timed out waiting for reply
    #[error("Timed out waiting for reply")]
    Timeout,

    /// Required registry dependency is not configured
    #[error("Registry dependency unavailable: {0}")]
    DependencyUnavailable(String),

    /// Messaging visibility denied (reserved; local routing uses `ActorRef` errors today).
    #[error("Actor visibility denied: {0}")]
    VisibilityDenied(String),

    /// Link/monitor/unlink/demonitor rejected (namespace/tenant scope or locality).
    #[error("link/monitor denied: {0}")]
    LinkMonitorDenied(String),
}

/// ## Actor Data Storage
/// ActorRegistry is the single source of truth for all actor-related data:
/// - Local actor senders and instances
/// - Facets (for facet access)
/// - Virtual actor metadata
/// - Monitoring links
/// - Actor links
/// - Actor configurations
/// - Lifecycle event subscribers
pub struct ActorRegistry {
    /// Local actor senders keyed by tenant/namespace scope plus actor id.
    /// Virtual actors may be absent while passivated and are re-instantiated
    /// from VirtualActorManager metadata on demand.
    actors: Arc<RwLock<HashMap<ScopedActorKey, Arc<dyn MessageSender>>>>,
    /// Current node ID
    local_node_id: String,

    /// FacetManager for facet storage and management
    facet_manager: Arc<FacetManager>,
    /// ActorFactory used to instantiate virtual actors and temporary senders.
    actor_factory: Arc<RwLock<Option<Arc<dyn ActorFactory>>>>,
    /// VirtualActorManager is the source of truth for virtual actor metadata.
    virtual_actor_manager: Arc<RwLock<Option<Arc<VirtualActorManager>>>>,
    /// ReplyWaiterRegistry is used by local ask() to await replies.
    reply_waiter_registry: Arc<RwLock<Option<Arc<ReplyWaiterRegistry>>>>,
    /// Monitor and link state (see actor_monitor module).
    actor_monitor: Arc<crate::actor_monitor::ActorMonitor>,
    /// Lifecycle event subscribers (for observability backends like Prometheus, StatsD)
    /// Supports multiple subscribers with independent filtering and backpressure
    lifecycle_subscribers: Arc<RwLock<Vec<mpsc::UnboundedSender<ActorLifecycleEvent>>>>,
    /// Actor configurations (Phase 3: Resource-aware scheduling)
    /// Maps actor_id -> ActorConfig (for resource requirement tracking)
    actor_configs: Arc<RwLock<HashMap<ActorId, plexspaces_proto::v1::actor::ActorConfig>>>,
    /// Scope-aware registered actor inventory.
    ///
    /// This includes actors that are known to the system even when no live sender
    /// currently exists, such as passivated virtual actors retained for discovery.
    registered_actor_entries: Arc<RwLock<HashSet<ScopedActorKey>>>,
    /// ActorService for routing messages to remote actors.
    /// Initialized with `LocalOnlyActorService`; replaced by `set_actor_service()` during
    /// node startup once a real service is available.  Always non-None so routing code
    /// never checks for an Optional.
    actor_service: Arc<RwLock<Arc<dyn ActorService>>>,
    /// Dialable HTTP base (`http://listen_addr`) for remote `NotifyActorDown` when the
    /// supervisor is on another node. Set via [`Self::set_local_listen_addr`] during node startup.
    local_listen_addr: Arc<RwLock<String>>,
    /// Temporary sender mappings: temporary_sender_id -> TemporarySenderEntry
    /// Used for ask() pattern when called from outside actor context
    /// Key: structured temporary sender ActorId
    /// Value: ActorRef ID that created it, correlation_id, and expiration time
    temporary_senders: Arc<RwLock<HashMap<ActorId, TemporarySenderEntry>>>,
    /// Efficient actor-type lookup: (tenant_id, namespace, actor_type) -> Vec<actor_id>
    /// Used for FaaS-style actor request routing to quickly find actors by type
    /// Maintained in sync with actors map for O(1) lookup
    /// Key: (tenant_id, namespace, actor_type), Value: List of actor IDs of that type
    actor_type_index: ActorTypeIndex,
    // === Parent-Child Relationships (Phase 3) ===
    /// Parent-to-children mapping: parent_id -> Vec<child_id>
    /// Tracks supervision hierarchy for graceful shutdown and subtree operations
    /// Used by supervisors to track their children (actors or nested supervisors)
    parent_to_children: Arc<RwLock<HashMap<ActorId, Vec<ActorId>>>>,

    /// Child-to-parent mapping: child_id -> parent_id
    /// Enables quick parent lookup for child actors
    /// Used for cascading shutdown and parent notification
    child_to_parent: Arc<RwLock<HashMap<ActorId, ActorId>>>,
}

/// Temporary sender entry for ask() pattern
///
/// ## Purpose
/// Stores metadata for temporary senders created when ask() is called from outside actor context.
/// The temporary sender itself is registered as an ActorRef in the actors map.
///
/// ## Design
/// - Temporary sender uses its own canonical ActorId as actor_ref_id
/// - Used for correlation_id lookup and expiration tracking
#[derive(Clone, Debug)]
pub struct TemporarySenderEntry {
    /// Temporary sender actor ID.
    pub actor_ref_id: ActorId,
    /// Correlation ID for matching replies
    pub correlation_id: String,
    /// Expiration time (for automatic cleanup)
    pub expires_at: Instant,
}

// MonitorLink is defined in crate::actor_monitor and re-exported above.

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ScopedActorKey {
    tenant_id: String,
    namespace: String,
    actor_id: ActorId,
}

/// Parameters for [`ActorRegistry::register_actor`].
pub struct ActorRegistrationParams {
    /// The actor's unique identifier.
    pub actor_id: ActorId,
    /// Message sender for the running actor.
    pub sender: Arc<dyn MessageSender>,
    /// Actor type string used for dashboard visibility and type-index lookups.
    pub actor_type: String,
    /// Optional resource configuration attached to this actor.
    pub config: Option<plexspaces_proto::v1::actor::ActorConfig>,
    /// Optional local runtime state handle (present only for locally running actors).
    pub instance: Option<Arc<dyn crate::actor_state_checker::ActorStateHandle>>,
    /// Optional OTP-style behavior kind for logging (GenServer, GenEvent, etc.).
    pub behavior_kind: Option<crate::BehaviorType>,
}

impl ActorRegistry {
    fn scoped_actor_key(
        tenant_id: impl Into<String>,
        namespace: impl Into<String>,
        actor_id: ActorId,
    ) -> ScopedActorKey {
        ScopedActorKey {
            tenant_id: tenant_id.into(),
            namespace: namespace.into(),
            actor_id,
        }
    }

    /// ## Arguments
    /// * `local_node_id` - ID of the local node
    ///
    /// ## Returns
    /// New ActorRegistry instance
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_actor::ActorRegistry;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let registry = ActorRegistry::new("node1".to_string());
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(local_node_id: String) -> Self {
        ActorRegistry {
            actors: Arc::new(RwLock::new(HashMap::new())),
            local_node_id,
            facet_manager: Arc::new(plexspaces_facet::FacetManager::new()),
            actor_monitor: Arc::new(crate::actor_monitor::ActorMonitor::new()),
            lifecycle_subscribers: Arc::new(RwLock::new(Vec::new())),
            actor_configs: Arc::new(RwLock::new(HashMap::new())),
            registered_actor_entries: Arc::new(RwLock::new(HashSet::new())),
            temporary_senders: Arc::new(RwLock::new(HashMap::new())),
            actor_type_index: Arc::new(RwLock::new(HashMap::new())),
            parent_to_children: Arc::new(RwLock::new(HashMap::new())),
            child_to_parent: Arc::new(RwLock::new(HashMap::new())),
            actor_factory: Arc::new(RwLock::new(None)),
            virtual_actor_manager: Arc::new(RwLock::new(None)),
            reply_waiter_registry: Arc::new(RwLock::new(None)),
            actor_service: Arc::new(RwLock::new(Arc::new(
                crate::actor_monitor::LocalOnlyActorService,
            ) as Arc<dyn ActorService>)),
            local_listen_addr: Arc::new(RwLock::new(String::new())),
        }
    }

    /// Sets the local node's public HTTP/gRPC listen base used in cross-node monitor RPCs.
    pub async fn set_local_listen_addr(&self, addr: String) {
        *self.local_listen_addr.write().await = addr;
    }

    /// Set the ActorFactory used to spawn new actors from spec.
    pub async fn set_actor_factory(&self, actor_factory: Arc<dyn ActorFactory>) {
        *self.actor_factory.write().await = Some(actor_factory);
    }

    /// Sets the VirtualActorManager used for metadata-driven activation.
    pub async fn set_virtual_actor_manager(&self, manager: Arc<VirtualActorManager>) {
        *self.virtual_actor_manager.write().await = Some(manager);
    }

    /// Sets the ReplyWaiterRegistry used by local ask().
    pub async fn set_reply_waiter_registry(&self, registry: Arc<ReplyWaiterRegistry>) {
        *self.reply_waiter_registry.write().await = Some(registry);
    }

    /// Replaces the ActorService used to route messages to remote actors.
    /// Call this during node startup once the real service is available.
    pub async fn set_actor_service(&self, actor_service: Arc<dyn ActorService>) {
        *self.actor_service.write().await = actor_service;
    }

    /// Convenience: extracts ActorService from a ServiceLocator and installs it.
    /// Must be called after node initialization so that ActorService is available.
    pub async fn set_service_locator(&self, locator: Arc<dyn ServiceLocator>) {
        if let Some(svc) = locator.get_actor_service().await {
            *self.actor_service.write().await = svc;
        }
    }

    /// HTTP base (`http://host:port`) used when peers must call back for supervision RPCs.
    pub async fn local_listen_base_url(&self) -> String {
        self.local_listen_addr.read().await.clone()
    }

    /// For operands **on this node** (others skipped): `ActorId::namespace` must match
    /// `ctx.namespace()` unless `ctx` is internal or [`RequestContext::should_skip_namespace_filter`].
    ///
    /// When **`ctx.auth_enabled`**, the actor must appear under this node’s registry inventory for
    /// **`(ctx.tenant_id(), ctx.namespace(), actor_id)`** (live sender or registered entry). No
    /// extra scans beyond that keyed check. Remote-only operands are validated on the peer when
    /// `ActorService` forwards the RPC.
    pub async fn validate_link_monitor_operand_scope(
        &self,
        ctx: &RequestContext,
        operands: &[&ActorId],
    ) -> Result<(), ActorRegistryError> {
        if ctx.internal || ctx.should_skip_namespace_filter() {
            return Ok(());
        }
        for id in operands {
            if !id.is_on_node(&self.local_node_id) {
                continue;
            }
            self.validate_one_local_operand_scope(ctx, id).await?;
        }
        Ok(())
    }

    async fn validate_one_local_operand_scope(
        &self,
        ctx: &RequestContext,
        id: &ActorId,
    ) -> Result<(), ActorRegistryError> {
        if id.namespace() != ctx.namespace() {
            return Err(ActorRegistryError::LinkMonitorDenied(format!(
                "actor {} namespace '{}' does not match request namespace '{}'",
                id,
                id.namespace(),
                ctx.namespace()
            )));
        }
        if !ctx.auth_enabled {
            return Ok(());
        }
        let key = Self::scoped_actor_key(
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            id.clone(),
        );
        if self
            .lookup_actor_in_scope(ctx.tenant_id(), ctx.namespace(), id)
            .await
            .is_some()
        {
            return Ok(());
        }
        let in_registered = {
            let entries = self.registered_actor_entries.read().await;
            entries.contains(&key)
        };
        if in_registered {
            return Ok(());
        }
        Err(ActorRegistryError::LinkMonitorDenied(format!(
            "actor {} is not registered under tenant '{}' namespace '{}' on this node",
            id,
            ctx.tenant_id(),
            ctx.namespace()
        )))
    }

    // === Accessor methods for actor-related data ===

    /// Get the local runtime/state handle for active actors and tests.
    ///
    /// ## Purpose
    /// Gets the stored actor instance for:
    /// 1. Active local actors that expose runtime state through `ActorStateHandle`
    /// 2. Test helpers that need to inspect the running runtime handle
    ///
    /// ## Design Principles
    /// - **Encapsulation**: Runtime state is exposed through the registered sender only
    /// - **Simple**: Single method to get the local runtime handle when one exists
    /// - **Consistent**: Local actor lifecycle flows through the same registered sender shape
    ///
    /// ## Note
    /// Production code should use `lookup_actor()` or `lookup_actor_in_scope()` to get a
    /// `MessageSender`. Lazy virtual actors do not keep a live handle here; they are rebuilt from
    /// `VirtualActorManager` metadata during activation.
    pub async fn get_actor_instance(
        &self,
        actor_id: &ActorId,
    ) -> Option<Arc<dyn crate::actor_state_checker::ActorStateHandle>> {
        self.lookup_actor(actor_id)
            .await
            .and_then(|sender| sender.local_state_handle())
    }

    /// Get actor state
    ///
    /// ## Purpose
    /// Gets the actual state of an actor instance.
    /// This is used to determine if an actor is truly active (state is Active).
    ///
    /// ## Returns
    /// `Option<ActorState>` - The actor's state as a proto enum value, or `None` if not an actor or instance doesn't exist
    ///
    /// ## Implementation
    /// Uses the `ActorStateHandle` trait to get state without importing `Actor` directly.
    /// The trait is implemented by `Actor` in the `plexspaces_actor` crate.
    ///
    /// ## Usage
    /// Called by `is_actor_state_active()` to check if an actor is truly active.
    /// This is consistent for all actor types (regular/virtual/workflows/etc.).
    pub async fn get_actor_state(
        &self,
        actor_id: &ActorId,
    ) -> Option<plexspaces_proto::v1::actor::ActorState> {
        if let Some(instance) = self.get_actor_instance(actor_id).await {
            Some(instance.actor_state().await)
        } else {
            None
        }
    }

    /// Check if actor state is Active
    ///
    /// ## Purpose
    /// Checks if an actor instance's state is `Active`.
    /// This is critical for lazy virtual actors where an instance exists but state is Creating (not Active).
    ///
    /// ## Returns
    /// `bool` - true if actor instance exists and state is Active, false otherwise
    ///
    /// ## Implementation
    /// Uses `get_actor_state()` to fetch the state, then checks if it's `Active`.
    /// This is consistent for all actor types (regular/virtual/workflows/etc.):
    /// - Regular actors: state is `Active` when message loop is running
    /// - Virtual actors (lazy): state is `Creating` until activated, then `Active`
    /// - Virtual actors (eager): state is `Active` immediately after registration
    /// - Workflows: state is `Active` when workflow is running
    ///
    /// ## Usage
    /// Called by `VirtualActorManager::is_active()` to check if a virtual actor is truly active
    /// (not just registered, but actually running with state = Active).
    pub async fn is_actor_state_active(&self, actor_id: &ActorId) -> bool {
        use plexspaces_proto::v1::actor::ActorState as ProtoActorState;

        if let Some(state_value) = self.get_actor_state(actor_id).await {
            let is_active = state_value == ProtoActorState::ActorStateActive;
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    "[ACTOR_REGISTRY] is_actor_state_active: actor_id={}, state_value={:?}, is_active={}",
                    actor_id,
                    state_value,
                    is_active
                );
            }
            is_active
        } else {
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    "[ACTOR_REGISTRY] is_actor_state_active: actor_id={}, state_value=None, is_active=false",
                    actor_id
                );
            }
            false
        }
    }

    /// Get actor metadata (tenant_id, namespace)
    ///
    /// ## Purpose
    /// Gets tenant/namespace for an actor without needing to access the actor instance.
    /// Used for proper tenant isolation during operations like stop().
    ///
    /// ## Note
    /// This is for internal use by ActorFactory. External code should not need this.
    pub async fn get_actor_metadata(&self, actor_id: &ActorId) -> Option<(String, String)> {
        if let Some(sender) = self.lookup_actor(actor_id).await {
            if let (Some(tenant_id), Some(namespace)) = (sender.tenant_id(), sender.namespace()) {
                return Some((tenant_id.to_string(), namespace.to_string()));
            }
        }

        let manager = self.virtual_actor_manager.read().await.clone();
        if let Some(manager) = manager {
            if let Some(metadata) = manager.get_metadata(actor_id).await {
                return Some((
                    metadata.tenant_id().to_string(),
                    metadata.namespace().to_string(),
                ));
            }
            if let Some(metadata) = manager.get_virtual_actor_type(actor_id.actor_type()).await {
                return Some((
                    metadata.tenant_id().to_string(),
                    metadata.namespace().to_string(),
                ));
            }
        }

        None
    }

    /// Get actor type for an actor_id
    ///
    /// ## Purpose
    /// Gets actor_type for an actor to enable rebuilding suspended actors.
    /// Used when reactivating suspended virtual actors that need to be rebuilt.
    ///
    /// ## Returns
    /// Some(actor_type) if found, None otherwise
    pub async fn get_actor_type(&self, actor_id: &ActorId) -> Option<String> {
        if let Some(sender) = self.lookup_actor(actor_id).await {
            if let Some(actor_type) = sender.actor_type() {
                return Some(actor_type);
            }
        }

        let manager = self.virtual_actor_manager.read().await.clone();
        let result = if let Some(manager) = manager {
            if let Some(metadata) = manager.get_metadata(actor_id).await {
                Some(metadata.actor_type().to_string())
            } else {
                manager
                    .get_virtual_actor_type(actor_id.actor_type())
                    .await
                    .map(|metadata| metadata.actor_type().to_string())
            }
        } else {
            None
        };
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                "[ACTOR_REGISTRY] get_actor_type: actor_id={}, result={:?}",
                actor_id,
                result
            );
        }
        result
    }

    //
    // Design: Simple and encapsulated
    // - runtime state lives on the registered sender
    // - get_actor_instance() - only way to read a local runtime handle
    // - register_actor() - only way to enrich the sender with local state
    // - unregister_with_cleanup() - only way to remove sender/runtime registration
    //
    // Lazy virtual actors rebuild from VirtualActorManager metadata, so the registry
    // only tracks a live runtime while the actor is active.

    /// Get FacetManager
    pub fn facet_manager(&self) -> &Arc<FacetManager> {
        &self.facet_manager
    }

    /// Get lifecycle subscribers
    pub fn lifecycle_subscribers(
        &self,
    ) -> &Arc<RwLock<Vec<mpsc::UnboundedSender<ActorLifecycleEvent>>>> {
        &self.lifecycle_subscribers
    }

    /// Get actor configs map
    pub fn actor_configs(
        &self,
    ) -> &Arc<RwLock<HashMap<ActorId, plexspaces_proto::v1::actor::ActorConfig>>> {
        &self.actor_configs
    }

    /// Returns the currently registered actor inventory entries.
    ///
    /// Unlike `live_actor_entries()`, this includes actors that remain registered
    /// for discovery without a live sender, such as passivated virtual actors.
    pub async fn registered_actor_entries(&self) -> Vec<(String, String, ActorId)> {
        let entries = self.registered_actor_entries.read().await;
        entries
            .iter()
            .map(|key| {
                (
                    key.tenant_id.clone(),
                    key.namespace.clone(),
                    key.actor_id.clone(),
                )
            })
            .collect()
    }

    /// Returns the number of registered actors across all scopes.
    pub async fn registered_actor_count(&self) -> usize {
        self.registered_actor_entries.read().await.len()
    }

    /// Returns the set of tenant ids with at least one registered actor.
    pub async fn registered_tenant_ids(&self) -> HashSet<String> {
        let entries = self.registered_actor_entries.read().await;
        entries.iter().map(|key| key.tenant_id.clone()).collect()
    }

    /// Returns the de-duplicated set of registered actor ids.
    ///
    /// This is primarily useful for callers that only need an actor-id inventory
    /// and do not require tenant/namespace scope information.
    pub async fn registered_actor_ids(&self) -> HashSet<ActorId> {
        let entries = self.registered_actor_entries.read().await;
        entries.iter().map(|key| key.actor_id.clone()).collect()
    }

    /// Returns the currently live registered actor entries.
    ///
    /// Each entry is keyed by the actor's isolation scope plus actor id so callers
    /// can reason about live actors without flattening distinct tenant/namespace
    /// registrations into a single global actor-id set.
    pub async fn live_actor_entries(&self) -> Vec<(String, String, ActorId)> {
        let actors = self.actors.read().await;
        actors
            .keys()
            .map(|key| {
                (
                    key.tenant_id.clone(),
                    key.namespace.clone(),
                    key.actor_id.clone(),
                )
            })
            .collect()
    }

    /// Returns the number of currently live registered actors.
    ///
    /// The count is scope-aware: the same actor id in two different scopes is
    /// counted twice because they are distinct live registrations.
    pub async fn live_actor_count(&self) -> usize {
        self.actors.read().await.len()
    }

    /// Returns the set of tenant ids with at least one live registered actor.
    pub async fn live_tenant_ids(&self) -> HashSet<String> {
        let actors = self.actors.read().await;
        actors.keys().map(|key| key.tenant_id.clone()).collect()
    }

    /// Get local node ID
    pub fn local_node_id(&self) -> &str {
        &self.local_node_id
    }

    /// Get the ActorMonitor (monitor/link state owner).
    pub fn actor_monitor(&self) -> &Arc<crate::actor_monitor::ActorMonitor> {
        &self.actor_monitor
    }

    /// Get actor type index (for efficient type-based lookups)
    pub fn actor_type_index(&self) -> &ActorTypeIndex {
        &self.actor_type_index
    }

    /// Convert a `BehaviorType` to the canonical string key stored in the registry.
    pub fn behavior_kind_key(behavior_kind: &crate::BehaviorType) -> String {
        match behavior_kind {
            crate::BehaviorType::GenServer => "gen_server".to_string(),
            crate::BehaviorType::GenEvent => "gen_event".to_string(),
            crate::BehaviorType::GenStateMachine => "gen_state_machine".to_string(),
            crate::BehaviorType::Workflow => "workflow".to_string(),
            crate::BehaviorType::Custom(name) => name.clone(),
        }
    }

    /// Get the runtime behavior kind registered for an actor.
    pub async fn get_behavior_kind(&self, actor_id: &ActorId) -> Option<String> {
        self.lookup_actor(actor_id)
            .await
            .and_then(|sender| sender.behavior_kind())
    }

    /// Register an actor (consolidated method for all actor types).
    ///
    /// Accepts a [`ActorRegistrationParams`] bundle instead of positional arguments
    /// to keep the signature within clippy's argument limit and make call sites self-documenting.
    pub async fn register_actor(&self, ctx: &RequestContext, p: ActorRegistrationParams) {
        let ActorRegistrationParams {
            actor_id,
            sender,
            actor_type,
            config,
            instance,
            behavior_kind,
        } = p;
        sender.set_actor_type(Some(actor_type.clone())).await;
        if let Some(ref behavior_kind) = behavior_kind {
            sender
                .set_behavior_kind(Some(Self::behavior_kind_key(behavior_kind)))
                .await;
        }
        if let Some(ref handle) = instance {
            sender.set_local_state_handle(Some(handle.clone())).await;
        }

        let mut actors = self.actors.write().await;
        let scoped_key = Self::scoped_actor_key(
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            actor_id.clone(),
        );

        if instance.is_none() {
            if let Some(existing_sender) = actors.get(&scoped_key) {
                if let Some(existing_handle) = existing_sender.local_state_handle() {
                    sender
                        .set_local_state_handle(Some(existing_handle.clone()))
                        .await;
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            actor_id = %actor_id,
                            tenant_id = %ctx.tenant_id(),
                            namespace = %ctx.namespace(),
                            "Preserved existing local state handle during actor re-registration"
                        );
                    }
                }
            }
        }

        let was_new = actors.insert(scoped_key.clone(), sender.clone()).is_none();
        drop(actors);

        // Keep registered discoverability inventory in sync with live registrations.
        if was_new {
            let mut registered_entries = self.registered_actor_entries.write().await;
            registered_entries.insert(scoped_key);
        }

        // Store actor config if provided (for resource tracking)
        if let Some(config) = config {
            let mut actor_configs = self.actor_configs.write().await;
            actor_configs.insert(actor_id.clone(), config);
        }

        let mut index = self.actor_type_index.write().await;
        let key = (
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            actor_type.clone(),
        );
        let actor_ids = index.entry(key).or_insert_with(Vec::new);
        if !actor_ids.contains(&actor_id) {
            actor_ids.push(actor_id.clone());
        }
        drop(index);

        // OBSERVABILITY: Log actor registration with type and behavior kind
        if tracing::enabled!(tracing::Level::DEBUG) {
            let behavior_str = behavior_kind.as_ref().map(|b| match b {
                crate::BehaviorType::GenServer => "GenServer",
                crate::BehaviorType::GenEvent => "EventHandler",
                crate::BehaviorType::GenStateMachine => "GenStateMachine",
                crate::BehaviorType::Workflow => "Workflow",
                crate::BehaviorType::Custom(s) => s.as_str(),
            });
            tracing::debug!(
                actor_id = %actor_id,
                actor_type = %actor_type,
                behavior = ?behavior_str,
                tenant_id = %ctx.tenant_id(),
                namespace = %ctx.namespace(),
                was_new = was_new,
                has_instance = instance.is_some(),
                "Actor registered"
            );
        }

        // Update metrics if this is a new actor
        if was_new {
            let ns = ctx.namespace().to_string();
            metrics::counter!("plexspaces_actor_spawn_total", "namespace" => ns.clone())
                .increment(1);
            metrics::gauge!("plexspaces_actor_active", "namespace" => ns).increment(1.0);
        }
    }

    /// Registers local discoverability state for a virtual actor that is known to the
    /// system but does not currently have a running sender.
    ///
    /// `VirtualActorManager` remains the source of truth for virtual actor metadata,
    /// including rebuild configuration, facets, labels, tenant/namespace, and activation
    /// strategy. The registry stores only the indexing state needed for local type discovery
    /// and tenant/namespace scoping while the actor is passivated.
    pub async fn register_virtual_actor_index(
        &self,
        ctx: &RequestContext,
        actor_id: ActorId,
        actor_type: String,
    ) {
        self.registered_actor_entries
            .write()
            .await
            .insert(Self::scoped_actor_key(
                ctx.tenant_id().to_string(),
                ctx.namespace().to_string(),
                actor_id.clone(),
            ));
        let key = (
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            actor_type,
        );
        let mut index = self.actor_type_index.write().await;
        let actor_ids = index.entry(key).or_insert_with(Vec::new);
        if !actor_ids.contains(&actor_id) {
            actor_ids.push(actor_id);
        }
    }

    /// Lookup MessageSender trait object
    ///
    /// ## Purpose
    /// Gets MessageSender trait object from registry. This enables simple tell() calls that
    /// automatically handle virtual actor activation (Orleans-inspired).
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    ///
    /// ## Returns
    /// MessageSender trait object if found, None otherwise.
    ///
    /// This method is intentionally conservative: if more than one live actor
    /// exists with the same `actor_id` across different scopes, lookup fails
    /// closed and returns `None`. Callers that already know tenant/namespace
    /// should use `lookup_actor_in_scope()` instead.
    pub async fn lookup_actor(&self, actor_id: &ActorId) -> Option<Arc<dyn MessageSender>> {
        let actors = self.actors.read().await;
        let mut matches = actors
            .iter()
            .filter(|(key, _)| key.actor_id == *actor_id)
            .map(|(_, sender)| sender.clone());
        let first = matches.next();
        if matches.next().is_some() {
            if tracing::enabled!(tracing::Level::WARN) {
                tracing::warn!(
                    actor_id = %actor_id,
                    "Ambiguous actor lookup across multiple scopes"
                );
            }
            None
        } else {
            first
        }
    }

    /// Lookup a live actor within an explicit tenant/namespace scope.
    ///
    /// This is the preferred hot-path lookup for request routing because it
    /// avoids ambiguous matches when the same actor id exists in multiple scopes.
    pub async fn lookup_actor_in_scope(
        &self,
        tenant_id: &str,
        namespace: &str,
        actor_id: &ActorId,
    ) -> Option<Arc<dyn MessageSender>> {
        let actors = self.actors.read().await;
        actors
            .get(&Self::scoped_actor_key(
                tenant_id.to_string(),
                namespace.to_string(),
                actor_id.clone(),
            ))
            .cloned()
    }

    /// Unregister actor
    /// Note: Currently only removes from actors map (MessageSender).
    /// ObjectRegistry unregister is not in the trait yet - can be added later if needed.
    pub async fn unregister(&self, actor_id: &ActorId) -> Result<(), ActorRegistryError> {
        // Remove from actors map (MessageSender)
        {
            let mut actors = self.actors.write().await;
            actors.retain(|key, _| key.actor_id != *actor_id);
        }

        // Remove from actor-type index (scan all entries to find and remove)
        {
            let mut index = self.actor_type_index.write().await;
            index.values_mut().for_each(|actor_ids| {
                actor_ids.retain(|id| id != actor_id);
            });
            // Clean up empty entries
            index.retain(|_, actor_ids| !actor_ids.is_empty());
        }

        // Remove from registered inventory
        {
            let mut registered_entries = self.registered_actor_entries.write().await;
            registered_entries.retain(|key| key.actor_id != *actor_id);
        }

        // TODO: Add unregister to ObjectRegistry trait and call it here
        // For now, we only remove from actors map

        Ok(())
    }

    /// Check if actor is activated (has a live registered sender) without exposing internals
    ///
    /// ## Purpose
    /// Allows checking if an actor is activated without exposing runtime internals.
    /// A registered live sender indicates that the actor is active.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    ///
    /// ## Returns
    /// true if actor is activated (has MessageSender), false otherwise
    pub async fn is_actor_activated(&self, actor_id: &ActorId) -> bool {
        let actors = self.actors.read().await;
        actors.keys().any(|key| key.actor_id == *actor_id)
    }

    /// Returns `true` if the actor has any registration entry (may be passivated/inactive).
    pub async fn is_actor_registered(&self, actor_id: &ActorId) -> bool {
        if self
            .registered_actor_entries
            .read()
            .await
            .iter()
            .any(|key| key.actor_id == *actor_id)
        {
            return true;
        }
        self.is_actor_activated(actor_id).await
    }

    async fn require_actor_factory(&self) -> Result<Arc<dyn ActorFactory>, ActorRegistryError> {
        self.actor_factory
            .read()
            .await
            .clone()
            .ok_or_else(|| ActorRegistryError::DependencyUnavailable("ActorFactory".to_string()))
    }

    async fn require_virtual_actor_manager(
        &self,
    ) -> Result<Arc<VirtualActorManager>, ActorRegistryError> {
        self.virtual_actor_manager
            .read()
            .await
            .clone()
            .ok_or_else(|| {
                ActorRegistryError::DependencyUnavailable("VirtualActorManager".to_string())
            })
    }

    async fn require_reply_waiter_registry(
        &self,
    ) -> Result<Arc<ReplyWaiterRegistry>, ActorRegistryError> {
        self.reply_waiter_registry
            .read()
            .await
            .clone()
            .ok_or_else(|| {
                ActorRegistryError::DependencyUnavailable("ReplyWaiterRegistry".to_string())
            })
    }

    async fn get_or_activate_local_sender(
        &self,
        actor_id: &ActorId,
    ) -> Result<Arc<dyn MessageSender>, ActorRegistryError> {
        if let Some(sender) = self.lookup_actor(actor_id).await {
            return Ok(sender);
        }

        let manager = match self.require_virtual_actor_manager().await {
            Ok(m) => m,
            Err(ActorRegistryError::DependencyUnavailable(_)) => {
                return Err(ActorRegistryError::ActorNotFound(actor_id.to_string()));
            }
            Err(e) => return Err(e),
        };
        if !manager.is_virtual(actor_id).await {
            return Err(ActorRegistryError::ActorNotFound(actor_id.to_string()));
        }

        let actor_factory = self.require_actor_factory().await?;
        actor_factory
            .activate_virtual_actor(actor_id)
            .await
            .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;

        self.lookup_actor(actor_id)
            .await
            .ok_or_else(|| ActorRegistryError::ActorNotFound(actor_id.to_string()))
    }

    async fn dispatch_local_message(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        message: Message,
    ) -> Result<(), ActorRegistryError> {
        if let Some(sender) = self.lookup_actor(actor_id).await {
            return sender
                .tell(ctx, message)
                .await
                .map_err(|e| ActorRegistryError::SendFailed(e.to_string()));
        }

        // VirtualActorManager is optional; treat its absence as "no virtual actors" rather
        // than a dependency error, so the caller gets ActorNotFound instead of Internal.
        let manager = match self.require_virtual_actor_manager().await {
            Ok(m) => m,
            Err(ActorRegistryError::DependencyUnavailable(_)) => {
                return Err(ActorRegistryError::ActorNotFound(actor_id.to_string()));
            }
            Err(e) => return Err(e),
        };
        if !manager.is_virtual(actor_id).await {
            return Err(ActorRegistryError::ActorNotFound(actor_id.to_string()));
        }

        if manager.is_active(actor_id).await {
            let sender = self.get_or_activate_local_sender(actor_id).await?;
            return sender
                .tell(ctx, message)
                .await
                .map_err(|e| ActorRegistryError::SendFailed(e.to_string()));
        }

        // Validate caller's tenant matches the virtual actor's stored tenant.
        // Both empty = system/internal (allowed). Both set = must match.
        // Caller set + actor empty = allowed (system actor). Caller empty + actor set = denied.
        if let Some(metadata) = manager.get_metadata(actor_id).await {
            let caller_tenant = ctx.tenant_id();
            let actor_tenant = &metadata.spec.tenant_id;
            if !caller_tenant.is_empty() && !actor_tenant.is_empty() && caller_tenant != actor_tenant {
                return Err(ActorRegistryError::SendFailed(format!(
                    "Tenant isolation violation: caller tenant '{}' cannot access virtual actor in tenant '{}'",
                    caller_tenant, actor_tenant
                )));
            }
            if caller_tenant.is_empty() && !actor_tenant.is_empty() && !ctx.is_internal() {
                return Err(ActorRegistryError::SendFailed(format!(
                    "Tenant isolation violation: unauthenticated caller cannot access virtual actor in tenant '{}'",
                    actor_tenant
                )));
            }
        }

        let mut should_activate = true;
        if let Ok(facet_arc) = manager.get_facet(actor_id).await {
            let facet_guard = facet_arc.read().await;
            should_activate = facet_guard.start_activation().await;
        }

        manager.queue_message(ctx, actor_id, message).await;

        if should_activate {
            let actor_factory = self.require_actor_factory().await?;
            actor_factory
                .activate_virtual_actor(actor_id)
                .await
                .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;
            manager.update_last_access(actor_id).await;
        }

        Ok(())
    }

    /// Sends a message to an actor, routing locally or remotely as needed.
    ///
    /// If the actor's node_id matches the local node, dispatches via the local
    /// mailbox (fast path). If the actor is on a different node and [`ActorService`]
    /// has been wired up, routes via gRPC with `ctx` attached to outbound metadata
    /// (same pattern as [`Self::ask`]). Returns an error if the actor is not found
    /// locally and no [`ActorService`] is available.
    ///
    /// Callers must pass the same [`RequestContext`] they use for other scoped work
    /// (JWT-derived tenant when auth is enabled; namespace from the request boundary
    /// or WASM application scope). For framework-internal fan-out where only the
    /// target namespace is known, use [`RequestContext::new_without_auth`] with the
    /// appropriate tenant and [`ActorId::namespace`].
    pub async fn tell(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        message: Message,
    ) -> Result<(), ActorRegistryError> {
        let start = std::time::Instant::now();

        // Routing: local fast-path when the actor is on this node, or its node_id is
        // an unresolved local placeholder. Route via ActorService only when the actor
        // is on an explicit, different node.
        let is_remote = !actor_id.is_on_node(&self.local_node_id);

        let result = if is_remote {
            // Remote node: route via ActorService (gRPC).
            let svc = self.actor_service.read().await.clone();
            svc.send(ctx, actor_id.as_ref(), message)
                .await
                .map(|_| ())
                .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))
        } else {
            self.dispatch_local_message(ctx, actor_id, message).await
        };

        metrics::histogram!("plexspaces_actor_registry_local_tell_duration_seconds")
            .record(start.elapsed().as_secs_f64());
        if result.is_ok() {
            metrics::counter!("plexspaces_actor_registry_local_tell_total", "result" => "ok")
                .increment(1);
        } else {
            metrics::counter!("plexspaces_actor_registry_local_tell_total", "result" => "error")
                .increment(1);
        }
        result
    }

    /// Sends a local request and waits for a reply using the temporary-sender pattern.
    pub async fn ask(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        mut message: Message,
        timeout: Duration,
    ) -> Result<Message, ActorRegistryError> {
        let waiter_registry = self.require_reply_waiter_registry().await?;
        let actor_factory = self.require_actor_factory().await?;
        let correlation_id = Ulid::new().to_string();
        let temp_sender_id =
            ActorId::temporary_sender(&correlation_id, ctx.namespace(), &self.local_node_id)
                .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;
        let expires_at = Instant::now() + (timeout * 2);

        actor_factory
            .create_temporary_sender(
                ctx,
                temp_sender_id.clone(),
                correlation_id.clone(),
                expires_at,
            )
            .await
            .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;

        let waiter = ReplyWaiter::new();
        waiter_registry
            .register(correlation_id.clone(), waiter.clone())
            .await;

        message.sender_id = temp_sender_id.to_string();
        message.correlation_id = correlation_id.clone();
        if message.receiver_id.is_empty() {
            message.receiver_id = actor_id.to_string();
        }

        // Route: local fast-path or remote via ActorService.
        // Only route remotely when node_id is an explicit non-local node name.
        let is_remote = !actor_id.is_on_node(&self.local_node_id);

        let dispatch_result = if is_remote {
            let svc = self.actor_service.read().await.clone();
            svc.send(ctx, actor_id.as_ref(), message)
                .await
                .map(|_| ())
                .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))
        } else {
            self.dispatch_local_message(ctx, actor_id, message).await
        };
        if let Err(err) = dispatch_result {
            waiter_registry.remove(&correlation_id).await;
            self.remove_temporary_sender(&temp_sender_id).await;
            return Err(err);
        }

        let reply = waiter.wait(timeout).await.map_err(|e| match e {
            crate::ReplyWaiterError::Timeout => ActorRegistryError::Timeout,
            other => ActorRegistryError::SendFailed(other.to_string()),
        });

        waiter_registry.remove(&correlation_id).await;
        self.remove_temporary_sender(&temp_sender_id).await;
        reply
    }

    /// Removes only the live sender/runtime for a passivated virtual actor while preserving
    /// metadata required for reactivation.
    pub async fn remove_live_actor_runtime(&self, actor_id: &ActorId) {
        self.actors
            .write()
            .await
            .retain(|key, _| key.actor_id != *actor_id);
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(actor_id = %actor_id, "Removed live actor runtime; metadata preserved");
        }
    }

    /// Subscribe to lifecycle events
    ///
    /// ## Purpose
    /// Adds a subscriber to receive actor lifecycle events.
    /// Used for observability backends (Prometheus, StatsD, OpenTelemetry, etc.).
    ///
    /// ## Arguments
    /// * `subscriber` - Channel sender for lifecycle events
    pub async fn subscribe_lifecycle_events(
        &self,
        subscriber: mpsc::UnboundedSender<plexspaces_proto::ActorLifecycleEvent>,
    ) {
        let mut subscribers = self.lifecycle_subscribers.write().await;
        subscribers.push(subscriber);
    }

    /// Unsubscribe from lifecycle events
    ///
    /// ## Purpose
    /// Removes all subscribers from lifecycle events.
    /// Useful when shutting down observability backends.
    ///
    /// ## Note
    /// Currently clears all subscribers. Future enhancement could add
    /// subscription IDs for selective unsubscribe.
    pub async fn unsubscribe_lifecycle_events(&self) {
        let mut subscribers = self.lifecycle_subscribers.write().await;
        subscribers.clear();
    }

    /// Publish lifecycle event to all subscribers
    ///
    /// ## Purpose
    /// Publishes actor lifecycle events to all registered subscribers.
    /// Used for observability backends (Prometheus, StatsD, OpenTelemetry, etc.).
    ///
    /// ## Arguments
    /// * `event` - The lifecycle event to publish
    pub async fn publish_lifecycle_event(&self, event: plexspaces_proto::ActorLifecycleEvent) {
        let subscribers = self.lifecycle_subscribers.read().await;
        for subscriber in subscribers.iter() {
            let _ = subscriber.send(event.clone());
        }
    }

    /// Unregister actor with cleanup
    ///
    /// ## Purpose
    /// Unregisters an actor and cleans up all associated state.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    pub async fn unregister_with_cleanup(
        &self,
        actor_id: &ActorId,
    ) -> Result<(), ActorRegistryError> {
        // Check if actor existed before removing
        let existed = {
            let actors = self.actors.read().await;
            actors.keys().any(|key| key.actor_id == *actor_id)
        };

        let namespaces_for_actor: Vec<String> = {
            let actors = self.actors.read().await;
            actors
                .keys()
                .filter(|key| key.actor_id == *actor_id)
                .map(|key| key.namespace.clone())
                .collect()
        };
        for ns in namespaces_for_actor {
            metrics::gauge!("plexspaces_actor_active", "namespace" => ns).decrement(1.0);
        }

        // Remove from actors (MessageSender trait objects)
        {
            let mut actors = self.actors.write().await;
            actors.retain(|key, _| key.actor_id != *actor_id);
        }

        // Remove from actor_type_index
        {
            let mut index = self.actor_type_index.write().await;
            // Find and remove actor_id from all entries
            index.retain(|_key, actor_ids| {
                actor_ids.retain(|id| id != actor_id);
                !actor_ids.is_empty() // Remove empty entries
            });
        }
        // CRITICAL: Lock acquisition order must be consistent to prevent deadlocks
        // Order: 1. facet_manager (via remove_facets), 2. registered inventory, 3. actor_configs
        let mut registered_entries = self.registered_actor_entries.write().await;
        let mut actor_configs = self.actor_configs.write().await;

        // Clean up parent-child relationships (Phase 3)
        {
            // Remove from parent's children list
            let mut parent_to_children = self.parent_to_children.write().await;
            parent_to_children.retain(|_parent, children| {
                children.retain(|child| child != actor_id);
                !children.is_empty() // Remove empty entries
            });
        }

        // Remove from child-to-parent mapping
        {
            let mut child_to_parent = self.child_to_parent.write().await;
            child_to_parent.remove(actor_id);
        }
        self.facet_manager.remove_facets(actor_id).await;
        registered_entries.retain(|key| key.actor_id != *actor_id);
        actor_configs.remove(actor_id);

        // OBSERVABILITY: Log actor unregistration (TRACE to reduce log noise)
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_id = %actor_id,
                existed = existed,
                "Actor unregistered with cleanup"
            );
        }

        Ok(())
    }

    // === Temporary Sender Management ===

    /// Register a temporary sender ActorRef for ask() pattern
    ///
    /// ## Purpose
    /// Registers a temporary sender ActorRef in ActorRegistry so it can be looked up when replies arrive.
    /// Used when ask() is called from outside an actor context.
    ///
    /// ## Design
    /// Temporary senders are registered as actual ActorRefs in the actors map (not just IDs).
    /// This allows `send_reply()` to look them up via `lookup_actor()` and call `tell()` on them.
    /// The temporary sender ActorRef's `tell()` method routes messages to ReplyWaiter.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `temporary_sender_id` - Temporary sender actor ID
    /// * `temporary_sender_ref` - ActorRef for the temporary sender (implements MessageSender)
    /// * `correlation_id` - Correlation ID for matching replies
    /// * `expires_at` - Expiration time for automatic cleanup
    pub async fn register_temporary_sender(
        &self,
        ctx: &RequestContext,
        temporary_sender_id: ActorId,
        temporary_sender_ref: Arc<dyn MessageSender>,
        correlation_id: String,
        expires_at: Instant,
    ) {
        let correlation_id_clone = correlation_id.clone();
        self.register_actor(
            ctx,
            ActorRegistrationParams {
                actor_id: temporary_sender_id.clone(),
                sender: temporary_sender_ref,
                actor_type: TEMP_SENDER_ACTOR_TYPE.to_string(),
                config: None,
                instance: None,
                behavior_kind: None,
            },
        )
        .await;

        let mut temp_senders = self.temporary_senders.write().await;
        temp_senders.insert(
            temporary_sender_id.clone(),
            TemporarySenderEntry {
                actor_ref_id: temporary_sender_id.clone(),
                correlation_id: correlation_id_clone,
                expires_at,
            },
        );
        let count = temp_senders.len();
        drop(temp_senders);

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
            "ActorRegistry: Registered temporary sender ActorRef: temporary_sender_id={}, correlation_id={}, expires_at={:?}, total_temp_senders={}",
            temporary_sender_id,
            correlation_id,
            expires_at,
            count
        );
        }
    }

    /// Lookup temporary sender entry
    ///
    /// ## Arguments
    /// * `temporary_sender_id` - Temporary sender ID to lookup
    ///
    /// ## Returns
    /// Some(TemporarySenderEntry) if found, None otherwise
    pub async fn lookup_temporary_sender(
        &self,
        temporary_sender_id: &ActorId,
    ) -> Option<TemporarySenderEntry> {
        let temp_senders = self.temporary_senders.read().await;
        temp_senders.get(temporary_sender_id).cloned()
    }

    /// Remove a temporary sender mapping
    ///
    /// ## Purpose
    /// Removes temporary sender from both actors map and temporary_senders map.
    /// This ensures complete cleanup when temporary sender is no longer needed.
    ///
    /// ## Arguments
    /// * `temporary_sender_id` - Temporary sender ID to remove
    pub async fn remove_temporary_sender(&self, temporary_sender_id: &ActorId) {
        if let Err(e) = self.unregister_with_cleanup(temporary_sender_id).await {
            tracing::warn!(
                "ActorRegistry: Failed to unregister temporary sender ActorRef: temporary_sender_id={}, error={}",
                temporary_sender_id, e
            );
        }
        let mut temp_senders = self.temporary_senders.write().await;
        if temp_senders.remove(temporary_sender_id).is_some()
            && tracing::enabled!(tracing::Level::TRACE)
        {
            tracing::trace!(
                "ActorRegistry: Removed temporary sender: temporary_sender_id={}, remaining={}",
                temporary_sender_id,
                temp_senders.len()
            );
        }
    }

    /// Cleanup expired temporary senders
    ///
    /// ## Purpose
    /// Removes expired temporary sender mappings and unregisters them from the actors map.
    /// This prevents memory leaks. Should be called periodically (e.g., every 30 seconds).
    ///
    /// ## Returns
    /// `(expired_count, remaining_temporary_senders_after)`.
    pub async fn cleanup_expired_temporary_senders(&self) -> (usize, usize) {
        let now = Instant::now();
        let expired_ids: Vec<ActorId> = {
            let temp_senders = self.temporary_senders.read().await;
            temp_senders
                .iter()
                .filter(|(_id, entry)| entry.expires_at <= now)
                .map(|(id, _)| id.clone())
                .collect()
        };

        let expired_count = expired_ids.len();

        for temp_sender_id in &expired_ids {
            if let Err(e) = self.unregister_with_cleanup(temp_sender_id).await {
                tracing::warn!(
                    "ActorRegistry: Failed to unregister expired temporary sender ActorRef: temporary_sender_id={}, error={}",
                    temp_sender_id, e
                );
            }
        }

        let remaining = if expired_count > 0 {
            let mut temp_senders = self.temporary_senders.write().await;
            for temp_sender_id in &expired_ids {
                temp_senders.remove(temp_sender_id);
            }
            let after_count = temp_senders.len();

            // OBSERVABILITY: Track expired temporary sender cleanup
            metrics::counter!("plexspaces_actor_registry_temporary_sender_expired_total",
                "node_id" => self.local_node_id.clone()
            )
            .increment(expired_count as u64);
            metrics::gauge!("plexspaces_actor_registry_temporary_sender_mappings",
                "node_id" => self.local_node_id.clone()
            )
            .set(after_count as f64);

            after_count
        } else {
            self.temporary_senders.read().await.len()
        };

        (expired_count, remaining)
    }

    /// Get count of temporary senders (for metrics/monitoring)
    /// Discover actors by type (efficient O(1) lookup using index)
    ///
    /// ## Purpose
    /// Finds actors by actor_type within a tenant using efficient hashmap lookup.
    /// Used for FaaS-like actor request routing where we need to find any actor of a given type.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (first parameter)
    /// * `actor_type` - Actor type to search for
    ///
    /// ## Returns
    /// Vector of actor IDs matching the type
    ///
    /// ## Performance
    /// O(1) lookup using hashmap index, much faster than scanning ObjectRegistry
    pub async fn discover_actors_by_type(
        &self,
        ctx: &RequestContext,
        actor_type: &str,
    ) -> Vec<ActorId> {
        let index = self.actor_type_index.read().await;
        let key = (
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            actor_type.to_string(),
        );
        let actor_ids = index.get(&key).cloned().unwrap_or_default();
        tracing::debug!(
            tenant_id = %key.0,
            namespace = %key.1,
            actor_type = %key.2,
            actor_count = actor_ids.len(),
            actor_ids = ?actor_ids,
            "discover_actors_by_type"
        );
        actor_ids
    }

    /// Returns the number of outstanding temporary senders (reply correlations in flight).
    pub async fn temporary_sender_count(&self) -> usize {
        let temp_senders = self.temporary_senders.read().await;
        temp_senders.len()
    }

    // ============================================================================
    // Parent-Child Relationship Tracking (Phase 3)
    // ============================================================================

    /// Register parent-child relationship
    ///
    /// ## Purpose
    /// Tracks supervision hierarchy for graceful shutdown and subtree operations.
    /// Used by supervisors to register their children (actors or nested supervisors).
    ///
    /// ## Erlang/OTP Equivalent
    /// In Erlang, supervisors track children via child_spec. This method provides
    /// the same tracking capability for supervision trees.
    ///
    /// ## Arguments
    /// * `parent_id` - Parent actor/supervisor ID
    /// * `child_id` - Child actor/supervisor ID
    ///
    /// ## Note
    /// If child already has a parent, it will be removed from the old parent's children list
    /// and added to the new parent's children list. A child can only have one parent.
    ///
    /// ## Example
    /// ```rust,ignore
    /// registry.register_parent_child("supervisor1", "worker1").await;
    /// registry.register_parent_child("supervisor1", "worker2").await;
    /// let children = registry.get_children("supervisor1").await;
    /// assert_eq!(children.len(), 2);
    /// ```
    pub async fn register_parent_child(&self, parent_id: &ActorId, child_id: &ActorId) {
        // Remove child from old parent's children list (if any)
        {
            let mut parent_to_children = self.parent_to_children.write().await;
            if let Some(old_parent) = self.child_to_parent.read().await.get(child_id) {
                if let Some(old_children) = parent_to_children.get_mut(old_parent) {
                    old_children.retain(|id| id != child_id);
                    if old_children.is_empty() {
                        parent_to_children.remove(old_parent);
                    }
                }
            }
        }

        // Add to parent -> children mapping
        {
            let mut map = self.parent_to_children.write().await;
            map.entry(parent_id.clone())
                .or_insert_with(Vec::new)
                .push(child_id.clone());
        }

        // Add to child -> parent mapping
        {
            let mut map = self.child_to_parent.write().await;
            map.insert(child_id.clone(), parent_id.clone());
        }

        // OBSERVABILITY: Metrics and logging
        metrics::gauge!("plexspaces_actor_children_count",
            "parent_id" => parent_id.to_string()
        )
        .set({
            let map = self.parent_to_children.read().await;
            map.get(parent_id).map(|v| v.len() as f64).unwrap_or(0.0)
        });

        metrics::counter!("plexspaces_actor_parent_child_registered_total",
            "parent_id" => parent_id.to_string(),
            "child_id" => child_id.to_string()
        )
        .increment(1);

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                parent = %parent_id,
                child = %child_id,
                "Registered parent-child relationship"
            );
        }
    }

    /// Unregister parent-child relationship
    ///
    /// ## Purpose
    /// Removes parent-child relationship when child is terminated or removed.
    /// Called automatically during actor unregistration.
    ///
    /// ## Arguments
    /// * `parent_id` - Parent actor/supervisor ID
    /// * `child_id` - Child actor/supervisor ID
    pub async fn unregister_parent_child(&self, parent_id: &ActorId, child_id: &ActorId) {
        // Remove from parent -> children mapping
        {
            let mut map = self.parent_to_children.write().await;
            if let Some(children) = map.get_mut(parent_id) {
                children.retain(|id| id != child_id);
                if children.is_empty() {
                    map.remove(parent_id);
                }
            }
        }

        // Remove from child -> parent mapping
        {
            let mut map = self.child_to_parent.write().await;
            map.remove(child_id);
        }

        // OBSERVABILITY: Metrics and logging
        metrics::gauge!("plexspaces_actor_children_count",
            "parent_id" => parent_id.to_string()
        )
        .set({
            let map = self.parent_to_children.read().await;
            map.get(parent_id).map(|v| v.len() as f64).unwrap_or(0.0)
        });

        metrics::counter!("plexspaces_actor_parent_child_unregistered_total",
            "parent_id" => parent_id.to_string(),
            "child_id" => child_id.to_string()
        )
        .increment(1);

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                parent = %parent_id,
                child = %child_id,
                "Unregistered parent-child relationship"
            );
        }
    }

    /// Get all children of a parent
    ///
    /// ## Purpose
    /// Returns all direct children of a parent actor/supervisor.
    /// Used by supervisors to enumerate their children for shutdown/restart operations.
    ///
    /// ## Arguments
    /// * `parent_id` - Parent actor/supervisor ID
    ///
    /// ## Returns
    /// Vector of child actor/supervisor IDs
    ///
    /// ## Example
    /// ```rust,ignore
    /// let children = registry.get_children("supervisor1").await;
    /// for child_id in children {
    ///     // Stop or restart child
    /// }
    /// ```
    pub async fn get_children(&self, parent_id: &ActorId) -> Vec<ActorId> {
        let map = self.parent_to_children.read().await;
        map.get(parent_id).cloned().unwrap_or_default()
    }

    /// Get parent of a child
    ///
    /// ## Purpose
    /// Returns the parent actor/supervisor of a child.
    /// Used for cascading shutdown and parent notification.
    ///
    /// ## Arguments
    /// * `child_id` - Child actor/supervisor ID
    ///
    /// ## Returns
    /// Some(parent_id) if child has a parent, None otherwise
    ///
    /// ## Example
    /// ```rust,ignore
    /// if let Some(parent_id) = registry.get_parent("worker1").await {
    ///     // Notify parent of child termination
    /// }
    /// ```
    pub async fn get_parent(&self, child_id: &ActorId) -> Option<ActorId> {
        let map = self.child_to_parent.read().await;
        map.get(child_id).cloned()
    }

    /// Get entire subtree under a supervisor (recursive)
    ///
    /// ## Purpose
    /// Returns all actors/supervisors in the subtree rooted at the given supervisor.
    /// Used for graceful shutdown of entire supervision trees.
    ///
    /// ## Arguments
    /// * `root_id` - Root supervisor ID
    ///
    /// ## Returns
    /// Vector of all actor/supervisor IDs in the subtree (breadth-first order)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let subtree = registry.get_subtree("root-supervisor").await;
    /// // Shutdown all actors in subtree
    /// for actor_id in subtree {
    ///     // Stop actor
    /// }
    /// ```
    pub async fn get_subtree(&self, root_id: &ActorId) -> Vec<ActorId> {
        use std::collections::VecDeque;

        let mut result = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(root_id.clone());

        while let Some(current) = queue.pop_front() {
            let children = self.get_children(&current).await;
            for child in children {
                result.push(child.clone());
                queue.push_back(child);
            }
        }

        // OBSERVABILITY: Track subtree size
        metrics::gauge!("plexspaces_actor_subtree_size",
            "root_id" => root_id.to_string()
        )
        .set(result.len() as f64);

        result
    }

    /// Get children count for a parent
    ///
    /// ## Purpose
    /// Returns the number of direct children for a parent.
    /// Used for metrics and monitoring.
    ///
    /// ## Arguments
    /// * `parent_id` - Parent actor/supervisor ID
    ///
    /// ## Returns
    /// Number of direct children
    pub async fn children_count(&self, parent_id: &ActorId) -> usize {
        let map = self.parent_to_children.read().await;
        map.get(parent_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Start background cleanup task for expired temporary senders
    ///
    /// ## Purpose
    /// Periodically cleans up expired temporary sender mappings to prevent memory leaks.
    /// Runs every 30 seconds.
    ///
    /// ## Note
    /// This should be called once when the node starts. The task will run until
    /// the node shuts down.
    ///
    /// ## Arguments
    /// * `registry` - Arc<ActorRegistry> to use for cleanup (must be Arc to share across tasks)
    pub fn start_temporary_sender_cleanup(registry: Arc<Self>) {
        let local_node_id = registry.local_node_id.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));

            loop {
                interval.tick().await;

                let (expired_count, after_count) =
                    registry.cleanup_expired_temporary_senders().await;
                if expired_count > 0 && tracing::enabled!(tracing::Level::DEBUG) {
                    let before_count = expired_count + after_count;
                    tracing::debug!(
                        expired_count = expired_count,
                        before_count = before_count,
                        after_count = after_count,
                        node_id = %local_node_id,
                        "ActorRegistry: Cleaned up expired temporary senders"
                    );
                }
            }
        });
    }

    // ============================================================================
    // Link/Monitor Semantics (Phase 6: Erlang/OTP-style)
    // ============================================================================

    /// Link two actors (bidirectional death propagation)
    ///
    /// ## Purpose
    /// Creates a bidirectional link between two actors. If one dies abnormally,
    /// the other automatically dies (cascading failure). This is the foundation
    /// for supervision trees.
    ///
    /// ## Erlang Equivalent
    /// Maps to Erlang's `link/1` function.
    ///
    /// ## Arguments
    /// * `actor1_id` - First actor ID
    /// * `actor2_id` - Second actor ID
    ///
    /// ## Behavior
    /// - Links are bidirectional: if A links to B, B is automatically linked to A
    /// - Normal/Shutdown exits don't propagate to links
    /// - Error/Killed exits propagate to links (cascading failure)
    /// - If linked actor has trap_exit=true, it receives EXIT as message
    /// - If linked actor has trap_exit=false, it terminates immediately
    ///
    /// ## Returns
    /// `Ok(())` on success, `Err(ActorRegistryError)` on failure
    /// Add a bidirectional link between two **local** actors. Delegates to `ActorMonitor`.
    pub async fn local_link(
        &self,
        actor1_id: &ActorId,
        actor2_id: &ActorId,
    ) -> Result<(), ActorRegistryError> {
        self.actor_monitor
            .add_link(actor1_id, actor2_id)
            .await
            .map_err(|e| ActorRegistryError::RegistrationFailed(e.to_string()))
    }

    /// Remove the bidirectional link between two **local** actors.
    pub async fn local_unlink(
        &self,
        actor1_id: &ActorId,
        actor2_id: &ActorId,
    ) -> Result<(), ActorRegistryError> {
        self.actor_monitor.remove_link(actor1_id, actor2_id).await;
        Ok(())
    }

    /// Return all actors linked to `actor_id`.  Delegates to `ActorMonitor`.
    pub async fn get_links(&self, actor_id: &ActorId) -> Vec<ActorId> {
        self.actor_monitor.get_links(actor_id).await
    }

    /// Register a one-way monitor in the local `ActorMonitor` only.
    ///
    /// `ctx` must be the authenticated [`RequestContext`] for the monitor-establishing
    /// operation (edge gRPC/JWT or explicit caller scope). It is stored and replayed
    /// when delivering `__DOWN__` to a remote supervisor.
    pub async fn local_monitor(
        &self,
        ctx: &RequestContext,
        target_id: &ActorId,
        monitor_id: &ActorId,
        monitor_ref: String,
    ) -> Result<(), ActorRegistryError> {
        self.actor_monitor
            .add_monitor(target_id, monitor_id, monitor_ref, ctx.clone())
            .await;
        Ok(())
    }

    /// Remove a specific monitor by its `monitor_ref` on this node only.
    pub async fn local_demonitor(
        &self,
        target_id: &ActorId,
        _monitor_id: &ActorId,
        monitor_ref: &str,
    ) -> Result<(), ActorRegistryError> {
        self.actor_monitor
            .remove_monitor(target_id, monitor_ref)
            .await;
        Ok(())
    }

    /// Establish a monitor (location-transparent): local targets update this node's monitor table;
    /// remote targets are forwarded via [`ActorService::monitor_actor`].
    ///
    /// # TODO (lazy virtual actors)
    ///
    /// When the monitored actor is a **passivated** lazy virtual actor (`is_actor_activated` is
    /// false but it is registered), decide whether to activate first, register metadata-only, or
    /// defer until activation. Until then, local monitor requires an activated sender.
    pub async fn monitor(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        supervisor_id: &ActorId,
    ) -> Result<String, ActorRegistryError> {
        if actor_id.is_on_node(&self.local_node_id) {
            self.validate_link_monitor_operand_scope(ctx, &[actor_id, supervisor_id])
                .await?;
            // TODO(lazy-virtual): allow monitor/link for passivated lazy virtuals (policy TBD).
            if !self.is_actor_activated(actor_id).await {
                return Err(ActorRegistryError::ActorNotFound(actor_id.to_string()));
            }
            let monitor_ref = ulid::Ulid::new().to_string();
            self.local_monitor(ctx, actor_id, supervisor_id, monitor_ref.clone())
                .await?;
            Ok(monitor_ref)
        } else {
            let svc = self.actor_service.read().await.clone();
            let callback = self.local_listen_base_url().await;
            if callback.is_empty() {
                return Err(ActorRegistryError::DependencyUnavailable(
                    "local listen URL unset; call set_local_listen_addr before cross-node monitor"
                        .into(),
                ));
            }
            svc.monitor_actor(ctx, actor_id.as_str(), supervisor_id.as_str(), &callback)
                .await
                .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))
        }
    }

    /// Cancel a monitor (location-transparent).
    pub async fn demonitor(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        supervisor_id: &ActorId,
        monitor_ref: &str,
    ) -> Result<(), ActorRegistryError> {
        if actor_id.is_on_node(&self.local_node_id) {
            self.validate_link_monitor_operand_scope(ctx, &[actor_id, supervisor_id])
                .await?;
            self.local_demonitor(actor_id, supervisor_id, monitor_ref)
                .await
        } else {
            let svc = self.actor_service.read().await.clone();
            svc.demonitor_actor(ctx, actor_id.as_str(), supervisor_id.as_str(), monitor_ref)
                .await
                .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))
        }
    }

    /// Bidirectional link between two actors (location-transparent via [`ActorService`]).
    ///
    /// # TODO (lazy virtual actors)
    ///
    /// Same as [`Self::monitor`]: linking when one or both operands are passivated lazy virtuals
    /// needs an explicit policy (activate vs metadata-only).
    pub async fn link(
        &self,
        ctx: &RequestContext,
        actor1_id: &ActorId,
        actor2_id: &ActorId,
    ) -> Result<(), ActorRegistryError> {
        if actor1_id == actor2_id {
            return Err(ActorRegistryError::RegistrationFailed(
                "Cannot link actor to itself".into(),
            ));
        }
        let local = &self.local_node_id;
        if actor1_id.is_on_node(local) {
            if actor2_id.is_on_node(local) {
                self.validate_link_monitor_operand_scope(ctx, &[actor1_id, actor2_id])
                    .await?;
                return self.local_link(actor1_id, actor2_id).await;
            }
            self.validate_link_monitor_operand_scope(ctx, &[actor1_id, actor2_id])
                .await?;
            // Guard: if this node already recorded the link, a return-RPC from the peer arrived.
            // Registering locally without calling back breaks the cross-node echo cycle.
            let already_linked = self.get_links(actor1_id).await.contains(actor2_id);
            self.local_link(actor1_id, actor2_id).await?;
            if !already_linked {
                let svc = self.actor_service.read().await.clone();
                svc.link_actor(ctx, actor2_id.as_str(), actor1_id.as_str())
                    .await
                    .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;
            }
            return Ok(());
        }
        if actor2_id.is_on_node(local) {
            // Guard: if this node already recorded the link, the peer has already been notified.
            let already_linked = self.get_links(actor2_id).await.contains(actor1_id);
            if !already_linked {
                let svc = self.actor_service.read().await.clone();
                svc.link_actor(ctx, actor1_id.as_str(), actor2_id.as_str())
                    .await
                    .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;
            }
            self.validate_link_monitor_operand_scope(ctx, &[actor1_id, actor2_id])
                .await?;
            self.local_link(actor1_id, actor2_id).await?;
            return Ok(());
        }
        let svc = self.actor_service.read().await.clone();
        svc.link_actor(ctx, actor1_id.as_str(), actor2_id.as_str())
            .await
            .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;
        svc.link_actor(ctx, actor2_id.as_str(), actor1_id.as_str())
            .await
            .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;
        Ok(())
    }

    /// Remove a bidirectional link (location-transparent via [`ActorService`]).
    pub async fn unlink(
        &self,
        ctx: &RequestContext,
        actor1_id: &ActorId,
        actor2_id: &ActorId,
    ) -> Result<(), ActorRegistryError> {
        let local = &self.local_node_id;
        if actor1_id.is_on_node(local) {
            if actor2_id.is_on_node(local) {
                self.validate_link_monitor_operand_scope(ctx, &[actor1_id, actor2_id])
                    .await?;
                return self.local_unlink(actor1_id, actor2_id).await;
            }
            self.validate_link_monitor_operand_scope(ctx, &[actor1_id, actor2_id])
                .await?;
            // Guard: only send the RPC if the link exists locally; absence means the return-RPC
            // from the peer already processed this unlink and we must not echo back.
            let was_linked = self.get_links(actor1_id).await.contains(actor2_id);
            self.local_unlink(actor1_id, actor2_id).await?;
            if was_linked {
                let svc = self.actor_service.read().await.clone();
                svc.unlink_actor(ctx, actor2_id.as_str(), actor1_id.as_str())
                    .await
                    .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;
            }
            return Ok(());
        }
        if actor2_id.is_on_node(local) {
            let was_linked = self.get_links(actor2_id).await.contains(actor1_id);
            if was_linked {
                let svc = self.actor_service.read().await.clone();
                svc.unlink_actor(ctx, actor1_id.as_str(), actor2_id.as_str())
                    .await
                    .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;
            }
            self.validate_link_monitor_operand_scope(ctx, &[actor1_id, actor2_id])
                .await?;
            self.local_unlink(actor1_id, actor2_id).await?;
            return Ok(());
        }
        let svc = self.actor_service.read().await.clone();
        svc.unlink_actor(ctx, actor1_id.as_str(), actor2_id.as_str())
            .await
            .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;
        svc.unlink_actor(ctx, actor2_id.as_str(), actor1_id.as_str())
            .await
            .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))?;
        Ok(())
    }

    /// Handle actor termination - notify monitors and propagate to links (Phase 6)
    ///
    /// ## Purpose
    /// Comprehensive termination handler that:
    /// 1. Sends DOWN messages to all monitors
    /// 2. Propagates EXIT signals to all linked actors
    /// 3. Cleans up actor's link/monitor entries
    ///
    /// ## Erlang Semantics
    /// 1. Send DOWN to all monitors (informational, receiver continues)
    /// 2. Send EXIT to all linked actors:
    ///    - If linked actor traps exits: receives EXIT as message
    ///    - If linked actor doesn't trap: terminates with same reason
    /// 3. Normal/Shutdown exits don't propagate to links
    /// 4. Error/Killed exits propagate to links (cascading failure)
    ///
    /// ## Arguments
    /// * `actor_id` - The actor that terminated
    /// * `reason` - Exit reason for termination
    ///
    /// ## Behavior
    /// - Normal/Shutdown: Only sends DOWN to monitors, doesn't propagate to links
    /// - Error/Killed: Sends DOWN to monitors AND propagates EXIT to links
    /// - Linked actors with trap_exit=true receive EXIT as message
    /// - Linked actors with trap_exit=false terminate immediately
    pub async fn handle_actor_termination(&self, actor_id: &ActorId, reason: ExitReason) {
        // 1. Send DOWN to all monitors (always, regardless of exit reason)
        self.send_down_to_monitors(actor_id, &reason).await;

        // 2. Propagate EXIT to linked actors (only for error exits)
        // Normal and Shutdown exits don't propagate to links (Erlang semantics)
        let is_error = reason.is_error();
        if is_error {
            self.propagate_exit_to_links(actor_id, &reason).await;
        }

        // 3. Clean up this actor's link/monitor entries
        self.cleanup_terminated_actor_links_monitors(actor_id).await;

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_id = %actor_id,
                reason = ?reason,
                propagated_exit_to_links = is_error,
                "Actor termination handled; link and monitor state cleaned up"
            );
        }
    }

    /// Send DOWN message to all monitors (Phase 6)
    ///
    /// ## Purpose
    /// Notifies all monitors that the target actor has terminated.
    /// This is a one-way notification - monitors don't die.
    ///
    /// ## Arguments
    /// * `actor_id` - The actor that terminated
    /// * `reason` - Exit reason
    async fn send_down_to_monitors(&self, actor_id: &ActorId, reason: &ExitReason) {
        let monitors = self.actor_monitor.get_monitors(actor_id).await;

        let reason_str = exit_reason_to_string(reason);

        for monitor_link in &monitors {
            let down_msg = create_down_message(actor_id, &monitor_link.monitor_ref, &reason_str);
            let monitoring_id = &monitor_link.monitoring_actor_id;

            let delivery_result = if monitoring_id.is_on_node(&self.local_node_id) {
                self.dispatch_local_message(
                    &monitor_link.monitoring_context,
                    monitoring_id,
                    down_msg,
                )
                .await
            } else {
                let svc = self.actor_service.read().await.clone();
                svc.send(
                    &monitor_link.monitoring_context,
                    monitoring_id.as_ref(),
                    down_msg,
                )
                .await
                .map(|_| ())
                .map_err(|e| ActorRegistryError::SendFailed(e.to_string()))
            };
            if let Err(e) = delivery_result {
                tracing::warn!(
                    from = %actor_id,
                    to = %monitoring_id,
                    error = %e,
                    "Failed to deliver __DOWN__ to monitoring actor"
                );
            }

            // Notify facets on the monitoring actor (best-effort; actor may be remote)
            let facet_exit_reason = match &reason {
                ExitReason::Normal => FacetExitReason::Normal,
                ExitReason::Shutdown => FacetExitReason::Shutdown,
                ExitReason::Killed => FacetExitReason::Killed,
                ExitReason::Error(msg) => FacetExitReason::Error(msg.clone()),
                ExitReason::Linked {
                    actor_id: linked_id,
                    reason: linked_reason,
                } => {
                    let linked_reason_str = match linked_reason.as_ref() {
                        ExitReason::Normal => "normal",
                        ExitReason::Shutdown => "shutdown",
                        ExitReason::Killed => "killed",
                        ExitReason::Error(msg) => msg,
                        ExitReason::Linked { .. } => "linked",
                    };
                    FacetExitReason::Error(format!(
                        "Linked: {} -> {}",
                        linked_id, linked_reason_str
                    ))
                }
            };

            let facet_down_start = std::time::Instant::now();
            let monitoring_actor_id_str = monitoring_id.to_string();
            let facet_down_result: Result<Vec<plexspaces_facet::FacetError>, String> = self
                .facet_manager
                .call_on_down(
                    monitoring_actor_id_str.clone(),
                    actor_id.to_string(),
                    &facet_exit_reason,
                )
                .await;

            let facet_down_duration = facet_down_start.elapsed();
            match facet_down_result {
                Ok(errors) if !errors.is_empty() => {
                    metrics::counter!("plexspaces_facet_down_errors_total",
                        "monitoring_actor_id" => monitoring_actor_id_str.clone(),
                        "monitored_actor_id" => actor_id.to_string(),
                        "error_count" => errors.len().to_string()
                    )
                    .increment(errors.len() as u64);
                    tracing::warn!(
                        monitoring_actor_id = %monitoring_actor_id_str,
                        monitored_actor_id = %actor_id,
                        error_count = errors.len(),
                        "Some facets failed to handle DOWN notification (continuing)"
                    );
                }
                Ok(_) => {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            monitoring_actor_id = %monitoring_actor_id_str,
                            monitored_actor_id = %actor_id,
                            duration_ms = facet_down_duration.as_millis(),
                            "All facets handled DOWN notification successfully"
                        );
                    }
                }
                Err(e) => {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            monitoring_actor_id = %monitoring_actor_id_str,
                            monitored_actor_id = %actor_id,
                            error = %e,
                            "Facets not found in FacetManager (regular actor)"
                        );
                    }
                }
            }

            metrics::histogram!("plexspaces_facet_down_duration_seconds",
                "monitoring_actor_id" => monitoring_actor_id_str.clone(),
                "monitored_actor_id" => actor_id.to_string()
            )
            .record(facet_down_duration.as_secs_f64());
            metrics::counter!("plexspaces_facet_down_total",
                "monitoring_actor_id" => monitoring_actor_id_str.clone(),
                "monitored_actor_id" => actor_id.to_string()
            )
            .increment(1);

            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    from = %actor_id,
                    to = %monitoring_id,
                    monitor_ref = %monitor_link.monitor_ref,
                    reason = %reason_str,
                    "Sent __DOWN__ notification to monitoring actor"
                );
            }
        }

        // OBSERVABILITY: Metrics
        metrics::counter!("plexspaces_actor_exit_handled_total",
            "actor_id" => actor_id.to_string(),
            "action" => "down_sent"
        )
        .increment(monitors.len() as u64);
    }

    /// Propagate EXIT to linked actors (Phase 6)
    ///
    /// ## Purpose
    /// Sends EXIT signals to all linked actors, causing cascading failures.
    /// Only called for error exits (Normal/Shutdown don't propagate).
    ///
    /// ## Arguments
    /// * `actor_id` - The actor that terminated
    /// * `reason` - Exit reason (must be an error)
    ///
    /// ## Behavior
    /// - Gets all linked actors
    /// - For each linked actor:
    ///   - If trap_exit=true: Sends EXIT as message to actor's mailbox
    ///   - If trap_exit=false: Terminates actor immediately with Linked reason
    async fn propagate_exit_to_links(&self, actor_id: &ActorId, reason: &ExitReason) {
        let linked_actors = self.actor_monitor.get_links(actor_id).await;

        if linked_actors.is_empty() {
            return;
        }

        let linked_count = linked_actors.len();
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                from = %actor_id,
                linked_count = linked_count,
                reason = ?reason,
                "Propagating EXIT to linked actors"
            );
        }

        // Create Linked exit reason for linked actors
        let _linked_reason = ExitReason::Linked {
            actor_id: actor_id.clone(),
            reason: Box::new(reason.clone()),
        };

        let reason_str = exit_reason_to_string(reason);

        // Propagate to each linked actor
        for linked_id in &linked_actors {
            let exit_message = create_exit_message(actor_id.to_string(), &reason_str);

            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    from = %actor_id,
                    to = %linked_id,
                    reason_str = %reason_str,
                    "Sending EXIT to linked actor"
                );
            }

            // Tenant from the dying actor's registration when present; namespace from the
            // linked peer's canonical id so remote gRPC metadata matches the target mailbox.
            let (tenant_id, _) = self
                .get_actor_metadata(actor_id)
                .await
                .unwrap_or_else(|| ("system".to_string(), actor_id.namespace().to_string()));
            let exit_ctx = crate::RequestContext::new_without_auth(
                tenant_id,
                linked_id.namespace().to_string(),
            );
            if let Err(e) = self.tell(&exit_ctx, linked_id, exit_message).await {
                tracing::warn!(
                    from = %actor_id,
                    to = %linked_id,
                    error = %e,
                    "Failed to deliver __EXIT__ to linked actor"
                );
            } else if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    from = %actor_id,
                    to = %linked_id,
                    "Delivered __EXIT__ to local linked actor"
                );
            }
        }

        // OBSERVABILITY: Metrics
        metrics::counter!("plexspaces_actor_exit_propagated_total",
            "actor_id" => actor_id.to_string(),
            "linked_count" => linked_count.to_string()
        )
        .increment(linked_count as u64);
    }

    /// Clean up link/monitor entries for terminated actor (Phase 6)
    ///
    /// ## Purpose
    /// Removes all link and monitor entries for a terminated actor.
    ///
    /// ## Arguments
    /// * `actor_id` - The actor that terminated
    async fn cleanup_terminated_actor_links_monitors(&self, actor_id: &ActorId) {
        self.actor_monitor
            .cleanup_monitors_for_actor(actor_id)
            .await;
        self.actor_monitor.cleanup_links_for_actor(actor_id).await;

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_id = %actor_id,
                "Cleaned up link/monitor entries for terminated actor"
            );
        }
    }

    /// Return all monitor entries (delegates to `ActorMonitor`).
    /// Used by the stale-monitor GC.
    pub async fn all_monitor_entries(&self) -> Vec<(ActorId, String, ActorId)> {
        self.actor_monitor.all_monitor_entries().await
    }

    /// Remove monitor entries for actors in `actor_ids` (delegates to `ActorMonitor`).
    pub async fn remove_monitors_for_actors(&self, actor_ids: &[ActorId]) {
        self.actor_monitor
            .remove_monitors_for_actors(actor_ids)
            .await;
    }

    /// Resolve a flexible actor target string into a canonical [`ActorId`].
    ///
    /// Accepts three addressing formats used by WASM, HTTP, and gRPC callers:
    ///
    /// | Format | Example | Resolution |
    /// |--------|---------|------------|
    /// | Canonical | `alerts//channel::ns@node` | Parsed directly |
    /// | `actor_type:name` | `channel:alerts` | Live lookup then virtual |
    /// | Bare name / type | `channel` | Discover live actors by type |
    ///
    /// Resolution order (fastest to slowest):
    /// 1. Canonical `//` format → `ActorId::from_canonical` (zero registry traffic)
    /// 2. `type:name` → O(1) live lookup (type-index), then virtual definition build
    /// 3. Bare address → O(1) type-index lookup, then virtual definition build
    ///
    /// All three paths consult `VirtualActorManager` when no live actor is found so that
    /// passivated virtual actors can be addressed by short form.
    pub async fn resolve_actor_id(
        &self,
        ctx: &RequestContext,
        target: &str,
    ) -> Result<ActorId, String> {
        // 1. Canonical format — fast path, no registry traffic needed.
        if target.contains("//") {
            return ActorId::from_canonical(target)
                .map_err(|e| format!("Invalid canonical actor ID '{target}': {e}"));
        }

        let namespace = ctx.namespace().to_string();

        if let Some((actor_type, name)) = target.split_once(':') {
            if actor_type.is_empty() || name.is_empty() {
                return Err(format!(
                    "Invalid actor target format '{target}': type and name must both be non-empty"
                ));
            }

            // 2a. Live O(1) lookup: interpret left=actor_type, right=name.
            {
                let actor_ids = self.discover_actors_by_type(ctx, actor_type).await;
                if let Some(live_id) = actor_ids
                    .iter()
                    .find(|id| id.name() == name && id.namespace() == namespace)
                {
                    return Ok(live_id.clone());
                }
            }

            // 2b. Virtual actor definition lookup via VirtualActorManager.
            if let Some(manager) = self.virtual_actor_manager.read().await.clone() {
                // Resolve actor_type in case the left side is a definition name.
                let resolved_type = manager
                    .resolve_actor_type_for_name(&namespace, actor_type)
                    .await;
                // Try to get a virtual actor type registration so we can obtain the namespace.
                if let Some(type_meta) = manager.get_virtual_actor_type(&resolved_type).await {
                    let canonical_ns = type_meta.namespace().to_string();
                    let actor_id =
                        ActorId::new(name, &resolved_type, &canonical_ns, &self.local_node_id)
                            .map_err(|e| format!("Cannot build actor ID for '{target}': {e}"))?;
                    manager
                        .prime_instance_from_definition(&actor_id, &type_meta)
                        .await;
                    return Ok(actor_id);
                }
                // Definition-name lookup: left=definition_name, right=instance_name.
                if let Some(def_meta) = manager
                    .get_virtual_actor_definition(&namespace, actor_type)
                    .await
                {
                    let def_ns = def_meta.namespace().to_string();
                    let resolved = manager
                        .resolve_actor_type_for_name(&def_ns, actor_type)
                        .await;
                    let actor_id = ActorId::new(name, &resolved, &def_ns, &self.local_node_id)
                        .map_err(|e| format!("Cannot build actor ID for '{target}': {e}"))?;
                    manager
                        .prime_instance_from_definition(&actor_id, &def_meta)
                        .await;
                    return Ok(actor_id);
                }
            }

            // 2c. Fallback: build canonical directly (actor may not be virtual).
            ActorId::new(name, actor_type, &namespace, &self.local_node_id)
                .map_err(|e| format!("Cannot build actor ID for '{target}': {e}"))
        } else {
            // 3. Bare address — treat as actor_type (or instance name for sole singleton).
            let actor_ids = self.discover_actors_by_type(ctx, target).await;

            // Return an active actor of this type if one exists.
            let active: Vec<&ActorId> = actor_ids
                .iter()
                .filter(|id| id.namespace() == namespace)
                .collect();
            if active.len() == 1 {
                return Ok(active[0].clone());
            }
            if active.len() > 1 {
                // Multiple live actors of this type — pick one at random (load-balance).
                use rand::Rng;
                let idx = rand::thread_rng().gen_range(0..active.len());
                return Ok(active[idx].clone());
            }

            // No live actor found; check VirtualActorManager for a registered type.
            if let Some(manager) = self.virtual_actor_manager.read().await.clone() {
                let resolved_type = manager
                    .resolve_actor_type_for_name(&namespace, target)
                    .await;
                if let Some(type_meta) = manager.get_virtual_actor_type(&resolved_type).await {
                    let canonical_ns = type_meta.namespace().to_string();
                    // Use the resolved type as both the name and type for singleton-style actors.
                    let actor_id = ActorId::new(
                        &resolved_type,
                        &resolved_type,
                        &canonical_ns,
                        &self.local_node_id,
                    )
                    .map_err(|e| format!("Cannot build actor ID for '{target}': {e}"))?;
                    manager
                        .prime_instance_from_definition(&actor_id, &type_meta)
                        .await;
                    return Ok(actor_id);
                }
            }

            Err(format!(
                "No actor found for target '{}' in namespace '{}'",
                target, namespace
            ))
        }
    }
}

// Implement Service trait for ActorRegistry
impl Service for ActorRegistry {
    fn service_name(&self) -> String {
        crate::ServiceName::ServiceNameActorRegistry
            .as_str()
            .to_string()
    }
}

// ============================================================================
// LinkProvider Implementation (Phase 8.5: Link Semantics Integration)
// ============================================================================

/// LinkProvider implementation for ActorRegistry
///
/// ## Purpose
/// Primary link/unlink surface for supervisors and other components that hold an
/// `Arc<dyn LinkProvider>`.
///
/// ## Design
/// Delegates to location-transparent [`ActorRegistry::link`] / [`ActorRegistry::unlink`].
#[async_trait::async_trait]
impl crate::LinkProvider for ActorRegistry {
    async fn link(
        &self,
        ctx: &crate::RequestContext,
        actor_id: &ActorId,
        linked_actor_id: &ActorId,
    ) -> Result<(), String> {
        self.link(ctx, actor_id, linked_actor_id)
            .await
            .map_err(|e| format!("Link failed: {}", e))
    }

    async fn unlink(
        &self,
        ctx: &crate::RequestContext,
        actor_id: &ActorId,
        linked_actor_id: &ActorId,
    ) -> Result<(), String> {
        self.unlink(ctx, actor_id, linked_actor_id)
            .await
            .map_err(|e| format!("Unlink failed: {}", e))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ActorStateChecker impl for ActorRegistry
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
impl plexspaces_service_traits::ActorStateChecker for ActorRegistry {
    async fn is_actor_state_active(&self, actor_id: &ActorId) -> bool {
        ActorRegistry::is_actor_state_active(self, actor_id).await
    }
}
