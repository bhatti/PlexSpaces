# Matrix-Vector Multiplication - HPC with MPI-style Collective Operations

## Overview

This example demonstrates **high-level MPI collective operations** (scatter, broadcast, gather, reduce, barrier) using PlexSpaces TupleSpace. It implements **parallel matrix-vector multiplication** using classical HPC patterns from MPI (Message Passing Interface).

## Key Features

- **TupleSpace for Dataflow**: Uses TupleSpace for coordination (designed for dataflow patterns)
- **Actor-based Workers**: Worker actors process matrix rows in parallel
- **ConfigBootstrap**: Erlang/OTP-style configuration loading
- **CoordinationComputeTracker**: Framework metrics for coordination vs compute
- **Application Trait**: Demonstrates building applications with multiple actors/supervisors

## Problem: Parallel Matrix-Vector Multiplication

### Mathematical Foundation

Given matrix **A** (M×N) and vector **x** (N×1), compute **y** (M×1) where:

```
y[i] = Σ(j=0 to N-1) A[i][j] * x[j]
```

### Parallel Strategy: Row-wise Decomposition

```
Matrix A (8×4):        Vector x (4×1):     Result y (8×1):
┌────────────────┐     ┌───┐              ┌────┐
│ 1  2  3  4     │     │ 1 │              │ 30 │
│ 5  6  7  8     │  ×  │ 2 │         =    │ 70 │
│ 1  1  1  1     │     │ 3 │              │ 10 │
│ 2  2  2  2     │     │ 4 │              │ 20 │
│ 3  3  3  3     │     └───┘              │ 30 │
│ 4  4  4  4     │                        │ 40 │
│ 5  5  5  5     │                        │ 50 │
│ 6  6  6  6     │                        │ 60 │
└────────────────┘                        └────┘

With 2 workers:
Worker 0: Rows 0-3 → [30, 70, 10, 20]
Worker 1: Rows 4-7 → [30, 40, 50, 60]
```

## Architecture: MPI-style Collective Communication

### MPI Primitives Demonstrated

| MPI Operation | Purpose | TupleSpace Pattern |
|---------------|---------|-------------------|
| **MPI_Scatter** | Distribute matrix rows to workers | `("scatter", "matrix_rows", worker_id, row_data)` |
| **MPI_Bcast** | Broadcast vector to all workers | `("broadcast", "vector", vector_data)` |
| **MPI_Barrier** | Synchronize all workers | `space.barrier(name, pattern, count)` |
| **MPI_Gather** | Collect results from workers | `("gather", "result", worker_id, partial_result)` |
| **MPI_Reduce** | Aggregate with operation (sum, max) | `("reduce", "operation", values...)` |

### Workflow

```
Phase 1: SCATTER (Master → Workers)
┌────────┐
│ Master │ →→→ [Row 0-3] →→→ Worker 0
│        │ →→→ [Row 4-7] →→→ Worker 1
└────────┘

Phase 2: BROADCAST (Master → All)
┌────────┐
│ Master │ →→→ Vector [1,2,3,4] →→→ All Workers
└────────┘

Phase 3: COMPUTE (Workers in parallel)
Worker 0: [1,2,3,4] × rows → [30,70,10,20]
Worker 1: [1,2,3,4] × rows → [30,40,50,60]

Phase 4: BARRIER (Synchronize)
Worker 0 ━━━┓
Worker 1 ━━━┫ BARRIER → All done
Master   ━━━┛

Phase 5: GATHER (Workers → Master)
Worker 0 →→→ [30,70,10,20] →→→ ┐
Worker 1 →→→ [30,40,50,60] →→→ ├→ Master assembles [30,70,10,20,30,40,50,60]
```

## Running the Example

### Quick Start (Local Mode)

