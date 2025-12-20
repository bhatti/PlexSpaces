// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Erlang/OTP GenServer with Supervision

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::GenServer;
use plexspaces_core::{ActorId, Actor, ActorContext, BehaviorType, BehaviorError, Reply};
use plexspaces_mailbox::Message;
use plexspaces_node::{Node, NodeBuilder};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn};

/// Counter message types (equivalent to Erlang gen_server calls)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CounterMessage {
    Increment,
    Decrement,
    Get,
    Count(u64),
}

/// Counter state (equivalent to Erlang #state record)
#[derive(Debug, Clone)]
pub struct CounterState {
    count: u64,
}

/// Counter actor implementing GenServer behavior
pub struct CounterActor {
    state: CounterState,
}

impl CounterActor {
    pub fn new() -> Self {
        Self {
            state: CounterState { count: 0 },
        }
    }
}

// Actor is a struct, not a trait - no need to implement it

#[async_trait::async_trait]
impl Actor for CounterActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        reply: &dyn Reply,
    ) -> Result<(), BehaviorError> {
        // Delegate to GenServer's route_message
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
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        // Parse message payload
        let counter_msg: CounterMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse message: {}", e)))?;
        
        match counter_msg {
            CounterMessage::Increment => {
                self.state.count += 1;
                info!("Counter incremented to: {}", self.state.count);
                let reply = CounterMessage::Count(self.state.count);
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            CounterMessage::Decrement => {
                if self.state.count > 0 {
                    self.state.count -= 1;
                }
                info!("Counter decremented to: {}", self.state.count);
                let reply = CounterMessage::Count(self.state.count);
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            CounterMessage::Get => {
                info!("Counter get: {}", self.state.count);
                let reply = CounterMessage::Count(self.state.count);
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            _ => Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== Erlang/OTP vs PlexSpaces Comparison ===");
    info!("Demonstrating GenServer with Supervision");

    // Create a node
    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Spawn counter actor (equivalent to Erlang gen_server:start_link)
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
    let actor_id = "counter@comparison-node-1".to_string();
    let ctx = plexspaces_core::RequestContext::internal();
    let _message_sender = actor_factory.spawn_actor(
        &ctx,
        &actor_id,
        "GenServer",
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![], // facets
    ).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    // Create ActorRef directly - no need to access mailbox
    let counter_actor = plexspaces_actor::ActorRef::remote(
        actor_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );

    info!("Counter actor spawned: {}", counter_actor.id());

    // Wait for actor to be fully initialized
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test increment (equivalent to gen_server:call(counter, increment))
    let msg = Message::new(serde_json::to_vec(&CounterMessage::Increment)?)
        .with_message_type("call".to_string());
    let result = counter_actor
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: CounterMessage = serde_json::from_slice(result.payload())?;
    if let CounterMessage::Count(count) = reply {
        info!("Increment result: {}", count);
        assert_eq!(count, 1);
    }

    // Test increment again
    let msg = Message::new(serde_json::to_vec(&CounterMessage::Increment)?)
        .with_message_type("call".to_string());
    let result = counter_actor
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: CounterMessage = serde_json::from_slice(result.payload())?;
    if let CounterMessage::Count(count) = reply {
        info!("Increment result: {}", count);
        assert_eq!(count, 2);
    }

    // Test decrement
    let msg = Message::new(serde_json::to_vec(&CounterMessage::Decrement)?)
        .with_message_type("call".to_string());
    let result = counter_actor
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: CounterMessage = serde_json::from_slice(result.payload())?;
    if let CounterMessage::Count(count) = reply {
        info!("Decrement result: {}", count);
        assert_eq!(count, 1);
    }

    // Test get
    let msg = Message::new(serde_json::to_vec(&CounterMessage::Get)?)
        .with_message_type("call".to_string());
    let result = counter_actor
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: CounterMessage = serde_json::from_slice(result.payload())?;
    if let CounterMessage::Count(count) = reply {
        info!("Get result: {}", count);
        assert_eq!(count, 1);
    }

    info!("=== Comparison Complete ===");
    info!("All operations successful!");

    // Cleanup - Node doesn't have shutdown method, just drop it

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_counter_increment() {
        let node = NodeBuilder::new("test-node")
            .build();

        // Spawn using ActorFactory with facets
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        use std::sync::Arc;
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        let actor_id = "test-counter@test-node".to_string();
        let ctx = plexspaces_core::RequestContext::internal();
        let _message_sender = actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "GenServer",
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![], // facets
        ).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();
        
        // Create ActorRef directly - no need to access mailbox
        let counter = plexspaces_actor::ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        );

        // Wait for actor to be fully initialized
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

    #[tokio::test]
    async fn test_counter_decrement() {
        let node = NodeBuilder::new("test-node-2")
            .build();

        let behavior = Box::new(CounterActor::new());
        let actor = ActorBuilder::new(behavior)
            .with_id("test-counter-2@test-node-2".to_string())
            .build()
            .await;
        // Spawn using ActorFactory with facets
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        use std::sync::Arc;
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        let actor_id = "test-counter-2@test-node-2".to_string();
        let ctx = plexspaces_core::RequestContext::internal();
        let _message_sender = actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "GenServer",
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![], // facets
        ).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();
        
        // Create ActorRef directly - no need to access mailbox
        let counter = plexspaces_actor::ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        );

        // Wait for actor to be fully initialized
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Increment first
        let msg = Message::new(serde_json::to_vec(&CounterMessage::Increment).unwrap())
            .with_message_type("call".to_string());
        counter
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        // Then decrement
        let msg = Message::new(serde_json::to_vec(&CounterMessage::Decrement).unwrap())
            .with_message_type("call".to_string());
        let result = counter
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        let reply: CounterMessage = serde_json::from_slice(result.payload()).unwrap();
        if let CounterMessage::Count(count) = reply {
            assert_eq!(count, 0);
        } else {
            panic!("Expected Count message");
        }
    }
}
