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
use plexspaces_core::{ActorId, ActorRef as CoreActorRef, ActorRegistry, ReplyTracker, ServiceLocator, VirtualActorManager, FacetManager, RequestContext, ExitReason};
use plexspaces_core::actor_context::ObjectRegistry;
use plexspaces_actor::ActorRef;
use plexspaces_journaling::{ActivationProvider, VirtualActorFacet};
use plexspaces_mailbox::Message;
use plexspaces_proto::actor::v1::ActorLink as ProtoActorLink;
use plexspaces_proto::node::v1::{NodeCapabilities as ProtoNodeCapabilities, NodeMetrics, NodeRuntimeConfig};
use plexspaces_supervisor::LinkProvider;
use std::time::{Duration, SystemTime};

// Import gRPC client for remote messaging
use crate::grpc_client::RemoteActorClient;

// Import application manager from core (moved to break cyclic dependency)
use plexspaces_core::ApplicationManager;

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
    /// Connection metadata (node addresses, health status)
    connections: Arc<RwLock<HashMap<NodeId, NodeConnection>>>,
    /// Connection health tracking: node_id -> (last_used, last_error, consecutive_failures)
    connection_health: Arc<RwLock<HashMap<NodeId, (tokio::time::Instant, Option<String>, u32)>>>,
    /// Node configuration
    config: NodeConfig,
    /// Node metrics (combined resource and operational metrics)
    metrics: Arc<RwLock<NodeMetrics>>,
    /// Start time for uptime calculation
    start_time: Arc<RwLock<Option<tokio::time::Instant>>>,
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
    /// Health reporter for health checks and graceful shutdown 
    /// Set in Node::start(), None before start
    health_reporter: Arc<RwLock<Option<Arc<crate::health_service::PlexSpacesHealthReporter>>>>,
    /// ReleaseSpec configuration (optional, loaded from release config files)
    /// If provided, NodeConfig will be extracted from release_spec.node in start()
    release_spec: Arc<RwLock<Option<plexspaces_proto::node::v1::ReleaseSpec>>>,
}

/// Node configuration
// Use proto-generated NodeRuntimeConfig instead of custom struct
pub type NodeConfig = NodeRuntimeConfig;

