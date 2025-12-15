// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Test loading and executing the compiled WASM calculator module
//!
//! This test verifies that:
//! 1. The WASM module compiles successfully
//! 2. It loads into the WasmRuntime
//! 3. It exports the required functions (init, handle_message, snapshot_state)
//! 4. It can perform calculations correctly

use plexspaces_wasm_runtime::{WasmConfig, WasmRuntime};
use std::path::PathBuf;

#[tokio::test]
async fn test_load_compiled_wasm_calculator() {
    // Path to compiled WASM module
    let wasm_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("wasm-actors/target/wasm32-unknown-unknown/release/calculator_wasm_actor.wasm");

    // Skip test if WASM module doesn't exist (CI might not compile it)
    if !wasm_path.exists() {
        eprintln!(
            "Skipping test: WASM module not found at {:?}. \
             Build it with: cd wasm-actors && cargo build --target wasm32-unknown-unknown --release",
            wasm_path
        );
        return;
    }

    // Read WASM bytes
    let wasm_bytes = std::fs::read(&wasm_path).expect("Failed to read WASM file");

    eprintln!("Loaded WASM module: {} bytes", wasm_bytes.len());
    assert!(wasm_bytes.len() > 1000, "WASM module seems too small");

    // Create WasmRuntime
    let runtime = WasmRuntime::new()
        .await
        .expect("Failed to create WasmRuntime");

    // Load module
    let module = runtime
        .load_module("calculator", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load WASM module");

    eprintln!("Module loaded successfully with hash: {}", module.hash);

    // Instantiate actor
    let actor_id = "calc-001";

    let instance = runtime
        .instantiate(module, actor_id.to_string(), &[], WasmConfig::default(), None)
        .await
        .expect("Failed to instantiate WASM actor");

    eprintln!("Actor instantiated successfully: {}", actor_id);

    // Test calculation request
    let calculation_request = serde_json::json!({
        "operation": "Add",
        "operands": [5.0, 3.0]
    });

    let request_bytes = serde_json::to_vec(&calculation_request).unwrap();

    // Send calculate message
    instance
        .handle_message("test-sender", "calculate", request_bytes)
        .await
        .expect("Failed to handle message");

    eprintln!("Calculation message handled successfully");

    // Snapshot state
    let state_bytes = instance
        .snapshot_state()
        .await
        .expect("Failed to snapshot state");

    eprintln!("State snapshot: {} bytes", state_bytes.len());

    // Deserialize and verify state
    if !state_bytes.is_empty() {
        let state: serde_json::Value = serde_json::from_slice(&state_bytes)
            .expect("Failed to deserialize state");

        eprintln!("State: {:?}", state);

        // Verify calculation count increased
        assert_eq!(
            state["calculation_count"], 1,
            "Calculation count should be 1"
        );
        assert_eq!(
            state["last_result"], 8.0,
            "Last result should be 8.0 (5+3)"
        );
    }
}

#[tokio::test]
async fn test_wasm_calculator_multiple_operations() {
    let wasm_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("wasm-actors/target/wasm32-unknown-unknown/release/calculator_wasm_actor.wasm");

    if !wasm_path.exists() {
        eprintln!("Skipping test: WASM module not compiled yet");
        return;
    }

    let wasm_bytes = std::fs::read(&wasm_path).expect("Failed to read WASM file");

    let runtime = WasmRuntime::new().await.unwrap();
    let module = runtime
        .load_module("calculator", "1.0.0", &wasm_bytes)
        .await
        .unwrap();

    let instance = runtime
        .instantiate(module, "calc-002".to_string(), &[], WasmConfig::default(), None)
        .await
        .unwrap();

    // Test multiple operations
    let operations = vec![
        (("Add", vec![10.0, 5.0]), 15.0),
        (("Subtract", vec![20.0, 8.0]), 12.0),
        (("Multiply", vec![3.0, 4.0]), 12.0),
        (("Divide", vec![15.0, 3.0]), 5.0),
    ];

    for ((op, operands), expected) in operations {
        let request = serde_json::json!({
            "operation": op,
            "operands": operands
        });

        let request_bytes = serde_json::to_vec(&request).unwrap();

        instance
            .handle_message("test", "calculate", request_bytes)
            .await
            .expect(&format!("Failed to execute {}", op));

        let state_bytes = instance.snapshot_state().await.unwrap();

        if !state_bytes.is_empty() {
            let state: serde_json::Value = serde_json::from_slice(&state_bytes).unwrap();
            assert_eq!(
                state["last_result"], expected,
                "{} {:?} should equal {}",
                op, operands, expected
            );
        }
    }

    // Verify all 4 operations were counted
    let final_state_bytes = instance.snapshot_state().await.unwrap();
    if !final_state_bytes.is_empty() {
        let state: serde_json::Value = serde_json::from_slice(&final_state_bytes).unwrap();
        assert_eq!(state["calculation_count"], 4, "Should have 4 calculations");
    }
}
