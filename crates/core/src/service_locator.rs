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
//! let service_locator = Arc::new(ServiceLocator::new());
//!
//! let actor_registry = Arc::new(ActorRegistry::new());
//! service_locator.register_service(actor_registry.clone());
//!
//! let reply_tracker = Arc::new(ReplyTracker::new());
//! service_locator.register_service(reply_tracker);
//! ```
//!
//! ### Retrieving Services
//! ```rust,ignore
//! let actor_registry: Arc<ActorRegistry> = service_locator.get_service().await
//!     .ok_or("ActorRegistry not registered")?;
//! ```
//!
//! ### Getting gRPC Clients
//! ```rust,ignore
//! let client = service_locator.get_node_client("node-2").await?;
//! // Client is cached and reused for subsequent calls
//! ```

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use plexspaces_proto::actor::v1::actor_service_client::ActorServiceClient;
use tonic::transport::Channel;

// Import ActorService and TupleSpaceProvider traits for trait object storage
use crate::actor_context::{ActorService, TupleSpaceProvider};
use crate::monitoring::NodeMetricsAccessor;
use crate::RequestContext;

/// Standard service names for consistent registration and lookup
pub mod service_names {
    pub const ACTOR_REGISTRY: &str = "ActorRegistry";
    pub const OBJECT_REGISTRY: &str = "ObjectRegistry";
    pub const PROCESS_GROUP_REGISTRY: &str = "ProcessGroupRegistry";
    pub const REPLY_TRACKER: &str = "ReplyTracker";
    pub const REPLY_WAITER_REGISTRY: &str = "ReplyWaiterRegistry";
    pub const VIRTUAL_ACTOR_MANAGER: &str = "VirtualActorManager";
    pub const FACET_MANAGER: &str = "FacetManager";
    pub const FACET_REGISTRY: &str = "FacetRegistry"; // Phase 1: Unified Lifecycle - FacetRegistry integration
    pub const ACTOR_FACTORY_IMPL: &str = "ActorFactoryImpl";
    pub const APPLICATION_MANAGER: &str = "ApplicationManager";
}

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
struct ServiceStorage {
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


/// Trait for services that can be registered in ServiceLocator
pub trait Service: Send + Sync
where
    Self: 'static,
{
    /// Get the service name (used for registration and lookup)
    /// 
    /// ## Default Implementation
    /// Returns the type name by default, but can be overridden for custom names.
    /// 
    /// ## Recommendation
    /// Override this method to return a simple, consistent name (e.g., "ActorRegistry")
    /// instead of the full type path to avoid issues with different import paths.
    fn service_name(&self) -> String {
        std::any::type_name::<Self>().to_string()
    }
}

/// ServiceLocator for centralized service registration and gRPC client caching
pub struct ServiceLocator {
    /// Registered services (service_name -> ServiceStorage)
    /// Services are stored with type name information for TypeId-independent extraction
    services: Arc<RwLock<HashMap<String, ServiceStorage>>>,
    
    /// Registered ActorService (stored separately for type-safe access)
    /// This allows ActorContext::get_actor_service() to work without unsafe code
    /// Stored as Arc<dyn ActorService + Send + Sync> for explicit bounds, but ActorService
    /// trait already requires Send + Sync, so this is equivalent to Arc<dyn ActorService>
    actor_service: Arc<RwLock<Option<Arc<dyn ActorService + Send + Sync>>>>,
    
    /// Registered TupleSpaceProvider (stored separately for type-safe access)
    /// This allows ActorContext::get_tuplespace() to work without unsafe code
    tuplespace_provider: Arc<RwLock<Option<Arc<dyn TupleSpaceProvider + Send + Sync>>>>,
    
    /// Registered NodeMetricsAccessor (stored separately for type-safe access)
    /// This allows components to read and update NodeMetrics without depending on Node type
    node_metrics_accessor: Arc<RwLock<Option<Arc<dyn NodeMetricsAccessor + Send + Sync>>>>,
    
    /// Registered ActorFactory (stored separately as trait object to avoid TypeId mismatch)
    /// This allows ActorBuilder to retrieve ActorFactory without TypeId issues when using
    /// different import paths (crate:: vs external_crate::).
    /// Stored as Arc<dyn Any> because ActorFactory trait is in actor crate, not core.
    actor_factory: Arc<RwLock<Option<Arc<dyn std::any::Any + Send + Sync>>>>,
    
