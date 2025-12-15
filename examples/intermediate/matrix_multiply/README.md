# Distributed Block-Based Matrix Multiplication

## Overview

This example demonstrates **block-based parallel matrix multiplication** using PlexSpaces' TupleSpace for work distribution and result gathering. It implements a **master-worker pattern** where a master divides matrices into blocks, distributes work via TupleSpace, and workers process blocks in parallel.

## Key Features

- **TupleSpace for Coordination**: Uses TupleSpace for work distribution and result gathering
- **Actor-based Workers**: Worker actors process matrix blocks in parallel
- **ConfigBootstrap**: Erlang/OTP-style configuration loading
- **CoordinationComputeTracker**: Framework metrics for coordination vs compute
- **NodeBuilder/ActorBuilder**: Simplified actor creation using framework builders

## Problem: Block Matrix Multiplication

### Mathematical Foundation

Given two matrices **A** (M×K) and **B** (K×N), compute **C** (M×N) where:

```
C[i][j] = Σ(k=0 to K-1) A[i][k] * B[k][j]
```

### Block Decomposition

Instead of computing element-by-element, we divide matrices into **blocks**:

```
Matrix A (4×4) divided into 2×2 blocks:

┌─────┬─────┐
│ A00 │ A01 │  Each block is 2×2
├─────┼─────┤
│ A10 │ A11 │
└─────┴─────┘

C = A × B becomes:

C00 = A00×B00 + A01×B10
C01 = A00×B01 + A01×B11
C10 = A10×B00 + A11×B10
C11 = A10×B01 + A11×B11
```

## Architecture: Master-Worker Pattern

### Components

1. **Master Actor**:
   - Divides matrices into blocks
   - Distributes work via TupleSpace
   - Gathers results from TupleSpace
   - Assembles final matrix

2. **Worker Actors** (multiple):
   - Read work from TupleSpace
   - Compute block multiplication
   - Write results back to TupleSpace

### Workflow

```
Phase 1: DISTRIBUTE (Master → TupleSpace)
┌────────┐
│ Master │ →→→ Work tuples →→→ TupleSpace
│        │     ("work", i, j, A_blocks, B_blocks)
└────────┘

Phase 2: COMPUTE (Workers in parallel)
Worker 0 →→→ Take work →→→ Compute C[i][j] →→→ Write result
Worker 1 →→→ Take work →→→ Compute C[i][j] →→→ Write result
Worker 2 →→→ Take work →→→ Compute C[i][j] →→→ Write result
...

Phase 3: GATHER (Master ← TupleSpace)
┌────────┐
│ Master │ ←←← Result tuples ←←← TupleSpace
│        │     ("result", i, j, C_block)
└────────┘
```

## Running the Example

### Quick Start

```bash
# Default: 4 workers, 8×8 matrix, 2×2 blocks (from release.toml)
cargo run

# With environment variable overrides
MATRIX_MULTIPLY_MATRIX_SIZE=16 \
MATRIX_MULTIPLY_BLOCK_SIZE=4 \
MATRIX_MULTIPLY_NUM_WORKERS=8 \
cargo run
```

### Configuration

Configuration is loaded using `ConfigBootstrap` from `release.toml`:

```toml
[matrix_multiply]
matrix_size = 8
block_size = 2
num_workers = 4
```

Environment variables can override these values (with `MATRIX_MULTIPLY_` prefix).

### Using Test Scripts

```bash
# Run all tests (unit, integration, E2E)
./scripts/run_tests.sh

# Run E2E test only (shows detailed metrics)
./scripts/run_e2e.sh

# Run distributed multi-node tests
./scripts/run_distributed_tests.sh
```

## Key Framework Features Used

1. **ConfigBootstrap**: Erlang/OTP-style configuration loading
2. **CoordinationComputeTracker**: Metrics for coordination vs compute
3. **NodeBuilder**: Fluent API for node creation
4. **ActorBuilder**: Fluent API for actor creation
5. **TupleSpace**: Work distribution and result gathering
6. **ActorBehavior**: Master and Worker behaviors implement ActorBehavior trait

## Implementation Details

### Master Behavior

The `MasterBehavior` implements `ActorBehavior` and:
- Handles `Start` message to begin computation
- Generates input matrices
- Divides matrices into blocks
- Distributes work via TupleSpace
- Waits for workers to complete
- Gathers results and verifies correctness

**File**: `src/master_behavior.rs`

### Worker Behavior

The `WorkerBehavior` implements `ActorBehavior` and:
- Handles `Start` message to begin background processing
- Continuously polls TupleSpace for work tuples
- Deserializes A and B blocks
- Computes result block: C[i][j] = Σ(k) A[i][k] × B[k][j]
- Writes results back to TupleSpace

**File**: `src/worker_behavior.rs`

### Main Entry Point

The `main.rs` demonstrates:
- Using `NodeBuilder` and `ActorBuilder`
- ConfigBootstrap for configuration
- CoordinationComputeTracker for metrics
- TupleSpace for work distribution

**File**: `src/main.rs`

## Performance Characteristics

### Compute vs Coordination Metrics

**Key Metric**: Granularity Ratio = Compute Time / Coordination Time

```
Good Granularity (8×8 matrix, 2×2 blocks, 4 workers):
├─ Compute: 2×2 block multiplication (4 multiplications per block)
├─ Coordinate: TupleSpace read/write operations
└─ Ratio: High (compute dominates)

Bad Granularity (4×4 matrix, 1×1 blocks, 4 workers):
├─ Compute: Single multiplication per block
├─ Coordinate: TupleSpace read/write operations
└─ Ratio: Low (coordination overhead dominates)
```

### Speedup Analysis

**Amdahl's Law:**
- Sequential fraction (distribution + gathering): ~10-20%
- Parallel fraction (computation): ~80-90%
- Max speedup with N workers: Limited by coordination overhead

## HPC Research Context

### Algorithms from HPC Literature

This example implements patterns from:

1. **ScaLAPACK** (Scalable Linear Algebra Package)
   - Block-based matrix operations
   - Distributed memory parallelism

2. **Cannon's Algorithm**
   - Block matrix multiplication
   - Work distribution patterns

3. **Parallel Matrix Multiplication**
   - Classic HPC kernel
   - Building block for many linear algebra operations

### Key HPC Principles

| Principle | Implementation |
|-----------|---------------|
| **Data Parallelism** | Each worker processes independent blocks |
| **SPMD** (Single Program Multiple Data) | Same worker code, different data blocks |
| **Work Distribution** | TupleSpace provides work queue |
| **Load Balancing** | Workers pull work as available |

## Further Reading

- Framework behavior documentation: `crates/behavior/src/mod.rs`
- Config bootstrap: `crates/node/src/config_bootstrap.rs`
- Metrics helper: `crates/node/src/metrics_helper.rs`
- TupleSpace: `crates/tuplespace/src/lib.rs`
