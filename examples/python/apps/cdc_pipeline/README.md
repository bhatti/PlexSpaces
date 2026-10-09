# CDC (Change Data Capture) Pipeline (Python)

Production-grade CDC pipeline — Python WASM implementation.

WAL event ingestion → position tracking (LSN watermarks) → schema transformation → fan-out to search index, analytics, and cache invalidation sinks.

## Features

- 4 table schemas: users, orders, products, inventory
- Realistic WAL events: INSERT (40%), UPDATE (50%), DELETE (10%)
- LSN position tracking with per-table watermarks
- Event deduplication by event ID
- 3 sink transformations:
  - **Search Index**: Field extraction for Elasticsearch/OpenSearch
  - **Analytics**: Event flattening for data warehouse
  - **Cache Invalidation**: Key generation for Redis/Memcached
- Compute vs coordination metrics tracking

## Architecture

```
WAL Events → [Leader] → scatter_gather → [Worker Shards]
                                              ↓
                                    ┌─────────┼──────────┐
                                    ↓         ↓          ↓
                              Search Index  Analytics  Cache Invalidation
```

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## Benchmarks

- **Basic Run**: 10,000 events, 8 workers
- **Strong Scaling**: Fixed 10,000 events, vary workers [2, 4, 8, 16]
- **Weak Scaling**: Fixed events per worker, grow total

## References

- [Go version](../../../go/apps/cdc_pipeline/) (when available)
- [Architecture](../../../../docs/architecture.md)
- [Debezium](https://debezium.io/) — real-world CDC analog
