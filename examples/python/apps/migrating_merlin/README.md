# Parameter sweep (Merlin-style) – worker pool, elastic size

**Single actor type**: coordinator (`run`) and **worker pool** workers (`work_available` handler). Uses **elastic pool API** (`host.pool_checkout` / `host.pool_checkin`) when the pool is configured, with **tuple space** as the work queue. Falls back to **process group** broadcast when the pool is not available.

## Abstractions

### Worker pool (elastic size)

- **Pool** = elastic pool `merlin-workers`. Coordinator uses `host.pool_checkout("merlin-workers", timeout_ms)` to get workers, sends each `work_available`, then `host.pool_checkin(...)` after gather. If the pool is not configured, falls back to `host.process_groups.broadcast(WORKER_GROUP, "work_available", payload)` (workers join the process group for that path).
- **Work queue** = tuple space: coordinator writes parameter-sweep tasks; workers take tasks, run simulation, write results.
- **Flow**: Coordinator scatter → checkout workers from pool (or broadcast) → workers take/work/write → coordinator gather → checkin workers.

### Tuple space (`host.ts`)

- **Scatter**: Coordinator writes task tuples: `host.ts.write([prefix, sweep_id, "task", param_id, {...}])`.
- **Workers**: `host.ts.take(pattern)` → run simulation → `host.ts.write([..., "result", param_id, result])`.
- **Gather**: Coordinator `host.ts.read_all([..., "result", None, None])`.

## Convention

| Instance           | Role        | Usage |
|--------------------|------------|--------|
| `sweep:coord-1`    | Coordinator | Send `workflow_run` with `sweep_id`, `num_params`. |
| `sweep:worker-0`, `sweep:worker-1`, ... | Pool workers (elastic) | Wake with `work_available` (empty payload) so they join the pool; they then receive coordinator broadcasts and pull work from tuple space. |

## Quick start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/python/apps/migrating_merlin
./build.sh
./test.sh 8092
```

## API

- **Coordinator run**: `POST .../sweep:coord-1` with `{"op":"workflow_run","sweep_id":"s1","num_params":20}`.
- **Wake workers (elastic pool)**: `POST .../sweep:worker-0`, `.../sweep:worker-1`, ... with `{"op":"work_available","sweep_id":"","num_params":0}`. Pool size = number of workers woken.
- **Signal**: `{"op":"workflow_signal:cancel"}`.
- **Query**: `{"op":"workflow_query:status"}`.

## Metrics

`test.sh` runs with pool size 2, then scales to 5 (elastic), runs batch sweeps with larger data (80 params first sweep, 24 params/sweep in batch), and prints:

- **Sweep ID**, status, params completed
- **Worker pool size** (number of workers)
- **Data size**: params count and approximate bytes (task + result payload)
- **Throughput**: **Req/sec** (sweeps/sec), **Params/sec**
- **Compute ms** and **Coord ms** (and %)
- **Batch wall** (ms)

## Native (Merlin) reference

See **`native/merlin_ref.md`** for Merlin/HPC parameter-sweep concepts and the PlexSpaces mapping.

## Comparison

| Feature        | Merlin / HPC        | PlexSpaces (this example)                    |
|----------------|---------------------|----------------------------------------------|
| Worker pool    | Distributed workers | Elastic pool `merlin-workers` (checkout/checkin); fallback: process group |
| Task queue    | Queue / broker      | `host.ts` (write tasks, take/read_all)      |
| Notify workers| Queue or broadcast  | `host.pool_checkout` → `host.send(actor_id, "work_available", ...)` → `host.pool_checkin`; or `host.process_groups.broadcast` |
| Elastic size  | Scale workers       | Pool size = workers in pool; wake more `sweep:worker-N` for broadcast fallback |

## References

- [PLAN.md – migrating_merlin](../../../../PLAN.md)
- [Python SDK – host.ts and process groups](../../../../sdks/python/README.md)
- [Merlin (LLNL)](https://github.com/LLNL/merlin)
