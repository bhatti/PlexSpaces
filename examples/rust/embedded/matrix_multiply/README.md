# Matrix Multiplication Example (Actor Workers)

**Purpose**: Demonstrate actor-based parallel computation with tell/ask patterns.

**PlexSpaces APIs**: `ActorBuilder::spawn()`, `ActorRef::tell()`, `ActorRef::ask()`

## Quick Start

```bash
cd examples/rust/embedded/matrix_multiply

# Build
cargo build

# Run
cargo run
```

## What It Demonstrates

1. **Actor Workers**: Create worker actors to compute row partitions
2. **Scatter via tell()**: Distribute work to workers (fire-and-forget)
3. **Gather via ask()**: Collect results from workers (request-reply)

## PlexSpaces API Usage

### Create Worker Actors

```rust
use plexspaces_actor::ActorBuilder;
use plexspaces_node::NodeBuilder;
use plexspaces_core::RequestContext;

let node = Arc::new(NodeBuilder::new("matrix-node").build().await);
let service_locator = node.service_locator();
let ctx = RequestContext::new_without_auth("tenant".into(), "compute".into());

// Create worker actor with custom behavior
let worker = ActorBuilder::new(Box::new(MatrixWorker::new(id)))
    .with_id(format!("worker-{}@matrix-node", id))
    .with_namespace("compute")
    .spawn(&ctx, service_locator.clone())
    .await?;
```

### Scatter Work via tell()

```rust
use plexspaces_mailbox::Message;

// Distribute row partitions to workers
let work = WorkerMessage::ComputeRows {
    start_row: 0,
    end_row: 2,
    matrix_a: a.clone(),
    matrix_b: b.clone(),
};

let msg = Message::json(&work)?.with_message_type("compute_rows");
worker.tell(msg).await?;  // Fire-and-forget
```

### Gather Results via ask()

```rust
// Collect results from workers
let response = worker.ask(
    Message::json(&WorkerMessage::GetResult)?,
    Duration::from_secs(5)
).await?;

let result: WorkerResult = serde_json::from_slice(&response.payload)?;
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    Master                           │
│  - Partition rows among workers                     │
│  - Distribute via tell() (scatter)                  │
│  - Collect via ask() (gather)                       │
└─────────────────┬───────────────────────────────────┘
                  │
      ┌───────────┼───────────┐
      ▼           ▼           ▼
┌──────────┐ ┌──────────┐ ┌──────────┐
│ Worker 0 │ │ Worker 1 │ │ Worker N │
│ rows 0-1 │ │ rows 2-3 │ │ rows ... │
└──────────┘ └──────────┘ └──────────┘
```

## Expected Output

```
Step 1: Create node and worker actors
  Created worker-0
  Created worker-1

Step 3: SCATTER work via ActorRef::tell()
  tell(worker-0, ComputeRows { rows: 0..1 })
    Worker 0: computing rows 0..1
    Worker 0: done
  tell(worker-1, ComputeRows { rows: 2..3 })
    Worker 1: computing rows 2..3
    Worker 1: done

Step 4: GATHER results via ActorRef::ask()
  ask(worker-0, GetResult)
    Worker 0: returning 2 rows starting at 0
  ask(worker-1, GetResult)
    Worker 1: returning 2 rows starting at 2
```

## Key APIs

| Operation | PlexSpaces API |
|-----------|----------------|
| Create actor | `ActorBuilder::new(behavior).spawn(&ctx, service_locator)` |
| Send work | `actor_ref.tell(msg).await` |
| Get result | `actor_ref.ask(msg, timeout).await` |

## Use Cases

- **Scientific computing**: Matrix operations, linear algebra
- **ML inference**: Neural network forward pass
- **Graphics**: 3D transformations, rendering
- **Signal processing**: FFT, convolution

## See Also

- [Heat Diffusion](../heat_diffusion/) - TupleSpace coordination
- [MPI Collectives](../mpi_collectives/) - Collective patterns
- [Actor Groups](../actor_groups_sharding/) - Sharding pattern
