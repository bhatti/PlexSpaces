// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Config Service - Control Plane
// Demonstrates scalable config distribution to millions of workers

use crate::messages::*;
use crate::metrics::{MetricsCollector, PerformanceMetrics};
use plexspaces_actor::ActorRef;
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_journaling::{DurabilityFacet, MemoryJournalStorage};
use plexspaces_mailbox::Message;
use plexspaces_node::Node;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::info;

/// Config Service Actor (GenServer)
/// Manages worker configurations and notifies workers of updates
pub struct ConfigServiceActor {
    /// Latest config version
    latest_version: u64,
    
    /// Configs by version
    configs: HashMap<u64, WorkerConfig>,
    
    /// Registered workers (worker_id -> (node_id, current_version))
    workers: HashMap<String, (String, u64)>,
}

impl ConfigServiceActor {
    pub fn new() -> Self {
        Self {
            latest_version: 0,
            configs: HashMap::new(),
            workers: HashMap::new(),
        }
    }
}

#[async_trait::async_trait]
impl Actor for ConfigServiceActor {
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
impl GenServer for ConfigServiceActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let config_msg: ConfigServiceMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        let reply = match config_msg {
            ConfigServiceMessage::RegisterWorker { worker_id, node_id, current_version } => {
                info!("Registering worker: {} on node: {} (current_version: {})", 
                    worker_id, node_id, current_version);
                
                // Register worker
                self.workers.insert(worker_id.clone(), (node_id, current_version));
                
                // Check if newer config available
                let config = if self.latest_version > current_version {
                    self.configs.get(&self.latest_version).cloned()
                } else {
                    None
                };
                
                ConfigServiceMessage::RegisterWorkerResponse {
                    latest_version: self.latest_version,
                    config,
                }
            }
            
            ConfigServiceMessage::GetConfig { worker_id: _, version } => {
                let target_version = if version == 0 { self.latest_version } else { version };
                
                if let Some(config) = self.configs.get(&target_version) {
                    ConfigServiceMessage::GetConfigResponse {
                        config: config.clone(),
                    }
                } else {
                    return Err(BehaviorError::ProcessingError(
                        format!("Config version {} not found", target_version)
                    ));
                }
            }
            
            ConfigServiceMessage::NotifyConfigChange { version, config } => {
                info!("Config change notification: version {}", version);
                
                // Store new config
                self.configs.insert(version, config.clone());
                self.latest_version = version;
                
                // Notify workers (scalable: use group communication within nodes)
                // Each node has a config watcher actor that shares config with local workers
                let notified = self.notify_workers(ctx, version).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to notify workers: {}", e)))?;
                
                ConfigServiceMessage::NotifyConfigChangeResponse {
                    workers_notified: notified,
                }
            }
            
            _ => {
                return Err(BehaviorError::ProcessingError("Unexpected message".to_string()));
            }
        };

        // Send reply
        if let Some(sender_id) = &msg.sender {
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

        Ok(())
    }
}

impl ConfigServiceActor {
    /// Notify workers of config change (scalable approach)
    /// Uses node-level config watchers to avoid bottlenecks
    async fn notify_workers(
        &self,
        _ctx: &ActorContext,
        version: u64,
    ) -> Result<u64, BehaviorError> {
        // Group workers by node to avoid per-worker messaging
        let mut nodes: HashMap<String, Vec<String>> = HashMap::new();
        for (worker_id, (node_id, _)) in &self.workers {
            nodes.entry(node_id.clone())
                .or_insert_with(Vec::new)
                .push(worker_id.clone());
        }

        // Notify each node's config watcher (one message per node, not per worker)
        // Config watcher then shares config with local workers via file or shared memory
        let mut notified = 0;
        for (node_id, _workers) in &nodes {
            // In production, use ActorRef to send to node's config watcher
            // For demo, we'll just count
            info!("Notifying node {} config watcher (version {})", node_id, version);
            notified += 1;
        }

        Ok(notified)
    }
}

/// Config Watcher Actor (one per node)
/// Watches for config changes and shares with local workers
pub struct ConfigWatcherActor {
    node_id: String,
    current_version: u64,
    config: Option<WorkerConfig>,
}

