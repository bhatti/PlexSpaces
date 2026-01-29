# Heat Diffusion Example (TupleSpace Coordination)

**Purpose**: Demonstrate TupleSpace for neighbor communication in stencil computation.

**PlexSpaces APIs**: `TupleSpace::write()`, `TupleSpace::read()`, barrier synchronization

## Quick Start

```bash
cd examples/rust/embedded/heat_diffusion

# Build
cargo build

# Run
cargo run
```

## What It Demonstrates

1. **TupleSpace for Ghost Cells**: Regions write boundaries, neighbors read them
2. **Barrier Synchronization**: All regions sync before next iteration
3. **Decoupled Communication**: No direct actor-to-actor references needed

## PlexSpaces API Usage

### Ghost Cell Exchange via TupleSpace

```rust
use plexspaces_tuplespace::TupleSpace;
use plexspaces_core::RequestContext;

let tuplespace = TupleSpace::with_tenant_namespace("heat-sim", "diffusion");
let ctx = RequestContext::new_without_auth("heat-sim".into(), "diffusion".into());

// WRITE: Region publishes its boundary
tuplespace.write(&ctx, tuple!["boundary", iteration, region_id, "south", boundary_data]).await?;

// READ: Region receives neighbor's boundary  
let tuple = tuplespace.read(&ctx, pattern!["boundary", iteration, neighbor_id, "north", _]).await?;
let ghost_cells = extract_data(tuple);
```

### Barrier Synchronization

```rust
// All regions must reach barrier before next iteration
tuplespace.barrier(&ctx, format!("iteration_{}", iter), num_regions).await?;
```

## Architecture

```
┌──────────────┐          ┌──────────────┐
│  Region 0    │          │  Region 1    │
│  (Actor)     │          │  (Actor)     │
└──────┬───────┘          └──────┬───────┘
       │ write south              │ write north
       ▼                          ▼
┌─────────────────────────────────────────┐
│            TupleSpace                   │
│  ["boundary", iter, region, edge, data] │
└─────────────────────────────────────────┘
       │ read north               │ read south
       ▼                          ▼
┌──────────────┐          ┌──────────────┐
│  compute()   │          │  compute()   │
└──────────────┘          └──────────────┘
```

## Expected Output

```
Step 2: Run diffusion with TupleSpace ghost cell exchange
  Iteration 1: WRITE phase
    Region 0 writes south boundary to TupleSpace: ["0.0", "0.0", ...]
    Region 1 writes north boundary to TupleSpace: ["100.0", "100.0", ...]
    Region 0 reads from TupleSpace (south neighbor): ["100.0", ...]
    Region 1 reads from TupleSpace (north neighbor): ["0.0", ...]
    Region 0 after compute: ["0.0", "25.0", "25.0", ...]
    Region 1 after compute: ["100.0", "75.0", "75.0", ...]
    Max diff: 25.00

  Converged at iteration 4 (diff 0.39 < 0.50)
```

## Key APIs

| Operation | PlexSpaces API |
|-----------|----------------|
| Publish boundary | `tuplespace.write(&ctx, tuple![...])` |
| Receive boundary | `tuplespace.read(&ctx, pattern![...])` |
| Sync iterations | `tuplespace.barrier(&ctx, name, count)` |

## Use Cases

- **Thermal simulation**: Heat flow in materials
- **Image processing**: Blur, edge detection (stencil operations)
- **Weather modeling**: Temperature/pressure diffusion
- **Finite element analysis**: Neighbor-dependent computations

## See Also

- [Matrix Multiply](../matrix_multiply/) - Actor-based parallel computation
- [MPI Collectives](../mpi_collectives/) - Collective operations
- [Architecture Docs](../../../../docs/architecture.md)
