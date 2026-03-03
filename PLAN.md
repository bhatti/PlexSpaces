# PlexSpaces Migration Examples & Abstraction Showcase Plan

## Previous Steps 1-11 Status

| # | Step | Status | Notes |
|---|------|--------|-------|
| 1 | Re-instantiation test | DONE | reinstantiation_tests.rs |
| 2 | State preservation fix | DONE | get_state/set_state cycle in instance.rs |
| 3 | Deduplicate ComponentContext | PARTIAL | Deferred - not blocking |
| 4 | WIT extensions to simple-actor | DONE | 31 host functions in world.wit |
| 5 | Host bindings implementation | DONE | simple_component_host.rs |
| 6 | Python SDK parity | DONE | host.py with ask, spawn, self_id, etc. |
| 7 | TypeScript SDK host wrappers | DONE | host.ts Host class |
| 8 | TypeScript behavior classes | NOT DONE | Missing GenServerActor, EventActor, FsmActor |
| 9 | Polyglot examples | PARTIAL | Rust only, Python/TS/Go ports needed |
| 10 | Context in messaging WIT | DEFERRED | Breaking change, deferred by design |
| 11 | Go SDK | DONE | actor.go, host.go with full API |

### Remaining Framework Work

- [ ] **Step 3**: Deduplicate ComponentContext creation in instance.rs
- [ ] **Step 8**: Add TypeScript behavior classes (GenServerActor, EventActor, FsmActor)
- [x] **Multi-actor ApplicationSpec**: Pass child_spec.id + args as init config (DONE - 0e6b537)
- [x] **Python SDK multi-actor**: Support multiple @actor classes per WASM module (DONE - 0e6b537)

---

## Vision: Complete Abstraction Showcase

Every PlexSpaces abstraction must have at least one compelling, real-world example.
Goal: **faster, simpler, polyglot** — compete with Ray, Temporal, Akka, Erlang/OTP, Orleans.

### Abstraction Coverage Matrix

| Abstraction | Facet/Behavior | Current Coverage | Target Example(s) |
|-------------|---------------|-----------------|-------------------|
| **GenServer** | Behavior | Extensive (30+) | Already covered |
| **GenEvent** | Behavior | Good (5+) | Already covered |
| **GenStateMachine** | Behavior | Limited (1 Python) | Need Rust/TS example |
| **Workflow** | Behavior | Good (4+) | Add AI pipeline workflow |
| **VirtualActor** | VirtualActorFacet | Limited (3) | **Discord guild/session** |
| **VirtualActor + Durability** | VirtualActorFacet + DurabilityFacet | NONE | **Discord read states** |
| **Timer** | TimerFacet | Limited (1) | Already covered (timers/) |
| **Reminder** | ReminderFacet | Limited (1) | Already covered (reminders/) |
| **Timer + Reminder** | Both | Limited (1) | Already covered (reminders/) |
| **Durability** | DurabilityFacet | Extensive (10+) | Already covered |
| **EventSourcing** | EventSourcingFacet | Limited (2) | Need standalone example |
| **ProcessGroup** | ProcessGroupFacet | Good (3+) | Already covered |
| **TupleSpace** | TupleSpace API | Good (6+) | Already covered |
| **Lock** | LockFacet | **NONE** | **Need: distributed reservation** |
| **Registry** | RegistryFacet | Limited (1) | Need: service discovery |
| **HttpClient** | HttpClientFacet | **NONE** | **Need: API gateway** |
| **KeyValue** | KeyValueFacet | Limited | Need: session store |
| **Channel** | ChannelService | Limited | Need: streaming pipeline |

---

## Mandatory Criteria for EVERY Example

1. **Correct PlexSpaces APIs** - SDK decorators/macros, no manual trait impls
2. **Real-World Use Case** - Domain-specific, not abstract API demos
3. **Simple Code** - Single file preferred, minimal dependencies
4. **Clear Output** - Show what's happening with progress and results
5. **README.md** - Purpose, APIs used, quick start, comparison table (one per example, no other .md)
6. **Metrics & Benchmarks (MANDATORY)**:
   - Coordination vs computation latency breakdown
   - Cost analysis (% time on coordination vs computation)
   - Granularity ratio (compute/coordinate >= 10x)
   - Benchmark metrics (GFLOPS, throughput MB/s, ops/sec, estimated speedup)
   - Non-trivial data sizes (runs for 2+ seconds)
   - Prominently displayed with section headers