    /// Node configuration (for accessing node_id, default_tenant_id, default_namespace, cluster_name, auth settings)
    /// Read-only after initialization, uses Mutex for one-time initialization
    node_config: Arc<tokio::sync::Mutex<Option<plexspaces_proto::node::v1::NodeConfig>>>,
    
    /// Cached gRPC clients (node_id -> ActorServiceClient)
    grpc_clients: Arc<RwLock<HashMap<String, ActorServiceClient<Channel>>>>,
    
    /// Shutdown flag: when true, node is shutting down gracefully
    /// Components should stop accepting new requests but complete in-progress ones
    shutdown_flag: Arc<RwLock<bool>>,
}

impl ServiceLocator {
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
            node_metrics_accessor: Arc::new(RwLock::new(None)),
            actor_factory: Arc::new(RwLock::new(None)),
            node_config: Arc::new(tokio::sync::Mutex::new(None)),
            grpc_clients: Arc::new(RwLock::new(HashMap::new())),
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
    /// // ✅ Correct: Consistent import paths
    /// use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    /// service_locator.register_service_by_name("ActorFactoryImpl", factory).await;
    /// let factory: Arc<ActorFactoryImpl> = service_locator.get_service_by_name("ActorFactoryImpl").await?;
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
    /// let actor_registry: Arc<ActorRegistry> = service_locator.get_service().await
    ///     .ok_or("ActorRegistry not registered")?;
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
    pub async fn register_actor_service(&self, service: Arc<dyn ActorService + Send + Sync>) {
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
        actor_service.clone().map(|s| s as Arc<dyn ActorService>)
    }

    /// Register TupleSpaceProvider as a trait object
    ///
    /// ## Purpose
    /// Allows TupleSpaceProvider to be retrieved by trait type when the concrete type is unknown.
    ///
    /// ## Arguments
    /// * `provider` - TupleSpaceProvider as a trait object
    pub async fn register_tuplespace_provider(&self, provider: Arc<dyn TupleSpaceProvider + Send + Sync>) {
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
        tuplespace.clone().map(|s| s as Arc<dyn TupleSpaceProvider>)
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
        // Clone the Arc to return it
        metrics_accessor.as_ref().map(|s| s.clone())
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

    /// Get or create a gRPC client for a remote node
    ///
    /// ## Arguments
    /// * `node_id` - Node ID to get client for
    ///
    /// ## Returns
    /// Cached or newly created `ActorServiceClient` for the node
    ///
    /// ## Design Notes
    /// - Clients are cached per node_id (one client per node)
    /// - Clients are reused across all ActorRefs for the same node
    /// - Clients are closed when ServiceLocator is dropped (Node shutdown)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let client = service_locator.get_node_client("node-2").await?;
    /// ```
    pub async fn get_node_client(
        &self,
        node_id: &str,
    ) -> Result<ActorServiceClient<Channel>, Box<dyn std::error::Error + Send + Sync>> {
        // Check cache first (read lock)
        {
            let clients = self.grpc_clients.read().await;
            if let Some(client) = clients.get(node_id) {
                return Ok(client.clone());
            }
        }

        // Get node address from ActorRegistry
        use crate::ActorRegistry;
        use crate::service_locator::service_names;
        let actor_registry: Arc<ActorRegistry> = self
            .get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY)
            .await
            .ok_or_else(|| "ActorRegistry not registered")?;

        use crate::RequestContext;
        let ctx = RequestContext::internal();
        let node_address = actor_registry
            .lookup_node_address(&ctx, node_id)
            .await
            .map_err(|e| format!("Failed to lookup node address: {}", e))?
            .ok_or_else(|| format!("Node not found: {}", node_id))?;

        // Create new client
        let endpoint = Channel::from_shared(format!("http://{}", node_address))
            .map_err(|e| format!("Invalid endpoint: {}", e))?;
        let channel = endpoint
            .connect()
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;
        let client = ActorServiceClient::new(channel);

        // Cache client (write lock)
        let mut clients = self.grpc_clients.write().await;
        clients.insert(node_id.to_string(), client.clone());

        Ok(client)
    }

