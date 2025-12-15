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

## Usage

### Prerequisites

```bash
# Build Python WASM actors (optional - example will use placeholders if not built)
./scripts/build_python_actors.sh

# Or build manually:
# pip install componentize-py
# componentize-py actors/python/durable_calculator_actor.py -o wasm-modules/durable_calculator_actor.wasm --wit ../../../wit/plexspaces-actor/actor.wit
# componentize-py actors/python/tuplespace_calculator_actor.py -o wasm-modules/tuplespace_calculator_actor.wasm --wit ../../../wit/plexspaces-actor/actor.wit
```

### Run the Example

```bash
# Build Python actors and run
cargo run --release --bin wasm_calculator -- --build

# Or run without building (uses placeholders)
cargo run --release --bin wasm_calculator
```

### What the Example Demonstrates

1. **Python Application Deployment**:
   - Deploys `durable_calculator_actor.py` as a WASM application via ApplicationService
   - Deploys `tuplespace_calculator_actor.py` as a WASM application via ApplicationService
   - Shows how Python applications are deployed to PlexSpaces nodes at runtime
   - Demonstrates node-based deployment (applications deployed to nodes, not individual actors)

2. **Durable Actor Features (from Python)**:
   - State persistence: Actor state is automatically journaled by DurabilityFacet
   - Checkpointing: Periodic snapshots for fast recovery
   - Replay: Deterministic replay of messages after restart
   - The `durable_calculator_actor.py` implements `snapshot_state()` and `init(initial_state)`
   - All durability features are demonstrated from Python code, not Rust

3. **TupleSpace Coordination (from Python)**:
   - Write results to tuplespace for sharing using `host.tuplespace_write()`
   - Read results from tuplespace using pattern matching with `host.tuplespace_read()`
   - The `tuplespace_calculator_actor.py` demonstrates tuplespace coordination from Python
   - Results are shared across all actors in the node via distributed tuplespace

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

## Testing

```bash
# Run validation script
./scripts/test.sh

# Or run manually
cargo run --release --bin wasm_calculator -- --build
```

## Next Steps

- Add more complex operations (trigonometry, statistics)
- Demonstrate state recovery after restart
- Add multi-node tuplespace coordination
- Add performance benchmarks comparing Python vs Rust actors

## References

- [PlexSpaces Architecture](../../../../docs/architecture.md) - System design and abstractions
- [Detailed Design - WASM Runtime](../../../../docs/detailed-design.md#wasm-runtime) - WASM runtime details
- [PlexSpaces WASM Runtime Crate](../../../../crates/wasm-runtime/README.md) - Crate documentation
- [Getting Started Guide](../../../../docs/getting-started.md) - Quick start guide
- [WebAssembly Component Model](https://github.com/WebAssembly/component-model)
- [wasmtime Documentation](https://docs.wasmtime.dev/)

