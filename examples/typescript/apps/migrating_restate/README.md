# Exactly-Once Payment (TypeScript WASM) – Restate-style

Idempotent payment processing with **durability** (journaling replay). Same `idempotency_key` returns cached result (exactly-once). Uses **Workflow** behavior with **virtual_actor** and **durability** facets.

## Purpose

- **WorkflowActor**: `run(payload)` with `idempotency_key`, `amount_cents`, `from_account`, `to_account`; duplicate key returns cached result.
- **Durability**: State checkpointed so replay does not double-execute steps.
- **Steps**: validate → debit → credit → confirm (simulated); result stored keyed by idempotency_key.
- **Virtual actor**: One actor per service/entity (e.g. `payment:svc-1`); multiple idempotency keys cached in state.

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/typescript/apps/migrating_restate
./build.sh
./test.sh 8091
```

## API

- **Run**: `POST /api/v1/actors/{app_id}/payment:{id}` with `{"op":"workflow_run","idempotency_key":"key-1","amount_cents":9999,"from_account":"acc-a","to_account":"acc-b"}`.
- **Signal**: `{"op":"workflow_signal:cancel"}`.
- **Query**: `{"op":"workflow_query:status"}`.

## Native (Restate) reference

Restate provides **durable execution** with journaling: each step is journaled so on replay, completed steps return cached results and only new steps run. Idempotency is typically achieved by using request id or idempotency key as the execution id. See **`native/restate_payment.md`** for a short native pattern description. This example provides the PlexSpaces equivalent with WorkflowActor + durability + idempotency in state.

## Comparison: Restate vs PlexSpaces

| Feature        | Restate                    | PlexSpaces TypeScript              |
|----------------|----------------------------|------------------------------------|
| Durability     | Journal per step           | Durability facet (checkpoint state)|
| Idempotency    | Execution id / key         | idempotency_key → cached in state  |
| Replay         | Replay from journal        | Restore state, skip duplicate keys |
| Service        | Restate service + handlers | WorkflowActor run/signal/query     |
| Language       | Any (SDK)                  | TypeScript WASM                    |

## References

- [PLAN.md – migrating_restate](../../../../PLAN.md)
- [Restate](https://restate.dev)
