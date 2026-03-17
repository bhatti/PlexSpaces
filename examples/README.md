# PlexSpaces Examples

Each example’s README should **explain what the example does** and **which PlexSpaces APIs it uses** (e.g. ProcessGroupRegistry, TupleSpace, RequestContext, SqliteKVStore). See `rust/embedded/chat_room`, `rust/embedded/feature_flags`, and `rust/embedded/mpi_collectives` for the expected format (Purpose, What It Demonstrates, API mapping/code patterns, Quick Start).

**Benchmarks and metrics (blog / HPC-style examples):** Examples that demonstrate large data or HPC patterns must **print a clear benchmarks block to stdout** (e.g. via `println!` or `eprintln!`) so it is always visible regardless of log level. Include: **data size** (grid size, record count, iterations, etc.), **compute time vs coordination time** (and %), **latency** (e.g. per message or per batch), **throughput** (ops/s or msg/s), **errors** (count), and **granularity** (compute/coord ratio). Goal: show how the framework handles non-trivial data with measurable metrics.

## Example evaluation (right abstractions / APIs / SDKs)

Every example must be evaluated to ensure it uses the **correct abstractions and APIs**; this is part of the standard review before an example is considered complete.

### Embedded (native Rust)

- **Use SDK, not low-level APIs**: Actor logic uses SDK decorators/macros (e.g. `#[gen_server_actor]`, `#[workflow_actor]`, `#[event_actor]`, `#[fsm_actor]`) and SDK spawn helpers (`spawn`, `spawn_with_facets`, `spawn_workflow_actor`). No direct `ActorFactory` or manual `Message::new`; use `call_message` / `cast_message` or the typed ref APIs (`GenServerRef::call`, `WorkflowRef::run` / `query` / `signal`).
- **Inter-actor coordination**: Via `GenServerRef` / `WorkflowRef` (ask/tell, run/query/signal), or TupleSpace/ProcessGroup where the example demonstrates those primitives. No bypass of the public API surface.
- **RequestContext**: Use explicit tenant/namespace (`RequestContext::new_without_auth(...)` or context from request). Do not use `RequestContext::internal()` except for documented system init.
- **README**: Must state which APIs/SDK features the example uses (e.g. “SDK: gen_server_actor, spawn(), GenServerRef, TupleSpace”).

### Apps (WASM)

- **Correct ABI**: WASM apps implement the **plexspaces:simple-actor** ABI expected by the node: `init`, `handle`, `get-state`, `set-state` (and `cabi_realloc` / `cabi_post_*` as required). Export names must match the WIT (e.g. `plexspaces:simple-actor/actor@0.1.0#handle`).
- **GenServer-style ops**: Messages use an `op` (or `message_type`) field; handler dispatches on op (e.g. `run`, `query`, `signal`, `get_status`). State is serializable (e.g. JSON) for get/set-state and durability.
- **Config**: `app-config.toml` declares the actor as a GenServer child with the correct `id`/`type`; test script uses the same actor type and instance id when calling the HTTP API.
- **Framework services via WIT**: Use simple-actor host functions for tuple space, pools, and shard-group scatter-gather (`create-shard-group`, `bulk-update-shard-group`, `scatter-gather`) instead of direct gRPC clients in examples.
- **No host-only code in WASM**: WASM crate does not depend on tokio, plexspaces-node, or other host-only crates; only serde/serde_json (or language equivalents) and the export surface. Host provides the runtime.

### Checklist before marking an example done

- [ ] Uses SDK decorators and helpers (or simple-actor ABI for WASM) — no raw ActorFactory / manual Message construction where SDK exists.
- [ ] README documents which abstractions/APIs the example uses.
- [ ] Shared target and debug build by default; scripts (build.sh / test.sh) in example directory.
- [ ] Benchmarks block (for blog/HPC examples) with data size, compute vs coord, latency, errors.
- [ ] Test script validates success and, where applicable, presence of metrics.

## Multi-node and realistic benchmarks (plan)

For **realistic benchmarks**, examples may support **multi-node** execution: run multiple nodes (same machine or EC2) with optional **shared DB** (e.g. DB2) and parallelize the workload. Evaluation of examples should consider scaling and parallelism.

### Server script: optional port, per-port log

- **scripts/server.sh [GRPC_PORT]** — Optional gRPC port (default **8091**). HTTP = gRPC+1 (e.g. 8092). Log file: **plexspaces-node-{port}.log** so multiple servers can run (e.g. `./scripts/server.sh` then `./scripts/server.sh 8093` in another terminal for two nodes).

