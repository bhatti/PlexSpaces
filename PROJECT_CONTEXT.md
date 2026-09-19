# PlexSpaces — Project Context

Authoritative reference for PlexSpaces. Read this at the start of every session — it replaces mining archived docs and scattered READMEs.

---

## 1. Project Identity

| Attribute | Value |
|---|---|
| Name | PlexSpaces |
| Language | Rust (28 crates) + polyglot actors via WASM (Python, TypeScript, Go) |
| Inspiration | Erlang/OTP (supervision, behaviors), Restate (durable execution), Orleans (virtual actors), wasmCloud (WASM polyglot), Linda (TupleSpace) |
| License | AGPL-3.0-or-later — every file must carry the SPDX header |
| Source of truth for status | `PROJECT_TRACKER.md` |
| Source of truth for rules | `CLAUDE.md` / `AGENTS.md` (identical files) |

The goal is a **high-performance, low-memory-footprint distributed actor framework** that scales to millions of actors across nodes with full multi-tenant isolation, polyglot WASM support, durable execution, and production-grade observability.

---

## 2. Five Foundational Pillars

### Pillar 1 — Erlang/OTP Behaviors

The core computation model. Every actor belongs to one of these behavior types:

| Behavior | Description |
|---|---|
| `GenServer` | Stateful request/reply server. Handles `Call` (request/reply), `Cast` (fire-and-forget), `Info` (system) |
| `GenFSM` | Finite state machine. Typed transitions between states |
| `GenEvent` | Multi-handler event bus. Dynamic add/remove of handlers |
| `Workflow` | Durable multi-step execution. Signals, suspend/resume, saga/compensation |

Supervision: `RootSupervisor` with three strategies — `OneForOne` (restart only failed child), `OneForAll` (restart all on any crash), `RestForOne` (restart failed + all after it). Restart policies per child: `Permanent`, `Transient`, `Temporary`. Advanced strategies: `ExponentialBackoff` (100ms → 30s cap), `Adaptive` (auto-switches strategy on failure rate threshold).

"Let it crash" philosophy: actors fail fast; supervisors restart. No defensive try/catch inside actor handlers.

**Behavior stack** (`become`/`unbecome`): actors hold a `Vec<Box<dyn ActorBehavior>>`. Calling `become(B)` pushes new behavior; `unbecome()` pops back. Enables layered state-machine transitions without explicit FSM boilerplate.

### Pillar 2 — Durable Execution

Restate-inspired journaling for resilience:

- Every message and state change is logged to the journal before being processed
- Deterministic replay on restart: actor reconstructs state from journal events
- Exactly-once semantics: replay skips already-executed journal entries
- Checkpoints: periodic state snapshots for fast recovery (no full replay from genesis)
- Time-travel debugging: replay journal to any historical point

Journal is transparent to actor code. The `DurabilityFacet` (priority 90) intercepts every message, journals it, then forwards to the actor.

### Pillar 3 — WASM Polyglot Runtime

wasmtime Component Model is the execution engine for non-Rust actors:

- One canonical WIT package: `plexspaces:actor@0.1.0`
- Two worlds (see Section 7 for details)
- Polyglot support: Python, TypeScript, Go (TinyGo), Rust
- `get_state`/`set_state` cycle preserves actor state across WASM store reinstantiations
- Host interfaces: logging, messaging (tell/ask/spawn/stop), tuplespace, channels, durability, workflow, blob, keyvalue, process-groups, locks, registry, outbound-http

### Pillar 4 — TupleSpace (Linda Model)

Spatial and temporal decoupling via associative shared memory:

| Operation | Semantics |
|---|---|
| `write` | Non-blocking insert of a tuple |
| `read` | Non-destructive read (blocking if no match) |
| `take` | Destructive read (blocking if no match) |

Pattern matching with wildcards. Backends: SQLite in-memory (tests), Redis (production), PostgreSQL (transactional). Actors access TupleSpace via `ActorContext` or WASM `host-ts.wit` interface.

