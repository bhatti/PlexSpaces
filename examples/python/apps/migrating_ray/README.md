# Ray Parameter Server - Distributed ML Training

Demonstrates **distributed ML training** using the Ray parameter server pattern.

**Real-world use case**: Distributed training (TensorFlow, PyTorch, Ray Train) where a
centralized parameter server manages model weights and elastic worker pools compute
gradients in parallel.

## Architecture

```
                ┌────────────────────┐
                │  Parameter Server  │  Centralized weights (100x64 model)
                │  (host.ask workers)│
                └────────┬───────────┘
        ┌────────┬───────┼───────┬────────┐
        │        │       │       │        │
   ┌────▼──┐ ┌──▼────┐ ┌▼─────┐ ┌▼──────┐
   │Worker0│ │Worker1│ │Worker2│ │Worker3│  Compute gradients
   │ 2000  │ │ 2000  │ │ 2000  │ │ 2000  │  on data shards
   │samples│ │samples│ │samples│ │samples│
   └───────┘ └───────┘ └───────┘ └───────┘
```

**Training loop** (inside ParameterServer via `host.ask`):
1. Fan-out: send weights to all workers (coordination)
2. Workers: forward+backward pass on data shard (computation)
3. Fan-in: aggregate gradients, SGD update (computation)

## Quick Start

```bash
# Terminal 1: Start PlexSpaces node
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:7992

# Terminal 2: Build and run
cd examples/python/apps/migrating_ray
./build.sh        # Builds ray_actors.wasm
./test.sh 8092    # Deploy + train (HTTP port = gRPC 7992 + 100)
```

## PlexSpaces SDK Features

| Feature | How Used |
|---------|----------|
| `@actor` | Marks `ParameterServer` and `DataWorker` as actors |
| `state()` | Persistent state (weights, iteration, benchmarks) |
| `@handler()` | Routes `train`, `get_weights`, `compute_gradients` |
| `@init_handler` | Initialize from framework config (child_spec args) |
| `host.ask()` | Inter-actor request-reply (PS -> workers) |
| `host.now_ms()` | Timing for coordination/computation benchmarks |
| `ACTOR_ROLES` | Multi-actor class routing in one WASM module |

## Multi-Actor ApplicationSpec

One WASM module serves both actor types via `ACTOR_ROLES` mapping:

```python
ACTOR_ROLES = {
    "parameter-server": ParameterServer,  # exact match
    "data-worker": DataWorker,            # prefix match: data-worker-0, -1, -2, -3
}
```

The framework passes `{"actor_id": "parameter-server", "args": {...}}` in init config.

## Comparison: Ray vs PlexSpaces

| Feature | Ray | PlexSpaces |
|---------|-----|------------|
| Actor model | `@ray.remote` | `@actor` + `ACTOR_ROLES` |
| State | Class attributes | `state()` fields |
| RPC | `.remote()` + `ray.get()` | `host.ask(worker, op, payload)` |
| Scheduling | Ray scheduler | ApplicationSpec supervisor |
| Scaling | `ray.autoscaler` | Add workers to app-config.toml |
| Language | Python-only | Python, TypeScript, Go, Rust |

## Files

| File | Description |
|------|-------------|
| `ray_actors.py` | ParameterServer + DataWorker actors |
| `app-config.toml` | ApplicationSpec (1 PS + 4 workers) |
| `build.sh` | Build WASM module |
| `test.sh` | Deploy + run training + benchmarks |
| `native/parameter_server.py` | Native Ray reference |