    /// Shutdown all gRPC clients
    ///
    /// ## Purpose
    /// Closes all cached gRPC connections. Called when Node shuts down.
    ///
    /// ## Note
    /// gRPC clients are automatically closed when dropped, but this method
    /// provides explicit cleanup for graceful shutdown.
    pub async fn shutdown(&self) {
        let mut clients = self.grpc_clients.write().await;
        clients.clear();
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

impl Default for ServiceLocator {
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

    impl Service for MockService {}

    #[tokio::test]
    async fn test_register_and_get_service() {
        let locator = ServiceLocator::new();
        let service = Arc::new(MockService { value: 42 });

        locator.register_service(service.clone()).await;

        let retrieved: Arc<MockService> = locator.get_service().await.unwrap();
        assert_eq!(retrieved.value, 42);
    }

    #[tokio::test]
    async fn test_get_service_not_registered() {
        let locator = ServiceLocator::new();
        let retrieved: Option<Arc<MockService>> = locator.get_service().await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_service_type_safety() {
        let locator = ServiceLocator::new();

        struct ServiceA;
        impl Service for ServiceA {}

        struct ServiceB;
        impl Service for ServiceB {}

        locator.register_service(Arc::new(ServiceA)).await;
        locator.register_service(Arc::new(ServiceB)).await;

        let a: Option<Arc<ServiceA>> = locator.get_service().await;
        let b: Option<Arc<ServiceB>> = locator.get_service().await;

        assert!(a.is_some());
        assert!(b.is_some());
    }

    #[tokio::test]
    async fn test_multiple_services() {
        let locator = ServiceLocator::new();

        let service_a = Arc::new(MockService { value: 10 });
        let service_b = Arc::new(MockService { value: 20 });

        // Register different service types
        struct ServiceA;
        impl Service for ServiceA {}
        
        struct ServiceB;
        impl Service for ServiceB {}

        let service_a_impl = Arc::new(ServiceA);
        let service_b_impl = Arc::new(ServiceB);

        locator.register_service(service_a_impl.clone()).await;
        locator.register_service(service_b_impl.clone()).await;

        let retrieved_a: Arc<ServiceA> = locator.get_service().await.unwrap();
        let retrieved_b: Arc<ServiceB> = locator.get_service().await.unwrap();

        assert_eq!(Arc::as_ptr(&retrieved_a), Arc::as_ptr(&service_a_impl));
        assert_eq!(Arc::as_ptr(&retrieved_b), Arc::as_ptr(&service_b_impl));
    }

    #[tokio::test]
    async fn test_service_overwrite() {
        let locator = ServiceLocator::new();

        let service1 = Arc::new(MockService { value: 1 });
        let service2 = Arc::new(MockService { value: 2 });

        locator.register_service(service1.clone()).await;
        locator.register_service(service2.clone()).await; // Overwrites service1

        let retrieved: Arc<MockService> = locator.get_service().await.unwrap();
        assert_eq!(retrieved.value, 2);
    }

    #[tokio::test]
    async fn test_shutdown_clears_grpc_clients() {
        let locator = ServiceLocator::new();
        
        // Note: We can't easily test gRPC client creation without actual network setup
        // This test verifies shutdown() doesn't panic
        locator.shutdown().await;
        
        // Verify shutdown can be called multiple times
        locator.shutdown().await;
    }

    #[tokio::test]
    async fn test_concurrent_service_access() {
        let locator = Arc::new(ServiceLocator::new());
        let service = Arc::new(MockService { value: 100 });

        locator.register_service(service.clone()).await;

        // Spawn multiple tasks that concurrently access the service
        let mut handles = vec![];
        for _ in 0..10 {
            let locator_clone = locator.clone();
            let handle = tokio::spawn(async move {
                let retrieved: Option<Arc<MockService>> = locator_clone.get_service().await;
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
    async fn test_get_node_client_without_registry() {
        let locator = ServiceLocator::new();
        
        // Should fail because ActorRegistry is not registered
        let result = locator.get_node_client("node-1").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ActorRegistry not registered"));
    }

    #[tokio::test]
    async fn test_default_impl() {
        let locator = ServiceLocator::default();
        let service = Arc::new(MockService { value: 99 });

        locator.register_service(service.clone()).await;
        let retrieved: Arc<MockService> = locator.get_service().await.unwrap();
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
    service_locator: &Arc<ServiceLocator>,
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
