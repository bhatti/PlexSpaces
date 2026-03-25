# PlexSpaces examples

Examples live under `examples/<language>/{apps,embedded}/<name>/`. Each directory should include a **README** that states purpose, which PlexSpaces APIs and SDK features are used, and how to build and test.

**Framework migration:** Examples named `migrating_*` show how to model workloads that originally used another framework (Temporal, Merlin, Ray, Dapr, and so on). Treat them as migration guides, not generic tutorials.

**Parallelism, collectives, scatter/gather, and workload / ML-style demos:** For MPI-style shard-group collectives, multi-node benchmarks, and ring/allreduce-style training narratives, start with **Go `mpi_collectives`**, **Rust embedded `matrix_vector_mpi`**, **Rust apps `ring_allreduce`**, **`data_parallel_worker`**, **`parameter_server`** (Rust, Python, Go), **`batch_image_classification`**, **`genomics_pipeline`**, **`heat_diffusion`** (apps and embedded), **`data_lake_rag`**, and **Python `job_processing`** (TupleSpace scatter/gather). **Rust embedded `event_analytics`** and **`realtime_stream_processor`** illustrate shard groups and streaming analytics at scale.

Further reading: [SDK guide](../docs/sdk.md), [Polyglot WASM](../docs/polyglot.md), [WASM deployment](../docs/wasm-deployment.md), [Durability](../docs/durability.md).

---

## Go (`examples/go/apps/`)

| Example | Description | Abstractions / APIs | README |
|---------|-------------|---------------------|--------|
| `data_lake_rag` | Synthetic data-lake RAG-style benchmark: ingest, chunking, embedding, retrieval across leader/worker shard groups. | Go SDK `ActorRouter`, shard-group placement, application metrics | [README](go/apps/data_lake_rag/README.md) |
| `migrating_aws_durable_lambda` | **Migration:** AWS Lambda–style durable execution patterns (webhook-style actor). | Go SDK WASM actor, workflow/durability patterns; see `native/` | [README](go/apps/migrating_aws_durable_lambda/README.md) |
| `migrating_cadence` | **Migration:** Cadence-style payment workflow with idempotency and retries. | `WorkflowActor`, Run/Signal/Query, virtual actor + durability | [README](go/apps/migrating_cadence/README.md) |
| `migrating_cloudflare_workers` | **Migration:** Durable Objects–style guild chat. | Go SDK WASM; compare with `native/guild_chat.js` | [README](go/apps/migrating_cloudflare_workers/README.md) |
| `migrating_dapr` | **Migration:** Dapr-style background jobs, retries, DLQ. | `WorkflowActor`, queue/DLQ in state; `native/job_processor_dapr.go` | [README](go/apps/migrating_dapr/README.md) |
| `migrating_eflows4hpc` | **Migration:** eFlows4HPC-style ensemble workflows. | WASM ensemble actor; see `native/` | [README](go/apps/migrating_eflows4hpc/README.md) |
| `migrating_erlang_otp` | **Migration:** Erlang/OTP sliding-window rate limiter (GenServer-style). | `BaseActor`, `Host`, `ActorRouter`; `native/rate_limiter.erl` | [README](go/apps/migrating_erlang_otp/README.md) |
| `migrating_gosiris` | **Migration:** Go sIRIS-style sensor aggregation. | Go SDK WASM; `native/sensor_system.go` | [README](go/apps/migrating_gosiris/README.md) |
| `migrating_merlin` | **Migration:** Merlin-style parameter sweep (elastic pool + tuple space, process-group fallback). | `WorkflowActor`, pool checkout/checkin, TupleSpace, `work_available` | [README](go/apps/migrating_merlin/README.md) |
| `migrating_mozartspaces` | **Migration:** MozartSpaces-style auction coordination. | Go SDK WASM; `native/mozartspaces_ref.md` | [README](go/apps/migrating_mozartspaces/README.md) |
| `migrating_rivet` | **Migration:** Rivet-style matchmaking. | Go SDK WASM; `native/` reference | [README](go/apps/migrating_rivet/README.md) |
| `migrating_temporal` | **Migration:** Temporal-style order fulfillment workflow. | `WorkflowActor`, signals/queries; `native/` | [README](go/apps/migrating_temporal/README.md) |
| `mpi_collectives` | **Parallel / MPI:** Benchmark of BroadcastShardGroup, ScatterGather, ReduceShardGroup, AllReduceShardGroup, BarrierShardGroup over a dynamic shard group. | Shard-group collectives, per-node metrics, registry placement | [README](go/apps/mpi_collectives/README.md) |
| `parameter_server` | **Workload / ML:** Synthetic distributed training coordination (leader/worker, shard groups, metrics). | `ActorRouter`, shard-group placement, application metrics | [README](go/apps/parameter_server/README.md) |