7. **Consistent Shell Scripts** - `build.sh` and `test.sh` in example base dir (not scripts/)
8. **No Dead/Legacy Code** - Clean, production-quality
9. **Polyglot** - Match native language of original framework where possible
10. **WASM apps** tested via: `./scripts/server.sh` (start node) then `./test.sh` (deploy + run)
11. **Embedded apps** use `.cargo/config.toml` with shared target dir, debug builds

### Example Types

- **App** (WASM): Python/TypeScript/Go actors deployed to running node via HTTP API
- **Embedded**: Rust standalone binary with NodeBuilder (for Rust-native patterns only)

---

## Phase 1: Fix migrating_ray Python Example (DONE)

- [x] Framework: pass child_spec.id as init config to WASM actors
- [x] Python SDK: support multiple @actor classes per WASM module
- [x] Create ray_actors.py with ParameterServer + DataWorker (inter-actor host.ask)
- [x] Create build.sh and test.sh
- [x] Test end-to-end: server.sh -> build.sh -> test.sh
- [ ] Remove examples/rust/embedded/migrating_ray (deferred - keep for reference)

---

## Phase 2: Migration Examples (Convert Rust Embedded -> WASM Apps)

All `examples/rust/embedded/migrating_*` will be converted to Python, TypeScript, or Go WASM apps
based on the native framework's language. Work one at a time with full verification.

### Group A: GenServer + Basic Patterns

| # | Example | Target Lang | Real-World Use Case | Key Abstractions |
|---|---------|------------|---------------------|-----------------|
| 1 | migrating_ray | Python | **Distributed ML training** - parameter server with gradient aggregation (Ray Train style) | GenServer, host.ask, multi-actor ApplicationSpec |
| 2 | migrating_erlang_otp | Go | **Rate limiter service** - sliding window with distributed counters (API gateway) | GenServer, state, handler |
| 3 | migrating_gosiris | Go | **IoT sensor aggregation** - temperature/humidity from 1000s of sensors | GenServer, process groups |
| 4 | migrating_ractor | Python | **Matrix computation service** - parallel matrix multiply with work distribution | GenServer, spawn, parallel compute |
| 5 | migrating_wasmcloud | Python | **API gateway with rate limiting** - HTTP proxy with per-tenant rate limits | GenServer, KV store, timer |

### Group B: Virtual Actors (Orleans/Cloudflare/Discord Patterns)

| # | Example | Target Lang | Real-World Use Case | Key Abstractions |
|---|---------|------------|---------------------|-----------------|
| 6 | migrating_cloudflare_workers | TypeScript | **Discord-style guild manager** - virtual actors for chat rooms with message fan-out (5M concurrent) | **VirtualActor**, GenServer, process groups |
| 7 | migrating_orbit | TypeScript | **Discord-style read states** - per-user message read tracking with durable virtual actors | **VirtualActor + Durability**, GenServer, KV |
| 8 | migrating_rivet | Go | **Game matchmaking** - virtual actor per game room, auto-activate/deactivate | **VirtualActor**, GenServer, timer |

### Group C: Workflow + Durability (Temporal/Step Functions Patterns)

