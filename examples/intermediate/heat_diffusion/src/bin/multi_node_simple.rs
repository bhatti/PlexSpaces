// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Simplified Multi-Node Heat Diffusion Example
//
// Demonstrates WASM actor usage for distributed heat diffusion
// without complex actor system requirements.

use anyhow::Result;
use heat_diffusion::config::GridConfig;
use plexspaces_wasm_runtime::{WasmCapabilities, WasmInstance, WasmRuntime};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

/// Cell initialization message
#[derive(Debug, Serialize, Deserialize)]
struct InitMessage {
    row: usize,
    col: usize,
}

/// Set neighbor message
#[derive(Debug, Serialize, Deserialize)]
struct SetNeighborMessage {
    direction: String,
    temperature: f64,
}

/// Cell state
#[derive(Debug, Serialize, Deserialize)]
struct CellState {
    row: usize,
    col: usize,
    temperature: f64,
    north: f64,
    south: f64,
    east: f64,
    west: f64,
    iterations: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("heat_diffusion=info".parse()?),
        )
        .init();

    info!("PlexSpaces Multi-Node Heat Diffusion (Simplified)");

    // Configuration (small grid to avoid wasmtime concurrent memory limits)
    let grid_size = 2;
    let max_iterations = 10;

    let config = GridConfig::new((grid_size, grid_size), (1, 1))?;
    info!(
        "Grid: {}x{}, Regions: {:?}, Actors: {}",
        grid_size,
        grid_size,
        config.region_size,
        config.actor_count()
    );

    // Load WASM module (path relative to cwd)
    let wasm_path = if PathBuf::from("wasm-modules/heat_diffusion_wasm_actor.wasm").exists() {
        // Running from examples/heat_diffusion directory
        PathBuf::from("wasm-modules/heat_diffusion_wasm_actor.wasm")
    } else if PathBuf::from("examples/heat_diffusion/wasm-modules/heat_diffusion_wasm_actor.wasm").exists() {
        // Running from workspace root
        PathBuf::from("examples/heat_diffusion/wasm-modules/heat_diffusion_wasm_actor.wasm")
    } else {
        anyhow::bail!(
            "WASM module not found. Run 'make build-wasm' first.\n\
             Tried:\n  - wasm-modules/heat_diffusion_wasm_actor.wasm\n  \
             - examples/heat_diffusion/wasm-modules/heat_diffusion_wasm_actor.wasm"
        );
    };

    let wasm_bytes = tokio::fs::read(&wasm_path).await?;
    info!("Loaded WASM module: {} bytes", wasm_bytes.len());

    // Create WASM runtime
    let runtime = WasmRuntime::new().await?;
    let module = runtime
        .load_module("heat_diffusion", "1.0.0", &wasm_bytes)
        .await?;

    info!("WASM module compiled successfully");

    // Create cell instances (simplified: all in same process)
    let mut cells = Vec::new();

    for row in 0..grid_size {
        for col in 0..grid_size {
            let actor_id = format!("cell-{}-{}", row, col);

            // Create WASM instance with resource limits
            let limits = wasmtime::StoreLimitsBuilder::new()
                .memory_size(16 * 1024 * 1024)
                .build();

            let instance = WasmInstance::new(
                runtime.engine(),
                module.clone(),
                actor_id.clone(),
                &[],
                WasmCapabilities::default(),
                limits,
                None, // channel_service
            )
            .await?;

            // Initialize cell
            let init_msg = InitMessage { row, col };
            let init_bytes = serde_json::to_vec(&init_msg)?;
            instance
                .handle_message("coordinator", "initialize", init_bytes)
                .await?;

            cells.push((row, col, instance));
        }
    }

    info!("Created {} WASM cell instances", cells.len());

    // Set boundary conditions
    for col in 0..grid_size {
        // Top row: 100°C
        let cell = cells
            .iter()
            .find(|(r, c, _)| *r == 0 && *c == col)
            .map(|(_, _, inst)| inst)
            .unwrap();

        let neighbor_msg = SetNeighborMessage {
            direction: "north".to_string(),
            temperature: 100.0,
        };
        let msg_bytes = serde_json::to_vec(&neighbor_msg)?;
        cell.handle_message("coordinator", "set_neighbor", msg_bytes)
            .await?;

        // Bottom row: 0°C
        let last_row = grid_size - 1;
        let cell = cells
            .iter()
            .find(|(r, c, _)| *r == last_row && *c == col)
            .map(|(_, _, inst)| inst)
            .unwrap();

        let neighbor_msg = SetNeighborMessage {
            direction: "south".to_string(),
            temperature: 0.0,
        };
        let msg_bytes = serde_json::to_vec(&neighbor_msg)?;
        cell.handle_message("coordinator", "set_neighbor", msg_bytes)
            .await?;
    }

    info!("Set boundary conditions");

    // Run iterations
    for iteration in 0..max_iterations {
        // Update all cells
        for (_, _, instance) in &cells {
            let update_msg = serde_json::json!({"type": "UpdateTemperature"});
            let msg_bytes = serde_json::to_vec(&update_msg)?;
            instance
                .handle_message("coordinator", "update", msg_bytes)
                .await?;
        }

        if (iteration + 1) % 5 == 0 {
            info!("Completed iteration {}/{}", iteration + 1, max_iterations);
        }
    }

    // Print final grid
    println!("\nFinal Temperature Grid (after {} iterations):", max_iterations);
    println!("============================================");

    for row in 0..grid_size {
        for col in 0..grid_size {
            let cell = cells
                .iter()
                .find(|(r, c, _)| *r == row && *c == col)
                .map(|(_, _, inst)| inst)
                .unwrap();

            let snapshot = cell.snapshot_state().await?;

            if !snapshot.is_empty() {
                let state: CellState = serde_json::from_slice(&snapshot)?;
                print!("{:6.2} ", state.temperature);
            } else {
                print!("{:6.2} ", 50.0); // Default
            }
        }
        println!();
    }

    info!("\nExample complete");
    info!("This simplified version runs all cells in one process");
    info!("For true multi-node: use TupleSpace coordination (see Phase 3)");

    Ok(())
}
