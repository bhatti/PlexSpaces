# Order Fulfillment Workflow (Rust WASM)

E-commerce order fulfillment workflow as a **Rust WASM app** using the same interface as the Go/TypeScript/Python variants: **plexspaces-simple-actor** (init, handle, get-state, set-state) with workflow_run / workflow_signal / workflow_query.

## Purpose

- **Rust WASM app**: Built as a cdylib for `wasm32-wasip2`, then wrapped with `wasm-tools component embed` and the WASI adapter so the node loads it as a component (same pipeline as Go).
- **Workflow behavior**: Implements run (saga steps), signal(cancel), query(status) with the same message types as the other languages.
- **Facets**: virtual_actor + durability (configured in `app-config.toml`).

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/rust/apps/migrating_temporal
./build.sh
./test.sh 8092
```

## Build

- **Requirements**: Rust (wasm32-wasip1 target), `wasm-tools`, WASI adapter (e.g. from `jco`).
- **Steps**: `cargo build --release --target wasm32-wasip1` → `wasm-tools component embed` (WIT) → `wasm-tools component new` (WASI adapter) → `order_fulfillment_actor.wasm`.
- **Time**: The actor uses a monotonic counter for step timestamps so the module has no host imports and builds with `wasm-tools component embed`. For real timestamps the runtime can wire the host interface when instantiating.

## API

Same as Go/TS/Python:

- **Run**: `POST /api/v1/actors/{app_id}/order-fulfillment:{order_id}` with `{"op":"workflow_run","order_id":"...","customer_id":"..."}`.
- **Signal**: Same path with `{"op":"workflow_signal:cancel"}`.
- **Query**: Same path with `{"op":"workflow_query:status"}`.

## Polyglot

| Language  | Location |
|----------|----------|
| TypeScript | `examples/typescript/apps/migrating_temporal` (WASM) |
| Go        | `examples/go/apps/migrating_temporal` (WASM) |
| Python    | `examples/python/apps/migrating_temporal` (WASM) |
| Rust      | `examples/rust/apps/migrating_temporal` (WASM, this dir) |
| Rust (embedded) | `examples/rust/embedded/migrating_temporal` (in-process) |

## References

- [PLAN.md – Workflow behavior](../../../../PLAN.md)
- [Rust embedded example](../../embedded/migrating_temporal) (in-process Node + WorkflowBehavior)