| # | Example | Target Lang | Real-World Use Case | Key Abstractions |
|---|---------|------------|---------------------|-----------------|
| 9 | migrating_temporal | TypeScript ✅ / Go / Python / Rust | **E-commerce order fulfillment** - multi-step with compensation/saga | **Workflow**, signals, queries, durability |
| 10 | migrating_cadence | Go | **Payment processing with retries** - idempotent payments, retry policies | **Workflow**, durability, retry |
| 11 | migrating_aws_step_functions | Python | **AI/ML pipeline** - data prep -> training -> evaluation -> deploy (Ray Train-style) | **Workflow**, fan-out/fan-in |
| 12 | migrating_azure_durable_functions | TypeScript | **Document processing** - OCR -> classify -> extract -> store (fan-out/fan-in) | **Workflow**, parallel activities |
| 13 | migrating_conductor | Python | **Microservice saga** - order -> payment -> inventory -> shipping with compensation | **Workflow**, multi-step coordination |
| 14 | migrating_dapr | Go | **Background job processor** - durable queue with retries, dead letter, scheduling | **Workflow** + GenServer, KV store |
| 15 | migrating_restate | TypeScript | **Exactly-once payment** - idempotent processing with journaling replay | **Durability** + EventSourcing, GenServer |
| 16 | migrating_zeebe | Python | **Insurance claim workflow** - BPMN-style with human tasks, escalation, SLA timers | **Workflow**, event sourcing, timer |

### Group D: AI/ML & HPC (Ray/SkyPilot/HPC Patterns)

| # | Example | Target Lang | Real-World Use Case | Key Abstractions |
|---|---------|------------|---------------------|-----------------|
| 17 | migrating_kueue | Python | **GPU job scheduler** - AI training job queue with priority, preemption, resource quotas | GenServer, KV, channel, **lock** |
| 18 | migrating_skypilot | Python | **Multi-cloud ML orchestration** - spot instance management, checkpointing, failover | **Workflow**, GenServer, KV |
| 19 | migrating_eflows4hpc | Python | **HPC ensemble simulation** - Monte Carlo + analytics + ML (fan-out 100s of workers) | **Workflow**, tuple space, process groups |
| 20 | migrating_merlin | Python | **Scientific simulation manager** - parameter sweep with adaptive sampling | **Workflow**, parallel compute |

### Group E: Coordination & Streaming (Tuple Space, Events, Data Lake)

| # | Example | Target Lang | Real-World Use Case | Key Abstractions |
|---|---------|------------|---------------------|-----------------|
| 21 | migrating_mozartspaces | Go | **Distributed auction** - real-time bidding with coordination (XVSM) | **TupleSpace**, process groups, lock |
| 22 | migrating_luats | Python | **Event-driven data pipeline** - CDC events with Linda-style coordination | **TupleSpace**, events, timer |
| 23 | migrating_dirigo | TypeScript | **Real-time analytics stream** - Kafka-style windowed aggregation (clickstream) | **GenEvent**, channels, windowing |
| 24 | migrating_v8_isolates | TypeScript | **High-throughput log processor** - 100K events/sec with batching and routing | GenServer, channels, batch processing |
| 25 | migrating_aws_durable_lambda | Python | **Serverless webhook processor** - exactly-once with deduplication | GenServer, **durability**, idempotency |

---

## Phase 3: New Showcase Examples (Gap Coverage)

These are NEW examples (not migrations) that showcase abstractions not yet covered:

| # | Example Name | Target Lang | Real-World Use Case | Key Abstractions | Inspired By |
|---|-------------|------------|---------------------|-----------------|-------------|
| N1 | discord_guild_sessions | TypeScript | **Discord at scale** - 5M concurrent guild sessions with message fan-out, read states, typing indicators | **VirtualActor + Durability + ProcessGroup + Timer** | Discord engineering blog |
| N2 | distributed_lock_reservation | Python | **Airline seat reservation** - exactly-once seat assignment with distributed lock | **Lock** + GenServer | Booking.com patterns |
| N3 | ml_serving_pipeline | Python | **Real-time ML inference** - model serving with batching, routing, A/B testing (Ray Serve-style) | GenServer, channel, **registry** | Ray Serve |
| N4 | data_lake_ingestion | Go | **Data lake streaming** - S3/MinIO ingestion with dedup, schema evolution, partitioning | GenServer, **blob**, KV, channel | Databricks Delta Lake |
| N5 | rl_training_loop | Python | **Reinforcement learning** - RLHF loop with policy server, environment workers, replay buffer | GenServer, host.ask, multi-actor | Ray RLlib |
| N6 | llm_fine_tuning | Python | **LLM fine-tuning pipeline** - LoRA training with gradient accumulation across workers | **Workflow**, GenServer, host.ask | Ray Train + HuggingFace |
| N7 | feature_store | TypeScript | **Real-time feature store** - ML feature computation, caching, serving (100K req/sec) | GenServer, KV, **timer**, channel | Feast/Tecton |
| N8 | cron_scheduler | Python | **Distributed cron** - durable scheduled jobs with exactly-once, retry, dead letter | **Reminder** + Durability + GenServer | Temporal schedules |

