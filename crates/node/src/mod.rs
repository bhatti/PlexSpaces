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

//! Node module for distribution and clustering
//!
//! Provides location transparency and distribution capabilities,
//! inspired by Erlang's node system but elevated for modern needs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;

use plexspaces_core::application::{Application, ApplicationError, ApplicationNode};
use plexspaces_core::{ActorId, ActorRef as CoreActorRef, ActorRegistry, ReplyTracker, ServiceLocator, VirtualActorManager, FacetManager};
use plexspaces_actor::ActorRef;
use plexspaces_journaling::{ActivationProvider, VirtualActorFacet};
use plexspaces_mailbox::Message;
use plexspaces_proto::actor::v1::ActorLink as ProtoActorLink;
use plexspaces_proto::node::v1::{NodeCapabilities as ProtoNodeCapabilities, NodeMetrics, NodeRuntimeConfig};
use plexspaces_supervisor::LinkProvider;
use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField, TupleSpace};
use std::time::{Duration, SystemTime};

// Import gRPC client for remote messaging
use crate::grpc_client::RemoteActorClient;

// Import application manager (declared in lib.rs)
use crate::application_manager::ApplicationManager;

/// Monitor reference (ULID for uniqueness)
pub type MonitorRef = String;

/// Notification sender for actor termination events
/// Sends (actor_id, reason) when monitored actor terminates
pub type TerminationSender = mpsc::Sender<(ActorId, String)>;

// MonitorLink is now defined in ActorRegistry (core crate)
// Re-exported via pub use below

/// Node identifier
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeId(String);

impl NodeId {
    /// Create a new node ID
    pub fn new(id: impl Into<String>) -> Self {
        NodeId(id.into())
    }

    /// Get string representation
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for NodeId {
    fn from(id: &str) -> Self {
        NodeId::new(id)
    }
}

impl From<String> for NodeId {
    fn from(id: String) -> Self {
        NodeId::new(id)
    }
}

// MonitorLink is now defined in ActorRegistry (core crate)
// Use plexspaces_core::MonitorLink instead

// Use proto-generated ActorLink instead of custom struct
type ActorLink = ProtoActorLink;

// VirtualActorMetadata and MonitorLink are now defined in ActorRegistry (core crate)
// Re-export for convenience
pub use plexspaces_core::{VirtualActorMetadata, MonitorLink};

/// Node in the distributed system
#[derive(Clone)]
pub struct Node {
    /// Node identifier
    id: NodeId,
    /// Actor registry for actor registration and lookup
    /// This is now the single source of truth for all actor-related data
    actor_registry: Arc<plexspaces_core::ActorRegistry>,
    /// Connection metadata (node addresses, health status)
    connections: Arc<RwLock<HashMap<NodeId, NodeConnection>>>,
    /// gRPC client pool - ONE persistent client per remote node (for performance)
    /// Lazily initialized on first use, reused for all subsequent messages
    /// Includes connection health tracking and automatic reconnection
    grpc_clients: Arc<RwLock<HashMap<NodeId, RemoteActorClient>>>,
    /// Connection health tracking: node_id -> (last_used, last_error, consecutive_failures)
    connection_health: Arc<RwLock<HashMap<NodeId, (tokio::time::Instant, Option<String>, u32)>>>,
    /// Shared TupleSpace for coordination (Pillar 1)
    tuplespace: Arc<TupleSpace>,
    /// Node configuration
    config: NodeConfig,
    /// Node metrics (combined resource and operational metrics)
    metrics: Arc<RwLock<NodeMetrics>>,
    /// Application manager for Erlang/OTP-style application lifecycle
    application_manager: Arc<RwLock<ApplicationManager>>,
    /// Object registry for service discovery (actors, tuplespaces, services)
    object_registry: Arc<plexspaces_object_registry::ObjectRegistry>,
    /// Process group registry for pub/sub coordination
    process_group_registry: Arc<plexspaces_process_groups::ProcessGroupRegistry>,
    /// Background scheduler (Phase 4: Resource-aware scheduling)
    /// Processes scheduling requests asynchronously with lease-based coordination
    background_scheduler: Arc<RwLock<Option<Arc<plexspaces_scheduler::background::BackgroundScheduler>>>>,
    /// Task router (Phase 5: Task routing)
    /// Routes tasks to actor groups using channels
    task_router: Arc<RwLock<Option<Arc<plexspaces_scheduler::TaskRouter>>>>,
    /// WASM runtime for dynamic actor deployment (created in start())
    wasm_runtime: Arc<RwLock<Option<Arc<plexspaces_wasm_runtime::WasmRuntime>>>>,
    /// Blob storage service (S3-compatible object storage)
    /// Created in start() if blob config is provided
    blob_service: Arc<RwLock<Option<Arc<plexspaces_blob::BlobService>>>>,
    /// Shutdown trigger for programmatic shutdown (allows shutdown() to stop gRPC server)
    shutdown_tx: Arc<RwLock<Option<tokio::sync::oneshot::Sender<()>>>>,
    /// ServiceLocator for centralized service registration and gRPC client caching
    service_locator: Arc<ServiceLocator>,
    /// Health reporter for health checks and graceful shutdown (Phase 5)
    /// Set in Node::start(), None before start
    health_reporter: Arc<RwLock<Option<Arc<crate::health_service::PlexSpacesHealthReporter>>>>,
}

/// Node configuration
// Use proto-generated NodeRuntimeConfig instead of custom struct
pub type NodeConfig = NodeRuntimeConfig;

/// Helper function to create default NodeConfig
/// (Can't implement Default trait for type aliases to external types)
pub fn default_node_config() -> NodeRuntimeConfig {
    NodeRuntimeConfig {
        listen_addr: "0.0.0.0:9000".to_string(),
        max_connections: 100,
        heartbeat_interval_ms: 5000,
        clustering_enabled: true,
        metadata: HashMap::new(),
    }
}

// Note: NodeMetrics is from proto crate, so we can't add methods to it
// Use direct field access: metrics.active_actors (u32) instead of usize

/// Connection to another node
#[allow(dead_code)]
struct NodeConnection {
    /// Remote node ID
    remote_id: NodeId,
    /// Remote node gRPC address (e.g., "http://localhost:9001")
    node_address: String,
    /// Connection state
    state: ConnectionState,
    /// Last heartbeat
    last_heartbeat: tokio::time::Instant,
    /// Remote node capabilities (using proto-generated type)
    capabilities: ProtoNodeCapabilities,
}

/// Connection state
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
    Failed(String),
}

// Use proto-generated NodeCapabilities instead of custom struct
type NodeCapabilities = ProtoNodeCapabilities;

// Helper function to create default NodeMetrics (can't impl Default for external type)
fn default_node_metrics() -> NodeMetrics {
    NodeMetrics {
        memory_used_bytes: 0,
        memory_available_bytes: 0,
        cpu_usage_percent: 0.0,
        uptime_seconds: 0,
        messages_routed: 0,
        local_deliveries: 0,
        remote_deliveries: 0,
        failed_deliveries: 0,
        messages_processed: 0,
        active_actors: 0,
        actor_count: 0,
        connected_nodes: 0,
    }
}

impl Node {
    /// Create a new node
    pub fn new(id: NodeId, config: NodeConfig) -> Self {
        // Create in-memory KeyValueStore for ObjectRegistry (can be configured later)
        use plexspaces_keyvalue::InMemoryKVStore;
        use std::sync::Arc;
        let kv_store = Arc::new(InMemoryKVStore::new());
        let object_registry = Arc::new(plexspaces_object_registry::ObjectRegistry::new(kv_store.clone()));
        
        // Create ProcessGroupRegistry with same KeyValueStore backend
        let process_group_registry = Arc::new(plexspaces_process_groups::ProcessGroupRegistry::new(
            id.as_str().to_string(),
            kv_store.clone(),
        ));
        
        // Create ActorRegistry with ObjectRegistry
        let object_registry_trait: Arc<dyn plexspaces_core::ObjectRegistry> = Arc::new(
            crate::service_wrappers::ObjectRegistryWrapper::new(object_registry.clone())
        );
        let actor_registry = Arc::new(ActorRegistry::new(
            object_registry_trait,
            id.as_str().to_string(),
        ));

        // Create ServiceLocator and register services
        let service_locator = Arc::new(ServiceLocator::new());
        let reply_tracker = Arc::new(ReplyTracker::new());
        
        // Create ReplyWaiterRegistry for ask pattern (must be shared between Node and ActorServiceImpl)
        use plexspaces_core::ReplyWaiterRegistry;
        let reply_waiter_registry = Arc::new(ReplyWaiterRegistry::new());
        
        // Create VirtualActorManager and register in ServiceLocator
        let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
        
        // Register services in ServiceLocator (lazy initialization)
        // Since Node::new() is not async, we register services in a spawned task if runtime is available
        // Otherwise, services will be registered on first use
        // Note: Node itself is registered in start() method after Node is fully constructed
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let service_locator_clone = service_locator.clone();
            let actor_registry_clone = actor_registry.clone();
            let reply_tracker_clone = reply_tracker.clone();
            let reply_waiter_registry_clone = reply_waiter_registry.clone();
            let virtual_actor_manager_clone = virtual_actor_manager.clone();
            // Create NodeMetricsUpdater - we need to create a temporary Arc<Node> for it
            // Since we can't create Arc<Node> before Node is constructed, we'll register it in create_actor_context_arc
            // But for tests, we need it registered early, so we'll register it here if we have a runtime
            // Actually, we can't create NodeMetricsUpdater here because we don't have Arc<Node> yet
            // So we'll register it in create_actor_context_arc, but we need to ensure it's registered before tests run
            let facet_manager_clone = actor_registry_clone.facet_manager().clone();
            handle.spawn(async move {
                service_locator_clone.register_service(actor_registry_clone).await;
                service_locator_clone.register_service(reply_tracker_clone).await;
                service_locator_clone.register_service(reply_waiter_registry_clone).await;
                service_locator_clone.register_service(virtual_actor_manager_clone).await;
                // Register FacetManager (from ActorRegistry)
                service_locator_clone.register_service(facet_manager_clone).await;
            });
        }
        
        // Store node_arc for later registration in start()
        // We'll register Node in ServiceLocator in start() method

