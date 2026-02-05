// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Chat Room Example (Process Groups / Pub/Sub)
//
// Demonstrates process groups for broadcast messaging:
// - Users join/leave chat rooms
// - Messages broadcast to all room members via publish_to_group
// - list_groups for room discovery
// - Real-time group coordination
//
// Use Case: Chat application, live notifications, collaborative editing

use plexspaces_keyvalue::SqliteKVStore;
use plexspaces_process_groups::ProcessGroupRegistry;
use plexspaces_core::{ActorId, RequestContext};
use serde::Serialize;
use std::sync::Arc;

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
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║           Chat Room Example (Process Groups)                   ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Use Case: Real-time chat with broadcast messaging");
    println!();

    // Create backend (SQLite :memory: for demo, use Redis/PostgreSQL for production)
    let kv_store = Arc::new(SqliteKVStore::new(":memory:").await?);
    let registry = ProcessGroupRegistry::new("chat-server", kv_store);

    let tenant_id = "acme-corp";
    let namespace = "chat";
    let room_name = "general";
    
    // Create RequestContext for operations that require it
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // =========================================================================
    // Step 0: list_groups (empty before any rooms)
    // =========================================================================
    println!("Step 0: List rooms (before create)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let groups_before = registry.list_groups(&ctx).await?;
    println!("  Rooms: {:?}", groups_before);
    assert!(groups_before.is_empty(), "Expected no rooms yet");
    println!();

    // =========================================================================
    // Step 1: Create chat room (process group)
    // =========================================================================
    println!("Step 1: Create chat room");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let room = registry.create_group(&ctx, room_name).await?;
    println!("  Room created: #{}", room.group_name);
    println!("  Tenant: {}", room.tenant_id);

    let groups_after_create = registry.list_groups(&ctx).await?;
    println!("  list_groups: {:?}", groups_after_create);
    assert_eq!(groups_after_create.len(), 1);
    assert_eq!(groups_after_create[0], room_name);
    println!();

    // =========================================================================
    // Step 2: Users join the room
    // =========================================================================
    println!("Step 2: Users join the room");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let users = vec![
        ActorId::from("alice@chat-server"),
        ActorId::from("bob@chat-server"),
        ActorId::from("charlie@chat-server"),
    ];
    
    for user in &users {
        registry.join_group(&ctx, room_name, user, vec![]).await?;
        println!("  {} joined #general", user);
    }
    
    let members = registry.get_members(&ctx, room_name).await?;
    println!();
    println!("  Room members: {}", members.len());
    println!();

    // =========================================================================
    // Step 3: Alice sends a message (publish_to_group broadcast)
    // =========================================================================
    println!("Step 3: Alice sends a message");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = ChatMessage::new("alice", "Hey everyone! How's it going?");
    let payload = msg.to_bytes()?;

    // Broadcast to all room members via publish_to_group
    let recipients = registry
        .publish_to_group(&ctx, room_name, None, payload)
        .await?;
    println!("  [alice]: {}", msg.text);
    println!("  publish_to_group -> {} recipients: {:?}", recipients.len(), recipients);
    println!();

    // =========================================================================
    // Step 4: Bob replies (publish_to_group)
    // =========================================================================
    println!("Step 4: Bob replies");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = ChatMessage::new("bob", "Great! Working on the new feature.");
    let payload = msg.to_bytes()?;
    let recipients_bob = registry
        .publish_to_group(&ctx, room_name, None, payload)
        .await?;
    println!("  [bob]: {}", msg.text);
    println!("  publish_to_group -> {} recipients", recipients_bob.len());
    println!();

    // =========================================================================
    // Step 5: Charlie leaves the room
    // =========================================================================
    println!("Step 5: Charlie leaves the room");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let charlie = ActorId::from("charlie@chat-server");
    registry.leave_group(&ctx, room_name, &charlie).await?;
    println!("  charlie left #general");
    
    let remaining = registry.get_members(&ctx, room_name).await?;
    println!("  Remaining: {} members", remaining.len());
    println!();

    // =========================================================================
    // Step 6: Alice sends another message (publish_to_group; Charlie left)
    // =========================================================================
    println!("Step 6: Alice sends another message");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = ChatMessage::new("alice", "Charlie left, it's just us now!");
    let payload = msg.to_bytes()?;
    let recipients_final = registry
        .publish_to_group(&ctx, room_name, None, payload)
        .await?;
    println!("  [alice]: {}", msg.text);
    println!("  publish_to_group -> {} recipients (charlie left, so only alice & bob)", recipients_final.len());
    assert_eq!(recipients_final.len(), 2, "Only alice and bob should receive");
    println!();

    // =========================================================================
    // Step 7: Cleanup and list_groups (empty after delete)
    // =========================================================================
    println!("Step 7: Close room");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    registry.delete_group(&ctx, room_name).await?;
    println!("  Room #general closed");

    let groups_after_delete = registry.list_groups(&ctx).await?;
    println!("  list_groups after delete: {:?}", groups_after_delete);
    assert!(groups_after_delete.is_empty(), "Expected no rooms after delete");
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Chat Room Example Complete");
    println!();
    println!("Key Concepts:");
    println!("  - Process Groups: Named groups of actors");
    println!("  - Pub/Sub: Broadcast to all group members");
    println!("  - Dynamic membership: Join/leave at runtime");
    println!();
    println!("PlexSpaces Integration:");
    println!("  - ProcessGroupRegistry: create_group, delete_group");
    println!("  - join_group / leave_group / get_members");
    println!("  - publish_to_group (broadcast) / list_groups");
    println!();
    println!("Use Cases:");
    println!("  - Chat rooms / channels");
    println!("  - Live notifications");
    println!("  - Collaborative editing");
    println!("  - Config update broadcasts");
    println!();

    Ok(())
}
