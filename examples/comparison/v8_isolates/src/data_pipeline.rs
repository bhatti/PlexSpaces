// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Data Pipeline - Data Plane
// Demonstrates pipeline processing with actors, channels, and workflows

use crate::messages::*;
use crate::metrics::{MetricsCollector, PerformanceMetrics};
use plexspaces_actor::ActorRef;
use plexspaces_behavior::GenServer;
// Channels would be used for backpressure and durability in production
// use plexspaces_channel::{Channel, ChannelConfig, ChannelBackend, InMemoryChannel};
use plexspaces_core::{Actor, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_journaling::{DurabilityFacet, MemoryJournalStorage};
use std::sync::Arc;
use plexspaces_core::JournalStorage;
use plexspaces_mailbox::Message;
use plexspaces_node::Node;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Ingestion Actor
/// Receives logs/metrics from external sources and sends to pipeline
pub struct IngestionActor {
    // pipeline_channel: Option<Arc<dyn Channel>>, // Would use channel in production
    backpressure_threshold: u64,
}

impl IngestionActor {
    pub fn new() -> Self {
        Self {
            // pipeline_channel: None,
            backpressure_threshold: 1000,
        }
    }
}

#[async_trait::async_trait]
impl Actor for IngestionActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let pipeline_msg: PipelineMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::Ingest { events } => {
                // In production, send to pipeline channel with backpressure check
                // For demo, we'll just log
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("IngestionActor received {} events", events.len());
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

/// Pipeline Processor Actor
/// Processes events through filter, enrich, transform stages
pub struct PipelineProcessorActor {
    config: Option<PipelineConfig>,
    pending_events: Vec<PipelineEvent>,
}

impl PipelineProcessorActor {
    pub fn new() -> Self {
        Self {
            config: None,
            pending_events: Vec::new(),
        }
    }
}

#[async_trait::async_trait]
impl Actor for PipelineProcessorActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for PipelineProcessorActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let pipeline_msg: PipelineMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::Process { events } => {
                // Check backpressure
                if let Some(config) = &self.config {
                    if self.pending_events.len() as u64 >= config.backpressure_threshold {
                        return Err(BehaviorError::ProcessingError(
                            "Backpressure: too many pending events".to_string()
                        ));
                    }
                }

                // Process through pipeline stages
                let processed = self.process_pipeline(events).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Pipeline processing failed: {}", e)))?;

                // Store pending events if durability enabled
                if let Some(config) = &self.config {
                    if config.durability_enabled {
                        self.pending_events.extend(processed.clone());
                    }
                }

                // Send to destinations (fire-and-forget)
                if let Some(config) = &self.config {
                    for dest in &config.destinations {
                        // In production, send to destination actors via channels
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            tracing::debug!("Sending {} events to destination: {}", 
                                processed.len(), dest.destination_type);
                        }
                    }
                }

                // Reply
                if let Some(sender_id) = &msg.sender {
                    let reply = PipelineMessage::Processed {
                        events: processed,
                        stage_name: "pipeline".to_string(),
                    };
                    let reply_msg = if let Some(corr_id) = &msg.correlation_id {
                        Message::new(serde_json::to_vec(&reply)
                            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?)
                            .with_correlation_id(corr_id.clone())
                    } else {
                        Message::new(serde_json::to_vec(&reply)
                            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?)
                    };
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
            _ => {}
        }

        Ok(())
    }
}

impl PipelineProcessorActor {
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
        }

        Ok(processed)
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

/// Destination Actor
/// Sends events to external destinations (Splunk, Datadog, Kinesis, S3, etc.)
pub struct DestinationActor {
    destination_type: String,
    config: String,
    retry_config: RetryConfig,
}

impl DestinationActor {
    pub fn new(destination_type: String, config: String, retry_config: RetryConfig) -> Self {
        Self {
            destination_type,
            config,
            retry_config,
        }
    }
}

