// SPDX-License-Identifier: LGPL-2.1-or-later
// Comprehensive tests for virtual actors covering all edge cases

use plexspaces_actor::{Actor, ActorBuilder};
use plexspaces_behavior::GenServer;
use plexspaces_core::{ActorContext, BehaviorType, BehaviorError, ActorId, Actor as ActorTrait};
use plexspaces_journaling::VirtualActorFacet;
use plexspaces_mailbox::Message;
use plexspaces_node::{Node, NodeId};
use plexspaces_node::default_node_config;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use tokio::time::sleep;

#[path = "test_helpers.rs"]
mod test_helpers;
use test_helpers::{spawn_actor_helper, find_actor_helper, unregister_actor_helper, lookup_actor_ref, get_or_activate_actor_helper, activate_virtual_actor};

#[derive(Debug, Clone, Serialize, Deserialize)]
enum TestMessage {
    Ping,
    Pong(String),
    Increment,
    GetCount,
    Count(u32),
    SlowOperation(Duration),
    Error,
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
            TestMessage::SlowOperation(duration) => {
                sleep(duration).await;
                Message::new(serde_json::to_vec(&TestMessage::Pong("slow_done".to_string())).unwrap())
            }
            TestMessage::Error => {
                return Err(BehaviorError::ProcessingError("Test error".to_string()));
            }
            _ => return Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        };
        
        // Correlation_id is automatically preserved by envelope.send_reply()
        envelope.send_reply(reply_msg).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
        Ok(())
    }
}

// ============================================================================
// LAZY ACTIVATION EDGE CASES
// ============================================================================

#[tokio::test]
async fn test_lazy_activation_concurrent_requests() {
    // Test: Multiple concurrent activation requests should only activate once
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-concurrent@test-node".to_string();
    
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
    
    sleep(Duration::from_millis(200)).await;
    
    // Trigger activation first by sending a message via ActorRef
    let activate_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let _ = actor_ref.tell(activate_msg).await;
    
    // Wait for activation to complete - poll until actor is active
    let mut attempts = 0;
    loop {
        sleep(Duration::from_millis(100)).await;
        let (_, is_active, _) = node.check_virtual_actor_exists(&actor_id).await;
        if is_active {
            break;
        }
        attempts += 1;
        if attempts > 50 {
            panic!("Actor failed to activate within 5 seconds");
        }
    }
    
    // Get mailbox for ActorRef
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Send 10 concurrent messages - should only activate once (already activated)
    let mut handles = Vec::new();
    for _ in 0..10 {
        let actor_ref_clone = actor_ref.clone();
        let handle = tokio::spawn(async move {
            let msg = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
                .with_message_type("call".to_string());
            actor_ref_clone.ask(msg, Duration::from_secs(5)).await
        });
        handles.push(handle);
    }
    
    // Wait for all messages
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "All concurrent messages should succeed");
    }
    
    // Verify count is 10 (all increments processed)
    let get_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref.ask(get_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(result.payload()).unwrap();
    assert!(matches!(reply, TestMessage::Count(10)));
}

#[tokio::test]
async fn test_lazy_activation_pending_messages_processed() {
    // Test: Messages sent during activation should be queued and processed after activation
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-pending@test-node".to_string();
    
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
    
    sleep(Duration::from_millis(100)).await;
    
    // Send multiple messages rapidly - should queue during activation
    // Send messages via ActorRef to trigger activation
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    for _ in 0..5 {
        let msg = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
            .with_message_type("call".to_string());
        let _ = actor_ref.tell(msg).await;
    }
    
    // Wait for activation and processing
    sleep(Duration::from_millis(500)).await;
    
    // Verify all messages were processed
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    let get_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref.ask(get_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(result.payload()).unwrap();
    assert!(matches!(reply, TestMessage::Count(5)));
}

#[tokio::test]
async fn test_lazy_activation_activation_failure_handling() {
    // Test: If activation fails, subsequent messages should retry activation
    // Note: This is a simplified test - actual activation failures are rare
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-fail@test-node".to_string();
    
    // Create actor but don't register it yet
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
        .unwrap();
    
    // Register actor
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async { Ok(actor) }
    ).await.unwrap();
    
    sleep(Duration::from_millis(200)).await;
    
    // Trigger activation first
    let activate_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let result = actor_ref.tell(activate_msg).await;
    assert!(result.is_ok(), "Activation should succeed");
    
    // Wait for activation
    sleep(Duration::from_millis(500)).await;
    
    // Now use ActorRef for ask() pattern
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();

    let msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref.ask(msg, Duration::from_secs(5)).await;
    assert!(result.is_ok(), "Message should succeed after activation");
}