---

## Phase 4: SDK Parity

For 3-5 key examples, create equivalent in all 4 languages:

| Example | Python | TypeScript | Go | Rust (embedded) |
|---------|--------|-----------|-----|-----------------|
| migrating_erlang_otp | Port | Port | Primary | Keep existing |
| migrating_temporal | Port | Primary | Port | Keep existing |
| migrating_ray | Primary | - | - | Remove |
| discord_guild_sessions | - | Primary | - | - |
| distributed_lock_reservation | Primary | Port | Port | - |

---

## Per-Example Workflow

For each example:
1. **Review**: Read current code, identify outdated APIs, check native/ reference
2. **Propose**: Suggest real-world use case and abstractions - get user approval
3. **Rewrite**: Complete rewrite in target language with:
   - SDK decorators/macros (no manual impls)
   - Inter-actor coordination via host.ask/host.send
   - Non-trivial data sizes (runs 2+ seconds)
   - Benchmark metrics prominently displayed
4. **Native**: Ensure native/ reference matches the PlexSpaces example use case
5. **Test**: Build and run, verify output and metrics
6. **User Verify**: User reviews and confirms
7. **Commit**: One commit per example
8. **Next**: Show next example, propose use case

---

## Current Status (Session 2026-02-21)

### Critical Fixes Applied This Session

#### 1. Go SDK WASM Canonical ABI Exports (exports.go - Complete Rewrite)

**Problem**: Go WASM actors crashed with `unreachable` because `wasm-tools component embed --dummy-names legacy` generated dummy stub modules (12KB) instead of using the real TinyGo code (1.1MB).

**Root Causes (3 layers)**:
1. `--dummy-names legacy` flag tells wasm-tools to generate a dummy core module with `unreachable` bodies, discarding the actual TinyGo code entirely
2. Without `--dummy-names legacy`, wasm-tools couldn't find exports because TinyGo exported `init` but Component Model expected qualified names like `plexspaces:simple-actor/actor@0.1.0#init`
3. Even with qualified export names, ABI mismatch: TinyGo auto-marshals Go string returns by adding a retptr param and returning void `(i32, i32, i32) -> ()`, but canonical ABI expects `(i32, i32) -> (i32)`

**Fixes Applied**:
1. **Removed `--dummy-names legacy`** from all 3 Go build scripts
2. **Rewrote `sdks/go/plexspaces/exports.go`** with raw uint32 types matching canonical ABI:
   - `cabi_realloc` for host-to-guest memory allocation
   - `ptrToString` / `stringToRetArea` helpers for string marshaling
   - All 4 actor functions with raw uint32 params/returns
   - `cabi_post_*` cleanup functions for each export
3. **Moved actor registration from `main()` to `init()`** in all 3 Go examples:
   - Reactor WASI adapter calls `_initialize` (which runs `init()`) but NOT `_start` (which runs `main()`)
   - Without this fix, `GetRegisteredActor()` returned nil
4. **Added "op" field extraction** in `wasmHandle()`:
   - HTTP gateway sends `msgType="call"` or `"cast"` for all requests
   - Actor Handle() methods expect specific types like "check_rate", "stats"
   - SDK now extracts the `"op"` field from JSON payload to use as msgType

#### 2. KeyValueStore Configuration (service_locator.rs)

**Problem**: WASM actors got `KeyValue store not configured` because each actor created its own ephemeral `:memory:` SQLite KV store.

