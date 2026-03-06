# HPC Ensemble (TypeScript WASM) – EFlows4HPC-style

Same behavior as the [Python example](../../../python/apps/migrating_eflows4hpc/README.md): **single actor type** with coordinator (`run`) and workers (`onTasks_ready`). Uses **host.ts** (tuple space) and **host.processGroups** (join/broadcast).

## Abstractions

- **Tuple space**: `host.ts.write(tuple)`, `host.ts.take(pattern)`, `host.ts.readAll(pattern)` — list-in/list-out; use `null` for wildcards.
- **Process group**: `host.processGroups.join(group)`, `host.processGroups.broadcast(group, "tasks_ready", payload)` — `msgType` is used for routing; payload can be data-only.

## Convention

| Instance | Role | Usage |
|----------|------|--------|
| `ensemble:coord-1` | Coordinator | Send `workflow_run` with `ensemble_id`, `num_tasks`. |
| `ensemble:worker-0`, `ensemble:worker-1` | Workers | Wake with `tasks_ready` (empty payload); they join the group and receive coordinator broadcasts. |

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/typescript/apps/migrating_eflows4hpc
./build.sh
./test.sh 8092
```

## API

- **Coordinator run**: `POST .../ensemble:coord-1` with `{"op":"workflow_run","ensemble_id":"e1","num_tasks":20}`.
- **Wake workers**: `POST .../ensemble:worker-0` and `.../ensemble:worker-1` with `{"op":"tasks_ready","ensemble_id":"","num_tasks":0}`.
- **Query**: `{"op":"workflow_query:status"}`.

## References

- [Python EFlows4HPC example](../../../python/apps/migrating_eflows4hpc/README.md)
- [TypeScript SDK – host.ts and process groups](../../../../sdks/typescript/README.md)
- [docs/sdk.md – Cross-SDK consistency](../../../../docs/sdk.md)
