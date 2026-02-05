# MPI Collectives Example (PlexSpaces APIs)

**Purpose**: Demonstrate MPI/Hadoop-style scatter/gather and map/reduce using PlexSpaces **TupleSpace** and ProcessGroupRegistry.

**PlexSpaces APIs**: `TupleSpace` (scatter, gather, reduce, barrier), `ProcessGroupRegistry` (broadcast, all-reduce), `RequestContext` (multi-tenancy; no `internal()`).

## Quick Start

```bash
cd examples/rust/embedded/mpi_collectives

# Build
cargo build

# Run
cargo run
```

## What It Demonstrates

- **TupleSpace** (real APIs): scatter (write tasks, take by workers), gather (read_all results), reduce (write partial_sum, read_all and aggregate), barrier (barrier + write + recv).
- **ProcessGroupRegistry**: broadcast (publish_to_group), all-reduce (reduce via TupleSpace then broadcast).
- **Multi-tenancy**: `RequestContext::new_without_auth(tenant, namespace)` with explicit tenant/namespace. In production use `RequestContext::from_auth(tenant_from_jwt, namespace, ...)` or extract from gRPC/HTTP; auth token can be added when JWT/mTLS is enabled.
- **Storage**: SqliteKVStore `:memory:` for process groups (keyvalue `sql-backend`).

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

```
Step 1: BROADCAST via ProcessGroupRegistry
  worker-0@mpi-node joined 'workers' group
  ...
Step 2: SCATTER via TupleSpace (write tasks, take by workers)
  Coordinator writes one tuple per worker ...
  Workers take their task (take pattern), process, write gather_result:
Step 3: GATHER via TupleSpace (read_all gather_result)
  Gathered partial sums (sorted): [3.0, 7.0, 11.0, 15.0]
Step 4: REDUCE via TupleSpace ...
  Coordinator read_all(partial_sum) -> global sum = 36
Step 5: BARRIER via TupleSpace ...
  All workers arrived - barrier released!
Step 6: ALL_REDUCE = Reduce + Broadcast
```

## Key APIs

| Pattern | API | Description |
|---------|-----|-------------|
| Broadcast | `publish_to_group()` | One-to-all (ProcessGroupRegistry) |
| Scatter | TupleSpace `write` + `take` | Distribute tasks via tuples |
| Gather | TupleSpace `read_all` | Collect results via tuples |
| Reduce | TupleSpace `write` + `read_all` | Aggregate partial results |
| Barrier | TupleSpace `barrier` + `write` + `recv` | Synchronization point |
| AllReduce | Reduce + Broadcast | Combine then broadcast |

## Use Cases

- **Distributed ML**: Gradient averaging (AllReduce)
- **Monte Carlo**: Aggregate random samples (Reduce)
- **MapReduce**: Scatter map tasks, gather results (TupleSpace)
- **Consensus**: Barrier + broadcast for voting

## See Also

- [Feature Flags](../feature_flags/) - Broadcast pattern, SqliteKVStore
- [Chat Room](../chat_room/) - Process groups, publish_to_group, list_groups
- [Heat Diffusion](../heat_diffusion/) - TupleSpace coordination
