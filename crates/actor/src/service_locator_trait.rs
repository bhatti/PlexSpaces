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

//! ServiceLocator and InitializableServiceLocator traits
//!
//! ## Design
//! - `ServiceLocator` — readonly accessor trait (get_*, is_*, etc.)
//! - `InitializableServiceLocator: ServiceLocator` — adds all register_* methods for node startup
//! - Runtime code takes `Arc<dyn ServiceLocator>`; init code takes `Arc<dyn InitializableServiceLocator>`
//!   or the concrete `Arc<ServiceLocatorImpl>` directly

use crate::ApplicationNode;
use async_trait::async_trait;
use std::sync::Arc;

use crate::actor_context::{ActorService, ChannelService, ObjectRegistry, TupleSpaceProvider};
use crate::behavior_factory::BehaviorRegistry;
use crate::facet_service_wrapper::{FacetManagerServiceWrapper, FacetRegistryServiceWrapper};
use crate::grpc_connection_manager::GrpcConnectionManager;
use crate::metrics_renderer::MetricsPrometheusRenderer;
use crate::metrics_service_access::MetricsServiceAccess;
use crate::monitoring::NodeConnectionInfo;
use crate::ActorFactory;
use crate::JournalStorage;
use crate::KeyValueStore;
use crate::RequestContext;
use crate::{ActorRegistry, ReplyWaiterRegistry, Service, VirtualActorManager};

/// Readonly service locator — runtime code depends only on this trait.
///
/// All `get_*`, `is_*`, `request_context_*`, and helper methods live here.
/// Registration belongs in `InitializableServiceLocator` and is called exclusively
/// during node startup via `Node::initialize_services()`.
///
/// ## Object Safety
/// Fully object-safe. Generic methods (`get_service`, `register_service`, etc.)
/// are on `InitializableServiceLocator` with `where Self: Sized` bounds.
#[async_trait]
pub trait ServiceLocator: plexspaces_service_traits::ServiceLocatorBase {
    /// Get ActorRegistry
    async fn actor_registry(&self) -> Option<Arc<ActorRegistry>>;

    /// Get VirtualActorManager
    async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>>;

    /// Get ReplyWaiterRegistry
    async fn reply_waiter_registry(&self) -> Option<Arc<ReplyWaiterRegistry>>;

    /// Get ChannelService
    async fn get_channel_service(&self) -> Option<Arc<dyn ChannelService>>;

    /// Get TupleSpaceProvider
    async fn get_tuplespace_provider(&self) -> Option<Arc<dyn TupleSpaceProvider>>;

    /// Get ObjectRegistry
    async fn get_object_registry(&self) -> Option<Arc<dyn ObjectRegistry>>;

    /// Prometheus text renderer for the in-process metrics recorder (optional).
    async fn get_metrics_prometheus_renderer(
        &self,
    ) -> Option<Arc<dyn MetricsPrometheusRenderer + Send + Sync>>;

    /// In-process metrics service (same recorder as gRPC `MetricsService`).
    async fn get_metrics_service_access(
        &self,
    ) -> Option<Arc<dyn MetricsServiceAccess + Send + Sync>>;

    /// Get FacetManager
    async fn get_facet_manager(&self) -> Option<Arc<FacetManagerServiceWrapper>>;

    /// Returns the in-memory facet container for `actor_id`.
    async fn facet_container_for_actor(
        &self,
        actor_id: &str,
    ) -> Option<Arc<tokio::sync::RwLock<plexspaces_facet::FacetContainer>>>;

    /// Get FacetRegistry
    async fn get_facet_registry(&self) -> Option<Arc<FacetRegistryServiceWrapper>>;

    /// Initialize default services in this ServiceLocator
    async fn initialize_services(
        &self,
        release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
    );

    /// Get node config
    async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig>;

    /// Get security config
    async fn get_security_config(&self) -> Option<plexspaces_proto::node::v1::SecurityConfig>;

    /// Get RuntimeConfig
    async fn get_runtime_config(&self) -> Option<plexspaces_proto::node::v1::RuntimeConfig>;

    /// Check if authentication is disabled
    async fn is_auth_disabled(&self) -> bool;

    /// Get NodeConnectionInfo accessor
    async fn get_node_connection_info(&self) -> Option<Arc<dyn NodeConnectionInfo + Send + Sync>>;

