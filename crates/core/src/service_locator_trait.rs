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

//! ServiceLocator trait for service registration and retrieval
//!
//! ## Purpose
//! Defines the interface for service registration and retrieval.
//! The concrete implementation is in `plexspaces-services` crate.
//!
//! ## Design
//! - Core defines the trait
//! - Services implements the trait
//! - ActorContext uses the trait

use std::sync::Arc;
use async_trait::async_trait;

use crate::{ActorRegistry, VirtualActorManager, ReplyWaiterRegistry, Service};
use crate::actor_context::{ActorService, ChannelService, TupleSpaceProvider, ObjectRegistry};
use crate::actor_trait::MessageSender;
use crate::monitoring::{NodeMetricsAccessor, NodeConnectionInfo};
use crate::RequestContext;
use crate::JournalStorage;
use crate::KeyValueStore;
// LockManager is in plexspaces-locks crate
use crate::facet_service_wrapper::{FacetManagerServiceWrapper, FacetRegistryServiceWrapper};
use crate::behavior_factory::BehaviorRegistry;
use crate::grpc_connection_manager::GrpcConnectionManager;
use crate::ActorFactory;

/// Trait for service registration and retrieval
///
/// ## Purpose
/// Provides centralized service registration and retrieval interface.
/// Concrete implementation is in `plexspaces-services` crate.
///
/// ## Object Safety
/// This trait is designed to be object-safe. The generic methods have `where Self: Sized`
/// bounds and cannot be called on trait objects (`Arc<dyn ServiceLocator>`).
///
/// ## Best Practice for Trait Objects
/// When working with `Arc<dyn ServiceLocator>`, use the **strongly-typed accessor methods**
/// instead of generic methods. For example:
/// - Use `register_facet_registry()` instead of `register_service_by_name::<FacetRegistry>()`
/// - Use `get_actor_service()` instead of `get_service::<ActorService>()`
///
/// The generic methods are intended for use with concrete `ServiceLocatorImpl` only.
///
/// ## Note on ActorFactory
/// ActorFactory trait is defined in `plexspaces-core` crate to avoid circular dependencies.
/// Use `get_actor_factory()` and `register_actor_factory()` methods on ServiceLocator directly.
#[async_trait]
pub trait ServiceLocator: Send + Sync {
    // ============================================================================
    // GENERIC METHODS (require `Self: Sized`, cannot be called on trait objects)
    // Use strongly-typed methods below when working with Arc<dyn ServiceLocator>
    // ============================================================================
    
    /// Register a service by type (requires concrete type, cannot use on trait objects)
    async fn register_service<T: Service + 'static>(&self, service: Arc<T>)
    where Self: Sized;
    
    /// Get a service by type (requires concrete type, cannot use on trait objects)
    async fn get_service<T: Service + 'static>(&self) -> Option<Arc<T>>
    where Self: Sized;
    
    /// Register a service by name (requires concrete type, cannot use on trait objects)
    async fn register_service_by_name<T: Service + 'static>(&self, name: &str, service: Arc<T>)
    where Self: Sized;
    
    /// Get a service by name (requires concrete type, cannot use on trait objects)
    async fn get_service_by_name<T: Service + 'static>(&self, name: &str) -> Option<Arc<T>>
    where Self: Sized;
    
    // ============================================================================
    // STRONGLY-TYPED ACCESSOR METHODS (object-safe, work with trait objects)
    // Use these methods when working with Arc<dyn ServiceLocator>
    // ============================================================================
    
    /// Get ActorRegistry
    async fn actor_registry(&self) -> Option<Arc<ActorRegistry>>;
    
    /// Register ActorRegistry
    async fn register_actor_registry(&self, registry: Arc<ActorRegistry>);
    
    /// Get VirtualActorManager
    async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>>;
    
    /// Get ReplyWaiterRegistry
    async fn reply_waiter_registry(&self) -> Option<Arc<ReplyWaiterRegistry>>;
    
    /// Get ActorService
    async fn get_actor_service(&self) -> Option<Arc<dyn ActorService>>;
    
    /// Register ActorService
    async fn register_actor_service(&self, service: Arc<dyn ActorService>);
    
    /// Get ChannelService
    async fn get_channel_service(&self) -> Option<Arc<dyn ChannelService>>;
    
    /// Register ChannelService
    async fn register_channel_service(&self, service: Arc<dyn ChannelService>);
    
