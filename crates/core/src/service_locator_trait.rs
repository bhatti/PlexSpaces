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

use std::any::Any;
use std::sync::Arc;
use async_trait::async_trait;

use crate::{ActorRegistry, VirtualActorManager, ReplyWaiterRegistry, Service};
use crate::actor_context::{ActorService, ChannelService, TupleSpaceProvider, ObjectRegistry};
use crate::actor_trait::MessageSender;
use crate::monitoring::{NodeMetricsAccessor, NodeConnectionInfo};
use crate::RequestContext;
use crate::JournalStorage;
use crate::KeyValueStore;
use crate::LockManager;
use crate::facet_service_wrapper::{FacetManagerServiceWrapper, FacetRegistryServiceWrapper};
use crate::behavior_factory::BehaviorRegistry;
use crate::grpc_connection_manager::GrpcConnectionManager;

/// Trait for service registration and retrieval
///
/// ## Purpose
/// Provides centralized service registration and retrieval interface.
/// Concrete implementation is in `plexspaces-services` crate.
#[async_trait]
pub trait ServiceLocator: Send + Sync {
    /// Register a service by type
    async fn register_service<T: Service + 'static>(&self, service: Arc<T>)
    where Self: Sized;
    
    /// Get a service by type
    async fn get_service<T: Service + 'static>(&self) -> Option<Arc<T>>
    where Self: Sized;
    
    /// Register a service by name
    async fn register_service_by_name<T: Service + 'static>(&self, name: &str, service: Arc<T>)
    where Self: Sized;
    
    /// Get a service by name
    async fn get_service_by_name<T: Service + 'static>(&self, name: &str) -> Option<Arc<T>>
    where Self: Sized;
    
    /// Get ActorRegistry
    async fn actor_registry(&self) -> Option<Arc<ActorRegistry>>;
    
    /// Get VirtualActorManager
    async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>>;
    
    /// Get ReplyWaiterRegistry
    async fn reply_waiter_registry(&self) -> Option<Arc<ReplyWaiterRegistry>>;
    
    /// Get ActorFactory (returns Arc<dyn Any> for type erasure)
    async fn get_actor_factory(&self) -> Option<Arc<dyn Any + Send + Sync>>;
    
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
    
    /// Get node config
    async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig>;
    
    /// Register node config
    async fn register_node_config(&self, config: plexspaces_proto::node::v1::NodeConfig);
    
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
    
    /// Create RequestContext for internal/system operations using NodeConfig defaults
    ///
    /// ## Purpose
    /// Creates a RequestContext for internal operations using default tenant_id and namespace
    /// from NodeConfig. This ensures system operations use configured defaults rather than
    /// hardcoded values.
    ///
    /// ## Note
    /// This should only be used for internal purposes like object registry lookups,
    /// not for main application methods that should use RequestContext from gRPC requests.
    ///
    /// ## Returns
    /// RequestContext with defaults from NodeConfig, or empty strings if NodeConfig not available
    async fn request_context_for_system_operations(&self) -> RequestContext;
    
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
    /// Uses request_context_for_system_operations for internal ObjectRegistry lookups.
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
    /// `message_sender` uses `plexspaces_core::MessageSender` for proper trait-based design.
    /// The wasm-runtime implementation will need to adapt between the core MessageSender
    /// and its internal MessageSender trait if needed.
    async fn instantiate(
        &self,
        module: std::sync::Arc<dyn std::any::Any + Send + Sync>,
        actor_id: String,
        initial_state: &[u8],
        config: std::sync::Arc<dyn std::any::Any + Send + Sync>,
        channel_service: Option<std::sync::Arc<dyn ChannelService>>,
        message_sender: Option<std::sync::Arc<dyn MessageSender>>,
        tuplespace_provider: Option<std::sync::Arc<dyn TupleSpaceProvider>>,
        keyvalue_store: Option<std::sync::Arc<dyn KeyValueStore>>,
        process_group_registry: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,
        lock_manager: Option<std::sync::Arc<dyn LockManager>>,
        object_registry: Option<std::sync::Arc<dyn ObjectRegistry>>,
        journal_storage: Option<std::sync::Arc<dyn JournalStorage>>,
        blob_service: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,
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

/// Trait for ServiceLocator initialization
///
/// ## Purpose
/// Provides a method to initialize all default services in a ServiceLocator.
/// This is the centralized initialization logic that can be called from
/// `create_default_service_locator` or `Node::initialize_services`.
#[async_trait]
pub trait ServiceLocatorInitialization: ServiceLocator {
    /// Initialize default services in this ServiceLocator
    ///
    /// ## Purpose
    /// Populates the ServiceLocator with all default services needed for a node.
    /// This is the centralized initialization logic that can be called from
    /// `create_default_service_locator` or `Node::initialize_services`.
    ///
    /// ## Arguments
    /// * `node_id` - Node ID for services (defaults to "test-node" if None)
    /// * `node_config` - Optional NodeConfig (if None, will be created from release_config.node or defaults)
    /// * `release_config` - Optional ReleaseSpec (if provided, node_config will be extracted from release_config.node)
    async fn initialize_services(
        &self,
        node_id: Option<String>,
        node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
        release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
    );
}

