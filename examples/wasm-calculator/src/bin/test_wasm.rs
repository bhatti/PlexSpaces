// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! WASM Calculator Test Binary
//!
//! Tests WASM module loading and execution for calculator actors.
//!
//! ## Usage
//!
//! ```bash
//! # Test WASM module compilation and loading
//! cargo run --bin test-wasm
//! ```

use anyhow::{Context, Result};
use tracing::info;
use tracing_subscriber;

use plexspaces_wasm_runtime::{WasmRuntime, WasmConfig};
use wasm_calculator::{CalculationRequest, Operation};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(true)
        .with_level(true)
        .init();

    info!("WASM Calculator Test");
    info!("====================");
    info!("");

    // Create WASM runtime
    info!("Creating WASM runtime...");
    let runtime = WasmRuntime::with_config(WasmConfig::default())
        .await
        .context("Failed to create WASM runtime")?;
    info!("✓ WASM runtime created");
    info!("");

    // TODO: Load calculator WASM module
    // This will be implemented once we have a compiled WASM module
    info!("TODO: Compile calculator actor to WASM");
    info!("  1. Build: cargo build --target wasm32-wasip2 --release");
    info!("  2. Load module: runtime.load_module(name, version, bytes)");
    info!("  3. Instantiate actor from module");
    info!("");

    // Test calculation models (native Rust for now)
    info!("Testing calculation models (native)...");

    let test_cases = vec![
        ("Add", Operation::Add, vec![10.0, 5.0], 15.0),
        ("Subtract", Operation::Subtract, vec![10.0, 5.0], 5.0),
        ("Multiply", Operation::Multiply, vec![10.0, 5.0], 50.0),
        ("Divide", Operation::Divide, vec![10.0, 5.0], 2.0),
        ("Power", Operation::Power, vec![2.0, 3.0], 8.0),
        ("SquareRoot", Operation::SquareRoot, vec![16.0], 4.0),
    ];

    for (name, op, operands, expected) in test_cases {
        let req = CalculationRequest::new(op, operands, "test".to_string());
        match req.execute() {
            Ok(result) => {
                if (result - expected).abs() < 0.0001 {
                    info!("  ✓ {} = {} (expected: {})", name, result, expected);
                } else {
                    info!("  ✗ {} = {} (expected: {})", name, result, expected);
                }
            }
            Err(e) => {
                info!("  ✗ {} failed: {}", name, e);
            }
        }
    }
    info!("");

    info!("WASM runtime stats:");
    info!("  Module cache size: {} modules", runtime.module_count().await);
    info!("");

    info!("✅ Test complete");
    info!("");
    info!("Next steps:");
    info!("  1. Implement WASM compilation target (wasm32-wasip2)");
    info!("  2. Add WIT bindings for calculator actor");
    info!("  3. Test WASM module instantiation and execution");

    Ok(())
}
