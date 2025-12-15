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

//! Test helper functions to replace deprecated Node methods

use plexspaces_actor::{ActorRef, RegularActorWrapper};
use plexspaces_core::{ActorId, ActorRegistry, MessageSender};
use plexspaces_node::Node;
use std::sync::Arc;

/// Lookup ActorRef for an actor (replaces Node::lookup_actor_ref)
pub async fn lookup_actor_ref(
    node: &Node,
    actor_id: &ActorId,
) -> Result<Option<ActorRef>, plexspaces_node::NodeError> {
    // Normalize actor ID to include node ID if missing
    let actor_id = normalize_actor_id(node, actor_id);
    
    // Get ActorRegistry from ServiceLocator
    let actor_registry: Arc<ActorRegistry> = node.service_locator().get_service().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string()))?;
    
    // Check if Actor trait object exists in registry
    if let Some(_actor_trait) = actor_registry.lookup_actor(&actor_id).await {
        // Actor exists - create ActorRef pointing to local node
        Ok(Some(ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        )))
    } else {
        // Check routing for remote actors
        let routing = actor_registry.lookup_routing(&actor_id).await
            .map_err(|e| plexspaces_node::NodeError::ActorRefCreationFailed(actor_id.clone(), e.to_string()))?;
        
        if let Some(routing_info) = routing {
            if routing_info.is_local {
                // Local actor but no Actor trait - actor doesn't exist
                Ok(None)
            } else {
                // Remote actor
                Ok(Some(ActorRef::remote(
                    actor_id.clone(),
                    routing_info.node_id,
                    node.service_locator().clone(),
                )))
            }
        } else {
            Ok(None)
        }
    }
}

/// Activate a virtual actor (replaces Node::activate_virtual_actor)
pub async fn activate_virtual_actor(
    node: &Node,
    actor_id: &ActorId,
) -> Result<ActorRef, plexspaces_node::NodeError> {
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    
    // Normalize actor ID
    let actor_id = normalize_actor_id(node, actor_id);
    
    // Get ActorFactory from ServiceLocator
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorFactory not found".to_string()))?;
    
    // Use ActorFactory to activate
    actor_factory.activate_virtual_actor(&actor_id).await
        .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), e.to_string()))?;
    
    // Get ActorRef from ActorRegistry
    lookup_actor_ref(node, &actor_id).await?
        .ok_or_else(|| plexspaces_node::NodeError::ActorNotFound(actor_id))
}

/// Get or activate an actor (replaces Node::get_or_activate_actor)
pub async fn get_or_activate_actor_helper<F, Fut>(
    node: &Node,
    actor_id: ActorId,
    actor_factory: F,
) -> Result<ActorRef, plexspaces_node::NodeError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<plexspaces_actor::Actor, plexspaces_node::NodeError>>,
{
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    
    // Normalize actor ID
    let actor_id = normalize_actor_id(node, &actor_id);
    
    // Get ActorRegistry and ActorFactory from ServiceLocator
    let actor_registry: Arc<ActorRegistry> = node.service_locator().get_service().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string()))?;
    let actor_factory_impl: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorFactory not found".to_string()))?;
    
    // Check if actor already exists (activated or virtual)
    if actor_registry.is_actor_activated(&actor_id).await {
        // Actor exists - get ActorRef
        lookup_actor_ref(node, &actor_id).await?
            .ok_or_else(|| plexspaces_node::NodeError::ActorNotFound(actor_id.clone()))
    } else {
        // Check routing for remote actors
        let routing = actor_registry.lookup_routing(&actor_id).await
            .map_err(|e| plexspaces_node::NodeError::ActorRefCreationFailed(actor_id.clone(), e.to_string()))?;
        
        if let Some(routing_info) = routing {
            if routing_info.is_local {
                // Local actor but not activated - create and spawn it
                let actor = actor_factory().await?;
                actor_factory_impl.spawn_built_actor(Arc::new(actor), None, None).await
                    .map_err(|e| plexspaces_node::NodeError::ConfigError(format!("Failed to spawn actor: {}", e)))?;
                
                // Get ActorRef
                lookup_actor_ref(node, &actor_id).await?
                    .ok_or_else(|| plexspaces_node::NodeError::ActorNotFound(actor_id.clone()))
            } else {
                // Remote actor
                Ok(ActorRef::remote(
                    actor_id.clone(),
                    routing_info.node_id,
                    node.service_locator().clone(),
                ))
            }
        } else {
            // Actor doesn't exist - create and spawn it
            let actor = actor_factory().await?;
            let actor_any: Arc<dyn std::any::Any + Send + Sync> = Arc::new(actor);
            actor_factory_impl.spawn_built_actor(actor_any).await
                .map_err(|e| plexspaces_node::NodeError::ConfigError(format!("Failed to spawn actor: {}", e)))?;
            
            // Get ActorRef
            lookup_actor_ref(node, &actor_id).await?
                .ok_or_else(|| plexspaces_node::NodeError::ActorNotFound(actor_id.clone()))
        }
    }
}

