// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Scalable Pipeline Architecture
// Fixed number of pipelines, each with dedicated input/processor/output actors
// Each pipeline is self-contained and processes data independently
//
// This module demonstrates proper channel usage with ACK/NACK for backpressure
// and durability, as required for production-grade log/metric processing systems.

use crate::messages::*;
use plexspaces_behavior::GenServer;
use plexspaces_channel::{Channel, create_channel};
use plexspaces_core::{Actor, ActorContext, ActorId, BehaviorError, BehaviorType, RequestContext};
use plexspaces_journaling::{DurabilityFacet, MemoryJournalStorage};
use std::sync::Arc;
use plexspaces_core::JournalStorage;
use plexspaces_mailbox::Message;
use plexspaces_node::Node;
use plexspaces_proto::channel::v1::{ChannelBackend, ChannelConfig, ChannelMessage, DeliveryGuarantee, OrderingGuarantee};
use std::io::Write;
use tracing::{info, warn, error};
use ulid::Ulid;

/// Input Worker Actor
/// Reads data and transforms to log entries
pub struct InputWorkerActor {
    pipeline_id: String,
    events_processed: u64,
}

impl InputWorkerActor {
    pub fn new(pipeline_id: String) -> Self {
        Self {
            pipeline_id,
            events_processed: 0,
        }
    }
}

#[async_trait::async_trait]
impl Actor for InputWorkerActor {
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
impl GenServer for InputWorkerActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let pipeline_msg: PipelineMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::Ingest { events } => {
                self.events_processed += events.len() as u64;
                
                // Transform events (already done in io_benchmark, but could add more processing here)
                let processed = events;
                
                // Reply with processed events
                if let Some(sender_id) = &msg.sender {
                    let reply = PipelineMessage::Processed {
                        events: processed,
                        stage_name: format!("input-{}", self.pipeline_id),
                    };
                    let reply_msg = Message::new(serde_json::to_vec(&reply)
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?);
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
            _ => {
                return Err(BehaviorError::ProcessingError(
                    format!("InputWorkerActor received unexpected message")
                ));
            }
        }

        Ok(())
    }
}

/// Processor Worker Actor
/// Processes events through filter, enrich, transform stages
/// Uses channels with ACK/NACK for backpressure and durability
pub struct ProcessorWorkerActor {
    pipeline_id: String,
    events_processed: u64,
    config: Option<PipelineConfig>,
    // Channel for receiving events (with ACK/NACK support)
    input_channel: Option<Box<dyn Channel>>,
    // Channel for sending processed events (with ACK/NACK support)
    output_channel: Option<Box<dyn Channel>>,
}

impl ProcessorWorkerActor {
    pub fn new(pipeline_id: String) -> Self {
        Self {
            pipeline_id,
            events_processed: 0,
            config: None,
            input_channel: None,
            output_channel: None,
        }
    }
    
    /// Initialize channels for this processor (with ACK/NACK support and backpressure)
    ///
    /// ## Backpressure Architecture
    /// - **Memory Channels**: Bounded capacity (4096 messages) provides Go-like backpressure
    ///   - When channel is full, `send()` blocks until space is available
    ///   - This naturally throttles producers when consumers are slow
    /// - **Actor Mailboxes**: Also provide backpressure when full
    ///   - When mailbox is full, `tell()`/`ask()` will block or return error
    /// - **Persistent Channels** (SQLite/Kafka): Don't need capacity limits
    ///   - They have their own backpressure mechanisms (disk space, network buffers)
    ///
    /// ## Industry Best Practices
    /// - Use bounded channels for in-memory communication (prevents memory exhaustion)
    /// - Default capacity of 4K messages balances throughput and memory usage
    /// - Backpressure propagates naturally through the pipeline (producer blocks when consumer is slow)
    /// - No semaphores needed - channels and mailboxes handle backpressure automatically
    pub async fn init_channels(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Create input channel (receives events from input worker)
        // Memory channel with backpressure: capacity of 4096 messages (4K default)
        // When full, send() will block (Go-like behavior) providing natural backpressure
        // Persistent channels (SQLite/Kafka) don't need capacity limits as they have their own backpressure
        let input_config = ChannelConfig {
            name: format!("processor-input-{}", self.pipeline_id),
            backend: ChannelBackend::ChannelBackendInMemory as i32,
            capacity: 4096, // 4K messages - provides backpressure when full
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            max_retries: 3,
            dlq_enabled: true,
            dead_letter_queue: format!("processor-input-{}-dlq", self.pipeline_id),
            ..Default::default()
        };
        let input_channel = create_channel(input_config).await
            .map_err(|e| format!("Failed to create input channel: {}", e))?;
        
        // Create output channel (sends processed events to output worker)
        // Memory channel with backpressure: capacity of 4096 messages (4K default)
        let output_config = ChannelConfig {
            name: format!("processor-output-{}", self.pipeline_id),
            backend: ChannelBackend::ChannelBackendInMemory as i32,
            capacity: 4096, // 4K messages - provides backpressure when full
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            max_retries: 3,
            dlq_enabled: true,
            dead_letter_queue: format!("processor-output-{}-dlq", self.pipeline_id),
            ..Default::default()
        };
        let output_channel = create_channel(output_config).await
            .map_err(|e| format!("Failed to create output channel: {}", e))?;
        
        // Store channels (create_channel returns Box<dyn Channel>)
        self.input_channel = Some(input_channel);
        self.output_channel = Some(output_channel);
        
        info!(
            pipeline_id = %self.pipeline_id,
            "Initialized channels with ACK/NACK support for processor"
        );
        
        Ok(())
    }
    
