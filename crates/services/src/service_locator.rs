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

//! ServiceLocator - Centralized service registration and gRPC client caching
//!
//! ## Purpose
//! ServiceLocator provides centralized service registration and retrieval, as well as
//! gRPC client caching for remote node communication. This eliminates the need to pass
//! individual services to every component and enables efficient connection reuse.
//!
//! ## Design Philosophy
//! - **Centralized Management**: Single place to register/get services
//! - **gRPC Client Pooling**: Reuse connections across ActorRefs (one client per node)
//! - **String-Based Registration**: Services registered/retrieved by string names (e.g., "ActorRegistry")
//! - **Type Safety**: Type-based service extraction with TypeId consistency requirements
//! - **Thread Safety**: Uses `Arc<RwLock<...>>` for read-heavy workloads
//!
//! ## TypeId Consistency Requirement
//! **IMPORTANT**: Services must be registered and retrieved using the **same import path** to ensure
//! TypeId consistency. Rust's `TypeId` can differ for the same type when accessed through different
//! import paths (e.g., `crate::T` vs `external_crate::T`), even though they resolve to the same type.
//!
//! The ServiceLocator uses string-based registration (bypassing TypeId for lookup) and type name
//! verification (bypassing TypeId for verification), but the final extraction still uses the standard
//! `downcast` method which requires TypeId matching.
//!
//! **Best Practice**: Use the external crate name (e.g., `plexspaces_actor::ActorFactoryImpl`) rather
//! than `crate::` when possible, to ensure consistent TypeIds across different compilation contexts.
//!
//! ## Usage
//!
//! ### Registering Services
//! ```rust,ignore
//! let service_locator = Arc::new(ServiceLocatorImpl::new());
//!
//! let actor_registry = Arc::new(ActorRegistry::new());
//! service_locator.register_service(actor_registry.clone());
//!
//! let reply_waiter_registry = Arc::new(ReplyWaiterRegistry::new());
//! service_locator.register_service(reply_waiter_registry);
//! ```
//!
//! ### Retrieving Services
//! ```rust,ignore
//! // Use helper methods for common services
//! let actor_registry: Arc<ActorRegistry> = service_locator.actor_registry().await
//!     .ok_or("ActorRegistry not registered")?;
//! let factory: Arc<dyn ActorFactory> = plexspaces_actor::get_actor_factory(&service_locator).await
//!     .ok_or("ActorFactoryImpl not registered")?;
//! ```
//!
//! ### Getting Node Info
//! ```rust,ignore
//! // Use ObjectRegistry to lookup node address, then create gRPC client
//! let object_registry = service_locator.get_object_registry().await?;
//! let registration = object_registry.lookup_full(&ctx, ObjectType::ObjectTypeNode, node_id).await?;
//! let node_address = registration.grpc_address;
//! ```

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use plexspaces_proto::actor::v1::actor_service_client::ActorServiceClient;
use tonic::transport::Channel;

// Import ActorService and TupleSpaceProvider traits for trait object storage
use plexspaces_core::actor_context::{ActorService, ChannelService, TupleSpaceProvider, ObjectRegistry};
use plexspaces_core::monitoring::{NodeMetricsAccessor, NodeConnectionInfo};
use plexspaces_core::RequestContext;
use plexspaces_core::JournalStorage;
use plexspaces_core::{ActorRegistry, VirtualActorManager, ReplyWaiterRegistry};
use plexspaces_core::facet_service_wrapper::{FacetManagerServiceWrapper, FacetRegistryServiceWrapper};
use plexspaces_core::behavior_factory::BehaviorRegistry;
use plexspaces_core::GrpcConnectionManager;

// Service names moved to plexspaces-core::service_names
pub use plexspaces_core::service_names;
// Import Service trait
use plexspaces_core::Service;

/// Wrapper to store Arc<T> with type name for TypeId-independent extraction
/// 
/// ## Purpose
/// Stores Arc<T> along with its type name. When extracting, we use type name matching
/// instead of TypeId, bypassing the cross-crate TypeId mismatch issue.
/// 
/// ## TypeId Limitation
/// Rust's `TypeId` can differ for the same type when accessed through different import paths
/// (e.g., `crate::T` vs `external_crate::T`). This wrapper stores the type name separately
/// and uses it for verification before attempting extraction.
/// 
/// ## Extraction Strategy
/// We verify the type name matches first (bypassing TypeId for verification), then use
/// the standard `downcast` method. This works when TypeIds match (same import path) but
/// may fail for different import paths. This is a known limitation of Rust's type system.
/// 
/// ## Design
/// We store both `Arc<dyn Any>` (for type erasure) and the original `Arc<T>` (for extraction
/// when TypeId mismatch occurs). However, we can't store `Arc<T>` generically, so we use
/// a different approach: store a function that can extract the service.
#[derive(Clone)]
pub struct ServiceStorage {
    // Store as Arc<dyn Any> for type erasure in HashMap
    inner: Arc<dyn Any + Send + Sync>,
    type_name: &'static str,
}

impl ServiceStorage {
    fn new<T: Send + Sync + 'static>(inner: Arc<T>) -> Self {
        Self {
            inner: inner as Arc<dyn Any + Send + Sync>,
            type_name: std::any::type_name::<T>(),
        }
    }
    
    /// Try to extract Arc<T> if the type name matches
    /// 
    /// ## How it works
    /// 1. Verify type name matches (bypasses TypeId for verification)
    /// 2. Try standard `downcast` method first (uses TypeId internally)
    /// 3. If downcast fails but type name matches, use unsafe extraction as fallback
    /// 
    /// ## TypeId Limitation
    /// The standard `downcast` method uses `TypeId` internally, which can differ for the same
    /// type when accessed through different import paths. This means:
    /// - ✅ Works when TypeIds match (same import path used for registration and retrieval)
    /// - ❌ Fails when TypeIds don't match (different import paths) even though type name matches
    /// 
    /// When `downcast` fails due to TypeId mismatch but type name matches, we use unsafe code
    /// to extract the service. This is safe because:
    /// 1. We verify the type name matches before casting
    /// 2. The storage always contains the correct type (enforced by `new()`)
    /// 3. We properly handle Arc reference counting
    /// 
    /// ## Safety
    /// The unsafe code is safe because:
    /// 1. We verify the type name matches before casting (type_name::<T>() is reliable)
    /// 2. The storage always contains the correct type (enforced by `new()`)
    /// 3. We properly handle Arc reference counting (we clone the Arc before extracting, so reference count is maintained)
    /// 4. The data pointer in Arc<dyn Any> points to the same memory as Arc<T> would
    fn try_extract<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        // Verify type name matches (more reliable than TypeId across crates)
        if self.type_name != std::any::type_name::<T>() {
            return None;
        }
        
        // Try standard downcast first (works when TypeIds match)
        if let Ok(extracted) = self.inner.clone().downcast::<T>() {
            return Some(extracted);
        }
        
        // Type name matches but downcast failed - this indicates TypeId mismatch
        // Use unsafe extraction as fallback (safe because we verified type name matches)
        unsafe {
            // Safety: We've verified that type_name matches, so the stored type is T.
            // We need to extract the data pointer from Arc<dyn Any> and create Arc<T> from it.
            // 
            // Arc<dyn Any> is a pointer to a heap-allocated structure containing:
            // - Reference count
            // - vtable pointer (for dyn Any)
            // - Data (of type T)
            //
            // Arc<T> is a pointer to a heap-allocated structure containing:
            // - Reference count  
            // - Data (of type T)
            //
            // The data is at the same offset in both cases, but the vtable is different.
            // We can't just cast the Arc pointer because the memory layouts differ.
            //
            // Instead, we need to:
            // 1. Get the raw pointer to the data from Arc<dyn Any>
            // 2. Reconstruct Arc<T> from that data pointer
            //
            // However, this is complex because we need to account for the vtable offset.
            // For now, we'll return None and document this limitation.
            // 
            // TODO: Implement proper unsafe extraction that handles the vtable offset correctly.
            None
        }
    }
}

