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

//! Node module for distribution and clustering
//!
//! Provides location transparency and distribution capabilities,
//! inspired by Erlang's node system but elevated for modern needs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;

use plexspaces_actor::{
    ActorId, ActorRegistry,
    ApplicationManager as ApplicationManagerTrait, ExitReason, InitializableServiceLocator,
    ProcessResourceSampler, RequestContext, RequestContextExt,
    ServiceLocator as ServiceLocatorTrait,
};
#[cfg(test)]
use plexspaces_actor::ActorRegistrationParams;
use plexspaces_application::{ApplicationError, ApplicationManager, ApplicationNode};
#[cfg(test)]
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::node::v1::{NodeCapabilities as ProtoNodeCapabilities, NodeMetrics};
use plexspaces_service_traits::ServiceLocatorBase;
use plexspaces_services::ServiceLocatorImpl;
use std::time::Duration;

// Import gRPC client for remote messaging

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

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// MonitorLink is now defined in ActorRegistry (core crate)
// Use plexspaces_actor::MonitorLink instead


// VirtualActorMetadata and MonitorLink are now defined in ActorRegistry (core crate)
// Re-export for convenience
pub use plexspaces_actor::{MonitorLink, VirtualActorMetadata};

/// Node in the distributed system
#[derive(Clone)]
pub struct Node {
    /// Node identifier
    id: NodeId,
    /// Node configuration
    config: plexspaces_proto::node::v1::NodeConfig,
    /// Node metrics (combined resource and operational metrics)
    metrics: Arc<RwLock<NodeMetrics>>,
    /// Reused process sampler so node metrics reflect the current process footprint.
    process_sampler: Arc<std::sync::Mutex<ProcessResourceSampler>>,
    /// Start time for uptime calculation
    start_time: Arc<RwLock<Option<tokio::time::Instant>>>,
    /// Background scheduler (Phase 4: Resource-aware scheduling)
    /// Processes scheduling requests asynchronously with lease-based coordination
    background_scheduler:
        Arc<RwLock<Option<Arc<plexspaces_scheduler::background::BackgroundScheduler>>>>,
    /// Task router (Phase 5: Task routing)
    /// Routes tasks to shard groups using channels
    task_router: Arc<RwLock<Option<Arc<plexspaces_scheduler::TaskRouter>>>>,
    /// WASM runtime for dynamic actor deployment (created in start())
    wasm_runtime: Arc<RwLock<Option<Arc<plexspaces_wasm_runtime::WasmRuntime>>>>,
    /// Blob storage service (S3-compatible object storage)
    /// Created in start() if blob config is provided
    blob_service: Arc<RwLock<Option<Arc<plexspaces_blob::BlobService>>>>,
    /// Embedded object store process (kept alive for node's lifetime when using embedded backend)
    _embedded_object_store: Arc<RwLock<Option<plexspaces_blob::embedded_object_store::EmbeddedObjectStore>>>,
    /// Shutdown trigger for programmatic shutdown (allows shutdown() to stop gRPC server)
    shutdown_tx: Arc<RwLock<Option<tokio::sync::oneshot::Sender<()>>>>,
    /// ServiceLocator for centralized service registration and gRPC client caching
    service_locator: Arc<ServiceLocatorImpl>,
    /// Health reporter for health checks and graceful shutdown
    /// Set in Node::start(), None before start
    health_reporter: Arc<RwLock<Option<Arc<plexspaces_actor::PlexSpacesHealthReporter>>>>,
    /// ReleaseSpec configuration (optional, loaded from release config files)
    /// If provided, NodeConfig will be extracted from release_spec.node in start()
    release_spec: Arc<RwLock<Option<plexspaces_proto::node::v1::ReleaseSpec>>>,
    /// ApplicationManager for managing application lifecycle
    /// NOT registered in ServiceLocator - managed directly by Node
    application_manager: Arc<ApplicationManager>,
}

/// Helper function to create default NodeConfig
/// (Can't implement Default trait for proto-generated types)
pub fn default_node_config() -> plexspaces_proto::node::v1::NodeConfig {
    plexspaces_proto::node::v1::NodeConfig {
        grpc_connection_pool_size: 2,
        id: "default-node".to_string(),
        listen_addr: "0.0.0.0:8000".to_string(),
        cluster_seed_nodes: vec![],
        cluster_name: String::new(),
        max_connections: 100,
        heartbeat_interval_ms: 5000,
        clustering_enabled: true,
        metadata: HashMap::new(),
        node_registry: None,         // Use defaults from NodeRegistryConfig
        grpc_address: String::new(), // Derived from listen_addr if empty
        blob_http_port: 0,           // 0 = derive as grpc_port + 100
    }
}

/// Resolve the blob HTTP port from config, defaulting to `grpc_port + 100`.
///
/// Used for both the Axum REST server (non-embedded) and the embedded rustfs
/// subprocess — they share a single `blob_http_port`.
fn resolve_blob_http_port(config: &plexspaces_proto::node::v1::NodeConfig) -> u16 {
    if config.blob_http_port != 0 {
        return config.blob_http_port as u16;
    }
    config
        .listen_addr
        .parse::<std::net::SocketAddr>()
        .map(|a| a.port().saturating_add(100))
        .unwrap_or(8100)
}

