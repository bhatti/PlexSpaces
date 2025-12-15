# WASM Runtime - WebAssembly Component Model Runtime

**Purpose**: Provides WebAssembly Component Model runtime for **actor implementation** (not framework runtime). Enables polyglot, sandboxed, and portable actor execution with dynamic deployment.

## Overview

This crate implements WASM as the **actor implementation layer** (like AWS Lambda function code):
- **Framework = Rust**: Provides services, runtime, infrastructure (journaling, TupleSpace, etc.)
- **Actors = WASM**: Provides business logic, polyglot support (Rust, Go, Python, JavaScript)
- **Separation**: Code (WASM module) and state (actor data) are separate for fast migration

## Key Design Principles

- **WASM = Actor Implementation**: WASM modules specify actor business logic, not framework
- **Code Caching**: WASM modules cached everywhere, only state migrates (10ms vs 500ms)
- **Polyglot Support**: Rust, JavaScript (Javy), Go (TinyGo), Python (componentize-py)
- **Capability-Based Security**: WASI + PlexSpaces facets for fine-grained control
- **Resource Limits**: Memory, fuel (gas), CPU time, stack size
- **32x Memory Efficiency**: 2MB per actor vs JavaNow's 64MB

## Key Components

### WasmRuntime

Main runtime for loading and executing WASM modules:

```rust
pub struct WasmRuntime {
    engine: Engine,
    module_cache: ModuleCache,
    instance_pool: InstancePool,
}
```

### HostFunctions

Host functions provided to WASM actors:

```rust
pub struct HostFunctions {
    send_message: fn(actor_id: &str, message: Vec<u8>) -> Result<()>,
    spawn_actor: fn(actor_id: &str, behavior: Vec<u8>) -> Result<()>,
    tuplespace_write: fn(tuple: Vec<u8>) -> Result<()>,
    tuplespace_read: fn(pattern: Vec<u8>) -> Result<Vec<u8>>,
    log: fn(level: u32, message: &str) -> Result<()>,
}
```

### ResourceLimits

Memory, fuel, CPU time limits for sandboxing:

```rust
pub struct ResourceLimits {
    pub max_memory_bytes: usize,
    pub max_fuel: u64,
    pub max_cpu_time_ms: Option<u64>,
    pub max_stack_size: usize,
}
```

## Usage Examples

### Basic Usage: Load and Execute WASM Module

```rust
use plexspaces_wasm_runtime::{WasmRuntime, WasmConfig, ResourceLimits, WasmCapabilities};

// Create runtime with default config
let runtime = WasmRuntime::new().await?;

// Load WASM module (Component Model format)
let module_bytes = std::fs::read("actor.wasm")?;
let module = runtime.load_module("counter-actor", "1.0.0", &module_bytes).await?;

// Configure resource limits
let config = WasmConfig {
    limits: ResourceLimits {
        max_memory_bytes: 16 * 1024 * 1024,  // 16MB
        max_fuel: 10_000_000_000,            // 10 billion fuel units
        ..Default::default()
    },
    capabilities: WasmCapabilities {
        allow_tuplespace: true,
        allow_send_messages: true,
        allow_logging: true,
        ..Default::default()
    },
    ..Default::default()
};

// Instantiate actor
let actor_id = "actor-001".to_string();
let initial_state = vec![]; // Empty state for new actor
let instance = runtime.instantiate(module, actor_id, &initial_state, config, None).await?;
```

### Polyglot Support

```rust
// Rust actor (compiled to WASM)
let rust_module = runtime.load_module("rust-actor", "1.0.0", &rust_wasm_bytes).await?;

// TypeScript actor (compiled with Javy)
let ts_module = runtime.load_module("ts-actor", "1.0.0", &ts_wasm_bytes).await?;

// Go actor (compiled with TinyGo)
let go_module = runtime.load_module("go-actor", "1.0.0", &go_wasm_bytes).await?;

// Python actor (compiled with componentize-py)
let py_module = runtime.load_module("py-actor", "1.0.0", &py_wasm_bytes).await?;
```

## Dependencies

This crate depends on:
- `plexspaces_core`: Common types and errors
- `plexspaces_actor`: Actor abstraction and behavior trait
- `plexspaces_tuplespace`: TupleSpace for coordination
- `wasmtime`: WebAssembly runtime (Component Model support)
- `wasmtime_wasi`: WASI preview 2 implementation

## Dependents

This crate is used by:
- `plexspaces_node`: Node spawns WASM actors
- `plexspaces`: Root crate re-exports WASM runtime

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design](../../docs/detailed-design.md) - Component details
- [Getting Started Guide](../../docs/getting-started.md) - Quick start with WASM actors
- [Use Cases](../../docs/use-cases.md) - Real-world WASM applications
- Implementation: `crates/wasm-runtime/src/`
- Tests: `crates/wasm-runtime/tests/`
- WIT definitions: `wit/plexspaces-actor/actor.wit`
- Inspiration: WebAssembly Component Model, wasmCloud

