# Examples

PlexSpaces includes comprehensive examples organized by complexity. Each example demonstrates specific patterns and use cases.

## Quick Navigation

- [Simple Examples](#simple-examples): Basic patterns and concepts
- [Intermediate Examples](#intermediate-examples): Multi-actor coordination
- [Advanced Examples](#advanced-examples): Complex distributed systems
- [Domain Examples](#domain-examples): Real-world applications
- [Framework Comparisons](#framework-comparisons): Side-by-side comparisons

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

Learn WASM actor deployment with polyglot support (Python/JavaScript).

**Features**:
- Python actors
- JavaScript actors
- WASM deployment

**Run**:
```bash
cd examples/simple/wasm_calculator
./test.sh
```

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

### Order Processing

**Location**: `examples/domains/order-processing/`

E-commerce order processing with workflow orchestration.

**Features**:
- Workflow orchestration
- Multi-step processes
- Error handling

**Run**:
```bash
cd examples/domains/order-processing
cargo run
```

### Genomics Pipeline

**Location**: `examples/domains/genomics-pipeline/`

Scientific computing with data processing pipelines.

**Features**:
- Data pipelines
- Scientific computing
- Large-scale processing

**Run**:
```bash
cd examples/domains/genomics-pipeline
cargo run
```

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
