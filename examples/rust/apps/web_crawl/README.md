# Web Crawler (Rust WASM)

A parallel web crawler deployed as a WASM component, using ElasticPool (fetcher workers),
TupleSpace (URL queue), and ShardGroup (map-reduce word frequency).

Modeled after [Ray's web-crawl](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
and [map-reduce](https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html) examples.

**This example imports the SDK from GitHub — no local PlexSpaces checkout needed.**

## What It Demonstrates

| PlexSpaces Feature | Role |
|---|---|
| WIT guest (WASM component) | All actors compiled to `web_crawl_wasm.wasm` |
| ElasticPool pattern | `fetcher` actors reused round-robin across URLs |
| TupleSpace | `url_queue` space: pending → in-flight → done tracking |
| ShardGroup pattern | `analyzer-0` / `analyzer-1` scatter-gather reduce |
| `host::ask` | Actor-to-actor RPC for fetch + analyze calls |
| `app-config.toml` | Supervisor with orchestrator + fetcher + 2 analyzer shards |

## Actors

| Actor | Behavior | Role |
|---|---|---|
| `orchestrator` | GenServer | BFS crawl loop, coordinates pool + shards |
| `fetcher` | GenServer (virtual) | Fetches one URL, returns links + word counts |
| `analyzer-0`, `analyzer-1` | GenServer | Shard: merges word counts, returns top-N words |

## Build

```bash
./build.sh
```

Requirements: `rustup target add wasm32-wasip1`, `cargo install wasm-tools`, `jco` installed.

## Run Tests

```bash
# Start a node first (from repo root):
./scripts/server.sh

# Then run tests:
./test.sh 8091
```

## Test Scenarios

1. Deploy WASM component to running node
2. Send `crawl` request to orchestrator with seed URLs
3. Orchestrator seeds TupleSpace, dispatches to fetcher pool
4. Results scatter to analyzer shards, reduce to top-N words
5. Validate: pages_crawled > 0, top_words present

## Expected Output

```
Step 0: Build WASM
  ✓ web_crawl_actor.wasm (142K)

Step 1: Deploy to localhost:8091
  ✓ Deployed

Step 2: Start crawl
Step 3: Results
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Pages crawled : 14
  Total links   : 28
  Top words:
    example              8
    docs                 6
    api                  4
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Web crawl test passed.
```

## SDK Import (external project)

```toml
# Cargo.toml — git tag import, no local clone needed
[target.'cfg(target_arch = "wasm32")'.dependencies]
plexspaces-sdk = { git = "https://github.com/plexobject/plexspaces", tag = "v0.1.0", default-features = false }

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
plexspaces-sdk = { git = "https://github.com/plexobject/plexspaces", tag = "v0.1.0" }
```

## Key PlexSpaces Features

- `wit_bindgen::generate!` — WIT guest bindings
- `host::ask` — actor-to-actor request/reply
- `host::tuplespace_write` / `host::tuplespace_read` — URL queue coordination
- `app-config.toml` — supervisor with elastic-pool and shard-group child specs

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Detailed Design](../../../../docs/detailed-design.md)
- [SDK Guide](../../../../sdks/rust/plexspaces-sdk/README.md)
- [Ray web-crawl example](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
