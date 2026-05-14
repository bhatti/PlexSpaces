# Web Crawler (Python WASM)

A parallel web crawler compiled to WASM with componentize-py, using ElasticPool
(fetcher pool), TupleSpace (URL queue), and ShardGroup (map-reduce word frequency).

Modeled after [Ray's web-crawl](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
and [map-reduce](https://docs.ray.io/en/latest/ray-core/examples/map_reduce.html) examples.

**This example installs the Python SDK from PyPI — no local PlexSpaces checkout needed.**

## What It Demonstrates

| PlexSpaces Feature | Role |
|---|---|
| `@gen_server_actor` | PageFetcher, LinkAnalyzer, WebCrawlOrchestrator |
| `@handler("op")` | Typed message dispatch |
| ElasticPool pattern | 4 fetchers reused round-robin across URLs |
| TupleSpace | `url_queue` space: pending → done URL tracking |
| ShardGroup pattern | 2 analyzer shards: scatter-gather reduce |
| `host.ask` | Actor-to-actor RPC for fetch + analyze |
| `ACTOR_ROLES` dict | Multi-role dispatch (one WASM, multiple roles) |

## Actors

| Class | Behavior | Role |
|---|---|---|
| `WebCrawlOrchestrator` | GenServer | BFS crawl loop |
| `PageFetcher` (×4) | GenServer (virtual) | Fetch URL, return links + words |
| `LinkAnalyzer` (×2) | GenServer | Shard: merge counts, top-N words |

## Build

```bash
./build.sh
```

Requirements: Python ≥ 3.11, `componentize-py`, `plexspaces` from PyPI.

## Run Tests

```bash
# Start a node first:
./scripts/server.sh  # from repo root

./test.sh 8091
```

## Expected Output

```
Step 0: Build
  ✓ web_crawl_actor.wasm (2.1M)

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
Web crawl (Python) test passed.
```

## SDK Import (external project)

```bash
pip install plexspaces==0.1.0
```

```python
from plexspaces import gen_server_actor, handler, host, state
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Python SDK README](../../../../sdks/python/README.md)
- [Ray web-crawl example](https://docs.ray.io/en/latest/ray-core/examples/web_crawler.html)