    /// Get TupleSpaceProvider
    async fn get_tuplespace_provider(&self) -> Option<Arc<dyn TupleSpaceProvider>>;
    
    /// Register TupleSpaceProvider
    async fn register_tuplespace_provider(&self, service: Arc<dyn TupleSpaceProvider>);
    
    /// Get ObjectRegistry
    async fn get_object_registry(&self) -> Option<Arc<dyn ObjectRegistry>>;
    
    /// Register ObjectRegistry
    async fn register_object_registry(&self, service: Arc<dyn ObjectRegistry>);
    
    /// Get JournalStorage
    async fn get_journal_storage(&self) -> Option<Arc<dyn JournalStorage + Send + Sync>>;
    
    /// Register JournalStorage
    async fn register_journal_storage(&self, service: Arc<dyn JournalStorage + Send + Sync>);
    
    /// Get LockManager
    async fn get_lock_manager(&self) -> Option<Arc<dyn plexspaces_locks::LockManager + Send + Sync>>;
    
    /// Register LockManager
    async fn register_lock_manager(&self, service: Arc<dyn plexspaces_locks::LockManager + Send + Sync>);
    
    /// Get NodeMetricsAccessor
    async fn get_node_metrics_accessor(&self) -> Option<Arc<dyn NodeMetricsAccessor + Send + Sync>>;
    
    /// Register NodeMetricsAccessor
    async fn register_node_metrics_accessor(&self, service: Arc<dyn NodeMetricsAccessor + Send + Sync>);
    
    /// Get FacetManager
    async fn get_facet_manager(&self) -> Option<Arc<FacetManagerServiceWrapper>>;
    
    /// Register FacetManager
    async fn register_facet_manager(&self, service: Arc<FacetManagerServiceWrapper>);
    
    /// Get FacetRegistry
    async fn get_facet_registry(&self) -> Option<Arc<FacetRegistryServiceWrapper>>;
    
    /// Register FacetRegistry
    async fn register_facet_registry(&self, service: Arc<FacetRegistryServiceWrapper>);
    
    /// Get ActorFactory
    ///
    /// ## Purpose
    /// Retrieves ActorFactory for spawning actors. ActorFactory trait is defined in core crate.
    ///
    /// ## Returns
    /// `Some(Arc<dyn ActorFactory>)` if registered, `None` otherwise
    async fn get_actor_factory(&self) -> Option<Arc<dyn ActorFactory>>;
    
    /// Register ActorFactory
    ///
    /// ## Purpose
    /// Registers ActorFactory for actor spawning. ActorFactory trait is defined in core crate.
    ///
    /// ## Arguments
    /// * `factory` - ActorFactory to register (as `Arc<dyn ActorFactory>`)
    async fn register_actor_factory(&self, factory: Arc<dyn ActorFactory>);
    
    /// Initialize default services in this ServiceLocator
    ///
    /// ## Purpose
    /// Populates the ServiceLocator with all default services needed for a node.
    /// This is the centralized initialization logic that can be called from
    /// `create_default_service_locator` or `Node::initialize_services`.
    ///
    /// ## Idempotent
    /// Safe to call multiple times - checks if services are already initialized and returns early.
    ///
    /// ## Arguments
    /// * `node_id` - Node ID for services (defaults to "test-node" if None)
    /// * `node_config` - Optional NodeConfig (if None, will be created from release_config.node or defaults)
    /// * `release_config` - Optional ReleaseSpec (if provided, node_config will be extracted from release_config.node)
    ///
    /// ## Note
    /// This method creates all default services including:
    /// - ActorFactoryImpl and facet factories (LockFacetFactory, RegistryFacetFactory, ProcessGroupFacetFactory)
    /// - ActorServiceImpl
    /// - TupleSpaceProvider
    /// Services crate depends on actor crate, so it can create these directly without closures.
    async fn initialize_services(
        &self,
        node_id: Option<String>,
        node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
        release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
    );
    
    /// Get node config
    async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig>;
    
    /// Register node config
    async fn register_node_config(&self, config: plexspaces_proto::node::v1::NodeConfig);
    
    /// Get security config (for authentication settings)
    async fn get_security_config(&self) -> Option<plexspaces_proto::node::v1::SecurityConfig>;
    
    /// Register security config
    async fn register_security_config(&self, config: plexspaces_proto::node::v1::SecurityConfig);

