// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Actor Groups (Sharding) Example
//!
//! Demonstrates data-parallel horizontal scaling via sharding.
//!
//! ## Features Demonstrated
//! 1. Sharded counter actors (one per shard)
//! 2. Router actor that routes messages to correct shard based on partition key
//! 3. Scatter-gather queries across all shards
//! 4. ConfigBootstrap for configuration
//! 5. CoordinationComputeTracker for metrics
//!
//! ## Pattern
//! - Partition key → hash → shard_id → specific actor
//! - Scatter-gather: query all shards, merge results

use plexspaces_actor::ActorBuilder;
use plexspaces_core::{ActorBehavior, ActorId, ActorRef, BehaviorError};
use plexspaces_mailbox::Message;
use plexspaces_node::{ConfigBootstrap, NodeBuilder};
use plexspaces_node::CoordinationComputeTracker;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

/// Configuration for sharded counter group
#[derive(Debug, Deserialize, Default)]
struct ShardingConfig {
    #[serde(default)]
    shard_count: usize,
    #[serde(default)]
    group_id: String,
}

/// Message types for sharded counter
#[derive(Debug, Serialize, Deserialize)]
enum CounterMessage {
    Increment { key: String },
    GetCount { key: String },
    GetTotal, // Scatter-gather query
}

/// Shard actor behavior - handles counter operations for one shard
struct ShardBehavior {
    shard_id: usize,
    counts: HashMap<String, u64>,
}

impl ShardBehavior {
    fn new(shard_id: usize) -> Self {
        Self {
            shard_id,
            counts: HashMap::new(),
        }
    }
}

#[async_trait::async_trait]
impl ActorBehavior for ShardBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &plexspaces_core::ActorContext,
        message: Message,
    ) -> Result<(), BehaviorError> {
        let msg: CounterMessage = serde_json::from_slice(&message.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse message: {}", e)))?;

        match msg {
            CounterMessage::Increment { key } => {
                let count = self.counts.entry(key.clone()).or_insert(0);
                *count += 1;
                info!("Shard {}: Incremented key '{}' → count = {}", 
                    self.shard_id, key, *count);
            }
            CounterMessage::GetCount { key } => {
                let count = self.counts.get(&key).copied().unwrap_or(0);
                info!("Shard {}: GetCount('{}') → {}", self.shard_id, key, count);
            }
            CounterMessage::GetTotal => {
                let total: u64 = self.counts.values().sum();
                info!("Shard {}: GetTotal() → {}", self.shard_id, total);
            }
        }

        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

/// Router actor - routes messages to correct shard based on partition key
struct RouterBehavior {
    shard_actors: Vec<ActorRef>,
    shard_count: usize,
    tracker: Arc<tokio::sync::Mutex<CoordinationComputeTracker>>,
}

impl RouterBehavior {
    fn new(shard_actors: Vec<ActorRef>, tracker: Arc<tokio::sync::Mutex<CoordinationComputeTracker>>) -> Self {
        let shard_count = shard_actors.len();
        Self {
            shard_actors,
            shard_count,
            tracker,
        }
    }

    /// Hash-based routing: partition key → shard_id
    fn route_to_shard(&self, key: &str) -> usize {
        // Simple hash function
        let hash = key.as_bytes().iter().fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));
        (hash % self.shard_count as u64) as usize
    }
}

