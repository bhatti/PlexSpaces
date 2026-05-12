// SPDX-License-Identifier: AGPL-3.0-or-later
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
//! let instance = runtime.instantiate(module, actor_id, &initial_state, config, None, None, None, None, None).await?;
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
//! let pool = InstancePool::new(runtime.engine(), module.clone(), 10, config, "my-actor", "my-namespace", "node-1").await?;
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
pub mod simple_component_host;

// Re-export ModuleCache for external use
pub use module_cache::ModuleCache;

// Re-exports
pub use capabilities::WasmCapabilities;
pub use deployment_service::WasmDeploymentService;
pub use error::{WasmError, WasmResult};
pub use grpc_service::WasmRuntimeServiceImpl;
pub use host_functions::{HostFunctions, MessageSender};
pub use instance::{InstanceContext, WasmInstance};
pub use instance_pool::{InstancePool, PoolStats, PooledInstance};
pub use resource_limits::ResourceLimits;
pub use runtime::{WasmModule, WasmRuntime};

/// Canonical log message when a actor-world handle() fails.
/// Used so tests and logs can assert/check for this message; full backtrace is only at DEBUG.
pub const SIMPLE_ACTOR_HANDLE_FAILED_LOG_MESSAGE: &str = "Simple actor handle() call failed";

/// Helper functions to extract concrete types from WasmRuntimeTrait
/// These functions handle downcasting internally, so user code doesn't need to.
pub mod wasm_runtime_helpers {
    use super::*;
    use std::sync::Arc;

    /// Extract WasmModule from Arc<dyn Any>
    pub fn extract_wasm_module(
        module_any: Arc<dyn std::any::Any + Send + Sync>,
    ) -> Result<Arc<WasmModule>, WasmError> {
        module_any
            .downcast::<WasmModule>()
            .map_err(|_| WasmError::CompilationError("Failed to downcast WasmModule".to_string()))
    }

    /// Extract WasmConfig from Arc<dyn Any>
    pub fn extract_wasm_config(
        config_any: Arc<dyn std::any::Any + Send + Sync>,
    ) -> Result<Arc<WasmConfig>, WasmError> {
        config_any
            .downcast::<WasmConfig>()
            .map_err(|_| WasmError::CompilationError("Failed to downcast WasmConfig".to_string()))
    }

    /// Extract WasmInstance from Arc<dyn Any>
    pub fn extract_wasm_instance(
        instance_any: Arc<dyn std::any::Any + Send + Sync>,
    ) -> Result<Arc<WasmInstance>, WasmError> {
        instance_any
            .downcast::<WasmInstance>()
            .map_err(|_| WasmError::CompilationError("Failed to downcast WasmInstance".to_string()))
    }
}

/// WASM actor configuration combining limits and capabilities
#[derive(Clone)]
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

    /// Enable checkpoint/restore durability (load on init, save on terminate).
    /// Off by default for performance; enable when actor state must survive restarts.
    pub durability_enabled: bool,

    /// Use pre-instantiated instance pool (InstancePool) when spawning actors.
    /// On by default. When true, runtime may checkout from a per-module pool for faster spawn.
    /// Deploy-path integration is planned; until then, only engine-level pooling is active.
    pub use_instance_pool: bool,

    /// Maximum concurrent instantiations (initial + re-instantiation) when pooling is enabled.
    /// Limits total concurrent Wasmtime instantiations to avoid hitting Wasmtime's memory-stripe limit (default 10).
    /// Per-actor re-instantiation lock serializes per actor; this global cap prevents N actors from
    /// re-instantiating at once. Default: 8 (stays under Wasmtime's limit with headroom).
    /// Only used when `enable_pooling` is true.
    pub max_concurrent_instantiations: Option<u32>,

    /// Shared pool of send_after timer handles for the entire application.
    /// When set, all timers spawned by any actor in this application register here.
    /// On undeploy, the application calls cancel_all() on this pool to abort all pending timers.
    pub shared_timer_pool:
        Option<std::sync::Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>>,

    /// Trusted tenant ID for this WASM application instance.
    /// Comes from JWT at gRPC deploy time or from app-config.toml for file-copy deploys.
    /// Injected into HostFunctions so WIT host calls never trust guest-supplied tenant_id.
    pub tenant_id: String,

    /// Default namespace for this WASM application instance.
    /// Fallback when the guest WIT call does not supply a namespace.
    pub default_namespace: String,
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            limits: ResourceLimits {
                max_memory_bytes: 64 * 1024 * 1024, // 64MB (increased for Python)
                max_stack_bytes: 8 * 1024 * 1024,   // 8MB (required for Python)
                max_fuel: 10_000_000_000,           // 10 billion units
                max_execution_time: None,           // Rely on fuel instead
                max_table_elements: 10_000,
                max_pooled_instances: 10,
            },
            capabilities: crate::capabilities::profiles::default(),
            profile_name: "default".to_string(),
            enable_pooling: true,                   // Warm starts by default
            enable_aot: false,                      // JIT by default (faster deployment)
            durability_enabled: false,              // Off by default for performance
            use_instance_pool: true, // On by default; used when deploy-path integration is done
            max_concurrent_instantiations: Some(7), // Default: 7 permits (leaves headroom under Wasmtime's limit of 10)
            shared_timer_pool: None,
            tenant_id: String::new(),
            default_namespace: String::new(),
        }
    }
}

/// Convert proto WasmConfig to crate WasmConfig (keeps proto and crate in sync).
impl From<plexspaces_proto::wasm::v1::WasmConfig> for WasmConfig {
    fn from(p: plexspaces_proto::wasm::v1::WasmConfig) -> Self {
        let default = WasmConfig::default();
        Self {
            limits: p.limits.unwrap_or(default.limits),
            capabilities: p.capabilities.unwrap_or(default.capabilities),
            profile_name: if p.profile_name.is_empty() {
                default.profile_name
            } else {
                p.profile_name
            },
            enable_pooling: p.enable_pooling,
            enable_aot: p.enable_aot,
            durability_enabled: p.durability_enabled,
            use_instance_pool: p.use_instance_pool,
            max_concurrent_instantiations: if p.max_concurrent_instantiations > 0 {
                Some(p.max_concurrent_instantiations)
            } else {
                default.max_concurrent_instantiations
            },
            shared_timer_pool: None,
            tenant_id: String::new(),
            default_namespace: String::new(),
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
        assert!(config.use_instance_pool);
        assert_eq!(config.limits.max_memory_bytes, 64 * 1024 * 1024); // 64MB (for Python)
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
