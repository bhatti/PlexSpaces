// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! # Config Updates Example - Process Groups Pub/Sub Pattern
//!
//! ## Purpose
//! Demonstrates PlexSpaces process groups for distributed configuration updates.
//! All actors subscribe to "config-updates" group and receive broadcast notifications
//! when configuration changes.
//!
//! ## Pattern
//! Classic pub/sub using Erlang pg/pg2-style process groups.
//!
//! ## What This Example Shows
//! - Creating process groups
//! - Subscribing actors to groups (join)
//! - Broadcasting messages to all group members
//! - Unsubscribing from groups (leave)
//!
//! ## Expected Output
//! ```text
//! [Admin] Creating 'config-updates' process group...
//! [Admin] Process group 'config-updates' created successfully
//!
//! [Worker-1] Joining 'config-updates' group...
//! [Worker-2] Joining 'config-updates' group...
//! [Worker-3] Joining 'config-updates' group...
//! [Info] 3 workers subscribed to config updates
//!
//! [Admin] Publishing new configuration to all workers...
//! [Worker-1] ✓ Received config update: {"database_url": "postgres://localhost/prod"}
//! [Worker-2] ✓ Received config update: {"database_url": "postgres://localhost/prod"}
//! [Worker-3] ✓ Received config update: {"database_url": "postgres://localhost/prod"}
//!
//! [Worker-1] Leaving 'config-updates' group...
//! [Info] Workers remaining: 2
//!
//! [Admin] Publishing another config update...
//! [Worker-2] ✓ Received config update: {"max_connections": 100}
//! [Worker-3] ✓ Received config update: {"max_connections": 100}
//! [Info] Worker-1 did not receive update (already left group)
//!
//! Example completed successfully!
//! ```

use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_process_groups::ProcessGroupRegistry;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Configuration data structure
#[derive(Debug, Serialize, Deserialize)]
struct Config {
    database_url: Option<String>,
    max_connections: Option<u32>,
    cache_enabled: Option<bool>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== PlexSpaces Process Groups: Config Updates Example ===\n");

    // Create KeyValueStore backend (in-memory for this example)
    let kv_store = Arc::new(InMemoryKVStore::new());

    // Create ProcessGroupRegistry
    let registry = ProcessGroupRegistry::new("example-node-1", kv_store);

    // Constants
    const TENANT_ID: &str = "example-tenant";
    const NAMESPACE: &str = "production";
    const GROUP_NAME: &str = "config-updates";

    // Step 1: Create process group
    println!("[Admin] Creating '{}' process group...", GROUP_NAME);
    let group = registry.create_group(GROUP_NAME, TENANT_ID, NAMESPACE).await?;
    println!("[Admin] Process group '{}' created successfully\n", group.group_name);

    // Step 2: Simulate workers joining the group
    let workers = vec!["worker-1", "worker-2", "worker-3"];

    for worker_id in &workers {
        println!("[{}] Joining '{}' group...", worker_id.to_uppercase(), GROUP_NAME);
        registry.join_group(GROUP_NAME, TENANT_ID, &worker_id.to_string()).await?;
    }

    // Verify all workers joined
    let members = registry.get_members(GROUP_NAME, TENANT_ID).await?;
    println!("[Info] {} workers subscribed to config updates\n", members.len());
    assert_eq!(members.len(), 3);

    // Step 3: Simulate configuration update broadcast
    println!("[Admin] Publishing new configuration to all workers...");

    let config = Config {
        database_url: Some("postgres://localhost/prod".to_string()),
        max_connections: None,
        cache_enabled: None,
    };
    let config_json = serde_json::to_string_pretty(&config)?;

    // In real implementation, this would use publish_to_group RPC
    // For now, we simulate broadcast by manually sending to all members
    for member_id in &members {
        println!("[{}] ✓ Received config update: {}",
            member_id.to_uppercase(),
            config_json.lines().collect::<Vec<_>>().join(" "));
    }
    println!();

    // Step 4: One worker leaves the group
    println!("[Worker-1] Leaving '{}' group...", GROUP_NAME);
    registry.leave_group(GROUP_NAME, TENANT_ID, &"worker-1".to_string()).await?;

    let remaining_members = registry.get_members(GROUP_NAME, TENANT_ID).await?;
    println!("[Info] Workers remaining: {}\n", remaining_members.len());
    assert_eq!(remaining_members.len(), 2);

    // Step 5: Publish another update (only remaining workers receive it)
    println!("[Admin] Publishing another config update...");

    let config2 = Config {
        database_url: None,
        max_connections: Some(100),
        cache_enabled: Some(true),
    };
    let config_json2 = serde_json::to_string_pretty(&config2)?;

    for member_id in &remaining_members {
        println!("[{}] ✓ Received config update: {}",
            member_id.to_uppercase(),
            config_json2.lines().collect::<Vec<_>>().join(" "));
    }
    println!("[Info] Worker-1 did not receive update (already left group)\n");

    // Step 6: Demonstrate multiple joins (Erlang pg2 semantics)
    println!("[Worker-2] Joining '{}' group again (multiple joins)...", GROUP_NAME);
    registry.join_group(GROUP_NAME, TENANT_ID, &"worker-2".to_string()).await?;

    // Still only appears once in members list
    let members_after_double_join = registry.get_members(GROUP_NAME, TENANT_ID).await?;
    println!("[Info] Members count still {}: {:?}", members_after_double_join.len(), members_after_double_join);
    assert_eq!(members_after_double_join.len(), 2);

    // Must leave twice to fully remove
    println!("[Worker-2] Leaving '{}' group (first time)...", GROUP_NAME);
    registry.leave_group(GROUP_NAME, TENANT_ID, &"worker-2".to_string()).await?;

    let members_after_first_leave = registry.get_members(GROUP_NAME, TENANT_ID).await?;
    println!("[Info] Worker-2 still in group after first leave: {:?}\n", members_after_first_leave);
    assert!(members_after_first_leave.contains(&"worker-2".to_string()));

    // Step 7: Clean up
    println!("[Admin] Deleting '{}' group...", GROUP_NAME);
    registry.delete_group(GROUP_NAME, TENANT_ID).await?;

    let groups = registry.list_groups(TENANT_ID).await?;
    println!("[Info] Groups remaining: {}", groups.len());
    assert_eq!(groups.len(), 0);

    println!("\n✅ Example completed successfully!");
    println!("\n## Key Takeaways:");
    println!("1. Process groups enable pub/sub patterns for actor coordination");
    println!("2. Actors can join/leave groups dynamically");
    println!("3. Broadcast messages reach all current group members");
    println!("4. Erlang pg2 semantics: multiple joins require equal leaves");
    println!("5. Groups are tenant-isolated (multi-tenancy support)");

    Ok(())
}
