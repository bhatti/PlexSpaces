// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Finance Risk Assessment Node Binary
//!
//! Entry point for running finance risk assessment nodes in a distributed cluster.
//!
//! ## Usage
//!
//! ```bash
//! # Start coordinator node
//! finance-node --config config/node1.toml
//!
//! # Start worker node
//! finance-node --config config/node2.toml
//! ```

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber;

use plexspaces_node::Node;
use finance_risk::{FinanceRiskApplication, StorageConfig};

/// Command line arguments
#[derive(Parser, Debug)]
#[command(name = "finance-node")]
#[command(about = "PlexSpaces Finance Risk Assessment Node", long_about = None)]
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

    info!("Starting Finance Risk Assessment Node");
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

    // Create and start node using NodeBuilder
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new(node_id.clone())
        .with_listen_address(listen_addr.clone())
        .with_max_connections(100)
        .with_heartbeat_interval_ms(5000)
        .with_clustering_enabled(true)
        .build().await);

    info!("Starting node: {}", node_id);

    // Check if durability is enabled via environment variables
    let app = if std::env::var("FINANCE_DATA_DIR").is_ok() {
        // Durability enabled - create StorageConfig from environment
        info!("Durability enabled - initializing storage configuration");
        let storage_config = StorageConfig::from_env();
        Box::new(FinanceRiskApplication::with_storage(storage_config))
    } else {
        // No durability - use default configuration
        info!("Durability disabled - actors will not persist state");
        Box::new(FinanceRiskApplication::new())
    };

    // Register finance risk assessment application
    node.register_application(app)
        .await
        .with_context(|| "Failed to register finance risk application")?;

    info!("Registered finance-risk application");

    // Start finance risk assessment application
    node.start_application("finance-risk")
        .await
        .with_context(|| "Failed to start finance risk application")?;

    info!("Started finance-risk application");

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
