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

//! Actor registry for looking up actor mailboxes and routing info
//!
//! Composes over ObjectRegistry to reuse existing infrastructure.
//! Provides fast local cache for performance while using ObjectRegistry as source of truth.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

use crate::{ActorId, ActorRef, MessageSender, RequestContext, ActorMetricsHandle, ActorMetricsExt, ExitReason, actor_state_checker};
use crate::actor_context::ObjectRegistry;
use crate::Service;
use crate::ActorFactory;
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::object_registry::v1::ObjectType;
use plexspaces_proto::ActorLifecycleEvent;
use plexspaces_facet::{FacetManager, ExitReason as FacetExitReason};

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

/// Cached node lookup entry with expiration
#[derive(Clone, Debug)]
struct NodeCacheEntry {
    node_address: String,
    expires_at: Instant,
}

impl NodeCacheEntry {
    fn new(node_address: String, ttl: Duration) -> Self {
        Self {
            node_address,
            expires_at: Instant::now() + ttl,
        }
    }

    fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
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
}

/// Routing information for an actor
#[derive(Clone, Debug)]
pub struct ActorRoutingInfo {
    /// Node ID where actor is located
    pub node_id: String,
    /// Network address of the node (for remote actors)
    pub node_address: Option<String>,
    /// Whether the actor is on the local node
    pub is_local: bool,
}

/// Actor registry for looking up actor mailboxes and routing info
///
/// Composes over ObjectRegistry to reuse existing infrastructure.
/// Maintains a local cache for fast lookups while using ObjectRegistry as source of truth.
///
/// ## Actor Data Storage
/// ActorRegistry is the single source of truth for all actor-related data:
/// - Actor instances (for lazy virtual actors)
/// - Facets (for facet access)
/// - Virtual actor metadata
/// - Monitoring links
/// - Actor links
/// - Actor configurations
/// - Lifecycle event subscribers
pub struct ActorRegistry {
    /// ObjectRegistry for storing actor metadata and node info
    object_registry: Arc<dyn ObjectRegistry>,
    /// Local actors cache: actor_id -> MessageSender trait object (for simplified virtual actor activation)
    /// Stores MessageSender trait objects - regular actors and VirtualActorWrapper for virtual actors
    /// This enables automatic activation on tell() calls (Orleans-inspired)
    /// Mailbox is internal - only MessageSender is exposed via this registry
    actors: Arc<RwLock<HashMap<ActorId, Arc<dyn MessageSender>>>>,
    /// Node lookup cache: node_id -> (node_address, expires_at)
    /// TTL: 30-60 seconds to avoid frequent DB lookups
    node_cache: Arc<RwLock<HashMap<String, NodeCacheEntry>>>,
    /// TTL for node cache entries (default: 60 seconds)
    node_cache_ttl: Duration,
    /// Current node ID
    local_node_id: String,
    
    /// Actor instances (for all actors - virtual and regular)
    /// Stores the Actor instance after it's started (for both virtual and regular actors)
    /// Used by ask() to check if an actor is activated and to get the mailbox directly
    /// For lazy virtual actors: stored before activation (for activation), then updated after activation
    /// For eager virtual actors and regular actors: stored after start() completes
    actor_instances: Arc<RwLock<HashMap<ActorId, Arc<dyn std::any::Any + Send + Sync>>>>,
    /// FacetManager for facet storage and management
    facet_manager: Arc<FacetManager>,
    /// Optional ActorFactory for activating virtual actors (set during initialization)
    /// Used by ActivationProvider.activate_actor to activate deactivated virtual actors
    actor_factory: Arc<RwLock<Option<Arc<dyn ActorFactory>>>>,
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
    /// Track registered actor IDs (for duplicate detection, even without config)
    registered_actor_ids: Arc<RwLock<HashSet<ActorId>>>,
    /// Actor metrics (extracted from NodeMetrics for better separation of concerns)
    actor_metrics: ActorMetricsHandle,
    /// Temporary sender mappings: temporary_sender_id -> TemporarySenderEntry
    /// Used for ask() pattern when called from outside actor context
    /// Key: temporary_sender_id (format: "ask-{correlation_id}@{node_id}")
    /// Value: ActorRef ID that created it, correlation_id, and expiration time
    temporary_senders: Arc<RwLock<HashMap<String, TemporarySenderEntry>>>,
    /// Efficient actor-type lookup: (tenant_id, namespace, actor_type) -> Vec<actor_id>
    /// Used for FaaS-style actor invocation to quickly find actors by type
    /// Maintained in sync with actors map for O(1) lookup
    /// Key: (tenant_id, namespace, actor_type), Value: List of actor IDs of that type
    actor_type_index: Arc<RwLock<HashMap<(String, String, String), Vec<ActorId>>>>,
    
