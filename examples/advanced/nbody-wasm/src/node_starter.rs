// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Simple node starter for testing nbody-wasm example

use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    let node_id = std::env::var("PLEXSPACES_NODE_ID").unwrap_or_else(|_| "nbody-test-node".to_string());
    let listen_addr = std::env::var("PLEXSPACES_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:9001".to_string());

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Starting PlexSpaces Node                                  ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Node ID: {}", node_id);
    println!("Listen Address: {}", listen_addr);
    println!();

    // Create node using NodeBuilder
    let node = NodeBuilder::new(node_id)
        .with_listen_address(listen_addr.clone())
        .build().await;

    let node = Arc::new(node);

    // Start node in background
    let node_for_start = node.clone();
    tokio::spawn(async move {
        if let Err(e) = node_for_start.start().await {
            eprintln!("Node failed to start: {}", e);
        }
    });

    // Wait a bit for node to initialize
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    println!("✅ Node started successfully!");
    println!("   Listening on: {}", listen_addr);
    println!("   gRPC endpoint: http://{}", listen_addr);
    println!();
    println!("Press Ctrl+C to stop the node");
    println!();

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    println!();
    println!("Shutting down node...");

    // Shutdown node
    node.shutdown(tokio::time::Duration::from_secs(10)).await?;

    println!("✅ Node stopped");
    Ok(())
}

