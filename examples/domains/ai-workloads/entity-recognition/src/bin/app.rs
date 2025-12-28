// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Application binary for entity recognition example.
//!
//! Orchestrates the entity recognition workflow:
//! 1. Spawns Loader, Processor, and Aggregator actors
//! 2. Sends documents to Loader actors
//! 3. Routes processed documents to Processor actors
//! 4. Aggregates results from Processor actors
//! 5. Displays final results

use anyhow::Result;
use clap::Parser;
use entity_recognition::{
    aggregator::{AggregatorBehavior, AggregatorRequest, AggregatorResponse},
    config::EntityRecognitionConfig,
    loader::{LoaderBehavior, LoaderRequest, LoaderResponse},
    processor::{ProcessorBehavior, ProcessorRequest, ProcessorResponse},
};
use plexspaces_core::ActorRef;
use plexspaces_mailbox::Message;
use plexspaces_node::{NodeBuilder, ConfigBootstrap, CoordinationComputeTracker};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, Level};
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "entity-recognition-app")]
#[command(about = "Entity recognition application orchestrator")]
struct Args {
    /// Node address (gRPC endpoint)
    #[arg(long, default_value = "http://localhost:9000")]
    node_addr: String,

    /// Backend type (memory or redis)
    #[arg(long, default_value = "memory")]
    backend: String,

    /// Redis URL (if backend is redis)
    #[arg(long)]
    redis_url: Option<String>,

    /// Documents to process
    #[arg(required = true)]
    documents: Vec<String>,
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

    info!("Starting entity recognition application");
    info!("Node address: {}", args.node_addr);
    info!("Backend: {}", config.backend);
    info!("Documents: {:?}", args.documents);
    info!("Configuration:");
    info!("  Loaders: {}", config.loader_count);
    info!("  Processors: {}", config.processor_count);
    info!("  Aggregators: {}", config.aggregator_count);

    // Create node using NodeBuilder
    let node = NodeBuilder::new("entity-recognition-app")
        .with_listen_address("0.0.0.0:9000")
        .with_clustering_enabled(true)
        .build().await;

    let node = Arc::new(node);
    // Note: We don't call node.start() here because:
    // 1. This is a simple single-node example that doesn't need gRPC server
    // 2. node.start() spawns background tasks (gRPC server, heartbeat) that keep the process alive
    // 3. Actors can be spawned without starting the gRPC server (like nbody example)
    // If you need remote actor communication, uncomment the line below:
    // node.clone().start().await?;

    info!("Node started, spawning actors...");

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("entity-recognition".to_string());
    metrics_tracker.start_coordinate();

    // Spawn actors with resource requirements
    let start_time = Instant::now();

    // Spawn Loader actors (CPU-intensive)
    let mut loader_refs = Vec::new();
    for i in 0..config.loader_count {
        let loader_ref = node.clone()
            .spawn_actor_builder()
            .with_behavior(Box::new(LoaderBehavior::new(args.documents.clone())))
            .with_name(format!("loader-{}", i))
            .with_resource_requirements(
                2.0,                              // CPU cores
                1024 * 1024 * 1024,              // 1GB memory
                0,                                // No disk
                0,                                // No GPU
                HashMap::from([
                    ("workload".to_string(), "cpu-intensive".to_string()),
                    ("service".to_string(), "loader".to_string()),
                ]),
                vec!["entity-recognition-loaders".to_string()],
            )
            .spawn()
            .await?;
        let loader_id = loader_ref.id().clone();
        loader_refs.push(loader_ref);
        info!("Spawned loader actor: {}", loader_id);
    }

    // Spawn Processor actors (GPU-intensive)
    let mut processor_refs = Vec::new();
    for i in 0..config.processor_count {
        let processor_ref = node.clone()
            .spawn_actor_builder()
            .with_behavior(Box::new(ProcessorBehavior::new()))
            .with_name(format!("processor-{}", i))
            .with_resource_requirements(
                1.0,                              // CPU cores
                4 * 1024 * 1024 * 1024,          // 4GB memory
                0,                                // No disk
                1,                                // 1 GPU
                HashMap::from([
                    ("workload".to_string(), "gpu-intensive".to_string()),
                    ("service".to_string(), "processor".to_string()),
                ]),
                vec!["entity-recognition-processors".to_string()],
            )
            .spawn()
            .await?;
        let processor_id = processor_ref.id().clone();
        processor_refs.push(processor_ref);
        info!("Spawned processor actor: {}", processor_id);
    }

    // Spawn Aggregator actors (CPU-intensive)
    let mut aggregator_refs = Vec::new();
    for i in 0..config.aggregator_count {
        let aggregator_ref = node.clone()
            .spawn_actor_builder()
            .with_behavior(Box::new(AggregatorBehavior::new(args.documents.len() as u32)))
            .with_name(format!("aggregator-{}", i))
            .with_resource_requirements(
                2.0,                              // CPU cores
                1024 * 1024 * 1024,              // 1GB memory
                0,                                // No disk
                0,                                // No GPU
                HashMap::from([
                    ("workload".to_string(), "cpu-intensive".to_string()),
                    ("service".to_string(), "aggregator".to_string()),
                ]),
                vec!["entity-recognition-aggregators".to_string()],
            )
            .spawn()
            .await?;
        aggregator_refs.push(aggregator_ref.clone());
        info!("Spawned aggregator actor: {}", aggregator_ref.id());
    }

    metrics_tracker.end_coordinate();
    info!("All actors spawned in {:?}", start_time.elapsed());

    // Process documents using tell() pattern
    // For simplicity, we'll use a fire-and-forget pattern and wait for completion
    info!("Processing {} documents...", args.documents.len());
    let process_start = Instant::now();

    // Send documents to loader actors (round-robin)
    for (idx, document_id) in args.documents.iter().enumerate() {
        let loader_ref = loader_refs[idx % loader_refs.len()].clone();
        
        // Send load request (fire-and-forget)
        let load_request = LoaderRequest::LoadDocument {
            document_id: document_id.clone(),
        };
        let load_msg = Message::new(serde_json::to_vec(&load_request)?);
        loader_ref.tell(load_msg).await?;
        info!("Sent document {} to loader", document_id);
        
        // Small delay to prevent mailbox overflow
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    // Wait a bit for processing to complete
    // In a real implementation, you would use channels or a completion signal
    info!("Waiting for processing to complete...");
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    metrics_tracker.start_compute();
    let process_duration = process_start.elapsed();
    metrics_tracker.end_compute();
    
    info!("Processing completed in {:?}", process_duration);
    info!("Total time: {:?}", start_time.elapsed());

    // Report metrics
    let metrics = metrics_tracker.finalize();
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 Performance Metrics:");
    println!("  Coordination time: {:.2} ms", metrics.coordinate_duration_ms);
    println!("  Compute time: {:.2} ms", metrics.compute_duration_ms);
    println!("  Granularity ratio: {:.2}x", metrics.granularity_ratio);
    println!("  Efficiency: {:.2}%", metrics.efficiency * 100.0);
    println!();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║              Entity Recognition Complete                        ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Note: In a production implementation, use channels or ask() pattern for request-reply");

    // Node will be cleaned up when process exits
    // Since we don't call node.start(), there are no background tasks to stop
    // (gRPC server, heartbeat, scheduler are only started by node.start())
    Ok(())
}

