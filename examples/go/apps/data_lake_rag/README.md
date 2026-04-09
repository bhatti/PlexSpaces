# Data Lake RAG (Go WASM)

Synthetic retrieval-augmented generation benchmark using the Go SDK with a multinode leader/worker shard-group layout.

## Purpose

Show ingest, chunking, embedding, and retrieval across multiple workers with:

- explicit `leader` and `worker` child types
- Go SDK `ActorRouter` for multi-actor routing in one WASM module
- shard-group placement with framework-owned worker actor IDs
- application-metrics-backed per-role and per-node reporting

## Architecture

```mermaid
flowchart LR
  C["Client"] --> L["Leader / Query Coordinator"]
  L --> W1["Worker 0"]
  L --> W2["Worker 1"]
  L --> WN["Worker N"]
  W1 --> L
  W2 --> L
  WN --> L
  L --> A["Top-K Retrieval Results / Metrics"]
```

```mermaid
sequenceDiagram
  participant Client
  participant Leader
  participant Workers

  Client->>Leader: run(query_count, chunks_per_query, ...)
  loop each query round
    Leader->>Workers: host.ScatterGather(search_chunks, query)
    Workers-->>Leader: top candidates + chunk/embed stats + latency
    Leader->>Leader: aggregate top-k results
  end
  Leader-->>Client: retrieval benchmark report
```

## Files

- `data_lake_rag_actor.go`: leader and worker actors plus metrics helpers
- `app-config.toml`: reusable `leader` and `worker` child types for shard-group spawning
- `build.sh`: build the Go/TinyGo WASM component
- `test.sh`: deploy to both nodes, render `seed_nodes`, run retrieval, and print benchmark stats

## Quick Start

```bash
cd examples/go/apps/data_lake_rag
./build.sh
./test.sh
```

## Multinode

Shard placement uses `from_registry`, which requires a non-empty node registry (`list_nodes`). If you see `Placement produced no target nodes`, check:

1. **`list_nodes` cluster filter** — If `NodeConfig.cluster_name` is set but registry entries lack a matching `cluster` capability, every node is filtered out.
2. **Shared SQLite** — Two processes using the same default DB path under `base_dir` can contend; use distinct DB URLs per process when needed (for example `PLEXSPACES_DATABASE_URL`).
3. **Peer connectivity** — App `seed_nodes` drive `connect_to_node_addresses`; mTLS or address mistakes prevent remote members from appearing.
4. **Release `node.cluster_seed_nodes`** — Optional static peers at node boot, in addition to app `seed_nodes`.

The actor service logs a warning with `list_cluster_filter`, `node_config_cluster_name`, and `placement_cluster_field` when `list_nodes` returns no members for this placement path.

## Notes

- This is a synthetic RAG benchmark, not a real embedding/vector database integration.
- The leader models ingest/chunking/query orchestration while workers simulate chunk embedding and retrieval scoring.
- Workers write node-local `ApplicationMetrics`, and the leader combines per-node application status deltas into the final report.
