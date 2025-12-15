# PlexSpaces Framework Comparisons

This directory contains side-by-side comparisons between PlexSpaces and other actor/workflow/dataflow frameworks. Each comparison demonstrates how to implement the same use case in both the native framework and PlexSpaces, with benchmarks and metrics.

---

## Overview

PlexSpaces integrates patterns from multiple distributed systems frameworks, building upon the foundation of **JavaNow**, a comprehensive parallel computing framework that I developed for post-graduate research in the late 1990s. JavaNow pioneered tuple spaces (EntitySpaces), mobile agents, dynamic capabilities (facets), and distributed coordination patterns that are now central to PlexSpaces.

PlexSpaces integrates patterns from:

- **JavaNow** (Original Inspiration) - Tuple spaces, mobile agents, dynamic capabilities, distributed coordination
- **Erlang/OTP** - Supervision trees, behaviors, fault tolerance
- **Akka** - Actor model, message passing
- **Microsoft Orleans** - Virtual actors, timers, reminders
- **Temporal** - Durable workflows, activity patterns
- **Restate** - Durable execution, deterministic replay
- **Ray** - Distributed ML workloads, resource scheduling
- **Azure Durable Functions** - Serverless workflows
- **AWS Step Functions** - State machine workflows
- **Cloudflare Workers** - Edge computing, Durable Objects
- **wasmCloud** - WASM components, capability providers
- **Swift Actors** - Compiler-enforced isolation (removed from comparison)
- **Orbit Cloud** - Virtual actor lifecycle
- **Voyager** - Mobile agents, facets
- **Merlin** - HPC workflow orchestration for scientific simulations
- **eFlows4HPC** - Unified workflow platform (HPC + Big Data + ML)
- **MozartSpaces** - XVSM (eXtended Virtual Shared Memory) with coordinator objects
- **LuaTS** - Linda + Event-driven programming
- **Cadence** - Workflow orchestration (Temporal's predecessor)
- **Netflix Conductor/Orkes** - JSON-based workflow DSL for microservices
- **Zeebe** - BPMN workflow engine with event sourcing
- **Dirigo** - Distributed stream processing with virtual actors

---

## Comparison Structure

Each comparison includes:

1. **Native Framework Implementation** - Sample code showing how to implement the use case in the original framework
2. **PlexSpaces Implementation** - Fully functional Rust implementation using PlexSpaces
3. **README.md** - Detailed documentation, architecture, and usage
4. **Scripts** - Build, test, and benchmark scripts
5. **Tests** - Unit and integration tests
6. **Metrics** - Performance benchmarks and overhead analysis
7. **Benchmarks** - Comparative performance data

---

## Available Comparisons

### Actor Frameworks

| Framework | Use Case | Abstractions | Status | Directory |
|-----------|----------|--------------|--------|-----------|
| **Erlang/OTP** | GenServer with supervision | GenServerBehavior, Supervision | ✅ Complete | `erlang_otp/` |
| **Cloudflare Workers** | Durable Objects pattern | VirtualActorFacet, GenServerBehavior | ✅ Complete | `cloudflare_workers/` |
| **Orleans** | Virtual actor with timers | VirtualActorFacet, TimerFacet, ReminderFacet | ✅ Complete | `orleans/` |
| **Rivet** | Cloudflare Durable Objects | VirtualActorFacet, DurabilityFacet | ✅ Complete | `rivet/` |
| **Swift Actors** | Actor isolation patterns | Actor isolation | 🚧 Planned | `swift_actors/` |
| **Orbit Cloud** | Virtual actor lifecycle | VirtualActorFacet | ✅ Complete | `orbit/` |
| **Gosiris** | Go actor framework | GenServerBehavior, Message passing | ✅ Complete | `gosiris/` |
| **Ractor** | Rust actor framework | GenServerBehavior, Rust-native actors | ✅ Complete | `ractor/` |
| **Dirigo** | Virtual actors for stream processing | GenServerBehavior, DurabilityFacet | ✅ Complete | `dirigo/` |

### Workflow Frameworks

| Framework | Use Case | Abstractions | Status | Directory |
|-----------|----------|--------------|--------|-----------|
| **Temporal** | Durable workflow with activities | WorkflowBehavior, DurabilityFacet | ✅ Complete | `temporal/` |
| **Restate** | Durable execution with journaling | DurabilityFacet, Journaling | ✅ Complete | `restate/` |
| **Azure Durable Functions** | Serverless workflow orchestration | WorkflowBehavior, DurabilityFacet | ✅ Complete | `azure_durable_functions/` |
| **AWS Step Functions** | State machine workflow | WorkflowBehavior, State Machine Pattern | ✅ Complete | `aws_step_functions/` |
| **AWS Durable Lambda** | Durable execution patterns | DurabilityFacet, Idempotency | ✅ Complete | `aws_durable_lambda/` |
| **Cadence** | Workflow orchestration (Temporal predecessor) | WorkflowBehavior, DurabilityFacet | ✅ Complete | `cadence/` |
| **Netflix Conductor/Orkes** | JSON-based workflow DSL | WorkflowBehavior, DurabilityFacet | ✅ Complete | `conductor/` |
| **Zeebe** | BPMN workflow engine with event sourcing | WorkflowBehavior, DurabilityFacet | ✅ Complete | `zeebe/` |
| **Merlin** | HPC workflow orchestration | WorkflowBehavior, DurabilityFacet | ✅ Complete | `merlin/` |
| **eFlows4HPC** | Unified workflow platform (HPC + Big Data + ML) | WorkflowBehavior, DurabilityFacet | ✅ Complete | `eflows4hpc/` |

### Dataflow/ML Frameworks

| Framework | Use Case | Abstractions | Status | Directory |
|-----------|----------|--------------|--------|-----------|
| **Ray** | Parameter Server (Distributed ML training) | GenServerBehavior, Elastic worker pools | ✅ Complete | `ray/` |
| **Kueue** | ML Training Job Queue with Elastic Resource Pools | GenServerBehavior, Resource scheduling | ✅ Complete | `kueue/` |
| **SkyPilot** | Multi-cloud AI workload orchestration | GenServerBehavior, Multi-cloud scheduling | ✅ Complete | `skypilot/` |

### Edge/Serverless Frameworks

| Framework | Use Case | Abstractions | Status | Directory |
|-----------|----------|--------------|--------|-----------|
| **wasmCloud** | WASM component deployment | GenServerBehavior, Capability providers | ✅ Complete | `wasmcloud/` |
| **Dapr** | Unified durable workflows | WorkflowBehavior, DurabilityFacet | ✅ Complete | `dapr/` |

### TupleSpace/Coordination Frameworks

| Framework | Use Case | Abstractions | Status | Directory |
|-----------|----------|--------------|--------|-----------|
| **MozartSpaces** | XVSM (eXtended Virtual Shared Memory) | TupleSpace, Coordinator objects (FIFO, LIFO, Label) | ✅ Complete | `mozartspaces/` |
| **LuaTS** | Linda + Event-driven programming | TupleSpace, GenEventBehavior | ✅ Complete | `luats/` |

**Legend**:
- ✅ Complete - Full implementation with native code + PlexSpaces + tests + documentation + test scripts
- 📝 README - Documentation complete, needs Rust implementation
- 🚧 Planned - Not yet started

**Note**: All complete examples include:
- Native framework code in `native/` directory (TypeScript/Python/Java/Go/Rust/JSON/YAML)
- Fully functional PlexSpaces Rust implementation in `src/main.rs`
- Comprehensive test script in `scripts/test.sh`
- Complete README with side-by-side comparison

**Status**: ✅ **All 24 Examples Complete** (2025-12-10)
- All examples have native code, test scripts, and documentation
- Fixed stalling issues (removed `get_or_activate_actor` where causing problems)
- All READMEs updated to reference native code files
- See `docs/FRAMEWORK_COMPARISON_EXAMPLES_PLAN.md` for detailed plan

## Completed Examples Summary

### ✅ Erlang/OTP (`erlang_otp/`)
- **Status**: Fully functional and tested
- **Abstractions**: GenServerBehavior, Supervision Trees, Actor Model, Message Passing
- **Features**: Counter with increment/decrement/get operations
- **Documentation**: Complete with native Erlang code, PlexSpaces implementation, design decisions, and benchmarks

### ✅ Temporal (`temporal/`)
- **Status**: Fully functional and tested
- **Abstractions**: WorkflowBehavior, DurabilityFacet, Activity Patterns, Retry Policies
- **Features**: Order processing workflow (validate → payment → ship → confirm)
- **Documentation**: Complete with native TypeScript code, PlexSpaces implementation, design decisions, and benchmarks

### ✅ Ray (`ray/`)
- **Status**: Fully functional and tested
- **Abstractions**: Actor Coordination, TupleSpace, Resource Scheduling, Distributed Computation
- **Features**: Parallel document processing with entity recognition and result aggregation
- **Documentation**: Complete with native Python code, PlexSpaces implementation, design decisions, and benchmarks

### ✅ Cloudflare Workers (`cloudflare_workers/`)
- **Status**: Fully functional and tested
- **Abstractions**: VirtualActorFacet, DurabilityFacet, GenServerBehavior
- **Features**: Counter Durable Object with state persistence
- **Documentation**: Complete with native TypeScript code, PlexSpaces implementation, design decisions, and benchmarks

---

## PlexSpaces Abstractions Showcase

Each comparison demonstrates specific PlexSpaces abstractions:

### Facets (Dynamic Capabilities)

| Comparison | Facets Demonstrated | Description |
|------------|---------------------|-------------|
| **Orleans** | VirtualActorFacet, TimerFacet, ReminderFacet | Automatic activation/deactivation, in-memory timers, durable reminders |
| **Restate** | DurabilityFacet | Journaling, deterministic replay, exactly-once semantics |
| **Cloudflare Workers** | VirtualActorFacet | Durable Objects pattern with automatic persistence |

### Behaviors (Compile-time Patterns)

| Comparison | Behaviors Demonstrated | Description |
|------------|------------------------|-------------|
| **Erlang/OTP** | GenServerBehavior | Request-reply pattern, synchronous communication |
| **Temporal** | WorkflowBehavior | Durable workflow execution with activities |
| **AWS Step Functions** | WorkflowBehavior, GenFSMBehavior | State machine workflows, finite state machines |

### Coordination

| Comparison | Coordination Demonstrated | Description |
|------------|---------------------------|-------------|
| **Ray** | TupleSpace, Actor coordination | Distributed result aggregation, decoupled coordination |
| **Erlang/OTP** | Supervision trees | Hierarchical fault tolerance |

### Durability

| Comparison | Durability Demonstrated | Description |
|------------|-------------------------|-------------|
| **Temporal** | Journaling, Checkpoints | Durable workflows, fast recovery |
| **Restate** | Journaling, Side effect caching | Deterministic replay, exactly-once execution |

---

## Key Features Comparison

### Actor Model

| Feature | Erlang/OTP | Orleans | Temporal | PlexSpaces |
|---------|------------|---------|----------|------------|
| Virtual Actors | ❌ | ✅ | ❌ | ✅ |
| Supervision Trees | ✅ | ❌ | ❌ | ✅ |
| Durable Execution | ❌ | ⚠️ | ✅ | ✅ |
| Journaling | ❌ | ⚠️ | ✅ | ✅ |
| Timers/Reminders | ✅ | ✅ | ✅ | ✅ |
| Location Transparency | ✅ | ✅ | ✅ | ✅ |
| Fault Tolerance | ✅ | ⚠️ | ✅ | ✅ |

### Workflow Orchestration

| Feature | Temporal | Restate | Azure Durable | AWS Step | PlexSpaces |
|---------|----------|---------|---------------|----------|------------|
| Durable Execution | ✅ | ✅ | ✅ | ✅ | ✅ |
| Deterministic Replay | ✅ | ✅ | ✅ | ❌ | ✅ |
| Activity Patterns | ✅ | ✅ | ✅ | ✅ | ✅ |
| Sub-workflows | ✅ | ✅ | ✅ | ✅ | ✅ |
| Human Tasks | ✅ | ❌ | ✅ | ✅ | ✅ |
| Event Sourcing | ✅ | ✅ | ⚠️ | ❌ | ✅ |

### Performance Characteristics

| Framework | Latency (P50) | Throughput | Cold Start | Memory |
|-----------|---------------|------------|------------|--------|
| Erlang/OTP | <1ms | 100K+ msg/s | N/A | Low |
| Orleans | <5ms | 50K+ msg/s | <100ms | Medium |
| Temporal | <10ms | 10K+ wf/s | N/A | Medium |
| Restate | <5ms | 20K+ ops/s | N/A | Low |
| Ray | <10ms | 100K+ tasks/s | <1s | High |
| PlexSpaces | <2ms | 50K+ msg/s | <50ms | Low |

*Note: Benchmarks are approximate and depend on workload. See individual comparison directories for detailed metrics.*

---

## Running Comparisons

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install dependencies (varies by comparison)
# See individual README files for specific requirements
```

### Run a Comparison

```bash
# Navigate to a comparison directory
cd examples/comparison/erlang_otp

# Run the native example (if applicable)
./scripts/run_native.sh

# Run the PlexSpaces example
./scripts/run_plexspaces.sh

# Run benchmarks
./scripts/benchmark.sh

# Run tests
cargo test
```

### Run All Comparisons

```bash
# From the comparison directory
./scripts/run_all_comparisons.sh
```

---

## Benchmarking

Each comparison includes:

1. **Latency Metrics** - P50, P95, P99 message/workflow latency
2. **Throughput Metrics** - Messages/workflows per second
3. **Resource Usage** - CPU, memory, network overhead
4. **Cold Start Times** - Actor/workflow initialization
5. **Scalability** - Performance at different scales

### Viewing Results

```bash
# Run benchmarks
cd examples/comparison/<framework>
./scripts/benchmark.sh

# View results
cat metrics/benchmark_results.json
```

---

## PlexSpaces Abstractions Explained

### Facets: Dynamic Capabilities

Facets enable runtime composition of capabilities without changing core actor code:

```rust
// Actor without durability (zero overhead)
let actor = ActorBuilder::new()
    .with_behavior(MyBehavior)
    .build();

// Actor with durability (opt-in)
let durable_actor = ActorBuilder::new()
    .with_behavior(MyBehavior)
    .with_durability(DurabilityConfig::default())
    .build();

// Actor with virtual actor lifecycle
let virtual_actor = ActorBuilder::new()
    .with_behavior(MyBehavior)
    .with_virtual_actor(VirtualActorConfig {
        activation_strategy: ActivationStrategy::Lazy,
        deactivation_timeout: Duration::from_secs(300),
    })
    .build();
```

**Design Decision**: Facets follow the "Static for core, Dynamic for extensions" principle. Core actor functionality (identity, state, behavior, mailbox) is static and compiled in. Extensions (durability, virtual actor lifecycle, timers) are optional facets that can be attached at runtime.

### Behaviors: Compile-time Patterns

Behaviors are traits that define how actors process messages:

```rust
// GenServer: Request-reply pattern
impl GenServerBehavior<Request, Response> for MyActor {
    async fn handle_call(&mut self, req: Request, ctx: &ActorContext) -> Result<Response> {
        // Synchronous - MUST return a reply
        Ok(Response::new())
    }
}

// Workflow: Durable execution
impl WorkflowBehavior for MyWorkflow {
    async fn run(&mut self, ctx: &ActorContext, input: Message) -> Result<Message> {
        // All operations are journaled
        ctx.step(|| async { /* activity */ }).await?;
        Ok(Message::new(b"complete".to_vec()))
    }
}
```

**Design Decision**: Behaviors are compile-time traits (zero overhead) vs runtime facets (small interception overhead). This allows the compiler to optimize and provides type safety.

### TupleSpace: Decoupled Coordination

TupleSpace enables coordination without direct actor references:

```rust
// Producer: Write result to tuplespace
ctx.get_tuplespace().await?
    .write(tuple!["result", data])
    .await?;

// Consumer: Read result from tuplespace (doesn't need to know producer)
let results = ctx.get_tuplespace().await?
    .read(pattern!["result", _])
    .await?;
```

**Design Decision**: TupleSpace provides decoupled coordination - producers and consumers don't need to know each other. This enables flexible, scalable coordination patterns.

### Durable Execution: Journaling

DurabilityFacet adds journaling and deterministic replay:

```rust
// Actor with durability
let actor = ActorBuilder::new()
    .with_behavior(MyBehavior)
    .with_durability(DurabilityConfig {
        checkpoint_interval: 10,
        replay_on_activation: true,
        cache_side_effects: true,
    })
    .build();
```

**Design Decision**: Durability is optional - actors without durability have zero overhead. This allows mixing durable and non-durable actors in the same system.

---

## Architecture Patterns

### Pattern 1: Virtual Actors (Orleans/Orbit)

**Native (Orleans)**:
```csharp
public class CounterGrain : Grain, ICounterGrain
{
    private int _count = 0;
    
    public Task<int> Increment() => Task.FromResult(++_count);
}
```

**PlexSpaces**:
```rust
#[derive(Actor)]
pub struct CounterActor {
    count: u64,
}

impl ActorBehavior for CounterActor {
    async fn handle(&mut self, msg: Message, ctx: &ActorContext) -> Result<()> {
        match msg {
            Message::Increment => {
                self.count += 1;
                ctx.reply(Message::Count(self.count)).await?;
            }
            _ => {}
        }
        Ok(())
    }
}
```

### Pattern 2: Durable Workflows (Temporal/Restate)

**Native (Temporal)**:
```typescript
export async function orderWorkflow(order: Order): Promise<void> {
  await validateOrder(order);
  await processPayment(order);
  await shipOrder(order);
}
```

**PlexSpaces**:
```rust
#[workflow]
pub async fn order_workflow(ctx: &WorkflowContext, order: Order) -> Result<()> {
    ctx.step(|| validate_order(order.clone())).await?;
    ctx.step(|| process_payment(order.clone())).await?;
    ctx.step(|| ship_order(order.clone())).await?;
    Ok(())
}
```

---

## Key Differentiators

### PlexSpaces Advantages

1. **Unified Model** - Combines actor model, workflows, and tuplespace coordination
2. **Proto-First** - All contracts defined in Protocol Buffers
3. **WASM Support** - Polyglot actors via WebAssembly
4. **Firecracker Integration** - Strong isolation with microVMs
5. **Rust Performance** - Zero-cost abstractions, memory safety
6. **Comprehensive Observability** - Built-in metrics, tracing, logging

### When to Use PlexSpaces

- ✅ Need unified actor + workflow + coordination model
- ✅ Require strong isolation (Firecracker microVMs)
- ✅ Want polyglot support (WASM actors)
- ✅ Need high performance (Rust)
- ✅ Require proto-first contracts
- ✅ Want comprehensive observability

### When to Use Alternatives

- **Erlang/OTP**: Mature ecosystem, battle-tested, BEAM VM features
- **Orleans**: .NET ecosystem, Microsoft support
- **Temporal**: Complex workflows, large community
- **Ray**: ML workloads, Python ecosystem
- **Cloudflare Workers**: Edge computing, global distribution

---

## Contributing

To add a new comparison:

1. Create a new directory: `examples/comparison/<framework>/`
2. Add native framework example (sample code)
3. Add PlexSpaces implementation (fully functional)
4. Add README.md with documentation
5. Add scripts for running and benchmarking
6. Add tests
7. Update this README.md

See `examples/comparison/TEMPLATE/` for a template structure.

---

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design and abstractions
- [Detailed Design](../../docs/detailed-design.md) - Comprehensive component documentation with all facets, behaviors, and APIs
- [Getting Started Guide](../../docs/getting-started.md) - Quick start guide
- [Use Cases](../../docs/use-cases.md) - Real-world applications
- [Examples](../../examples/README.md) - Example gallery

---

**Last Updated**: 2025-12-10