### Pillar 5 — Firecracker Isolation

Each actor in its own Firecracker microVM for strong multi-tenant sandboxing:

- Boot time: <125ms
- CPU/memory/I/O quotas per actor
- Strong isolation guarantees (no shared kernel namespaces)
- Managed by `crates/firecracker/`

---

## 3. Proto-First Design (Non-Negotiable)

**All data shapes MUST be defined in `.proto` files first.** Rust (and other language) types are generated. No Rust struct for a domain concept may exist before its proto definition.

```
Correct workflow:
  1. Define in .proto
  2. buf format -w proto/ && buf lint
  3. buf generate  (user runs this)
  4. Implement in Rust
  5. Write tests
```

**What goes in proto:**
- Data structures (Message, ActorId, Config types, ActorSpawnSpec)
- Error enums (WorkflowError, MailboxError)
- gRPC service definitions (for remote communication only)

**What stays in Rust:**
- Traits (`ActorBehavior`, `JournalStorage`, `ServiceLocator`)
- Implementation details (Actor, Mailbox, internal state machines)
- Helper functions

**Key canonical proto types:**

| Type | File | Significance |
|---|---|---|
| `ActorSpawnSpec` | `actors/actor_runtime.proto` | Single source of truth for spawn contracts |
| `ActorId` | `actors/actor_runtime.proto` | Format: `name//actor_type::namespace@node_id` |
| `RequestContext` | `common.proto` | Tenant + namespace for every operation |
| `Message` | `common.proto` | Unified message envelope |
| `NodePlacement` | `actors/actor_runtime.proto` | Unified placement for ShardGroups + scheduler |
| `CollectiveReduction` | `actors/actor_runtime.proto` | SUM, PRODUCT, MAX, MIN, BOOL_AND, BOOL_OR, CONCAT |

Every request/response carries `request_id` (ULID string, max 40 chars) for end-to-end tracing.

Proto tree: `proto/plexspaces/v1/` (public APIs) and `proto/plexspaces/prv/` (internal contracts).

---

## 4. Crate Dependency Layers

28 crates under `crates/`, all prefixed `plexspaces-`. Dependencies flow bottom-up; cross-layer imports are forbidden.

```
Layer 0 — Wire types (generated):
  proto (plexspaces-proto)
  └── Root of the dependency graph. Everything depends on this.

Layer 1 — Shared primitives (no internal deps):
  common    — ConfigManager, KeyValueStore trait, ReleaseSpec/RuntimeConfig
  lattice   — CRDT implementations (G-Counter, OR-Set)

Layer 2 — Storage backends:
  keyvalue        — Multi-backend KV store (SQLite/Postgres/Redis/DynamoDB)
  locks           — Distributed lock/lease coordination
  tuplespace      — Linda-style TupleSpace (uses lattice CRDTs)
  blob            — S3-compatible blob storage
  object-registry — Distributed actor/service registry (Orleans grain directory)
  scheduler       — Resource-aware scheduling; uses locks + object-registry + channel

Layer 3 — Trait firewall (CRITICAL — breaks service↔actor cycles):
  service-traits  — ServiceLocatorBase, ActorFactory, JournalStorage,
                    OutboundHttpClient, WasmRuntimeTrait, ActorTransportClient,
                    NodeTransportClient, BlobServiceTrait, MetricsServiceAccess,
                    MessageSender, ChannelService, TupleSpaceProvider, etc.
                    This crate is the seam between actor and services.

Layer 4 — Messaging primitives:
  channel   — Extensible pub/sub + queue abstraction; observability.rs for metrics
  mailbox   — Per-actor inbox; composable; supports priority ordering

Layer 5 — Actor core:
  facet     — FacetManager, Facet trait, cross-cutting capability injection
  actor     — ActorContext, ActorRef (local/remote), ActorRegistry, Actor trait,
              BehaviorFactory, BehaviorRegistry, supervisor trees,
              VirtualActorManager, GrpcConnectionManager, ActorBuilder,
              ServiceLocator trait (full extension of ServiceLocatorBase)

Layer 6 — Execution engines:
  journaling    — DurabilityFacet, ContractFacet, TrajectoryExportFacet, reminders/timers
  persistence   — Event-sourcing state persistence
  wasm-runtime  — wasmtime Component Model execution engine
  workflow      — Durable workflow orchestration (built on actor + persistence + facet)
  elastic-pool  — Auto-scaling elastic worker pool

Layer 7 — Application lifecycle:
  application   — Deploy/start/stop multi-actor applications; WASM component loading;
                  translates ChildSpec → ActorSpawnSpec

Layer 8 — Service implementations + gRPC handlers:
  services      — ServiceLocatorImpl (lives here), all gRPC service impls
                  (ActorService, NodeService, ApplicationService, BlobService,
                  MetricsService, DashboardService, TupleService, WorkflowService,
                  ObjectRegistryService, ProcessGroupService, SystemService, etc.)

Layer 9 — Node binary + HTTP gateway:
  node          — Top-level binary: gRPC server, Axum REST gateway, WebSocket,
                  TLS, JWT auth, SWIM node registry, WASM app loader

Layer 10 — Tooling + infrastructure:
  grpc-middleware — Production gRPC middleware: metrics, auth, rate limiting, tracing
  http-client     — ResilientOutboundHttpClient (circuit breaker + retry)
  dashboard       — Monitoring dashboard service + UI
  firecracker     — Firecracker microVM integration
  cli             — plexspaces CLI tool
  test-utils      — TestServiceLocator and shared test helpers (eliminates test duplication)
```

