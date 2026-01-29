// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Durable Actor Example
//
// Demonstrates actor durability with journaling:
// - All operations are journaled (persisted)
// - Checkpoints for fast recovery
// - Deterministic replay on restart
// - Side effect caching (prevent duplicate external calls)

use plexspaces_journaling::{
    DurabilityConfig, DurabilityFacet, JournalBackend, CompressionType,
    JournalEntry, JournalStorage, MemoryJournalStorage,
};
use plexspaces_facet::Facet;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// =============================================================================
// Messages
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
enum CounterMessage {
    Increment(i32),
    Decrement(i32),
    GetValue,
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
    println!("║           Durable Actor Example                                ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // =========================================================================
    // Step 1: Create journal storage (in-memory for demo, use SQLite for prod)
    // =========================================================================
    println!("Step 1: Setting up journal storage");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let storage: Arc<dyn JournalStorage> = Arc::new(MemoryJournalStorage::new());
    println!("  Storage: In-memory (use SQLite for production)");
    println!();

    // =========================================================================
    // Step 2: Create durability configuration
    // =========================================================================
    println!("Step 2: Configuring durability");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let config = DurabilityConfig {
        backend: JournalBackend::JournalBackendMemory as i32,
        checkpoint_interval: 5,  // Checkpoint every 5 operations
        checkpoint_timeout: None,
        replay_on_activation: true,  // Replay journal on restart
        cache_side_effects: true,    // Cache external call results
        compression: CompressionType::CompressionTypeNone as i32,
        state_schema_version: 1,
        backend_config: None,
    };
    
    println!("  Checkpoint interval: {} operations", config.checkpoint_interval);
    println!("  Replay on activation: {}", config.replay_on_activation);
    println!("  Cache side effects: {}", config.cache_side_effects);
    println!();

    // =========================================================================
    // Step 3: Create durable actor with DurabilityFacet
    // =========================================================================
    println!("Step 3: Creating durable actor");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let actor_id = "counter-actor@durable-node";
    let config_json = serde_json::json!({
        "backend": config.backend,
        "checkpoint_interval": config.checkpoint_interval,
        "replay_on_activation": config.replay_on_activation,
        "cache_side_effects": config.cache_side_effects,
        "compression": config.compression,
        "state_schema_version": config.state_schema_version,
    });
    
    let mut facet = DurabilityFacet::new(storage.clone(), config_json.clone(), 50);
    facet.on_attach(actor_id, serde_json::json!({})).await?;
    println!("  Actor ID: {}", actor_id);
    println!("  Durability facet attached");
    println!();

    // =========================================================================
    // Step 4: Process messages (all operations journaled)
    // =========================================================================
    println!("Step 4: Processing messages (journaled)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let mut counter = 0i32;
    
    // Process several operations
    let operations = vec![
        CounterMessage::Increment(10),
        CounterMessage::Increment(5),
        CounterMessage::Decrement(3),
        CounterMessage::Increment(8),
        CounterMessage::GetValue,
    ];
    
    for (i, op) in operations.iter().enumerate() {
        let method = match op {
            CounterMessage::Increment(_) => "increment",
            CounterMessage::Decrement(_) => "decrement",
            CounterMessage::GetValue => "get_value",
        };
        let payload = serde_json::to_vec(op)?;
        
        // Before method: journal the operation
        facet.before_method(method, &payload).await?;
        
        // Execute the operation
        match op {
            CounterMessage::Increment(v) => {
                counter += v;
                println!("  [{}/{}] Increment({}) → counter = {}", i + 1, operations.len(), v, counter);
            }
            CounterMessage::Decrement(v) => {
                counter -= v;
                println!("  [{}/{}] Decrement({}) → counter = {}", i + 1, operations.len(), v, counter);
            }
            CounterMessage::GetValue => {
                println!("  [{}/{}] GetValue → {}", i + 1, operations.len(), counter);
            }
        }
        
        // After method: journal the result
        let result = serde_json::to_vec(&counter)?;
        facet.after_method(method, &payload, &result).await?;
    }
    println!();

    // =========================================================================
    // Step 5: Check journal statistics
    // =========================================================================
    println!("Step 5: Journal statistics");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    storage.flush().await?;
    let entries: Vec<JournalEntry> = storage.replay_from(actor_id, 0).await?;
    println!("  Journal entries: {}", entries.len());
    
    if let Ok(checkpoint) = storage.get_latest_checkpoint(actor_id).await {
        println!("  Latest checkpoint: sequence {}", checkpoint.sequence);
    } else {
        println!("  No checkpoint yet");
    }
    println!();

    // =========================================================================
    // Step 6: Simulate crash and restart (replay from journal)
    // =========================================================================
    println!("Step 6: Simulating crash and restart");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Detach (simulate crash)
    facet.on_detach(actor_id).await?;
    println!("  Actor crashed (detached)");
    
    // Create new facet (simulate restart)
    let mut new_facet = DurabilityFacet::new(storage.clone(), config_json, 50);
    new_facet.on_attach(actor_id, serde_json::json!({})).await?;
    println!("  Actor restarted (reattached)");
    println!("  Journal replayed automatically");
    println!();

    // =========================================================================
    // Step 7: Continue processing after restart
    // =========================================================================
    println!("Step 7: Processing after restart");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Counter state would be recovered from journal in real implementation
    // For this demo, we continue from where we left off
    let op = CounterMessage::Increment(2);
    let payload = serde_json::to_vec(&op)?;
    new_facet.before_method("increment", &payload).await?;
    counter += 2;
    println!("  Increment(2) → counter = {}", counter);
    let result = serde_json::to_vec(&counter)?;
    new_facet.after_method("increment", &payload, &result).await?;
    println!();

    // Final journal count
    storage.flush().await?;
    let final_entries: Vec<JournalEntry> = storage.replay_from(actor_id, 0).await?;
    println!("  Total journal entries: {}", final_entries.len());
    println!();

    // =========================================================================
    // Done
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Durable Actor Example Complete");
    println!();
    println!("Key Concepts:");
    println!("  - Journaling: All operations persisted before execution");
    println!("  - Checkpoints: Periodic snapshots for fast recovery");
    println!("  - Replay: State recovered from journal on restart");
    println!("  - Side effects: External calls cached (exactly-once)");
    println!();

    Ok(())
}