### What multi-node parallelization means (one run, work split)

**Multi-node parallelization is not “run the same job N times on N nodes.”** It means **one logical run** where work is **split across nodes** in a **leader-worker** way:

- **Deploy** the app to multiple nodes (so each node can run workers).
- **Entry point**: The client sends **one** request (e.g. one “run”) to **one** node (the **leader** node).
- **Leader** (coordinator on that node): Receives the single run, **splits the workload** (e.g. by chunk, shard, or partition), **spawns or uses workers** on the same node and/or on **other** nodes, sends sub-tasks to them (via routing to `actor_id@node_id`), and **aggregates** results.
- **Workers** on other nodes: Receive sub-tasks from the leader, do the work, reply. Messaging uses existing **location-transparent** routing (`actor_id@node_id` → gRPC to that node).

APIs involved: **ConnectNodes** (so the leader node knows peer nodes), **NodeRegistry** / list nodes, **remote spawn** (call `ActorService.SpawnActor` on another node’s gRPC channel via `get_actor_service_client(node_id)`), and **message routing** (send to `worker_id@node_id`). ShardGroup placement uses the same proto `NodePlacement` model as the rest of the framework, so examples can rely on registry-based placement without inventing SDK-specific placement types.

**Leader = first node.** The node that receives the single run is the **leader**; it splits work and coordinates workers.

**Preferred: virtual actors (no explicit spawn).** Deploy the same app to all nodes; register the worker as a **virtual** actor type. The leader gets peer node IDs (e.g. `plexspaces_sdk::leader_worker::list_worker_node_ids`), then sends work to `worker/chunk-1@node-B`, `worker/chunk-2@node-C`, etc. Virtual actors are **created lazily on first message receive**—no explicit spawn or ensure. The leader just sends to each address; the target node creates the actor when the first message arrives. Same semantics in all SDKs (Rust, Python, TypeScript, Go).

**Alternative: elastic pool.** Use an elastic pool of workers; today the pool is single-node. For multi-node, the leader can create a pool per node or use **spawn_actor_on_node** (SDK: `plexspaces_sdk::leader_worker::spawn_actor_on_node`) to create workers on each peer node, then send work to the returned actor refs. Pool semantics (checkout/checkin, scale by data size) can be built on top of these helpers.

**SDK helpers (feature `grpc`):** `leader_worker::list_worker_node_ids`, `leader_worker::spawn_actor_on_node` (for non-virtual workers only). Virtual actors: send to `actor_id@node_id`; no ensure. See `sdks/rust/plexspaces-sdk/src/leader_worker.rs`.

### Test script contract: optional host:port list

- **test.sh** should **optionally** accept a **list of host:port** (e.g. `./test.sh 8092` for single node, or `./test.sh "localhost:8092 localhost:8094"` for multi-node — use HTTP ports). Single port can be a number (implies `localhost:port`).
- When multiple host:port are given: **deploy** to all nodes so each can run workers; then send **one** run to the **entry node** (e.g. first in the list). That one run is handled by the **leader** on the entry node, which should split work and coordinate with workers on other nodes (when the example implements leader-worker). Do **not** run one full job per node; that is not multi-node parallelization.
- Default: single node at `localhost:${HTTP_PORT}` (e.g. 8092).

### Abstractions for scaling and parallelism

Use these where the example benefits; document which ones the example uses:

- **Behaviors**: GenServer, Workflow, GenEvent, FSM (durable, virtual facets).
- **Elastic pool**: Scale workers up/down; use for batch or queue processing.
- **ShardGroup**: Shard key → worker; data-parallel or partitioned workload.
- **Channels**: Kafka/NATS/SQS-style or in-process channels for pipeline stages.
- **Process group**: Pub/sub or group membership (e.g. broadcast to all workers).
- **TupleSpace**: Linda-style coordination (requires shared state; see below).
- **Node gRPC / node registry**: `ConnectNodes` (and node registry on top of object registry) to add other nodes so multi-node tests can form a cluster (Erlang-style “connect nodes”).

### Multi-node limitation: local SQLite vs shared DB

- **By default**, when testing **locally with multiple nodes**, each node typically uses **local SQLite**. Some constructs (e.g. **TupleSpace** with shared backing, or shared object registry) **may not work** across nodes without a **shared database**.
- For multi-node examples that need shared state (TupleSpace, shared registry, etc.), either:
  - Use a **shared DB** (e.g. DB2, PostgreSQL) configured for all nodes, or
  - Restrict the example to single-node when using local SQLite and document that.
