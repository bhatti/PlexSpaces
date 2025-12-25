# WASM Calculator Example

## Overview

This example demonstrates **Python application deployment** to PlexSpaces nodes. It shows how Python applications written as WASM modules can be:
- Deployed to nodes via ApplicationService
- Use durable actors for state persistence
- Use tuplespace for distributed coordination

## Features

- **Python Application Deployment**: Deploy Python WASM applications to PlexSpaces nodes
- **Durable Actors**: State persistence, journaling, checkpointing for fault tolerance
- **TupleSpace Coordination**: Distributed coordination via tuplespace for result sharing
- **Dynamic Deployment**: Deploy applications at runtime via ApplicationService
- **Content-Addressed Caching**: Modules cached by hash for efficiency

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│              PlexSpaces Node (wasm-calculator-node-1)        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  ApplicationService (gRPC)                           │  │
│  │  - Deploy Python WASM applications                   │  │
│  │  - Manage application lifecycle                     │  │
│  └──────────────────────────────────────────────────────┘  │
│                          │                                   │
│                          ▼                                   │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Deployed Python Applications                        │  │
│  │                                                      │  │
│  │  ┌──────────────────┐    ┌──────────────────┐      │  │
│  │  │ Durable Calc     │    │ TupleSpace Calc  │      │  │
│  │  │ (Python WASM)    │    │ (Python WASM)    │      │  │
│  │  │                  │    │                  │      │  │
│  │  │ • State persist  │    │ • Write results  │      │  │
│  │  │ • Journaling     │    │ • Read results   │      │  │
│  │  │ • Checkpointing  │    │ • Coordination   │      │  │
│  │  └──────────────────┘    └──────────────────┘      │  │
│  │                          │                          │  │
│  └──────────────────────────┼──────────────────────────┘  │
│                             │                               │
│                             ▼                               │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  TupleSpace (Distributed Coordination)               │  │
│  │  - Shared results across all actors                  │  │
│  │  - Pattern matching for queries                      │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Prerequisites

- Rust toolchain (latest stable)
- Python 3.9+ (for building Python WASM actors)
- `componentize-py` (for compiling Python to WASM)

Install `componentize-py`:
```bash
pip install componentize-py
```

## Building

### Build the Example

```bash
cd examples/simple/wasm_calculator
cargo build --release
```

### Build Python WASM Actors (Optional)

```bash
# Build Python actors
./scripts/build_python_actors.sh

# Or build manually:
componentize-py actors/python/durable_calculator_actor.py \
    -o wasm-modules/durable_calculator_actor.wasm \
    --wit ../../../wit/plexspaces-actor/actor.wit

componentize-py actors/python/tuplespace_calculator_actor.py \
    -o wasm-modules/tuplespace_calculator_actor.wasm \
    --wit ../../../wit/plexspaces-actor/actor.wit
```

**Note**: If Python actors are not built, the example will use placeholder WASM modules.

## Running

### Run the Example

```bash
# Run without building Python actors (uses placeholders)
cargo run --release --bin wasm_calculator

# Run with Python actor building
cargo run --release --bin wasm_calculator -- --build
```

### Run Tests

```bash
# Run integration tests
cargo test --test integration_test

# Run all tests
cargo test
```

## Testing

### Integration Tests

The example includes comprehensive integration tests:

```bash
cargo test --test integration_test
```

All 5 tests should pass:
- ✅ `test_node_creation` - Node creation
- ✅ `test_node_startup` - Node startup
- ✅ `test_application_service_connection` - gRPC connection
- ✅ `test_deploy_wasm_application` - WASM deployment
- ✅ `test_tuplespace_access` - TupleSpace access

### Test Script

```bash
# Run validation script
./scripts/test.sh

# Or run manually
cargo run --release --bin wasm_calculator -- --build
```

## Deployment

### 1. Start an Empty Node

The example creates and starts a node automatically, but you can also start one manually:

