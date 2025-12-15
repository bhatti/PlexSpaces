# WASM Calculator Modules

This directory contains pre-compiled WASM modules for the calculator example.

## Modules

### calculator_wasm_actor.wasm

**Size**: ~57KB (optimized for size)
**Target**: `wasm32-unknown-unknown`
**Source**: `../wasm-actors/src/lib.rs`

**Exports**:
- `init(state_ptr, state_len) -> i32`: Initialize actor with state
- `handle_message(from_ptr, from_len, type_ptr, type_len, payload_ptr, payload_len) -> *const u8`: Process messages
- `snapshot_state() -> u64`: Snapshot current state (returns packed ptr + len)

**Operations Supported**:
- Add (a + b)
- Subtract (a - b)
- Multiply (a * b)
- Divide (a / b)

**Message Types**:
- `"calculate"`: Perform a calculation (payload: CalculationRequest)
- `"get_stats"`: Get statistics (returns calculation count, last result)

## Building

To rebuild the WASM module:

```bash
cd ../wasm-actors
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/calculator_wasm_actor.wasm ../wasm-modules/
```

## Usage

The WASM module is loaded by the PlexSpaces WASM runtime and instantiated as actors.

Example (from calc-node binary):

```rust
use plexspaces_wasm_runtime::WasmRuntime;

let mut runtime = WasmRuntime::new().await?;
let module_bytes = std::fs::read("wasm-modules/calculator_wasm_actor.wasm")?;
runtime.load_module("calculator", module_bytes).await?;

// Spawn actor from WASM module
let actor_id = runtime.spawn_actor("calculator", initial_state).await?;
```

## Performance

- **Cold start**: < 50ms (module loading + instantiation)
- **Warm start**: < 5ms (cached module instantiation)
- **Message processing**: < 1ms (simple arithmetic operations)
- **Size**: 57KB (including serde and allocator)

## Security

WASM provides sandboxed execution:
- No file system access
- No network access
- Memory isolation (cannot access host memory)
- Execution limits enforced by runtime

## Next Steps

- [ ] Add more operations (power, sqrt, etc.)
- [ ] Optimize for even smaller size (target < 50KB)
- [ ] Add Component Model support (WASI preview 2)
- [ ] Benchmark vs native actor performance
