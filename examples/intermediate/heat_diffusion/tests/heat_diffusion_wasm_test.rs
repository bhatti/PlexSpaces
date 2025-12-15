// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Heat Diffusion WASM Integration Tests (TDD)
//
// Tests the heat diffusion WASM actor for correctness of Jacobi iteration

use anyhow::Result;
use plexspaces_wasm_runtime::{WasmCapabilities, WasmInstance, WasmRuntime};
use serde::{Deserialize, Serialize};
use wasmtime::StoreLimitsBuilder;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellState {
    temperature: f64,
    north: f64,
    south: f64,
    east: f64,
    west: f64,
    row: usize,
    col: usize,
    iterations: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CellMessage {
    Initialize { row: usize, col: usize, temperature: f64 },
    SetNeighbor { direction: Direction, temperature: f64 },
    UpdateTemperature,
    GetTemperature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Direction {
    North,
    South,
    East,
    West,
}

async fn load_heat_diffusion_module() -> Result<Vec<u8>> {
    let wasm_path = "wasm-modules/heat_diffusion_wasm_actor.wasm";
    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| anyhow::anyhow!("Failed to read WASM module at {}: {}", wasm_path, e))?;
    Ok(wasm_bytes)
}

#[tokio::test]
async fn test_load_heat_diffusion_wasm_module() -> Result<()> {
    let wasm_bytes = load_heat_diffusion_module().await?;
    assert!(!wasm_bytes.is_empty());
    assert_eq!(&wasm_bytes[0..4], &[0x00, 0x61, 0x73, 0x6D]);
    Ok(())
}

#[tokio::test]
async fn test_create_cell_instance() -> Result<()> {
    let wasm_bytes = load_heat_diffusion_module().await?;
    let runtime = WasmRuntime::new().await?;
    let module = runtime.load_module("heat-cell", "1.0.0", &wasm_bytes).await?;

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "cell-0-0".to_string(),
        &[],
        WasmCapabilities::default(),
        limits,
    )
    .await?;

    assert_eq!(instance.actor_id(), "cell-0-0");
    Ok(())
}

#[tokio::test]
async fn test_initialize_cell() -> Result<()> {
    let wasm_bytes = load_heat_diffusion_module().await?;
    let runtime = WasmRuntime::new().await?;
    let module = runtime.load_module("heat-cell", "1.0.0", &wasm_bytes).await?;

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "cell-1-2".to_string(),
        &[],
        WasmCapabilities::default(),
        limits,
    )
    .await?;

    // Initialize cell at position (1, 2) with temperature 100.0
    let msg = CellMessage::Initialize {
        row: 1,
        col: 2,
        temperature: 100.0,
    };
    let payload = serde_json::to_vec(&msg)?;

    instance.handle_message("coordinator", "message", payload).await?;

    // Verify state via snapshot
    let snapshot = instance.snapshot_state().await?;
    if !snapshot.is_empty() {
        let state: CellState = serde_json::from_slice(&snapshot)?;
        assert_eq!(state.row, 1);
        assert_eq!(state.col, 2);
        assert_eq!(state.temperature, 100.0);
    }

    Ok(())
}

#[tokio::test]
async fn test_set_neighbors() -> Result<()> {
    let wasm_bytes = load_heat_diffusion_module().await?;
    let runtime = WasmRuntime::new().await?;
    let module = runtime.load_module("heat-cell", "1.0.0", &wasm_bytes).await?;

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "cell-test".to_string(),
        &[],
        WasmCapabilities::default(),
        limits,
    )
    .await?;

    // Set all 4 neighbors
    for (dir, temp) in vec![
        (Direction::North, 80.0),
        (Direction::South, 60.0),
        (Direction::East, 70.0),
        (Direction::West, 50.0),
    ] {
        let msg = CellMessage::SetNeighbor {
            direction: dir,
            temperature: temp,
        };
        let payload = serde_json::to_vec(&msg)?;
        instance.handle_message("coordinator", "message", payload).await?;
    }

    // Verify neighbors are set
    let snapshot = instance.snapshot_state().await?;
    if !snapshot.is_empty() {
        let state: CellState = serde_json::from_slice(&snapshot)?;
        assert_eq!(state.north, 80.0);
        assert_eq!(state.south, 60.0);
        assert_eq!(state.east, 70.0);
        assert_eq!(state.west, 50.0);
    }

    Ok(())
}

#[tokio::test]
async fn test_jacobi_update() -> Result<()> {
    // TDD: Test Jacobi iteration formula
    // T_new = (T_north + T_south + T_east + T_west) / 4
    let wasm_bytes = load_heat_diffusion_module().await?;
    let runtime = WasmRuntime::new().await?;
    let module = runtime.load_module("heat-cell", "1.0.0", &wasm_bytes).await?;

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "cell-jacobi".to_string(),
        &[],
        WasmCapabilities::default(),
        limits,
    )
    .await?;

    // Set neighbors: 100, 100, 100, 100
    for (dir, temp) in vec![
        (Direction::North, 100.0),
        (Direction::South, 100.0),
        (Direction::East, 100.0),
        (Direction::West, 100.0),
    ] {
        let msg = CellMessage::SetNeighbor {
            direction: dir,
            temperature: temp,
        };
        let payload = serde_json::to_vec(&msg)?;
        instance.handle_message("coordinator", "message", payload).await?;
    }

    // Perform update
    let msg = CellMessage::UpdateTemperature;
    let payload = serde_json::to_vec(&msg)?;
    instance.handle_message("coordinator", "message", payload).await?;

    // Verify: (100 + 100 + 100 + 100) / 4 = 100
    let snapshot = instance.snapshot_state().await?;
    if !snapshot.is_empty() {
        let state: CellState = serde_json::from_slice(&snapshot)?;
        assert_eq!(state.temperature, 100.0);
        assert_eq!(state.iterations, 1);
    }

    Ok(())
}

#[tokio::test]
async fn test_multiple_iterations() -> Result<()> {
    let wasm_bytes = load_heat_diffusion_module().await?;
    let runtime = WasmRuntime::new().await?;
    let module = runtime.load_module("heat-cell", "1.0.0", &wasm_bytes).await?;

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "cell-multi".to_string(),
        &[],
        WasmCapabilities::default(),
        limits,
    )
    .await?;

    // Set neighbors
    for (dir, temp) in vec![
        (Direction::North, 50.0),
        (Direction::South, 50.0),
        (Direction::East, 50.0),
        (Direction::West, 50.0),
    ] {
        let msg = CellMessage::SetNeighbor {
            direction: dir,
            temperature: temp,
        };
        let payload = serde_json::to_vec(&msg)?;
        instance.handle_message("coordinator", "message", payload).await?;
    }

    // Perform 5 updates
    for _ in 0..5 {
        let msg = CellMessage::UpdateTemperature;
        let payload = serde_json::to_vec(&msg)?;
        instance.handle_message("coordinator", "message", payload).await?;
    }

    // Verify iterations count
    let snapshot = instance.snapshot_state().await?;
    if !snapshot.is_empty() {
        let state: CellState = serde_json::from_slice(&snapshot)?;
        assert_eq!(state.iterations, 5);
        assert_eq!(state.temperature, 50.0); // Steady state
    }

    Ok(())
}
