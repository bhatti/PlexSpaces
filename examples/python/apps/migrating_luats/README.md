# LuaTS → PlexSpaces: Event-Driven Data Pipeline (CDC)

Event-driven data pipeline with Linda-style coordination: events as tuples in TupleSpace, take/read by pattern, timer-style window flush.

## Use Case

CDC-style (change data capture) event stream: producers write events as tuples; consumers take or read by pattern. Window flush (periodic or timer-triggered) aggregates events in a time or count window. Demonstrates TupleSpace as event buffer and coord vs compute metrics.

## Abstractions

| Abstraction | Usage |
|-------------|--------|
| **TupleSpace** | Event buffer: `host.ts.write` (event tuples), `host.ts.take` / `host.ts.read` by pattern |
| **Workflow** | `run()`: write stream + take/aggregate; `signal`/`query` for control and status |
| **Handlers** | `publish`: write one event tuple; `window_flush`: take events in window and aggregate |
| **VirtualActor + Durability** | Lazy activation, checkpointed state |

## Quick Start

```bash
# From repo root: start node
./scripts/server.sh

# In another terminal: build and test
cd examples/python/apps/migrating_luats
./build.sh
./test.sh 8091
```

## API

- **workflow_run** (run): `{"op": "workflow_run", "pipeline_id": "run-1", "num_events": 200}` — Write `num_events` event tuples to TupleSpace, then take by pattern and aggregate. Returns status, events_written, events_processed, total_compute_ms, total_coord_ms.
- **publish**: `{"op": "publish", "source": "ingest", "seq": 0, "payload": {...}}` — Write one CDC event tuple (Linda-style).
- **window_flush**: `{"op": "window_flush", "pipeline_id": "run-1"}` — Take all matching event tuples in window and aggregate (timer-style).
- **workflow_query:status**: `{"op": "workflow_query:status"}` — Return pipeline_id, status, events_written, events_processed, total_compute_ms, total_coord_ms.

## Metrics

- **Coord ms**: Time in TupleSpace write/take (coordination).
- **Compute ms**: Time in event processing/aggregation.
- **Events/sec**: Throughput from batch wall time and event count.
- Non-trivial run: 2+ seconds with hundreds of events per run.

## Comparison: LuaTS vs PlexSpaces

| Feature | LuaTS | PlexSpaces |
|---------|-------|------------|
| Linda | `read`/`write`/`in` | `host.ts.read` / `host.ts.write` / `host.ts.take` |
| Events | Subscribe/publish | Handlers (cast) + TupleSpace tuples |
| Multi-thread | Lua threads + coordination | Actors + TupleSpace |
| Timer/window | Event loop timers | `host.send_after` or handler `window_flush` |

See [native/luats_ref.md](native/luats_ref.md) for LuaTS snippet and mapping.

## References

- [PlexSpaces TupleSpace](../../../../docs/detailed-design.md#tuplespace)
- [PlexSpaces Workflow](../../../../docs/detailed-design.md#workflows)
- [Getting Started](../../../../docs/getting-started.md)
