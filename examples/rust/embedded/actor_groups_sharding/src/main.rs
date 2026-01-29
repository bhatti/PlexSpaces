// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Actor Groups (Sharding) Example
//
// Demonstrates data-parallel horizontal scaling via sharding:
// - Partition key → hash → shard_id → specific actor
// - Scatter-gather queries across all shards

use async_trait::async_trait;
use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType, RequestContext,
};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// =============================================================================
// Messages
// =============================================================================

#[derive(Debug, Serialize, Deserialize)]
enum ShardMessage {
    Increment { key: String },
    GetCount { key: String },
    GetTotal,
}

// =============================================================================
// Shard Actor - handles counter operations for one shard
// =============================================================================

struct ShardActor {
    shard_id: usize,
    counts: HashMap<String, u64>,
}

impl ShardActor {
    fn new(shard_id: usize) -> Self {
        Self {
            shard_id,
            counts: HashMap::new(),
        }
    }
}

#[async_trait]
impl ActorTrait for ShardActor {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        message: plexspaces_proto::common::v1::Message,
    ) -> Result<(), BehaviorError> {
        let msg: ShardMessage = serde_json::from_slice(&message.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Parse error: {}", e)))?;

        match msg {
            ShardMessage::Increment { key } => {
                let count = self.counts.entry(key.clone()).or_insert(0);
                *count += 1;
                println!("  Shard {}: {} → {}", self.shard_id, key, *count);
            }
            ShardMessage::GetCount { key } => {
                let count = self.counts.get(&key).copied().unwrap_or(0);
                println!("  Shard {}: GetCount({}) → {}", self.shard_id, key, count);
            }
            ShardMessage::GetTotal => {
                let total: u64 = self.counts.values().sum();
                println!("  Shard {}: GetTotal → {}", self.shard_id, total);
            }
        }
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║           Actor Groups (Sharding) Example                      ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Configuration
    let shard_count = 4;
    let node_id = "sharding-node";

    // Create node (services auto-initialized)
    let node = Arc::new(NodeBuilder::new(node_id).build().await);

    // Create request context with tenant/namespace (REQUIRED - explicit)
    let ctx = RequestContext::new_without_auth(
        "example-tenant".to_string(),
        "sharding-demo".to_string(),
    );

    // =========================================================================
    // Step 1: Create shard actors using ActorBuilder
    // =========================================================================
    println!("Step 1: Creating {} shard actors...", shard_count);
    
    let mut shards: Vec<ActorRef> = Vec::new();
    let service_locator = node.service_locator();
    
    for i in 0..shard_count {
        let actor_id = format!("shard-{}@{}", i, node_id);
        
        // Create shard actor with custom behavior using ActorBuilder
        let shard = ActorBuilder::new(Box::new(ShardActor::new(i)))
            .with_id(actor_id.clone())
            .with_namespace("sharding-demo")
            .spawn(&ctx, service_locator.clone())
            .await
            .map_err(|e| format!("Failed to spawn shard-{}: {}", i, e))?;
        
        shards.push(shard);
        println!("  ✓ Created shard-{}", i);
    }
    println!();

    // =========================================================================
    // Step 2: Send messages with partition key routing
    // =========================================================================
    println!("Step 2: Sending messages (partition key → shard)...");
    
    let keys = vec!["user-1", "user-2", "user-3", "user-4", "user-5"];
    
    for key in &keys {
        // Hash-based routing: partition key → shard_id
        let hash = key.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let shard_id = (hash % shard_count as u64) as usize;
        
        // Create message using Message::json()
        let msg = Message::json(&ShardMessage::Increment { key: key.to_string() })?
            .with_message_type("increment");
        
        // Send to correct shard (tell = fire-and-forget)
        shards[shard_id].tell(msg).await
            .map_err(|e| format!("Send failed: {}", e))?;
        
        println!("  {} → shard-{}", key, shard_id);
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
    println!();

    // =========================================================================
    // Step 3: Scatter-gather query (GetTotal from all shards)
    // =========================================================================
    println!("Step 3: Scatter-gather query (GetTotal)...");
    
    for (i, shard) in shards.iter().enumerate() {
        let msg = Message::json(&ShardMessage::GetTotal)?
            .with_message_type("get_total");
        
        shard.tell(msg).await
            .map_err(|e| format!("Send failed: {}", e))?;
        
        println!("  → Querying shard-{}", i);
    }
    
    // Wait for responses
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    println!();

    // =========================================================================
    // Done
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Example complete!");
    println!();
    println!("Key Takeaways:");
    println!("  • Actor Groups enable horizontal scaling via sharding");
    println!("  • Partition key routes to specific shard (hash → shard_id)");
    println!("  • Scatter-gather queries all shards and merges results");
    println!();

    Ok(())
}