    /// Get RuntimeConfig (for accessing wasm_apps_directory, save_wasm_apps, etc.)
    async fn get_runtime_config(&self) -> Option<plexspaces_proto::node::v1::RuntimeConfig>;
    
    /// Register RuntimeConfig
    async fn register_runtime_config(&self, config: plexspaces_proto::node::v1::RuntimeConfig);
    
    /// Check if authentication is disabled
    /// 
    /// Returns true if disable_auth=true in SecurityConfig or PLEXSPACES_DISABLE_AUTH env var is set.
    /// Returns false (auth enabled) if SecurityConfig is not set or disable_auth=false.
    async fn is_auth_disabled(&self) -> bool;
    
    /// Get NodeConnectionInfo accessor
    async fn get_node_connection_info(&self) -> Option<Arc<dyn NodeConnectionInfo + Send + Sync>>;
    
    /// Register NodeConnectionInfo accessor
    async fn register_node_connection_info(&self, accessor: Arc<dyn NodeConnectionInfo + Send + Sync>);
    
    /// Check if shutdown is requested
    fn is_shutdown_requested(&self) -> bool;
    
    /// Request shutdown
    fn request_shutdown(&self);
    
    /// Get ApplicationManager
    ///
    /// ## Purpose
    /// Returns ApplicationManager for managing application lifecycle.
    /// ApplicationManager is managed by the application crate, not registered in ServiceLocator.
    ///
    /// ## Returns
    /// Some(Arc<ApplicationManager>) if available, None otherwise
    ///
    /// ## Note
    /// This method is optional - implementations that don't have ApplicationManager
    /// (e.g., in tests) can return None.
    async fn application_manager(&self) -> Option<Arc<dyn ApplicationManager>>;
    
    /// Register ApplicationManager
    ///
    /// ## Purpose
    /// Registers ApplicationManager for managing application lifecycle.
    ///
    /// ## Arguments
    /// * `manager` - ApplicationManager to register
    async fn register_application_manager(&self, manager: Arc<dyn ApplicationManager>);
    
    /// Get BehaviorRegistry
    ///
    /// ## Purpose
    /// Returns BehaviorRegistry for creating actor behaviors from registered factories.
    ///
    /// ## Returns
    /// Some(Arc<BehaviorRegistry>) if registered, None otherwise
    async fn get_behavior_registry(&self) -> Option<Arc<BehaviorRegistry>>;
    
    /// Register BehaviorRegistry
    ///
    /// ## Purpose
    /// Registers BehaviorRegistry for behavior creation.
    ///
    /// ## Arguments
    /// * `registry` - BehaviorRegistry to register
    async fn register_behavior_registry(&self, registry: Arc<BehaviorRegistry>);
    
    /// Create RequestContext for operations that have no request (e.g. node registration, heartbeat).
    ///
    /// ## Purpose
    /// Tenant/namespace from node config default_tenant_id/default_namespace if available, else blank.
    /// Use for system operations only; API handlers must use context from request (JWT/headers/mTLS).
    ///
    /// ## Returns
    /// RequestContext with defaults from NodeConfig, or empty strings if NodeConfig not available
    async fn request_context_for_system_operations(&self) -> RequestContext;

    /// Same as request_context_for_system_operations but with explicit namespace (e.g. cluster_name).
    async fn request_context_for_system_operations_with_namespace(&self, namespace: String) -> RequestContext;
    
    /// Get GrpcConnectionManager
    ///
    /// ## Purpose
    /// Returns GrpcConnectionManager for managing gRPC client connections with pooling.
    ///
    /// ## Returns
    /// Some(Arc<GrpcConnectionManager>) if registered, None otherwise
    async fn get_grpc_connection_manager(&self) -> Option<Arc<GrpcConnectionManager>>;
    
    /// Register GrpcConnectionManager
    ///
    /// ## Purpose
    /// Registers GrpcConnectionManager for connection pooling.
    ///
    /// ## Arguments
    /// * `manager` - GrpcConnectionManager to register
    async fn register_grpc_connection_manager(&self, manager: Arc<GrpcConnectionManager>);
    