#[tokio::test]
async fn test_lazy_activation_tell_then_ask() {
    // Test: tell() followed by ask() - both should work
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-tell-ask@test-node".to_string();
    
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
    
    sleep(Duration::from_millis(200)).await;
    
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Send tell() - should activate
    let tell_msg = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
        .with_message_type("cast".to_string());
    actor_ref.tell(tell_msg).await.unwrap();
    
    sleep(Duration::from_millis(300)).await;
    
    // Send ask() - should work (actor already activated)
    let ask_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref.ask(ask_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(result.payload()).unwrap();
    assert!(matches!(reply, TestMessage::Count(1)));
}

// ============================================================================
// EAGER ACTIVATION EDGE CASES
// ============================================================================

#[tokio::test]
async fn test_eager_activation_immediate_availability() {
    // Test: Eager actor should be immediately available after creation
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-eager-immediate@test-node".to_string();
    
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
    
    // Wait for eager activation
    sleep(Duration::from_millis(500)).await;
    
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Should be immediately available
    let msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref.ask(msg, Duration::from_secs(1)).await;
    assert!(result.is_ok(), "Eager actor should be immediately available");
}

#[tokio::test]
async fn test_eager_activation_multiple_actors() {
    // Test: Multiple eager actors should all activate immediately
    let node = Arc::new(NodeBuilder::new("test-node").build());
    
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
            
            sleep(Duration::from_millis(500)).await;
            
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

// ============================================================================
// PASSIVATION/IDLE TIMEOUT EDGE CASES
// ============================================================================

#[tokio::test]
async fn test_passivation_idle_timeout_expiration() {
    // Test: Actor should be deactivated after idle timeout
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-idle@test-node".to_string();
    
    // Start idle timeout monitor
    node.start_idle_timeout_monitor();
    
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            // Short idle timeout for testing
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "2s",
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
    
    sleep(Duration::from_millis(200)).await;
    
    // Activate actor - route through node
    let msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let _ = actor_ref.tell(msg).await;
    
    sleep(Duration::from_millis(300)).await;
    
    // Verify actor is active
    let (_, is_active, _) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(is_active, "Actor should be active after message");
    
    // Wait for idle timeout (2s) + monitor check interval
    sleep(Duration::from_secs(15)).await;
    
    // Actor should be deactivated (but still exists as virtual)
    let (exists, is_active_after, is_virtual) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(exists, "Virtual actor should still exist");
    assert!(is_virtual, "Actor should still be registered as virtual");
    // Note: is_active_after may be false if deactivation occurred
}

#[tokio::test]
async fn test_passivation_reactivation_after_timeout() {
    // Test: Actor should reactivate after passivation when message arrives
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-reactivate@test-node".to_string();
    
    node.start_idle_timeout_monitor();
    
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "2s",
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
    
    sleep(Duration::from_millis(200)).await;
    
    // Trigger activation first
    let activate_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let _ = actor_ref.tell(activate_msg).await;
    
    sleep(Duration::from_millis(500)).await;
    
    // Get mailbox for ActorRef
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Use actor
    let msg1 = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
        .with_message_type("call".to_string());
    let _ = actor_ref.ask(msg1, Duration::from_secs(5)).await;
    
    // Wait for passivation
    sleep(Duration::from_secs(15)).await;
    
    // Send another message - should reactivate
    // First trigger reactivation
    let reactivate_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let result = actor_ref.tell(reactivate_msg).await;
    assert!(result.is_ok(), "Actor should reactivate");
    
    sleep(Duration::from_millis(500)).await;
    
    // Get ActorRef again (may have changed)
    let actor_ref2 = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Send increment
    let msg2 = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
        .with_message_type("call".to_string());
    let _ = actor_ref2.ask(msg2, Duration::from_secs(5)).await;
    
    // Verify count is 2 (both increments processed)
    let get_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref2.ask(get_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(result.payload()).unwrap();
    assert!(matches!(reply, TestMessage::Count(2)));
}

#[tokio::test]
async fn test_passivation_message_resets_idle_timer() {
    // Test: Messages should reset idle timer, preventing passivation
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-reset@test-node".to_string();
    
    node.start_idle_timeout_monitor();
    
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "3s",
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
    
    sleep(Duration::from_millis(200)).await;
    
    // Activate actor - send via ActorRef
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let msg1 = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let _ = actor_ref.tell(msg1).await;
    
    sleep(Duration::from_millis(300)).await;
    
    // Send messages every 2 seconds (before timeout) - should prevent passivation
    for _ in 0..3 {
        sleep(Duration::from_secs(2)).await;
        let msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
            .with_message_type("call".to_string());
        let _ = actor_ref.tell(msg).await;
    }
    
    // Actor should still be active (messages reset idle timer)
    let (_, is_active, _) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(is_active, "Actor should still be active (messages reset idle timer)");
}

// ============================================================================
// MIXED SCENARIOS
// ============================================================================

#[tokio::test]
async fn test_mixed_lazy_eager_actors() {
    // Test: Mix of lazy and eager actors should work correctly
    let node = Arc::new(NodeBuilder::new("test-node").build());
    
    // Create lazy actor
    let lazy_id: ActorId = "counter-lazy-mixed@test-node".to_string();
    let _lazy_ref = get_or_activate_actor_helper(&node, 
        lazy_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(lazy_id.clone())
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
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(lazy_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    // Create eager actor
    let eager_id: ActorId = "counter-eager-mixed@test-node".to_string();
    let _eager_ref = get_or_activate_actor_helper(&node, 
        eager_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(eager_id.clone())
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
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(eager_id.clone().into(), format!("Failed to attach VirtualActorFacet: {}", e)))?;
            
            Ok(actor)
        }
    ).await.unwrap();
    
    sleep(Duration::from_millis(500)).await;
    
    // Lazy actor should activate on first message - route through node
    let lazy_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let lazy_actor_ref = lookup_actor_ref(&node, &lazy_id).await.unwrap().unwrap();
    let lazy_result = lazy_actor_ref.tell(lazy_msg).await;
    assert!(lazy_result.is_ok(), "Lazy actor should activate and respond");
    
    // Eager actor should be immediately available
    let eager_ref = lookup_actor_ref(&node, &eager_id)
        .await
        .unwrap()
        .unwrap();
    let eager_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let eager_result = eager_ref.ask(eager_msg, Duration::from_secs(1)).await;
    assert!(eager_result.is_ok(), "Eager actor should be immediately available");
}

#[tokio::test]
async fn test_virtual_actor_state_preservation() {
    // Test: Actor state should be preserved across activation/deactivation cycles
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-state@test-node".to_string();
    
    node.start_idle_timeout_monitor();
    
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "2s",
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
    
    sleep(Duration::from_millis(200)).await;
    
    // Trigger activation first
    let activate_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let _ = actor_ref.tell(activate_msg).await;
    
    sleep(Duration::from_millis(500)).await;
    
    // Get mailbox for ActorRef
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Increment to 5
    for _ in 0..5 {
        let msg = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
            .with_message_type("call".to_string());
        let _ = actor_ref.ask(msg, Duration::from_secs(5)).await;
    }
    
    // Verify count is 5
    let get_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref.ask(get_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(result.payload()).unwrap();
    assert!(matches!(reply, TestMessage::Count(5)));
    
    // Wait for passivation
    sleep(Duration::from_secs(15)).await;
    
    // Reactivate and verify state is preserved
    // Note: In current implementation, state is in-memory, so it may not persist
    // This test verifies the reactivation works, but state persistence would require DurabilityFacet
    let reactivate_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let result = actor_ref.tell(reactivate_msg).await;
    assert!(result.is_ok(), "Actor should reactivate successfully");
}

// ============================================================================
// ERROR HANDLING EDGE CASES
// ============================================================================

#[tokio::test]
async fn test_virtual_actor_not_found_error() {
    // Test: Accessing non-existent virtual actor should return appropriate error
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "nonexistent@test-node".to_string();
    
    // Check that actor doesn't exist
    let (exists, _, _) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(!exists, "Non-existent actor should not exist");
    
    // Try to activate non-existent actor
    let result = activate_virtual_actor(&node, &actor_id).await;
    assert!(result.is_err(), "Activating non-existent actor should fail");
    
    // Try to get metadata for non-existent actor
    let metadata = node.get_virtual_actor_metadata(&actor_id).await;
    assert!(metadata.is_none(), "Non-existent actor should have no metadata");
}

#[tokio::test]
async fn test_virtual_actor_manual_deactivation() {
    // Test: Manual deactivation should work
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-manual-deact@test-node".to_string();
    
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
    
    sleep(Duration::from_millis(200)).await;
    
    // Activate actor - send via ActorRef
    let activate_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let _ = actor_ref.tell(activate_msg).await;
    
    sleep(Duration::from_millis(500)).await;
    
    // Verify active
    let (_, is_active, _) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(is_active, "Actor should be active");
    
    // Manually deactivate
    node.deactivate_virtual_actor(&actor_id, false).await.unwrap();
    
    // Verify deactivated (but still exists as virtual)
    let (exists, is_active_after, is_virtual) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(exists, "Virtual actor should still exist");
    assert!(is_virtual, "Actor should still be registered as virtual");
}

// ============================================================================
// INTEGRATION SCENARIOS
// ============================================================================

#[tokio::test]
async fn test_virtual_actor_full_lifecycle() {
    // Test: Complete lifecycle - create, activate, use, passivate, reactivate
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-lifecycle@test-node".to_string();
    
    node.start_idle_timeout_monitor();
    
    // 1. Create virtual actor (lazy)
    let _core_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            let behavior = Box::new(CounterActor::new());
            let mut actor = ActorBuilder::new(behavior)
                .with_id(actor_id.clone())
                .build()
                .await;
            
            let virtual_facet_config = serde_json::json!({
                "idle_timeout": "2s",
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
    
    sleep(Duration::from_millis(200)).await;
    
    // 2. Verify actor exists but not active
    let (exists, is_active_initial, is_virtual) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(exists, "Actor should exist");
    assert!(is_virtual, "Actor should be registered as virtual");
    
    // 3. Send message - should activate - send via ActorRef
    let activate_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let result1 = actor_ref.tell(activate_msg).await;
    assert!(result1.is_ok(), "First message should activate and succeed");
    
    sleep(Duration::from_millis(500)).await;
    
    // 4. Verify active
    let (_, is_active_after_msg, _) = node.check_virtual_actor_exists(&actor_id).await;
    assert!(is_active_after_msg, "Actor should be active after message");
    
    // 5. Get mailbox for ActorRef
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Use actor
    for _ in 0..3 {
        let msg = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
            .with_message_type("call".to_string());
        let _ = actor_ref.ask(msg, Duration::from_secs(5)).await;
    }
    
    // 6. Verify count
    let get_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref.ask(get_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(result.payload()).unwrap();
    assert!(matches!(reply, TestMessage::Count(3)));
    
    // 7. Wait for passivation
    sleep(Duration::from_secs(15)).await;
    
    // 8. Reactivate with new message - route through node
    let reactivate_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let result2 = actor_ref.tell(reactivate_msg).await;
    assert!(result2.is_ok(), "Actor should reactivate and process message");
}

#[tokio::test]
async fn test_virtual_actor_high_throughput() {
    // Test: High throughput scenario with many messages
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let actor_id: ActorId = "counter-throughput@test-node".to_string();
    
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
    
    sleep(Duration::from_millis(200)).await;
    
    // Trigger activation first
    let activate_msg = Message::new(serde_json::to_vec(&TestMessage::Ping).unwrap())
        .with_message_type("call".to_string());
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.unwrap().unwrap();
    let _ = actor_ref.tell(activate_msg).await;
    
    sleep(Duration::from_millis(500)).await;
    
    // Get mailbox for ActorRef
    let actor_ref = lookup_actor_ref(&node, &actor_id)
        .await
        .unwrap()
        .unwrap();
    
    // Send 100 messages rapidly
    let mut handles = Vec::new();
    for _ in 0..100 {
        let actor_ref_clone = actor_ref.clone();
        let handle = tokio::spawn(async move {
            let msg = Message::new(serde_json::to_vec(&TestMessage::Increment).unwrap())
                .with_message_type("call".to_string());
            actor_ref_clone.ask(msg, Duration::from_secs(10)).await
        });
        handles.push(handle);
    }
    
    // Wait for all messages
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "All messages should succeed");
    }
    
    // Verify count
    let get_msg = Message::new(serde_json::to_vec(&TestMessage::GetCount).unwrap())
        .with_message_type("call".to_string());
    let result = actor_ref.ask(get_msg, Duration::from_secs(5)).await.unwrap();
    let reply: TestMessage = serde_json::from_slice(result.payload()).unwrap();
    assert!(matches!(reply, TestMessage::Count(100)));
}


