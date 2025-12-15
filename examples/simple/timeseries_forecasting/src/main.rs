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

//! Time-Series Forecasting Example
//!
//! This example demonstrates an end-to-end time-series forecasting application
//! using PlexSpaces actors, inspired by Ray's time-series forecasting example.
//!
//! ## Features
//! - Distributed data preprocessing
//! - Model training
//! - Model validation (offline batch inference)
//! - Online model serving

mod data_loader;
mod preprocessor;
mod trainer;
mod validator;
mod server;

use anyhow::Result;
use plexspaces_actor::ActorBuilder;
use plexspaces_core::ActorId;
use plexspaces_node::{ConfigBootstrap, NodeBuilder};
use plexspaces_node::CoordinationComputeTracker;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("timeseries_forecasting=info,plexspaces=warn")
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Time-Series Forecasting Example                               ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    info!("Starting Time-Series Forecasting Example");

    // Load configuration (optional, using defaults for now)
    let _config: serde_json::Value = ConfigBootstrap::load()
        .unwrap_or_else(|_| serde_json::json!({}));

    // Create node (disable clustering to avoid ObjectRegistry heartbeat errors in simple example)
    let node = NodeBuilder::new("timeseries-node-1")
        .with_clustering_enabled(false)
        .build();

    let node_arc = Arc::new(node);
    let _tracker = Arc::new(tokio::sync::Mutex::new(
        CoordinationComputeTracker::new("timeseries-forecasting".to_string())
    ));

    info!("Node created: timeseries-node-1");

    // Start node in background task (start() runs indefinitely)
    let node_for_start = node_arc.clone();
    tokio::spawn(async move {
        if let Err(e) = node_for_start.start().await {
            eprintln!("Node start error: {}", e);
        }
    });
    
    // Give node time to start
    sleep(Duration::from_millis(500)).await;
    info!("Node started");

    // Spawn actors using ActorBuilder
    info!("Spawning actors...");
    
    let data_loader_behavior = Box::new(data_loader::DataLoaderActor::new());
    let data_loader = ActorBuilder::new(data_loader_behavior)
        .with_id(ActorId::from("data-loader@local"))
        .build();
    let _data_loader_ref = node_arc.spawn_actor(data_loader).await?;
    info!("  ✓ Data loader actor spawned");

    let preprocessor_behavior = Box::new(preprocessor::PreprocessorActor::new());
    let preprocessor = ActorBuilder::new(preprocessor_behavior)
        .with_id(ActorId::from("preprocessor@local"))
        .build();
    let _preprocessor_ref = node_arc.spawn_actor(preprocessor).await?;
    info!("  ✓ Preprocessor actor spawned");

    let trainer_behavior = Box::new(trainer::TrainerActor::new());
    let trainer = ActorBuilder::new(trainer_behavior)
        .with_id(ActorId::from("trainer@local"))
        .build();
    let _trainer_ref = node_arc.spawn_actor(trainer).await?;
    info!("  ✓ Trainer actor spawned");

    let validator_behavior = Box::new(validator::ValidatorActor::new());
    let validator = ActorBuilder::new(validator_behavior)
        .with_id(ActorId::from("validator@local"))
        .build();
    let _validator_ref = node_arc.spawn_actor(validator).await?;
    info!("  ✓ Validator actor spawned");

    let server_behavior = Box::new(server::ServerActor::new());
    let server = ActorBuilder::new(server_behavior)
        .with_id(ActorId::from("server@local"))
        .build();
    let _server_ref = node_arc.spawn_actor(server).await?;
    info!("  ✓ Server actor spawned");

    println!();
    info!("All actors spawned");

    // Simulate workflow
    info!("Starting forecasting workflow...");

    // 1. Load data
    info!("Step 1: Loading data...");
    // In a real implementation, we would send messages to actors
    // For now, this is a placeholder demonstrating the architecture

    // 2. Preprocess data
    info!("Step 2: Preprocessing data...");

    // 3. Train model
    info!("Step 3: Training model...");

    // 4. Validate model
    info!("Step 4: Validating model...");

    // 5. Serve predictions
    info!("Step 5: Serving predictions...");

    // Keep running for a bit to demonstrate the workflow
    sleep(Duration::from_secs(2)).await;

    println!();
    info!("Shutting down...");
    node_arc.shutdown(Duration::from_secs(5)).await?;

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Example complete!");
    println!();
    println!("Key Takeaways:");
    println!("  • Distributed data preprocessing using actors");
    println!("  • Model training with actor coordination");
    println!("  • Model validation with batch inference");
    println!("  • Online model serving via actor messages");
    println!();

    Ok(())
}

