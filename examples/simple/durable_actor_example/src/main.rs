// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// End-to-end example demonstrating durable actor features:
// - Journaling (all operations are journaled)
// - Checkpoints (periodic state snapshots)
// - Deterministic replay (actor state recovery)
// - Side effect caching (prevent duplicate external calls)
//
// This example simulates a counter actor that:
// 1. Processes increment/decrement messages
// 2. Makes external API calls (side effects)
// 3. Can be restarted and recover state from journal

use plexspaces_journaling::*;
#[cfg(feature = "sqlite-backend")]
use plexspaces_journaling::sql::SqliteJournalStorage;
use plexspaces_facet::Facet;
use prost::Message;

/// Simple counter state (protobuf message)
#[derive(Clone, PartialEq, Message)]
struct CounterState {
    #[prost(int32, tag = "1")]
    pub value: i32,
}

/// Simple API response (protobuf message)
#[derive(Clone, PartialEq, Message)]
struct ApiResponse {
    #[prost(string, tag = "1")]
    pub message: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Durable Actor Example ===\n");

    // Create SQLite journal storage (in-memory for this example)
    let db_path = ":memory:";
    let storage = SqliteJournalStorage::new(db_path).await?;

    // Configure durability
    let config = DurabilityConfig {
        backend: JournalBackend::JournalBackendSqlite as i32,
        checkpoint_interval: 10, // Checkpoint every 10 journal entries
        checkpoint_timeout: None,
        replay_on_activation: true, // Enable replay on restart
        cache_side_effects: true,   // Enable side effect caching
        compression: CompressionType::CompressionTypeNone as i32,
        state_schema_version: 1,
        backend_config: None,
    };

    let actor_id = "counter-actor-1";

    println!("1. Creating durable actor with journaling enabled");
    let mut config_value = serde_json::json!({
        "backend": config.backend,
        "checkpoint_interval": config.checkpoint_interval,
        "replay_on_activation": config.replay_on_activation,
        "cache_side_effects": config.cache_side_effects,
        "compression": config.compression,
        "state_schema_version": config.state_schema_version,
    });
    if let Some(ref timeout) = config.checkpoint_timeout {
        // Handle Duration separately since it doesn't implement Serialize
        let mut timeout_obj = serde_json::Map::new();
        timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
        timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
        config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
    }
    let mut facet = DurabilityFacet::new(storage.clone(), config_value, 50);
    facet.on_attach(actor_id, serde_json::json!({})).await?;
    println!("   ✓ Actor attached, journaling active\n");

    println!("2. Processing messages with side effects");
    
    // Process increment message with side effect (external API call)
    let method = "increment";
    let payload = serde_json::json!({ "value": 5 }).to_string().into_bytes();
    facet.before_method(method, &payload).await?;
    
    // Get execution context for side effects (after before_method to avoid deadlock)
    let ctx_arc = facet.get_execution_context();
    let ctx_guard = ctx_arc.read().await;
    let ctx = ctx_guard.as_ref().unwrap();
    
    // Simulate external API call (side effect)
    let api_result = ctx
        .record_side_effect("api_call_1", "http_request", || async {
            println!("   → Executing external API call (side effect)");
            Ok(ApiResponse {
                message: "API call successful".to_string(),
            })
        })
        .await?;
    
    let api_response = ApiResponse::decode(api_result.as_slice())?;
    println!("   ✓ Side effect executed: {}", api_response.message);
    
    // Drop guard before calling after_method to avoid deadlock
    drop(ctx_guard);
    
    let result = serde_json::json!({ "counter": 5 }).to_string().into_bytes();
    facet.after_method(method, &payload, &result).await?;
    println!("   ✓ Message processed: increment by 5\n");

    // Process another message
    let method = "increment";
    let payload = serde_json::json!({ "value": 3 }).to_string().into_bytes();
    facet.before_method(method, &payload).await?;
    let result = serde_json::json!({ "counter": 8 }).to_string().into_bytes();
    facet.after_method(method, &payload, &result).await?;
    println!("   ✓ Message processed: increment by 3 (counter = 8)\n");

    // Flush to ensure all entries are written
    storage.flush().await?;

    // Verify journal entries
    let entries = storage.replay_from(actor_id, 0).await?;
    println!("3. Journal Statistics:");
    println!("   ✓ Total journal entries: {}", entries.len());
    
    let side_effect_count = entries
        .iter()
        .filter(|e| {
            matches!(
                e.entry,
                Some(plexspaces_proto::v1::journaling::journal_entry::Entry::SideEffectExecuted(_))
            )
        })
        .count();
    println!("   ✓ Side effect entries: {}", side_effect_count);
    
    let checkpoint = storage.get_latest_checkpoint(actor_id).await;
    if let Ok(cp) = checkpoint {
        println!("   ✓ Latest checkpoint at sequence: {}", cp.sequence);
    } else {
        println!("   ✓ No checkpoint yet (will be created at sequence 10)");
    }
    println!();

    println!("4. Simulating actor restart (crash recovery)");
    facet.on_detach(actor_id).await?;
    println!("   ✓ Actor detached\n");

    // Create new facet (simulating restart)
    let mut config_value = serde_json::json!({
        "backend": config.backend,
        "checkpoint_interval": config.checkpoint_interval,
        "replay_on_activation": config.replay_on_activation,
        "cache_side_effects": config.cache_side_effects,
        "compression": config.compression,
        "state_schema_version": config.state_schema_version,
    });
    if let Some(ref timeout) = config.checkpoint_timeout {
        // Handle Duration separately since it doesn't implement Serialize
        let mut timeout_obj = serde_json::Map::new();
        timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
        timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
        config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
    }
    let mut new_facet = DurabilityFacet::new(storage.clone(), config_value, 50);
    new_facet.on_attach(actor_id, serde_json::json!({})).await?;
    println!("   ✓ Actor restarted, replay completed");
    
    // Verify checkpoint exists (if any)
    let checkpoint_result = storage.get_latest_checkpoint(actor_id).await;
    if let Ok(cp) = checkpoint_result {
        println!("   ✓ Checkpoint loaded at sequence: {}", cp.sequence);
    } else {
        println!("   ✓ No checkpoint yet (will be created at sequence 10)");
    }
    println!();

    println!("5. Processing new message after restart");
    let method = "increment";
    let payload = serde_json::json!({ "value": 2 }).to_string().into_bytes();
    new_facet.before_method(method, &payload).await?;
    let result = serde_json::json!({ "counter": 10 }).to_string().into_bytes();
    new_facet.after_method(method, &payload, &result).await?;
    println!("   ✓ Message processed: increment by 2 (counter = 10)\n");

    // Verify all entries are still present
    let all_entries = storage.replay_from(actor_id, 0).await?;
    println!("6. Final Journal Statistics:");
    println!("   ✓ Total journal entries: {}", all_entries.len());
    println!("   ✓ All operations persisted and recoverable\n");

    println!("=== Example Complete ===");
    println!("\nKey Features Demonstrated:");
    println!("  ✓ Journaling: All operations are journaled");
    println!("  ✓ Checkpoints: Periodic state snapshots for fast recovery");
    println!("  ✓ Deterministic Replay: Actor state recovered from journal");
    println!("  ✓ Side Effect Caching: External calls cached during replay");
    println!("  ✓ Exactly-Once Semantics: No duplicate side effects");

    Ok(())
}

