# MPI Collectives Example (PlexSpaces APIs)

**Purpose**: Demonstrate MPI collective patterns using PlexSpaces abstractions.

**PlexSpaces APIs**: `ProcessGroupRegistry`, `TupleSpace`, `ActorRef::tell/ask`

## Quick Start

```bash
cd examples/rust/embedded/mpi_collectives

# Build
cargo build

# Run
cargo run
```

## What It Demonstrates

Mapping MPI collective operations to PlexSpaces APIs:

| MPI Operation | PlexSpaces API |
|---------------|----------------|
| **Broadcast** | `ProcessGroupRegistry::publish_to_group()` |
| **Scatter** | `ActorRef::tell()` to each worker |
| **Gather** | `ActorRef::ask()` from each worker |
| **Reduce** | TupleSpace write/read coordination |
| **Barrier** | TupleSpace barrier synchronization |
| **AllReduce** | Reduce (TupleSpace) + Broadcast (ProcessGroup) |

## PlexSpaces API Usage

### Broadcast via Process Groups

```rust
use plexspaces_process_groups::ProcessGroupRegistry;
use plexspaces_core::RequestContext;

let ctx = RequestContext::new_without_auth("tenant".into(), "namespace".into());

// Create group and add workers
registry.create_group(&ctx, "workers").await?;
registry.join_group(&ctx, "workers", &worker_id, vec![]).await?;

// Broadcast to all workers
let data = b"config_data".to_vec();
let recipients = registry.publish_to_group(&ctx, "workers", None, data).await?;
```

### Scatter via tell()

```rust
// Partition data and send to each worker
for (i, worker) in workers.iter().enumerate() {
    let chunk = &data[start..end];
    let msg = Message::json(&chunk)?;
    worker.tell(msg).await?;
}
```

### Gather via ask()

```rust
// Collect results from all workers
let mut results = Vec::new();
for worker in &workers {
    let response = worker.ask(GetResultMsg, timeout).await?;
    results.push(response);
}
```

### Reduce via TupleSpace

```rust
use plexspaces_tuplespace::TupleSpace;

// Each worker writes partial result
tuplespace.write(&ctx, tuple!["partial_sum", worker_id, local_sum]).await?;

// Coordinator reads and aggregates
let mut global_sum = 0.0;
for _ in 0..num_workers {
    let tuple = tuplespace.read(&ctx, pattern!["partial_sum", _, _]).await?;
    global_sum += extract_value(tuple);
}
```

## Architecture

```
BROADCAST (ProcessGroup)           REDUCE (TupleSpace)
┌────────┐                        ┌────────┐
│ Master │                        │Worker 0│──┐
└───┬────┘                        └────────┘  │ write partial
    │ publish_to_group            ┌────────┐  │
    ├───────┬───────┐             │Worker 1│──┼──▶ TupleSpace
    ▼       ▼       ▼             └────────┘  │    [sum, id, val]
[all]   [all]   [all]             ┌────────┐  │
                                  │Worker N│──┘
                                  └────────┘
                                       │
                                       ▼ read all
                                  ┌────────┐
                                  │Coord.  │ → global_sum
                                  └────────┘
```

## Expected Output

```
Step 1: BROADCAST via ProcessGroupRegistry
  worker-0@mpi-node joined 'workers' group
  worker-1@mpi-node joined 'workers' group
  Broadcast config to 4 workers:
    -> worker-0@mpi-node received config

Step 4: REDUCE via TupleSpace
  Workers write partial sums to TupleSpace:
    tuplespace.write(["partial_sum", "worker-0", 3.0])
    tuplespace.write(["partial_sum", "worker-1", 7.0])
  Coordinator reads partial sums:
    tuplespace.read(["partial_sum", _, _]) -> 3.0
    tuplespace.read(["partial_sum", _, _]) -> 7.0
  Global sum (reduce): 36.0
```

## Key APIs

| Pattern | API | Description |
|---------|-----|-------------|
| Broadcast | `publish_to_group()` | One-to-all communication |
| Scatter | `tell()` | Distribute partitions |
| Gather | `ask()` | Collect results |
| Reduce | TupleSpace write/read | Aggregate with operation |
| Barrier | TupleSpace barrier | Synchronization point |

## Use Cases

- **Distributed ML**: Gradient averaging (AllReduce)
- **Monte Carlo**: Aggregate random samples (Reduce)
- **MapReduce**: Scatter map tasks, gather results
- **Consensus**: Barrier + broadcast for voting

## See Also

- [Feature Flags](../feature_flags/) - Broadcast pattern
- [Chat Room](../chat_room/) - Process groups
- [Heat Diffusion](../heat_diffusion/) - TupleSpace coordination
