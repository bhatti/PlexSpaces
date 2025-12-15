// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Dirigo (Distributed Stream Processing with Virtual Actors)
// Based on: https://arxiv.org/abs/2308.03615
// Use Case: Real-time Event Stream Processing (Sensor Data → Filtering → Aggregation → Alerting)

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId, Reply};
use plexspaces_journaling::{VirtualActorFacet, DurabilityFacet, DurabilityConfig, MemoryJournalStorage};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;
use std::collections::HashMap;

/// Sensor event (real-time event stream)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorEvent {
    pub event_id: String,
    pub sensor_id: String,
    pub sensor_type: String, // "temperature", "pressure", "motion"
    pub value: f64,
    pub timestamp: u64,
    pub location: String,
}

/// Stream operator (Dirigo-style virtual actor for stream processing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamOperator {
    pub operator_id: String,
    pub operator_type: String, // "map", "filter", "reduce", "window"
    pub config: std::collections::HashMap<String, serde_json::Value>,
}

/// Aggregated result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedResult {
    pub operator_id: String,
    pub aggregation_type: String,
    pub value: f64,
    pub count: u64,
    pub timestamp: u64,
}

/// Dirigo stream processing message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DirigoMessage {
    CreateOperator { operator: StreamOperator },
    ProcessEvent { operator_id: String, event: SensorEvent },
    ProcessEventBatch { operator_id: String, events: Vec<SensorEvent> },
    StreamResult { operator_id: String, result: Vec<u8> },
    AggregatedResult { result: AggregatedResult },
    GetStats,
    Stats { processed_count: u64, operator_type: String },
}

/// Stream processing actor (Dirigo-style virtual actors for stream processing)
/// Demonstrates: VirtualActorFacet, Stream Processing, Time-Sharing Compute Resources
pub struct StreamProcessingActor {
    operator_id: String,
    operator_type: String,
    processed_count: u64,
    config: std::collections::HashMap<String, serde_json::Value>,
    // For aggregation operators
    aggregated_values: Vec<f64>,
    window_size: usize,
}

impl StreamProcessingActor {
    pub fn new(operator: StreamOperator) -> Self {
        let window_size = operator.config
            .get("window_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;
        
        Self {
            operator_id: operator.operator_id.clone(),
            operator_type: operator.operator_type.clone(),
            processed_count: 0,
            config: operator.config.clone(),
            aggregated_values: Vec::new(),
            window_size,
        }
    }

    async fn process_event(&mut self, event: &SensorEvent) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        self.processed_count += 1;
        info!("[DIRIGO] Processing event: {} (operator: {}, type: {}, count: {})", 
            event.event_id, self.operator_id, self.operator_type, self.processed_count);
        
        // Simulate stream processing based on operator type
        match self.operator_type.as_str() {
            "map" => {
                // Map operation: transform sensor data
                let transformed = format!("{}:{}={:.2}", event.sensor_type, event.sensor_id, event.value);
                Ok(transformed.into_bytes())
            }
            "filter" => {
                // Filter operation: filter events by threshold
                let threshold = self.config
                    .get("threshold")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(50.0);
                
                if event.value > threshold {
                    Ok(serde_json::to_vec(event)?)
                } else {
                    Ok(vec![]) // Filtered out
                }
            }
            "reduce" => {
                // Reduce operation: aggregate values
                self.aggregated_values.push(event.value);
                if self.aggregated_values.len() >= self.window_size {
                    let sum: f64 = self.aggregated_values.iter().sum();
                    let avg = sum / self.aggregated_values.len() as f64;
                    self.aggregated_values.clear();
                    Ok(format!("aggregated:avg={:.2},count={}", avg, self.window_size).into_bytes())
                } else {
                    Ok(vec![]) // Not enough values yet
                }
            }
            "window" => {
                // Window operation: windowed aggregation
                self.aggregated_values.push(event.value);
                if self.aggregated_values.len() >= self.window_size {
                    let max = self.aggregated_values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
                    let min = self.aggregated_values.iter().fold(f64::INFINITY, |a, &b| a.min(b));
                    self.aggregated_values.clear();
                    Ok(format!("windowed:max={:.2},min={:.2},size={}", max, min, self.window_size).into_bytes())
                } else {
                    Ok(vec![]) // Window not full yet
                }
            }
            _ => Ok(serde_json::to_vec(event)?),
        }
    }

