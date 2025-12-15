// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! # End-to-End WASM Calculator Test
//!
//! ## Purpose
//! Comprehensive integration test demonstrating PlexSpaces WASM runtime with:
//! - WASM actor compilation and loading
//! - Host function calls (log, send_message)
//! - InstancePool for warm starts
//! - Multi-actor coordination
//! - Durable execution (future)
//!
//! ## Test Scenarios
//! 1. Load WASM module and create instance
//! 2. Call actor's handle_message function
//! 3. Verify host function calls (log output)
//! 4. Test InstancePool performance
//! 5. Multi-actor calculation coordination

use plexspaces_wasm_runtime::{
    InstancePool, WasmCapabilities, WasmConfig, WasmRuntime, ResourceLimits,
};
use std::sync::Arc;

/// Simple calculator WASM module (WebAssembly Text format)
///
/// This module demonstrates:
/// - Memory export for host-guest communication
/// - handle_message function for processing requests
/// - snapshot_state function for state persistence
/// - Host function calls (log, send_message)
const CALCULATOR_WASM: &str = r#"
(module
    ;; Import host functions from PlexSpaces
    (import "plexspaces" "log" (func $log (param i32 i32)))
    (import "plexspaces" "send_message" (func $send_message (param i32 i32 i32 i32) (result i32)))

    ;; Export memory for host-guest communication
    (memory (export "memory") 1)

    ;; Global state: calculation count
    (global $calc_count (mut i32) (i32.const 0))

    ;; handle_message function
    ;; Parameters: from_ptr, from_len, msg_type_ptr, msg_type_len, payload_ptr, payload_len
    ;; Returns: result_ptr (0 for now)
    (func (export "handle_message") (param $from_ptr i32) (param $from_len i32)
                                      (param $msg_type_ptr i32) (param $msg_type_len i32)
                                      (param $payload_ptr i32) (param $payload_len i32)
                                      (result i32)
        ;; Log message receipt
        (call $log
            (i32.const 0)   ;; ptr to "Received message"
            (i32.const 16)) ;; len

        ;; Increment calculation count
        (global.set $calc_count
            (i32.add (global.get $calc_count) (i32.const 1)))

        ;; Return 0 (no response payload for now)
        (i32.const 0)
    )

    ;; snapshot_state function
    ;; Returns: (ptr, len) tuple
    (func (export "snapshot_state") (result i32 i32)
        ;; Return calculation count as state
        ;; For simplicity, return (100, 4) pointing to calc_count
        (i32.const 100)
        (i32.const 4)
    )

    ;; Initialize memory with log message
    (data (i32.const 0) "Received message")
)
"#;