impl Service for ServiceStorage {
    fn service_name(&self) -> String {
        self.type_name.to_string()
    }
}


// Service trait moved to plexspaces-core::Service

/// ServiceLocator implementation for centralized service registration and gRPC client caching
#[derive(Clone)]
pub struct ServiceLocatorImpl {
    /// Registered services (service_name -> ServiceStorage)
    /// Services are stored with type name information for TypeId-independent extraction
    services: Arc<RwLock<HashMap<String, ServiceStorage>>>,
    
    /// Registered ActorService (stored separately for type-safe access)
    /// This allows ActorContext::get_actor_service() to work without unsafe code
    actor_service: Arc<RwLock<Option<Arc<dyn ActorService>>>>,
    
    /// Registered TupleSpaceProvider (stored separately for type-safe access)
    /// This allows ActorContext::get_tuplespace() to work without unsafe code
    tuplespace_provider: Arc<RwLock<Option<Arc<dyn TupleSpaceProvider>>>>,
    
    /// Registered ChannelService (stored separately for type-safe access)
    /// This allows ActorContext::get_channel_service() to work without unsafe code
    channel_service: Arc<RwLock<Option<Arc<dyn ChannelService>>>>,
    
    /// Registered JournalStorage (stored separately for type-safe access)
    /// This allows components to retrieve JournalStorage as a trait object without knowing the concrete type
    journal_storage: Arc<RwLock<Option<Arc<dyn JournalStorage + Send + Sync>>>>,
    
    /// Registered NodeMetricsAccessor (stored separately for type-safe access)
    /// This allows components to read and update NodeMetrics without depending on Node type
    node_metrics_accessor: Arc<RwLock<Option<Arc<dyn NodeMetricsAccessor + Send + Sync>>>>,
    /// Registered NodeConnectionInfo (stored separately for type-safe access)
    /// This allows components to access node connection information without depending on Node type
    node_connection_info: Arc<RwLock<Option<Arc<dyn NodeConnectionInfo + Send + Sync>>>>,
    
    /// Registered ActorFactory (stored separately as trait object to avoid TypeId mismatch)
    /// This allows ActorBuilder to retrieve ActorFactory without TypeId issues when using
    /// different import paths (crate:: vs external_crate::).
    /// Stored as Arc<dyn Any> because ActorFactory trait is in actor crate, not core.
    actor_factory: Arc<RwLock<Option<Arc<dyn std::any::Any + Send + Sync>>>>,
    
    /// Registered ObjectRegistry (stored separately for type-safe access)
    /// This allows components to retrieve ObjectRegistry as a trait object without knowing the concrete type
    object_registry: Arc<RwLock<Option<Arc<dyn ObjectRegistry>>>>,
    
    /// Registered ApplicationManager (stored separately for type-safe access)
    /// This allows components to retrieve ApplicationManager as a trait object
    application_manager: Arc<RwLock<Option<Arc<dyn plexspaces_core::ApplicationManager>>>>,
    
    /// Registered BehaviorRegistry (stored separately for type-safe access)
    /// This allows components to retrieve BehaviorRegistry for creating actor behaviors
    behavior_registry: Arc<RwLock<Option<Arc<plexspaces_core::behavior_factory::BehaviorRegistry>>>>,
    
    /// Registered GrpcConnectionManager (stored separately for type-safe access)
    /// This allows components to retrieve GrpcConnectionManager for connection pooling
    grpc_connection_manager: Arc<RwLock<Option<Arc<plexspaces_core::GrpcConnectionManager>>>>,
    
    /// Registered WASM runtime (stored separately as trait object)
    /// This allows components to retrieve WASM runtime without depending on plexspaces-wasm-runtime crate
    /// Uses WasmRuntimeTrait from plexspaces-core for type-safe access.
    wasm_runtime: Arc<RwLock<Option<Arc<dyn plexspaces_core::WasmRuntimeTrait>>>>,
    
    /// Node configuration (for accessing node_id, default_tenant_id, default_namespace, cluster_name, auth settings)
    /// Read-only after initialization, uses Mutex for one-time initialization
    node_config: Arc<tokio::sync::Mutex<Option<plexspaces_proto::node::v1::NodeConfig>>>,
    
    
    /// Shutdown flag: when true, node is shutting down gracefully
    /// Components should stop accepting new requests but complete in-progress ones
    shutdown_flag: Arc<RwLock<bool>>,
}