**The `service-traits` crate is the most important architectural seam.** It prevents `actor` from directly importing `services`, keeping the dependency graph acyclic in production code. Tests use dev-dependencies to close the loop.

---

## 5. Key Architectural Patterns

### ServiceLocator (Dependency Injection)

The central DI mechanism. All service access goes through it — never import service implementations directly.

| Variant | Location | Scope |
|---|---|---|
| `ServiceLocatorBase` | `service-traits/src/service_locator_base.rs` | Minimal; shared by `actor` and `journaling` without pulling in `services` |
| `ServiceLocator` (full) | `actor/src/service_locator_trait.rs` | Full extension; adds channel, tuplespace, object-registry, facet, WASM, blob, transport, etc. |
| `ServiceLocatorImpl` | `services/src/service_locator.rs` | Production implementation; `Arc<RwLock<Option<Arc<dyn T>>>>` per service field |
| `TestServiceLocator` | `actor/src/test_service_locator.rs` | Test stub; returns `None` for everything (overridable) |

```rust
// Never do this:
let factory = ActorFactoryImpl::new(...);

// Always do this:
let factory = service_locator.get_actor_factory().await?;
```

### ActorContext

`actor/src/actor_context.rs` — static, reusable, not per-message.

Fields: `node_id`, `tenant_id`, `namespace`, `metadata`, `config`, `service_locator: Arc<dyn ServiceLocator>`, `trap_exit`, `self_ref: Option<ActorRef>`. Message-specific data (sender_id, correlation_id) lives in `Envelope`, not in context.

### ActorRef

`actor/src/actor_ref.rs` — location-transparent actor handle.

Two internal variants: `Local { mailbox, service_locator, visibility }` and `Remote { node_id, service_locator, visibility }`. Remote refs use `service_locator.get_actor_transport_client()` → gRPC or WebSocket. Same `tell()`/`ask()` API regardless of locality.

### Facets (AOP / Cross-Cutting Concerns)

`facet/src/` — composable, priority-ordered interceptors attached at actor level. Actor code never changes to add observability, guardrails, or cost tracking — they are purely additive.

Priority ordering (higher runs first on ingress, last on egress):

