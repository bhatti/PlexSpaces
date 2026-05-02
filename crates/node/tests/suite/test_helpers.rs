// SPDX-License-Identifier: AGPL-3.0-or-later
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
use plexspaces_core::{ActorId, ActorRegistry, MessageSender, RequestContext, VirtualActorMetadata};
use plexspaces_node::Node;
use std::collections::HashSet;
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

/// Build a canonical actor ID for tests that register concrete runtime actors.
pub fn test_runtime_actor_id(name: &str, node_id: &str) -> ActorId {
    ActorId::new(name, "gen_server", "default", node_id)
        .expect("test runtime actor IDs must be valid")
}

/// Build a canonical actor ID for generic node integration tests.
pub fn test_actor_id(name: &str, node_id: &str) -> ActorId {
    ActorId::new(name, "test_actor", "default", node_id).expect("test actor IDs must be valid")
}

/// Same semantics as the former `Node::check_virtual_actor_exists` (virtual manager as source of truth).
///
/// Returns `(exists, is_active, is_virtual)` where `exists` means instance metadata is registered
/// (`get_metadata` is `Some`), matching passivated actors that remain in the virtual registry.
pub async fn check_virtual_actor_exists_triplet(
    node: &Node,
    actor_id: &ActorId,
) -> (bool, bool, bool) {
    let Some(manager) = node.service_locator().virtual_actor_manager().await else {
        return (false, false, false);
    };
    let exists = manager.get_metadata(actor_id).await.is_some();
    let is_virtual = manager.is_virtual(actor_id).await;
    let is_active = if is_virtual {
        manager.is_active(actor_id).await
    } else {
        false
    };
    (exists, is_active, is_virtual)
}

/// Read virtual actor metadata via [`ServiceLocator::virtual_actor_manager`].
pub async fn virtual_actor_metadata_optional(
    node: &Node,
    actor_id: &ActorId,
) -> Option<VirtualActorMetadata> {
    let manager = node.service_locator().virtual_actor_manager().await?;
    manager.get_metadata(actor_id).await
}

/// Registered actor ids from the node's actor registry.
pub async fn registered_actor_ids_from_node(node: &Node) -> Result<HashSet<ActorId>, plexspaces_node::NodeError> {
    let registry = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string()))?;
    Ok(registry.registered_actor_ids().await)
}

async fn actor_exists_locally(actor_registry: &ActorRegistry, actor_id: &ActorId) -> bool {
    if actor_registry.lookup_actor(actor_id).await.is_some() {
        return true;
    }
    if actor_registry
        .registered_actor_ids()
        .await
        .contains(actor_id)
    {
        return true;
    }
    false
}

/// Lookup ActorRef for a **live** actor in the registry.
///
/// Returns the registered `ActorRef` by downcasting the `MessageSender` stored in the live
/// registry. Returns `None` if the actor is not currently active (e.g., lazy virtual actors
/// that have not yet been activated, or actors on remote nodes).
///
/// For lazy virtual actors use `registry_tell` / `registry_ask` which route through
/// `ActorRegistry` and trigger activation automatically on the first message.
pub async fn lookup_actor_ref(
    node: &Node,
    actor_id: &ActorId,
) -> Result<Option<ActorRef>, plexspaces_node::NodeError> {
    let actor_registry: Arc<ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })?;

    // Return the live ActorRef by downcasting the registered MessageSender directly.
    if let Some(sender) = actor_registry.lookup_actor(actor_id).await {
        if let Some(actor_ref) = sender.as_any().downcast_ref::<ActorRef>() {
            return Ok(Some(actor_ref.clone()));
        }
    }

    Ok(None)
}

/// Send a message to an actor via the registry (fire-and-forget).
///
/// Routes through `ActorRegistry.tell()` which automatically activates lazy virtual actors
/// on the first message. Use this instead of `ActorRef.tell()` for virtual actors.
pub async fn registry_tell(
    node: &Node,
    actor_id: &ActorId,
    message: plexspaces_core::Message,
) -> Result<(), plexspaces_node::NodeError> {
    let actor_registry: Arc<ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })?;
    let ctx = RequestContext::new_without_auth(
        "test-tenant".into(),
        actor_id.namespace().to_string(),
    );
    actor_registry
        .tell(&ctx, actor_id, message)
        .await
        .map_err(|e| plexspaces_node::NodeError::ConfigError(e.to_string()))
}