**Build / test (typical Go WASM app):**

```bash
cd go/apps/<example>
./build.sh
./test.sh <http_port>   # node must be running; port matches your HTTP API
```

---

## Python (`examples/python/apps/`)

| Example | Description | Abstractions / APIs | README |
|---------|-------------|---------------------|--------|
| `audit_log` | Fire-and-forget audit events via host logging (storage patterns evolve with WASM). | Python SDK `@actor` / handlers, `host.log` | [README](python/apps/audit_log/README.md) |
| `bank_account` | Durable bank account state with deposit/withdraw/history. | `@actor`, `state()`, `@handler`, WASM durable state | [README](python/apps/bank_account/README.md) |
| `calculator` | Simple calculator operations and batch throughput. | GenServer-style handlers, WASM | [README](python/apps/calculator/README.md) |
| `cdn_cache` | CDN-style edge caching behavior. | Python SDK WASM, host KV / coordination as documented | [README](python/apps/cdn_cache/README.md) |
| `chat_room` | Real-time chat using process groups. | Process groups, pub/sub-style messaging | [README](python/apps/chat_room/README.md) |
| `feature_flags` | Gradual rollout with supervision. | `@actor`, supervisor patterns | [README](python/apps/feature_flags/README.md) |
| `fsm` | Order workflow as a finite state machine. | FSM / workflow-style handlers | [README](python/apps/fsm/README.md) |
| `job_processing` | **Parallel:** Distributed jobs via TupleSpace scatter/gather coordination. | TupleSpace, coordinator/worker pattern | [README](python/apps/job_processing/README.md) |
| `leader_election` | Single-leader election using distributed lock pattern. | LockFacet; see README for current status | [README](python/apps/leader_election/README.md) |
| `migrating_aws_step_functions` | **Migration:** AWS Step Functions–style ML pipeline stages. | Workflow-style actor; `native/` | [README](python/apps/migrating_aws_step_functions/README.md) |
| `migrating_conductor` | **Migration:** Conductor-style order saga (compensating transactions). | Workflow, virtual actor, durability | [README](python/apps/migrating_conductor/README.md) |
| `migrating_eflows4hpc` | **Migration:** eFlows4HPC ensemble actor. | Ensemble workflow; `native/` | [README](python/apps/migrating_eflows4hpc/README.md) |
| `migrating_kueue` | **Migration:** Kueue-style batch job scheduling. | Scheduler actor; `native/kueue_ref.md` | [README](python/apps/migrating_kueue/README.md) |
| `migrating_luats` | **Migration:** LuaTS-style pipeline actor. | Pipeline stages; `native/luats_ref.md` | [README](python/apps/migrating_luats/README.md) |
| `migrating_merlin` | **Migration:** Merlin-style parameter sweep (pool + tuple space). | Workflow, elastic pool, TupleSpace | [README](python/apps/migrating_merlin/README.md) |
| `migrating_ractor` | **Migration:** Rust `ractor`-style calculator comparison. | Side-by-side with `native/calculator.rs` | [README](python/apps/migrating_ractor/README.md) |
| `migrating_ray` | **Migration:** Ray-style parameter server and distributed tasks. | `native/parameter_server.py` | [README](python/apps/migrating_ray/README.md) |
| `migrating_skypilot` | **Migration:** SkyPilot-style elastic job runner. | Job runner actor; `native/skypilot_ref.md` | [README](python/apps/migrating_skypilot/README.md) |
| `migrating_temporal` | **Migration:** Temporal-style order fulfillment. | Workflow actor; `native/` | [README](python/apps/migrating_temporal/README.md) |
| `migrating_wasmcloud` | **Migration:** wasmCloud-style session store. | Actor capabilities pattern; `native/` | [README](python/apps/migrating_wasmcloud/README.md) |
| `migrating_zeebe` | **Migration:** Zeebe/BPMN-style claim workflow. | Workflow; `native/zeebe_claim_bpmn.md` | [README](python/apps/migrating_zeebe/README.md) |
| `nbody` | **Workload / ML:** N-body gravitational simulation on WASM. | Multi-step `run`, physics ops, metrics | [README](python/apps/nbody/README.md) |
| `parameter_server` | **Workload / ML:** Parameter server–style training coordination (Python WASM). | Leader/worker, shard groups, metrics | [README](python/apps/parameter_server/README.md) |
| `payment_handler` | Payment processing demo. | Python SDK WASM handlers | [README](python/apps/payment_handler/README.md) |
| `receipt_storage` | Receipt / expense storage with filtering. | Durable state, queries | [README](python/apps/receipt_storage/README.md) |
| `registry` | Service discovery via registry facet. | `RegistryFacet` | [README](python/apps/registry/README.md) |
| `storefront` | E-commerce storefront API on host KV. | `host.kv_*`; see README for node feature requirements | [README](python/apps/storefront/README.md) |
| `task-queue` | Background tasks with distributed locks. | `LockFacet`, task claiming | [README](python/apps/task-queue/README.md) |