    /// Request shutdown
    fn request_shutdown(&self);

    /// Get ApplicationManager
    async fn application_manager(&self) -> Option<Arc<dyn ApplicationManager>>;

    /// Get BehaviorRegistry
    async fn get_behavior_registry(&self) -> Option<Arc<BehaviorRegistry>>;

    /// Get GrpcConnectionManager
    async fn get_grpc_connection_manager(&self) -> Option<Arc<GrpcConnectionManager>>;

    /// Get ActorServiceClient channel for a remote node.
    async fn get_actor_service_client(
        &self,
        node_id: &str,
    ) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>>;

    /// Get ApplicationService client channel for a remote node.
    async fn get_application_service_client(
        &self,
        node_id: &str,
    ) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>>;

    /// Get WASM runtime
    async fn get_wasm_runtime(&self) -> Option<std::sync::Arc<dyn WasmRuntimeTrait>>;

    /// Get ElasticPoolService
    async fn get_elastic_pool_service(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::ElasticPoolService>> {
        None
    }

    /// Get BlobService
    async fn get_blob_service(&self) -> Option<std::sync::Arc<dyn BlobServiceTrait>>;

    /// Get ServiceLinkAccess for live service link catalog
    async fn get_service_link_service(
        &self,
    ) -> Option<std::sync::Arc<dyn plexspaces_service_traits::ServiceLinkAccess>> {
        None
    }

    /// Get NodeRegistry
    async fn get_node_registry(&self) -> Option<std::sync::Arc<dyn NodeRegistryTrait>>;

    /// Get the WebSocket session registry (populated by Node::start()).
    async fn get_ws_registry(&self) -> Option<std::sync::Arc<dyn WsRegistryTrait>>;

    /// Get transport-agnostic actor client (WS-first with gRPC fallback when WsRegistry is registered).
    async fn get_actor_transport_client(
        &self,
    ) -> Option<std::sync::Arc<dyn plexspaces_service_traits::ActorTransportClient>>;

    /// Get transport-agnostic node client (WS-first with gRPC fallback when WsRegistry is registered).
    async fn get_node_transport_client(
        &self,
    ) -> Option<std::sync::Arc<dyn plexspaces_service_traits::NodeTransportClient>>;

    /// Get resilient outbound HTTP client for runtime service links.
    async fn get_outbound_http_client(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::OutboundHttpClient>> {
        None
    }

    /// Get ProcessGroupRegistry (as Arc<dyn Any>)
    async fn get_process_group_registry(
        &self,
    ) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        None
    }
}

/// Write-side of ServiceLocator — used exclusively during node startup / test setup.
///
/// Runtime code must NOT depend on this trait. Call `Node::initialize_services()`
/// or the concrete `ServiceLocatorImpl` to wire services at startup.
#[async_trait]
pub trait InitializableServiceLocator: ServiceLocator {
    /// Register a service by type (requires concrete type, cannot use on trait objects)
    async fn register_service<T: Service + 'static>(&self, service: Arc<T>)
    where
        Self: Sized;

    /// Get a service by type (requires concrete type, cannot use on trait objects)
    async fn get_service<T: Service + 'static>(&self) -> Option<Arc<T>>
    where
        Self: Sized;

    /// Register a service by name (requires concrete type, cannot use on trait objects)
    async fn register_service_by_name<T: Service + 'static>(&self, name: &str, service: Arc<T>)
    where
        Self: Sized;

    /// Get a service by name (requires concrete type, cannot use on trait objects)
    async fn get_service_by_name<T: Service + 'static>(&self, name: &str) -> Option<Arc<T>>
    where
        Self: Sized;

