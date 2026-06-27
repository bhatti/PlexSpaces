# Web Crawler (Rust Embedded)

A single-binary parallel web crawler that demonstrates ElasticPool, TupleSpace, ShardGroup,
and a **native scaling benchmark** using `tokio::spawn` (1/4/8/16 workers, stride-N URL
dispatch, Amdahl's Law metrics) — modeled after [Ray's web-crawl](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
and [map-reduce](https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html) examples.

**This example uses the published SDK from GitHub — no local PlexSpaces checkout needed.**

## What It Demonstrates

| PlexSpaces Feature | Role in This Example |
|---|---|
| `#[gen_server_actor]` | PageFetcher, LinkAnalyzer, WebCrawlOrchestrator actors |
| `#[handler("op")]` | Typed message dispatch (fetch, analyze, top_words, crawl) |
| ElasticPool | Round-robin pool of PageFetcher workers |
| TupleSpace pattern | URL queue: pending → in-flight → done tracking |
| ShardGroup pattern | Scatter results across LinkAnalyzer shards, reduce word counts |
| Native scaling benchmark | `tokio::spawn` parallel workers, stride-N URL assignment |
| Single-binary deploy | `NodeBuilder` + `spawn` + `main()` in one binary |
| `RequestContext` | Tenant isolation (`"demo"` / `"web_crawl"`) |

## Actors

| Actor | Behavior | Role |
|---|---|---|
| `WebCrawlOrchestrator` | GenServer | Controls BFS crawl, coordinates pool + shards |
| `PageFetcher` | GenServer | Fetches one URL, extracts links + word counts |
| `LinkAnalyzer` (×2) | GenServer | Shard: merges word counts for assigned URL slice |

## Scaling Benchmark

After the BFS crawl, `run_benchmark` spawns N tokio tasks (1/4/8/16), each processing
`urls[worker_index], urls[worker_index+N], ...` concurrently. Reports elapsed_ms, coord_ms,
fetch_ms, pages_per_sec, speedup, efficiency_pct, parallel_fraction — no WASM overhead,
showing native Tokio throughput as the performance ceiling.

## Build

```bash
# Requires a GitHub tag v0.1.0 to exist on https://github.com/plexobject/plexspaces
# Cargo resolves the full workspace from the single git reference.
cargo build --release
```

## Run

```bash
# Default seed URLs
./target/release/web_crawl

# Custom seed URLs
./target/release/web_crawl https://docs.rs https://crates.io
```

## Expected Output

```
╔══════════════════════════════════════════════════════════════╗
║     Web Crawler — Embedded Single-Binary                    ║
╚══════════════════════════════════════════════════════════════╝

Pattern: ElasticPool (fetchers) + TupleSpace (URL queue) + ShardGroup (map-reduce)

Seed URLs:
  https://example.com
  https://docs.example.com

Step 1: Creating PlexSpaces node...
  ✓ Node ready

Step 2: Spawning WebCrawlOrchestrator...
  ✓ Orchestrator spawned

Step 3: Running crawl...
  ✓ Crawl complete in 42.1ms

Step 4: Results
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Pages crawled : 14
  Total links   : 42
  Elapsed (ms)  : 42

  Top words:
    example              8
    docs                 6
    api                  4
    about                4
    com                  4
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

## How It Compares to Ray

| Ray concept | PlexSpaces equivalent |
|---|---|
| `@ray.remote` actor pool | `ElasticPool<PageFetcher>` |
| `ray.put()` / `ray.get()` shared object | `TupleSpace` write/take |
| `ray.get([map_fn.remote(...)])` | `ShardGroup` scatter-gather → reduce |

## SDK Import (external project)

```toml
# Cargo.toml — no local checkout required
[dependencies]
plexspaces-sdk  = { git = "https://github.com/plexobject/plexspaces", tag = "v0.1.0" }
plexspaces-node = { git = "https://github.com/plexobject/plexspaces", tag = "v0.1.0" }
plexspaces-actor = { git = "https://github.com/plexobject/plexspaces", tag = "v0.1.0" }
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Detailed Design — ElasticPool](../../../../docs/detailed-design.md#elastic-pool)
- [Detailed Design — TupleSpace](../../../../docs/detailed-design.md#tuplespace)
- [Detailed Design — ShardGroup](../../../../docs/detailed-design.md#shard-group)
- [SDK Guide](../../../../sdks/rust/plexspaces-sdk/README.md)
- [Ray web-crawl example](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
- [Ray map-reduce example](https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html)
