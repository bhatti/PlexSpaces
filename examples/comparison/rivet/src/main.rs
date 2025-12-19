// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Rivet Actors (Cloudflare Durable Objects pattern)

use plexspaces_actor::{ActorBuilder, ActorRef, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_behavior::GenServerBehavior;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId};
use plexspaces_journaling::{VirtualActorFacet, DurabilityFacet, DurabilityConfig, MemoryJournalStorage};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// Counter message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CounterMessage {
    Increment,
    Decrement,
    Get,
    Count(u64),
}

/// Counter actor (Rivet-style actor, similar to Cloudflare Durable Objects)
/// Demonstrates: Virtual Actor Lifecycle, Automatic Activation, State Persistence
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
    ) -> Result<(), BehaviorError> {
        // Delegate to GenServerBehavior's route_message
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServerBehavior for CounterActor {
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
                // State automatically persisted via DurabilityFacet
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
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== Rivet vs PlexSpaces Comparison ===");
    info!("Demonstrating Rivet Actors (Cloudflare Durable Objects pattern)");

    // Create a node
    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Create counter actor with VirtualActorFacet and DurabilityFacet
    // Rivet actors are virtual actors with automatic activation and state persistence
    let actor_id: ActorId = "counter/rivet-1@comparison-node-1".to_string();
    
    // Create counter actor with VirtualActorFacet and DurabilityFacet
    // Rivet actors are virtual actors with automatic activation and state persistence
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating counter actor (Rivet-style virtual actor)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(CounterActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await;
    
    // Attach VirtualActorFacet (Rivet actors are virtual)
    let virtual_facet_config = serde_json::json!({
        "idle_timeout": "5m",
        "activation_strategy": "lazy"
    });
    let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone()));
    actor
        .attach_facet(virtual_facet, 100, virtual_facet_config)
        .await?;
    
    // Attach DurabilityFacet (Rivet actors persist state)
    let storage = MemoryJournalStorage::new();
    let durability_config = DurabilityConfig::default();
    let durability_facet = Box::new(DurabilityFacet::new(storage, durability_config));
    actor
        .attach_facet(durability_facet, 50, serde_json::json!({}))
        .await?;
    
    // Spawn using ActorFactory
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
    let actor_id = actor.id().clone();
    let ctx = plexspaces_core::RequestContext::internal();
    // Note: Since actor has VirtualActorFacet attached, we use spawn_built_actor
    let _message_sender = actor_factory.spawn_built_actor(&ctx, Arc::new(actor), Some("GenServer".to_string())).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    let core_ref = plexspaces_core::ActorRef::new(actor_id)
        .map_err(|e| format!("Failed to create ActorRef: {}", e))?;

    // Convert to actor::ActorRef for ask method
    let mailbox = node.actor_registry()
        .lookup_mailbox(core_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    
    let counter = plexspaces_actor::ActorRef::local(
        core_ref.id().clone(),
        mailbox,
        node.service_locator().clone(),
    );

    info!("✅ Counter actor created/activated: {}", counter.id());
    info!("✅ VirtualActorFacet attached - automatic activation/deactivation");
    info!("✅ DurabilityFacet attached - state persistence enabled");

    // Wait for actor to be fully initialized
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test increment (Rivet: await actor.increment())
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 1: Increment (triggers automatic activation if deactivated)");
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

    // Demonstrate get_or_activate with same ID (should return existing actor)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 2: get_or_activate with same ID (should return existing actor)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let existing_ref = node.get_or_activate_actor(actor_id.clone(), || async {
        // This factory should not be called since actor already exists
        panic!("Factory should not be called for existing actor");
    }).await?;
    info!("✅ get_or_activate returned existing actor: {}", existing_ref.id());
    info!("   - Virtual actor is addressable even when deactivated");
    info!("   - get_or_activate automatically reactivates if needed");

    info!("=== Comparison Complete ===");
    info!("✅ VirtualActorFacet: Automatic activation/deactivation (Rivet pattern)");
    info!("✅ DurabilityFacet: State persistence across requests");
    info!("✅ Virtual actor pattern: Automatic activation on first message");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_counter_increment() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "counter/test-1@test-node".to_string();
        let behavior = Box::new(CounterActor::new());
        let mut actor = ActorBuilder::new(behavior)
            .with_id(actor_id.clone())
            .build()
            .await;
        
        let virtual_facet = Box::new(VirtualActorFacet::new(serde_json::json!({})));
        actor.attach_facet(virtual_facet, 100, serde_json::json!({})).await.unwrap();
        
        let storage = MemoryJournalStorage::new();
        let durability_facet = Box::new(DurabilityFacet::new(storage, DurabilityConfig::default()));
        actor.attach_facet(durability_facet, 50, serde_json::json!({})).await.unwrap();
        
        // Spawn using ActorFactory
        // Note: Since actor has VirtualActorFacet attached, we use spawn_built_actor
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        let actor_id = actor.id().clone();
        let ctx = plexspaces_core::RequestContext::internal();
        let _message_sender = actor_factory.spawn_built_actor(&ctx, Arc::new(actor), Some("GenServer".to_string())).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();
        let core_ref = plexspaces_core::ActorRef::new(actor_id)
            .map_err(|e| format!("Failed to create ActorRef: {}", e)).unwrap();

        let mailbox = node.actor_registry()
            .lookup_mailbox(core_ref.id())
            .await
            .unwrap()
            .unwrap();
        
        let counter = plexspaces_actor::ActorRef::local(
            core_ref.id().clone(),
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
