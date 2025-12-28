// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Node binary for entity recognition example.
//!
//! Starts a PlexSpaces node with resource-aware scheduling enabled.
//! Supports both in-memory (single-node) and Redis (multi-node) backends.

use anyhow::Result;
use clap::Parser;
use entity_recognition::config::EntityRecognitionConfig;
use plexspaces_node::{NodeBuilder, ConfigBootstrap};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "entity-recognition-node")]
#[command(about = "PlexSpaces node for entity recognition AI workload")]
struct Args {
    /// Configuration file path
    #[arg(short, long)]
    config: Option<PathBuf>,
    
    /// Node ID
    #[arg(long)]
    node_id: Option<String>,
    
    /// Listen address
    #[arg(long, default_value = "0.0.0.0:9000")]
    listen_addr: String,
    
    /// Backend type (memory or redis)
    #[arg(long, default_value = "memory")]
    backend: String,
    
    /// Redis URL (if backend is redis)
    #[arg(long)]
    redis_url: Option<String>,
    
    /// Node labels (comma-separated key=value pairs)
    #[arg(long)]
    labels: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    let args = Args::parse();

    // Load configuration using ConfigBootstrap
    let config: EntityRecognitionConfig = ConfigBootstrap::load().unwrap_or_default();
    config.validate().map_err(|e| anyhow::anyhow!("Config validation failed: {}", e))?;

    // Override with command line args if provided
    let node_id = args.node_id.unwrap_or_else(|| {
        if config.node.node_id.is_empty() {
            format!("node-{}", ulid::Ulid::new())
        } else {
            config.node.node_id.clone()
        }
    });
    
    let listen_addr = if args.listen_addr != "0.0.0.0:9000" {
        args.listen_addr
    } else {
        config.node.listen_addr.clone()
    };

    info!("Starting entity recognition node: {}", node_id);
    info!("Backend: {}", config.backend);
    info!("Listen address: {}", listen_addr);

    // Create node using NodeBuilder
    let node = NodeBuilder::new(node_id.clone())
        .with_listen_address(listen_addr)
        .with_clustering_enabled(true)
        .build().await;

    let node = std::sync::Arc::new(node);

    // Start node (this will start gRPC services, background scheduler, etc.)
    info!("Node {} starting...", node.id().as_str());
    node.start().await?;

    Ok(())
}

