# Redis Cluster with PlexSpaces Actors

Demonstrates how PlexSpaces distributed actor primitives replace — and far exceed — the manual
concurrency patterns in "Rust Projects - Write a Redis Clone" (Ch5–9). Two real gRPC nodes, hash-partitioned
shard groups, replication, transactions, and cross-shard queries, all in ~300 lines of application code.

## What it demonstrates

| Book concept (Ch5–9)              | PlexSpaces primitive        | Benefit                              |
|-----------------------------------|-----------------------------|--------------------------------------|
| Server actor + MPSC channels (Ch5) | Shard Group of StorageActors | Auto-partitioned across N shards     |
| ConnectionHandler per client      | Virtual Actor               | Auto-lifecycle, no registry needed   |
| tokio::select! multiplexing       | Actor mailbox (built-in)    | Zero boilerplate                     |
| Command modules (Ch6)             | `#[handler]` annotations    | Declarative dispatch                 |
| Replication broadcast (Ch7–8)     | `broadcast_shard_group`     | One call fans out to all replicas    |
| WAIT for N ACKs (Ch8)             | `scatter_gather` + threshold | Built-in ACK collection             |
| Transactions MULTI/EXEC (Ch9)     | Per-VirtualActor queue      | No locks, no shared state            |
| Cross-shard KEYS/DBSIZE           | `scatter_gather` + `reduce(SUM)` | Parallel fan-out                |
| Coordinated snapshot              | `map` (parallel)            | All shards queried simultaneously    |
| Active key expiry                 | `broadcast expire_sweep`    | Parallel sweep across all shards     |

## Architecture

```
redis-master-node (gRPC :8091)      redis-replica-node (gRPC :8093)
┌──────────────────────────────┐    ┌──────────────────────────────┐
│  StorageActor shard-0        │    │  StorageActor replica-0      │
│  StorageActor shard-1        │──▶ │  StorageActor replica-1      │
│  StorageActor shard-2        │    │  StorageActor replica-2      │
│                              │    │                              │
│  ConnectionActor client-a    │    │  (replicas spawned via gRPC  │
│  ConnectionActor client-b    │    │   during create_shard_group) │
└──────────────────────────────┘    └──────────────────────────────┘
         ▲ bulk_update (SET/GET/INCR/DEL — hash-partitioned)
         ▲ broadcast_shard_group (replication, expire_sweep)
         ▲ scatter_gather (WAIT ACK, KEYS)
         ▲ reduce(SUM) (DBSIZE)
         ▲ map (snapshot, parallel queries)
```

Both nodes start in-process with real gRPC servers on fixed ports:
- `redis-master-node` → **:8091**
- `redis-replica-node` → **:8093**

Replica shards are created on `redis-replica-node` via a gRPC call during `create_shard_group`.
All subsequent primitives (`broadcast`, `scatter_gather`, `reduce`, `map`) route across nodes
transparently. Ports are fixed (not ephemeral) so external tools, logs, and the Python WASM
counterpart can observe activity on both nodes.

## Key files

| File | Purpose |
|------|---------|
| `src/main.rs` | Demo driver — 10 steps exercising all primitives |
| `src/cluster.rs` | `RedisCluster` struct wrapping `ShardGroupClientLocal`; implements SET/GET/INCR/broadcast/scatter_gather/reduce/map |
| `src/storage.rs` | `StorageActor` — one shard, all Redis handlers (`#[handler]` annotations) |
| `src/connection.rs` | `ConnectionActor` — virtual actor per client, holds MULTI/EXEC transaction queue |
| `src/replication.rs` | Replication handshake and bulk state sync helpers |
| `src/lib.rs` | `StoredEntry` struct, module re-exports |
| `proto/redis_cluster.proto` | Protocol contract for cluster messages |
| `scripts/test.sh` | Build, run, and validate all 10 demo steps |

## How to run

```bash
cd examples/rust/embedded/redis_cluster
cargo run --bin redis_cluster
```

Requires the shared workspace target (configured in `.cargo/config.toml`):

```toml
[build]
target-dir = "../../../../target"
```

