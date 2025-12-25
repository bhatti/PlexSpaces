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

//! Firecracker VM management commands
//!
//! ## Purpose
//! CLI commands for managing Firecracker VMs:
//! - `start` - Start a Firecracker VM with PlexSpaces framework
//!   - `--empty` - Start empty VM (framework only, no app)
//!   - `--app <WASM_FILE>` - Deploy WASM application to VM via gRPC
//!   - `--node <ADDRESS>` - Node address for app deployment (required with --app)
//! - `stop` - Stop a Firecracker VM
//! - `list` - List running VMs
//! - `status` - Get VM status
//!
//! ## Example Usage
//! ```bash
//! # Start empty VM
//! plexspaces vm start --id vm-001 --empty
//!
//! # Start VM and deploy application
//! plexspaces vm start --id vm-001 \
//!   --app app.wasm \
//!   --node localhost:9001
//! ```

use anyhow::{Context, Result};
use clap::Subcommand;
use plexspaces_firecracker::VmConfig;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum VmCommands {
    /// Start a Firecracker VM
    Start {
        /// VM ID
        #[arg(short, long)]
        id: String,
        
        /// Config file path
        #[arg(short, long)]
        config: Option<PathBuf>,
        
        /// Kernel image path
        #[arg(short, long)]
        kernel: Option<PathBuf>,
        
        /// Root filesystem path
        #[arg(short, long)]
        rootfs: Option<PathBuf>,

        /// Start empty VM (framework only, no app)
        #[arg(long)]
        empty: bool,

        /// Deploy bundled app (WASM file path)
        #[arg(long)]
        app: Option<PathBuf>,

        /// Node address for app deployment (e.g., localhost:9001)
        /// Required when using --app flag
        #[arg(long)]
        node: Option<String>,
    },
    
    /// Stop a Firecracker VM
    Stop {
        /// VM ID
        #[arg(short, long)]
        id: String,
    },
    
    /// List running VMs
    List,
    
    /// Get VM status
    Status {
        /// VM ID
        #[arg(short, long)]
        id: String,
    },
}

pub async fn handle_vm_command(cmd: VmCommands) -> Result<()> {
    match cmd {
        VmCommands::Start { id, config, kernel, rootfs, empty, app, node } => {
            start_vm(&id, config.as_ref(), kernel.as_ref(), rootfs.as_ref(), empty, app.as_ref(), node.as_deref()).await
        }
        VmCommands::Stop { id } => {
            stop_vm(&id).await
        }
        VmCommands::List => {
            list_vms().await
        }
        VmCommands::Status { id } => {
            status_vm(&id).await
        }
    }
}

async fn start_vm(
    vm_id: &str,
    _config_path: Option<&PathBuf>,
    kernel: Option<&PathBuf>,
    rootfs: Option<&PathBuf>,
    empty: bool,
    app: Option<&PathBuf>,
    node_addr: Option<&str>,
) -> Result<()> {
    if empty && app.is_some() {
        return Err(anyhow::anyhow!("Cannot specify both --empty and --app"));
    }

    if app.is_some() && node_addr.is_none() {
        return Err(anyhow::anyhow!("--node address is required when using --app flag"));
    }

    // OBSERVABILITY: Log VM start attempt
    tracing::info!(
        vm_id = %vm_id,
        empty = empty,
        has_app = app.is_some(),
        has_kernel = kernel.is_some(),
        has_rootfs = rootfs.is_some(),
        "Starting Firecracker VM"
    );
    
    if empty {
        println!("🚀 Starting empty Firecracker VM (framework only): {}", vm_id);
    } else if app.is_some() {
        println!("🚀 Starting Firecracker VM with bundled app: {}", vm_id);
    } else {
        println!("🚀 Starting Firecracker VM: {}", vm_id);
    }

    // Load config or use defaults
    let mut vm_config = if let Some(_config_path) = _config_path {
        // TODO: Load from config file (TOML/JSON)
        VmConfig::default()
    } else {
        let mut config = VmConfig::default();
        config.vm_id = vm_id.to_string();
        
        if let Some(kernel) = kernel {
            config.kernel_image_path = kernel.to_string_lossy().to_string();
        }
        
        if let Some(rootfs) = rootfs {
            config.rootfs = plexspaces_firecracker::config::DriveConfig {
                path_on_host: rootfs.to_string_lossy().to_string(),
                ..Default::default()
            };
        }
        
        config
    };

    // Create VM
    let mut vm = plexspaces_firecracker::FirecrackerVm::create(vm_config.clone())
        .await
        .context("Failed to create VM")?;

    println!("   ✓ VM created (state: {:?})", vm.state());

    // Boot VM (this starts Firecracker process and boots the kernel)
    vm.boot()
        .await
        .context("Failed to boot VM")?;

    // OBSERVABILITY: Log successful VM start
    tracing::info!(
        vm_id = %vm_id,
        state = ?vm.state(),
        socket_path = %vm_config.socket_path,
        "Firecracker VM started successfully"
    );
    println!("✅ VM started: {}", vm_id);
    println!("   State: {:?}", vm.state());
    println!("   Socket: {:?}", vm_config.socket_path);

    // Deploy app if specified
    if let Some(app_path) = app {
        let node_addr = node_addr.expect("node address should be validated above");
        deploy_app_to_node(node_addr, vm_id, app_path).await?;
    }

    Ok(())
}

