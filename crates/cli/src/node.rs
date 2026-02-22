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
use clap::Subcommand;
use plexspaces_node::NodeBuilder;
use plexspaces_proto::node::v1::{
    node_service_client::NodeServiceClient, ConnectNodesRequest, DisconnectNodesRequest,
    ListConnectedNodesRequest,
};
use plexspaces_proto::prost_types;
use plexspaces_proto::system::v1::{
    system_service_client::SystemServiceClient, GetHealthRequest,
};
use std::sync::Arc;
use tokio::signal;
use tonic::transport::Channel;
use tonic::Request;
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
    // Use RUST_LOG environment variable, default to warn level to capture debug logs
    // Note: tracing_subscriber::fmt() writes to stderr by default, which is captured by test script
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
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
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("Loading release config from: {}", release_config_path);
        }
        use plexspaces_node::config::loader::ConfigLoader;
        let loader = ConfigLoader::new();
        match loader.load_release_spec(release_config_path).await {
            Ok(mut spec) => {
                // CRITICAL: Override node.id from CLI args (CLI args take precedence over release config)
                // This ensures actor IDs use the correct node ID (e.g., node1, node2, node3) instead of
                // the hardcoded value in release.yaml (e.g., my-node)
                if let Some(ref mut node_config) = spec.node {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        debug!("Overriding node.id from '{}' to '{}' (from CLI --node-id)", node_config.id, node_id);
                    }
                    node_config.id = node_id.to_string();
                } else {
                    // Create node config if missing
                    spec.node = Some(plexspaces_proto::node::v1::NodeConfig {
                        id: node_id.to_string(),
                        listen_addr: listen_addr.to_string(),
                        ..Default::default()
                    });
                }
                spec
            },
            Err(e) => {
                // FATAL: If release config path was explicitly provided, fail startup
                eprintln!("FATAL: Failed to load release config from '{}': {}", release_config_path, e);
                eprintln!("       Release config loading is mandatory when --release-config is specified.");
                eprintln!("       Fix the configuration file or remove --release-config to use defaults.");
                std::process::exit(1);
            }
        }
    } else {
        // No release config provided - use defaults
        use plexspaces_common::release_config::create_default_release_config;
        create_default_release_config(
            "plexspaces-cluster".to_string(),
            "1.0.0".to_string(),
            node_id.to_string(),
            listen_addr.to_string(),
        ).await
    };
    
    // CRITICAL: Set PLEXSPACES_NODE_ID env var so config_manager::initialize() can use it
    // This ensures consistent node-id usage across all components (ActorRegistry, etc.)
    std::env::set_var("PLEXSPACES_NODE_ID", node_id);
    
    // Initialize config: apply env overrides, set defaults, validate, and log
    use plexspaces_common::config_manager::initialize;
    initialize(&mut spec);
    
    builder = builder.with_release_spec(spec);
    
    // Create node using NodeBuilder (this may call fatal_exit() if security validation fails)
    // Note: build() returns Node directly, not Result - fatal errors are handled via fatal_exit()
    let node = Arc::new(builder.build().await);
    
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

/// Subcommands for node connectivity (connect, disconnect, list).
#[derive(Subcommand)]
pub enum NodeCommands {
    /// Connect to remote nodes by address (same-cluster only).
    Connect {
        /// Node address to run the command against (e.g., localhost:8000)
        #[arg(long, default_value = "localhost:8000")]
        node: String,
        /// Remote node addresses to connect to (e.g., node2:8000, 192.168.1.10:8000)
        #[arg(long, short = 'a', value_delimiter = ',')]
        addresses: Vec<String>,
        /// Optional cluster name for grouping
        #[arg(long, default_value = "")]
        cluster: String,
        /// Timeout in seconds for each connection attempt (default: 5)
        #[arg(long, default_value = "5")]
        timeout_secs: u64,
    },
    /// Disconnect from nodes by node ID.
    Disconnect {
        /// Node address to run the command against
        #[arg(long, default_value = "localhost:8000")]
        node: String,
        /// Node IDs to disconnect (use `node list` to see IDs)
        #[arg(long, short = 'i', value_delimiter = ',')]
        node_ids: Vec<String>,
        /// Notify remote node before disconnecting
        #[arg(long)]
        notify_remote: bool,
    },
    /// List connected nodes.
    List {
        /// Node address to run the command against
        #[arg(long, default_value = "localhost:8000")]
        node: String,
        /// Filter by cluster name
        #[arg(long, default_value = "")]
        cluster: String,
        /// Include health status
        #[arg(long)]
        include_health: bool,
    },
}