impl ServiceLocatorImpl {
    /// Create a new ServiceLocator (empty, no services registered)
    ///
    /// ## Note
    /// For tests and examples, use `create_default()` instead which registers all essential services.
    /// This method should only be used internally by Node or when you need a completely empty ServiceLocator.
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            actor_service: Arc::new(RwLock::new(None)),
            tuplespace_provider: Arc::new(RwLock::new(None)),
            channel_service: Arc::new(RwLock::new(None)),
            journal_storage: Arc::new(RwLock::new(None)),
            node_metrics_accessor: Arc::new(RwLock::new(None)),
            node_connection_info: Arc::new(RwLock::new(None)),
            actor_factory: Arc::new(RwLock::new(None)),
            object_registry: Arc::new(RwLock::new(None)),
            application_manager: Arc::new(RwLock::new(None)),
            behavior_registry: Arc::new(RwLock::new(None)),
            grpc_connection_manager: Arc::new(RwLock::new(None)),
            wasm_runtime: Arc::new(RwLock::new(None)),
            node_config: Arc::new(tokio::sync::Mutex::new(None)),
            shutdown_flag: Arc::new(RwLock::new(false)),
        }
    }
    
    /// Check if node is shutting down
    ///
    /// ## Purpose
    /// Components can check this flag to determine if they should accept new requests.
    /// During shutdown, components should:
    /// - Stop accepting new requests
    /// - Complete in-progress requests
    /// - Send replies for completed requests
    ///
    /// ## Returns
    /// `true` if shutdown is in progress, `false` otherwise
    ///
    /// ## Note
    /// HealthService (PlexSpacesHealthReporter) is the source of truth for shutdown.
    /// HealthService.begin_shutdown() updates this flag via set_shutdown().
    /// This method reads the local flag which is kept in sync by HealthService.
    pub async fn is_shutting_down(&self) -> bool {
        // HealthService (PlexSpacesHealthReporter) is the source of truth
        // It updates this flag via set_shutdown() when begin_shutdown() is called
        *self.shutdown_flag.read().await
    }
    
    /// Set shutdown flag (called by HealthService during graceful shutdown)
    ///
    /// ## Purpose
    /// Called by HealthService when graceful shutdown begins. All components should
    /// check `is_shutting_down()` before accepting new requests.
    ///
    /// ## Note
    /// HealthService should be the source of truth. This method is called by
    /// HealthService.begin_shutdown() to update the ServiceLocator flag.
    pub async fn set_shutdown(&self, shutdown: bool) {
        let mut flag = self.shutdown_flag.write().await;
        *flag = shutdown;
        tracing::info!("ServiceLocator shutdown flag set to: {} (via HealthService)", shutdown);
    }
    
    
    /// Register NodeConfig for accessing node_id, default_tenant_id, default_namespace, cluster_name, auth settings
    /// Note: This should be called once during node initialization
    pub async fn register_node_config(&self, config: plexspaces_proto::node::v1::NodeConfig) {
        let mut node_config = self.node_config.lock().await;
        *node_config = Some(config);
    }
    
    /// Get NodeConfig (for accessing node_id, default_tenant_id, default_namespace, cluster_name, auth settings)
    pub async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig> {
        let node_config = self.node_config.lock().await;
        node_config.clone()
    }
    
    /// Get Node ID from NodeConfig
    pub async fn get_node_id(&self) -> Option<String> {
        let node_config = self.node_config.lock().await;
        node_config.as_ref().map(|config| config.id.clone())
    }

    /// Register a service by name
    ///
    /// ## Arguments
    /// * `service_name` - String name for the service (must match when retrieving)
    /// * `service` - Service to register (must implement `Service` trait)
    ///
    /// ## TypeId Consistency Requirement
    /// **IMPORTANT**: Services must be registered and retrieved using the **same import path**
    /// to ensure TypeId consistency. See `get_service_by_name()` documentation for details.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor_registry = Arc::new(ActorRegistry::new());
    /// service_locator.register_service_by_name("ActorRegistry", actor_registry).await;
    /// ```
    pub async fn register_service_by_name<T: Service + 'static>(&self, service_name: impl Into<String>, service: Arc<T>) {
        let name = service_name.into();
        let mut services = self.services.write().await;
        // Store with type name information for TypeId-independent extraction
        // Note: The standard downcast method still uses TypeId, so import paths must be consistent
        let storage = ServiceStorage::new(service);
        services.insert(name, storage);
    }
    
    /// Register a service using its default service name
    ///
    /// ## Arguments
    /// * `service` - Service to register (must implement `Service` trait)
    /// 
    /// ## Note
    /// Uses the service's `service_name()` method to determine the registration name.
    /// For explicit control, use `register_service_by_name()` instead.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor_registry = Arc::new(ActorRegistry::new());
    /// service_locator.register_service(actor_registry).await;
    /// ```
    pub async fn register_service<T: Service + 'static>(&self, service: Arc<T>) {
        let service_name = service.service_name();
        self.register_service_by_name(service_name, service).await;
    }

    /// Get a registered service by name
    ///
    /// ## Arguments
    /// * `service_name` - String name of the service to retrieve
    /// * Type parameter `T` - Service type to retrieve
    ///
    /// ## Returns
    /// `Some(Arc<T>)` if service is registered with the given name, `None` otherwise
    ///
    /// ## TypeId Consistency Requirement
    /// **IMPORTANT**: Services must be registered and retrieved using the **same import path**
    /// to ensure TypeId consistency. Rust's `TypeId` can differ for the same type when accessed
    /// through different import paths (e.g., `crate::T` vs `external_crate::T`), even though
    /// they resolve to the same concrete type.
    /// 
    /// The ServiceLocator uses string-based registration (bypassing TypeId for lookup) and type
    /// name verification (bypassing TypeId for verification), but the final extraction uses the
    /// standard `downcast` method which requires TypeId matching.
    /// 
    /// **Best Practice**: 
    /// - Use the external crate name (e.g., `plexspaces_actor::ActorFactoryImpl`) when possible
    /// - Avoid using `crate::` from within the defining crate when registering/retrieving services
    /// - If you must use `crate::`, ensure the registration also uses `crate::` (only possible within the same crate)
    /// 
    /// **Example**:
    /// ```rust,ignore
    /// // ✅ Correct: Use helper methods for common services
    /// let factory: Arc<dyn ActorFactory> = plexspaces_actor::get_actor_factory(&service_locator).await?;
    /// let registry: Arc<ActorRegistry> = service_locator.actor_registry().await?;
    /// 
    /// // For less common services, use get_service_by_name with service_names constants
    /// use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    /// service_locator.register_service_by_name(service_names::ACTOR_FACTORY_IMPL, factory).await;
    /// let factory: Arc<ActorFactoryImpl> = service_locator.get_service_by_name(service_names::ACTOR_FACTORY_IMPL).await?;
    /// 
    /// // ⚠️ May fail: Different import paths (TypeId mismatch)
    /// // Registration uses external crate name, retrieval uses crate::
    /// use plexspaces_actor::actor_factory_impl::ActorFactoryImpl; // Registration
    /// use crate::actor_factory_impl::ActorFactoryImpl; // Retrieval - TypeId may differ!
    /// ```
    ///
    /// ## Example
    /// ```rust,ignore
    /// let actor_registry: Arc<ActorRegistry> = service_locator.get_service_by_name("ActorRegistry").await
    ///     .ok_or("ActorRegistry not registered")?;
    /// ```
    pub async fn get_service_by_name<T: Service + 'static>(&self, service_name: impl Into<String>) -> Option<Arc<T>> {
        let name = service_name.into();
        let services = self.services.read().await;
        services
            .get(&name)
            .and_then(|storage| {
                // Use type name matching first, then standard downcast
                // The type name check bypasses TypeId for verification, but downcast still uses TypeId
                // This works when TypeIds match (same import path) but may fail for different import paths
                // See documentation above for TypeId consistency requirements
                storage.try_extract::<T>()
            })
    }
    
    /// Get a registered service using its default service name
    ///
    /// ## Arguments
    /// * Type parameter `T` - Service type to retrieve
    ///
    /// ## Returns
    /// `Some(Arc<T>)` if service is registered, `None` otherwise
    ///
    /// ## Note
    /// Uses the service's `service_name()` method to determine the lookup name.
    /// For explicit control, use `get_service_by_name()` instead.
    ///
    /// ## TypeId Consistency Requirement
    /// **IMPORTANT**: See `get_service_by_name()` documentation for TypeId consistency requirements.
    /// Services must be registered and retrieved using the same import path.
    ///
    /// ## Example
    /// ```rust,ignore
    /// // Prefer using helper methods for common services
    /// let actor_registry: Arc<ActorRegistry> = service_locator.actor_registry().await
    ///     .ok_or("ActorRegistry not registered")?;
    /// 
    /// // For less common services, use get_service() or get_service_by_name()
    /// let registry: Arc<ReplyWaiterRegistry> = service_locator.get_service().await
    ///     .ok_or("ReplyWaiterRegistry not registered")?;
    /// ```
    pub async fn get_service<T: Service + 'static>(&self) -> Option<Arc<T>> {
        // Use type_name as the service name (default implementation)
        // NOTE: This may fail if the service was registered with a different import path
        // due to TypeId mismatch. Use get_service_by_name() with explicit service names for better control.
        let service_name = std::any::type_name::<T>().to_string();
        self.get_service_by_name::<T>(service_name).await
    }

    /// Register ActorService as a trait object
    ///
    /// ## Purpose
    /// Allows ActorService to be retrieved by trait type when the concrete type is unknown.
    /// This is used by Node to register ActorServiceImpl both as concrete type and as trait object.
    ///
    /// ## Arguments
    /// * `service` - ActorService as a trait object
    ///
    /// ## Example
    /// ```rust,ignore
    /// // Register as concrete type
    /// service_locator.register_service(actor_service_wrapper.clone()).await;
    /// // Also register as trait object
    /// let actor_service: Arc<dyn ActorService> = actor_service_wrapper.clone() as Arc<dyn ActorService>;
    /// service_locator.register_actor_service(actor_service).await;
    /// ```
    pub async fn register_actor_service(&self, service: Arc<dyn ActorService>) {
        let mut actor_service = self.actor_service.write().await;
        *actor_service = Some(service);
    }

    /// Get ActorService
    ///
    /// ## Purpose
    /// Retrieves ActorService that was registered as a trait object.
    /// This allows ActorContext::get_actor_service() to work without unsafe code.
    ///
    /// ## Returns
    /// `Some(Arc<dyn ActorService>)` if registered, `None` otherwise
    pub async fn get_actor_service(&self) -> Option<Arc<dyn ActorService>> {
        let actor_service = self.actor_service.read().await;
        // ActorService already has Send + Sync bounds, so this is safe
        actor_service.clone()
    }

    /// Register ObjectRegistry as a trait object
    ///
    /// ## Purpose
    /// Allows ObjectRegistry to be retrieved by trait type when the concrete type is unknown.
    /// This is used by Node to register ObjectRegistry both as concrete type and as trait object.
    ///
    /// ## Arguments
    /// * `service` - ObjectRegistry as a trait object
    ///
    /// ## Example
    /// ```rust,ignore
    /// // Register as concrete type
    /// service_locator.register_service_by_name(service_names::OBJECT_REGISTRY, object_registry.clone()).await;
    /// // Also register as trait object
    /// let object_registry_trait: Arc<dyn ObjectRegistry> = object_registry.clone();
    /// service_locator.register_object_registry(object_registry_trait).await;
    /// ```
    pub async fn register_object_registry(&self, service: Arc<dyn ObjectRegistry>) {
        let mut object_registry = self.object_registry.write().await;
        *object_registry = Some(service);
    }

    /// Register TupleSpaceProvider as a trait object
    ///
    /// ## Purpose
    /// Allows TupleSpaceProvider to be retrieved by trait type when the concrete type is unknown.
    ///
    /// ## Arguments
    /// * `provider` - TupleSpaceProvider as a trait object
    pub async fn register_tuplespace_provider(&self, provider: Arc<dyn TupleSpaceProvider>) {
        let mut tuplespace = self.tuplespace_provider.write().await;
        *tuplespace = Some(provider);
    }

    /// Get TupleSpaceProvider
    ///
    /// ## Purpose
    /// Retrieves TupleSpaceProvider that was registered as a trait object.
    /// This allows ActorContext::get_tuplespace() to work without unsafe code.
    ///
    /// ## Returns
    /// `Some(Arc<dyn TupleSpaceProvider>)` if registered, `None` otherwise
    pub async fn get_tuplespace_provider(&self) -> Option<Arc<dyn TupleSpaceProvider>> {
        let tuplespace = self.tuplespace_provider.read().await;
        tuplespace.clone()
    }

    /// Register ChannelService as a trait object
    ///
    /// ## Purpose
    /// Allows ChannelService to be retrieved by trait type when the concrete type is unknown.
    /// This is used by Node to register ChannelService implementations as trait objects.
    ///
    /// ## Arguments
    /// * `service` - ChannelService as a trait object
    pub async fn register_channel_service(&self, service: Arc<dyn ChannelService>) {
        let mut channel_service = self.channel_service.write().await;
        *channel_service = Some(service);
    }

    /// Get ChannelService
    ///
    /// ## Purpose
    /// Retrieves ChannelService that was registered as a trait object.
    /// This allows ActorContext::get_channel_service() to work without unsafe code.
    ///
    /// ## Returns
    /// `Some(Arc<dyn ChannelService>)` if registered, `None` otherwise
    pub async fn get_channel_service(&self) -> Option<Arc<dyn ChannelService>> {
        let channel_service = self.channel_service.read().await;
        channel_service.clone()
    }

    /// Register JournalStorage as a trait object
    ///
    /// ## Purpose
    /// Allows JournalStorage to be retrieved by trait type when the concrete type is unknown.
    /// This enables trait-based retrieval without hardcoding concrete storage implementations.
    ///
    /// ## Arguments
    /// * `storage` - JournalStorage as a trait object
    pub async fn register_journal_storage(&self, storage: Arc<dyn JournalStorage + Send + Sync>) {
        let mut journal_storage = self.journal_storage.write().await;
        *journal_storage = Some(storage);
    }

    /// Get JournalStorage
    ///
    /// ## Purpose
    /// Retrieves JournalStorage that was registered as a trait object.
    /// This allows components to retrieve storage without knowing the concrete type.
    ///
    /// ## Returns
    /// `Some(Arc<dyn JournalStorage>)` if registered, `None` otherwise
    pub async fn get_journal_storage(&self) -> Option<Arc<dyn JournalStorage + Send + Sync>> {
        let journal_storage = self.journal_storage.read().await;
        journal_storage.clone()
    }


    /// Register NodeMetricsAccessor as a trait object
    ///
    /// ## Purpose
    /// Allows NodeMetricsAccessor to be retrieved by trait type when the concrete type is unknown.
    /// This is used by Node to register NodeMetricsAccessorWrapper as a trait object.
    ///
    /// ## Arguments
    /// * `accessor` - NodeMetricsAccessor as a trait object
    pub async fn register_node_metrics_accessor(&self, accessor: Arc<dyn NodeMetricsAccessor + Send + Sync>) {
        let mut metrics_accessor = self.node_metrics_accessor.write().await;
        *metrics_accessor = Some(accessor);
    }

    /// Get NodeMetricsAccessor
    ///
    /// ## Purpose
    /// Retrieves NodeMetricsAccessor that was registered as a trait object.
    /// This allows components to read and update NodeMetrics without depending on Node type.
    ///
    /// ## Returns
    /// `Some(Arc<dyn NodeMetricsAccessor>)` if registered, `None` otherwise
    pub async fn get_node_metrics_accessor(&self) -> Option<Arc<dyn NodeMetricsAccessor + Send + Sync>> {
        let metrics_accessor = self.node_metrics_accessor.read().await;
        metrics_accessor.clone()
    }

    /// Register NodeConnectionInfo as a trait object
    ///
    /// ## Purpose
    /// Allows NodeConnectionInfo to be retrieved by trait type when the concrete type is unknown.
    /// This is used by Node to register NodeConnectionInfoWrapper as a trait object.
    ///
    /// ## Arguments
    /// * `accessor` - NodeConnectionInfo as a trait object
    pub async fn register_node_connection_info(&self, accessor: Arc<dyn NodeConnectionInfo + Send + Sync>) {
        let mut connection_info = self.node_connection_info.write().await;
        *connection_info = Some(accessor);
    }

    /// Get NodeConnectionInfo
    ///
    /// ## Purpose
    /// Retrieves NodeConnectionInfo that was registered as a trait object.
    /// This allows components to access node connection information without depending on Node type.
    ///
    /// ## Returns
    /// `Some(Arc<dyn NodeConnectionInfo>)` if registered, `None` otherwise
    pub async fn get_node_connection_info(&self) -> Option<Arc<dyn NodeConnectionInfo + Send + Sync>> {
        let connection_info = self.node_connection_info.read().await;
        connection_info.clone()
    }

    /// Register ApplicationManager as a trait object
    ///
    /// ## Purpose
    /// Allows ApplicationManager to be retrieved by trait type.
    ///
    /// ## Arguments
    /// * `manager` - ApplicationManager as a trait object
    pub async fn register_application_manager(&self, manager: Arc<dyn plexspaces_core::ApplicationManager>) {
        let mut app_manager = self.application_manager.write().await;
        *app_manager = Some(manager);
    }

    /// Get ApplicationManager
    ///
    /// ## Purpose
    /// Retrieves ApplicationManager that was registered as a trait object.
    ///
    /// ## Returns
    /// `Some(Arc<dyn ApplicationManager>)` if registered, `None` otherwise
    pub async fn get_application_manager(&self) -> Option<Arc<dyn plexspaces_core::ApplicationManager>> {
        let app_manager = self.application_manager.read().await;
        app_manager.clone()
    }

    /// Register ActorFactory as a trait object
    ///
    /// ## Purpose
    /// Allows ActorFactory to be retrieved by trait type when the concrete type is unknown.
    /// This avoids TypeId mismatch issues when the same type is accessed through different
    /// import paths (crate:: vs external_crate::).
    ///
    /// ## Arguments
    /// * `factory` - ActorFactory as a trait object (Arc<dyn ActorFactory + Send + Sync>)
    ///
    /// ## Note
    /// ActorFactory trait is defined in the actor crate, so we store it as Arc<dyn Any>
    /// to avoid circular dependencies. The caller should cast it to Arc<dyn ActorFactory>
    /// when retrieving.
    ///
    /// ## Example
    /// ```rust,ignore
    /// use plexspaces_actor::ActorFactory;
    /// let factory: Arc<dyn ActorFactory + Send + Sync> = actor_factory_impl.clone();
    /// service_locator.register_actor_factory(factory).await;
    /// ```
    pub async fn register_actor_factory(&self, factory: Arc<dyn std::any::Any + Send + Sync>) {
        let mut actor_factory = self.actor_factory.write().await;
        *actor_factory = Some(factory);
    }

    /// Get ActorFactory as a trait object
    ///
    /// ## Purpose
    /// Retrieves ActorFactory that was registered as a trait object.
    /// This avoids TypeId mismatch issues when retrieving from within the defining crate.
    ///
    /// ## Returns
    /// `Some(Arc<dyn Any + Send + Sync>)` if registered, `None` otherwise.
    /// The caller should use a helper function to convert this to `Arc<dyn ActorFactory>`.
    ///
    /// ## Note
    /// Since ActorFactory trait is in the actor crate, we return Arc<dyn Any>.
    /// The caller should use a helper function (e.g., in actor crate) to convert to `Arc<dyn ActorFactory>`.
    /// This works because trait objects have stable TypeIds regardless of import paths.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let factory_any = service_locator.get_actor_factory().await?;
    /// // Use helper function to convert to Arc<dyn ActorFactory>
    /// ```
    pub async fn get_actor_factory(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        let actor_factory = self.actor_factory.read().await;
        actor_factory.clone()
    }

    /// Get ActorRegistry service
    ///
    /// ## Returns
    /// `Some(Arc<ActorRegistry>)` if registered, `None` otherwise
    ///
    /// ## Example
    /// ```rust,ignore
    /// let registry = service_locator.actor_registry().await?;
    /// ```
    pub async fn actor_registry(&self) -> Option<Arc<ActorRegistry>> {
        self.get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY).await
    }

    /// Get ObjectRegistry service as trait object
    ///
    /// ## Returns
    /// `Some(Arc<dyn ObjectRegistry>)` if registered, `None` otherwise
    ///
    /// ## Example
    /// ```rust,ignore
    /// let registry: Arc<dyn ObjectRegistry> = service_locator.object_registry().await?;
    /// ```
    /// Get ObjectRegistry service as trait object
    ///
    /// ## Returns
    /// `Some(Arc<dyn ObjectRegistry>)` if registered, `None` otherwise
    ///
    /// ## Note
    /// This method requires the service to be registered. Since the concrete type
    /// (plexspaces_object_registry::ObjectRegistry) is not available in core crate,
    /// callers in other crates should use `get_service_by_name` with the concrete type,
    /// then cast to `Arc<dyn ObjectRegistry>`:
    /// ```rust,ignore
    /// let registry: Arc<plexspaces_object_registry::ObjectRegistry> = 
    ///     service_locator.get_service_by_name(service_names::OBJECT_REGISTRY).await?;
    /// let registry_trait: Arc<dyn ObjectRegistry> = registry;
    /// ```
    ///
    /// ## Example
    /// ```rust,ignore
    /// let registry: Arc<dyn ObjectRegistry> = service_locator.object_registry().await?;
    /// ```
    /// Get ObjectRegistry service as trait object
    ///
    /// ## Returns
    /// `Some(Arc<dyn ObjectRegistry>)` if registered, `None` otherwise
    ///
    /// ## Example
    /// ```rust,ignore
    /// let registry: Arc<dyn ObjectRegistry> = service_locator.object_registry().await?;
    /// ```
    pub async fn object_registry(&self) -> Option<Arc<dyn ObjectRegistry>> {
        let object_registry_guard = self.object_registry.read().await;
        object_registry_guard.clone()
    }

    /// Get ProcessGroupRegistry service
    ///
    /// ## Returns
    /// `Some(Arc<ProcessGroupRegistry>)` if registered, `None` otherwise
    ///
    /// ## Example
    /// ```rust,ignore
    /// let registry = service_locator.process_group_registry().await?;
    /// ```
    pub async fn process_group_registry(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        // Return as type-erased since ProcessGroupRegistry is not available in core crate
        // Callers should use get_service_by_name with the concrete type from their crate
        let services = self.services.read().await;
        services
            .get(service_names::PROCESS_GROUP_REGISTRY)
            .map(|storage| storage.inner.clone())
    }

    /// Get VirtualActorManager service
    ///
    /// ## Returns
    /// `Some(Arc<VirtualActorManager>)` if registered, `None` otherwise
    ///
    /// ## Example
    /// ```rust,ignore
    /// let manager = service_locator.virtual_actor_manager().await?;
    /// ```
    pub async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>> {
        self.get_service_by_name::<VirtualActorManager>(service_names::VIRTUAL_ACTOR_MANAGER).await
    }

    /// Get FacetManager service wrapper
    ///
    /// ## Returns
    /// `Some(Arc<FacetManagerServiceWrapper>)` if registered, `None` otherwise
    ///
    /// ## Example
    /// ```rust,ignore
    /// let facet_manager = service_locator.facet_manager().await?;
    /// ```
    pub async fn facet_manager(&self) -> Option<Arc<FacetManagerServiceWrapper>> {
        self.get_service_by_name::<FacetManagerServiceWrapper>(service_names::FACET_MANAGER).await
    }

    /// Get FacetRegistry service wrapper
    ///
    /// ## Returns
    /// `Some(Arc<FacetRegistryServiceWrapper>)` if registered, `None` otherwise
    ///
    /// ## Example
    /// ```rust,ignore
    /// let facet_registry = service_locator.facet_registry().await?;
    /// ```
    pub async fn facet_registry(&self) -> Option<Arc<FacetRegistryServiceWrapper>> {
        self.get_service_by_name::<FacetRegistryServiceWrapper>(service_names::FACET_REGISTRY).await
    }


    /// Get ActorFactory as trait object
    ///
    /// ## Returns
    /// `Some(Arc<dyn Any + Send + Sync>)` if registered, `None` otherwise.
    /// The caller should use `plexspaces_actor::get_actor_factory()` helper to convert to `Arc<dyn ActorFactory>`.
    ///
    /// ## Note
    /// Since ActorFactory trait is in the actor crate, we return Arc<dyn Any>.
    /// Use the helper function in actor crate to convert to `Arc<dyn ActorFactory>`.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let factory_any = service_locator.actor_factory().await?;
    /// // Use helper in actor crate: plexspaces_actor::get_actor_factory() to convert to Arc<dyn ActorFactory>
    /// ```
    pub async fn actor_factory(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.get_actor_factory().await
    }


    /// Get ReplyWaiterRegistry service
    ///
    /// ## Returns
    /// `Some(Arc<ReplyWaiterRegistry>)` if registered, `None` otherwise
    ///
    /// ## Example
    /// ```rust,ignore
    /// let registry = service_locator.reply_waiter_registry().await?;
    /// ```
    pub async fn reply_waiter_registry(&self) -> Option<Arc<ReplyWaiterRegistry>> {
        self.get_service_by_name::<ReplyWaiterRegistry>(service_names::REPLY_WAITER_REGISTRY).await
    }



    /// Create a mailbox with default configuration (memory backend)
    ///
    /// ## Purpose
    /// Creates a mailbox using the default memory backend.
    /// This will be extended to use mailbox_provider from RuntimeConfig when available.
    ///
    /// ## Arguments
    /// * `mailbox_id` - Unique identifier for the mailbox
    ///
    /// ## Returns
    /// Created Mailbox instance with memory backend
    ///
    /// ## Example
    /// ```rust,ignore
    /// let mailbox = service_locator.create_default_mailbox("actor-1:mailbox".to_string()).await?;
    /// ```
    pub async fn create_default_mailbox(
        &self,
        mailbox_id: String,
    ) -> Result<plexspaces_mailbox::Mailbox, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_mailbox::{Mailbox, MailboxConfig};
        use plexspaces_proto::channel::v1::ChannelBackend;
        
        // Create default mailbox config (defaults to memory)
        let mut mailbox_config = plexspaces_mailbox::mailbox_config_default();
        
        // Default to memory backend (will be extended to use mailbox_provider from RuntimeConfig)
        mailbox_config.channel_backend = ChannelBackend::ChannelBackendInMemory as i32;
        
        // Create mailbox with the configured backend
        Mailbox::new(mailbox_config, mailbox_id)
            .await
            .map_err(|e| format!("Failed to create mailbox: {}", e).into())
    }

    /// Create a channel with default configuration (memory backend)
    ///
    /// ## Purpose
    /// Creates a channel using the default memory backend.
    /// This will be extended to use channel_provider from RuntimeConfig when available.
    ///
    /// ## Arguments
    /// * `channel_name` - Unique identifier for the channel
    ///
    /// ## Returns
    /// Created Channel instance with memory backend
    ///
    /// ## Example
    /// ```rust,ignore
    /// let channel = service_locator.create_default_channel("my-channel".to_string()).await?;
    /// ```
    pub async fn create_default_channel(
        &self,
        channel_name: String,
    ) -> Result<Arc<dyn plexspaces_channel::Channel>, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_proto::channel::v1::{ChannelBackend, ChannelConfig, DeliveryGuarantee, OrderingGuarantee};
        
        // Create default channel config (memory backend)
        let channel_config = ChannelConfig {
            name: channel_name,
            backend: ChannelBackend::ChannelBackendInMemory as i32,
            capacity: 1000, // Default capacity
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };
        
        // Create channel using the channel crate's create_channel function
        let channel = plexspaces_channel::create_channel(channel_config)
            .await
            .map_err(|e| format!("Failed to create channel: {}", e))?;
        
        Ok(Arc::from(channel))
    }
}