impl ConfigWatcherActor {
    pub fn new(node_id: String) -> Self {
        Self {
            node_id,
            current_version: 0,
            config: None,
        }
    }
}

#[async_trait::async_trait]
impl Actor for ConfigWatcherActor {
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
impl GenServer for ConfigWatcherActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let config_msg: ConfigServiceMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        let reply = match config_msg {
            ConfigServiceMessage::NotifyConfigChange { version, config } => {
                info!("Config watcher {} received config version {}", self.node_id, version);
                self.current_version = version;
                self.config = Some(config.clone());
                
                // Share config with local workers (via file or shared memory)
                // For demo, we'll just log
                info!("Config watcher {} sharing config with local workers", self.node_id);
                
                ConfigServiceMessage::NotifyConfigChangeResponse {
                    workers_notified: 1, // In production, count local workers
                }
            }
            
            ConfigServiceMessage::GetConfig { .. } => {
                if let Some(config) = &self.config {
                    ConfigServiceMessage::GetConfigResponse {
                        config: config.clone(),
                    }
                } else {
                    return Err(BehaviorError::ProcessingError("No config available".to_string()));
                }
            }
            
            _ => {
                return Err(BehaviorError::ProcessingError("Unexpected message".to_string()));
            }
        };

        if let Some(sender_id) = &msg.sender {
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

        Ok(())
    }
}

/// Worker Actor
/// Checks for config updates and applies them
pub struct WorkerActor {
    worker_id: String,
    node_id: String,
    current_config_version: u64,
    config: Option<WorkerConfig>,
    config_service: Option<ActorRef>,
}

impl WorkerActor {
    pub fn new(worker_id: String, node_id: String) -> Self {
        Self {
            worker_id,
            node_id,
            current_config_version: 0,
            config: None,
            config_service: None,
        }
    }
}

#[async_trait::async_trait]
impl Actor for WorkerActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Handle config check messages
        let payload_str = String::from_utf8_lossy(msg.payload());
        if payload_str == "check_config" {
            self.check_config(ctx).await
                .map_err(|e| BehaviorError::ProcessingError(format!("Failed to check config: {}", e)))?;
        }
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

impl WorkerActor {
    async fn check_config(&mut self, _ctx: &ActorContext) -> Result<(), BehaviorError> {
        // Get config from local config watcher (or directly from config service)
        // For demo, we'll use config service directly
        if let Some(config_service) = &self.config_service {
            let msg = ConfigServiceMessage::GetConfig {
                worker_id: self.worker_id.clone(),
                version: 0, // Get latest
            };
            
            let mut request = Message::new(serde_json::to_vec(&msg)
                .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?)
                .with_message_type("call".to_string());
            request.receiver = config_service.id().clone();
            
            if let Ok(response) = config_service.ask(request, Duration::from_secs(5)).await {
                let config_msg: ConfigServiceMessage = serde_json::from_slice(response.payload())
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to deserialize: {}", e)))?;
                if let ConfigServiceMessage::GetConfigResponse { config } = config_msg {
                    if config.version > self.current_config_version {
                        info!("Worker {} applying config version {}", 
                            self.worker_id, config.version);
                        self.config = Some(config.clone());
                        self.current_config_version = config.version;
                        // Apply config...
                    }
                }
            }
        }
        Ok(())
    }
}

