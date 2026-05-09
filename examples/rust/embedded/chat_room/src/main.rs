// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Chat Room Example (Process Groups / Pub/Sub)
//
// Demonstrates process groups for broadcast messaging:
// - Users join/leave chat rooms
// - Messages broadcast to all room members via publish_to_group
// - list_groups for room discovery
// - Real-time group coordination
// - CoordinationComputeTracker metrics for pub/sub overhead analysis
//
// Use Case: Chat application, live notifications, collaborative editing
//
// ## Architecture
//
// This example demonstrates ProcessGroupRegistry for distributed pub/sub:
// 1. **Process Groups**: Named groups of actors for broadcast messaging
// 2. **Pub/Sub**: Broadcast messages to all group members
// 3. **Dynamic Membership**: Join/leave at runtime
// 4. **Multi-tenancy**: Groups scoped to tenant/namespace via RequestContext
// 5. **Metrics**: CoordinationComputeTracker tracks coordination overhead vs message processing
//
// ## Design Principles
//
// - **Core Functionality**: ProcessGroupRegistry lives in main crates
// - **No Hacks**: Proper trait usage, no cyclic dependencies
// - **Observability**: CoordinationComputeTracker for metrics
// - **Tenant Isolation**: Explicit RequestContext with tenant/namespace (no internal())

use plexspaces_keyvalue::SqliteKVStore;
use plexspaces_process_groups::ProcessGroupRegistry;
use plexspaces_actor::{ActorId, RequestContext, RequestContextExt};
use plexspaces_node::CoordinationComputeTracker;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

// =============================================================================
// Chat Message (serialized for publish_to_group)
// =============================================================================

#[derive(Debug, Clone, Serialize)]
struct ChatMessage {
    from: String,
    text: String,
}

impl ChatMessage {
    fn new(from: &str, text: &str) -> Self {
        Self {
            from: from.to_string(),
            text: text.to_string(),
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("chat_room=info,plexspaces=warn"))
        )
        .try_init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║           Chat Room Example (Process Groups)                   ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    info!("Multi-tenancy: RequestContext with tenant/namespace (no internal())");
    info!("API: ProcessGroupRegistry for distributed pub/sub");
    info!("Pattern: Broadcast messaging with dynamic membership");
    println!();

    // Configuration: Use non-trivial scenario (multiple rooms, many messages)
    let num_rooms = 3;
    let users_per_room = 8;
    let messages_per_room = 20;
    
    info!("Configuration:");
    info!("  Rooms: {}", num_rooms);
    info!("  Users per room: {}", users_per_room);
    info!("  Messages per room: {}", messages_per_room);
    info!("  Total messages: {}", num_rooms * messages_per_room);
    println!();

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("chat-room".to_string());
    let total_start = Instant::now();

    // Create backend (SQLite :memory: for demo, use Redis/PostgreSQL for production)
    let kv_store = Arc::new(SqliteKVStore::new(":memory:").await?);
    let registry = ProcessGroupRegistry::new("chat-server", kv_store);

    let tenant_id = "acme-corp";
    let namespace = "chat";
    
    // Create RequestContext for operations that require it
    // Design: Explicit tenant/namespace for multi-tenancy (no internal())
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // =========================================================================
    // Step 1: Create chat rooms (coordination phase)
    // =========================================================================
    info!("Step 1: Create {} chat rooms", num_rooms);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    metrics_tracker.start_coordinate();
    let create_start = Instant::now();
    
    let room_names: Vec<String> = (0..num_rooms)
        .map(|i| format!("room-{}", i))
        .collect();
    
    for room_name in &room_names {
        let room = registry.create_group(&ctx, room_name).await?;
        if room_names.len() <= 3 || room_name == &room_names[0] || room_name == &room_names[room_names.len() - 1] {
            info!("  Room created: #{}", room.group_name);
        }
    }
    