```bash
cd python/apps/<app>
./build.sh    # or project-specific build
./test.sh <http_port>
```

---

## Rust — WASM apps (`examples/rust/apps/`)

| Example | Description | Abstractions / APIs | README |
|---------|-------------|---------------------|--------|
| `batch_image_classification` | **Parallel / ML:** Batch image classification fan-out (synthetic workload), shard-group scatter/gather, metrics. | `#[gen_server_actor(wasm)]`, shard groups, application metrics | [README](rust/apps/batch_image_classification/README.md) |
| `calculator` | Four-function calculator WASM actor with batch throughput. | `#[gen_server_actor(wasm)]`, `#[plexspaces_handlers(wasm)]`, WIT Guest | [README](rust/apps/calculator/README.md) |
| `data_parallel_worker` | **Parallel:** Shard group rounds with scatter/gather over workers (SDK + WIT `scatter_gather`). | WASM GenServer, shard group, `host::scatter_gather` | [README](rust/apps/data_parallel_worker/README.md) |
| `entity_recognition` | Doc → entity extraction heuristics (EMAIL/URL/TOKEN); batch ops. | WASM SDK annotations, `application_metrics_add` | [README](rust/apps/entity_recognition/README.md) |
| `genomics_pipeline` | **Workload / ML:** Genomics pipeline leader/worker stages with shard-group coordination. | WASM leader/worker, genomics narrative, metrics | [README](rust/apps/genomics_pipeline/README.md) |
| `heat_diffusion` | **Parallel:** Thermal diffusion leader/worker (synthetic PDE-style workload). | WASM shard groups, diffusion steps, metrics | [README](rust/apps/heat_diffusion/README.md) |
| `migrating_eflows4hpc` | **Migration:** eFlows4HPC ensemble (Rust WASM). | Workflow/WASM patterns; `native/` | [README](rust/apps/migrating_eflows4hpc/README.md) |
| `migrating_merlin` | **Migration:** Merlin parameter sweep (Rust WASM). | Pool + TupleSpace + workflow handlers | [README](rust/apps/migrating_merlin/README.md) |
| `migrating_skypilot` | **Migration:** SkyPilot-style workload (Rust WASM). | Job/worker patterns; `native/` | [README](rust/apps/migrating_skypilot/README.md) |
| `migrating_temporal` | **Migration:** Temporal order workflow (Rust WASM). | Workflow; `native/` | [README](rust/apps/migrating_temporal/README.md) |
| `nbody` | **Workload / ML:** Gravitational N-body on one WASM GenServer (`step`, `run_steps`). | `#[gen_server_actor(wasm)]`, metrics | [README](rust/apps/nbody/README.md) |
| `parameter_server` | **Workload / ML:** Parameter server benchmark (Rust WASM leader/worker). | Shard groups, scatter/gather, metrics | [README](rust/apps/parameter_server/README.md) |
| `ring_allreduce` | **Parallel / ML:** Ring all-reduce–style gradient aggregation narrative. | Leader/worker WASM, collective-style rounds | [README](rust/apps/ring_allreduce/README.md) |
| `session_manager` | Idle timeout and heartbeat using WASM timer host calls. | `#[gen_server_actor(wasm)]`, `send_after` / touch | [README](rust/apps/session_manager/README.md) |
| `test-common.sh` | Shared helpers invoked by Rust WASM app `test.sh` scripts (not a standalone example). | Shell helpers for deploy/test | [test-common.sh](rust/apps/test-common.sh) |

