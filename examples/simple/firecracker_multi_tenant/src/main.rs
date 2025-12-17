// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Firecracker Multi-Tenant Example
//!
//! Demonstrates application-level isolation using Firecracker microVMs
//! for multi-tenant deployments with coordination vs compute metrics.

use anyhow::Result;
use clap::{Parser, Subcommand};
use plexspaces_firecracker::{ApplicationDeployment, FirecrackerVm, VmConfig, VmRegistry, VmState};
use plexspaces_node::{ConfigBootstrap, CoordinationComputeTracker};
use std::collections::HashMap;
use tracing::{info, Level};
use tracing_subscriber;
use ulid::Ulid;

#[derive(Parser)]
#[command(name = "firecracker-multi-tenant")]
#[command(about = "Firecracker Multi-Tenant Example - Application-Level Isolation with Metrics")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Deploy tenant application to Firecracker VM
    Deploy {
        /// Tenant ID
        #[arg(long)]
        tenant: String,

        /// VM vCPU count
        #[arg(long, default_value = "1")]
        vcpus: u32,

        /// VM memory in MiB
        #[arg(long, default_value = "256")]
        memory: u32,
    },
    /// List running tenants
    List,
    /// Stop tenant application
    Stop {
        /// Tenant ID
        #[arg(long)]
        tenant: String,
    },
    /// Get tenant status
    Status {
        /// Tenant ID
        #[arg(long)]
        tenant: String,
    },
    /// Run example workload with metrics
    Run {
        /// Number of tenants to simulate
        #[arg(long, default_value = "3")]
        tenants: u32,

        /// Number of operations per tenant
        #[arg(long, default_value = "10")]
        operations: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("firecracker_multi_tenant=info,plexspaces=warn")
        .init();

    // Load configuration
    let _config: serde_json::Value = ConfigBootstrap::load()
        .unwrap_or_else(|_| serde_json::json!({}));

    let args = Args::parse();

    info!("Starting Firecracker Multi-Tenant Example");

    match args.command {
        Commands::Deploy { tenant, vcpus, memory } => {
            deploy_tenant(&tenant, vcpus, memory).await?;
        }
        Commands::List => {
            list_tenants().await?;
        }
        Commands::Stop { tenant } => {
            stop_tenant(&tenant).await?;
        }
        Commands::Status { tenant } => {
            status_tenant(&tenant).await?;
        }
        Commands::Run { tenants, operations } => {
            run_example_workload(tenants, operations).await?;
        }
    }

    Ok(())
}

