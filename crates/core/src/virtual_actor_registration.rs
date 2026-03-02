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

//! Centralized virtual actor type registration helper
//!
//! ## Purpose
//! Provides a consistent helper function to register virtual actor types from SDK annotations,
//! app-config.toml, or WASM applications. Ensures consistent behavior across all languages and entry points.
//!
//! ## Design
//! - Extracts facet_config from facets consistently using `extract_facet_config_for_registration()`
//! - Uses keyed format required by `register_virtual_actor_type()`
//! - Logs errors but doesn't fail (actor will still work, just no auto-activation)
//! - Idempotent (safe to call multiple times)

use plexspaces_proto::common::v1::Facet as ProtoFacet;
use plexspaces_proto::v1::actor::ActorConfig;
use plexspaces_facet::Facet;
use plexspaces_facet::facet_helpers::extract_facet_config_for_registration;

/// Register virtual actor type consistently across all entry points
///
/// ## Purpose
/// Centralized helper to register virtual actor types from SDK annotations, app-config.toml,
/// or WASM applications. Ensures consistent behavior across all languages and entry points.
///
/// ## Arguments
/// * `service_locator` - ServiceLocator for accessing VirtualActorManager
/// * `actor_type` - Actor type (e.g., "GenServer", "read-state-tracker")
/// * `namespace` - Namespace for isolation
/// * `facets` - Optional vector of facet trait objects (from SDK)
/// * `proto_facets` - Optional vector of proto facets (from WASM app-config.toml)
/// * `actor_config` - Optional actor configuration
/// * `tenant_id` - Optional tenant ID (defaults to empty for type-level registration)
/// * `init_config_template` - Optional init config template for WASM actors (JSON bytes)
///   Preserves config structure from ApplicationSpec's ChildSpec.args for virtual actor activation
///
/// ## Returns
/// `Ok(())` if registration succeeded, `Err` if failed (logged but non-fatal)
///
/// ## Design
/// - Extracts facet_config from facets consistently using `extract_facet_config_for_registration()`
/// - Uses keyed format required by `register_virtual_actor_type()`
/// - Logs errors but doesn't fail (actor will still work, just no auto-activation)
/// - Idempotent (safe to call multiple times)
pub async fn register_virtual_actor_type_consistent(
    service_locator: &std::sync::Arc<dyn crate::ServiceLocator>,
    actor_type: String,
    namespace: String,
    facets: Option<&[Box<dyn Facet>]>,
    proto_facets: Option<&[ProtoFacet]>,
    actor_config: Option<ActorConfig>,
    tenant_id: Option<String>,
    init_config_template: Option<Vec<u8>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Get VirtualActorManager
    let virtual_actor_manager = service_locator.virtual_actor_manager().await
        .ok_or_else(|| "VirtualActorManager not available".to_string())?;
    
    // Extract facet_config consistently from facets
    let facet_config = extract_facet_config_for_registration(facets, proto_facets);
    
    // Register virtual actor type (idempotent)
    virtual_actor_manager.register_virtual_actor_type(
        actor_type.clone(),
        actor_config,
        namespace.clone(),
        facet_config,
        tenant_id,
        init_config_template,
    ).await.map_err(|e| {
        tracing::warn!(
            actor_type = %actor_type,
            namespace = %namespace,
            error = %e,
            "Failed to register virtual actor type (actor will still work, but auto-activation may fail)"
        );
        Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>
    })?;
    
    tracing::debug!(
        actor_type = %actor_type,
        namespace = %namespace,
        "Registered virtual actor type for auto-activation"
    );
    
    Ok(())
}
