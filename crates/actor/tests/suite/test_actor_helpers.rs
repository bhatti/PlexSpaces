// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Shared helpers for integration tests that call [`Actor::start`], which registers the actor
// via [`plexspaces_core::ServiceLocator::actor_registry`]. Plain [`Actor::new`] installs a stub
// locator without a registry and causes `register_in_registry` to fail.

use plexspaces_actor::Actor;
use plexspaces_core::{ActorContext, ActorId, BehaviorType, ServiceLocator};
use plexspaces_mailbox::Mailbox;
use std::sync::Arc;

pub(crate) enum TestActorIdentity {
    ActorId(ActorId),
    Name(String),
}

impl From<ActorId> for TestActorIdentity {
    fn from(value: ActorId) -> Self {
        Self::ActorId(value)
    }
}

impl From<String> for TestActorIdentity {
    fn from(value: String) -> Self {
        Self::Name(value)
    }
}

impl From<&str> for TestActorIdentity {
    fn from(value: &str) -> Self {
        Self::Name(value.to_string())
    }
}

fn behavior_type_name(behavior_type: BehaviorType) -> String {
    match behavior_type {
        BehaviorType::GenServer => "GenServer".to_string(),
        BehaviorType::GenEvent => "GenEvent".to_string(),
        BehaviorType::GenStateMachine => "GenStateMachine".to_string(),
        BehaviorType::Workflow => "Workflow".to_string(),
        BehaviorType::Custom(name) => name,
    }
}

fn normalized_test_actor_id(
    name: String,
    behavior_type: String,
    namespace: String,
    default_node_id: String,
) -> ActorId {
    if let Ok(actor_id) = ActorId::from_canonical(&name) {
        return actor_id;
    }

    if let Some((logical_name, node_id)) = name.split_once('@') {
        return ActorId::new(
            logical_name.to_string(),
            behavior_type,
            namespace,
            node_id.to_string(),
        )
        .expect("test actor id should be valid");
    }

    ActorId::new(name, behavior_type, namespace, default_node_id)
        .expect("test actor id should be valid")
}

/// Builds an [`Actor`] whose context uses [`plexspaces_node::create_default_service_locator`]
/// so [`Actor::start`] can register with [`ActorRegistry`].
pub async fn actor_with_default_service_locator(
    id: impl Into<TestActorIdentity>,
    behavior: Box<dyn plexspaces_core::Actor>,
    mailbox: Mailbox,
    tenant_id: String,
    namespace: String,
) -> Actor {
    let locator_impl = plexspaces_node::create_default_service_locator(None, None).await;
    let node_id = ServiceLocator::get_node_config(locator_impl.as_ref())
        .await
        .map(|n| n.id)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "test-node".to_string());
    let service_locator: Arc<dyn ServiceLocator> = locator_impl;
    let context = Arc::new(ActorContext::new(
        node_id.clone(),
        tenant_id.clone(),
        namespace.clone(),
        service_locator,
        None,
    ));
    let behavior_type = behavior_type_name(behavior.behavior_type());
    let actor_id = match id.into() {
        TestActorIdentity::ActorId(actor_id) => actor_id,
        TestActorIdentity::Name(name) => {
            normalized_test_actor_id(name, behavior_type, namespace.clone(), node_id)
        }
    };
    Actor::new(actor_id, behavior, mailbox, tenant_id, namespace, None).set_context(context)
}
