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

//! HealthService helper functions for unified creation and registration
//!
//! ## Purpose
//! Provides unified helper functions to create and register HealthService
//! in ServiceLocator, ensuring consistent shutdown behavior across Node
//! and test/example code.

use std::sync::Arc;
use crate::health_service::PlexSpacesHealthReporter;
use plexspaces_core::ServiceLocator;
use plexspaces_proto::system::v1::HealthProbeConfig;

/// Create and register HealthService in ServiceLocator
///
/// ## Purpose
/// Creates a HealthService (PlexSpacesHealthReporter) and registers it in
/// ServiceLocator as the source of truth for shutdown. This ensures consistent
/// shutdown behavior across all components.
///
/// ## Arguments
/// * `service_locator` - ServiceLocator to register HealthService in
/// * `config` - Optional HealthProbeConfig (uses default if None)
///
/// ## Returns
/// Tuple of:
/// - `Arc<PlexSpacesHealthReporter>`: HealthService instance
/// - gRPC health service for server registration
///
/// ## Behavior
/// 1. Creates HealthService with ServiceLocator reference
/// 2. Registers HealthService in ServiceLocator
/// 3. HealthService.begin_shutdown() will update ServiceLocator.shutdown_flag
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_node::health_service_helpers::create_and_register_health_service;
///
/// let (health_reporter, health_service) = create_and_register_health_service(
///     service_locator.clone(),
///     None
/// ).await;
///
/// // Register with gRPC server
/// Server::builder()
///     .add_service(health_service)
///     .serve(addr).await?;
/// ```
pub async fn create_and_register_health_service(
    service_locator: Arc<ServiceLocator>,
    config: Option<HealthProbeConfig>,
) -> (Arc<PlexSpacesHealthReporter>, impl tonic::server::NamedService) {
    // Create HealthService with ServiceLocator reference
    let config = config.unwrap_or_default();
    let (health_reporter, health_service) = PlexSpacesHealthReporter::with_config_and_service_locator(
        config,
        Some(service_locator.clone()),
    );
    
    let health_reporter = Arc::new(health_reporter);
    
    // Register HealthService in ServiceLocator (source of truth for shutdown)
    service_locator.register_service(health_reporter.clone()).await;
    
    tracing::info!("✅ Registered HealthService in ServiceLocator (shutdown source of truth)");
    
    (health_reporter, health_service)
}

/// Create HealthService with default configuration
///
/// ## Purpose
/// Convenience function to create HealthService with default config.
///
/// ## Arguments
/// * `service_locator` - ServiceLocator to register HealthService in
///
/// ## Returns
/// Tuple of HealthService and gRPC service
pub async fn create_default_health_service(
    service_locator: Arc<ServiceLocator>,
) -> (Arc<PlexSpacesHealthReporter>, impl tonic::server::NamedService) {
    create_and_register_health_service(service_locator, None).await
}

