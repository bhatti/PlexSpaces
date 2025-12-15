// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Dynamic WASM Module Deployment Tests
//
// Demonstrates:
// - Sending WASM bytes to remote nodes
// - Content-addressed caching across multiple nodes
// - Module deduplication via SHA-256 hashing
// - Runtime module loading without restart

use plexspaces_wasm_runtime::{WasmConfig, WasmRuntime};
use std::path::PathBuf;

/// Load WASM bytes from compiled module
fn load_wasm_bytes() -> Vec<u8> {
    let wasm_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("wasm-actors/target/wasm32-unknown-unknown/release/calculator_wasm_actor.wasm");

    if !wasm_path.exists() {
        panic!(
            "WASM module not found at {:?}. Build it with: cd wasm-actors && cargo build --target wasm32-unknown-unknown --release",
            wasm_path
        );
    }

    std::fs::read(&wasm_path).expect("Failed to read WASM file")
}

#[tokio::test]
async fn test_dynamic_module_deployment_to_single_node() {
    eprintln!("\n=== Dynamic Module Deployment (Single Node) ===");

    // Simulate node starting without any modules
    let node = WasmRuntime::new()
        .await
        .expect("Failed to create runtime (node)");

    assert_eq!(
        node.module_count().await,
        0,
        "Node should start with no modules"
    );

    // Simulate receiving WASM bytes over network (e.g., via gRPC)
    let wasm_bytes = load_wasm_bytes();
    eprintln!("Received WASM module: {} bytes", wasm_bytes.len());

    // Deploy module at runtime (no restart needed!)
    let module = node
        .load_module("calculator", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to deploy module");

    eprintln!("✓ Module deployed with hash: {}", module.hash);
    assert_eq!(node.module_count().await, 1, "Module should be cached");

    // Spawn actor from newly deployed module
    let instance = node
        .instantiate(
            module,
            "calculator-001".to_string(),
            &[],
            WasmConfig::default(),
            None,
        )
        .await
        .expect("Failed to spawn actor from deployed module");

    eprintln!("✓ Actor spawned: {}", instance.actor_id());

    // Verify actor works
    let request = serde_json::to_vec(&serde_json::json!({
        "operation": "Add",
        "operands": [10.0, 20.0]
    }))
    .unwrap();

    instance
        .handle_message("test", "calculate", request)
        .await
        .expect("Failed to execute");

    let state = instance.snapshot_state().await.expect("Failed to snapshot");
    let state: serde_json::Value = serde_json::from_slice(&state).unwrap();

    assert_eq!(state["calculation_count"], 1);
    assert_eq!(state["last_result"], 30.0);

    eprintln!("✓ Dynamic deployment successful!");
}

#[tokio::test]
async fn test_content_addressed_caching_across_nodes() {
    eprintln!("\n=== Content-Addressed Caching (Multi-Node) ===");

    // Simulate 3 nodes in a cluster
    let node1 = WasmRuntime::new().await.expect("Failed to create node1");
    let node2 = WasmRuntime::new().await.expect("Failed to create node2");
    let node3 = WasmRuntime::new().await.expect("Failed to create node3");

    let wasm_bytes = load_wasm_bytes();

    // Node 1 receives and deploys module
    eprintln!("Node 1: Deploying module...");
    let module1 = node1
        .load_module("calc", "1.0.0", &wasm_bytes)
        .await
        .expect("Node 1 failed to load");
    eprintln!("  Hash: {}", module1.hash);

    // Node 2 receives same module (simulates deployment to another node)
    eprintln!("Node 2: Deploying same module...");
    let module2 = node2
        .load_module("calc", "1.0.0", &wasm_bytes)
        .await
        .expect("Node 2 failed to load");
    eprintln!("  Hash: {}", module2.hash);

    // Node 3 receives same module
    eprintln!("Node 3: Deploying same module...");
    let module3 = node3
        .load_module("calc", "1.0.0", &wasm_bytes)
        .await
        .expect("Node 3 failed to load");
    eprintln!("  Hash: {}", module3.hash);

    // Verify content-addressed hashing: same content = same hash
    assert_eq!(module1.hash, module2.hash, "Hashes should match");
    assert_eq!(module2.hash, module3.hash, "Hashes should match");
    eprintln!("✓ Content-addressed hashing works: all 3 nodes computed same hash");

    // Simulate cross-node module lookup (by hash)
    let retrieved_from_node1 = node1
        .get_module(&module1.hash)
        .await
        .expect("Module should be cached on node1");
    assert_eq!(retrieved_from_node1.name, "calc");

    let retrieved_from_node2 = node2
        .get_module(&module2.hash)
        .await
        .expect("Module should be cached on node2");
    assert_eq!(retrieved_from_node2.name, "calc");

    eprintln!("✓ Module retrieval by hash works on all nodes");

    // Spawn actors on different nodes from same module
    let actor1 = node1
        .instantiate(
            module1,
            "actor-on-node1".to_string(),
            &[],
            WasmConfig::default(),
            None,
        )
        .await
        .expect("Failed to spawn on node1");

    let actor2 = node2
        .instantiate(
            module2,
            "actor-on-node2".to_string(),
            &[],
            WasmConfig::default(),
            None,
        )
        .await
        .expect("Failed to spawn on node2");

    let actor3 = node3
        .instantiate(
            module3,
            "actor-on-node3".to_string(),
            &[],
            WasmConfig::default(),
            None,
        )
        .await
        .expect("Failed to spawn on node3");

    eprintln!("✓ Spawned actors on all 3 nodes from same module");

    // Verify all actors work correctly
    for (i, actor) in [&actor1, &actor2, &actor3].iter().enumerate() {
        let request = serde_json::to_vec(&serde_json::json!({
            "operation": "Multiply",
            "operands": [(i + 1) as f64, 10.0]
        }))
        .unwrap();

        actor
            .handle_message("test", "calculate", request)
            .await
            .expect("Failed to execute");
    }

    eprintln!("✓ All actors executed successfully");
}

#[tokio::test]
async fn test_module_versioning_and_updates() {
    eprintln!("\n=== Module Versioning & Hot Updates ===");

    let node = WasmRuntime::new().await.expect("Failed to create node");
    let wasm_bytes = load_wasm_bytes();

    // Deploy version 1.0.0
    let v1 = node
        .load_module("calculator", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load v1.0.0");
    eprintln!("✓ Deployed v1.0.0 (hash: {})", v1.hash);

    // Spawn actor from v1
    let actor_v1 = node
        .instantiate(
            v1.clone(),
            "actor-v1".to_string(),
            &[],
            WasmConfig::default(),
            None,
        )
        .await
        .expect("Failed to spawn v1 actor");

    // Simulate receiving updated module (v2.0.0) - same code but different version
    let v2 = node
        .load_module("calculator", "2.0.0", &wasm_bytes)
        .await
        .expect("Failed to load v2.0.0");
    eprintln!("✓ Deployed v2.0.0 (hash: {})", v2.hash);

    // Verify: same bytes = same hash (version is metadata, not content)
    assert_eq!(
        v1.hash, v2.hash,
        "Same WASM bytes should produce same hash"
    );

    // But both modules are in cache (different names/versions)
    assert_eq!(
        node.module_count().await,
        1,
        "Same hash should deduplicate"
    );

    // Old actors keep running on v1, new actors use v2
    let actor_v2 = node
        .instantiate(
            v2,
            "actor-v2".to_string(),
            &[],
            WasmConfig::default(),
            None,
        )
        .await
        .expect("Failed to spawn v2 actor");

    eprintln!("✓ Old actors (v1) and new actors (v2) coexist");
    eprintln!("  Actor v1: {}", actor_v1.actor_id());
    eprintln!("  Actor v2: {}", actor_v2.actor_id());

    // Both actors work correctly
    for actor in [&actor_v1, &actor_v2] {
        let request = serde_json::to_vec(&serde_json::json!({
            "operation": "Add",
            "operands": [5.0, 3.0]
        }))
        .unwrap();

        actor
            .handle_message("test", "calculate", request)
            .await
            .expect("Failed to execute");
    }

    eprintln!("✓ Hot update successful: no downtime, gradual migration");
}

#[tokio::test]
async fn test_hash_verification_security() {
    eprintln!("\n=== Hash Verification (Security) ===");

    let node = WasmRuntime::new().await.expect("Failed to create node");
    let wasm_bytes = load_wasm_bytes();

    // Compute expected hash
    let expected_hash = WasmRuntime::compute_hash(&wasm_bytes);
    eprintln!("Expected hash: {}", expected_hash);

    // Load with verification (hash matches)
    let module = node
        .load_module_verified("calculator", "1.0.0", &wasm_bytes, &expected_hash)
        .await
        .expect("Verification should succeed");

    eprintln!("✓ Module loaded with hash verification");
    assert_eq!(module.hash, expected_hash);

    // Attempt to load with wrong hash (security check)
    let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    let result = node
        .load_module_verified("tampered", "1.0.0", &wasm_bytes, wrong_hash)
        .await;

    assert!(
        result.is_err(),
        "Loading with wrong hash should fail (security)"
    );
    eprintln!("✓ Hash verification prevents tampered modules");

    // Verify module retrieval by hash
    let retrieved = node
        .get_module(&expected_hash)
        .await
        .expect("Should retrieve by hash");
    assert_eq!(retrieved.hash, expected_hash);

    eprintln!("✓ Content-addressed retrieval works");
}

#[tokio::test]
async fn test_module_cache_management() {
    eprintln!("\n=== Module Cache Management ===");

    let node = WasmRuntime::new().await.expect("Failed to create node");
    let wasm_bytes = load_wasm_bytes();

    // Load multiple modules
    node.load_module("calc1", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load");
    node.load_module("calc2", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load");
    node.load_module("calc3", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load");

    // All have same hash (same bytes), so only 1 in cache
    assert_eq!(
        node.module_count().await,
        1,
        "Same hash should deduplicate"
    );

    eprintln!("✓ Module deduplication works: 3 loads → 1 cached module");

    // Clear cache
    node.clear_cache().await;
    assert_eq!(node.module_count().await, 0, "Cache should be empty");
    eprintln!("✓ Cache cleared successfully");

    // Reload after clear
    let module = node
        .load_module("calc-reloaded", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to reload");

    assert_eq!(node.module_count().await, 1);
    eprintln!("✓ Module reloaded after cache clear: {}", module.hash);
}
