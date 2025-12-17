// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: MozartSpaces (XVSM - eXtended Virtual Shared Memory with Coordinator Objects)
// Based on: Modern implementations like MozartSpaces (XVSM) introduce coordinator objects
// defining how tuples are stored and fetched with patterns like FIFO, LIFO, and Label-based retrieval

use plexspaces_actor::{ActorBuilder, ActorRef, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId};
use plexspaces_tuplespace::{Tuple, TupleField, Pattern, PatternField};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::collections::VecDeque;
use tracing::info;

/// Coordinator type (XVSM pattern)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordinatorType {
    FIFO,      // First-In-First-Out
    LIFO,      // Last-In-First-Out
    Label,     // Label-based retrieval
    Priority,  // Priority-based
}

/// Tuple with label (XVSM pattern)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabeledTuple {
    pub label: String,
    pub data: Vec<u8>,
    pub priority: u32,
}

/// XVSM coordinator message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum XVSMMessage {
    Write { tuple: LabeledTuple, coordinator: CoordinatorType },
    Read { pattern: String, coordinator: CoordinatorType },
    Take { pattern: String, coordinator: CoordinatorType },
    Tuple { tuple: Option<LabeledTuple> },
}

/// XVSM Coordinator Actor (MozartSpaces-style)
/// Demonstrates: TupleSpace Coordination, XVSM Patterns (FIFO, LIFO, Label-based), Coordinator Objects
pub struct XVSMCoordinatorActor {
    fifo_queue: VecDeque<LabeledTuple>,
    lifo_stack: Vec<LabeledTuple>,
    labeled_tuples: std::collections::HashMap<String, Vec<LabeledTuple>>,
    priority_queue: Vec<LabeledTuple>, // Simplified (would use heap in production)
}

impl XVSMCoordinatorActor {
    pub fn new() -> Self {
        Self {
            fifo_queue: VecDeque::new(),
            lifo_stack: Vec::new(),
            labeled_tuples: std::collections::HashMap::new(),
            priority_queue: Vec::new(),
        }
    }

    fn write_fifo(&mut self, tuple: LabeledTuple) {
        self.fifo_queue.push_back(tuple);
    }

    fn read_fifo(&self) -> Option<LabeledTuple> {
        self.fifo_queue.front().cloned()
    }

    fn take_fifo(&mut self) -> Option<LabeledTuple> {
        self.fifo_queue.pop_front()
    }

    fn write_lifo(&mut self, tuple: LabeledTuple) {
        self.lifo_stack.push(tuple);
    }

    fn read_lifo(&self) -> Option<LabeledTuple> {
        self.lifo_stack.last().cloned()
    }

    fn take_lifo(&mut self) -> Option<LabeledTuple> {
        self.lifo_stack.pop()
    }

    fn write_label(&mut self, tuple: LabeledTuple) {
        self.labeled_tuples
            .entry(tuple.label.clone())
            .or_insert_with(Vec::new)
            .push(tuple);
    }

    fn read_label(&self, label: &str) -> Option<LabeledTuple> {
        self.labeled_tuples.get(label)?.first().cloned()
    }

    fn take_label(&mut self, label: &str) -> Option<LabeledTuple> {
        let tuples = self.labeled_tuples.get_mut(label)?;
        if tuples.is_empty() {
            None
        } else {
            Some(tuples.remove(0))
        }
    }

