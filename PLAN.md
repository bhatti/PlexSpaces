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

## Phase 1: Fix migrating_ray Python Example (CURRENT)

- [x] Framework: pass child_spec.id as init config to WASM actors
- [x] Python SDK: support multiple @actor classes per WASM module
- [ ] Create ray_actors.py with ParameterServer + DataWorker (inter-actor host.ask)
- [ ] Create build.sh and test.sh
- [ ] Test end-to-end: server.sh -> build.sh -> test.sh
- [ ] Remove examples/rust/embedded/migrating_ray

---

## Phase 2: Migration Examples (Convert Rust Embedded -> WASM Apps)

All `examples/rust/embedded/migrating_*` will be converted to Python, TypeScript, or Go WASM apps
based on the native framework's language. Work one at a time with full verification.

### Group A: GenServer + Basic Patterns

| # | Example | Target Lang | Real-World Use Case | Key Abstractions |
|---|---------|------------|---------------------|-----------------|
| 1 | migrating_ray | Python | **Distributed ML training** - parameter server with gradient aggregation (Ray Train style) | GenServer, host.ask, multi-actor ApplicationSpec |
| 2 | migrating_erlang_otp | Python | **Rate limiter service** - sliding window with distributed counters (API gateway) | GenServer, state, handler |
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
| 9 | migrating_temporal | TypeScript | **E-commerce order fulfillment** - multi-step with compensation/saga | **Workflow**, signals, queries, durability |
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
| migrating_erlang_otp | Primary | Port | Port | Keep existing |
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

## Current Focus: #1 migrating_ray (Python)

Fix the Python WASM app example end-to-end:
1. [x] Framework fix: pass child_spec.id in init config
2. [x] SDK fix: multi-actor class support in runtime.py
3. [ ] Rewrite ray_actors.py with host.ask-based training coordination
4. [ ] Create build.sh and test.sh
5. [ ] Test: server.sh -> build.sh -> test.sh
6. [ ] Verify metrics output
7. [ ] Remove examples/rust/embedded/migrating_ray
