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
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{mpsc, RwLock};

use crate::{ActorId, ActorRef, MessageSender, RequestContext, ActorMetricsHandle, ActorMetrics, ActorMetricsExt, ExitReason};
use crate::actor_context::ObjectRegistry;
use crate::actor_context::ObjectRegistration;
use crate::service_locator::Service;
use plexspaces_mailbox::{Mailbox, Message};
use plexspaces_proto::object_registry::v1::ObjectType;
use plexspaces_proto::ActorLifecycleEvent;
use plexspaces_facet::{FacetManager, ExitReason as FacetExitReason};

// Observability
use metrics;
use tracing;

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
    #[error("Actor not found: {0}")]
    ActorNotFound(String),

    #[error("Lookup failed: {0}")]
    LookupFailed(String),

    #[error("Registration failed: {0}")]
    RegistrationFailed(String),

    #[error("Unregistration failed: {0}")]
    UnregistrationFailed(String),
}

/// Routing information for an actor
#[derive(Clone, Debug)]
pub struct ActorRoutingInfo {
    pub node_id: String,
    pub node_address: Option<String>,
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
    
    // === Actor-related data (moved from Node) ===
    
    /// Actor instances (for lazy virtual actors - not started yet)
    /// Note: Only stored for lazy virtual actors (normal actors are consumed by start())
    actor_instances: Arc<RwLock<HashMap<ActorId, Arc<dyn std::any::Any + Send + Sync>>>>,
    /// FacetManager for facet storage and management
    /// Note: Facet storage moved to FacetManager for better separation of concerns
    facet_manager: Arc<FacetManager>,
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
    /// Virtual actor metadata (Phase 8.5: Orleans-inspired lifecycle)
    /// Maps actor_id -> VirtualActorMetadata (facet, lifecycle state)
    virtual_actors: Arc<RwLock<HashMap<ActorId, VirtualActorMetadata>>>,
    /// Pending messages for actors being activated (Phase 8.5)
    /// Maps actor_id -> Vec<Message> (queued during activation)
    pending_activations: Arc<RwLock<HashMap<ActorId, Vec<Message>>>>,
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
#[derive(Clone, Debug)]
pub struct TemporarySenderEntry {
    /// ActorRef ID that created this temporary sender (for routing replies)
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

/// Virtual actor metadata (Phase 8.5: Orleans-inspired lifecycle)
/// Note: VirtualActorFacet is defined in journaling crate, but we use trait object here
/// to avoid circular dependency. Node will handle the actual type casting.
///
/// The facet is stored as Arc<RwLock<Box<dyn Any + Send + Sync>>> to allow downcasting.
/// Node will downcast the Box to VirtualActorFacet when needed.
#[derive(Clone)]
pub struct VirtualActorMetadata {
    /// Virtual actor facet (tracks lifecycle state)
    /// Stored as Box<dyn Any> to allow downcasting to VirtualActorFacet in Node
    /// Node will downcast this to VirtualActorFacet when needed
    pub facet: Arc<tokio::sync::RwLock<Box<dyn std::any::Any + Send + Sync>>>,
    /// Last deactivation time (for metrics)
    pub last_deactivated: Option<SystemTime>,
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
            virtual_actors: Arc::new(RwLock::new(HashMap::new())),
            pending_activations: Arc::new(RwLock::new(HashMap::new())),
            actor_configs: Arc::new(RwLock::new(HashMap::new())),
            registered_actor_ids: Arc::new(RwLock::new(HashSet::new())),
            actor_metrics: Arc::new(RwLock::new(ActorMetricsExt::new())),
            temporary_senders: Arc::new(RwLock::new(HashMap::new())),
            actor_type_index: Arc::new(RwLock::new(HashMap::new())),
            // Initialize parent-child relationship tracking
            parent_to_children: Arc::new(RwLock::new(HashMap::new())),
            child_to_parent: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    // === Accessor methods for actor-related data ===
    
