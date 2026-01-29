# N-Body Simulation Actor (Python WASM)

> **📝 STATUS: RUNTIME IMPLEMENTED** - The runtime's WASM Component Model `handle_message` is now implemented.
> 
> The runtime uses wasmtime's `bindgen!`-generated `PlexspacesActor` bindings for typed export calls.
> Actors deploy successfully and are created. Message handling calls the component's exports.
> 
> **Note**: Python components need to implement the exact WIT interface (returning `actor-result` variant).
> See `wit/plexspaces-actor/actor.wit` for the expected interface.

**Purpose**: Demonstrate parallel gravitational simulation using Python WASM actors deployed to PlexSpaces.

**PlexSpaces APIs**: `handle_message()`, JSON payloads, WASM deployment

## Overview

Each celestial body is represented as a Python WASM actor. Bodies receive force updates from a coordinator and compute new positions using Newtonian physics.

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Body 0    │     │   Body 1    │     │   Body 2    │
│  (Earth)    │     │   (Moon)    │     │   (Sun)     │
└──────┬──────┘     └──────┬──────┘     └──────┬──────┘
       │                   │                   │
       │ ← calculate_force →                   │
       │                   │                   │
       ├── add_force(fx, fy, fz) ──────────────┤
       │                   │                   │
       ├── update(dt=3600s) ───────────────────┤
       │                   │                   │
       └── get_state() ────────────────────────┘
```

## Prerequisites

1. **componentize-py** - Python to WASM compiler:
   ```bash
   pip install componentize-py
   # Or activate virtualenv that has it:
   source ~/venv/bin/activate
   ```

2. **PlexSpaces** - Built from source:
   ```bash
   cd /path/to/tspaces
   cargo build -p plexspaces-cli
   ```

## Quick Start

### Terminal 1: Start PlexSpaces Node

```bash
cd /path/to/tspaces
cargo run -p plexspaces-cli -- start --node-id nbody-node --listen-addr 0.0.0.0:8090
```

### Terminal 2: Build and Test

```bash
cd examples/python/apps/nbody

# Build the Python WASM actor (~40MB, includes Python runtime)
./build.sh

# Deploy and test
./test.sh
```

## Manual Testing

```bash
# Check node status
cargo run -p plexspaces-cli -- status --node localhost:8090

# Deploy a body actor
cargo run -p plexspaces-cli -- deploy \
    --node localhost:8090 \
    -i body-earth \
    -n body-earth \
    -w examples/python/apps/nbody/nbody_actor.wasm

# List deployed applications
cargo run -p plexspaces-cli -- list --node localhost:8090

# Undeploy when done
cargo run -p plexspaces-cli -- undeploy --node localhost:8090 --app-id body-earth
```

## Actor Commands

| Command | Payload | Response | Description |
|---------|---------|----------|-------------|
| `add_force` | `{"fx": float, "fy": float, "fz": float}` | `{"status": "force_added", "total_force": [...]}` | Add force vector |
| `update` | `{"dt": float}` | Position/velocity/acceleration | Apply forces, update position |
| `get_state` | `{}` | Full body state | Return current state |
| `calculate_force` | `{"mass": float, "position": [x,y,z]}` | `{"fx": ..., "fy": ..., "fz": ..., "distance": ...}` | Calculate force from other body |
| `reset` | `{}` | `{"status": "reset"}` | Reset accumulated forces |

## Actor State

```python
_body = {
    "id": "body-0",           # Body identifier
    "mass": 1.0e24,           # Mass in kg (Earth-like)
    "position": [0.0, 0.0, 0.0],  # [x, y, z] in meters
    "velocity": [0.0, 0.0, 0.0],  # [vx, vy, vz] in m/s
}
_accumulated_force = [0.0, 0.0, 0.0]  # Force accumulator
_step_count = 0                       # Simulation step counter
```

## Physics

```
Gravitational Force: F = G * m1 * m2 / r²
Acceleration: a = F / m
Velocity update: v = v₀ + a * dt
Position update: x = x₀ + v * dt

G = 6.67430e-11 m³ kg⁻¹ s⁻²
```

## File Structure

```
nbody/
├── nbody_actor.py    # Python WASM actor implementation
├── nbody_actor.wasm  # Compiled WASM (~40MB with Python runtime)
├── build.sh          # Build script (uses componentize-py)
├── test.sh           # Deploy and test script
└── README.md         # This file
```

## Building Details

The actor is compiled to WASM using `componentize-py`:

```bash
source ~/venv/bin/activate  # Activate virtualenv with componentize-py

componentize-py \
    -d ../../../../wit/plexspaces-actor \
    -w plexspaces-actor \
    componentize \
    -o nbody_actor.wasm \
    nbody_actor
```

**Note**: Python WASM files are ~40MB because they bundle the complete Python runtime. This is expected behavior for componentize-py.

## Known Limitations

1. **WIT Interface Compliance**: componentize-py produces **WASM Components** (Component Model).
   The Python implementation must match the WIT interface exactly:
   - `handle_message` must return `actor-result` variant (not raw `bytes`)
   - See `wit/plexspaces-actor/types.wit` for variant definitions
   - The runtime's `handle_message_component()` uses typed bindings via `PlexspacesActor`

2. **File Size**: Python WASM is ~40MB due to bundled Python runtime. For smaller WASM, consider Rust or Go actors.

## Use Cases

- **Astrophysics simulations**: Planetary motion, galaxy formation
- **Molecular dynamics**: Protein folding, particle interactions
- **Game physics**: Gravity wells, particle systems
- **Education**: Demonstrate actor-based parallel computation

## See Also

- [Counter Actor](../counter/) - Simple Python WASM actor
- [Calculator Actor](../calculator/) - GenServer pattern
- [PlexSpaces Architecture](../../../../docs/architecture.md)
- [WASM Deployment Guide](../../../../docs/wasm-deployment.md)
