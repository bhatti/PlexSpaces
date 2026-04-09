# Parameter sweep (Merlin-style) – Rust WASM

**Single actor type**: coordinator (`workflow_run`) and **worker pool** workers (`work_available`). Uses **elastic pool API** (checkout/checkin) when the pool is configured, with **tuple space** as the work queue. Falls back to **process group** broadcast when the pool is not available.

This Rust example is a WASM component built with **`wit_bindgen::generate!`** (world `actor-world`), **`#[gen_server_actor(wasm)]`**, and **`plexspaces::actor::host`** (`pool_checkout` / `pool_checkin`, `ts_*`, `pg_*`, `send`, `application_metrics_add`) — the same SDK + WIT pattern as [`migrating_temporal`](../migrating_temporal/README.md) and [`migrating_eflows4hpc`](../migrating_eflows4hpc/README.md).

## Abstractions

- **Pool** = elastic pool `merlin-workers`. Coordinator calls `host::pool_checkout`, `host::send(..., "work_available", ...)`, then `host::pool_checkin` after gather. If checkout returns error/empty, falls back to `host::pg_broadcast`.
- **Work queue** = tuple space: coordinator `host::ts_write` tasks; workers `host::ts_take`, run simulation, `host::ts_write` results.
- **Flow**: Coordinator scatter → checkout (or broadcast) → workers take/work/write → coordinator gather → checkin.

```mermaid
flowchart LR
  C[coord workflow_run] --> S[ts_write tasks]
  S --> P[pool_checkout / pg_broadcast work_available]
  P --> W[workers ts_take / ts_write results]
  W --> G[coord ts_read_all gather]
  G --> I[pool_checkin]
```

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