#[tokio::test]
async fn test_load_and_execute_wasm_actor() {
    // Initialize runtime
    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");

    // Compile WASM module from WAT
    let wasm_bytes = wat::parse_str(CALCULATOR_WASM).expect("Failed to parse WAT");

    // Load module
    let module = runtime
        .load_module("calculator-actor", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    println!("✅ Loaded WASM module: {} v{}", module.name, module.version);
    println!("   Module size: {} bytes", module.size_bytes);
    println!("   Module hash: {}", module.hash);

    // Instantiate actor
    let config = WasmConfig {
        limits: ResourceLimits {
            max_memory_bytes: 16 * 1024 * 1024, // 16MB
            max_fuel: 10_000_000,
            ..Default::default()
        },
        capabilities: WasmCapabilities {
            allow_logging: true,
            allow_send_messages: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let instance = runtime
        .instantiate(module, "calc-001".to_string(), &[], config, None)
        .await
        .expect("Failed to instantiate");

    println!("✅ Instantiated WASM actor: {}", instance.actor_id());

    // Call handle_message
    println!("\n📨 Calling handle_message...");
    let result = instance
        .handle_message("test-caller", "calculate", vec![1, 2, 3, 4])
        .await
        .expect("Failed to call handle_message");

    println!("✅ handle_message returned {} bytes", result.len());

    // Call snapshot_state
    println!("\n💾 Calling snapshot_state...");
    let state = instance
        .snapshot_state()
        .await
        .expect("Failed to call snapshot_state");

    println!("✅ snapshot_state returned {} bytes", state.len());
    assert_eq!(state.len(), 4); // Should return 4 bytes (i32 calc_count)
}

#[tokio::test]
async fn test_instance_pool_warm_starts() {
    println!("\n🏊 Testing InstancePool for warm starts...\n");

    // Create runtime and load module
    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let wasm_bytes = wat::parse_str(CALCULATOR_WASM).expect("Failed to parse WAT");
    let module = runtime
        .load_module("calculator-pool", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    // Create instance pool with 5 pre-warmed instances
    let pool_capacity = 5;
    let pool = InstancePool::new(
        runtime.engine(),
        module,
        pool_capacity,
        WasmConfig::default(),
    )
    .await
    .expect("Failed to create pool");

    let stats = pool.stats().await;
    println!("✅ Created InstancePool:");
    println!("   Capacity: {}", pool.capacity());
    println!("   Pre-created instances: {}", stats.total_created);
    println!("   Available: {}", stats.available);

    assert_eq!(stats.total_created, pool_capacity);
    assert_eq!(stats.available, pool_capacity);

    // Measure warm start time
    println!("\n⏱️  Measuring warm start latency...");

    let start = std::time::Instant::now();
    let instance = pool.checkout().await.expect("Failed to checkout");
    let checkout_time = start.elapsed();

    println!("✅ Warm start checkout: {:?}", checkout_time);
    assert!(checkout_time.as_millis() < 10, "Warm start should be < 10ms");

    // Use the instance
    let result = instance
        .instance()
        .handle_message("pool-test", "calculate", vec![5, 6, 7])
        .await
        .expect("Failed to call handle_message");

    println!("✅ Instance executed successfully: {} bytes returned", result.len());

    // Instance returns to pool on drop
    drop(instance);
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let stats = pool.stats().await;
    println!("\n✅ After return to pool:");
    println!("   Available: {}", stats.available);
    println!("   In-use: {}", stats.in_use);
    println!("   Total checkouts: {}", stats.total_checkouts);

    assert_eq!(stats.available, pool_capacity);
    assert_eq!(stats.in_use, 0);
}

#[tokio::test]
async fn test_concurrent_wasm_actors() {
    println!("\n🚀 Testing concurrent WASM actor execution...\n");

    // Create runtime and pool
    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let wasm_bytes = wat::parse_str(CALCULATOR_WASM).expect("Failed to parse WAT");
    let module = runtime
        .load_module("calculator-concurrent", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let pool = Arc::new(
        InstancePool::new(
            runtime.engine(),
            module,
            10,
            WasmConfig::default(),
        )
        .await
        .expect("Failed to create pool"),
    );

    // Spawn 20 concurrent calculations
    let num_tasks = 20;
    let mut handles = vec![];

    println!("📊 Spawning {} concurrent calculations...", num_tasks);

    for i in 0..num_tasks {
        let pool = pool.clone();
        let handle = tokio::spawn(async move {
            let instance = pool.checkout().await.expect("Failed to checkout");

            // Perform calculation
            let payload = vec![i as u8, (i + 1) as u8];
            let result = instance
                .instance()
                .handle_message("concurrent-test", "calculate", payload)
                .await
                .expect("Failed to calculate");

            // Simulate processing time
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

            format!("Task {} completed: {} bytes", i, result.len())
        });
        handles.push(handle);
    }

    // Wait for all tasks
    let start = std::time::Instant::now();
    for handle in handles {
        let result = handle.await.expect("Task failed");
        println!("  ✓ {}", result);
    }
    let total_time = start.elapsed();

    println!("\n✅ All {} tasks completed in {:?}", num_tasks, total_time);
    println!("   Average time per task: {:?}", total_time / num_tasks);

    // Check pool stats
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    let stats = pool.stats().await;

    println!("\n📊 Final pool statistics:");
    println!("   Total checkouts: {}", stats.total_checkouts);
    println!("   Total instances created: {}", stats.total_created);
    println!("   Currently available: {}", stats.available);
    println!("   Currently in-use: {}", stats.in_use);

    assert_eq!(stats.total_checkouts, num_tasks as usize);
    assert_eq!(stats.in_use, 0); // All returned
}

#[tokio::test]
async fn test_wasm_host_function_calls() {
    println!("\n📞 Testing WASM host function calls...\n");

    // WASM module that calls host functions
    let host_call_wasm = r#"
    (module
        (import "plexspaces" "log" (func $log (param i32 i32)))
        (import "plexspaces" "send_message" (func $send_message (param i32 i32 i32 i32) (result i32)))

        (memory (export "memory") 1)

        (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
            ;; Log: "Starting calculation"
            (call $log (i32.const 0) (i32.const 21))

            ;; Send message to "result-collector"
            (call $send_message
                (i32.const 50)   ;; to_ptr: "result-collector"
                (i32.const 16)   ;; to_len
                (i32.const 100)  ;; msg_ptr: "Result: 42"
                (i32.const 10))  ;; msg_len
            drop

            ;; Log: "Calculation complete"
            (call $log (i32.const 22) (i32.const 20))

            (i32.const 0)
        )

        (func (export "snapshot_state") (result i32 i32)
            (i32.const 0)
            (i32.const 0)
        )

        ;; Data section with messages
        (data (i32.const 0) "Starting calculation")
        (data (i32.const 22) "Calculation complete")
        (data (i32.const 50) "result-collector")
        (data (i32.const 100) "Result: 42")
    )
    "#;

    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let wasm_bytes = wat::parse_str(host_call_wasm).expect("Failed to parse WAT");
    let module = runtime
        .load_module("host-call-test", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let instance = runtime
        .instantiate(
            module,
            "host-caller-001".to_string(),
            &[],
            WasmConfig::default(),
            None,
        )
        .await
        .expect("Failed to instantiate");

    println!("✅ Instantiated WASM actor: {}", instance.actor_id());
    println!("\n📝 Calling WASM function (should trigger host function calls)...\n");

    // This will trigger log() and send_message() calls from WASM
    let result = instance
        .handle_message("test", "calculate", vec![])
        .await
        .expect("Failed to call handle_message");

    println!("\n✅ WASM execution completed");
    println!("   Result: {} bytes", result.len());
    println!("\nℹ️  Check output above for log() calls:");
    println!("   - Should see: [WASM:host-caller-001] Starting calculation");
    println!("   - Should see: [WASM:host-caller-001] send_message to=result-collector");
    println!("   - Should see: [WASM:host-caller-001] Calculation complete");
}

#[tokio::test]
async fn test_module_caching() {
    println!("\n🗄️  Testing WASM module caching...\n");

    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let wasm_bytes = wat::parse_str(CALCULATOR_WASM).expect("Failed to parse WAT");

    // Load module first time
    let start = std::time::Instant::now();
    let module1 = runtime
        .load_module("cached-module", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");
    let first_load_time = start.elapsed();

    println!("✅ First load: {:?}", first_load_time);
    println!("   Hash: {}", module1.hash);

    // Load same module again (should use cache)
    let start = std::time::Instant::now();
    let module2 = runtime
        .load_module("cached-module", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");
    let cached_load_time = start.elapsed();

    println!("✅ Cached load: {:?}", cached_load_time);
    println!("   Hash: {}", module2.hash);

    // Verify same hash (same module)
    assert_eq!(module1.hash, module2.hash);

    // Cached load should be faster
    println!("\n📊 Cache performance:");
    println!("   Speedup: {:.2}x faster",
        first_load_time.as_micros() as f64 / cached_load_time.as_micros() as f64);

    // Check module count
    let count = runtime.module_count().await;
    println!("   Cached modules: {}", count);
    assert_eq!(count, 1); // Only one unique module cached
}
