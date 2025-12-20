// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Ractor (Rust Actor Framework - Native Rust Actors)

use plexspaces_actor::{ActorBuilder, ActorRef, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// Calculator message (Ractor-style)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CalculatorMessage {
    Add { a: f64, b: f64 },
    Subtract { a: f64, b: f64 },
    Multiply { a: f64, b: f64 },
    Divide { a: f64, b: f64 },
    Result { value: f64 },
}

/// Calculator actor (Ractor-style Rust-native actor)
/// Demonstrates: Rust-Native Actors, Message Passing, Type Safety
pub struct CalculatorActor {
    operation_count: u64,
}

impl CalculatorActor {
    pub fn new() -> Self {
        Self { operation_count: 0 }
    }
}

#[async_trait::async_trait]
impl Actor for CalculatorActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        
    ) -> Result<(), BehaviorError> {
        <Self as GenServer>::route_message(self, ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for CalculatorActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        let calc_msg: CalculatorMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        self.operation_count += 1;
        
        match calc_msg {
            CalculatorMessage::Add { a, b } => {
                let result = a + b;
                info!("[CALCULATOR] Add: {} + {} = {} (operation #{})", a, b, result, self.operation_count);
                let reply = CalculatorMessage::Result { value: result };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            CalculatorMessage::Subtract { a, b } => {
                let result = a - b;
                info!("[CALCULATOR] Subtract: {} - {} = {} (operation #{})", a, b, result, self.operation_count);
                let reply = CalculatorMessage::Result { value: result };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            CalculatorMessage::Multiply { a, b } => {
                let result = a * b;
                info!("[CALCULATOR] Multiply: {} * {} = {} (operation #{})", a, b, result, self.operation_count);
                let reply = CalculatorMessage::Result { value: result };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            CalculatorMessage::Divide { a, b } => {
                if b == 0.0 {
                    return Err(BehaviorError::ProcessingError("Division by zero".to_string()));
                }
                let result = a / b;
                info!("[CALCULATOR] Divide: {} / {} = {} (operation #{})", a, b, result, self.operation_count);
                let reply = CalculatorMessage::Result { value: result };
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

    info!("=== Ractor vs PlexSpaces Comparison ===");
    info!("Demonstrating Ractor Rust-Native Actors (Rust actor framework)");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Ractor actors are Rust-native with type-safe message passing
    let actor_id: ActorId = "calculator/ractor-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating calculator actor (Ractor-style Rust-native actor)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Spawn using ActorFactory with facets
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
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
    let calculator = plexspaces_actor::ActorRef::remote(
        actor_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );

    info!("✅ Calculator actor created: {}", calculator.id());
    info!("✅ Rust-native: Type-safe message passing");
    info!("✅ Actor isolation: State protected by actor boundaries");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test arithmetic operations (Ractor: actor.call(Add { a: 10, b: 5 }))
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test: Arithmetic operations (Ractor-style message passing)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let operations = vec![
        ("Add", CalculatorMessage::Add { a: 10.0, b: 5.0 }),
        ("Subtract", CalculatorMessage::Subtract { a: 10.0, b: 5.0 }),
        ("Multiply", CalculatorMessage::Multiply { a: 10.0, b: 5.0 }),
        ("Divide", CalculatorMessage::Divide { a: 10.0, b: 5.0 }),
    ];

    for (op_name, op_msg) in operations {
        let msg = Message::new(serde_json::to_vec(&op_msg)?)
            .with_message_type("call".to_string());
        let result = calculator
            .ask(msg, Duration::from_secs(5))
            .await?;
        let reply: CalculatorMessage = serde_json::from_slice(result.payload())?;
        if let CalculatorMessage::Result { value } = reply {
            info!("✅ {} result: {}", op_name, value);
        }
    }

    info!("=== Comparison Complete ===");
    info!("✅ Rust-Native Actors: Type-safe message passing");
    info!("✅ GenServerBehavior: Ractor-style request-reply pattern");
    info!("✅ Actor Isolation: State protected by actor boundaries");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_calculator() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "calculator/test-1@test-node".to_string();
        let behavior = Box::new(CalculatorActor::new());
        let mut actor = ActorBuilder::new(behavior)
            .with_id(actor_id.clone())
            .build()
            .await;
        
        // Spawn using ActorFactory with spawn_actor
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
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
        let calculator = plexspaces_actor::ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let msg = Message::new(serde_json::to_vec(&CalculatorMessage::Add {
            a: 10.0,
            b: 5.0,
        }).unwrap())
            .with_message_type("call".to_string());
        let result = calculator
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        let reply: CalculatorMessage = serde_json::from_slice(result.payload()).unwrap();
        if let CalculatorMessage::Result { value } = reply {
            assert_eq!(value, 15.0);
        } else {
            panic!("Expected Result message");
        }
    }
}