```bash
cd rust/apps/<app>
./build.sh
./test.sh
```

---

## Rust — embedded (native, `examples/rust/embedded/`)

| Example | Description | Abstractions / APIs | README |
|---------|-------------|---------------------|--------|
| `bank_account` | Durable bank account with journaling. | `#[gen_server_actor(durability)]`, `spawn_with_storage()` | [README](rust/embedded/bank_account/README.md) |
| `byzantine` | Byzantine consensus (PBFT-style) simulation. | GenServer, supervision, distributed protocol | [README](rust/embedded/byzantine/README.md) |
| `chat_room` | Process groups for broadcast (Slack/Discord-style). | `ProcessGroupRegistry`, `RequestContext` | [README](rust/embedded/chat_room/README.md) |
| `document_approval` | Document approval workflow. | GenServer / workflow-style embedded app | [README](rust/embedded/document_approval/README.md) |
| `entity_recognition` | Multi-node entity recognition pipeline (CPU/GPU config). | Native GenServer, sharding, aggregators | [README](rust/embedded/entity_recognition/README.md) |
| `event_analytics` | **Parallel:** Shard groups, hash routing, scatter/gather analytics aggregates. | `#[gen_server_actor]`, shard groups, metrics | [README](rust/embedded/event_analytics/README.md) |
| `genomic_workflow_pipeline` | Multi-sample genomic workflow with recovery scripts. | Supervision, durable workers | [README](rust/embedded/genomic_workflow_pipeline/README.md) |
| `genomics_pipeline` | DNA sequencing pipeline with worker pools and workflows. | GenServer, supervision, domain workers | [README](rust/embedded/genomics_pipeline/README.md) |
| `heat_diffusion` | **Parallel:** 2D stencil, TupleSpace ghost cells, barriers. | TupleSpace, `GenServerRef`, metrics | [README](rust/embedded/heat_diffusion/README.md) |
| `matrix_multiply` | **Parallel:** Matrix multiply with scatter/gather leader/workers. | `#[gen_server_actor]`, `spawn()`, `call()` | [README](rust/embedded/matrix_multiply/README.md) |
| `matrix_vector_mpi` | **Parallel / MPI:** MPI-style collectives and scatter/gather patterns. | TupleSpace / MPI ops module, shard-style coordination | [README](rust/embedded/matrix_vector_mpi/README.md) |
| `migrating_skypilot` | **Migration:** SkyPilot-style AI workload (embedded). | Native Rust SDK; `native/ai_workload.py` | [README](rust/embedded/migrating_skypilot/README.md) |
| `migrating_temporal` | **Migration:** Temporal workflow (embedded Rust + `native/` TS). | WorkflowActor patterns | [README](rust/embedded/migrating_temporal/README.md) |
| `mpi_collectives` | Reserved for Rust embedded MPI collectives; the full benchmark and README live under Go `mpi_collectives`. | Same collective APIs as documented in Go example | [README](go/apps/mpi_collectives/README.md) |
| `order_processing` | Order processing microservices with supervision. | Hybrid GenServer/Actor, services | [README](rust/embedded/order_processing/README.md) |
| `player_session` | Game / player session lifecycle. | GenServer, session patterns | [README](rust/embedded/player_session/README.md) |
| `realtime_stream_processor` | **Workload:** Clickstream-style stream processing with supervision. | Supervision trees, throughput metrics | [README](rust/embedded/realtime_stream_processor/README.md) |
| `reminders` | Durable reminders facet. | `ReminderFacet`, `#[actor]` | [README](rust/embedded/reminders/README.md) |
| `timers` | In-memory timers (idle, heartbeat, retry). | `TimerFacet`, `spawn_with_facets` | [README](rust/embedded/timers/README.md) |
| `timeseries_forecasting` | **Workload / ML:** Time-series forecasting pipeline. | Node spawn, pipeline actors | [README](rust/embedded/timeseries_forecasting/README.md) |
| `webhook_handler` | HTTP-delivered webhook actor (list/deliver). | `#[gen_server_actor]`, AskReply/SendMessage HTTP paths | [README](rust/embedded/webhook_handler/README.md) |

