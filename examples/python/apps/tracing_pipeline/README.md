# Distributed Tracing Pipeline (Python)

Production-grade distributed tracing pipeline — Python WASM implementation.

OTLP span ingestion, out-of-order trace assembly, tail-based sampling (error-biased + latency-biased + random), service graph construction.

## Architecture

- **LeaderActor**: Orchestrates span generation, scatter-gather to workers, tail sampling, service graph
- **WorkerActor**: Partitioned by trace_id hash, assembles spans into complete traces

## Features

- 6 services: api-gateway, user-service, order-service, payment-service, inventory-service, notification-service
- Realistic call graph with probabilistic edges and latency distributions
- Out-of-order span arrival (shuffled before processing)
- ~5% error injection with elevated latency on error spans
- Tail-based sampling: keep all errors, keep p99 latency traces, 1% random
- Service graph: node stats (calls, errors, avg latency) and edge stats (call count, p50/p99 latency)

## PlexSpaces Primitives

| Primitive | Usage |
|-----------|-------|
| `@actor` / `@handler` | Actor definition and message dispatch |
| `host.create_shard_group()` | Partition workers by trace_id hash |
| `host.scatter_gather()` | Parallel span processing across shards |
| `host.application_metrics_add()` | Compute/coordination metrics |
| ACTOR_ROLES pattern | Multi-role dispatch (leader/worker) |

## Measured Performance

**Basic run** — 1000 traces, 8 workers: ~1,960 spans/sec, 6 nodes + 5 edges in service graph, 4.3% sampling rate.

**Strong scaling** (1,000 traces fixed, 4 passes/SG call):

| Workers | Spans/s | Wall ms | Gran | Speedup | Eff% |
|---------|---------|---------|------|---------|------|
| 2 | 111,870 | 171 | 1.7× | 1.00× | 100% |
| 4 | 162,956 | 148 | 1.9× | 1.16× | 58% |
| 8 | 182,123 | 197 | 1.4× | 0.87× | 22% |
| 16 | 269,896 | 194 | 1.6× | 0.88× | 11% |

**Weak scaling** (250 traces/shard × 4 passes, fixed per shard):

| Workers | Total Traces | Spans/s | Gran | Eff% |
|---------|-------------|---------|------|------|
| 2 | 500 | 105,752 | 1.4× | 100% |
| 4 | 1,000 | 104,779 | 1.5× | 99% |
| 8 | 2,000 | 213,520 | 2.6× | 202% |
| 16 | 4,000 | 432,417 | 4.4× | 409% |

Workers generate spans locally (no payload serialization) and run 4 assembly passes per scatter-gather call. Strong scaling plateaus at 4 workers for fixed 1K traces. Weak scaling shows the real throughput story: super-linear 409% efficiency at 16 workers as coordination overhead amortizes over proportionally more work.

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## References

- [Go version](../../../go/apps/tracing_pipeline/) (when available)
- [TypeScript version](../../../typescript/apps/tracing_pipeline/) (when available)
- [Architecture](../../../../docs/architecture.md)
