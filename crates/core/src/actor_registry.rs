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

//! Actor registry for local actor lifecycle, lookup, and message delivery.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

use crate::actor_context::ObjectRegistry;
use crate::ActorFactory;
use crate::Service;
use crate::{
    ActorId, ExitReason, MessageSender, ReplyWaiter, ReplyWaiterRegistry, RequestContext,
    VirtualActorManager, TEMP_SENDER_ACTOR_TYPE,
};
use plexspaces_facet::{ExitReason as FacetExitReason, FacetManager};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::ActorLifecycleEvent;
use ulid::Ulid;

// Observability
use metrics;
use tracing;

/// Create an EXIT message for linked actor notification
/// This is the message sent when an actor terminates to notify linked actors
fn create_exit_message(from: String, reason_str: &str) -> Message {
    let mut headers = std::collections::HashMap::new();
    headers.insert("type".to_string(), "__EXIT__".to_string());
    headers.insert("exit_from".to_string(), from.clone());
    headers.insert("exit_reason".to_string(), reason_str.to_string());

    Message {
        id: ulid::Ulid::new().to_string(),
        sender_id: from,
        message_type: "__EXIT__".to_string(),
        payload: b"__EXIT__".to_vec(),
        headers,
        ..Default::default()
    }
}

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
    /// ObjectRegistry retained for local registry integration points.
    object_registry: Arc<dyn ObjectRegistry>,
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
    /// Monitoring links: actor_id -> Vec<MonitorLink>
    /// Supports multiple supervisors monitoring the same actor (Erlang-style)
    monitors: Arc<RwLock<HashMap<ActorId, Vec<MonitorLink>>>>,
    /// Actor links: actor_id -> Vec<ActorId> (bidirectional death propagation)
    /// Supports multiple links per actor (Erlang-style)
    /// Links are bidirectional: if A links to B, B is linked to A
    links: Arc<RwLock<HashMap<ActorId, Vec<ActorId>>>>,
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
    /// Temporary sender mappings: temporary_sender_id -> TemporarySenderEntry
    /// Used for ask() pattern when called from outside actor context
    /// Key: structured temporary sender ActorId
    /// Value: ActorRef ID that created it, correlation_id, and expiration time
    temporary_senders: Arc<RwLock<HashMap<ActorId, TemporarySenderEntry>>>,
    /// Efficient actor-type lookup: (tenant_id, namespace, actor_type) -> Vec<actor_id>
    /// Used for FaaS-style actor request routing to quickly find actors by type
    /// Maintained in sync with actors map for O(1) lookup
    /// Key: (tenant_id, namespace, actor_type), Value: List of actor IDs of that type
    actor_type_index: Arc<RwLock<HashMap<(String, String, String), Vec<ActorId>>>>,

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