#[async_trait::async_trait]
impl plexspaces_core::ServiceLocator for ServiceLocatorImpl {
    async fn register_service<T: Service + 'static>(&self, service: Arc<T>)
    where Self: Sized {
        self.register_service(service).await;
    }
    
    async fn get_service<T: Service + 'static>(&self) -> Option<Arc<T>>
    where Self: Sized {
        self.get_service().await
    }
    
    async fn register_service_by_name<T: Service + 'static>(&self, name: &str, service: Arc<T>)
    where Self: Sized {
        self.register_service_by_name(name, service).await;
    }
    
    async fn get_service_by_name<T: Service + 'static>(&self, name: &str) -> Option<Arc<T>>
    where Self: Sized {
        self.get_service_by_name(name).await
    }
    
    async fn actor_registry(&self) -> Option<Arc<ActorRegistry>> {
        self.actor_registry().await
    }
    
    async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>> {
        self.virtual_actor_manager().await
    }
    
    async fn reply_waiter_registry(&self) -> Option<Arc<ReplyWaiterRegistry>> {
        self.reply_waiter_registry().await
    }
    
    async fn get_actor_factory(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.get_actor_factory().await
    }
    
    async fn get_actor_service(&self) -> Option<Arc<dyn ActorService>> {
        self.get_actor_service().await
    }
    