    /// Get ActorServiceClient for remote node
    ///
    /// ## Purpose
    /// Helper method that combines ObjectRegistry lookup and GrpcConnectionManager
    /// to get an ActorServiceClient for a remote node. This eliminates duplicate
    /// code across actor-service and actor-ref.
    ///
    /// ## Arguments
    /// * `node_id` - Remote node ID
    ///
    /// ## Returns
    /// ActorServiceClient ready to use, or error if node not found or connection failed
    ///
    /// ## Note
    /// Uses request_context_for_system_operations for ObjectRegistry lookups (no request context).
    async fn get_actor_service_client(
        &self,
        node_id: &str,
    ) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get WASM runtime
    ///
    /// ## Purpose
    /// Returns WASM runtime for deploying and running WASM modules.
    ///
    /// ## Returns
    /// Some(Arc<dyn WasmRuntimeTrait>) if registered, None otherwise
    async fn get_wasm_runtime(&self) -> Option<std::sync::Arc<dyn WasmRuntimeTrait>>;
    
    /// Register WASM runtime
    ///
    /// ## Purpose
    /// Registers WASM runtime for deploying and running WASM modules.
    ///
    /// ## Arguments
    /// * `runtime` - WASM runtime to register (as Arc<dyn WasmRuntimeTrait>)
    async fn register_wasm_runtime(&self, runtime: std::sync::Arc<dyn WasmRuntimeTrait>);
    
    /// Get ProcessGroupService
    ///
    /// ## Purpose
    /// Returns ProcessGroupService for distributed pub/sub and broadcast messaging.
    /// ProcessGroupService provides Erlang pg/pg2-style process groups.
    ///
    /// ## Returns
    /// Arc<dyn ProcessGroupService> if registered, None otherwise
    async fn get_process_group_service(&self) -> Option<std::sync::Arc<dyn crate::actor_context::ProcessGroupService>>;
    
    /// Register ProcessGroupService
    ///
    /// ## Purpose
    /// Registers ProcessGroupService for distributed pub/sub.
    ///
    /// ## Arguments
    /// * `service` - ProcessGroupService to register
    async fn register_process_group_service(&self, service: std::sync::Arc<dyn crate::actor_context::ProcessGroupService>);
    
    /// Get BlobService
    ///
    /// ## Purpose
    /// Returns BlobService for blob storage operations (S3/MinIO/GCS/Azure).
    ///
    /// ## Returns
    /// Some(Arc<dyn BlobServiceTrait>) if registered, None otherwise
    async fn get_blob_service(&self) -> Option<std::sync::Arc<dyn BlobServiceTrait>>;
    
    /// Register BlobService
    ///
    /// ## Purpose
    /// Registers BlobService for blob storage operations.
    ///
    /// ## Arguments
    /// * `service` - BlobService to register (as Arc<dyn BlobServiceTrait>)
    async fn register_blob_service(&self, service: std::sync::Arc<dyn BlobServiceTrait>);
    
    /// Get NodeRegistry
    ///
    /// ## Purpose
    /// Returns NodeRegistry for node discovery and liveness tracking.
    /// NodeRegistry wraps ObjectRegistry with caching and gossip protocol support.
    ///
    /// ## Returns
    /// Some(Arc<dyn NodeRegistryTrait>) if registered, None otherwise
    async fn get_node_registry(&self) -> Option<std::sync::Arc<dyn NodeRegistryTrait>>;
    
    /// Register NodeRegistry
    ///
    /// ## Purpose
    /// Registers NodeRegistry for node discovery.
    ///
    /// ## Arguments
    /// * `registry` - NodeRegistry to register
    async fn register_node_registry(&self, registry: std::sync::Arc<dyn NodeRegistryTrait>);
}

/// Trait for WASM Runtime (defined in wasm-runtime crate)
///
/// ## Purpose
/// Allows ServiceLocator to return WASM runtime without depending on wasm-runtime crate.
/// The concrete WasmRuntime type is in plexspaces-wasm-runtime crate.
///
/// ## Note
/// This trait provides the full interface needed by WasmDeploymentService and WasmApplication.
/// Since these crates already depend on plexspaces-wasm-runtime, they can work with the concrete
/// types. The trait methods return Arc<dyn Any> which can be downcast to the concrete types
/// by the callers who know what types to expect.
#[async_trait]
pub trait WasmRuntimeTrait: Send + Sync {
    /// Get the number of cached modules
    async fn module_count(&self) -> usize;
    
    /// Clear the module cache
    async fn clear_cache(&self);
    