**Fix**: Updated `initialize_services_impl` to use `create_keyvalue_stores_from_env()` / `create_keyvalue_stores_from_config()` instead of hardcoded SQLite. Added `plexspaces_common::KeyValueStore` trait impl to all backends (Redis, DynamoDB, Blob, SQLite, PostgreSQL). Returns tuple of both trait objects from config functions.

### Test Results (All Passing)

| Example | Language | Status | Performance |
|---------|----------|--------|-------------|
| migrating_erlang_otp | Go | **PASS** (all steps) | 79,859 ops/sec rate checking |
| migrating_cloudflare_workers | Go | **PASS** (all steps) | 2,500,000 rate checks/sec |
| migrating_cloudflare_workers | TypeScript | **PASS** (all steps) | 312,500 rate checks/sec |
| migrating_gosiris | Go | **MOSTLY PASS** | ProcessGroupRegistry not configured (server config) |

### WASM SDK Build Infrastructure

- **jco installed globally**: `npm install -g @bytecodealliance/jco` at `/opt/node22/lib/node_modules`
- **All build.sh scripts** for Go, TypeScript, and Python examples search global npm path via `npm root -g`
- **WASI adapter**: Found at `$(npm root -g)/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm`
- All Go examples build successfully to ~1.1MB components (real code)
- TypeScript migrating_cloudflare_workers builds successfully

### Build Status

All Go examples, TypeScript migrating_cloudflare_workers, and the full workspace compile successfully.

---

## Example Completion Status

### Go Apps (4/4 have build.sh + test.sh + .wasm)
| App | Status |
|-----|--------|
| migrating_cloudflare_workers | **DONE - Tested E2E** - guild_chat.wasm |
| migrating_erlang_otp | **DONE - Tested E2E** - rate_limiter.wasm |
| migrating_gosiris | **DONE - Tested E2E** - sensor_aggregation.wasm (process groups need server config) |
| migrating_rivet | **DONE - Tested E2E** - matchmaking.wasm (virtual actor + durability, game matchmaking) |

### TypeScript Apps
| App | Status |
|-----|--------|
| bank_account | Has build.sh + test.sh, needs build |
| migrating_cloudflare_workers | **DONE - Tested E2E** - guild_chat_actor.wasm |
| migrating_orbit | **DONE - Tested E2E** - read_state_actor.wasm (virtual actor + durability) |
| migrating_orleans | **DONE - Tested E2E** - batch_predictor_model.wasm (virtual actor + lazy activation) |

### Python Apps (4/17 built)
| App | Status |
|-----|--------|
| bank_account | DONE |
| calculator | DONE |
| migrating_ractor | DONE |
| migrating_ray | DONE |
| *13 others* | Have build.sh/test.sh but not yet built |

### Rust Embedded: 42 examples in examples/rust/embedded/

---

## Next Steps (For Next Session)

1. **Next example: migrating_temporal (TypeScript)** – E-commerce order fulfillment (saga-style with durability). See "Next Example to Work On" below.

2. **Build remaining Python examples** - Run build.sh for the 13 unbuilt Python apps

3. **Fix ProcessGroupRegistry** - Wire process group service for gosiris polling feature

4. **Continue migration examples** per Phase 2 plan (Group C, D, E) – one at a time with full verification

5. **Consider**: Fix `deployment_service.rs` to also pass `keyvalue_store` from a ServiceLocator
   reference (currently only wasm_application.rs path has it)

## SDK Improvements (TODOs)

### High Priority

- [ ] **Add HTTP Client Helper** - Add `host.http_request()` wrapper function for convenient HTTP API calls (simulates wasmCloud HTTP capability)
- [ ] **Add Capability Helper Classes to SDK** - Add helper classes (`host.keyvalue()`, `host.blob()`, `host.locks()`, `host.tuplespace()`) similar to `ProcessGroups` for better ergonomics
  - ✅ Example `KeyValue` helper class implemented in `migrating_wasmcloud` example
- [ ] **Add Detailed Examples** - Create example files showing usage of each capability (KV, Blob, Locks, TupleSpace)

### Medium Priority

- [ ] **Improve Type Hints** - Add comprehensive type hints throughout `host.py` with `TypedDict` for structured return types
- [ ] **Add HTTP Server Capability** - Actors can register HTTP routes, framework routes HTTP requests to actor handlers