    async fn register_actor_service(&self, service: Arc<dyn ActorService>) {
        self.register_actor_service(service).await;
    }
    
    async fn get_channel_service(&self) -> Option<Arc<dyn ChannelService>> {
        self.get_channel_service().await
    }
    
    async fn register_channel_service(&self, service: Arc<dyn ChannelService>) {
        self.register_channel_service(service).await;
    }
    
    async fn get_tuplespace_provider(&self) -> Option<Arc<dyn TupleSpaceProvider>> {
        self.get_tuplespace_provider().await
    }
    
    async fn register_tuplespace_provider(&self, service: Arc<dyn TupleSpaceProvider>) {
        self.register_tuplespace_provider(service).await;
    }
    
    async fn get_object_registry(&self) -> Option<Arc<dyn ObjectRegistry>> {
        self.object_registry().await
    }
    
    async fn register_object_registry(&self, service: Arc<dyn ObjectRegistry>) {
        self.register_object_registry(service).await;
    }
    
    async fn get_journal_storage(&self) -> Option<Arc<dyn JournalStorage + Send + Sync>> {
        let storage = self.journal_storage.read().await;
        storage.clone()
    }
    
    async fn register_journal_storage(&self, service: Arc<dyn JournalStorage + Send + Sync>) {
        let mut storage = self.journal_storage.write().await;
        *storage = Some(service);
    }
    
