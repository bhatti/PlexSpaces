# Metrics Aggregation Pipeline (Rust Embedded)

Production-grade metrics pipeline — standalone Rust binary using PlexSpaces embedded mode.

StatsD/OTLP-style metrics ingestion, tumbling window aggregation, cascading rollup (1s→1m→1h), Z-score anomaly detection.

## Architecture

- **AggregatorShard** actors: Hash-partitioned by metric name, each maintains windowed values
- **Hash-Based Routing**: Deterministic metric→shard mapping for even distribution
- **Scatter-Gather**: Parallel aggregate collection from all shards
- **Cascading Rollup**: 1s → 1m → 1h aggregation levels
- **Anomaly Detection**: Z-score based with severity classification

## PlexSpaces Primitives Used

| Primitive | Usage |
|-----------|-------|
| `#[gen_server_actor]` | AggregatorShard actor definition |
| `#[plexspaces_handlers]` | Auto-generated message dispatch |
| `spawn_gen_server()` | SDK actor spawning |
| `GenServerRef.cast()` | Fire-and-forget batch ingestion |
| `GenServerRef.call()` | Request-reply aggregate queries |
| `CoordinationComputeTracker` | Compute vs coordination metrics |
| `NodeBuilder` | Embedded node creation |

## Metrics Tracked

16 metric types: cpu.usage, memory.used, disk.io.read/write, network.rx/tx, http.request.duration/count, cache.hit_rate, queue.depth, gc.pause_ms, thread.count, db.query.duration, db.connections.active, api.latency.p99, api.error_rate

## Quick Start

```bash
./test.sh
```

## References

- [Python version](../../../python/apps/metrics_aggregation/)
- [Go version](../../../go/apps/metrics_aggregation/)
- [Architecture](../../../../docs/architecture.md)