// Note: NodeMetrics is from proto crate, so we can't add methods to it
// Use direct field access: metrics.active_actors (u32) instead of usize

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
        active_actors: 0,
        connected_nodes: 0,
        shard_groups_created: 0,
        shard_messages_sent: 0,
        shard_messages_received: 0,
        shard_operations_total: 0,
        shard_operations_failed: 0,
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
    pub fn new(id: NodeId, config: plexspaces_proto::node::v1::NodeConfig) -> Self {
        // Create ServiceLocator - services will be registered in start() using create_default_service_locator
        // This ensures production-ready service initialization
        let service_locator = Arc::new(ServiceLocatorImpl::new());
        let node_id_str = id.as_str().to_string();

        Node {
            id,
            service_locator,
            config,
            metrics: Arc::new(RwLock::new(default_node_metrics(&node_id_str, ""))),
            process_sampler: Arc::new(std::sync::Mutex::new(
                ProcessResourceSampler::new()
                    .expect("process metrics sampler must initialize for current process"),
            )),
            start_time: Arc::new(RwLock::new(None)), // Set in start()
            shutdown_tx: Arc::new(RwLock::new(None)), // Shutdown trigger (set in start())
            background_scheduler: Arc::new(RwLock::new(None)), // Phase 4: Background scheduler (created in start())
            task_router: Arc::new(RwLock::new(None)), // Phase 5: Task router (created in start())
            wasm_runtime: Arc::new(RwLock::new(None)), // WASM runtime (created in start())
            blob_service: Arc::new(RwLock::new(None)), // Blob service (created in start())
            _embedded_object_store: Arc::new(RwLock::new(None)), // Embedded object store process (if any)
            health_reporter: Arc::new(RwLock::new(None)), // Health reporter (created in start())
            release_spec: Arc::new(RwLock::new(None)), // ReleaseSpec (optional, loaded from config)
            application_manager: Arc::new(ApplicationManager::new()), // ApplicationManager (NOT in ServiceLocator)
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
        // CRITICAL: Node ID from NodeBuilder/CLI takes priority over release.yaml
        // This ensures --node-id "node1" overrides release.yaml's "my-node"
        let actual_node_id = self.id.as_str().to_string();

        // Determine NodeConfig: use release_spec.node for defaults, but override runtime-resolved
        // identity and addresses from NodeBuilder/CLI so multi-node local runs do not keep the
        // release.yaml default port in SWIM, seed-node self-alias checks, or gRPC dialing.
        let mut proto_node_config = {
            let release_spec = self.release_spec.read().await;
            if let Some(ref spec) = *release_spec {
                // Use NodeConfig from ReleaseSpec if available, but override id
                if let Some(ref node_config) = spec.node {
                    let mut config = node_config.clone();
                    // CRITICAL: Override node ID with actual node ID from NodeBuilder/CLI
                    // This ensures actor IDs use the correct node suffix for routing
                    config.id = actual_node_id.clone();
                    config
                } else {
                    // ReleaseSpec exists but node is None - create default from Node config
                    plexspaces_proto::node::v1::NodeConfig {
                        id: actual_node_id.clone(),
                        listen_addr: self.config.listen_addr.clone(),
                        cluster_seed_nodes: self.config.cluster_seed_nodes.clone(),
                        cluster_name: self.config.cluster_name.clone(),
                        max_connections: self.config.max_connections,
                        heartbeat_interval_ms: self.config.heartbeat_interval_ms,
                        clustering_enabled: self.config.clustering_enabled,
                        grpc_connection_pool_size: self.config.grpc_connection_pool_size,
                        metadata: self.config.metadata.clone(),
                        node_registry: None,
                        grpc_address: self.config.grpc_address.clone(),
                        blob_http_port: self.config.blob_http_port,
                    }
                }
            } else {
                // No ReleaseSpec - create default from Node config
                plexspaces_proto::node::v1::NodeConfig {
                    id: actual_node_id.clone(),
                    listen_addr: self.config.listen_addr.clone(),
                    cluster_seed_nodes: self.config.cluster_seed_nodes.clone(),
                    cluster_name: self.config.cluster_name.clone(),
                    max_connections: self.config.max_connections,
                    heartbeat_interval_ms: self.config.heartbeat_interval_ms,
                    clustering_enabled: self.config.clustering_enabled,
                    grpc_connection_pool_size: self.config.grpc_connection_pool_size,
                    metadata: self.config.metadata.clone(),
                    node_registry: None,
                    grpc_address: self.config.grpc_address.clone(),
                    blob_http_port: self.config.blob_http_port,
                }
            }
        };

        // CRITICAL: Ensure runtime-resolved identity and addresses win over release defaults.
        proto_node_config.id = actual_node_id.clone();
        if !self.config.listen_addr.is_empty() {
            proto_node_config.listen_addr = self.config.listen_addr.clone();
        }
        if !self.config.grpc_address.is_empty() {
            proto_node_config.grpc_address = self.config.grpc_address.clone();
        } else {
            proto_node_config.grpc_address = self.config.listen_addr.clone();
        }
        // Propagate cluster_name from NodeBuilder config if explicitly set
        if !self.config.cluster_name.is_empty() {
            proto_node_config.cluster_name = self.config.cluster_name.clone();
        }

        // CRITICAL: Set PLEXSPACES_NODE_ID env var so config_manager::initialize() uses correct node ID
        // This ensures ActorRegistry and all components use the correct node ID (from CLI args, not release.yaml)
        std::env::set_var("PLEXSPACES_NODE_ID", &actual_node_id);

        // Initialize all services from the effective release configuration.
        // ServiceLocator now creates ActorFactoryImpl, facet factories, ActorServiceImpl, and TupleSpaceProvider.
        // Ensure release.node reflects the resolved node identity from NodeBuilder/CLI.
        self.service_locator
            .initialize_services(Some({
                let mut release = self.release_spec.read().await.clone().unwrap_or_default();
                release.node = Some(proto_node_config.clone());
                release
            }))
            .await;

        // Register ApplicationManager in ServiceLocator for ApplicationServiceImpl to use
        // (This is Node-specific, so it stays here)
        let app_manager: Arc<dyn plexspaces_actor::ApplicationManager> =
            self.application_manager.clone() as Arc<dyn plexspaces_actor::ApplicationManager>;
        self.service_locator
            .register_application_manager(app_manager)
            .await;

        // Create and register WASM runtime so deploy_application works after build() (e.g. in tests)
        // start() will skip re-creating if already registered (ServiceLocator is idempotent for this)
        if self.service_locator.get_wasm_runtime().await.is_none() {
            use plexspaces_wasm_runtime::WasmRuntime;
            let wasm_runtime = Arc::new(WasmRuntime::new().await.map_err(|e| {
                NodeError::ConfigError(format!("Failed to create WASM runtime: {}", e))
            })?);
            let wasm_runtime_trait: Arc<dyn plexspaces_actor::WasmRuntimeTrait> =
                wasm_runtime.clone();
            self.service_locator
                .register_wasm_runtime(wasm_runtime_trait)
                .await;
            let mut stored_runtime = self.wasm_runtime.write().await;
            *stored_runtime = Some(wasm_runtime);
        }

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

    /// Get release name from ReleaseSpec
    ///
    /// ## Returns
    /// * `Some(name)` - Release name if ReleaseSpec is set
    /// * `None` - No ReleaseSpec configured
    pub async fn release_name(&self) -> Option<String> {
        let spec = self.release_spec.read().await;
        spec.as_ref().map(|s| s.name.clone())
    }

    /// Get release version from ReleaseSpec
    ///
    /// ## Returns
    /// * `Some(version)` - Release version if ReleaseSpec is set
    /// * `None` - No ReleaseSpec configured
    pub async fn release_version(&self) -> Option<String> {
        let spec = self.release_spec.read().await;
        spec.as_ref().map(|s| s.version.clone())
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
    pub(crate) async fn load_release_config(
        &self,
    ) -> Result<plexspaces_proto::node::v1::ReleaseSpec, NodeError> {
        use crate::config::loader::ConfigLoader;
        use std::env;

        if cfg!(test) && env::var("PLEXSPACES_RELEASE_CONFIG_PATH").is_err() {
            return Err(NodeError::ConfigError(
                "Implicit release config auto-loading is disabled for test builds".to_string(),
            ));
        }

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
            let mut spec = loader.load_release_spec(&path).await.map_err(|e| {
                NodeError::ConfigError(format!(
                    "Failed to load release config from {}: {}",
                    path, e
                ))
            })?;
            // Apply env overrides and set defaults through config_manager
            plexspaces_common::config_manager::initialize(&mut spec);
            Ok(spec)
        } else {
            Err(NodeError::ConfigError(
                "No release config file found and PLEXSPACES_RELEASE_CONFIG_PATH not set"
                    .to_string(),
            ))
        }
    }

    /// Get node ID
    pub fn id(&self) -> &NodeId {
        &self.id
    }

    /// Get node configuration
    pub fn config(&self) -> &plexspaces_proto::node::v1::NodeConfig {
        &self.config
    }

    /// Get ServiceLocator (returns concrete ServiceLocatorImpl type)
    ///
    /// ## Purpose
    /// Returns the concrete ServiceLocatorImpl type. ServiceLocatorImpl implements
    /// the ServiceLocator trait, so it can be used wherever a trait object is needed.
    ///
    /// ## Design
    /// ServiceLocatorImpl is the only production implementation, so returning the concrete
    /// type is safe and production-grade. Cast to trait object when needed:
    /// `service_locator.clone() as Arc<dyn ServiceLocator>`
    pub fn service_locator(&self) -> Arc<plexspaces_services::ServiceLocatorImpl> {
        self.service_locator.clone()
    }

    /// Spawn an actor on this node
    ///
    /// Delegates to `ActorFactory::spawn_actor()` - same parameters.
    /// This is a convenience method that avoids getting ActorFactory from ServiceLocator.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant/namespace isolation (REQUIRED, explicit)
    /// * `actor_id` - Canonical actor ID (`name//actor_type::namespace@node_id`)
    /// * `actor_type` - Type of actor (e.g., "GenServer")
    /// * `initial_state` - Initial state bytes
    /// * `config` - Optional actor configuration
    /// * `labels` - Optional labels
    /// * `facets` - Optional facets to attach
    ///
    /// ## Returns
    /// `Arc<dyn MessageSender>` for the spawned actor
    ///
    /// ## Example
    /// ```rust,ignore
    /// let ctx = RequestContext::new_without_auth("tenant".to_string(), "namespace".to_string());
    /// let actor_id = ActorId::from_canonical("counter//gen_server::default@my-node")?;
    /// let actor = node.spawn(&ctx, &actor_id, "GenServer", vec![], None, HashMap::new(), vec![]).await?;
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        actor_type: &str,
        _initial_state: Vec<u8>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        labels: std::collections::HashMap<String, String>,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<Arc<dyn plexspaces_actor::MessageSender>, NodeError> {
        let actor_factory = self
            .service_locator()
            .get_actor_factory()
            .await
            .ok_or_else(|| {
                NodeError::ConfigError("ActorFactory not found in ServiceLocator".to_string())
            })?;

        use plexspaces_actor::ActorSpawnSpec;
        use plexspaces_proto::common::v1::ActorIdentity;
        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: actor_id.name().to_string(),
                actor_type: actor_type.to_string(),
            }),
            role: String::new(),
            namespace: ctx.namespace().to_string(),
            tenant_id: ctx.tenant_id().to_string(),
            visibility: 0,
            behavior_kind: String::new(),
            args: std::collections::HashMap::new(),
            facets: vec![],
            config,
            labels,
            ..Default::default()
        };
        actor_factory
            .spawn_actor(ctx, &spec, facets)
            .await
            .map_err(|e| NodeError::ActorSpawnFailed(e.to_string()))
    }

    /// Register proto NodeConfig in ServiceLocator
    ///
    /// ## Purpose
    /// Registers the proto NodeConfig (from ReleaseSpec.node) in ServiceLocator
    /// so that services can access node configuration.
    ///
    /// ## Arguments
    /// * `node_config` - Proto NodeConfig from ReleaseSpec.node
    ///
    /// ## Note
    /// This should be called after Node creation if ReleaseSpec is available.
    pub async fn register_node_config(&self, node_config: plexspaces_proto::node::v1::NodeConfig) {
        self.service_locator.register_node_config(node_config).await;
    }

    /// Get ActorRegistry (internal use only - use service_locator.actor_registry() instead)
    pub(crate) async fn actor_registry(&self) -> Result<Arc<ActorRegistry>, NodeError> {
        self.service_locator.actor_registry().await.ok_or_else(|| {
            NodeError::ConfigError("ActorRegistry not found in ServiceLocator".to_string())
        })
    }

    fn map_actor_registry_error(e: plexspaces_actor::ActorRegistryError) -> NodeError {
        use plexspaces_actor::ActorRegistryError;
        match e {
            ActorRegistryError::ActorNotFound(id) => NodeError::ActorNotFound(id),
            ActorRegistryError::SendFailed(msg) => NodeError::NetworkError(msg),
            ActorRegistryError::RegistrationFailed(msg) => NodeError::InvalidArgument(msg),
            ActorRegistryError::LookupFailed(msg) => NodeError::InvalidArgument(msg),
            ActorRegistryError::UnregistrationFailed(msg) => NodeError::InvalidArgument(msg),
            ActorRegistryError::Timeout => {
                NodeError::InvalidArgument("ActorRegistry timeout".into())
            }
            ActorRegistryError::DependencyUnavailable(m) => NodeError::InvalidArgument(m),
            ActorRegistryError::VisibilityDenied(m) => NodeError::InvalidArgument(m),
            ActorRegistryError::LinkMonitorDenied(m) => NodeError::InvalidArgument(m),
        }
    }

    // Actor registry, virtual actors, facets: use `service_locator().actor_registry().await`,
    // `.virtual_actor_manager().await`, `.facet_manager().await` per [`plexspaces_actor::ServiceLocator`].

    /// Get health reporter (for tests and advanced usage)
    pub async fn health_reporter(&self) -> Option<Arc<plexspaces_actor::PlexSpacesHealthReporter>> {
        let guard = self.health_reporter.read().await;
        guard.clone()
    }

    /// Get node statistics
    ///
    /// ## Note
    /// For gRPC-based metrics, use `NodeService::get_metrics`.
    pub async fn metrics(&self) -> NodeMetrics {
        self.update_metrics_with_system_info().await;
        let mut m = self.metrics.read().await.clone();
        let sl: Arc<dyn plexspaces_actor::ServiceLocator> = self.service_locator.clone();
        if let Some(renderer) = sl.get_metrics_prometheus_renderer().await {
            let text = renderer.render_prometheus_text();
            plexspaces_actor::overlay_node_operational_counters_from_exposition(
                &text,
                self.id.as_str(),
                &mut m,
            );
        }
        m
    }

    /// Look up a remote node's address from NodeRegistry
    async fn lookup_node_address(&self, node_id: &NodeId) -> Result<String, NodeError> {
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator().clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
        if let Some(node_registry) = service_locator_trait.get_node_registry().await {
            let ctx = service_locator_trait
                .request_context_for_system_operations()
                .await;
            match node_registry.lookup_node(&ctx, node_id.as_str()).await {
                Ok(Some(registration)) => Ok(registration.node_address),
                Ok(None) => Err(NodeError::NodeNotConnected(node_id.clone())),
                Err(e) => Err(NodeError::NetworkError(format!(
                    "Failed to lookup node: {}",
                    e
                ))),
            }
        } else {
            Err(NodeError::NetworkError(
                "NodeRegistry not available".to_string(),
            ))
        }
    }

    /// Update metrics with current system info (CPU, memory, uptime, actors, connected nodes)
    pub async fn update_metrics_with_system_info(&self) {
        let process_snapshot = self
            .process_sampler
            .lock()
            .expect("process metrics sampler lock poisoned")
            .sample();

        // Calculate uptime (time since node started)
        let uptime_seconds = if let Some(start_time) = self.start_time.read().await.as_ref() {
            start_time.elapsed().as_secs()
        } else {
            0
        };

        // Get actor counts from ActorRegistry
        let active_actors =
            if let Some(actor_registry) = self.service_locator.actor_registry().await {
                actor_registry.live_actor_count().await as u32
            } else {
                0
            };

        // Get connected nodes count from NodeRegistry
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator().clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
        let connected_nodes =
            if let Some(node_registry) = service_locator_trait.get_node_registry().await {
                let ctx = service_locator_trait
                    .request_context_for_system_operations()
                    .await;
                let local_node_id = self.id().as_str().to_string();
                // List nodes from registry (excluding self)
                match node_registry.list_nodes(&ctx, None, 1000, "").await {
                    Ok((nodes, _)) => {
                        nodes.iter().filter(|n| n.node_id != local_node_id).count() as u32
                    }
                    Err(_) => 0,
                }
            } else {
                0
            };

        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.memory_used_bytes = process_snapshot.memory_used_bytes;
        metrics.memory_available_bytes = 0;
        metrics.cpu_usage_percent = process_snapshot.cpu_usage_percent;
        metrics.uptime_seconds = uptime_seconds;
        metrics.active_actors = active_actors;
        metrics.connected_nodes = connected_nodes;
    }

    /// Get node statistics (alias for metrics)
    ///
    /// ## Note
    /// For gRPC-based metrics, use `NodeService::get_metrics`.
    pub async fn stats(&self) -> NodeMetrics {
        self.metrics().await
    }

    /// Calculate the current node capacity.
    /// This includes total, allocated, and available resources.
    #[cfg_attr(test, allow(dead_code))] // Allow dead code in tests (used by tests)
    pub(crate) async fn calculate_node_capacity(&self) -> plexspaces_proto::node::v1::NodeCapacity {
        use plexspaces_proto::{common::v1::ResourceSpec, node::v1::NodeCapacity};
        use sysinfo::System;

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
        let _gpu_count = 0u32; // TODO: Integrate GPU detection library if needed

        let total_resources = ResourceSpec {
            cpu_cores: total_cpu_cores,
            memory_bytes: total_memory_bytes,
            disk_bytes: total_disk_bytes,
            gpu_count: 0,            // Placeholder
            gpu_type: String::new(), // Placeholder
        };

        // Allocated resources (sum of all active actors' resource requirements)
        let mut allocated_cpu_cores = 0.0;
        let mut allocated_memory_bytes = 0u64;
        let mut allocated_disk_bytes = 0u64;
        let mut allocated_gpu_count = 0u32;

        // Get actor configs and sum up resource requirements
        let actor_configs_arc = self
            .actor_registry()
            .await
            .map(|r| r.actor_configs().clone())
            .unwrap_or_else(|_| Arc::new(RwLock::new(HashMap::new())));
        let actor_configs = actor_configs_arc.read().await;
        for config in actor_configs.values() {
            if let Some(ref resource_reqs) = config.resource_requirements {
                let resources = resource_reqs
                    .placement
                    .as_ref()
                    .and_then(|p| p.resource_requirements.as_ref());
                if let Some(resources) = resources {
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

    /// Send heartbeat with node capacity through NodeRegistry.
    async fn send_heartbeat_with_capacity(&self) -> Result<(), NodeError> {
        let node_capacity = self.calculate_node_capacity().await;
        let active_actors = {
            let metrics = self.metrics.read().await;
            metrics.active_actors as u64
        };

        // Build metrics map from capacity (convert bytes to MB for readability)
        let mut metrics = std::collections::HashMap::new();
        if let Some(total) = &node_capacity.total {
            metrics.insert("total_cpu_cores".to_string(), total.cpu_cores);
            metrics.insert(
                "total_memory_mb".to_string(),
                (total.memory_bytes / (1024 * 1024)) as f64,
            );
            metrics.insert(
                "total_disk_mb".to_string(),
                (total.disk_bytes / (1024 * 1024)) as f64,
            );
            metrics.insert("total_gpu_count".to_string(), total.gpu_count as f64);
        }
        if let Some(allocated) = &node_capacity.allocated {
            metrics.insert("allocated_cpu_cores".to_string(), allocated.cpu_cores);
            metrics.insert(
                "allocated_memory_mb".to_string(),
                (allocated.memory_bytes / (1024 * 1024)) as f64,
            );
            metrics.insert(
                "allocated_disk_mb".to_string(),
                (allocated.disk_bytes / (1024 * 1024)) as f64,
            );
            metrics.insert(
                "allocated_gpu_count".to_string(),
                allocated.gpu_count as f64,
            );
        }
        if let Some(available) = &node_capacity.available {
            metrics.insert("available_cpu_cores".to_string(), available.cpu_cores);
            metrics.insert(
                "available_memory_mb".to_string(),
                (available.memory_bytes / (1024 * 1024)) as f64,
            );
            metrics.insert(
                "available_disk_mb".to_string(),
                (available.disk_bytes / (1024 * 1024)) as f64,
            );
            metrics.insert(
                "available_gpu_count".to_string(),
                available.gpu_count as f64,
            );
        }
        metrics.insert("active_actors".to_string(), active_actors as f64);

        // Get cluster_name from NodeConfig if available (same as registration)
        let cluster_name = self
            .service_locator
            .get_node_config()
            .await
            .and_then(|config| {
                if !config.cluster_name.is_empty() {
                    Some(config.cluster_name)
                } else {
                    None
                }
            });

        // Use same context as registration (internal context, cluster_name as namespace if defined)
        let ctx = if let Some(cluster) = &cluster_name {
            // Use cluster_name as namespace for cluster isolation (same as registration)
            let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
                self.service_locator().clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
            service_locator_trait
                .request_context_for_system_operations_with_namespace(cluster.clone())
                .await
        } else {
            // Use default internal context (same as registration)
            let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
                self.service_locator().clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
            service_locator_trait
                .request_context_for_system_operations()
                .await
        };

        if let Some(node_registry) = self.service_locator.get_node_registry().await {
            if let Err(e) = node_registry
                .send_heartbeat(&ctx, self.id.as_str(), Some(node_capacity))
                .await
            {
                return Err(NodeError::NetworkError(format!(
                    "NodeRegistry heartbeat failed: {}",
                    e
                )));
            }
        } else {
            return Err(NodeError::ConfigError(
                "NodeRegistry not found in ServiceLocator".to_string(),
            ));
        }

        // Scan for stale object registrations and advance their health lifecycle.
        // Threshold = 3 × heartbeat_interval (matches default max_heartbeat_failures=3).
        // Done inline so no extra background task is needed.
        self.scan_stale_object_heartbeats(self.config.heartbeat_interval_ms).await;

        Ok(())
    }

    /// Scan the object registry for stale registrations and call `record_heartbeat_failure`
    /// for each one.  Uses an admin context so the scan is cross-tenant.
    ///
    /// `heartbeat_interval_ms` is the node's own heartbeat interval; the staleness
    /// threshold is set to 3× this value to align with `max_heartbeat_failures=3`.
    async fn scan_stale_object_heartbeats(&self, heartbeat_interval_ms: u64) {
        let object_registry = match self.service_locator.get_object_registry().await {
            Some(r) => r,
            None => return,
        };

        // Empty tenant_id + is_admin=true → cross-tenant scan.
        let admin_ctx =
            RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);
        let threshold_secs = ((heartbeat_interval_ms * 3) / 1000) as i64;

        let stale = match object_registry
            .find_stale_heartbeats(&admin_ctx, threshold_secs, 1000)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    node_id = %self.id.as_str(),
                    error = %e,
                    "Failed to scan stale object heartbeats"
                );
                return;
            }
        };

        let local_node_suffix = format!("@{}", self.id.as_str());
        for reg in stale {
            if reg.object_id.ends_with(&local_node_suffix) || reg.object_id == self.id.as_str() {
                continue;
            }
            let tenant_ctx = RequestContext::new_without_auth(
                reg.tenant_id.clone(),
                reg.namespace.clone(),
            );
            match object_registry
                .record_heartbeat_failure(&tenant_ctx, &reg.object_id)
                .await
            {
                Ok(new_status) => {
                    tracing::debug!(
                        node_id = %self.id.as_str(),
                        object_id = %reg.object_id,
                        object_type = %reg.object_type,
                        grpc_address = %reg.grpc_address,
                        tenant_id = %reg.tenant_id,
                        new_status = ?new_status,
                        "Stale object heartbeat: recorded failure"
                    );
                }
                Err(e) => {
                    // NotFound is benign — object removed between scan and failure record.
                    let msg = e.to_string();
                    if msg.contains("not found") || msg.contains("NotFound") {
                        tracing::debug!(
                            object_id = %reg.object_id,
                            "Stale object scan: object removed before failure recorded (benign)"
                        );
                    } else {
                        tracing::warn!(
                            node_id = %self.id.as_str(),
                            object_id = %reg.object_id,
                            error = %e,
                            "Stale object scan: record_heartbeat_failure failed"
                        );
                    }
                }
            }
        }
    }

    /// Check liveness of actor object-registry entries and keep health status in sync.
    ///
    /// ## Local actors
    /// Presence in the local `ActorRegistry` is the authoritative liveness signal —
    /// no round-trip needed.  Actors found in the registry get their object-registry
    /// heartbeat refreshed; actors NOT found get a failure recorded.
    ///
    /// ## Remote actors
    /// Actor entries hosted on OTHER nodes are checked via a single `GetActorStates`
    /// batch RPC per remote node (grouped by `node_id`), which is far cheaper than
    /// individual `__PING__` messages.
    ///
    /// This is intentionally sparse — called at 2× the node heartbeat interval.
    async fn check_actor_liveness(&self) {
        use plexspaces_proto::object_registry::v1::ObjectType;
        use plexspaces_proto::v1::actor::{ActorState, GetActorStatesRequest};
        use plexspaces_proto::ActorServiceClient;

        let actor_registry = match self.service_locator.actor_registry().await {
            Some(r) => r,
            None => return,
        };
        let object_registry = match self.service_locator.get_object_registry().await {
            Some(r) => r,
            None => return,
        };
        let node_id_str = self.id.as_str().to_string();

        // Admin context for cross-tenant discovery of all ACTOR entries.
        let admin_ctx =
            RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);

        // Discover HEALTHY and DEGRADED actors (both are still "live" — DEAD/STOPPING are skipped).
        // Two queries: one per health status, then merge.
        let mut registrations = Vec::new();
        for status in [
            plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy,
            plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusDegraded,
        ] {
            match object_registry
                .discover(
                    &admin_ctx,
                    plexspaces_object_registry::DiscoverOptions {
                        object_type: Some(ObjectType::ObjectTypeActor),
                        health_status: Some(status),
                        limit: 10_000,
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(mut r) => registrations.append(&mut r),
                Err(e) => {
                    tracing::warn!(node_id = %node_id_str, error = %e, "Actor liveness: discover failed");
                    return;
                }
            }
        }

        if registrations.is_empty() {
            return;
        }

        // Partition into local (this node) and remote (other nodes).
        let mut remote_by_node: std::collections::HashMap<String, Vec<(String, String, String)>> =
            std::collections::HashMap::new();

        for reg in registrations {
            let ctx = RequestContext::new_without_auth(
                reg.tenant_id.clone(),
                reg.namespace.clone(),
            );

            if reg.node_id == node_id_str || reg.node_id.is_empty() {
                // Local: actor_registry presence = alive.
                let actor_id = match ActorId::from_canonical(&reg.object_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                if actor_registry.is_actor_activated(&actor_id).await {
                    // Alive — refresh object-registry heartbeat to prevent stale scan from flagging it.
                    if let Err(e) = object_registry
                        .heartbeat(&ctx, ObjectType::ObjectTypeActor, &reg.object_id)
                        .await
                    {
                        let msg = e.to_string();
                        if !msg.contains("not found") && !msg.contains("NotFound") {
                            tracing::warn!(
                                node_id = %node_id_str,
                                actor_id = %reg.object_id,
                                error = %e,
                                "Actor liveness: heartbeat update failed"
                            );
                        }
                    }
                } else {
                    // Not in local registry — record failure so health lifecycle advances.
                    if let Err(e) = object_registry
                        .record_heartbeat_failure(&ctx, &reg.object_id)
                        .await
                    {
                        let msg = e.to_string();
                        if !msg.contains("not found") && !msg.contains("NotFound") {
                            tracing::warn!(
                                node_id = %node_id_str,
                                actor_id = %reg.object_id,
                                error = %e,
                                "Actor liveness: record_heartbeat_failure failed"
                            );
                        }
                    }
                }
            } else {
                // Remote: group by node_id for batch RPC.
                remote_by_node
                    .entry(reg.node_id.clone())
                    .or_default()
                    .push((reg.object_id, reg.tenant_id, reg.namespace));
            }
        }

        // Check remote actors via GetActorStates batch call per node.
        if remote_by_node.is_empty() {
            return;
        }

        let node_registry = match self.service_locator.get_node_registry().await {
            Some(r) => r,
            None => return,
        };
        let conn_manager = match self.service_locator.get_grpc_connection_manager().await {
            Some(m) => m,
            None => return,
        };
        let sys_ctx = self
            .service_locator()
            .request_context_for_system_operations()
            .await;

        for (remote_node_id, actor_entries) in remote_by_node {
            let node_address = match node_registry.lookup_node(&sys_ctx, &remote_node_id).await {
                Ok(Some(reg)) => reg.node_address,
                _ => {
                    tracing::debug!(node_id = %remote_node_id, "Actor liveness: remote node not found");
                    continue;
                }
            };

            let channel = match conn_manager
                .get_actor_service_connection(&remote_node_id, &node_address)
                .await
            {
                Ok(ch) => ch,
                Err(e) => {
                    tracing::debug!(node_id = %remote_node_id, error = %e, "Actor liveness: connect failed");
                    continue;
                }
            };

            let actor_ids: Vec<String> = actor_entries.iter().map(|(id, _, _)| id.clone()).collect();
            let mut client = ActorServiceClient::new(channel);
            let resp = match client
                .get_actor_states(tonic::Request::new(GetActorStatesRequest { request_id: ulid::Ulid::new().to_string(), actor_ids }))
                .await
            {
                Ok(r) => r.into_inner(),
                Err(e) => {
                    tracing::debug!(node_id = %remote_node_id, error = %e, "Actor liveness: GetActorStates RPC failed");
                    continue;
                }
            };

            for (actor_id, tenant_id, namespace) in &actor_entries {
                let ctx = RequestContext::new_without_auth(tenant_id.clone(), namespace.clone());
                let state = resp
                    .states
                    .get(actor_id)
                    .copied()
                    .unwrap_or(ActorState::ActorStateUnspecified as i32);

                if state == ActorState::ActorStateActive as i32 {
                    // Still alive on remote node — refresh heartbeat.
                    if let Err(e) = object_registry
                        .heartbeat(&ctx, ObjectType::ObjectTypeActor, actor_id)
                        .await
                    {
                        let msg = e.to_string();
                        if !msg.contains("not found") && !msg.contains("NotFound") {
                            tracing::warn!(
                                remote_node = %remote_node_id,
                                actor_id = %actor_id,
                                error = %e,
                                "Actor liveness: remote heartbeat update failed"
                            );
                        }
                    }
                } else {
                    // Not active on remote node — record failure.
                    if let Err(e) = object_registry
                        .record_heartbeat_failure(&ctx, actor_id)
                        .await
                    {
                        let msg = e.to_string();
                        if !msg.contains("not found") && !msg.contains("NotFound") {
                            tracing::warn!(
                                remote_node = %remote_node_id,
                                actor_id = %actor_id,
                                error = %e,
                                "Actor liveness: remote record_heartbeat_failure failed"
                            );
                        }
                    }
                }
            }
        }
    }

    /// Get the shared database URL from ReleaseSpec config
    ///
    /// This reads from the spec that was already initialized by config_manager::initialize(),
    /// which has applied env overrides and set defaults.
    ///
    /// Note: All env var handling is centralized in config_manager::initialize().
    /// This function only reads from the already-initialized spec.
    async fn get_shared_database_url(&self) -> String {
        // Read from ReleaseSpec config (already initialized by config_manager with env overrides)
        if let Some(spec) = self.release_spec.read().await.as_ref() {
            if let Some(ref runtime) = spec.runtime {
                if let Some(ref db_config) = runtime.db {
                    if !db_config.connection_string.is_empty() {
                        tracing::info!(db_url = %db_config.connection_string, "Using shared database from release config");
                        return db_config.connection_string.clone();
                    }
                }
            }
        }

        // Fallback default if spec not initialized (shouldn't happen in normal flow)
        let base_dir = plexspaces_common::config_manager::get_default_base_dir();
        let url = plexspaces_common::config_manager::default_shared_db_url(&base_dir);
        tracing::warn!(db_url = %url, "Using fallback shared database (release config not available)");
        url
    }

    /// Get the shared database config from ReleaseSpec config.
    ///
    /// This reads from the spec that was already initialized by config_manager::initialize(),
    /// which has applied env overrides and set defaults.
    async fn get_shared_database_config(&self) -> plexspaces_proto::storage::v1::SharedDbConfig {
        if let Some(spec) = self.release_spec.read().await.as_ref() {
            if let Some(ref runtime) = spec.runtime {
                if let Some(ref db_config) = runtime.db {
                    return db_config.clone();
                }
            }
        }

        plexspaces_proto::storage::v1::SharedDbConfig {
            connection_string: plexspaces_common::config_manager::default_shared_db_url(
                &plexspaces_common::config_manager::get_default_base_dir(),
            ),
            ..Default::default()
        }
    }

    /// Initialize blob service via [`plexspaces_blob::node_startup`] and store on this node.
    ///
    /// Returns `Arc<BlobService>` on success. Caller (`start`) treats failure as optional when
    /// object storage is unavailable.
    async fn init_blob_service(&self) -> Result<Arc<plexspaces_blob::BlobService>, NodeError> {
        let blob_config = {
            let release_spec = self.release_spec.read().await;
            plexspaces_blob::node_startup::blob_config_from_release_spec(release_spec.as_ref())
        };
        let db_url = self.get_shared_database_url().await;
        let locator: Arc<dyn plexspaces_actor::InitializableServiceLocator + Send + Sync> =
            self.service_locator.clone();

        // The embedded rustfs subprocess serves on blob_http_port (same port as blob REST).
        let embedded_port = resolve_blob_http_port(&self.config);

        let (service_arc, embedded_store) =
            plexspaces_blob::node_startup::create_and_register_blob_service(
                locator,
                &db_url,
                blob_config,
                embedded_port,
            )
            .await
            .map_err(|e| NodeError::ConfigError(e.to_string()))?;
        {
            let mut blob_service_guard = self.blob_service.write().await;
            *blob_service_guard = Some(service_arc.clone());
        }
        if let Some(store) = embedded_store {
            let mut store_guard = self._embedded_object_store.write().await;
            *store_guard = Some(store);
        }
        Ok(service_arc)
    }

    /// Start node services (heartbeat, discovery, etc.)
    /// Start the node with gRPC server for both ActorService and TupleSpaceService
    ///
    /// ## Purpose
    /// Starts all node services:
    /// - gRPC server for ActorService (remote actor messaging)
    /// - gRPC server for TupleSpaceService (distributed TupleSpace)
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
        use plexspaces_grpc_middleware::GrpcHttpServerBuilder;
        use plexspaces_proto::{ActorServiceServer, TupleSpaceServiceServer};
        use plexspaces_services::actor_service::ActorServiceImpl;
        use plexspaces_services::tuple_service::TupleSpaceServiceImpl;
        use tonic_web;

        // Install sqlx::any default drivers before any database operations
        sqlx::any::install_default_drivers();

        // Record start time for uptime calculation
        {
            let mut start_time = self.start_time.write().await;
            *start_time = Some(tokio::time::Instant::now());
        }

        // Initialize services if not already done (idempotent)
        self.initialize_services().await?;

        // Set node context for application manager
        self.application_manager
            .set_node_context(self.clone())
            .await;

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
                } else if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("No release config found, using defaults");
                }
            }
        }

        // Auto-start applications from Release if configured
        {
            let release_spec = self.release_spec.read().await;
            if let Some(ref spec) = *release_spec {
                // Iterate through applications (simple approach - dependency ordering would require Release helper)
                for app_config in &spec.applications {
                    // Only auto-start if enabled and auto_start is true
                    if !app_config.enabled || !app_config.auto_start {
                        continue;
                    }

                    // Check if application is registered
                    let app_state = ApplicationManagerTrait::get_state(
                        self.application_manager.as_ref(),
                        &app_config.name,
                    )
                    .await;
                    if app_state.is_none() {
                        tracing::warn!(
                            application = %app_config.name,
                            "Application in release config is not registered, skipping auto-start"
                        );
                        continue;
                    }

                    // Start application (environment variables are already in ApplicationSpec)
                    if let Err(e) = self.application_manager.start(&app_config.name).await {
                        tracing::error!(
                            application = %app_config.name,
                            error = %e,
                            "Failed to auto-start application from release config"
                        );
                        // Continue with other applications even if one fails
                    } else {
                        tracing::info!(
                            application = %app_config.name,
                            "Auto-started application from release config"
                        );
                    }
                }
            }
        }

        // Register RuntimeConfig and SecurityConfig in ServiceLocator (services crate)
        {
            let release_spec = self.release_spec.read().await;
            if let Some(ref spec) = *release_spec {
                let locator: Arc<dyn plexspaces_actor::InitializableServiceLocator + Send + Sync> =
                    self.service_locator.clone();
                plexspaces_services::release_runtime_registration::register_runtime_and_security_from_release(
                    locator,
                    spec,
                )
                .await;
            }
        }

        // Get NodeConfig for node registration
        let _proto_node_config = self
            .service_locator
            .get_node_config()
            .await
            .ok_or_else(|| {
                NodeError::ConfigError("NodeConfig not found in ServiceLocator".to_string())
            })?;

        // Register the node in NodeRegistry before starting heartbeats so it is
        // immediately visible to placement and discovery.
        let node_id_str = self.id.as_str().to_string();
        let cluster_name = self
            .service_locator
            .get_node_config()
            .await
            .and_then(|config| {
                if !config.cluster_name.is_empty() {
                    Some(config.cluster_name)
                } else {
                    None
                }
            });
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator().clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
        let registration_ctx = if let Some(cluster) = &cluster_name {
            service_locator_trait
                .request_context_for_system_operations_with_namespace(cluster.clone())
                .await
        } else {
            service_locator_trait
                .request_context_for_system_operations()
                .await
        };
        // Use grpc_address (per-node port) if available; fall back to listen_addr.
        // Normalize 0.0.0.0/127.0.0.1 → localhost for dialable gRPC endpoint.
        let effective_addr = self
            .service_locator
            .get_node_config()
            .await
            .and_then(|c| {
                if !c.grpc_address.is_empty() {
                    Some(c.grpc_address)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| self.config.listen_addr.clone());
        let grpc_address = plexspaces_common::dialable_node_address(&effective_addr);

        if let Some(node_registry) = self.service_locator.get_node_registry().await {
            let mut capabilities = self.config.metadata.clone();
            if let Some(cluster) = &cluster_name {
                capabilities.insert("cluster".to_string(), cluster.clone());
            }

            node_registry
                .register_node(
                    &registration_ctx,
                    plexspaces_proto::node::v1::NodeRegistration {
                        node_id: node_id_str.clone(),
                        node_address: grpc_address.clone(),
                        capabilities,
                        status: plexspaces_proto::node::v1::NodeStatus::NodeStatusReady as i32,
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| {
                    NodeError::ConfigError(format!(
                        "Failed to register node in NodeRegistry: {}",
                        e
                    ))
                })?;

            tracing::info!(
                node_id = %node_id_str,
                namespace = %registration_ctx.namespace(),
                cluster_name = ?cluster_name,
                grpc_address = %grpc_address,
                "Node registered in NodeRegistry"
            );
        } else {
            return Err(NodeError::ConfigError(
                "NodeRegistry not found in ServiceLocator".to_string(),
            ));
        }

        // Start heartbeat task with capacity tracking
        let node_for_heartbeat = self.clone();
        let heartbeat_interval = self.config.heartbeat_interval_ms;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(heartbeat_interval)).await;

                // Heartbeat updates this node's own node-registry and object-registry state.
                // Also scans for stale object registrations inline (no separate background task).
                if let Err(e) = node_for_heartbeat.send_heartbeat_with_capacity().await {
                    tracing::warn!(
                        node_id = %node_for_heartbeat.id.as_str(),
                        error = %e,
                        "Failed to send heartbeat"
                    );
                }
            }
        });

        // Sparse actor liveness check using __PING__ messages.
        // Runs at 2× the node heartbeat interval (e.g. 10 s when heartbeat is 5 s).
        // Only probes local actors; unresponsive ones are recorded in the object registry.
        // This is kept intentionally sparse to avoid flooding the actor mailboxes.
        let node_for_ping = self.clone();
        let actor_ping_interval = heartbeat_interval * 2;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(actor_ping_interval)).await;
                node_for_ping.check_actor_liveness().await;
            }
        });


        // Parse listen address
        let addr = self
            .config
            .listen_addr
            .parse()
            .map_err(|e| NodeError::ConfigError(format!("Invalid listen address: {}", e)))?;

        // Register Node in ServiceLocator so ActorServiceImpl can access it
        self.service_locator.register_service(self.clone()).await;

        // Initialize blob service - optional; node starts without blob storage if init fails (e.g. backend unreachable)
        let blob_service = match self.init_blob_service().await {
            Ok(service) => Some(service),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Blob service unavailable (e.g. S3-compatible backend not running or bucket missing). Node will start without blob storage."
                );
                None
            }
        };

        // Read blob backend to decide whether to start the Axum REST wrapper.
        // When backend = "embedded", rustfs already owns blob_http_port — no separate Axum server.
        let blob_backend = {
            let release_spec = self.release_spec.read().await;
            plexspaces_blob::node_startup::blob_config_from_release_spec(release_spec.as_ref())
                .backend
        };

        // Start the Axum blob REST server only for non-embedded backends (s3, gcp, azure).
        // For "embedded", rustfs itself handles all HTTP on blob_http_port.
        let _blob_http_handle: Option<tokio::task::JoinHandle<()>> =
            if let Some(blob_svc) = blob_service.as_ref().filter(|_| blob_backend != "embedded") {
                use plexspaces_blob::server::http_axum::create_blob_router;
                let router = create_blob_router(blob_svc.clone());

                let grpc_addr: std::net::SocketAddr = self
                    .config
                    .listen_addr
                    .parse()
                    .unwrap_or_else(|_| "127.0.0.1:9999".parse().unwrap());
                let http_port = resolve_blob_http_port(&self.config);
                let http_addr = format!("{}:{}", grpc_addr.ip(), http_port)
                    .parse::<std::net::SocketAddr>()
                    .unwrap_or_else(|_| "127.0.0.1:10000".parse().unwrap());

                tracing::info!(addr = %http_addr, "Starting blob HTTP server");

                match tokio::net::TcpListener::bind(http_addr).await {
                    Ok(listener) => Some(tokio::spawn(async move {
                        use axum::serve;
                        if let Err(e) = serve(listener, router).await {
                            tracing::error!(error = %e, "Blob HTTP server error");
                        }
                    })),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Could not bind blob HTTP server; blob HTTP endpoints will be unavailable"
                        );
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
        // Note: ActorService is already registered during initialize_services(), but we ensure it's available here
        // for gRPC server (idempotent - won't register twice if already registered)
        if self.service_locator.get_actor_service().await.is_none() {
            let actor_service_for_context =
                Arc::new(plexspaces_services::actor_service::ActorServiceImpl::new(
                    self.service_locator.clone(),
                    self.id.as_str().to_string(),
                ));
            self.service_locator
                .register_actor_service(actor_service_for_context.clone()
                    as Arc<dyn plexspaces_actor::ActorService + Send + Sync>)
                .await;
        }

        // Register NodeConnectionInfo for services that need connection information
        use crate::service_wrappers::NodeConnectionInfoWrapper;
        let connection_info = Arc::new(NodeConnectionInfoWrapper::new(self.clone()));
        self.service_locator
            .register_service(connection_info.clone())
            .await;
        let connection_info_trait: Arc<dyn plexspaces_actor::NodeConnectionInfo + Send + Sync> =
            connection_info.clone() as Arc<dyn plexspaces_actor::NodeConnectionInfo + Send + Sync>;
        self.service_locator
            .register_node_connection_info(connection_info_trait)
            .await;

        let tuplespace_service = TupleSpaceServiceImpl::new(self.service_locator.clone());

        // Start background cleanup task for expired temporary senders (in ActorRegistry)
        let actor_registry = self.actor_registry().await?;

        // Wire ServiceLocator into ActorRegistry for remote DOWN/EXIT delivery.
        actor_registry
            .set_service_locator(
                self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>
            )
            .await;

        actor_registry
            .set_local_listen_addr(plexspaces_common::dialable_node_address(&self.config.listen_addr))
            .await;

        ActorRegistry::start_temporary_sender_cleanup(actor_registry.clone());

        // Start stale monitor GC background task (default 60s interval).
        plexspaces_actor::start_monitor_gc_task(
            actor_registry.actor_monitor().clone(),
            self.id.as_str().to_string(),
            self.service_locator.clone(),
            60,
        );

        // Create scheduling components (Phase 4 & 5)
        use plexspaces_channel::InMemoryChannel;
        use plexspaces_proto::channel::v1::{
            ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee,
        };
        use plexspaces_scheduler::{
            background::BackgroundScheduler, capacity_tracker::CapacityTracker,
            state_store::SchedulingStateStore, SchedulingServiceImpl, TaskRouter,
        };

        let shared_db = self.get_shared_database_config().await;

        // Create state store using the shared database config from RuntimeConfig.db.
        use plexspaces_scheduler::state_store::create_state_store_from_shared_db;
        let state_store: Arc<dyn SchedulingStateStore> = match create_state_store_from_shared_db(
            &shared_db,
        )
        .await
        {
            Ok(store) => store,
            Err(e) => {
                // FATAL: Cannot create state store - fail startup
                let error_msg = format!(
                    "FATAL: Failed to create scheduler state store with shared database '{}': {}. Cannot proceed without database access.",
                    shared_db.connection_string, e
                );
                tracing::error!(error = %e, connection_string = %shared_db.connection_string, "{}", error_msg);
                return Err(NodeError::ConfigError(error_msg));
            }
        };

        // Create capacity tracker
        // CapacityTracker needs ObjectRegistry, get it from ServiceLocator
        let object_registry = self
            .service_locator
            .get_object_registry()
            .await
            .ok_or_else(|| {
                NodeError::ConfigError("ObjectRegistry not found in ServiceLocator".to_string())
            })?;
        // CapacityTracker uses trait ObjectRegistry
        let capacity_tracker = Arc::new(CapacityTracker::new(object_registry));

        // Create scheduling:requests channel
        let request_channel_config = ChannelConfig {
            name: "scheduling:requests".to_string(),
            provider: ChannelProvider::ChannelProviderInMemory as i32,
            capacity: 1000,
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };
        let request_channel = Arc::new(
            InMemoryChannel::new(request_channel_config.clone())
                .await
                .map_err(|e| {
                    NodeError::ConfigError(format!("Failed to create scheduling channel: {}", e))
                })?,
        );

        // Create scheduling service
        // Tenant comes from auth (JWT/mTLS); namespace from request context.
        let scheduling_service = SchedulingServiceImpl::new(
            state_store.clone(),
            request_channel.clone(),
            capacity_tracker.clone(),
        );

        // Create lock manager for background scheduler
        // Use in-memory if URL contains ":memory:", otherwise SQLite file-based
        // Get LockManager from ServiceLocator (created during initialize_services)
        // BackgroundScheduler uses plexspaces_locks::LockManager directly
        let lock_manager = self.service_locator.get_lock_manager().await
            .ok_or_else(|| NodeError::ConfigError("LockManager not found in ServiceLocator. Ensure initialize_services() has been called.".to_string()))?;

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
                tracing::warn!("Background scheduler error: {}", e);
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
                let channel = channel_service
                    .get_or_create_channel(&group_name)
                    .await
                    .map_err(|e| format!("Failed to get/create channel {}: {}", group_name, e))?;
                Ok(channel)
            }
        }));

        // Store task router in Node
        {
            let mut router = self.task_router.write().await;
            *router = Some(task_router.clone());
        }

        // Register TaskRouter in ServiceLocator for ShardGroup integration
        self.service_locator()
            .register_task_router(task_router.clone())
            .await;
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!("✅ TaskRouter registered in ServiceLocator");
        }

        // Logged below after health service registration

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
                        tracing::warn!("Node {}: Received SIGTERM", node_for_shutdown.id.as_str());
                    }
                    _ = sigint.recv() => {
                        tracing::warn!("Node {}: Received SIGINT (Ctrl+C)", node_for_shutdown.id.as_str());
                    }
                    _ = shutdown_rx => {
                        tracing::warn!("Node {}: Received programmatic shutdown", node_for_shutdown.id.as_str());
                    }
                }
            }

            // Windows: Only support Ctrl+C
            #[cfg(not(unix))]
            {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        tracing::warn!("Node {}: Received Ctrl+C", node_for_shutdown.id.as_str());
                    }
                    _ = shutdown_rx => {
                        tracing::warn!("Node {}: Received programmatic shutdown", node_for_shutdown.id.as_str());
                    }
                }
            }

            // Perform graceful shutdown
            tracing::warn!(
                "Node {}: Initiating graceful shutdown...",
                node_for_shutdown.id.as_str()
            );
            if let Err(e) = node_for_shutdown
                .shutdown(tokio::time::Duration::from_secs(30))
                .await
            {
                tracing::warn!(
                    "Node {}: Shutdown error: {}",
                    node_for_shutdown.id.as_str(),
                    e
                );
            }
        };

        // Create health reporter and services
        use plexspaces_actor::PlexSpacesHealthReporter;
        use plexspaces_proto::system::v1::system_service_server::SystemServiceServer;
        use plexspaces_services::system_service::SystemServiceImpl;

        // Create and register HealthService (source of truth for shutdown)
        // This helper ensures consistent creation and registration
        use crate::health::helpers::create_and_register_health_service;
        let (plexspaces_health_reporter, _) = create_and_register_health_service(
            self.service_locator.clone(),
            None, // Use default HealthProbeConfig
        )
        .await;

        // Store health reporter in Node for shutdown access
        {
            let mut health_reporter_guard = self.health_reporter.write().await;
            *health_reporter_guard = Some(plexspaces_health_reporter.clone());
        }

        // Create ActorServiceImpl
        // This must be created after HealthService is initialized
        let actor_service = Arc::new(ActorServiceImpl::new(
            self.service_locator.clone(),
            self.id.as_str().to_string(),
        ));

        // Create ProcessGroupService for distributed pub/sub
        use plexspaces_proto::ProcessGroupServiceServer;
        use plexspaces_services::process_group_service::{
            ProcessGroupServiceGrpc, ProcessGroupServiceImpl,
        };
        let process_group_impl = Arc::new(ProcessGroupServiceImpl::new(
            self.service_locator.clone(),
            self.id.as_str().to_string(),
        ));

        // Register ProcessGroupService in ServiceLocator so it can be accessed by other components
        let process_group_service_trait: Arc<dyn plexspaces_actor::ProcessGroupService> =
            process_group_impl.clone();
        self.service_locator
            .register_process_group_service(process_group_service_trait)
            .await;

        let process_group_service =
            ProcessGroupServiceGrpc::new(process_group_impl, self.service_locator.clone());

        // Create standard gRPC health service (for Kubernetes probes)
        // Use our custom implementation that integrates with PlexSpacesHealthReporter
        use crate::standard_health_service::StandardHealthServiceImpl;
        use tonic_health::pb::health_server::HealthServer;
        let standard_health_service_impl =
            StandardHealthServiceImpl::new(plexspaces_health_reporter.clone());
        let standard_health_service = HealthServer::new(standard_health_service_impl);

        // Register built-in dependencies (Redis, PostgreSQL, Kafka) if enabled
        // These are automatically detected from environment variables
        use crate::dependency_registration::register_builtin_dependencies;
        let deps_registered = register_builtin_dependencies(plexspaces_health_reporter.clone())
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Warning: Failed to register built-in dependencies: {}", e);
                0
            });
        tracing::warn!(
            "Node {}: Starting gRPC server on {} (health_checkers={})",
            self.id.as_str(),
            addr,
            deps_registered
        );

        // Register dependencies from object-registry if configured
        // This allows registering dependencies by name/type from the registry

        // Note: Health config (dependency registration) is configured via HealthProbeConfig
        // which is part of the health reporter, not NodeConfig. Dependencies are registered
        // via environment variables or object-registry discovery as configured in HealthProbeConfig.

        // Create SystemService (provides HTTP endpoints via gRPC-Gateway)
        let system_service = SystemServiceImpl::new(plexspaces_health_reporter.clone());

        // Create MetricsService for Prometheus export (install global `metrics` recorder once)
        use plexspaces_proto::metrics::v1::metrics_service_server::MetricsServiceServer;
        use plexspaces_services::metrics_service::{
            install_metrics_recorder, MetricsServiceImpl, PrometheusHandleRenderer,
        };
        let prometheus_handle = install_metrics_recorder();
        self.service_locator
            .register_metrics_prometheus_renderer(Arc::new(PrometheusHandleRenderer::new(
                prometheus_handle.clone(),
            )))
            .await;
        let metrics_service = MetricsServiceImpl::new(prometheus_handle);
        self.service_locator
            .register_metrics_service_access(Arc::new(metrics_service.clone()))
            .await;

        // Start connection health monitoring and stale connection cleanup
        // Connection health monitoring is handled by gRPC client pool

        // Mark startup complete after services are registered
        plexspaces_health_reporter.mark_startup_complete(None).await;

        // Create WASM runtime service for dynamic actor deployment (if not already registered by initialize_services)
        use plexspaces_proto::wasm::v1::wasm_runtime_service_server::WasmRuntimeServiceServer;
        use plexspaces_wasm_runtime::{grpc_service::WasmRuntimeServiceImpl, WasmRuntime};
        let wasm_runtime_trait_for_service: Arc<dyn plexspaces_actor::WasmRuntimeTrait> =
            if let Some(rt) = self.service_locator.get_wasm_runtime().await {
                // Already registered in initialize_services() (e.g. for tests that use build() only)
                rt
            } else {
                let rt = Arc::new(WasmRuntime::new().await.map_err(|e| {
                    NodeError::ConfigError(format!("Failed to create WASM runtime: {}", e))
                })?);
                let rt_trait: Arc<dyn plexspaces_actor::WasmRuntimeTrait> = rt.clone();
                self.service_locator
                    .register_wasm_runtime(rt_trait.clone())
                    .await;
                let mut stored_runtime = self.wasm_runtime.write().await;
                *stored_runtime = Some(rt);
                rt_trait
            };
        let wasm_runtime_service = WasmRuntimeServiceImpl::new(wasm_runtime_trait_for_service);

        // Create NodeService first so we can inject it into ApplicationService for seed_nodes on deploy
        use plexspaces_services::node_service::NodeServiceImpl;
        let release_spec_for_node_svc = {
            let mut effective = self.release_spec.read().await.clone().unwrap_or_default();
            if let Some(node_config) = self.service_locator.get_node_config().await {
                effective.node = Some(node_config);
            }
            Some(effective)
        };
        let node_service: Arc<NodeServiceImpl> = if let Some(ref spec) = release_spec_for_node_svc {
            Arc::new(NodeServiceImpl::with_release_spec(
                self.service_locator.clone(),
                self.id.as_str().to_string(),
                spec.clone(),
            ))
        } else {
            Arc::new(NodeServiceImpl::new(
                self.service_locator.clone(),
                self.id.as_str().to_string(),
            ))
        };
        node_service
            .register_node_connectivity(
                node_service.clone() as Arc<dyn plexspaces_actor::NodeConnectivity>
            )
            .await;

        // Create ApplicationService with NodeConnectivity for ApplicationSpec.seed_nodes on deploy
        use plexspaces_proto::application::v1::application_service_server::ApplicationServiceServer;
        use plexspaces_services::application_service::ApplicationServiceImpl;
        let application_service = Arc::new(ApplicationServiceImpl::new(
            self.service_locator(),
            Some(node_service.clone() as Arc<dyn plexspaces_actor::NodeConnectivity>),
        ));

        // Capture configured auto-deploy directory now, but deploy after the server starts
        // accepting requests so large WASM compilation does not block node readiness.
        let wasm_apps_directory: Option<String> = self
            .release_spec
            .read()
            .await
            .as_ref()
            .and_then(|spec| spec.runtime.as_ref())
            .map(|runtime| runtime.wasm_apps_directory.clone())
            .filter(|s| !s.is_empty());

        // Create Firecracker VM Service (if Firecracker support is enabled)
        #[cfg(feature = "firecracker")]
        let _firecracker_service = {
            #[cfg(feature = "firecracker")]
            {
                use plexspaces_proto::firecracker::v1::firecracker_vm_service_server::FirecrackerVmServiceServer;
                use plexspaces_services::firecracker_service::FirecrackerVmServiceImpl;
                Some(FirecrackerVmServiceServer::new(
                    FirecrackerVmServiceImpl::new(),
                ))
            }
        };
        #[cfg(not(feature = "firecracker"))]
        let firecracker_service: Option<()> = None;

        // Run server with graceful shutdown and gRPC-Gateway support
        use plexspaces_proto::scheduling::v1::scheduling_service_server::SchedulingServiceServer;

        // Create DashboardService with health reporter access (if dashboard feature enabled)
        // Create both gRPC and HTTP instances (they share ServiceLocator so have same data)
        #[cfg(feature = "dashboard")]
        let (dashboard_service_opt, dashboard_service_for_http_opt): (
            Option<plexspaces_dashboard::DashboardServiceImpl>,
            Option<Arc<plexspaces_dashboard::DashboardServiceImpl>>,
        ) = {
            use plexspaces_dashboard::{DashboardServiceImpl, HealthReporterAccess};

            // Create health reporter access wrapper to avoid circular dependency
            struct HealthReporterAccessImpl {
                health_reporter: Arc<PlexSpacesHealthReporter>,
            }

            #[async_trait::async_trait]
            impl HealthReporterAccess for HealthReporterAccessImpl {
                async fn get_detailed_health(
                    &self,
                    include_non_critical: bool,
                ) -> plexspaces_proto::system::v1::DetailedHealthCheck {
                    self.health_reporter
                        .get_detailed_health(include_non_critical)
                        .await
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
        let (dashboard_service_opt, dashboard_service_for_http_opt): (
            Option<()>,
            Option<Arc<plexspaces_services::dashboard_service::DashboardServiceImpl>>,
        ) = (None, None);

        // Build gRPC server with all services
        // Set max message size to 5MB for gRPC methods (larger than default 4MB for flexibility)
        // Note: For large WASM file uploads (>5MB), use HTTP multipart endpoint instead
        const GRPC_MAX_MESSAGE_SIZE: usize = 5 * 1024 * 1024; // 5MB

        let actor_service_for_http = actor_service.clone();
        let server_builder = GrpcHttpServerBuilder::new(addr)
            .grpc_service(tonic_web::enable(
                ActorServiceServer::new(
                    plexspaces_services::actor_service::ActorServiceWrapper::from(actor_service),
                )
                .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
            ))
            .grpc_service(tonic_web::enable(
                TupleSpaceServiceServer::new(tuplespace_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
            ))
            .grpc_service(tonic_web::enable(
                SchedulingServiceServer::new(scheduling_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
            ))
            .grpc_service(tonic_web::enable(
                WasmRuntimeServiceServer::new(wasm_runtime_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
            ))
            .grpc_service(tonic_web::enable(
                ApplicationServiceServer::new(application_service.as_ref().clone())
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
            ))
            .grpc_service(tonic_web::enable(standard_health_service))
            .grpc_service(tonic_web::enable(
                SystemServiceServer::new(system_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
            ))
            .grpc_service(tonic_web::enable(
                MetricsServiceServer::new(metrics_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
            ))
            .grpc_service(tonic_web::enable(
                ProcessGroupServiceServer::new(process_group_service)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
            ))
            .grpc_service(tonic_web::enable({
                use crate::node_service_handler::NodeServiceHandler;
                use plexspaces_proto::node::v1::node_service_server::NodeServiceServer;
                NodeServiceServer::new(NodeServiceHandler(node_service.clone()))
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE)
            }));

        // Add dashboard service if feature enabled
        #[cfg(feature = "dashboard")]
        let server_builder = {
            use plexspaces_proto::dashboard::v1::dashboard_service_server::DashboardServiceServer;
            if let Some(dashboard_svc) = dashboard_service_opt {
                server_builder.grpc_service(tonic_web::enable(
                    DashboardServiceServer::new(dashboard_svc)
                        .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                        .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
                ))
            } else {
                server_builder
            }
        };

        // Add blob gRPC service only when blob service is available
        let server_builder = if let Some(ref blob_svc) = blob_service {
            use plexspaces_blob::server::grpc::BlobServiceImpl;
            use plexspaces_proto::storage::v1::blob_service_server::BlobServiceServer;
            server_builder.grpc_service(tonic_web::enable(
                BlobServiceServer::new(BlobServiceImpl::new(blob_svc.clone()))
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
            ))
        } else {
            server_builder
        };

        // Add ServiceLinkService for runtime management of outbound service links
        let server_builder = {
            use plexspaces_proto::node::v1::service_link_service_server::ServiceLinkServiceServer;
            use plexspaces_services::service_link_service::ServiceLinkServiceImpl;
            let sls = ServiceLinkServiceImpl::new(
                self.service_locator.clone()
                    as Arc<dyn plexspaces_actor::InitializableServiceLocator>,
            )
            .await;
            // Register in ServiceLocator so dashboard can query live service links
            self.service_locator
                .register_service_link_service(Arc::new(sls.clone()))
                .await;
            server_builder.grpc_service(tonic_web::enable(
                ServiceLinkServiceServer::new(sls)
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
            ))
        };

        // Add ObjectRegistry gRPC service for network-accessible service discovery
        let server_builder = {
            use plexspaces_proto::object_registry::v1::object_registry_server::ObjectRegistryServer;
            use plexspaces_services::object_registry_service::ObjectRegistryServiceImpl;
            use plexspaces_services::ServiceLocatorTrait;
            if let Some(obj_reg) = self.service_locator.get_object_registry().await {
                server_builder.grpc_service(tonic_web::enable(
                    ObjectRegistryServer::new(ObjectRegistryServiceImpl::new(
                        obj_reg,
                        self.service_locator.clone() as Arc<dyn ServiceLocatorTrait>,
                    ))
                    .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                    .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
                ))
            } else {
                server_builder
            }
        };

        // Add UserService for OAuth login flow, tenant management, and API token management.
        // Also builds the AuthRouteState used by the HTTP auth routes module.
        let (server_builder, auth_route_state) = {
            use plexspaces_proto::security::v1::user_service_server::UserServiceServer;
            use plexspaces_services::user_service::{
                SqlApiTokenRepository, SqlTenantRepository, SqlUserRepository,
                UserServiceImpl,
                oidc::build_oidc_state,
            };
            use crate::http_routes::AuthRouteState;

            let shared_db = self.get_shared_database_config().await;
            let user_pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(5)
                .connect(&shared_db.connection_string)
                .await;

            match user_pool {
                Ok(pool) => {
                    let user_repo = Arc::new(SqlUserRepository::new(pool.clone()))
                        as Arc<dyn plexspaces_services::user_service::UserRepository>;
                    let tenant_repo = Arc::new(SqlTenantRepository::new(pool.clone()))
                        as Arc<dyn plexspaces_services::user_service::TenantRepository>;
                    let token_repo = Arc::new(SqlApiTokenRepository::new(pool))
                        as Arc<dyn plexspaces_services::user_service::ApiTokenRepository>;

                    let user_service = UserServiceImpl::new(
                        user_repo.clone(),
                        tenant_repo.clone(),
                        token_repo.clone(),
                        self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>,
                    );
                    let builder = server_builder.grpc_service(tonic_web::enable(
                        UserServiceServer::new(user_service)
                            .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
                            .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE),
                    ));

                    // Resolve JWT key pair once — used for both OIDC signing and HTTP auth validation.
                    let jwt_cfg = self.service_locator.get_security_config().await
                        .and_then(|sc| sc.jwt);
                    let auth_jwt_key_pair: Option<Arc<plexspaces_grpc_middleware::JwtKeyPair>> = match jwt_cfg {
                        Some(ref cfg) => {
                            match plexspaces_grpc_middleware::JwtKeyPair::from_config(
                                &cfg.private_key_pem,
                                &cfg.private_key_file,
                                &cfg.secret,
                                cfg.auto_generate_key,
                            ) {
                                Ok(kp) => {
                                    Some(Arc::new(kp))
                                }
                                Err(e) => {
                                    tracing::error!(error = %e, "Failed to load JWT key pair from config");
                                    None
                                }
                            }
                        }
                        None => {
                            match plexspaces_grpc_middleware::JwtKeyPair::from_env(None) {
                                Ok(kp) => {
                                    Some(Arc::new(kp))
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "No JWT key pair available");
                                    None
                                }
                            }
                        }
                    };

                    // Build OidcState — uses the same key pair for signing session JWTs.
                    let oidc_cfg = self.service_locator.get_security_config().await
                        .and_then(|sc| sc.oidc);
                    let oidc_state = if let (Some(cfg), Some(kp)) = (oidc_cfg, auth_jwt_key_pair.clone()) {
                        match build_oidc_state(&cfg, user_repo.clone(), tenant_repo.clone(), kp).await {
                            Ok(state) => {
                                Some(state)
                            }
                            Err(e) => {
                                tracing::info!(reason = %e, "OIDC not mounted");
                                None
                            }
                        }
                    } else {
                        None
                    };

                    tracing::info!(
                        jwt = auth_jwt_key_pair.as_ref().map(|kp| format!("{:?}:{}", kp.algorithm(), kp.kid())).unwrap_or_else(|| "none".into()),
                        oidc = oidc_state.is_some(),
                        "Auth configured"
                    );

                    let auth_state = AuthRouteState {
                        user_repo,
                        tenant_repo,
                        token_repo,
                        service_locator: self.service_locator.clone()
                            as Arc<dyn plexspaces_actor::ServiceLocator>,
                        oidc: oidc_state,
                        jwt_key_pair: auth_jwt_key_pair,
                    };

                    (builder, Some(auth_state))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "UserService unavailable: failed to connect to database");
                    (server_builder, None)
                }
            }
        };

        // Wire tenant_repo into dashboard service for accurate tenant count.
        #[cfg(feature = "dashboard")]
        if let Some(ref auth_state) = auth_route_state {
            if let Some(ref http_dash) = dashboard_service_for_http_opt {
                http_dash.set_tenant_repo(auth_state.tenant_repo.clone()).await;
            }
        }

        // Connect to cluster_seed_nodes if configured (non-blocking; node is already listening)
        {
            let node_connectivity =
                node_service.clone() as Arc<dyn plexspaces_actor::NodeConnectivity>;
            let service_locator = self.service_locator.clone();
            tokio::spawn(async move {
                if let Some(cfg) = service_locator.get_node_config().await {
                    if !cfg.cluster_seed_nodes.is_empty() {
                        let addrs = cfg.cluster_seed_nodes.clone();
                        match node_connectivity.connect_to_node_addresses(addrs).await {
                            Ok(r) => tracing::info!(
                                connected = r.connected.len(),
                                failed = r.failed.len(),
                                "Connected to cluster_seed_nodes"
                            ),
                            Err(e) => {
                                tracing::warn!(error = %e, "Failed to connect to cluster_seed_nodes")
                            }
                        }
                    }
                }
            });
        }

        // Build HTTP routes and assemble single-port gRPC+HTTP server
        let (auth_disabled, jwt_key_pair) =
            plexspaces_grpc_middleware::http_jwt_auth_snapshot(self.service_locator.clone()
                as Arc<dyn plexspaces_actor::ServiceLocator + Send + Sync>)
            .await;
        let token_repo_for_gateway: Option<Arc<dyn plexspaces_services::user_service::ApiTokenRepository>> =
            auth_route_state.as_ref().map(|s| s.token_repo.clone());
        let node_connectivity_for_http =
            node_service.clone() as Arc<dyn plexspaces_actor::NodeConnectivity>;

        // Construct WS registry and pending asks for WebSocket thin-client support.
        let ws_registry = Arc::new(crate::ws_registry::WsRegistry::new());
        let pending_asks = std::sync::Arc::new(crate::ws_transport_client::PendingAsks::new());
        let ws_node_registry = self.service_locator.get_node_registry().await;
        let ws_state = crate::http_routes::WsRouteState {
            actor_service: actor_service_for_http.clone(),
            ws_registry: ws_registry.clone(),
            pending_asks: pending_asks.clone(),
            service_locator: self.service_locator.clone(),
            node_registry: ws_node_registry,
            auth_disabled,
            jwt_key_pair: jwt_key_pair.clone(),
        };

        // Register WS transport clients in ServiceLocator for outbound WS routing.
        {
            use plexspaces_actor::InitializableServiceLocator;
            use plexspaces_actor::{GrpcActorTransportClient, GrpcNodeTransportClient};
            use crate::ws_transport_client::{WsActorTransportClient, WsNodeTransportClient};

            let grpc_actor = std::sync::Arc::new(GrpcActorTransportClient::new(self.service_locator.clone()));
            let grpc_node = std::sync::Arc::new(GrpcNodeTransportClient::new(self.service_locator.clone()));

            let ws_actor = std::sync::Arc::new(WsActorTransportClient::new(
                ws_registry.clone(),
                pending_asks.clone(),
                grpc_actor as std::sync::Arc<dyn plexspaces_service_traits::ActorTransportClient>,
            ));
            let ws_node = std::sync::Arc::new(WsNodeTransportClient::new(
                ws_registry.clone(),
                pending_asks.clone(),
                grpc_node as std::sync::Arc<dyn plexspaces_service_traits::NodeTransportClient>,
            ));

            self.service_locator.register_actor_transport_client(
                ws_actor as std::sync::Arc<dyn plexspaces_service_traits::ActorTransportClient>,
            ).await;
            self.service_locator.register_node_transport_client(
                ws_node as std::sync::Arc<dyn plexspaces_service_traits::NodeTransportClient>,
            ).await;
            let ws_reg_trait: std::sync::Arc<dyn plexspaces_actor::WsRegistryTrait> = ws_registry.clone();
            self.service_locator.register_ws_registry(ws_reg_trait).await;
        }

        let static_registry: crate::http_routes::StaticRegistry =
            Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));

        let http_routes = crate::http_routes::all_http_routes(
            actor_service_for_http.clone(),
            self.service_locator.clone(),
            node_connectivity_for_http,
            auth_disabled,
            jwt_key_pair.clone(),
            auth_route_state,
            ws_state,
            static_registry,
        );

        #[cfg(feature = "dashboard")]
        let http_routes = {
            use plexspaces_dashboard::create_dashboard_router;
            let gateway_state: crate::http_gateway::HttpGatewayState = (
                actor_service_for_http,
                auth_disabled,
                jwt_key_pair,
                self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>,
                dashboard_service_for_http_opt,
                token_repo_for_gateway,
            );
            let merged = http_routes.merge(create_dashboard_router().with_state(gateway_state.clone()));
            merged.layer(axum::middleware::from_fn_with_state(
                gateway_state,
                crate::http_gateway::http_auth_middleware,
            ))
        };

        #[cfg(not(feature = "dashboard"))]
        let http_routes = {
            let gateway_state: crate::http_gateway::HttpGatewayState = (
                actor_service_for_http,
                auth_disabled,
                jwt_key_pair,
                self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>,
                None,
                token_repo_for_gateway,
            );
            http_routes.layer(axum::middleware::from_fn_with_state(
                gateway_state,
                crate::http_gateway::http_auth_middleware,
            ))
        };

        // Server-side mTLS: In single-port mode (shared HTTP+gRPC), we do NOT wrap
        // the listener in TLS because browsers need plain HTTP access to the dashboard
        // and OIDC endpoints. mTLS is used for OUTBOUND connections to peer nodes
        // (see grpc_client.rs::connect_with_tls). If you need inbound mTLS, run a
        // dedicated gRPC-only port behind a TLS terminator or reverse proxy.
        let mtls_server_config: Option<std::sync::Arc<rustls::ServerConfig>> = None;
        let mtls_outbound = std::env::var("PLEXSPACES_MTLS_CA_CERT").is_ok();

        let (listener, app) = server_builder
            .http_routes(http_routes)
            .build()
            .await
            .map_err(|e| NodeError::NetworkError(e.to_string()))?;

        tracing::info!(
            addr = %listener.local_addr().unwrap_or(addr),
            mtls_inbound = mtls_server_config.is_some(),
            mtls_outbound = mtls_outbound,
            "Single-port gRPC+HTTP server ready"
        );

        if let Some(wasm_apps_dir_str) = wasm_apps_directory {
            let service_locator_for_deploy = self.service_locator.clone();
            let node_connectivity_for_deploy =
                node_service.clone() as Arc<dyn plexspaces_actor::NodeConnectivity>;
            tokio::spawn(async move {
                let wasm_apps_dir = std::path::PathBuf::from(&wasm_apps_dir_str);
                tracing::info!(
                    wasm_apps_directory = %wasm_apps_dir_str,
                    "Starting background auto-deploy of WASM applications"
                );
                match crate::wasm_apps_loader::deploy_all_from_directory(
                    &wasm_apps_dir,
                    service_locator_for_deploy,
                    Some(node_connectivity_for_deploy),
                )
                .await
                {
                    Ok(deployed) => {
                        if !deployed.is_empty() {
                            tracing::info!(
                                count = deployed.len(),
                                apps = ?deployed,
                                "Successfully auto-deployed WASM applications"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to auto-deploy some WASM applications (node will continue)"
                        );
                    }
                }
            });
        }

        // Server-side mTLS: when cert files are present, wrap the TCP listener with
        // tokio-rustls so every inbound connection is TLS-authenticated.
        // Clients (other nodes) must present a cert signed by the configured CA.
        if let Some(tls_cfg) = mtls_server_config {
            use tokio_rustls::TlsAcceptor;
            let acceptor = TlsAcceptor::from(tls_cfg);
            let service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
            tokio::select! {
                result = crate::tls_server::serve_tls(listener, acceptor, service) => {
                    result.map_err(|e| NodeError::GrpcError(e.to_string()))?;
                }
                _ = shutdown_signal => {}
            }
        } else {
            tokio::select! {
                result = axum::serve(listener, app) => {
                    result.map_err(|e| NodeError::GrpcError(e.to_string()))?;
                }
                _ = shutdown_signal => {}
            }
        }

        Ok(())
    }

    /// Monitor an actor (Erlang-style location-transparent monitoring).
    ///
    /// Establishes a one-way watch: when `actor_id` terminates for any reason
    /// a `__DOWN__` control message is delivered to `supervisor_id`'s mailbox.
    /// Works identically for local and remote actors.
    ///
    /// Returns a ULID `monitor_ref` that can be passed to [`Self::demonitor`].
    pub async fn monitor(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        actor_id: &ActorId,
        supervisor_id: &ActorId,
    ) -> Result<MonitorRef, NodeError> {
        if !actor_id.is_on_node(self.id.as_str()) {
            let remote_node = NodeId::new(actor_id.node_id().to_string());
            self.lookup_node_address(&remote_node).await?;
        }
        let registry = self.actor_registry().await?;
        let result = registry
            .monitor(ctx, actor_id, supervisor_id)
            .await
            .map_err(Self::map_actor_registry_error);
        match &result {
            Ok(monitor_ref) => {
                metrics::counter!("plexspaces_node_monitor_established_total",
                    "node_id" => self.id.as_str().to_string(),
                    "local" => (actor_id.node_id() == self.id.as_str()).to_string()
                )
                .increment(1);
                tracing::debug!(
                    actor_id = %actor_id,
                    supervisor_id = %supervisor_id,
                    monitor_ref = %monitor_ref,
                    node_id = %self.id.as_str(),
                    "Monitor established"
                );
            }
            Err(e) => {
                tracing::warn!(
                    actor_id = %actor_id,
                    supervisor_id = %supervisor_id,
                    error = %e,
                    "Monitor failed"
                );
            }
        }
        result
    }

    /// Cancel a previously established monitor (idempotent).
    ///
    /// Removes the monitor identified by `monitor_ref` so `supervisor_id` no
    /// longer receives `__DOWN__` when `actor_id` terminates.  Safe to call
    /// multiple times — missing refs are silently ignored.
    pub async fn demonitor(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        supervisor_id: &ActorId,
        monitor_ref: &str,
    ) -> Result<(), NodeError> {
        let registry = self.actor_registry().await?;
        let result = registry
            .demonitor(ctx, actor_id, supervisor_id, monitor_ref)
            .await
            .map_err(Self::map_actor_registry_error);
        if let Err(ref e) = result {
            tracing::warn!(
                actor_id = %actor_id,
                supervisor_id = %supervisor_id,
                monitor_ref = %monitor_ref,
                error = %e,
                "Demonitor failed"
            );
        } else {
            metrics::counter!("plexspaces_node_monitor_cancelled_total",
                "node_id" => self.id.as_str().to_string()
            )
            .increment(1);
            tracing::debug!(
                actor_id = %actor_id,
                supervisor_id = %supervisor_id,
                monitor_ref = %monitor_ref,
                "Monitor cancelled"
            );
        }
        result
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
    /// * `ctx` - RequestContext for tenant/namespace isolation
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
    /// // Link two actors (with RequestContext for tenant isolation)
    /// let ctx = RequestContext::new_without_auth("tenant-1".to_string(), "namespace-1".to_string());
    /// node.link(&ctx, actor1.id(), actor2.id()).await?;
    ///
    /// // If actor-1 dies abnormally, actor-2 automatically dies
    /// // If actor-2 dies abnormally, actor-1 automatically dies
    /// ```
    pub async fn link(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        linked_actor_id: &ActorId,
    ) -> Result<(), NodeError> {
        if actor_id == linked_actor_id {
            return Err(NodeError::InvalidArgument(
                "Cannot link actor to itself".to_string(),
            ));
        }
        let registry = self.actor_registry().await?;
        registry
            .link(ctx, actor_id, linked_actor_id)
            .await
            .map_err(Self::map_actor_registry_error)
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
    /// * `ctx` - RequestContext for tenant/namespace isolation on remote RPCs
    ///
    /// ## Returns
    /// Success or error
    ///
    /// ## Errors
    /// - `NodeError::ActorNotFound` if either actor doesn't exist
    /// - `NodeError::NetworkError` if remote actor unlink fails
    pub async fn unlink(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        linked_actor_id: &ActorId,
    ) -> Result<(), NodeError> {
        let registry = self.actor_registry().await?;
        registry
            .unlink(ctx, actor_id, linked_actor_id)
            .await
            .map_err(Self::map_actor_registry_error)
    }

    /// Publish a lifecycle event to all subscribers
    ///
    /// ## Purpose
    /// Internal helper to multicast lifecycle events to all observability backends.
    ///
    /// ## Arguments
    /// * `event` - The lifecycle event to publish
    #[allow(dead_code)]
    async fn publish_lifecycle_event(&self, event: plexspaces_proto::ActorLifecycleEvent) {
        let actor_registry = match self.actor_registry().await {
            Ok(ar) => ar,
            Err(_) => return, // If registry not available, skip
        };
        let subscribers_arc = actor_registry.lifecycle_subscribers();
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
    /// use plexspaces_actor::behavior::GenServer;
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
                    tracing::warn!("Error handling lifecycle event for {}: {:?}", actor_id, e);
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
                )
                .increment(1);
                metrics::gauge!("plexspaces_node_active_actors",
                    "node_id" => self.id().as_str().to_string()
                )
                .decrement(1.0);
                tracing::info!(actor_id = %event.actor_id, node_id = %self.id().as_str(), reason = %terminated.reason, "Actor terminated");

                // Actor terminated normally - handle termination comprehensively
                if let Ok(actor_registry) = self.actor_registry().await {
                    let exit_reason = terminated.reason.parse().unwrap_or(ExitReason::Normal);
                    if let Ok(actor_id) = ActorId::from_canonical(&event.actor_id) {
                        actor_registry
                            .handle_actor_termination(&actor_id, exit_reason)
                            .await;
                    }
                }
            }
            Some(EventType::Failed(ref failed)) => {
                // Record metrics
                metrics::counter!("plexspaces_node_actors_failed_total",
                    "node_id" => self.id().as_str().to_string()
                )
                .increment(1);
                metrics::gauge!("plexspaces_node_active_actors",
                    "node_id" => self.id().as_str().to_string()
                )
                .decrement(1.0);
                tracing::error!(actor_id = %event.actor_id, node_id = %self.id().as_str(), error = %failed.error, "Actor failed");

                // Actor failed (panic/error) - handle termination comprehensively
                if let Ok(actor_registry) = self.actor_registry().await {
                    let exit_reason = ExitReason::Error(failed.error.clone());
                    if let Ok(actor_id) = ActorId::from_canonical(&event.actor_id) {
                        actor_registry
                            .handle_actor_termination(&actor_id, exit_reason)
                            .await;
                    }
                }
            }
            _ => {
                // Other lifecycle events (Starting, Activated, etc.) - log for observability
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %event.actor_id, node_id = %self.id().as_str(), "Lifecycle event");
                }
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
    /// * `ctx` - Request context carrying tenant scope; the application namespace is normalized to the application ID
    /// * `app` - Application implementation to register
    ///
    /// ## Returns
    /// * `Ok(())` - Application registered successfully
    /// * `Err(ApplicationError)` - Registration failed (e.g., duplicate name)
    ///
    /// ## Example
    /// ```ignore
    /// let app = Box::new(MyApplication::new());
    /// let ctx = plexspaces_common::RequestContext::new_without_auth(String::new(), "my-app".to_string());
    /// node.application_manager().register(&ctx, app).await?;
    /// ```
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
        tracing::warn!("\n╔════════════════════════════════════════════════════════════════╗");
        tracing::warn!("║  Starting Graceful Shutdown                                    ║");
        tracing::warn!("╚════════════════════════════════════════════════════════════════╝");
        tracing::warn!("Node: {} | Timeout: {:?}\n", self.id.as_str(), timeout);

        self.application_manager.request_shutdown().await;

        // Collect initial metrics before shutdown
        let (app_count, actor_count, queue_size, active_reqs, conn_nodes) =
            self.collect_shutdown_metrics().await;
        tracing::warn!("📊 Initial State:");
        tracing::warn!("   • Applications: {}", app_count);
        tracing::warn!("   • Actors: {}", actor_count);
        tracing::warn!("   • Total Mailbox Queue Size: {}", queue_size);
        tracing::warn!("   • Messages Routed: {}", active_reqs);
        tracing::warn!("   • Connected Nodes: {}", conn_nodes);

        // Begin graceful shutdown on health reporter (sets NOT_SERVING, prevents new requests)
        // HealthService.begin_shutdown() will set ServiceLocator.shutdown_flag
        {
            let health_reporter_guard = self.health_reporter.read().await;
            if let Some(ref health_reporter) = *health_reporter_guard {
                let (drained, duration, completed) =
                    health_reporter.begin_shutdown(Some(timeout)).await;
                tracing::warn!("🛑 Phase 1: Health Status");
                tracing::warn!("   ✓ Health set to NOT_SERVING");
                tracing::warn!(
                    "   ✓ Requests drained: {} | Duration: {:?} | Completed: {}",
                    drained,
                    duration,
                    completed
                );
            } else {
                // Fallback: if health reporter not available, set ServiceLocator flag directly
                self.service_locator.request_shutdown();
                tracing::warn!("🛑 Phase 1: Health Status");
                tracing::warn!(
                    "   ✓ ServiceLocator shutdown flag set (health reporter not available)"
                );
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
                tracing::warn!("   ✓ gRPC server shutdown signal sent");
            }
        }

        // Stop background scheduler (Phase 4)
        {
            let scheduler = self.background_scheduler.read().await;
            if let Some(scheduler) = scheduler.as_ref() {
                tracing::warn!("🛑 Phase 2: Background Services");
                tracing::warn!("   ✓ Background scheduler stopped");
                scheduler.stop();
            }
        }

        // Stop all applications (use Release order if available, otherwise reverse registration order)
        tracing::warn!("🛑 Phase 3: Stopping Applications");
        let apps_before =
            ApplicationManagerTrait::list_applications(self.application_manager.as_ref())
                .await
                .len();
        tracing::warn!("   • Stopping {} applications...", apps_before);

        let stop_start = std::time::Instant::now();

        // Try to use Release shutdown order if ReleaseSpec is available.
        // Embedded/test nodes may carry a runtime-only ReleaseSpec (for example to select
        // in-memory backends) without listing applications. In that case we still need to
        // stop every running application registered directly with the ApplicationManager.
        let mut stop_errors = Vec::new();
        let release_spec = self.release_spec.read().await;
        if let Some(ref spec) = *release_spec {
            let mut stopped_from_release = std::collections::HashSet::new();

            // Stop applications in reverse order (simple approach - proper dependency ordering would require Release helper)
            // Reverse iterate to stop dependents before dependencies
            for app_config in spec.applications.iter().rev() {
                if !app_config.enabled {
                    continue;
                }

                // Check if application is running
                let app_state = ApplicationManagerTrait::get_state(
                    self.application_manager.as_ref(),
                    &app_config.name,
                )
                .await;
                if app_state != Some(plexspaces_proto::v1::application::ApplicationState::ApplicationStateRunning) {
                    continue;
                }

                // Use app-specific timeout or fall back to global timeout
                let app_timeout = if let Some(ref duration) = app_config.shutdown_timeout {
                    tokio::time::Duration::from_secs(duration.seconds.max(0) as u64)
                } else {
                    timeout
                };

                if let Err(e) = self
                    .application_manager
                    .stop(&app_config.name, app_timeout)
                    .await
                {
                    tracing::warn!(
                        application = %app_config.name,
                        error = %e,
                        "Failed to stop application during shutdown (continuing with others)"
                    );
                    stop_errors.push(format!("{}: {}", app_config.name, e));
                } else {
                    stopped_from_release.insert(app_config.name.clone());
                }
            }

            // Stop any remaining running applications that were not part of the ReleaseSpec.
            // This keeps embedded/manual registrations aligned with node shutdown semantics.
            let remaining_apps =
                ApplicationManagerTrait::list_applications(self.application_manager.as_ref()).await;
            for app_name in remaining_apps.into_iter().rev() {
                if stopped_from_release.contains(&app_name) {
                    continue;
                }

                let app_state = ApplicationManagerTrait::get_state(
                    self.application_manager.as_ref(),
                    &app_name,
                )
                .await;
                if app_state
                    != Some(
                        plexspaces_proto::v1::application::ApplicationState::ApplicationStateRunning,
                    )
                {
                    continue;
                }

                if let Err(e) = self.application_manager.stop(&app_name, timeout).await {
                    tracing::warn!(
                        application = %app_name,
                        error = %e,
                        "Failed to stop application during shutdown (continuing with others)"
                    );
                    stop_errors.push(format!("{}: {}", app_name, e));
                }
            }
        } else {
            // No ReleaseSpec - use default stop_all (reverse registration order)
            if let Err(e) = self.application_manager.stop_all(timeout).await {
                stop_errors.push(e.to_string());
            }
        }

        let stop_duration = stop_start.elapsed();

        let apps_after =
            ApplicationManagerTrait::list_applications(self.application_manager.as_ref())
                .await
                .len();
        let apps_stopped = apps_before - apps_after;

        // Collect metrics after stopping applications
        let (_, after_actor_count, after_queue_size, _after_active_reqs, _) =
            self.collect_shutdown_metrics().await;

        tracing::warn!(
            "   ✓ Applications stopped: {} | Duration: {:?}",
            apps_stopped,
            stop_duration
        );
        tracing::warn!(
            "   • Remaining actors: {} (down from {})",
            after_actor_count,
            actor_count
        );
        tracing::warn!(
            "   • Remaining mailbox queue size: {} (down from {})",
            after_queue_size,
            queue_size
        );

        // Close network connections via NodeRegistry
        tracing::warn!("🛑 Phase 4: Network Connections");
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator().clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
        if let Some(node_registry) = service_locator_trait.get_node_registry().await {
            let ctx = service_locator_trait
                .request_context_for_system_operations()
                .await;
            match node_registry.list_nodes(&ctx, None, 1000, "").await {
                Ok((nodes, _)) => {
                    let node_count = nodes.len();
                    tracing::warn!("   • Closing {} connections...", node_count);
                    for node in nodes {
                        let _ = node_registry.unregister_node(&ctx, &node.node_id).await;
                    }
                }
                Err(e) => {
                    tracing::warn!("   • Could not list nodes: {}", e);
                }
            }
        } else {
            tracing::warn!("   • NodeRegistry not available");
        }
        tracing::warn!("   ✓ All network connections closed");

        // Flush TupleSpace pending operations (ensure all writes are persisted)
        // TupleSpace operations are synchronous, so no explicit flush needed
        // For external backends, they handle persistence automatically
        tracing::warn!("🛑 Phase 5: Final Cleanup");
        tracing::warn!("   ✓ TupleSpace operations flushed");

        // Final metrics
        let (final_app_count, final_actor_count, final_queue_size, final_active_reqs, _) =
            self.collect_shutdown_metrics().await;
        tracing::warn!("\n📊 Final State:");
        tracing::warn!(
            "   • Applications: {} (stopped: {})",
            final_app_count,
            apps_stopped
        );
        tracing::warn!(
            "   • Actors: {} (stopped: {})",
            final_actor_count,
            actor_count.saturating_sub(final_actor_count)
        );
        tracing::warn!(
            "   • Mailbox Queue Size: {} (drained: {})",
            final_queue_size,
            queue_size.saturating_sub(final_queue_size)
        );
        tracing::warn!(
            "   • Messages Routed: {} (during shutdown: {})",
            final_active_reqs,
            final_active_reqs.saturating_sub(active_reqs)
        );

        tracing::warn!("╔════════════════════════════════════════════════════════════════╗");
        tracing::warn!("║  ✅ Graceful Shutdown Complete                                ║");
        tracing::warn!("╚════════════════════════════════════════════════════════════════╝\n");
        if stop_errors.is_empty() {
            Ok(())
        } else {
            Err(ApplicationError::Other(format!(
                "Failed to stop applications: {}",
                stop_errors.join(", ")
            )))
        }
    }

    /// Collect shutdown metrics for logging
    async fn collect_shutdown_metrics(&self) -> (usize, usize, usize, usize, usize) {
        // Get application count
        let application_count =
            ApplicationManagerTrait::list_applications(self.application_manager.as_ref())
                .await
                .len();

        // Get actor count and mailbox queue sizes
        let (actor_count, total_mailbox_queue_size) =
            if let Some(actor_registry) = self.service_locator.actor_registry().await {
                let live_actor_entries = actor_registry.live_actor_entries().await;
                let actor_count = live_actor_entries.len();

                // Try to get mailbox queue sizes (may not be accessible for all actors)
                let total_queue_size = 0;
                for (_, _, actor_id) in &live_actor_entries {
                    if let Some(_sender) = actor_registry.lookup_actor(actor_id).await {
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

        // Get connected nodes from NodeRegistry
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator().clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
        let connected_nodes =
            if let Some(node_registry) = service_locator_trait.get_node_registry().await {
                let ctx = service_locator_trait
                    .request_context_for_system_operations()
                    .await;
                match node_registry.list_nodes(&ctx, None, 1000, "").await {
                    Ok((nodes, _)) => nodes.len(),
                    Err(_) => 0,
                }
            } else {
                0
            };

        (
            application_count,
            actor_count,
            total_mailbox_queue_size,
            active_requests,
            connected_nodes,
        )
    }

    /// Check if shutdown has been requested
    pub async fn is_shutdown_requested(&self) -> bool {
        self.application_manager.is_shutdown_requested().await
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
    pub async fn background_scheduler(
        &self,
    ) -> Option<Arc<plexspaces_scheduler::background::BackgroundScheduler>> {
        let scheduler = self.background_scheduler.read().await;
        scheduler.clone()
    }

    /// Get ApplicationManager
    ///
    /// ## Purpose
    /// Returns the ApplicationManager for this node.
    /// ApplicationManager is NOT registered in ServiceLocator - it's managed directly by Node.
    ///
    /// ## Returns
    /// Arc<ApplicationManager> for this node
    pub fn application_manager(&self) -> Arc<ApplicationManager> {
        self.application_manager.clone()
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
                let actor_ids = match node.service_locator().virtual_actor_manager().await {
                    Some(manager) => {
                        let virtual_actors = manager.registry().virtual_actors().read().await;
                        virtual_actors.keys().cloned().collect::<Vec<ActorId>>()
                    }
                    None => continue, // If virtual_actors not available, skip this iteration
                };

                // Check each virtual actor for idle timeout
                for actor_id in actor_ids {
                    let should_deactivate =
                        match node.service_locator().virtual_actor_manager().await {
                            Some(manager) => {
                                let virtual_actors =
                                    manager.registry().virtual_actors().read().await;
                                if let Some(virtual_meta) = virtual_actors.get(&actor_id) {
                                    // Use trait method directly - facet is Box<dyn VirtualActorLifecycleFacet>
                                    if let Some(facet_arc) = &virtual_meta.facet {
                                        let facet_guard = facet_arc.read().await;
                                        let result = facet_guard.should_deactivate().await;
                                        drop(facet_guard);
                                        drop(virtual_actors);
                                        result
                                    } else {
                                        drop(virtual_actors);
                                        false
                                    }
                                } else {
                                    drop(virtual_actors);
                                    false
                                }
                            }
                            None => false, // If virtual_actors not available, don't deactivate
                        };

                    if should_deactivate {
                        if let Some(actor_factory) =
                            node.service_locator().get_actor_factory().await
                        {
                            let ctx = node
                                .service_locator()
                                .request_context_for_system_operations()
                                .await;
                            if let Err(e) = actor_factory.stop_actor(&ctx, &actor_id).await {
                                tracing::warn!(
                                    "Failed to deactivate idle virtual actor {}: {}",
                                    actor_id,
                                    e
                                );
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
    fn service_locator(&self) -> Option<Arc<dyn ServiceLocatorTrait>> {
        Some(self.service_locator().clone() as Arc<dyn ServiceLocatorTrait>)
    }

    /// Get InitializableServiceLocator for startup/initialization code
    fn initializable_service_locator(&self) -> Option<Arc<dyn InitializableServiceLocator>> {
        Some(self.service_locator().clone() as Arc<dyn InitializableServiceLocator>)
    }

    /// Get BlobService for WASM actors
    async fn blob_service(&self) -> Option<Arc<dyn plexspaces_actor::BlobServiceTrait>> {
        let guard = self.blob_service.read().await;
        guard
            .clone()
            .map(|bs| bs as Arc<dyn plexspaces_actor::BlobServiceTrait>)
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
    #[error("Actor not found: {0}")]
    ActorNotFound(String),

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

    /// Actor spawn failed
    #[error("Actor spawn failed: {0}")]
    ActorSpawnFailed(String),

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
        // Connect to seed nodes via NodeRegistry
        let service_locator = self.local_node.service_locator();
        if let Some(node_registry) = service_locator.get_node_registry().await {
            let ctx = service_locator
                .request_context_for_system_operations()
                .await;
            for (node_id, address) in &self.config.seed_nodes {
                if node_id != self.local_node.id() {
                    // Register the seed node in NodeRegistry
                    let registration = plexspaces_proto::node::v1::NodeRegistration {
                        node_id: node_id.as_str().to_string(),
                        node_address: address.clone(),
                        ..Default::default()
                    };
                    if let Err(e) = node_registry.register_node(&ctx, registration).await {
                        tracing::warn!(node_id = %node_id.as_str(), address = %address, error = %e, "Failed to register seed node");
                    }
                }
            }
        }
        Ok(())
    }

    /// Leave the cluster
    pub async fn leave(&self) -> Result<(), NodeError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_actor::ActorId;
    use plexspaces_actor::ActorRef;
    use plexspaces_proto::actor::v1::ActorVisibility;
    use std::time::Duration;

    fn test_runtime_actor_id(name: &str, node_id: &str) -> ActorId {
        ActorId::new(name, "gen_server", "default", node_id).expect("test actor IDs must be valid")
    }

    // Helper functions for tests (defined inline since we can't import from tests/ directory)
    async fn lookup_actor_ref_helper(
        node: &Node,
        actor_id: &ActorId,
    ) -> Result<Option<ActorRef>, NodeError> {
        use plexspaces_actor::ActorRegistry;
        use std::sync::Arc;

        // Get ActorRegistry
        let actor_registry: Arc<ActorRegistry> = node
            .service_locator()
            .actor_registry()
            .await
            .ok_or_else(|| NodeError::ConfigError("ActorRegistry not found".to_string()))?;

        // Check if actor exists
        if let Some(_actor_trait) = actor_registry.lookup_actor(actor_id).await {
            Ok(Some(ActorRef::remote(
                actor_id.clone(),
                "".to_string(), // tenant_id
                "".to_string(), // TODO: get namespace from context
                node.id().as_str().to_string(),
                node.service_locator().clone(),
                ActorVisibility::ActorVisibilityPublic,
            )))
        } else {
            if actor_id.node_id() != node.id().as_str() {
                Ok(Some(ActorRef::remote(
                    actor_id.clone(),
                    "".to_string(),
                    "".to_string(),
                    actor_id.node_id().to_string(),
                    node.service_locator().clone(),
                    ActorVisibility::ActorVisibilityPublic,
                )))
            } else {
                Ok(None)
            }
        }
    }

    // Alias for consistency with test files
    use lookup_actor_ref_helper as lookup_actor_ref;

    use super::*;
    use plexspaces_mailbox::Mailbox;

    use plexspaces_actor::MessageSender;

    // Import NodeBuilder for tests
    use crate::NodeBuilder;

    // Helper to get ActorRegistry from service_locator
    async fn get_actor_registry(node: &Node) -> Arc<ActorRegistry> {
        // Ensure services are initialized
        node.initialize_services().await.unwrap();
        node.actor_registry().await.unwrap()
    }

    // Helper to register actor with MessageSender (replaces register_local)
    // Test helper function - registering test actors
    // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
    async fn register_actor_for_test(node: &Node, actor_id: &ActorId, mailbox: Arc<Mailbox>) {
        let wrapper = Arc::new(ActorRef::local(
            actor_id.clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox,
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(node).await;
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor_id.clone(),
                    sender: wrapper,
                    actor_type: actor_id.actor_type().to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
    }

    #[tokio::test]
    async fn test_node_creation() {
        let node = NodeBuilder::new("test-node").build().await;
        assert_eq!(node.id().as_str(), "test-node");
    }

    #[tokio::test]
    async fn test_actor_registration() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = NodeBuilder::new("test-node").build().await;

        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_ref = ActorRef::local(
            test_runtime_actor_id("test-actor", "test-node"),
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );

        // Register with ActorRegistry first
        // Register actor with MessageSender (mailbox is internal)

        use plexspaces_actor::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Test code - registering and looking up test actors
        // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        // Register actor (idempotent - can be called multiple times)
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender: wrapper,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Should find local actor via ActorRegistry
        assert!(actor_registry.lookup_actor(actor_ref.id()).await.is_some());
    }

    #[tokio::test]
    async fn test_tuplespace_integration() {
        let node = NodeBuilder::new("test-node").build().await;

    }

    #[tokio::test]
    async fn test_actor_unregistration() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = NodeBuilder::new("test-node").build().await;

        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_id = test_runtime_actor_id("test-actor", "test-node");
        let actor_ref = ActorRef::local(
            actor_id,
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );

        // Register with ActorRegistry first
        // Register actor with MessageSender (mailbox is internal)

        use plexspaces_actor::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Test code - registering test actors
        // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender: wrapper,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Update actor registration with config (idempotent - actor already registered)
        let actor_registry = get_actor_registry(&node).await;
        if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
            // Tenant comes from auth, not config
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            actor_registry
                .register_actor(
                    &ctx,
                    ActorRegistrationParams {
                        actor_id: actor_ref.id().clone(),
                        sender,
                        actor_type: "test_actor".to_string(),
                        config: None,
                        instance: None,
                        behavior_kind: None,
                    },
                )
                .await;
        }
        // Test code - looking up test actors
        // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
        assert!(actor_registry.lookup_actor(actor_ref.id()).await.is_some());

        // Unregister
        actor_registry
            .unregister_with_cleanup(actor_ref.id())
            .await
            .unwrap();
        // After unregistering, the local sender should no longer be discoverable.
        let lookup_result = actor_registry.lookup_actor(actor_ref.id()).await;
        assert!(
            lookup_result.is_none(),
            "Actor should not be found after unregistering"
        );
    }

    #[tokio::test]
    async fn test_duplicate_actor_registration() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = NodeBuilder::new("test-node").build().await;

        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_id = test_runtime_actor_id("test-actor", "test-node");
        let actor_ref = ActorRef::local(
            actor_id,
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );

        // Register with ActorRegistry first (using MessageSender)
        register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

        // First registration should succeed
        let actor_registry = get_actor_registry(&node).await;
        if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
            // Tenant comes from auth, not config
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            actor_registry
                .register_actor(
                    &ctx,
                    ActorRegistrationParams {
                        actor_id: actor_ref.id().clone(),
                        sender,
                        actor_type: "test_actor".to_string(),
                        config: None,
                        instance: None,
                        behavior_kind: None,
                    },
                )
                .await;
        }

        // Second registration should also succeed (idempotent - safe to call multiple times)
        // This is a production-grade design: register_actor is idempotent
        // to allow safe retries and prevent errors from duplicate calls
        let actor_registry = get_actor_registry(&node).await;
        if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
            // Tenant comes from auth, not config
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            actor_registry
                .register_actor(
                    &ctx,
                    ActorRegistrationParams {
                        actor_id: actor_ref.id().clone(),
                        sender,
                        actor_type: "test_actor".to_string(),
                        config: None,
                        instance: None,
                        behavior_kind: None,
                    },
                )
                .await;
        }
    }

    #[tokio::test]
    async fn test_route_message_local() {
        use plexspaces_mailbox::mailbox_config_default;

        let node_arc = Arc::new(NodeBuilder::new("test-node").build().await);

        let node = node_arc.as_ref();

        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_id = test_runtime_actor_id("test-actor", "test-node");
        let actor_ref = ActorRef::local(
            actor_id,
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );

        // Register with ActorRegistry (using MessageSender)
        register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        let ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref.clone());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id,
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Send message via ActorRef directly to local mailbox
        let message = Message {
            id: ulid::Ulid::new().to_string(),
            payload: vec![1, 2, 3],
            ..Default::default()
        };
        actor_ref.tell(&ctx, message).await.unwrap();

        // Verify message was delivered to the mailbox
        // ActorRef::tell() enqueues directly into the mailbox; routing metrics are
        // only updated when messages flow through Node::route_message() / routing.rs.
        let msg = mailbox
            .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(200)))
            .await;
        assert!(msg.is_some(), "message should be delivered to mailbox");
        assert_eq!(msg.unwrap().payload, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_route_message_actor_not_found() {
        let node = NodeBuilder::new("test-node").build().await;

        // Initialize services
        node.initialize_services().await.unwrap();

        let message = Message {
            id: ulid::Ulid::new().to_string(),
            payload: vec![1, 2, 3],
            ..Default::default()
        };

        // Try to send to non-existent actor
        // lookup_actor_ref returns Ok(None) for local actors that don't exist
        let tell_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let result =
            lookup_actor_ref(&node, &test_runtime_actor_id("nonexistent", "test-node")).await;
        let result = match result {
            Ok(Some(actor_ref)) => actor_ref
                .tell(&tell_ctx, message)
                .await
                .map_err(|e| NodeError::DeliveryFailed(format!("{}", e))),
            Ok(None) => {
                // Local actor not found - try to send anyway to get ActorNotFound error
                // Or return ActorNotFound directly
                Err(NodeError::ActorNotFound(
                    "nonexistent@test-node".to_string(),
                ))
            }
            Err(e) => Err(e),
        };
        assert!(result.is_err());
        match result {
            Err(NodeError::ActorNotFound(_)) => {} // Expected
            _ => panic!("Expected ActorNotFound error, got: {:?}", result),
        }
    }

    #[tokio::test]
    async fn test_node_announcement() {
        let node = NodeBuilder::new("test-node").build().await;

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

    }

    // ============================================================================
    // spawn_actor() Tests (Erlang-style supervision)
    // ============================================================================

    #[tokio::test]
    async fn test_spawn_actor_creates_and_returns_ref() {
        use plexspaces_actor::behavior::MockBehavior;
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use std::sync::Arc;

        let node = NodeBuilder::new("test-node").build().await;

        // Initialize services (registers all services including ActorFactory)
        node.initialize_services().await.unwrap();

        // Register a BehaviorRegistry so spawn_actor can create "test" actors
        {
            use plexspaces_actor::behavior_factory::BehaviorRegistry;
            let registry = BehaviorRegistry::new();
            registry
                .register_simple("test", || {
                    Box::pin(async move {
                        Ok(Box::new(MockBehavior::new()) as Box<dyn plexspaces_actor::Actor>)
                    })
                })
                .await;
            node.service_locator()
                .register_behavior_registry(Arc::new(registry))
                .await;
        }

        // Get ActorFactory from ServiceLocator using extension trait
        let service_locator = node.service_locator();
        use plexspaces_actor::ActorFactory;
        let actor_factory = service_locator
            .get_actor_factory()
            .await
            .expect("ActorFactoryImpl should be registered after initialize_services()");

        // Test code - spawn with explicit tenant/namespace per Rule #6
        let spawn_ctx = plexspaces_actor::RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "default".to_string(),
        );
        // Build actor_id to match spawn spec: name="test-actor", type="test", namespace="default", node="test-node"
        let actor_id =
            ActorId::new("test-actor", "test", "default", "test-node").expect("valid actor id");
        let _message_sender = actor_factory
            .spawn_actor(
                &spawn_ctx,
                &plexspaces_actor::ActorSpawnSpec {
                    identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                        name: actor_id.name().to_string(),
                        actor_type: actor_id.actor_type().to_string(),
                    }),
                    namespace: actor_id.namespace().to_string(),
                    tenant_id: "test-tenant".to_string(),
                    ..Default::default()
                },
                vec![],
            )
            .await
            .unwrap();

        // Verify actor registered in ActorRegistry — lookup by canonical ActorId
        let actor_registry = get_actor_registry(&node).await;
        assert!(
            actor_registry.lookup_actor(&actor_id).await.is_some(),
            "spawned actor must appear in registry"
        );
    }

    // Known issue: Actors don't terminate automatically - they require explicit stop.
    // Dropping actor_ref doesn't trigger termination notification.
    // TODO: Implement proper actor lifecycle with explicit stop_actor() method
    // that triggers termination notifications to monitors.

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

        // Register a BehaviorRegistry so spawn_actor can create "test" actors
        {
            use plexspaces_actor::behavior_factory::BehaviorRegistry;
            let registry = BehaviorRegistry::new();
            registry
                .register_simple("test", || {
                    Box::pin(async move {
                        Ok(Box::new(plexspaces_actor::behavior::MockBehavior::new())
                            as Box<dyn plexspaces_actor::Actor>)
                    })
                })
                .await;
            node.service_locator()
                .register_behavior_registry(Arc::new(registry))
                .await;
        }

        // Get ActorFactory from ServiceLocator using extension trait
        let service_locator = node.service_locator();
        use plexspaces_actor::ActorFactory;
        let actor_factory = service_locator
            .get_actor_factory()
            .await
            .expect("ActorFactory should be registered after initialize_services()");

        // Test code - spawn with explicit tenant/namespace per Rule #6
        let spawn_ctx = plexspaces_actor::RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "default".to_string(),
        );
        // actor_id must match spawn spec: type="test", namespace="default"
        let actor_id =
            ActorId::new("test-actor", "test", "default", "test-node").expect("valid actor id");
        let _message_sender = actor_factory
            .spawn_actor(
                &spawn_ctx,
                &plexspaces_actor::ActorSpawnSpec {
                    identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                        name: actor_id.name().to_string(),
                        actor_type: actor_id.actor_type().to_string(),
                    }),
                    namespace: actor_id.namespace().to_string(),
                    tenant_id: "test-tenant".to_string(),
                    ..Default::default()
                },
                vec![],
            )
            .await
            .unwrap();

        // Establish monitoring link — supervisor just needs to be registered to receive __DOWN__.
        let supervisor_id = test_runtime_actor_id("supervisor", "test-node");
        register_actor_for_test(node.as_ref(), &supervisor_id, {
            Arc::new(
                Mailbox::new(
                    mailbox_config_default(),
                    format!("sup-{}", ulid::Ulid::new()),
                )
                .await
                .unwrap(),
            )
        })
        .await;
        node.monitor(&spawn_ctx, &actor_id, &supervisor_id)
            .await
            .unwrap();

        // Verify the spawned actor is in the registry (monitoring only works on activated actors)
        let actor_registry = get_actor_registry(node.as_ref()).await;
        assert!(
            actor_registry.lookup_actor(&actor_id).await.is_some(),
            "spawned actor must appear in registry before monitoring"
        );
    }

    // ============================================================================
    // Remote Messaging Tests (ActorRef::tell() for remote actors, gRPC client pooling)
    // ============================================================================

    #[tokio::test]
    async fn test_tell_to_remote_node() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};

        // Create two nodes
        let node1 = Arc::new(
            NodeBuilder::new("node1")
                .with_in_memory_backends()
                .build()
                .await,
        );

        let node2 = Arc::new(
            NodeBuilder::new("node2")
                .with_in_memory_backends()
                .build()
                .await,
        );

        // Note: We no longer use register_remote_node - node discovery goes through ObjectRegistry/NodeRegistry
        // The registration happens below via ObjectRegistry.register()

        // Register actor on node2
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let remote_actor_id = test_runtime_actor_id("test-actor", "node2");
        let actor_ref = ActorRef::remote(
            remote_actor_id.clone(),
            "".to_string(),
            "".to_string(),
            "node2",
            node2.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );

        // Register actor with ActorRegistry on node2
        register_actor_for_test(&node2, actor_ref.id(), mailbox.clone()).await;
        let actor_registry2 = get_actor_registry(&node2).await;
        let ctx = node2
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
        actor_registry2
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id,
                    sender,
                    actor_type: "gen_server".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Register node2 in ObjectRegistry on node1 (so node1 can find it)
        use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        let registration = ObjectRegistration {
            object_type: ObjectType::ObjectTypeNode as i32,
            object_id: "node2".to_string(),
            grpc_address: "http://localhost:9999".to_string(),
            object_category: "Node".to_string(),
            ..Default::default()
        };
        let object_registry = node1.service_locator.get_object_registry().await.unwrap();
        object_registry.register(&ctx, registration).await.unwrap();

        // Try to route message from node1 to actor on node2
        let message = Message {
            id: ulid::Ulid::new().to_string(),
            payload: vec![1, 2, 3],
            ..Default::default()
        };

        // This will fail because we don't have a real gRPC server running
        // But it exercises the remote routing code path via ActorRef::tell()
        // Initialize services on node1
        node1.initialize_services().await.unwrap();
        let tell_ctx = node1
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let actor_ref = lookup_actor_ref(&node1, &remote_actor_id).await;
        let result = match actor_ref {
            Ok(Some(actor_ref)) => actor_ref
                .tell(&tell_ctx, message)
                .await
                .map_err(|e| NodeError::DeliveryFailed(format!("{}", e))),
            Ok(None) => Err(NodeError::ActorNotFound(remote_actor_id.to_string())),
            Err(e) => Err(e),
        };

        // Should fail with network error (no server listening)
        // The error could be NetworkError, DeliveryFailed, or ActorNotFound (if routing fails before network)
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(
                err,
                NodeError::NetworkError(_)
                    | NodeError::DeliveryFailed(_)
                    | NodeError::ActorNotFound(_)
            ),
            "Expected NetworkError, DeliveryFailed, or ActorNotFound, got: {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_find_actor_remote_via_node_id() {
        let node = NodeBuilder::new("node1").build().await;

        // Register remote node via NodeRegistry so it ends up in the in-memory cache,
        // which lookup_node checks even when use_shared_db=false.
        let ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let node_registry = node
            .service_locator
            .get_node_registry()
            .await
            .expect("NodeRegistry should be registered");
        node_registry
            .register_node(
                &ctx,
                plexspaces_proto::node::v1::NodeRegistration {
                    node_id: "node2".to_string(),
                    node_address: "http://localhost:9999".to_string(),
                    status: plexspaces_proto::node::v1::NodeStatus::NodeStatusReady as i32,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let result = node.lookup_node_address(&crate::NodeId::new("node2")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_find_actor_remote_not_found() {
        let node = NodeBuilder::new("node1").build().await;

        let result = node
            .lookup_node_address(&crate::NodeId::new("unknown-node"))
            .await;
        assert!(result.is_err(), "Should fail for non-existent remote node");
    }

    #[tokio::test]
    async fn test_find_actor_via_tuplespace() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};

        let node1 = NodeBuilder::new("node1").build().await;

        let node2 = NodeBuilder::new("node2").build().await;

        // Register actor on node2 (this writes to node2's TupleSpace)
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let remote_actor_id = test_runtime_actor_id("test-actor", "node2");
        let actor_ref = ActorRef::remote(
            remote_actor_id.clone(),
            "".to_string(),
            "".to_string(),
            "node2",
            node2.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );

        // Register actor with ActorRegistry on node2
        register_actor_for_test(&node2, actor_ref.id(), mailbox.clone()).await;
        let actor_registry2 = get_actor_registry(&node2).await;
        let ctx = node2
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
        actor_registry2
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id,
                    sender,
                    actor_type: "gen_server".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Register node2 in NodeRegistry on node1 so remote node resolution can succeed
        // (lookup_node checks the in-memory cache, populated by register_node).
        let ctx = node1
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let node_registry1 = node1
            .service_locator
            .get_node_registry()
            .await
            .expect("NodeRegistry should be registered");
        node_registry1
            .register_node(
                &ctx,
                plexspaces_proto::node::v1::NodeRegistration {
                    node_id: "node2".to_string(),
                    node_address: "http://localhost:9999".to_string(),
                    status: plexspaces_proto::node::v1::NodeStatus::NodeStatusReady as i32,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let result = node1
            .lookup_node_address(&crate::NodeId::new("node2"))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_to_remote_node_not_found() {
        let node = NodeBuilder::new("node1").build().await;

        let message = Message {
            id: ulid::Ulid::new().to_string(),
            payload: vec![1, 2, 3],
            ..Default::default()
        };

        // Lookup returns Ok(Some(remote_ref)) even when node is unregistered;
        // the failure surfaces at send time (connection refused / gRPC error).
        let actor_ref =
            lookup_actor_ref(&node, &test_runtime_actor_id("test-actor", "unknown-node")).await;
        assert!(
            actor_ref.is_ok(),
            "lookup should succeed (returns remote ref)"
        );
        let actor_ref = actor_ref.unwrap();
        assert!(
            actor_ref.is_some(),
            "lookup should return a remote actor ref"
        );

        // Sending to an unregistered node fails at delivery time
        let tell_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let send_result = actor_ref.unwrap().tell(&tell_ctx, message).await;
        assert!(send_result.is_err(), "tell to unknown node should fail");
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
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let monitored_actor_id = test_runtime_actor_id("monitored-actor", "test-node");
        let supervisor_id = test_runtime_actor_id("supervisor", "test-node");
        let actor_ref = ActorRef::local(
            monitored_actor_id.clone(),
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );

        // Register with ActorRegistry first (using MessageSender)
        register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        let ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id,
                    sender,
                    actor_type: "gen_server".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Register supervisor so it has a mailbox to receive __DOWN__ messages.
        let sup_mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("sup-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        register_actor_for_test(&node, &supervisor_id, sup_mailbox.clone()).await;

        // Monitor the actor
        let monitor_ref = node
            .monitor(&ctx, &monitored_actor_id, &supervisor_id)
            .await
            .unwrap();

        // Verify monitor_ref is a ULID (26 characters)
        assert_eq!(monitor_ref.len(), 26);

        // Notify actor down (via ActorRegistry)
        let actor_registry = get_actor_registry(&node).await;
        actor_registry
            .handle_actor_termination(
                &monitored_actor_id,
                ExitReason::Error("test reason".to_string()),
            )
            .await;

        // Supervisor's mailbox should receive __DOWN__ message.
        let mut down_msg: Option<Message> = None;
        for _ in 0..10 {
            if let Some(msg) = sup_mailbox
                .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(100)))
                .await
            {
                if msg.message_type == "__DOWN__" {
                    down_msg = Some(msg);
                    break;
                }
            }
        }
        let msg = down_msg.expect("Supervisor must receive __DOWN__ message");
        assert_eq!(
            msg.headers.get("down_from").map(|s| s.as_str()),
            Some(monitored_actor_id.to_string().as_str())
        );
        assert_eq!(
            msg.headers.get("down_reason").map(|s| s.as_str()),
            Some("test reason")
        );
    }

    #[tokio::test]
    async fn test_monitor_local_actor_not_found() {
        use tokio::sync::mpsc;

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Initialize services
        node.initialize_services().await.unwrap();

        // Try to monitor non-existent actor
        let ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let result = node
            .monitor(
                &ctx,
                &test_runtime_actor_id("nonexistent", "test-node"),
                &test_runtime_actor_id("supervisor", "test-node"),
            )
            .await;

        // Should fail with ActorNotFound
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NodeError::ActorNotFound(_)));
    }

    #[tokio::test]
    async fn test_monitor_remote_actor_node_not_connected() {
        let node = Arc::new(NodeBuilder::new("monitor-test-local").build().await);

        // Try to monitor actor on a node that was never registered anywhere
        let ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let result = node
            .monitor(
                &ctx,
                &test_runtime_actor_id("test-actor", "nonexistent-remote-node"),
                &test_runtime_actor_id("supervisor", "monitor-test-local"),
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
        actor_registry
            .handle_actor_termination(
                &test_runtime_actor_id("unmonitored-actor", "test-node"),
                ExitReason::Error("reason".to_string()),
            )
            .await;

        // Should succeed (no-op) - handle_actor_termination doesn't return Result, it's void
    }

    #[tokio::test]
    async fn test_handle_actor_termination_multiple_monitors() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use tokio::sync::mpsc;

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Register a local actor
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let watched_actor_id = test_runtime_actor_id("watched-actor", "test-node");
        let actor_ref = ActorRef::local(
            watched_actor_id.clone(),
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );

        // Register with ActorRegistry first (using MessageSender)
        register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        let ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id,
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Register 3 supervisor actors with mailboxes.
        let sup1_id = test_runtime_actor_id("sup1", "test-node");
        let sup2_id = test_runtime_actor_id("sup2", "test-node");
        let sup3_id = test_runtime_actor_id("sup3", "test-node");

        let sup1_mbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("sup1-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let sup2_mbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("sup2-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let sup3_mbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("sup3-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        register_actor_for_test(&node, &sup1_id, sup1_mbox.clone()).await;
        register_actor_for_test(&node, &sup2_id, sup2_mbox.clone()).await;
        register_actor_for_test(&node, &sup3_id, sup3_mbox.clone()).await;

        node.monitor(&ctx, &watched_actor_id, &sup1_id)
            .await
            .unwrap();
        node.monitor(&ctx, &watched_actor_id, &sup2_id)
            .await
            .unwrap();
        node.monitor(&ctx, &watched_actor_id, &sup3_id)
            .await
            .unwrap();

        // Notify actor down
        let actor_registry = node.actor_registry().await.unwrap();
        actor_registry
            .handle_actor_termination(&watched_actor_id, ExitReason::Error("crashed".to_string()))
            .await;

        // All 3 supervisor mailboxes should receive __DOWN__ messages.
        for (sup_mbox, sup_name) in [
            (&sup1_mbox, "sup1"),
            (&sup2_mbox, "sup2"),
            (&sup3_mbox, "sup3"),
        ] {
            let mut found = false;
            for _ in 0..10 {
                if let Some(msg) = sup_mbox
                    .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(100)))
                    .await
                {
                    if msg.message_type == "__DOWN__" {
                        assert_eq!(
                            msg.headers.get("down_reason").map(|s| s.as_str()),
                            Some("crashed"),
                            "{} should get 'crashed' reason",
                            sup_name
                        );
                        found = true;
                        break;
                    }
                }
            }
            assert!(found, "{} must receive __DOWN__", sup_name);
        }
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

        let node = Arc::new(
            NodeBuilder::new("test-node")
                .with_in_memory_backends()
                .build()
                .await,
        );
        node.initialize_services().await.unwrap();

        // Create subscription channel
        let (tx, mut rx) = mpsc::unbounded_channel();

        // Subscribe
        node.subscribe_lifecycle_events(tx).await;

        // Publish one event and confirm subscription is active first.
        let first_event = plexspaces_proto::ActorLifecycleEvent {
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
        actor_registry.publish_lifecycle_event(first_event).await;
        let received = tokio::time::timeout(tokio::time::Duration::from_millis(500), rx.recv())
            .await
            .unwrap();
        assert!(
            received.is_some(),
            "subscriber should receive event before unsubscribe"
        );

        // Unsubscribe
        node.unsubscribe_lifecycle_events().await;

        // Publish another event after unsubscribe.
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

        actor_registry.publish_lifecycle_event(event).await;

        // After unsubscribe, no new event should be delivered.
        let after_unsubscribe =
            tokio::time::timeout(tokio::time::Duration::from_millis(100), rx.recv()).await;
        assert!(
            !matches!(after_unsubscribe, Ok(Some(_))),
            "subscriber should not receive events after unsubscribe"
        );
    }

    #[tokio::test]
    async fn test_handle_lifecycle_event_terminated() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use tokio::sync::mpsc;

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Register actor and monitor it
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let monitored_actor_id = test_runtime_actor_id("test-actor", "test-node");
        let actor_ref = ActorRef::local(
            monitored_actor_id.clone(),
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );
        // Register actor with MessageSender (mailbox is internal)

        use plexspaces_actor::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Test code - registering test actors
        // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender: wrapper,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Register with ActorRegistry first (using MessageSender)
        register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        let ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id,
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        let supervisor_id = test_runtime_actor_id("supervisor", "test-node");
        let sup_mbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("sup-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        register_actor_for_test(&node, &supervisor_id, sup_mbox.clone()).await;
        node.monitor(&ctx, &monitored_actor_id, &supervisor_id)
            .await
            .unwrap();

        // Create Terminated event
        let event = plexspaces_proto::ActorLifecycleEvent {
            actor_id: monitored_actor_id.as_str().to_string(),
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

        // Supervisor mailbox should receive __DOWN__ message.
        let mut down: Option<Message> = None;
        for _ in 0..10 {
            if let Some(msg) = sup_mbox
                .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(100)))
                .await
            {
                if msg.message_type == "__DOWN__" {
                    down = Some(msg);
                    break;
                }
            }
        }
        let msg = down.expect("Supervisor must receive __DOWN__");
        assert_eq!(
            msg.headers.get("down_reason").map(|s| s.as_str()),
            Some("normal")
        );
    }

    #[tokio::test]
    async fn test_handle_lifecycle_event_failed() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
        use tokio::sync::mpsc;

        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        // Register actor and monitor it
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let monitored_actor_id = test_runtime_actor_id("test-actor", "test-node");
        let actor_ref = ActorRef::local(
            monitored_actor_id.clone(),
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );
        // Register actor with MessageSender (mailbox is internal)

        use plexspaces_actor::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Test code - registering test actors
        // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender: wrapper,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Register with ActorRegistry first (using MessageSender)
        register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        let ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref);
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id,
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        let supervisor_id = test_runtime_actor_id("supervisor", "test-node");
        let sup_mbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("sup-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        register_actor_for_test(&node, &supervisor_id, sup_mbox.clone()).await;
        node.monitor(&ctx, &monitored_actor_id, &supervisor_id)
            .await
            .unwrap();

        // Create Failed event
        let event = plexspaces_proto::ActorLifecycleEvent {
            actor_id: monitored_actor_id.as_str().to_string(),
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

        // Supervisor mailbox should receive __DOWN__ message.
        let mut down: Option<Message> = None;
        for _ in 0..10 {
            if let Some(msg) = sup_mbox
                .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(100)))
                .await
            {
                if msg.message_type == "__DOWN__" {
                    down = Some(msg);
                    break;
                }
            }
        }
        let msg = down.expect("Supervisor must receive __DOWN__");
        assert_eq!(
            msg.headers.get("down_reason").map(|s| s.as_str()),
            Some("panic: index out of bounds")
        );
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

        let node = node_arc.as_ref();

        // Initial stats
        let node_metrics = node.metrics().await;
        assert_eq!(node_metrics.messages_routed, 0);
        assert_eq!(node_metrics.local_deliveries, 0);
        assert_eq!(node_metrics.active_actors, 0);

        // Register an actor
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_id = test_runtime_actor_id("test-actor", "test-node");
        let actor_ref = ActorRef::local(
            actor_id,
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );

        // Register with ActorRegistry (using MessageSender)
        register_actor_for_test(&node, actor_ref.id(), mailbox.clone()).await;

        let actor_registry = get_actor_registry(&node).await;
        let ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref.clone());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id,
                    sender,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // active_actors is only updated when actors are spawned via ActorFactory, not when registered
        // So we check that the actor is registered instead
        let actor_registry = get_actor_registry(&node).await;
        let lookup_result = actor_registry.lookup_actor(actor_ref.id()).await;
        assert!(lookup_result.is_some(), "Actor should be registered");

        // Send local message via ActorRef directly to mailbox
        let message = Message {
            id: ulid::Ulid::new().to_string(),
            payload: vec![1, 2, 3],
            ..Default::default()
        };
        actor_ref.tell(&ctx, message).await.unwrap();

        // Verify delivery: ActorRef::tell() enqueues into the mailbox.
        // Routing counters (messages_routed, local_deliveries) are only updated
        // when messages flow through Node::route_message() / routing.rs — not here.
        let delivered = mailbox
            .dequeue_with_timeout(Some(tokio::time::Duration::from_millis(200)))
            .await;
        assert!(
            delivered.is_some(),
            "message should be delivered to mailbox"
        );

        // Unregister actor
        let actor_registry = get_actor_registry(&node).await;
        actor_registry
            .unregister_with_cleanup(actor_ref.id())
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
            common::v1::ResourceSpec,
            v1::actor::{
                ActorConfig, ActorResourceRequirements, NodePlacement, NodePlacementStrategy,
            },
        };

        let resource_requirements = ActorResourceRequirements {
            placement: Some(NodePlacement {
                strategy: NodePlacementStrategy::NodePlacementStrategyUnspecified as i32,
                cluster: String::new(),
                node_ids: vec![],
                required_labels: std::collections::HashMap::new(),
                avoid_node_ids: vec![],
                resource_requirements: Some(ResourceSpec {
                    cpu_cores,
                    memory_bytes,
                    disk_bytes,
                    gpu_count,
                    gpu_type: String::new(),
                }),
                affinity_labels: std::collections::HashMap::new(),
            }),
        };

        ActorConfig {
            mailbox_timeout: None,
            max_mailbox_size: 1000,
            enable_persistence: false,
            checkpoint_interval: None,
            restart_policy: None,
            supervision_strategy: 0,
            properties: std::collections::HashMap::new(),
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

        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_id = test_runtime_actor_id("test-actor", "test-node");
        let actor_ref = ActorRef::local(
            actor_id,
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );
        // Register actor with MessageSender (mailbox is internal)

        use plexspaces_actor::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Test code - registering test actors
        // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender: wrapper,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        let config =
            create_actor_config_with_resources(2.0, 1024 * 1024 * 512, 1024 * 1024 * 1024, 0);

        // Update actor registration with config (idempotent - actor already registered)
        let actor_registry = get_actor_registry(&node).await;
        if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
            // Tenant comes from auth, not config
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            actor_registry
                .register_actor(
                    &ctx,
                    ActorRegistrationParams {
                        actor_id: actor_ref.id().clone(),
                        sender,
                        actor_type: "test_actor".to_string(),
                        config: Some(config.clone()),
                        instance: None,
                        behavior_kind: None,
                    },
                )
                .await;
        }

        // Verify config is stored
        let actor_configs_arc = get_actor_registry(&node).await.actor_configs().clone();
        let actor_configs = actor_configs_arc.read().await;
        assert!(actor_configs.contains_key(actor_ref.id()));
        let stored_config = actor_configs.get(actor_ref.id()).unwrap();
        assert_eq!(
            stored_config
                .resource_requirements
                .as_ref()
                .unwrap()
                .placement
                .as_ref()
                .unwrap()
                .resource_requirements
                .as_ref()
                .unwrap()
                .cpu_cores,
            2.0
        );
    }

    #[tokio::test]
    async fn test_register_actor_without_config() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;
        let node = NodeBuilder::new("test-node").build().await;

        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_id = test_runtime_actor_id("test-actor", "test-node");
        let actor_ref = ActorRef::local(
            actor_id,
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );
        // Register actor with MessageSender (mailbox is internal)

        use plexspaces_actor::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Test code - registering test actors
        // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender: wrapper,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Actor already registered - no need to update config
        let actor_registry = get_actor_registry(&node).await;

        // Verify config is not stored
        let actor_configs_arc = get_actor_registry(&node).await.actor_configs().clone();
        let actor_configs = actor_configs_arc.read().await;
        assert!(!actor_configs.contains_key(actor_ref.id()));
    }

    #[tokio::test]
    async fn test_unregister_actor_removes_config() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;
        let node = NodeBuilder::new("test-node").build().await;

        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_id = test_runtime_actor_id("test-actor", "test-node");
        let actor_ref = ActorRef::local(
            actor_id,
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );
        // Register actor with MessageSender (mailbox is internal)

        use plexspaces_actor::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Test code - registering test actors
        // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender: wrapper,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        let config = create_actor_config_with_resources(1.0, 1024 * 1024 * 256, 0, 0);

        // Register actor with config
        let actor_registry = get_actor_registry(&node).await;
        if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
            // Tenant comes from auth, not config
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            actor_registry
                .register_actor(
                    &ctx,
                    ActorRegistrationParams {
                        actor_id: actor_ref.id().clone(),
                        sender,
                        actor_type: "test_actor".to_string(),
                        config: Some(config),
                        instance: None,
                        behavior_kind: None,
                    },
                )
                .await;
        }

        // Verify config is stored
        {
            let actor_configs_arc = get_actor_registry(&node).await.actor_configs().clone();
            let actor_configs = actor_configs_arc.read().await;
            assert!(actor_configs.contains_key(actor_ref.id()));
        }

        // Unregister actor
        let actor_registry = get_actor_registry(&node).await;
        actor_registry
            .unregister_with_cleanup(actor_ref.id())
            .await
            .unwrap();

        // Verify config is removed
        let actor_configs_arc = get_actor_registry(&node).await.actor_configs().clone();
        let actor_configs = actor_configs_arc.read().await;
        assert!(!actor_configs.contains_key(actor_ref.id()));
    }

    #[tokio::test]
    async fn test_calculate_node_capacity_with_actors() {
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;

        let node = NodeBuilder::new("test-node").build().await;

        // Register first actor with resources
        let mailbox1 = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-1-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor1_id = test_runtime_actor_id("actor-1", "test-node");
        let actor1_ref = ActorRef::local(
            actor1_id,
            "".to_string(),
            "".to_string(),
            mailbox1.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );
        // Register actor with MessageSender (mailbox is internal)

        use plexspaces_actor::MessageSender;
        let wrapper1 = Arc::new(ActorRef::local(
            actor1_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox1.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor1_ref.id().clone(),
                    sender: wrapper1,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
        let config1 =
            create_actor_config_with_resources(2.0, 1024 * 1024 * 512, 1024 * 1024 * 1024, 0);
        let actor_registry = get_actor_registry(&node).await;
        if let Some(sender1) = actor_registry.lookup_actor(&actor1_ref.id().clone()).await {
            // Tenant comes from auth, not config
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            actor_registry
                .register_actor(
                    &ctx,
                    ActorRegistrationParams {
                        actor_id: actor1_ref.id().clone(),
                        sender: sender1,
                        actor_type: "test_actor".to_string(),
                        config: Some(config1),
                        instance: None,
                        behavior_kind: None,
                    },
                )
                .await;
        }

        // Register second actor with resources
        let mailbox2 = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-2-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor2_id = test_runtime_actor_id("actor-2", "test-node");
        let actor2_ref = ActorRef::local(
            actor2_id,
            "".to_string(),
            "".to_string(),
            mailbox2.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );
        let config2 =
            create_actor_config_with_resources(1.5, 1024 * 1024 * 256, 512 * 1024 * 1024, 1);
        // Register actor2 with MessageSender first
        let wrapper2 = Arc::new(ActorRef::local(
            actor2_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox2.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor2_ref.id().clone(),
                    sender: wrapper2,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
        let actor_registry = get_actor_registry(&node).await;
        if let Some(sender2) = actor_registry.lookup_actor(&actor2_ref.id().clone()).await {
            // Tenant comes from auth, not config
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            actor_registry
                .register_actor(
                    &ctx,
                    ActorRegistrationParams {
                        actor_id: actor2_ref.id().clone(),
                        sender: sender2,
                        actor_type: "test_actor".to_string(),
                        config: Some(config2),
                        instance: None,
                        behavior_kind: None,
                    },
                )
                .await;
        }

        // Calculate capacity
        let capacity = node.calculate_node_capacity().await;

        // Verify allocated resources are summed correctly
        let allocated = capacity.allocated.as_ref().unwrap();
        assert_eq!(allocated.cpu_cores, 3.5); // 2.0 + 1.5
        assert_eq!(
            allocated.memory_bytes,
            1024 * 1024 * 512 + 1024 * 1024 * 256
        ); // 512MB + 256MB
        assert_eq!(allocated.disk_bytes, 1024 * 1024 * 1024 + 512 * 1024 * 1024); // 1GB + 512MB
        assert_eq!(allocated.gpu_count, 1); // 0 + 1

        // Verify available resources are calculated correctly
        let available = capacity.available.as_ref().unwrap();
        let total = capacity.total.as_ref().unwrap();
        assert_eq!(available.cpu_cores, total.cpu_cores - allocated.cpu_cores);
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
        let mailbox = Arc::new(
            Mailbox::new(
                plexspaces_mailbox::mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_id = test_runtime_actor_id("actor-1", "test-node");
        let actor_ref = ActorRef::local(
            actor_id,
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );
        // Register actor with MessageSender (mailbox is internal)

        use plexspaces_actor::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Test code - registering test actors
        // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender: wrapper,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
        let mut config = plexspaces_proto::v1::actor::ActorConfig::default();
        config.resource_requirements = None; // No resource requirements
        config.config_schema_version = 1;
        let actor_registry = get_actor_registry(&node).await;
        if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
            // Tenant comes from auth, not config
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            actor_registry
                .register_actor(
                    &ctx,
                    ActorRegistrationParams {
                        actor_id: actor_ref.id().clone(),
                        sender,
                        actor_type: "test_actor".to_string(),
                        config: Some(config),
                        instance: None,
                        behavior_kind: None,
                    },
                )
                .await;
        }

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
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_id = test_runtime_actor_id("actor-1", "test-node");
        let actor_ref = ActorRef::local(
            actor_id,
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );
        let config = create_actor_config_with_resources(2.0, 1024 * 1024 * 512, 0, 0);
        let actor_registry = get_actor_registry(&node).await;

        // Register actor first
        // Tenant comes from auth, not config
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_actor::MessageSender> = Arc::new(actor_ref.clone());
        actor_registry
            .register_actor(
                &ctx,
                ActorRegistrationParams {
                    actor_id: actor_id.clone(),
                    sender: sender.clone(),
                    actor_type: "test_actor".to_string(),
                    config: Some(config),
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;

        // Verify allocated resources
        let capacity = node.calculate_node_capacity().await;
        let allocated = capacity.allocated.as_ref().unwrap();
        assert_eq!(allocated.cpu_cores, 2.0);

        // Unregister actor
        let actor_registry = get_actor_registry(&node).await;
        actor_registry
            .unregister_with_cleanup(actor_ref.id())
            .await
            .unwrap();

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
            common::v1::ResourceSpec,
            v1::actor::{ActorConfig, ActorResourceRequirements},
        };

        let resources = ResourceSpec {
            cpu_cores: 1.0,
            memory_bytes: 0,
            disk_bytes: 0,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        let resource_requirements = ActorResourceRequirements {
            placement: Some(plexspaces_proto::v1::actor::NodePlacement {
                strategy: plexspaces_proto::v1::actor::NodePlacementStrategy::NodePlacementStrategyUnspecified as i32,
                cluster: String::new(),
                node_ids: vec![],
                required_labels: std::collections::HashMap::new(),
                avoid_node_ids: vec![],
                resource_requirements: Some(resources),
                affinity_labels: std::collections::HashMap::new(),
            }),
        };

        let mut config = ActorConfig::default();
        config.resource_requirements = Some(resource_requirements);
        config.config_schema_version = 1;

        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        use std::sync::Arc;
        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                format!("test-mailbox-{}", ulid::Ulid::new()),
            )
            .await
            .unwrap(),
        );
        let actor_id = test_runtime_actor_id("actor-1", "test-node");
        let actor_ref = ActorRef::local(
            actor_id,
            "".to_string(),
            "".to_string(),
            mailbox.clone(),
            node.service_locator(),
            ActorVisibility::ActorVisibilityPublic,
        );
        // Register actor with MessageSender (mailbox is internal)

        use plexspaces_actor::MessageSender;
        let wrapper = Arc::new(ActorRef::local(
            actor_ref.id().clone(),
            "".to_string(), // test tenant
            "".to_string(), // test namespace
            mailbox.clone(),
            node.service_locator().clone(),
            ActorVisibility::ActorVisibilityPublic,
        ));
        let actor_registry = get_actor_registry(&node).await;
        // Test code - registering test actors
        // This is test code, so node.service_locator().request_context_for_system_operations().await is acceptable for test operations
        let internal_ctx = node
            .service_locator()
            .request_context_for_system_operations()
            .await;
        actor_registry
            .register_actor(
                &internal_ctx,
                ActorRegistrationParams {
                    actor_id: actor_ref.id().clone(),
                    sender: wrapper,
                    actor_type: "test_actor".to_string(),
                    config: None,
                    instance: None,
                    behavior_kind: None,
                },
            )
            .await;
        let actor_registry = get_actor_registry(&node).await;
        if let Some(sender) = actor_registry.lookup_actor(&actor_ref.id().clone()).await {
            // Tenant comes from auth, not config
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            actor_registry
                .register_actor(
                    &ctx,
                    ActorRegistrationParams {
                        actor_id: actor_ref.id().clone(),
                        sender,
                        actor_type: "test_actor".to_string(),
                        config: Some(config),
                        instance: None,
                        behavior_kind: None,
                    },
                )
                .await;
        }

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
    use plexspaces_application::{Application, ApplicationError, ApplicationNode};
    use plexspaces_common::{RequestContext, RequestContextExt};
    use plexspaces_proto::v1::application::{ApplicationState, HealthStatus};

    fn app_ctx(name: &str) -> RequestContext {
        RequestContext::new_without_auth(String::new(), name.to_string())
    }

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
            tracing::warn!("MockApp '{}' starting on node: {}", self.name, node.id());
            if self.should_fail_start {
                Err(ApplicationError::StartupFailed("mock failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn stop(&mut self) -> Result<(), ApplicationError> {
            *self.stop_called.write().await = true;
            tracing::warn!("MockApp '{}' stopping", self.name);
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
        node.application_manager()
            .register(&app_ctx("test-app"), app)
            .await
            .unwrap();

        // Verify application is registered
        let state = node.application_manager().get_state("test-app").await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateCreated));
    }

    #[tokio::test]
    async fn test_register_duplicate_application() {
        let node = NodeBuilder::new("test-node").build().await;

        let app1 = Box::new(MockTestApplication::new("test-app"));
        node.application_manager()
            .register(&app_ctx("test-app"), app1)
            .await
            .unwrap();

        let app2 = Box::new(MockTestApplication::new("test-app"));
        let result = node
            .application_manager()
            .register(&app_ctx("test-app"), app2)
            .await;

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

        node.application_manager()
            .register(&app_ctx("test-app"), app)
            .await
            .unwrap();
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager().start("test-app").await.unwrap();

        // Verify application started
        let state = node.application_manager().get_state("test-app").await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateRunning));

        // Verify start was called
        assert!(*start_called.read().await);
    }

    #[tokio::test]
    async fn test_start_application_failure() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let app = Box::new(MockTestApplication::new_failing_start("test-app"));
        node.application_manager()
            .register(&app_ctx("test-app"), app)
            .await
            .unwrap();

        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        let result = node.application_manager().start("test-app").await;

        // Should fail
        assert!(result.is_err());

        // State should be Failed
        let state = node.application_manager().get_state("test-app").await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateFailed));
    }

    #[tokio::test]
    async fn test_stop_application() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let app = Box::new(MockTestApplication::new("test-app"));
        let stop_called = app.stop_called.clone();

        node.application_manager()
            .register(&app_ctx("test-app"), app)
            .await
            .unwrap();
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager().start("test-app").await.unwrap();
        node.application_manager()
            .stop("test-app", tokio::time::Duration::from_secs(5))
            .await
            .unwrap();

        // Verify application stopped
        let state = node.application_manager().get_state("test-app").await;
        assert_eq!(state, Some(ApplicationState::ApplicationStateStopped));

        // Verify stop was called
        assert!(*stop_called.read().await);
    }

    #[tokio::test]
    async fn test_stop_application_failure() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let app = Box::new(MockTestApplication::new_failing_stop("test-app"));
        node.application_manager()
            .register(&app_ctx("test-app"), app)
            .await
            .unwrap();
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager().start("test-app").await.unwrap();

        let result = node
            .application_manager()
            .stop("test-app", tokio::time::Duration::from_secs(5))
            .await;

        // Should fail
        assert!(result.is_err());

        // State should be Failed
        let state = node.application_manager().get_state("test-app").await;
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
            node.application_manager()
                .register(&app_ctx(app.name()), app)
                .await
                .unwrap();
        }

        node.application_manager()
            .ensure_node_context(node.clone())
            .await;
        node.application_manager().start("app1").await.unwrap();
        node.application_manager()
            .ensure_node_context(node.clone())
            .await;
        node.application_manager().start("app2").await.unwrap();
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager().start("app3").await.unwrap();

        // Shutdown all applications
        node.shutdown(tokio::time::Duration::from_secs(10))
            .await
            .unwrap();

        // Verify all applications stopped
        assert_eq!(
            node.application_manager().get_state("app1").await,
            Some(ApplicationState::ApplicationStateStopped)
        );
        assert_eq!(
            node.application_manager().get_state("app2").await,
            Some(ApplicationState::ApplicationStateStopped)
        );
        assert_eq!(
            node.application_manager().get_state("app3").await,
            Some(ApplicationState::ApplicationStateStopped)
        );

        // Verify shutdown flag set
        assert!(node.is_shutdown_requested().await);
    }

    #[tokio::test]
    async fn test_application_node_trait_implementation() {
        use crate::NodeBuilder;
        let node = NodeBuilder::new("test-node")
            .with_listen_addr("0.0.0.0:9999")
            .build()
            .await;

        // Test ApplicationNode trait methods (uses trait methods, not Node methods)
        let node_ref: &dyn ApplicationNode = &node;
        assert_eq!(node_ref.id(), "test-node");
        assert_eq!(node_ref.listen_addr(), "0.0.0.0:9999");
    }

    #[tokio::test]
    async fn test_start_nonexistent_application() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        let result = node.application_manager().start("nonexistent").await;

        // Should fail with not found error
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_stop_nonexistent_application() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let result = node
            .application_manager()
            .stop("nonexistent", tokio::time::Duration::from_secs(5))
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
        node.application_manager()
            .register(&app_ctx("lifecycle-test"), app)
            .await
            .unwrap();
        assert_eq!(
            node.application_manager().get_state("lifecycle-test").await,
            Some(ApplicationState::ApplicationStateCreated)
        );

        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager()
            .start("lifecycle-test")
            .await
            .unwrap();
        assert_eq!(
            node.application_manager().get_state("lifecycle-test").await,
            Some(ApplicationState::ApplicationStateRunning)
        );
        assert!(*start_called.read().await);

        node.application_manager()
            .stop("lifecycle-test", tokio::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            node.application_manager().get_state("lifecycle-test").await,
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

        node.application_manager()
            .register(&app_ctx("good-app"), app1)
            .await
            .unwrap();
        node.application_manager()
            .register(&app_ctx("bad-app"), app2)
            .await
            .unwrap();

        node.application_manager()
            .ensure_node_context(node.clone())
            .await;
        node.application_manager().start("good-app").await.unwrap();
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager().start("bad-app").await.unwrap();

        // Shutdown should fail due to bad-app
        let result = node.shutdown(tokio::time::Duration::from_secs(5)).await;
        assert!(result.is_err());

        // good-app should still be stopped
        assert_eq!(
            node.application_manager().get_state("good-app").await,
            Some(ApplicationState::ApplicationStateStopped)
        );

        // bad-app should be in Failed state
        assert_eq!(
            node.application_manager().get_state("bad-app").await,
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
        node.application_manager()
            .register(&app_ctx("test-app"), app)
            .await
            .unwrap();
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager().start("test-app").await.unwrap();

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
        let apps = manager.list_applications().await;
        assert_eq!(apps.len(), 0);
    }

    #[tokio::test]
    async fn test_multiple_start_attempts() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let app = Box::new(MockTestApplication::new("test-app"));
        node.application_manager()
            .register(&app_ctx("test-app"), app)
            .await
            .unwrap();

        // First start succeeds
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager().start("test-app").await.unwrap();
        assert_eq!(
            node.application_manager().get_state("test-app").await,
            Some(ApplicationState::ApplicationStateRunning)
        );

        // Second start should fail (not in Created state)
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        let result = node.application_manager().start("test-app").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("state"));
    }

    #[tokio::test]
    async fn test_stop_already_stopped_application() {
        let node = Arc::new(NodeBuilder::new("test-node").build().await);

        let app = Box::new(MockTestApplication::new("test-app"));
        node.application_manager()
            .register(&app_ctx("test-app"), app)
            .await
            .unwrap();
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager().start("test-app").await.unwrap();

        // First stop succeeds
        node.application_manager()
            .stop("test-app", tokio::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(
            node.application_manager().get_state("test-app").await,
            Some(ApplicationState::ApplicationStateStopped)
        );

        // Second stop should succeed (already stopped)
        let result = node
            .application_manager()
            .stop("test-app", tokio::time::Duration::from_secs(5))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_shutdown_stops_all_applications() {
        let node = Arc::new(
            NodeBuilder::new("test-node")
                .with_in_memory_backends()
                .build()
                .await,
        );

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
            node.application_manager()
                .register(&app_ctx(&format!("app{}", i)), app)
                .await
                .unwrap();
                node.application_manager()
                .ensure_node_context(
                    node.clone() as Arc<dyn plexspaces_application::ApplicationNode>
                )
                .await;
            node.application_manager()
                .start(&format!("app{}", i))
                .await
                .unwrap();
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

    #[tokio::test]
    async fn test_shutdown_stops_manual_applications_with_runtime_only_release_spec() {
        let node = Arc::new(
            NodeBuilder::new("test-node")
                .with_in_memory_backends()
                .build()
                .await,
        );

        let app = Box::new(MockTestApplication::new("test-app"));
        let stop_called = app.stop_called.clone();

        node.application_manager()
            .register(&app_ctx("test-app"), app)
            .await
            .unwrap();
        node.application_manager()
            .ensure_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
            .await;
        node.application_manager().start("test-app").await.unwrap();

        node.shutdown(tokio::time::Duration::from_secs(5))
            .await
            .unwrap();

        assert_eq!(
            node.application_manager().get_state("test-app").await,
            Some(ApplicationState::ApplicationStateStopped)
        );
        assert!(
            *stop_called.read().await,
            "shutdown should stop manually registered applications even when the node only carries runtime configuration in ReleaseSpec"
        );
    }
}

// ============================================================================
// Linking notes
// ============================================================================
//
// Local link and unlink behavior is owned by ActorRegistry.
// Node keeps the remote-link orchestration surface because remote resolution belongs at the node
// boundary rather than in the local actor registry.
// 3. Delegating to Node for remote actor linking