### Low Priority

- [ ] **Better Error Handling** - Add exception classes for structured error handling (currently returns "ERROR:message" strings)
- [ ] **Performance Optimizations** - Connection pooling, batch operations, caching layer

---

## Completed Examples

### #1 migrating_ray (Python) - DONE

Python WASM app example completed end-to-end:
1. [x] Framework fix: pass child_spec.id in init config
2. [x] SDK fix: multi-actor class support in runtime.py
3. [x] Rewrite ray_actors.py with host.ask-based training coordination
4. [x] Create build.sh and test.sh
5. [x] Test: server.sh -> build.sh -> test.sh
6. [x] Verify metrics output

### #2 migrating_erlang_otp (Go) - DONE (E2E Tested)

Go WASM rate limiter service:
1. [x] Go SDK: add WASM exports (exports.go) for TinyGo - canonical ABI with raw uint32 types
2. [x] Go SDK: add multi-actor router (ActorRouter) for ACTOR_ROLES equivalent
3. [x] Create rate_limiter.go with SlidingWindowRateLimiter actor
4. [x] Create native Erlang reference (rate_limiter.erl)
5. [x] Create build.sh and test.sh
6. [x] Build and test verified end-to-end (79,859 ops/sec)

### #3 migrating_gosiris (Go) - DONE (E2E Tested)

Go WASM IoT sensor aggregation:
1. [x] Build.sh and test.sh created
2. [x] sensor_aggregation.wasm built successfully
3. [x] End-to-end tested: sensor reads, batch readings, aggregation all pass
4. [ ] Process group polling needs ProcessGroupRegistry server configuration

### #6 migrating_cloudflare_workers (TypeScript + Go) - DONE (E2E Tested)

Both TypeScript and Go versions build and test successfully:
- TypeScript: guild_chat_actor.wasm - all tests pass (312,500 rate checks/sec)
- Go: guild_chat.wasm - all tests pass (2,500,000 rate checks/sec)

### #5 migrating_wasmcloud (Python) - ✅ COMPLETE

Python WASM session store service:
1. [x] Create SessionStore actor with wasmCloud-style capability-based design
2. [x] Implement session CRUD operations using KV store capability
3. [x] Add timer-based cleanup for expired sessions (manual trigger via HTTP API)
4. [x] Use host.ask() to validate sessions (simulating HTTP capability)
5. [x] Add KeyValue helper class for convenient KV operations (example implementation)
6. [x] Create build.sh and test.sh scripts
7. [x] Create README.md with metrics and capability comparison
8. [x] Create app-config.toml for ApplicationSpec
9. [x] Fix self-messaging validation (tell() logs DEBUG, ask() errors)
10. [x] Add message_type parameter to MessageSender::send_message() (removes JSON parsing hacks)
11. [x] Update all call sites and test mocks
12. [x] Restore benchmark data sizes (200 sessions, 1000 operations, 50 refreshes)
9. [x] Improve type hints in host.py
10. [x] Test end-to-end: server.sh -> build.sh -> test.sh

### #7 migrating_orbit (TypeScript) - ✅ DONE (E2E Tested)

TypeScript WASM read state tracker (Discord-style read receipts):
1. [x] Create ReadStateTracker actor with virtual actor pattern
2. [x] Add durability for read state persistence
3. [x] Implement mark_read, get_read_state, get_all_read_states operations
4. [x] Add batch_mark_read for performance testing
5. [x] Create build.sh and test.sh scripts
6. [x] Create README.md with metrics and Orbit comparison
7. [x] Build WASM successfully (read_state_actor.wasm)
8. [x] Test end-to-end: server.sh -> build.sh -> test.sh

### #8 migrating_orleans (TypeScript) - ✅ DONE (E2E Tested)