/// Deploy application to node via gRPC DeployApplication
async fn deploy_app_to_node(
    node_addr: &str,
    vm_id: &str,
    wasm_path: &PathBuf,
) -> Result<()> {
    use plexspaces_proto::application::v1::{
        application_service_client::ApplicationServiceClient,
        DeployApplicationRequest, ApplicationSpec, ApplicationType,
    };
    use plexspaces_proto::wasm::v1::WasmModule;
    use std::fs;
    use tonic::transport::Channel;

    // OBSERVABILITY: Log app deployment to node
    tracing::info!(
        vm_id = %vm_id,
        node_addr = %node_addr,
        wasm_path = %wasm_path.display(),
        "Deploying application to node from Firecracker VM"
    );
    println!("   📦 Deploying app to node {}: {}", node_addr, wasm_path.display());

    // Connect to node
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = ApplicationServiceClient::new(channel);

    // Read WASM file
    let wasm_bytes = fs::read(wasm_path)
        .with_context(|| format!("Failed to read WASM file: {}", wasm_path.display()))?;

    // Extract app name from filename (without extension)
    let app_name = wasm_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("app")
        .to_string();

    // Create WASM module
    let wasm_module = WasmModule {
        name: app_name.clone(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes,
        module_hash: String::new(), // Will be computed by server
        ..Default::default()
    };

    // Create minimal application config
    let app_config = ApplicationSpec {
        name: app_name.clone(),
        version: "1.0.0".to_string(),
        description: format!("Application deployed to VM {}", vm_id),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: std::collections::HashMap::new(),
        supervisor: None,
    };

    // Create deployment request
    let request = DeployApplicationRequest {
        application_id: format!("{}-{}", vm_id, app_name),
        name: app_name.clone(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_config),
        release_config: None,
        initial_state: vec![],
    };

    // Deploy application
    let response = client
        .deploy_application(tonic::Request::new(request))
        .await
        .context("Failed to deploy application via gRPC")?
        .into_inner();

    if !response.success {
        anyhow::bail!(
            "Application deployment failed: {}",
            response.error.unwrap_or_else(|| "Unknown error".to_string())
        );
    }

    println!("   ✅ App deployed successfully");
    println!("      Application ID: {}", response.application_id);
    println!("      Status: {:?}", response.status);

    Ok(())
}

async fn stop_vm(vm_id: &str) -> Result<()> {
    // OBSERVABILITY: Log VM stop attempt
    tracing::info!(
        vm_id = %vm_id,
        "Stopping Firecracker VM"
    );
    println!("🛑 Stopping Firecracker VM: {}", vm_id);
    
    // Find VM socket path
    let socket_path = plexspaces_firecracker::VmRegistry::get_vm_socket_path(vm_id)
        .await
        .context("Failed to query VM registry")?
        .ok_or_else(|| anyhow::anyhow!("VM {} not found", vm_id))?;

    // Create config with socket path
    let config = VmConfig {
        vm_id: vm_id.to_string(),
        socket_path: socket_path.clone(),
        ..Default::default()
    };

    // Create VM instance and connect to existing socket
    let mut vm = plexspaces_firecracker::FirecrackerVm::create(config)
        .await
        .context("Failed to create VM instance")?;

    // Connect to existing VM (reuse socket)
    // Note: This requires adding a method to connect to existing VM
    // For now, we'll use the API client directly
    let client = plexspaces_firecracker::FirecrackerApiClient::new(&socket_path);
    client.send_ctrl_alt_del().await
        .context("Failed to send shutdown signal")?;

    // Wait a bit for graceful shutdown
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // OBSERVABILITY: Log successful VM stop
    tracing::info!(
        vm_id = %vm_id,
        "Firecracker VM stopped successfully"
    );
    println!("✅ VM stopped: {}", vm_id);
    Ok(())
}

async fn list_vms() -> Result<()> {
    println!("📋 Listing Firecracker VMs:");

    let vms = plexspaces_firecracker::VmRegistry::discover_vms()
        .await
        .context("Failed to discover VMs")?;

    if vms.is_empty() {
        println!("   No running VMs found");
        return Ok(());
    }

    println!("   Found {} VM(s):", vms.len());
    for vm in vms {
        println!("   - {}: {:?} ({})", vm.vm_id, vm.state, vm.socket_path);
    }

    Ok(())
}

async fn status_vm(vm_id: &str) -> Result<()> {
    println!("📊 VM Status: {}", vm_id);

    let entry = plexspaces_firecracker::VmRegistry::find_vm(vm_id)
        .await
        .context("Failed to query VM registry")?;

    match entry {
        Some(vm) => {
            println!("   State: {:?}", vm.state);
            println!("   Socket: {}", vm.socket_path);
            
            // Try to get detailed info from API
            let client = plexspaces_firecracker::FirecrackerApiClient::new(&vm.socket_path);
            if let Ok(info) = client.get_instance_info().await {
                println!("   Firecracker State: {}", info.state);
                if !info.id.is_empty() {
                    println!("   Instance ID: {}", info.id);
                }
            }
        }
        None => {
            println!("   VM not found (not running or socket not accessible)");
            return Err(anyhow::anyhow!("VM {} not found", vm_id));
        }
    }

    Ok(())
}