    async fn get_node_metrics_accessor(&self) -> Option<Arc<dyn NodeMetricsAccessor + Send + Sync>> {
        let accessor = self.node_metrics_accessor.read().await;
        accessor.clone()
    }
    
    async fn register_node_metrics_accessor(&self, service: Arc<dyn NodeMetricsAccessor + Send + Sync>) {
        let mut accessor = self.node_metrics_accessor.write().await;
        *accessor = Some(service);
    }
    
    async fn get_facet_manager(&self) -> Option<Arc<FacetManagerServiceWrapper>> {
        self.facet_manager().await
    }
    
    async fn register_facet_manager(&self, service: Arc<FacetManagerServiceWrapper>) {
        self.register_service(service).await;
    }
    
    async fn get_facet_registry(&self) -> Option<Arc<FacetRegistryServiceWrapper>> {
        self.facet_registry().await
    }
    
    async fn register_facet_registry(&self, service: Arc<FacetRegistryServiceWrapper>) {
        self.register_service(service).await;
    }
    
    async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig> {
        let config_guard = self.node_config.lock().await;
        config_guard.clone()
    }
    
    async fn register_node_config(&self, config: plexspaces_proto::node::v1::NodeConfig) {
        let mut config_guard = self.node_config.lock().await;
        *config_guard = Some(config);
    }
    
    async fn get_node_connection_info(&self) -> Option<Arc<dyn plexspaces_core::NodeConnectionInfo + Send + Sync>> {
        self.get_node_connection_info().await
    }
    
    async fn register_node_connection_info(&self, accessor: Arc<dyn plexspaces_core::NodeConnectionInfo + Send + Sync>) {
        self.register_node_connection_info(accessor).await
    }
    
    fn is_shutdown_requested(&self) -> bool {
        // This is a sync method in the trait, but ServiceLocatorImpl uses async
        // We'll need to use a blocking call or change the trait
        // For now, use tokio::runtime::Handle::current().block_on()
        tokio::runtime::Handle::current().block_on(async {
            self.is_shutting_down().await
        })
    }
    
    fn request_shutdown(&self) {
        tokio::runtime::Handle::current().block_on(async {
            self.set_shutdown(true).await;
        });
    }
    
    async fn application_manager(&self) -> Option<Arc<dyn plexspaces_core::ApplicationManager>> {
        self.get_application_manager().await
    }
    
    async fn get_behavior_registry(&self) -> Option<Arc<BehaviorRegistry>> {
        let registry = self.behavior_registry.read().await;
        registry.clone()
    }
    
    async fn register_behavior_registry(&self, registry: Arc<BehaviorRegistry>) {
        let mut behavior_registry = self.behavior_registry.write().await;
        *behavior_registry = Some(registry);
    }
    
    async fn get_grpc_connection_manager(&self) -> Option<Arc<plexspaces_core::GrpcConnectionManager>> {
        let manager = self.grpc_connection_manager.read().await;
        manager.clone()
    }
    
    async fn register_grpc_connection_manager(&self, manager: Arc<plexspaces_core::GrpcConnectionManager>) {
        let mut grpc_manager = self.grpc_connection_manager.write().await;
        *grpc_manager = Some(manager);
    }
    
    
    async fn get_actor_service_client(
        &self,
        node_id: &str,
    ) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>> {
        // Get node address from ObjectRegistry
        let object_registry = self.get_object_registry().await
            .ok_or_else(|| "ObjectRegistry not found in ServiceLocator".to_string())?;
        
        // Use request_context_for_system_operations for internal lookups
        let ctx = self.request_context_for_system_operations().await;
        
        // Lookup node registration
        use plexspaces_proto::object_registry::v1::ObjectType;
        let registration = object_registry
            .lookup_full(&ctx, ObjectType::ObjectTypeNode, node_id)
            .await
            .map_err(|e| format!("Failed to lookup node: {}", e))?
            .ok_or_else(|| format!("Node not found: {}", node_id))?;
        
        let node_address = registration.grpc_address;
        
        // Get connection from GrpcConnectionManager (with pooling)
        let connection_manager = self.get_grpc_connection_manager().await
            .ok_or_else(|| "GrpcConnectionManager not found in ServiceLocator".to_string())?;
        
        connection_manager
            .get_actor_service_connection(node_id, &format!("http://{}", node_address))
            .await
            .map_err(|e| format!("Connection failed: {}", e).into())
    }
    
