// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Chat Room Example (Process Groups / Pub/Sub)
//
// Demonstrates process groups for broadcast messaging:
// - Users join/leave chat rooms
// - Messages broadcast to all room members
// - Real-time group coordination
//
// Use Case: Chat application, live notifications, collaborative editing

use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_process_groups::ProcessGroupRegistry;
use plexspaces_core::{ActorId, RequestContext};
use std::sync::Arc;

// =============================================================================
// Chat Message (would be serialized in real app)
// =============================================================================

#[derive(Debug, Clone)]
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

    // Create backend (in-memory for demo, use Redis for production)
    let kv_store = Arc::new(InMemoryKVStore::new());
    let registry = ProcessGroupRegistry::new("chat-server", kv_store);

    let tenant_id = "acme-corp";
    let namespace = "chat";
    let room_name = "general";
    
    // Create RequestContext for operations that require it
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // =========================================================================
    // Step 1: Create chat room (process group)
    // =========================================================================
    println!("Step 1: Create chat room");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let room = registry.create_group(&ctx, room_name).await?;
    println!("  Room created: #{}", room.group_name);
    println!("  Tenant: {}", room.tenant_id);
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
    // Step 3: Alice sends a message (broadcast to all)
    // =========================================================================
    println!("Step 3: Alice sends a message");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = ChatMessage::new("alice", "Hey everyone! How's it going?");
    
    // Broadcast to all room members
    println!("  [alice]: {}", msg.text);
    println!();
    println!("  Delivered to:");
    for member in &members {
        if !member.as_str().starts_with("alice") {
            println!("    -> {}", member);
        }
    }
    println!();

    // =========================================================================
    // Step 4: Bob replies
    // =========================================================================
    println!("Step 4: Bob replies");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = ChatMessage::new("bob", "Great! Working on the new feature.");
    
    println!("  [bob]: {}", msg.text);
    println!();
    println!("  Delivered to:");
    for member in &members {
        if !member.as_str().starts_with("bob") {
            println!("    -> {}", member);
        }
    }
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
    // Step 6: Alice sends another message (Charlie doesn't receive)
    // =========================================================================
    println!("Step 6: Alice sends another message");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = ChatMessage::new("alice", "Charlie left, it's just us now!");
    
    println!("  [alice]: {}", msg.text);
    println!();
    println!("  Delivered to:");
    for member in &remaining {
        if !member.as_str().starts_with("alice") {
            println!("    -> {}", member);
        }
    }
    println!("  (charlie did NOT receive - left room)");
    println!();

    // =========================================================================
    // Step 7: Cleanup
    // =========================================================================
    println!("Step 7: Close room");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    registry.delete_group(&ctx, room_name).await?;
    println!("  Room #general closed");
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
    println!("  - ProcessGroupRegistry: Manages groups");
    println!("  - create_group(name, tenant, namespace)");
    println!("  - join_group / leave_group");
    println!("  - get_members / publish_to_group");
    println!();
    println!("Use Cases:");
    println!("  - Chat rooms / channels");
    println!("  - Live notifications");
    println!("  - Collaborative editing");
    println!("  - Config update broadcasts");
    println!();

    Ok(())
}
