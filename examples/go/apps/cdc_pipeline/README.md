# CDC (Change Data Capture) Pipeline (Go)

Production-grade CDC pipeline — Go WASM implementation.

PostgreSQL-style WAL event capture, transformation with computed fields, fan-out to search index, analytics, and cache invalidation sinks.

## Architecture

- **LeaderActor**: Orchestrates WAL event generation, distributes batches to workers via shard groups
- **WorkerActor**: Processes events — deduplication, position tracking, transformation, fan-out

### Pipeline Stages

1. **WAL Event Generation**: INSERT/UPDATE/DELETE on 4 tables (users, orders, products, inventory)
2. **CDC Position Tracking**: Per-table LSN watermarks for reliable replay
3. **Deduplication**: Event ID tracking to prevent double-processing
4. **Transformation**: Schema mapping, field renaming, computed fields (full_name, email_domain, available stock, utilization %)
5. **Fan-out**: Each event routes to 1-3 sinks based on operation type

### Sink Routing

| Operation | Search Index | Analytics | Cache Invalidation |
|-----------|:---:|:---:|:---:|
| INSERT | ✓ | ✓ | — |
| UPDATE | ✓ | ✓ | ✓ |
| DELETE | — | ✓ | ✓ |

## PlexSpaces Primitives

| Primitive | Usage |
|-----------|-------|
| Shard Groups | Partition events across workers |
| Scatter/Gather | Distribute batches, collect results |
| Process Groups | Fan-out to sinks |
| Application Metrics | Compute vs coordination tracking |

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## Benchmarks

- events/sec throughput at varying shard counts
- Compute (transformation) vs coordination (fan-out) breakdown
- Strong scaling with worker counts [2, 4, 8, 16]
- Weak scaling with fixed events per shard

## References

- [Architecture](../../../../docs/architecture.md)
- [Detailed Design](../../../../docs/detailed-design.md)
