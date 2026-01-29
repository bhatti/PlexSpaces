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
#[cfg(feature = "firecracker")]
mod firecracker;
mod node;
mod security;

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
        /// Node address (e.g., localhost:8000)
        #[arg(long, default_value = "localhost:8000")]
        node: String,
        
        /// Application ID
        #[arg(short = 'i', long)]
        app_id: String,
        
        /// Application name
        #[arg(short = 'n', long)]
        name: String,
        
        /// Application version
        #[arg(short = 'v', long, default_value = "1.0.0")]
        version: String,
        
        /// WASM file path (for WASM applications)
        #[arg(short = 'w', long)]
        wasm: Option<String>,
        
        /// WASM module file path (alias for --wasm)
        #[arg(long)]
        wasm_module: Option<String>,
        
        /// Application config file (TOML/JSON)
        #[arg(short = 'c', long)]
        config: Option<String>,
        
        /// Release config file (TOML, optional)
        #[arg(short = 'r', long)]
        release_config: Option<String>,
    },
    
    /// Deploy an actor (legacy, use Deploy for applications)
    DeployActor {
        /// Node address (e.g., localhost:8000)
        #[arg(long, default_value = "localhost:8000")]
        node: String,
        
        /// Actor name
        #[arg(short = 'n', long)]
        name: String,
        
        /// WASM file path (for WASM actors)
        #[arg(short = 'w', long)]
        wasm: Option<String>,
        
        /// Actor type (wasm, rust, js, go, python)
        #[arg(short = 't', long, default_value = "wasm")]
        r#type: String,
        
        /// Initial state (JSON)
        #[arg(short = 's', long)]
        state: Option<String>,
    },
    
    /// Invoke an actor (like AWS Lambda invoke)
    Invoke {
        /// Node address
        #[arg(short, long, default_value = "localhost:8000")]
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
        #[arg(short, long, default_value = "localhost:8000")]
        node: String,
    },
    
    /// Undeploy an application (graceful shutdown)
    Undeploy {
        /// Node address
        #[arg(short, long, default_value = "localhost:8000")]
        node: String,
        
        /// Application ID
        #[arg(short, long)]
        app_id: String,
    },
    
    /// Firecracker VM management
    #[cfg(feature = "firecracker")]
    #[command(subcommand)]
    Vm(firecracker::VmCommands),
    
    /// Node status and health
    Status {
        /// Node address
        #[arg(short, long, default_value = "localhost:8000")]
        node: String,
    },
    
    /// Start a PlexSpaces node instance
    Start {
        /// Node ID (unique identifier for this node)
        #[arg(long, default_value = "node-1")]
        node_id: String,
        
        /// Listen address for gRPC server
        #[arg(long, default_value = "0.0.0.0:8000")]
        listen_addr: String,
        
        /// Release config file path (YAML format)
        /// If not provided, a default configuration will be created automatically
        #[arg(long, value_name = "FILE")]
        release_config: Option<String>,
    },
    
    /// Generate mTLS certificates for node-to-node authentication
    GenerateMtls {
        /// Output directory for certificates (default: ./certs)
        #[arg(short, long, default_value = "./certs")]
        output: String,
        
        /// CA common name (default: "PlexSpaces CA")
        #[arg(long, default_value = "PlexSpaces CA")]
        ca_common_name: String,
        
        /// Server common name (default: "PlexSpaces Server")
        #[arg(long, default_value = "PlexSpaces Server")]
        server_common_name: String,
        
        /// Validity in days for server certificate (default: 90)
        #[arg(long, default_value = "90")]
        validity_days: u32,
    },
    
    /// Generate default release configuration file
    GenerateReleaseConfig {
        /// Output file path (default: release.yaml)
        #[arg(short, long, default_value = "release.yaml")]
        output: String,
        
        /// Release name (default: "plexspaces-cluster")
        #[arg(long, default_value = "plexspaces-cluster")]
        release_name: String,
        
        /// Release version (default: "1.0.0")
        #[arg(long, default_value = "1.0.0")]
        release_version: String,
        
        /// Node ID (default: "node-1")
        #[arg(long, default_value = "node-1")]
        node_id: String,
        
        /// Listen address (default: "0.0.0.0:8000")
        #[arg(long, default_value = "0.0.0.0:8000")]
        listen_addr: String,
    },
    
    /// Create a JWT token for API authentication (tenant_id, roles, groups, is_admin)
    JwtCreate {
        /// Tenant ID (required for multi-tenancy)
        #[arg(long)]
        tenant_id: String,
        
        /// Subject / user ID (default: "cli-user")
        #[arg(long, default_value = "cli-user")]
        sub: String,
        
        /// Comma-separated roles (e.g. admin,user)
        #[arg(long, value_delimiter = ',')]
        roles: Vec<String>,
        
        /// Comma-separated groups (e.g. team-a,team-b)
        #[arg(long, value_delimiter = ',')]
        groups: Vec<String>,
        
        /// Set is_admin claim to true
        #[arg(long)]
        is_admin: bool,
        
        /// Token validity in hours (default: 24)
        #[arg(long, default_value = "24")]
        exp_hours: u32,
        
        /// JWT secret (default: PLEXSPACES_JWT_SECRET env var)
        #[arg(long, env = "PLEXSPACES_JWT_SECRET")]
        secret: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Deploy { node, app_id, name, version, wasm, wasm_module, config, release_config } => {
            // Support both --wasm and --wasm-module flags
            let wasm_file = wasm.or(wasm_module);
            application::deploy(
                &node,
                &app_id,
                &name,
                &version,
                wasm_file.as_deref(),
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
        #[cfg(feature = "firecracker")]
        Commands::Vm(cmd) => {
            firecracker::handle_vm_command(cmd).await
        }
        Commands::Status { node } => {
            node::status(&node).await
        }
        Commands::Start { node_id, listen_addr, release_config } => {
            node::start(&node_id, &listen_addr, release_config.as_deref()).await
        }
        Commands::GenerateMtls { output, ca_common_name, server_common_name, validity_days } => {
            security::generate_mtls_certificates(
                Some(std::path::PathBuf::from(output)),
                Some(ca_common_name),
                Some(server_common_name),
                Some(validity_days),
            ).await
        }
        Commands::GenerateReleaseConfig { output, release_name, release_version, node_id, listen_addr } => {
            security::generate_release_config(
                Some(std::path::PathBuf::from(output)),
                Some(release_name),
                Some(release_version),
                Some(node_id),
                Some(listen_addr),
            ).await
        }
        Commands::JwtCreate { tenant_id, sub, roles, groups, is_admin, exp_hours, secret } => {
            security::create_jwt_token(tenant_id, sub, roles, groups, is_admin, exp_hours, secret).await
        }
    }
}

