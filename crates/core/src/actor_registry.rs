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

use crate::{ActorId, ActorRef, MessageSender, ActorMetricsHandle, ActorMetrics, ActorMetricsExt};
use crate::actor_context::ObjectRegistry;
use crate::actor_context::ObjectRegistration;
use crate::service_locator::Service;
use plexspaces_mailbox::{Mailbox, Message};
use plexspaces_proto::object_registry::v1::ObjectType;
use plexspaces_proto::ActorLifecycleEvent;
use crate::facet_manager::FacetManager;

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
            facet_manager: Arc::new(FacetManager::new()),
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
    
    /// Register a MessageSender trait object (Orleans-inspired design)
    ///
    /// ## Purpose
    /// Stores MessageSender trait object in registry. For virtual actors, this is a VirtualActorWrapper
    /// that automatically handles activation. For regular actors, this is a RegularActorWrapper.
    /// Also maintains actor-type index for efficient FaaS-style actor invocation when type information is provided.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `sender` - MessageSender trait object
    /// * `actor_type` - Optional actor type (for efficient type-based lookup via hashmap index)
    /// * `tenant_id` - Optional tenant ID (defaults to "default" if not provided, used for type index)
    /// * `namespace` - Optional namespace (defaults to "default" if not provided, used for type index)
    ///
    /// ## Performance
    /// When `actor_type`, `tenant_id`, and `namespace` are provided, the actor is indexed for O(1) lookup
    /// by type, enabling efficient FaaS-style actor invocation.
    pub async fn register_actor(
        &self,
        actor_id: ActorId,
        sender: Arc<dyn MessageSender>,
        actor_type: Option<String>,
        tenant_id: Option<String>,
        namespace: Option<String>,
    ) {
        let mut actors = self.actors.write().await;
        let was_new = actors.insert(actor_id.clone(), sender).is_none();
        drop(actors);
        
        // Update actor-type index if type information is provided
        if let (Some(actor_type), Some(tenant_id), Some(namespace)) = (actor_type, tenant_id, namespace) {
            let mut index = self.actor_type_index.write().await;
            let key = (tenant_id, namespace, actor_type);
            index.entry(key).or_insert_with(Vec::new).push(actor_id.clone());
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
    pub async fn lookup_node_address(&self, node_id: &str) -> Result<Option<String>, ActorRegistryError> {
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
        let node_object_id = format!("_node@{}", node_id);
        let registration = self.object_registry
            .lookup_full("default", "default", ObjectType::ObjectTypeService, &node_object_id)
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
    pub async fn lookup_routing(&self, actor_id: &ActorId) -> Result<Option<ActorRoutingInfo>, ActorRegistryError> {
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
            let node_address = self.lookup_node_address(&node_id).await?;

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

        // CRITICAL: Lock acquisition order must be consistent to prevent deadlocks
        // Order: 1. actor_instances, 2. facet_manager (via remove_facets), 3. registered_ids, 4. actor_configs
        let mut actor_instances = self.actor_instances.write().await;
        let mut registered_ids = self.registered_actor_ids.write().await;
        let mut actor_configs = self.actor_configs.write().await;

        actor_instances.remove(actor_id);
        self.facet_manager.remove_facets(actor_id).await;
        registered_ids.remove(actor_id);
        actor_configs.remove(actor_id);
        
        // Update metrics if actor existed
        if existed {
            let mut metrics = self.actor_metrics.write().await;
            metrics.decrement_active();
        }

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
    pub async fn notify_actor_down(&self, actor_id: &ActorId, reason: &str) {
        // Notify monitors (one-way notification)
        let mut monitors = self.monitors.write().await;

        // Get all monitoring links for this actor
        if let Some(links) = monitors.remove(actor_id) {
            // Send notifications to all supervisors
            for link in links {
                // Send notification (non-blocking)
                // If send fails, supervisor channel is closed (supervisor died), ignore error
                let _ = link
                    .termination_sender
                    .send((actor_id.clone(), reason.to_string()))
                    .await;
            }
        }
    }
    
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
    /// * `tenant_id` - Tenant identifier
    /// * `actor_type` - Actor type to search for
    ///
    /// ## Returns
    /// Vector of actor IDs matching the type
    ///
    /// ## Performance
    /// O(1) lookup using hashmap index, much faster than scanning ObjectRegistry
    pub async fn discover_actors_by_type(
        &self,
        tenant_id: &str,
        namespace: &str,
        actor_type: &str,
    ) -> Vec<ActorId> {
        let index = self.actor_type_index.read().await;
        let key = (tenant_id.to_string(), namespace.to_string(), actor_type.to_string());
        index.get(&key).cloned().unwrap_or_default()
    }

    pub async fn temporary_sender_count(&self) -> usize {
        let temp_senders = self.temporary_senders.read().await;
        temp_senders.len()
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

}

// Implement Service trait for ActorRegistry
impl Service for ActorRegistry {}

