# LLM Serving Pipeline (Python)

Production-grade LLM serving — Python WASM implementation.

Multi-model tiers (small/medium/large), cost-aware routing, dynamic request batching, health monitoring, compute vs coordination metrics.

## Architecture

- **RouterActor** (leader): Cost-aware routing, request batching, health tracking
- **ModelActor** (worker): Simulated inference with realistic latencies, OOM/timeout failures, token generation

## Model Tiers

| Tier | Model | Params | Latency | Tokens/sec | Cost/1K | Tasks |
|------|-------|--------|---------|------------|---------|-------|
| small | plx-small-7b | 7B | 10ms | 500 | $0.02 | classification, extraction, QA |
| medium | plx-medium-30b | 30B | 50ms | 200 | $0.10 | summarization, translation, reasoning |
| large | plx-large-70b | 70B | 200ms | 80 | $0.50 | generation, code, creative |

## PlexSpaces Primitives

| Primitive | Usage |
|-----------|-------|
| Shard Groups | Model replica pool |
| Scatter/Gather | Batch distribution |
| Process Groups | Health broadcast |
| Application Metrics | Token/latency tracking |

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## Comparison

| Feature | PlexSpaces | Ray Serve | vLLM | TGI |
|---------|-----------|-----------|------|-----|
| Multi-model routing | ✓ | ✓ | ✗ | ✗ |
| Dynamic batching | ✓ | ✓ | ✓ | ✓ |
| Fault tolerance | Supervision | Manual | ✗ | ✗ |
| Polyglot | 4 languages | Python | Python | Rust |
| Cost-aware routing | ✓ | Manual | ✗ | ✗ |

## References

- [Architecture](../../../../docs/architecture.md)
- [TypeScript version](../../../typescript/apps/llm_serving/) (planned)
