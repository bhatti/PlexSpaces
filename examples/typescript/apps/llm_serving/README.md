# LLM Serving with Request Batching & Model Multiplexing (TypeScript)

Production-grade LLM serving pipeline — TypeScript WASM implementation.

Cost-aware routing across model tiers (small-7b/medium-13b/large-70b), dynamic request batching, health monitoring, and supervision.

## Architecture

- **RouterActor** (leader): Receives inference requests, routes to optimal model tier based on task type and priority, manages batching
- **ModelActor** (worker): Simulates model inference with tier-specific latency/cost/failure characteristics

### Model Tiers

| Tier | Model | Base Latency | Cost/Token | Batch Size | Tasks |
|------|-------|-------------|-----------|-----------|-------|
| small | 7B | 5ms | $0.01 | 32 | classification, embedding |
| medium | 13B | 15ms | $0.05 | 16 | summarization, chat |
| large | 70B | 50ms | $0.15 | 8 | generation, reasoning |

### Cost-Aware Routing

- `classification` / `embedding` → small (cheapest)
- `summarization` / `chat` → medium (balanced)
- `generation` / `reasoning` → large (most capable)
- High-priority requests upgrade one tier

## PlexSpaces Primitives Used

| Primitive | Usage |
|-----------|-------|
| Shard Groups | Model worker pool with hash partitioning |
| Scatter/Gather | Batch dispatch to workers, collect results |
| Process Groups | Health monitoring broadcast |
| Virtual Actor | Lazy activation for model workers |
| Application Metrics | Per-tier compute/coordination tracking |

## Benchmarks

- **Strong scaling**: Fixed 1000 requests, vary worker count [2, 4, 6, 8]
- **Weak scaling**: Fixed requests/shard, grow total with workers
- **Metrics**: requests/sec, tokens/sec, cost, latency percentiles, error rate, compute vs coordination ratio

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## References

- [Python LLM Serving](../../../python/apps/llm_serving/)
- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
