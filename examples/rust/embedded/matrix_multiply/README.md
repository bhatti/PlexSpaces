# Matrix Multiplication - Parallel Computation with Scatter-Gather Pattern

**Real-world use case**: Scientific computing, ML inference, graphics, signal processing - parallel matrix multiplication using leader-worker pattern with scatter-gather coordination.

**Pattern**: Scatter-gather pattern (cast for distribution, call for collection) demonstrating parallel computation with actor workers.

## Overview

This example demonstrates parallel matrix multiplication (C = A × B) using PlexSpaces actors and the scatter-gather pattern. A leader actor distributes row partitions to worker actors via `cast()` (scatter) and collects results via `call()` (gather).

## Architecture

### MatrixWorker Actor

Each worker actor computes a partition of rows:
- **State**: Worker ID, computed result rows
- **Computation**: Standard matrix multiplication algorithm (O(n³))
- **Coordination**: Receives work via `cast()`, returns results via `call()`

### Scatter-Gather Pattern

1. **Scatter Phase**: Master distributes work to workers via `cast()` (fire-and-forget)
2. **Compute Phase**: Workers perform matrix multiplication in parallel
3. **Gather Phase**: Master collects results via `call()` (request-reply)

### Coordination vs. Computation Metrics

- **Coordination**: Message sending/receiving overhead (scatter phase)
- **Computation**: Actual matrix multiplication work (gather phase includes computation)
- **Granularity Ratio**: compute_time / coordinate_time (target: >10x)

## SDK Features Demonstrated

- `#[gen_server_actor]` - Declares GenServer behavior
- `#[plexspaces_handlers(gen_server)]` - Auto-generated message dispatch
- `#[handler("compute_rows")]` - Compute handler (supports both call and cast)
- `#[handler("get_result")]` - Result retrieval handler
- `spawn()` - SDK helper for spawning actors
- `GenServerRef.cast()` - Fire-and-forget messaging (scatter)
- `GenServerRef.call()` - Request-reply messaging (gather)

## Communication Patterns

### Scatter (Fire-and-Forget)

```rust
use plexspaces_sdk::{GenServerRef, json};

// Distribute work to workers
let work_request = json!({
    "start_row": 0,
    "end_row": 100,
    "matrix_a": matrix_a,
    "matrix_b": matrix_b,
});

worker_ref.cast("compute_rows", &work_request).await?;
```

### Gather (Request-Reply)

```rust
// Collect results from workers
let result: serde_json::Value = worker_ref.call("get_result", &json!({})).await?;
let rows: Vec<Vec<f64>> = serde_json::from_value(result["rows"].clone())?;
```

## Quick Start

```bash
cd examples/rust/embedded/matrix_multiply
cargo run --bin matrix_multiply
```

## Expected Output

```
╔════════════════════════════════════════════════════════════════╗
║       Matrix Multiplication with Actor Workers                 ║
╚════════════════════════════════════════════════════════════════╝

Configuration:
  Matrix size: 1000×1000
  Workers: 8
  Total operations: 2000000000 (2×1000³)

Step 1: Spawn 8 worker actors
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Spawned worker-0
  Spawned worker-1
  ...

Step 3: SCATTER work via GenServerRef::cast()
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  cast(worker-0, compute_rows, rows 0..124)
  cast(worker-1, compute_rows, rows 125..249)
  ...
  Scattered work to 8 workers in 12.34ms

Step 4: GATHER results via GenServerRef::call()
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  call(worker-0, get_result) -> 125 rows in 1234.56ms
  call(worker-1, get_result) -> 125 rows in 1234.78ms
  ...
  Gathered results from 8 workers in 9876.54ms

Step 6: Performance Metrics
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Execution Summary:
  Total execution time: 10000.00ms (10.00s)
  Matrix size: 1000×1000
  Workers: 8
  Total operations: 2000000000 (2×1000³)

Coordination vs Computation Breakdown:
  Coordination time: 12.34ms (0.1%)
  Computation time: 9876.54ms (98.8%)
  Efficiency (compute/total): 98.8%

Benchmark Metrics:
  Performance: 0.20 GFLOPS
  Data processed: 22.89 MB
  Throughput: 2.29 MB/s

Granularity Analysis:
  Granularity ratio (compute/coordinate): 800.00
  ✅ Excellent granularity (coordination overhead is negligible)
```

## Real-World Use Cases

- **Scientific Computing**: Large-scale linear algebra operations
- **ML Inference**: Neural network forward pass (matrix-vector multiplication)
- **Graphics**: 3D transformations, rendering pipelines
- **Signal Processing**: FFT, convolution operations
- **Data Analytics**: Feature transformations, dimensionality reduction

## Design Principles

- **Scatter-Gather Pattern**: Efficient work distribution and result collection
- **Parallel Computation**: Workers compute independently in parallel
- **Coordination Overhead**: Minimal compared to computation (target: <1%)
- **Scalability**: Performance improves with more workers (up to matrix size)

## See Also

- [Heat Diffusion](../heat_diffusion/) - TupleSpace coordination for stencil computation
- [MPI Collectives (Go WASM)](../../../go/apps/mpi_collectives/) - Collective communication patterns with shard-group APIs
- [Event Analytics](../event_analytics/) - Shard groups for distributed analytics
