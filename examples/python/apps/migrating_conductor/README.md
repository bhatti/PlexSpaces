# Order Saga Workflow (Python WASM) – Netflix Conductor/Orkes style

E-commerce order saga: **reserve_payment** → **reserve_inventory** → **check_discount** → **create_shipment** → **publish_event**, with **compensation** (release/restore/cancel) on failure. Uses **@workflow_actor** with **virtual_actor** and **durability** facets.

## Purpose

- **@workflow_actor(facets=["virtual_actor", "durability"])**: Workflow behavior with virtual actor and durability; app-config supplies both facets.
- **Saga steps**: Payment → Inventory → Discount check → Shipping → Event; optional `simulate_fail_at` in payload to trigger failure and run compensation in reverse order.
- **Virtual actor**: One workflow instance per order (`order-saga:order-1`, etc.) via virtual_actor facet.
- **Durability**: Checkpointing via durability facet for replay-safe execution.

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2 (with venv that has plexspaces-py)
cd examples/python/apps/migrating_conductor
./build.sh
./test.sh 8092
```

## API

- **Run**: `POST /api/v1/actors/{app_id}/order-saga:{order_id}` with `{"op":"workflow_run","order_id":"..."}`. Optional: `"simulate_fail_at":"update_inventory"` to force failure and run compensation.
- **Signal**: Same path with `{"op":"workflow_signal:cancel"}`.
- **Query**: Same path with `{"op":"workflow_query:status"}`.

## Native (Conductor) reference

Conductor uses a **JSON-based workflow DSL**. The same order flow is defined declaratively; the Conductor server orchestrates HTTP, DECISION, and EVENT tasks. See **`native/order_workflow.json`** for the native Conductor workflow definition (payment → inventory → discount check → shipment → notification). This file is not executed by PlexSpaces; it is the reference native implementation for comparison.

## Comparison

| Feature        | Netflix Conductor/Orkes   | PlexSpaces                          |
|----------------|---------------------------|-------------------------------------|
| Workflow       | JSON-based DSL            | @workflow_actor run/signal/query    |
| Saga steps     | HTTP/DECISION/EVENT tasks | Sequential steps in run()           |
| Compensation   | Manual/compensating tasks | Reverse-order compensation in run() |
| Durability     | Built-in                  | Durability facet                    |
| Idle timeout   | N/A                       | virtual_actor idle_timeout          |

## References

- [PLAN.md – migrating_conductor](../../../../PLAN.md)
- [Netflix Conductor](https://github.com/Netflix/conductor)
