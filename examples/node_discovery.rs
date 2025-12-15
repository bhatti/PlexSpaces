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

//! NodeRegistry Example - Distributed Service Discovery
//!
//! ## Purpose
//! Demonstrates how to use NodeRegistry for distributed service discovery,
//! health monitoring, and capability-based node selection in a PlexSpaces cluster.
//!
//! ## What This Example Shows
//! 1. **Node Registration**: Nodes register themselves with metadata and capabilities
//! 2. **Service Discovery**: Nodes discover each other across the cluster
//! 3. **Health Monitoring**: Track node health via heartbeats and error rates
//! 4. **Capability Matching**: Find nodes by features (WASM, Firecracker, GPU)
//! 5. **Load Balancing**: Select least-loaded nodes for actor placement
//! 6. **Failure Detection**: Detect and handle node failures
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   Shared KeyValueStore                       │
//! │         (SQLite/Redis - Distributed Registry)               │
//! └───────┬─────────────────────┬───────────────┬──────────────┘
//!         │                     │               │
//!    ┌────▼────┐           ┌────▼────┐     ┌────▼────┐
//!    │ Node 1  │           │ Node 2  │     │ Node 3  │
//!    │ (WASM)  │           │ (FC)    │     │ (Both)  │
//!    └─────────┘           └─────────┘     └─────────┘
//! ```
//!
//! ## Running This Example
//! ```bash
//! # Run with default configuration (in-memory KeyValueStore)
//! cargo run --example node_discovery
//!
//! # Run with SQLite backend (for multi-process testing)
//! PLEXSPACES_KV_BACKEND=sqlite \
//! PLEXSPACES_KV_SQLITE_PATH=/tmp/plexspaces_registry.db \
//! cargo run --example node_discovery
//!
//! # Run with Redis backend (for distributed cluster)
//! PLEXSPACES_KV_BACKEND=redis \
//! PLEXSPACES_KV_REDIS_URL=redis://localhost:6379 \
//! PLEXSPACES_KV_REDIS_NAMESPACE=plexspaces: \
//! cargo run --example node_discovery
//! ```
//!
//! ## Key Takeaways
//! - NodeRegistry provides DNS-like service discovery for PlexSpaces nodes
//! - Nodes automatically track health via heartbeats (every 10s recommended)
//! - Capability-based selection enables heterogeneous clusters
//! - Registry is backend-agnostic (InMemory, SQLite, Redis, PostgreSQL)

// TODO: Update this example to use ObjectRegistry instead of NodeRegistry
// The old NodeRegistry has been removed as part of registry consolidation.
// This example needs to be rewritten to use ObjectRegistry with ObjectType::ObjectTypeService
// for node registration and discovery.

