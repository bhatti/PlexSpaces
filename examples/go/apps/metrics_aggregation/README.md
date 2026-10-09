# Metrics Aggregation Pipeline (Go)

Production-grade metrics pipeline — Go WASM implementation.

StatsD/OTLP-style metrics ingestion, tumbling window aggregation, cascading rollup (1s->1m->1h), Z-score anomaly detection, and alerting.

## Features

- 16 metric types (CPU, memory, disk, network, HTTP, cache, DB, API)
- Window aggregation: count, sum, avg, min, max, p50, p95, p99
- Cascading rollup: 1s -> 1m -> 1h
- Z-score anomaly detection with severity classification
- Compute vs coordination metrics tracking

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## References

- [Python version](../../../python/apps/metrics_aggregation/)
- [Architecture](../../../../docs/architecture.md)
