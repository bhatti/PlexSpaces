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

//! Node status, health, and lifecycle commands

use anyhow::{Context, Result};
use plexspaces_node::NodeBuilder;
use plexspaces_proto::system::v1::{
    system_service_client::SystemServiceClient, GetHealthRequest,
};
use std::sync::Arc;
use tokio::signal;
use tonic::transport::Channel;
use tracing::{debug, info, warn};
use tracing_subscriber;

pub async fn status(node_addr: &str) -> Result<()> {
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = SystemServiceClient::new(channel);

    println!("📊 Node Status: {}", node_addr);

    let request = GetHealthRequest {
        components: vec![],
    };
    let response = client
        .get_health(tonic::Request::new(request))
        .await
        .context("Failed to get node status")?
        .into_inner();

    println!("   Overall Status: {:?}", response.overall_status);
    println!("   Health Checks: {}", response.checks.len());

    Ok(())
}

/// Start a PlexSpaces node instance
pub async fn start(node_id: &str, listen_addr: &str, release_config: Option<&str>) -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║     Starting PlexSpaces Node                                  ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
    info!(node_id = %node_id, listen_addr = %listen_addr, "Node starting");

    // Create shutdown channel for fatal errors FIRST (before any initialization)
    // This allows fatal errors during node construction to signal the main thread to exit
    // CRITICAL: Must be registered BEFORE NodeBuilder::build() which calls initialize_services()
    let (fatal_error_tx, mut fatal_error_rx) = tokio::sync::oneshot::channel::<String>();
    use plexspaces_services::service_locator::register_fatal_error_channel;
    register_fatal_error_channel(fatal_error_tx);

    // Load release config if provided
    let mut builder = NodeBuilder::new(node_id.to_string())
        .with_listen_addr(listen_addr.to_string());
    
    let mut spec = if let Some(release_config_path) = release_config {
        debug!("Loading release config from: {}", release_config_path);
        use plexspaces_node::config::loader::ConfigLoader;
        let loader = ConfigLoader::new();
        match loader.load_release_spec(release_config_path).await {
            Ok(spec) => spec,
            Err(e) => {
                warn!("Failed to load release config: {}. Using defaults.", e);
                // Fall back to default config
                use plexspaces_common::release_config::create_default_release_config;
                create_default_release_config(
                    "plexspaces-cluster".to_string(),
                    "1.0.0".to_string(),
                    node_id.to_string(),
                    listen_addr.to_string(),
                ).await
            }
        }
    } else {
        // Use default release config
        use plexspaces_common::release_config::create_default_release_config;
        create_default_release_config(
            "plexspaces-cluster".to_string(),
            "1.0.0".to_string(),
            node_id.to_string(),
            listen_addr.to_string(),
        ).await
    };
    
    // Initialize config: apply env overrides, set defaults, validate, and log
    use plexspaces_common::config_manager::initialize;
    initialize(&mut spec);
    
    builder = builder.with_release_spec(spec);
    
    // Create node using NodeBuilder (this may call fatal_exit() if security validation fails)
    let node = builder.build().await;
    let node = Arc::new(node);
    
    // Start node in spawned task
    let node_for_start = node.clone();
    tokio::spawn(async move {
        if let Err(e) = node_for_start.start().await {
            let error_msg = format!("Node failed to start: {}", e);
            tracing::error!(error = %e, "Node startup failed");
            eprintln!("FATAL: {}", error_msg);
            // For non-security startup errors, exit directly
            // (Security errors are handled by fatal_exit() which signals via global channel)
            std::process::exit(1);
        }
    });

    // Wait a bit for node to initialize
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    info!("✅ Node started successfully!");
    info!("   Listening on: {}", listen_addr);
    info!("   gRPC endpoint: http://{}", listen_addr);

    // Wait for either shutdown signal (Ctrl+C) OR fatal error from anywhere
    // This follows the Go graceful shutdown pattern where the main thread listens
    // for both OS signals and internal fatal error signals
    tokio::select! {
        result = signal::ctrl_c() => {
            match result {
                Ok(()) => {
                    info!("Shutdown signal received, stopping node...");
                }
                Err(err) => {
                    warn!("Unable to listen for shutdown signal: {}", err);
                }
            }
        }
        fatal_error = &mut fatal_error_rx => {
            match fatal_error {
                Ok(msg) => {
                    tracing::error!("Fatal error during startup: {}", msg);
                    eprintln!("FATAL: {}", msg);
                }
                Err(_) => {
                    // Channel closed - fatal_exit was called (sender dropped)
                    // This happens when fatal_exit() calls _exit() after sending
                    tracing::error!("Fatal error channel closed - fatal_exit() was called");
                    eprintln!("FATAL: Process terminated due to fatal initialization error");
                }
            }
            // Exit immediately on fatal error
            std::process::exit(1);
        }
    }

    // Graceful shutdown
    info!("Node shutting down...");
    Ok(())
}