    /// Process messages from channel with proper ACK/NACK handling
    pub async fn process_from_channel(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.input_channel.as_ref()
            .ok_or("Input channel not initialized")?;
        
        // Receive messages from channel (with at-least-once delivery)
        let messages = channel.receive(10).await
            .map_err(|e| format!("Failed to receive from channel: {}", e))?;
        
        for msg in messages {
            let message_id = msg.id.clone();
            
            // Deserialize events from channel message
            let events: Vec<PipelineEvent> = match serde_json::from_slice(&msg.payload) {
                Ok(events) => events,
                Err(e) => {
                    // Invalid message - NACK without requeue (send to DLQ)
                    warn!(
                        pipeline_id = %self.pipeline_id,
                        message_id = %message_id,
                        error = %e,
                        "Invalid message format, sending to DLQ"
                    );
                    if let Err(nack_err) = channel.nack(&message_id, false).await {
                        error!(
                            pipeline_id = %self.pipeline_id,
                            message_id = %message_id,
                            error = %nack_err,
                            "Failed to NACK invalid message"
                        );
                    }
                    continue;
                }
            };
            
            // Process events through pipeline
            match self.process_pipeline(events).await {
                Ok(processed_events) => {
                    // Success - ACK the message
                    if let Err(ack_err) = channel.ack(&message_id).await {
                        error!(
                            pipeline_id = %self.pipeline_id,
                            message_id = %message_id,
                            error = %ack_err,
                            "Failed to ACK processed message"
                        );
                    } else {
                        self.events_processed += processed_events.len() as u64;
                        
                        // Send processed events to output channel
                        if let Some(output_ch) = &self.output_channel {
                            let output_payload = serde_json::to_vec(&processed_events)
                                .map_err(|e| format!("Failed to serialize processed events: {}", e))?;
                            
                            let output_msg = ChannelMessage {
                                id: Ulid::new().to_string(),
                                channel: output_ch.get_config().name.clone(),
                                payload: output_payload,
                                sender_id: format!("processor-{}", self.pipeline_id),
                                correlation_id: msg.correlation_id.clone(),
                                ..Default::default()
                            };
                            
                            if let Err(send_err) = output_ch.send(output_msg).await {
                                error!(
                                    pipeline_id = %self.pipeline_id,
                                    error = %send_err,
                                    "Failed to send processed events to output channel"
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    // Processing failed - NACK with requeue (retry)
                    warn!(
                        pipeline_id = %self.pipeline_id,
                        message_id = %message_id,
                        error = %e,
                        "Processing failed, requeuing message"
                    );
                    if let Err(nack_err) = channel.nack(&message_id, true).await {
                        error!(
                            pipeline_id = %self.pipeline_id,
                            message_id = %message_id,
                            error = %nack_err,
                            "Failed to NACK failed message"
                        );
                    }
                }
            }
        }
        
        Ok(())
    }
    
    async fn process_pipeline(
        &self,
        events: Vec<PipelineEvent>,
    ) -> Result<Vec<PipelineEvent>, BehaviorError> {
        let mut processed = events;

        if let Some(config) = &self.config {
            // Filter stage
            if !config.filter_function.is_empty() {
                processed = self.apply_filter(processed, &config.filter_function)?;
            }

            // Enrichment stage
            if !config.enrichment_function.is_empty() {
                processed = self.apply_enrichment(processed, &config.enrichment_function)?;
            }

            // Transformation stage
            if !config.transform_function.is_empty() {
                processed = self.apply_transform(processed, &config.transform_function)?;
            }
        } else {
            // Default processing: simple pass-through with minor transformations
            processed = self.apply_default_processing(processed)?;
        }

        Ok(processed)
    }
    
    fn apply_default_processing(
        &self,
        events: Vec<PipelineEvent>,
    ) -> Result<Vec<PipelineEvent>, BehaviorError> {
        // Simple processing: filter out DEBUG logs, add processing metadata
        Ok(events.into_iter().filter_map(|event| {
            match &event {
                PipelineEvent::Log { data } => {
                    if data.level == "DEBUG" {
                        None // Filter out DEBUG logs
                    } else {
                        Some(event) // Keep other logs
                    }
                }
                PipelineEvent::Metric { .. } => Some(event), // Keep all metrics
            }
        }).collect())
    }

    fn apply_filter(
        &self,
        events: Vec<PipelineEvent>,
        _filter_fn: &str,
    ) -> Result<Vec<PipelineEvent>, BehaviorError> {
        // In production, use JavaScript sandbox (e.g., v8, deno_core)
        // For demo, simple filter
        Ok(events.into_iter().filter(|_| true).collect())
    }

    fn apply_enrichment(
        &self,
        events: Vec<PipelineEvent>,
        _enrich_fn: &str,
    ) -> Result<Vec<PipelineEvent>, BehaviorError> {
        // In production, use JavaScript sandbox
        // For demo, return as-is
        Ok(events)
    }

    fn apply_transform(
        &self,
        events: Vec<PipelineEvent>,
        _transform_fn: &str,
    ) -> Result<Vec<PipelineEvent>, BehaviorError> {
        // In production, use JavaScript sandbox
        // For demo, return as-is
        Ok(events)
    }
}

#[async_trait::async_trait]
impl Actor for ProcessorWorkerActor {
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
impl GenServer for ProcessorWorkerActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let pipeline_msg: PipelineMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::Process { events } => {
                self.events_processed += events.len() as u64;
                
                // Process through pipeline stages
                let processed = self.process_pipeline(events).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Pipeline processing failed: {}", e)))?;

                // Reply
                if let Some(sender_id) = &msg.sender {
                    let reply = PipelineMessage::Processed {
                        events: processed,
                        stage_name: format!("processor-{}", self.pipeline_id),
                    };
                    let reply_msg = Message::new(serde_json::to_vec(&reply)
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?);
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
            _ => {
                return Err(BehaviorError::ProcessingError(
                    format!("ProcessorWorkerActor received unexpected message")
                ));
            }
        }

        Ok(())
    }
}

/// Output Worker Actor
/// Writes processed events to destinations (/dev/null for benchmark)
pub struct OutputWorkerActor {
    pipeline_id: String,
    events_sent: u64,
    bytes_written: u64,
}

impl OutputWorkerActor {
    pub fn new(pipeline_id: String) -> Self {
        Self {
            pipeline_id,
            events_sent: 0,
            bytes_written: 0,
        }
    }
    
    fn write_events_to_dev_null(&mut self, events: &[PipelineEvent]) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        let mut dev_null = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")?;
        
        let mut bytes_written = 0;
        for event in events {
            let json = serde_json::to_string(event)?;
            let bytes = json.as_bytes();
            dev_null.write_all(bytes)?;
            dev_null.write_all(b"\n")?;
            bytes_written += bytes.len() as u64 + 1;
        }
        
        Ok(bytes_written)
    }
}

#[async_trait::async_trait]
impl Actor for OutputWorkerActor {
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
impl GenServer for OutputWorkerActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let pipeline_msg: PipelineMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::SendToDestination { events, .. } => {
                self.events_sent += events.len() as u64;
                
                // Write to /dev/null (simulating destination)
                let bytes = self.write_events_to_dev_null(&events)
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to write: {}", e)))?;
                self.bytes_written += bytes;
                
                // Reply
                if let Some(sender_id) = &msg.sender {
                    let reply = PipelineMessage::SendToDestinationResponse {
                        events_sent: events.len() as u64,
                        events_failed: 0,
                        errors: Vec::new(),
                    };
                    let reply_msg = Message::new(serde_json::to_vec(&reply)
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?);
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
            _ => {
                return Err(BehaviorError::ProcessingError(
                    format!("OutputWorkerActor received unexpected message")
                ));
            }
        }

        Ok(())
    }
}

/// Pipeline Definition
/// Each pipeline has dedicated input, processor, and output actors
pub struct Pipeline {
    pub pipeline_id: String,
    pub input_actor_ref: plexspaces_actor::ActorRef,
    pub processor_actor_ref: plexspaces_actor::ActorRef,
    pub output_actor_ref: plexspaces_actor::ActorRef,
}

/// Create fixed number of pipelines, each with dedicated actors
pub async fn create_pipelines(
    node: &Node,
    num_pipelines: usize,
) -> Result<Vec<Pipeline>, Box<dyn std::error::Error>> {
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string()).with_internal(true).with_admin(true);
    let mut pipelines = Vec::new();
    
    use plexspaces_actor::ActorBuilder;
    
    info!("Creating {} pipelines, each with dedicated input/processor/output actors", num_pipelines);
    
    for i in 0..num_pipelines {
        let pipeline_id = format!("pipeline-{}", i);
        
        // Create input worker
        let input_actor_id: ActorId = format!("input-{}@v8-isolates-node-1", pipeline_id);
        let input_behavior: Box<dyn plexspaces_core::Actor> = Box::new(InputWorkerActor::new(pipeline_id.clone()));
        
        let input_ref = ActorBuilder::new(input_behavior)
            .with_id(input_actor_id.clone())
            .with_namespace(ctx.namespace().to_string())
            .spawn(&ctx, service_locator.clone())
            .await
            .map_err(|e| format!("Failed to spawn input actor for pipeline {}: {}", i, e))?;
        
        // Create processor worker with durability
        // Use journal storage from ServiceLocator (shared across all pipelines)
        // If not available, create a default in-memory storage for this example
        let storage: Arc<dyn JournalStorage> = service_locator
            .get_journal_storage()
            .await
            .unwrap_or_else(|| Arc::new(MemoryJournalStorage::new()) as Arc<dyn JournalStorage>);
        let processor_actor_id: ActorId = format!("processor-{}@v8-isolates-node-1", pipeline_id);
        let processor_behavior: Box<dyn plexspaces_core::Actor> = Box::new(ProcessorWorkerActor::new(pipeline_id.clone()));
        
        let durability_facet = Box::new(DurabilityFacet::new(
            storage,
            serde_json::json!({
                "checkpoint_interval": 1000,  // Checkpoint every 1000 messages instead of default 100
            }),
            50,
        ));
        
        let processor_ref = ActorBuilder::new(processor_behavior)
            .with_id(processor_actor_id.clone())
            .with_namespace(ctx.namespace().to_string())
            .with_facet(durability_facet)
            .spawn(&ctx, service_locator.clone())
            .await
            .map_err(|e| format!("Failed to spawn processor actor for pipeline {}: {}", i, e))?;
        
        // Create output worker
        let output_actor_id: ActorId = format!("output-{}@v8-isolates-node-1", pipeline_id);
        let output_behavior: Box<dyn plexspaces_core::Actor> = Box::new(OutputWorkerActor::new(pipeline_id.clone()));
        
        let output_ref = ActorBuilder::new(output_behavior)
            .with_id(output_actor_id.clone())
            .with_namespace(ctx.namespace().to_string())
            .spawn(&ctx, service_locator.clone())
            .await
            .map_err(|e| format!("Failed to spawn output actor for pipeline {}: {}", i, e))?;
        
        pipelines.push(Pipeline {
            pipeline_id: pipeline_id.clone(),
            input_actor_ref: input_ref,
            processor_actor_ref: processor_ref,
            output_actor_ref: output_ref,
        });
    }
    
    info!("Created {} pipelines with {} total actors", num_pipelines, num_pipelines * 3);
    
    Ok(pipelines)
}
