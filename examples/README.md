# PlexSpaces Examples

This directory contains examples organized by complexity to help you learn PlexSpaces progressively.

---

## Learning Path

### 🟢 Beginner - Simple Examples

Start here to learn the basics:

1. **Timers Example** (`simple/timers_example/`)
   - Learn how to use timers in actors
   - Basic scheduled tasks

2. **Durable Actor Example** (`simple/durable_actor_example/`)
   - Learn durability and journaling
   - Actor state persistence

3. **Actor Groups (Sharding)** (`simple/actor_groups_sharding/`)
   - Learn actor groups for horizontal scaling
   - Hash-based routing

4. **WASM Calculator** (`simple/wasm_calculator/`)
   - Learn WASM actor deployment
   - Polyglot actors (Python)

5. **Firecracker Multi-Tenant** (`simple/firecracker_multi_tenant/`)
   - Learn Firecracker VM isolation
   - Application-level isolation

---

### 🟡 Intermediate - Multi-Actor Coordination

Learn coordination patterns:

1. **Matrix Multiply** (`intermediate/matrix_multiply/`)
   - Multi-actor computation
   - Task distribution

2. **Matrix Vector MPI** (`intermediate/matrix_vector_mpi/`)
   - MPI-style coordination
   - Scatter-gather patterns

3. **Heat Diffusion** (`intermediate/heat_diffusion/`)
   - Multi-actor simulation
   - Region-based coordination

---

### 🔴 Advanced - Distributed Systems

Learn advanced distributed patterns:

1. **Byzantine Generals** (`advanced/byzantine/`)
   - Byzantine fault tolerance
   - Consensus algorithms

2. **N-Body Simulation** (`advanced/nbody/`)
   - Complex simulation
   - Multi-actor physics

3. **N-Body WASM** (`advanced/nbody-wasm/`)
   - WASM-based simulation
   - Performance optimization

---

### 🏢 Domain Examples

Real-world applications:

1. **Order Processing** (`domains/order-processing/`)
   - E-commerce order processing
   - Workflow orchestration

2. **Genomics Pipeline** (`domains/genomics-pipeline/`)
   - Scientific computing
   - Data processing pipelines

3. **Finance Risk** (`domains/finance-risk/`)
   - Financial risk analysis
   - Complex workflows

4. **AI Workloads** (`domains/ai-workloads/`)
   - Machine learning workloads
   - Entity recognition

---

## Quick Start

### Run a Simple Example

```bash
cd examples/simple/timers_example
cargo run
```

### Run an Intermediate Example

```bash
cd examples/intermediate/matrix_multiply
cargo run
```

### Run a Domain Example

```bash
cd domains/order-processing
cargo run
```

---

## Example Structure

Each example includes:

- **README.md** - Documentation and instructions
- **src/** - Source code
- **scripts/** - Helper scripts
- **tests/** - Integration tests (if applicable)

---

## Prerequisites

- Rust 1.70+
- Cargo
- For WASM examples: `wasm-pack` (optional)
- For Firecracker examples: Firecracker binary
- For multi-node examples: Redis or PostgreSQL (optional)

---

## Getting Help

- See [Getting Started Guide](../docs/getting-started.md) for basics
- See [Architecture Guide](../docs/architecture.md) for concepts
- See [Detailed Design](../docs/detailed-design.md) for component details
- See [Use Cases](../docs/use-cases.md) for real-world applications
- See individual example READMEs for specific instructions

---

## Contributing

When adding new examples:

1. Place in appropriate complexity directory
2. Add README.md with:
   - Purpose
   - Prerequisites
   - Running instructions
   - Key concepts
3. Update this README.md
4. Add tests if applicable

---

## Comparison Examples

See [`comparison/`](./comparison/) for side-by-side comparisons with other frameworks:

- **Erlang/OTP** - GenServer with supervision
- **Orleans** - Virtual actors with timers/reminders
- **Temporal** - Durable workflows
- **Ray** - Distributed ML workloads
- And more...

Each comparison includes native framework code, PlexSpaces implementation, benchmarks, and metrics.

---

## Testing

### Test Levels

PlexSpaces examples support three levels of testing:

1. **Unit Tests** - Fast tests with small datasets (~10-20ms)
2. **Integration Tests** - Multi-node coordination with moderate datasets
3. **E2E Tests** - Production-realistic tests with large datasets

### Running Tests

```bash
# Run all tests for an example
cd examples/intermediate/heat_diffusion
cargo test

# Run specific test level
cargo test --test basic_test
cargo test --test distributed_test
cargo test --test e2e_test
```

### Multi-Node Testing

Many examples support multi-node testing via:
- **Local processes** - Multiple processes on same machine
- **Docker Compose** - Containerized multi-node clusters
- **Kubernetes** - Production-like deployments

See individual example READMEs for multi-node setup instructions.

---

## Example Status

### ✅ Complete Examples

- All simple examples (timers, durable actors, WASM calculator, etc.)
- All intermediate examples (matrix multiply, heat diffusion, etc.)
- All advanced examples (Byzantine generals, N-body simulation)
- All domain examples (order processing, genomics pipeline, finance risk, AI workloads)

### 🔧 Configuration

Examples support multiple backends:
- **InMemory** - Fast, single-process only
- **SQLite** - File-based, multi-process support
- **Redis** - Network-based, multi-node support
- **PostgreSQL** - Production-ready, multi-node support

Configuration via:
- Environment variables (`PLEXSPACES_TUPLESPACE_BACKEND`, etc.)
- Config files (`config/*.toml`)
- Programmatic configuration

---

## Contributing

When adding new examples:

1. Place in appropriate complexity directory
2. Add README.md with:
   - Purpose
   - Prerequisites
   - Running instructions
   - Key concepts
3. Update this README.md
4. Add tests if applicable
5. Add scripts for validation and benchmarking

---

**Happy Learning!** 🚀
