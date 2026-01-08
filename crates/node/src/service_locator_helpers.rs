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

//! ServiceLocator helper functions for tests and examples
//!
//! This module provides helper functions to create ServiceLocator instances
//! with all default services registered, suitable for tests and examples
//! that don't need a full Node instance.

use std::sync::Arc;
use plexspaces_core::{ServiceLocator, service_locator::service_names};

/// Initialize services in an existing ServiceLocator
///
/// ## Purpose
/// Populates an existing ServiceLocator with all default services. This is the shared
/// initialization logic used by both `create_default_service_locator` and `Node::initialize_services`.
///
/// ## TypeId Consistency
/// Services are registered and retrieved by their **concrete type**, not trait type.
/// For example:
/// - Register: `service_locator.register_service(Arc::new(ActorFactoryImpl::new(...))).await`
/// - Retrieve: `service_locator.get_service::<ActorFactoryImpl>().await` (NOT `get_service::<dyn ActorFactory>`)
///
/// This is because `get_service<T>()` uses `TypeId::of::<T>()`, and the TypeId of a concrete type
/// is different from the TypeId of a trait object.
///
/// ## Arguments
/// * `service_locator` - The ServiceLocator to populate
/// * `node_id` - Node ID for services (defaults to "test-node" if None)
/// * `node_config` - Optional NodeConfig (if None, will be created from release_config.node or defaults)
/// * `release_config` - Optional ReleaseSpec (if provided, node_config will be extracted from release_config.node)
///
/// ## Services Registered
/// - ActorRegistry (with ObjectRegistry and ProcessGroupRegistry)
/// - ReplyTracker
/// - ReplyWaiterRegistry
/// - VirtualActorManager
/// - FacetManager
/// - ActorFactoryImpl (created with the provided service_locator)
/// - ApplicationManager
/// - NodeConfig
/// - HealthService
/// - JournalStorage (SQLite if available, otherwise Memory)
pub async fn initialize_services_in_locator(
    service_locator: Arc<ServiceLocator>,
    node_id: Option<String>,
    node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
    release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
) {
    use plexspaces_core::{ActorRegistry, ReplyTracker, ReplyWaiterRegistry, VirtualActorManager};
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_object_registry::ObjectRegistry;
    use plexspaces_process_groups::ProcessGroupRegistry;
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    
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
                listen_address: "127.0.0.1:0".to_string(),
                cluster_seed_nodes: vec![],
                default_tenant_id: "internal".to_string(),
                default_namespace: "system".to_string(),
                cluster_name: String::new(),
            }
        })
    } else {
        // Create default NodeConfig
        let node_id_str = node_id.unwrap_or_else(|| "test-node".to_string());
        plexspaces_proto::node::v1::NodeConfig {
            id: node_id_str.clone(),
            listen_address: "127.0.0.1:0".to_string(),
            cluster_seed_nodes: vec![],
            default_tenant_id: "internal".to_string(),
            default_namespace: "system".to_string(),
            cluster_name: String::new(),
        }
    };
    
    let node_id_str = final_node_config.id.clone();
    
    // Create in-memory KeyValueStore for ObjectRegistry
    let kv_store = Arc::new(InMemoryKVStore::new());
    let object_registry = Arc::new(ObjectRegistry::new(kv_store.clone()));
    
    // Create ProcessGroupRegistry with same KeyValueStore backend
    let process_group_registry = Arc::new(ProcessGroupRegistry::new(
        node_id_str.clone(),
        kv_store.clone(),
    ));
    
    // Create ActorRegistry with ObjectRegistry (ObjectRegistry implements the trait directly)
    let object_registry_trait: Arc<dyn plexspaces_core::ObjectRegistry> = object_registry.clone();
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        node_id_str.clone(),
    ));
    
    // Create and register essential services
    let reply_tracker = Arc::new(ReplyTracker::new());
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
    use plexspaces_core::service_locator::service_names;
    service_locator.register_service_by_name(service_names::OBJECT_REGISTRY, object_registry.clone()).await;
    service_locator.register_service_by_name(service_names::PROCESS_GROUP_REGISTRY, process_group_registry.clone()).await;
    service_locator.register_service_by_name(service_names::ACTOR_REGISTRY, actor_registry.clone()).await;
    service_locator.register_service_by_name(service_names::REPLY_TRACKER, reply_tracker).await;
    service_locator.register_service_by_name(service_names::REPLY_WAITER_REGISTRY, reply_waiter_registry).await;
    service_locator.register_service_by_name(service_names::VIRTUAL_ACTOR_MANAGER, virtual_actor_manager).await;
    service_locator.register_service_by_name(service_names::FACET_MANAGER, facet_manager_wrapper).await;
    service_locator.register_service_by_name(service_names::FACET_REGISTRY, facet_registry_wrapper).await;
    
    // Register ActorFactoryImpl (created with the provided service_locator)
    // Note: Must be created AFTER all other services are registered, as ActorFactoryImpl
    // uses ServiceLocator to access ActorRegistry and other services
    let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    
    // Register as both concrete type (for backward compatibility) and trait object (to avoid TypeId mismatch)
    // The trait object registration avoids TypeId issues when retrieving from within the actor crate.
    use plexspaces_actor::ActorFactory;
    let actor_factory_trait: Arc<dyn ActorFactory + Send + Sync> = actor_factory_impl.clone();
    
    // Convert Arc<dyn ActorFactory> to Arc<dyn Any> for storage in ServiceLocator.
    // This requires unsafe code because we can't directly cast between different trait object types.
    // This is safe because:
    // 1. Arc<dyn ActorFactory> can be converted to Arc<dyn Any> since dyn ActorFactory: Any
    // 2. We maintain the Arc reference count correctly
    // 3. The conversion is reversible (we know it's actually Arc<dyn ActorFactory>)
    unsafe {
        let ptr = Arc::into_raw(actor_factory_trait);
        // Transmute the pointer from *const (dyn ActorFactory + Send + Sync) to *const (dyn Any + Send + Sync)
        // This is safe because both are trait objects with the same memory layout (Arc pointer + vtable)
        let any_ptr = std::mem::transmute::<
            *const (dyn ActorFactory + Send + Sync),
            *const (dyn std::any::Any + Send + Sync),
        >(ptr);
        let actor_factory_any: Arc<dyn std::any::Any + Send + Sync> = Arc::from_raw(any_ptr);
        service_locator.register_actor_factory(actor_factory_any).await;
    }
    
    // Also register as concrete type for services that need the concrete type
    service_locator.register_service_by_name(service_names::ACTOR_FACTORY_IMPL, actor_factory_impl).await;
    
    // Create and register ApplicationManager
    use plexspaces_core::ApplicationManager;
    let application_manager = Arc::new(ApplicationManager::new());
    service_locator.register_service_by_name(service_names::APPLICATION_MANAGER, application_manager).await;
    
    // Register NodeConfig (determined above)
    service_locator.register_node_config(final_node_config).await;
    
    // Create and register HealthService (source of truth for shutdown)
    // This ensures consistent shutdown behavior in tests/examples
    use crate::health_service_helpers::create_and_register_health_service;
    let (_health_reporter, _health_service) = create_and_register_health_service(
        service_locator.clone(),
        None, // Use default HealthProbeConfig
    ).await;
    
    // Register ActorService in ServiceLocator so ActorContext::send_reply() can use it
    // Production-grade: All essential services should be available after initialization
    // This ensures actors can send replies even when gRPC server is not started
    use plexspaces_actor_service::ActorServiceImpl;
    use std::sync::Arc;
    // Use node_id_str which is already available from final_node_config.id
    let actor_service_for_context = Arc::new(
        ActorServiceImpl::new(
            service_locator.clone(),
            node_id_str.clone(),
        )
    );
    service_locator.register_actor_service(actor_service_for_context.clone() as Arc<dyn plexspaces_core::ActorService + Send + Sync>).await;
    
    // Create and register default journal storage (shared by all actors)
    // Use config-based approach following the same pattern as create_channel and create_storage
    // Get DurabilityConfig from release_config.runtime.journaling_provider
    use plexspaces_journaling::create_journal_storage;
    use plexspaces_proto::v1::journaling::{DurabilityConfig, JournalBackend};
    
    // Get DurabilityConfig from release_config, or use default
    let durability_config = if let Some(ref release) = release_config {
        if let Some(ref runtime) = release.runtime {
            // Use journaling_provider from RuntimeConfig if available
            if runtime.journaling_provider.is_some() {
                runtime.journaling_provider.clone().unwrap()
            } else {
                // Default: SQLite if available, otherwise Memory
                create_default_durability_config()
            }
        } else {
            create_default_durability_config()
        }
    } else {
        create_default_durability_config()
    };
    
    // Create journal storage using config-based factory function
    // This returns Arc<dyn JournalStorage> which we register as a trait object
    match create_journal_storage(durability_config.clone()).await {
        Ok(storage) => {
            // Register as trait object - no need to know concrete type
            service_locator.register_journal_storage(storage).await;
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to create journal storage from config, falling back to Memory"
            );
            // Fallback to Memory on error
            use plexspaces_journaling::MemoryJournalStorage;
            let memory_storage = MemoryJournalStorage::new();
            let storage_arc: Arc<dyn plexspaces_core::JournalStorage> = Arc::new(memory_storage);
            service_locator.register_journal_storage(storage_arc).await;
        }
    }
}

