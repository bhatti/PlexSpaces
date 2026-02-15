# Heat Diffusion - Thermal Simulation with TupleSpace Coordination

**Real-world use case**: Thermal simulation, image processing, weather modeling - parallel stencil computation with ghost cell exchange between distributed regions.

**Pattern**: TupleSpace coordination for neighbor communication, barrier synchronization for iteration control.

## Overview

This example demonstrates parallel stencil computation (5-point stencil for heat diffusion) using PlexSpaces actors and TupleSpace coordination. Each actor manages a horizontal strip of a 2D grid and exchanges boundary values (ghost cells) with neighbors via TupleSpace.

## Architecture

### GridRegionActor

Each actor manages a horizontal strip of the grid:
- **State**: Current temperature values (1D array)
- **Computation**: 5-point stencil (average of north, south, east, west neighbors)
- **Coordination**: Writes boundary values to TupleSpace, reads neighbor boundaries

### TupleSpace Coordination Pattern

1. **Write Phase**: Each region publishes its north and south boundaries to TupleSpace
2. **Read Phase**: Each region reads neighbor boundaries from TupleSpace
3. **Compute Phase**: Update values using stencil with ghost cells
4. **Barrier Phase**: Synchronize all regions before next iteration

### Coordination vs. Computation Metrics

- **Coordination**: TupleSpace operations (write, read, barrier)
- **Computation**: Stencil computation (actual work)
- **Granularity Ratio**: compute_time / coordinate_time (target: >10x)

## SDK Features Demonstrated

- `#[gen_server_actor]` - Declares GenServer behavior
- `#[plexspaces_handlers(gen_server)]` - Auto-generated message dispatch
- `#[handler("compute")]` - Iteration handler
- `spawn()` - SDK helper for spawning actors
- `GenServerRef.call()` - Request-reply messaging
- `ActorContext::get_tuplespace()` - Access TupleSpace from actor

## TupleSpace Operations

- **Write**: `tuplespace.write(tuple)` - Publish boundary values
- **Read**: `tuplespace.read(pattern)` - Get neighbor boundaries
- **Barrier**: `tuplespace.barrier(name, pattern, count)` - Synchronize iterations

## Quick Start

```bash
cd examples/rust/embedded/heat_diffusion
cargo run --bin heat_diffusion
```

## Expected Output

```
╔════════════════════════════════════════════════════════════════╗
║       Heat Diffusion with TupleSpace Coordination              ║
╚════════════════════════════════════════════════════════════════╝

Configuration:
  Grid width: 1000 columns
  Regions: 8 horizontal strips
  Max iterations: 100
  Tolerance: 0.5

Step 1: Spawn 8 region actors
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Spawned region-0 (initial temp: 0.0°C)
  Spawned region-1 (initial temp: 33.3°C)
  Spawned region-2 (initial temp: 66.7°C)
  Spawned region-3 (initial temp: 100.0°C)

Step 2: Run diffusion with TupleSpace ghost cell exchange
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Iteration 1: max_diff=25.0000, time=12.34ms
  Iteration 10: max_diff=2.5000, time=11.23ms
  Iteration 20: max_diff=0.2500, time=10.12ms
  Converged at iteration 25 (diff 0.4500 < 0.50)

Step 3: Performance Metrics
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Total execution time: 250.00ms
Iterations completed: 25

Coordination Metrics:
  Coordination time: 50.00ms (20.0%)
  Computation time: 200.00ms (80.0%)
  Messages sent: 100
  Barriers: 25

Granularity ratio (compute/coordinate): 4.00
  ⚠️  Moderate granularity (coordination overhead is noticeable)
```

## Real-World Use Cases

- **Thermal Simulation**: Heat transfer in materials, engines, buildings
- **Image Processing**: Gaussian blur, edge detection (stencil operations)
- **Weather Modeling**: Temperature, pressure, humidity diffusion
- **Fluid Dynamics**: Navier-Stokes solvers (stencil-based)

## Design Principles

- **SDK Patterns**: Use annotations and helpers, not low-level APIs
- **Tenant Isolation**: Explicit RequestContext with tenant/namespace
- **Observability**: CoordinationComputeTracker for metrics
- **Non-trivial Data**: 200 columns × 4 regions = 800 cells (runs 2+ seconds)
- **Shared Target**: Uses workspace shared target directory

## References

- [Architecture](../../../../docs/architecture.md)
- [TupleSpace Coordination](../../../../docs/detailed-design.md#tuplespace)
- [Getting Started](../../../../docs/getting-started.md)
