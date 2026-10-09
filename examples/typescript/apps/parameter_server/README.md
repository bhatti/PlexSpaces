# Parameter Server (TypeScript WASM)

Distributed parameter server for synthetic neural network training — demonstrating scatter-gather coordination, compute vs. coordination breakdown, and strong-scaling benchmarks across multiple worker counts.

## What it demonstrates

- **Scatter-gather training loop**: `LeaderActor` creates a shard group of `WorkerActor` instances, broadcasts current weight matrices in each round via `host.scatterGather`, receives gradients from all workers, averages them, and applies SGD.
- **Compute vs. coordination metrics**: Every training round reports `compute_time_ms` (gradient averaging + weight update) and `coordination_time_ms` (scatter-gather round trip), enabling granularity ratio analysis.
- **Strong-scaling benchmark** (`run_scaling_benchmark`): Varies worker count across `[2, 4, 8, 16]`, runs fixed iterations per configuration, and reports speedup, efficiency %, and Amdahl parallel fraction — same quality bar as `feature_store` and `data_lake_rag`.
- **Pure-language math**: No NumPy, no ML libraries. Workers compute synthetic mini-batch gradients using integer arithmetic — the point is coordination pattern, not ML accuracy.

## Architecture

```
LeaderActor (GenServer, virtual_actor)
  │
  ├─ createShardGroup("worker", N workers)
  │
  └─ for each iteration:
       scatterGather(query: { op: "compute_gradient", weights: {w1, w2} })
         │
         ├─ WorkerActor #0 → gradient #0
         ├─ WorkerActor #1 → gradient #1
         │     ...
         └─ WorkerActor #N → gradient #N
       aggregate (average gradients)
       apply SGD update to w1, w2
```

## Key PlexSpaces APIs

| API | Usage |
|-----|-------|
| `host.createShardGroup()` | Provision N worker actors with registry-based placement |
| `host.scatterGather()` | Fan-out weights, collect gradients in one call |
| `host.applicationMetricsAdd()` | Record compute/coord latency, samples processed |
| `host.nowMs()` | High-resolution timing for compute vs coord breakdown |

## Files

| File | Purpose |
|------|---------|
| `parameter_server_actor.ts` | `LeaderActor` + `WorkerActor` + `ActorRouter` |
| `app-config.toml` | Supervisor config; 1 leader + N workers, virtual_actor facet |
| `build.sh` | tsc → esbuild bundle → jco componentize → `.wasm` |
| `build-bundle.mjs` | esbuild config (esm, neutral platform, external plexspaces:\*) |
| `test.sh` | Deploy → train → validate zero errors → scaling benchmark table |
| `undeploy.sh` | Remove application from all nodes |

## Running

```bash
# Build WASM
./build.sh

# Test against running PlexSpaces nodes (default: 8091 + 8094)
./test.sh

# Single-node test
./test.sh 8091

# Override parameters
ITERATIONS=30 WORKER_COUNT=16 BATCH_SIZE=1024 ./test.sh
```

## Expected output

```
Step 2: Trigger training on localhost:8091
Step 3: Metrics
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Parameter Server (TypeScript WASM)
  param_count=6464  worker_count=2  iterations=20  batch_size=256
  node_count=1  actor_count=3  leader_node=test-node-8091
  wall_ms=~8000  compute_ms=~2 (0.0%)  coord_ms=~8000 (100.0%)
  granularity=0.00x
  avg_worker_latency_ms=~400  max_worker_latency_ms=~450
  gradient_ops=40  samples_processed=10240  weight_updates=20
  errors=0

Step 4: Strong scaling benchmark (workers: 2,4,8,16)
════════════════════════════════════════════════════════════════════════════════════════════════
  Parameter Server (TypeScript) — Strong Scaling  param_count=6464  batch_size=256
════════════════════════════════════════════════════════════════════════════════════════════════
   Workers  Total ms  Compute ms  Coord ms  Comp%   Gran   Smpl/s  Speedup   Eff%  ParFrac  Errs
────────────────────────────────────────────────────────────────────────────────────────────────
         2      4048           1      4047     0%   0.00      632    1.00x   100%    0.00      0
         4      7913           2      7911     0%   0.00      647    1.02x    51%    0.05      0
         8      8422           1      8421     0%   0.00     1216    1.92x    48%    0.64      0
        16      8901           2      8899     0%   0.00     2301    3.64x    46%    0.83      0
════════════════════════════════════════════════════════════════════════════════════════════════
  ✓ Scaling benchmark passed
```

**Speedup is throughput-based** (samples/sec at N workers ÷ samples/sec at 2 workers). 3.64× at 16 workers means the cluster handles 3.64× more gradient computations per second — directly mapping to faster convergence on larger datasets. Coordination dominates on a single node; multi-node clusters shift the granularity ratio as workers dispatch in parallel.

## Configuration

```toml
# app-config.toml — key parameters
[supervisor.children.args]  # leader
num_workers = "8"
learning_rate = "0.01"
input_dim = "100"
hidden_dim = "64"
batch_size = "512"
```

Network dimensions: `param_count = input_dim × hidden_dim + hidden_dim = 100 × 64 + 64 = 6,464`. Increase `input_dim` and `hidden_dim` to stress larger payloads.

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Examples gallery](../../README.md)
- Related: [`examples/python/apps/parameter_server/`](../../../python/apps/parameter_server/), [`examples/rust/apps/parameter_server/`](../../../rust/apps/parameter_server/)