```bash
cd rust/embedded/<example>
cargo run
# or ./test.sh where provided
```

---

## TypeScript (`examples/typescript/apps/`)

| Example | Description | Abstractions / APIs | README |
|---------|-------------|---------------------|--------|
| `bank_account` | Durable bank account (same conceptual API as Python). | `PlexSpacesActor`, jco componentize, WIT world | [README](typescript/apps/bank_account/README.md) |
| `migrating_azure_durable_functions` | **Migration:** Azure Durable Functions–style document processing. | TS SDK workflow patterns; `native/` | [README](typescript/apps/migrating_azure_durable_functions/README.md) |
| `migrating_cloudflare_workers` | **Migration:** Cloudflare Workers / Durable Objects patterns. | TS WASM actor | [README](typescript/apps/migrating_cloudflare_workers/README.md) |
| `migrating_dirigo` | **Migration:** Dirigo-style stream processing. | `native/dirigo_ref.md` | [README](typescript/apps/migrating_dirigo/README.md) |
| `migrating_eflows4hpc` | **Migration:** eFlows4HPC ensemble actor. | Ensemble workflow WASM | [README](typescript/apps/migrating_eflows4hpc/README.md) |
| `migrating_merlin` | **Migration:** Merlin parameter sweep. | WorkflowActor, pool, TupleSpace | [README](typescript/apps/migrating_merlin/README.md) |
| `migrating_orbit` | **Migration:** Orbit-style virtual actors for read state / Discord patterns. | Virtual actors, durability | [README](typescript/apps/migrating_orbit/README.md) |
| `migrating_orleans` | **Migration:** Orleans-style grain batch predictor. | `native/BatchPredictorGrain.cs` | [README](typescript/apps/migrating_orleans/README.md) |
| `migrating_restate` | **Migration:** Restate-style durable payment flow. | `native/restate_payment.md` | [README](typescript/apps/migrating_restate/README.md) |
| `migrating_temporal` | **Migration:** Temporal order fulfillment. | `native/temporal_order_workflow.ts` | [README](typescript/apps/migrating_temporal/README.md) |
| `migrating_v8_isolates` | **Migration:** V8 isolates–style log processing. | `native/v8_isolates_ref.md` | [README](typescript/apps/migrating_v8_isolates/README.md) |
| `streaming_pipeline` | Streaming data pipeline WASM actor. | TS SDK, streaming handlers | [README](typescript/apps/streaming_pipeline/README.md) |

```bash
cd typescript/apps/<app>
./build.sh
./test.sh
```

---

## Running many examples

From the repository root:

```bash
make test-examples
```

Native comparison or reference code for migrations often lives under each example’s `native/` directory.