## How to test

```bash
./scripts/test.sh
```

The script builds, runs the demo, and validates that all 10 steps emit expected output.
Exits 0 on success, 1 on any failure.

## Metrics

Each step reports `coord_ms` — the time spent in collective PlexSpaces operations (broadcast, scatter_gather, reduce, map, create_shard_group, bulk_update):

```
Step 1: Cluster Setup — two real nodes, shard groups, replication handshake
  ✓ redis-master-node started with gRPC server on :8091
  ✓ redis-replica-node started with gRPC server on :8093
  ✓ Master shard group 'redis-masters': 3 shards on redis-master-node
  ✓ Replica shard group 'redis-replicas': 3 shards on redis-replica-node
  ✓ Replication handshake: PING → REPLCONF → PSYNC
  → Cluster ready: 3 masters on node-1, 3 replicas on node-2
  → coord: 87ms (create_shard_group x2, handshake broadcast, bulk_sync)

Step 6: Replication (Ch7-8 — broadcast_shard_group to all replicas)
  ✓ broadcast_shard_group → 3 replica shards ACKed
  → Replication: write propagated to all 3 replica shards via broadcast  |  coord: 8ms

Step 7: WAIT Command (Ch8 — scatter_gather for ACK collection)
  ✓ WAIT 2 5000 → 3 replicas at offset >= 2
  → scatter_gather collected ACKs from 3 replica shards  |  coord: 5ms

Step 8: Cross-Shard Queries (scatter_gather + reduce)
  ✓ DBSIZE via reduce(SUM) across 3 shards → 12 total keys
  ✓ KEYS via map + concat across 3 shards → 12 keys
  → Cross-shard queries: scatter-gather + reduce working  |  coord: 12ms

Step 9: Coordinated Snapshot (parallel map across all shards)
  ✓ Shard 0: 4 keys
  ✓ Shard 1: 5 keys
  ✓ Shard 2: 3 keys
  → Parallel map snapshot: all shards queried simultaneously, results merged  |  coord: 15ms
```

Most operations complete in single-digit milliseconds. Step 1 (cluster setup) takes longer because it creates two shard groups, runs the replication handshake over gRPC, and performs the initial bulk sync.

## How to Test

### Run the demo

```bash
cd examples/rust/embedded/redis_cluster
cargo run --bin redis_cluster
```

All 10 steps print with ✓ for each check and `coord: Xms` timing at the end of each step. If any step fails, it prints ✗ and the program exits with code 1.

### Run the test suite

```bash
./scripts/test.sh
```

The test script:
1. Builds the binary with `CARGO_TARGET_DIR=../../../../target cargo build --bin redis_cluster`
2. Runs with `RUST_LOG=warn` to suppress framework noise
3. Verifies each step's expected output patterns (cluster ready, primitives used, timing lines)
4. Exits 0 on full success, 1 on any failure

### Expected output

All 10 steps should pass with ✓. The final summary looks like:

```
  ✓ Virtual Actors        — ConnectionActor per-client (auto-lifecycle)
  ✓ Shard Groups          — hash-partitioned StorageActor fleet
  ✓ Broadcast             — replication propagation to all replicas
  ✓ Scatter-Gather        — WAIT ACK collection, cross-shard reads
  ✓ Reduce (SUM)          — DBSIZE aggregation across shards
  ✓ Map (parallel)        — coordinated snapshot across all shards
  ✓ Multi-Node (real gRPC)— masters on node-1, replicas on node-2
```

## Book concepts eliminated

- Manual MPSC channels — actor mailbox handles it
- tokio::select! loops — actor model handles it
- Manual replica loops — `broadcast_shard_group` handles it
- Manual connection tracking — virtual actor auto-lifecycle
- Manual shard routing — `bulk_update` partition key handles it
- Locks for MULTI/EXEC — actor processes one message at a time

## References

- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
- [PlexSpaces SDK](../../../../sdks/rust/plexspaces-sdk/)

## License

SPDX-License-Identifier: AGPL-3.0-or-later  
Copyright (C) 2025 Shahzad A. Bhatti