    /// Actor metadata: actor_id -> (tenant_id, namespace)
    /// Stores tenant/namespace for each actor for proper isolation during operations like stop()
    /// This avoids needing to access actor instance just to get context
    actor_metadata: Arc<RwLock<HashMap<ActorId, (String, String)>>>, // (tenant_id, namespace)
    /// Actor types: actor_id -> actor_type
    /// Stores actor_type for each actor to enable rebuilding suspended actors
    /// Used when reactivating suspended virtual actors that need to be rebuilt
    actor_types: Arc<RwLock<HashMap<ActorId, String>>>,
    
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
/// - Temporary sender ID is its own actor_ref_id (format: "ask-{correlation_id}@{node_id}")
/// - Used for correlation_id lookup and expiration tracking
#[derive(Clone, Debug)]
pub struct TemporarySenderEntry {
    /// Temporary sender ID (same as actor_ref_id, format: "ask-{correlation_id}@{node_id}")
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

impl ActorRegistry {
    /// Create a new ActorRegistry with default TTL (60 seconds)
    ///
    /// ## Arguments
    /// * `object_registry` - Object registry for service discovery
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
    pub fn new(
        object_registry: Arc<dyn ObjectRegistry>,
        local_node_id: String,
    ) -> Self {
        Self::new_with_ttl(object_registry, local_node_id, Duration::from_secs(60))
    }

    /// Create a new ActorRegistry with custom TTL
    ///
    /// ## Arguments
    /// * `object_registry` - Object registry for service discovery
    /// * `local_node_id` - ID of the local node
    /// * `node_cache_ttl` - TTL for node cache entries
    ///
    /// ## Returns
    /// New ActorRegistry instance
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_core::ActorRegistry;
    /// # use std::sync::Arc;
    /// # use std::time::Duration;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let object_registry = Arc::new(plexspaces_object_registry::ObjectRegistry::new(/* ... */));
    /// let registry = ActorRegistry::new_with_ttl(
    ///     object_registry,
    ///     "node1".to_string(),
    ///     Duration::from_secs(120),
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn new_with_ttl(
        object_registry: Arc<dyn ObjectRegistry>,
        local_node_id: String,
        node_cache_ttl: Duration,
    ) -> Self {
        ActorRegistry {
            object_registry,
            actors: Arc::new(RwLock::new(HashMap::new())), // MessageSender trait objects (Orleans-inspired)
            node_cache: Arc::new(RwLock::new(HashMap::new())),
            node_cache_ttl,
            local_node_id,
            // Initialize actor-related data structures
            actor_instances: Arc::new(RwLock::new(HashMap::new())),
            facet_manager: Arc::new(plexspaces_facet::FacetManager::new()),
            monitors: Arc::new(RwLock::new(HashMap::new())),
            links: Arc::new(RwLock::new(HashMap::new())),
            lifecycle_subscribers: Arc::new(RwLock::new(Vec::new())),
            actor_configs: Arc::new(RwLock::new(HashMap::new())),
            registered_actor_ids: Arc::new(RwLock::new(HashSet::new())),
            actor_metrics: Arc::new(RwLock::new(ActorMetricsExt::new())),
            temporary_senders: Arc::new(RwLock::new(HashMap::new())),
            actor_type_index: Arc::new(RwLock::new(HashMap::new())),
            actor_metadata: Arc::new(RwLock::new(HashMap::new())), // (tenant_id, namespace)
            actor_types: Arc::new(RwLock::new(HashMap::new())),
            // Initialize parent-child relationship tracking
            parent_to_children: Arc::new(RwLock::new(HashMap::new())),
            child_to_parent: Arc::new(RwLock::new(HashMap::new())),
            // Initialize ActorFactory as None - set via set_actor_factory() during Node initialization
            actor_factory: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Set ActorFactory for virtual actor activation
    ///
    /// ## Purpose
    /// Sets the ActorFactory reference used by ActivationProvider.activate_actor.
    /// Called during Node initialization to enable virtual actor activation.
    ///
    /// ## Arguments
    /// * `actor_factory` - ActorFactory implementation for activating virtual actors
    ///
    /// ## Design
    /// ActorRegistry doesn't have direct access to ServiceLocator to avoid circular dependencies.
    /// Instead, Node sets the ActorFactory reference during initialization.
    pub async fn set_actor_factory(&self, actor_factory: Arc<dyn ActorFactory>) {
        *self.actor_factory.write().await = Some(actor_factory);
    }
    
    // === Accessor methods for actor-related data ===
    
    /// Check if actor instance exists (for ask() to determine if actor is activated)
    /// 
    /// ## Purpose
    /// Internal method to check if an actor instance is stored (indicating the actor is activated).
    /// Used by ask() to determine if it should use the actor instance directly or activate a lazy virtual actor.
    /// 
    /// ## Note
    /// This is kept private to maintain encapsulation. External code should use lookup_actor() to get MessageSender.
    pub(crate) fn has_actor_instance(&self, actor_id: &ActorId) -> bool {
        // This is a synchronous check - we can't use async in a sync method
        // So we use try_read() which returns immediately
        if let Ok(instances) = self.actor_instances.try_read() {
            instances.contains_key(actor_id)
        } else {
            false
        }
    }
    
    /// Get actor instance (for lazy virtual actor activation and test helpers)
    /// 
    /// ## Purpose
    /// Gets the actor instance for:
    /// 1. Lazy virtual actor activation (ActorFactory needs Actor before unregistering)
    /// 2. Test helpers (need mailbox to create ActorRef for testing)
    /// 
    /// ## Design Principles
    /// - **Encapsulation**: `actor_instances` map is private, accessed only via this method
    /// - **Simple**: Single method to get instance, use `unregister_with_cleanup()` to remove
    /// - **Consistent**: All instance management goes through `register_actor()` and `unregister_with_cleanup()`
    /// 
    /// ## Usage Pattern for Lazy Activation
    /// ```rust,ignore
    /// // 1. Get instance
    /// let instance = registry.get_actor_instance(&actor_id).await?;
    /// // 2. Unregister (removes instance from registry)
    /// registry.unregister_with_cleanup(&actor_id).await?;
    /// // 3. Spawn (re-registers with ActorRef)
    /// spawn_actor(instance).await?;
    /// ```
    /// 
    /// ## Note
    /// Production code should use `lookup_actor()` to get MessageSender.
    /// Only ActorFactory (for lazy activation) and test helpers should use this method.
    pub async fn get_actor_instance(&self, actor_id: &ActorId) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        let instances = self.actor_instances.read().await;
        instances.get(actor_id).cloned()
    }
    
    /// Get actor state
    ///
    /// ## Purpose
    /// Gets the actual state of an actor instance.
    /// This is used to determine if an actor is truly active (state is Active).
    ///
    /// ## Returns
    /// `Option<i32>` - The actor's state as a proto `ActorState` enum value, or `None` if not an Actor or instance doesn't exist
    ///
    /// ## Implementation
    /// Uses `ActorStateFetcher` trait to fetch state without importing `Actor` directly.
    /// The trait is implemented by `Actor` in the `plexspaces_actor` crate.
    ///
    /// ## Usage
    /// Called by `is_actor_state_active()` to check if an actor is truly active.
    /// This is consistent for all actor types (regular/virtual/workflows/etc.).
    pub async fn get_actor_state(&self, actor_id: &ActorId) -> Option<i32> {
        if let Some(instance) = self.get_actor_instance(actor_id).await {
            // Use ActorStateFetcher trait to get state without importing Actor directly
            actor_state_checker::get_actor_state(&instance).await
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
            // Check if state is Active (proto enum value)
            let is_active = state_value == ProtoActorState::ActorStateActive as i32;
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!("[ACTOR_REGISTRY] is_actor_state_active: actor_id={}, state_value={}, ActorStateActive={}, is_active={}", 
                    actor_id, state_value, ProtoActorState::ActorStateActive as i32, is_active);
            }
            is_active
        } else {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!("[ACTOR_REGISTRY] is_actor_state_active: actor_id={}, state_value=None, is_active=false", actor_id);
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
        let metadata = self.actor_metadata.read().await;
        metadata.get(actor_id).cloned()
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
        let actor_types = self.actor_types.read().await;
        let result = actor_types.get(actor_id).cloned();
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("[ACTOR_REGISTRY] get_actor_type: actor_id={}, result={:?}, total_types={}", actor_id, result, actor_types.len());
        }
        result
    }
    
