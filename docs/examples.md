# Examples

PlexSpaces includes comprehensive examples organized by core functionality and complexity. Each example demonstrates specific patterns and use cases.

## Quick Navigation

- [Examples by Core Functionality](#examples-by-core-functionality): Actors, Workflows, GenServer, Services
- [Simple Examples](#simple-examples): Basic patterns and concepts
- [Intermediate Examples](#intermediate-examples): Multi-actor coordination
- [Advanced Examples](#advanced-examples): Complex distributed systems
- [Domain Examples](#domain-examples): Real-world applications
- [Framework Comparisons](#framework-comparisons): Side-by-side comparisons

## Examples by Core Functionality

### Actors

**Basic Actor Pattern** (Fire-and-Forget):
- `examples/simple/timers_example/` - Timer-based actors
- `examples/simple/wasm_calculator/actors/python/calculator_actor.py` - Basic message handling
- `examples/simple/polyglot_wasm_deployment/actors/typescript/greeter.ts` - TypeScript actor

**GenServer Pattern** (Request-Reply):
- `examples/simple/wasm_calculator/actors/python/calculator_actor.py` - GenServer behavior with request-reply
- `examples/domains/order-processing/src/actors/order_processor.rs` - Rust GenServer implementation
- `examples/simple/faas_actor/` - FaaS-style request-reply

**Durable Actors** (State Persistence):
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
- `examples/intermediate/heat_diffusion/` - Region-based coordination via TupleSpace

**Key-Value Store**:
- `examples/wasm_showcase/` - Key-value operations via WIT host functions
- `examples/simple/durable_actor_example/` - State storage with KV store
- `examples/domains/order-processing/` - Order state management

**Blob Storage**:
- `examples/wasm_showcase/` - Blob upload/download operations
- `examples/simple/faas_actor/` - FaaS-style file handling

**Channels (Queues & Pub/Sub)**:
- `examples/simple/process_groups_pubsub/` - Process groups with pub/sub messaging
- `examples/domains/order-processing/` - Event-driven architecture with channels
- `examples/simple/durable_actor_example/` - Channel-based mailboxes with ACK/NACK

**Process Groups**:
- `examples/simple/process_groups_pubsub/` - Actor clustering and group messaging
- `examples/simple/actor_groups_sharding/` - Hash-based routing and load distribution

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
- `examples/simple/polyglot_wasm_deployment/actors/typescript/greeter.ts` - TypeScript actor
- `examples/advanced/nbody-wasm/ts-actors/` - TypeScript physics simulation

**Rust WASM**:
- `examples/simple/polyglot_wasm_deployment/actors/rust/` - Rust WASM actors
- `examples/advanced/nbody-wasm/wasm-actors/` - Rust physics simulation

**Multi-Language Deployment**:
- `examples/simple/polyglot_wasm_deployment/` - Deploy actors from multiple languages
- `examples/wasm_showcase/` - Comprehensive WASM capabilities across languages

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

### Actor Groups (Sharding)

**Location**: `examples/simple/actor_groups_sharding/`

Learn actor groups for horizontal scaling with hash-based routing.

**Features**:
- Actor groups
- Hash-based routing
- Load distribution

**Run**:
```bash
cd examples/simple/actor_groups_sharding
cargo run
```

### FaaS Actor (HTTP-Based Invocation)

**Location**: `examples/simple/faas_actor/`

Learn FaaS-style actor invocation via HTTP GET/POST requests. Demonstrates serverless patterns and AWS Lambda integration.

**Features**:
- HTTP-based actor invocation (GET/POST)
- Actor lookup by type
- Load balancing across actor instances
- Multi-tenant isolation
- Path and subpath routing support
- AWS Lambda Function URL ready

**Run**:
```bash
cd examples/simple/faas_actor
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

**Location**: `examples/intermediate/heat_diffusion/`

Multi-actor simulation with region-based coordination.

**Features**:
- Multi-actor simulation
- Region-based coordination
- State synchronization

**Run**:
```bash
cd examples/intermediate/heat_diffusion
cargo run
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
