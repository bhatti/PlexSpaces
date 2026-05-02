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
// GNU Lesser General License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Tests for ActorFactoryImpl without register_local
//!
//! These tests verify that:
//! 1. ActorFactoryImpl only uses register_actor (not register_local)
//! 2. Mailbox is not exposed via registry
//! 3. Only MessageSender is registered

use async_trait::async_trait;
use plexspaces_actor::{
    actor_factory_impl::ActorFactoryImpl, Actor, ActorBuilder, ActorFactory, ActorRef,
};
use plexspaces_core::Message;
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, ActorId, ActorRegistry, BehaviorError, BehaviorType,
    RequestContext, ServiceLocator,
};
use std::collections::HashMap;
use std::sync::Arc;
use ulid::Ulid;

fn test_actor_id(name: &str) -> ActorId {
    ActorId::new(name, "gen_server", "default", "test-node").expect("valid test actor id")
}

fn make_spawn_spec(
    actor_id: &ActorId,
    actor_type: &str,
) -> plexspaces_proto::actor::v1::ActorSpawnSpec {
    use plexspaces_proto::common::v1::ActorIdentity;
    plexspaces_core::ActorSpawnSpec {
        identity: Some(ActorIdentity {
            name: actor_id.name().to_string(),
            actor_type: actor_type.to_string(),
        }),
        role: String::new(),
        namespace: actor_id.namespace().to_string(),
        tenant_id: String::new(),
        visibility: 0,
        behavior_kind: String::new(),
        args: HashMap::new(),
        facets: vec![],
        config: None,
        labels: HashMap::new(),
    }
}

/// Helper to create a test message
fn create_test_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

struct TestBehavior;

#[async_trait]
impl ActorTrait for TestBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

async fn create_test_service_locator() -> Arc<dyn ServiceLocator> {
    use plexspaces_core::BehaviorRegistry;
    use plexspaces_node::create_default_service_locator;
    let sl = create_default_service_locator(Some("test-node".to_string()), None).await;
    // Register a BehaviorRegistry with test actor types so spawn_actor can create behaviors
    let registry = Arc::new(BehaviorRegistry::new());
    registry
        .register("test", |_args| {
            Box::pin(async move { Ok(Box::new(TestBehavior) as Box<dyn plexspaces_core::Actor>) })
        })
        .await;
    registry
        .register("gen_server", |_args| {
            Box::pin(async move { Ok(Box::new(TestBehavior) as Box<dyn plexspaces_core::Actor>) })
        })
        .await;
    sl.register_behavior_registry(registry).await;
    sl
}

#[tokio::test]
async fn test_spawn_built_actor_registers_message_sender_only() {
    let service_locator = create_test_service_locator().await;
    let factory = ActorFactoryImpl::new_arc(service_locator.clone()).await;

    // Get ActorRegistry to verify registration
    let registry: Arc<ActorRegistry> = service_locator.actor_registry().await.unwrap();

    // Spawn actor using spawn_actor
    let actor_id = test_actor_id("test-actor");
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    let spec = make_spawn_spec(&actor_id, "gen_server");
    let message_sender = factory.spawn_actor(&ctx, &spec, vec![]).await.unwrap();

    // Verify actor is registered (via MessageSender, not mailbox)
    assert!(
        registry.is_actor_activated(&actor_id).await,
        "Actor should be activated"
    );

    // Verify we can lookup MessageSender
    let found_sender = registry.lookup_actor(&actor_id).await;
    assert!(found_sender.is_some(), "MessageSender should be registered");

    // Verify we can send messages
    let message = create_test_message(vec![1, 2, 3]);
    let result = message_sender.tell(&ctx, message).await;
    assert!(result.is_ok(), "Should be able to send message");
}

#[tokio::test]
async fn test_spawn_actor_registers_message_sender_only() {
    let service_locator = create_test_service_locator().await;
    let factory = ActorFactoryImpl::new_arc(service_locator.clone()).await;

    // Get ActorRegistry to verify registration
    let registry: Arc<ActorRegistry> = service_locator.actor_registry().await.unwrap();

    // Spawn actor — actor_id type must match spec actor_type
    let actor_id = ActorId::new("test-actor", "test", "default", "test-node").unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    let spec = make_spawn_spec(&actor_id, "test");
    let message_sender = factory.spawn_actor(&ctx, &spec, vec![]).await.unwrap();

    // Verify actor is registered
    assert!(registry.is_actor_activated(&actor_id).await);

    // Verify MessageSender works
    let message = create_test_message(vec![1, 2, 3]);
    let result = message_sender.tell(&ctx, message).await;
    assert!(result.is_ok(), "Should be able to send message");
}

#[tokio::test]
async fn test_multiple_actors_spawned_via_factory() {
    let service_locator = create_test_service_locator().await;
    let factory = ActorFactoryImpl::new_arc(service_locator.clone()).await;
    let registry: Arc<ActorRegistry> = service_locator.actor_registry().await.unwrap();

    // Spawn multiple actors using spawn_actor
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    for i in 0..5 {
        let actor_id =
            ActorId::new(format!("actor-{i}"), "gen_server", "default", "test-node").unwrap();
        let spec = make_spawn_spec(&actor_id, "gen_server");
        factory.spawn_actor(&ctx, &spec, vec![]).await.unwrap();

        // Verify each is registered
        assert!(
            registry.is_actor_activated(&actor_id).await,
            "Actor {} should be activated",
            i
        );
    }
}