```bash
# From project root
cargo run --release --bin plexspaces-node -- \
    --node-id wasm-calc-node \
    --listen-address 0.0.0.0:9001
```

### 2. Deploy WASM Applications

The example demonstrates deploying Python WASM applications via ApplicationService:

```rust
// Connect to ApplicationService
let channel = Channel::from_shared("http://0.0.0.0:9001")?
    .connect()
    .await?;
let mut client = ApplicationServiceClient::new(channel);

// Deploy application
let deploy_request = DeployApplicationRequest {
    application_id: "calc-app-1".to_string(),
    name: "calculator-app".to_string(),
    version: "1.0.0".to_string(),
    wasm_module: Some(wasm_module),
    config: Some(app_config),
    release_config: None,
    initial_state: vec![],
};

let response = client.deploy_application(tonic::Request::new(deploy_request)).await?;
```

### 3. Verify Deployment

```bash
# List deployed applications via gRPC
# Or use the dashboard at http://localhost:9001/ to see deployed applications
```

### 4. Using the CLI

You can also deploy using the PlexSpaces CLI:

```bash
# Deploy a WASM application
plexspaces application deploy \
    --name calculator-app \
    --version 1.0.0 \
    --wasm-module wasm-modules/calculator_actor.wasm \
    --node-id wasm-calc-node

# List deployed applications
plexspaces application list --node-id wasm-calc-node
```

## What the Example Demonstrates

### 1. Python Application Deployment

- Deploys `durable_calculator_actor.py` as a WASM application via ApplicationService
- Deploys `tuplespace_calculator_actor.py` as a WASM application via ApplicationService
- Shows how Python applications are deployed to PlexSpaces nodes at runtime
- Demonstrates node-based deployment (applications deployed to nodes, not individual actors)

### 2. Durable Actor Features (from Python)

- State persistence: Actor state is automatically journaled by DurabilityFacet
- Checkpointing: Periodic snapshots for fast recovery
- Replay: Deterministic replay of messages after restart
- The `durable_calculator_actor.py` implements `snapshot_state()` and `init(initial_state)`
- All durability features are demonstrated from Python code, not Rust

### 3. TupleSpace Coordination (from Python)

- Write results to tuplespace for sharing using `host.tuplespace_write()`
- Read results from tuplespace using pattern matching with `host.tuplespace_read()`
- The `tuplespace_calculator_actor.py` demonstrates tuplespace coordination from Python
- Results are shared across all actors in the node via distributed tuplespace

## Python Actor Development

### Writing Python Actors

Python actors use the WIT interface defined in `wit/plexspaces-actor/actor.wit`:

```python
from componentize_py import Component

class CalculatorActor(Component):
    def init(self, initial_state):
        # Initialize actor state
        self.count = 0
        self.last_result = 0.0
    
    def handle_message(self, from_actor, message_type, payload):
        # Handle messages
        if message_type == "calculate":
            # Perform calculation
            result = self.calculate(payload)
            self.count += 1
            self.last_result = result
            return result
    
    def snapshot_state(self):
        # Serialize state for checkpointing
        return {"count": self.count, "last_result": self.last_result}
```

### Building Python Actors

```bash
componentize-py actors/python/calculator_actor.py \
    -o wasm-modules/calculator_actor.wasm \
    --wit ../../../wit/plexspaces-actor/actor.wit
```

## Key PlexSpaces Features Demonstrated

- **Python Application Deployment**: Deploy Python WASM applications to nodes via ApplicationService
- **Durable Actors**: State persistence, journaling, checkpointing for fault tolerance
- **TupleSpace Coordination**: Distributed coordination primitives for result sharing
- **Dynamic Deployment**: Deploy applications at runtime without restarting nodes
- **Content-Addressed Caching**: Modules cached by hash for efficiency
- **WIT Interface**: Python actors use WIT host functions for tuplespace access

## Python Actor Implementation

### Durable Calculator Actor (`durable_calculator_actor.py`)