| Priority | Facet | Purpose |
|---|---|---|
| 1000+ | Security/Auth facets | Authentication, authorization |
| 900–999 | Tracing/Logging facets | Distributed tracing |
| 800–899 | MetricsFacet | Actor-level metrics |
| 100–500 | Domain facets | Business-specific interceptors |
| 100 | VirtualActorFacet | Virtual actor lifecycle |
| 95 | ContractFacet | Tool-call validation (before journaling) |
| 90 | DurabilityFacet | Journaling (event sourcing) |
| 85 | TrajectoryExportFacet | Collects trace data after journaling |
| 80 | MetricsFacet | Standard actor metrics |

Facets are declared in `app-config.toml`, not in actor code.

### PendingAsks (ask/reply — no temporary actors)

16-shard `DashMap<ULID, oneshot::Sender<Message>>`. When `ask()` is called: insert sender, send message with correlation_id, await receiver. On reply arrival: lookup correlation_id, send on oneshot. Background GC sweeps expired entries every 5 seconds for hard memory bound. Two heap allocations per ask, no mailbox pre-allocation.

### Virtual Actors (Orleans grain pattern)

- **Type is primary for recreation**: activation always uses type registry metadata; instance metadata is fallback.
- **Eviction**: only `Lazy` activation strategy actors are evicted by LRU pressure; `Eager` actors are never evicted.
- **Addressability invariant**: after LRU eviction, `restore_as_virtual_wrapper()` ensures actor remains addressable via `VirtualActorWrapper` — a lightweight stub, not a full registration.
- **Object registry alias**: `"{actor_type}:{name}:{namespace}:{tenant_id}"`

### GrpcConnectionManager

Use `GrpcConnectionManager` (in `actor` crate) for **all** gRPC client creation. Never create gRPC channel/clients directly. The manager handles connection pooling and lifecycle.

### SWIM / Node Health

- 1 missed heartbeat → `DEGRADED`
- N misses (default 3) → `DEAD`
- Node death cascades to all objects registered on that node in `ObjectRegistry`
- LRU cache (30s TTL, 10k cap) for alias lookups
- Thin nodes (`NodeRoleThin`) are excluded from SWIM probing; they use WebSocket heartbeat forwarded to `NodeRegistry`

### WebSocket Transport

`WsRegistry`, `WsActorTransportClient`, `WsNodeTransportClient` in `node` crate. Binary protobuf `WsFrame` envelopes. Used by browser clients and thin nodes behind firewalls. `ActorTransportClient` trait abstracts gRPC vs. WebSocket — callers never know which wire is used.

---

## 6. Actor Lifecycle and Behaviors

### Actor Trait (`actor/src/actor_types.rs`)

```rust
pub trait Actor: Send + Sync {
    async fn init(&mut self, ctx: &ActorContext) -> Result<(), ActorError>;
    async fn handle_message(&mut self, ctx: &ActorContext, envelope: Envelope) -> Result<(), ActorError>;
    async fn handle_exit(&mut self, ctx: &ActorContext, linked_actor_id: &str, reason: ExitReason);
    async fn terminate(&mut self, ctx: &ActorContext) -> Result<(), ActorError>;
    async fn capture_checkpoint_state(&self) -> Option<Vec<u8>>;
    async fn restore_checkpoint_state(&mut self, state: Vec<u8>) -> Result<(), ActorError>;
    async fn on_facets_ready(&mut self, ctx: &ActorContext) -> Result<(), ActorError>;
    async fn on_facets_detaching(&mut self, ctx: &ActorContext);
    fn replay_signal(&self) -> Option<Arc<AtomicBool>>;
    fn behavior_type(&self) -> BehaviorType;
}
```

The SDK annotations (`#[gen_server_actor]`, etc.) generate these implementations — **do not implement this trait manually** unless building a new behavior type.

### Behavior (Simplified Pattern, `actor/src/behavior/simplified.rs`)

```rust
pub trait Behavior: Send + Sync {
    async fn process(&mut self, input: Input) -> Result<Output, BehaviorError>;
    fn handles(&self) -> Vec<String>;
    async fn init(&mut self, config: HashMap<String, String>) -> Result<(), BehaviorError>;
    async fn cleanup(&mut self) -> Result<(), BehaviorError>;
}
```

