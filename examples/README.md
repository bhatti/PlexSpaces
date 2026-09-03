# PlexSpaces examples

Examples live under `examples/<language>/{apps,embedded}/<name>/`. Each directory should include a **README** that states purpose, which PlexSpaces APIs and SDK features are used, and how to build and test.

## Running Examples

### Prerequisites

```bash
# 1. Build the framework
make build

# 2. Python venv with cryptography (for ES256 JWT generation)
source ~/venv/bin/activate
pip install cryptography

# 3. Start one or two PlexSpaces nodes (auth enabled by default, ES256 key auto-generated)
./scripts/server.sh 8091        # primary node
./scripts/server.sh 8094        # optional second node (shares same JWT key)

# 4. Run all examples
./examples/test_all.sh

# Or run a single example
./examples/go/apps/web_crawl/test.sh
./examples/go/apps/web_crawl/test.sh 8094   # against second node
```

Each `test.sh` automatically generates an ES256 JWT token using the shared signing key at `./certs/jwt-es256.pem`. Both nodes share the same key so tokens work on either port.

**Framework migration:** Examples named `migrating_*` show how to model workloads that originally used another framework (Temporal, Merlin, Ray, Dapr, and so on). Treat them as migration guides, not generic tutorials.

**Parallelism, collectives, scatter/gather, and workload / ML-style demos:** For MPI-style shard-group collectives, multi-node benchmarks, and ring/allreduce-style training narratives, start with **Go `mpi_collectives`**, **Rust embedded `matrix_vector_mpi`**, **Rust apps `ring_allreduce`**, **`data_parallel_worker`**, **`parameter_server`** (Rust, Python, Go), **`batch_image_classification`**, **`genomics_pipeline`**, **`heat_diffusion`** (apps and embedded), **`data_lake_rag`**, and **Python `job_processing`** (TupleSpace scatter/gather). **Rust embedded `event_analytics`** and **`realtime_stream_processor`** illustrate shard groups and streaming analytics at scale. **`web_crawl`** (all five targets: Go, Python, Rust WASM, Rust embedded, TypeScript) demonstrates ElasticPool + TupleSpace + ShardGroup in a BFS web crawler with map-reduce word frequency, plus a **ScatterGather scaling benchmark** (1/4/8/16 workers, Amdahl's Law metrics: elapsed, coord_time, fetch_time, pages/sec, speedup, efficiency, parallel fraction). The WASM variants use `CreateShardGroup` + `ScatterGather` host calls; the embedded Rust variant uses `tokio::spawn` with stride-N URL dispatch — modeled after Ray's web-crawl and map-reduce examples.

**AI agent harness and eval:** For the full agent harness and eval pipeline, start with **`minipi`** (all five language ports: Go, Python, TypeScript, Rust WASM, Rust embedded). MiniPi demonstrates: OODA loop with token budget enforcement, SchemaValidationFacet for tool-call guardrails, EvalRunnerActor for durable scenario fan-out, RegressionDetectorActor for score-diff tracking, BenchmarkActor for harness-config comparison, AdvisorActor for two-tier LLM routing, and ApprovalGateActor (GenFSM) for human-in-the-loop. Companion blog post: [`docs/blog-agent-harness.md`](../docs/blog-agent-harness.md). For other AI/ML agent patterns see **Python `parallel_ai_inference`** and **Rust `parallel_ai_inference`** (all four parallelization mechanisms + MPI collectives + strong/weak scaling benchmarks), **Python `mcp_tool_server`** (Model Context Protocol tool calling), **Go `agentic_rag_pipeline`** (retrieve→generate→validate durable RAG), **Go `a2a_multi_agent`** (multi-agent A2A collaboration), **Go `miniclaw`** (mini agent framework with LLM routing, tool calling, agent loops), **Go `resource_aware_inference`** (cost-aware model routing with budget enforcement), **TypeScript `llm_workflow_orchestrator`** (prompt chaining, routing, reflection, LLM-as-Judge), and **TypeScript/Python `multi_agent_coordination`** (ten coordination patterns — blackboard, scatter-gather, generator-verifier, pipeline, pub-sub, voting, task delegation, veto, barrier, capability discovery — inspired by the METR/HuggingFace incident). These examples collectively demonstrate all four behavior types (GenServer, GenEvent, GenFSM, Workflow) and all key facets (virtual_actor, durability, timer, metrics).

Further reading: [Component / service design (internal)](../archived_docs/component-services-design.md), [SDK guide](../docs/sdk.md), [Polyglot WASM](../docs/polyglot.md), [Services — components, service links, ObjectRegistry](../docs/services.md#built-in-and-custom-components-facets), [Implementation roadmap](../archived_docs/component-services-roadmap.md), [WASM deployment](../docs/wasm-deployment.md), [Durability](../docs/durability.md).

---

## Go (`examples/go/apps/`)

| Example | Description | Abstractions / APIs | README |
|---------|-------------|---------------------|--------|
| `csp_structured` | **Structured concurrency:** Scatter-gather with actors + tuplespace (WASM) and native Go CSP gotchas (goroutine leak, nil channel, send-on-closed, buffered semantics, select non-determinism). `native/` has standalone errgroup fix. Blog companion example. | Go SDK `ActorRouter`, `host.TSWrite`/`TSReadAll`, `host.Spawn`/`Stop`/`SendAfter`, `context.WithTimeout`, `errgroup` | [README](go/apps/csp_structured/README.md) |
| `data_lake_rag` | Synthetic data-lake RAG-style benchmark: ingest, chunking, embedding, retrieval across leader/worker shard groups. | Go SDK `ActorRouter`, shard-group placement, application metrics | [README](go/apps/data_lake_rag/README.md) |
| `abstractions` | Unified SDK authoring contract example. | GenServer + event-actor + workflow definitions, virtual actor facets, KV, tuple space, blob, process groups, timers, spawn/stop | [README](go/apps/abstractions/README.md) |
| `chat_room` | Large-scale real-time chat: multi-actor dispatch by type, process groups, FSM, durable workflow. | `ActorRouter` (9 distinct actor types), virtual_actor, durability, timer, process groups, `WorkflowActor` (ModerationWorkflow), GenEvent (AuditEventActor), GenFSM (ConnectionFSM) | [README](go/apps/chat_room/README.md) |
| `weather_actor` | **Service links + KV cache:** Outbound HTTP via named service link (`weather-api`), KV TTL caching, all 4 host operations. | `RuntimeConfig.service_links`, `ApplicationSpec.required_service_links`, `host.HTTPFetch` | [README](go/apps/weather_actor/README.md) |
| `migrating_aws_durable_lambda` | **Migration:** AWS Lambda–style durable execution patterns (webhook-style actor). | Go SDK WASM actor, workflow/durability patterns; see `native/` | [README](go/apps/migrating_aws_durable_lambda/README.md) |
| `migrating_cadence` | **Migration:** Cadence-style payment workflow with idempotency and retries. | `WorkflowActor`, Run/Signal/Query, virtual actor + durability | [README](go/apps/migrating_cadence/README.md) |
| `migrating_cloudflare_workers` | **Migration:** Durable Objects–style guild chat with rate limiting, batch history, durable alarms. Side-by-side comparison: DO KV/alarms/virtual actors vs PlexSpaces equivalents. | Go SDK WASM, `host_kv`, `alarm_set`, `kv_multi_get/put`, `kv_increment`, `kv_cas`; compare with `native/guild_chat.js` | [README](go/apps/migrating_cloudflare_workers/README.md) |
| `alarms_api` | **Cloudflare Alarms API pattern:** `RequestQueueActor` batches incoming items and processes them 10s after first write using a durable alarm. Alarm survives node restart. | `host_kv` alarm ops, durable alarm backed by `ReminderFacet`, KV batch ops | [README](go/apps/alarms_api/README.md) |
| `chat_agent` | **Cloudflare Agents SDK pattern:** Conversation history in KV, LLM calls via service link, durable alarm for periodic summarization. | `host_kv`, `host_http`, service links, durable alarm, KV state | [README](go/apps/chat_agent/README.md) |
| `migrating_dapr` | **Migration:** Dapr-style background jobs, retries, DLQ. | `WorkflowActor`, queue/DLQ in state; `native/job_processor_dapr.go` | [README](go/apps/migrating_dapr/README.md) |
| `migrating_eflows4hpc` | **Migration:** eFlows4HPC-style ensemble workflows. | WASM ensemble actor; see `native/` | [README](go/apps/migrating_eflows4hpc/README.md) |
| `migrating_erlang_otp` | **Migration:** Erlang/OTP sliding-window rate limiter (GenServer-style). | `BaseActor`, `Host`, `ActorRouter`; `native/rate_limiter.erl` | [README](go/apps/migrating_erlang_otp/README.md) |
| `migrating_gosiris` | **Migration:** Go sIRIS-style sensor aggregation. | Go SDK WASM; `native/sensor_system.go` | [README](go/apps/migrating_gosiris/README.md) |
| `migrating_merlin` | **Migration:** Merlin-style parameter sweep (elastic pool + tuple space, process-group fallback). | `WorkflowActor`, pool checkout/checkin, TupleSpace, `work_available` | [README](go/apps/migrating_merlin/README.md) |
| `migrating_mozartspaces` | **Migration:** MozartSpaces-style auction coordination. | Go SDK WASM; `native/mozartspaces_ref.md` | [README](go/apps/migrating_mozartspaces/README.md) |
| `migrating_rivet` | **Migration:** Rivet-style matchmaking. | Go SDK WASM; `native/` reference | [README](go/apps/migrating_rivet/README.md) |
| `migrating_temporal` | **Migration:** Temporal-style order fulfillment workflow. | `WorkflowActor`, signals/queries; `native/` | [README](go/apps/migrating_temporal/README.md) |
| `mpi_collectives` | **Parallel / MPI:** Benchmark of BroadcastShardGroup, ScatterGather, ReduceShardGroup, AllReduceShardGroup, BarrierShardGroup over a dynamic shard group. | Shard-group collectives, per-node metrics, registry placement | [README](go/apps/mpi_collectives/README.md) |
| `web_crawl` | **Parallel:** BFS web crawler using ElasticPool (fetcher pool), TupleSpace (url_queue), ShardGroup pattern (scatter word counts to analyzer shards, reduce to top-N words), and **ScatterGather scaling benchmark** (1/4/8/16 workers, Amdahl's Law metrics: elapsed, coord, fetch, pages/sec, speedup, efficiency, parallel fraction). 16 fetchers in config. Imports SDK from GitHub tag — no local checkout needed. Modeled after Ray's web-crawl + map-reduce examples. | `ActorRouter`, `host.Ask`, `host.TS().Write`, `host.CreateShardGroup`, `host.ScatterGather`, ElasticPool + ShardGroup patterns | [README](go/apps/web_crawl/README.md) |
| `parameter_server` | **Workload / ML:** Synthetic distributed training coordination (leader/worker, shard groups, metrics). | `ActorRouter`, shard-group placement, application metrics | [README](go/apps/parameter_server/README.md) |

**AI Agents, Harness & Eval (Go)**

| Example | Description | Actors & Behaviors | README |
|---------|-------------|-------------------|--------|
| `minipi` | **AI / Agent Harness:** Full agent harness and eval pipeline — OODA loop, durable execution, SchemaValidationFacet, eval fan-out, regression detection, benchmark, two-tier LLM advisor, human-in-the-loop FSM. Companion to the blog post "The Other Half of Your AI Agent". WASM binary: 1.5M. Go is the reference implementation. Demonstrated metrics: 5-scenario eval at 48 scenarios/sec, 5x parallel speedup, 40% advisor escalation rate, benchmark winner varies by task complexity (conservative on simple math, aggressive on multi-step reasoning). | `AgentActor` (WorkflowActor + AgentLoop + DurabilityFacet), `LLMGatewayActor` (GenServer, Ollama + mock), `ToolRegistryActor` (GenServer + SchemaValidationFacet), `EvalRunnerActor` (Workflow + ExecutionTraceFacet), `ScenarioStoreActor` (GenServer, KV), `ScorerActor` (GenServer, heuristic + llm_judge), `TrajectoryStoreActor` (GenServer, KV + TupleSpace), `RegressionDetectorActor` (GenServer, ±5% threshold), `BenchmarkActor` (Workflow, N-config comparison), `AdvisorActor` (GenServer, cheap/expensive routing), `ApprovalGateActor` (GenFSM, idle→awaiting→idle), `DashboardActor` (GenServer, read-only aggregator) | [README](go/apps/minipi/README.md) |
| `agentic_rag_pipeline` | **AI / RAG:** Durable retrieve→generate→validate→retry RAG pipeline. `RetrieverActor` supports `single` and `deep` (multi-hop word-level) search modes. `GeneratorActor` has an in-actor circuit breaker (3 failures → open). `ValidatorActor` runs three quality checks: length, source grounding, and safety. `RAGWorkflow` checkpoints between steps and fires fire-and-forget audit events to `PipelineEventActor` after every step. | `IndexerActor` (GenServer), `RetrieverActor` (GenServer), `GeneratorActor` (GenServer), `ValidatorActor` (GenServer), `RAGWorkflow` (WorkflowActor + durability), `PipelineEventActor` (GenEvent + process group), `RAGPipelineFSM` (GenFSM, 6 states: idle/indexing/retrieving/generating/validating/error) | [README](go/apps/agentic_rag_pipeline/README.md) |
| `a2a_multi_agent` | **AI / A2A:** Multi-agent A2A collaboration with capability-based discovery. `AgentRegistryActor` maintains a dual KV index (`agent:{id}` and `cap:{cap}:{id}`) so the orchestrator can find agents by capability intersection. `OrchestratorAgent` decomposes tasks, delegates to `ResearchAgent`, `AnalysisAgent`, and `WriterAgent`, then aggregates intermediate results via TupleSpace tuples. All agents join the `agents` process group on startup. | `AgentRegistryActor` (GenServer + virtual_actor), `ResearchAgent` / `AnalysisAgent` / `WriterAgent` (GenServer), `OrchestratorAgent` (WorkflowActor + durability), `TaskEventActor` (GenEvent + process group), `AgentStateFSM` (GenFSM, 5 states: idle/assigned/working/reporting/error) | [README](go/apps/a2a_multi_agent/README.md) |
| `miniclaw` | **AI / Agent Framework:** Mini agent framework demonstrating core agentic abstractions. `LLMRouterActor` simulates LLM calls with prompt caching and circuit breaker. `ToolRegistryActor` provides tool registration and execution (calculator, weather, memory, web search). `AgentActor` implements the core agent loop (message → LLM → tool_use → execute → repeat). `OrchestratorActor` decomposes tasks and delegates to agents via process-group discovery. `MemoryActor` provides scoped memory (global/agent/session) via KV + TupleSpace. Inspired by OpenClaw/NanoClaw/MicroClaw. | `LLMRouterActor` (GenServer + circuit breaker), `ToolRegistryActor` (GenServer), `AgentActor` (GenServer + agent loop), `SessionManagerActor` (GenServer), `OrchestratorActor` (WorkflowActor + durability), `MemoryActor` (GenServer), `AuditEventActor` (GenEvent), `AgentStateFSM` (GenFSM, 5 states: idle/processing/tool_executing/responding/error) | [README](go/apps/miniclaw/README.md) |
| `resource_aware_inference` | **AI / Routing:** Cost-aware model tier selection with per-tenant USD budget enforcement. `RoutingWorkflow` runs: budget check → complexity estimate → model select → infer → deduct cost. `ModelRegistryActor` seeds three tiers (gpt-nano/gpt-base/gpt-large) and picks by complexity + remaining budget + GPU preference. `BudgetManagerActor` tracks spend per tenant in KV. `BudgetFSM` transitions active→warning→throttled→exhausted based on spend percentage. | `ModelRegistryActor` (GenServer + virtual_actor), `InferenceActor` ×3 tiers (GenServer + virtual_actor), `BudgetManagerActor` (GenServer + durability), `RoutingWorkflow` (WorkflowActor + durability), `InferenceEventActor` (GenEvent + process group), `BudgetFSM` (GenFSM, 4 states: active/warning/throttled/exhausted) | [README](go/apps/resource_aware_inference/README.md) |

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
| `abstractions` | Unified SDK authoring contract example. | `@gen_server_actor`, `@event_actor`, `@workflow_actor`, workflow handler decorators, host send / KV / tuple space / blob / process groups / timer / spawn | [README](python/apps/abstractions/README.md) |
| `bank_account` | Durable bank account state with deposit/withdraw/history. | `@actor`, `state()`, `@handler`, WASM durable state | [README](python/apps/bank_account/README.md) |
| `calculator` | Simple calculator operations and batch throughput. | GenServer-style handlers, WASM | [README](python/apps/calculator/README.md) |
| `weather_actor` | **Service links + KV cache:** Outbound HTTP via named service link, TTL caching, 5 contract tests. | `ServiceHttpClient`, `host.http_fetch`, `host.kv_*`, `app-config.toml` required links | [README](python/apps/weather_actor/README.md) |
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
| `migrating_cloudflare_workers` | **Migration:** Cloudflare DO–style guild chat with rate limiting, batch history, durable alarms (Python). | Python SDK WASM, `host.kv`, `host.alarm`, `get_actor_ref`; compare with `native/guild_chat.js` | [README](python/apps/migrating_cloudflare_workers/README.md) |
| `alarms_api` | **Cloudflare Alarms API pattern (Python):** `RequestQueueActor` with durable alarm batch processing. | `host.alarm.set/get/delete`, `host.kv`, Python decorator handlers | [README](python/apps/alarms_api/README.md) |
| `chat_agent` | **Cloudflare Agents SDK pattern (Python):** Conversation state in KV, LLM via service link, alarm for summarization. | `host.kv`, `host.http_client`, durable alarm, Python decorators | [README](python/apps/chat_agent/README.md) |
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
| `web_crawl` | **Parallel:** BFS web crawler WASM component. ElasticPool pattern (16 fetchers), TupleSpace url_queue, ShardGroup pattern (2 analyzer shards, scatter-gather reduce), and **ScatterGather scaling benchmark** (1/4/8/16 workers, Amdahl's Law metrics). Fetcher shards process interleaved URL slices via `pool_slot`. Installs SDK from PyPI — no local checkout needed. Modeled after Ray's web-crawl + map-reduce examples. | `@gen_server_actor`, `@handler`, `host.ask`, `host.create_shard_group`, `host.scatter_gather`, `host.tuplespace.write`, `ACTOR_ROLES` multi-role dispatch | [README](python/apps/web_crawl/README.md) |

**AI Agents, Harness & Eval (Python)**

| Example | Description | Actors & Behaviors | README |
|---------|-------------|-------------------|--------|
| `minipi` | **AI / Agent Harness:** Python WASM port of the full MiniPi harness and eval pipeline. WASM binary: 47M. Object registry integration for multi-node service discovery (falls back to same-node peer ID). Most readable actor code — all 12 actors in separate `.py` files with `@workflow_actor`, `@actor`, `@handler`, `@init_handler` decorators. Demonstrated metrics: 5-scenario eval (avg 0.76), 40% advisor escalation rate, 4 eval runs tracked in dashboard (scores 0.70–0.88). See also: companion blog post. | `AgentActor` + `LLMGatewayActor` + `ToolRegistryActor` + `EvalRunnerActor` + `ScenarioStoreActor` + `ScorerActor` + `TrajectoryStoreActor` + `RegressionDetectorActor` + `BenchmarkActor` + `AdvisorActor` + `ApprovalGateActor` + `DashboardActor` — same 12 actors as Go, implemented with Python SDK decorators | [README](python/apps/minipi/README.md) |
| `parallel_ai_inference` | **AI / Scaling:** All four parallelization mechanisms side-by-side in one benchmark. `BenchmarkActor` runs: (1) shard-group scatter-gather **strong scaling** (fixed total work ÷ shards) and **weak scaling** (fixed work per shard) across configurable shard counts with batch_size, Gran, Eff%, Comp%, p50/p95/p99 metrics; (2) elastic pool `pool_checkout` / `pool_checkin`; (3) MPI collectives pipeline — `broadcast_shard_group` → `barrier_shard_group` → `scatter_gather` → `reduce_shard_group` → `all_reduce_shard_group`. `OrchestratorWorkflow` exposes all three modes via `workflow_run` with `signal` (scale) and `query` (status). | `InferenceWorkerActor` (GenServer + virtual_actor + durability + metrics), `BenchmarkActor` (GenServer + virtual_actor), `OrchestratorWorkflow` (WorkflowActor + durability + timer), `MetricsEventActor` (GenEvent + process group `metrics-events`), `WorkerCircuitBreakerFSM` (GenFSM, 3 states: closed/open/half_open + timer) | [README](python/apps/parallel_ai_inference/README.md) |
| `mcp_tool_server` | **AI / MCP:** Model Context Protocol tool server with JSON-RPC 2.0 gateway. `ToolRegistryActor` maintains a catalogue of tools with JSON Schema descriptors, validates required fields, routes `tools/call` to specialist actors, and fires fire-and-forget audit events to `ToolAuditEventActor` after each invocation. `MCPGatewayWorkflow` handles the full JSON-RPC 2.0 session lifecycle and validates `tenant` namespace on every request. `ToolCircuitBreakerFSM` tracks health of tool execution. Supports dynamic tool registration at runtime without redeployment. | `ToolRegistryActor` (GenServer + virtual_actor + durability + metrics), `CalculatorToolActor` / `SearchToolActor` / `WeatherToolActor` (GenServer + virtual_actor), `MCPGatewayWorkflow` (WorkflowActor + durability), `ToolAuditEventActor` (GenEvent + process group `tool-audit`), `ToolCircuitBreakerFSM` (GenFSM, 3 states: healthy/degraded/circuit_open + timer) | [README](python/apps/mcp_tool_server/README.md) |
| `multi_agent_coordination` | **AI / Multi-Agent Coordination:** Ten coordination patterns for multi-agent AI systems — blackboard (TupleSpace Linda in/rd/out), scatter-gather (shard groups), generator-verifier, pipeline, pub-sub (process groups), consensus/voting, dynamic task delegation, veto protocol, two-phase commit/barrier, capability discovery. 8 actors demonstrate a security vulnerability analysis scenario. Inspired by METR/HuggingFace incident (Aug 2026) where ~1,200 agents self-organized via shared state. Companion blog: [`archived_docs/blog-multi-agent-coordination.md`](../archived_docs/blog-multi-agent-coordination.md). | `CoordinatorWorkflow` (WorkflowActor + durability), `ResearchAgent` / `AnalysisAgent` / `VerifierAgent` / `SynthesizerAgent` / `BenchmarkAgent` (GenServer + virtual_actor), `AuditEventActor` (GenEvent), `CoordinationFSM` (GenFSM, 9 states: idle/decomposing/researching/analyzing/verifying/voting/synthesizing/complete/failed) | [README](python/apps/multi_agent_coordination/README.md) |

```bash
cd python/apps/<app>
./build.sh    # or project-specific build
./test.sh <http_port>
```

---

## Rust — WASM apps (`examples/rust/apps/`)

| Example | Description | Abstractions / APIs | README |
|---------|-------------|---------------------|--------|
| `minipi` | **AI / Agent Harness:** Rust WASM port of the full MiniPi harness and eval pipeline. WASM binary: 6.3M. All 12 actors implemented with Rust SDK macros (`#[gen_server_actor(wasm)]`, `#[workflow_actor(wasm)]`, `#[fsm_actor(wasm)]`) in a single `lib.rs` with role-based dispatch. The most complete benchmark implementation — BenchmarkActor returns real scores showing aggressive config (8192 tokens, 20 iterations) outperforming conservative (1024 tokens, 3 iterations) on multi-step tasks: 0.83 vs 0.73. AdvisorActor fully wired with 60% escalation rate and 57% advisor token share on complex prompts. | All 12 MiniPi roles in one Rust WASM binary, `#[gen_server_actor(wasm)]`, `#[workflow_actor(wasm)]`, `#[fsm_actor(wasm)]` macros, role-based dispatch via `ACTOR_ROLES` | [README](rust/apps/minipi/README.md) |
| `parallel_ai_inference` | **AI / Scaling:** Rust implementation of all four parallelization mechanisms. `BenchmarkActor` runs strong-scaling and weak-scaling suites (shard-group scatter-gather with batching, configurable actors/data/compute intensity), MPI collectives pipeline, and orchestrator workflow with signal/query. Includes `run_scaling_benchmark` (fixed total work ÷ shards) and `run_weak_scaling_benchmark` (fixed work per shard). Reports Gran, Eff%, Comp%, p50/p95/p99. | `InferenceWorkerActor` (GenServer, wasm), `BenchmarkActor` (GenServer, wasm), `OrchestratorActor` (GenServer, wasm) | [README](rust/apps/parallel_ai_inference/README.md) |
| `batch_image_classification` | **Parallel / ML:** Batch image classification fan-out (synthetic workload), shard-group scatter/gather, metrics. | `#[gen_server_actor(wasm)]`, shard groups, application metrics | [README](rust/apps/batch_image_classification/README.md) |
| `abstractions` | Unified SDK authoring and deployable WASM example. | GenServer + Workflow + GenEvent child specs, virtual actor reactivation, timer/reminder, KV / tuple space / blob / process groups | [README](rust/apps/abstractions/README.md) |
| `calculator` | Four-function calculator WASM actor with batch throughput. | `#[gen_server_actor(wasm)]`, `#[plexspaces_handlers(wasm)]`, WIT Guest | [README](rust/apps/calculator/README.md) |
| `weather_actor` | **Service links + KV cache:** Outbound HTTP via named service link, TTL caching, 5 contract tests. | `ServiceHttpClient`, `http_fetch` host function, `host.kv_*`, `app-config.toml` required links | [README](rust/apps/weather_actor/README.md) |
| `data_parallel_worker` | **Parallel:** Shard group rounds with scatter/gather over workers (SDK + WIT `scatter_gather`). | WASM GenServer, shard group, `host::scatter_gather` | [README](rust/apps/data_parallel_worker/README.md) |
| `entity_recognition` | Doc → entity extraction heuristics (EMAIL/URL/TOKEN); batch ops. | WASM SDK annotations, `application_metrics_add` | [README](rust/apps/entity_recognition/README.md) |
| `genomics_pipeline` | **Workload / ML:** Genomics pipeline leader/worker stages with shard-group coordination. | WASM leader/worker, genomics narrative, metrics | [README](rust/apps/genomics_pipeline/README.md) |
| `heat_diffusion` | **Parallel:** Thermal diffusion leader/worker (synthetic PDE-style workload). | WASM shard groups, diffusion steps, metrics | [README](rust/apps/heat_diffusion/README.md) |
| `migrating_eflows4hpc` | **Migration:** eFlows4HPC ensemble (Rust WASM). | Workflow/WASM patterns; `native/` | [README](rust/apps/migrating_eflows4hpc/README.md) |
| `migrating_merlin` | **Migration:** Merlin parameter sweep (Rust WASM). | Pool + TupleSpace + workflow handlers | [README](rust/apps/migrating_merlin/README.md) |
| `migrating_skypilot` | **Migration:** SkyPilot-style workload (Rust WASM). | Job/worker patterns; `native/` | [README](rust/apps/migrating_skypilot/README.md) |
| `migrating_cloudflare_workers` | **Migration:** Cloudflare DO–style guild chat (Rust WASM). Rate limiting with `kv_increment`/`kv_cas`, batch history with `kv_multi_get/put`, durable alarms. | `plexspaces::actor::host_kv`, `plexspaces::actor::host_actor`, `plexspaces_sdk::simple_actor`; compare with `native/guild_chat.js` | [README](rust/apps/migrating_cloudflare_workers/README.md) |
| `alarms_api` | **Cloudflare Alarms API pattern (Rust WASM):** `RequestQueueActor` with durable alarm. Alarm backed by `ReminderFacet`, survives restart. | `plexspaces::actor::host_kv::{alarm_set, alarm_get, alarm_delete}`, Rust WIT guest | [README](rust/apps/alarms_api/README.md) |
| `chat_agent` | **Cloudflare Agents SDK pattern (Rust WASM):** Conversation state in KV, LLM via `http_fetch`, alarm for summarization. | `plexspaces::actor::host_kv`, `plexspaces::actor::host_http`, `plexspaces_sdk::simple_actor` | [README](rust/apps/chat_agent/README.md) |
| `migrating_temporal` | **Migration:** Temporal order workflow (Rust WASM). | Workflow; `native/` | [README](rust/apps/migrating_temporal/README.md) |
| `nbody` | **Workload / ML:** Gravitational N-body on one WASM GenServer (`step`, `run_steps`). | `#[gen_server_actor(wasm)]`, metrics | [README](rust/apps/nbody/README.md) |
| `parameter_server` | **Workload / ML:** Parameter server benchmark (Rust WASM leader/worker). | Shard groups, scatter/gather, metrics | [README](rust/apps/parameter_server/README.md) |
| `ring_allreduce` | **Parallel / ML:** Ring all-reduce–style gradient aggregation narrative. | Leader/worker WASM, collective-style rounds | [README](rust/apps/ring_allreduce/README.md) |
| `chat_room` | Large-scale real-time chat: multi-actor dispatch by type, process groups, FSM, durable workflow. Object Registry for FanoutActor and AuditEventActor. | `#[gen_server_actor(wasm)]`, 9 actor types, virtual_actor, durability, timer, process groups, WorkflowActor (ModerationWorkflow), GenEvent (AuditEventActor), GenFSM (ConnectionFSM) | [README](rust/apps/chat_room/README.md) |
| `session_manager` | Idle timeout and heartbeat using WASM timer host calls. | `#[gen_server_actor(wasm)]`, `send_after` / touch | [README](rust/apps/session_manager/README.md) |
| `web_crawl` | **Parallel:** BFS web crawler WASM component. ElasticPool pattern (16 fetchers), TupleSpace url_queue, ShardGroup pattern (2 analyzer shards, scatter-gather reduce), and **ScatterGather scaling benchmark** (1/4/8/16 workers, Amdahl's Law metrics). Uses protobuf-encoded `host::create_shard_group` + `host::scatter_gather` WIT calls. Imports SDK via git tag — no local checkout needed. Modeled after Ray's web-crawl + map-reduce examples. | WIT guest, `host::ask`, `host::ts_write`, `host::create_shard_group`, `host::scatter_gather`, protobuf encoding, native contract tests | [README](rust/apps/web_crawl/README.md) |
| `actor_csp` | **Structured concurrency:** Scatter-gather with supervised actors + Linda tuplespace coordination. Orchestrator spawns N workers, collects first K results via tuplespace, stops stragglers. Linda-style wrappers (out/in/rd) over host tuplespace. Blog companion example. | `#[gen_server_actor(wasm)]`, `ts_write`/`ts_take`/`ts_read` (Linda), `spawn`, `send_after`, `stop`, supervisor one_for_one | [README](rust/apps/actor_csp/README.md) |
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
| `minipi` | **AI / Agent Harness:** Rust embedded port of the full MiniPi harness and eval pipeline. Runs entirely in-process — no WASM compilation, no HTTP gateway. Pure Rust SDK with `#[gen_server_actor]`, `#[workflow_actor]`, `#[fsm_actor]` macros, `spawn_with_facets`, and `spawn_with_storage`. All 12 actors in `src/` — the reference implementation for understanding PlexSpaces internals. Test runs in milliseconds. Validates: node started, 12 actors spawned, scenarios seeded, OODA completed, regression detected, benchmark passed. | `#[gen_server_actor]`, `#[workflow_actor]`, `#[fsm_actor]`, `spawn_with_facets`, `spawn_with_storage`, `RequestContext`, supervision trees, durable workflows — full SDK surface area without WASM | [README](rust/embedded/minipi/README.md) |
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
| `csp_channels` | **Structured concurrency / CSP:** Pure Rust scatter-gather in three approaches — naive (task leak), JoinSet+timeout (structured), CSP-style Nursery+select! (Hoare guarded commands). Minimal CSP primitives: rendezvous Channel, Nursery (structured lifetime), scoped(). Blog companion example. | `tokio::task::JoinSet`, `tokio::select!`, `mpsc::channel`, custom `Nursery<T>` | [README](rust/embedded/csp_channels/README.md) |
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
| `web_crawl` | **Parallel:** Single-binary BFS web crawler: Node + ElasticPool (PageFetcher pool), TupleSpace url_queue, ShardGroup (2 LinkAnalyzer shards, scatter-gather reduce word counts), and **native scaling benchmark** using `tokio::spawn` (1/4/8/16 workers, stride-N URL dispatch, Amdahl's Law metrics). All in one `main()`. Imports SDK via git tag — no local checkout needed. Modeled after Ray's web-crawl + map-reduce examples. | `#[gen_server_actor]`, `spawn()`, `GenServerRef`, NodeBuilder, `RequestContext`, `tokio::spawn` parallel benchmark | [README](rust/embedded/web_crawl/README.md) |

```bash
cd rust/embedded/<example>
cargo run
# or ./test.sh where provided
```

---

## TypeScript (`examples/typescript/apps/`)

| Example | Description | Abstractions / APIs | README |
|---------|-------------|---------------------|--------|
| `abstractions` | Unified SDK authoring contract example. | decorators, GenServer + event-actor + workflow definitions, virtual actor facets, KV / tuple space / blob / process groups / timer / spawn contract | [README](typescript/apps/abstractions/README.md) |
| `chat_room` | Large-scale real-time chat: multi-actor dispatch by type, process groups, FSM, durable workflow. | `ActorRouter` (9 distinct actor types), `PlexSpacesActor`, `WorkflowActor`, virtual_actor, durability, timer, process groups, GenEvent (AuditEventActor), GenFSM (ConnectionFSM) | [README](typescript/apps/chat_room/README.md) |
| `bank_account` | Durable bank account (same conceptual API as Python). | `PlexSpacesActor`, jco componentize, WIT world | [README](typescript/apps/bank_account/README.md) |
| `weather_actor` | **Service links + KV cache:** Outbound HTTP via named service link, TTL caching, 7 contract tests. | `ServiceHttpClient`, `host.httpFetch`, `host.kvGet/kvPut`, `host.nowMs()` for TTL | [README](typescript/apps/weather_actor/README.md) |
| `migrating_azure_durable_functions` | **Migration:** Azure Durable Functions–style document processing. | TS SDK workflow patterns; `native/` | [README](typescript/apps/migrating_azure_durable_functions/README.md) |
| `migrating_cloudflare_workers` | **Migration:** Cloudflare DO–style guild chat (TypeScript WASM). Rate limiting, batch history, durable alarms, virtual actors. | TS WASM actor, `host.kvMultiGet/Put`, `host.kvIncrement`, `host.kvCas`, `host.alarmSet`, `getActorRef`; compare with `native/guild_chat.js` | [README](typescript/apps/migrating_cloudflare_workers/README.md) |
| `alarms_api` | **Cloudflare Alarms API pattern (TypeScript):** `RequestQueueActor` with durable alarm batch processing. | `host.alarmSet/Get/Delete`, `host.kvGet/Put/Delete`, TS WASM actor | [README](typescript/apps/alarms_api/README.md) |
| `chat_agent` | **Cloudflare Agents SDK pattern (TypeScript):** Conversation state in KV, LLM via service link, alarm for summarization. | `host.kvGet/Put`, `host.httpClient`, `host.alarmSet`, TS WASM actor | [README](typescript/apps/chat_agent/README.md) |
| `migrating_dirigo` | **Migration:** Dirigo-style stream processing. | `native/dirigo_ref.md` | [README](typescript/apps/migrating_dirigo/README.md) |
| `migrating_eflows4hpc` | **Migration:** eFlows4HPC ensemble actor. | Ensemble workflow WASM | [README](typescript/apps/migrating_eflows4hpc/README.md) |
| `migrating_merlin` | **Migration:** Merlin parameter sweep. | WorkflowActor, pool, TupleSpace | [README](typescript/apps/migrating_merlin/README.md) |
| `migrating_orbit` | **Migration:** Orbit-style virtual actors for read state / Discord patterns. | Virtual actors, durability | [README](typescript/apps/migrating_orbit/README.md) |
| `migrating_orleans` | **Migration:** Orleans-style grain batch predictor. | `native/BatchPredictorGrain.cs` | [README](typescript/apps/migrating_orleans/README.md) |
| `migrating_restate` | **Migration:** Restate-style durable payment flow. | `native/restate_payment.md` | [README](typescript/apps/migrating_restate/README.md) |
| `migrating_temporal` | **Migration:** Temporal order fulfillment. | `native/temporal_order_workflow.ts` | [README](typescript/apps/migrating_temporal/README.md) |
| `migrating_v8_isolates` | **Migration:** V8 isolates–style log processing. | `native/v8_isolates_ref.md` | [README](typescript/apps/migrating_v8_isolates/README.md) |
| `streaming_pipeline` | Streaming data pipeline WASM actor. | TS SDK, streaming handlers | [README](typescript/apps/streaming_pipeline/README.md) |
| `web_crawl` | **Parallel:** BFS web crawler WASM component. ElasticPool pattern (16 fetchers), TupleSpace url_queue, ShardGroup pattern (2 analyzer shards, scatter-gather reduce), and **ScatterGather scaling benchmark** (1/4/8/16 workers, Amdahl's Law metrics: elapsed, coord, fetch, pages/sec, speedup, efficiency, parallel fraction). Fetcher shards process interleaved URL slices via `pool_slot`. Installs SDK from npm — no local checkout needed. Modeled after Ray's web-crawl + map-reduce examples. | `PlexSpacesActor`, `ActorRouter`, `host.ask`, `host.createShardGroup`, `host.scatterGather`, `host.tuplespace.write`, async crawl loop | [README](typescript/apps/web_crawl/README.md) |
| `ws_chat_room` | **WebSocket thin nodes:** Real-time chat where browser tabs connect as thin nodes via binary `WsFrame` protobuf over WebSocket. `ChatRoomActor` (WASM) manages PG membership and broadcasts `chat_message` events back to thin-node sessions via `WsActorTransportClient`. Includes **end-to-end latency benchmark** (avg, p50, p95 ms). | `WsThinClient`, binary `WsFrame` wire encoding, `NODE_ROLE_THIN` registration, `host.processGroups.broadcast()`, `PresenceActor` with `host.sendAfter` reminder | [README](typescript/apps/ws_chat_room/README.md) |
| `mersenne_prime` | **WebSocket thin nodes + distributed compute:** SETI@home-style Mersenne prime search. Browser tabs connect as thin-node workers; each runs Lucas-Lehmer (BigInt) in a **Web Worker Blob URL**. `CoordinatorActor` (WASM) manages work queue with resource-aware scheduling (`cpu_cores` hint), writes results to TupleSpace, increments Prometheus counters. Includes **parallel scaling benchmark** (1 vs 2 workers, speedup, efficiency%). | `WsThinClient`, `NodeRegistration.capabilities`, `PingResponse` resource fields (cpu_percent/memory_available_mb/available_cores), `host.send()`, `host.ts.write()`, `host.incrCounter()` | [README](typescript/apps/mersenne_prime/README.md) |

**AI Agents, Harness & Eval (TypeScript)**

| Example | Description | Actors & Behaviors | README |
|---------|-------------|-------------------|--------|
| `minipi` | **AI / Agent Harness:** TypeScript WASM port of the full MiniPi harness and eval pipeline. WASM binary: 13M. All 11 actors in a single `minipi_actors.ts` file. The only implementation that tracks real token counts and estimated LLM cost per scenario (311 input / 223 output tokens per 5-scenario run, ~$0.00018 at mock rates). Demonstrated metrics: avg eval score 0.818, sc-math-01 and sc-search-01 scoring 0.92. | All 11 MiniPi actors in one TypeScript file using `PlexSpacesActor` class hierarchy and `@handler` decorators | [README](typescript/apps/minipi/README.md) |
| `llm_workflow_orchestrator` | **AI / LLM:** Five foundational agentic LLM patterns in one example. `RouterActor` classifies input (keyword + length heuristics) and dispatches to four specialist pipelines. `ChainActor` executes multi-step transforms (`summarize → extract_keywords → format_output`) and Evol-Instruct prompt mutation (prefix injection, suffix appending, synonym substitution). `JudgeActor` scores output on relevance, completeness, and clarity (0–10 composite score). `OrchestratorWorkflow` runs a reflection loop: chain → judge → refine until score threshold or max iterations; writes final result to TupleSpace; accepts `feedback` and `reset` signals; answers `progress` and `history` queries. `QualityFSMActor` tracks per-task approval state. `PipelineAuditActor` receives fire-and-forget step events and emits Prometheus metrics. | `RouterActor` (GenServer + virtual_actor), `ChainActor` (GenServer + virtual_actor + durability), `JudgeActor` (GenServer + virtual_actor + durability), `OrchestratorWorkflow` (WorkflowActor + durability + TupleSpace), `PipelineAuditActor` (GenEvent), `QualityFSMActor` (GenFSM, 5 states: pending/evaluating/approved/rejected/escalated + durability) | [README](typescript/apps/llm_workflow_orchestrator/README.md) |
| `multi_agent_coordination` | **AI / Multi-Agent Coordination:** Ten coordination patterns — blackboard (TupleSpace Linda in/rd/out), scatter-gather (shard groups), generator-verifier, pipeline, pub-sub (process groups), consensus/voting, dynamic task delegation, veto protocol, two-phase commit/barrier, capability discovery. 8 actors in a security vulnerability analysis scenario. Inspired by METR/HuggingFace incident (Aug 2026) where ~1,200 agents self-organized via shared state and invented HOLD/VETO/STOP/owner protocols. Companion blog: [`archived_docs/blog-multi-agent-coordination.md`](../archived_docs/blog-multi-agent-coordination.md). | `CoordinatorWorkflow` (WorkflowActor + durability), `ResearchAgent` / `AnalysisAgent` / `VerifierAgent` / `SynthesizerAgent` / `BenchmarkAgent` (GenServer + virtual_actor), `AuditEventAgent` (GenEvent), `CoordinationFSM` (GenFSM, 9 states: idle/decomposing/researching/analyzing/verifying/voting/synthesizing/complete/failed) | [README](typescript/apps/multi_agent_coordination/README.md) |

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
