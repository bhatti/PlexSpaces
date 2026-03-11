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

### Benchmarks and Metrics (always printed to stdout)

Every run prints a **BENCHMARKS** block so the framework’s behavior on larger HPC-style data is visible:

- **Data size**: Grid dimensions, iterations, total points (stencil updates)
- **Execution**: Wall time, compute time vs coordination time (%), efficiency
- **Latency & throughput**: Message count, barriers, avg latency per message, msg/s
- **Granularity**: compute/coord ratio (target: >10x for good scaling)
- **Errors**: Count (0 on success)

Goal: show that the framework can handle non-trivial data sizes with clear compute vs coord breakdown.

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
./test.sh
```

Optional: pass a port or list of `host:port` for multi-node convention (see [Multi-node and realistic benchmarks](../../../README.md#multi-node-and-realistic-benchmarks-plan)):

```bash
./test.sh 8092
./test.sh "localhost:8092 localhost:8094"
```

Currently the example runs a single in-process node; `PLEXSPACES_PEERS` is set when a list is given for future use. Run directly: `cargo run`

## Multi-node: one run, work split across nodes (leader-worker / data-parallel)

Multi-node parallelization means **one** simulation run with work **split across nodes**, not one run per node.

### How this example parallelizes (one run, many regions)

- **Single logical run**: One grid, one iteration loop; the driver sends **compute** to all region actors each iteration and collects results until convergence.
- **Region actors**: The grid is split into **8 horizontal regions**. Each region is a **GridRegionActor** (GenServer). The driver spawns all 8 with the SDK `spawn()` and sends **compute** each iteration — so **one** run uses 8 actors doing stencil work in parallel.
- **TupleSpace**: Neighbors exchange ghost cells via **TupleSpace** (`write` / `read` boundaries). **Barrier** synchronizes each iteration. APIs: `spawn()`, `GenServerRef.call()`, `ActorContext::get_tuplespace()`, `tuplespace.write()`, `tuplespace.read()`, `tuplespace.barrier()`, **CoordinationComputeTracker**.

Today all 8 regions run on **one node**. True multi-node would be: **one** run, with regions **placed on different nodes** (e.g. regions 0–2 on node A, 3–5 on node B, 6–7 on node C). The driver (or a leader) would spawn region actors on the right nodes (via **remote spawn**: `ActorService.SpawnActor` on each node’s gRPC channel), then send compute to `region_id@node_id`. TupleSpace would need shared backing across nodes (shared DB) or be replaced with direct message passing between neighbor regions.

### Testing with multiple nodes (future)

The test script accepts an optional **host:port list** and sets **`PLEXSPACES_PEERS`** for a future binary that connects to other nodes (e.g. **ConnectNodes**), spawns region actors on those nodes, and runs the **same** single simulation with work split across nodes. See [What multi-node parallelization means](../../../README.md#what-multi-node-parallelization-means-one-run-work-split) and [Multi-node and realistic benchmarks](../../../README.md#multi-node-and-realistic-benchmarks-plan).

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

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  BENCHMARKS (compute vs coord, latency, data size)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  Data size:
    Grid:           1000 columns × 8 regions = 8000 points
    Iterations:     25
    Total points:   200000 (stencil updates)

  Execution:
    Wall time:     250.00 ms  (0.25 s)
    Compute time:  200.00 ms  (80.0%)
    Coord time:    50.00 ms  (20.0%)

  Latency & throughput:
    Messages:      200   Barriers: 25
    Avg latency:   1.00 ms/msg   Throughput: 800.0 msg/s

  Granularity (compute/coord):  4.00x
  Errors: 0
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Heat Diffusion Example Complete
```

## Real-World Use Cases

- **Thermal Simulation**: Heat transfer in materials, engines, buildings
- **Image Processing**: Gaussian blur, edge detection (stencil operations)
- **Weather Modeling**: Temperature, pressure, humidity diffusion
- **Fluid Dynamics**: Navier-Stokes solvers (stencil-based)

## Design Principles

- **Abstractions/APIs**: Uses SDK only (see [Example evaluation](../../../README.md#example-evaluation-right-abstractions--apis--sdks)): `#[gen_server_actor]`, `spawn()`, `GenServerRef`, TupleSpace; no ActorFactory or manual `Message::new`.
- **SDK Patterns**: Use annotations and helpers, not low-level APIs
- **Tenant Isolation**: Explicit RequestContext with tenant/namespace
- **Observability**: CoordinationComputeTracker for metrics
- **Non-trivial Data**: 1000 columns × 8 regions (runs 2+ seconds)
- **Shared target**: Uses workspace shared target directory (`.cargo/config.toml`). **Debug** build by default (no `--release`)

## References

- [Architecture](../../../../docs/architecture.md)
- [TupleSpace Coordination](../../../../docs/detailed-design.md#tuplespace)
- [Getting Started](../../../../docs/getting-started.md)
