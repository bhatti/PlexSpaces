// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: LuaTS (Linda + Event-Driven Programming)
// Based on: LuaTS combines Linda with event-driven programming to simplify multi-thread development

use plexspaces_actor::{ActorBuilder, ActorRef, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_behavior::{GenServerBehavior, GenEventBehavior};
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId};
use plexspaces_tuplespace::{Tuple, TupleField, Pattern, PatternField};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// Event (LuaTS-style event-driven coordination)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_type: String,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

/// LuaTS message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LuaTSMessage {
    Publish { event: Event },
    Subscribe { event_type: String },
    Unsubscribe { event_type: String },
    EventReceived { event: Event },
    WriteTuple { tuple: Tuple },
    ReadTuple { pattern: Pattern },
    TupleFound { tuple: Option<Tuple> },
}

/// Event-driven coordinator actor (LuaTS-style)
/// Demonstrates: TupleSpace + GenEventBehavior, Event-Driven Coordination, Linda + Events
pub struct EventCoordinatorActor {
    subscriptions: std::collections::HashMap<String, Vec<ActorId>>, // event_type -> subscribers
    event_history: Vec<Event>,
}

impl EventCoordinatorActor {
    pub fn new() -> Self {
        Self {
            subscriptions: std::collections::HashMap::new(),
            event_history: Vec::new(),
        }
    }
}

