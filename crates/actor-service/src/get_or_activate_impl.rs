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

//! Unified implementation of get_or_activate_actor
//!
//! This module provides a consistent, rock-solid implementation that:
//! 1. Properly handles virtual actors (eager and lazy activation)
//! 2. Uses VirtualActorManager to track virtual actors
//! 3. Uses ActorFactory for all actor operations
//! 4. Handles both existing and new actors correctly

use std::sync::Arc;
use plexspaces_core::{ActorRegistry, VirtualActorManager, RequestContext, service_locator::service_names};
use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_proto::actor::v1::GetOrActivateActorRequest;
use tonic::Status;

/// Unified get_or_activate_actor implementation
///
/// ## Flow
/// 1. Check if actor exists and is active
/// 2. If exists but not active:
///    - Check if virtual (using VirtualActorManager)
///    - If virtual: use ActorFactory.activate_virtual_actor
///    - If not virtual: create new actor
/// 3. If doesn't exist: create new actor with ActorFactory.spawn_actor
///
/// ## Returns
/// (was_activated, actor_id) - whether actor was activated/created and the final actor_id
pub async fn get_or_activate_actor_impl(
    service_locator: &Arc<plexspaces_core::ServiceLocator>,
    local_node_id: &str,
    ctx: &RequestContext,
    req: &GetOrActivateActorRequest,
) -> Result<(bool, String), Status> {
    // Validate actor_id is not empty
    if req.actor_id.is_empty() {
        return Err(Status::invalid_argument("actor_id is required"));
    }
    
    let actor_id: plexspaces_core::ActorId = req.actor_id.clone();
    
    // Get required services
    let actor_registry: Arc<ActorRegistry> = service_locator
        .get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY)
        .await
        .ok_or_else(|| Status::internal("ActorRegistry not found in ServiceLocator"))?;
    
    let virtual_actor_manager: Arc<VirtualActorManager> = service_locator
        .get_service_by_name::<VirtualActorManager>(service_names::VIRTUAL_ACTOR_MANAGER)
        .await
        .ok_or_else(|| Status::internal("VirtualActorManager not found in ServiceLocator"))?;
    
    let actor_factory: Arc<ActorFactoryImpl> = service_locator
        .get_service_by_name::<ActorFactoryImpl>(service_names::ACTOR_FACTORY_IMPL)
        .await
        .ok_or_else(|| Status::internal("ActorFactory not found in ServiceLocator"))?;
    
    // Create RequestContext for routing lookup (use internal context for system operations)
    let internal_ctx = RequestContext::internal();
    
    // Try to lookup routing - if it fails, assume actor doesn't exist (will create it)
    let routing = actor_registry
        .lookup_routing(&internal_ctx, &actor_id)
        .await
        .ok()
        .flatten();
    
    let (was_activated, final_actor_id) = match routing {
        Some(routing_info) if routing_info.is_local => {
            // Actor ID points to local node
            // Check if actor is active
            if actor_registry.lookup_actor(&actor_id).await.is_some() {
                // Actor exists and is active
                tracing::debug!(actor_id = %actor_id, "Actor already exists and is active");
                (false, actor_id)
            } else {
                // Actor ID is for local node but actor is not active
                // Check if it's a virtual actor
                if virtual_actor_manager.is_virtual(&actor_id).await {
                    // Virtual actor - activate it
                    tracing::debug!(actor_id = %actor_id, "Virtual actor exists but not active - activating");
                    actor_factory
                        .activate_virtual_actor(&actor_id)
                        .await
                        .map_err(|e| {
                            Status::internal(format!("Failed to activate virtual actor: {}", e))
                        })?;
                    (true, actor_id)
                } else {
                    // Not virtual - need to create it
                    // Check if actor_type is provided for creation
                    if req.actor_type.is_empty() {
                        return Err(Status::invalid_argument(
                            "actor_type is required when creating new actor",
                        ));
                    }
                    
                    // Create actor using ActorFactory
                    tracing::debug!(actor_id = %actor_id, "Actor not found - creating new actor");
                    actor_factory
                        .spawn_actor(
                            ctx,
                            &actor_id,
                            &req.actor_type,
                            req.initial_state.clone(),
                            req.config.clone(),
                            std::collections::HashMap::new(), // labels (empty for now)
                            vec![], // facets (empty - facets should be attached via config or separate API)
                        )
                        .await
                        .map_err(|e| {
                            Status::internal(format!("Failed to spawn actor: {}", e))
                        })?;
                    (true, actor_id)
                }
            }
        }
        Some(_) => {
            // Actor exists on remote node - return remote ActorRef
            tracing::debug!(actor_id = %actor_id, "Actor exists on remote node");
            (false, actor_id)
        }
        None => {
            // Actor doesn't exist (node_id not found or parsing failed) - need to create it locally
            // Check if actor_type is provided for creation
            if req.actor_type.is_empty() {
                return Err(Status::invalid_argument(
                    "actor_type is required when creating new actor",
                ));
            }
            
            // Ensure actor_id has local node_id
            let local_actor_id = if actor_id.contains('@') {
                // Parse and replace node_id with local node_id
                if let Some((actor_name, _)) = actor_id.split_once('@') {
                    format!("{}@{}", actor_name, local_node_id)
                } else {
                    actor_id.clone()
                }
            } else {
                format!("{}@{}", actor_id, local_node_id)
            };
            
            // Create actor using ActorFactory
            tracing::debug!(actor_id = %local_actor_id, "Actor doesn't exist - creating new actor");
            actor_factory
                .spawn_actor(
                    ctx,
                    &local_actor_id,
                    &req.actor_type,
                    req.initial_state.clone(),
                    req.config.clone(),
                    std::collections::HashMap::new(), // labels (empty for now)
                    vec![], // facets (empty - facets should be attached via config or separate API)
                )
                .await
                .map_err(|e| {
                    Status::internal(format!("Failed to spawn actor: {}", e))
                })?;
            
            (true, local_actor_id)
        }
    };
    
    Ok((was_activated, final_actor_id))
}

