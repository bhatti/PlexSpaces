# Order Fulfillment Workflow (Go WASM)

E-commerce order fulfillment workflow using **Workflow behavior** (run / signal / query) with virtual actor and durability. Same use case as the TypeScript and Python variants.

## Purpose

- **WorkflowActor**: `Run(payload)`, `Signal(name, data)`, `Query(name, params)` for saga execution, cancel signal, and status query.
- **Saga steps**: Validate → Reserve inventory → Charge payment → Ship; compensation on cancel or failure.
- **Virtual actor**: One workflow instance per order (`order-fulfillment:order-1`, etc.) via `virtual_actor` facet.

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/go/apps/migrating_temporal
./build.sh
./test.sh 8091
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
- [Temporal TypeScript SDK](https://docs.temporal.io/dev-guide/typescript)
