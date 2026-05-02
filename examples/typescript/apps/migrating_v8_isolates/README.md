# V8 Isolates → PlexSpaces: High-Throughput Log Processor (TypeScript WASM)

Batch processing of log lines: parse level (INFO/WARN/ERROR/DEBUG), aggregate counts, report throughput. Simulates V8 isolate–style per-batch processing with a single GenServer-style actor.

## Use Case

High-throughput log ingestion: accept batches of log lines, parse level/source, aggregate by level. Suited for log pipelines that process 100K+ events/sec with batching and routing by level.

## Abstractions

| Abstraction | Usage |
|-------------|--------|
| **GenServer** | Request-reply; handlers process_batch (batch of lines), status (metrics) |
| **VirtualActor** | Lazy activation so first invoke to `log-processor:default` creates the actor |
| **Batch processing** | process_batch accepts `lines: string[]`; parse level, update state, return counts |

## Quick Start

```bash
# From repo root: start node
./scripts/server.sh

# In another terminal: build and test
cd examples/typescript/apps/migrating_v8_isolates
./build.sh
./test.sh 8091
```

## API

- **process_batch**: `{"op": "process_batch", "lines": ["INFO msg", "WARN msg", ...]}` — Process a batch of log lines; parse level, aggregate. Returns ok, lines, bytes, by_level, processed_count, batches_received, total_compute_ms, total_coord_ms.
- **status**: `{"op": "status"}` — Return processor_id, processed_count, batches_received, total_bytes, by_level, total_compute_ms, total_coord_ms, elapsed_ms, events_per_sec.

## Metrics

- **Events/sec**: processed_count / (elapsed_ms / 1000).
- **Compute ms / Coord ms**: Time in parse/aggregate vs coordination.
- Non-trivial run: 2+ seconds with many batches (e.g. 80+ batches × 200 lines).

## Comparison: V8 Isolates vs PlexSpaces

| Feature | V8 Isolates | PlexSpaces |
|---------|-------------|------------|
| Isolation | One V8 isolate per tenant/task | One actor (or actor per tenant); batch messages |
| Throughput | High via isolate pooling | High via batch process_batch; scale with more actors |
| Batching | Application-level | process_batch(lines[]) in one call |
| Routing | By level/topic in app | By level in state; optional channels downstream |

See [native/v8_isolates_ref.md](native/v8_isolates_ref.md) for V8 isolate reference and mapping.

## References

- [PlexSpaces GenServer](../../../../docs/detailed-design.md#behaviors)
- [Getting Started](../../../../docs/getting-started.md)
