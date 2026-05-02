# Parameter sweep (Merlin-style) – Go WASM

**Single actor type**: coordinator (`Run`) and **worker pool** workers (`Handle` with `work_available`). Uses **elastic pool API** (`host.PoolCheckout` / `host.PoolCheckin`) when the pool is configured, with **tuple space** (`host.TS()`) as the work queue. Falls back to **process group** broadcast when the pool is not available.

## Abstractions

- **Pool** = elastic pool `merlin-workers`. Coordinator uses `host.PoolCheckout("merlin-workers", timeoutMs)` to get workers, sends each `work_available` via `host.Send(actorID, ...)`, then `host.PoolCheckin(...)` after gather. If the pool is not configured, falls back to `host.PG().Broadcast(workerGroup, "work_available", payload)`.
- **Work queue** = tuple space: coordinator writes tasks with `host.TS().Write(...)`; workers `host.TS().Take(pattern)`, run simulation, `host.TS().Write(...)` results.
- **Flow**: Coordinator scatter → checkout workers (or broadcast) → workers take/work/write → coordinator gather → checkin workers.

## Convention

| Instance           | Role        |
|--------------------|-------------|
| `sweep:coord-1`    | Coordinator; send `workflow_run` with `sweep_id`, `num_params`. |
| `sweep:worker-0`, `sweep:worker-1`, ... | Pool workers; wake with `work_available` (empty payload); they join the pool and pull work from tuple space. |

## Quick start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/go/apps/migrating_merlin
./build.sh
./test.sh 8091
```

## API

- **Coordinator run**: `POST .../sweep:coord-1` with `{"op":"workflow_run","sweep_id":"s1","num_params":20}`.
- **Wake workers**: `POST .../sweep:worker-0`, `.../sweep:worker-1`, ... with `{"op":"work_available","sweep_id":"","num_params":0}`.
- **Query**: `{"op":"workflow_query:status"}`.

## References

- [Python migrating_merlin](../python/apps/migrating_merlin/README.md) for full comparison and metrics.
- [Go SDK](../../../../sdks/go/README.md)
