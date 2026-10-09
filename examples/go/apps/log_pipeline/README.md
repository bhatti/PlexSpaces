# Log Ingestion & Routing Pipeline (Go)

Production-grade observability pipeline built with PlexSpaces — Go WASM implementation.

Ingests syslog/HTTP/security/infra events, applies configurable pipeline functions (parse, mask PII, enrich, rename, drop, sample), routes by rules, and formats for sinks (Splunk HEC, Datadog, S3 Parquet).

## Architecture

```
Sources (syslog, app_json, security, infra)
    │
    ▼
┌──────────┐     ┌──────────────────────────┐     ┌─────────────┐
│  Leader   │────▶│  Pipeline Worker Shards   │────▶│  Sink       │
│  Actor    │     │  (shard group fan-out)    │     │  Formatters │
└──────────┘     └──────────────────────────┘     └─────────────┘
                  json_parse → mask_pii →            splunk_hec
                  enrich → rename → drop             datadog
                                                     s3_parquet
```

## Actors

| Actor | Type | Role |
|-------|------|------|
| LeaderActor | GenServer | Orchestrates pipeline, manages shard groups, runs benchmarks |
| PipelineWorkerActor | GenServer | Processes event batches through function chain |

## Pipeline Functions

| Function | Purpose |
|----------|---------|
| `json_parse` | Parse JSON payloads, extract structured fields |
| `regex_extract` | Extract fields via regex patterns |
| `mask_pii` | Redact emails, SSNs, IPs for compliance |
| `enrich` | Add geo, threat intel, asset metadata |
| `rename_fields` | Normalize field names across sources |
| `drop` | Filter events by severity/source rules |

## Sink Formats

- **Splunk HEC** — `{"event": ..., "sourcetype": ..., "index": ..., "time": ...}`
- **Datadog** — `{"ddsource": ..., "ddtags": ..., "message": ...}`
- **S3 Parquet** — `{"bucket": ..., "key": ..., "partition": ..., "columns": [...]}`

## PlexSpaces Primitives Used

- **Shard Groups** — partition workers for parallel pipeline processing
- **Scatter/Gather** — distribute event batches, collect results
- **Process Groups** — sink fan-out to multiple destinations
- **Channels** — async stage coupling between pipeline stages
- **Facets** — virtual_actor (100), durability (90), metrics (80)
- **GenServer** — request-reply for all actors
- **Application Metrics** — compute vs coordination tracking via `host.ApplicationMetricsAdd()`

## Benchmarks

The test script runs three benchmark suites:

### Strong Scaling
Fixed 10K events, vary worker count [2, 4, 8, 16]. Measures speedup and parallel efficiency.

### Weak Scaling
5K events per shard, grow total with shard count. Efficiency should stay ~100%.

### Pipeline Depth
Vary function chain length [1..5] with fixed workers. Measures throughput impact of pipeline complexity.

All benchmarks report: `compute_time_ms`, `coordination_time_ms`, `granularity_ratio`, `events_per_sec`.

## Quick Start

```bash
# Build WASM component
./build.sh

# Run full benchmark suite (requires running PlexSpaces nodes)
./test.sh 8091 8094

# Undeploy
./undeploy.sh
```

## Comparison

| Feature | This Example | Stream | Fluentd | Vector |
|---------|-------------|-------------|---------|--------|
| Language | Go (WASM) | Node.js | Ruby/C | Rust |
| Pipeline functions | Composable actors | JS functions | Plugin chain | TOML transforms |
| Fault tolerance | Supervisor tree | Process restart | Manual | Crash restart |
| Scaling | Shard groups + scatter/gather | Worker threads | Manual sharding | Adaptive concurrency |
| Multi-sink | Process group fan-out | Output groups | Match directive | Sink config |

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Python version](../../python/apps/log_pipeline/)
- [Detailed Design — Facets](../../../../docs/detailed-design.md#facets)
