# Parameter sweep (Merlin-style) – Rust WASM

**Single actor type**: coordinator (`workflow_run`) and **worker pool** workers (`work_available`). Uses **elastic pool API** (checkout/checkin) when the pool is configured, with **tuple space** as the work queue. Falls back to **process group** broadcast when the pool is not available.

This Rust example is a WASM component that implements the same behavior as the Python, Go, and TypeScript migrating_merlin examples. It uses in-crate host stubs for local simulation; when deployed, the framework provides the real host bindings.

## Abstractions

- **Pool** = elastic pool `merlin-workers`. Coordinator calls pool_checkout, send(actor_id, "work_available", ...), then pool_checkin after gather. If checkout returns error/empty, falls back to pg_broadcast.
- **Work queue** = tuple space: coordinator ts_write tasks; workers ts_take, run simulation, ts_write results.
- **Flow**: Coordinator scatter → checkout (or broadcast) → workers take/work/write → coordinator gather → checkin.

## Convention

| Instance           | Role        |
|--------------------|-------------|
| `sweep:coord-1`    | Coordinator; send `workflow_run` with `sweep_id`, `num_params`. |
| `sweep:worker-0`, `sweep:worker-1`, ... | Pool workers; wake with `work_available` (empty payload). |

## Quick start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/rust/apps/migrating_merlin
./build.sh
./test.sh 8092
```

## API

- **Coordinator run**: `POST .../sweep:coord-1` with `{"op":"workflow_run","sweep_id":"s1","num_params":20}`.
- **Wake workers**: `POST .../sweep:worker-0`, ... with `{"op":"work_available","sweep_id":"","num_params":0}`.
- **Query**: `{"op":"workflow_query:status"}`.

## References

- [Python migrating_merlin](../../../python/apps/migrating_merlin/README.md) for full comparison and metrics.
- [Rust EFlows4HPC example](../migrating_eflows4hpc/README.md) for similar structure.
