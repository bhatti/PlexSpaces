# HPC Ensemble Simulation (Python WASM) – EFlows4HPC-style

**Single actor type** with two roles: **coordinator** (`run`) and **workers** (`tasks_ready` handler). Uses **tuple space** (`host.ts`) and **process group** (join/broadcast).

## Abstractions

### Tuple space (`host.ts` — list-in, list-out)

- **Scatter**: Coordinator writes task tuples: `host.ts.write([prefix, ensemble_id, "task", task_id, param])`. Use `None` in patterns for wildcards.
- **Workers**: Each worker **takes** one task: `t = host.ts.take([prefix, ensemble_id, "task", None, None])`; if `t`, process and `host.ts.write([..., "result", task_id, result])`.
- **Gather**: Coordinator **reads all** result tuples: `host.ts.read_all([prefix, ensemble_id, "result", None, None])` until `num_tasks` results.

### Process group

- **Workers** join on first use: `host.process_groups.join("ensemble-workers")`.
- **Coordinator** notifies workers: `host.process_groups.broadcast("ensemble-workers", "tasks_ready", {"ensemble_id": ..., "num_tasks": ...})`. The `msg_type` (`"tasks_ready"`) is used for routing; payload is data-only.
- Each worker receives the broadcast and runs `on_tasks_ready`, pulling tasks from tuple space until none remain.

## Convention

| Instance        | Role        | Usage |
|----------------|------------|--------|
| `ensemble:coord-1` | Coordinator | Send `workflow_run` with `ensemble_id`, `num_tasks`. |
| `ensemble:worker-0`, `ensemble:worker-1` | Workers | Wake with `tasks_ready` (empty payload) so they join the group; then they receive coordinator broadcasts. |

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/python/apps/migrating_eflows4hpc
./build.sh
./test.sh 8091
```

## API

- **Coordinator run**: `POST .../ensemble:coord-1` with `{"op":"workflow_run","ensemble_id":"e1","num_tasks":20}`.
- **Wake workers**: `POST .../ensemble:worker-0` and `.../ensemble:worker-1` with `{"op":"tasks_ready","ensemble_id":"","num_tasks":0}`.
- **Signal**: `{"op":"workflow_signal:cancel"}`.
- **Query**: `{"op":"workflow_query:status"}`.

## Native (EFlows4HPC) reference

See **`native/eflows4hpc_ref.md`**. This example uses PlexSpaces **host.ts** (tuple space) and **process group** for scatter/gather and worker notification.

## Comparison

| Feature       | EFlows4HPC / Nextflow | PlexSpaces Python |
|---------------|------------------------|-------------------|
| Scatter       | Channel.from / emit    | `host.ts.write` task tuples |
| Notify workers| Job scheduler / queues | `host.process_groups.broadcast(..., "tasks_ready", payload)` |
| Workers       | Cluster jobs            | `host.process_groups.join` + `host.ts.take` loop |
| Gather        | collect / Channel       | `host.ts.read_all` result pattern |

## References

- [PLAN.md – migrating_eflows4hpc](../../../../PLAN.md)
- [Python SDK – host.ts and process groups](../../../../sdks/python/README.md)
- [EFlows4HPC](https://eflows4hpc.eu/)
