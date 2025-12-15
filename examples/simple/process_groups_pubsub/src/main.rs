// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Simple Process Groups (Pub/Sub) Example
//
// ## Purpose
// Demonstrates Process Groups for pub/sub coordination - the simplest possible example.
//
// ## Pattern
// Broadcast messaging - all group members receive messages
//
// ## What This Shows
// - Creating a process group
// - Actors joining the group
// - Broadcasting messages to all members
// - Actors leaving the group
//
// ## Run
// ```bash
// cargo run --example process_groups_pubsub
// ```

use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_process_groups::ProcessGroupRegistry;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Process Groups (Pub/Sub) - Simple Example                   ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Create KeyValueStore backend (in-memory for simplicity)
    let kv_store = Arc::new(InMemoryKVStore::new());

    // Create ProcessGroupRegistry
    let registry = ProcessGroupRegistry::new("simple-node", kv_store);

    const TENANT_ID: &str = "simple-tenant";
    const NAMESPACE: &str = "default";
    const GROUP_NAME: &str = "notifications";

    // Step 1: Create process group
    println!("Step 1: Creating process group '{}'...", GROUP_NAME);
    let group = registry
        .create_group(GROUP_NAME, TENANT_ID, NAMESPACE)
        .await?;
    println!("✅ Group created: {} (tenant: {})\n", group.group_name, group.tenant_id);

    // Step 2: Actors join the group
    println!("Step 2: Actors joining group...");
    let actor_ids = vec!["actor-1", "actor-2", "actor-3"];
    
    for actor_id in &actor_ids {
        registry
            .join_group(GROUP_NAME, TENANT_ID, &actor_id.to_string())
            .await?;
        println!("  ✅ {} joined", actor_id);
    }

    // Verify members
    let members = registry.get_members(GROUP_NAME, TENANT_ID).await?;
    println!("\n✅ Total members: {}\n", members.len());
    assert_eq!(members.len(), 3);

    // Step 3: Broadcast message to all members
    println!("Step 3: Broadcasting message to all members...");
    println!("  📢 Message: 'Hello from broadcaster!'");
    
    // In a real implementation, this would use publish_to_group RPC
    // For this simple example, we demonstrate the pattern
    for member_id in &members {
        println!("  → {} received: 'Hello from broadcaster!'", member_id);
    }
    println!();

    // Step 4: One actor leaves
    println!("Step 4: Actor-1 leaving group...");
    registry
        .leave_group(GROUP_NAME, TENANT_ID, &"actor-1".to_string())
        .await?;
    
    let remaining = registry.get_members(GROUP_NAME, TENANT_ID).await?;
    println!("✅ Remaining members: {}\n", remaining.len());
    assert_eq!(remaining.len(), 2);

    // Step 5: Broadcast again (only remaining members receive)
    println!("Step 5: Broadcasting another message...");
    println!("  📢 Message: 'Second broadcast!'");
    for member_id in &remaining {
        println!("  → {} received: 'Second broadcast!'", member_id);
    }
    println!("  → actor-1 did NOT receive (left group)\n");

    // Step 6: Cleanup
    println!("Step 6: Cleaning up...");
    registry.delete_group(GROUP_NAME, TENANT_ID).await?;
    println!("✅ Group deleted\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Example complete!");
    println!();
    println!("Key Takeaways:");
    println!("  • Process Groups enable pub/sub patterns");
    println!("  • All members receive broadcast messages");
    println!("  • Members can join/leave dynamically");
    println!("  • Use for: config updates, event notifications, coordination");
    println!();
    println!("See: docs/GROUPS_COMPARISON.md for when to use this pattern");
    println!();

    Ok(())
}

