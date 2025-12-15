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
use tracing::{info, warn};
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
pub async fn start(node_id: &str, listen_addr: &str) -> Result<()> {
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
    info!("Node ID: {}", node_id);
    info!("Listen Address: {}", listen_addr);

    // Create node using NodeBuilder
    let node = NodeBuilder::new(node_id.to_string())
        .with_listen_address(listen_addr.to_string())
        .build();

    let node = Arc::new(node);

    // Start node
    let node_for_start = node.clone();
    tokio::spawn(async move {
        if let Err(e) = node_for_start.start().await {
            eprintln!("Node failed to start: {}", e);
            std::process::exit(1);
        }
    });

    // Wait a bit for node to initialize
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    info!("✅ Node started successfully!");
    info!("   Listening on: {}", listen_addr);
    info!("   gRPC endpoint: http://{}", listen_addr);

    // Wait for shutdown signal
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("Shutdown signal received, stopping node...");
        }
        Err(err) => {
            warn!("Unable to listen for shutdown signal: {}", err);
        }
    }

    // Graceful shutdown
    info!("Node shutting down...");
    Ok(())
}