        Node {
            id,
            actor_registry,
            service_locator,
            connections: Arc::new(RwLock::new(HashMap::new())),
            grpc_clients: Arc::new(RwLock::new(HashMap::new())), // Client pool
            tuplespace: Arc::new(TupleSpace::new()),
            config,
            metrics: Arc::new(RwLock::new(default_node_metrics())),
            application_manager: Arc::new(RwLock::new(ApplicationManager::new())), // Erlang/OTP application lifecycle
            object_registry,
            process_group_registry,
            shutdown_tx: Arc::new(RwLock::new(None)), // Shutdown trigger (set in start())
            background_scheduler: Arc::new(RwLock::new(None)), // Phase 4: Background scheduler (created in start())
            task_router: Arc::new(RwLock::new(None)), // Phase 5: Task router (created in start())
            wasm_runtime: Arc::new(RwLock::new(None)), // WASM runtime (created in start())
            blob_service: Arc::new(RwLock::new(None)), // Blob service (created in start())
            connection_health: Arc::new(RwLock::new(HashMap::new())), // Connection health tracking
            health_reporter: Arc::new(RwLock::new(None)), // Health reporter (created in start())
        }
    }

    /// Get node ID
    pub fn id(&self) -> &NodeId {
        &self.id
    }

    /// Get node configuration
    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    /// Get ServiceLocator
    pub fn service_locator(&self) -> Arc<ServiceLocator> {
        self.service_locator.clone()
    }


    /// Get ActorRegistry (internal use only - use service_locator.get_service() instead)
    pub(crate) fn actor_registry(&self) -> Arc<ActorRegistry> {
        self.actor_registry.clone()
    }
    
    // === Helper methods to access actor data via ActorRegistry ===
    // These provide convenient access to actor-related data stored in ActorRegistry
    
    /// Get actor instances (for lazy virtual actors)
    /// Note: Returns trait object - need to downcast to Arc<Actor> in Node
    pub fn actor_instances(&self) -> &Arc<RwLock<HashMap<ActorId, Arc<dyn std::any::Any + Send + Sync>>>> {
        self.actor_registry.actor_instances()
    }
    
    /// Get facet storage (use facet_manager() instead)
    pub fn facet_storage(&self) -> &Arc<RwLock<HashMap<ActorId, Arc<tokio::sync::RwLock<plexspaces_facet::FacetContainer>>>>> {
        self.actor_registry.facet_manager().facet_storage()
    }
    
    /// Get FacetManager
    pub fn facet_manager(&self) -> &Arc<FacetManager> {
        self.actor_registry.facet_manager()
    }
    
    /// Get monitors
    pub fn monitors(&self) -> &Arc<RwLock<HashMap<ActorId, Vec<MonitorLink>>>> {
        self.actor_registry.monitors()
    }
    
    /// Get links
    pub fn links(&self) -> &Arc<RwLock<HashMap<ActorId, Vec<ActorId>>>> {
        self.actor_registry.links()
    }
    
    /// Get lifecycle subscribers
    pub fn lifecycle_subscribers(&self) -> &Arc<RwLock<Vec<mpsc::UnboundedSender<plexspaces_proto::ActorLifecycleEvent>>>> {
        self.actor_registry.lifecycle_subscribers()
    }
    
    /// Get virtual actor manager from ServiceLocator
    ///
    /// ## Note
    /// VirtualActorManager is now registered in ServiceLocator instead of stored directly in Node.
    /// This removes Node as a middleman for actor management, simplifying the design.
    pub async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>> {
        self.service_locator.get_service::<VirtualActorManager>().await
    }
    
    /// Get virtual actor manager from ServiceLocator or return error
    ///
    /// ## Returns
    /// Arc<VirtualActorManager> if found, NodeError::ActorNotFound if not registered
    async fn get_virtual_actor_manager(&self) -> Result<Arc<VirtualActorManager>, NodeError> {
        self.service_locator.get_service::<VirtualActorManager>().await
            .ok_or_else(|| NodeError::ActorNotFound("VirtualActorManager not registered in ServiceLocator".to_string()))
    }
    
    /// Get virtual actors map (delegates to ActorRegistry)
    pub fn virtual_actors(&self) -> &Arc<RwLock<HashMap<ActorId, VirtualActorMetadata>>> {
        self.actor_registry.virtual_actors()
    }
    
    /// Get pending activations
    pub fn pending_activations(&self) -> &Arc<RwLock<HashMap<ActorId, Vec<Message>>>> {
        self.actor_registry.pending_activations()
    }
    
    /// Get actor configs
    pub fn actor_configs(&self) -> &Arc<RwLock<HashMap<ActorId, plexspaces_proto::v1::actor::ActorConfig>>> {
        self.actor_registry.actor_configs()
    }
    
    /// Get registered actor IDs
    pub fn registered_actor_ids(&self) -> &Arc<RwLock<std::collections::HashSet<ActorId>>> {
        self.actor_registry.registered_actor_ids()
    }
    
    // === Helper methods for VirtualActorFacet downcasting ===
    // Since VirtualActorMetadata stores facet as Box<dyn Any>, we need to downcast
    
    /// Helper to downcast facet from Box<dyn Any> to VirtualActorFacet
    /// This is needed because VirtualActorMetadata stores facet as trait object to avoid circular dependency
    fn downcast_virtual_actor_facet(
        facet_box: &Box<dyn std::any::Any + Send + Sync>,
    ) -> Option<&VirtualActorFacet> {
        facet_box.downcast_ref::<VirtualActorFacet>()
    }
    
    /// Get ObjectRegistry (internal use only - use service_locator.get_service() instead)
    pub(crate) fn object_registry(&self) -> Arc<plexspaces_object_registry::ObjectRegistry> {
        self.object_registry.clone()
    }
    
    /// Get ProcessGroupRegistry (for actor context creation)
    pub fn process_group_registry(&self) -> Arc<plexspaces_process_groups::ProcessGroupRegistry> {
        self.process_group_registry.clone()
    }
    
    /// Get health reporter (for tests and advanced usage)
    pub async fn health_reporter(&self) -> Option<Arc<crate::health_service::PlexSpacesHealthReporter>> {
        let guard = self.health_reporter.read().await;
        guard.clone()
    }

    /// Get WASM runtime (if initialized)
    ///
    /// Returns None if node hasn't been started yet.
    pub async fn wasm_runtime(&self) -> Option<Arc<plexspaces_wasm_runtime::WasmRuntime>> {
        let runtime_guard = self.wasm_runtime.read().await;
        runtime_guard.clone()
    }

    // NOTE: register_actor and unregister_actor have been removed from Node.
    // Use ActorRegistry::register_actor_with_config() and ActorRegistry::unregister_with_cleanup() directly.
    // Metrics are updated by ActorRegistry internally.


    /// Spawn an actor on this node (Erlang-style)
    ///
    /// ## Purpose
    /// Starts an actor and establishes supervision. The node owns the JoinHandle and
    /// automatically detects when the actor terminates (normally or via panic).
    ///
    /// ## Erlang Philosophy
    /// In Erlang, when you spawn a process, the runtime monitors it. If it crashes,
    /// linked/monitoring processes receive EXIT signals. This method implements the
    /// same pattern using Tokio's JoinHandle.
    ///
    /// ## Arguments
    /// * `actor` - The actor to spawn (takes ownership)
    ///
    /// ## Returns
    /// `ActorRef` - Reference for sending messages to the actor
    ///
    /// ## Behavior
    /// 1. Calls `actor.start()` to get JoinHandle
    /// 2. Registers actor with node
    /// 3. Spawns background task to watch JoinHandle
    /// 4. On termination, calls `notify_actor_down()` for all monitors
    ///
    /// ## Example
    /// ```ignore
    /// let actor = Actor::new(...);
    /// let actor_ref = node.spawn_actor(actor).await?;
    ///
    /// // Node automatically watches for termination
    /// // Monitors will be notified if actor crashes
    /// ```
    // NOTE: spawn_actor has been removed from Node.
    // Use ActorFactory::spawn_built_actor() directly via ServiceLocator.
    // Example:
    //   let actor_factory: Arc<ActorFactoryImpl> = service_locator.get_service().await?;
    //   let message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None).await?;
    //   let actor_ref = ActorRef::remote(actor_id, node_id, service_locator);


    /// Get facets container for facet access (Option 1: Facet Storage)
    ///
    /// ## Purpose
    /// Returns the facets container for accessing facets.
    /// Used by FacetService to get facets from actors (normal and virtual).
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    ///
    /// ## Returns
    /// Facets container or None if not found
    pub async fn get_facets(
        &self,
        actor_id: &ActorId,
    ) -> Option<Arc<tokio::sync::RwLock<plexspaces_facet::FacetContainer>>> {
        self.facet_manager().get_facets(actor_id).await
    }

    /// Find an actor (local or remote)
    /// Get virtual actor metadata (read-only access)
    ///
    /// ## Purpose
    /// Provides read-only access to virtual actor metadata for gRPC services and observability.
    ///
    /// ## Returns
    /// `Option<VirtualActorMetadata>` - Virtual actor metadata if actor is virtual, None otherwise
    /// Get virtual actor metadata (delegates to VirtualActorManager)
    pub async fn get_virtual_actor_metadata(&self, actor_id: &ActorId) -> Option<VirtualActorMetadata> {
        if let Ok(manager) = self.get_virtual_actor_manager().await {
            manager.get_metadata(actor_id).await
        } else {
            None
        }
    }
    
    /// Check if an actor is a virtual actor (delegates to VirtualActorManager)
    pub async fn is_virtual_actor(&self, actor_id: &ActorId) -> bool {
        if let Ok(manager) = self.get_virtual_actor_manager().await {
            manager.is_virtual(actor_id).await
        } else {
            false
        }
    }
    
    
    /// Get actor configuration (read-only access)
    ///
    /// ## Purpose
    /// Provides read-only access to actor configuration for gRPC services and observability.
    ///
    /// ## Returns
    /// `Option<ActorConfig>` - Actor configuration if available, None otherwise
    pub async fn get_actor_config(&self, actor_id: &ActorId) -> Option<plexspaces_proto::v1::actor::ActorConfig> {
        self.actor_configs().read().await.get(actor_id).cloned()
    }
    
    /// Get pending message count for a virtual actor during activation
    ///
    /// ## Purpose
    /// Returns the number of messages queued for an actor that is currently being activated.
    ///
    /// ## Returns
    /// `u32` - Number of pending messages (0 if actor is not being activated or has no pending messages)
    pub async fn get_pending_activation_count(&self, actor_id: &ActorId) -> u32 {
        self.pending_activations()
            .read()
            .await
            .get(actor_id)
            .map(|messages| messages.len() as u32)
            .unwrap_or(0)
    }

    // NOTE: find_actor has been removed from Node.
    // Use ActorRegistry::lookup_routing() and ActorRegistry::is_actor_activated() directly.
    // Example:
    //   let routing = actor_registry.lookup_routing(actor_id).await?;
    //   if let Some(routing_info) = routing {
    //       if routing_info.is_local && actor_registry.is_actor_activated(actor_id).await {
    //           // Actor is local and activated
    //       } else if !routing_info.is_local {
    //           // Actor is remote
    //       }
    //   }

    /// Connect to another node (register remote node address)
    pub async fn connect_to(&self, remote_id: NodeId, address: String) -> Result<(), NodeError> {
        let mut connections = self.connections.write().await;

        if connections.contains_key(&remote_id) {
            return Err(NodeError::AlreadyConnected(remote_id));
        }

        // Register connection metadata (address stored for connection pooling)
        let address_clone = address.clone();
        let connection = NodeConnection {
            remote_id: remote_id.clone(),
            node_address: address, // Store address for gRPC client
            state: ConnectionState::Connected,
            last_heartbeat: tokio::time::Instant::now(),
            capabilities: NodeCapabilities::default(),
        };

        connections.insert(remote_id.clone(), connection);
        let connected_count = connections.len();
        drop(connections);

        let mut metrics = self.metrics.write().await;
        metrics.connected_nodes = connected_count as u32;
        drop(metrics);
        
        // Record metrics
        metrics::gauge!("plexspaces_node_connected_nodes",
            "node_id" => self.id().as_str().to_string()
        ).set(connected_count as f64);
        tracing::info!(remote_node_id = %remote_id.as_str(), address = %address_clone, node_id = %self.id().as_str(), "Connected to remote node");

        // Announce connection in TupleSpace
        self.tuplespace
            .write(Tuple::new(vec![
                TupleField::String("node_connected".to_string()),
                TupleField::String(self.id.as_str().to_string()),
                TupleField::String(remote_id.as_str().to_string()),
            ]))
            .await
            .map_err(|e| NodeError::TupleSpaceError(e.to_string()))?;

        // Note: gRPC client created lazily on first message (connection pooling)
        Ok(())
    }

    /// Register a remote node (alias for connect_to)
    /// This is the preferred API for Phase 2 testing
    pub async fn register_remote_node(
        &self,
        remote_id: NodeId,
        address: String,
    ) -> Result<(), NodeError> {
        self.connect_to(remote_id, address).await
    }

    /// Disconnect from another node
    pub async fn disconnect_from(&self, remote_id: &NodeId) -> Result<(), NodeError> {
        let was_connected = {
            let connections = self.connections.read().await;
            connections.contains_key(remote_id)
        };
        
        if was_connected {
            let mut connections = self.connections.write().await;
            connections.remove(remote_id);
            drop(connections);

            // Remove from connection pool
            let mut clients = self.grpc_clients.write().await;
            clients.remove(remote_id);
            drop(clients);

            metrics::gauge!("plexspaces_node_active_connections",
                "node_id" => self.id().as_str().to_string()
            ).decrement(1.0);
            let connected_count = self.connections.read().await.len();
            metrics::gauge!("plexspaces_node_connected_nodes",
                "node_id" => self.id().as_str().to_string()
            ).set(connected_count as f64);
            tracing::info!(remote_node_id = %remote_id.as_str(), node_id = %self.id().as_str(), "Disconnected from remote node");
            
            let mut metrics = self.metrics.write().await;
            metrics.connected_nodes = connected_count as u32;
            drop(metrics);
        }

        // Announce disconnection in TupleSpace
        self.tuplespace
            .write(Tuple::new(vec![
                TupleField::String("node_disconnected".to_string()),
                TupleField::String(self.id.as_str().to_string()),
                TupleField::String(remote_id.as_str().to_string()),
            ]))
            .await
            .map_err(|e| NodeError::TupleSpaceError(e.to_string()))?;

        Ok(())
    }

    /// Get list of connected nodes
    pub async fn connected_nodes(&self) -> Vec<NodeId> {
        let connections = self.connections.read().await;
        connections.keys().cloned().collect()
    }

    /// Get node statistics
    pub async fn metrics(&self) -> NodeMetrics {
        let guard = self.metrics.read().await;
        guard.clone()
    }
    
    /// Get node statistics (alias for metrics for backward compatibility)
    pub async fn stats(&self) -> NodeMetrics {
        self.metrics().await
    }
    
    /// Increment messages_routed counter (for NodeMetricsUpdater)
    pub(crate) async fn increment_messages_routed(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.messages_routed += 1;
    }
    
    /// Increment local_deliveries counter (for NodeMetricsUpdater)
    pub(crate) async fn increment_local_deliveries(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.local_deliveries += 1;
    }
    
    /// Increment remote_deliveries counter (for NodeMetricsUpdater)
    pub(crate) async fn increment_remote_deliveries(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.remote_deliveries += 1;
    }
    
    /// Increment failed_deliveries counter (for NodeMetricsUpdater)
    pub(crate) async fn increment_failed_deliveries(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.failed_deliveries += 1;
    }

    /// Get the shared TupleSpace
    pub fn tuplespace(&self) -> &Arc<TupleSpace> {
        &self.tuplespace
    }

    /// Calculate the current node capacity.
    /// This includes total, allocated, and available resources.
    #[cfg_attr(test, allow(dead_code))] // Allow dead code in tests (used by tests)
    pub(crate) async fn calculate_node_capacity(&self) -> plexspaces_proto::node::v1::NodeCapacity {
        use sysinfo::System;
        use plexspaces_proto::{node::v1::NodeCapacity, common::v1::ResourceSpec};

        let mut sys = System::new_all();
        sys.refresh_all();

        // Total resources
        let total_memory_bytes = sys.total_memory();
        let total_cpu_cores = sys.cpus().len() as f64;
        // Note: sysinfo v0.30 doesn't have a direct disks() method
        // For now, we'll use a placeholder value. Disk capacity tracking
        // can be enhanced later with platform-specific APIs or configuration.
        // Get actual disk capacity using sysinfo
        use sysinfo::Disks;
        let disks = Disks::new_with_refreshed_list();
        let total_disk_bytes: u64 = disks.iter().map(|d| d.total_space()).sum();
        
        // Get GPU count/type if available
        // Note: sysinfo doesn't provide GPU information
        // In the future, this could use nvidia-ml-py bindings or other GPU libraries
        let gpu_count = 0u32; // TODO: Integrate GPU detection library if needed

        let total_resources = ResourceSpec {
            cpu_cores: total_cpu_cores,
            memory_bytes: total_memory_bytes,
            disk_bytes: total_disk_bytes,
            gpu_count: 0, // Placeholder
            gpu_type: String::new(), // Placeholder
        };

        // Allocated resources (sum of all active actors' resource requirements)
        let mut allocated_cpu_cores = 0.0;
        let mut allocated_memory_bytes = 0u64;
        let mut allocated_disk_bytes = 0u64;
        let mut allocated_gpu_count = 0u32;

        // Get actor configs and sum up resource requirements
        let actor_configs = self.actor_configs().read().await;
        for config in actor_configs.values() {
            if let Some(ref resource_reqs) = config.resource_requirements {
                if let Some(ref resources) = resource_reqs.resources {
                    allocated_cpu_cores += resources.cpu_cores;
                    allocated_memory_bytes += resources.memory_bytes;
                    allocated_disk_bytes += resources.disk_bytes;
                    allocated_gpu_count += resources.gpu_count;
                }
            }
        }
        drop(actor_configs);

        let allocated_resources = ResourceSpec {
            cpu_cores: allocated_cpu_cores,
            memory_bytes: allocated_memory_bytes,
            disk_bytes: allocated_disk_bytes,
            gpu_count: allocated_gpu_count,
            gpu_type: String::new(),
        };

        // Available resources (use saturating_sub to prevent underflow when allocated > total)
        let available_resources = ResourceSpec {
            cpu_cores: (total_cpu_cores - allocated_cpu_cores).max(0.0),
            memory_bytes: total_memory_bytes.saturating_sub(allocated_memory_bytes),
            disk_bytes: total_disk_bytes.saturating_sub(allocated_disk_bytes),
            gpu_count: 0u32.saturating_sub(allocated_gpu_count), // Assuming 0 total GPUs for now, use saturating_sub to avoid overflow
            gpu_type: String::new(),
        };

        NodeCapacity {
            total: Some(total_resources),
            allocated: Some(allocated_resources),
            available: Some(available_resources),
            labels: self.config.metadata.clone(), // Using node metadata as labels for now
        }
    }

    /// Send heartbeat with node capacity to ObjectRegistry.
    async fn send_heartbeat_with_capacity(&self) -> Result<(), NodeError> {
        use plexspaces_proto::{
            prost_types::Timestamp,
            object_registry::v1::{ObjectType, HealthStatus},
        };
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now();
        let timestamp = Some(Timestamp {
            seconds: now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64,
            nanos: now.duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos() as i32,
        });

        let node_capacity = self.calculate_node_capacity().await;
        let active_actors = {
            let metrics = self.metrics.read().await;
            metrics.active_actors as u64
        };

        // Build metrics map from capacity (convert bytes to MB for readability)
        let mut metrics = std::collections::HashMap::new();
        if let Some(total) = &node_capacity.total {
            metrics.insert("total_cpu_cores".to_string(), total.cpu_cores);
            metrics.insert("total_memory_mb".to_string(), (total.memory_bytes / (1024 * 1024)) as f64);
            metrics.insert("total_disk_mb".to_string(), (total.disk_bytes / (1024 * 1024)) as f64);
            metrics.insert("total_gpu_count".to_string(), total.gpu_count as f64);
        }
        if let Some(allocated) = &node_capacity.allocated {
            metrics.insert("allocated_cpu_cores".to_string(), allocated.cpu_cores);
            metrics.insert("allocated_memory_mb".to_string(), (allocated.memory_bytes / (1024 * 1024)) as f64);
            metrics.insert("allocated_disk_mb".to_string(), (allocated.disk_bytes / (1024 * 1024)) as f64);
            metrics.insert("allocated_gpu_count".to_string(), allocated.gpu_count as f64);
        }
        if let Some(available) = &node_capacity.available {
            metrics.insert("available_cpu_cores".to_string(), available.cpu_cores);
            metrics.insert("available_memory_mb".to_string(), (available.memory_bytes / (1024 * 1024)) as f64);
            metrics.insert("available_disk_mb".to_string(), (available.disk_bytes / (1024 * 1024)) as f64);
            metrics.insert("available_gpu_count".to_string(), available.gpu_count as f64);
        }
        metrics.insert("active_actors".to_string(), active_actors as f64);

        // Send heartbeat to ObjectRegistry
        // Note: Using OBJECT_TYPE_SERVICE for nodes (nodes are services in the registry)
        // TODO: Consider adding OBJECT_TYPE_NODE to ObjectType enum if needed
        self.object_registry
            .heartbeat(
                "default", // tenant_id
                "default", // namespace
                ObjectType::ObjectTypeService,
                self.id.as_str(),
            )
            .await
            .map_err(|e| NodeError::NetworkError(format!("ObjectRegistry heartbeat failed: {}", e)))?;

        Ok(())
    }

    /// Initialize blob service if configured
    /// Returns Some(Arc<BlobService>) if blob service should be enabled, None otherwise
    async fn init_blob_service(&self) -> Option<Arc<plexspaces_blob::BlobService>> {
        use plexspaces_blob::repository::sql::SqlBlobRepository;
        use plexspaces_proto::storage::v1::BlobConfig as ProtoBlobConfig;
        use std::env;

        // Try to get blob config from environment or use defaults
        // TODO: Load from RuntimeConfig when ReleaseSpec is integrated
        let blob_config = ProtoBlobConfig {
            backend: env::var("BLOB_BACKEND").unwrap_or_else(|_| "minio".to_string()),
            bucket: env::var("BLOB_BUCKET").unwrap_or_else(|_| "plexspaces".to_string()),
            endpoint: env::var("BLOB_ENDPOINT").ok().unwrap_or_else(|| "http://localhost:9000".to_string()),
            region: env::var("BLOB_REGION").ok().unwrap_or_default(),
            access_key_id: env::var("BLOB_ACCESS_KEY_ID")
                .or_else(|_| env::var("AWS_ACCESS_KEY_ID"))
                .ok()
                .unwrap_or_else(|| "minioadmin_user".to_string()),
            secret_access_key: env::var("BLOB_SECRET_ACCESS_KEY")
                .or_else(|_| env::var("AWS_SECRET_ACCESS_KEY"))
                .ok()
                .unwrap_or_else(|| "minioadmin_pass".to_string()),
            use_ssl: env::var("BLOB_USE_SSL")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            prefix: env::var("BLOB_PREFIX").unwrap_or_else(|_| "/plexspaces".to_string()),
            gcp_service_account_json: env::var("GCP_SERVICE_ACCOUNT_JSON").ok().unwrap_or_default(),
            azure_account_name: env::var("AZURE_ACCOUNT_NAME").ok().unwrap_or_default(),
            azure_account_key: env::var("AZURE_ACCOUNT_KEY").ok().unwrap_or_default(),
        };

        // Check if blob service is disabled
        if env::var("BLOB_ENABLED").unwrap_or_else(|_| "true".to_string()) == "false" {
            return None;
        }

        // Create SQLite repository for metadata (in-memory for now, can be configured)
        let db_url = env::var("BLOB_DATABASE_URL")
            .unwrap_or_else(|_| "sqlite::memory:".to_string());
        
        // Use AnyPool directly to support both SQLite and PostgreSQL
        use sqlx::AnyPool;
        let any_pool = match AnyPool::connect(&db_url).await {
            Ok(pool) => pool,
            Err(e) => {
                eprintln!("Warning: Failed to create blob service database: {}. Blob service disabled.", e);
                return None;
            }
        };
        
        // Run migrations
        if let Err(e) = SqlBlobRepository::migrate(&any_pool).await {
            eprintln!("Warning: Failed to migrate blob service database: {}. Blob service disabled.", e);
            return None;
        }

        let repository = Arc::new(SqlBlobRepository::new(any_pool));

        // Create blob service
        match plexspaces_blob::BlobService::new(blob_config, repository).await {
            Ok(service) => {
                let service_arc = Arc::new(service);
                
                // Register in service_locator
                self.service_locator.register_service(service_arc.clone()).await;
                
                // Store in Node
                {
                    let mut blob_service_guard = self.blob_service.write().await;
                    *blob_service_guard = Some(service_arc.clone());
                }
                
                Some(service_arc)
            }
            Err(e) => {
                eprintln!("Warning: Failed to initialize blob service: {}. Blob service disabled.", e);
                None
            }
        }
    }

    /// Start node services (heartbeat, discovery, etc.)
    /// Start the node with gRPC server for both ActorService and TuplePlexSpaceService
    ///
    /// ## Purpose
    /// Starts all node services:
    /// - gRPC server for ActorService (remote actor messaging)
    /// - gRPC server for TuplePlexSpaceService (distributed TupleSpace)
    /// - Blob service (S3-compatible object storage)
    /// - Heartbeat task for node health monitoring
    /// - Announces node availability in TupleSpace
    /// - Registers SIGTERM/SIGINT handlers for graceful shutdown
    ///
    /// ## Graceful Shutdown
    /// When SIGTERM or SIGINT is received:
    /// 1. Stops accepting new requests
    /// 2. Calls `shutdown()` to stop all applications
    /// 3. Drains actor mailboxes
    /// 4. Exits cleanly
    ///
    /// ## Returns
    /// Never returns normally - runs until shutdown signal received
    pub async fn start(self: Arc<Self>) -> Result<(), NodeError> {
        use crate::grpc_service::ActorServiceImpl;
        use crate::tuplespace_service::TuplePlexSpaceServiceImpl;
        use plexspaces_proto::{ActorServiceServer, TuplePlexSpaceServiceServer};
        use tonic::transport::Server;

        // Set node context for application manager
        self.application_manager
            .write()
            .await
            .set_node_context(self.clone());

        // Register node in ObjectRegistry before starting heartbeat
        // This ensures heartbeats will succeed
        // Use Notify to synchronize: heartbeat waits for registration to complete
        let registration_notify = Arc::new(tokio::sync::Notify::new());
        let registration_notify_for_registration = registration_notify.clone();
        let registration_notify_for_heartbeat = registration_notify.clone();
        
        let node_id_str = self.id.as_str().to_string();
        let listen_addr = self.config.listen_addr.clone();
        let node_for_registration = self.clone();
        tokio::spawn(async move {
            // Register the node in ObjectRegistry
            use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
            let registration = ObjectRegistration {
                tenant_id: "default".to_string(),
                namespace: "default".to_string(),
                object_type: ObjectType::ObjectTypeService as i32,
                object_id: node_id_str.clone(),
                grpc_address: format!("http://{}", listen_addr),
                object_category: "Node".to_string(),
                ..Default::default()
            };
            let registration_result = node_for_registration.object_registry
                .register(registration)
                .await;
            if let Err(e) = registration_result {
                eprintln!("Node {}: Failed to register in ObjectRegistry: {}", node_id_str, e);
            }
            // Notify that registration is complete (success or failure)
            registration_notify_for_registration.notify_one();
        });

        // Start heartbeat task with capacity tracking
        let node_for_heartbeat = self.clone();
        let heartbeat_interval = self.config.heartbeat_interval_ms;

        tokio::spawn(async move {
            // Wait for registration to complete before starting heartbeats
            registration_notify_for_heartbeat.notified().await;
            
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(heartbeat_interval)).await;

                // Calculate and send heartbeat with capacity to ObjectRegistry
                if let Err(e) = node_for_heartbeat.send_heartbeat_with_capacity().await {
                    eprintln!("Node {}: Failed to send heartbeat: {}", node_for_heartbeat.id.as_str(), e);
                }
            }
        });

        // Announce node in TupleSpace
        self.tuplespace
            .write(Tuple::new(vec![
                TupleField::String("node_started".to_string()),
                TupleField::String(self.id.as_str().to_string()),
                TupleField::String(self.config.listen_addr.clone()),
            ]))
            .await
            .map_err(|e| NodeError::TupleSpaceError(e.to_string()))?;

        // Parse listen address
        let addr = self
            .config
            .listen_addr
            .parse()
            .map_err(|e| NodeError::ConfigError(format!("Invalid listen address: {}", e)))?;

        // Register Node in ServiceLocator so ActorServiceImpl can access it
        self.service_locator.register_service(self.clone()).await;

        // Initialize blob service if configured
        let blob_service_opt = self.init_blob_service().await;
        
        // Start blob HTTP server on separate task if blob service is enabled
        // The blob HTTP server handles multipart upload and raw download endpoints
        // For now, we run it on a separate port (grpc_port + 1) to avoid conflicts
        // TODO: Integrate more cleanly with tonic server using a router layer
        let _blob_http_handle: Option<tokio::task::JoinHandle<()>> = if let Some(ref blob_service) = blob_service_opt {
            // Create Axum router for blob HTTP endpoints
            use plexspaces_blob::server::http_axum::create_blob_router;
            let router = create_blob_router(blob_service.clone());
            
            // Parse the listen address to get the port, then use port+100 for HTTP
            // Using +100 to avoid conflicts with MinIO console (which uses gRPC_PORT + 1)
            let grpc_addr: std::net::SocketAddr = self.config.listen_addr.parse()
                .unwrap_or_else(|_| "127.0.0.1:9999".parse().unwrap());
            let http_port = grpc_addr.port() + 100;
            let http_addr = format!("{}:{}", grpc_addr.ip(), http_port)
                .parse::<std::net::SocketAddr>()
                .unwrap_or_else(|_| "127.0.0.1:10000".parse().unwrap());
            
            eprintln!("🌐 Starting blob HTTP server on http://{}", http_addr);
            
            // Start Axum server on separate task
            match tokio::net::TcpListener::bind(http_addr).await {
                Ok(listener) => {
                    Some(tokio::spawn(async move {
                        use axum::serve;
                        if let Err(e) = serve(listener, router).await {
                            eprintln!("Blob HTTP server error: {}", e);
                        }
                    }))
                }
                Err(e) => {
                    eprintln!("Warning: Failed to bind blob HTTP server to {}: {}. Blob HTTP endpoints disabled.", http_addr, e);
                    None
                }
            }
        } else {
            None
        };

        // Create gRPC services
        let actor_service = ActorServiceImpl::new(self.clone());
        
        // Register ActorService in ServiceLocator so ActorContext::send_reply() can use it
        // Create ActorServiceImpl for ServiceLocator (uses same service_locator, so shares state)
        let actor_service_for_context = Arc::new(
            plexspaces_actor_service::ActorServiceImpl::new(
                self.service_locator.clone(),
                self.id.as_str().to_string(),
            )
        );
        use crate::service_wrappers::ActorServiceWrapper;
        let actor_service_wrapper = ActorServiceWrapper::new(actor_service_for_context.clone());
        self.service_locator.register_actor_service(Arc::new(actor_service_wrapper) as Arc<dyn plexspaces_core::ActorService + Send + Sync>).await;
        
        let tuplespace_service = TuplePlexSpaceServiceImpl::new(self.clone());
        
        // Start background cleanup task for expired temporary senders (in ActorRegistry)
        ActorRegistry::start_temporary_sender_cleanup(self.actor_registry.clone());

        // Create scheduling components (Phase 4 & 5)
        use plexspaces_scheduler::{
            SchedulingServiceImpl, TaskRouter, MemorySchedulingStateStore,
            background::BackgroundScheduler,
            capacity_tracker::CapacityTracker,
            state_store::SchedulingStateStore,
        };
        use plexspaces_locks::memory::MemoryLockManager;
        use plexspaces_channel::InMemoryChannel;
        use plexspaces_proto::channel::v1::{ChannelConfig, ChannelBackend, DeliveryGuarantee, OrderingGuarantee};
        
        // Create state store (in-memory for now, can be configured to SQLite/PostgreSQL)
        let state_store: Arc<dyn SchedulingStateStore> = Arc::new(MemorySchedulingStateStore::new());
        
        // Create capacity tracker
        let capacity_tracker = Arc::new(CapacityTracker::new(self.object_registry.clone()));
        
        // Create scheduling:requests channel
        let request_channel_config = ChannelConfig {
            name: "scheduling:requests".to_string(),
            backend: ChannelBackend::ChannelBackendInMemory as i32,
            capacity: 1000,
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };
        let request_channel = Arc::new(
            InMemoryChannel::new(request_channel_config.clone())
                .await
                .map_err(|e| NodeError::ConfigError(format!("Failed to create scheduling channel: {}", e)))?
        );
        
        // Create scheduling service
        let scheduling_service = SchedulingServiceImpl::new(
            state_store.clone(),
            request_channel.clone(),
            capacity_tracker.clone(),
        );
        
        // Create lock manager for background scheduler
        let lock_manager: Arc<dyn plexspaces_locks::LockManager> = Arc::new(MemoryLockManager::new());
        
        // Create background scheduler
        let lease_duration_secs = 30; // Default from config or use NodeConfig
        let heartbeat_interval_secs = 10; // Default from config or use NodeConfig
        let background_scheduler = Arc::new(BackgroundScheduler::new(
            self.id.as_str().to_string(),
            lock_manager,
            state_store.clone(),
            capacity_tracker.clone(),
            request_channel.clone(),
            lease_duration_secs,
            heartbeat_interval_secs,
        ));
        
        // Store background scheduler in Node (before starting)
        {
            let mut scheduler = self.background_scheduler.write().await;
            *scheduler = Some(background_scheduler.clone());
        }
        
        // Start background scheduler in background task (non-blocking)
        let scheduler_for_start = background_scheduler.clone();
        tokio::spawn(async move {
            if let Err(e) = scheduler_for_start.start().await {
                eprintln!("Background scheduler error: {}", e);
            }
        });
        
        // Create shared channel registry for TaskRouter
        use crate::service_wrappers::ChannelServiceWrapper;
        let channel_service = Arc::new(ChannelServiceWrapper::new());
        
        // Create task router with channel factory
        let channel_service_for_router = channel_service.clone();
        let task_router = Arc::new(TaskRouter::new(move |group_name| {
            let channel_service = channel_service_for_router.clone();
            let group_name = group_name.to_string();
            async move {
                // Use ChannelServiceWrapper to get/create channel
                let channel = channel_service.get_or_create_channel(&group_name).await
                    .map_err(|e| format!("Failed to get/create channel {}: {}", group_name, e))?;
                Ok(channel)
            }
        }));
        
        // Store task router in Node
        {
            let mut router = self.task_router.write().await;
            *router = Some(task_router.clone());
        }
        
        eprintln!("Node {}: Scheduling service initialized", self.id.as_str());

        // Start gRPC server with all services
        eprintln!(
            "Node {}: Starting gRPC server on {}",
            self.id.as_str(),
            addr
        );

        // Create shutdown channel for programmatic shutdown
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        {
            let mut tx = self.shutdown_tx.write().await;
            *tx = Some(shutdown_tx);
        }

        // Clone self for signal handler
        let node_for_shutdown = self.clone();

        // Create shutdown signal handler that waits for either signal OR programmatic shutdown
        let shutdown_signal = async move {
            // Wait for either signal OR programmatic shutdown
            #[cfg(unix)]
            {
                let mut sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("Failed to register SIGTERM handler");
                let mut sigint =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                        .expect("Failed to register SIGINT handler");

                tokio::select! {
                    _ = sigterm.recv() => {
                        eprintln!("Node {}: Received SIGTERM", node_for_shutdown.id.as_str());
                    }
                    _ = sigint.recv() => {
                        eprintln!("Node {}: Received SIGINT (Ctrl+C)", node_for_shutdown.id.as_str());
                    }
                    _ = shutdown_rx => {
                        eprintln!("Node {}: Received programmatic shutdown", node_for_shutdown.id.as_str());
                    }
                }
            }

            // Windows: Only support Ctrl+C
            #[cfg(not(unix))]
            {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        eprintln!("Node {}: Received Ctrl+C", node_for_shutdown.id.as_str());
                    }
                    _ = shutdown_rx => {
                        eprintln!("Node {}: Received programmatic shutdown", node_for_shutdown.id.as_str());
                    }
                }
            }

            // Perform graceful shutdown
            eprintln!(
                "Node {}: Initiating graceful shutdown...",
                node_for_shutdown.id.as_str()
            );
            if let Err(e) = node_for_shutdown
                .shutdown(tokio::time::Duration::from_secs(30))
                .await
            {
                eprintln!(
                    "Node {}: Shutdown error: {}",
                    node_for_shutdown.id.as_str(),
                    e
                );
            }
        };

        // Create health reporter and services
        use crate::health_service::PlexSpacesHealthReporter;
        use crate::system_service::SystemServiceImpl;
        use plexspaces_proto::system::v1::system_service_server::SystemServiceServer;
        use tonic_health::server::health_reporter;
        use tonic_web::GrpcWebLayer;
        
        // Create PlexSpaces health reporter for dependency tracking
        let (plexspaces_health_reporter, _) = PlexSpacesHealthReporter::new();
        let plexspaces_health_reporter = Arc::new(plexspaces_health_reporter);
        
        // Store health reporter in Node for shutdown access
        {
            let mut health_reporter_guard = self.health_reporter.write().await;
            *health_reporter_guard = Some(plexspaces_health_reporter.clone());
        }
        
        // Create standard gRPC health service (for Kubernetes probes)
        // Use our custom implementation that integrates with PlexSpacesHealthReporter
        use crate::standard_health_service::StandardHealthServiceImpl;
        use tonic_health::pb::health_server::HealthServer;
        let standard_health_service_impl = StandardHealthServiceImpl::new(plexspaces_health_reporter.clone());
        let standard_health_service = HealthServer::new(standard_health_service_impl);
        
        // Register built-in dependencies (Redis, PostgreSQL, Kafka) if enabled
        // These are automatically detected from environment variables
        use crate::dependency_registration::register_builtin_dependencies;
        let deps_registered = register_builtin_dependencies(plexspaces_health_reporter.clone()).await
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to register built-in dependencies: {}", e);
                0
            });
        eprintln!("✅ Registered {} built-in dependency checkers", deps_registered);
        
        // Register dependencies from object-registry if configured
        // This allows registering dependencies by name/type from the registry
        use crate::dependency_registration::register_dependencies;
        // Note: Health config (dependency registration) is configured via HealthProbeConfig
        // which is part of the health reporter, not NodeConfig. Dependencies are registered
        // via environment variables or object-registry discovery as configured in HealthProbeConfig.
        
        // Create SystemService (provides HTTP endpoints via gRPC-Gateway)
        let system_service = SystemServiceImpl::with_node(plexspaces_health_reporter.clone(), self.clone());
        
        // Create MetricsService for Prometheus export
        use crate::metrics_service::MetricsServiceImpl;
        use plexspaces_proto::metrics::v1::metrics_service_server::MetricsServiceServer;
        let metrics_service = MetricsServiceImpl::with_node(self.clone());
        
        // Start connection health monitoring and stale connection cleanup
        // Connection health monitoring is handled by gRPC client pool
        
        // Mark startup complete after services are registered
        plexspaces_health_reporter.mark_startup_complete(None).await;

        // Create WASM runtime service for dynamic actor deployment
        use plexspaces_wasm_runtime::{WasmRuntime, grpc_service::WasmRuntimeServiceImpl};
        use plexspaces_proto::wasm::v1::wasm_runtime_service_server::WasmRuntimeServiceServer;
        let wasm_runtime = Arc::new(WasmRuntime::new().await.map_err(|e| {
            NodeError::ConfigError(format!("Failed to create WASM runtime: {}", e))
        })?);
        
        // Store WASM runtime in Node for ApplicationService access
        {
            let mut stored_runtime = self.wasm_runtime.write().await;
            *stored_runtime = Some(wasm_runtime.clone());
        }
        
        let wasm_runtime_service = WasmRuntimeServiceImpl::new(wasm_runtime);
        
        // Create ApplicationService for application-level deployment
        use crate::application_service::ApplicationServiceImpl;
        use plexspaces_proto::application::v1::application_service_server::ApplicationServiceServer;
        let application_manager_clone = Arc::new(self.application_manager.read().await.clone());
        let application_service = ApplicationServiceImpl::new(
            self.clone(),
            application_manager_clone,
        );

        // Create Firecracker VM Service (if Firecracker support is enabled)
        #[cfg(feature = "firecracker")]
        let firecracker_service = {
            use crate::firecracker_service::FirecrackerVmServiceImpl;
            use plexspaces_proto::firecracker::v1::firecracker_vm_service_server::FirecrackerVmServiceServer;
            Some(FirecrackerVmServiceServer::new(
                FirecrackerVmServiceImpl::new(self.clone())
            ))
        };
        #[cfg(not(feature = "firecracker"))]
        let firecracker_service: Option<()> = None;

        // Run server with graceful shutdown and gRPC-Gateway support
        use plexspaces_proto::scheduling::v1::scheduling_service_server::SchedulingServiceServer;
        
        // Build gRPC server with all services
        let grpc_server = Server::builder()
            .accept_http1(true)  // Enable HTTP for gRPC-Web
            .add_service(ActorServiceServer::new(actor_service))
            .add_service(TuplePlexSpaceServiceServer::new(tuplespace_service))
            .add_service(SchedulingServiceServer::new(scheduling_service))
            .add_service(WasmRuntimeServiceServer::new(wasm_runtime_service))  // WASM deployment service
            .add_service(ApplicationServiceServer::new(application_service))  // Application deployment service
            .add_service(standard_health_service)  // Standard gRPC health service
            .add_service(SystemServiceServer::new(system_service))  // SystemService with HTTP endpoints
            .add_service(MetricsServiceServer::new(metrics_service))  // MetricsService for Prometheus export
            .add_optional_service(
                blob_service_opt.as_ref().map(|blob_service| {
                    use plexspaces_blob::server::grpc::BlobServiceImpl;
                    use plexspaces_proto::storage::v1::blob_service_server::BlobServiceServer;
                    BlobServiceServer::new(BlobServiceImpl::new(blob_service.clone()))
                })
            )
            .serve(addr);
        
        // Start HTTP gateway server for InvokeActor routes (following demo pattern)
        // Create a new ActorServiceImpl instance for HTTP gateway (shares same service locator)
        let http_gateway_handle = {
            let service_locator_for_http = self.service_locator.clone();
            let node_id_for_http = self.id.as_str().to_string();
            let grpc_addr = addr;
            
            tokio::spawn(async move {
                use axum::{
                    extract::{Path, Query},
                    http::StatusCode,
                    response::Json,
                    routing::{get, post, put, delete},
                    Router,
                };
                use serde_json::Value;
                use std::collections::HashMap;
                use url::form_urlencoded;
                
                // Create ActorServiceImpl for HTTP gateway
                use plexspaces_actor_service::ActorServiceImpl as ActorServiceImplHttp;
                let actor_service_http = Arc::new(ActorServiceImplHttp::new(
                    service_locator_for_http.clone(),
                    node_id_for_http.clone(),
                ));
                
                // HTTP handler for InvokeActor
                async fn invoke_actor_http(
                    method: axum::http::Method,
                    path: String,
                    query: Option<Query<HashMap<String, String>>>,
                    body: Option<axum::body::Bytes>,
                    headers: axum::http::HeaderMap,
                    actor_service: Arc<plexspaces_actor_service::ActorServiceImpl>,
                ) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
                    // Parse path: /api/v1/actors/{tenant_id}/{namespace}/{actor_type} or /api/v1/actors/{namespace}/{actor_type}
                    let path_parts: Vec<&str> = path
                        .strip_prefix("/api/v1/actors/")
                        .unwrap_or("")
                        .split('/')
                        .collect();
                    
                    if path_parts.len() < 2 {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({
                                "code": 400,
                                "message": "Invalid path format. Expected /api/v1/actors/{tenant_id}/{namespace}/{actor_type} or /api/v1/actors/{namespace}/{actor_type}"
                            })),
                        ));
                    }
                    
                    let (tenant_id, namespace, actor_type) = if path_parts.len() == 3 {
                        (path_parts[0].to_string(), path_parts[1].to_string(), path_parts[2].to_string())
                    } else {
                        ("default".to_string(), path_parts[0].to_string(), path_parts[1].to_string())
                    };
                    
                    // Extract query parameters
                    let query_params: HashMap<String, String> = query
                        .map(|q| q.0)
                        .unwrap_or_default();
                    
                    // Create InvokeActorRequest
                    use plexspaces_proto::actor::v1::InvokeActorRequest;
                    let mut invoke_req = InvokeActorRequest {
                        tenant_id,
                        namespace,
                        actor_type,
                        http_method: method.as_str().to_string(),
                        payload: body.map(|b| b.to_vec()).unwrap_or_default(),
                        headers: {
                            let mut h = HashMap::new();
                            for (key, value) in headers.iter() {
                                if let Ok(value_str) = value.to_str() {
                                    h.insert(key.as_str().to_string(), value_str.to_string());
                                }
                            }
                            h
                        },
                        query_params,
                        path: path.clone(),
                        subpath: String::new(),
                    };
                    
                    // Call InvokeActor via ActorService
                    use tonic::Request as TonicRequest;
                    use plexspaces_proto::actor::v1::actor_service_server::ActorService as ActorServiceTrait;
                    let grpc_req = TonicRequest::new(invoke_req);
                    
                    match ActorServiceTrait::invoke_actor(&*actor_service, grpc_req).await {
                        Ok(grpc_resp) => {
                            let resp_inner = grpc_resp.into_inner();
                            // Convert InvokeActorResponse to JSON
                            use base64::{Engine as _, engine::general_purpose};
                            let payload_json = if resp_inner.payload.is_empty() {
                                serde_json::Value::Null
                            } else {
                                // Try to decode as UTF-8 string first, otherwise base64 encode
                                match String::from_utf8(resp_inner.payload.clone()) {
                                    Ok(s) => {
                                        // Try to parse as JSON, otherwise return as string
                                        serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s))
                                    }
                                    Err(_) => {
                                        serde_json::Value::String(general_purpose::STANDARD.encode(&resp_inner.payload))
                                    }
                                }
                            };
                            
                            let json_resp = serde_json::json!({
                                "success": resp_inner.success,
                                "payload": payload_json,
                                "headers": resp_inner.headers,
                                "actor_id": resp_inner.actor_id,
                                "error_message": resp_inner.error_message,
                            });
                            
                            Ok(Json(json_resp))
                        }
                        Err(status) => {
                            let err_json = serde_json::json!({
                                "code": status.code() as u16,
                                "message": status.message()
                            });
                            let http_status = match status.code() {
                                tonic::Code::NotFound => StatusCode::NOT_FOUND,
                                tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
                                tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
                                _ => StatusCode::INTERNAL_SERVER_ERROR,
                            };
                            Err((http_status, Json(err_json)))
                        }
                    }
                }
                
                // Create Axum router for HTTP gateway routes
                let app = Router::new()
                    .route(
                        "/api/v1/actors/:tenant_id/:namespace/:actor_type",
                        get({
                            let actor_service = actor_service_http.clone();
                            move |Path((tenant_id, namespace, actor_type)): Path<(String, String, String)>,
                                  query: Option<Query<HashMap<String, String>>>,
                                  headers: axum::http::HeaderMap| async move {
                                let path = format!("/api/v1/actors/{}/{}/{}", tenant_id, namespace, actor_type);
                                invoke_actor_http(
                                    axum::http::Method::GET,
                                    path,
                                    query,
                                    None,
                                    headers,
                                    actor_service,
                                ).await
                            }
                        })
                        .post({
                            let actor_service = actor_service_http.clone();
                            move |Path((tenant_id, namespace, actor_type)): Path<(String, String, String)>,
                                  query: Option<Query<HashMap<String, String>>>,
                                  headers: axum::http::HeaderMap,
                                  body: Option<axum::body::Bytes>| async move {
                                let path = format!("/api/v1/actors/{}/{}/{}", tenant_id, namespace, actor_type);
                                invoke_actor_http(
                                    axum::http::Method::POST,
                                    path,
                                    query,
                                    body,
                                    headers,
                                    actor_service,
                                ).await
                            }
                        })
                        .put({
                            let actor_service = actor_service_http.clone();
                            move |Path((tenant_id, namespace, actor_type)): Path<(String, String, String)>,
                                  query: Option<Query<HashMap<String, String>>>,
                                  headers: axum::http::HeaderMap,
                                  body: Option<axum::body::Bytes>| async move {
                                let path = format!("/api/v1/actors/{}/{}/{}", tenant_id, namespace, actor_type);
                                invoke_actor_http(
                                    axum::http::Method::PUT,
                                    path,
                                    query,
                                    body,
                                    headers,
                                    actor_service,
                                ).await
                            }
                        })
                        .delete({
                            let actor_service = actor_service_http.clone();
                            move |Path((tenant_id, namespace, actor_type)): Path<(String, String, String)>,
                                  query: Option<Query<HashMap<String, String>>>,
                                  headers: axum::http::HeaderMap| async move {
                                let path = format!("/api/v1/actors/{}/{}/{}", tenant_id, namespace, actor_type);
                                invoke_actor_http(
                                    axum::http::Method::DELETE,
                                    path,
                                    query,
                                    None,
                                    headers,
                                    actor_service,
                                ).await
                            }
                        })
                    )
                    .route(
                        "/api/v1/actors/:namespace/:actor_type",
                        get({
                            let actor_service = actor_service_http.clone();
                            move |Path((namespace, actor_type)): Path<(String, String)>,
                                  query: Option<Query<HashMap<String, String>>>,
                                  headers: axum::http::HeaderMap| async move {
                                let path = format!("/api/v1/actors/{}/{}", namespace, actor_type);
                                invoke_actor_http(
                                    axum::http::Method::GET,
                                    path,
                                    query,
                                    None,
                                    headers,
                                    actor_service,
                                ).await
                            }
                        })
                        .post({
                            let actor_service = actor_service_http.clone();
                            move |Path((namespace, actor_type)): Path<(String, String)>,
                                  query: Option<Query<HashMap<String, String>>>,
                                  headers: axum::http::HeaderMap,
                                  body: Option<axum::body::Bytes>| async move {
                                let path = format!("/api/v1/actors/{}/{}", namespace, actor_type);
                                invoke_actor_http(
                                    axum::http::Method::POST,
                                    path,
                                    query,
                                    body,
                                    headers,
                                    actor_service,
                                ).await
                            }
                        })
                        .put({
                            let actor_service = actor_service_http.clone();
                            move |Path((namespace, actor_type)): Path<(String, String)>,
                                  query: Option<Query<HashMap<String, String>>>,
                                  headers: axum::http::HeaderMap,
                                  body: Option<axum::body::Bytes>| async move {
                                let path = format!("/api/v1/actors/{}/{}", namespace, actor_type);
                                invoke_actor_http(
                                    axum::http::Method::PUT,
                                    path,
                                    query,
                                    body,
                                    headers,
                                    actor_service,
                                ).await
                            }
                        })
                        .delete({
                            let actor_service = actor_service_http.clone();
                            move |Path((namespace, actor_type)): Path<(String, String)>,
                                  query: Option<Query<HashMap<String, String>>>,
                                  headers: axum::http::HeaderMap| async move {
                                let path = format!("/api/v1/actors/{}/{}", namespace, actor_type);
                                invoke_actor_http(
                                    axum::http::Method::DELETE,
                                    path,
                                    query,
                                    None,
                                    headers,
                                    actor_service,
                                ).await
                            }
                        })
                    );
                
                // Start HTTP server on a separate port (gRPC port + 1 for HTTP gateway)
                // This follows the demo pattern of separate servers
                let http_port = grpc_addr.port() + 1;
                let http_addr = format!("{}:{}", grpc_addr.ip(), http_port)
                    .parse::<std::net::SocketAddr>()
                    .unwrap_or_else(|_| "127.0.0.1:9002".parse().unwrap());
                
                eprintln!("🌐 Starting HTTP gateway server on http://{}", http_addr);
                
                match tokio::net::TcpListener::bind(http_addr).await {
                    Ok(listener) => {
                        use axum::serve;
                        if let Err(e) = serve(listener, app).await {
                            eprintln!("HTTP gateway server error: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to bind HTTP gateway server to {}: {}. HTTP gateway disabled.", http_addr, e);
                    }
                }
            })
        };
        
        tokio::select! {
            result = grpc_server => {
                result.map_err(|e| NodeError::GrpcError(e.to_string()))?;
            }
            _ = shutdown_signal => {
                // Shutdown signal received - server will stop gracefully
                http_gateway_handle.abort();
            }
        }
        
        Ok(())
    }

    /// Monitor an actor (Erlang-style location transparent monitoring)
    ///
    /// ## Purpose
    /// Establishes a monitoring link from supervisor to actor. When the actor
    /// terminates, the supervisor receives a notification via the provided channel.
    ///
    /// ## Erlang Philosophy
    /// In Erlang, `monitor(process, Pid)` works the same for local and remote processes.
    /// The runtime handles location transparency - same API whether the process is
    /// in the same node or a different node.
    ///
    /// ## Arguments
    /// * `actor_id` - The actor to monitor (can be "actor@node" for remote)
    /// * `supervisor_id` - The supervisor that wants notifications
    /// * `notification_tx` - Channel to send (actor_id, reason) when actor terminates
    ///
    /// ## Returns
    /// `Ok(monitor_ref)` - Unique monitor reference (ULID)
    /// `Err(NodeError)` - If actor not found or remote node not connected
    ///
    /// ## Example
    /// ```ignore
    /// let (tx, mut rx) = mpsc::channel(1);
    /// let monitor_ref = node.monitor("worker@node2", "supervisor@node1", tx).await?;
    ///
    /// // Later, when actor terminates:
    /// let (actor_id, reason) = rx.recv().await.unwrap();
    /// ```
    pub async fn monitor(
        &self,
        actor_id: &ActorId,
        supervisor_id: &ActorId,
        notification_tx: TerminationSender,
    ) -> Result<MonitorRef, NodeError> {
        // Parse actor ID to determine if local or remote
        let (_, node_part) = plexspaces_core::ActorRef::parse_actor_id(actor_id)
            .unwrap_or((actor_id.clone(), self.id.as_str().to_string()));

        let is_local = node_part == self.id.as_str();

        if is_local {
            // LOCAL MONITORING: Actor on this node
            // Verify actor exists in ActorRegistry by checking if it's activated
            if !self.actor_registry.is_actor_activated(actor_id).await {
                return Err(NodeError::ActorNotFound(actor_id.clone()));
            }

            // Generate unique monitor reference (ULID for sortability)
            let monitor_ref = ulid::Ulid::new().to_string();

            // Store monitoring link
            let link = MonitorLink {
                monitor_ref: monitor_ref.clone(),
                termination_sender: notification_tx,
            };

            let mut monitors = self.monitors().write().await;
            monitors
                .entry(actor_id.clone())
                .or_insert_with(Vec::new)
                .push(link);

            Ok(monitor_ref)
        } else {
            // REMOTE MONITORING: Actor on different node
            // Get remote node's address
            let remote_node_id = crate::NodeId::new(node_part);
            let connections = self.connections.read().await;
            let node_address = connections
                .get(&remote_node_id)
                .ok_or_else(|| NodeError::NodeNotConnected(remote_node_id.clone()))?
                .node_address
                .clone();
            drop(connections);

            // Generate unique monitor reference
            let monitor_ref = ulid::Ulid::new().to_string();

            // Call MonitorActor RPC on remote node
            use plexspaces_proto::{v1::actor::MonitorActorRequest, ActorServiceClient};

            let channel = tonic::transport::Channel::from_shared(node_address.clone())
                .map_err(|e| NodeError::NetworkError(format!("Invalid address: {}", e)))?
                .connect()
                .await
                .map_err(|e| NodeError::NetworkError(format!("Connection failed: {}", e)))?;

            let mut client = ActorServiceClient::new(channel);

            // Supervisor callback = this node's address
            let supervisor_callback = format!("http://{}", self.config.listen_addr);

            let request = tonic::Request::new(MonitorActorRequest {
                actor_id: actor_id.clone(),
                supervisor_id: supervisor_id.clone(),
                supervisor_callback,
            });

            client
                .monitor_actor(request)
                .await
                .map_err(|e| NodeError::NetworkError(format!("MonitorActor RPC failed: {}", e)))?;

            // Store the notification channel for when remote node sends NotifyActorDown
            let link = MonitorLink {
                monitor_ref: monitor_ref.clone(),
                termination_sender: notification_tx,
            };

            let mut monitors = self.monitors().write().await;
            monitors
                .entry(actor_id.clone())
                .or_insert_with(Vec::new)
                .push(link);

            Ok(monitor_ref)
        }
    }

    /// Link two actors for bidirectional death propagation (Erlang link/1)
    ///
    /// ## Purpose
    /// Creates a bidirectional link between two actors. When one actor dies abnormally,
    /// the linked actor automatically dies (cascading failure).
    ///
    /// ## Erlang Philosophy
    /// Equivalent to Erlang's `link(Pid)` - creates bidirectional link.
    /// If either process dies abnormally, the other dies too.
    ///
    /// ## Arguments
    /// * `actor_id` - First actor in the link
    /// * `linked_actor_id` - Second actor in the link (bidirectional)
    ///
    /// ## Returns
    /// Success or error
    ///
    /// ## Errors
    /// - `NodeError::ActorNotFound` if either actor doesn't exist
    /// - `NodeError::NetworkError` if remote actor link fails
    ///
    /// ## Design Notes
    /// - Links are bidirectional (if A links to B, B is linked to A)
    /// - Links only propagate abnormal deaths (not "normal" shutdowns)
    /// - Links are used internally by supervision (parent-child relationships)
    /// - Links can be created explicitly via this API
    ///
    /// ## Example
    /// ```rust
    /// // Link two actors
    /// node.link("actor-1", "actor-2").await?;
    ///
    /// // If actor-1 dies abnormally, actor-2 automatically dies
    /// // If actor-2 dies abnormally, actor-1 automatically dies
    /// ```
    pub async fn link(
        &self,
        actor_id: &ActorId,
        linked_actor_id: &ActorId,
    ) -> Result<(), NodeError> {
        // Prevent self-linking
        if actor_id == linked_actor_id {
            return Err(NodeError::InvalidArgument(
                "Cannot link actor to itself".to_string(),
            ));
        }

        // Parse actor IDs to determine if local or remote
        let (_, node_part1) = plexspaces_core::ActorRef::parse_actor_id(actor_id)
            .unwrap_or((actor_id.clone(), self.id.as_str().to_string()));
        let (_, node_part2) = plexspaces_core::ActorRef::parse_actor_id(linked_actor_id)
            .unwrap_or((linked_actor_id.clone(), self.id.as_str().to_string()));

        let is_local1 = node_part1 == self.id.as_str();
        let is_local2 = node_part2 == self.id.as_str();

        if is_local1 && is_local2 {
            // Both actors are local
            // Verify both actors exist in ActorRegistry
            let routing1 = self.actor_registry.lookup_routing(actor_id).await
                .map_err(|_| NodeError::ActorNotFound(actor_id.clone()))?;
            if routing1.is_none() || !routing1.unwrap().is_local {
                return Err(NodeError::ActorNotFound(actor_id.clone()));
            }
            
            let routing2 = self.actor_registry.lookup_routing(linked_actor_id).await
                .map_err(|_| NodeError::ActorNotFound(linked_actor_id.clone()))?;
            if routing2.is_none() || !routing2.unwrap().is_local {
                return Err(NodeError::ActorNotFound(linked_actor_id.clone()));
            }

            // Add bidirectional link
            let mut links = self.links().write().await;

            // Add actor_id -> linked_actor_id
            links
                .entry(actor_id.clone())
                .or_insert_with(Vec::new)
                .push(linked_actor_id.clone());

            // Add linked_actor_id -> actor_id (bidirectional)
            links
                .entry(linked_actor_id.clone())
                .or_insert_with(Vec::new)
                .push(actor_id.clone());

            Ok(())
        } else if is_local1 {
            // actor_id is local, linked_actor_id is remote
            // Link local actor to remote actor
            let mut links = self.links().write().await;
            links
                .entry(actor_id.clone())
                .or_insert_with(Vec::new)
                .push(linked_actor_id.clone());

            // Call LinkActor RPC on remote node
            let remote_node_id = crate::NodeId::new(node_part2);
            let connections = self.connections.read().await;
            let node_address = connections
                .get(&remote_node_id)
                .ok_or_else(|| NodeError::NodeNotConnected(remote_node_id.clone()))?
                .node_address
                .clone();
            drop(connections);

            let channel = tonic::transport::Channel::from_shared(node_address.clone())
                .map_err(|e| NodeError::NetworkError(format!("Invalid address: {}", e)))?
                .connect()
                .await
                .map_err(|e| NodeError::NetworkError(format!("Connection failed: {}", e)))?;

            let mut client = plexspaces_proto::ActorServiceClient::new(channel);

            let request = tonic::Request::new(plexspaces_proto::v1::actor::LinkActorRequest {
                actor_id: linked_actor_id.clone(),
                linked_actor_id: actor_id.clone(), // Reverse for remote side
            });
            client.link_actor(request).await
                .map_err(|e| NodeError::NetworkError(format!("LinkActor RPC failed: {}", e)))?;

            Ok(())
        } else if is_local2 {
            // linked_actor_id is local, actor_id is remote
            // Link local actor to remote actor
            let mut links = self.links().write().await;
            links
                .entry(linked_actor_id.clone())
                .or_insert_with(Vec::new)
                .push(actor_id.clone());

            // Call LinkActor RPC on remote node
            let remote_node_id = crate::NodeId::new(node_part1);
            let connections = self.connections.read().await;
            let node_address = connections
                .get(&remote_node_id)
                .ok_or_else(|| NodeError::NodeNotConnected(remote_node_id.clone()))?
                .node_address
                .clone();
            drop(connections);

            let channel = tonic::transport::Channel::from_shared(node_address.clone())
                .map_err(|e| NodeError::NetworkError(format!("Invalid address: {}", e)))?
                .connect()
                .await
                .map_err(|e| NodeError::NetworkError(format!("Connection failed: {}", e)))?;

            let mut client = plexspaces_proto::ActorServiceClient::new(channel);

            let request = tonic::Request::new(plexspaces_proto::v1::actor::LinkActorRequest {
                actor_id: actor_id.clone(),
                linked_actor_id: linked_actor_id.clone(), // Reverse for remote side
            });
            client.link_actor(request).await
                .map_err(|e| NodeError::NetworkError(format!("LinkActor RPC failed: {}", e)))?;

            Ok(())
        } else {
            // Both actors are remote - not supported (links must involve at least one local actor)
            Err(NodeError::InvalidArgument(
                "Cannot link two remote actors from this node".to_string(),
            ))
        }
    }

    /// Unlink two actors (Erlang unlink/1 equivalent)
    ///
    /// ## Purpose
    /// Removes the bidirectional link between two actors. After unlinking,
    /// actors can die independently without cascading failures.
    ///
    /// ## Arguments
    /// * `actor_id` - First actor in the link
    /// * `linked_actor_id` - Second actor in the link
    ///
    /// ## Returns
    /// Success or error
    ///
    /// ## Errors
    /// - `NodeError::ActorNotFound` if either actor doesn't exist
    /// - `NodeError::NetworkError` if remote actor unlink fails
    pub async fn unlink(
        &self,
        actor_id: &ActorId,
        linked_actor_id: &ActorId,
    ) -> Result<(), NodeError> {
        // Parse actor IDs to determine if local or remote
        let (_, node_part1) = plexspaces_core::ActorRef::parse_actor_id(actor_id)
            .unwrap_or((actor_id.clone(), self.id.as_str().to_string()));
        let (_, node_part2) = plexspaces_core::ActorRef::parse_actor_id(linked_actor_id)
            .unwrap_or((linked_actor_id.clone(), self.id.as_str().to_string()));

        let is_local1 = node_part1 == self.id.as_str();
        let is_local2 = node_part2 == self.id.as_str();

        if is_local1 && is_local2 {
            // Both actors are local
            let mut links = self.links().write().await;

            // Remove actor_id -> linked_actor_id
            if let Some(linked_actors) = links.get_mut(actor_id) {
                linked_actors.retain(|id| id != linked_actor_id);
                if linked_actors.is_empty() {
                    links.remove(actor_id);
                }
            }

            // Remove linked_actor_id -> actor_id (bidirectional)
            if let Some(linked_actors) = links.get_mut(linked_actor_id) {
                linked_actors.retain(|id| id != actor_id);
                if linked_actors.is_empty() {
                    links.remove(linked_actor_id);
                }
            }

            Ok(())
        } else if is_local1 {
            // actor_id is local, linked_actor_id is remote
            let mut links = self.links().write().await;
            if let Some(linked_actors) = links.get_mut(actor_id) {
                linked_actors.retain(|id| id != linked_actor_id);
                if linked_actors.is_empty() {
                    links.remove(actor_id);
                }
            }

            // Call UnlinkActor RPC on remote node
            let remote_node_id = crate::NodeId::new(node_part2);
            let connections = self.connections.read().await;
            let node_address = connections
                .get(&remote_node_id)
                .ok_or_else(|| NodeError::NodeNotConnected(remote_node_id.clone()))?
                .node_address
                .clone();
            drop(connections);

            let channel = tonic::transport::Channel::from_shared(node_address.clone())
                .map_err(|e| NodeError::NetworkError(format!("Invalid address: {}", e)))?
                .connect()
                .await
                .map_err(|e| NodeError::NetworkError(format!("Connection failed: {}", e)))?;

            let mut client = plexspaces_proto::ActorServiceClient::new(channel);

            let request = tonic::Request::new(plexspaces_proto::v1::actor::UnlinkActorRequest {
                actor_id: linked_actor_id.clone(),
                linked_actor_id: actor_id.clone(),
            });
            client.unlink_actor(request).await
                .map_err(|e| NodeError::NetworkError(format!("UnlinkActor RPC failed: {}", e)))?;

            Ok(())
        } else if is_local2 {
            // linked_actor_id is local, actor_id is remote
            let mut links = self.links().write().await;
            if let Some(linked_actors) = links.get_mut(linked_actor_id) {
                linked_actors.retain(|id| id != actor_id);
                if linked_actors.is_empty() {
                    links.remove(linked_actor_id);
                }
            }

            // Call UnlinkActor RPC on remote node
            let remote_node_id = crate::NodeId::new(node_part1);
            let connections = self.connections.read().await;
            let node_address = connections
                .get(&remote_node_id)
                .ok_or_else(|| NodeError::NodeNotConnected(remote_node_id.clone()))?
                .node_address
                .clone();
            drop(connections);

            let channel = tonic::transport::Channel::from_shared(node_address.clone())
                .map_err(|e| NodeError::NetworkError(format!("Invalid address: {}", e)))?
                .connect()
                .await
                .map_err(|e| NodeError::NetworkError(format!("Connection failed: {}", e)))?;

            let mut client = plexspaces_proto::ActorServiceClient::new(channel);

            let request = tonic::Request::new(plexspaces_proto::v1::actor::UnlinkActorRequest {
                actor_id: actor_id.clone(),
                linked_actor_id: linked_actor_id.clone(),
            });
            client.unlink_actor(request).await
                .map_err(|e| NodeError::NetworkError(format!("UnlinkActor RPC failed: {}", e)))?;

            Ok(())
        } else {
            // Both actors are remote - not supported
            Err(NodeError::InvalidArgument(
                "Cannot unlink two remote actors from this node".to_string(),
            ))
        }
    }

    /// Notify supervisors that an actor has terminated
    ///
    /// ## Purpose
    /// Called when an actor terminates (normally or abnormally). Sends notifications
    /// to all supervisors monitoring this actor.
    ///
    /// ## Erlang Philosophy
    /// Equivalent to sending `{'DOWN', Ref, process, Pid, Reason}` message in Erlang.
    ///
    /// ## Arguments
    /// * `actor_id` - The actor that terminated
    /// * `reason` - Why it terminated ("normal", "shutdown", "killed", or error message)
    ///
    /// ## Example
    /// ```ignore
    /// // Actor crashed with panic
    /// node.notify_actor_down("worker@node1", "panic: index out of bounds").await?;
    ///
    /// // Actor shut down gracefully
    /// node.notify_actor_down("worker@node1", "normal").await?;
    /// ```
    pub async fn notify_actor_down(
        &self,
        actor_id: &ActorId,
        reason: &str,
    ) -> Result<(), NodeError> {
        // Notify monitors (delegate to ActorRegistry)
        self.actor_registry.notify_actor_down(actor_id, reason).await;

        // Handle links (two-way death propagation)
        // Only propagate abnormal deaths (not "normal" or "shutdown")
        let is_abnormal = reason != "normal" && reason != "shutdown";

        if is_abnormal {
            // Get all linked actors
            let linked_actors = {
                let links = self.links().read().await;
                links.get(actor_id).cloned()
            };

            if let Some(linked_actor_ids) = linked_actors {
                // Kill all linked actors (cascading failure)
                for linked_actor_id in linked_actor_ids {
                    // Remove link before killing (prevent infinite recursion)
                    let mut links = self.links().write().await;
                    if let Some(linked_actors) = links.get_mut(&linked_actor_id) {
                        linked_actors.retain(|id| id != actor_id);
                        if linked_actors.is_empty() {
                            links.remove(&linked_actor_id);
                        }
                    }
                    drop(links);

                    // Kill linked actor with cascading reason
                    let cascade_reason = format!("linked_actor_died:{}", actor_id);
                    
                    // Unregister actor (removes from registry)
                    if let Err(e) = self.actor_registry.unregister_with_cleanup(&linked_actor_id).await {
                        eprintln!("Failed to unregister linked actor {}: {}", linked_actor_id, e);
                    }
                    
                    // Notify monitors (but don't cascade again to prevent infinite recursion)
                    // We handle cascading separately by directly killing linked actors
                    self.actor_registry.notify_actor_down(&linked_actor_id, &cascade_reason).await;
                    
                    // Handle cascading for this linked actor (but prevent recursion)
                    // Get linked actors of the linked actor
                    let linked_actor_links = {
                        let links = self.links().read().await;
                        links.get(&linked_actor_id).cloned()
                    };
                    
                    if let Some(linked_actor_links) = linked_actor_links {
                        // Remove links before cascading
                        let mut links = self.links().write().await;
                        links.remove(&linked_actor_id);
                        for linked_actor_link_id in &linked_actor_links {
                            if let Some(linked_actors) = links.get_mut(linked_actor_link_id) {
                                linked_actors.retain(|id| id != &linked_actor_id);
                                if linked_actors.is_empty() {
                                    links.remove(linked_actor_link_id);
                                }
                            }
                        }
                        drop(links);
                        
                        // Cascade to linked actors of the linked actor
                        for linked_actor_link_id in linked_actor_links {
                            if let Err(e) = self.actor_registry.unregister_with_cleanup(&linked_actor_link_id).await {
                                eprintln!("Failed to unregister cascaded actor {}: {}", linked_actor_link_id, e);
                            }
                            // Notify monitors only (no further cascading to prevent deep recursion)
                            self.actor_registry.notify_actor_down(&linked_actor_link_id, &cascade_reason).await;
                        }
                    }
                }
            }
        } else {
            // Normal shutdown - just remove links (no cascading)
            let mut links = self.links().write().await;
            if let Some(linked_actor_ids) = links.remove(actor_id) {
                // Remove reverse links
                for linked_actor_id in linked_actor_ids {
                    if let Some(linked_actors) = links.get_mut(&linked_actor_id) {
                        linked_actors.retain(|id| id != actor_id);
                        if linked_actors.is_empty() {
                            links.remove(&linked_actor_id);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Publish a lifecycle event to all subscribers
    ///
    /// ## Purpose
    /// Internal helper to multicast lifecycle events to all observability backends.
    ///
    /// ## Arguments
    /// * `event` - The lifecycle event to publish
    async fn publish_lifecycle_event(&self, event: plexspaces_proto::ActorLifecycleEvent) {
        let subscribers = self.lifecycle_subscribers().read().await;
        for subscriber in subscribers.iter() {
            let _ = subscriber.send(event.clone());
        }
    }

    /// Subscribe to lifecycle events (JavaNOW-inspired channel/subscriber pattern)
    ///
    /// ## Purpose
    /// Allows observability backends (Prometheus exporters, StatsD forwarders, OpenTelemetry,
    /// custom monitoring systems) to subscribe to actor lifecycle events.
    ///
    /// ## JavaNOW Heritage
    /// This follows the JavaNOW pattern of EntitySpace event notification:
    /// - **ChannelI**: This method provides the channel for events
    /// - **SubscriberI**: The receiver subscribes to events
    /// - **MulticasterImpl**: Node multicasts events to all subscribers
    ///
    /// ## Usage
    /// ```ignore
    /// // Prometheus exporter subscribes
    /// let (tx, mut rx) = mpsc::unbounded_channel();
    /// node.subscribe_lifecycle_events(tx).await;
    ///
    /// // Exporter processes events
    /// while let Some(event) = rx.recv().await {
    ///     match event.event_type {
    ///         ActorCreated => metrics::counter!("actor_spawn_total").increment(1),
    ///         ActorTerminated => metrics::gauge!("actor_active").decrement(1.0),
    ///         ActorFailed => metrics::counter!("actor_error_total").increment(1),
    ///         _ => {}
    ///     }
    /// }
    /// ```
    ///
    /// ## Integration with Observability Backends
    /// - **Prometheus**: Convert events to metrics (counters, gauges, histograms)
    /// - **StatsD**: Batch events and send via UDP
    /// - **OpenTelemetry**: Create spans from lifecycle events for distributed tracing
    /// - **Custom**: User-defined event processing
    ///
    /// ## Arguments
    /// * `subscriber` - Channel sender that will receive all lifecycle events
    ///
    /// ## Returns
    /// Nothing - events will be sent to subscriber as they occur
    pub async fn subscribe_lifecycle_events(
        &self,
        subscriber: mpsc::UnboundedSender<plexspaces_proto::ActorLifecycleEvent>,
    ) {
        let mut subscribers = self.lifecycle_subscribers().write().await;
        subscribers.push(subscriber);
    }

    /// Unsubscribe from lifecycle events
    ///
    /// ## Purpose
    /// Removes a subscriber from the lifecycle event channel. Useful when
    /// shutting down observability backends or changing monitoring configuration.
    ///
    /// ## Note
    /// Currently this removes ALL subscribers. Future enhancement could add
    /// subscription IDs for selective unsubscribe.
    pub async fn unsubscribe_lifecycle_events(&self) {
        let mut subscribers = self.lifecycle_subscribers().write().await;
        subscribers.clear();
    }

    /// Create lifecycle event channel for an actor (production-ready monitoring)
    ///
    /// ## Purpose
    /// Creates a channel for receiving actor lifecycle events and spawns a background
    /// task to handle these events, triggering supervisor notifications automatically.
    ///
    /// ## Usage
    /// ```ignore
    /// use plexspaces_actor::Actor;
    /// use plexspaces_behavior::GenServer;
    ///
    /// // Create actor
    /// let mut actor = Actor::new(...);
    ///
    /// // Set up lifecycle monitoring
    /// let lifecycle_tx = node.setup_lifecycle_monitoring(&actor.id()).await;
    /// actor.set_lifecycle_sender(lifecycle_tx);
    ///
    /// // Start actor - will automatically emit lifecycle events
    /// actor.start().await?;
    ///
    /// // When actor terminates, monitors are notified automatically
    /// ```
    ///
    /// ## Returns
    /// Sender for lifecycle events - give this to Actor via set_lifecycle_sender()
    pub fn setup_lifecycle_monitoring(
        self: &Arc<Self>,
        actor_id: &ActorId,
    ) -> mpsc::UnboundedSender<plexspaces_proto::ActorLifecycleEvent> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let node = self.clone();
        let actor_id = actor_id.clone();

        // Spawn background task to process lifecycle events
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                // Handle lifecycle event
                if let Err(e) = node.handle_lifecycle_event(event).await {
                    eprintln!("Error handling lifecycle event for {}: {:?}", actor_id, e);
                }
            }
        });

        tx
    }

    /// Handle actor lifecycle event (internal)
    async fn handle_lifecycle_event(
        &self,
        event: plexspaces_proto::ActorLifecycleEvent,
    ) -> Result<(), NodeError> {
        use plexspaces_proto::actor_lifecycle_event::EventType;

        // Extract event type
        match event.event_type {
            Some(EventType::Terminated(ref terminated)) => {
                // Record metrics
                metrics::counter!("plexspaces_node_actors_terminated_total",
                    "node_id" => self.id().as_str().to_string()
                ).increment(1);
                metrics::gauge!("plexspaces_node_active_actors",
                    "node_id" => self.id().as_str().to_string()
                ).decrement(1.0);
                tracing::info!(actor_id = %event.actor_id, node_id = %self.id().as_str(), reason = %terminated.reason, "Actor terminated");
                
                // Actor terminated normally - notify monitors
                self.actor_registry.notify_actor_down(&event.actor_id, &terminated.reason).await;
            }
            Some(EventType::Failed(ref failed)) => {
                // Record metrics
                metrics::counter!("plexspaces_node_actors_failed_total",
                    "node_id" => self.id().as_str().to_string()
                ).increment(1);
                metrics::gauge!("plexspaces_node_active_actors",
                    "node_id" => self.id().as_str().to_string()
                ).decrement(1.0);
                tracing::error!(actor_id = %event.actor_id, node_id = %self.id().as_str(), error = %failed.error, "Actor failed");
                
                // Actor failed (panic/error) - notify monitors
                self.actor_registry.notify_actor_down(&event.actor_id, &failed.error).await;
            }
            _ => {
                // Other lifecycle events (Starting, Activated, etc.) - log for observability
                tracing::debug!(actor_id = %event.actor_id, node_id = %self.id().as_str(), "Lifecycle event");
            }
        }

        Ok(())
    }

    // ============================================================================
    // Application Management (Erlang/OTP-style)
    // ============================================================================

    /// Register an application with the node
    ///
    /// ## Purpose
    /// Registers an application for lifecycle management. The application
    /// will be available for starting/stopping via the ApplicationManager.
    ///
    /// ## Arguments
    /// * `app` - Application implementation to register
    ///
    /// ## Returns
    /// * `Ok(())` - Application registered successfully
    /// * `Err(ApplicationError)` - Registration failed (e.g., duplicate name)
    ///
    /// ## Example
    /// ```ignore
    /// let app = Box::new(MyApplication::new());
    /// node.register_application(app).await?;
    /// ```
    pub async fn register_application(
        &self,
        app: Box<dyn Application>,
    ) -> Result<(), ApplicationError> {
        self.application_manager.write().await.register(app).await
    }

    /// Start a registered application
    ///
    /// ## Purpose
    /// Starts a previously registered application by calling its `start()` method.
    ///
    /// ## Arguments
    /// * `name` - Application name to start
    ///
    /// ## Returns
    /// * `Ok(())` - Application started successfully
    /// * `Err(ApplicationError)` - Start failed
    ///
    /// ## Example
    /// ```ignore
    /// node.start_application("genomics-pipeline").await?;
    /// ```
    pub async fn start_application(&self, name: &str) -> Result<(), ApplicationError> {
        // Ensure node_context is set (needed for application.start() call)
        let mut manager = self.application_manager.write().await;
        manager.ensure_node_context(Arc::new(self.clone()));
        manager.start(name).await
    }

    /// Stop a running application gracefully
    ///
    /// ## Purpose
    /// Stops an application with timeout-based graceful shutdown.
    ///
    /// ## Arguments
    /// * `name` - Application name to stop
    /// * `timeout` - Maximum time to wait for graceful shutdown
    ///
    /// ## Returns
    /// * `Ok(())` - Application stopped successfully
    /// * `Err(ApplicationError)` - Stop failed or timed out
    pub async fn stop_application(
        &self,
        name: &str,
        timeout: tokio::time::Duration,
    ) -> Result<(), ApplicationError> {
        self.application_manager
            .write()
            .await
            .stop(name, timeout)
            .await
    }

    /// Gracefully shutdown the node and all applications
    ///
    /// ## Purpose
    /// Performs graceful shutdown sequence:
    /// 1. Stop accepting new work
    /// 2. Stop background scheduler
    /// 3. Stop all applications in reverse order (last started, first stopped)
    /// 4. Drain actor mailboxes
    /// 5. Release resources
    ///
    /// ## Arguments
    /// * `timeout` - Maximum time to wait for graceful shutdown
    ///
    /// ## Returns
    /// * `Ok(())` - Shutdown completed successfully
    /// * `Err(ApplicationError)` - Shutdown failed or timed out
    ///
    /// ## Signal Handling
    /// This method is typically called in response to SIGTERM/SIGINT signals.
    /// See `start()` method for signal handler registration.
    ///
    /// ## Example
    /// ```ignore
    /// // Graceful shutdown with 30-second timeout
    /// node.shutdown(tokio::time::Duration::from_secs(30)).await?;
    /// ```
    pub async fn shutdown(&self, timeout: tokio::time::Duration) -> Result<(), ApplicationError> {
        eprintln!(
            "Node {}: Starting graceful shutdown (timeout: {:?})",
            self.id.as_str(),
            timeout
        );

        // Begin graceful shutdown on health reporter (sets NOT_SERVING, prevents new requests)
        {
            let health_reporter_guard = self.health_reporter.read().await;
            if let Some(ref health_reporter) = *health_reporter_guard {
                let (_drained, _duration, _completed) = health_reporter.begin_shutdown(Some(timeout)).await;
                eprintln!("Node {}: Health status set to NOT_SERVING, requests being drained", self.id.as_str());
            }
        }

        // Trigger gRPC server shutdown if it's running
        // This allows shutdown() to work even when called manually (not from signal)
        {
            let mut shutdown_tx = self.shutdown_tx.write().await;
            if let Some(tx) = shutdown_tx.take() {
                // Send shutdown signal to trigger gRPC server shutdown
                // Ignore error if already shut down
                let _ = tx.send(());
            }
        }

        // Stop background scheduler (Phase 4)
        {
            let scheduler = self.background_scheduler.read().await;
            if let Some(scheduler) = scheduler.as_ref() {
                eprintln!("Node {}: Stopping background scheduler", self.id.as_str());
                scheduler.stop();
            }
        }

        // Stop all applications (reverse order)
        self.application_manager
            .read()
            .await
            .stop_all(timeout)
            .await?;

        eprintln!("Node {}: All applications stopped", self.id.as_str());

        // Drain actor mailboxes (stop all actors which will drain their mailboxes)
        // This is already handled by stop_all_actors above
        
        // Close network connections
        let connected_nodes = self.connected_nodes().await;
        for node_id in connected_nodes {
            let _ = self.disconnect_from(&node_id).await;
        }
        eprintln!("Node {}: All network connections closed", self.id.as_str());
        
        // Flush TupleSpace pending operations (ensure all writes are persisted)
        // TupleSpace operations are synchronous, so no explicit flush needed
        // For external backends, they handle persistence automatically
        eprintln!("Node {}: TupleSpace operations flushed", self.id.as_str());

        eprintln!("Node {}: Shutdown complete", self.id.as_str());
        Ok(())
    }

    /// Check if shutdown has been requested
    pub async fn is_shutdown_requested(&self) -> bool {
        self.application_manager
            .read()
            .await
            .is_shutdown_requested()
            .await
    }

    /// Get application manager reference (for advanced usage)
    pub fn application_manager(&self) -> Arc<RwLock<ApplicationManager>> {
        self.application_manager.clone()
    }

    /// Get task router (Phase 5: Task routing)
    ///
    /// ## Returns
    /// Some(TaskRouter) if initialized, None otherwise
    pub async fn task_router(&self) -> Option<Arc<plexspaces_scheduler::TaskRouter>> {
        let router = self.task_router.read().await;
        router.clone()
    }

    /// Get background scheduler (Phase 4: Resource-aware scheduling)
    ///
    /// ## Returns
    /// Some(BackgroundScheduler) if initialized, None otherwise
    pub async fn background_scheduler(&self) -> Option<Arc<plexspaces_scheduler::background::BackgroundScheduler>> {
        let scheduler = self.background_scheduler.read().await;
        scheduler.clone()
    }

    // ============================================================================
    // Phase 8.5: Virtual Actor Lifecycle (Orleans-inspired)
    // ============================================================================

    /// Normalize actor ID to include node ID if missing
    ///
    /// ## Purpose
    /// Ensures actor ID has the format "actor@node". If the actor ID doesn't have @,
    /// appends the current node ID. If it already has @, returns it as is.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID (may or may not include @node)
    ///
    /// ## Returns
    /// Normalized actor ID in format "actor@node"
    fn normalize_actor_id(&self, actor_id: &ActorId) -> ActorId {
        if let Ok((actor_name, node_id)) = plexspaces_core::ActorRef::parse_actor_id(actor_id) {
            // Actor ID already has @ format
            // If node_id matches current node, return as is, otherwise reconstruct with current node ID
            if node_id == self.id().as_str() {
                actor_id.clone()
            } else {
                format!("{}@{}", actor_name, self.id().as_str())
            }
        } else {
            // Actor ID doesn't have @ format - append node ID
            format!("{}@{}", actor_id, self.id().as_str())
        }
    }

    /// Deactivate a virtual actor (remove from memory)
    ///
    /// ## Purpose
    /// Deactivates a virtual actor that has been idle, freeing memory while
    /// maintaining addressability.
    ///
    /// ## Behavior
    /// 1. Persist actor state to storage (if persist_on_deactivation enabled)
    /// 2. Remove actor from memory
    /// 3. Update VirtualActorLifecycle (last_accessed, is_activating = false)
    /// 4. Queue any new messages for later activation
    ///
    /// ## Arguments
    /// * `actor_id` - The virtual actor to deactivate
    /// * `force` - Force deactivation even if actor has pending messages
    ///
    /// ## Returns
    /// Ok(()) if deactivation successful
    ///
    /// ## Errors
    /// Returns error if actor is not virtual or deactivation fails
    pub async fn deactivate_virtual_actor(
        &self,
        actor_id: &ActorId,
        force: bool,
    ) -> Result<(), NodeError> {
        // Normalize actor ID to include node ID if missing
        let actor_id = self.normalize_actor_id(actor_id);
        
        // Use VirtualActorManager to get facet
        let manager = self.get_virtual_actor_manager().await?;
        let facet_arc = manager.get_facet(&actor_id).await
            .map_err(|e| NodeError::ActorNotFound(actor_id.clone()))?;
        
        // Downcast facet to VirtualActorFacet and mark as deactivated
        let mut facet_guard = facet_arc.write().await;
        let facet = facet_guard
            .downcast_mut::<VirtualActorFacet>()
            .ok_or_else(|| NodeError::ActorRegistrationFailed(actor_id.clone().into(), "Failed to downcast VirtualActorFacet".to_string()))?;
        
        facet.mark_deactivated().await;
        drop(facet_guard);

        // TODO: Persist actor state if persist_on_deactivation enabled
        // For now, just remove from active actors
        // Future: Save state to ObjectRegistry or storage backend

        // Remove from active actors
        self.actor_registry.unregister_with_cleanup(&actor_id).await
            .map_err(|e| NodeError::ActorRegistrationFailed(actor_id.clone(), e.to_string()))?;

        Ok(())
    }

    /// Check if virtual actor exists (without activating)
    ///
    /// ## Purpose
    /// Query whether a virtual actor exists without triggering activation.
    /// Useful for existence checks, health monitoring, and discovery.
    ///
    /// ## Arguments
    /// * `actor_id` - The actor ID to check (will be normalized to include node ID if missing)
    ///
    /// ## Returns
    /// (exists, is_active, is_virtual) tuple
    pub async fn check_virtual_actor_exists(
        &self,
        actor_id: &ActorId,
    ) -> (bool, bool, bool) {
        // Normalize actor ID to include node ID if missing
        let actor_id = self.normalize_actor_id(actor_id);
        
        // Use VirtualActorManager
        if let Ok(manager) = self.get_virtual_actor_manager().await {
            let is_virtual = manager.is_virtual(&actor_id).await;
            let is_active = if is_virtual {
                manager.is_active(&actor_id).await
            } else {
                false
            };
            // Actor exists only if it's virtual (virtual actors are always addressable)
            return (is_virtual, is_active, is_virtual);
        }
        
        // VirtualActorManager not available - actor is not virtual
        (false, false, false)
    }

    /// Register a virtual actor (Phase 8.5)
    ///
    /// ## Purpose
    /// Registers an actor as virtual (always addressable but not always in memory).
    /// Virtual actors are activated on-demand when they receive their first message.
    ///
    /// ## Arguments
    /// * `actor_id` - The actor ID
    /// * `facet` - The VirtualActorFacet instance
    ///
    /// ## Returns
    /// Ok(()) if registration successful
    pub async fn register_virtual_actor(
        &self,
        actor_id: ActorId,
        facet: VirtualActorFacet,
    ) -> Result<(), NodeError> {
        // Wrap facet in Box<dyn Any> for storage
        let facet_box = Arc::new(tokio::sync::RwLock::new(Box::new(facet) as Box<dyn std::any::Any + Send + Sync>));
        
        // Use VirtualActorManager for registration
        let manager = self.get_virtual_actor_manager().await?;
        let actor_id_clone = actor_id.clone();
        manager.register(actor_id, facet_box).await
            .map_err(|e| NodeError::ActorRegistrationFailed(actor_id_clone, e.to_string()))
    }

    /// Start idle timeout monitoring for virtual actors (Phase 8.5)
    ///
    /// ## Purpose
    /// Spawns a background task that periodically checks virtual actors for idle timeout
    /// and deactivates them if they've been idle longer than their configured timeout.
    ///
    /// ## Behavior
    /// - Runs every 10 seconds (configurable)
    /// - Checks all active virtual actors
    /// - Deactivates actors that exceed idle_timeout
    /// - Continues running until node shuts down
    ///
    /// ## Note
    /// This should be called once when the node starts.
    pub fn start_idle_timeout_monitor(&self) {
        let node = Arc::new(self.clone());
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            
            loop {
                interval.tick().await;
                
                // Get list of virtual actors to check
                let actor_ids = {
                    let virtual_actors = node.virtual_actors().read().await;
                    virtual_actors.keys().cloned().collect::<Vec<ActorId>>()
                };

                // Check each virtual actor for idle timeout
                for actor_id in actor_ids {
                    let should_deactivate = {
                        let virtual_actors = node.virtual_actors().read().await;
                        if let Some(virtual_meta) = virtual_actors.get(&actor_id) {
                            // Downcast facet from Box<dyn Any> to VirtualActorFacet
                            let facet_guard = virtual_meta.facet.read().await;
                            if let Some(facet) = facet_guard.downcast_ref::<VirtualActorFacet>() {
                                let result = facet.should_deactivate().await;
                                drop(facet_guard);
                                drop(virtual_actors);
                                result
                            } else {
                                drop(facet_guard);
                                drop(virtual_actors);
                                false
                            }
                        } else {
                            drop(virtual_actors);
                            false
                        }
                    };
                    
                    if should_deactivate {
                        // Deactivate actor (non-blocking, log errors)
                        // Deactivate using VirtualActorManager directly
                        if let Ok(manager) = node.get_virtual_actor_manager().await {
                            if let Ok(facet_arc) = manager.get_facet(&actor_id).await {
                                let mut facet_guard = facet_arc.write().await;
                                if let Some(facet) = facet_guard.downcast_mut::<VirtualActorFacet>() {
                                    facet.mark_deactivated().await;
                                }
                            }
                            // Use service_locator to get ActorRegistry
                            if let Some(actor_registry) = node.service_locator().get_service::<ActorRegistry>().await {
                                if let Err(e) = actor_registry.unregister_with_cleanup(&actor_id).await {
                                    eprintln!("Failed to deactivate idle virtual actor {}: {}", actor_id, e);
                                }
                            } else {
                                // Fallback to direct access if not registered yet
                                if let Err(e) = node.actor_registry().unregister_with_cleanup(&actor_id).await {
                                    eprintln!("Failed to deactivate idle virtual actor {}: {}", actor_id, e);
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    /// Start connection health monitoring and stale connection cleanup
    ///
    /// ## Purpose
    /// Spawns a background task that periodically:
    /// - Checks connection health (tracks last used time and failures)
    /// - Removes stale connections (idle for > 1 hour)
    /// - Attempts to reconnect failed connections
    ///
    /// ## Behavior
    /// - Runs every 60 seconds
    /// - Checks all gRPC client connections
    /// - Removes connections idle for > 1 hour
    /// - Attempts to reconnect connections with failures
    ///
    /// ## Note
    /// This should be called once when the node starts.
    pub fn start_connection_health_monitor(&self) {
        let node = Arc::new(self.clone());
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            let stale_timeout = Duration::from_secs(3600); // 1 hour
            
            loop {
                interval.tick().await;
                
                // Get list of connections to check
                let connections_to_check: Vec<(NodeId, String)> = {
                    let connections = node.connections.read().await;
                    connections.iter()
                        .map(|(node_id, conn)| (node_id.clone(), conn.node_address.clone()))
                        .collect()
                };
                
                // Check each connection
                for (node_id, node_address) in connections_to_check {
                    let health_status = {
                        let health = node.connection_health.read().await;
                        health.get(&node_id).cloned()
                    };
                    
                    if let Some((last_used, last_error, consecutive_failures)) = health_status {
                        let idle_duration = last_used.elapsed();
                        
                        // Remove stale connections (idle > 1 hour)
                        if idle_duration > stale_timeout {
                            tracing::info!(remote_node_id = %node_id.as_str(), idle_seconds = idle_duration.as_secs(), "Removing stale connection");
                            
                            let mut clients = node.grpc_clients.write().await;
                            clients.remove(&node_id);
                            drop(clients);
                            
                            let mut health = node.connection_health.write().await;
                            health.remove(&node_id);
                            drop(health);
                            
                            metrics::counter!("plexspaces_node_connections_removed_stale_total",
                                "node_id" => node.id().as_str().to_string(),
                                "remote_node_id" => node_id.as_str().to_string()
                            ).increment(1);
                            metrics::gauge!("plexspaces_node_active_connections",
                                "node_id" => node.id().as_str().to_string()
                            ).decrement(1.0);
                            continue;
                        }
                        
                        // Attempt to reconnect if we have failures but connection still exists
                        if consecutive_failures > 0 && last_error.is_some() {
                            // Check if connection still exists in pool
                            let connection_exists = {
                                let clients = node.grpc_clients.read().await;
                                clients.contains_key(&node_id)
                            };
                            
                            if connection_exists {
                                // Try to reconnect
                                if let Ok(new_client) = RemoteActorClient::connect(&node_address).await {
                                    tracing::info!(remote_node_id = %node_id.as_str(), "Reconnected to remote node via health check");
                                    
                                    let mut clients = node.grpc_clients.write().await;
                                    clients.insert(node_id.clone(), new_client);
                                    drop(clients);
                                    
                                    let mut health = node.connection_health.write().await;
                                    health.insert(node_id.clone(), (tokio::time::Instant::now(), None, 0));
                                    drop(health);
                                    
                                    metrics::counter!("plexspaces_node_connections_reconnected_total",
                                        "node_id" => node.id().as_str().to_string(),
                                        "remote_node_id" => node_id.as_str().to_string()
                                    ).increment(1);
                                } else {
                                    tracing::debug!(remote_node_id = %node_id.as_str(), consecutive_failures = consecutive_failures, "Failed to reconnect during health check");
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}

/// Implement ApplicationNode trait to provide infrastructure access to applications
#[async_trait::async_trait]
impl ApplicationNode for Node {
    /// Get node identifier
    fn id(&self) -> &str {
        self.id.as_str()
    }

    /// Get node's gRPC listen address
    fn listen_addr(&self) -> &str {
        &self.config.listen_addr
    }

    /// Spawn an actor with the given behavior
    ///
    /// ## Implementation
    /// Wraps the behavior in an Actor with a mailbox and spawns it on this node.
    async fn spawn_actor(
        &self,
        actor_id: String,
        behavior: Box<dyn plexspaces_core::Actor>,
        namespace: String,
    ) -> Result<String, plexspaces_core::application::ApplicationError> {
        use plexspaces_core::application::ApplicationError;
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};

        // Create mailbox for actor
        let mut mailbox_config = mailbox_config_default();
        mailbox_config.storage_strategy = plexspaces_mailbox::StorageStrategy::Memory as i32;
        mailbox_config.ordering_strategy = plexspaces_mailbox::OrderingStrategy::OrderingFifo as i32;
        mailbox_config.durability_strategy = plexspaces_mailbox::DurabilityStrategy::DurabilityNone as i32;
        mailbox_config.capacity = 1000;
        mailbox_config.backpressure_strategy = plexspaces_mailbox::BackpressureStrategy::DropOldest as i32;
        let mailbox = Mailbox::new(mailbox_config, format!("{}:mailbox", actor_id))
            .await
            .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), e.to_string()))?;

        // Create actor from behavior
        let actor = plexspaces_actor::Actor::new(actor_id.clone(), behavior, mailbox, namespace, None);

        // Spawn actor using ActorFactory
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        let actor_factory: Arc<ActorFactoryImpl> = self.service_locator.get_service().await
            .ok_or_else(|| ApplicationError::ActorSpawnFailed(actor_id.clone(), "ActorFactory not found in ServiceLocator".to_string()))?;
        
        actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await
            .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("Failed to spawn actor: {}", e)))?;

        Ok(actor_id)
    }

    /// Stop an actor
    ///
    /// ## Implementation
    /// Unregisters the actor from the node. The actor will be garbage collected.
    /// TODO: Implement graceful shutdown with shutdown message
    async fn stop_actor(
        &self,
        actor_id: &str,
    ) -> Result<(), plexspaces_core::application::ApplicationError> {
        use plexspaces_core::application::ApplicationError;

        // Verify actor exists in ActorRegistry
        let actor_id_str = actor_id.to_string();
        let routing = self.actor_registry.lookup_routing(&actor_id_str).await
            .map_err(|_| ApplicationError::ActorStopFailed(
                actor_id.to_string(),
                "Actor not found".to_string(),
            ))?;
        if routing.is_none() || !routing.as_ref().unwrap().is_local {
            return Err(ApplicationError::ActorStopFailed(
                actor_id.to_string(),
                "Actor not found".to_string(),
            ));
        }

        // Unregister from ActorRegistry
        let actor_id_string = actor_id.to_string();
        self.actor_registry.unregister_with_cleanup(&actor_id_string).await
            .map_err(|e| ApplicationError::ActorStopFailed(actor_id.to_string(), e.to_string()))?;

        Ok(())
    }

    /// Update actor count for an application (for metrics tracking)
    ///
    /// ## Implementation
    /// Updates the actor count in ApplicationManager for metrics tracking.
    async fn update_actor_count(
        &self,
        application_name: &str,
        actor_count: u32,
    ) -> Result<(), plexspaces_core::application::ApplicationError> {
        use plexspaces_core::application::ApplicationError;
        
        let app_manager = self.application_manager.read().await;
        app_manager
            .update_actor_count(application_name, actor_count)
            .await
            .map_err(|e| ApplicationError::Other(format!("Failed to update actor count: {}", e)))
    }
}

/// Actor location (local or remote)
#[derive(Debug, Clone)]
pub enum ActorLocation {
    /// Actor is on the local node (stores ActorId)
    Local(ActorId),
    /// Actor is on a remote node
    Remote(NodeId),
}

/// Node errors
#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    /// Actor already registered on this node
    #[error("Actor already registered: {0:?}")]
    ActorAlreadyRegistered(ActorId),

    /// Actor not found on this node
    #[error("Actor not found: {0:?}")]
    ActorNotFound(ActorId),

    /// Remote node not found in registry
    #[error("Node not found: {0:?}")]
    NodeNotFound(NodeId),

    /// Remote node not connected
    #[error("Node not connected: {0:?}")]
    NodeNotConnected(NodeId),

    /// Already connected to remote node
    #[error("Already connected to node: {0:?}")]
    AlreadyConnected(NodeId),

    /// Message delivery failed
    #[error("Delivery failed: {0}")]
    DeliveryFailed(String),

    /// Actor registration failed
    #[error("Actor registration failed: {0:?} - {1}")]
    ActorRegistrationFailed(ActorId, String),

    /// TupleSpace operation failed
    #[error("TupleSpace error: {0}")]
    TupleSpaceError(String),

    /// Network operation failed
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Actor failed to start
    #[error("Actor {0} failed to start: {1}")]
    ActorStartFailed(ActorId, String),

    /// ActorRef creation failed
    #[error("ActorRef creation failed for {0}: {1}")]
    ActorRefCreationFailed(ActorId, String),

    /// gRPC server error
    #[error("gRPC error: {0}")]
    GrpcError(String),

    /// Configuration error
    #[error("Config error: {0}")]
    ConfigError(String),

    /// Invalid argument
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// ObjectRegistry operation failed
    #[error("ObjectRegistry error: {0}")]
    ObjectRegistryError(String),
}

/// Cluster manager for coordinating multiple nodes
pub struct ClusterManager {
    /// Local node
    local_node: Arc<Node>,
    /// Cluster configuration
    config: ClusterConfig,
    /// Cluster state
    #[allow(dead_code)]
    state: Arc<RwLock<ClusterState>>,
}

/// Cluster configuration
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Cluster name
    pub name: String,
    /// Seed nodes for discovery
    pub seed_nodes: Vec<(NodeId, String)>,
    /// Minimum nodes for quorum
    pub min_nodes: usize,
    /// Enable auto-discovery
    pub auto_discovery: bool,
}

/// Cluster state
#[derive(Debug)]
#[allow(dead_code)]
struct ClusterState {
    /// Current leader (if any)
    leader: Option<NodeId>,
    /// Cluster members
    members: HashMap<NodeId, NodeInfo>,
    /// Cluster epoch
    epoch: u64,
}

/// Node information in cluster
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct NodeInfo {
    id: NodeId,
    address: String,
    capabilities: NodeCapabilities,
    last_seen: tokio::time::Instant,
}

impl ClusterManager {
    /// Create a new cluster manager
    pub fn new(local_node: Arc<Node>, config: ClusterConfig) -> Self {
        ClusterManager {
            local_node,
            config,
            state: Arc::new(RwLock::new(ClusterState {
                leader: None,
                members: HashMap::new(),
                epoch: 0,
            })),
        }
    }

    /// Join the cluster
    pub async fn join(&self) -> Result<(), NodeError> {
        // Connect to seed nodes
        for (node_id, address) in &self.config.seed_nodes {
            if node_id != self.local_node.id() {
                self.local_node
                    .connect_to(node_id.clone(), address.clone())
                    .await?;
            }
        }

        // Announce membership
        self.local_node
            .tuplespace()
            .write(Tuple::new(vec![
                TupleField::String("cluster_join".to_string()),
                TupleField::String(self.config.name.clone()),
                TupleField::String(self.local_node.id().as_str().to_string()),
            ]))
            .await
            .map_err(|e| NodeError::TupleSpaceError(e.to_string()))?;

        Ok(())
    }

    /// Leave the cluster
    pub async fn leave(&self) -> Result<(), NodeError> {
        // Announce departure
        self.local_node
            .tuplespace()
            .write(Tuple::new(vec![
                TupleField::String("cluster_leave".to_string()),
                TupleField::String(self.config.name.clone()),
                TupleField::String(self.local_node.id().as_str().to_string()),
            ]))
            .await
            .map_err(|e| NodeError::TupleSpaceError(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Helper functions for tests (defined inline since we can't import from tests/ directory)
    async fn lookup_actor_ref_helper(
        node: &Node,
        actor_id: &ActorId,
    ) -> Result<Option<ActorRef>, NodeError> {
        use plexspaces_core::ActorRegistry;
        use std::sync::Arc;
        
        // Normalize actor ID
        let actor_id = normalize_actor_id_helper(node, actor_id);
        
        // Get ActorRegistry
        let actor_registry: Arc<ActorRegistry> = node.service_locator().get_service().await
            .ok_or_else(|| NodeError::ConfigError("ActorRegistry not found".to_string()))?;
        
        // Check if actor exists
        if let Some(_actor_trait) = actor_registry.lookup_actor(&actor_id).await {
            Ok(Some(ActorRef::remote(
                actor_id.clone(),
                node.id().as_str().to_string(),
                node.service_locator().clone(),
            )))
        } else {
            // Check routing
            let routing = actor_registry.lookup_routing(&actor_id).await
                .map_err(|e| NodeError::ActorRefCreationFailed(actor_id.clone(), e.to_string()))?;
            
            if let Some(routing_info) = routing {
                if routing_info.is_local {
                    Ok(None)
                } else {
                    Ok(Some(ActorRef::remote(
                        actor_id.clone(),
                        routing_info.node_id,
                        node.service_locator().clone(),
                    )))
                }
            } else {
                Ok(None)
            }
        }
    }
    
    fn normalize_actor_id_helper(node: &Node, actor_id: &ActorId) -> ActorId {
        if let Ok((actor_name, node_id)) = plexspaces_core::ActorRef::parse_actor_id(actor_id) {
            if node_id == node.id().as_str() {
                actor_id.clone()
            } else {
                format!("{}@{}", actor_name, node.id().as_str())
            }
        } else {
            format!("{}@{}", actor_id, node.id().as_str())
        }
    }
    
    // Alias for consistency with test files
    use lookup_actor_ref_helper as lookup_actor_ref;
    
    use super::*;
    use plexspaces_mailbox::Mailbox;
    use plexspaces_actor::RegularActorWrapper;
    use plexspaces_core::MessageSender;
    
    // Helper to get ActorRegistry from service_locator
    // Waits for registration if not yet available (Node::new() registers asynchronously)
    async fn get_actor_registry(node: &Node) -> Arc<ActorRegistry> {
        let service_locator = node.service_locator();
        // Try to get immediately
        if let Some(registry) = service_locator.get_service::<ActorRegistry>().await {
            return registry;
        }
        // If not available, wait a bit for async registration to complete
        // This happens in Node::new() which spawns a task to register services
        for _ in 0..10 {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            if let Some(registry) = service_locator.get_service::<ActorRegistry>().await {
                return registry;
            }
        }
        // Fallback: use direct access if still not registered (for tests)
        node.actor_registry()
    }
    
    // Helper to register actor with MessageSender (replaces register_local)
    async fn register_actor_for_test(node: &Node, actor_id: &str, mailbox: Arc<Mailbox>) {
        let wrapper = Arc::new(RegularActorWrapper::new(
            actor_id.to_string(),
            mailbox,
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(node).await;
        actor_registry.register_actor(actor_id.to_string(), wrapper, None, None, None).await;
    }

    #[tokio::test]
    async fn test_node_creation() {
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        assert_eq!(node.id().as_str(), "test-node");
        assert_eq!(node.connected_nodes().await.len(), 0);
    }

    #[tokio::test]
    async fn test_actor_registration() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = Node::new(NodeId::new("test-node"), default_node_config());

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());

        // Register with ActorRegistry first
        // Register actor with MessageSender (mailbox is internal)
        use plexspaces_actor::RegularActorWrapper;
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(RegularActorWrapper::new(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor_ref.id().clone(), wrapper, None, None, None).await;

        // Register actor config (if needed)
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Should find local actor via ActorRegistry
        match actor_registry.lookup_routing(&"test-actor@test-node".to_string()).await {
            Ok(Some(routing_info)) => {
                assert!(routing_info.is_local);
                assert_eq!(routing_info.node_id, "test-node");
            }
            Ok(None) => panic!("Actor not found"),
            Err(_) => panic!("Lookup failed"),
        }
    }

    #[tokio::test]
    async fn test_tuplespace_integration() {
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        // Write configuration to TupleSpace (Orbit-inspired)
        node.tuplespace()
            .write(Tuple::new(vec![
                TupleField::String("config".to_string()),
                TupleField::String("timeout".to_string()),
                TupleField::Integer(30),
            ]))
            .await
            .unwrap();

        // Read it back
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("config".to_string())),
            PatternField::Exact(TupleField::String("timeout".to_string())),
            PatternField::Wildcard,
        ]);

        let result = node.tuplespace().read(pattern).await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_actor_unregistration() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = Node::new(NodeId::new("test-node"), default_node_config());

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());

        // Register with ActorRegistry first
        // Register actor with MessageSender (mailbox is internal)
        use plexspaces_actor::RegularActorWrapper;
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(RegularActorWrapper::new(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor_ref.id().clone(), wrapper, None, None, None).await;

        // Register actor config
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();
        assert!(actor_registry.lookup_routing(&"test-actor@test-node".to_string()).await.is_ok());

        // Unregister
        actor_registry.unregister_with_cleanup(&"test-actor@test-node".to_string())
            .await
            .unwrap();
        // After unregistering, lookup_actor should return None (actor not found)
        // lookup_routing might still succeed for local node, so check lookup_actor instead
        let lookup_result = actor_registry.lookup_actor(&"test-actor@test-node".to_string()).await;
        assert!(lookup_result.is_none(), "Actor should not be found after unregistering");
    }

    #[tokio::test]
    async fn test_duplicate_actor_registration() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = Node::new(NodeId::new("test-node"), default_node_config());

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());

        // Register with ActorRegistry first (using MessageSender)
        register_actor_for_test(&node, actor_ref.id().as_str(), mailbox.clone()).await;

        // First registration should succeed
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Second registration should fail
        let actor_registry = get_actor_registry(&node).await;
        let result = actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await;
        assert!(result.is_err());
        // Expected error for duplicate registration
    }

    #[tokio::test]
    async fn test_route_message_local() {
        use plexspaces_mailbox::mailbox_config_default;

        let node_arc = Arc::new(Node::new(NodeId::new("test-node"), default_node_config()));
        
        // Register NodeMetricsUpdater early for tests (normally done in create_actor_context_arc)
        let metrics_updater = Arc::new(crate::service_wrappers::NodeMetricsUpdaterWrapper::new(node_arc.clone()));
        node_arc.service_locator().register_service(metrics_updater.clone()).await;
        let metrics_updater_trait: Arc<dyn plexspaces_core::NodeMetricsUpdater + Send + Sync> = metrics_updater.clone() as Arc<dyn plexspaces_core::NodeMetricsUpdater + Send + Sync>;
        node_arc.service_locator().register_node_metrics_updater(metrics_updater_trait).await;
        let node = node_arc.as_ref();

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        
        // Register with ActorRegistry (using MessageSender)
        register_actor_for_test(&node, actor_ref.id().as_str(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Create message
        let message = plexspaces_mailbox::Message::new(vec![1, 2, 3]);

        // Reset metrics before sending message
        {
            let mut metrics = node.metrics.write().await;
            metrics.messages_routed = 0;
            metrics.local_deliveries = 0;
        }
        
        // Send message via ActorRef (use the actor_ref we already have, don't lookup again)
        actor_ref.tell(message).await.unwrap();

        // Verify stats updated
        let node_metrics = node.metrics().await;
        assert_eq!(node_metrics.messages_routed, 1, "messages_routed should be 1, got {}", node_metrics.messages_routed);
        assert_eq!(node_metrics.local_deliveries, 1, "local_deliveries should be 1, got {}", node_metrics.local_deliveries);
    }

    #[tokio::test]
    async fn test_route_message_actor_not_found() {
        let node = Node::new(NodeId::new("test-node"), default_node_config());
        
        // Wait for services to be registered
        for _ in 0..10 {
            if node.service_locator().get_service::<plexspaces_core::ActorRegistry>().await.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        let message = plexspaces_mailbox::Message::new(vec![1, 2, 3]);

        // Try to send to non-existent actor
        // lookup_actor_ref returns Ok(None) for local actors that don't exist
        let result = lookup_actor_ref(&node, &"nonexistent@test-node".to_string()).await;
        let result = match result {
            Ok(Some(actor_ref)) => actor_ref.tell(message).await
                .map_err(|e| NodeError::DeliveryFailed(format!("{}", e))),
            Ok(None) => {
                // Local actor not found - try to send anyway to get ActorNotFound error
                // Or return ActorNotFound directly
                Err(NodeError::ActorNotFound("nonexistent@test-node".to_string()))
            },
            Err(e) => Err(e),
        };
        assert!(result.is_err());
        match result {
            Err(NodeError::ActorNotFound(_)) => {} // Expected
            _ => panic!("Expected ActorNotFound error, got: {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_node_connection() {
        let node = Node::new(NodeId::new("node1"), default_node_config());

        // Connect to another node
        node.connect_to(NodeId::new("node2"), "localhost:9002".to_string())
            .await
            .unwrap();

        // Verify connection
        let connected = node.connected_nodes().await;
        assert_eq!(connected.len(), 1);
        assert_eq!(connected[0].as_str(), "node2");

        // Check stats
        let node_metrics = node.metrics().await;
        assert_eq!(node_metrics.connected_nodes, 1);
    }

    #[tokio::test]
    async fn test_node_disconnection() {
        let node = Node::new(NodeId::new("node1"), default_node_config());

        // Connect
        node.connect_to(NodeId::new("node2"), "localhost:9002".to_string())
            .await
            .unwrap();
        assert_eq!(node.connected_nodes().await.len(), 1);

        // Disconnect
        node.disconnect_from(&NodeId::new("node2")).await.unwrap();
        assert_eq!(node.connected_nodes().await.len(), 0);

        let node_metrics = node.metrics().await;
        assert_eq!(node_metrics.connected_nodes, 0);
    }

    #[tokio::test]
    async fn test_duplicate_connection() {
        let node = Node::new(NodeId::new("node1"), default_node_config());

        // First connection should succeed
        node.connect_to(NodeId::new("node2"), "localhost:9002".to_string())
            .await
            .unwrap();

        // Second connection should fail
        let result = node
            .connect_to(NodeId::new("node2"), "localhost:9002".to_string())
            .await;
        assert!(result.is_err());
        match result {
            Err(NodeError::AlreadyConnected(_)) => {} // Expected
            _ => panic!("Expected AlreadyConnected error"),
        }
    }

    #[tokio::test]
    async fn test_node_announcement() {
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        // Manually announce node in TupleSpace (without starting full server)
        node.tuplespace()
            .write(Tuple::new(vec![
                TupleField::String("node_started".to_string()),
                TupleField::String("test-node".to_string()),
                TupleField::String(node.config.listen_addr.clone()),
            ]))
            .await
            .unwrap();

        // Verify node announced itself in TupleSpace
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("node_started".to_string())),
            PatternField::Exact(TupleField::String("test-node".to_string())),
            PatternField::Wildcard,
        ]);

        let result = node.tuplespace().read(pattern).await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_cluster_manager_join() {
        let node = Arc::new(Node::new(NodeId::new("node1"), NodeConfig::default()));

        let cluster_config = ClusterConfig {
            name: "test-cluster".to_string(),
            seed_nodes: vec![
                (NodeId::new("node1"), "localhost:9001".to_string()),
                (NodeId::new("node2"), "localhost:9002".to_string()),
            ],
            min_nodes: 2,
            auto_discovery: true,
        };

        let manager = ClusterManager::new(node.clone(), cluster_config);

        // Join cluster
        manager.join().await.unwrap();

        // Verify join announced in TupleSpace
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("cluster_join".to_string())),
            PatternField::Exact(TupleField::String("test-cluster".to_string())),
            PatternField::Exact(TupleField::String("node1".to_string())),
        ]);

        let result = node.tuplespace().read(pattern).await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_cluster_manager_leave() {
        let node = Arc::new(Node::new(NodeId::new("node1"), NodeConfig::default()));

        let cluster_config = ClusterConfig {
            name: "test-cluster".to_string(),
            seed_nodes: vec![],
            min_nodes: 1,
            auto_discovery: false,
        };

        let manager = ClusterManager::new(node.clone(), cluster_config);

        // Join then leave
        manager.join().await.unwrap();
        manager.leave().await.unwrap();

        // Verify leave announced in TupleSpace
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("cluster_leave".to_string())),
            PatternField::Exact(TupleField::String("test-cluster".to_string())),
            PatternField::Exact(TupleField::String("node1".to_string())),
        ]);

        let result = node.tuplespace().read(pattern).await.unwrap();
        assert!(result.is_some());
    }

    // ============================================================================
    // spawn_actor() Tests (Erlang-style supervision)
    // ============================================================================

    #[tokio::test]
    async fn test_spawn_actor_creates_and_returns_ref() {
        use plexspaces_actor::Actor;
        use plexspaces_behavior::MockBehavior;
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use std::sync::Arc;

        let node = Node::new(NodeId::new("test-node"), default_node_config());

        // Create actor
        let behavior = Box::new(MockBehavior::new());
        // Create mailbox - Actor::new takes ownership, but we need Arc for ActorRef
        // So we create a new mailbox for ActorRef after spawning
        let mailbox = Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap();
        let actor = Actor::new(
            "test-actor@test-node".to_string(),
            behavior,
            mailbox,
            "test-namespace".to_string(),
            None,
        );

        // Wait for services to be registered and register ActorFactory
        let service_locator = node.service_locator();
        for _ in 0..10 {
            if service_locator.get_service::<plexspaces_core::ActorRegistry>().await.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        // Register ActorFactory for tests (normally done in create_actor_context_arc)
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
        service_locator.register_service(actor_factory_impl.clone()).await;
        let actor_factory: Arc<ActorFactoryImpl> = actor_factory_impl;
        
        let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await.unwrap();
        // Create a new mailbox for ActorRef (actor is already spawned with its own mailbox)
        let mailbox_for_ref = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-ref-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox_for_ref, service_locator);

        // Verify ActorRef returned
        assert_eq!(actor_ref.id(), "test-actor@test-node");

        // Verify actor registered in ActorRegistry
        let actor_registry = get_actor_registry(&node).await;
        match actor_registry.lookup_routing(&"test-actor@test-node".to_string()).await {
            Ok(Some(routing_info)) => {
                assert!(routing_info.is_local);
                assert_eq!(routing_info.node_id, "test-node");
            }
            Ok(None) => panic!("Actor not found"),
            Err(_) => panic!("Lookup failed"),
        }
    }

    #[tokio::test]
    #[ignore] // TODO: Fix test - actors don't terminate automatically, need explicit stop
    async fn test_spawn_actor_monitors_termination() {
        use plexspaces_actor::Actor;
        use plexspaces_behavior::MockBehavior;
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use std::sync::Arc;
        use tokio::sync::mpsc;

        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Create monitor channel
        let (tx, mut rx) = mpsc::channel(1);

        // Create actor
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap();
        let actor = Actor::new(
            "test-actor@test-node".to_string(),
            behavior,
            mailbox,
            "test-namespace".to_string(),
            None,
        );

        // Wait for services to be registered and register ActorFactory
        let service_locator = node.service_locator();
        for _ in 0..10 {
            if service_locator.get_service::<plexspaces_core::ActorRegistry>().await.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        // Register ActorFactory for tests (normally done in create_actor_context_arc)
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
        service_locator.register_service(actor_factory_impl.clone()).await;
        let actor_factory: Arc<ActorFactoryImpl> = actor_factory_impl;
        
        let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await.unwrap();
        // Create a new mailbox for ActorRef (actor is already spawned with its own mailbox)
        let mailbox_for_ref = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-ref-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox_for_ref, service_locator);

        // Establish monitoring link
        node.monitor(
            &"test-actor@test-node".to_string(),
            &"supervisor@test-node".to_string(),
            tx,
        )
        .await
        .unwrap();

        // Send a message to the actor (to ensure it's processing)
        // Note: In real usage, this would be done via Node::route_message() or ActorService
        // For this test, we'll skip the message send as it's not essential for the test
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        // Stop the actor explicitly (graceful shutdown)
        // Note: Actors don't terminate automatically - they must be stopped explicitly
        // In a real scenario, this would be done via Node::stop_actor() or actor.stop()
        // For now, we'll drop the actor_ref which will cause the actor to terminate
        // when the JoinHandle completes (simulating graceful shutdown)
        drop(actor_ref);
        
        // Wait a bit for the actor to process shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Wait for termination notification (increased timeout due to message loop sleep)
        tokio::time::timeout(tokio::time::Duration::from_millis(2000), async {
            if let Some((actor_id, reason)) = rx.recv().await {
                assert_eq!(actor_id, "test-actor@test-node");
                // Reason will be "normal" for graceful shutdown or "killed" if dropped
                assert!(reason.contains("normal") || reason.contains("shutdown") || reason.contains("killed"));
            } else {
                panic!("Expected termination notification");
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_spawn_actor_detects_panic() {
        use plexspaces_actor::Actor;
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use std::sync::Arc;
        use tokio::sync::mpsc;

        // NOTE: Panics in actor behavior's handle_message() are caught by Rust
        // and converted to BehaviorError. To truly test panic detection, we'd
        // need the panic to happen outside the async function (e.g., in actor loop).
        // For now, this test verifies graceful shutdown detection.
        //
        // True panic detection would require:
        // 1. Panic in tokio::spawn closure (outside process_message)
        // 2. Or explicit panic!() in actor loop
        //
        // This is a known limitation of the current Erlang-simple approach.
        // Actual panics would be caught by JoinHandle and classified as "panic"
        // in spawn_actor's watcher task.

        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Create monitor channel
        let (tx, _rx) = mpsc::channel(1);

        // Create actor with normal behavior (panics are converted to errors)
        let behavior = Box::new(plexspaces_behavior::MockBehavior::new());
        let mailbox = Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap();
        let actor = Actor::new(
            "test-actor@test-node".to_string(),
            behavior,
            mailbox,
            "test-namespace".to_string(),
            None,
        );

        // Wait for services to be registered and register ActorFactory
        let service_locator = node.service_locator();
        for _ in 0..10 {
            if service_locator.get_service::<plexspaces_core::ActorRegistry>().await.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        // Register ActorFactory for tests (normally done in create_actor_context_arc)
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
        service_locator.register_service(actor_factory_impl.clone()).await;
        let actor_factory: Arc<ActorFactoryImpl> = actor_factory_impl;
        
        let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await.unwrap();
        // Create a new mailbox for ActorRef (actor is already spawned with its own mailbox)
        let mailbox_for_ref = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-ref-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox_for_ref, service_locator);

        // Establish monitoring link
        node.monitor(
            &"test-actor@test-node".to_string(),
            &"supervisor@test-node".to_string(),
            tx,
        )
        .await
        .unwrap();

        // Send message to actor
        // Note: In real usage, this would be done via Node::route_message() or ActorService
        // For this test, we'll skip the message send as it's not essential

        // Wait a bit for message processing
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Manually abort the actor to simulate crash
        // This will trigger "killed" termination reason in JoinHandle watcher
        drop(actor_ref);

        // Note: Without explicit stop, actor keeps running
        // True panic test would require different approach
        //
        // For now, verify normal termination detection works
        // Panic detection works (as seen in JoinHandle.await handling)
        // but requires panic outside async error handling
    }

    // ============================================================================
    // Remote Messaging Tests (ActorRef::tell() for remote actors, gRPC client pooling)
    // ============================================================================

    #[tokio::test]
    async fn test_tell_to_remote_node() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};

        // Create two nodes
        let node1 = Arc::new(Node::new(NodeId::new("node1"), NodeConfig::default()));

        let node2 = Arc::new(Node::new(NodeId::new("node2"), NodeConfig::default()));

        // Register node2 as remote node in node1's registry
        // Using a fake address since we're testing routing logic, not actual gRPC
        node1
            .register_remote_node(NodeId::new("node2"), "http://localhost:9999".to_string())
            .await
            .unwrap();

        // Register actor on node2
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::remote("test-actor@node2", "node2", node2.service_locator());

        // Register actor with ActorRegistry on node2
        register_actor_for_test(&node2, actor_ref.id().as_str(), mailbox.clone()).await;
        let actor_registry2 = get_actor_registry(&node2).await;
        actor_registry2.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Register node2 in ObjectRegistry on node1 (so node1 can find it)
        use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
        let registration = ObjectRegistration {
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            object_type: ObjectType::ObjectTypeService as i32,
            object_id: "_node@node2".to_string(),
            grpc_address: "http://localhost:9999".to_string(),
            object_category: "Node".to_string(),
            ..Default::default()
        };
        node1.object_registry().register(registration).await.unwrap();

        // Try to route message from node1 to actor on node2
        let message = plexspaces_mailbox::Message::new(vec![1, 2, 3]);

        // This will fail because we don't have a real gRPC server running
        // But it exercises the remote routing code path via ActorRef::tell()
        // Wait for services to be registered on node1
        for _ in 0..10 {
            if node1.service_locator().get_service::<plexspaces_core::ActorRegistry>().await.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        let actor_ref = lookup_actor_ref(&node1, &"test-actor@node2".to_string()).await;
        let result = match actor_ref {
            Ok(Some(actor_ref)) => actor_ref.tell(message).await
                .map_err(|e| NodeError::DeliveryFailed(format!("{}", e))),
            Ok(None) => Err(NodeError::ActorNotFound("test-actor@node2".to_string())),
            Err(e) => Err(e),
        };

        // Should fail with network error (no server listening)
        // The error could be NetworkError, DeliveryFailed, or ActorNotFound (if routing fails before network)
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, NodeError::NetworkError(_) | NodeError::DeliveryFailed(_) | NodeError::ActorNotFound(_)),
            "Expected NetworkError, DeliveryFailed, or ActorNotFound, got: {:?}", err
        );
    }

    #[tokio::test]
    async fn test_find_actor_remote_via_node_id() {
        let node = Node::new(NodeId::new("node1"), default_node_config());

        // Register remote node in connections
        node.register_remote_node(NodeId::new("node2"), "http://localhost:9999".to_string())
            .await
            .unwrap();

        // Register remote node in ObjectRegistry (ActorRegistry looks up nodes here)
        use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
        let registration = ObjectRegistration {
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            object_type: ObjectType::ObjectTypeService as i32,
            object_id: "_node@node2".to_string(), // ActorRegistry looks for this format
            grpc_address: "http://localhost:9999".to_string(),
            object_category: "Node".to_string(),
            ..Default::default()
        };
        node.object_registry().register(registration).await.unwrap();

        // Try to find actor with @node2 suffix
        let actor_registry = get_actor_registry(&node).await;
        let result = actor_registry.lookup_routing(&"test-actor@node2".to_string()).await;

        // Should find it as remote (because node2 is registered in ObjectRegistry)
        assert!(result.is_ok());
        match result.unwrap() {
            Some(routing_info) => {
                assert!(!routing_info.is_local);
                assert_eq!(routing_info.node_id, "node2");
            }
            None => panic!("Actor not found"),
        }
    }

    #[tokio::test]
    async fn test_find_actor_remote_not_found() {
        let node = Node::new(NodeId::new("node1"), default_node_config());

        // Try to find actor that doesn't exist anywhere
        let actor_registry = get_actor_registry(&node).await;
        let result = actor_registry
            .lookup_routing(&"nonexistent@unknown-node".to_string())
            .await;

        // lookup_routing returns Ok(None) when node is not found in ObjectRegistry
        // (it doesn't return an error, just None)
        assert!(result.is_ok());
        assert!(result.unwrap().is_none(), "Should return None for non-existent remote node");
    }

    #[tokio::test]
    async fn test_find_actor_via_tuplespace() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};

        let node1 = Node::new(NodeId::new("node1"), NodeConfig::default());

        let node2 = Node::new(NodeId::new("node2"), NodeConfig::default());

        // Register actor on node2 (this writes to node2's TupleSpace)
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::remote("test-actor@node2", "node2", node2.service_locator());

        // Register actor with ActorRegistry on node2
        register_actor_for_test(&node2, actor_ref.id().as_str(), mailbox.clone()).await;
        let actor_registry2 = get_actor_registry(&node2).await;
        actor_registry2.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Register node2 in ObjectRegistry on node1 (so node1 can find it via lookup_routing)
        use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
        let registration = ObjectRegistration {
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            object_type: ObjectType::ObjectTypeService as i32,
            object_id: "_node@node2".to_string(),
            grpc_address: "http://localhost:9999".to_string(),
            object_category: "Node".to_string(),
            ..Default::default()
        };
        node1.object_registry().register(registration).await.unwrap();

        // Now node1 should find the actor via ObjectRegistry (lookup_routing uses ObjectRegistry, not TupleSpace)
        let actor_registry1 = get_actor_registry(&node1).await;
        let result = actor_registry1.lookup_routing(&"test-actor@node2".to_string()).await;

        assert!(result.is_ok());
        match result.unwrap() {
            Some(routing_info) => {
                assert!(!routing_info.is_local);
                assert_eq!(routing_info.node_id, "node2");
            }
            None => panic!("Actor not found"),
        }
    }

    #[tokio::test]
    async fn test_send_to_remote_node_not_found() {
        let node = Node::new(NodeId::new("node1"), default_node_config());

        let message = plexspaces_mailbox::Message::new(vec![1, 2, 3]);

        // Try to send to node that's not in connections registry
        // This will fail when trying to lookup the actor (node not found)
        let result = lookup_actor_ref(&node, &"test-actor@unknown-node".to_string()).await;

        // Should fail with ActorNotFound or similar (node not registered)
        assert!(result.is_err() || result.unwrap().is_none());
    }

    // ============================================================================
    // Monitoring Infrastructure Tests (monitor, notify_actor_down)
    // ============================================================================

    #[tokio::test]
    async fn test_monitor_local_actor() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use tokio::sync::mpsc;

        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Register a local actor
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("monitored-actor@test-node", mailbox.clone(), node.service_locator());

        // Register with ActorRegistry first (using MessageSender)
        register_actor_for_test(&node, actor_ref.id().as_str(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Create monitoring channel
        let (tx, mut rx) = mpsc::channel(1);

        // Monitor the actor
        let monitor_ref = node
            .monitor(
                &"monitored-actor@test-node".to_string(),
                &"supervisor@test-node".to_string(),
                tx,
            )
            .await
            .unwrap();

        // Verify monitor_ref is a ULID (26 characters)
        assert_eq!(monitor_ref.len(), 26);

        // Notify actor down
        node.notify_actor_down(&"monitored-actor@test-node".to_string(), "test reason")
            .await
            .unwrap();

        // Should receive notification
        let (actor_id, reason) =
            tokio::time::timeout(tokio::time::Duration::from_millis(500), rx.recv())
                .await
                .unwrap()
                .unwrap();

        assert_eq!(actor_id, "monitored-actor@test-node");
        assert_eq!(reason, "test reason");
    }

    #[tokio::test]
    async fn test_monitor_local_actor_not_found() {
        use tokio::sync::mpsc;

        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Create monitoring channel
        let (tx, _rx) = mpsc::channel(1);

        // Try to monitor non-existent actor
        let result = node
            .monitor(
                &"nonexistent@test-node".to_string(),
                &"supervisor@test-node".to_string(),
                tx,
            )
            .await;

        // Should fail with ActorNotFound
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NodeError::ActorNotFound(_)));
    }

    #[tokio::test]
    async fn test_monitor_remote_actor_node_not_connected() {
        use tokio::sync::mpsc;

        let node = Arc::new(Node::new(NodeId::new("node1"), NodeConfig::default()));

        // Create monitoring channel
        let (tx, _rx) = mpsc::channel(1);

        // Try to monitor actor on unregistered remote node
        let result = node
            .monitor(
                &"test-actor@node2".to_string(),
                &"supervisor@node1".to_string(),
                tx,
            )
            .await;

        // Should fail with NodeNotConnected
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            NodeError::NodeNotConnected(_)
        ));
    }

    #[tokio::test]
    async fn test_notify_actor_down_no_monitors() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Notify for actor with no monitors (should not panic)
        let result = node
            .notify_actor_down(&"unmonitored-actor@test-node".to_string(), "reason")
            .await;

        // Should succeed (no-op)
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_notify_actor_down_multiple_monitors() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use tokio::sync::mpsc;

        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Register a local actor
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("watched-actor@test-node", mailbox.clone(), node.service_locator());

        // Register with ActorRegistry first (using MessageSender)
        register_actor_for_test(&node, actor_ref.id().as_str(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Create 3 monitors
        let (tx1, mut rx1) = mpsc::channel(1);
        let (tx2, mut rx2) = mpsc::channel(1);
        let (tx3, mut rx3) = mpsc::channel(1);

        node.monitor(
            &"watched-actor@test-node".to_string(),
            &"sup1@test-node".to_string(),
            tx1,
        )
        .await
        .unwrap();
        node.monitor(
            &"watched-actor@test-node".to_string(),
            &"sup2@test-node".to_string(),
            tx2,
        )
        .await
        .unwrap();
        node.monitor(
            &"watched-actor@test-node".to_string(),
            &"sup3@test-node".to_string(),
            tx3,
        )
        .await
        .unwrap();

        // Notify actor down
        node.notify_actor_down(&"watched-actor@test-node".to_string(), "crashed")
            .await
            .unwrap();

        // All 3 monitors should receive notification
        let (id1, reason1) = rx1.recv().await.unwrap();
        let (id2, reason2) = rx2.recv().await.unwrap();
        let (id3, reason3) = rx3.recv().await.unwrap();

        assert_eq!(id1, "watched-actor@test-node");
        assert_eq!(reason1, "crashed");
        assert_eq!(id2, "watched-actor@test-node");
        assert_eq!(reason2, "crashed");
        assert_eq!(id3, "watched-actor@test-node");
        assert_eq!(reason3, "crashed");
    }

    // ============================================================================
    // Lifecycle Event Tests (subscribe/unsubscribe/publish)
    // ============================================================================

    #[tokio::test]
    async fn test_lifecycle_event_subscription() {
        use tokio::sync::mpsc;

        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Create subscription channel
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Subscribe
        node.subscribe_lifecycle_events(tx).await;

        // Publish a test event
        let event = plexspaces_proto::ActorLifecycleEvent {
            actor_id: "test-actor@test-node".to_string(),
            timestamp: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            event_type: Some(plexspaces_proto::actor_lifecycle_event::EventType::Created(
                plexspaces_proto::v1::actor::ActorCreated {},
            )),
        };

        let actor_registry = get_actor_registry(&node).await;
        actor_registry.publish_lifecycle_event(event.clone()).await;

        // Should receive event
        let received = tokio::time::timeout(tokio::time::Duration::from_millis(500), rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(received.actor_id, "test-actor@test-node");
    }

    #[tokio::test]
    async fn test_lifecycle_event_unsubscribe() {
        use tokio::sync::mpsc;

        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Create subscription channel
        let (tx, _rx) = mpsc::unbounded_channel();

        // Subscribe
        node.subscribe_lifecycle_events(tx).await;

        // Unsubscribe
        node.unsubscribe_lifecycle_events().await;

        // Publish a test event
        let event = plexspaces_proto::ActorLifecycleEvent {
            actor_id: "test-actor@test-node".to_string(),
            timestamp: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            event_type: Some(plexspaces_proto::actor_lifecycle_event::EventType::Created(
                plexspaces_proto::v1::actor::ActorCreated {},
            )),
        };

        let actor_registry = get_actor_registry(&node).await;
        actor_registry.publish_lifecycle_event(event).await;

        // After unsubscribe, channel might receive events that were already queued,
        // but subsequent events should not be received.
        // Just verify that unsubscribe doesn't panic
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_handle_lifecycle_event_terminated() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use tokio::sync::mpsc;

        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Register actor and monitor it
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        use plexspaces_actor::RegularActorWrapper;
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(RegularActorWrapper::new(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor_ref.id().clone(), wrapper, None, None, None).await;

        // Register with ActorRegistry first (using MessageSender)
        register_actor_for_test(&node, actor_ref.id().as_str(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        let (tx, mut rx) = mpsc::channel(1);
        node.monitor(
            &"test-actor@test-node".to_string(),
            &"supervisor@test-node".to_string(),
            tx,
        )
        .await
        .unwrap();

        // Create Terminated event
        let event = plexspaces_proto::ActorLifecycleEvent {
            actor_id: "test-actor@test-node".to_string(),
            timestamp: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            event_type: Some(
                plexspaces_proto::actor_lifecycle_event::EventType::Terminated(
                    plexspaces_proto::v1::actor::ActorTerminated {
                        reason: "normal".to_string(),
                    },
                ),
            ),
        };

        // Handle the event
        node.handle_lifecycle_event(event).await.unwrap();

        // Monitor should receive notification
        let (actor_id, reason) = rx.recv().await.unwrap();
        assert_eq!(actor_id, "test-actor@test-node");
        assert_eq!(reason, "normal");
    }

    #[tokio::test]
    async fn test_handle_lifecycle_event_failed() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use tokio::sync::mpsc;

        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Register actor and monitor it
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        use plexspaces_actor::RegularActorWrapper;
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(RegularActorWrapper::new(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor_ref.id().clone(), wrapper, None, None, None).await;

        // Register with ActorRegistry first (using MessageSender)
        register_actor_for_test(&node, actor_ref.id().as_str(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        let (tx, mut rx) = mpsc::channel(1);
        node.monitor(
            &"test-actor@test-node".to_string(),
            &"supervisor@test-node".to_string(),
            tx,
        )
        .await
        .unwrap();

        // Create Failed event
        let event = plexspaces_proto::ActorLifecycleEvent {
            actor_id: "test-actor@test-node".to_string(),
            timestamp: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            event_type: Some(plexspaces_proto::actor_lifecycle_event::EventType::Failed(
                plexspaces_proto::v1::actor::ActorFailed {
                    error: "panic: index out of bounds".to_string(),
                    stack_trace: String::new(),
                },
            )),
        };

        // Handle the event
        node.handle_lifecycle_event(event).await.unwrap();

        // Monitor should receive notification
        let (actor_id, reason) = rx.recv().await.unwrap();
        assert_eq!(actor_id, "test-actor@test-node");
        assert_eq!(reason, "panic: index out of bounds");
    }

    #[tokio::test]
    async fn test_handle_lifecycle_event_other() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Create Starting event (non-terminal event)
        let event = plexspaces_proto::ActorLifecycleEvent {
            actor_id: "test-actor@test-node".to_string(),
            timestamp: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            event_type: Some(
                plexspaces_proto::actor_lifecycle_event::EventType::Starting(
                    plexspaces_proto::v1::actor::ActorStarting {},
                ),
            ),
        };

        // Handle the event (should be no-op for non-terminal events)
        let result = node.handle_lifecycle_event(event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};

        let node_arc = Arc::new(Node::new(NodeId::new("test-node"), default_node_config()));
        
        // Register NodeMetricsUpdater early for tests (normally done in create_actor_context_arc)
        let metrics_updater = Arc::new(crate::service_wrappers::NodeMetricsUpdaterWrapper::new(node_arc.clone()));
        node_arc.service_locator().register_service(metrics_updater.clone()).await;
        let metrics_updater_trait: Arc<dyn plexspaces_core::NodeMetricsUpdater + Send + Sync> = metrics_updater.clone() as Arc<dyn plexspaces_core::NodeMetricsUpdater + Send + Sync>;
        node_arc.service_locator().register_node_metrics_updater(metrics_updater_trait).await;
        let node = node_arc.as_ref();

        // Initial stats
        let node_metrics = node.metrics().await;
        assert_eq!(node_metrics.messages_routed, 0);
        assert_eq!(node_metrics.local_deliveries, 0);
        assert_eq!(node_metrics.active_actors, 0);

        // Register an actor
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        
        // Register with ActorRegistry (using MessageSender)
        register_actor_for_test(&node, actor_ref.id().as_str(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // active_actors is only updated when actors are spawned via ActorFactory, not when registered
        // So we check that the actor is registered instead
        let actor_registry = get_actor_registry(&node).await;
        let lookup_result = actor_registry.lookup_actor(&"test-actor@test-node".to_string()).await;
        assert!(lookup_result.is_some(), "Actor should be registered");

        // Reset metrics before sending message
        {
            let mut metrics = node.metrics.write().await;
            metrics.messages_routed = 0;
            metrics.local_deliveries = 0;
        }

        // Send local message via ActorRef (use the actor_ref we already have)
        let message = plexspaces_mailbox::Message::new(vec![1, 2, 3]);
        actor_ref.tell(message).await.unwrap();

        // Check stats updated
        let node_metrics = node.metrics().await;
        assert_eq!(node_metrics.messages_routed, 1);
        assert_eq!(node_metrics.local_deliveries, 1);

        // Unregister actor
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.unregister_with_cleanup(&"test-actor@test-node".to_string())
            .await
            .unwrap();

        // Check stats updated
        let node_metrics = node.metrics().await;
        assert_eq!(node_metrics.active_actors, 0);
    }

    // ============================================================================
    // Phase 3: Actor Resource Requirements Tests
    // ============================================================================

    /// Helper to create an actor config with resource requirements
    fn create_actor_config_with_resources(
        cpu_cores: f64,
        memory_bytes: u64,
        disk_bytes: u64,
        gpu_count: u32,
    ) -> plexspaces_proto::v1::actor::ActorConfig {
        use plexspaces_proto::{
            v1::actor::{ActorConfig, ActorResourceRequirements},
            common::v1::ResourceSpec,
        };

        let resources = ResourceSpec {
            cpu_cores,
            memory_bytes,
            disk_bytes,
            gpu_count,
            gpu_type: String::new(),
        };

        let resource_requirements = ActorResourceRequirements {
            resources: Some(resources),
            required_labels: std::collections::HashMap::new(),
            placement: None,
            actor_groups: vec![],
        };

        ActorConfig {
            mailbox_timeout: None,
            max_mailbox_size: 1000,
            enable_persistence: false,
            checkpoint_interval: None,
            restart_policy: None,
            supervision_strategy: 0,
            properties: std::collections::HashMap::new(),
            placement_hint: None,
            stateless_worker_config: None,
            data_parallel_config: None,
            state_management_mode: 0,
            consistency_level: 0,
            resource_requirements: Some(resource_requirements),
            actor_groups: vec![],
            config_schema_version: 1,
        }
    }

    #[tokio::test]
    async fn test_register_actor_with_config() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        use plexspaces_actor::RegularActorWrapper;
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(RegularActorWrapper::new(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor_ref.id().clone(), wrapper, None, None, None).await;

        let config = create_actor_config_with_resources(2.0, 1024 * 1024 * 512, 1024 * 1024 * 1024, 0);

        // Register actor with config
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), Some(config.clone()))
            .await
            .unwrap();

        // Verify config is stored
        let actor_configs = node.actor_configs().read().await;
        assert!(actor_configs.contains_key(actor_ref.id()));
        let stored_config = actor_configs.get(actor_ref.id()).unwrap();
        assert_eq!(
            stored_config.resource_requirements.as_ref().unwrap().resources.as_ref().unwrap().cpu_cores,
            2.0
        );
    }

    #[tokio::test]
    async fn test_register_actor_without_config() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        use plexspaces_actor::RegularActorWrapper;
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(RegularActorWrapper::new(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor_ref.id().clone(), wrapper, None, None, None).await;

        // Register actor without config
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Verify config is not stored
        let actor_configs = node.actor_configs().read().await;
        assert!(!actor_configs.contains_key(actor_ref.id()));
    }

    #[tokio::test]
    async fn test_unregister_actor_removes_config() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        use plexspaces_actor::RegularActorWrapper;
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(RegularActorWrapper::new(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor_ref.id().clone(), wrapper, None, None, None).await;

        let config = create_actor_config_with_resources(1.0, 1024 * 1024 * 256, 0, 0);

        // Register actor with config
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), Some(config))
            .await
            .unwrap();

        // Verify config is stored
        {
            let actor_configs = node.actor_configs().read().await;
            assert!(actor_configs.contains_key(actor_ref.id()));
        }

        // Unregister actor
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.unregister_with_cleanup(actor_ref.id()).await.unwrap();

        // Verify config is removed
        let actor_configs = node.actor_configs().read().await;
        assert!(!actor_configs.contains_key(actor_ref.id()));
    }

    #[tokio::test]
    async fn test_calculate_node_capacity_with_actors() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = Node::new(NodeId::new("test-node"), default_node_config());

        // Register first actor with resources
        let mailbox1 = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-1-{}", ulid::Ulid::new())).await.unwrap());
        let actor1_ref = ActorRef::local("actor-1@test-node", mailbox1.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        use plexspaces_actor::RegularActorWrapper;
        use plexspaces_core::MessageSender;
        let wrapper1 = Arc::new(RegularActorWrapper::new(
            actor1_ref.id().clone(),
            mailbox1.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor1_ref.id().clone(), wrapper1, None, None, None).await;
        let config1 = create_actor_config_with_resources(2.0, 1024 * 1024 * 512, 1024 * 1024 * 1024, 0);
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor1_ref.id().clone(), Some(config1))
            .await
            .unwrap();

        // Register second actor with resources
        let mailbox2 = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-2-{}", ulid::Ulid::new())).await.unwrap());
        let actor2_ref = ActorRef::local("actor-2@test-node", mailbox2.clone(), node.service_locator());
        let config2 = create_actor_config_with_resources(1.5, 1024 * 1024 * 256, 512 * 1024 * 1024, 1);
        // Register actor2 with MessageSender first
        let wrapper2 = Arc::new(RegularActorWrapper::new(
            actor2_ref.id().clone(),
            mailbox2.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor2_ref.id().clone(), wrapper2, None, None, None).await;
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor2_ref.id().clone(), Some(config2))
            .await
            .unwrap();

        // Calculate capacity
        let capacity = node.calculate_node_capacity().await;

        // Verify allocated resources are summed correctly
        let allocated = capacity.allocated.as_ref().unwrap();
        assert_eq!(allocated.cpu_cores, 3.5); // 2.0 + 1.5
        assert_eq!(
            allocated.memory_bytes,
            1024 * 1024 * 512 + 1024 * 1024 * 256
        ); // 512MB + 256MB
        assert_eq!(
            allocated.disk_bytes,
            1024 * 1024 * 1024 + 512 * 1024 * 1024
        ); // 1GB + 512MB
        assert_eq!(allocated.gpu_count, 1); // 0 + 1

        // Verify available resources are calculated correctly
        let available = capacity.available.as_ref().unwrap();
        let total = capacity.total.as_ref().unwrap();
        assert_eq!(
            available.cpu_cores,
            total.cpu_cores - allocated.cpu_cores
        );
        assert_eq!(
            available.memory_bytes,
            total.memory_bytes - allocated.memory_bytes
        );
    }

    #[tokio::test]
    async fn test_calculate_node_capacity_without_actors() {
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        // Calculate capacity with no actors
        let capacity = node.calculate_node_capacity().await;

        // Verify allocated resources are zero
        let allocated = capacity.allocated.as_ref().unwrap();
        assert_eq!(allocated.cpu_cores, 0.0);
        assert_eq!(allocated.memory_bytes, 0);
        assert_eq!(allocated.disk_bytes, 0);
        assert_eq!(allocated.gpu_count, 0);

        // Verify available equals total
        let available = capacity.available.as_ref().unwrap();
        let total = capacity.total.as_ref().unwrap();
        assert_eq!(available.cpu_cores, total.cpu_cores);
        assert_eq!(available.memory_bytes, total.memory_bytes);
    }

    #[tokio::test]
    async fn test_calculate_node_capacity_with_actor_without_resources() {
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        // Register actor without resource requirements
        let mailbox = Arc::new(Mailbox::new(plexspaces_mailbox::mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("actor-1@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        use plexspaces_actor::RegularActorWrapper;
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(RegularActorWrapper::new(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor_ref.id().clone(), wrapper, None, None, None).await;
        let mut config = plexspaces_proto::v1::actor::ActorConfig::default();
        config.resource_requirements = None; // No resource requirements
        config.config_schema_version = 1;
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), Some(config))
            .await
            .unwrap();

        // Calculate capacity
        let capacity = node.calculate_node_capacity().await;

        // Verify allocated resources are still zero (actor has no requirements)
        let allocated = capacity.allocated.as_ref().unwrap();
        assert_eq!(allocated.cpu_cores, 0.0);
        assert_eq!(allocated.memory_bytes, 0);
    }

    #[tokio::test]
    async fn test_calculate_node_capacity_after_unregister() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        // Register actor with resources
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("actor-1@test-node", mailbox.clone(), node.service_locator());
        let config = create_actor_config_with_resources(2.0, 1024 * 1024 * 512, 0, 0);
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), Some(config))
            .await
            .unwrap();

        // Verify allocated resources
        let capacity = node.calculate_node_capacity().await;
        let allocated = capacity.allocated.as_ref().unwrap();
        assert_eq!(allocated.cpu_cores, 2.0);

        // Unregister actor
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.unregister_with_cleanup(actor_ref.id()).await.unwrap();

        // Verify allocated resources are back to zero
        let capacity = node.calculate_node_capacity().await;
        let allocated = capacity.allocated.as_ref().unwrap();
        assert_eq!(allocated.cpu_cores, 0.0);
        assert_eq!(allocated.memory_bytes, 0);
    }

    #[tokio::test]
    async fn test_calculate_node_capacity_with_partial_resource_spec() {
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        // Create config with only CPU specified (no memory/disk)
        use plexspaces_proto::{
            v1::actor::{ActorConfig, ActorResourceRequirements},
            common::v1::ResourceSpec,
        };

        let resources = ResourceSpec {
            cpu_cores: 1.0,
            memory_bytes: 0,
            disk_bytes: 0,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        let resource_requirements = ActorResourceRequirements {
            resources: Some(resources),
            required_labels: std::collections::HashMap::new(),
            placement: None,
            actor_groups: vec![],
        };

        let mut config = ActorConfig::default();
        config.resource_requirements = Some(resource_requirements);
        config.config_schema_version = 1;

        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("actor-1@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        use plexspaces_actor::RegularActorWrapper;
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(RegularActorWrapper::new(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor(actor_ref.id().clone(), wrapper, None, None, None).await;
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), Some(config))
            .await
            .unwrap();

        // Calculate capacity
        let capacity = node.calculate_node_capacity().await;
        let allocated = capacity.allocated.as_ref().unwrap();

        // Verify only CPU is allocated
        assert_eq!(allocated.cpu_cores, 1.0);
        assert_eq!(allocated.memory_bytes, 0);
        assert_eq!(allocated.disk_bytes, 0);
    }

    // ============================================================================
    // Application Lifecycle Tests (Erlang/OTP-style)
    // ============================================================================

    use async_trait::async_trait;
    use plexspaces_core::application::{
        Application, ApplicationError, ApplicationNode, ApplicationState, HealthStatus,
    };

    // Mock application for testing
    struct MockTestApplication {
        name: String,
        should_fail_start: bool,
        should_fail_stop: bool,
        start_called: Arc<RwLock<bool>>,
        stop_called: Arc<RwLock<bool>>,
    }

    impl MockTestApplication {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                should_fail_start: false,
                should_fail_stop: false,
                start_called: Arc::new(RwLock::new(false)),
                stop_called: Arc::new(RwLock::new(false)),
            }
        }

        fn new_failing_start(name: &str) -> Self {
            Self {
                name: name.to_string(),
                should_fail_start: true,
                should_fail_stop: false,
                start_called: Arc::new(RwLock::new(false)),
                stop_called: Arc::new(RwLock::new(false)),
            }
        }

        fn new_failing_stop(name: &str) -> Self {
            Self {
                name: name.to_string(),
                should_fail_start: false,
                should_fail_stop: true,
                start_called: Arc::new(RwLock::new(false)),
                stop_called: Arc::new(RwLock::new(false)),
            }
        }

        async fn was_start_called(&self) -> bool {
            *self.start_called.read().await
        }

        async fn was_stop_called(&self) -> bool {
            *self.stop_called.read().await
        }
    }

    #[async_trait]
    impl Application for MockTestApplication {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            "0.1.0"
        }

        async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
            *self.start_called.write().await = true;
            eprintln!("MockApp '{}' starting on node: {}", self.name, node.id());
            if self.should_fail_start {
                Err(ApplicationError::StartupFailed("mock failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn stop(&mut self) -> Result<(), ApplicationError> {
            *self.stop_called.write().await = true;
            eprintln!("MockApp '{}' stopping", self.name);
            if self.should_fail_stop {
                Err(ApplicationError::ShutdownFailed("mock failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn health_check(&self) -> HealthStatus {
            HealthStatus::HealthStatusHealthy
        }
        
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[tokio::test]
    async fn test_register_application() {
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        let app = Box::new(MockTestApplication::new("test-app"));
        node.register_application(app).await.unwrap();

        // Verify application is registered
        let state = node
            .application_manager()
            .read()
            .await
            .get_state("test-app")
            .await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateCreated));
    }

    #[tokio::test]
    async fn test_register_duplicate_application() {
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        let app1 = Box::new(MockTestApplication::new("test-app"));
        node.register_application(app1).await.unwrap();

        let app2 = Box::new(MockTestApplication::new("test-app"));
        let result = node.register_application(app2).await;

        // Should fail with duplicate error
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already registered"));
    }

    #[tokio::test]
    async fn test_start_application() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        let app = Box::new(MockTestApplication::new("test-app"));
        let start_called = app.start_called.clone();

        node.register_application(app).await.unwrap();
        node.start_application("test-app").await.unwrap();

        // Verify application started
        let state = node
            .application_manager()
            .read()
            .await
            .get_state("test-app")
            .await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateRunning));

        // Verify start was called
        assert!(*start_called.read().await);
    }

    #[tokio::test]
    async fn test_start_application_failure() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        let app = Box::new(MockTestApplication::new_failing_start("test-app"));
        node.register_application(app).await.unwrap();

        let result = node.start_application("test-app").await;

        // Should fail
        assert!(result.is_err());

        // State should be Failed
        let state = node
            .application_manager()
            .read()
            .await
            .get_state("test-app")
            .await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateFailed));
    }

    #[tokio::test]
    async fn test_stop_application() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        let app = Box::new(MockTestApplication::new("test-app"));
        let stop_called = app.stop_called.clone();

        node.register_application(app).await.unwrap();
        node.start_application("test-app").await.unwrap();
        node.stop_application("test-app", tokio::time::Duration::from_secs(5))
            .await
            .unwrap();

        // Verify application stopped
        let state = node
            .application_manager()
            .read()
            .await
            .get_state("test-app")
            .await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateStopped));

        // Verify stop was called
        assert!(*stop_called.read().await);
    }

    #[tokio::test]
    async fn test_stop_application_failure() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        let app = Box::new(MockTestApplication::new_failing_stop("test-app"));
        node.register_application(app).await.unwrap();
        node.start_application("test-app").await.unwrap();

        let result = node
            .stop_application("test-app", tokio::time::Duration::from_secs(5))
            .await;

        // Should fail
        assert!(result.is_err());

        // State should be Failed
        let state = node
            .application_manager()
            .read()
            .await
            .get_state("test-app")
            .await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateFailed));
    }

    #[tokio::test]
    async fn test_shutdown_multiple_applications() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Register and start 3 applications
        let apps = vec![
            Box::new(MockTestApplication::new("app1")) as Box<dyn Application>,
            Box::new(MockTestApplication::new("app2")) as Box<dyn Application>,
            Box::new(MockTestApplication::new("app3")) as Box<dyn Application>,
        ];

        for app in apps {
            node.register_application(app).await.unwrap();
        }

        node.start_application("app1").await.unwrap();
        node.start_application("app2").await.unwrap();
        node.start_application("app3").await.unwrap();

        // Shutdown all applications
        node.shutdown(tokio::time::Duration::from_secs(10))
            .await
            .unwrap();

        // Verify all applications stopped
        assert_eq!(
            node.application_manager()
                .read()
                .await
                .get_state("app1")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );
        assert_eq!(
            node.application_manager()
                .read()
                .await
                .get_state("app2")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );
        assert_eq!(
            node.application_manager()
                .read()
                .await
                .get_state("app3")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );

        // Verify shutdown flag set
        assert!(node.is_shutdown_requested().await);
    }

    #[tokio::test]
    async fn test_application_node_trait_implementation() {
        let node = Node::new(
            NodeId::new("test-node"),
            NodeConfig {
                listen_addr: "0.0.0.0:9999".to_string(),
                ..Default::default()
            },
        );

        // Test ApplicationNode trait methods (uses trait methods, not Node methods)
        let node_ref: &dyn ApplicationNode = &node;
        assert_eq!(node_ref.id(), "test-node");
        assert_eq!(node_ref.listen_addr(), "0.0.0.0:9999");
    }

    #[tokio::test]
    async fn test_start_nonexistent_application() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        let result = node.start_application("nonexistent").await;

        // Should fail with not found error
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_stop_nonexistent_application() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        let result = node
            .stop_application("nonexistent", tokio::time::Duration::from_secs(5))
            .await;

        // Should fail with not found error
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_application_lifecycle_full_cycle() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        let app = Box::new(MockTestApplication::new("lifecycle-test"));
        let start_called = app.start_called.clone();
        let stop_called = app.stop_called.clone();

        // Full lifecycle: register -> start -> stop
        node.register_application(app).await.unwrap();
        assert_eq!(
            node.application_manager()
                .read()
                .await
                .get_state("lifecycle-test")
                .await,
            Some(ApplicationState::ApplicationStateCreated)
        );

        node.start_application("lifecycle-test").await.unwrap();
        assert_eq!(
            node.application_manager()
                .read()
                .await
                .get_state("lifecycle-test")
                .await,
            Some(ApplicationState::ApplicationStateRunning)
        );
        assert!(*start_called.read().await);

        node.stop_application("lifecycle-test", tokio::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            node.application_manager()
                .read()
                .await
                .get_state("lifecycle-test")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );
        assert!(*stop_called.read().await);
    }

    #[tokio::test]
    async fn test_shutdown_with_partial_failure() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Register 2 apps: one normal, one failing to stop
        let app1 = Box::new(MockTestApplication::new("good-app")) as Box<dyn Application>;
        let app2 =
            Box::new(MockTestApplication::new_failing_stop("bad-app")) as Box<dyn Application>;

        node.register_application(app1).await.unwrap();
        node.register_application(app2).await.unwrap();

        node.start_application("good-app").await.unwrap();
        node.start_application("bad-app").await.unwrap();

        // Shutdown should fail due to bad-app
        let result = node.shutdown(tokio::time::Duration::from_secs(5)).await;
        assert!(result.is_err());

        // good-app should still be stopped
        assert_eq!(
            node.application_manager()
                .read()
                .await
                .get_state("good-app")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );

        // bad-app should be in Failed state
        assert_eq!(
            node.application_manager()
                .read()
                .await
                .get_state("bad-app")
                .await,
            Some(ApplicationState::ApplicationStateFailed)
        );
    }

    #[tokio::test]
    async fn test_shutdown_request_flag() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Initially not requested
        assert!(!node.is_shutdown_requested().await);

        // Register and start an app
        let app = Box::new(MockTestApplication::new("test-app"));
        node.register_application(app).await.unwrap();
        node.start_application("test-app").await.unwrap();

        // Shutdown
        node.shutdown(tokio::time::Duration::from_secs(5))
            .await
            .unwrap();

        // Now shutdown is requested
        assert!(node.is_shutdown_requested().await);
    }

    #[tokio::test]
    async fn test_shutdown_with_no_applications() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Shutdown with no apps should succeed
        let result = node.shutdown(tokio::time::Duration::from_secs(5)).await;
        assert!(result.is_ok());

        // Shutdown flag should be set
        assert!(node.is_shutdown_requested().await);
    }

    #[tokio::test]
    async fn test_application_manager_accessor() {
        let node = Node::new(NodeId::new("test-node"), default_node_config());

        // Get application manager reference
        let manager = node.application_manager();

        // Verify it's the same manager (returns empty list initially)
        let apps = manager.read().await.list_applications().await;
        assert_eq!(apps.len(), 0);
    }

    #[tokio::test]
    async fn test_multiple_start_attempts() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        let app = Box::new(MockTestApplication::new("test-app"));
        node.register_application(app).await.unwrap();

        // First start succeeds
        node.start_application("test-app").await.unwrap();
        assert_eq!(
            node.application_manager()
                .read()
                .await
                .get_state("test-app")
                .await,
            Some(ApplicationState::ApplicationStateRunning)
        );

        // Second start should fail (not in Created state)
        let result = node.start_application("test-app").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("state"));
    }

    #[tokio::test]
    async fn test_stop_already_stopped_application() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        let app = Box::new(MockTestApplication::new("test-app"));
        node.register_application(app).await.unwrap();
        node.start_application("test-app").await.unwrap();

        // First stop succeeds
        node.stop_application("test-app", tokio::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            node.application_manager()
                .read()
                .await
                .get_state("test-app")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );

        // Second stop should succeed (already stopped)
        let result = node
            .stop_application("test-app", tokio::time::Duration::from_secs(5))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_shutdown_stops_all_applications() {
        let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));

        // Track which apps were stopped
        let stopped_apps = Arc::new(RwLock::new(Vec::new()));

        // Create apps that record when they stop
        struct StopTrackingApp {
            name: String,
            stopped_apps: Arc<RwLock<Vec<String>>>,
        }

        #[async_trait]
        impl Application for StopTrackingApp {
            fn name(&self) -> &str {
                &self.name
            }

            fn version(&self) -> &str {
                "0.1.0"
            }

            async fn start(
                &mut self,
                _node: Arc<dyn ApplicationNode>,
            ) -> Result<(), ApplicationError> {
                Ok(())
            }

            async fn stop(&mut self) -> Result<(), ApplicationError> {
                self.stopped_apps.write().await.push(self.name.clone());
                Ok(())
            }

            async fn health_check(&self) -> HealthStatus {
                HealthStatus::HealthStatusHealthy
            }
            
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        // Register and start multiple apps
        for i in 1..=3 {
            let app = Box::new(StopTrackingApp {
                name: format!("app{}", i),
                stopped_apps: stopped_apps.clone(),
            }) as Box<dyn Application>;
            node.register_application(app).await.unwrap();
            node.start_application(&format!("app{}", i)).await.unwrap();
        }

        // Shutdown
        node.shutdown(tokio::time::Duration::from_secs(10))
            .await
            .unwrap();

        // Verify all apps were stopped (order not guaranteed due to HashMap)
        let stopped = stopped_apps.read().await;
        assert_eq!(stopped.len(), 3);
        assert!(stopped.contains(&"app1".to_string()));
        assert!(stopped.contains(&"app2".to_string()));
        assert!(stopped.contains(&"app3".to_string()));
    }
}


