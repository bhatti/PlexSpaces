// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Byzantine Generals - Main Entry Point
//
// Demonstrates Application framework usage:
// 1. Load config via ConfigBootstrap
// 2. Create Node
// 3. Start Application on Node

use byzantine_generals::{ByzantineApplication, ByzantineConfig};
use plexspaces_application::Application;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing - use try_init() to avoid panic if already initialized
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║     Byzantine Generals - Consensus Example                ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();

    // =========================================================================
    // Step 1: Load configuration
    // =========================================================================
    let config: ByzantineConfig = ByzantineConfig::load();
    config.validate()?;

    println!("Configuration (from release.toml):");
    println!("  Generals: {}", config.general_count);
    println!("  Byzantine (faulty): {}", config.fault_count);
    println!();

    // =========================================================================
    // Step 2: Create Node
    // =========================================================================
    let node = Arc::new(
        NodeBuilder::new("byzantine-node")
            .build()
            .await
    );

    // =========================================================================
    // Step 3: Create and start Application
    // =========================================================================
    let mut app = ByzantineApplication::from_config(config)?;
    
    // Application.start() receives Arc<dyn ApplicationNode>
    // Node implements ApplicationNode trait
    app.start(node.clone()).await?;

    // Let it run briefly
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // =========================================================================
    // Step 4: Stop Application
    // =========================================================================
    app.stop().await?;

    println!();
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║                    Example Complete                        ║");
    println!("╚════════════════════════════════════════════════════════════╝");

    Ok(())
}
