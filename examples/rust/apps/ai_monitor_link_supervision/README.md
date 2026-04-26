# AI Monitor/Link Supervision (Rust WASM)

Demonstrates actor-model **monitor** and **link** primitives for building fault-tolerant AI pipelines, using FLP impossibility and Byzantine fault detection as a realistic motivating scenario.

## Overview

This Rust WASM example implements an AI pipeline with four actors. The pure Rust business logic is host-portable (testable without WASM), while the WASM bindings wire in `host::monitor()`, `host::demonitor()`, `host::link()`, and `host::unlink()`.

### Key Actors

| Actor | Role | Pattern Used |
|-------|------|-------------|
| `InferenceWorker` | LLM inference backend; normal or Byzantine mode | `host::link()` / `host::unlink()` |
| `ValidatorAgent` | Output validator with Byzantine detection | `host::monitor()` / `host::demonitor()` |
| `PipelineSupervisor` | Fault-aware dispatcher, responds to `__DOWN__` | `host::monitor()` / `host::demonitor()` |
| `AuditLogActor` | GenEvent fire-and-forget audit log | — |

### WASM Host Calls

```rust
// In WASM bindings only (plexspaces::actor::host)
use plexspaces::actor::host;

// One-way watch — supervisor continues running when worker stops
let monitor_ref = host::monitor(&worker_id);

// Cancel watch
host::demonitor(&monitor_ref);

// Bidirectional fate-sharing — abnormal exits only
host::link(&peer_id);

// Safe decoupling before graceful shutdown
host::unlink(&peer_id);
```

### Message Handling Pattern

```rust
// Handle __DOWN__ (monitor fires on ANY termination)
"__DOWN__" => {
    let monitor_ref = get_str(&v, "monitor_ref", "");
    let actor_id = get_str(&v, "actor_id", "");
    state.worker_pool.retain(|w| w != actor_id);
    state.monitor_refs.retain(|m| m.monitor_ref != monitor_ref);
    // supervisor keeps running
}

// Handle __EXIT__ (link fires on ABNORMAL exit only)
"__EXIT__" => {
    let from_actor = get_str(&v, "from_actor", "");
    state.linked_peers.retain(|p| p != from_actor);
    // normal exits and Shutdown do NOT produce __EXIT__
}
```

## Architecture

The Rust implementation separates concerns:

- **Pure logic functions** (`handle_inference_worker`, `handle_validator`, `handle_supervisor`) — no WASM dependencies, fully unit-testable
- **WASM bridges** (in `wasm_app` module) — call `host::monitor()` etc. and delegate to logic functions
- **Protobuf state** — all actor state serialized as protobuf for durable storage support

## Unit Tests

The pure-Rust logic functions are tested without WASM:

```bash
cargo test --lib
```

Tests cover:
- Normal inference determinism
- Byzantine mode output patterns
- Validator FLP threshold (≥1/3 Byzantine → alert)
- `__DOWN__` cleanup of monitor refs
- `__EXIT__` cleanup of linked peers

## Usage

```bash
# Run unit tests (no node needed)
cargo test --lib

# Build WASM binary
./build.sh

# Run the full integration test (requires running PlexSpaces node)
./test.sh                          # default: localhost:8092
./test.sh 8092                     # single node
./test.sh localhost:8092 localhost:8094  # two-node cluster
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Detailed Design: Supervision](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Other Rust Examples](../../README.md)
