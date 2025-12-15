# Entity Recognition AI Workload Example

## Overview

This example demonstrates **resource-aware scheduling** with a realistic AI workload, similar to [Ray's entity recognition example](https://docs.ray.io/en/latest/ray-overview/examples/entity-recognition-with-llms/README.html).

The example shows how to:
- Use `NodeBuilder` and `ConfigBootstrap` for configuration
- Use `CoordinationComputeTracker` for metrics
- Deploy actors with resource requirements (CPU/GPU)
- Coordinate multi-stage workflows (Loader → Processor → Aggregator)

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Application: Entity Recognition           │
│                                                              │
│  ┌──────────────┐     ┌──────────────┐     ┌─────────────┐ │
│  │  Loader      │────▶│  Processor   │────▶│  Aggregator │ │
│  │  (CPU-bound) │     │  (GPU-bound) │     │  (CPU-bound)│ │
│  └──────────────┘     └──────────────┘     └─────────────┘ │
│         │                    │                    │          │
│         ▼                    ▼                    ▼          │
│  ┌──────────────────────────────────────────────────────┐   │
│  │         Scheduling Layer (Resource-Aware)            │   │
│  │  - Loader → CPU nodes                                │   │
│  │  - Processor → GPU nodes                             │   │
│  │  - Aggregator → CPU nodes                            │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                              │
│  ┌──────────────┐     ┌──────────────┐     ┌─────────────┐ │
│  │  Node 1      │     │  Node 2      │     │  Node 3     │ │
│  │  (CPU)       │     │  (GPU)       │     │  (CPU)      │ │
│  └──────────────┘     └──────────────┘     └─────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## Components

### 1. Loader Actor (CPU-intensive)
- **Resource requirements**: CPU=2, Memory=1GB
- **Labels**: `workload=cpu-intensive`, `service=loader`
- **Group**: `entity-recognition-loaders`
- **Purpose**: Reads documents from storage

### 2. Processor Actor (GPU-intensive)
- **Resource requirements**: CPU=1, Memory=4GB, GPU=1
- **Labels**: `workload=gpu-intensive`, `service=processor`
- **Group**: `entity-recognition-processors`
- **Purpose**: Runs LLM inference to extract entities

### 3. Aggregator Actor (CPU-intensive)
- **Resource requirements**: CPU=2, Memory=1GB
- **Labels**: `workload=cpu-intensive`, `service=aggregator`
- **Group**: `entity-recognition-aggregators`
- **Purpose**: Aggregates results from all processors

## Configuration

Configuration is loaded using `ConfigBootstrap` from `release.toml`:

```toml
[entity_recognition]
loader_count = 2
processor_count = 1
aggregator_count = 1
backend = "memory"  # or "redis" for multi-node
```

Environment variables can override configuration:
- `ENTITY_RECOGNITION_LOADER_COUNT=3`
- `ENTITY_RECOGNITION_BACKEND=redis`
- `ENTITY_RECOGNITION_REDIS_URL=redis://localhost:6379`

## Usage

### Single-Node (Memory Backend)

```bash
# Build and run
./scripts/run.sh

# Or manually:
cargo run --release --bin entity-recognition-app -- \
    doc1.txt doc2.txt doc3.txt
```

### Multi-Node (Redis Backend)

```bash
# Start Redis (if not running)
docker run -d -p 6379:6379 redis:7-alpine

# Deploy multi-node setup
./scripts/deploy.sh

# In another terminal, run application
cargo run --release --bin entity-recognition-app -- \
    --backend redis \
    --redis-url redis://localhost:6379 \
    doc1.txt doc2.txt doc3.txt
```

## Scripts

- `scripts/run.sh` - Run single-node example (simple)
- `scripts/run_with_metrics.sh` - Run example and display metrics prominently
- `scripts/test.sh` - Run tests, validation, and example execution
- `scripts/run_tests.sh` - Run unit tests, clippy, format check
- `scripts/deploy.sh` - Deploy multi-node setup with Redis

### Quick Start

```bash
# Run example with metrics
./scripts/run_with_metrics.sh

# Or simple run
./scripts/run.sh

# Run tests
./scripts/test.sh
```

## Metrics

The example uses `CoordinationComputeTracker` to track:
- **Coordination time**: Time spent on framework operations (spawning actors, messaging)
- **Compute time**: Time spent on actual computation (document processing, entity extraction)
- **Granularity ratio**: Coordination time / Compute time (lower is better for HPC)

Metrics are displayed at the end of execution:
```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📊 Performance Metrics:
  Coordination time: 45.23 ms
  Compute time: 1250.50 ms
  Granularity ratio: 27.65x
  Efficiency: 96.50%
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

**Metrics Explanation**:
- **Coordination time**: Time spent on framework operations (spawning actors, message passing)
- **Compute time**: Time spent on actual computation (document loading, LLM inference, aggregation)
- **Granularity ratio**: Compute time / Coordination time (higher is better - more compute per coordination overhead)
- **Efficiency**: Percentage of total time spent on computation (higher is better for HPC workloads)

## Framework Abstractions Used

This example demonstrates the use of framework abstractions:

- **`NodeBuilder`**: Fluent API for node creation
- **`ConfigBootstrap`**: Erlang/OTP-style configuration loading
- **`CoordinationComputeTracker`**: Standardized metrics tracking
- **`ActorBehavior`**: Actor behavior implementations (GenServer pattern)
- **`Application` trait**: (Optional) For complex lifecycle management

## Testing

```bash
# Run all tests
./scripts/run_tests.sh

# Or manually:
cargo test --lib
cargo clippy -- -D warnings
cargo fmt --check
```

## Implementation Notes

- Uses `GenServerBehavior` for request/reply patterns
- Resource-aware scheduling via actor groups and labels
- Configuration-driven via `release.toml` and environment variables
- Metrics tracking for coordination vs compute analysis
- Uses `NodeBuilder` for node creation (not `Node::new()` directly)
- Uses `ConfigBootstrap` for configuration loading (Erlang/OTP-style)
- Uses `CoordinationComputeTracker` for standardized metrics

## Code Documentation

All code is documented with:
- Module-level documentation (`//!`) explaining purpose and design
- Function documentation (`///`) for public APIs
- Inline comments for complex logic

Key files:
- `src/bin/app.rs` - Application orchestrator with metrics tracking
- `src/bin/node.rs` - Node binary using NodeBuilder and ConfigBootstrap
- `src/config.rs` - Configuration using ConfigBootstrap pattern
- `src/loader.rs` - Loader actor behavior (CPU-intensive)
- `src/processor.rs` - Processor actor behavior (GPU-intensive)
- `src/aggregator.rs` - Aggregator actor behavior (CPU-intensive)
- `src/application.rs` - Application trait implementation (optional, for lifecycle management)

## References

- [Ray Entity Recognition Example](https://docs.ray.io/en/latest/ray-overview/examples/entity-recognition-with-llms/README.html)
- [ConfigBootstrap Documentation](../../../crates/node/src/config_bootstrap.rs)
- [CoordinationComputeTracker](../../../crates/node/src/coordination_compute_tracker.rs)