    let create_time = create_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Created {} rooms in {:.2}ms", num_rooms, create_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 2: Users join rooms (coordination phase)
    // =========================================================================
    info!("Step 2: Users join rooms ({} users per room)", users_per_room);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    metrics_tracker.start_coordinate();
    let join_start = Instant::now();
    
    let mut all_users: Vec<ActorId> = Vec::new();
    for room_idx in 0..num_rooms {
        let room_name = &room_names[room_idx];
        for user_idx in 0..users_per_room {
            let user_id = ActorId::new(
                format!("user-{}-{}", room_idx, user_idx),
                "chat_user",
                "default",
                "chat-server",
            )?;
            registry.join_group(&ctx, room_name, &user_id, vec![]).await?;
            
            if (room_idx == 0 && user_idx < 3) || (room_idx == num_rooms - 1 && user_idx == users_per_room - 1) {
                info!("  {} joined #{}", user_id, room_name);
            }
            
            all_users.push(user_id);
        }
    }
    
    let join_time = join_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  {} users joined rooms in {:.2}ms", all_users.len(), join_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 3: Send messages (coordination + computation phase)
    // =========================================================================
    info!("Step 3: Send {} messages per room", messages_per_room);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut total_recipients = 0u64;
    let mut total_messages = 0u64;
    
    for room_idx in 0..num_rooms {
        let room_name = &room_names[room_idx];
        let members = registry.get_members(&ctx, room_name).await?;
        let num_members = members.len();
        
        metrics_tracker.start_coordinate();
        let message_start = Instant::now();
        
        for msg_idx in 0..messages_per_room {
            // Simulate different users sending messages
            let sender_idx = msg_idx % num_members;
            let sender = &members[sender_idx];
            
            let msg = ChatMessage::new(
                &sender.to_string(),
                &format!("Message {} from {} in {}", msg_idx + 1, sender, room_name)
            );
            let payload = msg.to_bytes()?;
            
            // Broadcast to all room members (coordination overhead)
            let recipients = registry
                .publish_to_group(&ctx, room_name, None, payload)
                .await?;
            
            total_recipients += recipients.len() as u64;
            total_messages += 1;
            metrics_tracker.increment_message();
            
            // Simulate message processing time (computation)
            metrics_tracker.end_coordinate();
            metrics_tracker.start_compute();
            tokio::time::sleep(Duration::from_micros(100)).await; // Simulate processing
            metrics_tracker.end_compute();
            metrics_tracker.start_coordinate();
            
            if (room_idx == 0 && msg_idx < 2) || (room_idx == num_rooms - 1 && msg_idx == messages_per_room - 1) {
                info!("  [{}]: {} -> {} recipients", sender, msg.text, recipients.len());
            }
        }
        
        let message_time = message_start.elapsed();
        metrics_tracker.end_coordinate();
        
        if room_idx == 0 || room_idx == num_rooms - 1 {
            info!("  Room #{}: {} messages sent in {:.2}ms", room_name, messages_per_room, message_time.as_secs_f64() * 1000.0);
        }
    }
    
    println!();
    info!("  Total messages sent: {}", total_messages);
    info!("  Total recipients: {} (avg {:.1} per message)", total_recipients, total_recipients as f64 / total_messages as f64);
    println!();

    // =========================================================================
    // Step 4: Cleanup (coordination phase)
    // =========================================================================
    info!("Step 4: Cleanup rooms");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    metrics_tracker.start_coordinate();
    let cleanup_start = Instant::now();
    
    for room_name in &room_names {
        registry.delete_group(&ctx, room_name).await?;
    }
    
    let cleanup_time = cleanup_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Deleted {} rooms in {:.2}ms", num_rooms, cleanup_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 5: Performance Metrics
    // =========================================================================
    info!("Step 5: Performance Metrics");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    let total_time = total_start.elapsed();
    let metrics = metrics_tracker.finalize();

    let coordinate_time = Duration::from_millis(metrics.coordinate_duration_ms);
    let compute_time = Duration::from_millis(metrics.compute_duration_ms);
    
