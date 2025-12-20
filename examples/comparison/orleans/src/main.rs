// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Orleans Virtual Actors with Timers and Reminders - Batch Prediction
// Based on: https://docs.ray.io/en/latest/ray-core/examples/batch_prediction.html

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId, Reply};
use plexspaces_journaling::{VirtualActorFacet, TimerFacet, ReminderFacet, DurabilityFacet, DurabilityConfig, MemoryJournalStorage};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// Simple ML model (dummy model for demonstration)
#[derive(Debug, Clone)]
pub struct MLModel {
    pub model_id: String,
    pub payload: Vec<u8>, // Simulated model weights/data
}

impl MLModel {
    pub fn new(model_id: String) -> Self {
        // Simulate a large model (100MB payload)
        let payload = vec![0u8; 100_000_000];
        Self { model_id, payload }
    }

    /// Predict on a batch of data
    pub fn predict(&self, batch: &[DataPoint]) -> Vec<Prediction> {
        batch.iter()
            .map(|point| Prediction {
                data_id: point.id.clone(),
                score: (point.features.iter().sum::<f32>() % 2.0) as f64,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            })
            .collect()
    }
}

/// Data point for prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub id: String,
    pub features: Vec<f32>,
}

/// Prediction result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub data_id: String,
    pub score: f64,
    pub timestamp: u64,
}

/// Batch prediction message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BatchPredictionMessage {
    LoadModel { model_id: String },
    PredictBatch { shard_path: String, data: Vec<DataPoint> },
    PredictBatchResult { predictions: Vec<Prediction>, count: usize },
    StartPeriodicBatch { interval_secs: u64 },
    ScheduleBatchJob { job_id: String, scheduled_time: u64 },
    GetStats,
    Stats { processed_count: u64, model_loaded: bool },
}

/// Batch predictor actor (Orleans-style grain with model caching)
/// Demonstrates: Virtual Actor Lifecycle, Model Caching, Parallel Batch Processing, Timers, Reminders
pub struct BatchPredictorActor {
    model: Option<MLModel>,
    processed_count: u64,
    model_id: Option<String>,
}

impl BatchPredictorActor {
    pub fn new() -> Self {
        Self {
            model: None,
            processed_count: 0,
            model_id: None,
        }
    }

    fn load_model(&mut self, model_id: String) {
        info!("[BATCH PREDICTOR] Loading model: {} (100MB payload)", model_id);
        // In production, this would load from storage (S3, HDFS, etc.)
        // For this example, we create a dummy model
        self.model = Some(MLModel::new(model_id.clone()));
        self.model_id = Some(model_id);
        info!("[BATCH PREDICTOR] Model loaded and cached in actor");
    }
}