    async fn process_event_batch(&mut self, events: &[SensorEvent]) -> Result<Vec<AggregatedResult>, Box<dyn std::error::Error + Send + Sync>> {
        let mut results = Vec::new();
        
        for event in events {
            let _ = self.process_event(event).await?;
        }
        
        // For reduce/window operators, return aggregated results
        if self.operator_type == "reduce" && self.aggregated_values.len() >= self.window_size {
            let sum: f64 = self.aggregated_values.iter().sum();
            let avg = sum / self.aggregated_values.len() as f64;
            results.push(AggregatedResult {
                operator_id: self.operator_id.clone(),
                aggregation_type: "average".to_string(),
                value: avg,
                count: self.aggregated_values.len() as u64,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            });
            self.aggregated_values.clear();
        }
        
        Ok(results)
    }
}

#[async_trait::async_trait]
impl Actor for StreamProcessingActor {
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
impl GenServer for StreamProcessingActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        let dirigo_msg: DirigoMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match dirigo_msg {
            DirigoMessage::ProcessEvent { operator_id, event } => {
                if operator_id != self.operator_id {
                    return Err(BehaviorError::ProcessingError("Operator ID mismatch".to_string()));
                }
                
                let result = self.process_event(&event).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Processing failed: {}", e)))?;
                
                let reply = DirigoMessage::StreamResult {
                    operator_id: self.operator_id.clone(),
                    result,
                };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            DirigoMessage::ProcessEventBatch { operator_id, events } => {
                if operator_id != self.operator_id {
                    return Err(BehaviorError::ProcessingError("Operator ID mismatch".to_string()));
                }
                
                let results = self.process_event_batch(&events).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Batch processing failed: {}", e)))?;
                
                // Return first aggregated result if available
                if let Some(result) = results.first() {
                    let reply = DirigoMessage::AggregatedResult {
                        result: result.clone(),
                    };
                    Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
                } else {
                    let reply = DirigoMessage::StreamResult {
                        operator_id: self.operator_id.clone(),
                        result: vec![],
                    };
                    Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
                }
            }
            DirigoMessage::GetStats => {
                let reply = DirigoMessage::Stats {
                    processed_count: self.processed_count,
                    operator_type: self.operator_type.clone(),
                };
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

    info!("=== Dirigo vs PlexSpaces Comparison ===");
    info!("Demonstrating Distributed Stream Processing with Virtual Actors");
    info!("Use Case: Real-time Event Stream Processing (Sensor Data → Filtering → Aggregation → Alerting)");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Dirigo uses virtual actors for stream processing operators
    // Create multiple operators for a stream processing pipeline
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating stream processing operators (Dirigo-style virtual actors)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Operator 1: Map (transform sensor data)
    let map_operator_id = "stream-operator/map-1@comparison-node-1".to_string();
    let map_operator = StreamOperator {
        operator_id: map_operator_id.clone(),
        operator_type: "map".to_string(),
        config: HashMap::new(),
    };
    
    let behavior = Box::new(StreamProcessingActor::new(map_operator.clone()));
    let mut map_actor = ActorBuilder::new(behavior)
        .with_id(map_operator_id.clone())
        .build()
        .await;
    
    // Attach VirtualActorFacet (Dirigo: virtual actors for stream operators)
    let virtual_facet_config = serde_json::json!({
        "idle_timeout": "5m",
        "activation_strategy": "lazy"
    });
    let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone()));
    map_actor
        .attach_facet(virtual_facet, 100, virtual_facet_config)
        .await?;
    
    // Attach DurabilityFacet (time-sharing compute resources with durability)
    let storage = MemoryJournalStorage::new();
    let durability_config = DurabilityConfig::default();
    let durability_facet = Box::new(DurabilityFacet::new(storage, durability_config));
    map_actor
        .attach_facet(durability_facet, 50, serde_json::json!({}))
        .await?;
    