#[async_trait::async_trait]
impl Actor for DestinationActor {
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
impl GenServer for DestinationActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let pipeline_msg: PipelineMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::SendToDestination { events, .. } => {
                // Send to destination with retry logic
                let mut retries = 0;
                let mut backoff = self.retry_config.initial_backoff_sec;

                while retries <= self.retry_config.max_retries {
                    match self.send_to_destination(&events).await {
                        Ok(_) => {
                            // Success
                            if let Some(sender_id) = &msg.sender {
                                let reply = PipelineMessage::SendToDestinationResponse {
                                    events_sent: events.len() as u64,
                                    events_failed: 0,
                                    errors: vec![],
                                };
                                let reply_msg = if let Some(corr_id) = &msg.correlation_id {
                                    Message::new(serde_json::to_vec(&reply)
                                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?)
                                        .with_correlation_id(corr_id.clone())
                                } else {
                                    Message::new(serde_json::to_vec(&reply)
                                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?)
                                };
                                ctx.send_reply(
                                    msg.correlation_id.as_deref(),
                                    sender_id,
                                    msg.receiver.clone(),
                                    reply_msg,
                                ).await
                                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                            }
                            return Ok(());
                        }
                        Err(e) => {
                            retries += 1;
                            if retries > self.retry_config.max_retries {
                                // Store in DLQ or log error
                                warn!("Failed to send to {} after {} retries: {}", 
                                    self.destination_type, retries, e);
                                
                                if let Some(sender_id) = &msg.sender {
                                    let reply = PipelineMessage::SendToDestinationResponse {
                                        events_sent: 0,
                                        events_failed: events.len() as u64,
                                        errors: vec![e.to_string()],
                                    };
                                    let reply_msg = if let Some(corr_id) = &msg.correlation_id {
                                        Message::new(serde_json::to_vec(&reply)
                                            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?)
                                            .with_correlation_id(corr_id.clone())
                                    } else {
                                        Message::new(serde_json::to_vec(&reply)
                                            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?)
                                    };
                                    ctx.send_reply(
                                        msg.correlation_id.as_deref(),
                                        sender_id,
                                        msg.receiver.clone(),
                                        reply_msg,
                                    ).await
                                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                                }
                                return Ok(());
                            }
                            // Exponential backoff
                            tokio::time::sleep(Duration::from_secs(backoff as u64)).await;
                            backoff = ((backoff as f64 * self.retry_config.backoff_multiplier) as u32)
                                .min(self.retry_config.max_backoff_sec);
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }
}

impl DestinationActor {
    async fn send_to_destination(
        &self,
        events: &[PipelineEvent],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // In production, use appropriate client libraries
        // For demo, simulate
        match self.destination_type.as_str() {
            "splunk" => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("Sending {} events to Splunk", events.len());
                }
                // Simulate HTTP request to Splunk HEC
            }
            "datadog" => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("Sending {} events to Datadog", events.len());
                }
                // Simulate HTTP request to Datadog API
            }
            "kinesis" => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("Sending {} events to Kinesis", events.len());
                }
                // Simulate AWS Kinesis put_records
            }
            "s3" => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("Sending {} events to S3", events.len());
                }
                // Simulate S3 put_object
            }
            "elasticsearch" => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("Sending {} events to Elasticsearch", events.len());
                }
                // Simulate Elasticsearch bulk API
            }
            _ => {
                return Err(format!("Unknown destination type: {}", self.destination_type).into());
            }
        }
        Ok(())
    }
}

