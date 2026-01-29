// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Genomics DNA Sequencing Pipeline - Standalone Runner
//!
//! Simple standalone runner for development and testing.
//! For production deployment, use `genomics-node` binary instead.

use anyhow::Result;
use clap::Parser;
use genomics_pipeline::{
    coordinator::{CoordinatorMessage, GenomicsCoordinator},
    models::Sample,
    GenomicsPipelineConfig,
};
use plexspaces_mailbox::Message;
use plexspaces_node::{ConfigBootstrap, CoordinationComputeTracker, NodeBuilder};
use plexspaces_actor::{ActorRef, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_core::RequestContext;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber;

/// Command line arguments
#[derive(Parser, Debug)]
#[command(name = "genomics-pipeline")]
#[command(about = "Genomics DNA Sequencing Pipeline - Standalone Runner")]
struct Args {
    /// Sample ID to process
    #[arg(long)]
    sample_id: Option<String>,

    /// Path to FASTQ file
    #[arg(long)]
    fastq: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Genomics DNA Sequencing Pipeline Example                  ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Parse command line arguments
    let args = Args::parse();

    // Load configuration using ConfigBootstrap
    let config: GenomicsPipelineConfig = ConfigBootstrap::load().unwrap_or_default();

    info!("📋 Configuration:");
    info!("   Worker Pools:");
    info!("     QC: {}", config.worker_pools.qc);
    info!("     Alignment: {}", config.worker_pools.alignment);
    info!("     Chromosome: {}", config.worker_pools.chromosome);
    info!("     Annotation: {}", config.worker_pools.annotation);
    info!("     Report: {}", config.worker_pools.report);
    println!();

    // Create node using NodeBuilder
    let node = Arc::new(
        NodeBuilder::new("genomics-pipeline-node")
            .with_listen_addr("0.0.0.0:9000")
            .build().await
            .await,
    );
    info!("✅ Node created: genomics-pipeline-node");
    println!();

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("genomics-pipeline".to_string());

    // Spawn coordinator
    info!("🎭 Spawning coordinator...");
    metrics_tracker.start_coordinate();
    let coordinator_id = format!("coordinator@{}", node.id().as_str());
    let actor_factory: Arc<plexspaces_actor::actor_factory_impl::ActorFactoryImpl> = node.service_locator().actor_factory_impl().await
        .ok_or_else(|| anyhow::anyhow!("ActorFactory not found in ServiceLocator"))?;
    let ctx = plexspaces_core::RequestContext::new_without_auth("internal".to_string(), "system".to_string()).with_internal(true).with_admin(true);
    let _message_sender = actor_factory.spawn_actor(
        &ctx,
        &coordinator_id,
        "GenServer", // actor_type
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![], // facets
    ).await
    .map_err(|e| anyhow::anyhow!("Failed to spawn coordinator: {}", e))?;
    let coordinator_ref = plexspaces_actor::ActorRef::remote(
        coordinator_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );
    metrics_tracker.end_coordinate();
    info!("✅ Coordinator spawned: {}", coordinator_ref.id());
    println!();

    // Submit sample if provided
    if let (Some(sample_id), Some(fastq_path)) = (args.sample_id, args.fastq) {
        let sample = Sample {
            sample_id: sample_id.clone(),
            fastq_path,
            metadata: HashMap::new(),
        };

        let coordinator_msg = CoordinatorMessage::ProcessSample(sample);
        let payload = serde_json::to_vec(&coordinator_msg)?;
        let message = Message::new(payload);

        metrics_tracker.start_coordinate();
        coordinator_ref.tell(message).await?;
        metrics_tracker.end_coordinate();

        info!("✅ Sample submitted: {}", sample_id);
        println!();

        // Wait for processing
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    } else {
        info!("ℹ️  No sample provided. Use --sample-id and --fastq to process a sample.");
        info!("   Example: cargo run -- --sample-id SAMPLE001 --fastq /path/to/sample.fastq");
        println!();
    }

    // Display metrics
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let metrics = metrics_tracker.finalize();
    println!("📊 Performance Metrics:");
    println!("  Coordination time: {:.2} ms", metrics.coordinate_duration_ms);
    println!("  Compute time: {:.2} ms", metrics.compute_duration_ms);
    println!("  Total time: {:.2} ms", metrics.total_duration_ms);
    println!("  Granularity ratio: {:.2}x", metrics.granularity_ratio);
    println!("  Efficiency: {:.2}%", metrics.efficiency * 100.0);
    println!("  Messages: {}", metrics.message_count);
    println!("  Barriers: {}", metrics.barrier_count);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║              Example Complete                                  ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("💡 For production deployment, use:");
    println!("   cargo run --bin genomics-node");
    println!();

    Ok(())
}