    /// Register ActorRegistry at node startup.
    async fn register_actor_registry(&self, registry: Arc<ActorRegistry>);
    /// Register ActorService at node startup.
    async fn register_actor_service(&self, service: Arc<dyn ActorService>);
    /// Register ChannelService at node startup.
    async fn register_channel_service(&self, service: Arc<dyn ChannelService>);
    /// Register TupleSpaceProvider at node startup.
    async fn register_tuplespace_provider(&self, service: Arc<dyn TupleSpaceProvider>);
    /// Register ObjectRegistry at node startup.
    async fn register_object_registry(&self, service: Arc<dyn ObjectRegistry>);
    /// Register JournalStorage at node startup.
    async fn register_journal_storage(&self, service: Arc<dyn JournalStorage + Send + Sync>);
    /// Register LockManager at node startup.
    async fn register_lock_manager(
        &self,
        service: Arc<dyn plexspaces_locks::LockManager + Send + Sync>,
    );
    /// Register Prometheus metrics renderer at node startup.
    async fn register_metrics_prometheus_renderer(
        &self,
        renderer: Arc<dyn MetricsPrometheusRenderer + Send + Sync>,
    );
    /// Register in-process MetricsServiceAccess at node startup.
    async fn register_metrics_service_access(
        &self,
        service: Arc<dyn MetricsServiceAccess + Send + Sync>,
    );
    /// Register FacetManagerServiceWrapper at node startup.
    async fn register_facet_manager(&self, service: Arc<FacetManagerServiceWrapper>);
    /// Register FacetRegistryServiceWrapper at node startup.
    async fn register_facet_registry(&self, service: Arc<FacetRegistryServiceWrapper>);
    /// Register ActorFactory at node startup.
    async fn register_actor_factory(&self, factory: Arc<dyn ActorFactory>);
    /// Register NodeConfig at node startup.
    async fn register_node_config(&self, config: plexspaces_proto::node::v1::NodeConfig);
    /// Register SecurityConfig at node startup.
    async fn register_security_config(&self, config: plexspaces_proto::node::v1::SecurityConfig);
    /// Register RuntimeConfig at node startup.
    async fn register_runtime_config(&self, config: plexspaces_proto::node::v1::RuntimeConfig);
    /// Register NodeConnectionInfo accessor at node startup.
    async fn register_node_connection_info(
        &self,
        accessor: Arc<dyn NodeConnectionInfo + Send + Sync>,
    );
    /// Register ApplicationManager at node startup.
    async fn register_application_manager(&self, manager: Arc<dyn ApplicationManager>);
    /// Register BehaviorRegistry at node startup.
    async fn register_behavior_registry(&self, registry: Arc<BehaviorRegistry>);
    /// Register GrpcConnectionManager at node startup.
    async fn register_grpc_connection_manager(&self, manager: Arc<GrpcConnectionManager>);
    /// Register WasmRuntime at node startup.
    async fn register_wasm_runtime(&self, runtime: std::sync::Arc<dyn WasmRuntimeTrait>);
    /// Register ProcessGroupService at node startup.
    async fn register_process_group_service(
        &self,
        service: std::sync::Arc<dyn crate::actor_context::ProcessGroupService>,
    );
    /// Register ElasticPoolService at node startup.
    async fn register_elastic_pool_service(
        &self,
        service: std::sync::Arc<dyn crate::ElasticPoolService>,
    );
    /// Register BlobService at node startup.
    async fn register_blob_service(&self, service: std::sync::Arc<dyn BlobServiceTrait>);
    /// Register ServiceLinkAccess at node startup.
    async fn register_service_link_service(
        &self,
        service: std::sync::Arc<dyn plexspaces_service_traits::ServiceLinkAccess>,
    );
    /// Register NodeRegistry at node startup.
    async fn register_node_registry(&self, registry: std::sync::Arc<dyn NodeRegistryTrait>);
    /// Register KeyValueStore at node startup.
    async fn register_keyvalue_store(&self, store: std::sync::Arc<dyn KeyValueStore>);
    /// Register outbound HTTP client at node startup.
    async fn register_outbound_http_client(
        &self,
        client: std::sync::Arc<dyn crate::OutboundHttpClient>,
    );
    /// Remove the registered outbound HTTP client.
    async fn unregister_outbound_http_client(&self);
    /// Register ProcessGroupRegistry at node startup.
    async fn register_process_group_registry(
        &self,
        registry: std::sync::Arc<dyn std::any::Any + Send + Sync>,
    );

    /// Register ActorTransportClient at node startup (replaces gRPC-only routing).
    async fn register_actor_transport_client(
        &self,
        client: std::sync::Arc<dyn plexspaces_service_traits::ActorTransportClient>,
    );

    /// Register NodeTransportClient at node startup.
    async fn register_node_transport_client(
        &self,
        client: std::sync::Arc<dyn plexspaces_service_traits::NodeTransportClient>,
    );

