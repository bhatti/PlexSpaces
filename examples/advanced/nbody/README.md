# N-Body Simulation Example

## Overview

This example demonstrates **parallel N-body gravitational simulation** using PlexSpaces actors. Each body in the simulation is represented as an actor that calculates gravitational forces from other bodies.

## Features Demonstrated

- **Actor-based parallel computation**: Each body is an actor
- **ConfigBootstrap**: Erlang-style configuration loading
- **NodeBuilder/ActorBuilder**: Simplified actor creation
- **CoordinationComputeTracker**: Performance metrics tracking
- **Message-based coordination**: Actors communicate via messages

## The N-Body Problem

The N-body problem simulates the motion of N particles under gravitational forces. Each body experiences a force from every other body according to Newton's law of universal gravitation:

**F = G × m₁ × m₂ / r²**

Where:
- **F** = gravitational force
- **G** = gravitational constant (6.67430×10⁻¹¹ m³ kg⁻¹ s⁻²)
- **m₁, m₂** = masses of the two bodies
- **r** = distance between the bodies

## Architecture

### System Diagram

```
┌──────────────────────────────────────────────────────────┐
│  N-Body Simulation System                                 │
│                                                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐      │
│  │ Body Actor  │  │ Body Actor  │  │ Body Actor  │      │
│  │   (Sun)     │  │  (Earth)    │  │   (Moon)    │      │
│  └─────────────┘  └─────────────┘  └─────────────┘      │
│         │                 │                 │             │
│         └─────────────────┴─────────────────┘             │
│                           │                               │
│                           ▼                               │
│                  ┌──────────────────┐                     │
│                  │  Coordinator     │                     │
│                  │  (Main Loop)     │                     │
│                  │                  │                     │
│                  │  - Collect states│                     │
│                  │  - Calculate     │                     │
│                  │    forces        │                     │
│                  │  - Update        │                     │
│                  │    positions     │                     │
│                  └──────────────────┘                     │
└──────────────────────────────────────────────────────────┘
```

### Components

#### 1. **Body Actor** (`src/body_actor.rs`)

Each body is an actor that:
- Maintains its position, velocity, and mass
- Calculates gravitational forces from other bodies
- Updates its position and velocity based on forces

**State**:
```rust
struct Body {
    id: String,
    mass: f64,
    position: [f64; 3],
    velocity: [f64; 3],
}
```

**Messages**:
- `get_state` - Request current body state
- `{"force": [fx, fy, fz]}` - Add force from another body
- `{"dt": 1.0}` - Apply accumulated forces and update position

#### 2. **Coordinator** (`src/main.rs`)

The main simulation loop:
- Spawns body actors
- Collects states from all bodies
- Calculates forces between bodies
- Updates body positions
- Tracks coordination vs compute metrics

## Configuration

### Using ConfigBootstrap

Configuration is loaded from `release.toml`:

```toml
[nbody]
body_count = 3
steps = 10
dt = 1.0
use_3body_system = true
```

Or via environment variables:
```bash
export NBODY_BODY_COUNT=5
export NBODY_STEPS=20
export NBODY_DT=0.5
```

### Configuration Options

- `body_count`: Number of bodies to simulate (default: 3)
- `steps`: Number of simulation steps (default: 10)
- `dt`: Time step in seconds (default: 1.0)
- `use_3body_system`: Use Sun-Earth-Moon system if true (default: true)

## Running the Example

### Quick Start

```bash
cd examples/nbody

# Run with scripts (shows metrics)
./scripts/run.sh

# Or run directly
cargo run --release --bin nbody
```

### Using Scripts

#### Run Example with Metrics
```bash
./scripts/run.sh
```

This will:
- Build the example in release mode
- Run the simulation
- Display metrics (coordination vs compute time, granularity ratio)

#### Run All Tests
```bash
./scripts/run_tests.sh
```

This will:
- Run unit tests
- Run integration tests
- Build binary
- Run example with metrics
- Display test summary

## Metrics

The example uses `CoordinationComputeTracker` to track performance metrics:

### Metrics Displayed

When running the example, you'll see:

```
📊 Performance Metrics:
   Coordination time: 12.34 ms
   Compute time: 45.67 ms
   Granularity ratio: 3.70x
   Efficiency: 78.74%
```

### Metrics Explained

- **Coordination time**: Time spent in message passing and state collection
- **Compute time**: Time spent in force calculations and position updates
- **Granularity ratio**: `compute_time / coordinate_time` (target: > 1.0 for good performance)
- **Efficiency**: `compute_time / total_time` (target: > 50% for good efficiency)

## Testing

### Run All Tests

```bash
# Using script (recommended)
./scripts/run_tests.sh

# Or directly
cargo test
```

### Test Suites

- `nbody_integration` - Integration tests for body actors

### Example Test

```bash
# Run integration tests
cargo test --test nbody_integration
```

## Key Features Demonstrated

1. **ConfigBootstrap**: Erlang-style configuration loading from `release.toml`
2. **NodeBuilder/ActorBuilder**: Simplified actor creation patterns
3. **CoordinationComputeTracker**: Performance metrics tracking
4. **ActorBehavior**: Message-based actor communication
5. **Parallel Computation**: Each body calculates forces independently

## Algorithm

### Simulation Loop

For each simulation step:

1. **Collect States** (Coordination):
   - Request state from all body actors
   - Wait for responses

2. **Calculate Forces** (Computation):
   - For each body, calculate force from all other bodies
   - Send force updates to body actors

3. **Update Positions** (Computation):
   - Apply accumulated forces to each body
   - Update position and velocity based on time step

4. **Display Results**:
   - Show current positions of all bodies
   - Display performance metrics

## Success Criteria

1. ✅ **Correctness**: Bodies move according to gravitational forces
2. ✅ **Parallelism**: Multiple bodies computed in parallel
3. ✅ **Performance**: Compute time dominates coordination time
4. ✅ **Scalability**: Works with 3 to 100+ bodies
5. ✅ **Metrics**: Tracks coordination vs compute overhead

## WASM Version

A separate WASM version of this example will be created to showcase:
- TypeScript-based actor implementation
- Application-level WASM deployment (not individual actors)
- Using CLI to deploy WASM application modules
- Framework support for packaging WASM applications

The WASM version will be located in a separate example directory.

## References

- [N-body problem](https://en.wikipedia.org/wiki/N-body_problem)
- [Newton's law of universal gravitation](https://en.wikipedia.org/wiki/Newton%27s_law_of_universal_gravitation)
- PlexSpaces Documentation: `docs/`
