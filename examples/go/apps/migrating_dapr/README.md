# Background Job Processor (Go WASM) – Dapr-style

Durable job queue with **retries** and **dead-letter queue (DLQ)**. Uses **Workflow** behavior plus **virtual_actor** and **durability** facets; queue and DLQ held in actor state (checkpointed). Optionally uses host KV for external queue storage.

## Purpose

- **WorkflowActor**: `Run(payload)` for enqueue and process actions; `Signal(cancel)`; `Query(status)` for queue depth and DLQ size.
- **Flow**: Enqueue jobs → process one at a time with retry → after max retries move to DLQ.
- **Virtual actor**: One processor per queue ID (`job-processor:default`, `job-processor:batch`, etc.) via virtual_actor facet.
- **Durability**: State (queue, DLQ, metrics) checkpointed by durability facet.

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/go/apps/migrating_dapr
./build.sh
./test.sh 8092
```

## API

- **Run (enqueue)**: `POST /api/v1/actors/{app_id}/job-processor:{queue_id}` with `{"op":"workflow_run","action":"enqueue","job_id":"j1","payload":{...}}`.
- **Run (process)**: Same path with `{"op":"workflow_run","action":"process"}` to process one job from the queue.
- **Signal**: `{"op":"workflow_signal:cancel"}` to request cancel.
- **Query**: `{"op":"workflow_query:status"}` for queue_depth, dlq_size, processed_count, metrics.

## Native (Dapr) reference

The same use case in Dapr is typically implemented with:

- **State Store**: Redis (or another component) stores `job-queue` and `job-dlq` as JSON; app uses Dapr client `GetState`/`SaveState` to enqueue, dequeue, and move failed jobs to DLQ.
- **Workflow** (optional): Dapr Workflow API to define a workflow that runs a “ProcessJob” activity with retry policy and a “MoveToDLQ” compensation.

See **`native/job_processor_dapr.go`** for a reference snippet and comments. That file is not built; it documents the native Dapr pattern for comparison.

## Comparison: Dapr vs PlexSpaces

| Feature           | Dapr                          | PlexSpaces Go                         |
|------------------|-------------------------------|----------------------------------------|
| Job queue        | State Store (Redis, etc.)     | Actor state (or host KV)               |
| Durability       | State store persistence       | Durability facet (checkpoint)          |
| Retries          | Workflow retry policy / app   | In-run retry then DLQ in Run()         |
| DLQ              | App writes to second key      | In-state DLQ slice                     |
| Workflow         | Dapr Workflow API             | WorkflowActor Run/Signal/Query         |
| Scheduling       | Dapr Scheduler / Cron         | host.SendAfter or external trigger     |
| Language         | Any (sidecar)                 | Go WASM in process                     |

## References

- [PLAN.md – migrating_dapr](../../../../PLAN.md)
- [Dapr State Management](https://docs.dapr.io/developing-applications/building-blocks/state-management/)
- [Dapr Workflow](https://docs.dapr.io/developing-applications/building-blocks/workflow/)