`BehaviorFactory` + `BehaviorRegistry` register constructor functions by module name.

### Message Flow (GenServer Call)

```
Client: call_message(payload) → actor_ref.ask(msg, timeout)
  → insert (correlation_id, oneshot_sender) into PendingAsks
  → deliver to actor mailbox (local) or gRPC/WS (remote)
Actor: DurabilityFacet journals message
  → handle_call() executes
  → journals state change
  → constructs reply with correlation_id
  → ActorRegistry.tell(sender_id, reply)
Client: PendingAsks lookup correlation_id → send on oneshot → await returns reply
```

### Monitor / Link (Erlang OTP Semantics)

| Mechanism | Direction | Effect |
|---|---|---|
| **Monitor** | One-way | Watcher receives `ActorDownNotification` in its mailbox when the monitored actor dies. No reciprocal effect. |
| **Link** | Bidirectional | When either actor dies, the other receives an exit signal (`is_link_signal=true` on `ActorDownNotification`). If `trap_exit=false` (default), the linked actor also terminates. |

Both are location-transparent: `ActorDownNotification` is routed via `ActorRegistry::tell()` regardless of node.

### Schema Versioning

`actor_state_schema_version` on `Actor` and `Checkpoint` tracks serialization format independently of proto field numbers. Same version = direct load; older = migrate forward; newer = reject (upgrade code first). Enables safe rolling upgrades for long-lived virtual actors.

---

## 7. WASM / WIT Integration

WIT files: `wit/plexspaces-actor/` (25 files).

### Two Actor Worlds

**`plexspaces-actor`** (typed world, for Rust actors):
- Full WIT-typed interfaces
- Exports `native-actor`
- Imports: `logging`, `messaging`, `tuplespace`, `channels`, `durability`, `workflow`, `blob`, `keyvalue`, `process-groups`, `locks`, `registry`

**`actor-world`** (polyglot world, for Python/TypeScript/Go):
- Simplified JSON-over-strings "host" interfaces
- Exports `actor`
- Imports: `host-logging`, `host-actor`, `host-kv`, `host-ts`, `host-locks`, `host-blob`, `host-pool`, `host-shard`, `host-http`, `channels`, `registry`

### Host Function Implementation

| File | Purpose |
|---|---|
| `wasm-runtime/src/component_host.rs` | WIT bindgen! typed host trait implementations |
| `wasm-runtime/src/simple_component_host.rs` | JSON-string polyglot host implementations |
| `wasm-runtime/src/instance.rs` | Instance lifecycle; manages both module and component instances |
| `wasm-runtime/src/instance_pool.rs` | Pool for traditional WASM module instances (components not poolable — not `Send`) |
| `wasm-runtime/src/host_functions.rs` | HostFunctions struct bridging WIT traits → Rust service implementations |

### Known WASM Bottlenecks

- **Store reinstantiation per message** (3.5ms overhead): wasmtime issue #8943. Fix: check if wasmtime patched #8943; if so, remove `create_fresh_plexspaces_actor_state()` call after `handle_event()` in `instance.rs:~1966`.
- **~137MB RSS per Python WASM actor**: CPython heap per Store; no sharing between instances. Near-term mitigation: `--max-wasm-instances N` cap.
- **InstancePool excludes component instances**: `wasmtime::component::Instance` is not `Send` (`instance_pool.rs:292`). Fix needs wasmtime adding `Send` or per-thread pooling.

---

## 8. SDK Design Contract

The SDK is a **decorator** — it removes boilerplate for application developers. It must not re-implement or duplicate core logic. Core logic lives in Rust crates only.

### Available Annotations (`sdks/rust/plexspaces-sdk`)

