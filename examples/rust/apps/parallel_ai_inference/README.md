# Parallel AI Inference (Rust WASM)

Rust WASM port of the Python `parallel_ai_inference` example. It demonstrates the same four PlexSpaces parallelization primitives with the standard Rust deployable-app pattern and the same scaling-benchmark surface as the Python version:

- `#[gen_server_actor(wasm)]`
- `#[plexspaces_handlers(wasm)]`
- actor-world WIT host APIs (`create_shard_group`, `scatter_gather`, collectives, pools, process groups, application metrics)

## What It Demonstrates

| Mechanism | Role / handler | Description |
|-----------|----------------|-------------|
| ShardGroup scatter-gather | `benchmark.run_shard_benchmark`, `orchestrator.workflow_run` (`mode=shard`) | Fan-out inference requests across N shards and collect responses |
| Elastic pool checkout/checkin | `benchmark.run_pool_benchmark`, `orchestrator.workflow_run` (`mode=pool`) | Dynamic pool checkout/checkin using the WIT pool APIs |
| MPI-style collectives | `benchmark.run_collective_benchmark`, `orchestrator.workflow_run` (`mode=collective`) | Broadcast, barrier, reduce, and all-reduce over shard groups |
| Process groups | `metrics_event`, worker event fan-out | Lightweight coordination and observability broadcast group |

## Layout

- [src/lib.rs](/Users/shahzadbhatti/workspace/myspaces/examples/rust/apps/parallel_ai_inference/src/lib.rs)
  Shared WIT bridge, state model, metrics helpers, benchmark math, and support roles (`metrics_event`, `circuit_breaker`).
- [src/worker.rs](/Users/shahzadbhatti/workspace/myspaces/examples/rust/apps/parallel_ai_inference/src/worker.rs)
  `inference_worker` behavior: simulated compute, per-request metrics, and numeric stats for reduction.
- [src/leader.rs](/Users/shahzadbhatti/workspace/myspaces/examples/rust/apps/parallel_ai_inference/src/leader.rs)
  `benchmark` and `orchestrator` behaviors: shard sweep benchmarking, collectives, and workflow-style orchestration.

## Roles

- `inference_worker`
  Simulates AI inference for `small`, `medium`, or `large` model profiles and records per-request compute vs coordination cost.
- `benchmark`
  Runs shard, scaling, pool, and collective benchmarks and stores historical results.
- `orchestrator`
  Coordinates higher-level inference workflows across shard or pool modes and exposes workflow-style query/signal operations.
- `metrics_event`
  Receives fire-and-forget completion events from workers through a process group.
- `circuit_breaker`
  Tracks worker health with simple closed/open/half-open transitions.

## Build

```bash
cd examples/rust/apps/parallel_ai_inference
./build.sh
```

## Test

```bash
# Default two-node run
./test.sh

# Single node
./test.sh localhost:8091

# Custom nodes
./test.sh localhost:8091 localhost:8094
```

The test script:
1. builds the WASM if needed
2. deploys the application to all target nodes
3. tests direct worker inference and worker metrics
4. runs a small shard benchmark smoke test
5. runs a shard-sweep scaling benchmark
6. runs a collective benchmark
7. runs orchestrator shard mode
8. runs orchestrator collective mode
9. queries orchestrator status
10. fetches benchmark result history

## Scaling benchmark

`test.sh` drives the shard sweep at runtime instead of baking it into `app-config.toml`. That keeps deployment topology stable while letting you compare different workloads across the same node set.

Useful runtime knobs:

```bash
SCALING_SHARDS=2,4,6,8,16,32,64,128 \
SCALING_REQUESTS_PER_SHARD=8 \
SCALING_WARMUP_REQUESTS=2 \
SCALING_LOGICAL_ACTORS=1000 \
SCALING_DATA_SIZE_BYTES=65536 \
SCALING_MODEL_TYPE=large \
SCALING_WORK_MULTIPLIER=20 \
./test.sh localhost:8091 localhost:8094
```

For blog-style runs, keep the node list, logical actor count, payload size, and work multiplier fixed, and only change `SCALING_SHARDS`.

Each scaling row reports:

- `throughput_rps`
- `p50_latency_ms`
- `p95_latency_ms`
- `p99_latency_ms`
- `compute_time_ms`
- `coordination_time_ms`
- `compute_pct`
- `coordination_pct`
- `granularity_ratio`
- `parallel_efficiency_pct`
- `worker_node_count`
- `remote_nodes_with_work`

Expected shape on real hardware is the usual tradeoff: throughput should rise with shard count until coordination dominates, while parallel efficiency gradually falls off.

## Notes

- Like the Python example, the deployable topology is declared through `app-config.toml`, but the actor behavior is implemented with Rust SDK annotations and WIT host APIs rather than manual behavior wiring.
- This Rust WASM version keeps the externally tested API surface aligned with Python, including the new `run_scaling_benchmark` benchmark entrypoint.

## Related

- [Python version](../../../python/apps/parallel_ai_inference/README.md)
- [Architecture](../../../../docs/architecture.md)
- [Examples gallery](../../../README.md)
