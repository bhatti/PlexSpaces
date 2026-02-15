# MPI Collectives Example (PlexSpaces APIs)

**Purpose**: Demonstrate MPI/Hadoop-style scatter/gather and map/reduce using PlexSpaces **TupleSpace** and ProcessGroupRegistry with comprehensive performance metrics.

**PlexSpaces APIs**: `TupleSpace` (scatter, gather, reduce, barrier), `ProcessGroupRegistry` (broadcast, all-reduce), `RequestContext` (multi-tenancy; no `internal()`), `CoordinationComputeTracker` (metrics).

## Quick Start

```bash
cd examples/rust/embedded/mpi_collectives

# Build (uses shared workspace target directory)
cargo build

# Run
cargo run
```

## What It Demonstrates

- **TupleSpace** (real APIs): scatter (write tasks, take by workers), gather (read_all results), reduce (write partial_sum, read_all and aggregate), barrier (barrier + write + recv).
- **ProcessGroupRegistry**: broadcast (publish_to_group), all-reduce (reduce via TupleSpace then broadcast).
- **Multi-tenancy**: `RequestContext::new_without_auth(tenant, namespace)` with explicit tenant/namespace. In production use `RequestContext::from_auth(tenant_from_jwt, namespace, ...)` or extract from gRPC/HTTP; auth token can be added when JWT/mTLS is enabled.
- **Storage**: SqliteKVStore `:memory:` for process groups (keyvalue `sql-backend`).
- **Performance Metrics**: `CoordinationComputeTracker` tracks coordination vs computation time, granularity ratio, efficiency, and benchmark metrics (throughput, ops/sec).

## Real-World Use Cases

- **Distributed Machine Learning**: Gradient averaging across workers (AllReduce pattern)
- **Monte Carlo Simulations**: Aggregate random samples from multiple workers (Reduce pattern)
- **MapReduce Workloads**: Scatter map tasks, gather results (TupleSpace scatter/gather)
- **Consensus Algorithms**: Barrier synchronization + broadcast for voting/agreement
- **Scientific Computing**: Parallel reduction operations (sum, max, min across distributed data)

## MPI → PlexSpaces API Mapping

| MPI Operation | PlexSpaces API |
|---------------|----------------|
| **Broadcast** | `ProcessGroupRegistry::publish_to_group(&ctx, group, None, data)` |
| **Scatter** | TupleSpace: `write(scatter_task tuple)`; workers `take(pattern)` |
| **Gather** | TupleSpace: workers write `gather_result`; coordinator `read_all(pattern)` |
| **Reduce** | TupleSpace: workers `write(partial_sum)`; coordinator `read_all(pattern)` and sum |
| **Barrier** | TupleSpace: `barrier(name, pattern, count)`; each worker `write(tuple)` then `rx.recv()` |
| **AllReduce** | Reduce (TupleSpace) + Broadcast (ProcessGroupRegistry) |

## PlexSpaces API Usage

### RequestContext (multi-tenancy)

```rust
use plexspaces_core::RequestContext;

// Example / tests: explicit tenant and namespace (no internal())
let ctx = RequestContext::new_without_auth("mpi-tenant".to_string(), "collectives".to_string());

// Production: from JWT or gRPC/HTTP metadata
// let ctx = RequestContext::from_auth(tenant_from_jwt, namespace, user_id, admin, auth_enabled, ...)?;
```

### Broadcast via ProcessGroupRegistry

```rust
registry.create_group(&ctx, "workers").await?;
registry.join_group(&ctx, "workers", &worker_id, vec![]).await?;
let recipients = registry.publish_to_group(&ctx, "workers", None, data).await?;
```

### Scatter / Gather / Reduce / Barrier via TupleSpace

```rust
use plexspaces_tuplespace::{TupleSpace, Tuple, TupleField, Pattern, PatternField, tuple};

let tuplespace = TupleSpace::with_tenant_namespace(tenant_id, namespace);

// Scatter: coordinator writes one tuple per worker; workers take(pattern)
tuplespace.write(Tuple::new(vec![
    TupleField::String("scatter_task".into()),
    TupleField::String(worker_id),
    TupleField::Integer(chunk_index),
    TupleField::Binary(payload),
])).await?;
let taken = tuplespace.take(pattern_scatter_task()).await?;

// Gather: workers write gather_result; coordinator read_all
tuplespace.write(Tuple::new(vec![
    TupleField::String("gather_result".into()),
    TupleField::String(worker_id),
    TupleField::Binary(result_bytes),
])).await?;
let results = tuplespace.read_all(pattern_gather_result()).await?;

// Reduce: workers write partial_sum; coordinator read_all and sum
tuplespace.write(Tuple::new(vec![
    TupleField::String("partial_sum".into()),
    TupleField::String(worker_id),
    TupleField::Float(OrderedFloat::new(local_sum)),
])).await?;
let partial_tuples = tuplespace.read_all(pattern_partial_sum()).await?;

// Barrier: register barrier, each worker writes tuple then waits
let pattern = Pattern::new(vec![
    PatternField::Exact(TupleField::String("barrier".into())),
    PatternField::Exact(TupleField::String(barrier_id.into())),
]);
let rx = tuplespace.barrier(barrier_id.to_string(), pattern, num_workers).await;
tuplespace.write(tuple!("barrier", barrier_id)).await?;
let _ = rx.recv().await;
```

