# Ractor Calculator - Type-Safe Actor Message Passing

Demonstrates **Ractor-style typed message passing** with a calculator actor.

**Real-world use case**: Computation service with type-safe RPC, where each operation
is a distinct message type handled by a dedicated method (like Ractor, Actix, Bastion).

## Architecture

```
    Client (HTTP/gRPC)
         │
         ▼
┌──────────────────┐
│  CalculatorActor  │  Handles typed messages:
│                    │  - Add { a, b } → result
│  operation_count   │  - Subtract { a, b } → result
│  history[]         │  - Multiply { a, b } → result
│  benchmarks        │  - Divide { a, b } → result
└──────────────────┘
```

## Quick Start

```bash
# Terminal 1: Start PlexSpaces node
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:7992

# Terminal 2: Build and run
cd examples/python/apps/migrating_ractor
./build.sh        # Builds ractor_calculator.wasm
./test.sh 7993    # Deploy + test (HTTP port = gRPC 7992 + 1)
```

## PlexSpaces SDK Features

| Feature | How Used |
|---------|----------|
| `@actor` | Marks `CalculatorActor` as a PlexSpaces actor |
| `state()` | Persistent state (operation_count, history, benchmarks) |
| `@handler()` | Routes `add`, `subtract`, `multiply`, `divide`, `batch` |
| `@init_handler` | Initialize from framework config |
| `host.now_ms()` | Timing for computation benchmarks |

## Comparison: Ractor vs PlexSpaces

| Feature | Ractor (Rust) | PlexSpaces (Python) |
|---------|---------------|---------------------|
| Actor definition | `impl Actor for CalculatorActor` | `@actor class CalculatorActor` |
| Message types | `enum CalculatorMessage { Add { a, b, reply } }` | `@handler("add") def add(self, a, b)` |
| Message dispatch | `match message { Add { .. } => .. }` | Automatic via `@handler` decorator |
| State | `&mut self.operation_count` | `state(default=0)` fields |
| Request-reply | `RpcReplyPort<f64>` | Return dict from handler |
| Actor spawn | `Actor::spawn(None, CalculatorActor, ())` | `app-config.toml` supervisor |
| Language | Rust only | Python, TypeScript, Go, Rust |

## Files

| File | Description |
|------|-------------|
| `ractor_calculator.py` | CalculatorActor with typed operations |
| `app-config.toml` | ApplicationSpec (single calculator actor) |
| `build.sh` | Build WASM module |
| `test.sh` | Deploy + test operations + batch benchmark |
| `native/calculator.rs` | Native Ractor reference implementation |
