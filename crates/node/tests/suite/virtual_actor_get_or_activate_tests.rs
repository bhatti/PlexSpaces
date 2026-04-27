// SPDX-License-Identifier: AGPL-3.0-or-later
// Tests for get_or_activate_actor with VirtualActorFacet

use super::test_helpers::{
    lookup_actor_ref, registry_ask, spawn_actor_helper, test_runtime_actor_id,
};
use async_trait::async_trait;
use plexspaces_actor::{Actor, ActorBuilder};
use plexspaces_behavior::GenServer;
use plexspaces_core::Message;
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType, ServiceLocator,
};
use plexspaces_journaling::VirtualActorFacet;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Helper to create a test message
fn create_test_message(payload: Vec<u8>) -> plexspaces_core::Message {
    plexspaces_core::Message {
        id: ulid::Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

/// Helper to create a test message with message type
fn create_test_message_with_type(payload: Vec<u8>, message_type: &str) -> plexspaces_core::Message {
    plexspaces_core::Message {
        id: ulid::Ulid::new().to_string(),
        payload,
        message_type: message_type.to_string(),
        ..Default::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum TestMessage {
    Ping,
    Pong(String),
}

struct TestActor {
    received: Arc<tokio::sync::Mutex<Vec<Message>>>,
}

impl TestActor {
    fn new() -> Self {
        Self {
            received: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl ActorTrait for TestActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait]
impl GenServer for TestActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let test_msg: TestMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        let reply_msg = match test_msg {
            TestMessage::Ping => create_test_message(
                serde_json::to_vec(&TestMessage::Pong("pong".to_string())).unwrap(),
            ),
            _ => {
                return Err(BehaviorError::ProcessingError(
                    "Unknown message".to_string(),
                ))
            }
        };

        // Send reply using ActorContext
        if !msg.sender_id.is_empty() {
            let correlation_id = if msg.correlation_id.is_empty() {
                None
            } else {
                Some(msg.correlation_id.as_str())
            };
            ctx.send_reply(
                correlation_id,
                &msg.sender_id,
                ActorId::from_canonical(&msg.receiver_id).map_err(|e| {
                    BehaviorError::ProcessingError(format!(
                        "Failed to parse sender actor id for reply: {}",
                        e
                    ))
                })?,
                reply_msg,
            )
            .await
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
        }
        Ok(())
    }
}

#[tokio::test]
async fn test_get_or_activate_with_virtual_facet_eager() {
    // Test: get_or_activate_actor with VirtualActorFacet (eager activation) should work with ask()
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let actor_id = test_runtime_actor_id("test-actor", "test-node");

    // Build and spawn actor with VirtualActorFacet (eager)
    let behavior = Box::new(TestActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await
        .unwrap();

    // Attach VirtualActorFacet with eager activation
    let virtual_facet_config = serde_json::json!({
        "idle_timeout": "5m",
        "activation_strategy": "eager"
    });
    let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone(), 100));
    actor.attach_facet(virtual_facet).await.unwrap();

    spawn_actor_helper(&node, actor).await.unwrap();

    // Wait for actor to be registered
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Get ActorRef
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();

    // Wait for actor to be ready
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Test ask() - this should work
    let msg =
        create_test_message_with_type(serde_json::to_vec(&TestMessage::Ping).unwrap(), "call");

    let result = actor_ref.ask(msg, Duration::from_secs(5)).await;

    assert!(
        result.is_ok(),
        "ask() should succeed with VirtualActorFacet (eager)"
    );
    let reply = result.unwrap();
    let reply_msg: TestMessage = serde_json::from_slice(&reply.payload).unwrap();
    assert!(matches!(reply_msg, TestMessage::Pong(_)));
}

#[tokio::test]
async fn test_get_or_activate_with_virtual_facet_lazy() {
    // Test: get_or_activate_actor with VirtualActorFacet (lazy activation) should activate on first message
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    // Register BehaviorRegistry so that activate_virtual_actor can rebuild the actor
    // from its stored actor_type ("GenServer") when the lazy actor receives its first message.
    use plexspaces_core::behavior_factory::BehaviorRegistry;
    let registry = BehaviorRegistry::new();
    registry
        .register_simple("gen_server", || {
            Box::pin(
                async move { Ok(Box::new(TestActor::new()) as Box<dyn plexspaces_core::Actor>) },
            )
        })
        .await;
    node.service_locator()
        .register_behavior_registry(Arc::new(registry))
        .await;

    let actor_id = test_runtime_actor_id("test-actor-lazy", "test-node");

    // Build and spawn actor with VirtualActorFacet (lazy)
    let behavior = Box::new(TestActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await
        .unwrap();

    // Attach VirtualActorFacet with lazy activation
    let virtual_facet_config = serde_json::json!({
        "idle_timeout": "5m",
        "activation_strategy": "lazy"
    });
    let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone(), 100));
    actor.attach_facet(virtual_facet).await.unwrap();

    spawn_actor_helper(&node, actor).await.unwrap();

    // Wait for actor to be registered
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Lazy actor is not in the live registry yet; route through registry so it activates on first message
    let msg =
        create_test_message_with_type(serde_json::to_vec(&TestMessage::Ping).unwrap(), "call");

    let result = registry_ask(&node, &actor_id, msg, Duration::from_secs(5)).await;

    assert!(
        result.is_ok(),
        "ask() should succeed with VirtualActorFacet (lazy) - should activate on first message"
    );
    let reply = result.unwrap();
    let reply_msg: TestMessage = serde_json::from_slice(&reply.payload).unwrap();
    assert!(matches!(reply_msg, TestMessage::Pong(_)));
}