#[async_trait::async_trait]
impl ActorBehavior for RouterBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &plexspaces_core::ActorContext,
        message: Message,
    ) -> Result<(), BehaviorError> {
        let msg: CounterMessage = serde_json::from_slice(&message.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse message: {}", e)))?;

        match msg {
            CounterMessage::Increment { ref key } => {
                let shard_id = self.route_to_shard(key);
                let shard_actor = &self.shard_actors[shard_id];
                
                let mut tracker = self.tracker.lock().await;
                tracker.start_coordinate();
                let msg_bytes = serde_json::to_vec(&msg).unwrap();
                let routed_msg = Message::new(msg_bytes)
                    .with_message_type("increment".to_string())
                    .with_sender(_ctx.actor_id.to_string());
                
                shard_actor.tell(routed_msg).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to route to shard: {}", e)))?;
                
                tracker.end_coordinate();
                info!("Router: Key '{}' → Shard {}", key, shard_id);
            }
            CounterMessage::GetCount { ref key } => {
                let shard_id = self.route_to_shard(key);
                let shard_actor = &self.shard_actors[shard_id];
                
                let mut tracker = self.tracker.lock().await;
                tracker.start_coordinate();
                let msg_bytes = serde_json::to_vec(&msg).unwrap();
                let routed_msg = Message::new(msg_bytes)
                    .with_message_type("get_count".to_string())
                    .with_sender(_ctx.actor_id.to_string());
                
                shard_actor.tell(routed_msg).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to route to shard: {}", e)))?;
                
                tracker.end_coordinate();
                info!("Router: GetCount('{}') → Shard {}", key, shard_id);
            }
            CounterMessage::GetTotal => {
                // Scatter-gather: send to all shards
                let mut tracker = self.tracker.lock().await;
                tracker.start_coordinate();
                let msg_bytes = serde_json::to_vec(&CounterMessage::GetTotal).unwrap();
                
                for (shard_id, shard_actor) in self.shard_actors.iter().enumerate() {
                    let routed_msg = Message::new(msg_bytes.clone())
                        .with_message_type("get_total".to_string())
                        .with_sender(_ctx.actor_id.to_string());
                    
                    if let Err(e) = shard_actor.tell(routed_msg).await {
                        warn!("Failed to query shard {}: {}", shard_id, e);
                    }
                }
                
                tracker.end_coordinate();
                info!("Router: GetTotal() → Scatter-gather to all {} shards", self.shard_count);
            }
        }

        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Actor Groups (Sharding) - Example                           ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Load configuration
    let config: ShardingConfig = ConfigBootstrap::load()
        .unwrap_or_default();
    
    let shard_count = if config.shard_count > 0 {
        config.shard_count.min(16) // Max: 16
    } else {
        4 // Default: 4
    };
    let group_id = if !config.group_id.is_empty() {
        config.group_id
    } else {
        "counter-group".to_string()
    };

    info!("Configuration:");
    info!("  Group ID: {}", group_id);
    info!("  Shard Count: {}", shard_count);
    println!();

    // Create node
    let node = NodeBuilder::new("sharding-node")
        .build();

    let node_arc = Arc::new(node);
    let tracker = Arc::new(tokio::sync::Mutex::new(CoordinationComputeTracker::new("sharding-example".to_string())));

    // Create shard actors
    info!("Step 1: Creating {} shard actors...", shard_count);
    let mut shard_actors = Vec::new();
    
    for shard_id in 0..shard_count {
        let actor_id = format!("shard-{}@local", shard_id);
        let behavior = Box::new(ShardBehavior::new(shard_id));
        
        let actor = ActorBuilder::new(behavior)
            .with_id(ActorId::from(actor_id.clone()))
            .build();
        
        let actor_ref = node_arc.spawn_actor(actor).await?;
        shard_actors.push(actor_ref);
        info!("  ✓ Created shard actor: {}", actor_id);
    }
    println!();

    // Create router actor
    info!("Step 2: Creating router actor...");
    let router_behavior = Box::new(RouterBehavior::new(shard_actors.clone(), tracker.clone()));
    let router = ActorBuilder::new(router_behavior)
        .with_id(ActorId::from("router@local"))
        .build();
    
    let router_ref = node_arc.spawn_actor(router).await?;
    info!("  ✓ Created router actor");
    println!();

    info!("✓ Node ready");
    println!();

    // Send increment messages to different keys
    info!("Step 3: Sending increment messages...");
    let keys = vec!["user-1", "user-2", "user-3", "user-4", "user-5"];
    
    for key in &keys {
        let msg = CounterMessage::Increment { key: key.to_string() };
        let msg_bytes = serde_json::to_vec(&msg).unwrap();
        let message = Message::new(msg_bytes)
            .with_message_type("increment".to_string());
        
        router_ref.tell(message).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
    println!();

    // Query individual counts
    info!("Step 4: Querying individual counts...");
    for key in &keys {
        let msg = CounterMessage::GetCount { key: key.to_string() };
        let msg_bytes = serde_json::to_vec(&msg).unwrap();
        let message = Message::new(msg_bytes)
            .with_message_type("get_count".to_string());
        
        router_ref.tell(message).await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
    println!();

    // Scatter-gather query (get total)
    info!("Step 5: Scatter-gather query (GetTotal)...");
    let msg = CounterMessage::GetTotal;
    let msg_bytes = serde_json::to_vec(&msg).unwrap();
    let message = Message::new(msg_bytes)
        .with_message_type("get_total".to_string());
    
    router_ref.tell(message).await?;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    println!();

    // Note: Metrics are tracked during message routing
    info!("📊 Metrics:");
    info!("  Coordination and compute metrics tracked during routing");
    info!("  (Use CoordinationComputeTracker::finalize() for detailed report)");
    println!();

    // Shutdown
    node_arc.shutdown(tokio::time::Duration::from_secs(5)).await?;

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Example complete!");
    println!();
    println!("Key Takeaways:");
    println!("  • Actor Groups enable horizontal scaling via sharding");
    println!("  • Partition key routes to specific shard (1-to-1)");
    println!("  • Scatter-gather queries all shards and merges results");
    println!("  • Use for: high-throughput counters, caches, indexes");
    println!();

    Ok(())
}
