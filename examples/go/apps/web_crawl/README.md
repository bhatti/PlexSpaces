# Web Crawler (Go WASM)

A parallel web crawler compiled to WASM with TinyGo, using ElasticPool (fetcher pool),
TupleSpace (URL queue), ShardGroup (map-reduce word frequency), and a **ScatterGather
scaling benchmark** (1/4/8/16 workers, Amdahl's Law metrics).

Modeled after [Ray's web-crawl](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
and [map-reduce](https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html) examples.

**This example imports the Go SDK from GitHub — no local PlexSpaces checkout needed.**

## What It Demonstrates

| PlexSpaces Feature | Role |
|---|---|
| `plexspaces.BaseActor` | Shared actor base (init + handle dispatch) |
| ElasticPool pattern | 16 fetcher workers, round-robin across URLs |
| TupleSpace | `url_queue` space: write pending, write done |
| ShardGroup pattern | 2 analyzer shards: scatter + reduce word counts |
| `host.CreateShardGroup` | Provision fetcher shard group for benchmark |
| `host.ScatterGather` | Dispatch 200 URLs to N parallel fetcher shards |
| `host.Ask` | Actor-to-actor RPC for fetch and analyze calls |
| `app-config.toml` | Supervisor with 16 fetchers + shard children |

## Actors

| Actor | Behavior | Role |
|---|---|---|
| `orchestrator` | GenServer | BFS crawl loop + scaling benchmark |
| `fetcher-0..15` | GenServer (virtual) | Fetch URL batch (stride-N shard dispatch) |
| `analyzer-0..1` | GenServer | Shard: merge counts, return top-N words |

## Scaling Benchmark

The `benchmark` handler runs 4 rounds (1, 4, 8, 16 workers) dispatching 200 pages via
`host.CreateShardGroup` + `host.ScatterGather`. Each fetcher processes its own URL slice
(`urls[pool_slot], urls[pool_slot+N], ...`). Metrics reported per round:

| Metric | Description |
|---|---|
| `elapsed_ms` | Wall-clock time for the round |
| `coord_ms` | Coordination overhead (shard setup + TupleSpace writes) |
| `fetch_ms` | Actual fetch work time |
| `pages_per_sec` | Throughput |
| `speedup` | vs 1-worker baseline |
| `efficiency_pct` | `speedup / N × 100` |
| `parallel_fraction` | `1 - coord_ms / elapsed_ms` (Amdahl's Law) |

## Build

```bash
./build.sh
```

Requirements: `tinygo`, `wasm-tools`, `jco`.

## Run Tests

```bash
# Start a node first:
./scripts/server.sh  # from repo root

./test.sh 8091
```

## Expected Output

```
Step 0: Build
  ✓ web_crawl_actor.wasm (156K)

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
Web crawl (Go) test passed.
```

## SDK Import (external project)

```go
// go.mod — no replace directive needed once tag is pushed
module your/module

require github.com/bhatti/plexspaces/sdks/go v0.1.1
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Go SDK README](../../../../sdks/go/README.md)
- [Ray web-crawl example](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