/// Run data plane benchmark with comprehensive metrics
pub async fn run_data_plane_benchmark(node: &Node) -> Result<PerformanceMetrics, Box<dyn std::error::Error>> {
    let mut collector = MetricsCollector::new("Data Plane".to_string());
    collector.sample_memory();
    
    let _start = std::time::Instant::now();
    
    info!("=== Data Plane Benchmark ===");
    
    let service_locator = node.service_locator();
    
    // Spawn pipeline processor actor using ActorBuilder with actual behavior
    let pipeline_id: ActorId = "pipeline-processor@v8-isolates-node-1".to_string();
    let ctx = plexspaces_core::RequestContext::internal();
    
    // Create durability facet using journal storage from ServiceLocator
    // If not available, create a default in-memory storage for this example
    let storage: Arc<dyn JournalStorage> = service_locator
        .get_journal_storage()
        .await
        .unwrap_or_else(|| Arc::new(MemoryJournalStorage::new()) as Arc<dyn JournalStorage>);
    let durability_facet = Box::new(DurabilityFacet::new(
        storage,
        serde_json::json!({
            "checkpoint_interval": 1000,  // Checkpoint every 1000 messages instead of default 100
        }),
        50,
    ));
    
    // Create actor with actual PipelineProcessorActor behavior using ActorBuilder
    use plexspaces_actor::ActorBuilder;
    let behavior: Box<dyn plexspaces_core::Actor> = Box::new(PipelineProcessorActor::new());
    let coord_start = std::time::Instant::now();
    
    let pipeline_ref = ActorBuilder::new(behavior)
        .with_id(pipeline_id.clone())
        .with_namespace(ctx.namespace().to_string())
        .with_facet(durability_facet)
        .spawn(&ctx, service_locator.clone())
        .await
        .map_err(|e| format!("Failed to spawn pipeline actor: {}", e))?;
    collector.record_coordination(coord_start.elapsed().as_millis() as u64);
    collector.record_actor_created();
    
    // Use the ActorRef returned from spawn
    let pipeline = pipeline_ref;
    
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Benchmark: Process multiple batches of events
    let num_batches = 100;
    let events_per_batch = 100;
    let total_events = num_batches * events_per_batch;
    
    info!("Processing {} batches of {} events each ({} total events)...", 
        num_batches, events_per_batch, total_events);
    
    for batch in 0..num_batches {
        // Create sample events
        let events: Vec<PipelineEvent> = (0..events_per_batch)
            .map(|i| {
                PipelineEvent::Log {
                    data: LogEntry {
                        id: ulid::Ulid::new().to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        level: if i % 10 == 0 { "ERROR".to_string() } else { "INFO".to_string() },
                        message: format!("Test log message {}", i),
                        fields: serde_json::json!({}),
                        source: format!("test-app-{}", batch),
                    },
                }
            })
            .collect();
        
        let process_start = Instant::now();
        let process_msg = PipelineMessage::Process { events };
        
        let mut request = Message::new(serde_json::to_vec(&process_msg)
            .map_err(|e| format!("Failed to serialize process message: {}", e))?)
            .with_message_type("call".to_string());
        request.receiver = pipeline_id.clone();
        collector.record_message_sent();
        
        let coord_start = Instant::now();
        let response = pipeline.ask(request, Duration::from_secs(10)).await
            .map_err(|e| format!("Failed to ask pipeline: {}", e))?;
        collector.record_coordination(coord_start.elapsed().as_millis() as u64);
        collector.record_message_received();
        
        let latency_us = process_start.elapsed().as_micros() as u64;
        collector.record_event(latency_us);
        
        let _result: PipelineMessage = serde_json::from_slice(response.payload())
            .map_err(|e| format!("Failed to deserialize response: {}", e))?;
        
        // Record computation time (simulated - assume 80% of time is computation)
        collector.record_computation(latency_us as u64 / 1000 / 5); // Convert to ms, assume 20% coordination
    }
    
    collector.sample_memory();
    
    let mut metrics = collector.finalize();
    metrics.total_events = total_events as u64;
    metrics.calculate_derived();
    
    Ok(metrics)
}

/// Run data plane demo (legacy, for compatibility)
pub async fn run_data_plane_demo(node: &Node) -> Result<(), Box<dyn std::error::Error>> {
    let _metrics = run_data_plane_benchmark(node).await?;
    Ok(())
}

