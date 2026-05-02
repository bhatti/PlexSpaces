# Payment Workflow (Go WASM) – Cadence-style

Idempotent payment processing with retries using **Workflow behavior** (run / signal / query), virtual actor, and durability. Cadence-style: retry policies and idempotency keys.

## Purpose

- **WorkflowActor**: `Run(payload)`, `Signal(name, data)`, `Query(name, params)` for payment steps, refund/cancel, and status.
- **Flow**: Validate → Authorize (with retry) → Capture → Settle; refund on signal or failure.
- **Idempotency**: Same `idempotency_key` returns cached result when already completed.
- **Virtual actor**: One workflow per payment (`payment-workflow:pay-1`, etc.) via `virtual_actor` facet.

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/go/apps/migrating_cadence
./build.sh
./test.sh 8091
```

## API

- **Run**: `POST /api/v1/actors/{app_id}/payment-workflow:{payment_id}` with `{"op":"workflow_run","payment_id":"...","idempotency_key":"...","amount_cents":9999}`.
- **Signal**: Same path with `{"op":"workflow_signal:refund"}` or `{"op":"workflow_signal:cancel"}`.
- **Query**: Same path with `{"op":"workflow_query:status"}` or `{"op":"workflow_query:payment_id"}`.

## Native (Cadence) reference

In Cadence, the same payment flow is implemented by starting a **Workflow** and calling **Activities** (Validate, Authorize, Capture, Settle) with a **RetryPolicy**. Workflow and activities are registered with the Cadence server; durability and replay are built-in. For a native code reference, see [Cadence samples](https://github.com/uber/cadence/tree/master/samples) (e.g. Go workflow with activities and retry). This example provides the PlexSpaces equivalent in one WorkflowActor.

## Comparison

| Feature        | Cadence              | PlexSpaces                    |
|----------------|----------------------|-------------------------------|
| Workflow       | ExecuteWorkflow      | WorkflowActor Run/Signal/Query |
| Activities     | ExecuteActivity      | Inline steps + retry in Run   |
| Retry          | RetryPolicy          | Retry loop in authorize step  |
| Idempotency    | Workflow ID / key    | idempotency_key in payload    |
| Durability     | Built-in             | DurabilityFacet               |

## References

- [PLAN.md – migrating_cadence](../../../../PLAN.md)
- [Cadence](https://github.com/uber/cadence) / [Temporal vs Cadence](https://temporal.io/temporal-versus/cadence)