/// Monitor link for actor supervision (Erlang-style)
#[derive(Clone, Debug)]
pub struct MonitorLink {
    /// Monitor reference (unique ID for this monitor)
    pub monitor_ref: String,
    /// Sender for termination notifications
    pub termination_sender: mpsc::Sender<(ActorId, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ScopedActorKey {
    tenant_id: String,
    namespace: String,
    actor_id: ActorId,
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
    /// * `object_registry` - Object registry service
    /// * `local_node_id` - ID of the local node
    ///
    /// ## Returns
    /// New ActorRegistry instance
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_core::ActorRegistry;
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let object_registry = Arc::new(plexspaces_object_registry::ObjectRegistry::new(/* ... */));
    /// let registry = ActorRegistry::new(object_registry, "node1".to_string());
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(object_registry: Arc<dyn ObjectRegistry>, local_node_id: String) -> Self {
        ActorRegistry {
            object_registry,
            actors: Arc::new(RwLock::new(HashMap::new())),
            local_node_id,
            facet_manager: Arc::new(plexspaces_facet::FacetManager::new()),
            monitors: Arc::new(RwLock::new(HashMap::new())),
            links: Arc::new(RwLock::new(HashMap::new())),
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
        }
    }

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

    // === Accessor methods for actor-related data ===

    /// Check if a live local runtime handle exists for an actor.
    ///
    /// ## Purpose
    /// Internal method to check whether a registered sender carries a local runtime/state handle.
    /// Used by runtime code that needs to distinguish a live local actor from a passivated
    /// virtual actor that is only represented by metadata.
    ///
    /// ## Note
    /// This is kept private to maintain encapsulation. External code should use lookup_actor() to get MessageSender.
    pub(crate) fn has_actor_instance(&self, actor_id: &ActorId) -> bool {
        if let Ok(actors) = self.actors.try_read() {
            actors.iter().any(|(key, sender)| {
                key.actor_id == *actor_id && sender.local_state_handle().is_some()
            })
        } else {
            false
        }
    }

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
                return Some((metadata.tenant_id, metadata.namespace));
            }
            if let Some(metadata) = manager.get_virtual_actor_type(actor_id.actor_type()).await {
                return Some((metadata.tenant_id, metadata.namespace));
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
                Some(metadata.actor_type)
            } else {
                manager
                    .get_virtual_actor_type(actor_id.actor_type())
                    .await
                    .map(|metadata| metadata.actor_type)
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

    /// Get monitors map
    pub fn monitors(&self) -> &Arc<RwLock<HashMap<ActorId, Vec<MonitorLink>>>> {
        &self.monitors
    }

    /// Get links map
    pub fn links(&self) -> &Arc<RwLock<HashMap<ActorId, Vec<ActorId>>>> {
        &self.links
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

    /// Get actor type index (for efficient type-based lookups)
    pub fn actor_type_index(
        &self,
    ) -> &Arc<RwLock<HashMap<(String, String, String), Vec<ActorId>>>> {
        &self.actor_type_index
    }

    /// Register an actor (consolidated method for all actor types)
    ///
    /// ## Purpose
    /// Unified registration method for all actors.
    ///
    /// ## Arguments
    /// * `ctx` - Request context for tenant/namespace isolation
    /// * `actor_id` - Actor ID
    /// * `sender` - MessageSender for a running actor
    /// * `actor_type` - Optional actor type for dashboard visibility
    /// * `config` - Optional actor configuration (resource requirements, etc.)
    /// * `instance` - Optional local runtime/state handle for running local actors
    /// * `behavior_kind` - Optional OTP-style behavior kind for logging (GenServer, GenEvent, etc.)
    ///
    /// ## Design
    /// - Running actors are registered with their live `MessageSender`
    /// - Local runtime state hangs off that sender through `ActorStateHandle`
    /// - Config remains separate for resource tracking
    pub async fn register_actor(
        &self,
        ctx: &RequestContext,
        actor_id: ActorId,
        sender: Arc<dyn MessageSender>,
        actor_type: String,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        instance: Option<Arc<dyn crate::actor_state_checker::ActorStateHandle>>,
        behavior_kind: Option<crate::BehaviorType>,
    ) {
        sender.set_actor_type(Some(actor_type.clone())).await;
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
            metrics::counter!("plexspaces_actor_spawn_total", "namespace" => ns.clone()).increment(1);
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

    fn is_local_actor_id(&self, actor_id: &ActorId) -> bool {
        actor_id.node_id() == self.local_node_id
    }

    async fn actor_exists_locally(&self, actor_id: &ActorId) -> bool {
        if self
            .registered_actor_entries
            .read()
            .await
            .iter()
            .any(|key| key.actor_id == *actor_id)
        {
            return true;
        }
        if self.lookup_actor(actor_id).await.is_some() {
            return true;
        }
        let manager = self.virtual_actor_manager.read().await.clone();
        if let Some(manager) = manager {
            return manager.is_virtual(actor_id).await;
        }
        false
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
        actor_id: &ActorId,
        message: Message,
    ) -> Result<(), ActorRegistryError> {
        if let Some(sender) = self.lookup_actor(actor_id).await {
            return sender
                .tell(message)
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
                .tell(message)
                .await
                .map_err(|e| ActorRegistryError::SendFailed(e.to_string()));
        }

        let mut should_activate = true;
        if let Ok(facet_arc) = manager.get_facet(actor_id).await {
            let facet_guard = facet_arc.read().await;
            should_activate = facet_guard.start_activation().await;
        }

        manager.queue_message(actor_id, message).await;

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

    /// Sends a local message, activating a virtual actor transparently when needed.
    pub async fn tell(
        &self,
        actor_id: &ActorId,
        message: Message,
    ) -> Result<(), ActorRegistryError> {
        let start = std::time::Instant::now();
        let result = self.dispatch_local_message(actor_id, message).await;
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

        let dispatch_result = self.dispatch_local_message(actor_id, message).await;
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

    /// Register actor with config
    ///
    /// ## Purpose
    /// Registers an actor with optional configuration for resource tracking.
    /// This is called after an actor is spawned to track it in the registry.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `config` - Optional actor configuration

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

    /// Notify monitors that an actor has terminated
    ///
    /// ## Purpose
    /// Notifies all supervisors monitoring this actor that it has terminated.
    /// This is part of the Erlang-style supervision system.
    ///
    /// ## Arguments
    /// * `actor_id` - The actor that terminated
    /// * `reason` - Reason for termination (e.g., "normal", "panic: ...", "killed")

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
            temporary_sender_id.clone(),
            temporary_sender_ref,
            TEMP_SENDER_ACTOR_TYPE.to_string(),
            None,
            None,
            None,
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
        if temp_senders.remove(temporary_sender_id).is_some() {
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    "ActorRegistry: Removed temporary sender: temporary_sender_id={}, remaining={}",
                    temporary_sender_id,
                    temp_senders.len()
                );
            }
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
            #[cfg(feature = "metrics")]
            {
                metrics::counter!("plexspaces_actor_registry_temporary_sender_expired_total",
                    "node_id" => self.local_node_id.clone()
                )
                .increment(expired_count as u64);
                metrics::gauge!("plexspaces_actor_registry_temporary_sender_mappings",
                    "node_id" => self.local_node_id.clone()
                )
                .set(after_count as f64);
            }

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
    pub async fn link(
        &self,
        actor1_id: &ActorId,
        actor2_id: &ActorId,
    ) -> Result<(), ActorRegistryError> {
        if actor1_id == actor2_id {
            return Err(ActorRegistryError::RegistrationFailed(
                "Cannot link actor to itself".to_string(),
            ));
        }

        let mut links = self.links.write().await;

        // Add actor2 to actor1's links (if not already present)
        links
            .entry(actor1_id.clone())
            .or_insert_with(Vec::new)
            .push(actor2_id.clone());

        // Add actor1 to actor2's links (bidirectional)
        links
            .entry(actor2_id.clone())
            .or_insert_with(Vec::new)
            .push(actor1_id.clone());

        // OBSERVABILITY: Log link creation
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor1 = %actor1_id,
                actor2 = %actor2_id,
                "Linked actors (bidirectional death propagation)"
            );
        }

        Ok(())
    }

    /// Unlink two actors
    ///
    /// ## Purpose
    /// Removes the bidirectional link between two actors.
    ///
    /// ## Erlang Equivalent
    /// Maps to Erlang's `unlink/1` function.
    ///
    /// ## Arguments
    /// * `actor1_id` - First actor ID
    /// * `actor2_id` - Second actor ID
    ///
    /// ## Returns
    /// `Ok(())` on success, `Err(ActorRegistryError)` on failure
    pub async fn unlink(
        &self,
        actor1_id: &ActorId,
        actor2_id: &ActorId,
    ) -> Result<(), ActorRegistryError> {
        let mut links = self.links.write().await;

        // Remove actor2 from actor1's links
        if let Some(actor1_links) = links.get_mut(actor1_id) {
            actor1_links.retain(|id| id != actor2_id);
            if actor1_links.is_empty() {
                links.remove(actor1_id);
            }
        }

        // Remove actor1 from actor2's links (bidirectional)
        if let Some(actor2_links) = links.get_mut(actor2_id) {
            actor2_links.retain(|id| id != actor1_id);
            if actor2_links.is_empty() {
                links.remove(actor2_id);
            }
        }

        // OBSERVABILITY: Log link removal
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor1 = %actor1_id,
                actor2 = %actor2_id,
                "Unlinked actors"
            );
        }

        Ok(())
    }

    /// Get all linked actors for an actor
    ///
    /// ## Purpose
    /// Returns all actors linked to the given actor.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    ///
    /// ## Returns
    /// Vector of linked actor IDs
    pub async fn get_links(&self, actor_id: &ActorId) -> Vec<ActorId> {
        let links = self.links.read().await;
        links.get(actor_id).cloned().unwrap_or_default()
    }

    /// Monitor an actor (one-way notification)
    ///
    /// ## Purpose
    /// Sets up monitoring so the monitor receives DOWN messages when the target actor terminates.
    /// Unlike links, monitors are one-way and don't cause cascading failures.
    ///
    /// ## Erlang Equivalent
    /// Maps to Erlang's `monitor/2` function.
    ///
    /// ## Arguments
    /// * `target_id` - Actor to monitor
    /// * `monitor_id` - Actor doing the monitoring
    /// * `monitor_ref` - Unique reference for this monitor
    /// * `termination_sender` - Channel to send DOWN messages to
    ///
    /// ## Behavior
    /// - Monitor receives DOWN message when target terminates
    /// - Monitor does NOT die when target dies (one-way)
    /// - Used for observability, health checks, supervision
    ///
    /// ## Returns
    /// `Ok(())` on success, `Err(ActorRegistryError)` on failure
    pub async fn monitor(
        &self,
        target_id: &ActorId,
        monitor_id: &ActorId,
        monitor_ref: String,
        termination_sender: mpsc::Sender<(ActorId, String)>,
    ) -> Result<(), ActorRegistryError> {
        let mut monitors = self.monitors.write().await;

        let monitor_link = MonitorLink {
            monitor_ref: monitor_ref.clone(),
            termination_sender,
        };

        monitors
            .entry(target_id.clone())
            .or_insert_with(Vec::new)
            .push(monitor_link);

        // OBSERVABILITY: Log monitor creation
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                target = %target_id,
                monitor = %monitor_id,
                monitor_ref = %monitor_ref,
                "Registered monitor (one-way notification)"
            );
        }

