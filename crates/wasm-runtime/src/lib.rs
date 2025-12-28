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

//! # PlexSpaces WASM Runtime
//!
//! ## Purpose
//! Provides WebAssembly Component Model runtime for **actor implementation** (not framework runtime).
//! Enables polyglot, sandboxed, and portable actor execution with dynamic deployment.
//!
//! ## Architecture Context
//! This crate implements WASM as the **actor implementation layer** (like AWS Lambda function code):
//!
//! - **Framework = Rust**: Provides services, runtime, infrastructure (journaling, TupleSpace, etc.)
//! - **Actors = WASM**: Provides business logic, polyglot support (Rust, Go, Python, JavaScript)
//! - **Separation**: Code (WASM module) and state (actor data) are separate for fast migration
//!
//! ## Key Design Principles
//! - **WASM = Actor Implementation**: WASM modules specify actor business logic, not framework
//! - **Code Caching**: WASM modules cached everywhere, only state migrates (10ms vs 500ms)
//! - **Polyglot Support**: Rust, JavaScript (Javy), Go (TinyGo), Python (componentize-py)
//! - **Capability-Based Security**: WASI + PlexSpaces facets for fine-grained control
//! - **Resource Limits**: Memory, fuel (gas), CPU time, stack size
//! - **32x Memory Efficiency**: 2MB per actor vs JavaNow's 64MB
//!
//! ## NOT This Crate's Responsibility
//! - ❌ VM management (belongs in `crates/firecracker/`)
//! - ❌ Application deployment (belongs in node/application layer)
//! - ❌ Framework runtime (framework is Rust, not WASM)
//!
//! ## Key Components
//! - [`WasmRuntime`]: Main runtime for loading and executing WASM modules
//! - [`HostFunctions`]: Host functions provided to WASM actors (send, spawn, tuplespace, log)
//! - [`ResourceLimits`]: Memory, fuel, CPU time limits for sandboxing
//! - [`WasmCapabilities`]: WASI + PlexSpaces capability-based security
//! - [`InstancePool`]: Pre-instantiated instances for warm starts (< 10ms)
//! - [`ModuleCache`]: Content-addressed cache for WASM modules
//!
//! ## Dependencies
//! This crate depends on:
//! - [`plexspaces_core`]: Common types and errors
//! - [`plexspaces_actor`]: Actor abstraction and behavior trait
//! - [`plexspaces_tuplespace`]: TupleSpace for coordination
//! - [`wasmtime`]: WebAssembly runtime (Component Model support)
//! - [`wasmtime_wasi`]: WASI preview 2 implementation
//!
//! ## Dependents
//! This crate is used by:
//! - [`plexspaces_node`]: Node spawns WASM actors
//! - [`plexspaces`]: Root crate re-exports WASM runtime
//!
//! ## Examples
//!
//! ### Basic Usage: Load and Execute WASM Module
//! ```rust,no_run
//! use plexspaces_wasm_runtime::{WasmRuntime, WasmConfig, ResourceLimits, WasmCapabilities};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create runtime with default config
//! let runtime = WasmRuntime::new().await?;
//!
//! // Load WASM module (Component Model format)
//! let module_bytes = std::fs::read("actor.wasm")?;
//! let module = runtime.load_module("counter-actor", "1.0.0", &module_bytes).await?;
//!
//! // Configure resource limits
//! let config = WasmConfig {
//!     limits: ResourceLimits {
//!         max_memory_bytes: 16 * 1024 * 1024,  // 16MB
//!         max_fuel: 10_000_000_000,            // 10 billion fuel units
//!         ..Default::default()
//!     },
//!     capabilities: WasmCapabilities {
//!         allow_tuplespace: true,
//!         allow_send_messages: true,
//!         allow_logging: true,
//!         ..Default::default()
//!     },
//!     ..Default::default()
//! };
//!
//! // Instantiate actor
//! let actor_id = "actor-001".to_string();
//! let initial_state = vec![]; // Empty state for new actor
//! let instance = runtime.instantiate(module, actor_id, &initial_state, config, None).await?;
//!
//! // Call actor's handle_message function
//! let from = "caller-actor";
//! let message_type = "increment";
//! let payload = vec![];
//! let response = instance.handle_message(from, message_type, payload).await?;
//!
//! // Snapshot actor state
//! let state = instance.snapshot_state().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Advanced Usage: Instance Pooling for Warm Starts
//! ```rust,no_run
//! use plexspaces_wasm_runtime::{WasmRuntime, InstancePool, WasmConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use std::sync::Arc;
//! let runtime = WasmRuntime::new().await?;
//! let module_bytes = std::fs::read("actor.wasm")?;
//! let module = runtime.load_module("fast-actor", "1.0.0", &module_bytes).await?;
//!
//! // Create instance pool (pre-warm 10 instances)
//! let config = WasmConfig::default();
//! let pool = InstancePool::new(runtime.engine(), module.clone(), 10, config).await?;
//!
//! // Get instance from pool (< 10ms warm start)
//! let mut pooled = pool.checkout().await?;
//!
//! // Use instance...
//! let response = pooled.instance_mut().handle_message("sender", "process", vec![]).await?;
//!
//! // Instance automatically returns to pool when pooled is dropped
//! # Ok(())
//! # }
//! ```
//!
//! ### WASI Capabilities Example
//! ```rust,no_run
//! use plexspaces_wasm_runtime::{WasmConfig, WasmCapabilities};
//!
//! // Untrusted actor (minimal capabilities)
//! let untrusted_config = WasmConfig {
//!     capabilities: WasmCapabilities {
//!         allow_filesystem: false,
//!         allow_network: false,
//!         allow_spawn_actors: false,
//!         allow_send_messages: true,  // Can only send messages
//!         allow_logging: true,
//!         ..Default::default()
//!     },
//!     ..Default::default()
//! };
//!
//! // Trusted supervisor actor (full capabilities)
//! let trusted_config = WasmConfig {
//!     capabilities: WasmCapabilities {
//!         allow_filesystem: true,
//!         allow_network: true,
//!         allow_spawn_actors: true,  // Can spawn children
//!         allow_send_messages: true,
//!         allow_tuplespace: true,
//!         allow_logging: true,
//!         ..Default::default()
//!     },
//!     ..Default::default()
//! };
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First Design
//! - All WASM definitions in `proto/plexspaces/v1/wasm.proto`
//! - WIT interface in `wit/plexspaces-actor/actor.wit`
//! - This crate implements the proto contracts
//!
//! ### Static vs Dynamic
//! - **Static**: wasmtime runtime, Component Model ABI (always present)
//! - **Dynamic**: Capabilities, resource limits (configured per actor)
//!
//! ### Test-Driven Development
//! - Module loading tests: Verify Component Model parsing
//! - Host function tests: Ensure WASM can call host functions
//! - Resource limit tests: Verify fuel, memory limits enforced
//! - Integration tests: End-to-end actor execution
//! - Target: 90%+ code coverage
//!
//! ## Testing
//! ```bash
//! # Run tests
//! cargo test -p plexspaces-wasm-runtime
//!
//! # Check coverage
//! cargo tarpaulin -p plexspaces-wasm-runtime
//!
//! # Run examples
//! cargo run --example hello_world_wasm
//! cargo run --example counter_wasm
//! ```
//!
//! ## Performance Characteristics
//! Based on User Decision A10 (balanced performance):
//! - **Cold Start**: < 100ms (module compilation + instantiation)
//! - **Warm Start**: < 10ms (instance from pool)
//! - **Migration**: < 10ms (state-only transfer, code cached)
//! - **Memory per Actor**: 2MB (32x better than JavaNow)
//! - **Throughput**: > 10,000 actors per node, > 100,000 messages/sec
//!
//! ## Known Limitations
//! - Component Model is still evolving (WASI preview 2)
//! - JavaScript support requires Javy (separate toolchain)
//! - Go support requires TinyGo (not all stdlib supported)
//!
//! ## Firecracker Integration
//! Firecracker is a **separate concern** for application-level isolation:
//! - Firecracker launches entire applications in VMs (like AWS Lambda)
//! - WASM actors run inside the framework (which may be in a Firecracker VM)
//! - See `crates/firecracker/` for VM lifecycle management

