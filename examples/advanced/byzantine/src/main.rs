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

//! Byzantine Generals Example - Main Entry Point
//!
//! Demonstrates Byzantine Fault Tolerant consensus using PlexSpaces actors.

use byzantine_generals::{application::ByzantineApplication, config::ByzantineConfig};
use plexspaces_core::application::Application;
use plexspaces_node::NodeBuilder;
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .init();

    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║     Byzantine Generals - Consensus Example                ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();

    // Load configuration using ConfigBootstrap
    let config: ByzantineConfig = ByzantineConfig::load();
    config.validate()?;

    println!("Configuration:");
    println!("  Generals: {}", config.general_count);
    println!("  Byzantine (faulty): {}", config.fault_count);
    println!("  TupleSpace backend: {}", config.tuplespace_backend);
    println!();

    // Create node
    let node = NodeBuilder::new("byzantine-node")
        .build();

    // Create and start application
    let mut app = ByzantineApplication::from_config(config)?;
    
    // Node implements ApplicationNode, so we can pass it directly
    let app_node = std::sync::Arc::new(node);
    
    app.start(app_node).await?;

    // Wait a bit for consensus to run
    println!();
    println!("Running consensus...");
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Stop application
    app.stop().await?;

    println!();
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║                    Example Complete                        ║");
    println!("╚════════════════════════════════════════════════════════════╝");

    Ok(())
}