- Node gRPC (ConnectNodes, node registry) works with or without shared DB for **node membership**; shared DB is still required for shared TupleSpace or shared KV/registry data.

### Redo / review for scaling

When redoing or reviewing an example (e.g. genomics_pipeline), ensure:

- All **abstractions** above that fit the use case are considered (behaviors, workflow, durable, virtual, elastic pool, ShardGroup, channels, process-group, TupleSpace where applicable).
- **Performance and scalability** are reflected in benchmarks (data size, multi-node if applicable, coord vs compute).
- **test.sh** supports optional list of host:port for multi-node runs when the example is designed for it.

## Structure

```
examples/
├── rust/
│   ├── embedded/           # 50 Rust embedded examples
│   └── apps/               # 2 Rust WASM actors
├── python/
│   └── apps/               # 10 Python WASM actors
├── typescript/
│   └── apps/               # 2 TypeScript WASM actors
├── go/
│   └── apps/               # 1 Go WASM actor
└── README.md
Native reference code (framework comparison) lives under each example’s `native/` folder when present (e.g. `examples/typescript/apps/migrating_temporal/native/`).

## Summary

| Language | Embedded | Apps | Total |
|----------|----------|------|-------|
| Rust | 50 | 2 | 52 |
| Python | - | 10 | 10 |
| TypeScript | - | 2 | 2 |
| Go | - | 1 | 1 |
| **Total** | **50** | **15** | **65** |

---

## Rust Embedded Examples (50)

### Core Patterns
| Example | Description |
|---------|-------------|
| `actor_groups_sharding` | Data-parallel scaling via sharding |
| `supervision_tree` | Erlang/OTP-style fault tolerance |
| `durable_actor` | Journaling and deterministic replay |
| `timers` | In-memory non-durable timers |
| `reminders` | Durable persistent reminders |
| `process_groups_pubsub` | Pub/Sub coordination |
| `chat_room` | Process groups: join/leave, publish_to_group, list_groups |
| `webhook_handler` | Webhook handler (HTTP deliver/list) |

### Scientific Computing
| Example | Description |
|---------|-------------|
| `heat_diffusion` | 2D stencil computation |
| `matrix_multiply` | Parallel matrix multiplication |
| `matrix_vector_mpi` | MPI-style collective primitives |
| `byzantine` | PBFT consensus |
| `nbody` | Gravitational simulation |
| `nbody_wasm` | N-Body with WASM actors |

### Domain Applications
| Example | Description |
|---------|-------------|
| `genomics_pipeline` | DNA sequence analysis |
| `genomic_workflow_pipeline` | Multi-sample processing |
| `finance_risk` | Monte Carlo risk analysis |
| `order_processing` | Workflow orchestration |
| `timeseries_forecasting` | ML prediction pipeline |
| `entity_recognition` | NER with streaming |

### WASM & Polyglot
| Example | Description |
|---------|-------------|
| `wasm_showcase` | Full WASM capability demo |
| `wasm_calculator` | Python WASM calculator actors |
| `firecracker_multi_tenant` | VM isolation |

### Migration Guides (26)
All `migrating_*` examples show how to migrate from other frameworks.

---

## Running Examples

### Rust Embedded
```bash
cd rust/embedded/<example>
cargo run
```

### Python Apps
```bash
cd python/apps/<app>
componentize-py <actor>.py -o <app>.wasm
plexspaces deploy <app>.wasm --tenant my-tenant
```

### TypeScript Apps
```bash
cd typescript/apps/<app>
jco compile <actor>.ts -o <app>.wasm
plexspaces deploy <app>.wasm --tenant my-tenant
```

### Go Apps
```bash
cd go/apps/<app>
./build.sh    # builds WASM with TinyGo
./test.sh     # deploys and runs E2E tests
```

#### Go SDK Examples

| Example | Description | README |
|---------|-------------|--------|
| **Migrating Erlang/OTP** | Sliding window rate limiter (GenServer pattern) | [README](go/apps/migrating_erlang_otp/README.md) |

---

## TODOs

- [ ] Add more Go apps (counter, calculator)
- [ ] Add more TypeScript apps
- [ ] Review and verify each example compiles/runs
- [ ] Consolidate similar migration examples

See [CHECKLIST.md](CHECKLIST.md) for detailed tracking.
