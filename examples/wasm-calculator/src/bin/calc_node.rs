// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! WASM Calculator Node Binary
//!
//! Entry point for running WASM calculator nodes in a distributed cluster.
//!
//! ## Usage
//!
//! ```bash
//! # Start coordinator node
//! calc-node --config config/node1.toml
//!
//! # Start worker node
//! calc-node --config config/node2.toml
//! ```

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber;

use plexspaces_node::{Node, NodeId, NodeConfig};
use wasm_calculator::WasmCalculatorApp;

/// Command line arguments
#[derive(Parser, Debug)]
#[command(name = "calc-node")]
#[command(about = "PlexSpaces WASM Calculator Node", long_about = None)]
struct Args {
    /// Path to TOML configuration file
    #[arg(short, long, value_name = "FILE")]
    config: PathBuf,

    /// Override node ID from config
    #[arg(long)]
    node_id: Option<String>,

    /// Override listen address from config
    #[arg(long)]
    listen_addr: Option<String>,
}

/// Node configuration from TOML file
#[derive(Debug, serde::Deserialize)]
struct TomlConfig {
    #[allow(dead_code)]
    release: Option<ReleaseConfig>,
    node: NodeTomlConfig,
    #[allow(dead_code)]
    runtime: Option<RuntimeConfig>,
    #[allow(dead_code)]
    env: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, serde::Deserialize)]
struct ReleaseConfig {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    version: String,
    #[allow(dead_code)]
    description: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct NodeTomlConfig {
    id: String,
    listen_address: String,
    #[allow(dead_code)]
    cluster_seed_nodes: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize)]
struct RuntimeConfig {
    // Runtime config fields (grpc, health, etc.) - parsed but not used yet
    #[allow(dead_code)]
    grpc: Option<toml::Value>,
    #[allow(dead_code)]
    health: Option<toml::Value>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(true)
        .with_level(true)
        .init();

    // Parse command line arguments
    let args = Args::parse();

    info!("Starting WASM Calculator Node");
    info!("Loading configuration from: {:?}", args.config);

    // Load configuration
    let config_str = std::fs::read_to_string(&args.config)
        .with_context(|| format!("Failed to read config file: {:?}", args.config))?;

    let toml_config: TomlConfig = toml::from_str(&config_str)
        .with_context(|| "Failed to parse TOML configuration")?;

    // Determine node ID (CLI override or from config)
    let node_id = args.node_id
        .unwrap_or_else(|| toml_config.node.id.clone());

    // Determine listen address (CLI override or from config)
    let listen_addr = args.listen_addr
        .unwrap_or_else(|| toml_config.node.listen_address.clone());

    info!("Node ID: {}", node_id);
    info!("Listen address: {}", listen_addr);

    // Create node configuration
    let node_config = NodeConfig {
        listen_addr: listen_addr.clone(),
        max_connections: 100,
        heartbeat_interval_ms: 5000,
        clustering_enabled: true,
        metadata: std::collections::HashMap::new(),
    };

    // Create and start node
    let node = Arc::new(Node::new(NodeId::new(node_id.clone()), node_config));

    info!("Starting node: {}", node_id);

    // Create WASM calculator application
    let app = Box::new(WasmCalculatorApp::new());

    // Register WASM calculator application
    node.register_application(app)
        .await
        .with_context(|| "Failed to register WASM calculator application")?;

    info!("Registered wasm-calculator application");

    // Start WASM calculator application
    node.start_application("wasm-calculator")
        .await
        .with_context(|| "Failed to start WASM calculator application")?;

    info!("Started wasm-calculator application");

    // Start node (this will start gRPC server, health checks, etc.)
    // This blocks until SIGTERM/SIGINT is received, then performs graceful shutdown
    info!("Listening on: {}", listen_addr);
    info!("Press Ctrl+C to shutdown");

    node.start()
        .await
        .with_context(|| "Failed to start node")?;

    info!("Node shutdown complete");

    Ok(())
}