| Annotation | Behavior | Use Case |
|---|---|---|
| `#[gen_server_actor]` | GenServer | Request/reply actors |
| `#[event_actor]` | GenEvent | Fire-and-forget event handling |
| `#[fsm_actor]` | GenStateMachine | Finite state machine |
| `#[fsm_actor(states=["a","b"], initial="a")]` | GenStateMachine | FSM with explicit states |
| `#[workflow_actor]` | Workflow | Durable workflow orchestration |
| `#[actor(facets=["timer"])]` | Custom | Custom behavior with facets |

### Handler Annotations

| Annotation | Semantics | Return Type |
|---|---|---|
| `#[handler("op")]` | call (GenServer default) | `Result<Value, BehaviorError>` |
| `#[handler("op", call)]` | Explicit call | `Result<Value, BehaviorError>` |
| `#[handler("op", cast)]` | Fire-and-forget | `Result<(), BehaviorError>` |

### Spawn Helpers

```rust
use plexspaces_sdk::{spawn, spawn_with_facets, spawn_with_storage};

// Use proper tenant/namespace — NEVER RequestContext::internal()
let ctx = RequestContext::new_without_auth("tenant".into(), "namespace".into());

let actor_ref = spawn(&ctx, service_locator, actor_id, "ns", actor).await?;
let actor_ref = spawn_with_facets(&ctx, service_locator, actor_id, "ns", actor, facets).await?;
let actor_ref = spawn_with_storage(&ctx, service_locator, actor_id, "ns", actor, storage).await?;
```

### Message Helpers

```rust
use plexspaces_sdk::{call_message, cast_message, json};

// Request-reply
let request = call_message(json!({ "action": "get_balance" }));
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;

// Fire-and-forget
let event = cast_message(json!({ "event": "user_login" }));
actor_ref.tell(event).await?;
```

**Forbidden patterns** (do not use, do not document):
- `ActorFactory` directly for spawning
- Manual `Message { id: ..., message_type: ..., ... }` construction
- Implementing `Actor` trait manually when SDK annotations exist
- Referencing `ActorTrait` or `Message::new` in docs/examples

### Polyglot Consistency

SDK behavior must be consistent across Rust, Python, TypeScript, and Go. Proto data models are compiled to Python/Go/TypeScript for polyglot type consistency. Same semantics and naming where the abstraction maps across languages.

---

## 9. Tenant Isolation (Security Requirement)

Every operation must be scoped to a tenant. This is a hard security requirement, not optional.

```rust
// FORBIDDEN — bypasses tenant isolation
let ctx = RequestContext::internal();

// CORRECT — explicit tenant/namespace
let ctx = RequestContext::new_without_auth("tenant-id".into(), "namespace".into());

// CORRECT — extracted from gRPC request
let ctx = extract_request_context(&request)?;
```

- `tenant_id`: from JWT claim (if auth enabled), or explicit in `new_without_auth`
- `namespace`: from request parameter; WASM app-id populates namespace
- Two-level isolation key: `(tenant_id, namespace, actor_type)` for O(1) lookups
- `RequestContext::internal()` is acceptable ONLY in `Node::new()` / `ServiceLocatorImpl::new()` system initialization; must be documented with a comment explaining why

---

## 10. ID and Naming Conventions

| Rule | Correct | Forbidden |
|---|---|---|
| All IDs | `ulid::Ulid::new().to_string()` | `uuid::Uuid::new_v4()` |
| ActorId construction | `ActorId::build(...)` / `ActorId::parse(...)` | Manual string format/parse |
| Shard group vocabulary | `BroadcastShardGroup`, `ReduceShardGroup` | MPI names in public APIs |
| Debug logging | Behind `tracing::enabled!(Level::DEBUG)` check | Unconditional `eprintln!` |

MPI conceptual mapping is **documentation-only** — never in type/function names.

---

## 11. Storage Factory Pattern

Each storage crate exposes a proto-config-driven factory:

```rust
// Example: keyvalue factory
KeyValueStoreImpl::from_config(&SharedDbConfig) -> Arc<dyn KeyValueStore>
```

`initialize_services_impl` in `services` is a thin orchestrator that calls these factories and registers the trait objects into `ServiceLocatorImpl`. No environment variable reading in service initialization code — `ConfigManager` owns all env-var binding.

