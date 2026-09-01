# Redis Cluster — Python WASM

A Redis-like distributed key-value store implemented as Python WASM actors, deployed
to a running PlexSpaces node. Demonstrates how PlexSpaces actor primitives eliminate
the manual concurrency code from "Rust Projects - Write a Redis Clone" (Ch5–9).

## Book Concept → PlexSpaces Primitive

| Book Concept (Ch5–9) | PlexSpaces Primitive | Benefit |
|---|---|---|
| Server actor + MPSC (Ch5) | Shard Group (StorageActor) | Auto-partitioned shards |
| ConnectionHandler per client | Virtual Actor (ConnectionActor) | Auto-lifecycle, no cleanup |
| tokio::select! multiplexing | Actor mailbox (built-in) | Zero boilerplate |
| Command modules (Ch6) | `@handler` annotations | Declarative dispatch |
| Replication broadcast (Ch7-8) | `broadcast_shard_group` | One call, all replicas |
| WAIT for N ACKs (Ch8) | `scatter_gather` + offset check | Built-in threshold |
| Transactions MULTI/EXEC (Ch9) | Per-VirtualActor queue | No locks needed |
| Cross-shard queries (KEYS, SIZE) | `scatter_gather` + `reduce(SUM)` | Parallel fan-out |
| Active key expiry | `broadcast` expire_sweep | Simultaneous all-shard |
| Coordinated snapshot | `map_shard_group` | Parallel across shards |

## Architecture

```
  Client HTTP request
         │
         ▼
  RedisCoordinator  ──── create_shard_group ────► StorageActor x3  (masters)
  (GenServer actor)  ──── broadcast_shard_group ─► StorageActor x3  (replicas)
         │             ── scatter_gather ─────────► StorageActor x3
         │             ── reduce_shard_group ──────► StorageActor x3
         │             ── map_shard_group ──────────► StorageActor x3
         │
         └──── per-client virtual actor ──────────► ConnectionActor (MULTI/EXEC)
```

**RedisCoordinator** — created by `app-config.toml`. Initializes the cluster via
`setup` handler, then routes GET/SET/INCR/KEYS/DBSIZE/WAIT/snapshot to the
appropriate PlexSpaces collective operation.

**StorageActor** — shard member. Owns a slice of the hash-partitioned keyspace.
Handles all Redis data commands plus replication, expiry, and snapshot handlers.

**ConnectionActor** — virtual actor per client. Manages MULTI/EXEC/DISCARD state
with no locks — structural isolation because each actor processes one message at a time.

## Prerequisites

- PlexSpaces node running (`plexspaces-node start` or `cargo run -p plexspaces-node`)
- Python 3.10+ with `venv` at `~/venv` (or SDK installed separately)
- `plexspaces-py` CLI installed (`pip install -e sdks/python`)

## Build

```bash
chmod +x build.sh
./build.sh
```

This produces `redis_cluster_actor.wasm`.

## Test

```bash
# Start a PlexSpaces node first (default port 8091)
chmod +x test.sh
./test.sh
# or with a custom port:
./test.sh 8091
```

The test script deploys the WASM application, calls `setup` to initialize shard
groups, then exercises all Redis operations.

## Manual Deploy

```bash
APP_ID="redis-cluster"
zip -j redis_cluster.zip redis_cluster_actor.wasm app-config.toml
curl -s -X POST "http://localhost:8091/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "app_file=@redis_cluster.zip"

# Initialize cluster
curl -s -X POST "http://localhost:8091/api/v1/actors/$APP_ID/redis-coordinator" \
    -H "Content-Type: application/json" \
    -d '{"op":"setup"}'

# Basic commands
curl -s -X POST "http://localhost:8091/api/v1/actors/$APP_ID/redis-coordinator" \
    -H "Content-Type: application/json" \
    -d '{"op":"set","key":"user:1","value":"alice"}'

curl -s -X POST "http://localhost:8091/api/v1/actors/$APP_ID/redis-coordinator" \
    -H "Content-Type: application/json" \
    -d '{"op":"get","key":"user:1"}'
```

## Key Files

| File | Purpose |
|---|---|
| `redis_cluster_actor.py` | All actor classes: `StorageActor`, `ConnectionActor`, `RedisCoordinator` |
| `app-config.toml` | Application spec: deploys `RedisCoordinator` under a supervisor |
| `build.sh` | Compiles `redis_cluster_actor.py` → `redis_cluster_actor.wasm` |
| `test.sh` | Full integration test against a running PlexSpaces node |
| `undeploy.sh` | Remove the application from the node |

## See Also

- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
- [Rust Embedded Version](../../embedded/redis_cluster/) — same example, Rust actors embedded in-process
- [Parameter Server Example](../parameter_server/) — `scatter_gather` + `create_shard_group` pattern

## License

AGPL-3.0-or-later. See [LICENSE](../../../../LICENSE).