TypeScript WASM batch predictor model service (Orleans-style virtual actors):
1. [x] Create BatchPredictorModel actor with virtual actor pattern and lazy activation
2. [x] Implement model loading, batch prediction, and state management
3. [x] Add batch prediction with configurable batch size
4. [x] Create build.sh and test.sh scripts
5. [x] Create README.md with metrics and Orleans comparison
6. [x] Build WASM successfully (batch_predictor_model.wasm)
7. [x] Test end-to-end: server.sh -> build.sh -> test.sh

### #8 migrating_rivet (Go) - ✅ COMPLETE (E2E Tested)

Go WASM game matchmaking (Rivet-style virtual actors per game room):
1. [x] Create matchmaking.go with GameRoom actor (virtual actor + durability)
2. [x] Implement join, leave, matchmake, get_status, get_stats operations
3. [x] Add skill/region matching and idle timeout (5m) for auto-deactivation
4. [x] Create build.sh and test.sh (in example dir), app-config.toml, README.md
5. [x] Benchmarks: coord vs compute, granularity, non-trivial batch (100 rooms × 10 players)
6. [x] Test: server.sh (base dir) -> build.sh -> test.sh 8092

### #9 migrating_temporal (TypeScript) - ✅ COMPLETE (E2E Tested)

E-commerce order fulfillment workflow (Workflow run/signal/query + virtual_actor + durability):
1. [x] TypeScript WASM: `examples/typescript/apps/migrating_temporal` (OrderFulfillmentActor, build.sh, test.sh)
2. [x] Workflow behavior: run (saga steps), signal(cancel), query(status); metrics (coord vs compute, non-negative)
3. [x] app-config: behavior_kind = "Workflow", virtual_actor + durability facets
4. [x] Polyglot variants (same use case, under examples/{lang}/apps/migrating_temporal):
   - **Go**: WorkflowActor (Run/Signal/Query), build.sh, test.sh
   - **Python**: @workflow_actor, run/signal/query, build.sh, test.sh
   - **Rust**: See `examples/rust/embedded/migrating_temporal` (embedded WorkflowBehavior); WASM app in `examples/rust/apps/migrating_temporal` links to embedded as reference

---

## Next Example to Work On: #10 migrating_cadence (Go)

**Phase 2 Group C** – Workflow + Durability (Temporal/Step Functions patterns).

### Plan Summary

| Field | Value |
|-------|--------|
| **Example** | migrating_cadence |
| **Target Lang** | Go |
| **Real-World Use Case** | **Payment processing with retries** – idempotent payments, retry policies |
| **Key Abstractions** | Workflow, durability, retry |
| **App vs Embedded** | WASM app (deploy via HTTP, test with server.sh + build.sh + test.sh 8092) |

### Real-World Use Case (E-commerce Order Fulfillment)

- **Flow**: Place order → Validate order → Reserve inventory → Charge payment → Ship (or compensate on any failure).
- **Signals**: e.g. `cancel` to abort and run compensation.
- **Queries**: e.g. `get_status` (read-only order state).
- **Durability**: Each step and compensation checkpointed; replay-safe.
- **Non-trivial run**: e.g. 50–100 orders, 2+ seconds total, so metrics are meaningful.

### Workflow Behavior (MANDATORY for This Example)

The example **must use the framework Workflow behavior** (run/signal/query), aligned with:

- **Rust**: `crates/behavior` `Workflow` trait; `#[workflow_actor]` + `#[plexspaces_handlers(workflow)]` with `run_handler`, `#[signal_handler("name")]`, `#[query_handler("name")]`; routing by `message_type` (workflow_run, workflow_signal:name, workflow_query:name).
- **Framework**: `MessageTypeExt` in behavior maps `message_type` string to `WorkflowRun` / `WorkflowSignal(name)` / `WorkflowQuery(name)`; WASM path uses `try_msg_type_from_payload` with **canonical payload key `message_type`** (aliases: `op`, `msg_type`; order: message_type → op → msg_type) then passes to instance `handle(from, msg_type, payload)`.
- **TypeScript SDK**: `PlexSpacesActor.handle()` routes `msgType === "workflow_run"` → `run(payload)`, `msgType.startsWith("workflow_signal:")` → `signal(name, payload)`, `msgType.startsWith("workflow_query:")` → `query(name, payload)`. Optional base class `WorkflowActor<TState>` with abstract `run`, `signal`, `query` (exported from `@plexspaces/sdk`).
- **Python SDK**: `@workflow_actor` sets `_plexspaces_workflow`; `dispatch_message()` routes `effective_type == "workflow_run"` → `run(payload)`, `workflow_signal:name` → `signal(name, payload)`, `workflow_query:name` → `query(name, params)`.
- **app-config.toml**: Use `behavior_kind = "Workflow"` (and facets e.g. durability) so the node/registry treat the actor as a workflow.