    /// Load WASM module from bytes
    ///
    /// ## Returns
    /// Arc<dyn Any> containing WasmModule (caller must downcast)
    async fn load_module(
        &self,
        name: &str,
        version: &str,
        bytes: &[u8],
    ) -> Result<std::sync::Arc<dyn std::any::Any + Send + Sync>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get cached module by hash
    ///
    /// ## Returns
    /// Arc<dyn Any> containing WasmModule if found (caller must downcast)
    async fn get_module(&self, hash: &str) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>;
    
    /// Resolve module by reference (name@version or hash)
    ///
    /// ## Returns
    /// Arc<dyn Any> containing WasmModule if found (caller must downcast)
    async fn resolve_module(&self, module_ref: &str) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>;
    
    /// Check if module is cached
    async fn contains_module(&self, hash: &str) -> bool;
    
    /// List all cached modules
    async fn list_modules(&self) -> Vec<(String, String, String)>;
    
    /// Remove module from cache
    async fn evict_module(&self, hash: &str) -> bool;
    
    /// Instantiate WASM module
    ///
    /// ## Returns
    /// Arc<dyn Any> containing WasmInstance (caller must downcast)
    ///
    /// ## Note
    /// `module` and `config` are kept as `Arc<dyn Any>` because they are concrete types
    /// specific to wasm-runtime crate. `process_group_registry` and `blob_service` are also
    /// concrete types, so they remain as `Arc<dyn Any>`.
    /// 
    /// `message_sender` is `Arc<dyn Any>` because the wasm-runtime crate has its own
    /// `MessageSender` trait (with ask, spawn, etc.) that differs from core's `MessageSender`.
    /// Callers pass the concrete wasm-runtime `MessageSender` wrapped in `Arc<dyn Any>`.
    async fn instantiate(
        &self,
        module: std::sync::Arc<dyn std::any::Any + Send + Sync>,
        actor_id: String,
        initial_state: &[u8],
        config: std::sync::Arc<dyn std::any::Any + Send + Sync>,
        channel_service: Option<std::sync::Arc<dyn ChannelService>>,
        message_sender: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,
        tuplespace_provider: Option<std::sync::Arc<dyn TupleSpaceProvider>>,
        keyvalue_store: Option<std::sync::Arc<dyn KeyValueStore>>,
        process_group_registry: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,
        lock_manager: Option<std::sync::Arc<dyn plexspaces_locks::LockManager + Send + Sync>>,
        object_registry: Option<std::sync::Arc<dyn ObjectRegistry>>,
        journal_storage: Option<std::sync::Arc<dyn JournalStorage>>,
        blob_service: Option<std::sync::Arc<dyn BlobServiceTrait>>,
    ) -> Result<std::sync::Arc<dyn std::any::Any + Send + Sync>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get as Arc<dyn Any> for downcasting to concrete type
    ///
    /// ## Purpose
    /// Allows components that need the concrete WasmRuntime type to downcast.
    /// This is necessary because WasmDeploymentService and WasmApplication require
    /// the concrete type, not the trait.
    ///
    /// ## Returns
    /// Arc<dyn Any + Send + Sync> that can be downcast to Arc<WasmRuntime>
    fn as_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync>;
}

/// Trait for ApplicationManager (defined in application crate)
///
/// ## Purpose
/// Allows ServiceLocator to return ApplicationManager without depending on application crate.
/// The concrete ApplicationManagerImpl type is in plexspaces-application crate.
#[async_trait]
pub trait ApplicationManager: Send + Sync {
    /// Get application state
    async fn get_state(&self, name: &str) -> Option<plexspaces_proto::v1::application::ApplicationState>;
    
    /// List all applications
    async fn list_applications(&self) -> Vec<String>;
    
    /// Check if shutdown is requested
    async fn is_shutdown_requested(&self) -> bool;
    
    /// Get application information
    ///
    /// ## Purpose
    /// Returns comprehensive information about an application including version, status, metrics, etc.
    ///
    /// ## Arguments
    /// * `name` - Application name
    ///
    /// ## Returns
    /// ApplicationInfo or None if not found
    async fn get_application_info(&self, name: &str) -> Option<plexspaces_proto::application::v1::ApplicationInfo>;
    
    
    /// Get as Arc<dyn Any> for downcasting to concrete type
    ///
    /// ## Purpose
    /// Allows components that need the concrete ApplicationManagerImpl type to downcast.
    /// This is necessary because ApplicationServiceImpl requires methods like `register`, `start`, `stop`
    /// that are on the concrete type, not the trait.
    ///
    /// ## Returns
    /// Arc<dyn Any + Send + Sync> that can be downcast to Arc<ApplicationManagerImpl>
    fn as_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync>;
}