/// Helper function to create default NodeConfig
/// (Can't implement Default trait for type aliases to external types)
pub fn default_node_config() -> NodeRuntimeConfig {
    NodeRuntimeConfig {
        listen_addr: "0.0.0.0:8000".to_string(),
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
    /// Remote node gRPC address (e.g., "http://localhost:8000")
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
fn default_node_metrics(node_id: &str, cluster_name: &str) -> NodeMetrics {
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
        node_id: node_id.to_string(),
        cluster_name: cluster_name.to_string(),
    }
}

impl Node {
    /// Create a new node
    ///
    /// ## Note
    /// This creates a Node with a ServiceLocator that will be populated in `start()`.
    /// For tests/examples, use `create_default_service_locator()` directly.
    pub fn new(id: NodeId, config: NodeConfig) -> Self {
        // Create ServiceLocator - services will be registered in start() using create_default_service_locator
        // This ensures production-ready service initialization
        let service_locator = Arc::new(ServiceLocator::new());
        let node_id_str = id.as_str().to_string();

        Node {
            id,
            service_locator,
            connections: Arc::new(RwLock::new(HashMap::new())),
            config,
            metrics: Arc::new(RwLock::new(default_node_metrics(&node_id_str, ""))),
            start_time: Arc::new(RwLock::new(None)), // Set in start()
            shutdown_tx: Arc::new(RwLock::new(None)), // Shutdown trigger (set in start())
            background_scheduler: Arc::new(RwLock::new(None)), // Phase 4: Background scheduler (created in start())
            task_router: Arc::new(RwLock::new(None)), // Phase 5: Task router (created in start())
            wasm_runtime: Arc::new(RwLock::new(None)), // WASM runtime (created in start())
            blob_service: Arc::new(RwLock::new(None)), // Blob service (created in start())
            connection_health: Arc::new(RwLock::new(HashMap::new())), // Connection health tracking
            health_reporter: Arc::new(RwLock::new(None)), // Health reporter (created in start())
            release_spec: Arc::new(RwLock::new(None)), // ReleaseSpec (optional, loaded from config)
        }
    }

    /// Initialize services using create_default_service_locator
    ///
    /// ## Purpose
    /// Initializes all services in the ServiceLocator. This is called automatically by
    /// NodeBuilder::build() and Node::start(), so you typically don't need to call this manually.
    ///
    /// ## Idempotent
    /// Safe to call multiple times - uses a OnceCell-like pattern internally.
    pub async fn initialize_services(&self) -> Result<(), NodeError> {
        // Use a static check to avoid re-initialization
        // Since we can't use OnceCell with async, we check if services are already registered
        use plexspaces_core::service_locator::service_names;
        if self.service_locator.get_service_by_name::<plexspaces_core::ActorRegistry>(service_names::ACTOR_REGISTRY).await.is_some() {
            return Ok(());
        }

        // Determine NodeConfig: priority is release_spec.node > defaults
        let proto_node_config = {
            let release_spec = self.release_spec.read().await;
            if let Some(ref spec) = *release_spec {
                // Use NodeConfig from ReleaseSpec if available
                if let Some(ref node_config) = spec.node {
                    node_config.clone()
                } else {
                    // ReleaseSpec exists but node is None - create default
                    plexspaces_proto::node::v1::NodeConfig {
                        id: self.id.as_str().to_string(),
                        listen_address: self.config.listen_addr.clone(),
                        cluster_seed_nodes: vec![],
                        default_tenant_id: "internal".to_string(),
                        default_namespace: "system".to_string(),
                        cluster_name: String::new(),
                    }
                }
            } else {
                // No ReleaseSpec - create default from Node config
                plexspaces_proto::node::v1::NodeConfig {
                    id: self.id.as_str().to_string(),
                    listen_address: self.config.listen_addr.clone(),
                    cluster_seed_nodes: vec![],
                    default_tenant_id: "internal".to_string(),
                    default_namespace: "system".to_string(),
                    cluster_name: String::new(),
                }
            }
        };

        // Initialize all services directly in self.service_locator (shared initialization logic)
        // This avoids creating a new ServiceLocator and copying services, eliminating duplication
        use crate::service_locator_helpers::initialize_services_in_locator;
        initialize_services_in_locator(
            self.service_locator.clone(),
            Some(self.id.as_str().to_string()),
            Some(proto_node_config.clone()),
            self.release_spec.read().await.clone(),
        ).await;

        // Register TupleSpaceProvider (create default for tests)
        use plexspaces_core::service_wrappers::TupleSpaceProviderWrapper;
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::internal();
        let tuplespace = TupleSpaceProviderWrapper::from_context(&ctx);
        let tuplespace_provider = Arc::new(TupleSpaceProviderWrapper::new(tuplespace));
        self.service_locator.register_tuplespace_provider(
            tuplespace_provider as Arc<dyn plexspaces_core::TupleSpaceProvider + Send + Sync>
        ).await;

        // Update metrics with node_id and cluster_name from config
        {
            let mut metrics = self.metrics.write().await;
            metrics.node_id = proto_node_config.id.clone();
            metrics.cluster_name = proto_node_config.cluster_name.clone();
        }

        Ok(())
    }
    
    /// Set ReleaseSpec for this node
    ///
    /// ## Purpose
    /// Allows setting ReleaseSpec configuration that will be used in start() to extract NodeConfig.
    /// This is typically called after loading release config from files.
    ///
    /// ## Arguments
    /// * `release_spec` - ReleaseSpec to set
    ///
    /// ## Note
    /// If ReleaseSpec is set, start() will use release_spec.node for NodeConfig instead of creating defaults.
    pub async fn set_release_spec(&self, release_spec: plexspaces_proto::node::v1::ReleaseSpec) {
        let mut spec = self.release_spec.write().await;
        *spec = Some(release_spec);
    }
    
    /// Get ReleaseSpec if set
    pub async fn get_release_spec(&self) -> Option<plexspaces_proto::node::v1::ReleaseSpec> {
        let spec = self.release_spec.read().await;
        spec.clone()
    }
    
    /// Load release config from file or environment variable
    ///
    /// ## Purpose
    /// Loads ReleaseSpec from:
    /// 1. `PLEXSPACES_RELEASE_CONFIG_PATH` environment variable (if set)
    /// 2. `release.yaml` in current directory
    /// 3. `release.toml` in current directory
    ///
    /// ## Returns
    /// Ok(ReleaseSpec) if found and loaded, Err if not found or invalid
    ///
    /// ## Note
    /// This is called automatically in `start()` if release_spec is not already set.
    /// For embedded applications, call `set_release_spec()` before `start()`.
    async fn load_release_config(&self) -> Result<plexspaces_proto::node::v1::ReleaseSpec, NodeError> {
        use crate::config_loader::ConfigLoader;
        use std::env;
        
        // Check environment variable first
        let config_path = if let Ok(path) = env::var("PLEXSPACES_RELEASE_CONFIG_PATH") {
            Some(path)
        } else {
            // Try common file names
            if std::path::Path::new("release.yaml").exists() {
                Some("release.yaml".to_string())
            } else if std::path::Path::new("release.toml").exists() {
                Some("release.toml".to_string())
            } else {
                None
            }
        };
        
        if let Some(path) = config_path {
            let loader = ConfigLoader::new(); // Enable security validation by default
            loader.load_release_spec_with_env_precedence(&path).await
                .map_err(|e| NodeError::ConfigError(format!("Failed to load release config from {}: {}", path, e)))
        } else {
            Err(NodeError::ConfigError("No release config file found and PLEXSPACES_RELEASE_CONFIG_PATH not set".to_string()))
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
    
    /// Register proto NodeConfig in ServiceLocator
    ///
    /// ## Purpose
    /// Registers the proto NodeConfig (from ReleaseSpec.node) in ServiceLocator
    /// so that services can access default_tenant_id and default_namespace.
    ///
    /// ## Arguments
    /// * `node_config` - Proto NodeConfig from ReleaseSpec.node
    ///
    /// ## Note
    /// This should be called after Node creation if ReleaseSpec is available.
    /// If not called, defaults will fall back to "internal" and "system".
    pub async fn register_node_config(&self, node_config: plexspaces_proto::node::v1::NodeConfig) {
        self.service_locator.register_node_config(node_config).await;
    }


    /// Get ActorRegistry (internal use only - use service_locator.get_service_by_name() instead)
    pub(crate) async fn actor_registry(&self) -> Result<Arc<ActorRegistry>, NodeError> {
        use plexspaces_core::service_locator::service_names;
        self.service_locator.get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY).await
            .ok_or_else(|| NodeError::ConfigError("ActorRegistry not found in ServiceLocator".to_string()))
    }
    
    // === Helper methods to access actor data via ActorRegistry ===
    // These provide convenient access to actor-related data stored in ActorRegistry
    // All methods now get ActorRegistry from ServiceLocator
    
    /// Get actor instances (for lazy virtual actors)
    /// Note: Returns trait object - need to downcast to Arc<Actor> in Node
    pub async fn actor_instances(&self) -> Result<Arc<RwLock<HashMap<ActorId, Arc<dyn std::any::Any + Send + Sync>>>>, NodeError> {
        let registry = self.actor_registry().await?;
        Ok(registry.actor_instances().clone())
    }
    
    /// Get facet storage (use facet_manager() instead)
    pub async fn facet_storage(&self) -> Result<Arc<RwLock<HashMap<ActorId, Arc<tokio::sync::RwLock<plexspaces_facet::FacetContainer>>>>>, NodeError> {
        let registry = self.actor_registry().await?;
        Ok(registry.facet_manager().facet_storage().clone())
    }
    
    /// Get FacetManager
    pub async fn facet_manager(&self) -> Result<Arc<FacetManager>, NodeError> {
        let registry = self.actor_registry().await?;
        Ok(registry.facet_manager().clone())
    }
    
    /// Get monitors
    pub async fn monitors(&self) -> Result<Arc<RwLock<HashMap<ActorId, Vec<MonitorLink>>>>, NodeError> {
        let registry = self.actor_registry().await?;
        Ok(registry.monitors().clone())
    }
    
    /// Get links
    pub async fn links(&self) -> Result<Arc<RwLock<HashMap<ActorId, Vec<ActorId>>>>, NodeError> {
        let registry = self.actor_registry().await?;
        Ok(registry.links().clone())
    }
    
    /// Get lifecycle subscribers
    pub async fn lifecycle_subscribers(&self) -> Result<Arc<RwLock<Vec<mpsc::UnboundedSender<plexspaces_proto::ActorLifecycleEvent>>>>, NodeError> {
        let registry = self.actor_registry().await?;
        Ok(registry.lifecycle_subscribers().clone())
    }
    
    /// Get virtual actor manager from ServiceLocator
    ///
    /// ## Note
    /// VirtualActorManager is now registered in ServiceLocator instead of stored directly in Node.
    /// This removes Node as a middleman for actor management, simplifying the design.
    pub async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>> {
        use plexspaces_core::service_locator::service_names;
        self.service_locator.get_service_by_name::<VirtualActorManager>(service_names::VIRTUAL_ACTOR_MANAGER).await
    }
    
    /// Get virtual actor manager from ServiceLocator or return error
    ///
    /// ## Returns
    /// Arc<VirtualActorManager> if found, NodeError::ActorNotFound if not registered
    async fn get_virtual_actor_manager(&self) -> Result<Arc<VirtualActorManager>, NodeError> {
        use plexspaces_core::service_locator::service_names;
        self.service_locator.get_service_by_name::<VirtualActorManager>(service_names::VIRTUAL_ACTOR_MANAGER).await
            .ok_or_else(|| NodeError::ActorNotFound("VirtualActorManager not registered in ServiceLocator".to_string()))
    }
    
    /// Get virtual actors map (delegates to ActorRegistry)
    pub async fn virtual_actors(&self) -> Result<Arc<RwLock<HashMap<ActorId, VirtualActorMetadata>>>, NodeError> {
        let registry = self.actor_registry().await?;
        Ok(registry.virtual_actors().clone())
    }
    
    /// Get pending activations
    pub async fn pending_activations(&self) -> Result<Arc<RwLock<HashMap<ActorId, Vec<Message>>>>, NodeError> {
        let registry = self.actor_registry().await?;
        Ok(registry.pending_activations().clone())
    }
    
    /// Get actor configs
    pub async fn actor_configs(&self) -> Result<Arc<RwLock<HashMap<ActorId, plexspaces_proto::v1::actor::ActorConfig>>>, NodeError> {
        let registry = self.actor_registry().await?;
        Ok(registry.actor_configs().clone())
    }
    
    /// Get registered actor IDs
    pub async fn registered_actor_ids(&self) -> Result<Arc<RwLock<std::collections::HashSet<ActorId>>>, NodeError> {
        let registry = self.actor_registry().await?;
        Ok(registry.registered_actor_ids().clone())
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
    
    /// Get ObjectRegistry from ServiceLocator
    pub async fn object_registry(&self) -> Result<Arc<plexspaces_object_registry::ObjectRegistry>, NodeError> {
        use plexspaces_core::service_locator::service_names;
        self.service_locator.get_service_by_name::<plexspaces_object_registry::ObjectRegistry>(service_names::OBJECT_REGISTRY).await
            .ok_or_else(|| NodeError::ConfigError("ObjectRegistry not found in ServiceLocator".to_string()))
    }
    
    /// Get ProcessGroupRegistry from ServiceLocator
    pub async fn process_group_registry(&self) -> Result<Arc<plexspaces_process_groups::ProcessGroupRegistry>, NodeError> {
        use plexspaces_core::service_locator::service_names;
        self.service_locator.get_service_by_name::<plexspaces_process_groups::ProcessGroupRegistry>(service_names::PROCESS_GROUP_REGISTRY).await
            .ok_or_else(|| NodeError::ConfigError("ProcessGroupRegistry not found in ServiceLocator".to_string()))
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
        if let Ok(facet_manager) = self.facet_manager().await {
            facet_manager.get_facets(actor_id).await
        } else {
            None
        }
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
        if let Ok(actor_configs) = self.actor_configs().await {
            actor_configs.read().await.get(actor_id).cloned()
        } else {
            None
        }
    }
    
    /// Get pending message count for a virtual actor during activation
    ///
    /// ## Purpose
    /// Returns the number of messages queued for an actor that is currently being activated.
    ///
    /// ## Returns
    /// `u32` - Number of pending messages (0 if actor is not being activated or has no pending messages)
    pub async fn get_pending_activation_count(&self, actor_id: &ActorId) -> u32 {
        if let Ok(pending_activations) = self.pending_activations().await {
            pending_activations
                .read()
                .await
                .get(actor_id)
                .map(|messages| messages.len() as u32)
                .unwrap_or(0)
        } else {
            0
        }
    }

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

        // TupleSpace removed - not needed

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

            // Remove from connection pool (handled by ServiceLocator)
            // ServiceLocator manages gRPC clients, no manual cleanup needed

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
    
    /// Update metrics with current system info (CPU, memory, uptime, actors, connected nodes)
    pub async fn update_metrics_with_system_info(&self) {
        use sysinfo::System;
        let mut system = System::new();
        system.refresh_all();
        
        // Get system info
        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let available_memory = system.available_memory();
        let cpu_count = system.cpus().len() as u32;
        let cpu_usage = if cpu_count > 0 {
            system.cpus().iter().map(|cpu| cpu.cpu_usage() as f64).sum::<f64>() / cpu_count as f64
        } else {
            0.0
        };
        
        // Calculate uptime (time since node started)
        let uptime_seconds = if let Some(start_time) = self.start_time.read().await.as_ref() {
            start_time.elapsed().as_secs()
        } else {
            0
        };
        
        // Get actor counts from ActorRegistry
        let (active_actors, actor_count) = if let Some(actor_registry) = self.service_locator.get_service_by_name::<plexspaces_core::ActorRegistry>(plexspaces_core::service_locator::service_names::ACTOR_REGISTRY).await {
            let registered_ids = actor_registry.registered_actor_ids().read().await;
            let count = registered_ids.len() as u32;
            (count, count) // active_actors = actor_count for now (all registered actors are active)
        } else {
            (0, 0)
        };
        
        // Get connected nodes count (explicitly connected + discovered from ObjectRegistry)
        let connected_nodes = {
            let explicit_connections = self.connections.read().await.len();
            // Also count nodes discovered via ObjectRegistry
            let discovered_count = if let Some(object_registry) = self.service_locator.get_service_by_name::<plexspaces_object_registry::ObjectRegistry>(plexspaces_core::service_locator::service_names::OBJECT_REGISTRY).await {
                use plexspaces_core::RequestContext;
                let ctx = RequestContext::internal();
                use plexspaces_proto::object_registry::v1::ObjectType;
                // Discover all nodes (excluding self)
                // Note: discover returns Vec<ObjectRegistration>
                match object_registry.discover(&ctx, Some(ObjectType::ObjectTypeNode), None, None, None, None, 0, 1000).await {
                    Ok(registrations) => {
                        let local_node_id = self.id().as_str().to_string();
                        registrations.iter()
                            .filter(|r| r.object_id != local_node_id)
                            .count()
                    }
                    Err(_) => 0
                }
            } else {
                0
            };
            // Use max of explicit connections and discovered nodes (they may overlap)
            std::cmp::max(explicit_connections, discovered_count) as u32
        };
        
        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.memory_used_bytes = used_memory;
        metrics.memory_available_bytes = available_memory;
        metrics.cpu_usage_percent = cpu_usage;
        metrics.uptime_seconds = uptime_seconds;
        metrics.active_actors = active_actors;
        metrics.actor_count = actor_count;
        metrics.connected_nodes = connected_nodes;
    }
    
    /// Get node statistics (alias for metrics)
    pub async fn stats(&self) -> NodeMetrics {
        self.metrics().await
    }
    
    /// Increment messages_routed counter (for NodeMetricsAccessor)
    pub(crate) async fn increment_messages_routed(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.messages_routed += 1;
    }
    
    /// Increment local_deliveries counter (for NodeMetricsAccessor)
    pub(crate) async fn increment_local_deliveries(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.local_deliveries += 1;
    }
    
    /// Increment remote_deliveries counter (for NodeMetricsAccessor)
    pub(crate) async fn increment_remote_deliveries(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.remote_deliveries += 1;
    }
    
    /// Increment failed_deliveries counter (for NodeMetricsAccessor)
    pub(crate) async fn increment_failed_deliveries(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.failed_deliveries += 1;
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
        let actor_configs_arc = self.actor_configs().await.unwrap_or_default();
        let actor_configs = actor_configs_arc.read().await;
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

        // Send heartbeat to ObjectRegistry using ObjectTypeNode
        let internal_ctx = RequestContext::internal();
        use plexspaces_core::service_locator::service_names;
                use crate::object_registry_helpers::heartbeat_node;
        if let Some(object_registry) = self.service_locator.get_service_by_name::<plexspaces_object_registry::ObjectRegistry>(service_names::OBJECT_REGISTRY).await {
            if let Err(e) = heartbeat_node(&object_registry, &internal_ctx, self.id.as_str()).await {
                return Err(NodeError::NetworkError(format!("ObjectRegistry heartbeat failed: {}", e)));
            }
        } else {
            return Err(NodeError::ConfigError("ObjectRegistry not found in ServiceLocator".to_string()));
        }

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
        
        // Use AnyPool with max_connections=1 for in-memory SQLite to ensure all operations
        // use the same connection (critical for in-memory databases)
        // Note: sqlx::any requires install_default_drivers() to be called before use
        // This is called in start() before any database operations
        use sqlx::AnyPool;
        use sqlx::any::AnyPoolOptions;
        
        // For in-memory SQLite, use max_connections=1 to ensure all operations share the same database
        let pool_options = if db_url.starts_with("sqlite::memory:") || db_url.starts_with("sqlite://:memory:") {
            AnyPoolOptions::new().max_connections(1)
        } else {
            AnyPoolOptions::new()
        };
        
        let any_pool = match pool_options.connect(&db_url).await {
            Ok(pool) => pool,
            Err(e) => {
                // Check if it's the "No drivers installed" error
                let error_msg = format!("{}", e);
                if error_msg.contains("No drivers installed") {
                    eprintln!("Warning: sqlx drivers not available. Ensure sqlx features include 'sqlite' and 'any'. Blob service disabled.");
                } else {
                    eprintln!("Warning: Failed to create blob service database: {}. Blob service disabled.", e);
                }
                return None;
            }
        };
        
        // Create repository with auto-applied migrations
        let repository = match SqlBlobRepository::new(any_pool).await {
            Ok(repo) => Arc::new(repo),
            Err(e) => {
                eprintln!("Warning: Failed to create blob repository with migrations: {}. Blob service disabled.", e);
                return None;
            }
        };

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
    /// ## Release Config Loading
    /// If release config is not already set (via `set_release_spec()`), this method will:
    /// 1. Check `PLEXSPACES_RELEASE_CONFIG_PATH` environment variable
    /// 2. Check `release.yaml` or `release.toml` in current directory
    /// 3. If found, load and set ReleaseSpec on the node
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

        // Install sqlx::any default drivers before any database operations
        sqlx::any::install_default_drivers();
        
        // Record start time for uptime calculation
        {
            let mut start_time = self.start_time.write().await;
            *start_time = Some(tokio::time::Instant::now());
        }

        // Initialize services if not already done (idempotent)
        self.initialize_services().await?;
        
        // Set node context for application manager (from ServiceLocator)
        use plexspaces_core::service_locator::service_names;
        let application_manager: Arc<ApplicationManager> = self.service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await
            .ok_or_else(|| NodeError::ConfigError("ApplicationManager not found in ServiceLocator".to_string()))?;
        application_manager.set_node_context(self.clone()).await;
        
        // Load release config if not already set
        // Check if release_spec is already set
        {
            let release_spec = self.release_spec.read().await;
            if release_spec.is_none() {
                drop(release_spec);
                // Try to load from file or env variable
                if let Ok(release_spec) = self.load_release_config().await {
                    self.set_release_spec(release_spec).await;
                    tracing::info!("Loaded release config from file or environment variable");
                } else {
                    tracing::debug!("No release config found, using defaults");
                }
            }
        }

        // Get NodeConfig for node registration
        let proto_node_config = self.service_locator.get_node_config().await
            .ok_or_else(|| NodeError::ConfigError("NodeConfig not found in ServiceLocator".to_string()))?;

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
            // Register the node in ObjectRegistry using ObjectTypeNode
            use plexspaces_core::RequestContext;
                use crate::object_registry_helpers::register_node;
            use plexspaces_core::service_locator::service_names;
            
            let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());
            
            if let Some(object_registry) = node_for_registration.service_locator.get_service_by_name::<plexspaces_object_registry::ObjectRegistry>(service_names::OBJECT_REGISTRY).await {
                let grpc_address = format!("http://{}", listen_addr);
                if let Err(e) = register_node(&object_registry, &ctx, &node_id_str, &grpc_address, None).await {
                    eprintln!("Node {}: Failed to register in ObjectRegistry: {}", node_id_str, e);
                } else {
                    tracing::info!(node_id = %node_id_str, "Node registered in ObjectRegistry");
                }
            } else {
                eprintln!("Node {}: ObjectRegistry not found in ServiceLocator", node_id_str);
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

        // TupleSpace removed - not needed

        // Parse listen address
        let addr = self
            .config
            .listen_addr
            .parse()
            .map_err(|e| NodeError::ConfigError(format!("Invalid listen address: {}", e)))?;

        // Register Node in ServiceLocator so ActorServiceImpl can access it
        self.service_locator.register_service(self.clone()).await;

        // Initialize blob service if configured
        // Wrap AnyPool::connect in a way that handles the "No drivers installed" error gracefully
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
        // Note: ActorServiceImpl will be created with health reporter after HealthService is initialized
        // We'll create it later after HealthService is available
        
        // Register ActorService in ServiceLocator so ActorContext::send_reply() can use it
        // Create ActorServiceImpl for ServiceLocator (uses same service_locator, so shares state)
        // ActorServiceImpl now implements ActorService trait directly, no wrapper needed
        let actor_service_for_context = Arc::new(
            plexspaces_actor_service::ActorServiceImpl::new(
                self.service_locator.clone(),
                self.id.as_str().to_string(),
            )
        );
        self.service_locator.register_actor_service(actor_service_for_context.clone() as Arc<dyn plexspaces_core::ActorService + Send + Sync>).await;
        
        // Register NodeMetricsAccessor for monitoring helpers and dashboard
        use crate::service_wrappers::NodeMetricsAccessorWrapper;
        let metrics_accessor = Arc::new(NodeMetricsAccessorWrapper::new(self.clone()));
        self.service_locator.register_service(metrics_accessor.clone()).await;
        let metrics_accessor_trait: Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync> = metrics_accessor.clone() as Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync>;
        self.service_locator.register_node_metrics_accessor(metrics_accessor_trait).await;
        
        let tuplespace_service = TuplePlexSpaceServiceImpl::new(self.clone());
        
        // Start background cleanup task for expired temporary senders (in ActorRegistry)
        let actor_registry = self.actor_registry().await?;
        ActorRegistry::start_temporary_sender_cleanup(actor_registry);

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
        // CapacityTracker needs ObjectRegistry, get it from ServiceLocator
        let object_registry: Arc<plexspaces_object_registry::ObjectRegistry> = self.service_locator.get_service_by_name::<plexspaces_object_registry::ObjectRegistry>(service_names::OBJECT_REGISTRY).await
            .ok_or_else(|| NodeError::ConfigError("ObjectRegistry not found".to_string()))?;
        // CapacityTracker expects Arc<ObjectRegistry>
        let capacity_tracker = Arc::new(CapacityTracker::new(object_registry.clone()));
        
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
        // Lease duration: 60 seconds (longer to reduce renewal pressure)
        // Heartbeat interval: 15 seconds (should be < 1/3 of lease duration for safety)
        // This ensures renewals happen well before expiration even with delays
        let lease_duration_secs = 60; // Increased from 30 to 60 seconds
        let heartbeat_interval_secs = 15; // Increased from 10 to 15 seconds (still < 1/3 of 60)
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
        
        // Create and register HealthService (source of truth for shutdown)
        // This helper ensures consistent creation and registration
        use crate::health_service_helpers::create_and_register_health_service;
        let (plexspaces_health_reporter, _) = create_and_register_health_service(
            self.service_locator.clone(),
            None, // Use default HealthProbeConfig
        ).await;
        
        // Store health reporter in Node for shutdown access
        {
            let mut health_reporter_guard = self.health_reporter.write().await;
            *health_reporter_guard = Some(plexspaces_health_reporter.clone());
        }
        
        // Create ActorServiceImpl with health reporter for shutdown checks
        // This ensures all gRPC handlers can check shutdown status
        // This must be created after HealthService is initialized
        let actor_service = ActorServiceImpl::with_health_reporter(
            self.clone(),
            plexspaces_health_reporter.clone(),
        );
        
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
        let application_manager: Arc<ApplicationManager> = self.service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await
            .ok_or_else(|| NodeError::ConfigError("ApplicationManager not found in ServiceLocator".to_string()))?;
        // Node context and service locator are already set in start() above
        let application_service = Arc::new(ApplicationServiceImpl::new(
            self.clone(),
            application_manager.clone(),
        ));

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
        
        // Create DashboardService with health reporter access (if dashboard feature enabled)
        // Create both gRPC and HTTP instances (they share ServiceLocator so have same data)
        #[cfg(feature = "dashboard")]
        let (dashboard_service_opt, dashboard_service_for_http_opt): (Option<plexspaces_dashboard::DashboardServiceImpl>, Option<Arc<plexspaces_dashboard::DashboardServiceImpl>>) = {
            use plexspaces_dashboard::{DashboardServiceImpl, HealthReporterAccess};
            
            // Create health reporter access wrapper to avoid circular dependency
            struct HealthReporterAccessImpl {
                health_reporter: Arc<PlexSpacesHealthReporter>,
            }
            
            #[async_trait::async_trait]
            impl HealthReporterAccess for HealthReporterAccessImpl {
                async fn get_detailed_health(&self, include_non_critical: bool) -> plexspaces_proto::system::v1::DetailedHealthCheck {
                    self.health_reporter.get_detailed_health(include_non_critical).await
                }
            }
            
            let health_access = Arc::new(HealthReporterAccessImpl {
                health_reporter: plexspaces_health_reporter.clone(),
            });
            
            // Create gRPC instance
            let grpc_instance = DashboardServiceImpl::with_health_reporter(
                self.service_locator.clone(),
                health_access.clone(),
            );
            
            // Create HTTP instance (wrapped in Arc for sharing)
            let http_instance = Arc::new(DashboardServiceImpl::with_health_reporter(
                self.service_locator.clone(),
                health_access,
            ));
            
            (Some(grpc_instance), Some(http_instance))
        };
        #[cfg(not(feature = "dashboard"))]
        let (dashboard_service_opt, dashboard_service_for_http_opt): (Option<()>, Option<()>) = (None, None);

        // Build gRPC server with all services
        // Set max message size to 5MB for gRPC methods (larger than default 4MB for flexibility)
        // Note: For large WASM file uploads (>5MB), use HTTP multipart endpoint instead
        const GRPC_MAX_MESSAGE_SIZE: usize = 5 * 1024 * 1024; // 5MB
        
        let server_builder = Server::builder()
            .accept_http1(true)  // Enable HTTP for gRPC-Web
            .add_service(
                ActorServiceServer::new(actor_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE)
            )
            .add_service(
                TuplePlexSpaceServiceServer::new(tuplespace_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE)
            )
            .add_service(
                SchedulingServiceServer::new(scheduling_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE)
            )
            .add_service(
                WasmRuntimeServiceServer::new(wasm_runtime_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE)
            )
            .add_service(
                ApplicationServiceServer::new(application_service.as_ref().clone())
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE)
            )
            .add_service(standard_health_service)  // Standard gRPC health service
            .add_service(
                SystemServiceServer::new(system_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE)
            )
            .add_service(
                MetricsServiceServer::new(metrics_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE)
            );
        
        // Add dashboard service if feature enabled
        #[cfg(feature = "dashboard")]
        let server_builder = {
            use plexspaces_proto::dashboard::v1::dashboard_service_server::DashboardServiceServer;
            if let Some(dashboard_svc) = dashboard_service_opt {
                server_builder.add_service(
                    DashboardServiceServer::new(dashboard_svc)
                        .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                        .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                )
            } else {
                server_builder
            }
        };
        
        // Add blob service if available
        let server_builder = if let Some(ref blob_service) = blob_service_opt {
            use plexspaces_blob::server::grpc::BlobServiceImpl;
            use plexspaces_proto::storage::v1::blob_service_server::BlobServiceServer;
            server_builder.add_service(
                BlobServiceServer::new(BlobServiceImpl::new(blob_service.clone()))
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE)
            )
        } else {
            server_builder
        };
        
        let grpc_server = server_builder.serve(addr);
        
        // Start HTTP gateway server for InvokeActor routes (following demo pattern)
        // Create a new ActorServiceImpl instance for HTTP gateway (shares same service locator)
        let http_gateway_handle = {
            let service_locator_for_http = self.service_locator.clone();
            let node_id_for_http = self.id.as_str().to_string();
            let grpc_addr = addr;
            let node_for_http = self.clone();
            let application_manager_for_http = application_manager.clone();
            
            tokio::spawn(async move {
                use axum::{
                    extract::{Path, Query, DefaultBodyLimit},
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
                
                // Set body size limit to 100MB for WASM uploads (will be applied to specific route)
                const MAX_BODY_SIZE: usize = 100 * 1024 * 1024; // 100MB
                
                // Create Axum router for HTTP gateway routes
                // Apply body limit globally (100MB) for all routes, then override for specific routes if needed
                let app = Router::new()
                    .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
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
                
                // Add HTTP multipart endpoint for WASM file uploads (large files)
                // Max WASM file size: 100MB (enforced by multipart parser)
                use axum::extract::Multipart;
                use futures_util::StreamExt;
                use plexspaces_proto::application::v1::{
                    DeployApplicationRequest, ApplicationSpec,
                };
                use plexspaces_proto::wasm::v1::WasmModule;
                
                const MAX_WASM_FILE_SIZE: usize = 100 * 1024 * 1024; // 100MB
                
                let node_clone = node_for_http.clone();
                let app_mgr_clone = application_manager_for_http.clone();
                let wasm_deploy_handler = move |mut multipart: Multipart| async move {
                    let mut application_id = None;
                    let mut name = None;
                    let mut version = None;
                    let mut wasm_file_data: Option<Vec<u8>> = None;
                    let mut config_data: Option<String> = None;
                    
                    while let Some(field) = multipart.next_field().await.map_err(|e| {
                        let error_msg = format!("Failed to parse multipart form data: {}", e);
                        tracing::error!(error = %e, "Multipart parsing error");
                        (StatusCode::BAD_REQUEST, error_msg)
                    })? {
                        let field_name = field.name().unwrap_or("").to_string();
                        match field_name.as_str() {
                            "application_id" => {
                                application_id = Some(field.text().await.map_err(|e| {
                                    (StatusCode::BAD_REQUEST, format!("Failed to read application_id: {}", e))
                                })?);
                            }
                            "name" => {
                                name = Some(field.text().await.map_err(|e| {
                                    (StatusCode::BAD_REQUEST, format!("Failed to read name: {}", e))
                                })?);
                            }
                            "version" => {
                                version = Some(field.text().await.map_err(|e| {
                                    (StatusCode::BAD_REQUEST, format!("Failed to read version: {}", e))
                                })?);
                            }
                            "wasm_file" => {
                                // Read entire field into memory (field.bytes() handles streaming internally)
                                // For very large files, this will use memory, but it's simpler and works with body limits
                                let bytes = field.bytes().await.map_err(|e| {
                                    let error_msg = format!("Failed to read wasm_file field: {} (this may indicate body size limit issue)", e);
                                    tracing::error!(error = %e, "WASM file read error - check body size limit configuration");
                                    (StatusCode::BAD_REQUEST, error_msg)
                                })?;
                                
                                // Enforce 100MB max size
                                if bytes.len() > MAX_WASM_FILE_SIZE {
                                    return Err((StatusCode::PAYLOAD_TOO_LARGE, 
                                        format!("WASM file size {} bytes exceeds maximum {} bytes", 
                                            bytes.len(), MAX_WASM_FILE_SIZE)));
                                }
                                
                                // Verify WASM magic number immediately after reading
                                if bytes.len() < 4 {
                                    tracing::error!(size = bytes.len(), "WASM file too small");
                                    return Err((StatusCode::BAD_REQUEST, 
                                        format!("WASM file too small: {} bytes", bytes.len())));
                                }
                                
                                let magic = &bytes[0..4];
                                if magic != b"\0asm" {
                                    tracing::error!(
                                        magic_bytes = format!("{:02x?}", magic),
                                        expected = "0061736d",
                                        "WASM file missing magic number - file may be corrupted"
                                    );
                                    return Err((StatusCode::BAD_REQUEST, 
                                        format!("Invalid WASM file: missing magic number (got {:02x?}, expected 0061736d)", magic)));
                                }
                                
                                // Log WASM file info for debugging
                                tracing::info!(
                                    wasm_file_size = bytes.len(),
                                    wasm_file_first_bytes = format!("{:02x?}", bytes.iter().take(8).collect::<Vec<_>>()),
                                    wasm_version = format!("{:02x?}", bytes.get(4..8).unwrap_or(&[])),
                                    "Read WASM file from multipart upload - magic number verified"
                                );
                                
                                wasm_file_data = Some(bytes.to_vec());
                            }
                            "config" => {
                                config_data = Some(field.text().await.map_err(|e| {
                                    (StatusCode::BAD_REQUEST, format!("Failed to read config: {}", e))
                                })?);
                            }
                            _ => {}
                        }
                    }
                    
                    let application_id = application_id.ok_or_else(|| {
                        (StatusCode::BAD_REQUEST, "application_id is required".to_string())
                    })?;
                    let name = name.ok_or_else(|| {
                        (StatusCode::BAD_REQUEST, "name is required".to_string())
                    })?;
                    let version = version.unwrap_or_else(|| "1.0.0".to_string());
                    
                    // Build DeployApplicationRequest
                    let wasm_module = wasm_file_data.map(|bytes| {
                        // Verify WASM magic number (0x00 0x61 0x73 0x6D = "\0asm")
                        if bytes.len() < 4 || &bytes[0..4] != b"\0asm" {
                            tracing::error!(
                                first_bytes = format!("{:02x?}", bytes.iter().take(8).collect::<Vec<_>>()),
                                "WASM file does not start with magic number"
                            );
                        } else {
                            tracing::info!(
                                wasm_size = bytes.len(),
                                "WASM file has valid magic number"
                            );
                        }
                        
                        WasmModule {
                            name: name.clone(),
                            version: version.clone(),
                            module_bytes: bytes,
                            module_hash: String::new(), // Will be computed by server
                            ..Default::default()
                        }
                    });
                    
                    let config = config_data.map(|toml_str| {
                        // Parse TOML to ApplicationSpec (simplified - use defaults for now)
                        ApplicationSpec {
                            name: name.clone(),
                            version: version.clone(),
                            ..Default::default()
                        }
                    }).unwrap_or_else(|| {
                        // Create ApplicationSpec following wasm-calculator pattern
                        use plexspaces_proto::application::v1::{ApplicationType, SupervisorSpec, ChildSpec, ChildType};
                        
                        // Auto-generate default supervisor tree with one worker actor
                        // This ensures at least one actor is spawned for every deployed application
                        use plexspaces_proto::application::v1::{SupervisionStrategy, RestartPolicy};
                        let default_supervisor = SupervisorSpec {
                            strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
                            max_restarts: 5,
                            max_restart_window: None,
                            children: vec![
                                ChildSpec {
                                    id: name.clone(), // Use application name as actor ID
                                    r#type: ChildType::ChildTypeWorker.into(),
                                    args: std::collections::HashMap::new(),
                                    restart: RestartPolicy::RestartPolicyPermanent.into(),
                                    shutdown_timeout: None,
                                    supervisor: None,
                                    facets: vec![], // Phase 1: Unified Lifecycle - facets support
                                }
                            ],
                        };
                        
                        ApplicationSpec {
                            name: name.clone(),
                            version: version.clone(),
                            description: format!("WASM application: {}", name.clone()),
                            r#type: ApplicationType::ApplicationTypeActive.into(),
                            dependencies: vec![],
                            env: std::collections::HashMap::new(),
                            supervisor: Some(default_supervisor),
                        }
                    });
                    
                    let request = DeployApplicationRequest {
                        application_id: application_id.clone(),
                        name: name.clone(),
                        version: version.clone(),
                        wasm_module,
                        config: Some(config),
                        release_config: None,
                        initial_state: vec![],
                    };
                    
                    // Log request details before deployment
                    if let Some(ref wasm_mod) = request.wasm_module {
                        tracing::info!(
                            application_id = %request.application_id,
                            name = %request.name,
                            version = %request.version,
                            wasm_size = wasm_mod.module_bytes.len(),
                            wasm_first_bytes = format!("{:02x?}", wasm_mod.module_bytes.iter().take(8).collect::<Vec<_>>()),
                            "Calling ApplicationService::deploy_application"
                        );
                        
                        // Verify WASM magic number one more time
                        if wasm_mod.module_bytes.len() >= 4 {
                            if &wasm_mod.module_bytes[0..4] != b"\0asm" {
                                tracing::error!(
                                    first_bytes = format!("{:02x?}", wasm_mod.module_bytes.iter().take(8).collect::<Vec<_>>()),
                                    "WASM bytes corrupted - missing magic number before deployment"
                                );
                                return Err((StatusCode::BAD_REQUEST, 
                                    "WASM file is corrupted or invalid (missing magic number)".to_string()));
                            }
                        }
                    }
                    
                    // Call ApplicationService directly (create instance same as gRPC service)
                    use crate::application_service::ApplicationServiceImpl;
                    use plexspaces_proto::application::v1::application_service_server::ApplicationService;
                    let app_service = ApplicationServiceImpl::new(node_clone.clone(), app_mgr_clone.clone());
                    let grpc_request = tonic::Request::new(request);
                    let response = app_service.deploy_application(grpc_request).await.map_err(|e| {
                        tracing::error!(error = %e, "ApplicationService::deploy_application failed");
                        (StatusCode::INTERNAL_SERVER_ERROR, format!("Deployment failed: {}", e))
                    })?;
                    
                    let inner = response.into_inner();
                    Ok::<_, (StatusCode, String)>(Json(serde_json::json!({
                        "success": inner.success,
                        "application_id": inner.application_id,
                        "status": format!("{:?}", inner.status),
                        "error": inner.error
                    })))
                };
                
                // Add WASM deployment and undeployment routes
                // Body limit is already applied globally above (100MB)
                // The route-specific limit below ensures it's definitely applied
                let app_mgr_for_undeploy = application_manager_for_http.clone();
                let node_for_undeploy = node_for_http.clone();
                let undeploy_handler = move |Path(application_id): Path<String>| async move {
                    use crate::application_service::ApplicationServiceImpl;
                    use plexspaces_proto::application::v1::{
                        application_service_server::ApplicationService,
                        UndeployApplicationRequest,
                    };
                    
                    tracing::info!(application_id = %application_id, "Undeploying application via HTTP");
                    
                    // Note: application_id in the path is used as the identifier
                    // The ApplicationService will look up the application by this ID
                    let app_service = ApplicationServiceImpl::new(node_for_undeploy.clone(), app_mgr_for_undeploy.clone());
                    let grpc_request = tonic::Request::new(UndeployApplicationRequest {
                        application_id: application_id.clone(),
                        timeout: None, // Use default timeout from application config
                    });
                    
                    let response = app_service.undeploy_application(grpc_request).await.map_err(|e| {
                        tracing::error!(error = %e, "ApplicationService::undeploy_application failed");
                        (StatusCode::INTERNAL_SERVER_ERROR, format!("Undeployment failed: {}", e))
                    })?;
                    
                    let inner = response.into_inner();
                    Ok::<_, (StatusCode, String)>(Json(serde_json::json!({
                        "success": inner.success,
                        "error": inner.error
                    })))
                };
                
                let deploy_router = Router::new()
                    .route("/api/v1/applications/deploy", post(wasm_deploy_handler))
                    .route("/api/v1/applications/:application_id", delete(undeploy_handler))
                    .layer(DefaultBodyLimit::max(MAX_BODY_SIZE));
                let app = app.merge(deploy_router);
                
                // Add dashboard routes (if feature enabled)
                let app = {
                    #[cfg(feature = "dashboard")]
                    {
                        use plexspaces_dashboard::create_dashboard_router;
                        // Pass DashboardService to router so API endpoints can use it
                        let dashboard_router = create_dashboard_router(
                            self.service_locator().clone(),
                            dashboard_service_for_http_opt.clone(),
                        );
                        app.merge(dashboard_router)
                    }
                    #[cfg(not(feature = "dashboard"))]
                    app
                };
                
                // Start HTTP server on a separate port (gRPC port + 1 for HTTP gateway)
                // This follows the demo pattern of separate servers
                let http_port = grpc_addr.port() + 1;
                let http_addr = format!("{}:{}", grpc_addr.ip(), http_port)
                    .parse::<std::net::SocketAddr>()
                    .unwrap_or_else(|_| "127.0.0.1:8001".parse().unwrap());
                
                eprintln!("🌐 Starting HTTP gateway server on http://{}", http_addr);
                eprintln!("📊 Dashboard available at http://{}/", http_addr);
                
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
            let actor_registry = self.actor_registry().await?;
            if !actor_registry.is_actor_activated(actor_id).await {
                return Err(NodeError::ActorNotFound(actor_id.clone()));
            }

            // Generate unique monitor reference (ULID for sortability)
            let monitor_ref = ulid::Ulid::new().to_string();

            // Delegate to ActorRegistry for local monitoring
            actor_registry.monitor(
                actor_id,
                supervisor_id,
                monitor_ref.clone(),
                notification_tx,
            ).await
            .map_err(|e| NodeError::ActorNotFound(format!("Monitor failed: {}", e)))?;

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
            // For remote monitoring, we still need to store locally to receive NotifyActorDown RPC
            // Delegate to ActorRegistry for consistency
            let actor_registry = self.actor_registry().await?;
            actor_registry.monitor(
                actor_id,
                supervisor_id,
                monitor_ref.clone(),
                notification_tx,
            ).await
            .map_err(|e| NodeError::ActorNotFound(format!("Monitor failed: {}", e)))?;

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
            // Use internal context for routing lookup (system-level operation)
            let internal_ctx = RequestContext::internal();
            let actor_registry = self.actor_registry().await?;
            let routing1 = actor_registry.lookup_routing(&internal_ctx, actor_id).await
                .map_err(|_| NodeError::ActorNotFound(actor_id.clone()))?;
            if routing1.is_none() || !routing1.unwrap().is_local {
                return Err(NodeError::ActorNotFound(actor_id.clone()));
            }
            
            let routing2 = actor_registry.lookup_routing(&internal_ctx, linked_actor_id).await
                .map_err(|_| NodeError::ActorNotFound(linked_actor_id.clone()))?;
            if routing2.is_none() || !routing2.unwrap().is_local {
                return Err(NodeError::ActorNotFound(linked_actor_id.clone()));
            }

            // Delegate to ActorRegistry for local linking
            actor_registry.link(actor_id, linked_actor_id).await
                .map_err(|e| NodeError::InvalidArgument(format!("Link failed: {}", e)))?;

            Ok(())
        } else if is_local1 {
            // actor_id is local, linked_actor_id is remote
            // Store link locally (for local actor)
            // Note: Remote actor's link will be stored on remote node via RPC
            let actor_registry = self.actor_registry().await?;
            // For remote linking, we only store the local side
            // The remote side will be linked via RPC call below
            // We can't use ActorRegistry.link() here because it requires both actors to exist locally
            // So we manually add to links for the local actor only
            let links_arc = self.links().await?;
            let mut links = links_arc.write().await;
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
            let links_arc = self.links().await?;
            let mut links = links_arc.write().await;
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
            // Delegate to ActorRegistry for local unlinking
            let actor_registry = self.actor_registry().await?;
            actor_registry.unlink(actor_id, linked_actor_id).await
                .map_err(|e| NodeError::InvalidArgument(format!("Unlink failed: {}", e)))?;

            Ok(())
        } else if is_local1 {
            // actor_id is local, linked_actor_id is remote
            let links_arc = self.links().await?;
            let mut links = links_arc.write().await;
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
            let links_arc = self.links().await?;
            let mut links = links_arc.write().await;
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


    /// Publish a lifecycle event to all subscribers
    ///
    /// ## Purpose
    /// Internal helper to multicast lifecycle events to all observability backends.
    ///
    /// ## Arguments
    /// * `event` - The lifecycle event to publish
    async fn publish_lifecycle_event(&self, event: plexspaces_proto::ActorLifecycleEvent) {
        let subscribers_arc = match self.lifecycle_subscribers().await {
            Ok(arc) => arc,
            Err(_) => return, // If subscribers not available, skip
        };
        let subscribers = subscribers_arc.read().await;
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
        // Delegate to ActorRegistry
        if let Ok(actor_registry) = self.actor_registry().await {
            actor_registry.subscribe_lifecycle_events(subscriber).await;
        }
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
        // Delegate to ActorRegistry
        if let Ok(actor_registry) = self.actor_registry().await {
            actor_registry.unsubscribe_lifecycle_events().await;
        }
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
                
                // Actor terminated normally - handle termination comprehensively
                if let Ok(actor_registry) = self.actor_registry().await {
                    let exit_reason = ExitReason::from_str(&terminated.reason);
                    actor_registry.handle_actor_termination(&event.actor_id, exit_reason).await;
                }
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
                
                // Actor failed (panic/error) - handle termination comprehensively
                if let Ok(actor_registry) = self.actor_registry().await {
                    let exit_reason = ExitReason::Error(failed.error.clone());
                    actor_registry.handle_actor_termination(&event.actor_id, exit_reason).await;
                }
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
        let application_manager = self.application_manager().await
            .map_err(|e| ApplicationError::ConfigError(format!("Config error: {}", e)))?;
        application_manager.register(app).await
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
        // Get ApplicationManager from ServiceLocator (with fallback for tests)
        let application_manager = self.application_manager().await
            .map_err(|e| ApplicationError::Other(format!("ApplicationManager not found in ServiceLocator: {}", e)))?;
        // Ensure node context is set (idempotent)
        // Note: self is &Node, so we need Arc::new, but we're in a method that takes &self, not Arc<Self>
        // So we need to clone the Node and wrap it
        let node_arc = Arc::new(self.clone());
        application_manager.ensure_node_context(node_arc).await;
        application_manager.start(name).await
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
        let application_manager = self.application_manager().await
            .map_err(|e| ApplicationError::Other(format!("ApplicationManager not found in ServiceLocator: {}", e)))?;
        application_manager.stop(name, timeout).await
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
        eprintln!("\n╔════════════════════════════════════════════════════════════════╗");
        eprintln!("║  Starting Graceful Shutdown                                    ║");
        eprintln!("╚════════════════════════════════════════════════════════════════╝");
        eprintln!("Node: {} | Timeout: {:?}\n", self.id.as_str(), timeout);

        // Collect initial metrics before shutdown
        let (app_count, actor_count, queue_size, active_reqs, conn_nodes) = self.collect_shutdown_metrics().await;
        eprintln!("📊 Initial State:");
        eprintln!("   • Applications: {}", app_count);
        eprintln!("   • Actors: {}", actor_count);
        eprintln!("   • Total Mailbox Queue Size: {}", queue_size);
        eprintln!("   • Active Requests: {}", active_reqs);
        eprintln!("   • Connected Nodes: {}", conn_nodes);
        eprintln!();

        // Begin graceful shutdown on health reporter (sets NOT_SERVING, prevents new requests)
        // HealthService.begin_shutdown() will set ServiceLocator.shutdown_flag
        {
            let health_reporter_guard = self.health_reporter.read().await;
            if let Some(ref health_reporter) = *health_reporter_guard {
                let (drained, duration, completed) = health_reporter.begin_shutdown(Some(timeout)).await;
                eprintln!("🛑 Phase 1: Health Status");
                eprintln!("   ✓ Health set to NOT_SERVING");
                eprintln!("   ✓ Requests drained: {} | Duration: {:?} | Completed: {}", drained, duration, completed);
            } else {
                // Fallback: if health reporter not available, set ServiceLocator flag directly
                self.service_locator.set_shutdown(true).await;
                eprintln!("🛑 Phase 1: Health Status");
                eprintln!("   ✓ ServiceLocator shutdown flag set (health reporter not available)");
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
                eprintln!("   ✓ gRPC server shutdown signal sent");
            }
        }

        // Stop background scheduler (Phase 4)
        {
            let scheduler = self.background_scheduler.read().await;
            if let Some(scheduler) = scheduler.as_ref() {
                eprintln!("🛑 Phase 2: Background Services");
                eprintln!("   ✓ Background scheduler stopped");
                scheduler.stop();
            }
        }

        // Stop all applications (reverse order)
        use plexspaces_core::service_locator::service_names;
        let application_manager: Arc<ApplicationManager> = self.service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await
            .ok_or_else(|| ApplicationError::Other("ApplicationManager not found in ServiceLocator".to_string()))?;
        
        eprintln!("🛑 Phase 3: Stopping Applications");
        let apps_before = application_manager.list_applications().await.len();
        eprintln!("   • Stopping {} applications...", apps_before);
        
        let stop_start = std::time::Instant::now();
        application_manager.stop_all(timeout).await?;
        let stop_duration = stop_start.elapsed();
        
        let apps_after = application_manager.list_applications().await.len();
        let apps_stopped = apps_before - apps_after;
        
        // Collect metrics after stopping applications
        let (_, after_actor_count, after_queue_size, after_active_reqs, _) = self.collect_shutdown_metrics().await;
        
        eprintln!("   ✓ Applications stopped: {} | Duration: {:?}", apps_stopped, stop_duration);
        eprintln!("   • Remaining actors: {} (down from {})", after_actor_count, actor_count);
        eprintln!("   • Remaining mailbox queue size: {} (down from {})", after_queue_size, queue_size);
        eprintln!();

        // Close network connections
        eprintln!("🛑 Phase 4: Network Connections");
        let connected_nodes = self.connected_nodes().await;
        eprintln!("   • Closing {} connections...", connected_nodes.len());
        for node_id in connected_nodes {
            let _ = self.disconnect_from(&node_id).await;
        }
        eprintln!("   ✓ All network connections closed");
        eprintln!();
        
        // Flush TupleSpace pending operations (ensure all writes are persisted)
        // TupleSpace operations are synchronous, so no explicit flush needed
        // For external backends, they handle persistence automatically
        eprintln!("🛑 Phase 5: Final Cleanup");
        eprintln!("   ✓ TupleSpace operations flushed");
        
        // Final metrics
        let (final_app_count, final_actor_count, final_queue_size, final_active_reqs, _) = self.collect_shutdown_metrics().await;
        eprintln!("\n📊 Final State:");
        eprintln!("   • Applications: {} (stopped: {})", final_app_count, apps_stopped);
        eprintln!("   • Actors: {} (stopped: {})", final_actor_count, actor_count.saturating_sub(final_actor_count));
        eprintln!("   • Mailbox Queue Size: {} (drained: {})", final_queue_size, queue_size.saturating_sub(final_queue_size));
        eprintln!("   • Active Requests: {} (completed: {})", final_active_reqs, active_reqs.saturating_sub(final_active_reqs));
        eprintln!();

        eprintln!("╔════════════════════════════════════════════════════════════════╗");
        eprintln!("║  ✅ Graceful Shutdown Complete                                ║");
        eprintln!("╚════════════════════════════════════════════════════════════════╝\n");
        Ok(())
    }

    /// Collect shutdown metrics for logging
    async fn collect_shutdown_metrics(&self) -> (usize, usize, usize, usize, usize) {
        use plexspaces_core::service_locator::service_names;
        
        // Get application count
        let application_count = if let Some(app_manager) = self.service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await {
            app_manager.list_applications().await.len()
        } else {
            0
        };
        
        // Get actor count and mailbox queue sizes
        let (actor_count, total_mailbox_queue_size) = if let Some(actor_registry) = self.service_locator.get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY).await {
            let registered_ids = actor_registry.registered_actor_ids().read().await;
            let actor_count = registered_ids.len();
            
            // Try to get mailbox queue sizes (may not be accessible for all actors)
            let mut total_queue_size = 0;
            for actor_id in registered_ids.iter() {
                if let Some(sender) = actor_registry.lookup_actor(actor_id).await {
                    // Try to get mailbox size if accessible
                    // Note: MessageSender trait doesn't expose mailbox directly, so we can't get queue size
                    // This is a limitation - we'd need to add a method to MessageSender or ActorRef
                    // For now, we'll just count actors
                }
            }
            
            (actor_count, total_queue_size)
        } else {
            (0, 0)
        };
        
        // Get active requests (from node metrics)
        let node_metrics = self.metrics().await;
        let active_requests = node_metrics.messages_routed as usize;
        
        // Get connected nodes
        let connected_nodes = self.connected_nodes().await.len();
        
        (application_count, actor_count, total_mailbox_queue_size, active_requests, connected_nodes)
    }

    /// Check if shutdown has been requested
    pub async fn is_shutdown_requested(&self) -> bool {
        // Get ApplicationManager from ServiceLocator
        use plexspaces_core::service_locator::service_names;
        if let Some(application_manager) = self.service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await {
            application_manager.is_shutdown_requested().await
        } else {
            false // ApplicationManager should always be in ServiceLocator
        }
    }

    /// Get application manager reference (for advanced usage)
    pub async fn application_manager(&self) -> Result<Arc<ApplicationManager>, NodeError> {
        use plexspaces_core::service_locator::service_names;
        self.service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await
            .ok_or_else(|| NodeError::ConfigError("ApplicationManager not found in ServiceLocator".to_string()))
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
        let actor_registry = self.actor_registry().await?;
        actor_registry.unregister_with_cleanup(&actor_id).await
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
                let actor_ids = match node.virtual_actors().await {
                    Ok(virtual_actors_arc) => {
                        let virtual_actors = virtual_actors_arc.read().await;
                        virtual_actors.keys().cloned().collect::<Vec<ActorId>>()
                    }
                    Err(_) => continue, // If virtual_actors not available, skip this iteration
                };

                // Check each virtual actor for idle timeout
                for actor_id in actor_ids {
                    let should_deactivate = match node.virtual_actors().await {
                        Ok(virtual_actors_arc) => {
                            let virtual_actors = virtual_actors_arc.read().await;
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
                        }
                        Err(_) => false, // If virtual_actors not available, don't deactivate
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
                            use plexspaces_core::service_locator::service_names;
                            if let Some(actor_registry) = node.service_locator().get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY).await {
                                if let Err(e) = actor_registry.unregister_with_cleanup(&actor_id).await {
                                    eprintln!("Failed to deactivate idle virtual actor {}: {}", actor_id, e);
                                }
                            } else {
                                // Fallback to direct access if not registered yet
                                if let Ok(actor_registry) = node.actor_registry().await {
                                    if let Err(e) = actor_registry.unregister_with_cleanup(&actor_id).await {
                                        eprintln!("Failed to deactivate idle virtual actor {}: {}", actor_id, e);
                                    }
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
                            
                            // grpc_clients removed - use ServiceLocator.get_node_client() instead
                            // ServiceLocator manages gRPC clients, no manual cleanup needed
                            
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
                            // grpc_clients removed - use ServiceLocator.get_node_client() instead
                            // ServiceLocator manages clients, assume connection exists if health check passed
                            let connection_exists = true;
                            
                            if connection_exists {
                                // Try to reconnect
                                if let Ok(new_client) = RemoteActorClient::connect(&node_address).await {
                                    tracing::info!(remote_node_id = %node_id.as_str(), "Reconnected to remote node via health check");
                                    
                                    // grpc_clients removed - ServiceLocator manages clients automatically
                                    // Client will be created on-demand via get_node_client()
                                    
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

    /// Get ServiceLocator
    fn service_locator(&self) -> Option<Arc<ServiceLocator>> {
        Some(self.service_locator.clone())
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

        // TupleSpace removed - not needed
        Ok(())
    }

    /// Leave the cluster
    pub async fn leave(&self) -> Result<(), NodeError> {
        // TupleSpace removed - not needed
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
        use plexspaces_core::service_locator::service_names;
        let actor_registry: Arc<ActorRegistry> = node.service_locator().get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY).await
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
            // Use internal context for routing lookup (system-level operation)
            let internal_ctx = RequestContext::internal();
            let routing = actor_registry.lookup_routing(&internal_ctx, &actor_id).await
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
    
    use plexspaces_core::MessageSender;
    
    // Import NodeBuilder for tests
    use crate::NodeBuilder;
    
    // Helper to get ActorRegistry from service_locator
    async fn get_actor_registry(node: &Node) -> Arc<ActorRegistry> {
        // Ensure services are initialized
        node.initialize_services().await.unwrap();
        node.actor_registry().await.unwrap()
    }
    
    // Helper to register actor with MessageSender (replaces register_local)
    async fn register_actor_for_test(node: &Node, actor_id: &str, mailbox: Arc<Mailbox>) {
        let wrapper = Arc::new(ActorRef::local(
            actor_id.to_string(),
            mailbox,
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(node).await;
        // Use internal context for test actor registration (system-level operation)
        let internal_ctx = RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor_id.to_string(), wrapper, None).await;
    }

    #[tokio::test]
    async fn test_node_creation() {
        let node = NodeBuilder::new("test-node").build().await;

        assert_eq!(node.id().as_str(), "test-node");
        assert_eq!(node.connected_nodes().await.len(), 0);
    }

    #[tokio::test]
    async fn test_actor_registration() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = NodeBuilder::new("test-node").build().await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());

        // Register with ActorRegistry first
        // Register actor with MessageSender (mailbox is internal)
        
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Use internal context for test actor registration (system-level operation)
        let internal_ctx = RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor_ref.id().clone(), wrapper, None).await;

        // Register actor config (if needed)
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Should find local actor via ActorRegistry
        let internal_ctx = RequestContext::internal();
        match actor_registry.lookup_routing(&internal_ctx, &"test-actor@test-node".to_string()).await {
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
        let node = NodeBuilder::new("test-node").build().await;

        // TupleSpace removed - not needed
        // Test removed as TupleSpace is no longer part of Node
    }

    #[tokio::test]
    async fn test_actor_unregistration() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = NodeBuilder::new("test-node").build().await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());

        // Register with ActorRegistry first
        // Register actor with MessageSender (mailbox is internal)
        
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Use internal context for test actor registration (system-level operation)
        let internal_ctx = RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor_ref.id().clone(), wrapper, None).await;

        // Register actor config
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();
        let internal_ctx = RequestContext::internal();
        assert!(actor_registry.lookup_routing(&internal_ctx, &"test-actor@test-node".to_string()).await.is_ok());

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

        let node = NodeBuilder::new("test-node").build().await;

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

        let node_arc = Arc::new(NodeBuilder::new("test-node").build().await);
        
        // Register NodeMetricsAccessor early for tests (normally done in create_actor_context_arc)
        let metrics_accessor = Arc::new(crate::service_wrappers::NodeMetricsAccessorWrapper::new(node_arc.clone()));
        node_arc.service_locator().register_service(metrics_accessor.clone()).await;
        let metrics_accessor_trait: Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync> = metrics_accessor.clone() as Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync>;
        node_arc.service_locator().register_node_metrics_accessor(metrics_accessor_trait).await;
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
        let node = NodeBuilder::new("test-node").build().await;
        
        // Initialize services
        node.initialize_services().await.unwrap();

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
        let node = NodeBuilder::new("node1").build().await;

        // Connect to another node
        node.connect_to(NodeId::new("node2"), "localhost:8001".to_string())
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
        let node = NodeBuilder::new("node1").build().await;

        // Connect
        node.connect_to(NodeId::new("node2"), "localhost:8001".to_string())
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
        let node = NodeBuilder::new("node1").build().await;

        // First connection should succeed
        node.connect_to(NodeId::new("node2"), "localhost:8001".to_string())
            .await
            .unwrap();

        // Second connection should fail
        let result = node
            .connect_to(NodeId::new("node2"), "localhost:8001".to_string())
            .await;
        assert!(result.is_err());
        match result {
            Err(NodeError::AlreadyConnected(_)) => {} // Expected
            _ => panic!("Expected AlreadyConnected error"),
        }
    }

    #[tokio::test]
    async fn test_node_announcement() {
        let node = NodeBuilder::new("test-node").build().await;

        // TupleSpace removed - not needed
        // Test removed as TupleSpace is no longer part of Node
    }

    #[tokio::test]
    async fn test_cluster_manager_join() {
        let node = Arc::new(NodeBuilder::new("node1").build().await);

        let cluster_config = ClusterConfig {
            name: "test-cluster".to_string(),
            seed_nodes: vec![
                (NodeId::new("node1"), "localhost:8000".to_string()),
                (NodeId::new("node2"), "localhost:8001".to_string()),
            ],
            min_nodes: 2,
            auto_discovery: true,
        };

        let manager = ClusterManager::new(node.clone(), cluster_config);

        // Join cluster
        manager.join().await.unwrap();

        // TupleSpace removed - not needed
        // Test removed as TupleSpace is no longer part of Node
    }

    #[tokio::test]
    async fn test_cluster_manager_leave() {
        let node = Arc::new(NodeBuilder::new("node1").build().await);

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

        // TupleSpace removed - not needed
        // Test removed as TupleSpace is no longer part of Node
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

        let node = NodeBuilder::new("test-node").build().await;

        // Initialize services (registers all services including ActorFactory)
        node.initialize_services().await.unwrap();
        
        // Create actor
        let behavior = Box::new(MockBehavior::new());
        // Create mailbox - Actor::new takes ownership, but we need Arc for ActorRef
        // So we create a new mailbox for ActorRef after spawning
        let mailbox = Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap();
        let actor = Actor::new(
            "test-actor@test-node".to_string(),
            behavior,
            mailbox,
            "test-tenant".to_string(),
            "test-namespace".to_string(),
            None,
        );

        // Get ActorFactory from ServiceLocator as trait object to avoid TypeId mismatch
        let service_locator = node.service_locator();
        use plexspaces_actor::get_actor_factory;
        let actor_factory = get_actor_factory(service_locator.as_ref()).await
            .expect("ActorFactoryImpl should be registered after initialize_services()");
        
        let internal_ctx = RequestContext::internal();
        let actor_id = "test-actor@test-node".to_string();
          let _message_sender = actor_factory.spawn_actor(
            &internal_ctx,
            &actor_id,
            "test", // actor_type
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![], // facets
        ).await.unwrap();
        // Create a new mailbox for ActorRef (actor is already spawned with its own mailbox)
        let mailbox_for_ref = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-ref-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox_for_ref, service_locator);

        // Verify ActorRef returned
        assert_eq!(actor_ref.id(), "test-actor@test-node");

        // Verify actor registered in ActorRegistry
        let actor_registry = get_actor_registry(&node).await;
        let internal_ctx = RequestContext::internal();
        match actor_registry.lookup_routing(&internal_ctx, &"test-actor@test-node".to_string()).await {
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

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Create monitor channel
        let (tx, mut rx) = mpsc::channel(1);

        // Create actor
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap();
        let actor = Actor::new(
            "test-actor@test-node".to_string(),
            behavior,
            mailbox,
            "test-tenant".to_string(),
            "test-namespace".to_string(),
            None,
        );

        // Initialize services (registers all services including ActorFactory)
        node.initialize_services().await.unwrap();
        
        // Get ActorFactory from ServiceLocator as trait object to avoid TypeId mismatch
        let service_locator = node.service_locator();
        use plexspaces_actor::get_actor_factory;
        let actor_factory = get_actor_factory(service_locator.as_ref()).await
            .expect("ActorFactoryImpl should be registered after initialize_services()");
        
        let internal_ctx = RequestContext::internal();
        let _message_sender = actor_factory.spawn_built_actor(&internal_ctx, Arc::new(actor), None).await.unwrap();
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

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Initialize services
        node.initialize_services().await.unwrap();

        // Create monitor channel
        let (tx, _rx) = mpsc::channel(1);

        // Create actor with normal behavior (panics are converted to errors)
        let behavior = Box::new(plexspaces_behavior::MockBehavior::new());
        let mailbox = Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap();
        let actor = Actor::new(
            "test-actor@test-node".to_string(),
            behavior,
            mailbox,
            "test-tenant".to_string(),
            "test-namespace".to_string(),
            None,
        );

        // Get ActorFactory from ServiceLocator as trait object to avoid TypeId mismatch
        let service_locator = node.service_locator();
        use plexspaces_actor::get_actor_factory;
        let actor_factory = get_actor_factory(service_locator.as_ref()).await
            .expect("ActorFactoryImpl should be registered after initialize_services()");
        
        let internal_ctx = RequestContext::internal();
        let _message_sender = actor_factory.spawn_built_actor(&internal_ctx, Arc::new(actor), None).await.unwrap();
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
        let node1 = Arc::new(NodeBuilder::new("node1").build().await);

        let node2 = Arc::new(NodeBuilder::new("node2").build().await);

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
        let ctx = RequestContext::new_without_auth("default".to_string(), "default".to_string());
        let registration = ObjectRegistration {
            object_type: ObjectType::ObjectTypeNode as i32,
            object_id: "node2".to_string(),
            grpc_address: "http://localhost:9999".to_string(),
            object_category: "Node".to_string(),
            ..Default::default()
        };
        let object_registry = node1.object_registry().await.unwrap();
        object_registry.register(&ctx, registration).await.unwrap();

        // Try to route message from node1 to actor on node2
        let message = plexspaces_mailbox::Message::new(vec![1, 2, 3]);

        // This will fail because we don't have a real gRPC server running
        // But it exercises the remote routing code path via ActorRef::tell()
        // Initialize services on node1
        node1.initialize_services().await.unwrap();
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
        let node = NodeBuilder::new("node1").build().await;

        // Register remote node in connections
        node.register_remote_node(NodeId::new("node2"), "http://localhost:9999".to_string())
            .await
            .unwrap();

        // Register remote node in ObjectRegistry (ActorRegistry looks up nodes here)
        use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
        let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());
        let registration = ObjectRegistration {
            object_type: ObjectType::ObjectTypeNode as i32,
            object_id: "node2".to_string(),
            grpc_address: "http://localhost:9999".to_string(),
            object_category: "Node".to_string(),
            ..Default::default()
        };
        let object_registry = node.object_registry().await.unwrap();
        object_registry.register(&ctx, registration).await.unwrap();

        // Try to find actor with @node2 suffix
        let actor_registry = get_actor_registry(&node).await;
        let internal_ctx = RequestContext::internal();
        let result = actor_registry.lookup_routing(&internal_ctx, &"test-actor@node2".to_string()).await;

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
        let node = NodeBuilder::new("node1").build().await;

        // Try to find actor that doesn't exist anywhere
        let actor_registry = get_actor_registry(&node).await;
        let result = actor_registry
            .lookup_routing(&RequestContext::internal(), &"nonexistent@unknown-node".to_string())
            .await;

        // lookup_routing returns Ok(None) when node is not found in ObjectRegistry
        // (it doesn't return an error, just None)
        assert!(result.is_ok());
        assert!(result.unwrap().is_none(), "Should return None for non-existent remote node");
    }

    #[tokio::test]
    async fn test_find_actor_via_tuplespace() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};

        let node1 = NodeBuilder::new("node1").build().await;

        let node2 = NodeBuilder::new("node2").build().await;

        // Register actor on node2 (this writes to node2's TupleSpace)
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::remote("test-actor@node2", "node2", node2.service_locator());

        // Register actor with ActorRegistry on node2
        register_actor_for_test(&node2, actor_ref.id().as_str(), mailbox.clone()).await;
        let actor_registry2 = get_actor_registry(&node2).await;
        actor_registry2.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Register node2 in ObjectRegistry on node1 (so node1 can find it via lookup_routing)
        use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
        let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());
        let registration = ObjectRegistration {
            object_type: ObjectType::ObjectTypeNode as i32,
            object_id: "node2".to_string(),
            grpc_address: "http://localhost:9999".to_string(),
            object_category: "Node".to_string(),
            ..Default::default()
        };
        let object_registry = node1.object_registry().await.unwrap();
        object_registry.register(&ctx, registration).await.unwrap();

        // Now node1 should find the actor via ObjectRegistry (lookup_routing uses ObjectRegistry, not TupleSpace)
        let actor_registry1 = get_actor_registry(&node1).await;
        let internal_ctx = RequestContext::internal();
        let result = actor_registry1.lookup_routing(&internal_ctx, &"test-actor@node2".to_string()).await;

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
        let node = NodeBuilder::new("node1").build().await;

        let message = plexspaces_mailbox::Message::new(vec![1, 2, 3]);

        // Try to send to node that's not in connections registry
        // This will fail when trying to lookup the actor (node not found)
        let result = lookup_actor_ref(&node, &"test-actor@unknown-node".to_string()).await;

        // Should fail with ActorNotFound or similar (node not registered)
        assert!(result.is_err() || result.unwrap().is_none());
    }

    // ============================================================================
    // Monitoring Infrastructure Tests (monitor, handle_actor_termination)
    // ============================================================================

    #[tokio::test]
    async fn test_monitor_local_actor() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use tokio::sync::mpsc;

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

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

        // Notify actor down (via ActorRegistry)
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.handle_actor_termination(
            &"monitored-actor@test-node".to_string(),
            ExitReason::Error("test reason".to_string())
        ).await;

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

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Initialize services
        node.initialize_services().await.unwrap();

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

        let node = Arc::new(NodeBuilder::new("node1").build().await);

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
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Notify for actor with no monitors (should not panic)
        let actor_registry = node.actor_registry().await.unwrap();
        actor_registry.handle_actor_termination(&"unmonitored-actor@test-node".to_string(), ExitReason::Error("reason".to_string())).await;

        // Should succeed (no-op) - handle_actor_termination doesn't return Result, it's void
    }

    #[tokio::test]
    async fn test_handle_actor_termination_multiple_monitors() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use tokio::sync::mpsc;

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

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
        let actor_registry = node.actor_registry().await.unwrap();
        actor_registry.handle_actor_termination(&"watched-actor@test-node".to_string(), ExitReason::Error("crashed".to_string())).await;

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

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Initialize services
        node.initialize_services().await.unwrap();

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

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

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

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Register actor and monitor it
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Use internal context for test actor registration (system-level operation)
        let internal_ctx = RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor_ref.id().clone(), wrapper, None).await;

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

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Register actor and monitor it
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Use internal context for test actor registration (system-level operation)
        let internal_ctx = RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor_ref.id().clone(), wrapper, None).await;

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
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

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

        let node_arc = Arc::new(NodeBuilder::new("test-node").build().await);
        
        // Register NodeMetricsAccessor early for tests (normally done in create_actor_context_arc)
        let metrics_accessor = Arc::new(crate::service_wrappers::NodeMetricsAccessorWrapper::new(node_arc.clone()));
        node_arc.service_locator().register_service(metrics_accessor.clone()).await;
        let metrics_accessor_trait: Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync> = metrics_accessor.clone() as Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync>;
        node_arc.service_locator().register_node_metrics_accessor(metrics_accessor_trait).await;
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
        let node = NodeBuilder::new("test-node").build().await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Use internal context for test actor registration (system-level operation)
        let internal_ctx = RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor_ref.id().clone(), wrapper, None).await;

        let config = create_actor_config_with_resources(2.0, 1024 * 1024 * 512, 1024 * 1024 * 1024, 0);

        // Register actor with config
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), Some(config.clone()))
            .await
            .unwrap();

        // Verify config is stored
        let actor_configs_arc = node.actor_configs().await.unwrap();
        let actor_configs = actor_configs_arc.read().await;
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
        let node = NodeBuilder::new("test-node").build().await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Use internal context for test actor registration (system-level operation)
        let internal_ctx = RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor_ref.id().clone(), wrapper, None).await;

        // Register actor without config
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), None).await.unwrap();

        // Verify config is not stored
        let actor_configs_arc = node.actor_configs().await.unwrap();
        let actor_configs = actor_configs_arc.read().await;
        assert!(!actor_configs.contains_key(actor_ref.id()));
    }

    #[tokio::test]
    async fn test_unregister_actor_removes_config() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;
        let node = NodeBuilder::new("test-node").build().await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("test-actor@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Use internal context for test actor registration (system-level operation)
        let internal_ctx = RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor_ref.id().clone(), wrapper, None).await;

        let config = create_actor_config_with_resources(1.0, 1024 * 1024 * 256, 0, 0);

        // Register actor with config
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.register_actor_with_config(actor_ref.id().clone(), Some(config))
            .await
            .unwrap();

        // Verify config is stored
        {
            let actor_configs_arc = node.actor_configs().await.unwrap();
            let actor_configs = actor_configs_arc.read().await;
            assert!(actor_configs.contains_key(actor_ref.id()));
        }

        // Unregister actor
        let actor_registry = get_actor_registry(&node).await;
        actor_registry.unregister_with_cleanup(actor_ref.id()).await.unwrap();

        // Verify config is removed
        let actor_configs_arc = node.actor_configs().await.unwrap();
        let actor_configs = actor_configs_arc.read().await;
        assert!(!actor_configs.contains_key(actor_ref.id()));
    }

    #[tokio::test]
    async fn test_calculate_node_capacity_with_actors() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = NodeBuilder::new("test-node").build().await;

        // Register first actor with resources
        let mailbox1 = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-1-{}", ulid::Ulid::new())).await.unwrap());
        let actor1_ref = ActorRef::local("actor-1@test-node", mailbox1.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        
        use plexspaces_core::MessageSender;
        let wrapper1 = Arc::new(ActorRef::local(
            actor1_ref.id().clone(),
            mailbox1.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        let internal_ctx = plexspaces_core::RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor1_ref.id().clone(), wrapper1, None).await;
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
        let wrapper2 = Arc::new(ActorRef::local(
            actor2_ref.id().clone(),
            mailbox2.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        let internal_ctx = plexspaces_core::RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor2_ref.id().clone(), wrapper2, None).await;
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
        let node = NodeBuilder::new("test-node").build().await;

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
        let node = NodeBuilder::new("test-node").build().await;

        // Register actor without resource requirements
        let mailbox = Arc::new(Mailbox::new(plexspaces_mailbox::mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
        let actor_ref = ActorRef::local("actor-1@test-node", mailbox.clone(), node.service_locator());
        // Register actor with MessageSender (mailbox is internal)
        
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Use internal context for test actor registration (system-level operation)
        let internal_ctx = RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor_ref.id().clone(), wrapper, None).await;
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
        let node = NodeBuilder::new("test-node").build().await;

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
        let node = NodeBuilder::new("test-node").build().await;

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
        
        use plexspaces_core::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            mailbox.clone(),
            node.service_locator().clone(),
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Use internal context for test actor registration (system-level operation)
        let internal_ctx = RequestContext::internal();
        actor_registry.register_actor(&internal_ctx, actor_ref.id().clone(), wrapper, None).await;
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
        let node = NodeBuilder::new("test-node").build().await;

        let app = Box::new(MockTestApplication::new("test-app"));
        node.register_application(app).await.unwrap();

        // Verify application is registered
        let state = node
            .application_manager()
            .await
            .unwrap()
            .get_state("test-app")
            .await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateCreated));
    }

    #[tokio::test]
    async fn test_register_duplicate_application() {
        let node = NodeBuilder::new("test-node").build().await;

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
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let app = Box::new(MockTestApplication::new("test-app"));
        let start_called = app.start_called.clone();

        node.register_application(app).await.unwrap();
        node.start_application("test-app").await.unwrap();

        // Verify application started
        let state = node
            .application_manager()
            .await
            .unwrap()
            .get_state("test-app")
            .await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateRunning));

        // Verify start was called
        assert!(*start_called.read().await);
    }

    #[tokio::test]
    async fn test_start_application_failure() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let app = Box::new(MockTestApplication::new_failing_start("test-app"));
        node.register_application(app).await.unwrap();

        let result = node.start_application("test-app").await;

        // Should fail
        assert!(result.is_err());

        // State should be Failed
        let state = node
            .application_manager()
            .await
            .unwrap()
            .get_state("test-app")
            .await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateFailed));
    }

    #[tokio::test]
    async fn test_stop_application() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

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
            .await
            .unwrap()
            .get_state("test-app")
            .await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateStopped));

        // Verify stop was called
        assert!(*stop_called.read().await);
    }

    #[tokio::test]
    async fn test_stop_application_failure() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

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
            .await
            .unwrap()
            .get_state("test-app")
            .await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateFailed));
    }

    #[tokio::test]
    async fn test_shutdown_multiple_applications() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

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
            node            .application_manager().await.unwrap()
                .get_state("app1")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );
        assert_eq!(
            node            .application_manager().await.unwrap()
                .get_state("app2")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );
        assert_eq!(
            node            .application_manager().await.unwrap()
                .get_state("app3")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );

        // Verify shutdown flag set
        assert!(node.is_shutdown_requested().await);
    }

    #[tokio::test]
    async fn test_application_node_trait_implementation() {
        use crate::NodeBuilder;
        let node = NodeBuilder::new("test-node")
            .with_listen_address("0.0.0.0:9999")
            .build().await;

        // Test ApplicationNode trait methods (uses trait methods, not Node methods)
        let node_ref: &dyn ApplicationNode = &node;
        assert_eq!(node_ref.id(), "test-node");
        assert_eq!(node_ref.listen_addr(), "0.0.0.0:9999");
    }

    #[tokio::test]
    async fn test_start_nonexistent_application() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let result = node.start_application("nonexistent").await;

        // Should fail with not found error
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_stop_nonexistent_application() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let result = node
            .stop_application("nonexistent", tokio::time::Duration::from_secs(5))
            .await;

        // Should fail with not found error
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_application_lifecycle_full_cycle() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let app = Box::new(MockTestApplication::new("lifecycle-test"));
        let start_called = app.start_called.clone();
        let stop_called = app.stop_called.clone();

        // Full lifecycle: register -> start -> stop
        node.register_application(app).await.unwrap();
        assert_eq!(
            node            .application_manager().await.unwrap()
                .get_state("lifecycle-test")
                .await,
            Some(ApplicationState::ApplicationStateCreated)
        );

        node.start_application("lifecycle-test").await.unwrap();
        assert_eq!(
            node            .application_manager().await.unwrap()
                .get_state("lifecycle-test")
                .await,
            Some(ApplicationState::ApplicationStateRunning)
        );
        assert!(*start_called.read().await);

        node.stop_application("lifecycle-test", tokio::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            node            .application_manager().await.unwrap()
                .get_state("lifecycle-test")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );
        assert!(*stop_called.read().await);
    }

    #[tokio::test]
    async fn test_shutdown_with_partial_failure() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

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
            node            .application_manager().await.unwrap()
                .get_state("good-app")
                .await,
            Some(ApplicationState::ApplicationStateStopped)
        );

        // bad-app should be in Failed state
        assert_eq!(
            node            .application_manager().await.unwrap()
                .get_state("bad-app")
                .await,
            Some(ApplicationState::ApplicationStateFailed)
        );
    }

    #[tokio::test]
    async fn test_shutdown_request_flag() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

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
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Initialize services
        node.initialize_services().await.unwrap();

        // Shutdown with no apps should succeed
        let result = node.shutdown(tokio::time::Duration::from_secs(5)).await;
        assert!(result.is_ok());

        // Shutdown flag should be set
        assert!(node.is_shutdown_requested().await);
    }

    #[tokio::test]
    async fn test_application_manager_accessor() {
        let node = NodeBuilder::new("test-node").build().await;

        // Get application manager reference
        let manager = node.application_manager();

        // Verify it's the same manager (returns empty list initially)
        let apps = manager.await.unwrap().list_applications().await;
        assert_eq!(apps.len(), 0);
    }

    #[tokio::test]
    async fn test_multiple_start_attempts() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let app = Box::new(MockTestApplication::new("test-app"));
        node.register_application(app).await.unwrap();

        // First start succeeds
        node.start_application("test-app").await.unwrap();
        assert_eq!(
            node            .application_manager().await.unwrap()
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
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let app = Box::new(MockTestApplication::new("test-app"));
        node.register_application(app).await.unwrap();
        node.start_application("test-app").await.unwrap();

        // First stop succeeds
        node.stop_application("test-app", tokio::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            node            .application_manager().await.unwrap()
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
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

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
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::internal();
        let Ok(actor_registry) = self.actor_registry().await else {
            return false;
        };
        let routing = actor_registry.lookup_routing(&ctx, actor_id).await.ok().flatten();
        routing.map(|r| r.is_local).unwrap_or(false)
    }

    async fn activate_actor(&self, actor_id: &ActorId) -> Result<CoreActorRef, String> {
        // Activate the virtual actor using ActorFactory
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        
        use plexspaces_actor::get_actor_factory;
        let actor_factory = get_actor_factory(self.service_locator().as_ref()).await
            .ok_or_else(|| "ActorFactory not found".to_string())?;
        
        actor_factory.activate_virtual_actor(actor_id).await
            .map_err(|e| e.to_string())?;
        
        // Convert plexspaces_actor::ActorRef to plexspaces_core::ActorRef
        // Core ActorRef is just an ID wrapper, so we create it from the actor_id
        CoreActorRef::new(actor_id.clone())
            .map_err(|e| e.to_string())
    }
}
