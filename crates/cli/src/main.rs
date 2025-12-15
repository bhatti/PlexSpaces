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

//! PlexSpaces CLI Tool
//!
//! ## Purpose
//! Command-line tool for managing PlexSpaces framework:
//! - Deploy actors (like AWS Lambda)
//! - Manage Firecracker VMs
//! - Query node status
//!
//! ## Design Philosophy
//! Keep it simple - focus on actor deployment workflow, not low-level WASM module management.
//! Similar to AWS Lambda: `deploy function`, `invoke function`, not `upload code`, `compile module`.

use clap::{Parser, Subcommand};
use anyhow::Result;

mod actor;
mod application;
mod firecracker;
mod node;

#[derive(Parser)]
#[command(name = "plexspaces")]
#[command(about = "PlexSpaces CLI - Manage actors and framework", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Deploy an application (like AWS Lambda deploy)
    Deploy {
        /// Node address (e.g., localhost:9001)
        #[arg(short, long, default_value = "localhost:9001")]
        node: String,
        
        /// Application ID
        #[arg(short, long)]
        app_id: String,
        
        /// Application name
        #[arg(short, long)]
        name: String,
        
        /// Application version
        #[arg(short, long, default_value = "1.0.0")]
        version: String,
        
        /// WASM file path (for WASM applications)
        #[arg(short, long)]
        wasm: Option<String>,
        
        /// Application config file (TOML/JSON)
        #[arg(short, long)]
        config: Option<String>,
        
        /// Release config file (TOML, optional)
        #[arg(short, long)]
        release_config: Option<String>,
    },
    
    /// Deploy an actor (legacy, use Deploy for applications)
    DeployActor {
        /// Node address (e.g., localhost:9001)
        #[arg(short, long, default_value = "localhost:9001")]
        node: String,
        
        /// Actor name
        #[arg(short, long)]
        name: String,
        
        /// WASM file path (for WASM actors)
        #[arg(short, long)]
        wasm: Option<String>,
        
        /// Actor type (wasm, rust, js, go, python)
        #[arg(short, long, default_value = "wasm")]
        r#type: String,
        
        /// Initial state (JSON)
        #[arg(short, long)]
        state: Option<String>,
    },
    
    /// Invoke an actor (like AWS Lambda invoke)
    Invoke {
        /// Node address
        #[arg(short, long, default_value = "localhost:9001")]
        node: String,
        
        /// Actor ID
        #[arg(short, long)]
        actor: String,
        
        /// Message payload (JSON)
        #[arg(short, long)]
        payload: String,
    },
    
    /// List applications on a node
    List {
        /// Node address
        #[arg(short, long, default_value = "localhost:9001")]
        node: String,
    },
    
    /// Undeploy an application (graceful shutdown)
    Undeploy {
        /// Node address
        #[arg(short, long, default_value = "localhost:9001")]
        node: String,
        
        /// Application ID
        #[arg(short, long)]
        app_id: String,
    },
    
    /// Firecracker VM management
    #[command(subcommand)]
    Vm(firecracker::VmCommands),
    
    /// Node status and health
    Status {
        /// Node address
        #[arg(short, long, default_value = "localhost:9001")]
        node: String,
    },
    
    /// Start a PlexSpaces node instance
    Start {
        /// Node ID (unique identifier for this node)
        #[arg(long, default_value = "node-1")]
        node_id: String,
        
        /// Listen address for gRPC server
        #[arg(long, default_value = "0.0.0.0:9001")]
        listen_addr: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Deploy { node, app_id, name, version, wasm, config, release_config } => {
            application::deploy(
                &node,
                &app_id,
                &name,
                &version,
                wasm.as_deref(),
                config.as_deref(),
                release_config.as_deref(),
            ).await
        }
        Commands::DeployActor { node, name, wasm, r#type, state } => {
            actor::deploy(&node, &name, wasm.as_deref(), &r#type, state.as_deref()).await
        }
        Commands::Invoke { node, actor, payload } => {
            actor::invoke(&node, &actor, &payload).await
        }
        Commands::List { node } => {
            application::list(&node).await
        }
        Commands::Undeploy { node, app_id } => {
            application::undeploy(&node, &app_id).await
        }
        Commands::Vm(cmd) => {
            firecracker::handle_vm_command(cmd).await
        }
        Commands::Status { node } => {
            node::status(&node).await
        }
        Commands::Start { node_id, listen_addr } => {
            node::start(&node_id, &listen_addr).await
        }
    }
}

