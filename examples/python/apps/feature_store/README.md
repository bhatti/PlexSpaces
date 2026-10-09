# Feature Store (Python)

Production-grade feature store — Python WASM implementation.

Feature ingestion, versioned KV storage, consistent-hash routing, low-latency online serving with LRU cache.

## Features

- 18 feature types across 3 entity categories (user, product, interaction)
- Versioned storage (last 3 versions per entity+feature)
- Consistent hash routing for shard assignment
- LRU cache per shard with hit/miss tracking
- Point lookups and batch feature vector assembly
- Compute vs coordination metrics tracking

## Architecture

- **LeaderActor**: Orchestrates ingestion and lookup phases, runs benchmarks
- **WorkerActor**: Owns a partition of entity features, serves lookups from cache or store

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## Benchmarks

- Strong scaling: Fixed entities/lookups, vary shard count
- Weak scaling: Fixed entities per shard, grow total

## References

- [Metrics Aggregation](../metrics_aggregation/) - sibling pipeline example
- [Architecture](../../../../docs/architecture.md)
