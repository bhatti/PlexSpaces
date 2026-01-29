// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Comparison: V8 Isolates (Cloudflare Workers) vs PlexSpaces
// High-throughput log/metric processing system (ELK/Splunk/Datadog-like)

mod config_service;
mod data_pipeline;
mod io_benchmark;
mod messages;
mod metrics;
mod pipeline_workers;

use plexspaces_node::NodeBuilder;
use std::time::Duration;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    // Default to 'info' level to reduce verbose output
    // Use RUST_LOG=debug for detailed debugging, RUST_LOG=error for minimal output
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  V8 Isolates vs PlexSpaces Comparison                         ║");
    println!("║  High-Throughput Log/Metric Processing System                  ║");
    println!("║  Production-Grade Performance Benchmarks                       ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    info!("=== V8 Isolates vs PlexSpaces Comparison ===");
    info!("Demonstrating high-throughput log/metric processing system with comprehensive benchmarks");

    // Create and initialize node with all services
    // Production-grade: Node is fully initialized after build() - all services available
    let node = NodeBuilder::new("v8-isolates-node-1")
        .build().await
        .await;

    // Initialize services (registers journal storage, actor registry, etc.)
    node.initialize_services().await
        .map_err(|e| format!("Failed to initialize services: {}", e))?;

    // Verify services are ready (ActorService is now registered during initialize_services)
    let service_locator = node.service_locator();
    for _ in 0..10 {
        if service_locator.get_service::<plexspaces_core::ActorRegistry>().await.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    
    // Verify journal storage is available (will be created by initialize_services if not present)
    if service_locator.get_journal_storage().await.is_none() {
        // Register default in-memory journal storage for this example
        use plexspaces_journaling::MemoryJournalStorage;
        use std::sync::Arc;
        use plexspaces_core::JournalStorage;
        let default_storage: Arc<dyn JournalStorage> = Arc::new(MemoryJournalStorage::new());
        service_locator.register_journal_storage(default_storage).await;
        info!("Registered default in-memory journal storage for example");
    }

    // Run control plane demo with metrics
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Control Plane Benchmark");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let control_plane_metrics = config_service::run_control_plane_benchmark(&node).await?;
    control_plane_metrics.print_report();

    // Run data plane demo with metrics
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Data Plane Benchmark");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let data_plane_metrics = data_pipeline::run_data_plane_benchmark(&node).await?;
    data_plane_metrics.print_report();

    // Run I/O performance benchmark - 60 second continuous test
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("I/O Performance Benchmark (60 seconds - High Throughput)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let num_pipelines = std::env::var("NUM_PIPELINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    info!("Running 60-second continuous I/O benchmark simulating real-world Splunk/Datadog workloads");
    info!("Pipeline configuration: {} pipelines (set NUM_PIPELINES env var to change, default: 4)", num_pipelines);
    let io_metrics = io_benchmark::run_io_benchmark(&node, 60).await?;
    io_metrics.print_report();

    // Save metrics to file
    let metrics_json = format!(
        "{}\n{}\n{}",
        control_plane_metrics.to_json(),
        data_plane_metrics.to_json(),
        io_metrics.to_json()
    );
    std::fs::write("metrics/benchmark_results.json", metrics_json)?;
    println!("\n📊 Metrics saved to: metrics/benchmark_results.json");

    println!();
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  ✅ Comparison Complete                                         ║");
    println!("║  ✅ Control Plane: Config service with scalable distribution    ║");
    println!("║  ✅ Data Plane: Pipeline with actors, channels, workflows     ║");
    println!("║  ✅ Production-Grade Performance Metrics Collected              ║");
    println!("╚════════════════════════════════════════════════════════════════╝");

    // Shutdown node to ensure clean exit
    info!("Shutting down node...");
    node.shutdown(Duration::from_secs(10)).await
        .map_err(|e| format!("Failed to shutdown node: {}", e))?;
    info!("Node shutdown complete");

    Ok(())
}

