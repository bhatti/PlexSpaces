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
use plexspaces_core::ServiceLocator;

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
pub async fn create_default_service_locator(
    node_id: Option<String>,
    node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
    release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
) -> Arc<ServiceLocator> {
    use plexspaces_core::{ActorRegistry, ReplyTracker, ReplyWaiterRegistry, VirtualActorManager};
    // FacetManager is accessed via ActorRegistry, not directly imported
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_object_registry::ObjectRegistry;
    use plexspaces_process_groups::ProcessGroupRegistry;
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    use crate::service_wrappers::ObjectRegistryWrapper;
    
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
        }
    };
    
    let node_id_str = final_node_config.id.clone();
    
    // Create in-memory KeyValueStore for ObjectRegistry
    let kv_store = Arc::new(InMemoryKVStore::new());
    let object_registry = Arc::new(ObjectRegistry::new(kv_store.clone()));
    
    // Create ProcessGroupRegistry with same KeyValueStore backend
    let _process_group_registry = Arc::new(ProcessGroupRegistry::new(
        node_id_str.clone(),
        kv_store.clone(),
    ));
    
    // Create ActorRegistry with ObjectRegistry
    let object_registry_trait: Arc<dyn plexspaces_core::ObjectRegistry> = Arc::new(
        ObjectRegistryWrapper::new(object_registry.clone())
    );
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        node_id_str.clone(),
    ));
    
    // Create ServiceLocator
    let service_locator = Arc::new(ServiceLocator::new());
    
    // Create and register essential services
    let reply_tracker = Arc::new(ReplyTracker::new());
    let reply_waiter_registry = Arc::new(ReplyWaiterRegistry::new());
    let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    let facet_manager = actor_registry.facet_manager().clone();
    
    // Register all services
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    service_locator.register_service(virtual_actor_manager).await;
    service_locator.register_service(facet_manager).await;
    
    // Register ActorFactoryImpl
    let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator.register_service(actor_factory_impl).await;
    
    // Register NodeConfig (determined above)
    service_locator.register_node_config(final_node_config).await;
    
    service_locator
}
