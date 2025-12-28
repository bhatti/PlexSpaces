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

//! Distributed Block-Based Matrix Multiplication
//!
//! This example demonstrates:
//! - TupleSpace for work distribution and result gathering
//! - Actor-based workers using ActorBuilder
//! - ConfigBootstrap for configuration
//! - CoordinationComputeTracker for metrics

use matrix_multiply::*;

use plexspaces_node::{NodeBuilder, ConfigBootstrap, CoordinationComputeTracker};
use plexspaces_actor::{ActorRef, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_core::RequestContext;
use plexspaces_tuplespace::TupleSpace;
use plexspaces_mailbox::Message;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== Distributed Block-Based Matrix Multiplication ===\n");

    // Load configuration using ConfigBootstrap
    let config: MatrixMultiplyConfig = ConfigBootstrap::load().unwrap_or_default();
    let matrix_size = config.matrix_size;
    let block_size = config.block_size;
    let num_workers = config.num_workers;

    // Validate configuration
    if matrix_size % block_size != 0 {
        anyhow::bail!(
            "Matrix size ({}) must be divisible by block size ({})",
            matrix_size,
            block_size
        );
    }

    let num_blocks_per_dim = matrix_size / block_size;
    let total_blocks = num_blocks_per_dim * num_blocks_per_dim;

    info!("Configuration:");
    info!("  Matrix A: {}×{}", matrix_size, matrix_size);
    info!("  Matrix B: {}×{}", matrix_size, matrix_size);
    info!("  Block size: {}×{}", block_size, block_size);
    info!("  Num blocks: {} ({}×{} grid)", total_blocks, num_blocks_per_dim, num_blocks_per_dim);
    info!("  Workers:  {}\n", num_workers);

    // Create node using NodeBuilder
    let node = NodeBuilder::new("matrix-node")
        .build()
        .await;

    // Create TupleSpace for coordination
    let space = Arc::new(TupleSpace::with_tenant_namespace("internal", "system"));

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("matrix-multiply".to_string());

    // Create and spawn worker actors
    info!("=== Creating Worker Actors ===");
    let mut worker_refs = Vec::new();
    for worker_id in 0..num_workers {
        let worker_behavior = WorkerBehavior::new(space.clone(), format!("worker-{}", worker_id));
        let behavior = Box::new(worker_behavior);
        
        // Build and spawn actor using ActorBuilder::spawn() which uses ActorFactory internally
        let ctx = plexspaces_core::RequestContext::internal();
        let actor_id = format!("worker-{}@{}", worker_id, node.id().as_str());
        let actor_factory: Arc<plexspaces_actor::actor_factory_impl::ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| anyhow::anyhow!("ActorFactory not found"))?;
        let _message_sender = actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "GenServer", // actor_type
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![], // facets
        ).await
            .map_err(|e| anyhow::anyhow!("Failed to spawn actor: {}", e))?;
        let actor_ref = plexspaces_actor::ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        );
        worker_refs.push(actor_ref);
        info!("  Created worker actor: worker-{}", worker_id);
    }

    // Wait for actors to initialize
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Start workers processing
    info!("\n=== Starting Workers ===");
    for actor_ref in &worker_refs {
        let msg = WorkerMessage::Start;
        let payload = serde_json::to_vec(&msg)?;
        let message = Message::new(payload)
            .with_message_type("Start".to_string());
        
        actor_ref.tell(message).await?;
    }
    info!("  All workers started\n");

    // Wait a bit for workers to be ready
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Create and spawn master actor
    info!("=== Creating Master Actor ===");
    let master_behavior = MasterBehavior::new(space.clone());
    let behavior = Box::new(master_behavior);
    
    // Spawn master using ActorFactory
    let master_id = format!("master@{}", node.id().as_str());
    let ctx = RequestContext::internal();
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| anyhow::anyhow!("ActorFactory not found"))?;
    let _message_sender = actor_factory.spawn_actor(
        &ctx,
        &master_id,
        "GenServer", // actor_type
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![], // facets
    ).await
        .map_err(|e| anyhow::anyhow!("Failed to spawn master: {}", e))?;
    let master_ref = ActorRef::remote(
        master_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );
    info!("  Created master actor\n");

    // Wait for master to initialize
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Start computation
    info!("=== Starting Matrix Multiplication ===");
    metrics_tracker.start_coordinate();
    
    let msg = MasterMessage::Start {
        matrix_size,
        block_size,
    };
    let payload = serde_json::to_vec(&msg)?;
    let message = Message::new(payload)
        .with_message_type("Start".to_string());
    
    master_ref.tell(message).await?;

    // Wait for computation to complete
    // The master will handle coordination, so we track it
    metrics_tracker.end_coordinate();

    // Wait for computation to finish (master handles everything)
    // In a real implementation, we might wait for a completion message
    let max_wait_time = std::time::Duration::from_secs(60);
    let start_wait = std::time::Instant::now();
    
    loop {
        // Check if all results are in TupleSpace
        let mut all_done = true;
        for block_i in 0..num_blocks_per_dim {
            for block_j in 0..num_blocks_per_dim {
                let pattern = plexspaces_tuplespace::Pattern::new(vec![
                    plexspaces_tuplespace::PatternField::Exact(
                        plexspaces_tuplespace::TupleField::String("result".to_string())
                    ),
                    plexspaces_tuplespace::PatternField::Exact(
                        plexspaces_tuplespace::TupleField::Integer(block_i as i64)
                    ),
                    plexspaces_tuplespace::PatternField::Exact(
                        plexspaces_tuplespace::TupleField::Integer(block_j as i64)
                    ),
                    plexspaces_tuplespace::PatternField::Wildcard,
                ]);
                
                if !space.exists(pattern).await? {
                    all_done = false;
                    break;
                }
            }
            if !all_done {
                break;
            }
        }
        
        if all_done {
            info!("\n✅ All blocks computed!");
            break;
        }
        
        if start_wait.elapsed() > max_wait_time {
            error!("\n❌ Timeout waiting for computation to complete");
            anyhow::bail!("Computation timeout");
        }
        
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Finalize metrics and print
    let metrics = metrics_tracker.finalize();
    info!("\n📊 Coordination vs Compute Metrics:");
    info!("  Compute time: {:.2}ms", metrics.compute_duration_ms);
    info!("  Coordination time: {:.2}ms", metrics.coordinate_duration_ms);
    info!("  Granularity ratio: {:.2}×", metrics.granularity_ratio);
    info!("  Efficiency: {:.2}%", metrics.efficiency * 100.0);
    info!("  Message count: {}", metrics.message_count);
    info!("  Barrier count: {}", metrics.barrier_count);

    Ok(())
}
