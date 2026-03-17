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
    node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
    release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
) -> Arc<plexspaces_services::ServiceLocatorImpl> {
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    use plexspaces_core::service_names;
    use plexspaces_services::ServiceLocatorImpl;

    let service_locator_impl = Arc::new(ServiceLocatorImpl::new());
    let service_locator: Arc<dyn plexspaces_core::ServiceLocator> = service_locator_impl.clone();

    // Initialize services using ServiceLocator trait
    // ServiceLocator now creates all default services including facet factories, ActorFactoryImpl, ActorServiceImpl, and TupleSpaceProvider
    service_locator_impl
        .initialize_services(node_id, node_config, release_config)
        .await;

    // Note: ActorFactoryImpl, facet factories, ActorServiceImpl, and TupleSpaceProvider are now
    // created automatically by ServiceLocator::initialize_services() since services crate depends on actor crate.

    service_locator_impl
}