/// Spawn actor builder (replaces Node::spawn_actor_builder)
/// Note: Use ActorBuilder from actor crate directly
pub fn spawn_actor_builder_helper(_node: &Node) {
    // NodeActorBuilder removed - use ActorBuilder from actor crate directly
    // This helper is kept for compatibility but doesn't do anything
    // Tests should use ActorBuilder::new(...).spawn(node.service_locator().clone())
}

/// Spawn remote actor (replaces Node::spawn_remote)
/// Note: This is deprecated - use ActorService gRPC client directly
pub async fn spawn_remote_helper(
    node: &Node,
    _target_node_id: &plexspaces_node::NodeId,
    _actor_type: &str,
    _initial_state: Vec<u8>,
    _config: Option<plexspaces_proto::v1::actor::ActorConfig>,
    _labels: std::collections::HashMap<String, String>,
) -> Result<ActorRef, plexspaces_node::NodeError> {
    Err(plexspaces_node::NodeError::ConfigError(
        "spawn_remote is deprecated - use ActorService gRPC client directly".to_string()
    ))
}

/// Helper to register an actor with MessageSender (replaces register_local)
pub async fn register_actor_with_message_sender(
    node: &Node,
    actor_id: &str,
    mailbox: Arc<plexspaces_mailbox::Mailbox>,
) {
    use plexspaces_actor::RegularActorWrapper;
    use plexspaces_core::MessageSender;
    let wrapper = Arc::new(RegularActorWrapper::new(
        actor_id.to_string(),
        mailbox,
        node.service_locator().clone(),
    ));
    node.actor_registry().register_actor(actor_id.to_string(), wrapper, None, None, None).await;
}

/// Unregister an actor (replaces Node::unregister_actor)
pub async fn unregister_actor_helper(
    node: &Node,
    actor_id: &ActorId,
) -> Result<(), plexspaces_node::NodeError> {
    // Delegate to ActorRegistry (handles all cleanup)
    node.actor_registry().unregister_with_cleanup(actor_id).await
        .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone(), e.to_string()))
}

/// Find actor location (replaces Node::find_actor)
pub async fn find_actor_helper(
    node: &Node,
    actor_id: &ActorId,
) -> Result<plexspaces_node::ActorLocation, plexspaces_node::NodeError> {
    // Normalize actor ID
    let actor_id = normalize_actor_id(node, actor_id);
    
    // Get ActorRegistry from ServiceLocator
    let actor_registry: Arc<ActorRegistry> = node.service_locator().get_service().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string()))?;
    
    // Check routing info
    let routing = actor_registry.lookup_routing(&actor_id).await
        .map_err(|e| plexspaces_node::NodeError::ActorNotFound(actor_id.clone()))?;
    
    if let Some(routing_info) = routing {
        if routing_info.is_local {
            // Check if actor is actually activated
            if actor_registry.is_actor_activated(&actor_id).await {
                Ok(plexspaces_node::ActorLocation::Local(actor_id.clone()))
            } else {
            // Check if it's a virtual actor (use Node's method if available, otherwise skip)
            // Virtual actors are handled by VirtualActorManager which is accessed via Node
            // For now, if actor is not activated, it doesn't exist locally
                Err(plexspaces_node::NodeError::ActorNotFound(actor_id))
            }
        } else {
            // Remote actor
            Ok(plexspaces_node::ActorLocation::Remote(plexspaces_node::NodeId::from(routing_info.node_id)))
        }
    } else {
        // No routing info - actor doesn't exist
        Err(plexspaces_node::NodeError::ActorNotFound(actor_id))
    }
}

/// Spawn actor using ActorFactory (replaces Node::spawn_actor)
pub async fn spawn_actor_helper(
    node: &Node,
    actor: plexspaces_actor::Actor,
) -> Result<ActorRef, plexspaces_node::NodeError> {
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    
    // Get ActorFactory from ServiceLocator
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError(
            "ActorFactory not found in ServiceLocator. Ensure Node::start() has been called.".to_string()
        ))?;
    
    // Extract actor ID before spawning
    let actor_id = actor.id().clone();
    
    // Use ActorFactory to spawn the actor
    let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None).await
        .map_err(|e| plexspaces_node::NodeError::ConfigError(format!("Failed to spawn actor via ActorFactory: {}", e)))?;
    
    // Create ActorRef from the actor ID (actor is now registered in ActorRegistry)
    Ok(ActorRef::remote(
        actor_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    ))
}

/// Normalize actor ID to include node ID if missing
fn normalize_actor_id(node: &Node, actor_id: &ActorId) -> ActorId {
    if let Ok((actor_name, node_id)) = plexspaces_core::ActorRef::parse_actor_id(actor_id) {
        // Actor ID already has @ format
        if node_id == node.id().as_str() {
            actor_id.clone()
        } else {
            format!("{}@{}", actor_name, node.id().as_str())
        }
    } else {
        // Actor ID doesn't have @ format - append node ID
        format!("{}@{}", actor_id, node.id().as_str())
    }
}