    /// Suspend a virtual actor (remove instance and ActorRef, but preserve actor_type, metadata, config)
    /// 
    /// ## Purpose
    /// Suspends a virtual actor by removing its instance and ActorRef, but preserves
    /// actor_type, metadata, and config so the actor can be rebuilt later.
    /// 
    /// ## Design
    /// Unlike `unregister_with_cleanup()`, this method:
    /// - Removes actor instance (so actor is not active, Arc will be dropped and actor will stop)
    /// - Removes ActorRef from actors map (will be replaced by VirtualActorWrapper)
    /// - Preserves actor_type (needed for rebuilding)
    /// - Preserves actor_metadata (needed for context)
    /// - Preserves actor_config (needed for resource tracking)
    /// - Does NOT remove from registered_actor_ids (actor is still registered as virtual)
    /// 
    /// ## Note
    /// This is specifically for virtual actor suspension. After calling this,
    /// the caller should re-register VirtualActorWrapper to keep the actor addressable.
    /// The actor will be stopped when the Arc reference is dropped (when instance is removed).
    pub async fn suspend_virtual_actor(&self, actor_id: &ActorId) {
        // Check actor_type before suspension (for debugging)
        let actor_type_before = {
            let actor_types = self.actor_types.read().await;
            actor_types.get(actor_id).cloned()
        };
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("[ACTOR_REGISTRY] suspend_virtual_actor: actor_id={}, actor_type_before={:?}", actor_id, actor_type_before);
        }
        
        // Remove instance (preserves actor_type, metadata, config for rebuilding)
        // When the Arc is dropped (removed from map), the actor will be stopped
        {
            let mut actor_instances = self.actor_instances.write().await;
            actor_instances.remove(actor_id);
        }
        
        // Remove ActorRef from actors map (will be replaced by VirtualActorWrapper)
        {
            let mut actors = self.actors.write().await;
            actors.remove(actor_id);
        }
        