/// Create default DurabilityConfig
/// Prefer SQLite if available, otherwise Memory
fn create_default_durability_config() -> plexspaces_proto::v1::journaling::DurabilityConfig {
    use plexspaces_proto::v1::journaling::{durability_config, SqliteJournalConfig};
    
    #[cfg(feature = "sqlite-backend")]
    {
        plexspaces_proto::v1::journaling::DurabilityConfig {
            backend: plexspaces_proto::v1::journaling::JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 100,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: plexspaces_proto::v1::journaling::CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
            backend_config: Some(durability_config::BackendConfig::Sqlite(SqliteJournalConfig {
                db_path: ":memory:".to_string(),
            })),
        }
    }
    #[cfg(not(feature = "sqlite-backend"))]
    {
        plexspaces_proto::v1::journaling::DurabilityConfig {
            backend: plexspaces_proto::v1::journaling::JournalBackend::JournalBackendMemory as i32,
            checkpoint_interval: 100,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: plexspaces_proto::v1::journaling::CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
            backend_config: None,
        }
    }
}


/// Create a ServiceLocator with all default services registered
///
/// ## Purpose
/// Creates a ServiceLocator with all essential services pre-registered, suitable for tests and examples.
/// This function moves all initialization logic from Node::new() and node.start() so that tests/examples
/// can create a fully functional ServiceLocator without needing a Node instance.
///
/// ## Services Registered
/// - ActorRegistry (with ObjectRegistry and ProcessGroupRegistry)
/// - ReplyTracker
/// - ReplyWaiterRegistry
/// - VirtualActorManager
/// - FacetManager
/// - ActorFactoryImpl
/// - NodeConfig (from release_config.node if provided, or created default)
///
/// ## Arguments
/// * `node_id` - Node ID for the ServiceLocator (defaults to "test-node" if None)
/// * `node_config` - Optional NodeConfig (if None, will be created from release_config.node or defaults)
/// * `release_config` - Optional ReleaseSpec (if provided, node_config will be extracted from release_config.node)
///
/// ## NodeConfig Priority
/// 1. If `node_config` is provided, use it directly
/// 2. If `release_config.node` exists, extract NodeConfig from it
/// 3. Otherwise, create default NodeConfig with node_id
///
/// ## Returns
/// Arc<ServiceLocator> with all services registered
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_node::create_default_service_locator;
/// let service_locator = create_default_service_locator(
///     Some("my-node".to_string()),
///     None,
///     None
/// ).await;
/// let actor_registry: Arc<ActorRegistry> = service_locator.get_service().await.unwrap();
/// ```
///
/// ## Example with ReleaseSpec
/// ```rust,ignore
/// use plexspaces_node::create_default_service_locator;
/// use plexspaces_proto::node::v1::ReleaseSpec;
/// let release_spec = ReleaseSpec { /* ... */ };
/// let service_locator = create_default_service_locator(
///     None,
///     None,
///     Some(release_spec)
/// ).await;
/// ```
/// Create a ServiceLocator with all default services registered
///
/// ## Purpose
/// Creates a new ServiceLocator and initializes it with all default services.
/// This is a convenience function that calls `initialize_services_in_locator` on a new ServiceLocator.
///
/// ## TypeId Consistency
/// See `initialize_services_in_locator` for details on TypeId consistency requirements.
///
/// ## Arguments
/// * `node_id` - Node ID for the ServiceLocator (defaults to "test-node" if None)
/// * `node_config` - Optional NodeConfig (if None, will be created from release_config.node or defaults)
/// * `release_config` - Optional ReleaseSpec (if provided, node_config will be extracted from release_config.node)
///
/// ## Returns
/// Arc<ServiceLocator> with all services registered
pub async fn create_default_service_locator(
    node_id: Option<String>,
    node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
    release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
) -> Arc<ServiceLocator> {
    let service_locator = Arc::new(ServiceLocator::new());
    initialize_services_in_locator(service_locator.clone(), node_id, node_config, release_config).await;
    service_locator
}
