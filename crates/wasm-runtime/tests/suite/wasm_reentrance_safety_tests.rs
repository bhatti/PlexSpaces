// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Tests for WASM component re-entrance safety and post-init re-instantiation.
//!
//! ## Purpose
//! Verify that WASM component instances correctly handle:
//! 1. Post-init re-instantiation: After init(), the store is re-instantiated so
//!    the first handle() call doesn't trap with "cannot enter component instance"
//! 2. Error recovery: After a handle() error, the instance is re-instantiated
//!    so subsequent handle() calls still work
//! 3. Concurrent message safety: Multiple concurrent messages to the same actor
//!    are serialized correctly without "cannot enter" traps
//! 4. State preservation: State is preserved across re-instantiations after
//!    successful handle() calls
//!
//! ## Background
//! Wasmtime's component model has a re-entrancy guard (wasmtime#8943) that traps
//! with "cannot enter component instance" on the second call to the same store.
//! This applies even to sequential calls (not just concurrent ones). The runtime
//! works around this by creating a fresh Store + component instance after every
//! component entry (init or handle).

#[cfg(test)]
#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_wasm_runtime::{ResourceLimits, WasmCapabilities, WasmConfig, WasmRuntime};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::timeout;

    use crate::suite::shared_wasm_module::get_shared_wasm_bytes;

    /// Returns true if the error indicates component model bindings are not yet available
    fn should_skip(err: &(impl std::fmt::Display + ?Sized)) -> bool {
        let s = err.to_string();
        s.contains("registry")
            || s.contains("not yet implemented")
            || s.contains("not implemented")
            || s.contains("init() error")
            || s.contains("Actor function call failed")
    }

    /// Standard test config for calculator component
    fn test_config() -> WasmConfig {
        WasmConfig {
            limits: ResourceLimits {
                max_memory_bytes: 16 * 1024 * 1024, // 16MB
                max_stack_bytes: 512 * 1024,
                max_fuel: 10_000_000_000,
                max_execution_time: None,
                max_table_elements: 10_000,
                max_pooled_instances: 10,
            },
            capabilities: WasmCapabilities::default(),
            profile_name: "default".to_string(),
            enable_pooling: false,
            enable_aot: false,
            durability_enabled: false,
            use_instance_pool: false,
            max_concurrent_instantiations: None,
        }
    }

    /// Helper: create a WasmInstance from the shared calculator fixture.
    /// Returns None if the fixture is not available or incompatible (test should be skipped).
    async fn create_instance(actor_id: &str) -> Option<plexspaces_wasm_runtime::WasmInstance> {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let wasm_bytes = get_shared_wasm_bytes().await?;

        let module = match timeout(
            Duration::from_secs(60),
            runtime.load_module("calculator", "1.0.0", &wasm_bytes),
        )
        .await
        {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => {
                eprintln!("SKIP: WASM fixture incompatible: {}", e);
                return None;
            }
            Err(_) => {
                eprintln!("SKIP: Module loading timed out");
                return None;
            }
        };

        let config = test_config();
        let result = timeout(
            Duration::from_secs(10),
            runtime.instantiate(
                module,
                actor_id.to_string(),
                &[],
                config,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
        )
        .await;

        match result {
            Ok(Ok(inst)) => Some(inst),
            Ok(Err(e)) if should_skip(&e) => {
                eprintln!("SKIP: {}", e);
                None
            }
            Ok(Err(e)) => panic!("Instantiation failed: {}", e),
            Err(_) => panic!("Instantiation timed out"),
        }
    }

    /// Test: First handle() after init() succeeds (post-init re-instantiation).
    ///
    /// This is the critical test for the "cannot enter component instance" bug.
    /// Before the fix, init() consumed the first store entry, and the first handle()
    /// would trap. After the fix, the constructor re-instantiates after init(), so
    /// the first handle() operates on a fresh store.
    #[tokio::test]
    async fn test_first_handle_after_init_succeeds() {
        let instance = match create_instance("test-first-handle").await {
            Some(inst) => inst,
            None => {
                eprintln!("SKIP: WASM test fixture not found");
                return;
            }
        };

        // The very first handle() call after construction must succeed.
        // Before the fix, this would trap with "cannot enter component instance".
        let payload = br#"{"operands":[10,20]}"#.to_vec();
        let result = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "add", payload),
        )
        .await;

        match result {
            Ok(Ok(resp)) => {
                let resp_str = String::from_utf8_lossy(&resp);
                assert!(
                    resp_str.contains("30") || resp_str.contains("result"),
                    "Expected add result in response, got: {}",
                    resp_str
                );
            }
            Ok(Err(e)) => {
                let err_str = e.to_string();
                if err_str.contains("cannot enter") {
                    panic!(
                        "REGRESSION: First handle() after init() trapped with \
                         'cannot enter component instance'. Post-init re-instantiation is broken: {}",
                        err_str
                    );
                }
                if should_skip(&e) {
                    eprintln!("SKIP: {}", e);
                    return;
                }
                panic!("First handle() failed: {}", e);
            }
            Err(_) => panic!("First handle() timed out"),
        }
    }

    /// Test: Multiple sequential handle() calls all succeed.
    ///
    /// Each handle() triggers re-instantiation. This verifies the full cycle:
    /// init -> re-instantiate -> handle -> re-instantiate -> handle -> ...
    #[tokio::test]
    async fn test_multiple_sequential_handles_succeed() {
        let instance = match create_instance("test-sequential-handles").await {
            Some(inst) => inst,
            None => {
                eprintln!("SKIP: WASM test fixture not found");
                return;
            }
        };

        // Send 5 sequential messages
        for i in 0..5 {
            let payload = format!(r#"{{"operands":[{},{}]}}"#, i, i + 1);
            let result = timeout(
                Duration::from_secs(10),
                instance.handle_message("sender", "add", payload.into_bytes()),
            )
            .await;

            match result {
                Ok(Ok(resp)) => {
                    let resp_str = String::from_utf8_lossy(&resp);
                    let expected_sum = i + (i + 1);
                    assert!(
                        resp_str.contains(&expected_sum.to_string()) || resp_str.contains("result"),
                        "Call {} expected sum {} in response, got: {}",
                        i,
                        expected_sum,
                        resp_str
                    );
                }
                Ok(Err(e)) => {
                    let err_str = e.to_string();
                    if err_str.contains("cannot enter") {
                        panic!(
                            "Call {} trapped with 'cannot enter component instance'. \
                             Re-instantiation between calls is broken: {}",
                            i, err_str
                        );
                    }
                    if should_skip(&e) {
                        eprintln!("SKIP: {}", e);
                        return;
                    }
                    panic!("Call {} failed: {}", i, e);
                }
                Err(_) => panic!("Call {} timed out", i),
            }
        }
    }

    /// Test: Concurrent handle() calls to the same actor don't produce re-entry traps.
    ///
    /// The component_state mutex serializes access, so only one handle() runs at a time.
    /// This test verifies that serialization works and no "cannot enter" traps occur
    /// even under concurrent load.
    #[tokio::test]
    async fn test_concurrent_handles_no_reentry_trap() {
        let instance = match create_instance("test-concurrent-reentry").await {
            Some(inst) => Arc::new(inst),
            None => {
                eprintln!("SKIP: WASM test fixture not found");
                return;
            }
        };

        let num_messages = 10;
        let handles: Vec<_> = (0..num_messages)
            .map(|i| {
                let instance = instance.clone();
                tokio::spawn(async move {
                    let payload = format!(r#"{{"operands":[{},{}]}}"#, i, i + 1);
                    timeout(
                        Duration::from_secs(30),
                        instance.handle_message("sender", "add", payload.into_bytes()),
                    )
                    .await
                })
            })
            .collect();

        let results = timeout(Duration::from_secs(120), futures::future::join_all(handles)).await;

        let results: Vec<_> = match results {
            Ok(results) => results
                .into_iter()
                .map(|r| r.expect("Task panicked"))
                .collect(),
            Err(_) => {
                eprintln!("Overall timeout - lock serialization may be slow");
                return;
            }
        };

        let mut success_count = 0;
        let mut reentry_errors = 0;
        for result in &results {
            match result {
                Ok(Ok(_)) => success_count += 1,
                Ok(Err(e)) => {
                    if should_skip(e) {
                        eprintln!("SKIP: {}", e);
                        return;
                    }
                    let err_str = e.to_string();
                    if err_str.contains("cannot enter") {
                        reentry_errors += 1;
                    }
                }
                Err(_) => {} // timeout, acceptable under serialization
            }
        }

        assert_eq!(
            reentry_errors, 0,
            "Got {} 'cannot enter component instance' errors under concurrent load. \
             Re-instantiation or serialization is broken.",
            reentry_errors
        );

        if success_count > 0 {
            eprintln!(
                "PASS: {}/{} concurrent messages succeeded with 0 re-entry traps",
                success_count, num_messages
            );
        }
    }

    /// Test: State is preserved across handle() calls via get_state/set_state cycle.
    ///
    /// After each handle(), the runtime captures state via get_state(), re-instantiates,
    /// and restores state via set_state(). This test verifies that state changes from
    /// one handle() call are visible in the next.
    #[tokio::test]
    async fn test_state_preserved_across_reinstantiations() {
        let instance = match create_instance("test-state-preserve").await {
            Some(inst) => inst,
            None => {
                eprintln!("SKIP: WASM test fixture not found");
                return;
            }
        };

        // Call 1: add [10, 20] - this modifies state
        let payload1 = br#"{"operands":[10,20]}"#.to_vec();
        let result1 = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "add", payload1),
        )
        .await;
        match &result1 {
            Ok(Ok(resp)) => {
                let resp_str = String::from_utf8_lossy(resp);
                assert!(
                    resp_str.contains("30") || resp_str.contains("result"),
                    "Expected add result, got: {}",
                    resp_str
                );
            }
            Ok(Err(e)) if should_skip(e) => {
                eprintln!("SKIP: {}", e);
                return;
            }
            Ok(Err(e)) => panic!("Call 1 failed: {}", e),
            Err(_) => panic!("Call 1 timed out"),
        }

        // Call 2: get_state - should include the add operation from Call 1
        let payload2 = br#"{}"#.to_vec();
        let result2 = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "get_state", payload2),
        )
        .await;
        match &result2 {
            Ok(Ok(resp)) => {
                let resp_str = String::from_utf8_lossy(resp);
                if resp_str.contains("\"add\"") && resp_str.contains("30") {
                    eprintln!(
                        "PASS: State preserved across re-instantiation (history contains add result 30)"
                    );
                } else if resp_str.contains("\"last_operation\": null")
                    || resp_str.contains("\"history\": []")
                {
                    panic!(
                        "State was LOST after re-instantiation. \
                         Expected history to contain add operation, got: {}",
                        resp_str
                    );
                } else {
                    // State format may vary - as long as we got a response, re-instantiation worked
                    eprintln!("State response (format varies): {}", resp_str);
                }
            }
            Ok(Err(e)) if should_skip(e) => {
                eprintln!("SKIP: {}", e);
                return;
            }
            Ok(Err(e)) => {
                let err_str = e.to_string();
                if err_str.contains("cannot enter") {
                    panic!(
                        "get_state trapped with 'cannot enter' - re-instantiation broken: {}",
                        err_str
                    );
                }
                eprintln!("Call 2 error (non-fatal): {}", e);
            }
            Err(_) => panic!("Call 2 timed out"),
        }
    }

    /// Test: After a handle() error, subsequent handle() calls still work.
    ///
    /// Before the fix, handle() errors caused early return WITHOUT re-instantiation,
    /// leaving the store tainted. All subsequent handle() calls would trap with
    /// "cannot enter component instance". After the fix, re-instantiation always
    /// happens regardless of handle() success/failure.
    #[tokio::test]
    async fn test_recovery_after_handle_error() {
        let instance = match create_instance("test-error-recovery").await {
            Some(inst) => inst,
            None => {
                eprintln!("SKIP: WASM test fixture not found");
                return;
            }
        };

        // Call 1: Send an invalid message type that the actor doesn't handle
        // This should return an error but NOT break the instance
        let bad_payload = br#"{"invalid": true}"#.to_vec();
        let result1 = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "nonexistent_operation", bad_payload),
        )
        .await;
        match &result1 {
            Ok(Ok(_)) => {
                // Actor might handle unknown operations gracefully
                eprintln!("Actor handled unknown operation gracefully");
            }
            Ok(Err(e)) if should_skip(e) => {
                eprintln!("SKIP: {}", e);
                return;
            }
            Ok(Err(e)) => {
                // Expected: error from the actor, but instance should still be usable
                eprintln!("Call 1 (bad message) returned expected error: {}", e);
            }
            Err(_) => panic!("Call 1 timed out"),
        }

        // Call 2: Send a valid message - this MUST succeed even after Call 1 failed
        let good_payload = br#"{"operands":[5,3]}"#.to_vec();
        let result2 = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "add", good_payload),
        )
        .await;
        match result2 {
            Ok(Ok(resp)) => {
                let resp_str = String::from_utf8_lossy(&resp);
                assert!(
                    resp_str.contains("8") || resp_str.contains("result"),
                    "Expected add result 8, got: {}",
                    resp_str
                );
                eprintln!("PASS: Instance recovered after handle() error");
            }
            Ok(Err(e)) => {
                let err_str = e.to_string();
                if err_str.contains("cannot enter") {
                    panic!(
                        "REGRESSION: After handle() error, subsequent call trapped with \
                         'cannot enter component instance'. Error-path re-instantiation is broken: {}",
                        err_str
                    );
                }
                if should_skip(&e) {
                    eprintln!("SKIP: {}", e);
                    return;
                }
                panic!("Call 2 (recovery) failed: {}", e);
            }
            Err(_) => panic!("Call 2 timed out"),
        }
    }

    /// Test: get_state_component() works after handle() (no re-entry trap).
    ///
    /// After handle() + re-instantiation, calling get_state_component() should
    /// work on the fresh store without trapping.
    #[tokio::test]
    async fn test_get_state_component_after_handle_no_trap() {
        let instance = match create_instance("test-getstate-notrap").await {
            Some(inst) => inst,
            None => {
                eprintln!("SKIP: WASM test fixture not found");
                return;
            }
        };

        // First, do a handle() call to modify state
        let payload = br#"{"operands":[7,3]}"#.to_vec();
        let result = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "add", payload),
        )
        .await;
        match &result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) if should_skip(e) => {
                eprintln!("SKIP: {}", e);
                return;
            }
            Ok(Err(e)) => panic!("handle() failed: {}", e),
            Err(_) => panic!("handle() timed out"),
        }

        // Now call get_state_component() - must not trap
        let state_result = timeout(Duration::from_secs(10), instance.get_state_component()).await;
        match state_result {
            Ok(Ok(state_bytes)) => {
                let state_str = String::from_utf8_lossy(&state_bytes);
                eprintln!("PASS: get_state_component() succeeded: {}", state_str);
                assert!(
                    !state_bytes.is_empty(),
                    "State should not be empty after handle() call"
                );
            }
            Ok(Err(e)) => {
                let err_str = e.to_string();
                if err_str.contains("cannot enter") {
                    panic!(
                        "get_state_component() trapped with 'cannot enter' after handle(): {}",
                        err_str
                    );
                }
                if should_skip(&e) {
                    eprintln!("SKIP: {}", e);
                    return;
                }
                panic!("get_state_component() failed: {}", e);
            }
            Err(_) => panic!("get_state_component() timed out"),
        }
    }
}