/*
use plexspaces::keyvalue::{create_keyvalue_from_env, KeyValueStore};
use plexspaces_object_registry::ObjectRegistry;
use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    println!("=== PlexSpaces NodeRegistry Example ===\n");

    // Create shared KeyValueStore (simulates distributed registry)
    let kv_store: Arc<dyn KeyValueStore> = create_keyvalue_from_env().await?;
    println!(
        "✓ KeyValueStore initialized (backend: {:?})\n",
        std::env::var("PLEXSPACES_KV_BACKEND").unwrap_or_else(|_| "inmemory".to_string())
    );

    // =========================================================================
    // SCENARIO 1: NODE REGISTRATION
    // =========================================================================
    println!("--- Scenario 1: Node Registration ---");

    // Node 1: WASM-capable compute node
    let mut caps1 = HashMap::new();
    caps1.insert("wasm".to_string(), "enabled".to_string());
    caps1.insert("firecracker".to_string(), "disabled".to_string());
    caps1.insert("gpu".to_string(), "none".to_string());

    let node1 = NodeRegistry::with_capabilities(
        "compute-node-1",
        "http://192.168.1.10:9001",
        caps1,
        kv_store.clone(),
    );

    // Node 2: Firecracker-capable isolation node
    let mut caps2 = HashMap::new();
    caps2.insert("wasm".to_string(), "disabled".to_string());
    caps2.insert("firecracker".to_string(), "enabled".to_string());
    caps2.insert("gpu".to_string(), "none".to_string());

    let node2 = NodeRegistry::with_capabilities(
        "isolation-node-1",
        "http://192.168.1.20:9002",
        caps2,
        kv_store.clone(),
    );

    // Node 3: Hybrid node with both capabilities + GPU
    let mut caps3 = HashMap::new();
    caps3.insert("wasm".to_string(), "enabled".to_string());
    caps3.insert("firecracker".to_string(), "enabled".to_string());
    caps3.insert("gpu".to_string(), "nvidia-a100".to_string());

    let node3 = NodeRegistry::with_capabilities(
        "hybrid-node-1",
        "http://192.168.1.30:9003",
        caps3,
        kv_store.clone(),
    );

    // Register all nodes
    node1.register().await?;
    println!("  ✓ Registered: compute-node-1 (WASM capable)");

    node2.register().await?;
    println!("  ✓ Registered: isolation-node-1 (Firecracker capable)");

    node3.register().await?;
    println!("  ✓ Registered: hybrid-node-1 (WASM + Firecracker + GPU)\n");

    // =========================================================================
    // SCENARIO 2: SERVICE DISCOVERY
    // =========================================================================
    println!("--- Scenario 2: Service Discovery ---");

    // List all registered nodes
    let all_nodes = node1.list_nodes().await?;
    println!("  Total nodes in cluster: {}", all_nodes.len());

    for node in &all_nodes {
        println!("    - {} @ {}", node.node_id, node.node_address);
    }
    println!();

    // Lookup specific node
    let found_node = node1.lookup("hybrid-node-1").await?;
    if let Some(node_info) = found_node {
        println!("  ✓ Found hybrid-node-1:");
        println!("      Address: {}", node_info.node_address);
        println!("      Capabilities: {:?}", node_info.capabilities);
        println!("      Status: {:?}", node_info.status);
    }
    println!();

    // =========================================================================
    // SCENARIO 3: CAPABILITY-BASED DISCOVERY
    // =========================================================================
    println!("--- Scenario 3: Capability-Based Discovery ---");

    // Find all WASM-capable nodes
    let wasm_nodes: Vec<_> = all_nodes
        .iter()
        .filter(|n| {
            n.capabilities
                .get("wasm")
                .map(|v| v == "enabled")
                .unwrap_or(false)
        })
        .collect();

    println!("  WASM-capable nodes ({}):", wasm_nodes.len());
    for node in &wasm_nodes {
        println!("    - {}", node.node_id);
    }
    println!();

    // Find all Firecracker-capable nodes
    let fc_nodes: Vec<_> = all_nodes
        .iter()
        .filter(|n| {
            n.capabilities
                .get("firecracker")
                .map(|v| v == "enabled")
                .unwrap_or(false)
        })
        .collect();

    println!("  Firecracker-capable nodes ({}):", fc_nodes.len());
    for node in &fc_nodes {
        println!("    - {}", node.node_id);
    }
    println!();

    // Find GPU nodes
    let gpu_nodes: Vec<_> = all_nodes
        .iter()
        .filter(|n| {
            n.capabilities
                .get("gpu")
                .map(|v| v != "none")
                .unwrap_or(false)
        })
        .collect();

    println!("  GPU-equipped nodes ({}):", gpu_nodes.len());
    for node in &gpu_nodes {
        let gpu_type = node.capabilities.get("gpu").unwrap();
        println!("    - {} (GPU: {})", node.node_id, gpu_type);
    }
    println!();

    // =========================================================================
    // SCENARIO 4: HEALTH MONITORING
    // =========================================================================
    println!("--- Scenario 4: Health Monitoring ---");

    // Simulate workload on nodes
    node1.record_metrics(1000, 10).await?; // 1% error rate (healthy)
    node2.record_metrics(500, 100).await?; // 20% error rate (degraded!)
    node3.record_metrics(2000, 5).await?; // 0.25% error rate (healthy)

    println!("  ✓ Recorded metrics for all nodes");

    // Check health status
    for node_id in &["compute-node-1", "isolation-node-1", "hybrid-node-1"] {
        let is_healthy = node1.is_node_healthy(node_id).await?;
        let status_emoji = if is_healthy { "✓" } else { "✗" };
        println!(
            "    {} {}: {}",
            status_emoji,
            node_id,
            if is_healthy { "Healthy" } else { "Unhealthy" }
        );
    }
    println!();

    // Find degraded nodes
    let degraded = node1.find_degraded_nodes().await?;
    if !degraded.is_empty() {
        println!("  ⚠️  Degraded nodes detected:");
        for node in &degraded {
            let error_rate = NodeRegistrationHelpers::error_rate(node) * 100.0;
            println!("    - {} (error rate: {:.1}%)", node.node_id, error_rate);
            println!(
                "        Messages: {}, Errors: {}",
                node.message_count, node.error_count
            );
        }
    }
    println!();

    // =========================================================================
    // SCENARIO 5: LOAD BALANCING (Actor Placement)
    // =========================================================================
    println!("--- Scenario 5: Load-Based Node Selection ---");

    // In real implementation, actor_count would be tracked by Node
    // Here we simulate it for demonstration
    println!("  Selecting node for new actor placement...");

    // For now, just show how to retrieve nodes for placement decisions
    let healthy_nodes: Vec<_> = all_nodes
        .iter()
        .filter(|n| NodeRegistrationHelpers::is_healthy(n))
        .collect();

    println!("  Healthy nodes available: {}", healthy_nodes.len());

    if let Some(selected) = healthy_nodes.first() {
        println!(
            "  ✓ Selected: {} @ {}",
            selected.node_id, selected.node_address
        );
        println!("      (In production: would route actor spawn to this node)\n");
    }

    // =========================================================================
    // SCENARIO 6: HEARTBEAT UPDATES
    // =========================================================================
    println!("--- Scenario 6: Heartbeat Updates ---");

    println!("  Updating heartbeats...");
    node1.update_heartbeat().await?;
    node2.update_heartbeat().await?;
    node3.update_heartbeat().await?;
    println!("  ✓ All nodes refreshed heartbeats\n");

    // Verify heartbeats are recent
    let updated_nodes = node1.list_nodes().await?;
    for node in &updated_nodes {
        if let Some(ref hb) = node.last_heartbeat {
            let now = chrono::Utc::now().timestamp();
            let age = now - hb.seconds;
            println!("    {} heartbeat age: {}s", node.node_id, age);
        }
    }
    println!();

    // =========================================================================
    // SCENARIO 7: NODE UNREGISTRATION (Graceful Shutdown)
    // =========================================================================
    println!("--- Scenario 7: Node Unregistration ---");

    println!("  Simulating node shutdown (compute-node-1)...");
    node1.unregister().await?;
    println!("  ✓ compute-node-1 unregistered\n");

    let remaining_nodes = node2.list_nodes().await?;
    println!("  Remaining nodes in cluster: {}", remaining_nodes.len());
    for node in &remaining_nodes {
        println!("    - {}", node.node_id);
    }
    println!();

    // =========================================================================
    // SUMMARY
    // =========================================================================
    println!("=== Example Complete ===");
    println!(
        "
Key Concepts Demonstrated:
  ✓ Node registration with capabilities
  ✓ Cross-node service discovery
  ✓ Capability-based node filtering
  ✓ Health monitoring via heartbeats
  ✓ Degraded node detection
  ✓ Load-aware node selection
  ✓ Graceful node shutdown

Next Steps:
  - Integrate with Actor system for dynamic actor placement
  - Add heartbeat background task (tokio::spawn interval)
  - Implement failover logic for actor migration
  - Set up monitoring dashboards (Prometheus/Grafana)
    "
    );

    Ok(())
}
*/

fn main() {
    println!("This example is temporarily disabled pending migration to ObjectRegistry.");
    println!("See TODO comment in examples/node_discovery.rs for details.");
}
