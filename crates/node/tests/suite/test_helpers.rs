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
use tonic::metadata::MetadataValue;
use tonic::Request;

/// Builds a gRPC Request with x-tenant-id and x-namespace metadata so ApplicationService
/// (request_context_from_grpc_request) accepts the request when auth is enabled.
pub fn app_request_with_tenant<T: Send>(body: T) -> Request<T> {
    let mut req = Request::new(body);
    req.metadata_mut().insert(
        "x-tenant-id",
        MetadataValue::try_from("test-tenant").unwrap(),
    );
    req.metadata_mut()
        .insert("x-namespace", MetadataValue::try_from("default").unwrap());
    req
}

fn parse_node_id(actor_id: &ActorId) -> Option<String> {
    plexspaces_core::actor_id::parse_actor_id(actor_id)
        .ok()
        .map(|parsed| parsed.node_id)
}

async fn actor_exists_locally(actor_registry: &ActorRegistry, actor_id: &ActorId) -> bool {
    if actor_registry.lookup_actor(actor_id).await.is_some() {
        return true;
    }
    if actor_registry
        .registered_actor_ids()
        .read()
        .await
        .contains(actor_id)
    {
        return true;
    }
    false
}

/// Lookup ActorRef for an actor (replaces Node::lookup_actor_ref)
pub async fn lookup_actor_ref(
    node: &Node,
    actor_id: &ActorId,
) -> Result<Option<ActorRef>, plexspaces_node::NodeError> {
    // Normalize actor ID to include node ID if missing
    let actor_id = normalize_actor_id(node, actor_id);

    // Get ActorRegistry from ServiceLocator
    let actor_registry: Arc<ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })?;

    if actor_exists_locally(&actor_registry, &actor_id).await {
        Ok(Some(ActorRef::remote(
            actor_id.clone(),
            "".to_string(),
            "".to_string(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        )))
    } else {
        let node_id = parse_node_id(&actor_id).unwrap_or_else(|| node.id().as_str().to_string());
        if node_id != node.id().as_str() {
            Ok(Some(ActorRef::remote(
                actor_id.clone(),
                String::new(),
                node_id,
                node.service_locator().clone(),
            )))
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
    use plexspaces_core::service_names;
    use plexspaces_core::VirtualActorManager;
    use tokio::task::yield_now;
    use tokio::time::Instant;

    let manager: Arc<VirtualActorManager> = node
        .service_locator()
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
    // Normalize actor ID
    let actor_id = normalize_actor_id(node, actor_id);

    // Get ActorFactory from ServiceLocator
    let actor_factory: Arc<dyn plexspaces_actor::ActorFactory> = node
        .service_locator()
        .get_actor_factory()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorFactory not found".to_string())
        })?;

    // Use ActorFactory to activate
    actor_factory
        .activate_virtual_actor(&actor_id)
        .await
        .map_err(|e| {
            plexspaces_node::NodeError::ActorRegistrationFailed(
                actor_id.clone().into(),
                e.to_string(),
            )
        })?;

    // Get ActorRef from ActorRegistry
    lookup_actor_ref(node, &actor_id)
        .await?
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
    // Normalize actor ID
    let actor_id = normalize_actor_id(node, &actor_id);

    // Get ActorRegistry and ActorFactory from ServiceLocator
    let actor_registry: Arc<ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })?;
    let actor_factory_trait: Arc<dyn plexspaces_core::ActorFactory> = node
        .service_locator()
        .get_actor_factory()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorFactory not found".to_string())
        })?;

    // Check if actor already exists (activated or virtual)
    if actor_registry.is_actor_activated(&actor_id).await {
        // Actor exists - get ActorRef
        lookup_actor_ref(node, &actor_id)
            .await?
            .ok_or_else(|| plexspaces_node::NodeError::ActorNotFound(actor_id.clone()))
    } else {
        let parsed_node_id =
            parse_node_id(&actor_id).unwrap_or_else(|| node.id().as_str().to_string());
        if parsed_node_id != node.id().as_str() {
            Ok(ActorRef::remote(
                actor_id.clone(),
                String::new(),
                parsed_node_id,
                node.service_locator().clone(),
            ))
        } else {
            // Actor doesn't exist - use spawn_built_actor to preserve behavior and facets
            let actor = actor_factory().await?;
            let mailbox = actor.mailbox().clone();

            // Extract actor_type from behavior
            let behavior_type = actor.behavior().read().await.behavior_type();
            let actor_type = match behavior_type {
                plexspaces_core::BehaviorType::GenServer => Some("GenServer".to_string()),
                plexspaces_core::BehaviorType::GenEvent => Some("GenEvent".to_string()),
                plexspaces_core::BehaviorType::GenStateMachine => {
                    Some("GenStateMachine".to_string())
                }
                plexspaces_core::BehaviorType::Workflow => Some("Workflow".to_string()),
                plexspaces_core::BehaviorType::Custom(ref s) => Some(s.clone()),
            };

            // Use spawn_actor to create the actor
            // Note: spawn_actor normalizes the actor ID internally
            let ctx = plexspaces_core::RequestContext::new_without_auth(
                "default".to_string(),
                "default".to_string(),
            );
            let actor_type_str = actor_type.unwrap_or_else(|| "GenServer".to_string());
            let _message_sender = actor_factory_trait
                .spawn_actor(
                    &ctx,
                    &actor_id,
                    &actor_type_str,
                    vec![],
                    None,
                    std::collections::HashMap::new(),
                    vec![],
                )
                .await
                .map_err(|e| {
                    plexspaces_node::NodeError::ConfigError(format!("Failed to spawn actor: {}", e))
                })?;

            // Registration is synchronous - get ActorRef from registry to ensure we have the correct ID (normalized)
            lookup_actor_ref(node, &actor_id)
                .await?
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
        String::new(),
        mailbox,
        node.service_locator().clone(),
    ));
    let actor_registry: Arc<ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })
        .unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    actor_registry
        .register_actor(
            &ctx,
            actor_id.to_string(),
            wrapper,
            "TestActor".to_string(),
            None,
            None,
            None,
        )
        .await;
}

