# Dirigo → PlexSpaces: Real-Time Analytics Stream (TypeScript WASM)

Kafka-style windowed aggregation for clickstream (or any event stream): events are pushed into a window; when the window is full (or on flush), an aggregate (count, sum, avg, min, max) is emitted.

## Use Case

Real-time analytics: ingest events (e.g. clicks, sensor readings), maintain a tumbling window, emit windowed aggregates. Mirrors Dirigo’s virtual-actor stream operators with a single WorkflowActor and window-in-state.

## Abstractions

| Abstraction | Usage |
|-------------|--------|
| **Workflow** | run() = process event batch, window aggregation; signal/query for cancel/status |
| **Handlers** | ingest = add one event; window_flush = emit aggregate for current window and clear |
| **VirtualActor + Durability** | Lazy activation, checkpointed state |

## Quick Start

```bash
# From repo root: start node
./scripts/server.sh

# In another terminal: build and test
cd examples/typescript/apps/migrating_dirigo
./build.sh
./test.sh 8092
```

## API

- **workflow_run**: `{"op": "workflow_run", "stream_id": "run-1", "window_size": 10, "events": [{...}, ...]}` — Process batch; push to window; when window is full, emit aggregate. Returns status, processed_count, windows_emitted, total_compute_ms, total_coord_ms.
- **ingest**: `{"op": "ingest", "event_id": "e1", "value": 42}` — Add one event to the window.
- **window_flush**: `{"op": "window_flush"}` — Emit aggregate for current window (count, sum, avg, min, max) and clear window.
- **workflow_query:status**: `{"op": "workflow_query:status"}` — Return stream_id, status, window_size, window_count, processed_count, windows_emitted, total_compute_ms, total_coord_ms.

## Metrics

- **Coord ms**: Coordination overhead (e.g. state sync).
- **Compute ms**: Time in event/window processing.
- Non-trivial run: 2+ seconds with multiple batch runs.

## Comparison: Dirigo vs PlexSpaces

| Feature | Dirigo | PlexSpaces |
|---------|--------|------------|
| Stream operators | Virtual actor per operator (map, filter, reduce, window) | WorkflowActor + window in state; handlers ingest, window_flush |
| Window | Per-operator config | window_size in state; emit when full or on flush |
| Activation | Lazy virtual actor | virtual_actor facet, lazy |

See [native/dirigo_ref.md](native/dirigo_ref.md) for Dirigo reference and mapping.

## References

- [PlexSpaces Workflow](../../../../docs/detailed-design.md#workflows)
- [Getting Started](../../../../docs/getting-started.md)