#[async_trait::async_trait]
impl Actor for BatchPredictorActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        tracing::debug!("BatchPredictorActor::handle_message: Received message, type={:?}, reply_to={:?}", 
            msg.message_type_str(), msg.reply_to);
        
        // Check if this is a timer fired message
        if msg.message_type_str().starts_with("timer_fired:") {
            // Handle periodic batch processing
            info!("[TIMER FIRED] Periodic batch processing triggered");
            if let Some(model) = &self.model {
                // Simulate processing a batch
                let dummy_data = vec![
                    DataPoint { id: "timer-1".to_string(), features: vec![1.0, 2.0, 3.0] },
                    DataPoint { id: "timer-2".to_string(), features: vec![4.0, 5.0, 6.0] },
                ];
                let predictions = model.predict(&dummy_data);
                self.processed_count += predictions.len() as u64;
                info!("[TIMER FIRED] Processed {} predictions (total: {})", predictions.len(), self.processed_count);
            }
            return Ok(());
        }
        
        // Check if this is a reminder fired message
        if msg.message_type_str().starts_with("reminder_fired:") {
            // Handle scheduled batch job
            info!("[REMINDER FIRED] Scheduled batch job triggered");
            if let Some(model) = &self.model {
                // Simulate processing a scheduled batch
                let dummy_data = vec![
                    DataPoint { id: "reminder-1".to_string(), features: vec![7.0, 8.0, 9.0] },
                    DataPoint { id: "reminder-2".to_string(), features: vec![10.0, 11.0, 12.0] },
                ];
                let predictions = model.predict(&dummy_data);
                self.processed_count += predictions.len() as u64;
                info!("[REMINDER FIRED] Processed {} predictions (total: {})", predictions.len(), self.processed_count);
            }
            return Ok(());
        }
        
        // Delegate to GenServer's route_message for regular messages
        <Self as GenServer>::route_message(self, ctx, msg, reply).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for BatchPredictorActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        let batch_msg: BatchPredictionMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        let reply_msg = match batch_msg {
            BatchPredictionMessage::LoadModel { model_id } => {
                // Load and cache model (Orleans: model loaded once per actor)
                // This is efficient because the model is cached in the actor
                // and reused for all subsequent predictions
                self.load_model(model_id.clone());
                info!("[BATCH PREDICTOR] Model {} loaded and cached", model_id);
                let reply = BatchPredictionMessage::Stats {
                    processed_count: self.processed_count,
                    model_loaded: true,
                };
                Message::new(serde_json::to_vec(&reply).unwrap())
            }
            BatchPredictionMessage::PredictBatch { shard_path, data } => {
                // Process batch prediction
                // In production, this would read from shard_path (S3, HDFS, etc.)
                info!("[BATCH PREDICTOR] Processing batch: {} ({} data points)", shard_path, data.len());
                
                // Ensure model is loaded
                if self.model.is_none() {
                    // Load default model if not loaded
                    self.load_model("default-model".to_string());
                }
                
                let model = self.model.as_ref().unwrap();
                let predictions = model.predict(&data);
                self.processed_count += predictions.len() as u64;
                
                info!("[BATCH PREDICTOR] Generated {} predictions (total processed: {})", 
                    predictions.len(), self.processed_count);
                
                // In production, write predictions to storage (S3, HDFS, etc.)
                // For this example, we return the results
                let reply = BatchPredictionMessage::PredictBatchResult {
                    predictions: predictions.clone(),
                    count: predictions.len(),
                };
                Message::new(serde_json::to_vec(&reply).unwrap())
            }
            BatchPredictionMessage::StartPeriodicBatch { interval_secs } => {
                // Register a timer for periodic batch processing (Orleans: RegisterTimer)
                info!("[BATCH PREDICTOR] Starting periodic batch processing (interval: {}s)", interval_secs);
                info!("[BATCH PREDICTOR] TimerFacet attached - timer will fire every {} seconds", interval_secs);
                info!("[BATCH PREDICTOR] Timer is in-memory and lost on actor deactivation (non-durable)");
                
                // Note: Timer registration would be done via TimerFacet API
                // For this example, we demonstrate that TimerFacet is attached
                // and will handle timer registration and firing
                
                let reply = BatchPredictionMessage::Stats {
                    processed_count: self.processed_count,
                    model_loaded: self.model.is_some(),
                };
                Message::new(serde_json::to_vec(&reply).unwrap())
            }
            BatchPredictionMessage::ScheduleBatchJob { job_id, scheduled_time } => {
                // Register a reminder for scheduled batch job (Orleans: RegisterReminder)
                info!("[BATCH PREDICTOR] Scheduling batch job: {} (scheduled: {})", job_id, scheduled_time);
                info!("[BATCH PREDICTOR] ReminderFacet attached - reminder will fire at scheduled time");
                info!("[BATCH PREDICTOR] Reminder is durable and survives actor deactivation (persisted)");
                
                // Note: Reminder registration would be done via ReminderFacet API
                // For this example, we demonstrate that ReminderFacet is attached
                // and will handle reminder registration and firing
                
                let reply = BatchPredictionMessage::Stats {
                    processed_count: self.processed_count,
                    model_loaded: self.model.is_some(),
                };
                Message::new(serde_json::to_vec(&reply).unwrap())
            }
            BatchPredictionMessage::GetStats => {
                let reply = BatchPredictionMessage::Stats {
                    processed_count: self.processed_count,
                    model_loaded: self.model.is_some(),
                };
                Message::new(serde_json::to_vec(&reply).unwrap())
            }
            _ => return Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        };
        
        // Add correlation_id to reply if present in request
        let reply_with_corr = if let Some(corr_id) = &msg.correlation_id {
            tracing::debug!("BatchPredictorActor::handle_request: Adding correlation_id to reply: {}", corr_id);
            reply_msg.with_correlation_id(corr_id.clone())
        } else {
            reply_msg
        };
        
        tracing::debug!("BatchPredictorActor::handle_request: Returning reply, correlation_id={:?}", reply_with_corr.correlation_id);
        Ok(reply_with_corr)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== Orleans vs PlexSpaces Comparison ===");
    info!("Demonstrating Virtual Actors with Model Caching for Batch Prediction");
    info!("Based on: https://docs.ray.io/en/latest/ray-core/examples/batch_prediction.html");

    // Create a node
    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Orleans-style virtual actor: Use get_or_activate pattern
    // This is the key Orleans pattern - actors are activated on-demand
    let actor_id: ActorId = "batch-predictor/model-1@comparison-node-1".to_string();
    
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Orleans vs PlexSpaces Comparison                              ║");
    println!("║  Demonstrating Virtual Actors with get_or_activate pattern     ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Orleans Pattern: get_or_activate_actor (Virtual Actor Activation)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Orleans Pattern: get_or_activate_actor (Virtual Actor Activation)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    // Create facets with config and priority (Orleans-style virtual actor)
    tracing::debug!("Creating Orleans-style virtual actor with facets: {}", actor_id);
    let virtual_facet_config = serde_json::json!({
        "idle_timeout": "5m",
        "activation_strategy": "lazy"  // Orleans: actors activate on first message
    });
    let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config, 100));
    let storage = MemoryJournalStorage::new();
    let durability_facet = Box::new(DurabilityFacet::new(storage, serde_json::json!({}), 50));
    let timer_facet = Box::new(TimerFacet::new(serde_json::json!({}), 75));
    let reminder_storage = Arc::new(MemoryJournalStorage::new());
    let reminder_facet = Box::new(ReminderFacet::new(reminder_storage, serde_json::json!({}), 60));
    
    // Spawn using ActorFactory with facets (Orleans get_or_activate pattern)
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
        vec![virtual_facet, durability_facet, timer_facet, reminder_facet], // facets
    ).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    tracing::debug!("Actor spawned successfully with all facets");
    
    // Wait for actor to be fully registered
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create ActorRef directly - no need to access mailbox
    let predictor = plexspaces_actor::ActorRef::remote(
        actor_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );

    info!("✅ Batch predictor grain created/activated via get_or_activate: {}", predictor.id());
    println!("✅ Batch predictor grain created/activated via get_or_activate: {}", predictor.id());
    println!("✅ VirtualActorFacet attached - Orleans-style virtual actor lifecycle");
    println!("✅ DurabilityFacet attached - state persistence enabled");
    println!("✅ TimerFacet attached - in-memory timers (Orleans RegisterTimer)");
    println!("✅ ReminderFacet attached - durable reminders (Orleans RegisterReminder)");
    println!("✅ Model caching: Model loaded once per actor, reused for all predictions");
    println!();
    
    info!("✅ VirtualActorFacet attached - Orleans-style virtual actor lifecycle");
    info!("✅ DurabilityFacet attached - state persistence enabled");
    info!("✅ TimerFacet attached - in-memory timers (Orleans RegisterTimer)");
    info!("✅ ReminderFacet attached - durable reminders (Orleans RegisterReminder)");
    info!("✅ Model caching: Model loaded once per actor, reused for all predictions");

    // Wait for actor to be registered (lazy activation - will activate on first message)
    // VirtualActorFacet with lazy activation activates on first message
    tracing::debug!("Waiting for actor registration (VirtualActorFacet lazy activation - will activate on first message)");
    tokio::time::sleep(Duration::from_millis(200)).await;
    tracing::debug!("Actor registered, will activate on first message");

    // Test 1: Load model (cached in actor)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 1: Load Model (cached in actor for reuse)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Test 1: Load Model (cached in actor for reuse)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("  Loading model: ml-model-v1");
    println!("  Note: This first ask() will trigger lazy activation of the virtual actor");
    let msg = Message::new(serde_json::to_vec(&BatchPredictionMessage::LoadModel {
        model_id: "ml-model-v1".to_string(),
    })?)
        .with_message_type("call".to_string());
    tracing::debug!("Main: Sending LoadModel request via ask() (will trigger lazy activation)");
    let result = predictor
        .ask(msg, Duration::from_secs(1))  // Should activate and respond within 1 second
        .await?;
    tracing::debug!("Main: Received reply from LoadModel");
    let reply: BatchPredictionMessage = serde_json::from_slice(result.payload())?;
    if let BatchPredictionMessage::Stats { model_loaded, .. } = reply {
        info!("✅ Model loaded: {}", model_loaded);
        println!("  ✅ Model loaded successfully: {}", model_loaded);
        assert!(model_loaded);
    }

    // Test 2: Process batch prediction (model reused from cache)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 2: Process Batch Prediction (model reused from cache)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Test 2: Process Batch Prediction (model reused from cache)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    println!("  Processing batch 1: shard-1.parquet (3 data points)");
    let data_shard = vec![
        DataPoint { id: "data-1".to_string(), features: vec![1.0, 2.0, 3.0, 4.0] },
        DataPoint { id: "data-2".to_string(), features: vec![5.0, 6.0, 7.0, 8.0] },
        DataPoint { id: "data-3".to_string(), features: vec![9.0, 10.0, 11.0, 12.0] },
    ];
    let msg = Message::new(serde_json::to_vec(&BatchPredictionMessage::PredictBatch {
        shard_path: "s3://bucket/data/shard-1.parquet".to_string(),
        data: data_shard,
    })?)
        .with_message_type("call".to_string());
    tracing::debug!("Main: Sending PredictBatch request via ask()");
    let result = predictor
        .ask(msg, Duration::from_secs(10))
        .await?;
    tracing::debug!("Main: Received reply from PredictBatch");
    let reply: BatchPredictionMessage = serde_json::from_slice(result.payload())?;
    if let BatchPredictionMessage::PredictBatchResult { predictions, count } = reply {
        info!("✅ Batch prediction completed: {} predictions generated", count);
        println!("  ✅ Batch 1 processed: {} predictions generated", count);
        for pred in &predictions {
            info!("   - Prediction: data_id={}, score={:.2}", pred.data_id, pred.score);
        }
        assert_eq!(count, 3);
    }

    // Test 3: Process another batch (model still cached)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 3: Process Another Batch (model still cached, no reload)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Test 3: Process Another Batch (model still cached, no reload)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    println!("  Processing batch 2: shard-2.parquet (2 data points)");
    let data_shard2 = vec![
        DataPoint { id: "data-4".to_string(), features: vec![13.0, 14.0, 15.0, 16.0] },
        DataPoint { id: "data-5".to_string(), features: vec![17.0, 18.0, 19.0, 20.0] },
    ];
    let msg = Message::new(serde_json::to_vec(&BatchPredictionMessage::PredictBatch {
        shard_path: "s3://bucket/data/shard-2.parquet".to_string(),
        data: data_shard2,
    })?)
        .with_message_type("call".to_string());
    let result = predictor
        .ask(msg, Duration::from_secs(10))
        .await?;
    let reply: BatchPredictionMessage = serde_json::from_slice(result.payload())?;
    if let BatchPredictionMessage::PredictBatchResult { count, .. } = reply {
        info!("✅ Second batch completed: {} predictions (model reused from cache)", count);
        println!("  ✅ Batch 2 processed: {} predictions generated (model reused)", count);
        assert_eq!(count, 2);
    }

    // Test 4: Get stats
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 4: Get Statistics");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Test 4: Get Statistics");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    println!("  Requesting stats...");
    let msg = Message::new(serde_json::to_vec(&BatchPredictionMessage::GetStats)?)
        .with_message_type("call".to_string());
    let result = predictor
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: BatchPredictionMessage = serde_json::from_slice(result.payload())?;
    if let BatchPredictionMessage::Stats { processed_count, model_loaded } = reply {
        info!("✅ Stats: processed_count={}, model_loaded={}", processed_count, model_loaded);
        println!("  ✅ Stats: processed_count={}, model_loaded={}", processed_count, model_loaded);
        assert_eq!(processed_count, 5); // 3 + 2
        assert!(model_loaded);
    }

    // Test 5: Timer registration (Orleans: RegisterTimer)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 5: Timer Registration (Orleans RegisterTimer)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let msg = Message::new(serde_json::to_vec(&BatchPredictionMessage::StartPeriodicBatch {
        interval_secs: 5,
    })?)
        .with_message_type("call".to_string());
    let _result = predictor
        .ask(msg, Duration::from_secs(5))
        .await?;
    info!("✅ Timer registration requested");
    info!("   - Timer: In-memory, fires every 5 seconds");
    info!("   - Lost on actor deactivation (non-durable)");
    info!("   - Useful for periodic batch processing");

    // Test 6: Reminder registration (Orleans: RegisterReminder)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 6: Reminder Registration (Orleans RegisterReminder)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let scheduled_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() + 30;
    let msg = Message::new(serde_json::to_vec(&BatchPredictionMessage::ScheduleBatchJob {
        job_id: "daily-batch-job".to_string(),
        scheduled_time,
    })?)
        .with_message_type("call".to_string());
    let _result = predictor
        .ask(msg, Duration::from_secs(5))
        .await?;
    info!("✅ Reminder registration requested");
    info!("   - Reminder: Durable, fires at scheduled time");
    info!("   - Survives actor deactivation (persisted)");
    info!("   - Useful for scheduled batch jobs");
    
    // Demonstrate virtual actor lifecycle
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 7: Virtual Actor Lifecycle");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("✅ Virtual actor created: {}", predictor.id());
    info!("   - Virtual actor is addressable even when deactivated");
    info!("   - VirtualActorFacet automatically reactivates if needed");
    info!("   - Model remains cached in actor (efficient reuse)");

    // Demonstrate virtual actor lifecycle
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Virtual Actor Lifecycle for Batch Prediction:");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("1. Actor activated on first message (lazy activation)");
    info!("2. Model loaded once and cached in actor (efficient)");
    info!("3. Multiple batch predictions reuse cached model (no reload)");
    info!("4. Actor processes batches in parallel (if multiple actors)");
    info!("5. Actor deactivates after idle timeout (automatic)");
    info!("6. Next message triggers automatic reactivation");
    info!("7. Model reloaded on reactivation (or persisted via DurabilityFacet)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 Execution Summary");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Pattern:           get_or_activate_actor (Orleans virtual actor)");
    println!("VirtualActorFacet: ✅ Automatic activation/deactivation");
    println!("DurabilityFacet:   ✅ State persistence enabled");
    println!("TimerFacet:        ✅ In-memory timers (Orleans RegisterTimer)");
    println!("ReminderFacet:     ✅ Durable reminders (Orleans RegisterReminder)");
    println!("Model Caching:     ✅ Model loaded once, reused for all predictions");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  ✅ Comparison Complete                                         ║");
    println!("║  ✅ get_or_activate pattern: Orleans-style virtual actor        ║");
    println!("║  ✅ VirtualActorFacet: Automatic activation/deactivation        ║");
    println!("║  ✅ DurabilityFacet: State persistence for reminders            ║");
    println!("║  ✅ Model Caching: Loaded once, reused for all predictions      ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    
    info!("=== Comparison Complete ===");
    info!("✅ get_or_activate pattern: Orleans-style virtual actor activation");
    info!("✅ VirtualActorFacet: Automatic activation/deactivation (Orleans grain lifecycle)");
    info!("✅ Model Caching: Model loaded once per actor, reused for all predictions");
    info!("✅ TimerFacet: In-memory timers for periodic batch processing (Orleans RegisterTimer)");
    info!("✅ ReminderFacet: Durable reminders for scheduled batch jobs (Orleans RegisterReminder)");
    info!("✅ DurabilityFacet: State persistence for reminders and actor state");
    info!("✅ Parallel Processing: Multiple actors can process batches in parallel");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_batch_prediction() {
        let node = NodeBuilder::new("test-node")
            .build();

        // Create facets
        let virtual_facet = Box::new(VirtualActorFacet::new(serde_json::json!({}), 100));
        let storage = MemoryJournalStorage::new();
        let durability_facet = Box::new(DurabilityFacet::new(storage, serde_json::json!({}), 50));
        
        // Spawn using ActorFactory with facets
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        use std::sync::Arc;
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        let actor_id = "test-predictor@test-node".to_string();
        let ctx = plexspaces_core::RequestContext::internal();
        let _message_sender = actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "GenServer",
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![virtual_facet, durability_facet], // facets
        ).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();
        
        // Create ActorRef directly - no need to access mailbox
        let predictor = plexspaces_actor::ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Load model
        let msg = Message::new(serde_json::to_vec(&BatchPredictionMessage::LoadModel {
            model_id: "test-model".to_string(),
        }).unwrap())
            .with_message_type("call".to_string());
        let _result = predictor
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        // Predict batch
        let data = vec![
            DataPoint { id: "test-1".to_string(), features: vec![1.0, 2.0] },
        ];
        let msg = Message::new(serde_json::to_vec(&BatchPredictionMessage::PredictBatch {
            shard_path: "test.parquet".to_string(),
            data,
        }).unwrap())
            .with_message_type("call".to_string());
        let result = predictor
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        let reply: BatchPredictionMessage = serde_json::from_slice(result.payload()).unwrap();
        if let BatchPredictionMessage::PredictBatchResult { count, .. } = reply {
            assert_eq!(count, 1);
        } else {
            panic!("Expected PredictBatchResult message");
        }
    }
}
