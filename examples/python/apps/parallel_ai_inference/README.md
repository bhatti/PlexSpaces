# Parallel AI Inference (Python WASM)

A PlexSpaces Python WASM example that demonstrates all four parallelization mechanisms in a single application, using simulated ML inference workloads.

## What It Demonstrates

This example showcases the four parallelization primitives available in PlexSpaces, organized around an AI inference use case:

| Mechanism | Actor/Handler | Description |
|-----------|--------------|-------------|
| **ShardGroup scatter-gather** | `BenchmarkActor.run_shard_benchmark`, `OrchestratorWorkflow` (shard mode) | Fan-out inference requests across N shards, collect and aggregate results |
| **Elastic pool checkout/checkin** | `BenchmarkActor.run_pool_benchmark`, `OrchestratorWorkflow` (pool mode) | Dynamic worker pool management with borrow/return semantics |
| **MPI collectives** | `BenchmarkActor.run_collective_benchmark` | `BroadcastShardGroup`, `ReduceShardGroup`, `AllReduceShardGroup`, `BarrierShardGroup` |
| **Process group coordination** | `OrchestratorWorkflow` (collective mode) | Broadcast → Barrier → Scatter-gather → Reduce pipeline |

## Actor Roles

- **`inference_worker`** — `InferenceWorkerActor`: Simulates ML inference with configurable model sizes (`small`, `medium`, `large`). Tracks per-worker latency and request counts.
- **`benchmark`** — `BenchmarkActor`: Runs benchmark suites for each parallelization mechanism and stores results.
- **`orchestrator`** — `OrchestratorWorkflow`: Durable workflow actor that orchestrates multi-mode inference pipelines. Supports `run`, `signal` (scale), and `query` (status).

## Build

```bash
cd examples/python/apps/parallel_ai_inference
./build.sh
```

Requirements: PlexSpaces Python SDK installed (`pip install -e <repo>/sdks/python`).

## Run Tests

```bash
# Against two nodes (default)
./test.sh

# Against a single node
./test.sh localhost:8091

# Against custom nodes
./test.sh localhost:8091 localhost:8092
```

The test script:
1. Builds the WASM if not already built
2. Deploys the application to all specified nodes
3. Tests direct inference worker calls
4. Runs shard benchmark (1 and 2 shards, 5 requests each)
5. Runs collective benchmark (broadcast, barrier, reduce, allreduce)
6. Runs orchestrator in shard mode
7. Runs orchestrator in collective mode
8. Queries orchestrator status
9. Checks application metrics
10. Retrieves all benchmark results

## Expected Output

```
Step 1: Deploy to localhost:8091
Step 2: Test inference worker directly
  Inference worker responded OK
Step 3: Test get_metrics on inference worker
  Metrics endpoint OK
Step 4: Test shard benchmark (2 shards, 5 requests each)
  Shard benchmark OK
Step 5: Test collective benchmark
  Collective benchmark OK
Step 6: Test orchestrator workflow (shard mode)
  Orchestrator shard mode OK
Step 7: Test orchestrator workflow (collective mode)
  Orchestrator collective mode OK
Step 8: Query orchestrator status
  Orchestrator status query OK
Step 9: Check application metrics
  application_id=python-parallel-ai-inference
  message_count=...
Step 10: Retrieve all benchmark results
  All benchmark results retrieved OK
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Python parallel AI inference example passed.
```

## Key PlexSpaces Features Showcased

- `host.create_shard_group` — creates a named group of sharded actors
- `host.scatter_gather` — fan-out query with aggregation
- `host.broadcast_shard_group` — broadcast a message to all shards (blog pattern 14)
- `host.reduce_shard_group` — map + reduce across shards (blog pattern 15)
- `host.all_reduce_shard_group` — all-to-all reduce (blog pattern 15)
- `host.barrier_shard_group` — synchronization barrier (blog pattern 16)
- `host.pool_checkout` / `host.pool_checkin` — elastic actor pool management
- `@workflow_actor` with `@run_handler`, `@signal_handler`, `@query_handler`
- `host.application_metrics_add` — per-actor counter and latency metrics

## Related Documentation

- [Architecture](../../../../docs/architecture.md)
- [Detailed Design](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Examples Gallery](../../../README.md)
