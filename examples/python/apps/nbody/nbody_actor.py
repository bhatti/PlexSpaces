#!/usr/bin/env python3
"""
N-Body Simulation Actor (Python WASM)

Demonstrates parallel gravitational simulation using PlexSpaces actors.
Each body is an actor that receives force updates and computes new positions.

Commands:
- "add_force": Add gravitational force vector {"fx": float, "fy": float, "fz": float}
- "update": Apply accumulated forces with time step {"dt": float}
- "get_state": Return current body state
- "calculate_force": Calculate gravitational force from another body
- "reset": Reset to initial state

Use Case: Scientific simulations, particle systems, game physics

NOTE: This actor uses the simplified string-only WIT interface for
componentize-py compatibility. All data is passed as JSON strings.
"""

import json
import math
from typing import Optional
from wit_world import exports

# Gravitational constant (m^3 kg^-1 s^-2)
G = 6.67430e-11

# Actor state (module-level for persistence)
_body = {
    "id": "body-0",
    "mass": 1.0e24,  # kg (Earth-like mass)
    "position": [0.0, 0.0, 0.0],  # [x, y, z] in meters
    "velocity": [0.0, 0.0, 0.0],  # [vx, vy, vz] in m/s
}
_accumulated_force = [0.0, 0.0, 0.0]
_step_count = 0


class Actor(exports.Actor):
    """
    N-Body Actor implementing PlexSpaces simple actor interface.
    
    Uses string-only interface for componentize-py compatibility.
    All data is passed as JSON strings.
    """
    
    def init(self, config_json: str) -> str:
        """
        Initialize body actor with JSON config.
        Returns empty string on success, error message on failure.
        """
        global _body, _accumulated_force, _step_count
        
        if config_json:
            try:
                state = json.loads(config_json)
                _body["id"] = state.get("id", "body-0")
                _body["mass"] = state.get("mass", 1.0e24)
                _body["position"] = state.get("position", [0.0, 0.0, 0.0])
                _body["velocity"] = state.get("velocity", [0.0, 0.0, 0.0])
            except Exception as e:
                return f"ERROR: Failed to parse config: {e}"
        
        _accumulated_force = [0.0, 0.0, 0.0]
        _step_count = 0
        return ""  # Success
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """
        Handle incoming messages (both sync and async).
        
        Returns JSON response string, or "ERROR:message" on failure.
        
        Message types:
        - add_force: Add force vector to accumulated force
        - update: Apply forces and update position/velocity  
        - get_state: Return current body state
        - calculate_force: Calculate force from another body
        - reset: Reset accumulated forces
        """
        global _body, _accumulated_force, _step_count
        
        try:
            data = json.loads(payload_json) if payload_json else {}
        except Exception as e:
            return f"ERROR: Invalid JSON payload: {e}"
        
        if msg_type == "add_force":
            # Add force vector to accumulated force
            fx = data.get("fx", 0.0)
            fy = data.get("fy", 0.0)
            fz = data.get("fz", 0.0)
            _accumulated_force[0] += fx
            _accumulated_force[1] += fy
            _accumulated_force[2] += fz
            return json.dumps({"status": "ok", "accumulated_force": _accumulated_force})
        
        elif msg_type == "update":
            # Apply accumulated forces with timestep
            dt = data.get("dt", 1.0)
            mass = _body["mass"]
            
            # F = ma -> a = F/m
            ax = _accumulated_force[0] / mass
            ay = _accumulated_force[1] / mass
            az = _accumulated_force[2] / mass
            
            # Update velocity: v = v + a*dt
            _body["velocity"][0] += ax * dt
            _body["velocity"][1] += ay * dt
            _body["velocity"][2] += az * dt
            
            # Update position: x = x + v*dt
            _body["position"][0] += _body["velocity"][0] * dt
            _body["position"][1] += _body["velocity"][1] * dt
            _body["position"][2] += _body["velocity"][2] * dt
            
            # Reset accumulated force for next step
            _accumulated_force = [0.0, 0.0, 0.0]
            _step_count += 1
            
            return json.dumps({
                "status": "ok",
                "step": _step_count,
                "position": _body["position"],
                "velocity": _body["velocity"]
            })
        
        elif msg_type == "calculate_force":
            # Calculate gravitational force from another body
            other_mass = data.get("mass", 1.0e24)
            other_pos = data.get("position", [0.0, 0.0, 0.0])
            
            # Vector from this body to other body
            dx = other_pos[0] - _body["position"][0]
            dy = other_pos[1] - _body["position"][1]
            dz = other_pos[2] - _body["position"][2]
            
            # Distance
            r_squared = dx*dx + dy*dy + dz*dz
            if r_squared < 1e-10:
                return json.dumps({"fx": 0.0, "fy": 0.0, "fz": 0.0})
            
            r = math.sqrt(r_squared)
            
            # Gravitational force magnitude: F = G * m1 * m2 / r^2
            force_mag = G * _body["mass"] * other_mass / r_squared
            
            # Force direction (unit vector)
            fx = force_mag * dx / r
            fy = force_mag * dy / r
            fz = force_mag * dz / r
            
            return json.dumps({"fx": fx, "fy": fy, "fz": fz})
        
        elif msg_type == "reset":
            _accumulated_force = [0.0, 0.0, 0.0]
            return json.dumps({"status": "ok"})
        
        elif msg_type == "get_state" or msg_type == "call":
            # Return current state (default handler)
            return json.dumps({
                "id": _body["id"],
                "mass": _body["mass"],
                "position": _body["position"],
                "velocity": _body["velocity"],
                "accumulated_force": _accumulated_force,
                "step": _step_count,
                "msg_type_received": msg_type,
                "from_actor": from_actor
            })
        
        else:
            # Unknown message type - return state with info
            return json.dumps({
                "status": "unknown_message_type",
                "msg_type": msg_type,
                "state": {
                    "id": _body["id"],
                    "position": _body["position"],
                    "velocity": _body["velocity"]
                }
            })
    
    def get_state(self) -> str:
        """Get actor state as JSON for persistence/snapshotting."""
        global _body, _accumulated_force, _step_count
        return json.dumps({
            "body": _body,
            "accumulated_force": _accumulated_force,
            "step_count": _step_count
        })
    
    def set_state(self, state_json: str) -> str:
        """
        Restore actor state from JSON.
        Returns empty string on success, error message on failure.
        """
        global _body, _accumulated_force, _step_count
        
        try:
            state = json.loads(state_json)
            _body = state.get("body", _body)
            _accumulated_force = state.get("accumulated_force", [0.0, 0.0, 0.0])
            _step_count = state.get("step_count", 0)
            return ""  # Success
        except Exception as e:
            return f"ERROR: Failed to restore state: {e}"