/// Run control plane benchmark with comprehensive metrics
pub async fn run_control_plane_benchmark(node: &Node) -> Result<PerformanceMetrics, Box<dyn std::error::Error>> {
    let mut collector = MetricsCollector::new("Control Plane".to_string());
    collector.sample_memory();
    
    let start = std::time::Instant::now();
    
    info!("=== Control Plane Benchmark ===");
    
    let service_locator = node.service_locator();
    
    // Spawn config service actor using ActorBuilder with actual behavior
    let config_service_id: ActorId = "config-service@v8-isolates-node-1".to_string();
    let ctx = plexspaces_core::RequestContext::internal();
    
    // Create durability facet
    let storage = MemoryJournalStorage::new();
    let durability_facet = Box::new(DurabilityFacet::new(
        storage,
        serde_json::json!({}),
        50,
    ));
    
    // Create actor with actual ConfigServiceActor behavior using ActorBuilder
    use plexspaces_actor::ActorBuilder;
    let behavior: Box<dyn plexspaces_core::Actor> = Box::new(ConfigServiceActor::new());
    let coord_start = std::time::Instant::now();
    
    let actor_ref = ActorBuilder::new(behavior)
        .with_id(config_service_id.clone())
        .with_namespace(ctx.namespace().to_string())
        .with_facet(durability_facet)
        .spawn(&ctx, service_locator.clone())
        .await
        .map_err(|e| format!("Failed to spawn config service actor: {}", e))?;
    collector.record_coordination(coord_start.elapsed().as_millis() as u64);
    collector.record_actor_created();
    
    // Use the ActorRef returned from spawn
    let config_service = actor_ref;
    
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Benchmark: Register multiple workers
    let num_workers = 100;
    info!("Registering {} workers...", num_workers);
    
    let mut _total_latency = 0u64;
    for i in 0..num_workers {
        let register_msg = ConfigServiceMessage::RegisterWorker {
            worker_id: format!("worker-{}", i),
            node_id: node.id().as_str().to_string(),
            current_version: 0,
        };
        
        let request_start = Instant::now();
        let mut request = Message::new(serde_json::to_vec(&register_msg)
            .map_err(|e| format!("Failed to serialize register message: {}", e))?)
            .with_message_type("call".to_string());
        request.receiver = config_service_id.clone();
        collector.record_message_sent();
        
        let coord_start = Instant::now();
        let response = config_service.ask(request, Duration::from_secs(5)).await
            .map_err(|e| format!("Failed to ask config service: {}", e))?;
        collector.record_coordination(coord_start.elapsed().as_millis() as u64);
        collector.record_message_received();
        
        let latency_us = request_start.elapsed().as_micros() as u64;
        _total_latency += latency_us;
        collector.record_event(latency_us);
        
        let _result: ConfigServiceMessage = serde_json::from_slice(response.payload())
            .map_err(|e| format!("Failed to deserialize response: {}", e))?;
    }
    
    // Benchmark: Config change notification
    info!("Testing config change notification...");
    let new_config = WorkerConfig {
        version: 1,
        config_json: r#"{"filter": "level == 'ERROR'", "batch_size": 100}"#.to_string(),
        metadata: std::collections::HashMap::new(),
    };
    
    let notify_start = Instant::now();
    let notify_msg = ConfigServiceMessage::NotifyConfigChange {
        version: 1,
        config: new_config,
    };
    
    let mut request = Message::new(serde_json::to_vec(&notify_msg)
        .map_err(|e| format!("Failed to serialize notify message: {}", e))?)
        .with_message_type("call".to_string());
    request.receiver = config_service_id.clone();
    collector.record_message_sent();
    
    let coord_start = Instant::now();
    let response = config_service.ask(request, Duration::from_secs(5)).await
        .map_err(|e| format!("Failed to ask config service: {}", e))?;
    collector.record_coordination(coord_start.elapsed().as_millis() as u64);
    collector.record_message_received();
    
    let latency_us = notify_start.elapsed().as_micros() as u64;
    collector.record_event(latency_us);
    
    let _result: ConfigServiceMessage = serde_json::from_slice(response.payload())
        .map_err(|e| format!("Failed to deserialize response: {}", e))?;
    
    // Record computation time (simulated)
    collector.record_computation(start.elapsed().as_millis() as u64 / 10); // Assume 10% is computation
    
    collector.sample_memory();
    
    let mut metrics = collector.finalize();
    metrics.total_events = num_workers as u64 + 1; // workers + 1 config change
    metrics.calculate_derived();
    
    Ok(metrics)
}

/// Run control plane demo (legacy, for compatibility)
pub async fn run_control_plane_demo(node: &Node) -> Result<(), Box<dyn std::error::Error>> {
    let _metrics = run_control_plane_benchmark(node).await?;
    Ok(())
}