/// Send a request-reply message to an actor via the registry.
///
/// Routes through `ActorRegistry.ask()` which automatically activates lazy virtual actors.
pub async fn registry_ask(
    node: &Node,
    actor_id: &ActorId,
    message: plexspaces_core::Message,
    timeout: std::time::Duration,
) -> Result<plexspaces_core::Message, plexspaces_node::NodeError> {
    let actor_registry: Arc<ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })?;
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    actor_registry
        .ask(&ctx, actor_id, message, timeout)
        .await
        .map_err(|e| plexspaces_node::NodeError::ConfigError(e.to_string()))
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
        .activate_virtual_actor(actor_id)
        .await
        .map_err(|e| {
            plexspaces_node::NodeError::ActorRegistrationFailed(
                actor_id.clone().into(),
                e.to_string(),
            )
        })?;

    // Get ActorRef from ActorRegistry
    lookup_actor_ref(node, actor_id)
        .await?
        .ok_or_else(|| plexspaces_node::NodeError::ActorNotFound(actor_id.to_string()))
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
    actor_id: &ActorId,
    mailbox: Arc<plexspaces_mailbox::Mailbox>,
) {
    use plexspaces_core::MessageSender;
    let wrapper = Arc::new(ActorRef::local(
        actor_id.clone(),
        String::new(),
        String::new(),
        mailbox,
        node.service_locator().clone(),
        plexspaces_proto::actor::v1::ActorVisibility::ActorVisibilityPublic,
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
            actor_id.clone(),
            wrapper,
            actor_id.actor_type().to_string(),
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
    // Get ActorRegistry from ServiceLocator
    let actor_registry: Arc<ActorRegistry> = node
        .service_locator()
        .actor_registry()
        .await
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())
        })?;

    if actor_exists_locally(&actor_registry, actor_id).await {
        Ok(plexspaces_node::ActorLocation::Local(actor_id.clone()))
    } else {
        let node_id = actor_id.node_id().to_string();
        if node_id != node.id().as_str() {
            Ok(plexspaces_node::ActorLocation::Remote(
                plexspaces_node::NodeId::from(node_id),
            ))
        } else {
            Err(plexspaces_node::NodeError::ActorNotFound(
                actor_id.to_string(),
            ))
        }
    }
}

/// Spawn actor using ActorFactory (replaces Node::spawn_actor)
///
/// Uses `spawn_built_actor_impl` to register the pre-built actor directly, bypassing
/// BehaviorRegistry. This allows test nodes to spawn actors without registering a
/// BehaviorRegistry (which is only needed for type-driven spawning via actor_type string).
pub async fn spawn_actor_helper(
    node: &Node,
    actor: plexspaces_actor::Actor,
) -> Result<ActorRef, plexspaces_node::NodeError> {
    use plexspaces_actor::ActorFactoryImpl;

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

    // Downcast to ActorFactoryImpl to use spawn_built_actor_impl, which registers
    // the pre-built actor directly without going through BehaviorRegistry.
    let factory_impl = actor_factory
        .as_any()
        .downcast_ref::<ActorFactoryImpl>()
        .ok_or_else(|| {
            plexspaces_node::NodeError::ConfigError(
                "ActorFactory is not ActorFactoryImpl — cannot spawn pre-built actor".to_string(),
            )
        })?;

    factory_impl
        .spawn_built_actor_impl(
            &ctx,
            Arc::new(actor),
            actor_type_str,
            vec![],
            std::collections::HashMap::new(),
        )
        .await
        .map_err(|e| {
            plexspaces_node::NodeError::ConfigError(format!(
                "Failed to spawn actor via ActorFactory: {}",
                e
            ))
        })
}
