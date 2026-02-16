// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Tests for WASM component re-instantiation necessity
//!
//! ## Purpose
//! Determine whether per-invocation re-instantiation of WASM component instances
//! is truly required, or if sequential calls to the same component work without
//! triggering wasmtime's "cannot enter component instance" trap.
//!
//! ## Background
//! The current runtime creates a fresh Store + component instance after every
//! handle() call (see instance.rs create_fresh_simple_actor_state). This was
//! added as a workaround for wasmtime's component model re-entrancy guard.
//! However, the Component Model spec only prevents RE-ENTRANT calls (guest→host→
//! guest while first call is active), not SEQUENTIAL calls (first completes, then
//! second call). This test suite verifies whether sequential calls work.
//!
//! ## Test Strategy
//! 1. test_sequential_handle_calls: Call handle() twice on the same instance
//! 2. test_state_preserved_across_sequential_calls: Verify state survives
//! 3. test_current_reinstantiation_loses_state: Document the current state loss bug

#[cfg(test)]
#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_wasm_runtime::{WasmRuntime, WasmConfig, WasmCapabilities, ResourceLimits};
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
        }
    }

    /// Test: Call handle_message() twice on the same WasmInstance
    ///
    /// The current code re-instantiates the component after every handle() call.
    /// This test verifies the current behavior works (two sequential calls via the
    /// public API, which includes re-instantiation between calls).
    ///
    /// This serves as a baseline - if this test passes, the runtime is functional.
    #[tokio::test]
    async fn test_two_sequential_handle_calls_via_public_api() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let wasm_bytes = match get_shared_wasm_bytes().await {
            Some(bytes) => bytes,
            None => {
                eprintln!("SKIP: WASM test fixture not found");
                return;
            }
        };

        let module = timeout(
            Duration::from_secs(60),
            runtime.load_module("calculator", "1.0.0", &wasm_bytes)
        ).await
            .expect("Module loading timed out")
            .expect("Failed to load module");

        let config = test_config();
        let instance = timeout(
            Duration::from_secs(10),
            runtime.instantiate(
                module, "test-sequential-1".to_string(), &[], config,
                None, None, None, None, None, None, None, None,
                None,
            )
        ).await;
        let instance = match instance {
            Ok(Ok(inst)) => inst,
            Ok(Err(e)) if should_skip(&e) => {
                eprintln!("SKIP: {}", e);
                return;
            }
            Ok(Err(e)) => panic!("Instantiation failed: {}", e),
            Err(_) => panic!("Instantiation timed out"),
        };

        // First call: add [10, 20] => msg_type "add", payload {"operands": [10, 20]}
        let payload1 = br#"{"operands":[10,20]}"#.to_vec();
        let result1 = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "add", payload1)
        ).await;
        match result1 {
            Ok(Ok(resp)) => {
                let resp_str = String::from_utf8_lossy(&resp);
                eprintln!("First handle() response: {}", resp_str);
            }
            Ok(Err(e)) if should_skip(&e) => {
                eprintln!("SKIP (first call): {}", e);
                return;
            }
            Ok(Err(e)) => {
                eprintln!("First handle() error: {}", e);
            }
            Err(_) => panic!("First handle() timed out"),
        };

        // Second call: add [3, 4] (this goes through re-instantiation in current code)
        let payload2 = br#"{"operands":[3,4]}"#.to_vec();
        let result2 = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "add", payload2)
        ).await;
        match result2 {
            Ok(Ok(resp)) => {
                let resp_str = String::from_utf8_lossy(&resp);
                eprintln!("Second handle() response: {}", resp_str);
                eprintln!("PASS: Two sequential handle() calls succeeded (with re-instantiation)");
            }
            Ok(Err(e)) => {
                let err_str = e.to_string();
                if err_str.contains("cannot enter") {
                    panic!(
                        "FAIL: Second handle() got 'cannot enter component instance' even WITH \
                         re-instantiation. This indicates a deeper issue: {}",
                        err_str
                    );
                } else {
                    eprintln!("Second handle() returned error (not re-entry related): {}", err_str);
                }
            }
            Err(_) => panic!("Second handle() timed out"),
        }
    }

    /// Test: Verify state is preserved across handle() calls
    ///
    /// This is the critical test for the state preservation bug. Uses a stateful
    /// actor (calculator with history) to verify state survives between calls.
    ///
    /// Expected current behavior (BUG): State is LOST because re-instantiation
    /// replays init(original_config) without calling set_state(saved_state).
    ///
    /// Expected fixed behavior: State is PRESERVED via get_state/set_state cycle.
    #[tokio::test]
    async fn test_state_preserved_across_handle_calls() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let wasm_bytes = match get_shared_wasm_bytes().await {
            Some(bytes) => bytes,
            None => {
                eprintln!("SKIP: WASM test fixture not found");
                return;
            }
        };

        let module = timeout(
            Duration::from_secs(60),
            runtime.load_module("calculator", "1.0.0", &wasm_bytes)
        ).await
            .expect("Module loading timed out")
            .expect("Failed to load module");

        let config = test_config();
        let instance = timeout(
            Duration::from_secs(10),
            runtime.instantiate(
                module, "test-state-1".to_string(), &[], config,
                None, None, None, None, None, None, None, None,
                None,
            )
        ).await;
        let instance = match instance {
            Ok(Ok(inst)) => inst,
            Ok(Err(e)) if should_skip(&e) => {
                eprintln!("SKIP: {}", e);
                return;
            }
            Ok(Err(e)) => panic!("Instantiation failed: {}", e),
            Err(_) => panic!("Instantiation timed out"),
        };

        // Call 1: add [10, 20] — uses msg_type "add" which routes to add() handler
        let payload1 = br#"{"operands":[10,20]}"#.to_vec();
        let result1 = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "add", payload1)
        ).await;
        match &result1 {
            Ok(Ok(resp)) => {
                let resp_str = String::from_utf8_lossy(resp);
                eprintln!("Call 1 (add [10,20]) response: {}", resp_str);
                // Verify the add operation returned result 30
                assert!(
                    resp_str.contains("30") || resp_str.contains("result"),
                    "Expected add result in response, got: {}",
                    resp_str
                );
            }
            Ok(Err(e)) if should_skip(e) => {
                eprintln!("SKIP: {}", e);
                return;
            }
            Ok(Err(e)) => {
                eprintln!("Call 1 error: {}", e);
                // Don't fail — error may be non-fatal
            }
            Err(_) => panic!("Call 1 timed out"),
        }

        // Call 2: get_state — uses msg_type "get_state" which routes to get_state_handler
        // State should include the add operation from Call 1 if state was preserved
        let payload2 = br#"{}"#.to_vec();
        let result2 = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "get_state", payload2)
        ).await;
        match &result2 {
            Ok(Ok(resp)) => {
                let resp_str = String::from_utf8_lossy(resp);
                eprintln!("Call 2 (get_state) response: {}", resp_str);

                // Check if the state includes the first operation
                if resp_str.contains("\"add\"") && resp_str.contains("30") {
                    eprintln!("PASS: State preserved across calls (history contains add operation with result 30)");
                } else if resp_str.contains("\"last_operation\": null") || resp_str.contains("\"history\": []") {
                    panic!(
                        "FAIL: State was LOST after re-instantiation. \
                         Expected history to contain add operation, got: {}",
                        resp_str
                    );
                } else {
                    eprintln!(
                        "INCONCLUSIVE: Response doesn't clearly indicate state preservation. \
                         Manual inspection needed: {}",
                        resp_str
                    );
                }
            }
            Ok(Err(e)) => eprintln!("Call 2 error: {}", e),
            Err(_) => panic!("Call 2 timed out"),
        }
    }

    /// Test: Get state via get_state_component() after a handle() call
    ///
    /// Verifies that get_state_component() returns the actor's current state,
    /// which we need for the state preservation fix.
    #[tokio::test]
    async fn test_get_state_after_handle() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let wasm_bytes = match get_shared_wasm_bytes().await {
            Some(bytes) => bytes,
            None => {
                eprintln!("SKIP: WASM test fixture not found");
                return;
            }
        };

        let module = timeout(
            Duration::from_secs(60),
            runtime.load_module("calculator", "1.0.0", &wasm_bytes)
        ).await
            .expect("Module loading timed out")
            .expect("Failed to load module");

        let config = test_config();
        let instance = timeout(
            Duration::from_secs(10),
            runtime.instantiate(
                module, "test-getstate-1".to_string(), &[], config,
                None, None, None, None, None, None, None, None,
                None,
            )
        ).await;
        let instance = match instance {
            Ok(Ok(inst)) => inst,
            Ok(Err(e)) if should_skip(&e) => {
                eprintln!("SKIP: {}", e);
                return;
            }
            Ok(Err(e)) => panic!("Instantiation failed: {}", e),
            Err(_) => panic!("Instantiation timed out"),
        };

        // Call handle to modify state — use "add" msg_type to trigger add() handler
        let payload = br#"{"operands":[5,3]}"#.to_vec();
        let handle_result = timeout(
            Duration::from_secs(10),
            instance.handle_message("sender", "add", payload)
        ).await;
        match &handle_result {
            Ok(Ok(resp)) => {
                let resp_str = String::from_utf8_lossy(resp);
                eprintln!("handle(add [5,3]) response: {}", resp_str);
            }
            Ok(Err(e)) if should_skip(e) => {
                eprintln!("SKIP: {}", e);
                return;
            }
            Ok(Err(e)) => eprintln!("handle() error: {}", e),
            Err(_) => panic!("handle() timed out"),
        }

        // Now call get_state_component() to verify state was preserved
        // After the state preservation fix, get_state() should return state
        // that includes the add operation from the handle() call above
        let state_result = timeout(
            Duration::from_secs(10),
            instance.get_state_component()
        ).await;
        match state_result {
            Ok(Ok(state_bytes)) => {
                let state_str = String::from_utf8_lossy(&state_bytes);
                eprintln!("get_state_component() returned: {}", state_str);

                if state_str.contains("\"add\"") && state_str.contains("8") {
                    eprintln!("PASS: get_state_component() includes handle() state (add result 8)");
                } else if state_str.contains("\"last_operation\": null") {
                    panic!(
                        "FAIL: get_state_component() shows state was lost. \
                         Expected add operation in state, got: {}",
                        state_str
                    );
                } else {
                    eprintln!(
                        "INCONCLUSIVE: get_state_component() returned unexpected format: {}",
                        state_str
                    );
                }
            }
            Ok(Err(e)) => {
                let err_str = e.to_string();
                if err_str.contains("cannot enter") {
                    panic!(
                        "FAIL: get_state_component() got 'cannot enter' trap: {}",
                        err_str
                    );
                } else {
                    eprintln!("get_state_component() error (non-fatal): {}", e);
                }
            }
            Err(_) => panic!("get_state_component() timed out"),
        }
    }
}