### Performance Metrics

```rust
use plexspaces_node::CoordinationComputeTracker;

let mut metrics_tracker = CoordinationComputeTracker::new("mpi-collectives".to_string());

// Track coordination (message passing, barriers)
metrics_tracker.start_coordinate();
// ... coordination operations ...
metrics_tracker.end_coordinate();

// Track computation (actual work)
metrics_tracker.start_compute();
// ... computation ...
metrics_tracker.end_compute();

// Get final metrics
let metrics = metrics_tracker.finalize();
println!("Granularity ratio: {:.2}×", metrics.granularity_ratio);
println!("Efficiency: {:.2}%", metrics.efficiency * 100.0);
```

## Architecture

```
BROADCAST (ProcessGroupRegistry)     SCATTER/GATHER/REDUCE (TupleSpace)
┌────────┐                           Coordinator writes scatter_task tuples
│ Master │                           Workers take(pattern), process, write gather_result
└───┬────┘                           Coordinator read_all(gather_result)
    │ publish_to_group               Reduce: write partial_sum; read_all; sum
    ├───────┬───────┐                Barrier: barrier(); write(); recv()
    ▼       ▼       ▼
[workers] [workers] [workers]         TupleSpace: [scatter_task, gather_result, partial_sum, barrier]
```

## Expected Output

The example runs with non-trivial data sizes (800k elements total, 100k per worker) to demonstrate real performance metrics:

```
╔════════════════════════════════════════════════════════════════╗
║     MPI Collectives with TupleSpace + ProcessGroupRegistry     ║
╚════════════════════════════════════════════════════════════════╝

Configuration:
  Workers: 8
  Data size per worker: 100000 elements
  Total data size: 800000 elements

Step 1: BROADCAST via ProcessGroupRegistry
  worker-0@mpi-node joined 'workers' group
  ...
  Broadcast time: 2.34ms

Step 2: SCATTER via TupleSpace (write tasks, take by workers)
  Scatter time: 15.67ms
  Workers take their task (take pattern), process, write gather_result:
  Compute time: 234.56ms

Step 3: GATHER via TupleSpace (read_all gather_result)
  Gather time: 8.23ms

Step 4: REDUCE via TupleSpace ...
  Reduce time: 3.45ms

Step 5: BARRIER via TupleSpace ...
  Barrier time: 1.23ms

Step 6: ALL_REDUCE = Reduce + Broadcast
  AllReduce time: 1.89ms

================================================================================
📊 PERFORMANCE METRICS & BENCHMARKS
================================================================================

⚡ LATENCY BREAKDOWN (Coordination vs Computation)
  Coordination:     32.81 ms (total)
  Computation:     234.56 ms (total)
  Total Time:      267.37 ms (0.27 seconds)

📈 COORDINATION vs COMPUTATION ANALYSIS
  Granularity ratio:     7.15× (compute/coordinate)
  Efficiency:           87.73% (compute/total)
  Message count:        25
  Barrier count:        1

🚀 BENCHMARK METRICS
  Throughput:        2.99 M ops/s
  Data throughput:   23.92 MB/s
```

## Performance Metrics Explained

### Granularity Ratio
- **< 10×**: Too much overhead, consider coarser granularity
- **10×-100×**: Acceptable for small-medium problems
- **> 100×**: Excellent efficiency, parallelism beneficial

### Efficiency
- Percentage of time spent on actual computation vs coordination
- Higher is better (closer to 100% means less overhead)

### Cost Breakdown
- Shows percentage of total time spent on coordination vs computation
- Helps identify bottlenecks and optimization opportunities

## Key APIs

| Pattern | API | Description |
|---------|-----|-------------|
| Broadcast | `publish_to_group()` | One-to-all (ProcessGroupRegistry) |
| Scatter | TupleSpace `write` + `take` | Distribute tasks via tuples |
| Gather | TupleSpace `read_all` | Collect results via tuples |
| Reduce | TupleSpace `write` + `read_all` | Aggregate partial results |
| Barrier | TupleSpace `barrier` + `write` + `recv` | Synchronization point |
| AllReduce | Reduce + Broadcast | Combine then broadcast |

## Configuration

The example uses non-trivial data sizes by default:
- **Workers**: 8
- **Data per worker**: 100,000 elements
- **Total data**: 800,000 elements

This ensures the example runs for 2+ seconds to show realistic performance metrics.

## Build Configuration

- **Shared Target Directory**: Uses workspace shared `target/` directory (configured via `.cargo/config.toml`)
- **Debug Builds**: Uses debug builds (not `--release`) for faster iteration

## Use Cases

- **Distributed ML**: Gradient averaging (AllReduce)
- **Monte Carlo**: Aggregate random samples (Reduce)
- **MapReduce**: Scatter map tasks, gather results (TupleSpace)
- **Consensus**: Barrier + broadcast for voting

## See Also

- [Matrix Vector MPI](../matrix_vector_mpi/) - Similar MPI-style example with actors and SDK patterns
- [Chat Room](../chat_room/) - Process groups, publish_to_group, list_groups
- [Heat Diffusion](../heat_diffusion/) - TupleSpace coordination