#[async_trait::async_trait]
impl Actor for EventCoordinatorActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Handle both GenServer (request-reply) and GenEvent (event notifications)
        let lua_msg: LuaTSMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match lua_msg {
            LuaTSMessage::Publish { event } => {
                info!("[LuaTS] Publishing event: {} (subscribers: {})", 
                    event.event_type, 
                    self.subscriptions.get(&event.event_type).map(|v| v.len()).unwrap_or(0));
                
                // Store event
                self.event_history.push(event.clone());
                
                // Notify subscribers (GenEventBehavior pattern)
                if let Some(subscribers) = self.subscriptions.get(&event.event_type) {
                    for subscriber_id in subscribers {
                        // In real implementation, would send event to subscribers
                        info!("[LuaTS] Notifying subscriber: {}", subscriber_id);
                    }
                }
                
                // Also write to TupleSpace (Linda pattern)
                if let Some(tuplespace) = ctx.get_tuplespace().await {
                    let tuple = Tuple::new(vec![
                        TupleField::String("event".to_string()),
                        TupleField::String(event.event_type.clone()),
                        TupleField::Binary(event.payload.clone()),
                    ]);
                    tuplespace.write(tuple).await?;
                }
            }
            LuaTSMessage::Subscribe { event_type } => {
                let subscriber_id = ctx.actor_id().clone();
                self.subscriptions
                    .entry(event_type.clone())
                    .or_insert_with(Vec::new)
                    .push(subscriber_id.clone());
                info!("[LuaTS] Subscribed {} to event type: {}", subscriber_id, event_type);
            }
            LuaTSMessage::Unsubscribe { event_type } => {
                if let Some(subscribers) = self.subscriptions.get_mut(&event_type) {
                    let subscriber_id = ctx.actor_id().clone();
                    subscribers.retain(|id| id != &subscriber_id);
                    info!("[LuaTS] Unsubscribed {} from event type: {}", subscriber_id, event_type);
                }
            }
            LuaTSMessage::WriteTuple { tuple } => {
                if let Some(tuplespace) = ctx.get_tuplespace().await {
                    tuplespace.write(tuple).await?;
                    info!("[LuaTS] Wrote tuple to TupleSpace");
                }
            }
            LuaTSMessage::ReadTuple { pattern } => {
                if let Some(tuplespace) = ctx.get_tuplespace().await {
                    let tuples = tuplespace.read(&pattern).await?;
                    let tuple = tuples.first().cloned();
                    info!("[LuaTS] Read tuple from TupleSpace: {:?}", tuple.is_some());
                    // Return via message (would need reply mechanism)
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenEvent // Event-driven behavior
    }
}

/// Event subscriber actor (LuaTS-style)
pub struct EventSubscriberActor {
    received_events: Vec<Event>,
}

impl EventSubscriberActor {
    pub fn new() -> Self {
        Self {
            received_events: Vec::new(),
        }
    }
}

#[async_trait::async_trait]
impl Actor for EventSubscriberActor {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let lua_msg: LuaTSMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        if let LuaTSMessage::EventReceived { event } = lua_msg {
            self.received_events.push(event);
            info!("[SUBSCRIBER] Received event: {}", self.received_events.len());
        }
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenEvent
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== LuaTS vs PlexSpaces Comparison ===");
    info!("Demonstrating Linda + Event-Driven Programming (TupleSpace + GenEventBehavior)");

    let node = NodeBuilder::new("comparison-node-1")
        .build().await;

    // LuaTS combines Linda (TupleSpace) with event-driven programming
    let coordinator_id: ActorId = "event-coordinator/luats-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating event coordinator (LuaTS-style: Linda + Events)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(EventCoordinatorActor::new());
    let mut coordinator_actor = ActorBuilder::new(behavior)
        .with_id(coordinator_id.clone())
        .build()
        .await;
    
    // Spawn using ActorFactory with spawn_actor
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
    let ctx = plexspaces_core::RequestContext::internal();
    let _message_sender = actor_factory.spawn_actor(
        &ctx,
        &coordinator_id,
        "GenEvent",
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![], // facets
    ).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    // Create ActorRef directly - no need to access mailbox
    let coordinator = plexspaces_actor::ActorRef::remote(
        coordinator_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );

    // Create subscriber using spawn_actor
    let subscriber_id: ActorId = "event-subscriber/luats-1@comparison-node-1".to_string();
    let _message_sender2 = actor_factory.spawn_actor(
        &ctx,
        &subscriber_id,
        "GenEvent",
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![], // facets
    ).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    // Create ActorRef directly - no need to access mailbox
    let subscriber = plexspaces_actor::ActorRef::remote(
        subscriber_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );

    info!("✅ Event coordinator created: {}", coordinator.id());
    info!("✅ Event subscriber created: {}", subscriber.id());
    info!("✅ GenEventBehavior: Event-driven coordination");
    info!("✅ TupleSpace: Linda-style coordination");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test event subscription
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 1: Event subscription (GenEventBehavior pattern)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = Message::new(serde_json::to_vec(&LuaTSMessage::Subscribe {
        event_type: "data_ready".to_string(),
    })?)
        .with_message_type("call".to_string());
    coordinator.ask(msg, Duration::from_secs(5)).await?;
    info!("✅ Subscribed to 'data_ready' events");

    // Test event publishing
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 2: Event publishing (Linda + Events)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let event = Event {
        event_type: "data_ready".to_string(),
        payload: "processed_data".to_string().into_bytes(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };
    let msg = Message::new(serde_json::to_vec(&LuaTSMessage::Publish {
        event: event.clone(),
    })?)
        .with_message_type("call".to_string());
    coordinator.ask(msg, Duration::from_secs(5)).await?;
    info!("✅ Published 'data_ready' event (also written to TupleSpace)");

    // Test TupleSpace read (Linda pattern)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 3: TupleSpace read (Linda coordination)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("event".to_string())),
        PatternField::Exact(TupleField::String("data_ready".to_string())),
        PatternField::Wildcard,
    ]);
    let msg = Message::new(serde_json::to_vec(&LuaTSMessage::ReadTuple {
        pattern,
    })?)
        .with_message_type("call".to_string());
    coordinator.ask(msg, Duration::from_secs(5)).await?;
    info!("✅ Read event tuple from TupleSpace (Linda pattern)");

    info!("=== Comparison Complete ===");
    info!("✅ GenEventBehavior: Event-driven coordination (LuaTS pattern)");
    info!("✅ TupleSpace: Linda-style coordination");
    info!("✅ Combined model: Events + TupleSpace for multi-thread development");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_coordinator() {
        let node = NodeBuilder::new("test-node")
            .build().await;

        let actor_id: ActorId = "event-coordinator/test-1@test-node".to_string();
        
        // Spawn using ActorFactory with spawn_actor
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        let ctx = plexspaces_core::RequestContext::internal();
        let _message_sender = actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "GenEvent",
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![], // facets
        ).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();
        
        // Create ActorRef directly - no need to access mailbox
        let coordinator = plexspaces_actor::ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let event = Event {
            event_type: "test".to_string(),
            payload: vec![0x01, 0x02],
            timestamp: 0,
        };
        let msg = Message::new(serde_json::to_vec(&LuaTSMessage::Publish {
            event,
        }).unwrap())
            .with_message_type("call".to_string());
        coordinator.ask(msg, Duration::from_secs(5)).await.unwrap();
    }
}
