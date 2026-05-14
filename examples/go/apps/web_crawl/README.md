# Web Crawler (Go WASM)

A parallel web crawler compiled to WASM with TinyGo, using ElasticPool (fetcher pool),
TupleSpace (URL queue), and ShardGroup (map-reduce word frequency).

Modeled after [Ray's web-crawl](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
and [map-reduce](https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html) examples.

**This example imports the Go SDK from GitHub — no local PlexSpaces checkout needed.**

## What It Demonstrates

| PlexSpaces Feature | Role |
|---|---|
| `plexspaces.BaseActor` | Shared actor base (init + handle dispatch) |
| ElasticPool pattern | 4 fetcher workers, round-robin across URLs |
| TupleSpace | `url_queue` space: write pending, write done |
| ShardGroup pattern | 2 analyzer shards: scatter + reduce word counts |
| `host.Ask` | Actor-to-actor RPC for fetch and analyze calls |
| `app-config.toml` | Supervisor with pool + shard children |

## Actors

| Actor | Behavior | Role |
|---|---|---|
| `orchestrator` | GenServer | BFS crawl loop |
| `fetcher-0..3` | GenServer (virtual) | Fetch one URL, extract links + words |
| `analyzer-0..1` | GenServer | Shard: merge counts, return top-N words |

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

require github.com/plexobject/plexspaces/sdks/go v0.1.0
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Go SDK README](../../../../sdks/go/README.md)
- [Ray web-crawl example](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