Backends by environment:

| Environment | KV/Locks/Journal | TupleSpace | ObjectRegistry |
|---|---|---|---|
| Tests | SQLite in-memory | SQLite in-memory | SQLite in-memory |
| Single-node | SQLite embedded | SQLite embedded | SQLite embedded |
| Production | PostgreSQL | Redis | PostgreSQL |
| Cloud | DynamoDB | Redis | DynamoDB |

---

## 12. Observability

**Metrics**: `metrics::counter!`/`histogram!`/`gauge!` calls use the `metrics` crate facade (no-op without recorder). Phase 1 installed `metrics-exporter-prometheus` recorder. `MetricsService` gRPC exposes `export_prometheus()`. `ServiceLocator::get_metrics_prometheus_renderer()` for in-process Prometheus exposition.

**Logging**: All debug logs must be gated. Use structured logging via `tracing` crate. No `eprintln!` in committed code. No temporary debug logs.

**Tracing**: `request_id` (ULID) propagates across the entire workspace for end-to-end correlation. `grpc-middleware` handles distributed tracing instrumentation.

**Actor-level metrics**: `ActorMetrics` (unified, replaces old per-crate `lazy_static!` prometheus metrics). Labels scoped to `(tenant_id, namespace, actor_type)` — never per-instance, which would cause cardinality explosion at scale.

---

## 13. Parallel / Collective Operations

Pure parallel helpers (reduction operations, stats computation, collective message construction, result conversion) live in `crates/actor/src/parallel.rs`. This module has no service state, no gRPC dependencies, and is independently testable.

Services (`crates/services/src/actor_service/`) are thin controllers: handle gRPC request/response, request context extraction, shard group state (`RwLock`), and parallel fan-out orchestration. They delegate all pure computation to `plexspaces_actor::parallel`.

Built-in reductions only: `CollectiveReduction` enum (SUM, PRODUCT, MAX, MIN, BOOL_AND, BOOL_OR, CONCAT). Do not introduce arbitrary user-defined reducer functions over RPC.

`BarrierShardGroup` is built on broadcast semantics with round tracking. Do not use or revive tuplespace barrier APIs.

---

## 14. Additional Runtime Primitives

### Process Groups (Erlang pg2)

`plexspaces-actor` (via `process_groups` proto service) provides distributed pub/sub group membership. Actors join/leave named groups; messages are broadcast to all members. Implemented in `crates/actor/` with multi-backend storage. Used for distributed event broadcasting without explicit routing.

### Elastic Pool

`crates/elastic-pool/` — auto-scaling pool of stateless worker actors. Checkout/checkin semantics. Backed by `channel` + `actor`. Exposed via `ElasticPool` gRPC service and `host-pool.wit` to WASM actors. Use for CPU-bound work fans-out across a bounded set of workers.

### Application Spec / ChildSpec

`ApplicationSpec` (proto: `application/application.proto`) is the deploy unit — a set of `ChildSpec` entries describing actors to supervise. `crates/application/` translates each `ChildSpec` into an `ActorSpawnSpec` at spawn time. The Erlang OTP application callback pattern: `start → init_children → supervisor_tree_up`.

---

## 16. gRPC Services Architecture

All gRPC services are thin wrappers. The pattern is:

```
gRPC handler (in services/) → extracts RequestContext → calls business logic (in actor/ or storage crates) → returns proto response
```

Services must not contain business logic. Business logic that is shared across gRPC/local/SDK/WIT must live in the appropriate `crates/` module.

All gRPC clients are created via `GrpcConnectionManager` (in `actor` crate). Direct `tonic` channel construction is forbidden.

Communication surfaces:
- **gRPC + Protobuf** — primary protocol
- **HTTP/REST gateway** — same TCP port as gRPC; Axum router merged in `node` via `GrpcHttpServerBuilder`; REST-only deploy endpoints at 100MB limit (avoids gRPC 5MB ceiling)
- **WebSocket** — `/ws` upgrade; binary protobuf `WsFrame`; for thin nodes and browser clients
- **JWKS endpoint** — `/.well-known/jwks.json` for ES256 public key distribution

