# Temporal → PlexSpaces: Order Fulfillment Workflow (TypeScript)

E-commerce order fulfillment workflow demonstrating **Workflow behavior** (run / signal / query) with durability—Temporal-style saga with compensation.

## Purpose

- **Workflow behavior**: One workflow instance per order; main execution (`run`), external signals (`signal`, e.g. cancel), read-only queries (`query`, e.g. status).
- **Saga**: Validate → Reserve inventory → Charge payment → Ship; on failure or cancel, compensate (rollback).
- **Durability**: State persisted via getState/setState (DurabilityFacet in app-config).
- **Unified design**: Same message types and run/signal/query contract as Rust (`#[workflow_actor]`) and Python (`@workflow_actor`).

## Real-World Use Case

**E-commerce order fulfillment** (Temporal/Step Functions style):

- Multi-step flow with compensation.
- Cancel signal to abort and run compensation.
- Status query for read-only order state.
- Durable so state survives crashes and restarts.

## PlexSpaces Abstractions

- **Workflow behavior** – run / signal / query (aligned with `crates/behavior` Workflow trait).
- **DurabilityFacet** – state checkpointing (app-config.toml).
- **TypeScript SDK** – `WorkflowActor<TState>` with `run()`, `signal()`, `query()`; routing by `msgType`: `workflow_run`, `workflow_signal:name`, `workflow_query:name`.

## Comparison: Temporal vs PlexSpaces

| Feature | Temporal (TypeScript) | PlexSpaces (TypeScript) |
|--------|------------------------|--------------------------|
| **Workflow definition** | `workflow.run()` async function | `WorkflowActor.run(payload)` |
| **Signals** | `defineSignal('cancel')` + `setHandler` | `signal(name, data)` (e.g. `workflow_signal:cancel`) |
| **Queries** | `defineQuery('status')` + `setHandler` | `query(name, params)` (e.g. `workflow_query:status`) |
| **Activities** | `proxyActivities` + separate worker | Inline steps or `host.ask()` to other actors |
| **Durability** | Built-in (replay) | DurabilityFacet + getState/setState |
| **Deployment** | Temporal server + workers | PlexSpaces node + WASM deploy |

## Native Reference

A **native Temporal-style** reference (TypeScript) is in this example's `native/` folder for comparison:

- **Path**: [native/temporal_order_workflow.ts](native/temporal_order_workflow.ts)
- It documents the typical Temporal pattern (workflow, activities, signals, queries) and how the same behavior maps to this PlexSpaces implementation.

## SDK Consistency: Annotations vs Conventions

| Language | Workflow declaration | Run / Signal / Query |
|----------|----------------------|-----------------------|
| **Rust** | `#[workflow_actor]` + `#[plexspaces_handlers(workflow)]` | `#[run_handler]`, `#[signal_handler("name")]`, `#[query_handler("name")]` |
| **Python** | `@workflow_actor` | Implement `run(payload)`, `signal(name, data)`, `query(name, params)`; dispatch via `dispatch_message()` |
| **TypeScript** | Extend `WorkflowActor<TState>` (or implement `run`/`signal`/`query` on `PlexSpacesActor`) | Same method names; `handle()` routes by `msgType` |

Message types are the same in all SDKs: `workflow_run`, `workflow_signal:<name>`, `workflow_query:<name>`. **Payload key**: canonical `message_type` (aliases: `op`, `msg_type`; resolved in that order in Rust, Python, TypeScript, Go).

## API (Client)

- **Run workflow**: `POST .../order-fulfillment:{orderId}` with `{"op": "workflow_run", "order_id": "...", "customer_id": "..."}`.
- **Signal**: `POST .../order-fulfillment:{orderId}` with `{"op": "workflow_signal:cancel"}`.
- **Query**: `POST .../order-fulfillment:{orderId}` with `{"op": "workflow_query:status"}` (or use ask for request-reply).

## Quick Start

```bash
# Terminal 1: start node (from repo root)
./scripts/server.sh

# Terminal 2: build and test
cd examples/typescript/apps/migrating_temporal
./build.sh
./test.sh 8091
```

## Metrics and Benchmarks (per PLAN criteria)

- **Coordination vs computation latency**: `test.sh` prints compute time, coordination time, and % of each.
- **Granularity ratio**: Compute/coordinate ≥ 10x (displayed as "Granularity" in test output).
- **Benchmark metrics**: Orders/sec, batch wall time; sample order shows `total_compute_ms`, `total_coord_ms`, `steps_count`.
- **Non-trivial data**: Batch of 50 orders; target 2+ seconds total run so metrics are meaningful.
- **Section headers**: Test output includes "Benchmarks (coord vs compute)" and "Batch total wall".

## Files

- `order_fulfillment_actor.ts` – Workflow actor (run/signal/query).
- `app-config.toml` – Supervisor, `behavior_kind = "Workflow"`, DurabilityFacet.
- `build.sh` – Build WASM from TypeScript.
- `test.sh` – Deploy and run workflow tests.

## References

- [Temporal TypeScript SDK](https://docs.temporal.io/dev-guide/typescript)
- [PLAN.md – Workflow behavior and migrating_temporal](../../../../PLAN.md)
- [Native reference: temporal_order_workflow.ts](native/temporal_order_workflow.ts)
- [PlexSpaces behavior: Workflow trait](../../../../crates/behavior/src/mod.rs)
