// SPDX-License-Identifier: LGPL-2.1-or-later
// Tests for eager virtual actors - start immediately

use plexspaces_actor::{Actor, ActorBuilder};
use plexspaces_behavior::GenServer;
use plexspaces_core::{ActorContext, BehaviorType, BehaviorError, ActorId, Actor as ActorTrait};
use plexspaces_journaling::VirtualActorFacet;
use plexspaces_mailbox::Message;
use plexspaces_node::{Node, NodeBuilder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use tokio::time::sleep;

#[path = "test_helpers.rs"]
mod test_helpers;
use test_helpers::{lookup_actor_ref, get_or_activate_actor_helper};

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
        let test_msg: TestMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        let reply_msg = match test_msg {
            TestMessage::Ping => {
                Message::new(serde_json::to_vec(&TestMessage::Pong("pong".to_string())).unwrap())
            }
            TestMessage::Increment => {
                let mut count = self.count.lock().await;
                *count += 1;
                Message::new(serde_json::to_vec(&TestMessage::Pong("incremented".to_string())).unwrap())
            }
            TestMessage::GetCount => {
                let count = *self.count.lock().await;
                Message::new(serde_json::to_vec(&TestMessage::Count(count)).unwrap())
            }
            _ => return Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        };
        
        // Send reply using ActorContext
        if let Some(sender_id) = &msg.sender {
            ctx.send_reply(
                msg.correlation_id.as_deref(),
                sender_id,
                msg.receiver.clone(),
                reply_msg,
            ).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
        }
        Ok(())
    }
}

// ============================================================================
// EAGER ACTIVATION TESTS
// ============================================================================

#[tokio::test]
async fn test_eager_activation_immediate_availability() {
    // Test: Eager actor should be immediately available after registration
    // Orleans: Eager actors are started immediately, not on first message
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let actor_id: ActorId = "counter-eager-immediate@test-node".to_string();
    
    // Register eager virtual actor (should start immediately)
    let _actor_ref = get_or_activate_actor_helper(&node, 
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
                "activation_strategy": "eager"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone(), 100));
            actor
                .attach_facet(virtual_facet)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    // Eager actor should be active immediately (no need to wait for first message)
    let (exists, is_active, is_virtual) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(exists, "Actor should exist");
    assert!(is_virtual, "Actor should be virtual");
    assert!(is_active, "Eager actor should be active immediately");
    
    // Get ActorRef (should be ActorRef, not VirtualActorWrapper for eager actors)
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Send ask() immediately - should work (actor already active)
    let msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref.ask(msg, Duration::from_secs(1)).await;
    assert!(result.is_ok(), "Eager actor should be immediately available");
    let reply: TestMessage = serde_json::from_slice(result.unwrap().payload()).unwrap();
    assert!(matches!(reply, TestMessage::Pong(_)));
}

#[tokio::test]
async fn test_eager_activation_tell_then_ask() {
    // Test: tell() followed by ask() - both should work immediately
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let actor_id: ActorId = "counter-eager-tell-ask@test-node".to_string();
    
    // Register eager virtual actor
    let _actor_ref = get_or_activate_actor_helper(&node, 
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
                "activation_strategy": "eager"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone(), 100));
            actor
                .attach_facet(virtual_facet)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    // Get ActorRef (eager actors are already active)
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Send tell() - should work immediately (actor already active)
    let tell_msg = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
        .with_message_type("cast".to_string());
    actor_ref.tell(tell_msg).await.unwrap();
    
    // Send ask() - should work immediately
    let ask_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref.ask(ask_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(result.payload()).unwrap();
    assert!(matches!(reply, TestMessage::Count(1)));
}

#[tokio::test]
async fn test_eager_activation_multiple_actors() {
    // Test: Multiple eager actors should all activate immediately
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    
    let mut handles = Vec::new();
    for i in 0..5 {
        let node_clone = node.clone();
        let actor_id: ActorId = format!("counter-eager-{}@test-node", i);
        let handle = tokio::spawn(async move {
            get_or_activate_actor_helper(&node_clone,
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
                        "activation_strategy": "eager"
                    });
                    let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone(), 100));
                    actor
                        .attach_facet(virtual_facet)
                        .await
                        .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
                    
                    Ok(actor)
                }
            ).await.unwrap();
            
            // Verify actor is active
            let (exists, is_active, is_virtual) = node_clone.check_virtual_actor_exists(&actor_id).await;
            assert!(exists, "Actor should exist");
            assert!(is_virtual, "Actor should be virtual");
            assert!(is_active, "Eager actor should be active immediately");
            
            let actor_ref = lookup_actor_ref(&node_clone, &actor_id)
                .await
                .unwrap()
                .unwrap();
            
            let msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
                .with_message_type("call".to_string());
            actor_ref.ask(msg, Duration::from_secs(1)).await
        });
        handles.push(handle);
    }
    
    // All should succeed
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "All eager actors should be available");
    }
}





