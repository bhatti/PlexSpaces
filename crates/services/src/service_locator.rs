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
use std::io::Write;
use tokio::sync::RwLock;
use std::sync::OnceLock;

/// Global fatal error channel sender (Go-style graceful shutdown pattern)
/// Uses Mutex to allow taking ownership of oneshot::Sender (which can't be cloned)
static FATAL_ERROR_TX: OnceLock<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>> = OnceLock::new();

/// Register fatal error channel sender (called once by CLI)
pub fn register_fatal_error_channel(tx: tokio::sync::oneshot::Sender<String>) {
    FATAL_ERROR_TX.set(std::sync::Mutex::new(Some(tx))).ok();
}

/// Exit the process immediately with a non-zero code
/// 
/// ## Purpose
/// Forces immediate process termination, flushing all output first.
/// This is used for fatal configuration errors during initialization.
/// 
/// ## Behavior
/// - Flushes stdout and stderr
/// - Attempts to signal main thread via global channel (if registered) - Go-style graceful shutdown
/// - Uses libc::_exit() on Unix for immediate termination (bypasses cleanup)
/// - Falls back to std::process::exit() on other platforms
/// - Does not wait for any tasks or cleanup
/// 
/// ## Testing
/// In test mode (detected via `cfg(test)` or `PLEXSPACES_TEST_MODE` env var),
/// this function panics instead of exiting, allowing tests to verify the error.
/// 
/// ## Note
/// We use libc::_exit() because it terminates the process immediately without
/// running any cleanup handlers, which ensures spawned tasks don't keep the
/// process alive. This works even when called from within a spawned tokio task.
fn fatal_exit(message: &str) -> ! {
    tracing::error!("{}", message);
    eprintln!("{}", message);
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    
    // In test mode, panic instead of exiting (allows tests to verify behavior)
    #[cfg(test)]
    {
        panic!("FATAL EXIT (test mode): {}", message);
    }
    
    // Check for test mode environment variable (for integration tests)
    if std::env::var("PLEXSPACES_TEST_MODE").is_ok() {
        panic!("FATAL EXIT (test mode): {}", message);
    }
    
    // Signal main thread via global channel (if registered)
    if let Some(tx_mutex) = FATAL_ERROR_TX.get() {
        if let Ok(mut tx_guard) = tx_mutex.lock() {
            if let Some(tx) = tx_guard.take() {
                let _ = tx.send(message.to_string());
                // Brief delay to allow main thread to receive signal
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
    
    // Force flush all output before exiting
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    
    // Production: Use _exit() for immediate termination - bypasses all cleanup
    // CRITICAL: Must use _exit() not exit() to bypass tokio runtime cleanup
    // This works even when called from within a tokio async context
    #[cfg(unix)]
    {
        unsafe {
            libc::_exit(1);
        }
    }
    #[cfg(not(unix))]
    {
        // On non-Unix, use exit() which should still work
        std::process::exit(1);
    }
}

/// Helper function to create SQLite lock manager using the shared database
///
/// Uses the same database URL as the main shared database, ensuring locks
/// are stored in the same DB file for consistency and simplicity.
#[cfg(feature = "sqlite-backend")]
async fn create_sqlite_lock_manager(
    db_url: &Option<String>,
    node_id_str: &str,
) -> Arc<dyn plexspaces_locks::LockManager> {
    use plexspaces_locks::sql::SqliteLockManager;
    
    // Use the shared database URL (same as KeyValue store)
    let lock_db_url = if let Some(ref url) = db_url {
        // Use the shared database URL directly (it already has mode=rwc)
        url.clone()
    } else {
        // Fallback: use default path with node_id
        let sanitized_node_id = node_id_str.replace(['@', '/', '\\', ':'], "-");
        format!("sqlite:///tmp/plexspaces-{}.db?mode=rwc", sanitized_node_id)
    };
    
    tracing::info!(
        backend = "SQLite",
        db_url = %lock_db_url,
        "Lock manager using shared SQLite database"
    );
    
    match SqliteLockManager::new(&lock_db_url).await {
        Ok(manager) => Arc::new(manager),
        Err(e) => {
            let error_msg = format!(
                "FATAL: Failed to initialize SQLite lock manager with shared DB '{}': {}. \
                Set PLEXSPACES_DATABASE_URL=sqlite::memory: for in-memory.",
                lock_db_url, e
            );
            tracing::error!(
                db_url = %lock_db_url,
                error = %e,
                "FATAL: Failed to initialize SQLite lock manager with shared database"
            );
            fatal_exit(&error_msg);
        }
    }
}

/// Fallback for when sqlite-backend feature is not enabled
#[cfg(not(feature = "sqlite-backend"))]
async fn create_sqlite_lock_manager(
    _db_url: &Option<String>,
    _node_id_str: &str,
) -> Arc<dyn plexspaces_locks::LockManager> {
    let error_msg = "FATAL: SQLite backend not available. Enable 'sqlite-backend' feature for plexspaces-locks.";
    tracing::error!("{}", error_msg);
    fatal_exit(error_msg);
}

// Import ActorService and TupleSpaceProvider traits for trait object storage
use plexspaces_core::actor_context::{ActorService, ChannelService, TupleSpaceProvider, ObjectRegistry};
use plexspaces_core::monitoring::{NodeMetricsAccessor, NodeConnectionInfo};
use plexspaces_core::RequestContext;
use plexspaces_core::JournalStorage;
use plexspaces_core::{ActorRegistry, VirtualActorManager, ReplyWaiterRegistry};
use plexspaces_core::facet_service_wrapper::{FacetManagerServiceWrapper, FacetRegistryServiceWrapper};
use plexspaces_core::behavior_factory::BehaviorRegistry;
use plexspaces_core::ServiceLocator;
use plexspaces_actor::ActorRef;

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
            // We can't safely cast Arc<dyn Any> to Arc<T> because the memory layouts differ
            // (trait objects have a vtable pointer that concrete types don't).
            //
            // Design note: This is intentional - services should be retrieved via typed methods
            // like get_node_registry(), get_blob_service() which store services in typed fields.
            // The generic registry is kept for backwards compatibility but new code should use
            // the typed accessors.
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
    
    /// Registered LockManager (stored separately for type-safe access)
    /// This allows components to retrieve LockManager as a trait object without knowing the concrete type
    lock_manager: Arc<RwLock<Option<Arc<dyn plexspaces_locks::LockManager + Send + Sync>>>>,
    
    /// Registered NodeMetricsAccessor (stored separately for type-safe access)
    /// This allows components to read and update NodeMetrics without depending on Node type
    node_metrics_accessor: Arc<RwLock<Option<Arc<dyn NodeMetricsAccessor + Send + Sync>>>>,
    /// Registered NodeConnectionInfo (stored separately for type-safe access)
    /// This allows components to access node connection information without depending on Node type
    node_connection_info: Arc<RwLock<Option<Arc<dyn NodeConnectionInfo + Send + Sync>>>>,
    
    /// Registered ActorFactory (stored as trait object for type-safe access)
    /// ActorFactory trait is in core crate, so we can store it directly without type erasure.
    actor_factory: Arc<RwLock<Option<Arc<dyn plexspaces_core::ActorFactory>>>>,
    
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
    
    /// Registered ProcessGroupService (stored separately for type-safe access)
    /// This allows components to retrieve ProcessGroupService for distributed pub/sub
    /// Uses ProcessGroupService trait from plexspaces-core for Erlang pg/pg2-style process groups
    process_group_service: Arc<RwLock<Option<Arc<dyn plexspaces_core::ProcessGroupService>>>>,
    
    /// Registered BlobService (stored separately for type-safe access)
    /// This allows components to retrieve BlobService for blob storage operations
    blob_service: Arc<RwLock<Option<Arc<dyn plexspaces_core::BlobServiceTrait>>>>,
    
    /// Registered NodeRegistry (stored separately for type-safe access)
    /// This allows components to retrieve NodeRegistry for node discovery with caching
    node_registry: Arc<RwLock<Option<Arc<dyn plexspaces_core::NodeRegistryTrait>>>>,

    /// Registered KeyValueStore (stored separately for type-safe access)
    /// This allows WASM actors and other components to access shared KV storage
    keyvalue_store: Arc<RwLock<Option<Arc<dyn plexspaces_core::KeyValueStore>>>>,

    /// Registered ProcessGroupRegistry (as Arc<dyn Any> to avoid dependency on process-groups crate)
    /// Created during node startup from the shared KeyValueStore
    process_group_registry: Arc<RwLock<Option<Arc<dyn std::any::Any + Send + Sync>>>>,

    /// Registered TaskRouter (stored separately for type-safe access)
    /// This allows components to register shard groups for task routing
    task_router: Arc<RwLock<Option<Arc<plexspaces_scheduler::TaskRouter>>>>,
    
    /// Node configuration (for accessing node_id, cluster_name, auth settings)
    /// Read-only after initialization, uses Mutex for one-time initialization
    node_config: Arc<tokio::sync::Mutex<Option<plexspaces_proto::node::v1::NodeConfig>>>,
    
    /// Security configuration (for accessing disable_auth, service_identity, etc.)
    /// Read-only after initialization, uses Mutex for one-time initialization
    security_config: Arc<tokio::sync::Mutex<Option<plexspaces_proto::node::v1::SecurityConfig>>>,
    
    /// Runtime configuration (for accessing wasm_apps_directory, save_wasm_apps, etc.)
    /// Read-only after initialization, uses Mutex for one-time initialization
    runtime_config: Arc<tokio::sync::Mutex<Option<plexspaces_proto::node::v1::RuntimeConfig>>>,
    
    /// Shutdown flag: when true, node is shutting down gracefully
    /// Components should stop accepting new requests but complete in-progress ones
    shutdown_flag: Arc<RwLock<bool>>,

    /// Registered capability providers (provider_id -> provider)
    /// Providers are plugins that extend PlexSpaces with new service types
    capability_providers: Arc<RwLock<HashMap<String, Arc<dyn plexspaces_core::CapabilityProvider>>>>,

    /// Registered lifecycle services (name -> service) for ordered startup/shutdown
    lifecycle_services: Arc<RwLock<Vec<(String, Arc<dyn plexspaces_core::ServiceLifecycle>)>>>,

    /// Registered HttpClientService for outbound HTTP requests
    http_client: Arc<RwLock<Option<Arc<dyn plexspaces_core::HttpClientService>>>>,

    /// Registered CronService for durable scheduled jobs
    cron_service: Arc<RwLock<Option<Arc<dyn plexspaces_core::CronService>>>>,

    /// Registered ServiceInvoker for cross-node service invocation
    service_invoker: Arc<RwLock<Option<Arc<dyn plexspaces_core::ServiceInvoker>>>>,
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
            lock_manager: Arc::new(RwLock::new(None)),
            node_metrics_accessor: Arc::new(RwLock::new(None)),
            node_connection_info: Arc::new(RwLock::new(None)),
            actor_factory: Arc::new(RwLock::new(None)),
            object_registry: Arc::new(RwLock::new(None)),
            application_manager: Arc::new(RwLock::new(None)),
            behavior_registry: Arc::new(RwLock::new(None)),
            grpc_connection_manager: Arc::new(RwLock::new(None)),
            wasm_runtime: Arc::new(RwLock::new(None)),
            process_group_service: Arc::new(RwLock::new(None)),
            blob_service: Arc::new(RwLock::new(None)),
            node_registry: Arc::new(RwLock::new(None)),
            keyvalue_store: Arc::new(RwLock::new(None)),
            process_group_registry: Arc::new(RwLock::new(None)),
            task_router: Arc::new(RwLock::new(None)),
            node_config: Arc::new(tokio::sync::Mutex::new(None)),
            security_config: Arc::new(tokio::sync::Mutex::new(None)),
            runtime_config: Arc::new(tokio::sync::Mutex::new(None)),
            shutdown_flag: Arc::new(RwLock::new(false)),
            capability_providers: Arc::new(RwLock::new(HashMap::new())),
            lifecycle_services: Arc::new(RwLock::new(Vec::new())),
            http_client: Arc::new(RwLock::new(None)),
            cron_service: Arc::new(RwLock::new(None)),
            service_invoker: Arc::new(RwLock::new(None)),
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
    
    
    /// Register NodeConfig for accessing node_id, cluster_name, auth settings
    /// Note: This should be called once during node initialization
    pub async fn register_node_config(&self, config: plexspaces_proto::node::v1::NodeConfig) {
        let mut node_config = self.node_config.lock().await;
        *node_config = Some(config);
    }
    
    /// Get NodeConfig (for accessing node_id, cluster_name, auth settings)
    pub async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig> {
        let node_config = self.node_config.lock().await;
        node_config.clone()
    }
    
    /// Get Node ID from NodeConfig
    pub async fn get_node_id(&self) -> Option<String> {
        let node_config = self.node_config.lock().await;
        node_config.as_ref().map(|config| config.id.clone())
    }

    /// Get SecurityConfig (for accessing disable_auth, service_identity, etc.)
    pub async fn get_security_config(&self) -> Option<plexspaces_proto::node::v1::SecurityConfig> {
        let security_config = self.security_config.lock().await;
        security_config.clone()
    }

    /// Check if authentication is disabled (from SecurityConfig or env var)
    /// 
    /// Returns true if auth is disabled via PLEXSPACES_DISABLE_AUTH env var or SecurityConfig.disable_auth.
    /// For security, auth is enabled by default if SecurityConfig is not set.
    /// Can be disabled via PLEXSPACES_DISABLE_AUTH env variable for testing.
    pub async fn is_auth_disabled(&self) -> bool {
        // Check env variable first (for testing)
        if std::env::var("PLEXSPACES_DISABLE_AUTH").is_ok() {
            let env_value = std::env::var("PLEXSPACES_DISABLE_AUTH").unwrap();
            if env_value == "1" || env_value.eq_ignore_ascii_case("true") || env_value.eq_ignore_ascii_case("yes") {
                return true;
            }
        }
        
        let security_config = self.security_config.lock().await;
        match security_config.as_ref() {
            Some(config) => config.disable_auth,
            None => false, // Auth enabled by default if no config
        }
    }

    /// Get RuntimeConfig (for accessing wasm_apps_directory, save_wasm_apps, etc.)
    pub async fn get_runtime_config(&self) -> Option<plexspaces_proto::node::v1::RuntimeConfig> {
        let runtime_config = self.runtime_config.lock().await;
        runtime_config.clone()
    }

    /// Register RuntimeConfig
    ///
    /// ## Purpose
    /// Registers runtime configuration for accessing wasm_apps_directory, save_wasm_apps, etc.
    ///
    /// ## Arguments
    /// * `config` - RuntimeConfig to register
    pub async fn register_runtime_config(&self, config: plexspaces_proto::node::v1::RuntimeConfig) {
        let mut runtime_config = self.runtime_config.lock().await;
        *runtime_config = Some(config);
    }

    /// Register SecurityConfig
    ///
    /// ## Purpose
    /// Registers security configuration and validates it.
    /// Exits with non-zero code if auth is enabled but required keys/secrets are missing.
    ///
    /// ## Arguments
    /// * `config` - SecurityConfig to register
    ///
    /// ## Errors
    /// Exits with code 1 if auth is enabled but keys/secrets are missing (fatal error).
    pub async fn register_security_config(&self, config: plexspaces_proto::node::v1::SecurityConfig) {
        // Validate security configuration (fatal if invalid)
        use plexspaces_common::security_validator::validate_security_config;
        if let Err(e) = validate_security_config(&config).await {
            let error_msg = format!("FATAL: Security configuration validation failed: {}", e);
            tracing::error!(error = %e, "{}", error_msg);
            fatal_exit(&error_msg);
        }
        
        let mut config_guard = self.security_config.lock().await;
        *config_guard = Some(config);
        tracing::info!("Security configuration registered and validated successfully");
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

    /// Register ActorFactory (internal method)
    pub async fn register_actor_factory(&self, factory: Arc<dyn plexspaces_core::ActorFactory>) {
        let mut actor_factory = self.actor_factory.write().await;
        *actor_factory = Some(factory);
    }

    /// Get ActorFactory (internal method)
    pub async fn get_actor_factory(&self) -> Option<Arc<dyn plexspaces_core::ActorFactory>> {
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

    /// Register ActorRegistry service
    ///
    /// ## Arguments
    /// * `registry` - The ActorRegistry to register
    pub async fn register_actor_registry(&self, registry: Arc<ActorRegistry>) {
        self.register_service_by_name(service_names::ACTOR_REGISTRY, registry).await;
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
    /// Get ActorFactory as trait object
    ///
    /// ## Note
    /// This is an alias for `get_actor_factory()`. Use `get_actor_factory()` directly instead.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let factory = service_locator.actor_factory().await?;
    /// factory.spawn_actor(&ctx, actor_id, ...).await?;
    /// ```
    pub async fn actor_factory(&self) -> Option<Arc<dyn plexspaces_actor::ActorFactory>> {
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
        use plexspaces_mailbox::Mailbox;
        use plexspaces_proto::channel::v1::ChannelProvider;
        
        // Create default mailbox config (defaults to memory)
        let mut mailbox_config = plexspaces_mailbox::mailbox_config_default();
        
        // Default to IN_MEMORY (config_manager sets mailbox_provider in RuntimeConfig)
        // TODO: Read from RuntimeConfig.mailbox_provider when ServiceLocator has access to release_config
        mailbox_config.channel_provider = ChannelProvider::ChannelProviderInMemory as i32;
        
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
        use plexspaces_proto::channel::v1::{ChannelProvider, ChannelConfig, DeliveryGuarantee, OrderingGuarantee};
        
        // Create default channel config (memory backend)
        // TODO: Read from RuntimeConfig.channel_provider when ServiceLocator has access to release_config
        let channel_config = ChannelConfig {
            name: channel_name,
            provider: ChannelProvider::ChannelProviderInMemory as i32,
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

    /// Create a channel with specified configuration
    ///
    /// ## Purpose
    /// Creates a channel using the specified configuration. If backend is 0 (undefined),
    /// uses priority-based backend selection.
    ///
    /// ## Backend Priority (when backend = 0/undefined)
    /// 1. Kafka (if config available)
    /// 2. NATS (if config available)
    /// 3. SQS (if config available)
    /// 4. PostgreSQL (if config available)
    /// 5. Process Group (if ProcessGroupService available) - DEFAULT
    /// 6. In-Memory (explicit only, not default)
    /// 7. Multicast (explicit only)
    ///
    /// ## Arguments
    /// * `config` - Channel configuration
    /// * `ctx` - Request context for tenant isolation
    ///
    /// ## Returns
    /// Created Channel instance with appropriate backend
    ///
    /// ## Errors
    /// Returns error if backend is specified but required config is missing
    pub async fn create_channel(
        &self,
        config: plexspaces_proto::channel::v1::ChannelConfig,
        ctx: &plexspaces_core::RequestContext,
    ) -> Result<Arc<dyn plexspaces_channel::Channel>, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_proto::channel::v1::ChannelProvider;
        
        // Use provider from config (defaults to IN_MEMORY if 0)
        // Provider is set by config_manager::initialize() based on RuntimeConfig.channel_provider
        let provider = ChannelProvider::try_from(config.provider)
            .unwrap_or(ChannelProvider::ChannelProviderInMemory);
        
        // Validate provider-specific config
        self.validate_channel_config(&config)?;
        
        // Create channel based on provider type
        match provider {
            ChannelProvider::ChannelProviderProcessGroup => {
                self.create_process_group_channel(config, ctx).await
            }
            _ => {
                // Use generic create_channel for other providers
                let channel = plexspaces_channel::create_channel(config)
                    .await
                    .map_err(|e| format!("Failed to create channel: {}", e))?;
                Ok(Arc::from(channel))
            }
        }
    }

    /// Validate channel configuration for the specified provider
    fn validate_channel_config(
        &self,
        config: &plexspaces_proto::channel::v1::ChannelConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_proto::channel::v1::ChannelProvider;
        
        let provider = ChannelProvider::try_from(config.provider)
            .unwrap_or(ChannelProvider::ChannelProviderInMemory);
        
        match provider {
            ChannelProvider::ChannelProviderKafka => {
                match &config.backend_config {
                    Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Kafka(kafka)) => {
                        if kafka.brokers.is_empty() {
                            return Err("Kafka provider requires brokers configuration".into());
                        }
                    }
                    _ => return Err("Kafka provider requires kafka config".into()),
                }
            }
            ChannelProvider::ChannelProviderNats => {
                match &config.backend_config {
                    Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Nats(nats)) => {
                        if nats.servers.is_empty() {
                            return Err("NATS provider requires servers configuration".into());
                        }
                    }
                    _ => return Err("NATS provider requires nats config".into()),
                }
            }
            ChannelProvider::ChannelProviderSqs => {
                match &config.backend_config {
                    Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqs(sqs)) => {
                        if sqs.region.is_empty() {
                            return Err("SQS provider requires region configuration".into());
                        }
                    }
                    _ => return Err("SQS provider requires sqs config".into()),
                }
            }
            ChannelProvider::ChannelProviderRedis => {
                match &config.backend_config {
                    Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Redis(redis)) => {
                        if redis.url.is_empty() {
                            return Err("Redis provider requires url configuration".into());
                        }
                    }
                    _ => return Err("Redis provider requires redis config".into()),
                }
            }
            // InMemory, ProcessGroup, SQLite, UDP, Postgres don't require config validation
            _ => {}
        }
        
        Ok(())
    }

    /// Create a ProcessGroup-based channel
    #[cfg(feature = "process-group-backend")]
    async fn create_process_group_channel(
        &self,
        config: plexspaces_proto::channel::v1::ChannelConfig,
        ctx: &plexspaces_core::RequestContext,
    ) -> Result<Arc<dyn plexspaces_channel::Channel>, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_channel::ProcessGroupChannel;
        
        // Get self as Arc for ProcessGroupChannel
        // Since we can't get Arc<Self> from &self, we create the channel differently
        let process_group_service = self.get_process_group_service().await
            .ok_or("ProcessGroupService not available for ProcessGroup channel backend")?;
        
        // Create ProcessGroupChannel using ServiceLocator
        // Note: ProcessGroupChannel requires Arc<dyn ServiceLocator>, which we can't easily get from &self
        // For now, return error suggesting to use plexspaces_channel::create_channel directly
        Err("ProcessGroup channel creation via ServiceLocator not yet supported. Use plexspaces_channel::create_channel with explicit ServiceLocator".into())
    }

    /// Create a ProcessGroup-based channel (fallback when feature disabled)
    #[cfg(not(feature = "process-group-backend"))]
    async fn create_process_group_channel(
        &self,
        _config: plexspaces_proto::channel::v1::ChannelConfig,
        _ctx: &plexspaces_core::RequestContext,
    ) -> Result<Arc<dyn plexspaces_channel::Channel>, Box<dyn std::error::Error + Send + Sync>> {
        Err("ProcessGroup channel backend not enabled. Enable 'process-group-backend' feature".into())
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
    
    async fn register_actor_registry(&self, registry: Arc<ActorRegistry>) {
        self.register_actor_registry(registry).await;
    }
    
    async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>> {
        self.virtual_actor_manager().await
    }
    
    async fn reply_waiter_registry(&self) -> Option<Arc<ReplyWaiterRegistry>> {
        self.reply_waiter_registry().await
    }
    
    async fn get_actor_factory(&self) -> Option<Arc<dyn plexspaces_core::ActorFactory>> {
        self.get_actor_factory().await
    }
    
    async fn register_actor_factory(&self, factory: Arc<dyn plexspaces_core::ActorFactory>) {
        self.register_actor_factory(factory).await;
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
    
    async fn get_lock_manager(&self) -> Option<Arc<dyn plexspaces_locks::LockManager + Send + Sync>> {
        let manager = self.lock_manager.read().await;
        manager.clone()
    }
    
    async fn register_lock_manager(&self, service: Arc<dyn plexspaces_locks::LockManager + Send + Sync>) {
        let mut manager = self.lock_manager.write().await;
        *manager = Some(service);
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
    
    async fn get_security_config(&self) -> Option<plexspaces_proto::node::v1::SecurityConfig> {
        self.get_security_config().await
    }
    
    async fn register_security_config(&self, config: plexspaces_proto::node::v1::SecurityConfig) {
        self.register_security_config(config).await
    }

    async fn get_runtime_config(&self) -> Option<plexspaces_proto::node::v1::RuntimeConfig> {
        self.get_runtime_config().await
    }

    async fn register_runtime_config(&self, config: plexspaces_proto::node::v1::RuntimeConfig) {
        self.register_runtime_config(config).await
    }
    
    async fn is_auth_disabled(&self) -> bool {
        self.is_auth_disabled().await
    }
    
    async fn get_node_connection_info(&self) -> Option<Arc<dyn plexspaces_core::NodeConnectionInfo + Send + Sync>> {
        self.get_node_connection_info().await
    }
    
    async fn register_node_connection_info(&self, accessor: Arc<dyn plexspaces_core::NodeConnectionInfo + Send + Sync>) {
        self.register_node_connection_info(accessor).await
    }
    
    fn is_shutdown_requested(&self) -> bool {
        // This is a sync method in the trait, but ServiceLocatorImpl uses async
        // Use try_current() to avoid panicking if called from outside runtime
        // If we're in a runtime, use block_in_place to avoid blocking the runtime
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            // We're in an async runtime - use block_in_place to avoid blocking
            tokio::task::block_in_place(|| {
                handle.block_on(async {
                    self.is_shutting_down().await
                })
            })
        } else {
            // Not in a runtime - can't check async state, return false
            false
        }
    }
    
    async fn initialize_services(
        &self,
        node_id: Option<String>,
        node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
        release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
    ) {
        // Check if already initialized (idempotent)
        if self.actor_registry().await.is_some() {
            // Services already initialized
            return;
        }
        
        // We can't get Arc from &self, so we need to work differently
        // The helper function needs Arc<ServiceLocatorImpl> for register_service_by_name
        // Since ServiceLocatorImpl contains only Arc fields, cloning is cheap (just clones the Arc pointers)
        // We'll clone self and create a new Arc pointing to the cloned instance
        // This is safe because ServiceLocatorImpl is just a container for services (all fields are Arc)
        let service_locator_impl = Arc::new(self.clone());
        initialize_services_impl(service_locator_impl, node_id, node_config, release_config).await;
    }
    
    fn request_shutdown(&self) {
        // Use try_current() to avoid panicking if called from outside runtime
        // If we're in a runtime, spawn a task to set shutdown
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            // We're in an async runtime - spawn task to avoid blocking
            let sl = self.clone();
            handle.spawn(async move {
                sl.set_shutdown(true).await;
            });
        } else {
            // Not in a runtime - can't set shutdown async, log warning
            tracing::warn!("Cannot request shutdown: not in async runtime context");
        }
    }
    
    async fn application_manager(&self) -> Option<Arc<dyn plexspaces_core::ApplicationManager>> {
        self.get_application_manager().await
    }
    
    async fn register_application_manager(&self, manager: Arc<dyn plexspaces_core::ApplicationManager>) {
        let mut app_manager = self.application_manager.write().await;
        *app_manager = Some(manager);
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
        
        // Use request_context_for_system_operations (default tenant from node config or blank)
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
    
    /// Context for operations that have no request (e.g. node registration, heartbeat).
    /// Tenant/namespace: empty strings - tenant comes from auth, not config.
    /// Admin=true so cross-namespace lookups work; never use tenant_id "internal".
    async fn request_context_for_system_operations(&self) -> plexspaces_common::RequestContext {
        // Tenant comes from auth, not config - use empty strings for system operations
        plexspaces_common::RequestContext::new_without_auth(String::new(), String::new())
            .with_admin(true)
    }

    /// Same as request_context_for_system_operations but with explicit namespace (e.g. cluster_name).
    async fn request_context_for_system_operations_with_namespace(&self, namespace: String) -> plexspaces_common::RequestContext {
        // Tenant comes from auth, not config - use empty string
        plexspaces_common::RequestContext::new_without_auth(String::new(), namespace)
            .with_admin(true)
    }
    
    async fn get_wasm_runtime(&self) -> Option<std::sync::Arc<dyn plexspaces_core::WasmRuntimeTrait>> {
        let runtime = self.wasm_runtime.read().await;
        runtime.clone()
    }
    
    async fn register_wasm_runtime(&self, runtime: std::sync::Arc<dyn plexspaces_core::WasmRuntimeTrait>) {
        let mut wasm_runtime = self.wasm_runtime.write().await;
        *wasm_runtime = Some(runtime);
    }

    async fn get_process_group_service(&self) -> Option<std::sync::Arc<dyn plexspaces_core::ProcessGroupService>> {
        let service = self.process_group_service.read().await;
        service.clone()
    }
    
    async fn register_process_group_service(&self, service: std::sync::Arc<dyn plexspaces_core::ProcessGroupService>) {
        let mut process_group_service = self.process_group_service.write().await;
        *process_group_service = Some(service);
    }
    
    async fn get_blob_service(&self) -> Option<std::sync::Arc<dyn plexspaces_core::BlobServiceTrait>> {
        let service = self.blob_service.read().await;
        service.clone()
    }
    
    async fn register_blob_service(&self, service: std::sync::Arc<dyn plexspaces_core::BlobServiceTrait>) {
        let mut blob_service = self.blob_service.write().await;
        *blob_service = Some(service);
    }
    
    async fn get_node_registry(&self) -> Option<std::sync::Arc<dyn plexspaces_core::NodeRegistryTrait>> {
        let registry = self.node_registry.read().await;
        registry.clone()
    }
    
    async fn register_node_registry(&self, registry: std::sync::Arc<dyn plexspaces_core::NodeRegistryTrait>) {
        let mut node_registry = self.node_registry.write().await;
        *node_registry = Some(registry);
    }

    async fn get_keyvalue_store(&self) -> Option<std::sync::Arc<dyn plexspaces_core::KeyValueStore>> {
        let store = self.keyvalue_store.read().await;
        store.clone()
    }

    async fn register_keyvalue_store(&self, store: std::sync::Arc<dyn plexspaces_core::KeyValueStore>) {
        let mut keyvalue_store = self.keyvalue_store.write().await;
        *keyvalue_store = Some(store);
    }

    async fn get_process_group_registry(&self) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        let registry = self.process_group_registry.read().await;
        registry.clone()
    }

    async fn register_process_group_registry(&self, registry: std::sync::Arc<dyn std::any::Any + Send + Sync>) {
        let mut pg_registry = self.process_group_registry.write().await;
        *pg_registry = Some(registry);
    }

    // ============================================================================
    // CAPABILITY PROVIDER METHODS
    // ============================================================================

    async fn register_capability_provider(&self, provider: std::sync::Arc<dyn plexspaces_core::CapabilityProvider>) {
        let provider_id = provider.provider_id().to_string();
        let mut providers = self.capability_providers.write().await;
        tracing::info!(provider_id = %provider_id, description = %provider.description(), "Registered capability provider");
        providers.insert(provider_id, provider);
    }

    async fn get_capability_provider(&self, provider_id: &str) -> Option<std::sync::Arc<dyn plexspaces_core::CapabilityProvider>> {
        let providers = self.capability_providers.read().await;
        providers.get(provider_id).cloned()
    }

    async fn list_capability_providers(&self) -> Vec<String> {
        let providers = self.capability_providers.read().await;
        providers.keys().cloned().collect()
    }

    // ============================================================================
    // SERVICE LIFECYCLE METHODS
    // ============================================================================

    async fn register_lifecycle_service(&self, name: &str, service: std::sync::Arc<dyn plexspaces_core::ServiceLifecycle>) {
        let mut services = self.lifecycle_services.write().await;
        tracing::info!(service = %name, startup_priority = service.startup_priority(), shutdown_priority = service.shutdown_priority(), "Registered lifecycle service");
        services.push((name.to_string(), service));
    }

    async fn start_lifecycle_services(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let services = self.lifecycle_services.read().await;
        let mut sorted: Vec<_> = services.iter().collect();
        sorted.sort_by_key(|(_, s)| s.startup_priority());

        for (name, service) in sorted {
            tracing::info!(service = %name, priority = service.startup_priority(), "Starting lifecycle service");
            if let Err(e) = service.on_start().await {
                tracing::error!(service = %name, error = %e, "Failed to start lifecycle service");
                return Err(e);
            }
        }
        Ok(())
    }

    async fn stop_lifecycle_services(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let services = self.lifecycle_services.read().await;
        let mut sorted: Vec<_> = services.iter().collect();
        sorted.sort_by_key(|(_, s)| s.shutdown_priority());

        for (name, service) in sorted {
            tracing::info!(service = %name, priority = service.shutdown_priority(), "Stopping lifecycle service");
            if let Err(e) = service.on_stop().await {
                tracing::warn!(service = %name, error = %e, "Error stopping lifecycle service (continuing shutdown)");
            }
        }
        Ok(())
    }

    // ============================================================================
    // HTTP CLIENT SERVICE
    // ============================================================================

    async fn get_http_client(&self) -> Option<std::sync::Arc<dyn plexspaces_core::HttpClientService>> {
        let client = self.http_client.read().await;
        client.clone()
    }

    async fn register_http_client(&self, service: std::sync::Arc<dyn plexspaces_core::HttpClientService>) {
        let mut client = self.http_client.write().await;
        *client = Some(service);
    }

    // ============================================================================
    // CRON SERVICE
    // ============================================================================

    async fn get_cron_service(&self) -> Option<std::sync::Arc<dyn plexspaces_core::CronService>> {
        let service = self.cron_service.read().await;
        service.clone()
    }

    async fn register_cron_service(&self, service: std::sync::Arc<dyn plexspaces_core::CronService>) {
        let mut cron = self.cron_service.write().await;
        *cron = Some(service);
    }

    // ============================================================================
    // SERVICE INVOKER
    // ============================================================================

    async fn get_service_invoker(&self) -> Option<std::sync::Arc<dyn plexspaces_core::ServiceInvoker>> {
        let invoker = self.service_invoker.read().await;
        invoker.clone()
    }

    async fn register_service_invoker(&self, invoker: std::sync::Arc<dyn plexspaces_core::ServiceInvoker>) {
        let mut service_invoker = self.service_invoker.write().await;
        *service_invoker = Some(invoker);
    }
}

impl ServiceLocatorImpl {
    /// Get TaskRouter
    ///
    /// ## Purpose
    /// Retrieves TaskRouter for registering actor groups and routing tasks.
    /// TaskRouter is registered by Node when it initializes scheduling services.
    ///
    /// ## Returns
    /// `Some(Arc<TaskRouter>)` if registered, `None` otherwise
    pub async fn get_task_router(&self) -> Option<Arc<plexspaces_scheduler::TaskRouter>> {
        let router = self.task_router.read().await;
        router.clone()
    }
    
    /// Register TaskRouter
    ///
    /// ## Purpose
    /// Registers TaskRouter for shard group management and task routing.
    /// Called by Node when initializing scheduling services.
    ///
    /// ## Arguments
    /// * `router` - TaskRouter to register
    pub async fn register_task_router(&self, router: Arc<plexspaces_scheduler::TaskRouter>) {
        let mut task_router = self.task_router.write().await;
        *task_router = Some(router);
    }
}



/// Internal helper function that implements service initialization
/// Takes concrete ServiceLocatorImpl to access register_service_by_name
/// 
/// # Configuration
/// All configuration comes from ReleaseSpec.runtime which has already been
/// initialized by config_manager::initialize() with env var overrides applied.
/// 
/// This function does NOT read environment variables directly - all env var
/// handling is centralized in config_manager.
/// 
/// # Backend Selection
/// - URL contains `:memory:` → SQLite :memory: backend (in-memory)
/// - Otherwise → SQLite file-based
/// 
/// # Panics
/// Panics if database initialization fails (fatal error).
async fn initialize_services_impl(
    service_locator_impl: Arc<ServiceLocatorImpl>,
    node_id: Option<String>,
    node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
    release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
) {
    // Get trait object for methods that need it (used implicitly via service_locator_impl)
    use plexspaces_core::{ActorRegistry, ReplyWaiterRegistry, VirtualActorManager};
    use plexspaces_object_registry::SqliteObjectRegistryRepository;
    use plexspaces_process_groups::ProcessGroupRegistry;
    use std::collections::HashMap;
    
    // LockManager imports (behind feature flags)
    #[cfg(feature = "sqlite-backend")]
    use plexspaces_locks::sql::SqliteLockManager;
    
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
                cluster_name: String::new(),
                max_connections: 100,
                heartbeat_interval_ms: 5000,
                clustering_enabled: true,
                grpc_connection_pool_size: 2,
                metadata: HashMap::new(),
                node_registry: None,
                grpc_address: String::new(),
            }
        })
    } else {
        // Create default NodeConfig
        let node_id_str = node_id.unwrap_or_else(|| "test-node".to_string());
        plexspaces_proto::node::v1::NodeConfig {
            id: node_id_str.clone(),
            listen_addr: "127.0.0.1:0".to_string(),
            cluster_seed_nodes: vec![],
            cluster_name: String::new(),
            grpc_connection_pool_size: 2,
            max_connections: 100,
            heartbeat_interval_ms: 5000,
            clustering_enabled: true,
            metadata: HashMap::new(),
            node_registry: None,
            grpc_address: String::new(),
        }
    };
    
    let node_id_str = final_node_config.id.clone();
    
    // Get database URL from RuntimeConfig.db (already initialized by config_manager with env overrides)
    // Note: All env var handling is centralized in config_manager::initialize()
    let db_url = release_config.as_ref()
        .and_then(|r| r.runtime.as_ref())
        .and_then(|rt| rt.db.as_ref())
        .map(|db| db.connection_string.clone())
        .filter(|s| !s.is_empty());
    
    // Use in-memory if URL contains ":memory:"
    let use_memory = db_url.as_ref()
        .map(|s| s.contains(":memory:"))
        .unwrap_or(false);
    
    // Extract db_path from URL (used for both KV store and Lock manager)
    // SqliteKVStore::new() expects just a path (it adds sqlite: prefix and ?mode=rwc internally)
    // We need to strip any existing scheme prefix and query params
    let db_path = if let Some(ref url) = db_url {
        // Extract path from URL: sqlite:///path?mode=rwc -> /path
        let path_with_params = if url.starts_with("sqlite:///") {
            url.strip_prefix("sqlite://").unwrap().to_string()
        } else if url.starts_with("sqlite://") {
            url.strip_prefix("sqlite://").unwrap().to_string()
        } else if url.starts_with("sqlite:") {
            url.strip_prefix("sqlite:").unwrap().to_string()
        } else {
            url.clone()
        };
        // Strip query parameters (e.g., ?mode=rwc) - SqliteKVStore::new() adds them
        path_with_params.split('?').next().unwrap_or(&path_with_params).to_string()
    } else {
        // Default: file-based SQLite with node_id in filename
        let sanitized_node_id = node_id_str.replace(['@', '/', '\\', ':'], "-");
        format!("/tmp/plexspaces-kv-{}.db", sanitized_node_id)
    };

    // Create KeyValueStore based on environment configuration.
    // If PLEXSPACES_KV_BACKEND is set, use create_keyvalue_stores_from_env() to select the backend.
    // Otherwise, fall back to SQLite with the config-derived path for backward compatibility.
    // Each backend implements both plexspaces_keyvalue::KeyValueStore (rich) and
    // plexspaces_common::KeyValueStore (consumer-facing for WASM actors).
    let effective_db_path = if use_memory { ":memory:".to_string() } else { db_path.clone() };
    let (kv_store, kv_store_common): (Arc<dyn plexspaces_keyvalue::KeyValueStore>, Arc<dyn plexspaces_core::KeyValueStore>) = if std::env::var("PLEXSPACES_KV_BACKEND").is_ok() {
        // Use environment-configured backend (Redis, PostgreSQL, DynamoDB, Blob, etc.)
        match plexspaces_keyvalue::create_keyvalue_stores_from_env().await {
            Ok(stores) => {
                tracing::info!("KeyValue storage initialized from PLEXSPACES_KV_BACKEND environment variable");
                stores
            }
            Err(e) => {
                let error_msg = format!(
                    "FATAL: Failed to initialize KeyValue store from environment: {}. \
                    Check PLEXSPACES_KV_BACKEND and related environment variables.",
                    e
                );
                tracing::error!(error = %e, "FATAL: Failed to initialize KeyValue store from environment.");
                fatal_exit(&error_msg);
            }
        }
    } else {
        // Default: SQLite with config-derived path (backward compatible)
        let config = plexspaces_keyvalue::KVConfig::new(plexspaces_keyvalue::BackendType::Sqlite {
            path: effective_db_path.clone(),
        });
        match plexspaces_keyvalue::create_keyvalue_stores_from_config(config).await {
            Ok(stores) => {
                if use_memory {
                    tracing::info!(backend = "SQLite :memory:", "KeyValue storage using in-memory SQLite");
                } else {
                    tracing::info!(backend = "SQLite", path = %effective_db_path, "KeyValue storage using file-based SQLite");
                }
                stores
            }
            Err(e) => {
                let error_msg = format!(
                    "FATAL: Failed to initialize SQLite KeyValue store at '{}': {}. \
                    Set PLEXSPACES_DATABASE_URL=sqlite::memory: for in-memory.",
                    effective_db_path, e
                );
                tracing::error!(
                    db_path = %effective_db_path,
                    error = %e,
                    "FATAL: Failed to initialize SQLite KeyValue store."
                );
                fatal_exit(&error_msg);
            }
        }
    };
    
    // Create ObjectRegistry with its own repository backend (indexed columns for fast queries)
    // Uses the same SQLite database path as KeyValueStore for simplicity, but with its own table
    // Always use SQLite - use :memory: for in-memory mode
    let object_registry_repo: Arc<dyn plexspaces_object_registry::repository::ObjectRegistryRepository> = match SqliteObjectRegistryRepository::new(&effective_db_path).await {
        Ok(repo) => {
            if use_memory {
                tracing::info!(backend = "SQLite :memory:", "Object Registry using in-memory SQLite");
            } else {
                tracing::info!(backend = "SQLite", path = %effective_db_path, "Object Registry using SQLite backend");
            }
            Arc::new(repo)
        }
        Err(e) => {
            let error_msg = format!(
                "FATAL: Failed to initialize SQLite Object Registry at '{}': {}. \
                Set PLEXSPACES_DATABASE_URL=sqlite::memory: for in-memory.",
                effective_db_path, e
            );
            tracing::error!(
                db_path = %effective_db_path,
                error = %e,
                "FATAL: Failed to initialize SQLite Object Registry."
            );
            fatal_exit(&error_msg);
        }
    };
    let object_registry = Arc::new(plexspaces_object_registry::ObjectRegistryImpl::new(object_registry_repo));
    
    // Create ProcessGroupRegistry with same KeyValueStore backend
    let process_group_registry = Arc::new(ProcessGroupRegistry::new(
        node_id_str.clone(),
        kv_store.clone(),
    ));
    
    // Create LockManager based on locks_provider from config
    // Config manager has already resolved this: Redis (if available) > Shared DB
    // Always use SQLite - use :memory: for in-memory mode
    let lock_manager: Arc<dyn plexspaces_locks::LockManager> = if use_memory {
        #[cfg(feature = "sqlite-backend")]
        {
            tracing::info!(backend = "SQLite :memory:", "Lock manager using in-memory SQLite");
            match SqliteLockManager::new(":memory:").await {
                Ok(manager) => Arc::new(manager),
                Err(e) => {
                    let error_msg = format!("FATAL: Failed to create SQLite :memory: lock manager: {}", e);
                    tracing::error!("{}", error_msg);
                    fatal_exit(&error_msg);
                }
            }
        }
        #[cfg(not(feature = "sqlite-backend"))]
        {
            let error_msg = "FATAL: SQLite backend not available. Enable 'sqlite-backend' feature for plexspaces-locks.";
            tracing::error!("{}", error_msg);
            fatal_exit(error_msg);
        }
    } else {
        // Read locks_provider from config (already resolved by config_manager)
        let locks_provider = release_config
            .as_ref()
            .and_then(|r| r.runtime.as_ref())
            .and_then(|rt| rt.locks_provider.as_ref());
        
        match locks_provider {
            Some(lp) if lp.provider == plexspaces_proto::storage::v1::StorageProvider::StorageProviderRedis as i32 => {
                // Use Redis for locks
                #[cfg(feature = "redis-backend")]
                {
                    use plexspaces_locks::redis::RedisLockManager;
                    
                    let redis_url = lp.config.as_ref()
                        .and_then(|c| match c {
                            plexspaces_proto::storage::v1::storage_provider_config::Config::Redis(r) => Some(r.url.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| "redis://localhost:6379".to_string());
                    
                    tracing::info!(backend = "Redis", redis_url = %redis_url, "Lock manager using Redis backend");
                    match RedisLockManager::new(&redis_url).await {
                        Ok(manager) => Arc::new(manager),
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "Failed to initialize Redis lock manager, falling back to shared database"
                            );
                            create_sqlite_lock_manager(&db_url, &node_id_str).await
                        }
                    }
                }
                #[cfg(not(feature = "redis-backend"))]
                {
                    tracing::warn!("Redis backend not available (feature disabled), using shared database for locks");
                    create_sqlite_lock_manager(&db_url, &node_id_str).await
                }
            }
            _ => {
                // Use shared database for locks (SQLite or Postgres)
                create_sqlite_lock_manager(&db_url, &node_id_str).await
            }
        }
    };
    
    // Register LockManager in ServiceLocator (use locks::LockManager directly)
    let service_locator: &dyn plexspaces_core::ServiceLocator = service_locator_impl.as_ref();
    service_locator.register_lock_manager(lock_manager.clone()).await;

    // Register KeyValueStore in ServiceLocator so WASM actors can access the shared store
    service_locator.register_keyvalue_store(kv_store_common).await;

    // Register ProcessGroupRegistry in ServiceLocator so WASM actors can use pg_join/pg_leave/pg_members/pg_broadcast
    service_locator.register_process_group_registry(process_group_registry.clone() as Arc<dyn std::any::Any + Send + Sync>).await;
    
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
    
    // Phase 1: Unified Lifecycle - Create and register FacetRegistry with default factories
    // FacetRegistry allows applications to create facets from proto configurations
    use plexspaces_facet::FacetRegistry;
    use plexspaces_core::facet_service_wrapper::{FacetRegistryServiceWrapper, FacetManagerServiceWrapper};
    
    // Create FacetRegistry and register default facet factories (LockFacetFactory, RegistryFacetFactory, ProcessGroupFacetFactory)
    // Services crate depends on actor crate, so we can create these directly
    use plexspaces_actor::{LockFacetFactory, RegistryFacetFactory, ProcessGroupFacetFactory};
    use std::sync::Arc as StdArc;
    let service_locator_for_factories: Arc<dyn plexspaces_core::ServiceLocator> = service_locator_impl.clone();
    
    let mut facet_registry = FacetRegistry::new();
    let lock_factory = StdArc::new(LockFacetFactory::new(service_locator_for_factories.clone()));
    facet_registry.register("locks".to_string(), lock_factory);
    let registry_factory = StdArc::new(RegistryFacetFactory::new(service_locator_for_factories.clone()));
    facet_registry.register("registry".to_string(), registry_factory);
    let process_group_factory = StdArc::new(ProcessGroupFacetFactory::new(service_locator_for_factories.clone()));
    facet_registry.register("process_groups".to_string(), process_group_factory);
    
    let facet_registry = Arc::new(facet_registry);
    let facet_registry_wrapper = Arc::new(FacetRegistryServiceWrapper::new(facet_registry.clone()));
    let facet_manager_wrapper = Arc::new(FacetManagerServiceWrapper::new(facet_manager.clone()));
    
    // Register all services using explicit service names for consistency
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
    tracing::info!(registered_types = ?facet_registry.list_types(), "📦 FacetRegistry initialized with {} facet types", facet_registry.list_types().len());
    
    // Create and register NodeRegistry (required for node connectivity and SWIM protocol)
    // NodeRegistry needs ObjectRegistry (concrete type), not trait object
    use crate::node_registry::NodeRegistry;
    let node_registry = Arc::new(NodeRegistry::from_config(
        object_registry.clone(), // Use concrete ObjectRegistryImpl, not trait object
        &final_node_config,
    ));
    let node_registry_trait: Arc<dyn plexspaces_core::NodeRegistryTrait> = node_registry.clone();
    service_locator.register_node_registry(node_registry_trait).await;
    tracing::info!("📡 NodeRegistry initialized for node discovery and SWIM protocol");
    
    // Create and register ActorFactoryImpl (services crate depends on actor crate, so this is safe)
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> = service_locator_impl.clone();
    let actor_factory_impl = ActorFactoryImpl::new_arc(service_locator_trait).await;
    
    // Register ActorFactoryImpl (it implements Service trait) in services map for backward compat
    service_locator_impl.register_service_by_name(service_names::ACTOR_FACTORY_IMPL, actor_factory_impl.clone()).await;
    
    // Register ActorFactory as trait object (ActorFactoryImpl implements ActorFactory from core)
    use plexspaces_core::ActorFactory;
    let factory_trait: Arc<dyn ActorFactory> = actor_factory_impl.clone();
    service_locator_impl.register_actor_factory(factory_trait).await;
    if tracing::enabled!(tracing::Level::TRACE) {
        tracing::trace!("✅ ActorFactoryImpl registered and ready for actor spawning");
    }
    
    // Create and register ActorServiceImpl (needs node_id, which we have from node_id_str)
    use crate::actor_service::ActorServiceImpl;
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator_impl.clone(), node_id_str.clone()));
    service_locator.register_actor_service(actor_service as Arc<dyn plexspaces_core::ActorService + Send + Sync>).await;
    if tracing::enabled!(tracing::Level::TRACE) {
        tracing::trace!(node_id = %node_id_str, "✅ ActorServiceImpl registered for message routing");
    }
    
    // Create and register default TupleSpaceProvider
    // Tenant comes from auth, not config - use empty strings for initialization
    use plexspaces_core::service_wrappers::TupleSpaceProviderWrapper;
    use plexspaces_core::RequestContext;
    let ctx = RequestContext::new_without_auth(String::new(), String::new())
        .with_admin(true);
    let tuplespace = TupleSpaceProviderWrapper::from_context(&ctx);
    let tuplespace_provider = Arc::new(TupleSpaceProviderWrapper::new(tuplespace));
    service_locator.register_tuplespace_provider(tuplespace_provider as Arc<dyn plexspaces_core::TupleSpaceProvider + Send + Sync>).await;
    if tracing::enabled!(tracing::Level::TRACE) {
        tracing::trace!("✅ TupleSpaceProvider registered");
    }
    
    // Register NodeConfig (determined above)
    service_locator.register_node_config(final_node_config.clone()).await;
    
    // Register SecurityConfig from ReleaseSpec.runtime if available
    if let Some(ref release) = release_config {
        if let Some(ref runtime) = release.runtime {
            if let Some(security) = runtime.security.clone() {
                service_locator.register_security_config(security).await;
            }
        }
    }
    
    // Create and register GrpcConnectionManager with connection pooling
    // NOTE: default_tenant_id and default_namespace have been removed from NodeConfig.
    // Tenant comes from auth (JWT/mTLS); namespace from application/actor.
    use plexspaces_core::GrpcConnectionManager;
    let pool_size = final_node_config.grpc_connection_pool_size;
    let connection_manager = Arc::new(GrpcConnectionManager::new(
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
