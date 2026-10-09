# Feature Store (TypeScript)

Production-grade online feature store — TypeScript WASM implementation.

Feature ingestion → versioned KV storage → consistent-hash routing → low-latency online serving with LRU cache.

## Architecture

- **LeaderActor**: Orchestrates ingestion and lookup benchmarks via shard groups
- **WorkerActor**: Stores features with version history, serves lookups from LRU cache

## Features

- 15 feature types: user demographics, product attributes, interaction signals
- Versioned storage: last 3 versions per entity+feature
- Consistent hash routing: entity_id → shard for co-located features
- LRU cache with configurable size, hit/miss tracking
- Point lookups, batch lookups, full vector assembly
- Compute vs coordination metrics tracking

## PlexSpaces Primitives

| Primitive | Usage |
|-----------|-------|
| GenServer | Leader + Worker actors |
| Shard Groups | Hash-partitioned feature shards |
| Scatter/Gather | Parallel ingestion + lookups |
| Virtual Actor | Lazy activation, idle timeout |
| Application Metrics | Compute/coordination tracking |

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