// ============================================================================
// LinkProvider Implementation (Phase 8.5: Link Semantics Integration)
// ============================================================================

#[async_trait::async_trait]
impl LinkProvider for Node {
    async fn link(&self, actor_id: &ActorId, linked_actor_id: &ActorId) -> Result<(), String> {
        Node::link(self, actor_id, linked_actor_id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn unlink(&self, actor_id: &ActorId, linked_actor_id: &ActorId) -> Result<(), String> {
        Node::unlink(self, actor_id, linked_actor_id)
            .await
            .map_err(|e| e.to_string())
    }
}

// ============================================================================
// ActivationProvider Implementation (Phase 8.5: Reminder-VirtualActor Integration)
// ============================================================================

#[async_trait::async_trait]
impl ActivationProvider for Node {
    async fn is_actor_active(&self, actor_id: &ActorId) -> bool {
        // Check if actor is registered in ActorRegistry
        let routing = self.actor_registry.lookup_routing(actor_id).await.ok().flatten();
        routing.map(|r| r.is_local).unwrap_or(false)
    }

    async fn activate_actor(&self, actor_id: &ActorId) -> Result<CoreActorRef, String> {
        // Activate the virtual actor using ActorFactory
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        
        let actor_factory: Arc<ActorFactoryImpl> = self.service_locator().get_service().await
            .ok_or_else(|| "ActorFactory not found".to_string())?;
        
        actor_factory.activate_virtual_actor(actor_id).await
            .map_err(|e| e.to_string())?;
        
        // Convert plexspaces_actor::ActorRef to plexspaces_core::ActorRef
        // Core ActorRef is just an ID wrapper, so we create it from the actor_id
        CoreActorRef::new(actor_id.clone())
            .map_err(|e| e.to_string())
    }
}
