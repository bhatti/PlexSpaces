# Matrix-Vector Multiplication - HPC with MPI-style Collective Operations

## Overview

This example demonstrates **high-level MPI collective operations** (scatter, broadcast, gather, reduce, barrier) using PlexSpaces TupleSpace. It implements **parallel matrix-vector multiplication** using classical HPC patterns from MPI (Message Passing Interface).

## Key Features

- **SDK Annotations**: Uses `#[event_actor]`, `#[plexspaces_handlers(event)]`, `#[handler]` for clean actor definitions
- **SDK Spawn Helpers**: Uses `spawn()` instead of low-level ActorFactory APIs
- **SDK Message Helpers**: Uses `cast_message()` for fire-and-forget messages
- **TupleSpace for Dataflow**: Uses TupleSpace for coordination (designed for dataflow patterns)
- **Actor-based Workers**: Worker actors process matrix rows in parallel
- **ConfigBootstrap**: Erlang/OTP-style configuration loading
- **CoordinationComputeTracker**: Framework metrics for coordination vs compute

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

1. **SDK Annotations**: `#[event_actor]` for GenEvent behavior, `#[plexspaces_handlers(event)]` for handler dispatch
2. **SDK Spawn Helpers**: `spawn()` for actor creation (simplifies ActorFactory usage)
3. **SDK Message Helpers**: `cast_message()` for fire-and-forget messages
4. **ConfigBootstrap**: Erlang/OTP-style configuration loading
5. **CoordinationComputeTracker**: Metrics for coordination vs compute
6. **NodeBuilder**: Fluent API for node creation
7. **TupleSpace**: Dataflow coordination (scatter, broadcast, gather, barrier)

## Implementation Details

### Worker Actors

Worker actors (`WorkerActor`) use SDK annotations:
- `#[event_actor]` - GenEvent behavior (fire-and-forget)
- `#[plexspaces_handlers(event)]` - Generates EventHandler dispatch
- `#[handler("Compute", cast)]` - Handles Compute messages

Worker behavior:
- Read assigned matrix rows from TupleSpace (scatter pattern)
- Read broadcast vector from TupleSpace
- Compute local matrix-vector product
- Write results back to TupleSpace (gather pattern)
- Signal barrier completion

**File**: `src/worker_actor.rs`

### Main Entry Point

The `main.rs` demonstrates:
- SDK spawn helpers: `spawn()` for actor creation
- SDK message helpers: `cast_message()` for fire-and-forget messages
- `NodeBuilder` for node creation
- ConfigBootstrap for configuration
- CoordinationComputeTracker for metrics
- TupleSpace for dataflow coordination

**File**: `src/main.rs`

### SDK Pattern Example

```rust
// Define actor with SDK annotation
#[event_actor]
pub struct WorkerActor {
    tuplespace: Arc<TupleSpace>,
    worker_id: usize,
}

// Generate handler dispatch
#[plexspaces_handlers(event)]
impl WorkerActor {
    #[handler("Compute", cast)]
    async fn handle_compute(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        // Process computation
        self.compute().await?;
        Ok(())
    }
}

// Spawn actor using SDK helper
let actor_ref = spawn(
    &ctx,
    service_locator,
    actor_id,
    "matrix-vector-mpi",
    worker_actor,
).await?;

// Send message using SDK helper
let message = cast_message(json!({
    "action": "Compute",
    "worker_id": worker_id,
}));
actor_ref.tell(message).await?;
```

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

## Real-World Use Cases

This example demonstrates patterns used in:

1. **Scientific Computing**
   - Linear algebra operations (iterative solvers: CG, GMRES)
   - Sparse matrix-vector products
   - Finite element method (FEM) computations
   - Computational fluid dynamics (CFD)

2. **Machine Learning**
   - Distributed training (gradient computation)
   - Batch processing of feature vectors
   - Neural network forward/backward passes
   - Large-scale data transformations

3. **Data Processing**
   - Parallel data transformations
   - Batch processing pipelines
   - ETL (Extract, Transform, Load) operations
   - Distributed aggregations

4. **HPC Workloads**
   - Parallel algorithms requiring collective communication
   - Distributed memory parallelism
   - Scientific simulations
   - High-performance numerical computing

### When to Use This Pattern

- **Use when**: You need parallel computation with coordination (scatter/gather patterns)
- **Use when**: Problem can be decomposed into independent work units (rows, chunks)
- **Use when**: Collective communication patterns are needed (MPI-style operations)
- **Avoid when**: Problem is too small (coordination overhead dominates)
- **Avoid when**: Work units are highly dependent (not suitable for parallelization)

## Further Reading

- SDK documentation: `docs/sdk.md`
- Framework behavior documentation: `crates/behavior/src/mod.rs`
- Config bootstrap: `crates/node/src/config_bootstrap.rs`
- Metrics helper: `crates/node/src/metrics_helper.rs`
- TupleSpace: `crates/tuplespace/src/lib.rs`