    /// Get actor instances map (for lazy virtual actors)
    pub fn actor_instances(&self) -> &Arc<RwLock<HashMap<ActorId, Arc<dyn std::any::Any + Send + Sync>>>> {
        &self.actor_instances
    }
    
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
    
    /// Get virtual actors map
    pub fn virtual_actors(&self) -> &Arc<RwLock<HashMap<ActorId, VirtualActorMetadata>>> {
        &self.virtual_actors
    }
    
    /// Get pending activations map
    pub fn pending_activations(&self) -> &Arc<RwLock<HashMap<ActorId, Vec<Message>>>> {
        &self.pending_activations
    }
    
    /// Get actor configs map
    pub fn actor_configs(&self) -> &Arc<RwLock<HashMap<ActorId, plexspaces_proto::v1::actor::ActorConfig>>> {
        &self.actor_configs
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
    
    /// Register a MessageSender trait object (Orleans-inspired design)
    ///
    /// ## Purpose
    /// Stores MessageSender trait object in registry. For virtual actors, this is a VirtualActorWrapper
    /// that automatically handles activation. For regular actors, this is an ActorRef (implements MessageSender).
    /// Also maintains actor-type index for efficient FaaS-style actor invocation when type information is provided.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (first parameter)
    /// * `actor_id` - Actor ID
    /// * `sender` - MessageSender trait object
    /// * `actor_type` - Optional actor type (for efficient type-based lookup via hashmap index)
    ///
    /// ## Performance
    /// When `actor_type` is provided, the actor is indexed for O(1) lookup
    /// by type, enabling efficient FaaS-style actor invocation.
    pub async fn register_actor(
        &self,
        ctx: &RequestContext,
        actor_id: ActorId,
        sender: Arc<dyn MessageSender>,
        actor_type: Option<String>,
    ) {
        let mut actors = self.actors.write().await;
        let was_new = actors.insert(actor_id.clone(), sender).is_none();
        drop(actors);
        
        // Update actor-type index if type information is provided
        if let Some(actor_type) = actor_type {
            let mut index = self.actor_type_index.write().await;
            let key = (ctx.tenant_id().to_string(), ctx.namespace().to_string(), actor_type.clone());
            index.entry(key).or_insert_with(Vec::new).push(actor_id.clone());
            
            // OBSERVABILITY: Log actor registration with type
            tracing::debug!(
                actor_id = %actor_id,
                actor_type = %actor_type,
                tenant_id = %ctx.tenant_id(),
                namespace = %ctx.namespace(),
                was_new = was_new,
                "Actor registered with type in actor_type_index"
            );
        } else {
            // OBSERVABILITY: Warn if actor_type is missing
            tracing::warn!(
                actor_id = %actor_id,
                "Actor registered without actor_type - will not appear in 'Actors by Type' dashboard"
            );
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
        // For node lookups, use internal context (nodes are registered with internal/system)
        // Nodes are registered with object_id = node_id using ObjectTypeNode (no "_node@" prefix)
        let internal_ctx = RequestContext::internal();
        let registration = self.object_registry
            .lookup_full(&internal_ctx, ObjectType::ObjectTypeNode, node_id)
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

    /// Register actor with config (moved from Node)
    ///
    /// ## Purpose
    /// Registers an actor with optional configuration for resource tracking.
    /// This is called after an actor is spawned to track it in the registry.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `config` - Optional actor configuration
    pub async fn register_actor_with_config(
        &self,
        actor_id: ActorId,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
    ) -> Result<(), ActorRegistryError> {
        // Check if actor is already registered
        {
            let registered_ids = self.registered_actor_ids.read().await;
            if registered_ids.contains(&actor_id) {
                return Err(ActorRegistryError::RegistrationFailed(
                    format!("Actor {} already registered", actor_id)
                ));
            }
        }

        // Track this actor as registered
        {
            let mut registered_ids = self.registered_actor_ids.write().await;
            registered_ids.insert(actor_id.clone());
        }

        // Store actor config if provided
        if let Some(config) = config {
            let mut actor_configs = self.actor_configs.write().await;
            actor_configs.insert(actor_id, config);
        }

        Ok(())
    }

    /// Unregister actor with cleanup (moved from Node)
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
        // Order: 1. actor_instances, 2. facet_manager (via remove_facets), 3. registered_ids, 4. actor_configs
        let mut actor_instances = self.actor_instances.write().await;
        let mut registered_ids = self.registered_actor_ids.write().await;
        let mut actor_configs = self.actor_configs.write().await;

        actor_instances.remove(actor_id);
        
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
        
        // OBSERVABILITY: Log actor unregistration
        tracing::debug!(
            actor_id = %actor_id,
            existed = existed,
            "Actor unregistered with cleanup"
        );

        Ok(())
    }

    /// Notify monitors that an actor has terminated (moved from Node)
    ///
    /// ## Purpose
    /// Notifies all supervisors monitoring this actor that it has terminated.
    /// This is part of the Erlang-style supervision system.
    ///
    /// ## Arguments
    /// * `actor_id` - The actor that terminated
    /// * `reason` - Reason for termination (e.g., "normal", "panic: ...", "killed")
    
    // === Temporary Sender Management ===
    
    /// Register a temporary sender mapping
    ///
    /// ## Purpose
    /// Stores temporary sender info for ask() pattern when called from outside actor context.
    /// Used to route replies back to the correct ActorRef's ReplyWaiter.
    ///
    /// ## Arguments
    /// * `temporary_sender_id` - Temporary sender ID (format: "ask-{correlation_id}@{node_id}")
    /// * `actor_ref_id` - ActorRef ID that created this temporary sender
    /// * `correlation_id` - Correlation ID for matching replies
    /// * `expires_at` - Expiration time for automatic cleanup
    pub async fn register_temporary_sender(
        &self,
        temporary_sender_id: String,
        actor_ref_id: ActorId,
        correlation_id: String,
        expires_at: Instant,
    ) {
        let actor_ref_id_clone = actor_ref_id.clone();
        let mut temp_senders = self.temporary_senders.write().await;
        temp_senders.insert(temporary_sender_id.clone(), TemporarySenderEntry {
            actor_ref_id: actor_ref_id_clone.clone(),
            correlation_id,
            expires_at,
        });
        let count = temp_senders.len();
        drop(temp_senders);
        
        tracing::debug!(
            "ActorRegistry: Registered temporary sender: temporary_sender_id={}, actor_ref_id={}, total_temp_senders={}",
            temporary_sender_id,
            actor_ref_id_clone,
            count
        );
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
    /// ## Arguments
    /// * `temporary_sender_id` - Temporary sender ID to remove
    pub async fn remove_temporary_sender(&self, temporary_sender_id: &str) {
        let mut temp_senders = self.temporary_senders.write().await;
        if temp_senders.remove(temporary_sender_id).is_some() {
            tracing::debug!(
                "ActorRegistry: Removed temporary sender: temporary_sender_id={}, remaining={}",
                temporary_sender_id,
                temp_senders.len()
            );
        }
    }
    
    /// Cleanup expired temporary senders
    ///
    /// ## Purpose
    /// Removes expired temporary sender mappings to prevent memory leaks.
    /// Should be called periodically (e.g., every 30 seconds).
    ///
    /// ## Returns
    /// Number of expired temporary senders removed
    pub async fn cleanup_expired_temporary_senders(&self) -> usize {
        let now = Instant::now();
        let (before_count, expired_count) = {
            let mut temp_senders = self.temporary_senders.write().await;
            let before = temp_senders.len();
            temp_senders.retain(|_id, entry| entry.expires_at > now);
            let after = temp_senders.len();
            (before, before - after)
        };
        
        if expired_count > 0 {
            tracing::debug!(
                "ActorRegistry: Cleaned up {} expired temporary senders (before: {}, after: {})",
                expired_count,
                before_count,
                before_count - expired_count
            );
            
            // OBSERVABILITY: Track expired temporary sender cleanup
            #[cfg(feature = "metrics")]
            {
                metrics::counter!("plexspaces_actor_registry_temporary_sender_expired_total",
                    "node_id" => self.local_node_id.clone()
                ).increment(expired_count as u64);
                metrics::gauge!("plexspaces_actor_registry_temporary_sender_mappings",
                    "node_id" => self.local_node_id.clone()
                ).set((before_count - expired_count) as f64);
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
        index.get(&key).cloned().unwrap_or_default()
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

        tracing::debug!(
            parent = %parent_id,
            child = %child_id,
            "Registered parent-child relationship"
        );
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

        tracing::debug!(
            parent = %parent_id,
            child = %child_id,
            "Unregistered parent-child relationship"
        );
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
                    tracing::debug!(
                        "ActorRegistry: Cleaned up {} expired temporary senders (node_id={})",
                        expired_count,
                        local_node_id
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
        tracing::debug!(
            actor1 = %actor1_id,
            actor2 = %actor2_id,
            "Linked actors (bidirectional death propagation)"
        );

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
        tracing::debug!(
            actor1 = %actor1_id,
            actor2 = %actor2_id,
            "Unlinked actors"
        );

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
        tracing::debug!(
            target = %target_id,
            monitor = %monitor_id,
            monitor_ref = %monitor_ref,
            "Registered monitor (one-way notification)"
        );

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
        tracing::debug!(
            target = %target_id,
            monitor = %monitor_id,
            monitor_ref = %monitor_ref,
            "Removed monitor"
        );

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
        if reason.is_error() {
            self.propagate_exit_to_links(actor_id, &reason).await;
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
                    tracing::debug!(
                        monitoring_actor_id = %monitor_link.monitor_ref,
                        monitored_actor_id = %actor_id,
                        duration_ms = facet_down_duration.as_millis(),
                        "All facets handled DOWN notification successfully"
                    );
                }
                Err(e) => {
                    // FacetManager couldn't find facets for this actor (expected for regular actors)
                    // Facets will be called when the actor processes the DOWN notification via termination_sender channel
                    tracing::debug!(
                        monitoring_actor_id = %monitor_link.monitor_ref,
                        monitored_actor_id = %actor_id,
                        error = %e,
                        "Facets not found in FacetManager (regular actor) - facets will be called when actor processes DOWN notification"
                    );
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
            tracing::debug!(
                from = %actor_id,
                monitor_ref = %monitor_link.monitor_ref,
                reason = %reason_str,
                "Sent DOWN notification to monitor"
            );
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
        tracing::debug!(
            from = %actor_id,
            linked_count = linked_count,
            reason = ?reason,
            "Propagating EXIT to linked actors"
        );

        // Create Linked exit reason for linked actors
        let linked_reason = ExitReason::Linked {
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

                // Create EXIT message using Message::exit() helper
                let exit_message = Message::exit(actor_id.to_string(), &reason_str);
                
                // Send EXIT signal to linked actor's mailbox
                // The actor's message loop will handle it based on trap_exit setting
                // Note: tell() takes only the message, no RequestContext
                if let Err(e) = actor_sender.tell(exit_message).await {
                    tracing::warn!(
                        from = %actor_id,
                        to = %linked_id,
                        error = %e,
                        "Failed to send EXIT to linked actor"
                    );
                } else {
                    tracing::debug!(
                        from = %actor_id,
                        to = %linked_id,
                        reason = ?linked_reason,
                        "Sent EXIT to linked actor"
                    );
                }
            } else {
                // Linked actor doesn't exist (already terminated)
                tracing::debug!(
                    from = %actor_id,
                    to = %linked_id,
                    "Linked actor not found (already terminated)"
                );
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

        tracing::debug!(
            actor_id = %actor_id,
            "Cleaned up link/monitor entries for terminated actor"
        );
    }


}

// Implement Service trait for ActorRegistry
impl Service for ActorRegistry {
    fn service_name(&self) -> String {
        crate::service_locator::service_names::ACTOR_REGISTRY.to_string()
    }
}

