# N-Body Simulation Actor (Python WASM with SDK)

Parallel gravitational simulation using Python WASM actors.

**Real-world use case**: Astrophysics simulations, molecular dynamics, game physics.

## PlexSpaces Python SDK

This example uses the [PlexSpaces Python SDK](../../../../sdks/python/README.md):

```python
from plexspaces import actor, state, handler
import math

G = 6.67430e-11  # Gravitational constant

@actor
class NBodyActor:
    mass: float = state(default=1.0e24)
    position: list = state(default_factory=lambda: [0.0, 0.0, 0.0])
    velocity: list = state(default_factory=lambda: [0.0, 0.0, 0.0])
    
    @handler("add_force")
    def add_force(self, fx: float, fy: float, fz: float) -> dict:
        self.accumulated_force[0] += fx
        return {"status": "ok"}
```

**Before SDK**: 210 lines with manual WIT interface  
**After SDK**: 130 lines with decorators

## Quick Start

```bash
./build.sh  # Build WASM actor (~40MB)
./test.sh   # Run tests (requires PlexSpaces node)
```

### Start Node

```bash
cargo run -p plexspaces-cli -- start --node-id nbody-node --listen-addr 0.0.0.0:8090
```

## Actor Commands

| Command | Payload | Description |
|---------|---------|-------------|
| add_force | `{"fx":1.0,"fy":0.0,"fz":0.0}` | Add force vector |
| update | `{"dt":3600}` | Apply forces, update position |
| calculate_force | `{"mass":1e24,"position":[1e9,0,0]}` | Calculate force from body |
| reset | `{}` | Reset accumulated forces |
| get_state | `{}` | Return body state |

## Physics

```
F = G * m1 * m2 / r²    (Gravitational force)
a = F / m               (Acceleration)
v = v₀ + a * dt         (Velocity update)
x = x₀ + v * dt         (Position update)
```

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@actor` | Marks `NBodyActor` as PlexSpaces actor |
| `state()` | Defines `mass`, `position`, `velocity` |
| `@handler()` | Routes add_force, update, calculate_force |
| `@init_handler` | Initializes body from config |

## Files

| File | Description |
|------|-------------|
| `nbody_actor.py` | N-body physics using SDK |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Integration test |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - Complete SDK reference
- [Calculator Actor](../calculator/) - Simpler math operations