---

## 17. Code Quality Rules

### No Duplicate Abstractions

Each type or concern has exactly one canonical definition. Other crates re-export via `pub use`, never re-define. Orphaned `.rs` files not included in any `mod.rs` / `lib.rs` must be deleted immediately.

### Cyclic Dependencies

Resolve by design (traits, layering, new crates) — never with hacks or backdoors. The `service-traits` crate is the established pattern for this. If you encounter a cycle, add a trait to `service-traits` rather than creating a workaround.

### Deep Modules over Shallow Modules

Prefer deep modules (rich functionality behind a narrow interface) over shallow modules (many tiny files with thin wrappers). See https://softengbook.org/articles/deep-modules.

### Testing

- TDD: write the failing test first, then the implementation
- 90%+ branch/line coverage
- No sleeps in tests — use condition variables or other robust synchronization primitives
- No mocking of code we own — test real behavior
- Tests call actual methods and verify side effects
- No empty tests, no unused variables in tests
- Use `test-utils` crate for shared test infrastructure
- Targeted `cargo test -p <crate> --lib` during iteration; `make test` before merge

### Comments

- Explain **why**, not what
- Do not put "production grade" in comments — the design should be production-grade by default
- No refactoring noise: do not add comments like "X moved to Y" or "Z deleted"
- No backward-compatibility shims or comments like "kept for legacy"
- No TODO/FIXME in committed code — every change is complete

---

## 18. Known Active Work Items

From `PROJECT_TRACKER.md` (last updated 2026-09-03):

**Active blockers:**

| Issue | Location | Fix Path |
|---|---|---|
| WASM Store reinstantiation per message (3.5ms) | `wasm-runtime/src/instance.rs:~1966` | Check if wasmtime #8943 is patched; remove `create_fresh_plexspaces_actor_state()` call |
| ~137MB RSS per Python WASM actor | wasmtime Store per actor | `--max-wasm-instances N` cap; long-term: shared CPython instance |
| InstancePool excludes component instances | `wasm-runtime/src/instance_pool.rs:292` | Needs wasmtime `Send` on `component::Instance` |
| Actor overhead ~19KB/actor (target 1-2KB) | Multiple: Tokio stack, mailbox pre-alloc, DashMap | Flamegraph at 100K/200K; reduce mailbox channel capacity; 64KB Tokio task stacks |
| `ActorFactory::spawn_actor()` takes 7 params | actor crate | Migrate to `&ActorSpawnSpec` parameter (~50 mock impls to update) |

**Test counts (last clean build, 2026-09-03):**
plexspaces-actor: 325 | plexspaces-services: 293 | plexspaces-node: 167 | plexspaces-wasm-runtime: 84 | plexspaces-journaling: 182 | plexspaces-common: 110 | plexspaces-application: 79 | plexspaces-workflow: 34 | plexspaces-grpc-middleware: 71

---

## 19. Pre-Commit Checklist

Before considering any work done:

- [ ] `make build` passes with zero errors
- [ ] `make test` passes — all tests green
- [ ] `make test-examples` passes (where applicable)
- [ ] No temporary or debug-only logging left (`eprintln!`, ungated debug statements)
- [ ] No hacks, duplicate abstractions, or unresolved cyclic dependencies
- [ ] New/changed code has tests and meets 90%+ coverage bar
- [ ] Public APIs and non-obvious logic are commented; `docs/*.md` (and related READMEs) are updated
- [ ] No backward-compat, legacy, or dead code introduced; any found in the change set is removed
- [ ] SDK/docs use node-based spawn, annotation-based actors, and canonical message constructors; deprecated patterns (`ActorTrait`, `actor-factory`, `Message::new`) replaced where applicable
- [ ] `PROJECT_TRACKER.md` updated for meaningful feature/bug work