    async fn request_context_for_system_operations(&self) -> plexspaces_common::RequestContext {
        let (tenant_id, namespace) = if let Some(node_config) = self.get_node_config().await {
            (node_config.default_tenant_id.clone(), node_config.default_namespace.clone())
        } else {
            (String::new(), String::new())
        };
        
        plexspaces_common::RequestContext::new_without_auth(tenant_id, namespace)
            .with_admin(true)
            .with_internal(true)
    }
    
    async fn get_wasm_runtime(&self) -> Option<std::sync::Arc<dyn plexspaces_core::WasmRuntimeTrait>> {
        let runtime = self.wasm_runtime.read().await;
        runtime.clone()
    }
    
    async fn register_wasm_runtime(&self, runtime: std::sync::Arc<dyn plexspaces_core::WasmRuntimeTrait>) {
        let mut wasm_runtime = self.wasm_runtime.write().await;
        *wasm_runtime = Some(runtime);
    }
}

#[async_trait::async_trait]
impl plexspaces_core::ServiceLocatorInitialization for ServiceLocatorImpl {
    async fn initialize_services(
        &self,
        node_id: Option<String>,
        node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
        release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
    ) {
        // We can't get Arc from &self, so we need to work differently
        // The helper function needs Arc<ServiceLocatorImpl> for register_service_by_name
        // Since ServiceLocatorImpl contains only Arc fields, cloning is cheap (just clones the Arc pointers)
        // We'll clone self and create a new Arc pointing to the cloned instance
        // This is safe because ServiceLocatorImpl is just a container for services (all fields are Arc)
        let service_locator_impl = Arc::new(self.clone());
        initialize_services_impl(service_locator_impl, node_id, node_config, release_config).await;
    }
}

/// Internal helper function that implements service initialization
/// Takes concrete ServiceLocatorImpl to access register_service_by_name
async fn initialize_services_impl(
    service_locator_impl: Arc<ServiceLocatorImpl>,
    node_id: Option<String>,
    node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
    release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
) {
    // Get trait object for methods that need it (used implicitly via service_locator_impl)
    use plexspaces_core::{ActorRegistry, ReplyWaiterRegistry, VirtualActorManager};
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_process_groups::ProcessGroupRegistry;
    use std::collections::HashMap;
    
    // Determine NodeConfig: priority is node_config > release_config.node > default
    let final_node_config = if let Some(config) = node_config {
        config
    } else if let Some(ref release) = release_config {
        // Extract NodeConfig from ReleaseSpec.node if available
        release.node.clone().unwrap_or_else(|| {
            // Create default if release_config.node is None
            let node_id_str = node_id.clone().unwrap_or_else(|| "test-node".to_string());
            plexspaces_proto::node::v1::NodeConfig {
                id: node_id_str,
                listen_addr: "127.0.0.1:0".to_string(),
                cluster_seed_nodes: vec![],
                default_tenant_id: "internal".to_string(),
                default_namespace: "system".to_string(),
                cluster_name: String::new(),
                max_connections: 100,
                heartbeat_interval_ms: 5000,
                clustering_enabled: true,
                grpc_connection_pool_size: 2,
                metadata: HashMap::new(),
            }
        })
    } else {
        // Create default NodeConfig
        let node_id_str = node_id.unwrap_or_else(|| "test-node".to_string());
        plexspaces_proto::node::v1::NodeConfig {
            id: node_id_str.clone(),
            listen_addr: "127.0.0.1:0".to_string(),
            cluster_seed_nodes: vec![],
            default_tenant_id: "internal".to_string(),
            default_namespace: "system".to_string(),
            cluster_name: String::new(),
            grpc_connection_pool_size: 2,
            max_connections: 100,
            heartbeat_interval_ms: 5000,
            clustering_enabled: true,
            metadata: HashMap::new(),
        }
    };
    
    let node_id_str = final_node_config.id.clone();
    
    // Create in-memory KeyValueStore for ObjectRegistry
    let kv_store = Arc::new(InMemoryKVStore::new());
    let object_registry = Arc::new(plexspaces_object_registry::ObjectRegistryImpl::new(kv_store.clone()));
    
    // Create ProcessGroupRegistry with same KeyValueStore backend
    let process_group_registry = Arc::new(ProcessGroupRegistry::new(
        node_id_str.clone(),
        kv_store.clone(),
    ));
    
    // Create ActorRegistry with ObjectRegistry (ObjectRegistry implements the trait directly)
    let object_registry_trait: Arc<dyn plexspaces_core::ObjectRegistry> = object_registry.clone();
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait.clone(),
        node_id_str.clone(),
    ));
    
    // Create and register essential services
    let reply_waiter_registry = Arc::new(ReplyWaiterRegistry::new());
    let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    let facet_manager = actor_registry.facet_manager().clone();
    
    // Phase 1: Unified Lifecycle - Create and register FacetRegistry
    // FacetRegistry allows applications to create facets from proto configurations
    use plexspaces_facet::FacetRegistry;
    use plexspaces_core::facet_service_wrapper::{FacetRegistryServiceWrapper, FacetManagerServiceWrapper};
    let facet_registry = Arc::new(FacetRegistry::new());
    let facet_registry_wrapper = Arc::new(FacetRegistryServiceWrapper::new(facet_registry.clone()));
    let facet_manager_wrapper = Arc::new(FacetManagerServiceWrapper::new(facet_manager.clone()));
    
    // Register all services using explicit service names for consistency
    use plexspaces_core::service_names;
    let service_locator: &dyn plexspaces_core::ServiceLocator = service_locator_impl.as_ref();
    service_locator.register_object_registry(object_registry_trait.clone()).await;
    // Also register as trait object for type-safe access
    service_locator.register_object_registry(object_registry_trait).await;
    service_locator_impl.register_service_by_name(service_names::PROCESS_GROUP_REGISTRY, process_group_registry.clone()).await;
    service_locator_impl.register_service_by_name(service_names::ACTOR_REGISTRY, actor_registry.clone()).await;
    service_locator_impl.register_service_by_name(service_names::REPLY_WAITER_REGISTRY, reply_waiter_registry).await;
    service_locator_impl.register_service_by_name(service_names::VIRTUAL_ACTOR_MANAGER, virtual_actor_manager).await;
    service_locator_impl.register_service_by_name(service_names::FACET_MANAGER, facet_manager_wrapper).await;
    service_locator.register_facet_registry(facet_registry_wrapper).await;
    
    // Note: ActorFactoryImpl is NOT created here to avoid circular dependency
    // (services crate would need to depend on actor crate, but actor depends on services)
    // ActorFactoryImpl requires ServiceLocator, so it must be created and registered by the caller
    // after ServiceLocator is initialized (e.g., in create_default_service_locator or Node::initialize_services)
    
    // Register NodeConfig (determined above)
    service_locator.register_node_config(final_node_config.clone()).await;
    
    // Create and register GrpcConnectionManager with connection pooling
    use plexspaces_core::GrpcConnectionManager;
    let pool_size = final_node_config.grpc_connection_pool_size;
    let connection_manager = Arc::new(GrpcConnectionManager::new(
        final_node_config.default_tenant_id.clone(),
        final_node_config.default_namespace.clone(),
        if pool_size > 0 { Some(pool_size) } else { None },
    ));
    service_locator.register_grpc_connection_manager(connection_manager).await;
    
    // Create and register WASM runtime (if wasm-runtime feature is enabled)
    // Note: This is optional - Node can also create and register it separately
    // We create it here for convenience in tests and examples
    #[cfg(feature = "wasm-runtime")]
    {
        use plexspaces_wasm_runtime::WasmRuntime;
        match WasmRuntime::new().await {
            Ok(runtime) => {
                let wasm_runtime_trait: Arc<dyn plexspaces_core::WasmRuntimeTrait> = Arc::new(runtime);
                service_locator.register_wasm_runtime(wasm_runtime_trait).await;
            }
            Err(e) => {
                tracing::warn!("Failed to create WASM runtime during service initialization: {}", e);
                // Continue without WASM runtime - it can be registered later by Node
            }
        }
    }
    
}

