# Log Ingestion & Routing Pipeline (Python)

A production-grade observability pipeline built with PlexSpaces actors, demonstrating how the actor model replaces traditional log processing systems (Stream, Splunk Heavy Forwarder, Fluentd, Vector).

## Architecture

```
Ingester → EventBreaker → Router → PipelineWorker (shard group) → SinkWriter
```

Each stage is a GenServer actor. Pipeline workers are deployed as a shard group for horizontal scaling. Events flow through a configurable function chain: parse → mask PII → enrich → rename → drop.

## Actors

| Actor | Type | Role |
|---|---|---|
| `LeaderActor` | GenServer | Orchestrates pipeline: generate events, break, route, scatter to workers, aggregate, sink |
| `PipelineWorkerActor` | GenServer (shard group) | Processes events through function chain; tracks compute vs coordination time |
| `PipelineAuditActor` | GenEvent | Fire-and-forget audit trail for pipeline operations |
| `PipelineHealthFSM` | GenFSM | Tracks pipeline health state (healthy → degraded → unhealthy) based on error rates |

## Pipeline Functions

| Function | What It Does |
|---|---|
| `json_parse` | Parse JSON raw body into structured fields |
| `regex_extract` | Extract fields via regex (timestamps, IPs) |
| `mask_pii` | Mask emails, SSNs, credit cards |
| `enrich` | Add environment, region, pipeline version |
| `rename_fields` | Normalize field names across sources |
| `drop` | Drop debug/trace level events |
| `sample` | Probabilistic sampling (e.g., 10%) |

## Sink Formats

- **Splunk HEC** — HTTP Event Collector JSON
- **Datadog** — Datadog Log API format
- **S3 Parquet** — Columnar batch for archival

## PlexSpaces Primitives Used

- **GenServer** — all pipeline stages
- **GenEvent** — audit trail (fire-and-forget)
- **GenFSM** — pipeline health state machine
- **Shard Groups** — partitioned pipeline workers for parallel processing
- **Scatter/Gather** — fan-out events to workers, aggregate results
- **Process Groups** — audit event broadcast
- **Facets** — virtual_actor, durability, metrics
- **Application Metrics** — compute vs coordination tracking

## Benchmarks

Three benchmark modes:
1. **Strong scaling** — fixed 10K events, vary worker count [2, 4, 8, 16]
2. **Weak scaling** — 5K events/worker, grow total work
3. **Pipeline depth** — vary function chain length [1..5]

All benchmarks report: events/sec, compute vs coordination ms, granularity ratio, speedup, parallel efficiency.

## Quick Start

```bash
# Start 2 PlexSpaces nodes
./scripts/server.sh 8091 &
./scripts/server.sh 8094 &

# Build and test
cd examples/python/apps/log_pipeline
./build.sh
./test.sh 8091 8094
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Detailed Design — Shard Groups](../../../../docs/detailed-design.md)
- [From Big Ball of Mud to Functional Pipeline](https://shahbhat.medium.com/from-big-ball-of-mud-to-functional-pipeline-building-an-observability-platform-in-rust-d86f35e3a5f5) — companion blog post on observability architecture
