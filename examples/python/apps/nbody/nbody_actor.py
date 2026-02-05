"""
N-Body Simulation Actor (Python WASM with SDK)

Demonstrates parallel gravitational simulation using PlexSpaces actors.
Each body is an actor that receives force updates and computes new positions.

Use Case: Scientific simulations, particle systems, game physics.

## SDK Features Used

- @actor: Marks class as PlexSpaces actor
- state(): Defines persistent body state
- @handler(): Routes physics operations
"""

import math
from plexspaces import actor, state, handler, init_handler

# Gravitational constant (m^3 kg^-1 s^-2)
G = 6.67430e-11


@actor
class NBodyActor:
    """N-Body simulation actor for gravitational physics."""
    
    # Body properties
    body_id: str = state(default="body-0")
    mass: float = state(default=1.0e24)  # Earth-like mass
    position: list = state(default_factory=lambda: [0.0, 0.0, 0.0])
    velocity: list = state(default_factory=lambda: [0.0, 0.0, 0.0])
    accumulated_force: list = state(default_factory=lambda: [0.0, 0.0, 0.0])
    step_count: int = state(default=0)
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize body with config."""
        self.body_id = config.get("id", "body-0")
        self.mass = config.get("mass", 1.0e24)
        self.position = config.get("position", [0.0, 0.0, 0.0])
        self.velocity = config.get("velocity", [0.0, 0.0, 0.0])
        self.accumulated_force = [0.0, 0.0, 0.0]
        self.step_count = 0
    
    @handler("add_force")
    def add_force(self, fx: float = 0.0, fy: float = 0.0, fz: float = 0.0) -> dict:
        """Add force vector to accumulated force."""
        self.accumulated_force[0] += fx
        self.accumulated_force[1] += fy
        self.accumulated_force[2] += fz
        return {"status": "ok", "accumulated_force": self.accumulated_force}
    
    @handler("update")
    def update_position(self, dt: float = 1.0) -> dict:
        """Apply accumulated forces with timestep."""
        # F = ma -> a = F/m
        ax = self.accumulated_force[0] / self.mass
        ay = self.accumulated_force[1] / self.mass
        az = self.accumulated_force[2] / self.mass
        
        # Update velocity: v = v + a*dt
        self.velocity[0] += ax * dt
        self.velocity[1] += ay * dt
        self.velocity[2] += az * dt
        
        # Update position: x = x + v*dt
        self.position[0] += self.velocity[0] * dt
        self.position[1] += self.velocity[1] * dt
        self.position[2] += self.velocity[2] * dt
        
        # Reset accumulated force
        self.accumulated_force = [0.0, 0.0, 0.0]
        self.step_count += 1
        
        return {
            "status": "ok",
            "step": self.step_count,
            "position": self.position,
            "velocity": self.velocity
        }
    
    @handler("calculate_force")
    def calculate_force(self, mass: float = 1.0e24, position: list = None) -> dict:
        """Calculate gravitational force from another body."""
        if position is None:
            position = [0.0, 0.0, 0.0]
        
        # Vector from this body to other
        dx = position[0] - self.position[0]
        dy = position[1] - self.position[1]
        dz = position[2] - self.position[2]
        
        # Distance
        r_squared = dx*dx + dy*dy + dz*dz
        if r_squared < 1e-10:
            return {"fx": 0.0, "fy": 0.0, "fz": 0.0}
        
        r = math.sqrt(r_squared)
        
        # Gravitational force: F = G * m1 * m2 / r^2
        force_mag = G * self.mass * mass / r_squared
        
        # Force direction
        fx = force_mag * dx / r
        fy = force_mag * dy / r
        fz = force_mag * dz / r
        
        return {"fx": fx, "fy": fy, "fz": fz}
    
    @handler("reset")
    def reset_forces(self) -> dict:
        """Reset accumulated forces."""
        self.accumulated_force = [0.0, 0.0, 0.0]
        return {"status": "ok"}
    
    @handler("get_state", "call")
    def get_body_state(self) -> dict:
        """Return current body state."""
        return {
            "id": self.body_id,
            "mass": self.mass,
            "position": self.position,
            "velocity": self.velocity,
            "accumulated_force": self.accumulated_force,
            "step": self.step_count
        }
