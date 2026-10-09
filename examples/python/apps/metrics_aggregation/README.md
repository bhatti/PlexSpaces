# Metrics Aggregation Pipeline (Python)

Production-grade observability pipeline built with PlexSpaces Python WASM actors.

Ingests StatsD/OTLP-style metrics from 16 metric types, runs tumbling-window aggregation
(count/sum/avg/min/max/p50/p95/p99), cascades rollups (1 s→1 min→1 h), and applies
dual anomaly detection (Z-score + EWMA). Real-world analog: Datadog Agent, Prometheus +
Thanos, CloudWatch, Graphite.

## How It Works

`LeaderActor` batches incoming metrics and fans them out to a shard group of
`WorkerActors` via scatter/gather. Each worker aggregates its batch into window
statistics across all 16 metric names and runs anomaly detection. The leader collects
all worker results, drives the cascading time-window rollup, and tracks compute vs
coordination latency.

```python
# LeaderActor — fan out and collect
group = host.create_shard_group(CreateShardGroupRequest(
    group_id="metrics-agg",
    actor_type="WorkerActor",
    shard_count=worker_count,
    partition_strategy="hash",
    placement=NodePlacement(strategy="from_registry"),
))

for batch in batches:
    result = host.scatter_gather(ScatterGatherRequest(
        group_id="metrics-agg",
        query={"op": "aggregate_batch", "metrics": batch, "window_ms": 1000},
        timeout_ms=30000,
    ))
    for resp in result.shard_responses:
        r = resp.get("payload", resp)
        all_aggregates.extend(r.get("aggregates", []))
        all_anomalies.extend(r.get("anomalies", []))
```

```python
# WorkerActor — aggregate window + detect anomalies
@handler("aggregate_batch")
def aggregate_batch(self, metrics: list = None, metric_count: int = 0,
                    num_passes: int = 1, window_ms: int = 1000,
                    anomaly_threshold: float = 2.5) -> dict:
    if not metrics and metric_count > 0:
        metrics = generate_metrics(metric_count, seed=self.metrics_processed)
    aggregates = aggregate_window(metrics, window_ms)
    anomalies = detect_anomalies_zscore(aggregates, anomaly_threshold)
    anomalies += detect_anomalies_ewma(aggregates)
    return {"aggregates": aggregates, "anomalies": anomalies, "compute_ms": ...}
```

## Architecture

```
Metric Sources (StatsD, OTLP, custom)
    │
    ▼
┌──────────────┐  scatter_gather  ┌────────────────────────────────┐
│  LeaderActor │ ───────────────▶ │  WorkerActor shards (N)        │
│  (GenServer) │ ◀─────────────── │  aggregate_window              │
└──────┬───────┘    aggregates    │  detect_anomalies_zscore/ewma  │
       │                          └────────────────────────────────┘
       ▼
  cascading rollup
  1 s → 1 min → 1 h
```

## Actors

| Actor | Type | Role |
|-------|------|------|
| `LeaderActor` | GenServer | Orchestrates scatter/gather, drives 3-level rollup, runs benchmarks |
| `WorkerActor` | GenServer | Aggregates metric window, runs Z-score + EWMA anomaly detection |

## PlexSpaces APIs Used

| API | Usage |
|-----|-------|
| `@actor` + `@handler` | Define `LeaderActor` and `WorkerActor` with typed message handlers |
| `host.create_shard_group` | Partition workers by metric name hash for parallel aggregation |
| `host.scatter_gather` | Fan out batches to all worker shards, collect aggregates |
| `host.application_metrics_add` | Track compute vs coordination latency per invocation |
| `state(default=...)` | Persist running totals (metrics processed, compute ms) across calls |

## Features

- **16 metric types**: cpu.usage, memory.used, disk.io, network rx/tx, http latency/count, cache hit rate, queue depth, gc.pause, thread count, db query/connections, api latency/error rate
- **Window aggregation**: count, sum, avg, min, max, p50, p95, p99 per metric per window
- **Cascading rollup**: 1 s → 1 min → 1 h (three levels in one pass)
- **Anomaly detection**: Z-score (global deviation) + EWMA (temporal deviation), configurable thresholds, deduped critical/warning severity
- **Compute-dominated benchmarks**: `num_passes` repeats the full pipeline per invocation, making compute > coordination and showing real speedup curves

## Measured Performance

**Baseline** — 10K metrics / 8 workers:
- ~8.6K met/s, 2560 aggregates, 16 unique metric names, 3-level rollup in ~1.1 s

**Strong scaling** — 4000 metrics fixed, split N ways, 8 aggregation passes:

| Workers | Agg/s | Wall ms | Gran | Speedup |
|---------|-------|---------|------|---------|
| 2  | 719K | 89 ms | 17× | 1.00× |
| 4  | 674K | 95 ms | 15× | 0.93× |
| 8  | 810K | 79 ms | 96× | 1.12× |
| 16 | 901K | 71 ms | 110× | 1.25× |

Granularity rises from 17× to 110× as per-shard work shrinks — compute overwhelms coordination at high N.

**Weak scaling** — 500 metrics/shard × 8 passes (fixed per shard):

| Workers | Total | Agg/s | Eff% |
|---------|-------|-------|------|
| 2  | 2K  | 444K  | 100% |
| 4  | 4K  | 451K  | 101% |
| 8  | 8K  | 901K  | 202% |
| 16 | 16K | 1.44M | 323% |

Super-linear efficiency because the constant scatter/gather overhead is amortized over more parallel shards.

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