#![warn(missing_docs)]
#![warn(clippy::all)]

// Module declarations
pub mod capabilities;
#[cfg(feature = "component-model")]
pub mod component_host;
pub mod deployment_service;
pub mod error;
pub mod grpc_service;
pub mod host_functions;
pub mod instance;
pub mod instance_pool;
pub mod memory;
pub mod module_cache;
pub mod resource_limits;
pub mod runtime;


// Re-export ModuleCache for external use
pub use module_cache::ModuleCache;

// Re-exports
pub use capabilities::WasmCapabilities;
pub use deployment_service::WasmDeploymentService;
pub use error::{WasmError, WasmResult};
pub use grpc_service::WasmRuntimeServiceImpl;
pub use host_functions::HostFunctions;
pub use instance::{InstanceContext, WasmInstance};
pub use instance_pool::{InstancePool, PoolStats, PooledInstance};
pub use resource_limits::ResourceLimits;
pub use runtime::{WasmModule, WasmRuntime};

/// WASM actor configuration combining limits and capabilities
#[derive(Debug, Clone)]
pub struct WasmConfig {
    /// Resource limits (memory, fuel, CPU time)
    pub limits: ResourceLimits,

    /// Capabilities (WASI + PlexSpaces)
    pub capabilities: WasmCapabilities,

    /// Configuration profile name (e.g., "default", "untrusted", "trusted")
    pub profile_name: String,

    /// Enable instance pooling for warm starts
    pub enable_pooling: bool,

    /// Enable ahead-of-time (AOT) compilation
    pub enable_aot: bool,
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            limits: ResourceLimits {
                max_memory_bytes: 16 * 1024 * 1024, // 16MB
                max_stack_bytes: 512 * 1024,        // 512KB
                max_fuel: 10_000_000_000,           // 10 billion units
                max_execution_time: None,           // Rely on fuel instead
                max_table_elements: 10_000,
                max_pooled_instances: 10,
            },
            capabilities: crate::capabilities::profiles::default(),
            profile_name: "default".to_string(),
            enable_pooling: true, // Warm starts by default
            enable_aot: false,    // JIT by default (faster deployment)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_config_default() {
        let config = WasmConfig::default();
        assert_eq!(config.profile_name, "default");
        assert!(config.enable_pooling);
        assert!(!config.enable_aot);
        assert_eq!(config.limits.max_memory_bytes, 16 * 1024 * 1024); // 16MB
    }

    #[test]
    fn test_untrusted_config() {
        let config = WasmConfig {
            capabilities: WasmCapabilities {
                allow_filesystem: false,
                allow_network: false,
                allow_spawn_actors: false,
                ..Default::default()
            },
            profile_name: "untrusted".to_string(),
            ..Default::default()
        };

        assert_eq!(config.profile_name, "untrusted");
        assert!(!config.capabilities.allow_filesystem);
        assert!(!config.capabilities.allow_network);
        assert!(!config.capabilities.allow_spawn_actors);
    }
}