async fn deploy_tenant(tenant_id: &str, vcpus: u32, memory: u32) -> Result<()> {
    info!("Deploying tenant: {}", tenant_id);

    let mut metrics = CoordinationComputeTracker::new(format!("deploy-{}", tenant_id));
    metrics.start_coordinate();

    // Load configuration from release.toml (with environment variable overrides)
    let config = ConfigBootstrap::load().unwrap_or_else(|_| serde_json::json!({}));
    let firecracker_config = config.get("firecracker").and_then(|c| c.as_object());
    let kernel_path = firecracker_config
        .and_then(|c| c.get("kernel_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("/var/lib/firecracker/vmlinux")
        .to_string();
    let rootfs_path = firecracker_config
        .and_then(|c| c.get("rootfs_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("/var/lib/firecracker/rootfs.ext4")
        .to_string();

    // Create VM config for tenant
    let vm_config = VmConfig {
        vm_id: format!("vm-{}", Ulid::new()),
        vcpu_count: vcpus as u8,
        mem_size_mib: memory,
        kernel_image_path: kernel_path,
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
        rootfs: plexspaces_firecracker::config::DriveConfig {
            drive_id: "rootfs".to_string(),
            path_on_host: rootfs_path,
            is_root_device: true,
            is_read_only: false,
        },
    };

    // Deploy application to VM (creates and boots real Firecracker VM)
    let deployment = ApplicationDeployment::new(tenant_id).with_vm_config(vm_config);
    
    info!("Creating and booting Firecracker VM...");
    let mut vm = deployment.deploy().await?;
    metrics.end_coordinate();
    
    let report = metrics.finalize();
    info!("Application deployed to VM for tenant: {}", tenant_id);
    info!("  VM ID: {}", vm.vm_id());
    info!("  VM State: {:?}", vm.state());
    info!("  Coordination time: {} ms", report.coordinate_duration_ms);
    
    // Store VM reference for later cleanup (in production, would use VM registry)
    info!("  VM is running and ready for application deployment");

    Ok(())
}

async fn list_tenants() -> Result<()> {
    info!("Listing running tenants...");

    let mut metrics = CoordinationComputeTracker::new("list-tenants".to_string());
    metrics.start_coordinate();
    let vms = VmRegistry::discover_vms().await?;
    metrics.end_coordinate();
    let report = metrics.finalize();

    if vms.is_empty() {
        info!("No tenants currently running");
    } else {
        info!("Found {} tenant(s):", vms.len());
        for vm in vms {
            info!("  - {}: {:?} ({})", vm.vm_id, vm.state, vm.socket_path);
        }
    }

    info!("Discovery time: {} ms", report.coordinate_duration_ms);
    Ok(())
}

async fn stop_tenant(tenant_id: &str) -> Result<()> {
    info!("Stopping tenant: {}", tenant_id);

    let mut metrics = CoordinationComputeTracker::new(format!("stop-{}", tenant_id));
    metrics.start_coordinate();

    // Find VM by tenant ID (in production, would use tenant->VM mapping)
    let entry = VmRegistry::find_vm(tenant_id).await?;

    if let Some(vm_entry) = entry {
        // Create VM instance to stop it
        let config = VmConfig {
            vm_id: vm_entry.vm_id.clone(),
            socket_path: vm_entry.socket_path.clone(),
        };

        let mut vm = FirecrackerVm::create(config).await?;
        vm.stop().await?;

        metrics.end_coordinate();
        let report = metrics.finalize();
        info!("Tenant stopped: {}", tenant_id);
        info!("Stop time: {} ms", report.coordinate_duration_ms);
    } else {
        info!("Tenant not found: {}", tenant_id);
    }

    Ok(())
}

async fn status_tenant(tenant_id: &str) -> Result<()> {
    info!("Getting status for tenant: {}", tenant_id);

    let mut metrics = CoordinationComputeTracker::new(format!("status-{}", tenant_id));
    metrics.start_coordinate();
    let entry = VmRegistry::find_vm(tenant_id).await?;
    metrics.end_coordinate();
    let report = metrics.finalize();

    if let Some(vm_entry) = entry {
        info!("Tenant: {}", tenant_id);
        info!("  VM ID: {}", vm_entry.vm_id);
        info!("  State: {:?}", vm_entry.state);
        info!("  Socket: {}", vm_entry.socket_path);
        info!("  Status query time: {} ms", report.coordinate_duration_ms);
    } else {
        info!("Tenant not found: {}", tenant_id);
    }

    Ok(())
}

async fn run_example_workload(num_tenants: u32, operations_per_tenant: u32) -> Result<()> {
    info!("Running example workload: {} tenants, {} operations each", num_tenants, operations_per_tenant);

    let mut tenant_metrics: HashMap<String, plexspaces_proto::metrics::v1::CoordinationComputeMetrics> = HashMap::new();

    // Simulate tenant deployments and operations
    for tenant_num in 0..num_tenants {
        let tenant_id = format!("tenant-{}", tenant_num);
        let mut metrics = CoordinationComputeTracker::new(tenant_id.clone());

        info!("Processing tenant: {}", tenant_id);

        // Simulate VM deployment (coordination)
        metrics.start_coordinate();
        // In real implementation: deploy VM
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await; // Simulate deployment
        metrics.end_coordinate();

        // Simulate operations (computation)
        for op in 0..operations_per_tenant {
            metrics.start_compute();
            // Simulate actor processing
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await; // Simulate computation
            metrics.end_compute();

            // Simulate message passing (coordination)
            metrics.start_coordinate();
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await; // Simulate message
            metrics.end_coordinate();
            metrics.increment_message();

            if op % 5 == 0 {
                info!("  Tenant {}: Completed operation {}", tenant_id, op);
            }
        }

        let report = metrics.finalize();
        print_tenant_report(&tenant_id, &report);
        // Store report data for aggregate metrics
        tenant_metrics.insert(tenant_id, report);
    }

    // Print aggregate metrics
    let mut total_compute: u64 = 0;
    let mut total_coordinate: u64 = 0;
    let mut total_messages: u64 = 0;

    for (_, report) in tenant_metrics.iter() {
        total_compute += report.compute_duration_ms;
        total_coordinate += report.coordinate_duration_ms;
        total_messages += report.message_count;
    }

    println!("\n=== Aggregate Metrics ===");
    println!("Total Tenants: {}", num_tenants);
    println!("Total Compute Time: {} ms", total_compute);
    println!("Total Coordinate Time: {} ms", total_coordinate);
    println!("Total Messages: {}", total_messages);
    if total_coordinate > 0 {
        println!("Overall Granularity Ratio: {:.2}×", total_compute as f64 / total_coordinate as f64);
    }
    println!("========================\n");

    Ok(())
}

fn print_tenant_report(tenant_id: &str, report: &plexspaces_proto::metrics::v1::CoordinationComputeMetrics) {
    println!("\n=== Firecracker Multi-Tenant Performance Report ===");
    println!("Tenant: {}", tenant_id);
    println!();
    println!("Timing:");
    println!("  Compute Time:     {:>8} ms", report.compute_duration_ms);
    println!("  Coordinate Time:  {:>8} ms", report.coordinate_duration_ms);
    println!("  Total Time:       {:>8} ms", report.total_duration_ms);
    println!();
    println!("Operations:");
    println!("  Messages Sent:    {:>8}", report.message_count);
    println!("  Barrier Count:    {:>8}", report.barrier_count);
    println!();
    println!("Performance:");
    println!("  Granularity Ratio: {:>7.2}× (compute/coordinate)", report.granularity_ratio);

    if report.granularity_ratio < 10.0 {
        println!("    ⚠️  WARNING: Ratio < 10×! Overhead too high!");
    } else if report.granularity_ratio < 100.0 {
        println!("    ✓  Acceptable (>10×), but could be better");
    } else {
        println!("    ✅ Excellent (>100×)! Optimal granularity");
    }

    println!("  Efficiency:        {:>7.1}% (compute/total)", report.efficiency * 100.0);
    println!("==================================================\n");
}