    /// Register WebSocket session registry at node startup.
    async fn register_ws_registry(&self, registry: std::sync::Arc<dyn WsRegistryTrait>);
}

/// Trait for WASM Runtime (defined in wasm-runtime crate)
///
/// ## Purpose
pub use plexspaces_service_traits::wasm_runtime::WasmRuntimeTrait;

/// Lifecycle and introspection interface for deployed WASM applications.
#[async_trait]
pub trait ApplicationManager: Send + Sync {
    /// Get the current state of a named application.
    async fn get_state(
        &self,
        name: &str,
    ) -> Option<plexspaces_proto::v1::application::ApplicationState>;
    /// List all deployed application names.
    async fn list_applications(&self) -> Vec<String>;
    /// Returns true if the node has been asked to shut down.
    async fn is_shutdown_requested(&self) -> bool;
    /// Get detailed ApplicationInfo for a named application.
    async fn get_application_info(
        &self,
        name: &str,
    ) -> Option<plexspaces_proto::application::v1::ApplicationInfo>;
    /// Get runtime metrics for a named application.
    async fn get_application_metrics(
        &self,
        name: &str,
    ) -> Option<plexspaces_proto::application::v1::ApplicationMetrics>;
    /// Merge (accumulate) metrics for a named application.
    async fn merge_application_metrics(
        &self,
        name: &str,
        metrics: plexspaces_proto::application::v1::ApplicationMetrics,
    ) -> Result<(), String>;
    /// Upcast to `Any` for downcasting by concrete type consumers.
    fn as_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync>;
    /// Get the node execution context associated with this manager.
    async fn get_node_context(&self) -> Option<std::sync::Arc<dyn ApplicationNode>>;
}

pub use plexspaces_service_traits::blob_service::BlobServiceTrait;

/// Cluster membership and gossip protocol interface for node discovery and heartbeats.
#[async_trait]
pub trait NodeRegistryTrait: Send + Sync {
    /// Look up a node registration by node ID.
    async fn lookup_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<
        Option<plexspaces_proto::node::v1::NodeRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    >;
    /// Register a node with the cluster.
    async fn register_node(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_proto::node::v1::NodeRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    /// Remove a node from the cluster registry.
    async fn unregister_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    /// List registered nodes, optionally filtered by cluster, with pagination.
    async fn list_nodes(
        &self,
        ctx: &RequestContext,
        cluster: Option<&str>,
        page_size: u32,
        page_token: &str,
    ) -> Result<
        (Vec<plexspaces_proto::node::v1::NodeRegistration>, String),
        Box<dyn std::error::Error + Send + Sync>,
    >;
    /// Send a liveness heartbeat for a node with optional capacity update.
    async fn send_heartbeat(
        &self,
        ctx: &RequestContext,
        node_id: &str,
        capacity: Option<plexspaces_proto::node::v1::NodeCapacity>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    /// Trigger a reconciliation ping to seed nodes.
    async fn kickoff_seed_reconcile_ping(
        &self,
        node_id: String,
        node_address: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    /// Start the gossip protocol loop.
    fn start_gossip_protocol(&self);
    /// Stop the gossip protocol loop.
    fn stop_gossip_protocol(&self);
    /// Returns true if gossip is currently running.
    fn is_gossip_running(&self) -> bool;
    /// Returns (total_entries, active_entries, oldest_entry_age) for the node cache.
    async fn cache_stats(&self) -> (usize, usize, std::time::Duration);
}

/// Interface for the WebSocket session registry.
///
/// Abstracts over `WsRegistry` (defined in `crates/node`) so that `ServiceLocator`
/// and consumers in other crates can access WS session state without depending on
/// the node crate (which would create a circular dependency).
///
/// # Design
/// Same pattern as `NodeRegistryTrait`: thin trait in `crates/actor`, concrete impl
/// in `crates/node`, registered via `InitializableServiceLocator::register_ws_registry`.
#[async_trait]
pub trait WsRegistryTrait: Send + Sync {
    /// Return the node IDs of all active thin-node sessions.
    async fn list_thin_nodes(&self) -> Vec<String>;
    /// Return the node IDs of all active sessions regardless of role.
    async fn list_all_nodes(&self) -> Vec<String>;
    /// Return true if a session for `node_id` is currently open.
    async fn is_connected(&self, node_id: &str) -> bool;
    /// Return the number of active sessions.
    async fn session_count(&self) -> usize;
}