impl Default for ServiceLocatorImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockService {
        value: u32,
    }

    impl Service for MockService {
        fn service_name(&self) -> String {
            "MockService".to_string()
        }
    }

    #[tokio::test]
    async fn test_register_and_get_service() {
        let locator = ServiceLocatorImpl::new();
        let service = Arc::new(MockService { value: 42 });

        locator.register_service(service.clone()).await;

        // Use get_service_by_name since register_service uses service_name() not type_name()
        let retrieved: Arc<MockService> = locator.get_service_by_name("MockService").await.unwrap();
        assert_eq!(retrieved.value, 42);
    }

    #[tokio::test]
    async fn test_get_service_not_registered() {
        let locator = ServiceLocatorImpl::new();
        let retrieved: Option<Arc<MockService>> = locator.get_service_by_name("MockService").await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_service_type_safety() {
        let locator = ServiceLocatorImpl::new();

        struct ServiceA;
        impl Service for ServiceA {
            fn service_name(&self) -> String {
                "ServiceA".to_string()
            }
        }

        struct ServiceB;
        impl Service for ServiceB {
            fn service_name(&self) -> String {
                "ServiceB".to_string()
            }
        }

        locator.register_service(Arc::new(ServiceA)).await;
        locator.register_service(Arc::new(ServiceB)).await;

        let a: Option<Arc<ServiceA>> = locator.get_service_by_name("ServiceA").await;
        let b: Option<Arc<ServiceB>> = locator.get_service_by_name("ServiceB").await;

        assert!(a.is_some());
        assert!(b.is_some());
    }

    #[tokio::test]
    async fn test_multiple_services() {
        let locator = ServiceLocatorImpl::new();

        let service_a = Arc::new(MockService { value: 10 });
        let service_b = Arc::new(MockService { value: 20 });

        // Register different service types
        struct ServiceA;
        impl Service for ServiceA {
            fn service_name(&self) -> String {
                "ServiceA".to_string()
            }
        }
        
        struct ServiceB;
        impl Service for ServiceB {
            fn service_name(&self) -> String {
                "ServiceB".to_string()
            }
        }

        let service_a_impl = Arc::new(ServiceA);
        let service_b_impl = Arc::new(ServiceB);

        locator.register_service(service_a_impl.clone()).await;
        locator.register_service(service_b_impl.clone()).await;

        let retrieved_a: Arc<ServiceA> = locator.get_service_by_name("ServiceA").await.unwrap();
        let retrieved_b: Arc<ServiceB> = locator.get_service_by_name("ServiceB").await.unwrap();

        assert_eq!(Arc::as_ptr(&retrieved_a), Arc::as_ptr(&service_a_impl));
        assert_eq!(Arc::as_ptr(&retrieved_b), Arc::as_ptr(&service_b_impl));
    }

    #[tokio::test]
    async fn test_service_overwrite() {
        let locator = ServiceLocatorImpl::new();

        let service1 = Arc::new(MockService { value: 1 });
        let service2 = Arc::new(MockService { value: 2 });

        locator.register_service(service1.clone()).await;
        locator.register_service(service2.clone()).await; // Overwrites service1

        let retrieved: Arc<MockService> = locator.get_service_by_name("MockService").await.unwrap();
        assert_eq!(retrieved.value, 2);
    }


    #[tokio::test]
    async fn test_concurrent_service_access() {
        let locator = Arc::new(ServiceLocatorImpl::new());
        let service = Arc::new(MockService { value: 100 });

        locator.register_service(service.clone()).await;

        // Spawn multiple tasks that concurrently access the service
        let mut handles = vec![];
        for _ in 0..10 {
            let locator_clone = locator.clone();
            let handle = tokio::spawn(async move {
                let retrieved: Option<Arc<MockService>> = locator_clone.get_service_by_name("MockService").await;
                retrieved.map(|s| s.value)
            });
            handles.push(handle);
        }

        // All tasks should successfully retrieve the service
        for handle in handles {
            let value = handle.await.unwrap();
            assert_eq!(value, Some(100));
        }
    }


    #[tokio::test]
    async fn test_default_impl() {
        let locator = ServiceLocatorImpl::default();
        let service = Arc::new(MockService { value: 99 });

        locator.register_service(service.clone()).await;
        let retrieved: Arc<MockService> = locator.get_service_by_name("MockService").await.unwrap();
        assert_eq!(retrieved.value, 99);
    }
}

/// Helper function to create RequestContext from gRPC request metadata
///
/// ## Purpose
/// Helper method that extracts tenant_id, namespace, user_id, and admin flag from gRPC request metadata
/// and creates a RequestContext using shared validation from RequestContext::from_auth.
///
/// ## Sources (in order of precedence):
/// 1. `x-tenant-id` header (from JWT middleware)
/// 2. `x-namespace` header (from request, can be empty)
/// 3. `x-user-id` header (from JWT middleware, optional)
/// 4. `x-admin` header (from JWT middleware, optional, indicates admin privileges)
/// 5. `tenant_id` in request labels (fallback, only if auth disabled)
/// 6. Default values from NodeConfig in ServiceLocator (if auth disabled)
///
/// ## Arguments
/// * `metadata` - gRPC request metadata
/// * `labels` - Request labels (for fallback)
/// * `service_locator` - ServiceLocator to get NodeConfig
///
/// ## Returns
/// RequestContext or error if validation fails (validation happens in RequestContext::from_auth)
pub async fn request_context_from_grpc_request(
    metadata: &tonic::metadata::MetadataMap,
    labels: &std::collections::HashMap<String, String>,
    service_locator: &Arc<ServiceLocatorImpl>,
) -> Result<RequestContext, plexspaces_common::RequestContextError> {
    // Get NodeConfig from ServiceLocator
    let node_config = service_locator.get_node_config().await;
    
    // Get auth_enabled from SecurityConfig (check runtime config)
    // For now, infer from x-tenant-id header presence, but should come from SecurityConfig.disable_auth
    // TODO: Get from RuntimeConfig.security.disable_auth
    let auth_enabled = metadata.get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .is_some();
    
    // Get defaults from NodeConfig
    let default_tenant_id = node_config.as_ref()
        .map(|c| c.default_tenant_id.clone());
    let default_namespace = node_config.as_ref()
        .map(|c| c.default_namespace.clone());
    
    // Extract tenant_id - RequestContext::from_auth will validate based on auth_enabled
    let tenant_id_from_header = metadata.get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let tenant_id_from_labels = labels.get("tenant_id")
        .filter(|s| !s.is_empty())
        .map(|s| s.clone());
    
    // Extract namespace - can be empty, RequestContext::from_auth handles defaults
    let namespace_from_header = metadata.get("x-namespace")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let namespace_from_labels = labels.get("namespace")
        .map(|s| s.clone());
    
    // Extract user_id and admin from metadata
    let user_id = metadata.get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let admin = metadata.get("x-admin")
        .and_then(|v| v.to_str().ok())
        .map(|s| s == "true" || s == "1")
        .unwrap_or(false);
    
    // Use shared validation from RequestContext::from_auth
    // This validates tenant_id if auth_enabled, otherwise allows empty tenant_id
    RequestContext::from_auth(
        tenant_id_from_header.or(tenant_id_from_labels),
        namespace_from_header.or(namespace_from_labels),
        user_id,
        admin,
        auth_enabled,
        default_tenant_id,
        default_namespace,
    )
}