    fn write_priority(&mut self, tuple: LabeledTuple) {
        self.priority_queue.push(tuple);
        // Sort by priority (highest first) - simplified
        self.priority_queue.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    fn read_priority(&self) -> Option<LabeledTuple> {
        self.priority_queue.first().cloned()
    }

    fn take_priority(&mut self) -> Option<LabeledTuple> {
        if self.priority_queue.is_empty() {
            None
        } else {
            Some(self.priority_queue.remove(0))
        }
    }
}

#[async_trait::async_trait]
impl Actor for XVSMCoordinatorActor {
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
impl GenServer for XVSMCoordinatorActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        let xvsm_msg: XVSMMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match xvsm_msg {
            XVSMMessage::Write { tuple, coordinator } => {
                match coordinator {
                    CoordinatorType::FIFO => {
                        self.write_fifo(tuple.clone());
                        info!("[XVSM] Wrote tuple to FIFO queue (label: {})", tuple.label);
                    }
                    CoordinatorType::LIFO => {
                        self.write_lifo(tuple.clone());
                        info!("[XVSM] Wrote tuple to LIFO stack (label: {})", tuple.label);
                    }
                    CoordinatorType::Label => {
                        self.write_label(tuple.clone());
                        info!("[XVSM] Wrote tuple with label: {}", tuple.label);
                    }
                    CoordinatorType::Priority => {
                        self.write_priority(tuple.clone());
                        info!("[XVSM] Wrote tuple to priority queue (priority: {})", tuple.priority);
                    }
                }
                let reply = XVSMMessage::Tuple { tuple: Some(tuple) };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            XVSMMessage::Read { pattern, coordinator } => {
                let tuple = match coordinator {
                    CoordinatorType::FIFO => self.read_fifo(),
                    CoordinatorType::LIFO => self.read_lifo(),
                    CoordinatorType::Label => self.read_label(&pattern),
                    CoordinatorType::Priority => self.read_priority(),
                };
                info!("[XVSM] Read tuple (coordinator: {:?}, pattern: {})", coordinator, pattern);
                let reply = XVSMMessage::Tuple { tuple };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            XVSMMessage::Take { pattern, coordinator } => {
                let tuple = match coordinator {
                    CoordinatorType::FIFO => self.take_fifo(),
                    CoordinatorType::LIFO => self.take_lifo(),
                    CoordinatorType::Label => self.take_label(&pattern),
                    CoordinatorType::Priority => self.take_priority(),
                };
                info!("[XVSM] Took tuple (coordinator: {:?}, pattern: {})", coordinator, pattern);
                let reply = XVSMMessage::Tuple { tuple };
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

    info!("=== MozartSpaces vs PlexSpaces Comparison ===");
    info!("Demonstrating XVSM (eXtended Virtual Shared Memory) with Coordinator Objects");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // MozartSpaces uses coordinator objects to define tuple storage/retrieval patterns
    let actor_id: ActorId = "xvsm-coordinator/mozartspaces-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating XVSM coordinator actor (MozartSpaces-style coordinator objects)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(XVSMCoordinatorActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await;
    
    // Spawn using ActorFactory
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
    let actor_id = actor.id().clone();
    let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    let actor_ref = plexspaces_core::ActorRef::new(actor_id)
        .map_err(|e| format!("Failed to create ActorRef: {}", e))?;

    let mailbox = node.actor_registry()
        .lookup_mailbox(actor_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    
    let coordinator = plexspaces_actor::ActorRef::local(
        actor_ref.id().clone(),
        mailbox,
        node.service_locator().clone(),
    );

    info!("✅ XVSM coordinator created: {}", coordinator.id());
    info!("✅ Coordinator patterns: FIFO, LIFO, Label-based, Priority");
    info!("✅ TupleSpace coordination: Extended Linda model with coordinators");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test FIFO coordinator
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 1: FIFO Coordinator (First-In-First-Out)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    for i in 0..3 {
        let tuple = LabeledTuple {
            label: format!("item-{}", i),
            data: format!("data-{}", i).into_bytes(),
            priority: i as u32,
        };
        let msg = Message::new(serde_json::to_vec(&XVSMMessage::Write {
            tuple: tuple.clone(),
            coordinator: CoordinatorType::FIFO,
        })?)
            .with_message_type("call".to_string());
        coordinator.ask(msg, Duration::from_secs(5)).await?;
    }
    
    // Take from FIFO (should get item-0 first)
    let msg = Message::new(serde_json::to_vec(&XVSMMessage::Take {
        pattern: "".to_string(),
        coordinator: CoordinatorType::FIFO,
    })?)
        .with_message_type("call".to_string());
    let result = coordinator.ask(msg, Duration::from_secs(5)).await?;
    let reply: XVSMMessage = serde_json::from_slice(result.payload())?;
    if let XVSMMessage::Tuple { tuple: Some(t) } = reply {
        info!("✅ FIFO take: {} (should be item-0)", t.label);
        assert_eq!(t.label, "item-0");
    }

    // Test LIFO coordinator
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 2: LIFO Coordinator (Last-In-First-Out)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    for i in 0..3 {
        let tuple = LabeledTuple {
            label: format!("stack-{}", i),
            data: format!("data-{}", i).into_bytes(),
            priority: i as u32,
        };
        let msg = Message::new(serde_json::to_vec(&XVSMMessage::Write {
            tuple: tuple.clone(),
            coordinator: CoordinatorType::LIFO,
        })?)
            .with_message_type("call".to_string());
        coordinator.ask(msg, Duration::from_secs(5)).await?;
    }
    
    // Take from LIFO (should get stack-2 first)
    let msg = Message::new(serde_json::to_vec(&XVSMMessage::Take {
        pattern: "".to_string(),
        coordinator: CoordinatorType::LIFO,
    })?)
        .with_message_type("call".to_string());
    let result = coordinator.ask(msg, Duration::from_secs(5)).await?;
    let reply: XVSMMessage = serde_json::from_slice(result.payload())?;
    if let XVSMMessage::Tuple { tuple: Some(t) } = reply {
        info!("✅ LIFO take: {} (should be stack-2)", t.label);
        assert_eq!(t.label, "stack-2");
    }

    // Test Label-based coordinator
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 3: Label-based Coordinator");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let tuple = LabeledTuple {
        label: "config".to_string(),
        data: "timeout=30s".to_string().into_bytes(),
        priority: 0,
    };
    let msg = Message::new(serde_json::to_vec(&XVSMMessage::Write {
        tuple: tuple.clone(),
        coordinator: CoordinatorType::Label,
    })?)
        .with_message_type("call".to_string());
    coordinator.ask(msg, Duration::from_secs(5)).await?;
    
    // Read by label
    let msg = Message::new(serde_json::to_vec(&XVSMMessage::Read {
        pattern: "config".to_string(),
        coordinator: CoordinatorType::Label,
    })?)
        .with_message_type("call".to_string());
    let result = coordinator.ask(msg, Duration::from_secs(5)).await?;
    let reply: XVSMMessage = serde_json::from_slice(result.payload())?;
    if let XVSMMessage::Tuple { tuple: Some(t) } = reply {
        info!("✅ Label read: {} = {}", t.label, String::from_utf8_lossy(&t.data));
        assert_eq!(t.label, "config");
    }

    info!("=== Comparison Complete ===");
    info!("✅ XVSM Coordinator Objects: FIFO, LIFO, Label-based, Priority patterns");
    info!("✅ TupleSpace Coordination: Extended Linda model with coordinators");
    info!("✅ Pattern-based retrieval: Flexible tuple access patterns");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_xvsm_coordinator() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "xvsm-coordinator/test-1@test-node".to_string();
        let behavior = Box::new(XVSMCoordinatorActor::new());
        let mut actor = ActorBuilder::new(behavior)
            .with_id(actor_id.clone())
            .build()
            .await;
        
        // Spawn using ActorFactory
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        let actor_id = actor.id().clone();
        let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();
        let actor_ref = plexspaces_core::ActorRef::new(actor_id)
            .map_err(|e| format!("Failed to create ActorRef: {}", e)).unwrap();

        let mailbox = node.actor_registry()
            .lookup_mailbox(actor_ref.id())
            .await
            .unwrap()
            .unwrap();
        
        let coordinator = plexspaces_actor::ActorRef::local(
            actor_ref.id().clone(),
            mailbox,
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Test FIFO
        let tuple = LabeledTuple {
            label: "test".to_string(),
            data: vec![0x01, 0x02],
            priority: 1,
        };
        let msg = Message::new(serde_json::to_vec(&XVSMMessage::Write {
            tuple: tuple.clone(),
            coordinator: CoordinatorType::FIFO,
        }).unwrap())
            .with_message_type("call".to_string());
        coordinator.ask(msg, Duration::from_secs(5)).await.unwrap();

        let msg = Message::new(serde_json::to_vec(&XVSMMessage::Take {
            pattern: "".to_string(),
            coordinator: CoordinatorType::FIFO,
        }).unwrap())
            .with_message_type("call".to_string());
        let result = coordinator.ask(msg, Duration::from_secs(5)).await.unwrap();

        let reply: XVSMMessage = serde_json::from_slice(result.payload()).unwrap();
        if let XVSMMessage::Tuple { tuple: Some(t) } = reply {
            assert_eq!(t.label, "test");
        } else {
            panic!("Expected Tuple message");
        }
    }
}