    // Calculate benchmark metrics
    let messages_per_sec = total_messages as f64 / total_time.as_secs_f64();
    let recipients_per_sec = total_recipients as f64 / total_time.as_secs_f64();
    let avg_message_size_bytes = 150.0; // Estimated average message size
    let throughput_mbps = (total_messages as f64 * avg_message_size_bytes) / (total_time.as_secs_f64() * 1024.0 * 1024.0);

    info!("Execution Summary:");
    info!("  Total execution time: {:.2}ms ({:.2}s)", 
          total_time.as_secs_f64() * 1000.0,
          total_time.as_secs_f64());
    info!("  Rooms: {}", num_rooms);
    info!("  Users per room: {}", users_per_room);
    info!("  Messages per room: {}", messages_per_room);
    info!("  Total messages: {}", total_messages);
    info!("  Total recipients: {}", total_recipients);
    println!();

    info!("Coordination vs Computation Breakdown:");
    info!("  Coordination time: {:.2}ms ({:.1}%)", 
          coordinate_time.as_secs_f64() * 1000.0,
          (coordinate_time.as_secs_f64() / total_time.as_secs_f64()) * 100.0);
    info!("  Computation time: {:.2}ms ({:.1}%)",
          compute_time.as_secs_f64() * 1000.0,
          (compute_time.as_secs_f64() / total_time.as_secs_f64()) * 100.0);
    info!("  Efficiency (compute/total): {:.1}%", metrics.efficiency * 100.0);
    println!();

    info!("Message & Broadcast Metrics:");
    info!("  Total messages sent: {}", metrics.message_count);
    if metrics.message_count > 0 {
        let avg_latency_ms = coordinate_time.as_secs_f64() * 1000.0 / metrics.message_count as f64;
        info!("  Average latency per message: {:.2}ms", avg_latency_ms);
        info!("  Message throughput: {:.1} msg/s", messages_per_sec);
        info!("  Recipient throughput: {:.1} recipients/s", recipients_per_sec);
    }
    println!();

    info!("Benchmark Metrics:");
    info!("  Throughput: {:.2} MB/s", throughput_mbps);
    info!("  Messages per second: {:.1}", messages_per_sec);
    info!("  Recipients per second: {:.1}", recipients_per_sec);
    println!();

    info!("Granularity Analysis:");
    if coordinate_time.as_secs_f64() > 0.0 {
        info!("  Granularity ratio (compute/coordinate): {:.2}", metrics.granularity_ratio);
        if metrics.granularity_ratio >= 100.0 {
            info!("  ✅ Excellent granularity (coordination overhead is negligible)");
        } else if metrics.granularity_ratio >= 10.0 {
            info!("  ✅ Good granularity (coordination overhead is low)");
        } else if metrics.granularity_ratio >= 1.0 {
            info!("  ⚠️  Moderate granularity (coordination overhead is noticeable)");
        } else {
            info!("  ❌ Poor granularity (coordination overhead dominates)");
        }
    } else {
        info!("  Granularity ratio: N/A (no coordination overhead)");
    }
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Chat Room Example Complete");
    println!();
    info!("Key Concepts:");
    info!("  - Process Groups: Named groups of actors");
    info!("  - Pub/Sub: Broadcast to all group members");
    info!("  - Dynamic membership: Join/leave at runtime");
    info!("  - Multi-tenancy: Groups scoped to tenant/namespace");
    println!();
    info!("PlexSpaces Integration:");
    info!("  - ProcessGroupRegistry: create_group, delete_group");
    info!("  - join_group / leave_group / get_members");
    info!("  - publish_to_group (broadcast) / list_groups");
    info!("  - RequestContext: Explicit tenant/namespace (no internal())");
    println!();
    info!("Use Cases:");
    info!("  - Chat rooms / channels (Slack, Discord-style)");
    info!("  - Live notifications (push updates to subscribers)");
    info!("  - Collaborative editing (real-time document sync)");
    info!("  - Config update broadcasts (notify all services)");
    info!("  - Game lobbies (player coordination)");
    println!();

    Ok(())
}