/// Trait for BlobService (defined in blob crate)
///
/// ## Purpose
/// Allows ServiceLocator to return BlobService without depending on blob crate.
/// The concrete BlobService type is in plexspaces-blob crate.
///
/// ## Name-based vs ID-based access
/// - `upload`: Uses `name` (path like "assets/images/logo.png") - returns internal blob_id
/// - `download` / `delete`: Use internal blob_id (ULID)
/// - `download_by_name` / `delete_by_name`: Use name (path) - preferred for WASM actors
/// - `list`: Returns names (paths) matching a prefix
#[async_trait]
pub trait BlobServiceTrait: Send + Sync {
    /// Upload blob with a user-friendly name (path).
    /// If a blob with the same name exists, it will be replaced (upsert).
    /// Returns the internal blob_id on success.
    async fn upload(
        &self,
        ctx: &RequestContext,
        name: &str,
        data: Vec<u8>,
        content_type: Option<String>,
        metadata: std::collections::HashMap<String, String>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Download blob by internal blob_id (ULID).
    async fn download(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>;

    /// Download blob by name (path). Preferred for WASM actors.
    async fn download_by_name(
        &self,
        ctx: &RequestContext,
        name: &str,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Delete blob by internal blob_id (ULID).
    async fn delete(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Delete blob by name (path). Preferred for WASM actors.
    async fn delete_by_name(
        &self,
        ctx: &RequestContext,
        name: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Check if blob exists by internal blob_id.
    async fn exists(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>;

    /// List blob names (paths) matching a prefix.
    /// Returns names, not internal blob_ids.
    async fn list(
        &self,
        ctx: &RequestContext,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get as Arc<dyn Any> for downcasting to concrete type.
    /// This is needed for advanced usage where concrete BlobService methods are required
    /// (e.g., component_host.rs which uses full blob API).
    fn as_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync>;
}

/// Trait for NodeRegistry (defined in services crate)
///
/// ## Purpose
/// Provides node discovery and liveness tracking with caching.
/// NodeRegistry wraps ObjectRegistry with a TTL-based cache and
/// optional gossip protocol for node liveness in non-shared-db deployments.
///
/// ## Design
/// - **Composition over inheritance**: Wraps ObjectRegistry, does not extend trait
/// - **Caching layer**: TTL-based cache (default 60s, configurable)
/// - **Gossip protocol**: Optional liveness exchange when not using shared DB
/// - **Clean design**: Simple, extensible, loosely coupled
#[async_trait]
pub trait NodeRegistryTrait: Send + Sync {
    /// Lookup a node by ID (cache-first, then ObjectRegistry)
    ///
    /// ## Returns
    /// NodeRegistration if found, None otherwise
    async fn lookup_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<Option<plexspaces_proto::node::v1::NodeRegistration>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Register a node (updates both cache and ObjectRegistry)
    async fn register_node(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_proto::node::v1::NodeRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Unregister a node (removes from cache and ObjectRegistry)
    async fn unregister_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// List nodes (from cache if fresh, otherwise ObjectRegistry)
    ///
    /// ## Arguments
    /// * `cluster` - Optional cluster filter
    /// * `page_size` - Maximum nodes to return (0 = unlimited)
    /// * `page_token` - Pagination token (empty = first page)
    async fn list_nodes(
        &self,
        ctx: &RequestContext,
        cluster: Option<&str>,
        page_size: u32,
        page_token: &str,
    ) -> Result<(Vec<plexspaces_proto::node::v1::NodeRegistration>, String), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Send heartbeat (updates liveness in cache and ObjectRegistry)
    async fn send_heartbeat(
        &self,
        ctx: &RequestContext,
        node_id: &str,
        capacity: Option<plexspaces_proto::node::v1::NodeCapacity>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Start gossip protocol (if enabled and not using shared DB)
    ///
    /// ## Purpose
    /// Starts background task that periodically exchanges node info with random
    /// subset of nodes for liveness tracking.
    fn start_gossip_protocol(&self);
    
    /// Stop gossip protocol
    fn stop_gossip_protocol(&self);
    
    /// Check if gossip protocol is running
    fn is_gossip_running(&self) -> bool;
    
    /// Get cache statistics (for observability)
    async fn cache_stats(&self) -> (usize, usize, std::time::Duration); // (cache_size, hits, ttl)
}