Demonstrates state persistence:
- `init(initial_state)`: Restores state from checkpoint
- `snapshot_state()`: Serializes state for checkpointing
- State is automatically journaled by DurabilityFacet
- State survives actor restarts and node failures

### TupleSpace Calculator Actor (`tuplespace_calculator_actor.py`)

Demonstrates distributed coordination:
- Uses `host.tuplespace_write()` to share calculation results
- Uses `host.tuplespace_read()` to read results from other actors
- Results are shared across all actors in the node
- Pattern matching enables flexible queries

## WIT Interface for Python

Python actors access PlexSpaces services via WIT host functions:
- `host.tuplespace_write(tuple)`: Write tuple to coordination space
- `host.tuplespace_read(pattern)`: Read tuple matching pattern
- `host.tuplespace_take(pattern)`: Take tuple (destructive read)
- `host.tuplespace_count(pattern)`: Count matching tuples

See `wit/plexspaces-actor/actor.wit` for the complete interface.

## Build and Test Status

### Build Status

✅ **Build Successful**: `cargo build` completes without errors  
✅ **Release Build**: `cargo build --release` completes successfully

### Test Status

✅ **Integration Tests**: All 5 tests passing
- ✅ `test_node_creation` - Node creation works
- ✅ `test_node_startup` - Node startup works
- ✅ `test_application_service_connection` - Connection validation works
- ✅ `test_deploy_wasm_application` - WASM deployment works
- ✅ `test_tuplespace_access` - TupleSpace access works

### Fixed Issues

1. **NodeBuilder::build() Async Issue**: Added `.await` to `NodeBuilder::new(...).build().await`
2. **Node Config Access**: Used `node_arc.config().listen_addr` to get listen address
3. **TupleSpace Access**: Access via `service_locator().get_tuplespace_provider().await`
4. **Node Shutdown**: Used `node_arc.shutdown(Duration::from_secs(5)).await`
5. **WasmModule Proto Fields**: Added all required fields (`created_at`, `metadata`, `size_bytes`, `version_number`, `source_languages`)
6. **Workspace Configuration**: Added empty `[workspace]` table to make it standalone

## Troubleshooting

### Build Errors

- **"ObjectRegistry not found"**: Ensure `node.initialize_services().await` is called
- **"ApplicationManager not found"**: Services are registered during `node.start()`
- **Port conflicts**: Use `with_listen_address("127.0.0.1:0")` for dynamic port allocation
- **Workspace errors**: Example has standalone `[workspace]` table - don't add to main workspace

### Runtime Errors

- **Connection refused**: Node may not be fully started - wait longer or check logs
- **WASM module not found**: Build Python actors first or use placeholders
- **TupleSpace errors**: Ensure TupleSpaceProvider is registered in ServiceLocator
- **Componentize-py architecture mismatch**: Reinstall `componentize-py` for your architecture (x86_64 vs ARM64)

### Python Actor Build Issues

If `componentize-py` fails with architecture mismatch:
```bash
# For ARM64 Macs, reinstall componentize-py
pip uninstall componentize-py
pip install componentize-py

# Or use Rosetta 2 for x86_64 compatibility
arch -x86_64 pip install componentize-py
```

## Next Steps

- Add more complex operations (trigonometry, statistics)
- Demonstrate state recovery after restart
- Add multi-node tuplespace coordination
- Add performance benchmarks comparing Python vs Rust actors
- Test actual Python actor execution
- Test state persistence and recovery
- Test multi-node coordination

## References

- [PlexSpaces Architecture](../../../../docs/architecture.md) - System design and abstractions
- [Detailed Design - WASM Runtime](../../../../docs/detailed-design.md#wasm-runtime) - WASM runtime details
- [PlexSpaces WASM Runtime Crate](../../../../crates/wasm-runtime/README.md) - Crate documentation
- [Getting Started Guide](../../../../docs/getting-started.md) - Quick start guide
- [WebAssembly Component Model](https://github.com/WebAssembly/component-model)
- [wasmtime Documentation](https://docs.wasmtime.dev/)
