// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Genomics Pipeline Node Binary
//!
//! Entry point for running genomics pipeline nodes in a distributed cluster.
//!
//! ## Usage
//!
//! ```bash
//! # Start coordinator node
//! genomics-node --config config/node1.toml
//!
//! # Start worker node
//! genomics-node --config config/node2.toml
//! ```

use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;
use tracing::{info, warn};
use tracing_subscriber;

use plexspaces_node::{NodeBuilder, ConfigBootstrap};
use genomics_pipeline::{GenomicsPipelineApplication, config::GenomicsPipelineConfig};

/// Command line arguments
#[derive(Parser, Debug)]
#[command(name = "genomics-node")]
#[command(about = "PlexSpaces Genomics Pipeline Node", long_about = None)]
struct Args {
    /// Override node ID from config
    #[arg(long)]
    node_id: Option<String>,

    /// Override listen address from config
    #[arg(long)]
    listen_addr: Option<String>,
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

    info!("Starting Genomics Pipeline Node");

    // Load configuration using ConfigBootstrap
    let app_config: GenomicsPipelineConfig = ConfigBootstrap::load()
        .unwrap_or_else(|_| {
            warn!("Failed to load config from release.toml, using defaults");
            GenomicsPipelineConfig::default()
        });

    // Override with CLI arguments if provided
    let node_id = args.node_id
        .unwrap_or_else(|| {
            if app_config.node.id.is_empty() {
                "genomics-node-1".to_string()
            } else {
                app_config.node.id.clone()
            }
        });

    let listen_addr = args.listen_addr
        .unwrap_or_else(|| app_config.node.listen_address.clone());

    info!("Node ID: {}", node_id);
    info!("Listen address: {}", listen_addr);
    info!("Worker pools: QC={}, Alignment={}, Chromosome={}, Annotation={}, Report={}",
        app_config.worker_pools.qc,
        app_config.worker_pools.alignment,
        app_config.worker_pools.chromosome,
        app_config.worker_pools.annotation,
        app_config.worker_pools.report);

    // Create node using NodeBuilder
    let node = Arc::new(
        NodeBuilder::new(node_id.clone())
            .with_listen_address(listen_addr.clone())
            .build()
            .await
    );

    info!("Node created: {}", node_id);

    // Create application with loaded config
    let app = Box::new(GenomicsPipelineApplication::with_config(app_config));
    node.register_application(app)
        .await
        .with_context(|| "Failed to register genomics application")?;

    info!("Registered genomics-pipeline application");

    // Start genomics pipeline application
    node.start_application("genomics-pipeline")
        .await
        .with_context(|| "Failed to start genomics application")?;

    info!("Started genomics-pipeline application");

    // Start node (this will start gRPC server, health checks, etc.)
    // This blocks until SIGTERM/SIGINT is received, then performs graceful shutdown
    info!("Listening on: {}", listen_addr);
    info!("Press Ctrl+C to shutdown");

    node.clone().start()
        .await
        .with_context(|| "Failed to start node")?;

    info!("Node shutdown complete");

    Ok(())
}
