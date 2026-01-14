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

use plexspaces_actor::ActorRef;
use plexspaces_core::{ActorId, ActorRegistry, MessageSender};
use plexspaces_node::Node;
use std::sync::Arc;
use std::time::Duration;

/// Lookup ActorRef for an actor (replaces Node::lookup_actor_ref)
pub async fn lookup_actor_ref(
    node: &Node,
    actor_id: &ActorId,
) -> Result<Option<ActorRef>, plexspaces_node::NodeError> {
    // Normalize actor ID to include node ID if missing
    let actor_id = normalize_actor_id(node, actor_id);
    
    // Get ActorRegistry from ServiceLocator
    let actor_registry: Arc<ActorRegistry> = node.service_locator().actor_registry().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string()))?;
    
    // Check if MessageSender exists in registry (lookup_actor returns MessageSender)
    // For local actors, the MessageSender is an ActorRef, but we can't downcast it directly
    // Instead, we'll use the routing info to determine if it's local, then create the appropriate ActorRef
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    let routing = actor_registry.lookup_routing(&ctx, &actor_id).await
        .map_err(|_e| plexspaces_node::NodeError::ActorRefCreationFailed(actor_id.clone(), "Failed to lookup routing".to_string()))?;
    
    if let Some(routing_info) = routing {
        if routing_info.is_local {
            // Local actor - check if it exists in registry
            if let Some(message_sender) = actor_registry.lookup_actor(&actor_id).await {
                // CRITICAL: Check if MessageSender is VirtualActorWrapper (lazy/suspended virtual actor)
                // For lazy virtual actors that aren't active, we need to route through VirtualActorWrapper
                // VirtualActorWrapper.tell() will activate the actor automatically
                // Use type_name to check if it's VirtualActorWrapper (hacky but works without as_any())
                let type_name = std::any::type_name_of_val(&*message_sender);
                if type_name.contains("VirtualActorWrapper") {
                    // VirtualActorWrapper found - actor is registered but not active
                    // Return remote ActorRef which will route through VirtualActorWrapper.tell() for activation
                    // This ensures lazy activation works correctly
                    Ok(Some(ActorRef::remote(
                        actor_id.clone(),
                        node.id().as_str().to_string(),
                        node.service_locator().clone(),
                    )))
                } else {
                    // ActorRef found - actor is active, get mailbox from actor instance
                    if let Some(actor_instance) = actor_registry.get_actor_instance(&actor_id).await {
                        use plexspaces_actor::Actor;
                        if let Some(actor) = actor_instance.downcast_ref::<plexspaces_actor::Actor>() {
                            // Get mailbox from actor (works for active actors)
                            let mailbox = actor.mailbox().clone();
                            Ok(Some(ActorRef::local(
                                actor_id.clone(),
                                mailbox,
                                node.service_locator().clone(),
                            )))
                        } else {
                            // Can't get mailbox - return remote ActorRef as fallback
                            Ok(Some(ActorRef::remote(
                                actor_id.clone(),
                                node.id().as_str().to_string(),
                                node.service_locator().clone(),
                            )))
                        }
                    } else {
                        // No actor instance but MessageSender exists - return remote ActorRef
                        Ok(Some(ActorRef::remote(
                            actor_id.clone(),
                            node.id().as_str().to_string(),
                            node.service_locator().clone(),
                        )))
                    }
                }
            } else {
                Ok(None)
            }
        } else {
            // Remote actor
            Ok(Some(ActorRef::remote(
                actor_id.clone(),
                routing_info.node_id,
                node.service_locator().clone(),
            )))
        }
    } else if actor_registry.lookup_actor(&actor_id).await.is_some() {
        // Actor exists but no routing info - assume local
        Ok(Some(ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        )))
    } else {
        // Check routing for remote actors
        let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
        let routing = actor_registry.lookup_routing(&ctx, &actor_id).await
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

/// Wait for virtual actor to be activated (polls VirtualActorManager)
/// NOTE: This should only be needed if activation is truly async (e.g., via message queue)
/// For synchronous activation via activate_virtual_actor(), this should not be needed
pub async fn wait_for_virtual_actor_activation(
    node: &Node,
    actor_id: &ActorId,
    timeout: Duration,
) -> bool {
    use plexspaces_core::VirtualActorManager;
    use plexspaces_core::service_names;
    use tokio::time::Instant;
    use tokio::task::yield_now;
    
    let manager: Arc<VirtualActorManager> = node.service_locator()
        .virtual_actor_manager()
        .await
        .expect("VirtualActorManager not found");
    
    let start = Instant::now();
    let mut last_check = Instant::now();
    
    while start.elapsed() < timeout {
        if manager.is_active(actor_id).await {
            return true;
        }
        
        // Adaptive polling: check more frequently at first, then back off
        let elapsed = last_check.elapsed();
        let sleep_duration = if elapsed < Duration::from_millis(100) {
            Duration::from_millis(10) // Fast polling initially
        } else if elapsed < Duration::from_millis(500) {
            Duration::from_millis(50) // Medium polling
        } else {
            Duration::from_millis(100) // Slower polling after 500ms
        };
        
        yield_now().await;
        tokio::time::sleep(sleep_duration).await;
        last_check = Instant::now();
    }
    
    false
}

/// Activate a virtual actor (replaces Node::activate_virtual_actor)
pub async fn activate_virtual_actor(
    node: &Node,
    actor_id: &ActorId,
) -> Result<ActorRef, plexspaces_node::NodeError> {
    use plexspaces_actor::{ActorFactory, get_actor_factory};
    
    // Normalize actor ID
    let actor_id = normalize_actor_id(node, actor_id);
    
    // Get ActorFactory from ServiceLocator
    let actor_factory: Arc<dyn ActorFactory> = get_actor_factory(node.service_locator().as_ref()).await
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
    use plexspaces_actor::{ActorFactory, get_actor_factory};
    
    // Normalize actor ID
    let actor_id = normalize_actor_id(node, &actor_id);
    
    // Get ActorRegistry and ActorFactory from ServiceLocator
    let actor_registry: Arc<ActorRegistry> = node.service_locator().actor_registry().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string()))?;
    let actor_factory_impl: Arc<dyn ActorFactory> = get_actor_factory(node.service_locator().as_ref()).await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorFactory not found".to_string()))?;
    
    // Check if actor already exists (activated or virtual)
    if actor_registry.is_actor_activated(&actor_id).await {
        // Actor exists - get ActorRef
        lookup_actor_ref(node, &actor_id).await?
            .ok_or_else(|| plexspaces_node::NodeError::ActorNotFound(actor_id.clone()))
    } else {
        // Check routing for remote actors
        let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
        let routing = actor_registry.lookup_routing(&ctx, &actor_id).await
            .map_err(|e| plexspaces_node::NodeError::ActorRefCreationFailed(actor_id.clone(), e.to_string()))?;
        
        if let Some(routing_info) = routing {
            if routing_info.is_local {
                // Local actor but not activated - use spawn_built_actor to preserve behavior and facets
                // spawn_built_actor is the correct choice when we already have a built actor with
                // custom behavior and facets attached
                let actor = actor_factory().await?;
                let mailbox = actor.mailbox().clone();
                
                // Extract actor_type from behavior
                let behavior_type = actor.behavior().read().await.behavior_type();
                let actor_type = match behavior_type {
                    plexspaces_core::BehaviorType::GenServer => Some("GenServer".to_string()),
                    plexspaces_core::BehaviorType::GenEvent => Some("GenEvent".to_string()),
                    plexspaces_core::BehaviorType::GenStateMachine => Some("GenStateMachine".to_string()),
                    plexspaces_core::BehaviorType::Workflow => Some("Workflow".to_string()),
                    plexspaces_core::BehaviorType::Custom(ref s) => Some(s.clone()),
                };
                
                // Use spawn_built_actor to preserve the built actor with its behavior and facets
                // Note: spawn_built_actor normalizes the actor ID internally, so we need to get
                // the actual registered ID after spawning
                let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
                let _message_sender = actor_factory_impl.spawn_built_actor(
                    &ctx,
                    std::sync::Arc::new(actor),
                    actor_type,
                ).await
                    .map_err(|e| plexspaces_node::NodeError::ConfigError(format!("Failed to spawn actor: {}", e)))?;
                
                // Registration is synchronous - spawn_built_actor awaits actor.start() which
                // calls register_in_registry().await, so the actor is already registered.
                // Get ActorRef from registry to ensure we have the correct ID (normalized)
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
            // Actor doesn't exist - use spawn_built_actor to preserve behavior and facets
            let actor = actor_factory().await?;
            let mailbox = actor.mailbox().clone();
            
            // Extract actor_type from behavior
            let behavior_type = actor.behavior().read().await.behavior_type();
            let actor_type = match behavior_type {
                plexspaces_core::BehaviorType::GenServer => Some("GenServer".to_string()),
                plexspaces_core::BehaviorType::GenEvent => Some("GenEvent".to_string()),
                plexspaces_core::BehaviorType::GenStateMachine => Some("GenStateMachine".to_string()),
                plexspaces_core::BehaviorType::Workflow => Some("Workflow".to_string()),
                plexspaces_core::BehaviorType::Custom(ref s) => Some(s.clone()),
            };
            
            // Use spawn_built_actor to preserve the built actor with its behavior and facets
            // Note: spawn_built_actor normalizes the actor ID internally
            let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
            let _message_sender = actor_factory_impl.spawn_built_actor(
                &ctx,
                std::sync::Arc::new(actor),
                actor_type,
            ).await
                .map_err(|e| plexspaces_node::NodeError::ConfigError(format!("Failed to spawn actor: {}", e)))?;
            
            // Registration is synchronous - get ActorRef from registry to ensure we have the correct ID (normalized)
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

/// Helper to register an actor with MessageSender (replaces register_local)
pub async fn register_actor_with_message_sender(
    node: &Node,
    actor_id: &str,
    mailbox: Arc<plexspaces_mailbox::Mailbox>,
) {
    
    use plexspaces_core::MessageSender;
    let wrapper = Arc::new(ActorRef::local(
        actor_id.to_string(),
        mailbox,
        node.service_locator().clone(),
    ));
    let actor_registry: Arc<ActorRegistry> = node.service_locator().actor_registry().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())).unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    actor_registry.register_actor(&ctx, actor_id.to_string(), wrapper, None, None, None).await;
}

/// Unregister an actor (replaces Node::unregister_actor)
pub async fn unregister_actor_helper(
    node: &Node,
    actor_id: &ActorId,
) -> Result<(), plexspaces_node::NodeError> {
    // Delegate to ActorRegistry (handles all cleanup)
    let actor_registry: Arc<ActorRegistry> = node.service_locator().actor_registry().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string()))?;
    actor_registry.unregister_with_cleanup(actor_id).await
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
    let actor_registry: Arc<ActorRegistry> = node.service_locator().actor_registry().await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string()))?;
    
    // Check routing info
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    let routing = actor_registry.lookup_routing(&ctx, &actor_id).await
        .map_err(|_e| plexspaces_node::NodeError::ActorNotFound(actor_id.clone()))?;
    
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
    use plexspaces_actor::{ActorFactory, get_actor_factory};
    
    // Get ActorFactory from ServiceLocator
    let actor_factory: Arc<dyn ActorFactory> = get_actor_factory(node.service_locator().as_ref()).await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError(
            "ActorFactory not found in ServiceLocator. Ensure Node::start() has been called.".to_string()
        ))?;
    
    // Extract actor_id and actor_type before wrapping in Arc
    let actor_id = actor.id().clone();
    let behavior_type = actor.behavior().read().await.behavior_type();
    let actor_type = match behavior_type {
        plexspaces_core::BehaviorType::GenServer => Some("GenServer".to_string()),
        plexspaces_core::BehaviorType::GenEvent => Some("GenEvent".to_string()),
        plexspaces_core::BehaviorType::GenStateMachine => Some("GenStateMachine".to_string()),
        plexspaces_core::BehaviorType::Workflow => Some("Workflow".to_string()),
        plexspaces_core::BehaviorType::Custom(ref s) => Some(s.clone()),
    };
    
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    
    // Use spawn_built_actor to preserve facets attached to the actor
    let actor_arc = Arc::new(actor);
    let _message_sender = actor_factory.spawn_built_actor(
        &ctx,
        actor_arc,
        actor_type,
    ).await
        .map_err(|e| plexspaces_node::NodeError::ConfigError(format!("Failed to spawn actor via ActorFactory: {}", e)))?;
    
    // Get ActorRef from ActorRegistry (should be local since we just spawned it)
    // Note: actor_id may have been normalized by spawn_built_actor, so we use the original
    // and let lookup_actor_ref normalize it
    lookup_actor_ref(node, &actor_id).await?
        .ok_or_else(|| plexspaces_node::NodeError::ActorNotFound(actor_id))
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