```bash
# Default: 2 workers, 8×4 matrix (from release.toml)
cargo run

# With environment variable overrides
MATRIX_VECTOR_MPI_NUM_ROWS=16 \
MATRIX_VECTOR_MPI_NUM_COLS=8 \
MATRIX_VECTOR_MPI_NUM_WORKERS=4 \
cargo run
```

### Configuration

Configuration is loaded using `ConfigBootstrap` from `release.toml`:

```toml
[matrix_vector_mpi]
num_rows = 8
num_cols = 4
num_workers = 2
```

Environment variables can override these values (with `MATRIX_VECTOR_MPI_` prefix).

### Using Test Scripts

```bash
# Run all tests (unit, integration, E2E with metrics)
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
5. **TupleSpace**: Dataflow coordination (scatter, broadcast, gather, barrier)
6. **Application Trait**: Building applications with multiple actors

## Implementation Details

### Worker Actors

Worker actors (`WorkerActor`) implement `ActorBehavior` and:
- Read assigned matrix rows from TupleSpace (scatter pattern)
- Read broadcast vector from TupleSpace
- Compute local matrix-vector product
- Write results back to TupleSpace (gather pattern)
- Signal barrier completion

**File**: `src/worker_actor.rs`

### Application Implementation

The `MatrixVectorApplication` implements the `Application` trait and demonstrates:
- Spawning multiple worker actors
- Coordinating via TupleSpace
- Tracking metrics using `CoordinationComputeTracker`
- Graceful shutdown

**File**: `src/application.rs`

### Main Entry Point

The `main.rs` demonstrates:
- Using `NodeBuilder` and `ActorBuilder`
- ConfigBootstrap for configuration
- CoordinationComputeTracker for metrics
- TupleSpace for dataflow coordination

**File**: `src/main.rs`

## Performance Characteristics

### Compute vs Coordination Metrics

**Key Metric**: Granularity Ratio = Compute Time / Coordination Time

```
Good Granularity (16 rows, 8 cols per worker):
├─ Compute: 16 × 8 = 128 multiplications + 16 additions
├─ Coordinate: 3 TupleSpace ops (read rows, read vector, write result)
└─ Ratio: 144 / 3 = 48× (GOOD!)

Bad Granularity (2 rows, 4 cols per worker):
├─ Compute: 2 × 4 = 8 multiplications + 2 additions
├─ Coordinate: 3 TupleSpace ops
└─ Ratio: 10 / 3 = 3.3× (TOO LOW! Overhead dominates)
```

### Speedup Analysis

**Amdahl's Law:**
- Sequential fraction (scatter + gather): ~10%
- Parallel fraction (computation): ~90%
- Max speedup with N workers: ~9.5× (even with infinite workers)

## HPC Research Context

### Algorithms from HPC Literature

This example implements patterns from:

1. **ScaLAPACK** (Scalable Linear Algebra Package)
   - Row-wise matrix distribution
   - Collective communication for linear algebra

2. **MPI (Message Passing Interface)**
   - Standard for distributed-memory parallelism
   - Collective operations: Scatter, Gather, Broadcast, Reduce, Barrier

3. **Parallel Matrix-Vector Product**
   - Classic HPC kernel (used in iterative solvers)
   - Building block for matrix multiplication (GEMM), sparse solvers

### Key HPC Principles

| Principle | Implementation |
|-----------|---------------|
| **Data Parallelism** | Each worker processes independent rows |
| **SPMD** (Single Program Multiple Data) | Same worker code, different data partitions |
| **Collective Communication** | Efficient one-to-all, all-to-one patterns |
| **Synchronization** | Barrier ensures consistency |
| **Load Balancing** | Equal row distribution |

## Further Reading

- Framework behavior documentation: `crates/behavior/src/mod.rs`
- Config bootstrap: `crates/node/src/config_bootstrap.rs`
- Metrics helper: `crates/node/src/metrics_helper.rs`
- TupleSpace: `crates/tuplespace/src/lib.rs`
- Application trait: `crates/core/src/application.rs`
