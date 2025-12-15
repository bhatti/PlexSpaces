// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Gosiris (Go Actor Framework - Message Passing Patterns)
// Based on: https://github.com/teivah/gosiris

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId, Reply};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// Counter message (Gosiris-style message passing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CounterMessage {
    Increment,
    Decrement,
    Get,
    Count(u64),
}

/// Counter actor (Gosiris-style actor)
/// Demonstrates: Actor Model, Message Passing, Request-Reply Pattern
pub struct CounterActor {
    count: u64,
}

impl CounterActor {
    pub fn new() -> Self {
        Self { count: 0 }
    }
}

#[async_trait::async_trait]
impl Actor for CounterActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        reply: &dyn Reply,
    ) -> Result<(), BehaviorError> {
        <Self as GenServer>::route_message(self, ctx, msg, reply).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for CounterActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        let counter_msg: CounterMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match counter_msg {
            CounterMessage::Increment => {
                self.count += 1;
                info!("Counter incremented to: {}", self.count);
                let reply = CounterMessage::Count(self.count);
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            CounterMessage::Decrement => {
                self.count = self.count.saturating_sub(1);
                info!("Counter decremented to: {}", self.count);
                let reply = CounterMessage::Count(self.count);
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            CounterMessage::Get => {
                info!("Counter get: {}", self.count);
                let reply = CounterMessage::Count(self.count);
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            _ => Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== Gosiris vs PlexSpaces Comparison ===");
    info!("Demonstrating Gosiris Actor Model (Go-style message passing)");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Gosiris actors use message passing for communication
    let actor_id: ActorId = "counter/gosiris-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating counter actor (Gosiris-style message passing)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(CounterActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await;
    
    let actor_ref = node.spawn_actor(actor).await?;

    let mailbox = node.actor_registry()
        .lookup_mailbox(actor_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    
    let counter = plexspaces_actor::ActorRef::local(
        actor_ref.id().clone(),
        mailbox,
        node.service_locator().clone(),
    );

    info!("✅ Counter actor created: {}", counter.id());
    info!("✅ Message passing: Request-reply pattern");
    info!("✅ Actor isolation: State protected by actor model");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test increment (Gosiris: actor.Send(ctx, Increment{}))
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 1: Increment (message passing)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = Message::new(serde_json::to_vec(&CounterMessage::Increment)?)
        .with_message_type("call".to_string());
    let result = counter
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: CounterMessage = serde_json::from_slice(result.payload())?;
    if let CounterMessage::Count(count) = reply {
        info!("✅ Increment result: {}", count);
        assert_eq!(count, 1);
    }

    // Test increment again
    let msg = Message::new(serde_json::to_vec(&CounterMessage::Increment)?)
        .with_message_type("call".to_string());
    let result = counter
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: CounterMessage = serde_json::from_slice(result.payload())?;
    if let CounterMessage::Count(count) = reply {
        info!("✅ Increment result: {}", count);
        assert_eq!(count, 2);
    }

    // Test get
    let msg = Message::new(serde_json::to_vec(&CounterMessage::Get)?)
        .with_message_type("call".to_string());
    let result = counter
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: CounterMessage = serde_json::from_slice(result.payload())?;
    if let CounterMessage::Count(count) = reply {
        info!("✅ Get result: {}", count);
        assert_eq!(count, 2);
    }

    info!("=== Comparison Complete ===");
    info!("✅ Actor Model: Message passing with request-reply pattern");
    info!("✅ GenServerBehavior: Gosiris-style actor behavior");
    info!("✅ Actor Isolation: State protected by actor boundaries");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_counter() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "counter/test-1@test-node".to_string();
        let behavior = Box::new(CounterActor::new());
        let mut actor = ActorBuilder::new(behavior)
            .with_id(actor_id.clone())
            .build()
            .await;
        
        let actor_ref = node.spawn_actor(actor).await.unwrap();

        let mailbox = node.actor_registry()
            .lookup_mailbox(actor_ref.id())
            .await
            .unwrap()
            .unwrap();
        
        let counter = plexspaces_actor::ActorRef::local(
            actor_ref.id().clone(),
            mailbox,
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let msg = Message::new(serde_json::to_vec(&CounterMessage::Increment).unwrap())
            .with_message_type("call".to_string());
        let result = counter
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        let reply: CounterMessage = serde_json::from_slice(result.payload()).unwrap();
        if let CounterMessage::Count(count) = reply {
            assert_eq!(count, 1);
        } else {
            panic!("Expected Count message");
        }
    }
}
