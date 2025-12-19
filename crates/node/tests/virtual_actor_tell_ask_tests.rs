// SPDX-License-Identifier: LGPL-2.1-or-later
// Tests for tell/ask with virtual actors (lazy and eager activation)

use plexspaces_actor::ActorBuilder;
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor as ActorTrait, ActorContext, BehaviorType, BehaviorError, ActorId};
use plexspaces_journaling::VirtualActorFacet;
use plexspaces_mailbox::Message;
use plexspaces_node::{Node, NodeConfig, NodeId};
use plexspaces_node::default_node_config;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
mod test_helpers;
use test_helpers::{lookup_actor_ref, activate_virtual_actor, get_or_activate_actor_helper, spawn_actor_builder_helper};


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
        reply: &dyn plexspaces_core::Reply,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg, reply).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait]
impl GenServer for CounterActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        envelope: plexspaces_core::Envelope,
    ) -> Result<(), BehaviorError> {
        let msg = &envelope.message;
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
        
        // Correlation_id is automatically preserved by envelope.send_reply()
        envelope.send_reply(reply_msg).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
        Ok(())
    }
}

#[tokio::test]
async fn test_tell_with_virtual_actor_eager() {
    // Test: tell() with VirtualActorFacet (eager activation)
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-eager@test-node".to_string();
    
    // Get or activate actor with VirtualActorFacet (eager)
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "eager"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone()));
            actor
                .attach_facet(virtual_facet, 100, virtual_facet_config)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    tokio::time::sleep(Duration::from_millis(300)).await;
    
    // Test tell() - should work (fire and forget)
    let msg = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
        .with_message_type("cast".to_string());
    
    actor_ref.tell(msg).await.unwrap();
    
    // Wait a bit for message to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Verify count was incremented using ask()
    let get_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    
    let result = actor_ref
        .ask(get_msg, Duration::from_secs(5))
        .await;
    
    assert!(result.is_ok(), "ask() should succeed after tell()");
    let reply = result.unwrap();
    let reply_msg: TestMessage = serde_json::from_slice(reply.payload()).unwrap();
    assert!(matches!(reply_msg, TestMessage::Count(1)));
}

#[tokio::test]
async fn test_ask_with_virtual_actor_eager() {
    // Test: ask() with VirtualActorFacet (eager activation)
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-eager-ask@test-node".to_string();
    
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "eager"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone()));
            actor
                .attach_facet(virtual_facet, 100, virtual_facet_config)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    tokio::time::sleep(Duration::from_millis(300)).await;
    
    // Test ask() - should work
    let msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    
    let result = actor_ref
        .ask(msg, Duration::from_secs(5))
        .await;
    
    assert!(result.is_ok(), "ask() should succeed with VirtualActorFacet (eager)");
    let reply = result.unwrap();
    let reply_msg: TestMessage = serde_json::from_slice(reply.payload()).unwrap();
    assert!(matches!(reply_msg, TestMessage::Pong(_)));
}

#[tokio::test]
async fn test_tell_with_virtual_actor_lazy() {
    // Test: tell() with VirtualActorFacet (lazy activation) - should activate on first message
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-lazy-tell@test-node".to_string();
    
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone()));
            actor
                .attach_facet(virtual_facet, 100, virtual_facet_config)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Test tell() - should activate actor on first message
    let msg = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
        .with_message_type("cast".to_string());
    
    actor_ref.tell(msg).await.unwrap();
    
    // Wait for activation and message processing
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Verify count was incremented using ask()
    let get_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    
    let result = actor_ref
        .ask(get_msg, Duration::from_secs(5))
        .await;
    
    assert!(result.is_ok(), "ask() should succeed after tell() with lazy activation. Error: {:?}", result.err());
    let reply = result.unwrap();
    let reply_msg: TestMessage = serde_json::from_slice(reply.payload()).unwrap();
    assert!(matches!(reply_msg, TestMessage::Count(1)));
}

#[tokio::test]
async fn test_ask_with_virtual_actor_lazy() {
    // Test: ask() with VirtualActorFacet (lazy activation) - should activate on first message
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-lazy-ask@test-node".to_string();
    
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone()));
            actor
                .attach_facet(virtual_facet, 100, virtual_facet_config)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Test ask() - should activate actor on first message
    let msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    
    let result = actor_ref
        .ask(msg, Duration::from_secs(10))
        .await;
    
    assert!(result.is_ok(), "ask() should succeed with VirtualActorFacet (lazy) - should activate on first message");
    let reply = result.unwrap();
    let reply_msg: TestMessage = serde_json::from_slice(reply.payload()).unwrap();
    assert!(matches!(reply_msg, TestMessage::Pong(_)));
}

#[tokio::test]
async fn test_multiple_ask_with_virtual_actor_lazy() {
    // Test: Multiple ask() calls with VirtualActorFacet (lazy activation)
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-lazy-multi@test-node".to_string();
    
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone()));
            actor
                .attach_facet(virtual_facet, 100, virtual_facet_config)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // First ask() - should activate actor
    let msg1 = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
        .with_message_type("call".to_string());
    
    let result1 = actor_ref
        .ask(msg1, Duration::from_secs(10))
        .await;
    assert!(result1.is_ok(), "First ask() should succeed and activate actor");
    
    // Second ask() - should work (actor already activated)
    let msg2 = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
        .with_message_type("call".to_string());
    
    let result2 = actor_ref
        .ask(msg2, Duration::from_secs(5))
        .await;
    assert!(result2.is_ok(), "Second ask() should succeed");
    
    // Verify count is 2
    let get_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    
    let result3 = actor_ref
        .ask(get_msg, Duration::from_secs(5))
        .await;
    
    assert!(result3.is_ok(), "Third ask() should succeed");
    let reply = result3.unwrap();
    let reply_msg: TestMessage = serde_json::from_slice(reply.payload()).unwrap();
    assert!(matches!(reply_msg, TestMessage::Count(2)));
}

#[tokio::test]
async fn test_ask_with_virtual_actor_lazy_reproduce_issue() {
    // Test to reproduce the orleans lazy activation timeout issue
    // This should activate on first message and respond within 1 second
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-lazy-reproduce@test-node".to_string();
    
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            });
            let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone()));
            actor
                .attach_facet(virtual_facet, 100, virtual_facet_config)
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    // Wait for actor to be registered
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Route through node to trigger virtual actor activation
    // Test ask() with 1 second timeout - should activate and respond
    let msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    
    let start = std::time::Instant::now();
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let result = actor_ref.tell(msg).await;
    let elapsed = start.elapsed();
    
    assert!(result.is_ok(), "tell() should succeed with VirtualActorFacet (lazy) within 1 second, elapsed: {:?}", elapsed);
    assert!(elapsed < Duration::from_secs(1), "Should respond within 1 second, but took {:?}", elapsed);
}