**Client contract**: Request body sets application message type; **canonical key is `message_type`** (aliases: `op`, `msg_type`). Values: `workflow_run` (start/run), `workflow_signal:cancel`, `workflow_query:status`. Use ask (request-reply) for run and query so the client gets the result.

### SDK and Framework Extensions Completed (for Workflow)

1. **crates/application**: `try_msg_type_from_payload()` uses **canonical key `message_type`** then aliases `op`, `msg_type` (same order in all SDKs).
2. **sdks/python**: `dispatch_message()` resolves effective_type from payload: message_type → op → msg_type; `@workflow_actor` routes workflow_run / workflow_signal:name / workflow_query:name to `run` / `signal` / `query`.
3. **sdks/typescript**: `handle()` uses payload.message_type → op → msg_type for op-based dispatch; workflow routing by `msgType` to `run` / `signal` / `query`; `WorkflowActor<TState>` base class.
4. **sdks/go**: `exports.go` resolves msgType from payload message_type → op → msg_type; `WorkflowActor` interface with `Run`/`Signal`/`Query`; workflow routing when actor implements `WorkflowActor`.

### Implementation Notes for migrating_temporal

1. **Use WorkflowActor** (or PlexSpacesActor with `run`, `signal`, `query`). One workflow actor instance per order (virtual actor or explicit ID); durability via getState/setState (DurabilityFacet in app-config).
2. **run(payload)**: Execute saga steps (validate → reserve → charge → ship); on failure run compensation; return final result. Use `host.ask()` for “activity” calls if we add child actors later, or inline steps for simplicity.
3. **signal(name, data)**: e.g. `cancel` sets a flag and triggers compensation (release inventory, refund).
4. **query(name, params)**: e.g. `status` returns current order state (read-only).
5. **Metrics**: Coordination vs computation %, granularity, total time, throughput (orders/sec); run 2+ s with non-trivial data (e.g. 50–100 orders).
6. **Scripts**: One README.md; `build.sh` and `test.sh` in example dir; test via `./scripts/server.sh` then `./build.sh` and `./test.sh 8092`.

### Design References (Review for Consistency)

- **Workflow trait and routing**: `crates/behavior/src/mod.rs` (Workflow trait, route_workflow_message, MessageTypeExt), `crates/behavior/src/workflow.rs` (ExecutionContext for durable run/sleep/now).
- **Message type from payload**: `crates/application/src/wasm_application.rs` (try_msg_type_from_payload, handle_message message_type).
- **Workflow actor registration**: `crates/node/src/object_registry_helpers.rs` (register_workflow), app-config `behavior_kind = "Workflow"`.
- **Rust workflow actor macro**: `sdks/rust/plexspaces-sdk-macros` (`#[workflow_actor]`, `#[plexspaces_handlers(workflow)]`, run_handler / signal_handler / query_handler).
- **Core types**: `crates/core/src/lib.rs` (BehaviorType, ActorContext, Message).

### Verification Checklist (Before Moving to Next Example)

- [ ] Review existing design/impl; use correct APIs (SDK or host); no dead/legacy code.
- [ ] One README.md only; build.sh and test.sh in example dir; server via scripts/server.sh.
- [ ] Shared target: debug build; no separate target dir for examples.
- [ ] Metrics: coord vs compute, benchmark section, run 2+ s, non-trivial data size.
- [ ] Full test: `./scripts/server.sh` (base) → `./build.sh` → `./test.sh 8092` (all pass).
- [ ] Principal-engineer review: prod-grade, minimal boilerplate, clear use case.
