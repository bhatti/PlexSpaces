# Order Fulfillment Workflow (Python WASM)

E-commerce order fulfillment workflow using **@workflow_actor** (run / signal / query) with virtual actor and durability. Same use case as the TypeScript and Go variants.

## Purpose

- **@workflow_actor(facets=["virtual_actor", "durability"])**: Workflow with facets declared in code; app-config supplies facet config (virtual_actor + durability).
- **Saga steps**: Validate → Reserve inventory → Charge payment → Ship; compensation on cancel or failure.
- **Virtual actor**: One workflow instance per order (virtual_actor facet in app-config).

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2 (with venv that has plexspaces-py)
cd examples/python/apps/migrating_temporal
./build.sh
./test.sh 8092
```

## API

- **Run**: `POST /api/v1/actors/{app_id}/order-fulfillment:{order_id}` with `{"op":"workflow_run","order_id":"...","customer_id":"..."}`.
- **Signal**: Same path with `{"op":"workflow_signal:cancel"}`.
- **Query**: Same path with `{"op":"workflow_query:status"}`.

## Polyglot

| Language  | Path |
|----------|------|
| TypeScript | `examples/typescript/apps/migrating_temporal` |
| Go        | `examples/go/apps/migrating_temporal` |
| Python    | `examples/python/apps/migrating_temporal` |
| Rust (embedded) | `examples/rust/embedded/migrating_temporal` |

## References

- [PLAN.md – Workflow behavior](../../../../PLAN.md)
- [Temporal docs](https://docs.temporal.io)
