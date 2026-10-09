# Log Ingestion & Routing Pipeline (TypeScript)

Production-grade observability pipeline built with PlexSpaces — TypeScript WASM implementation.

Ingests syslog/HTTP/security/infra events, applies configurable pipeline functions (parse, mask PII, enrich, rename, drop), routes by rules, and formats for sinks (Splunk HEC, Datadog, S3 Parquet).

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
| WorkerActor | GenServer | Processes event batches through function chain |

## PlexSpaces Primitives Used

- **Shard Groups** — partition workers for parallel pipeline processing
- **Scatter/Gather** — distribute event batches, collect results
- **Facets** — virtual_actor (100)
- **GenServer** — request-reply for all actors
- **Application Metrics** — compute vs coordination tracking

## Benchmarks

### Strong Scaling
Fixed 10K events, vary worker count [2, 4, 8, 16]. Measures speedup and parallel efficiency.

### Weak Scaling
5K events per shard, grow total with shard count. Efficiency should stay ~100%.

### Pipeline Depth
Vary function chain length [1..5] with fixed workers. Measures throughput impact.

## Quick Start

```bash
./build.sh           # Install deps, compile TS, bundle, componentize to WASM
./test.sh 8091 8094  # Deploy and run benchmarks
./undeploy.sh        # Clean up
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Python version](../../../python/apps/log_pipeline/)
- [Go version](../../../go/apps/log_pipeline/)