/// Unregister an actor (replaces Node::unregister_actor)
pub async fn unregister_actor_helper(
    node: &Node,
    actor_id: &ActorId,
) -> Result<(), plexspaces_node::NodeError> {
    // Delegate to ActorRegistry (handles all cleanup)
    let actor_registry: Arc<ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })?;
    actor_registry
        .unregister_with_cleanup(actor_id)
        .await
        .map_err(|e| {
            plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone(), e.to_string())
        })
}

/// Find actor location (replaces Node::find_actor)
pub async fn find_actor_helper(
    node: &Node,
    actor_id: &ActorId,
) -> Result<plexspaces_node::ActorLocation, plexspaces_node::NodeError> {
    // Normalize actor ID
    let actor_id = normalize_actor_id(node, actor_id);

    // Get ActorRegistry from ServiceLocator
    let actor_registry: Arc<ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })?;

    if actor_exists_locally(&actor_registry, &actor_id).await {
        Ok(plexspaces_node::ActorLocation::Local(actor_id.clone()))
    } else {
        let node_id = parse_node_id(&actor_id).unwrap_or_else(|| node.id().as_str().to_string());
        if node_id != node.id().as_str() {
            Ok(plexspaces_node::ActorLocation::Remote(
                plexspaces_node::NodeId::from(node_id),
            ))
        } else {
            Err(plexspaces_node::NodeError::ActorNotFound(actor_id))
        }
    }
}

/// Spawn actor using ActorFactory (replaces Node::spawn_actor)
pub async fn spawn_actor_helper(
    node: &Node,
    actor: plexspaces_actor::Actor,
) -> Result<ActorRef, plexspaces_node::NodeError> {
    // Get ActorFactory from ServiceLocator
    let actor_factory: Arc<dyn plexspaces_actor::ActorFactory> = node
        .service_locator()
        .get_actor_factory()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError(
                "ActorFactory not found in ServiceLocator. Ensure Node::start() has been called."
                    .to_string(),
            )
        })?;

    // Extract actor_id and actor_type before spawning
    let actor_id = actor.id().clone();
    let behavior_type = actor.behavior().read().await.behavior_type();
    let actor_type_str = match behavior_type {
        plexspaces_core::BehaviorType::GenServer => "GenServer".to_string(),
        plexspaces_core::BehaviorType::GenEvent => "GenEvent".to_string(),
        plexspaces_core::BehaviorType::GenStateMachine => "GenStateMachine".to_string(),
        plexspaces_core::BehaviorType::Workflow => "Workflow".to_string(),
        plexspaces_core::BehaviorType::Custom(ref s) => s.clone(),
    };

    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );

    // Use spawn_actor to create the actor
    let _message_sender = actor_factory
        .spawn_actor(
            &ctx,
            &actor_id,
            &actor_type_str,
            vec![],
            None,
            std::collections::HashMap::new(),
            vec![],
        )
        .await
        .map_err(|e| {
            plexspaces_node::NodeError::ConfigError(format!(
                "Failed to spawn actor via ActorFactory: {}",
                e
            ))
        })?;

    // Get ActorRef from ActorRegistry (should be local since we just spawned it)
    // Note: actor_id may have been normalized by spawn_built_actor, so we use the original
    // and let lookup_actor_ref normalize it
    lookup_actor_ref(node, &actor_id)
        .await?
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
