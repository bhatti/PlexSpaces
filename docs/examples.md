# Examples

PlexSpaces includes comprehensive examples organized by core functionality and complexity. Each example demonstrates specific patterns and use cases.

## Quick Navigation

- [SDK Examples (Recommended)](#sdk-examples): Python and TypeScript SDKs (decorator-based and inheritance-based)
- [Examples by Core Functionality](#examples-by-core-functionality): Actors, Workflows, GenServer, Services
- [Simple Examples](#simple-examples): Basic patterns and concepts
- [Intermediate Examples](#intermediate-examples): Multi-actor coordination
- [Advanced Examples](#advanced-examples): Complex distributed systems
- [Domain Examples](#domain-examples): Real-world applications
- [Framework Comparisons](#framework-comparisons): Side-by-side comparisons

## SDK Examples

The [PlexSpaces SDKs](sdk.md) provide minimal-boilerplate actor development: **Python** (decorator-based: `@actor`, `@handler`, `state()`) and **TypeScript** (inheritance-based: extend `PlexSpacesActor`, `on<Op>()` handlers). Start here for new projects. Each example has a **README.md** with overview, APIs, build, and test.

### Python SDK Examples

| Example | Description | README |
|---------|-------------|--------|
| **Bank Account** | Durable state with transaction log | [README](../examples/python/apps/bank_account/README.md) |
| **Calculator** | Simple math operations | [README](../examples/python/apps/calculator/README.md) |
| **Feature Flags** | Gradual rollout with supervisor | [README](../examples/python/apps/feature_flags/README.md) |
| **FSM** | Order workflow state machine | [README](../examples/python/apps/fsm/README.md) |
| **Receipt Storage** | Expense tracking with filtering | [README](../examples/python/apps/receipt_storage/README.md) |
| **Chat Room** | Real-time chat with ProcessGroups | [README](../examples/python/apps/chat_room/README.md) |
| **Task Queue** | Distributed locks via LockFacet | [README](../examples/python/apps/task-queue/README.md) |
| **Registry** | Service discovery via RegistryFacet | [README](../examples/python/apps/registry/README.md) |
| **Storefront API** ⚠️ | E-commerce backend: store config, shopping cart, checkout rate limit via host KV. **Status: buggy/incomplete** (host KV depends on node `sql-backend`; test may show "config set failed" until fixed). | [README](../examples/python/apps/storefront/README.md) |
| **Audit Log** | Event-handler (EventHandler): fire-and-forget audit events via `host.log` only (storage deferred until WASM stable) | [README](../examples/python/apps/audit_log/README.md) |
| **N-Body** | Multi-actor physics simulation | [README](../examples/python/apps/nbody/README.md) |
| **Leader Election** ⚠️ | Distributed lock for single-leader pattern (cron, scheduler). **Status: buggy/incomplete**. | [README](../examples/python/apps/leader_election/README.md) |

**⚠️ Buggy/incomplete examples**: Storefront API and Leader Election are marked incomplete; see their READMEs and PROJECT_TRACKER.md.

**Features demonstrated:**
- `@actor` and `@event_actor` decorators for actor classes
- `state()` for persistent state fields
- `@handler()` for message routing
- `@init_handler` for custom initialization
- `host.info()` / `host.error()` for logging
- `host.kv_get` / `host.kv_put` for key-value storage (WASM)
- `host.ts_write(tuple_json)` for TupleSpace write (WASM; existing API)
- `host.process_groups` for pub/sub
- Facets: LockFacet, RegistryFacet

**Build and test (Python):**
```bash
cd examples/python/apps/bank_account
./build.sh           # Build to WASM
./test.sh 8092       # Test (server must be running)
```

### TypeScript SDK Examples

| Example | Description | README |
|---------|-------------|--------|
| **Bank Account** | Same API as Python: durable state, deposit/withdraw/history/replay. Uses `@plexspaces/sdk` and jco componentize. | [README](../examples/typescript/apps/bank_account/README.md) |

**Build and test (TypeScript):**
```bash
cd examples/typescript/apps/bank_account
./test.sh            # Full E2E: start node, build WASM, deploy, HTTP ops (no Python)
```

### Go SDK Examples

| Example | Description | README |
|---------|-------------|--------|
| **Migrating Erlang/OTP** | Sliding window rate limiter demonstrating GenServer pattern. Per-client windows, batch benchmarks, configurable via ApplicationSpec. Uses Go SDK: `BaseActor`, `Host`, `ActorRouter`. | [README](../examples/go/apps/migrating_erlang_otp/README.md) |

**Build and test (Go):**
```bash
cd examples/go/apps/migrating_erlang_otp
./build.sh           # Build to WASM with TinyGo
./test.sh 7993       # Test (server must be running)
```

See [SDK Guide](sdk.md) for complete documentation (Python, TypeScript, Go, and Rust).

### Rust SDK Examples

Rust examples use the [Rust SDK](sdk.md#rust-sdk) annotations: `#[actor]`, `#[gen_server_actor]`, `#[handler]`, `#[plexspaces_handlers]`, and `spawn_actor` with facets.

| Example | Description | Annotations Used | README |
|---------|-------------|------------------|--------|
| **webhook_handler** | GenServer (request-reply); HTTP deliver/list webhooks | `#[gen_server_actor]`, `#[plexspaces_handlers]`, `#[handler]` | [embedded](../examples/rust/embedded/webhook_handler/) |
| **session_manager** | Custom actor + TimerFacet (idle timeout, heartbeat) | `#[actor]`, `#[plexspaces_handlers(custom)]`, `#[handler]` | [apps](../examples/rust/apps/session_manager/README.md) |
| **timers** | In-memory timers with TimerFacet (session management, idle timeout, heartbeat, retry) | `#[actor]`, `#[plexspaces_handlers(custom)]`, `#[handler]`, `spawn_with_facets()`, `CoordinationComputeTracker` | [embedded](../examples/rust/embedded/timers/README.md) |
| **reminders** | Durable reminders with ReminderFacet | `#[actor]`, `#[plexspaces_handlers(custom)]`, `#[handler]` | [embedded](../examples/rust/embedded/reminders/README.md) |
| **bank_account** | Durable bank account with journaling (banking use case) | `#[gen_server_actor(facets = ["durability"])]`, `#[plexspaces_handlers]`, `#[handler]`, `spawn_with_storage()` | [embedded](../examples/rust/embedded/bank_account/README.md) |
| **heat_diffusion** | Thermal simulation with TupleSpace coordination (stencil computation, ghost cell exchange, barrier sync) | `#[gen_server_actor]`, `#[plexspaces_handlers]`, `#[handler]`, `spawn()`, `GenServerRef.call()`, `ActorContext::get_tuplespace()` | [embedded](../examples/rust/embedded/heat_diffusion/README.md) |
| **matrix_multiply** | Parallel matrix multiplication with scatter-gather pattern (master-worker, scientific computing) | `#[gen_server_actor]`, `#[plexspaces_handlers]`, `#[handler]`, `spawn()`, `GenServerRef.call()` | [embedded](../examples/rust/embedded/matrix_multiply/README.md) |
| **chat_room** | Process groups for broadcast messaging (pub/sub, Slack/Discord-style) | `ProcessGroupRegistry`, `RequestContext`, `CoordinationComputeTracker` | [embedded](../examples/rust/embedded/chat_room/README.md) |
| **timeseries_forecasting** | Pipeline via ActorFactory (type-name spawn); see README for Factory vs SDK | ActorFactory pattern | [embedded](../examples/rust/embedded/timeseries_forecasting/README.md) |
| **firecracker_multi_tenant** | Data-parallel actors (DPA-inspired) with worker pools, **health-aware node connectivity** (liveness/readiness checks, exponential backoff retry), resource-based routing, coordination/compute metrics | `ParallelClient`, `UnifiedShardGroupClient`, `NodeClient` (with health checks) | [embedded](../examples/rust/embedded/firecracker_multi_tenant/README.md) |

**Annotations Quick Reference**:
| Annotation | Description |
|------------|-------------|
| `#[gen_server_actor]` | GenServer behavior (request-reply, call by default) |
| `#[actor(facets = ["timer"])]` | Custom behavior with facets declaration |
| `#[plexspaces_handlers]` | Generates GenServer dispatch from `#[handler]` methods |
| `#[plexspaces_handlers(custom)]` | Generates Actor dispatch for custom behaviors |
| `#[handler("op")]` | Route message to handler (GenServer defaults to call) |
| `#[handler("op", cast)]` | Fire-and-forget handler (no reply) |

**Conventions**: 
- ✅ **Use SDK patterns** (`spawn`, `spawn_with_facets`, `spawn_with_storage`, `call_message`, `cast_message`) for examples and user code
- ✅ **Use SDK annotations** (`#[gen_server_actor]`, `#[handler]`, `#[plexspaces_handlers]`) for actor definitions
- ⚠️ **ActorFactory** is for framework code only (e.g., `ActorServiceImpl` internal implementation) or pipeline-style multi-actor setups (see `timeseries_forecasting` example)
- ✅ GenServer uses **call by default** (like Python)

**Note**: For examples, prefer SDK patterns over low-level APIs. SDK removes boilerplate and provides better developer experience.

## Examples by Core Functionality

### Actors

**Basic Actor Pattern** (Fire-and-Forget):
- `examples/rust/embedded/timers/` - Timer-based actors
- `examples/rust/embedded/wasm_calculator/actors/python/calculator_actor.py` - Basic message handling
- `examples/typescript/apps/bank_account/account_actor.ts` - TypeScript bank account via [@plexspaces/sdk](sdk.md#typescript-sdk) (durable state, same API as Python)

**GenServer Pattern** (Request-Reply):
- `examples/simple/wasm_calculator/actors/python/calculator_actor.py` - GenServer behavior with request-reply
- `examples/domains/order-processing/src/actors/order_processor.rs` - Rust GenServer implementation
- `examples/rust/embedded/webhook_handler/` - Webhook handler (FaaS-style HTTP deliver/list)

**Durable Actors** (State Persistence):
- `examples/python/apps/bank_account/` - Python WASM durable bank account (SDK)
- `examples/rust/embedded/bank_account/` - Rust durable bank account with journaling (SDK)
- `examples/simple/wasm_calculator/actors/python/durable_calculator_actor.py` - Python durable actor
- `examples/simple/durable_actor_example/` - Complete durability example with journaling, checkpoints, replay
- `examples/domains/genomics-pipeline/src/workers/` - Durable workers with state recovery

**Supervision Trees**:
- `examples/simple/supervision_tree/` - Supervisor hierarchies and restart policies
- `examples/domains/genomics-pipeline/` - Multi-level supervision with worker pools
- `examples/domains/order-processing/` - Service supervision with circuit breakers

### Workflows

**Durable Workflows**:
- `examples/domains/genomics-pipeline/` - Multi-step DNA sequencing workflow
  - **Steps**: Quality Control → Alignment → Variant Calling → Annotation → Report
  - **Features**: Workflow orchestration, activity scheduling, state recovery, checkpointing
  - **See**: `src/coordinator.rs` for workflow coordinator implementation
- `examples/domains/order-processing/` - Order processing workflow
  - **Steps**: Order creation → Payment → Inventory → Shipping
  - **Features**: State machines, workflow coordination, event-driven architecture
  - **See**: `src/actors/order_processor.rs` for workflow implementation

**Workflow Patterns**:
- Multi-step orchestration with recovery
- Activity scheduling and coordination
- State machine transitions
- Long-running process management

**Documentation**: See [Polyglot WASM Development Guide](polyglot.md#5-workflow-orchestration) for workflow examples in Python/TypeScript

### Services

**TupleSpace Coordination**:
- `examples/simple/wasm_calculator/actors/python/tuplespace_calculator_actor.py` - TupleSpace for result sharing
- `examples/TUPLESPACE_COORDINATION.md` - Comprehensive TupleSpace examples and patterns
- `examples/rust/embedded/heat_diffusion/` - Thermal simulation with TupleSpace coordination (stencil computation, ghost cell exchange, barrier sync)

**Key-Value Store**:
- `examples/wasm_showcase/` - Key-value operations via WIT host functions
- `examples/simple/durable_actor_example/` - State storage with KV store
- `examples/domains/order-processing/` - Order state management

**Blob Storage**:
- `examples/wasm_showcase/` - Blob upload/download operations
- `examples/rust/embedded/webhook_handler/` - Webhook handler (HTTP deliver/list)

**Channels (Queues & Pub/Sub)**:
- `examples/simple/process_groups_pubsub/` - Process groups with pub/sub messaging
- `examples/domains/order-processing/` - Event-driven architecture with channels
- `examples/simple/durable_actor_example/` - Channel-based mailboxes with ACK/NACK

**Process Groups**:
- `examples/simple/process_groups_pubsub/` - Actor clustering and group messaging
- `examples/rust/embedded/event_analytics/` - Distributed event analytics with shard groups (hash-based routing)

**Distributed Locks (Task Queue)**:
- `examples/python/apps/task-queue/` - Distributed task queue with LockFacet
  - **Features**: Job submission, worker claiming, lock renewal (heartbeat), fault tolerance
  - **Use Cases**: Background job processing, scheduled tasks, distributed data processing
  - **See**: [Task Queue Example](../examples/python/apps/task-queue/) for complete implementation

**Registry (Service Discovery)**:
- `examples/wasm_showcase/` - Service registration and discovery
- `examples/node_discovery.rs` - Node discovery and registration

### Polyglot Examples

**Python WASM**:
- `examples/simple/wasm_calculator/actors/python/` - Complete Python actor examples
  - `calculator_actor.py` - GenServer pattern
  - `durable_calculator_actor.py` - Durable actor with state persistence
  - `tuplespace_calculator_actor.py` - TupleSpace coordination
  - `channel_calculator_actor.py` - Channel-based communication

**TypeScript/JavaScript WASM**:
- `examples/typescript/apps/bank_account/` - TypeScript bank account (SDK + jco componentize, simple-actor WIT, full E2E)
- `examples/advanced/nbody-wasm/ts-actors/` - TypeScript physics simulation (if present)

**Rust WASM**:
- `examples/advanced/nbody-wasm/wasm-actors/` - Rust physics simulation
- Other Rust WASM examples under `examples/`

**Multi-Language Deployment**:
- `examples/wasm_showcase/` - Comprehensive WASM capabilities across languages
- Python: `examples/python/apps/*`; TypeScript: `examples/typescript/apps/bank_account`

**Documentation**: See [Polyglot WASM Development Guide](polyglot.md) for complete language-specific guides and examples

## Simple Examples

### Timers Example

**Location**: `examples/simple/timers_example/`

Learn how to use timers in actors for scheduled tasks and periodic operations.

**Features**:
- Scheduled tasks
- Periodic operations
- Timer cancellation

**Run**:
```bash
cd examples/simple/timers_example
cargo run
```

### Durable Actor Example

**Location**: `examples/simple/durable_actor_example/`

Learn durability and journaling with automatic state persistence and recovery. This example demonstrates all durability features including journaling, checkpoints, deterministic replay, side effect caching, channel-based mailboxes, and dead letter queues.

**Features**:
- State persistence and journaling
- Automatic recovery and replay
- Checkpointing for fast recovery
- Side effect caching (exactly-once semantics)
- Channel-based mailbox with ACK/NACK
- Dead letter queue (DLQ) for poisonous messages
- Edge case handling and failure scenarios

**Run**:
```bash
cd examples/simple/durable_actor_example
cargo run --features sqlite-backend
```

**Documentation**: For comprehensive durability documentation, see [Durability Documentation](durability.md).

### WASM Calculator

**Location**: `examples/simple/wasm_calculator/`

Learn WASM actor deployment with polyglot support (Python/JavaScript). Demonstrates multiple core patterns:

**Core Patterns Demonstrated**:
- ✅ **GenServer Pattern**: Request-reply in `calculator_actor.py`
- ✅ **Durable Actors**: State persistence in `durable_calculator_actor.py`
- ✅ **TupleSpace Coordination**: Distributed coordination in `tuplespace_calculator_actor.py`
- ✅ **Channel Communication**: Queue-based messaging in `channel_calculator_actor.py`

**Features**:
- Python actors with multiple behavior patterns
- WASM deployment via HTTP multipart upload
- State persistence and recovery
- Distributed coordination

**Run**:
```bash
cd examples/simple/wasm_calculator
./test.sh
```

**Key Files**:
- `actors/python/calculator_actor.py` - GenServer behavior (request-reply)
- `actors/python/durable_calculator_actor.py` - Durable actor with journaling
- `actors/python/tuplespace_calculator_actor.py` - TupleSpace coordination
- `actors/python/channel_calculator_actor.py` - Channel-based communication

**Documentation**: 
- [WASM Deployment Guide](wasm-deployment.md) - Deployment instructions and API reference
- [Polyglot WASM Development Guide](polyglot.md) - Comprehensive guide for polyglot development with all WIT abstractions and language-specific examples

### Event Analytics (Shard Groups)

**Location**: `examples/rust/embedded/event_analytics/`

**Real-World Use Case**: Web analytics/event tracking system (Google Analytics, Mixpanel-style) that tracks page views, clicks, and conversions across multiple shards for horizontal scaling.

**Features**:
- Shard Groups - Data-parallel horizontal scaling via hash-based sharding
- Hash-based routing - Partition key (user_id) → hash → shard_id
- Scatter-gather queries - Aggregate metrics across all shards
- SDK patterns - `#[gen_server_actor]`, `spawn_gen_server()`, `GenServerRef.cast()`/`call()`
- Performance metrics - Coordination vs computation analysis, throughput metrics

**Run**:
```bash
cd examples/rust/embedded/event_analytics
cargo run
```

**See Also**:
- [Architecture: Data-Parallel Actors](../docs/architecture.md#data-parallel-actors-shardgroup)
- [SDK: Unified ShardGroup Client](../docs/sdk.md#unified-shardgroup-client-data-parallel-actors)

### Webhook Handler (HTTP-Based Actor)

**Location**: `examples/rust/embedded/webhook_handler/`

Webhook handler actor invoked via HTTP: POST to deliver webhooks, GET with `?action=list` to list recent deliveries. Uses explicit tenant/namespace and correct PlexSpaces APIs.

**Features**:
- HTTP-based actor invocation (GET list, POST deliver)
- Actor type `webhook_handler` for path routing
- RequestContext with explicit tenant/namespace (no internal)
- ActorBuilder::spawn with GenServer + Custom behavior type

**Run**:
```bash
cd examples/rust/embedded/webhook_handler
./test.sh
```

**See Also**: 
- [Concepts: FaaS-Style Invocation](../docs/concepts.md#faas-style-invocation) - Core concepts
- [Architecture: FaaS Invocation](../docs/architecture.md#faas-invocation) - System design
- [Detailed Design: InvokeActor Service](../docs/detailed-design.md#invokeactor-service) - Implementation details

## Intermediate Examples

### Matrix Multiply

**Location**: `examples/intermediate/matrix_multiply/`

Multi-actor computation with task distribution.

**Features**:
- Distributed computation
- Task distribution
- Result aggregation

**Run**:
```bash
cd examples/intermediate/matrix_multiply
cargo run
```

### Heat Diffusion

**Location**: `examples/rust/embedded/heat_diffusion/`

Thermal simulation with TupleSpace coordination demonstrating parallel stencil computation:
- **Ghost Cell Exchange**: Actors exchange boundary values via TupleSpace
- **Barrier Synchronization**: All regions synchronize before next iteration
- **SDK Patterns**: Uses `#[gen_server_actor]`, `spawn()`, `GenServerRef.call()`
- **Metrics**: CoordinationComputeTracker for coordination vs computation analysis

**Features**:
- Multi-actor simulation
- Region-based coordination
- State synchronization

**Run**:
```bash
cd examples/rust/embedded/heat_diffusion
cargo run --bin heat_diffusion
```

### Matrix Vector MPI

**Location**: `examples/intermediate/matrix_vector_mpi/`

MPI-style coordination with scatter-gather patterns.

**Features**:
- MPI patterns
- Scatter-gather
- Collective operations

**Run**:
```bash
cd examples/intermediate/matrix_vector_mpi
cargo run
```

## Advanced Examples

### Byzantine Generals

**Location**: `examples/advanced/byzantine/`

Byzantine fault tolerance with consensus algorithms.

**Features**:
- Byzantine fault tolerance
- Consensus algorithms
- Fault injection

**Run**:
```bash
cd examples/advanced/byzantine
cargo run
```

### N-Body Simulation

**Location**: `examples/advanced/nbody/`

Complex physics simulation with multi-actor coordination.

**Features**:
- Physics simulation
- Multi-actor coordination
- Performance optimization

**Run**:
```bash
cd examples/advanced/nbody
cargo run
```

## Domain Examples

### Genomics Pipeline

**Location**: `examples/domains/genomics-pipeline/`

Complete DNA sequencing analysis pipeline demonstrating **durable workflow orchestration**.

**Core Functionality**:
- ✅ **Durable Workflows**: Multi-step workflow with state recovery
- ✅ **GenServer Actors**: Request-reply pattern for workflow coordination
- ✅ **Supervision Trees**: Multi-level supervision with worker pools
- ✅ **State Persistence**: SQLite-backed journaling for crash recovery

**Workflow Steps**:
1. Quality Control (QC Worker Pool)
2. Genome Alignment (Alignment Worker Pool)
3. Variant Calling (24 Chromosome Workers - fan-out/fan-in)
4. Annotation (Annotation Worker)
5. Report Generation (Report Worker)

**Features**:
- Multi-step workflow orchestration
- Activity scheduling and coordination
- State recovery and checkpointing
- Exactly-once semantics
- Side effect caching
- Multi-node distributed deployment

**Run**:
```bash
cd examples/domains/genomics-pipeline
cargo run
```

**Key Files**:
- `src/coordinator.rs` - Workflow coordinator with durable orchestration
- `src/workers/` - Worker actors (QC, Alignment, Chromosome, Annotation, Report)
- `src/application.rs` - Application with supervision trees

**Documentation**: See [Polyglot WASM Development Guide](polyglot.md#5-workflow-orchestration) for workflow patterns

### Order Processing

**Location**: `examples/domains/order-processing/`

Complete order processing microservices architecture demonstrating **hybrid behavior patterns**.

**Core Functionality**:
- ✅ **GenServer Behavior**: Request-reply for queries (`GetOrder`, `GetStatus`)
- ✅ **Actor Behavior**: Fire-and-forget for commands (`CreateOrder`, `CancelOrder`)
- ✅ **State Machine**: Order state transitions (Pending → Payment → Reserved → Shipped)
- ✅ **Event-Driven**: Distributed pub/sub via channels
- ✅ **Service Coordination**: Payment, Inventory, Shipping services

**Architecture**:
```
RootSupervisor (OneForOne)
  ├─ ServiceSupervisor (OneForOne)
  │   ├─ PaymentServiceActor (Permanent, CircuitBreaker)
  │   ├─ InventoryServiceActor (Permanent)
  │   └─ ShippingServiceActor (Permanent, CircuitBreaker)
  └─ OrderProcessorActor (Permanent)
```

**Features**:
- Hybrid behavior patterns (GenServer + Actor + StateMachine)
- Event-driven architecture
- Service coordination
- Circuit breakers for fault tolerance
- Complete microservices example

**Run**:
```bash
cd examples/domains/order-processing
cargo run
```

**Key Files**:
- `src/actors/order_processor.rs` - Order processor with hybrid behaviors
- `src/actors/services/` - Service actors (Payment, Inventory, Shipping)
- `src/application.rs` - Application with supervision trees

**Documentation**: See [Polyglot WASM Development Guide](polyglot.md#2-genserver-request-reply-pattern) for GenServer examples

### Finance Risk

**Location**: `examples/domains/finance-risk/`

Financial risk analysis with complex workflows.

**Features**:
- Risk analysis
- Complex workflows
- Real-time processing

**Run**:
```bash
cd examples/domains/finance-risk
cargo run
```

## Framework Comparisons

Side-by-side comparisons with 24+ frameworks. Each comparison includes:

1. **Native Framework Implementation**: Original framework code
2. **PlexSpaces Implementation**: Equivalent PlexSpaces code
3. **Benchmarks**: Performance comparison
4. **Documentation**: Detailed analysis

### Available Comparisons

| Framework | Use Case | Location |
|-----------|----------|----------|
| Erlang/OTP | GenServer with supervision | `examples/comparison/erlang_otp/` |
| Temporal | Durable workflows | `examples/comparison/temporal/` |
| Ray | Distributed ML workloads | `examples/comparison/ray/` |
| Cloudflare Workers | Durable Objects | `examples/comparison/cloudflare_workers/` |
| Orleans | Virtual actors | `examples/comparison/orleans/` |
| Restate | Durable execution | `examples/comparison/restate/` |
| Azure Durable Functions | Serverless workflows | `examples/comparison/azure_durable_functions/` |
| AWS Step Functions | State machines | `examples/comparison/aws_step_functions/` |
| wasmCloud | WASM components | `examples/comparison/wasmcloud/` |
| Dapr | Unified workflows | `examples/comparison/dapr/` |

See [Framework Comparisons](../examples/comparison/README.md) for the complete list.

## Example Structure

Each example includes:

- **README.md**: Documentation and instructions
- **src/**: Source code
- **scripts/**: Helper scripts
- **tests/**: Integration tests (if applicable)

## Running Examples

### Single Example

```bash
cd examples/simple/timers_example
cargo run
```

### All Examples

```bash
# Run all examples
make test-examples

# Run specific category
cd examples/simple
for dir in */; do
    cd "$dir" && cargo run && cd ..
done
```

## Contributing Examples

When adding new examples:

1. Place in appropriate complexity directory
2. Add README.md with:
   - Purpose
   - Prerequisites
   - Running instructions
   - Key concepts
3. Update this document
4. Add tests if applicable

## Next Steps

- [Getting Started](getting-started.md): Learn the basics
- [Concepts Guide](concepts.md): Understand core concepts
- [Architecture](architecture.md): Understand the system design
- [Use Cases](use-cases.md): Explore real-world applications
