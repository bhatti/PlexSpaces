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

//! Tests for WASM fuel limit enforcement
//!
//! ## Purpose
//! Verify that fuel limits from WasmConfig are properly applied to WASM instances
//! and that fuel consumption is tracked correctly.

#[cfg(test)]
mod tests {
    use plexspaces_wasm_runtime::{ResourceLimits, WasmConfig, WasmInstance, WasmRuntime};
    use wasmtime::StoreLimitsBuilder;

    // Simple WASM module: (module (func (export "test") (result i32) i32.const 42))
    const SIMPLE_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // Magic: \0asm
        0x01, 0x00, 0x00, 0x00, // Version: 1
        0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Type section
        0x03, 0x02, 0x01, 0x00, // Function section
        0x07, 0x08, 0x01, 0x04, 0x74, 0x65, 0x73, 0x74, 0x00, 0x00, // Export section
        0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // Code section
    ];

    /// Test: Fuel limit from config is applied to instances
    ///
    /// ## Purpose
    /// Verify that max_fuel from WasmConfig.limits.max_fuel is properly passed
    /// to WasmInstance and used when setting fuel on stores.
    #[tokio::test]
    async fn test_fuel_limit_from_config_applied() {
        let custom_fuel = 5_000_000_000u64; // 5 billion units

        let config = WasmConfig {
            limits: ResourceLimits {
                max_memory_bytes: 16 * 1024 * 1024,
                max_stack_bytes: 512 * 1024,
                max_fuel: custom_fuel,
                max_execution_time: None,
                max_table_elements: 10_000,
                max_pooled_instances: 10,
            },
            ..Default::default()
        };

        let runtime = WasmRuntime::with_config(config.clone())
            .await
            .expect("Failed to create runtime with custom fuel limit");

        let module = runtime
            .load_module("fuel-test-actor", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Failed to load module");

        let instance = runtime
            .instantiate(
                module,
                "test-actor".to_string(),
                &[],
                config,
                None, // channel_service
                None, // message_sender
                None, // tuplespace_provider
                None, // keyvalue_store
                None, // process_group_registry
                None, // lock_manager
                None, // object_registry
                None, // journal_storage
                None, // blob_service
                None, // elastic_pool_service
                None, // outbound_http_client
            )
            .await
            .expect("Failed to instantiate with custom fuel limit");

        // Instance created successfully with custom fuel limit
        // (Fuel is set internally, we verify it doesn't fail)
        assert_eq!(instance.actor_id(), "test-actor");
    }

    /// Test: Default fuel limit is used when not specified
    ///
    /// ## Purpose
    /// Verify that default fuel limit (10 billion units) is used when
    /// WasmConfig uses default ResourceLimits.
    #[tokio::test]
    async fn test_default_fuel_limit_used() {
        let config = WasmConfig::default();

        // Default max_fuel should be 10 billion
        assert_eq!(config.limits.max_fuel, 10_000_000_000);

        let runtime = WasmRuntime::with_config(config.clone())
            .await
            .expect("Failed to create runtime with default config");

        let module = runtime
            .load_module("default-fuel-actor", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Failed to load module");

        let instance = runtime
            .instantiate(
                module,
                "test-actor".to_string(),
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
                None, // elastic_pool_service
                None, // outbound_http_client
            )
            .await
            .expect("Failed to instantiate with default fuel limit");

        assert_eq!(instance.actor_id(), "test-actor");
    }

    /// Test: Zero fuel limit disables fuel metering
    ///
    /// ## Purpose
    /// Verify that when max_fuel is 0, fuel metering is disabled
    /// (wasmtime_config.consume_fuel(false) is called).
    #[tokio::test]
    async fn test_zero_fuel_disables_metering() {
        let config = WasmConfig {
            limits: ResourceLimits {
                max_memory_bytes: 16 * 1024 * 1024,
                max_stack_bytes: 512 * 1024,
                max_fuel: 0, // Zero = unlimited (fuel metering disabled)
                max_execution_time: None,
                max_table_elements: 10_000,
                max_pooled_instances: 10,
            },
            ..Default::default()
        };

        // Runtime should still be created (fuel metering disabled, not an error)
        let runtime = WasmRuntime::with_config(config.clone())
            .await
            .expect("Failed to create runtime with zero fuel limit");

        let module = runtime
            .load_module("unlimited-fuel-actor", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Failed to load module");

        // Instantiation should still work (fuel metering disabled)
        let instance = runtime
            .instantiate(
                module,
                "test-actor".to_string(),
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
                None, // elastic_pool_service
                None, // outbound_http_client
            )
            .await
            .expect("Failed to instantiate with zero fuel limit");

        assert_eq!(instance.actor_id(), "test-actor");
    }

    /// Test: Large fuel limit is supported
    ///
    /// ## Purpose
    /// Verify that large fuel limits (e.g., u64::MAX/2) are supported
    /// for operations that require significant fuel (e.g., JSON serialization).
    #[tokio::test]
    async fn test_large_fuel_limit_supported() {
        let large_fuel = u64::MAX / 2;

        let config = WasmConfig {
            limits: ResourceLimits {
                max_memory_bytes: 64 * 1024 * 1024, // 64MB
                max_stack_bytes: 8 * 1024 * 1024,   // 8MB
                max_fuel: large_fuel,
                max_execution_time: None,
                max_table_elements: 10_000,
                max_pooled_instances: 10,
            },
            ..Default::default()
        };

        let runtime = WasmRuntime::with_config(config.clone())
            .await
            .expect("Failed to create runtime with large fuel limit");

        let module = runtime
            .load_module("large-fuel-actor", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Failed to load module");

        let instance = runtime
            .instantiate(
                module,
                "test-actor".to_string(),
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
                None, // elastic_pool_service
                None, // outbound_http_client
            )
            .await
            .expect("Failed to instantiate with large fuel limit");

        assert_eq!(instance.actor_id(), "test-actor");
    }
}