        Ok(())
    }

    /// Remove monitor (demonitor)
    ///
    /// ## Purpose
    /// Removes monitoring so the monitor no longer receives DOWN messages.
    ///
    /// ## Erlang Equivalent
    /// Maps to Erlang's `demonitor/1` function.
    ///
    /// ## Arguments
    /// * `target_id` - Actor being monitored
    /// * `monitor_id` - Actor doing the monitoring
    /// * `monitor_ref` - Monitor reference to remove
    ///
    /// ## Returns
    /// `Ok(())` on success, `Err(ActorRegistryError)` on failure
    pub async fn demonitor(
        &self,
        target_id: &ActorId,
        monitor_id: &ActorId,
        monitor_ref: &str,
    ) -> Result<(), ActorRegistryError> {
        let mut monitors = self.monitors.write().await;

        if let Some(links) = monitors.get_mut(target_id) {
            links.retain(|link| link.monitor_ref != monitor_ref);
            if links.is_empty() {
                monitors.remove(target_id);
            }
        }

        // OBSERVABILITY: Log demonitor
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                target = %target_id,
                monitor = %monitor_id,
                monitor_ref = %monitor_ref,
                "Removed monitor"
            );
        }

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

        tracing::info!(
            actor_id = %actor_id,
            reason = ?reason,
            propagated_exit_to_links = is_error,
            "Actor termination handled; link and monitor state cleaned up"
        );
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
        let monitors = {
            let map = self.monitors.read().await;
            map.get(actor_id).cloned().unwrap_or_default()
        };

        // Convert ExitReason to string for DOWN message
        let reason_str = match reason {
            ExitReason::Normal => "normal".to_string(),
            ExitReason::Shutdown => "shutdown".to_string(),
            ExitReason::Killed => "killed".to_string(),
            ExitReason::Error(msg) => msg.clone(),
            ExitReason::Linked {
                actor_id: linked_id,
                reason: linked_reason,
            } => {
                format!(
                    "linked:{}:{}",
                    linked_id,
                    match linked_reason.as_ref() {
                        ExitReason::Normal => "normal",
                        ExitReason::Shutdown => "shutdown",
                        ExitReason::Killed => "killed",
                        ExitReason::Error(msg) => msg,
                        ExitReason::Linked { .. } => "linked",
                    }
                )
            }
        };

        // Clone monitors before iterating to avoid move
        let monitors_clone = monitors.clone();
        for monitor_link in monitors_clone {
            // Phase 4: Monitoring/Linking Integration - Send DOWN notification
            // The monitoring actor will receive this via termination_sender channel
            // and should call facet.on_down() for all facets when processing the DOWN notification
            let _ = monitor_link
                .termination_sender
                .send((actor_id.clone(), reason_str.clone()))
                .await;

            // Phase 4: Monitoring/Linking Integration - Call facet.on_down() for ALL facets on monitoring actor
            // Note: monitor_ref is a String identifier, not necessarily the actor ID
            // We use FacetManager to call facet.on_down() for all facets on the monitoring actor
            // This avoids circular dependencies (plexspaces-core doesn't depend on plexspaces-facet)
            // Use FacetManager to call facet.on_down() for all facets on the monitoring actor
            // Convert core::ExitReason to facet::ExitReason
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
            let monitoring_actor_id = monitor_link.monitor_ref.clone();
            let facet_down_result: Result<Vec<plexspaces_facet::FacetError>, String> = self
                .facet_manager
                .call_on_down(
                    monitoring_actor_id.clone(),
                    actor_id.to_string(),
                    &facet_exit_reason,
                )
                .await;

            let facet_down_duration = facet_down_start.elapsed();
            match facet_down_result {
                Ok(errors) if !errors.is_empty() => {
                    metrics::counter!("plexspaces_facet_down_errors_total",
                        "monitoring_actor_id" => monitor_link.monitor_ref.clone(),
                        "monitored_actor_id" => actor_id.to_string(),
                        "error_count" => errors.len().to_string()
                    )
                    .increment(errors.len() as u64);
                    tracing::warn!(
                        monitoring_actor_id = %monitor_link.monitor_ref,
                        monitored_actor_id = %actor_id,
                        error_count = errors.len(),
                        "Some facets failed to handle DOWN notification (continuing)"
                    );
                }
                Ok(_) => {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            monitoring_actor_id = %monitor_link.monitor_ref,
                            monitored_actor_id = %actor_id,
                            duration_ms = facet_down_duration.as_millis(),
                            "All facets handled DOWN notification successfully"
                        );
                    }
                }
                Err(e) => {
                    // FacetManager couldn't find facets for this actor (expected for regular actors)
                    // Facets will be called when the actor processes the DOWN notification via termination_sender channel
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            monitoring_actor_id = %monitor_link.monitor_ref,
                            monitored_actor_id = %actor_id,
                            error = %e,
                            "Facets not found in FacetManager (regular actor) - facets will be called when actor processes DOWN notification"
                        );
                    }
                }
            }

            metrics::histogram!("plexspaces_facet_down_duration_seconds",
                "monitoring_actor_id" => monitor_link.monitor_ref.clone(),
                "monitored_actor_id" => actor_id.to_string()
            )
            .record(facet_down_duration.as_secs_f64());
            metrics::counter!("plexspaces_facet_down_total",
                "monitoring_actor_id" => monitor_link.monitor_ref.clone(),
                "monitored_actor_id" => actor_id.to_string()
            )
            .increment(1);

            // OBSERVABILITY: Log DOWN message
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    from = %actor_id,
                    monitor_ref = %monitor_link.monitor_ref,
                    reason = %reason_str,
                    "Sent DOWN notification to monitor"
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
        // Get all linked actors
        let linked_actors = {
            let links = self.links.read().await;
            links.get(actor_id).cloned().unwrap_or_default()
        };

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

        // Clone linked_actors before iterating to avoid move
        let linked_actors_clone = linked_actors.clone();
        // Propagate to each linked actor
        for linked_id in linked_actors_clone {
            // Check if linked actor exists and get its MessageSender
            if let Some(actor_sender) = self.lookup_actor(&linked_id).await {
                // Convert ExitReason to string for EXIT message
                let reason_str = match reason {
                    ExitReason::Normal => "normal".to_string(),
                    ExitReason::Shutdown => "shutdown".to_string(),
                    ExitReason::Killed => "killed".to_string(),
                    ExitReason::Error(msg) => msg.clone(),
                    ExitReason::Linked {
                        actor_id: linked_actor_id,
                        reason: linked_reason,
                    } => {
                        format!(
                            "linked:{}:{}",
                            linked_actor_id,
                            match linked_reason.as_ref() {
                                ExitReason::Normal => "normal",
                                ExitReason::Shutdown => "shutdown",
                                ExitReason::Killed => "killed",
                                ExitReason::Error(msg) => msg,
                                ExitReason::Linked { .. } => "linked",
                            }
                        )
                    }
                };

                // Create EXIT message
                let exit_message = create_exit_message(actor_id.to_string(), &reason_str);

                // Send EXIT signal to linked actor's mailbox
                // The actor's message loop will handle it based on trap_exit setting
                // Note: tell() takes only the message, no RequestContext
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        from = %actor_id,
                        to = %linked_id,
                        reason = ?reason,
                        reason_str = %reason_str,
                        "Attempting to send EXIT to linked actor"
                    );
                }
                if let Err(e) = actor_sender.tell(exit_message).await {
                    tracing::warn!(
                        from = %actor_id,
                        to = %linked_id,
                        error = %e,
                        reason = ?reason,
                        "Failed to send EXIT to linked actor"
                    );
                } else {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            from = %actor_id,
                            to = %linked_id,
                            reason = ?reason,
                            reason_str = %reason_str,
                            "Successfully sent EXIT to linked actor"
                        );
                    }
                }
            } else {
                // Linked actor doesn't exist (already terminated)
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        from = %actor_id,
                        to = %linked_id,
                        "Linked actor not found (already terminated)"
                    );
                }
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
        // Remove from monitors (target is gone, no need to keep monitor entries)
        {
            let mut monitors = self.monitors.write().await;
            monitors.remove(actor_id);
        }

        // Remove from links (remove actor from all other actors' link lists)
        {
            let mut links = self.links.write().await;

            // Remove actor from all other actors' link lists
            for (other_actor_id, other_links) in links.iter_mut() {
                if other_actor_id != actor_id {
                    other_links.retain(|id| id != actor_id);
                }
            }

            // Remove actor's own link entry
            links.remove(actor_id);
        }

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                actor_id = %actor_id,
                "Cleaned up link/monitor entries for terminated actor"
            );
        }
    }
}

