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

use plexspaces_core::ServiceLocator;
use std::sync::Arc;

/// Create a ServiceLocator with all default services registered
/// Create default ServiceLocator with all essential services initialized
///
/// ## Test/Example storage defaults
/// When callers do not provide `release_config.runtime.db`, this helper
/// configures an isolated in-memory SQLite database. Tests should not share a
/// file-backed default database in `/tmp`, because repeated initialization can
/// race on migrations and leak state across runs.
///
/// ## Returns
/// `Arc<ServiceLocatorImpl>` - the concrete type for accessing inherent methods like `get_actor_factory()`
///
/// ## Design
/// Returns concrete type instead of trait object because:
/// - ServiceLocatorImpl is the only production implementation
/// - Services that need ActorFactory require ServiceLocatorImpl directly
/// - This is production-grade and type-safe
pub async fn create_default_service_locator(
    node_id: Option<String>,
    release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
) -> Arc<plexspaces_services::ServiceLocatorImpl> {
    use plexspaces_proto::storage::v1::SharedDbConfig;
    use plexspaces_services::ServiceLocatorImpl;

    let service_locator_impl = Arc::new(ServiceLocatorImpl::new());
    let mut effective_release = release_config.unwrap_or_default();
    let original_node = effective_release.node.clone();
    let mut effective_node = original_node.clone().unwrap_or_default();
    if let Some(node_id) = node_id {
        effective_node.id = node_id;
    }
    if effective_node.id.is_empty() {
        effective_node.id = "test-node".to_string();
    }
    if effective_node.listen_addr.is_empty() {
        effective_node.listen_addr = "127.0.0.1:0".to_string();
    }
    if effective_node.grpc_connection_pool_size == 0 {
        effective_node.grpc_connection_pool_size = 2;
    }
    if effective_node.max_connections == 0 {
        effective_node.max_connections = 100;
    }
    if effective_node.heartbeat_interval_ms == 0 {
        effective_node.heartbeat_interval_ms = 5000;
    }
    effective_node.clustering_enabled = original_node
        .as_ref()
        .map(|node| node.clustering_enabled)
        .unwrap_or(true);
    effective_release.node = Some(effective_node);
    let original_runtime = effective_release.runtime.clone();
    let mut effective_runtime = original_runtime.clone().unwrap_or_default();
    if effective_runtime.db.is_none() {
        effective_runtime.db = Some(SharedDbConfig {
            connection_string: "sqlite::memory:".to_string(),
            pool_size: 1,
            auto_migrate: true,
            ..Default::default()
        });
    }
    effective_release.runtime = Some(effective_runtime);

    // Initialize services using ServiceLocator trait
    // ServiceLocator now creates all default services including facet factories, ActorFactoryImpl, ActorServiceImpl, and TupleSpaceProvider
    service_locator_impl
        .initialize_services(Some(effective_release))
        .await;

    // Note: ActorFactoryImpl, facet factories, ActorServiceImpl, and TupleSpaceProvider are now
    // created automatically by ServiceLocator::initialize_services() since services crate depends on actor crate.

    service_locator_impl
}