    let map_ref = node.spawn_actor(map_actor).await?;
    let map_mailbox = node.actor_registry()
        .lookup_mailbox(map_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    let map_operator_actor = plexspaces_actor::ActorRef::local(
        map_ref.id().clone(),
        map_mailbox,
        node.service_locator().clone(),
    );

    // Operator 2: Filter (filter by threshold)
    let filter_operator_id = "stream-operator/filter-1@comparison-node-1".to_string();
    let filter_operator = StreamOperator {
        operator_id: filter_operator_id.clone(),
        operator_type: "filter".to_string(),
        config: {
            let mut config = HashMap::new();
            config.insert("threshold".to_string(), serde_json::json!(50.0));
            config
        },
    };
    
    let behavior = Box::new(StreamProcessingActor::new(filter_operator.clone()));
    let mut filter_actor = ActorBuilder::new(behavior)
        .with_id(filter_operator_id.clone())
        .build()
        .await;
    
    let virtual_facet = Box::new(VirtualActorFacet::new(serde_json::json!({
        "idle_timeout": "5m",
        "activation_strategy": "lazy"
    })));
    filter_actor.attach_facet(virtual_facet, 100, serde_json::json!({})).await?;
    
    let storage = MemoryJournalStorage::new();
    let durability_facet = Box::new(DurabilityFacet::new(storage, DurabilityConfig::default()));
    filter_actor.attach_facet(durability_facet, 50, serde_json::json!({})).await?;
    
    let filter_ref = node.spawn_actor(filter_actor).await?;
    let filter_mailbox = node.actor_registry()
        .lookup_mailbox(filter_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    let filter_operator_actor = plexspaces_actor::ActorRef::local(
        filter_ref.id().clone(),
        filter_mailbox,
        node.service_locator().clone(),
    );

    // Operator 3: Reduce (aggregate values)
    let reduce_operator_id = "stream-operator/reduce-1@comparison-node-1".to_string();
    let reduce_operator = StreamOperator {
        operator_id: reduce_operator_id.clone(),
        operator_type: "reduce".to_string(),
        config: {
            let mut config = HashMap::new();
            config.insert("window_size".to_string(), serde_json::json!(5));
            config
        },
    };
    
    let behavior = Box::new(StreamProcessingActor::new(reduce_operator.clone()));
    let mut reduce_actor = ActorBuilder::new(behavior)
        .with_id(reduce_operator_id.clone())
        .build()
        .await;
    
    let virtual_facet = Box::new(VirtualActorFacet::new(serde_json::json!({
        "idle_timeout": "5m",
        "activation_strategy": "lazy"
    })));
    reduce_actor.attach_facet(virtual_facet, 100, serde_json::json!({})).await?;
    
    let storage = MemoryJournalStorage::new();
    let durability_facet = Box::new(DurabilityFacet::new(storage, DurabilityConfig::default()));
    reduce_actor.attach_facet(durability_facet, 50, serde_json::json!({})).await?;
    
    let reduce_ref = node.spawn_actor(reduce_actor).await?;
    let reduce_mailbox = node.actor_registry()
        .lookup_mailbox(reduce_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    let reduce_operator_actor = plexspaces_actor::ActorRef::local(
        reduce_ref.id().clone(),
        reduce_mailbox,
        node.service_locator().clone(),
    );

    info!("✅ Stream processing operators created:");
    info!("   - Map operator: {} (transform sensor data)", map_operator_id);
    info!("   - Filter operator: {} (filter by threshold)", filter_operator_id);
    info!("   - Reduce operator: {} (aggregate values)", reduce_operator_id);
    info!("✅ VirtualActorFacet: Virtual actors for stream operators");
    info!("✅ DurabilityFacet: Time-sharing compute resources with durability");
    info!("✅ High resource efficiency and performance isolation");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test stream processing pipeline (Dirigo: virtual actors for stream operators)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test: Real-time Event Stream Processing Pipeline");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Generate sensor events
    let sensor_events = vec![
        SensorEvent {
            event_id: "event-1".to_string(),
            sensor_id: "sensor-001".to_string(),
            sensor_type: "temperature".to_string(),
            value: 75.5,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            location: "room-1".to_string(),
        },
        SensorEvent {
            event_id: "event-2".to_string(),
            sensor_id: "sensor-002".to_string(),
            sensor_type: "pressure".to_string(),
            value: 45.2,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            location: "room-2".to_string(),
        },
        SensorEvent {
            event_id: "event-3".to_string(),
            sensor_id: "sensor-003".to_string(),
            sensor_type: "temperature".to_string(),
            value: 85.0, // Above threshold
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            location: "room-3".to_string(),
        },
    ];
    
    // Step 1: Map operator (transform events)
    info!("Step 1: Map operator (transform sensor data)");
    for event in &sensor_events {
        let msg = Message::new(serde_json::to_vec(&DirigoMessage::ProcessEvent {
            operator_id: map_operator_id.clone(),
            event: event.clone(),
        })?)
            .with_message_type("call".to_string());
        let result = map_operator_actor
            .ask(msg, Duration::from_secs(5))
            .await?;
        let reply: DirigoMessage = serde_json::from_slice(result.payload())?;
        if let DirigoMessage::StreamResult { result, .. } = reply {
            if !result.is_empty() {
                info!("   ✅ Mapped: {}", String::from_utf8_lossy(&result));
            }
        }
    }
    
    // Step 2: Filter operator (filter by threshold)
    info!("Step 2: Filter operator (filter events above threshold)");
    for event in &sensor_events {
        let msg = Message::new(serde_json::to_vec(&DirigoMessage::ProcessEvent {
            operator_id: filter_operator_id.clone(),
            event: event.clone(),
        })?)
            .with_message_type("call".to_string());
        let result = filter_operator_actor
            .ask(msg, Duration::from_secs(5))
            .await?;
        let reply: DirigoMessage = serde_json::from_slice(result.payload())?;
        if let DirigoMessage::StreamResult { result, .. } = reply {
            if !result.is_empty() {
                let filtered_event: SensorEvent = serde_json::from_slice(&result)?;
                info!("   ✅ Filtered: {} = {:.2} (above threshold)", filtered_event.sensor_type, filtered_event.value);
            }
        }
    }
    
    // Step 3: Reduce operator (aggregate values in window)
    info!("Step 3: Reduce operator (aggregate values in window)");
    let msg = Message::new(serde_json::to_vec(&DirigoMessage::ProcessEventBatch {
        operator_id: reduce_operator_id.clone(),
        events: sensor_events.clone(),
    })?)
        .with_message_type("call".to_string());
    let result = reduce_operator_actor
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: DirigoMessage = serde_json::from_slice(result.payload())?;
    if let DirigoMessage::AggregatedResult { result } = reply {
        info!("   ✅ Aggregated: type={}, value={:.2}, count={}", 
            result.aggregation_type, result.value, result.count);
    }

    // Get stats from operators
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Operator Statistics:");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    for (name, operator) in &[
        ("Map", &map_operator_actor),
        ("Filter", &filter_operator_actor),
        ("Reduce", &reduce_operator_actor),
    ] {
        let msg = Message::new(serde_json::to_vec(&DirigoMessage::GetStats)?)
            .with_message_type("call".to_string());
        let result = operator
            .ask(msg, Duration::from_secs(5))
            .await?;
        let reply: DirigoMessage = serde_json::from_slice(result.payload())?;
        if let DirigoMessage::Stats { processed_count, operator_type } = reply {
            info!("   {} operator: processed={}, type={}", name, processed_count, operator_type);
        }
    }

    info!("=== Comparison Complete ===");
    info!("✅ GenServerBehavior: Stream processing operators (Dirigo pattern)");
    info!("✅ VirtualActorFacet: Virtual actors for stream operators (automatic activation/deactivation)");
    info!("✅ DurabilityFacet: Time-sharing compute resources with durability");
    info!("✅ Stream pipeline: Map → Filter → Reduce (real-time event processing)");
    info!("✅ High resource efficiency and performance isolation");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stream_processing() {
        let node = NodeBuilder::new("test-node")
            .build();

        let operator_id = "stream-operator/test-1@test-node".to_string();
        let operator = StreamOperator {
            operator_id: operator_id.clone(),
            operator_type: "map".to_string(),
            config: HashMap::new(),
        };
        let behavior = Box::new(StreamProcessingActor::new(operator));
        let mut actor = ActorBuilder::new(behavior)
            .with_id(operator_id.clone())
            .build()
            .await;
        
        let virtual_facet = Box::new(VirtualActorFacet::new(serde_json::json!({
            "idle_timeout": "5m"
        })));
        actor.attach_facet(virtual_facet, 100, serde_json::json!({})).await.unwrap();
        
        let storage = MemoryJournalStorage::new();
        let durability_facet = Box::new(DurabilityFacet::new(storage, DurabilityConfig::default()));
        actor.attach_facet(durability_facet, 50, serde_json::json!({})).await.unwrap();
        
        let actor_ref = node.spawn_actor(actor).await.unwrap();

        let mailbox = node.actor_registry()
            .lookup_mailbox(actor_ref.id())
            .await
            .unwrap()
            .unwrap();
        
        let stream_operator = plexspaces_actor::ActorRef::local(
            actor_ref.id().clone(),
            mailbox,
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let event = SensorEvent {
            event_id: "test-event".to_string(),
            sensor_id: "test-sensor".to_string(),
            sensor_type: "temperature".to_string(),
            value: 75.0,
            timestamp: 0,
            location: "test-location".to_string(),
        };
        let msg = Message::new(serde_json::to_vec(&DirigoMessage::ProcessEvent {
            operator_id: operator_id.clone(),
            event,
        }).unwrap())
            .with_message_type("call".to_string());
        let result = stream_operator
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        let reply: DirigoMessage = serde_json::from_slice(result.payload()).unwrap();
        if let DirigoMessage::StreamResult { operator_id: op_id, result } = reply {
            assert_eq!(op_id, operator_id);
            assert!(!result.is_empty());
        } else {
            panic!("Expected StreamResult message");
        }
    }
}