// Implement Service trait for ActorRegistry
impl Service for ActorRegistry {
    fn service_name(&self) -> String {
        crate::service_names::ACTOR_REGISTRY.to_string()
    }
}

// ============================================================================
// LinkProvider Implementation (Phase 8.5: Link Semantics Integration)
// ============================================================================

/// LinkProvider implementation for ActorRegistry
///
/// ## Purpose
/// Provides link/unlink functionality for local actors. This is the primary
/// implementation used by supervisors and other components that need to link actors.
///
/// ## Design
/// - Supports local actors only (actors registered in this ActorRegistry)
/// - Remote actor linking is handled by Node (see TODO below)
/// - Follows Erlang/OTP link semantics (bidirectional death propagation)
///
/// ## TODO: Remote Actor Linking
/// Node currently supports remote actor linking via gRPC, but this is advanced
/// functionality. For now, LinkProvider in ActorRegistry only supports local actors.
/// Remote linking is intentionally unsupported here; node-level code should own
/// any future remote-link protocol.
#[async_trait::async_trait]
impl crate::LinkProvider for ActorRegistry {
    async fn link(
        &self,
        actor_id: &ActorId,
        linked_actor_id: &ActorId,
        _ctx: &crate::RequestContext,
    ) -> Result<(), String> {
        if !self.is_local_actor_id(actor_id) || !self.is_local_actor_id(linked_actor_id) {
            return Err(
                "Remote link is not supported by ActorRegistry; use a local actor pair".to_string(),
            );
        }
        if !self.actor_exists_locally(actor_id).await {
            return Err(format!("Actor {} is not local or not found", actor_id));
        }
        if !self.actor_exists_locally(linked_actor_id).await {
            return Err(format!(
                "Actor {} is not local or not found",
                linked_actor_id
            ));
        }
        self.link(actor_id, linked_actor_id)
            .await
            .map_err(|e| format!("Link failed: {}", e))
    }

    async fn unlink(
        &self,
        actor_id: &ActorId,
        linked_actor_id: &ActorId,
        _ctx: &crate::RequestContext,
    ) -> Result<(), String> {
        if !self.is_local_actor_id(actor_id) || !self.is_local_actor_id(linked_actor_id) {
            return Err(
                "Remote unlink is not supported by ActorRegistry; use a local actor pair"
                    .to_string(),
            );
        }
        self.unlink(actor_id, linked_actor_id)
            .await
            .map_err(|e| format!("Unlink failed: {}", e))
    }
}