        // Verify actor_type is still there after suspension
        let actor_type_after = {
            let actor_types = self.actor_types.read().await;
            actor_types.get(actor_id).cloned()
        };
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("[ACTOR_REGISTRY] suspend_virtual_actor: actor_id={}, actor_type_after={:?}", actor_id, actor_type_after);
        }
        
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor_id = %actor_id,
                "Suspended virtual actor (instance and ActorRef removed, actor_type/metadata/config preserved)"
            );
        }
    }
    
    // 
    // Design: Simple and encapsulated
    // - actor_instances map is private (encapsulation)
    // - get_actor_instance() - only way to read instances
    // - register_actor() - only way to store instances (via instance parameter)
    // - unregister_with_cleanup() - only way to remove instances
    // 
    // For lazy activation: get_instance() → unregister_with_cleanup() → spawn (re-registers)
    // This uses the proper registry methods instead of managing instances directly
    
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
    pub fn lifecycle_subscribers(&self) -> &Arc<RwLock<Vec<mpsc::UnboundedSender<ActorLifecycleEvent>>>> {
        &self.lifecycle_subscribers
    }
    
    /// Get actor configs map
    pub fn actor_configs(&self) -> &Arc<RwLock<HashMap<ActorId, plexspaces_proto::v1::actor::ActorConfig>>> {
        &self.actor_configs
    }
    
    pub fn actor_types(&self) -> &Arc<RwLock<HashMap<ActorId, String>>> {
        &self.actor_types
    }
    
    pub fn actor_metadata(&self) -> &Arc<RwLock<HashMap<ActorId, (String, String)>>> {
        &self.actor_metadata
    }
    
    /// Get registered actor IDs set
    pub fn registered_actor_ids(&self) -> &Arc<RwLock<HashSet<ActorId>>> {
        &self.registered_actor_ids
    }
    
    /// Get local node ID
    pub fn local_node_id(&self) -> &str {
        &self.local_node_id
    }
    
    /// Get actor metrics handle
    pub fn actor_metrics(&self) -> &ActorMetricsHandle {
        &self.actor_metrics
    }
    
    /// Get actor type index (for efficient type-based lookups)
    pub fn actor_type_index(&self) -> &Arc<RwLock<HashMap<(String, String, String), Vec<ActorId>>>> {
        &self.actor_type_index
    }
    
    /// Register an actor (consolidated method for all actor types)
    ///
    /// ## Purpose
    /// Unified registration method for all actors (virtual and regular, lazy and eager).
    /// Per Orleans design: virtual actors are always registered (even when not active).
    /// 
    /// ## Arguments
    /// * `ctx` - Request context for tenant/namespace isolation
    /// * `actor_id` - Actor ID
    /// * `sender` - MessageSender (ActorRef for regular/activated actors, VirtualActorWrapper for lazy virtual actors)
    /// * `actor_type` - Optional actor type for dashboard visibility
    /// * `config` - Optional actor configuration (resource requirements, etc.)
    /// * `instance` - Optional actor instance (for activated actors - stored for ask() to get mailbox)
    /// * `behavior_kind` - Optional OTP-style behavior kind for logging (GenServer, GenEvent, etc.)
    ///
    /// ## Design
    /// - Virtual actors: Always registered (VirtualActorWrapper when lazy, ActorRef when activated)
    /// - Regular actors: Always registered with ActorRef after start()
    /// - Actor instance: Stored for activated actors (both virtual and regular) so ask() can get mailbox directly
    /// - Config: Stored separately for resource tracking
    ///
    /// ## Orleans-Inspired Behavior
    /// - Virtual actors always exist (virtually) - registered even when not active
    /// - Activation is transparent - VirtualActorWrapper is replaced by ActorRef when activated
    /// - Location is transparent - registry handles routing
    pub async fn register_actor(
        &self,
        ctx: &RequestContext,
        actor_id: ActorId,
        sender: Arc<dyn MessageSender>,
        actor_type: Option<String>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        instance: Option<Arc<dyn std::any::Any + Send + Sync>>,
        behavior_kind: Option<crate::BehaviorType>,
    ) {
        // Register MessageSender in actors map (always - for both virtual and regular actors)
        // For virtual actors: VirtualActorWrapper when lazy, ActorRef when activated
        // For regular actors: ActorRef after start()
        let mut actors = self.actors.write().await;
        let was_new = actors.insert(actor_id.clone(), sender).is_none();
        drop(actors);
        
        // CRITICAL: Also add to registered_actor_ids to keep them in sync
        // This ensures tests can check registered_actor_ids reliably
        if was_new {
            let mut registered_ids = self.registered_actor_ids.write().await;
            registered_ids.insert(actor_id.clone());
        }
        
        // Store tenant/namespace metadata (for proper isolation during operations like stop())
        // This avoids needing to access actor instance just to get context
        {
            let mut metadata = self.actor_metadata.write().await;
            metadata.insert(actor_id.clone(), (ctx.tenant_id().to_string(), ctx.namespace().to_string()));
        }
        
        // Store actor config if provided (for resource tracking)
        if let Some(config) = config {
            let mut actor_configs = self.actor_configs.write().await;
            actor_configs.insert(actor_id.clone(), config);
        }
        
        // Store actor instance if provided (for activated actors - used by ask() to get mailbox)
        // This is stored for both virtual and regular actors after they're activated
        if let Some(ref instance) = instance {
            let mut actor_instances = self.actor_instances.write().await;
            actor_instances.insert(actor_id.clone(), instance.clone());
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    actor_id = %actor_id,
                    "Stored actor instance (actor is activated and ready for ask())"
                );
            }
        }
        
        // Update actor-type index if type information is provided
        // CRITICAL: Only update actor_type if provided - don't remove existing actor_type if None
        // This preserves actor_type for suspended actors when re-registering with VirtualActorWrapper
        if let Some(actor_type) = actor_type {
            // Store actor_type per actor_id (for rebuilding suspended actors)
            {
                let mut actor_types = self.actor_types.write().await;
                actor_types.insert(actor_id.clone(), actor_type.clone());
            }
            
            let mut index = self.actor_type_index.write().await;
            let key = (ctx.tenant_id().to_string(), ctx.namespace().to_string(), actor_type.clone());
            index.entry(key).or_insert_with(Vec::new).push(actor_id.clone());
            
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
                "Actor registered with type in actor_type_index"
            );
            }
        } else {
            // OBSERVABILITY: Warn if actor_type is missing (but don't remove existing actor_type)
            // This is important for suspended actors - we preserve their actor_type even when
            // re-registering with VirtualActorWrapper (which doesn't have actor_type)
            let has_existing_type = {
                let actor_types = self.actor_types.read().await;
                actor_types.contains_key(&actor_id)
            };
            if !has_existing_type {
                tracing::warn!(
                    actor_id = %actor_id,
                    "Actor registered without actor_type - will not appear in 'Actors by Type' dashboard"
                );
            } else {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                    actor_id = %actor_id,
                    "Actor registered without actor_type, but existing actor_type preserved (suspended actor?)"
                );
                }
            }
        }
        
        // Update metrics if this is a new actor
        if was_new {
            let mut metrics = self.actor_metrics.write().await;
            metrics.increment_spawn_total();
            metrics.increment_active();
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
    /// MessageSender trait object if found, None otherwise
    pub async fn lookup_actor(&self, actor_id: &ActorId) -> Option<Arc<dyn MessageSender>> {
        let actors = self.actors.read().await;
        actors.get(actor_id).cloned()
    }

    /// Look up node address with caching
    /// Uses cache first (TTL: 30-60 seconds), then falls back to ObjectRegistry
    /// 
    /// ## Note
    /// Uses the provided RequestContext for the lookup. Nodes should be registered
    /// with the same tenant/namespace that will be used for lookups.
    pub async fn lookup_node_address(&self, ctx: &RequestContext, node_id: &str) -> Result<Option<String>, ActorRegistryError> {
        // Check cache first
        {
            let cache = self.node_cache.read().await;
            if let Some(entry) = cache.get(node_id) {
                if !entry.is_expired() {
                    return Ok(Some(entry.node_address.clone()));
                }
                // Entry expired, will be removed below
            }
        }

        // Cache miss or expired - lookup in ObjectRegistry
        // Use the provided RequestContext for the lookup
        // Nodes are registered with object_id = node_id using ObjectTypeNode (no "_node@" prefix)
        let registration = self.object_registry
            .lookup_full(ctx, ObjectType::ObjectTypeNode, node_id)
            .await
            .map_err(|e| ActorRegistryError::LookupFailed(e.to_string()))?;

        let node_address = registration.as_ref().map(|r| r.grpc_address.clone());

        // Update cache if found
        if let Some(ref address) = node_address {
            let mut cache = self.node_cache.write().await;
            cache.insert(node_id.to_string(), NodeCacheEntry::new(address.clone(), self.node_cache_ttl));
            
            // Clean up expired entries while we have the lock
            cache.retain(|_, entry| !entry.is_expired());
        }

        Ok(node_address)
    }


    /// Look up actor routing info (for remote actors)
    /// Uses cached node lookups (TTL: 30-60 seconds) to avoid frequent DB queries
    pub async fn lookup_routing(&self, ctx: &RequestContext, actor_id: &ActorId) -> Result<Option<ActorRoutingInfo>, ActorRegistryError> {
        let (_, node_id) = ActorRef::parse_actor_id(actor_id)
            .map_err(|e| ActorRegistryError::LookupFailed(e.to_string()))?;

        if node_id == self.local_node_id {
            // Local actor
            Ok(Some(ActorRoutingInfo {
                node_id: node_id.clone(),
                node_address: None,
                is_local: true,
            }))
        } else {
            // Remote actor - look up node address (with caching)
            let node_address = self.lookup_node_address(ctx, &node_id).await?;

            if let Some(address) = node_address {
                Ok(Some(ActorRoutingInfo {
                    node_id,
                    node_address: Some(address),
                    is_local: false,
                }))
            } else {
                Ok(None)
            }
        }
    }


    /// Unregister actor
    /// Note: Currently only removes from actors map (MessageSender).
    /// ObjectRegistry unregister is not in the trait yet - can be added later if needed.
    pub async fn unregister(&self, actor_id: &ActorId) -> Result<(), ActorRegistryError> {
        // Remove from actors map (MessageSender)
        {
            let mut actors = self.actors.write().await;
            actors.remove(actor_id);
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

        // TODO: Add unregister to ObjectRegistry trait and call it here
        // For now, we only remove from actors map

        Ok(())
    }

    /// Check if actor is activated (has MessageSender) without exposing mailbox
    ///
    /// ## Purpose
    /// Allows checking if an actor is activated without exposing mailbox directly.
    /// Checks if MessageSender is registered, which indicates the actor is active.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    ///
    /// ## Returns
    /// true if actor is activated (has MessageSender), false otherwise
    pub async fn is_actor_activated(&self, actor_id: &ActorId) -> bool {
        let actors = self.actors.read().await;
        actors.contains_key(actor_id)
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
    pub async fn unregister_with_cleanup(&self, actor_id: &ActorId) -> Result<(), ActorRegistryError> {
        // Check if actor existed before removing
        let existed = {
            let actors = self.actors.read().await;
            actors.contains_key(actor_id)
        };
        
        // Remove from actors (MessageSender trait objects)
        {
            let mut actors = self.actors.write().await;
            actors.remove(actor_id);
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
        // Order: 1. actor_instances, 2. actor_metadata, 3. actor_types, 4. facet_manager (via remove_facets), 5. registered_ids, 6. actor_configs
        let mut actor_instances = self.actor_instances.write().await;
        let mut actor_metadata = self.actor_metadata.write().await;
        let mut actor_types = self.actor_types.write().await;
        let mut registered_ids = self.registered_actor_ids.write().await;
        let mut actor_configs = self.actor_configs.write().await;

        actor_instances.remove(actor_id);
        actor_metadata.remove(actor_id);
        actor_types.remove(actor_id);
        
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
        registered_ids.remove(actor_id);
        actor_configs.remove(actor_id);
        
        // Update metrics if actor existed
        if existed {
            let mut metrics = self.actor_metrics.write().await;
            metrics.decrement_active();
        }
        
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
    /// * `temporary_sender_id` - Temporary sender ID (format: "ask-{correlation_id}@{node_id}")
    /// * `temporary_sender_ref` - ActorRef for the temporary sender (implements MessageSender)
    /// * `correlation_id` - Correlation ID for matching replies
    /// * `expires_at` - Expiration time for automatic cleanup
    pub async fn register_temporary_sender(
        &self,
        ctx: &RequestContext,
        temporary_sender_id: String,
        temporary_sender_ref: Arc<dyn MessageSender>,
        correlation_id: String,
        expires_at: Instant,
    ) {
        // Register temporary sender as an actual ActorRef in the actors map.
        // This allows lookup_actor() to find it when send_reply() is called.
        let correlation_id_clone = correlation_id.clone();
        self.register_actor(
            ctx,
            temporary_sender_id.clone(),
            temporary_sender_ref,
            Some("TemporarySender".to_string()), // Actor type for observability
            None, // No config for temporary senders
            None, // No instance for temporary senders (they're just ActorRefs)
            None, // No behavior_kind for temporary senders
        ).await;
        
        // Also store in temporary_senders map for correlation_id lookup and cleanup
        let mut temp_senders = self.temporary_senders.write().await;
        temp_senders.insert(temporary_sender_id.clone(), TemporarySenderEntry {
            actor_ref_id: temporary_sender_id.clone(), // Temporary sender ID is its own actor_ref_id
            correlation_id: correlation_id_clone,
            expires_at,
        });
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
        temporary_sender_id: &str,
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
    pub async fn remove_temporary_sender(&self, temporary_sender_id: &str) {
        // Unregister from the actors map (so lookup_actor() won't find it).
        let actor_id = ActorId::from(temporary_sender_id.to_string());
        if let Err(e) = self.unregister_with_cleanup(&actor_id).await {
            tracing::warn!(
                "ActorRegistry: Failed to unregister temporary sender ActorRef: temporary_sender_id={}, error={}",
                temporary_sender_id, e
            );
        }
        
        // Also remove from temporary_senders map
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
    /// Number of expired temporary senders removed
    pub async fn cleanup_expired_temporary_senders(&self) -> usize {
        let now = Instant::now();
        let expired_ids: Vec<String> = {
            let temp_senders = self.temporary_senders.read().await;
            temp_senders.iter()
                .filter(|(_id, entry)| entry.expires_at <= now)
                .map(|(id, _)| id.clone())
                .collect()
        };
        
        let expired_count = expired_ids.len();
        
        // Unregister each expired temporary sender from the actors map
        for temp_sender_id in &expired_ids {
            let actor_id = ActorId::from(temp_sender_id.clone());
            if let Err(e) = self.unregister_with_cleanup(&actor_id).await {
                tracing::warn!(
                    "ActorRegistry: Failed to unregister expired temporary sender ActorRef: temporary_sender_id={}, error={}",
                    temp_sender_id, e
                );
            }
        }
        
        // Remove from temporary_senders map
        if expired_count > 0 {
            let mut temp_senders = self.temporary_senders.write().await;
            for temp_sender_id in &expired_ids {
                temp_senders.remove(temp_sender_id);
            }
            let after_count = temp_senders.len();
            
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                "ActorRegistry: Cleaned up {} expired temporary senders (before: {}, after: {})",
                expired_count,
                expired_count + after_count,
                after_count
            );
            }
            
            // OBSERVABILITY: Track expired temporary sender cleanup
            #[cfg(feature = "metrics")]
            {
                metrics::counter!("plexspaces_actor_registry_temporary_sender_expired_total",
                    "node_id" => self.local_node_id.clone()
                ).increment(expired_count as u64);
                metrics::gauge!("plexspaces_actor_registry_temporary_sender_mappings",
                    "node_id" => self.local_node_id.clone()
                ).set(after_count as f64);
            }
        }
        
        expired_count
    }
    
    /// Get count of temporary senders (for metrics/monitoring)
    /// Discover actors by type (efficient O(1) lookup using index)
    ///
    /// ## Purpose
    /// Finds actors by actor_type within a tenant using efficient hashmap lookup.
    /// Used for FaaS-like actor invocation where we need to find any actor of a given type.
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
        let key = (ctx.tenant_id().to_string(), ctx.namespace().to_string(), actor_type.to_string());
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
    pub async fn register_parent_child(
        &self,
        parent_id: &ActorId,
        child_id: &ActorId,
    ) {
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
            "parent_id" => parent_id.clone()
        ).set({
            let map = self.parent_to_children.read().await;
            map.get(parent_id).map(|v| v.len() as f64).unwrap_or(0.0)
        });
        
        metrics::counter!("plexspaces_actor_parent_child_registered_total",
            "parent_id" => parent_id.clone(),
            "child_id" => child_id.clone()
        ).increment(1);

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
    pub async fn unregister_parent_child(
        &self,
        parent_id: &ActorId,
        child_id: &ActorId,
    ) {
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
            "parent_id" => parent_id.clone()
        ).set({
            let map = self.parent_to_children.read().await;
            map.get(parent_id).map(|v| v.len() as f64).unwrap_or(0.0)
        });
        
        metrics::counter!("plexspaces_actor_parent_child_unregistered_total",
            "parent_id" => parent_id.clone(),
            "child_id" => child_id.clone()
        ).increment(1);

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
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
        map.get(parent_id)
            .cloned()
            .unwrap_or_default()
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
            "root_id" => root_id.clone()
        ).set(result.len() as f64);

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
        map.get(parent_id)
            .map(|v| v.len())
            .unwrap_or(0)
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
                
                // Cleanup expired temporary senders
                let expired_count = registry.cleanup_expired_temporary_senders().await;
                if expired_count > 0 {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                        "ActorRegistry: Cleaned up {} expired temporary senders (node_id={})",
                        expired_count,
                        local_node_id
                    );
                    }
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
    pub async fn handle_actor_termination(
        &self,
        actor_id: &ActorId,
        reason: ExitReason,
    ) {
        tracing::info!(
            actor_id = %actor_id,
            reason = ?reason,
            "Handling actor termination (Phase 6: Link/Monitor semantics)"
        );

        // 1. Send DOWN to all monitors (always, regardless of exit reason)
        self.send_down_to_monitors(actor_id, &reason).await;

        // 2. Propagate EXIT to linked actors (only for error exits)
        // Normal and Shutdown exits don't propagate to links (Erlang semantics)
        let is_error = reason.is_error();
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor_id = %actor_id,
                reason = ?reason,
                is_error = is_error,
                "Checking if exit reason should propagate to links"
            );
        }
        if is_error {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    actor_id = %actor_id,
                    reason = ?reason,
                    "Exit reason is error, propagating to linked actors"
                );
            }
            self.propagate_exit_to_links(actor_id, &reason).await;
        } else {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    actor_id = %actor_id,
                    reason = ?reason,
                    "Exit reason is not error, NOT propagating to linked actors"
                );
            }
        }

        // 3. Clean up this actor's link/monitor entries
        self.cleanup_terminated_actor_links_monitors(actor_id).await;
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
            ExitReason::Linked { actor_id: linked_id, reason: linked_reason } => {
                format!("linked:{}:{}", linked_id, match linked_reason.as_ref() {
                    ExitReason::Normal => "normal",
                    ExitReason::Shutdown => "shutdown",
                    ExitReason::Killed => "killed",
                    ExitReason::Error(msg) => msg,
                    ExitReason::Linked { .. } => "linked",
                })
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
            let monitoring_actor_id = ActorId::from(monitor_link.monitor_ref.clone());
            
            // Use FacetManager to call facet.on_down() for all facets on the monitoring actor
            // Convert core::ExitReason to facet::ExitReason
            let facet_exit_reason = match &reason {
                ExitReason::Normal => FacetExitReason::Normal,
                ExitReason::Shutdown => FacetExitReason::Shutdown,
                ExitReason::Killed => FacetExitReason::Killed,
                ExitReason::Error(msg) => FacetExitReason::Error(msg.clone()),
                ExitReason::Linked { actor_id: linked_id, reason: linked_reason } => {
                    let linked_reason_str = match linked_reason.as_ref() {
                        ExitReason::Normal => "normal",
                        ExitReason::Shutdown => "shutdown",
                        ExitReason::Killed => "killed",
                        ExitReason::Error(msg) => msg,
                        ExitReason::Linked { .. } => "linked",
                    };
                    FacetExitReason::Error(format!("Linked: {} -> {}", linked_id, linked_reason_str))
                }
            };
            
            let facet_down_start = std::time::Instant::now();
            let facet_down_result = self.facet_manager.call_on_down(
                monitoring_actor_id.to_string(),
                actor_id.to_string(),
                &facet_exit_reason,
            ).await;
            
            let facet_down_duration = facet_down_start.elapsed();
            match facet_down_result {
                Ok(errors) if !errors.is_empty() => {
                    metrics::counter!("plexspaces_facet_down_errors_total",
                        "monitoring_actor_id" => monitor_link.monitor_ref.clone(),
                        "monitored_actor_id" => actor_id.clone(),
                        "error_count" => errors.len().to_string()
                    ).increment(errors.len() as u64);
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
                "monitored_actor_id" => actor_id.clone()
            ).record(facet_down_duration.as_secs_f64());
            metrics::counter!("plexspaces_facet_down_total",
                "monitoring_actor_id" => monitor_link.monitor_ref.clone(),
                "monitored_actor_id" => actor_id.clone()
            ).increment(1);

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
            "actor_id" => actor_id.clone(),
            "action" => "down_sent"
        ).increment(monitors.len() as u64);
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
                    ExitReason::Linked { actor_id: linked_actor_id, reason: linked_reason } => {
                        format!("linked:{}:{}", linked_actor_id, match linked_reason.as_ref() {
                            ExitReason::Normal => "normal",
                            ExitReason::Shutdown => "shutdown",
                            ExitReason::Killed => "killed",
                            ExitReason::Error(msg) => msg,
                            ExitReason::Linked { .. } => "linked",
                        })
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
            "actor_id" => actor_id.clone(),
            "linked_count" => linked_count.to_string()
        ).increment(linked_count as u64);
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

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
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
/// Remote linking can be added later by:
/// 1. Adding a Node reference to ActorRegistry (optional, for remote operations)
/// 2. Checking if actors are local before linking
/// 3. Delegating to Node for remote actor linking
#[async_trait::async_trait]
impl crate::LinkProvider for ActorRegistry {
    async fn link(&self, actor_id: &ActorId, linked_actor_id: &ActorId, ctx: &crate::RequestContext) -> Result<(), String> {
        // TODO: Support remote actor linking
        // For now, only support local actors. Remote linking requires:
        // 1. Checking if actors are local (via lookup_routing)
        // 2. If remote, delegating to Node for gRPC-based linking
        // 3. This is advanced functionality and can be added later
        
        // Verify both actors are local (exist in this registry)
        // Use provided RequestContext for tenant/namespace isolation
        let routing1 = self.lookup_routing(ctx, actor_id).await
            .map_err(|e| format!("Failed to lookup actor {}: {}", actor_id, e))?;
        if routing1.is_none() || !routing1.unwrap().is_local {
            return Err(format!("Actor {} is not local or not found", actor_id));
        }
        
        let routing2 = self.lookup_routing(ctx, linked_actor_id).await
            .map_err(|e| format!("Failed to lookup actor {}: {}", linked_actor_id, e))?;
        if routing2.is_none() || !routing2.unwrap().is_local {
            return Err(format!("Actor {} is not local or not found", linked_actor_id));
        }
        
        // Both actors are local, delegate to ActorRegistry::link
        self.link(actor_id, linked_actor_id).await
            .map_err(|e| format!("Link failed: {}", e))
    }

    async fn unlink(&self, actor_id: &ActorId, linked_actor_id: &ActorId, _ctx: &crate::RequestContext) -> Result<(), String> {
        // TODO: Support remote actor unlinking
        // For now, only support local actors. Remote unlinking requires:
        // 1. Checking if actors are local (via lookup_routing)
        // 2. If remote, delegating to Node for gRPC-based unlinking
        // 3. This is advanced functionality and can be added later
        
        // Both actors should be local (unlink is idempotent, so we don't check)
        self.unlink(actor_id, linked_actor_id).await
            .map_err(|e| format!("Unlink failed: {}", e))
    }
}

// ============================================================================
// ActivationProvider Implementation (Phase 8.5: Reminder-VirtualActor Integration)
// ============================================================================

/// ActivationProvider implementation for ActorRegistry
///
/// ## Purpose
/// Provides activation functionality for virtual actors. This is used by
/// ReminderFacet to activate deactivated virtual actors when reminders fire.
///
/// ## Design
/// - Supports local actors only (actors registered in this ActorRegistry)
/// - Uses ActorFactory to activate virtual actors
/// - Checks actor status via lookup_routing
#[async_trait::async_trait]
impl crate::ActivationProvider for ActorRegistry {
    async fn is_actor_active(&self, actor_id: &ActorId) -> bool {
        // Check if actor is registered and active in this registry
        // Use empty tenant/namespace for internal operations (auth disabled)
        // Internal path (ReminderFacet): no request; use default tenant/namespace and admin for lookup
        use crate::RequestContext;
        let ctx = RequestContext::new_without_auth(String::new(), String::new())
            .with_admin(true);
        let routing = self.lookup_routing(&ctx, actor_id).await.ok().flatten();
        routing.map(|r| r.is_local).unwrap_or(false)
    }

    async fn activate_actor(&self, actor_id: &ActorId) -> Result<ActorRef, String> {
        // Check if actor is already active
        if self.is_actor_active(actor_id).await {
            // Actor is already active, return its ActorRef
            return ActorRef::new(actor_id.clone())
                .map_err(|e| format!("Failed to create ActorRef: {}", e));
        }
        
        // Actor is not active, need to activate it using ActorFactory
        let actor_factory_opt = self.actor_factory.read().await.clone();
        if let Some(actor_factory) = actor_factory_opt {
            // Use ActorFactory to activate the virtual actor
            actor_factory.activate_virtual_actor(actor_id).await
                .map_err(|e| format!("Failed to activate virtual actor {}: {}", actor_id, e))?;
            
            // Verify actor is now active
            if !self.is_actor_active(actor_id).await {
                return Err(format!("Actor {} was not activated successfully", actor_id));
            }
            
            // Return ActorRef for the activated actor
            ActorRef::new(actor_id.clone())
                .map_err(|e| format!("Failed to create ActorRef for activated actor {}: {}", actor_id, e))
        } else {
            // ActorFactory not set - return error with helpful message
            Err(format!(
                "Actor {} is not active and ActorFactory is not available. \
                ActorFactory must be set via ActorRegistry::set_actor_factory() during Node initialization.",
                actor_id
            ))
        }
    }
}

