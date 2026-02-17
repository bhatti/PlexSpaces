# PlexSpaces Migration Examples Rewrite Plan

## Previous PLAN.md Steps 1-11 Status

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

## Remaining Pre-work (Step 8)

- [ ] Add TypeScript behavior classes: GenServerActor, EventActor, FsmActor to sdks/typescript/src/actor.ts

---

## Migration Examples Master Checklist

### Mandatory Criteria for EVERY Example

1. **Use Correct PlexSpaces APIs** - SDK macros (#[gen_server_actor], #[handler], etc.), no manual Actor/GenServer impls
2. **Real-World Use Case** - Not abstract API demos, domain-specific scenarios
3. **Single main.rs** - Minimal dependencies, simple code
4. **Clear Output** - Show what's happening with println! (not just tracing)
5. **README.md** - Purpose, APIs used, quick start, comparison table
6. **Metrics & Benchmarks (MANDATORY)**:
   - Coordination vs computation latency breakdown
   - Cost analysis (% time on coordination vs computation)
   - Granularity ratio (compute/coordinate >= 10x)
   - Benchmark metrics (GFLOPS, throughput MB/s, ops/sec, estimated speedup)
   - Non-trivial data sizes (runs for 2+ seconds)
   - Prominently displayed with section headers
7. **Shared Target Directory** - `.cargo/config.toml` with `target-dir = "../../../../target"`
8. **Debug Builds** - No `--release` flag
9. **No Dead/Legacy Code** - Remove backward-compatibility hacks
10. **Consistent Shell Scripts** - All in `scripts/` subdirectory
11. **Only one README.md** - No other .md files per example

### Example Types

- **Embedded** (`examples/rust/embedded/`): Runs as standalone binary with NodeBuilder, in-process actors
- **App** (`examples/rust/apps/`, `examples/python/apps/`, etc.): WASM modules deployed to a running node via HTTP

---

## Phase 1: Fix Python Ray Example (Immediate)

- [ ] Fix examples/python/apps/migrating_ray/ - create proper build.sh and test.sh
- [ ] Verify the Python actor code works with current SDK/WIT

## Phase 2: Rust Embedded Migration Examples (One at a Time)

Work order: Start with simplest (GenServer pattern), progress to complex (Workflow, durability).

### Group A: GenServer Pattern (Request-Reply Actors)

| # | Example | Real-World Use Case Suggestion | Key APIs |
|---|---------|-------------------------------|----------|
| 1 | migrating_erlang_otp | Distributed counter / rate limiter service | #[gen_server_actor], #[handler], spawn, ask |
| 2 | migrating_gosiris | IoT sensor aggregation pipeline | #[gen_server_actor], process groups, messaging |
| 3 | migrating_ractor | Scientific matrix computation service | #[gen_server_actor], parallel compute, metrics |
| 4 | migrating_cloudflare_workers | Session store / shopping cart (Durable Objects) | #[gen_server_actor], durability facet |
| 5 | migrating_orbit | User profile cache with virtual actors | #[gen_server_actor], virtual_actor facet |
| 6 | migrating_rivet | Game room matchmaking server | #[gen_server_actor], process groups |
| 7 | migrating_wasmcloud | API gateway with rate limiting | #[gen_server_actor], http_client, kv |
| 8 | migrating_v8_isolates | High-throughput log/metrics processing | #[gen_server_actor], channels, batch processing |

### Group B: Workflow Pattern (Durable Orchestration)

| # | Example | Real-World Use Case Suggestion | Key APIs |
|---|---------|-------------------------------|----------|
| 9 | migrating_temporal | E-commerce order fulfillment workflow | #[workflow_actor], signals, queries, activities |
| 10 | migrating_cadence | Payment processing with retries | #[workflow_actor], durability facet |
| 11 | migrating_aws_step_functions | Multi-step data pipeline (ETL) | #[workflow_actor], state machine |
| 12 | migrating_azure_durable_functions | Fan-out/fan-in document processing | #[workflow_actor], parallel activities |
| 13 | migrating_conductor | Microservice orchestration (order saga) | #[workflow_actor], multi-step coordination |
| 14 | migrating_dapr | Unified actor + workflow hybrid | #[gen_server_actor] + workflow, kv store |
| 15 | migrating_restate | Idempotent payment processing with journaling | #[gen_server_actor], durability facet, event_sourcing |
| 16 | migrating_zeebe | BPMN-style insurance claim processing | #[workflow_actor], event sourcing, signals |

### Group C: Distributed Compute Pattern

| # | Example | Real-World Use Case Suggestion | Key APIs |
|---|---------|-------------------------------|----------|
| 17 | migrating_ray | Distributed ML training (parameter server) | #[gen_server_actor], parallel workers, gradient aggregation |
| 18 | migrating_kueue | AI/ML job scheduling with resource quotas | #[gen_server_actor], kv store, channels |
| 19 | migrating_skypilot | Multi-cloud ML training orchestration | #[workflow_actor], resource scheduling |
| 20 | migrating_eflows4hpc | HPC ensemble simulation pipeline | #[workflow_actor], fan-out/fan-in, large data |
| 21 | migrating_merlin | Scientific simulation ensemble manager | #[workflow_actor], parallel compute |

### Group D: Coordination Pattern (Tuple Space, Process Groups)

| # | Example | Real-World Use Case Suggestion | Key APIs |
|---|---------|-------------------------------|----------|
| 22 | migrating_mozartspaces | Distributed auction system (XVSM) | tuple space, process groups, coordination |
| 23 | migrating_luats | Event-driven workflow with Linda coordination | tuple space, events, timers |
| 24 | migrating_dirigo | Real-time stream analytics pipeline | #[event_actor], channels, windowing |

### Group E: Special Pattern

| # | Example | Real-World Use Case Suggestion | Key APIs |
|---|---------|-------------------------------|----------|
| 25 | migrating_aws_durable_lambda | Serverless payment processing (exactly-once) | #[gen_server_actor], durability facet, idempotency |

---

## Phase 3: SDK Parity Examples

For select examples (3-5 key ones), create equivalent:
- [ ] Python WASM app version
- [ ] TypeScript WASM app version
- [ ] Go WASM app version

Priority examples for polyglot ports:
1. migrating_erlang_otp (GenServer basics)
2. migrating_temporal (Workflow pattern)
3. migrating_ray (Distributed compute)

---

## Per-Example Workflow

For each example:
1. **Review**: Read current main.rs, identify issues, outdated APIs
2. **Suggest**: Propose real-world use case, get user approval
3. **Rewrite**: Complete rewrite with:
   - SDK macros (no manual trait impls)
   - CoordinationComputeTracker for metrics
   - Non-trivial data sizes
   - Clear output with benchmark section
4. **Test**: Build and run, verify output
5. **Verify**: User reviews and confirms
6. **Commit**: One commit per example
7. **Next**: Show next example, suggest use case

---

## Current Focus: Example #1 - migrating_ray (Rust Embedded)

Starting here because the Python Ray example test.sh is failing, and the Rust embedded version needs the same rewrite. Fix both together.
