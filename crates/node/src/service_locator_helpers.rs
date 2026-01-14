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

use std::sync::Arc;
use plexspaces_core::{ServiceLocator, ServiceLocatorInitialization};

/// Create a ServiceLocator with all default services registered
pub async fn create_default_service_locator(
    node_id: Option<String>,
    node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
    release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
) -> Arc<dyn plexspaces_core::ServiceLocator> {
    use plexspaces_services::ServiceLocatorImpl;
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    use plexspaces_core::service_names;
    
    let service_locator_impl = Arc::new(ServiceLocatorImpl::new());
    let service_locator: Arc<dyn plexspaces_core::ServiceLocator> = service_locator_impl.clone();
    
    // Initialize services using ServiceLocatorInitialization trait
    let service_locator_init: &dyn ServiceLocatorInitialization = service_locator_impl.as_ref() as &dyn ServiceLocatorInitialization;
    service_locator_init.initialize_services(node_id, node_config, release_config).await;
    
    // Register ActorFactoryImpl after core services are initialized
    // Design: initialize_services() creates all services it can (including WASM runtime)
    // ActorFactoryImpl requires ServiceLocator, so it must be registered here after ServiceLocator is created
    // This avoids circular dependency: services crate can't depend on actor crate (actor depends on services)
    let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator_impl.register_service_by_name(service_names::ACTOR_FACTORY_IMPL, actor_factory_impl).await;
    
    service_locator
}
