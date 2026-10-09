# Distributed Tracing Pipeline (TypeScript)

Production-grade distributed tracing pipeline — TypeScript WASM implementation.

OTLP span ingestion → trace assembly (out-of-order) → tail-based sampling → service graph construction.

## Architecture

- **LeaderActor**: Generates spans, partitions by traceId hash, scatter/gather to workers, merges results
- **WorkerActor**: Assembles traces from out-of-order spans, tail-based sampling, service graph edges

## Features

- 6 services with realistic call chains and latency distributions
- Out-of-order span arrival (shuffled before ingestion)
- Tail-based sampling: error-biased (100%), latency-biased (p99+), random (1%)
- Service graph: parent→child edge tracking with call counts and latencies
- ~5% error rate, ~3% high-latency traces
- Compute vs coordination metrics tracking

## PlexSpaces Primitives

| Primitive | Usage |
|-----------|-------|
| Shard Groups | Partition spans by traceId hash |
| Scatter/Gather | Parallel span processing across workers |
| GenServer | Request-reply for assembly + sampling |
| Virtual Actor | Lazy activation for workers |
| Application Metrics | Compute/coordination tracking |

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## Benchmarks

- **Basic run**: spans/sec, traces assembled, sampling stats, service graph edges
- **Strong scaling**: Fixed 10K spans, vary workers [2, 4, 8, 16]
- **Weak scaling**: Fixed spans/shard, grow total

## References

- [Go version](../../../go/apps/tracing_pipeline/) (when available)
- [Python version](../../../python/apps/tracing_pipeline/) (when available)
- [Architecture](../../../../docs/architecture.md)
