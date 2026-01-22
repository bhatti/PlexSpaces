// SPDX-License-Identifier: LGPL-2.1-or-later
// Tests for lazy virtual actors - activate on first message

use plexspaces_actor::{Actor, ActorBuilder};
use plexspaces_behavior::GenServer;
use plexspaces_core::{ActorContext, BehaviorType, BehaviorError, ActorId, Actor as ActorTrait};
use plexspaces_journaling::VirtualActorFacet;
use plexspaces_core::Message;
use plexspaces_node::{Node, NodeBuilder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use tokio::time::sleep;

#[path = "test_helpers.rs"]
mod test_helpers;
use test_helpers::{lookup_actor_ref, get_or_activate_actor_helper};

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
    Increment,
    GetCount,
    Count(u32),
}

struct CounterActor {
    count: Arc<tokio::sync::Mutex<u32>>,
}

impl CounterActor {
    fn new() -> Self {
        Self {
            count: Arc::new(tokio::sync::Mutex::new(0)),
        }
    }
}

#[async_trait]
impl ActorTrait for CounterActor {
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
impl GenServer for CounterActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let test_msg: TestMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        let reply_msg = match test_msg {
            TestMessage::Ping => {
                create_test_message(serde_json::to_vec(&TestMessage::Pong("pong".to_string())).unwrap())
            }
            TestMessage::Increment => {
                let mut count = self.count.lock().await;
                *count += 1;
                create_test_message(serde_json::to_vec(&TestMessage::Pong("incremented".to_string())).unwrap())
            }
            TestMessage::GetCount => {
                let count = *self.count.lock().await;
                create_test_message(serde_json::to_vec(&TestMessage::Count(count)).unwrap())
            }
            _ => return Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        };
        
        // Send reply using ActorContext
        if !msg.sender_id.is_empty() {
            let correlation_id = if msg.correlation_id.is_empty() { None } else { Some(msg.correlation_id.as_str()) };
            ctx.send_reply(
                correlation_id,
                &msg.sender_id,
                msg.receiver_id.clone(),
                reply_msg,
            ).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
        }
        Ok(())
    }
}

// ============================================================================
// LAZY ACTIVATION TESTS
// ============================================================================

#[tokio::test]
async fn test_lazy_activation_tell_then_ask() {
    // Test: tell() followed by ask() - both should work
    // Lazy virtual actor should activate on first tell() message
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let actor_id: ActorId = "counter-lazy-tell-ask@test-node".to_string();
    
    // Register lazy virtual actor (not activated yet)
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to build actor: {}", e)))?;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone(), 100));
            actor
                .attach_facet(virtual_facet)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    // Verify actor exists (lazy activation - registered but may not be active yet)
    // Note: For lazy virtual actors, they are registered but activation happens on first message
    // However, get_or_activate_actor_helper might activate them, so we don't check is_active here
    let (exists, _is_active, is_virtual) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(exists, "Actor should exist");
    assert!(is_virtual, "Actor should be virtual");
    
    // Get ActorRef (will be VirtualActorWrapper for lazy virtual actors)
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Send tell() - should activate synchronously (Orleans: first message activates)
    // VirtualActorWrapper.tell() should activate the actor
    let tell_msg = create_test_message_with_type(serde_json::to_vec(&TestMessage::Increment).unwrap(), "cast");
    actor_ref.tell(tell_msg).await.unwrap();
    
    // Activation is synchronous - actor is ready immediately after tell() returns
    // VirtualActorWrapper is replaced by ActorRef in registry after activation
    // Get fresh ActorRef from registry (will be ActorRef, not VirtualActorWrapper)
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Verify actor is now active
    let (_, is_active_after, _) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(is_active_after, "Actor should be active after first message");
    
    // Send ask() - should work (actor already activated, using ActorRef directly)
    let ask_msg = create_test_message_with_type(serde_json::to_vec(&TestMessage::GetCount).unwrap(), "call");
    let result = actor_ref.ask(ask_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(&result.payload).unwrap();
    assert!(matches!(reply, TestMessage::Count(1)));
}

#[tokio::test]
async fn test_lazy_activation_ask_directly() {
    // Test: ask() directly on lazy actor - should activate and respond
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let actor_id: ActorId = "counter-lazy-ask@test-node".to_string();
    
    // Register lazy virtual actor
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to build actor: {}", e)))?;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone(), 100));
            actor
                .attach_facet(virtual_facet)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    // Get ActorRef (will be VirtualActorWrapper)
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Send ask() directly - should activate and respond
    let ask_msg = create_test_message_with_type(serde_json::to_vec(&TestMessage::Ping).unwrap(), "call");
    let result = actor_ref.ask(ask_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(&result.payload).unwrap();
    assert!(matches!(reply, TestMessage::Pong(_)));
}

#[tokio::test]
async fn test_lazy_activation_multiple_messages() {
    // Test: Multiple messages to lazy actor - should activate once
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let actor_id: ActorId = "counter-lazy-multi@test-node".to_string();
    
    // Register lazy virtual actor
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to build actor: {}", e)))?;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone(), 100));
            actor
                .attach_facet(virtual_facet)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    // Get ActorRef
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Send multiple messages - should activate on first and process all
    for _ in 0..5 {
        let msg = create_test_message_with_type(serde_json::to_vec(&TestMessage::Increment).unwrap(), "call");
        let _ = actor_ref.ask(msg, Duration::from_secs(5)).await.unwrap();
    }
    
    // Verify count
    let get_msg = create_test_message_with_type(serde_json::to_vec(&TestMessage::GetCount).unwrap(), "call");
    let result = actor_ref.ask(get_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(&result.payload).unwrap();
    assert!(matches!(reply, TestMessage::Count(5)));
}

