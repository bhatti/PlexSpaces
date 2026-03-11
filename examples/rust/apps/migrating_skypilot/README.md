# SkyPilot → PlexSpaces: Multi-Cloud ML Orchestration (Rust WASM App)

Rust WASM app: SkyPilot-style multi-cloud AI workload orchestration with cost optimization. Deploy via HTTP; test with `./scripts/server.sh` then `./test.sh [HTTP_PORT]`.

## Use Case

A scheduler actor that:

- Finds cheapest available resources across AWS, GCP (simulated catalog)
- Matches task requirements (GPU, CPU, memory) to instances
- Supports ops: `submit_task`, `get_best_resources`, `get_status`

## PlexSpaces Abstractions

- **GenServer** – Request-reply for task scheduling
- **Virtual actor** – Lazy activation, one logical scheduler per instance id
- **Durability** – Checkpointed state for recovery

## API (HTTP JSON)

| Op | Payload | Returns |
|----|---------|--------|
| `submit_task` | `{"op":"submit_task","task":{...}}` | `{"allocation":{...}}` or `{"error":"..."}` |
| `get_best_resources` | `{"op":"get_best_resources","task":{...}}` | `{"allocation":{...}}` or `{"error":"..."}` |
| `get_status` | `{"op":"get_status"}` | See below (includes metrics). |

Task shape: `task_id`, `task_type`, `gpu_required`, `gpu_memory_gb`, `cpu_cores`, `memory_gb`, `cloud_preference` (optional).

### Metrics (get_status)

`get_status` returns coordination vs computation metrics (mandatory for all examples):

- **queue_size**, **running** – Current queue and running task counts.
- **tasks_scheduled** – Total tasks successfully scheduled (throughput).
- **total_compute_ms** – Time spent in compute (catalog matching).
- **total_coord_ms** – Coordination overhead (message handling, state).
- **compute_pct**, **coord_pct** – Percent of time in compute vs coord.
- **granularity_ratio** – compute/coordinate (target ≥ 10×).

## Metrics & Benchmarks

- **Coordination vs computation**: Each `submit_task` / `get_best_resources` adds simulated compute ms (catalog scan) and coord ms (overhead); accumulated in state.
- **Cost analysis**: `get_status` returns `compute_pct` and `coord_pct`.
- **Granularity ratio**: `granularity_ratio` = total_compute_ms / total_coord_ms (target ≥ 10×).
- **Non-trivial run**: `test.sh` runs a batch of 20 submit_task calls (~2+ s wall time) and prints benchmarks.

## Build and Test

From this directory:

```bash
./build.sh
./test.sh 8092
```

Requires: Rust (wasm32-wasip1), `wasm-tools`, WASI adapter (e.g. from `jco`). Node must be running: `./scripts/server.sh` from repo root.

## Comparison

| Feature | SkyPilot | PlexSpaces (this app) |
|---------|----------|------------------------|
| Multi-cloud | AWS, GCP, Azure | Simulated catalog (AWS, GCP) |
| Cost optimization | Finds cheapest | Cost-aware selection |
| Resource matching | By accelerators/CPU/mem | By GPU/CPU/memory fields |
| Deployment | CLI / YAML | WASM app deploy via HTTP |

## Native reference

See `native/skypilot_ref.md` for SkyPilot concepts and mapping to this actor.

## References

- [SkyPilot](https://skypilot.readthedocs.io/)
- [PlexSpaces architecture](../../../../docs/architecture.md)