/// Dispatches node connectivity subcommands.
pub async fn handle_node_command(cmd: NodeCommands) -> Result<()> {
    match cmd {
        NodeCommands::Connect {
            node,
            addresses,
            cluster,
            timeout_secs,
        } => connect_nodes(&node, &addresses, &cluster, timeout_secs).await,
        NodeCommands::Disconnect {
            node,
            node_ids,
            notify_remote,
        } => disconnect_nodes(&node, &node_ids, notify_remote).await,
        NodeCommands::List {
            node,
            cluster,
            include_health,
        } => list_connected_nodes(&node, &cluster, include_health).await,
    }
}

/// Lists connected nodes via NodeService ListConnectedNodes.
pub async fn list_connected_nodes(
    node_addr: &str,
    cluster: &str,
    include_health: bool,
) -> Result<()> {
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = NodeServiceClient::new(channel);
    let req = Request::new(ListConnectedNodesRequest {
        cluster: cluster.to_string(),
        page_size: 100,
        page_token: String::new(),
        include_health,
    });
    let res = client.list_connected_nodes(req).await.context("ListConnectedNodes failed")?;
    let inner = res.into_inner();

    println!("📋 Connected nodes ({} total):", inner.nodes.len());
    for n in &inner.nodes {
        let cluster_info = n
            .capabilities
            .get("cluster")
            .map(|c| format!(" cluster={}", c))
            .unwrap_or_default();
        println!("   {} @ {}{}", n.node_id, n.node_address, cluster_info);
    }
    Ok(())
}

/// Connects to remote nodes via NodeService ConnectNodes (same-cluster only).
pub async fn connect_nodes(
    node_addr: &str,
    addresses: &[String],
    cluster: &str,
    timeout_secs: u64,
) -> Result<()> {
    if addresses.is_empty() {
        anyhow::bail!("At least one address is required (--addresses)");
    }

    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = NodeServiceClient::new(channel);
    let req = Request::new(ConnectNodesRequest {
        node_addresses: addresses.to_vec(),
        cluster: cluster.to_string(),
        timeout: Some(prost_types::Duration {
            seconds: timeout_secs as i64,
            nanos: 0,
        }),
    });
    let res = client.connect_nodes(req).await.context("ConnectNodes failed")?;
    let inner = res.into_inner();

    if !inner.connected.is_empty() {
        println!("✅ Connected: {:?}", inner.connected);
    }
    if !inner.failed.is_empty() {
        for (addr, err) in &inner.failed {
            eprintln!("❌ {}: {}", addr, err);
        }
    }
    if inner.connected.is_empty() && !inner.failed.is_empty() {
        anyhow::bail!("All connection attempts failed");
    }
    Ok(())
}

/// Disconnects from nodes via NodeService DisconnectNodes.
pub async fn disconnect_nodes(
    node_addr: &str,
    node_ids: &[String],
    notify_remote: bool,
) -> Result<()> {
    if node_ids.is_empty() {
        anyhow::bail!("At least one node ID is required (--node-ids)");
    }

    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = NodeServiceClient::new(channel);
    let req = Request::new(DisconnectNodesRequest {
        node_ids: node_ids.to_vec(),
        notify_remote,
    });
    let res = client.disconnect_nodes(req).await.context("DisconnectNodes failed")?;
    let inner = res.into_inner();

    if !inner.disconnected.is_empty() {
        println!("✅ Disconnected: {:?}", inner.disconnected);
    }
    if !inner.failed.is_empty() {
        for (id, err) in &inner.failed {
            eprintln!("❌ {}: {}", id, err);
        }
    }
    Ok(())
}
