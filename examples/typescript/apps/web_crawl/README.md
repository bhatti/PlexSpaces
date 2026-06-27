# Web Crawler (TypeScript WASM)

A parallel web crawler compiled to WASM with jco/componentize-js, using ElasticPool
(fetcher pool), TupleSpace (URL queue), ShardGroup (map-reduce word frequency), and a
**ScatterGather scaling benchmark** (1/4/8/16 workers, Amdahl's Law metrics).

Modeled after [Ray's web-crawl](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
and [map-reduce](https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html) examples.

**This example installs the TypeScript SDK from npm — no local PlexSpaces checkout needed.**

## What It Demonstrates

| PlexSpaces Feature | Role |
|---|---|
| `PlexSpacesActor<State>` | Typed actor base class |
| `ActorRouter` | Multi-role dispatch (one WASM, three roles) |
| ElasticPool pattern | 16 fetchers reused across URLs |
| TupleSpace | `host.tuplespace.write` for URL queue |
| ShardGroup pattern | 2 analyzer shards: scatter-gather reduce |
| `host.createShardGroup` | Provision fetcher shard group for benchmark |
| `host.scatterGather` | Dispatch 200 URLs to N parallel fetcher shards |
| `host.ask` | Actor-to-actor async RPC |

## Actors

| Class | Behavior | Role |
|---|---|---|
| `WebCrawlOrchestrator` | GenServer | BFS crawl loop + scaling benchmark |
| `PageFetcher` (×16) | GenServer (virtual) | Fetch URL batch (stride-N shard dispatch) |
| `LinkAnalyzer` (×2) | GenServer | Shard: merge counts, top-N words |

## Scaling Benchmark

The `benchmark` handler runs 4 rounds (1, 4, 8, 16 workers) dispatching 200 pages via
`host.createShardGroup` + `host.scatterGather`. Each fetcher processes its own URL slice
(`urls[pool_slot], urls[pool_slot+N], ...`). Metrics: elapsed_ms, coord_ms, fetch_ms,
pages_per_sec, speedup, efficiency_pct, parallel_fraction (Amdahl's Law).

## Build

```bash
./build.sh
```

Requirements: Node.js ≥ 18, `jco`, `@bytecodealliance/componentize-js`.

## Run Tests

```bash
# Start a node first:
./scripts/server.sh  # from repo root

./test.sh 8091
```

## Expected Output

```
Step 0: Build
  ✓ web_crawl_actor.wasm (280K)

Step 1: Deploy
  ✓ Deployed

Step 2: Run crawl
Step 3: Results
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Pages crawled : 14
  Total links   : 28
  Top words:
    example              8
    docs                 6
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Web crawl (TypeScript) test passed.
```

## SDK Import (external project)

```json
{
  "dependencies": {
    "@plexspaces/sdk": "0.1.0"
  }
}
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [TypeScript SDK README](../../../../sdks/typescript/README.md)
- [Ray web-crawl example](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
