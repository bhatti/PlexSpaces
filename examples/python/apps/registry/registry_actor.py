#!/usr/bin/env python3
"""
Registry Actor - Service Discovery (Python WASM)

Demonstrates using object registry for service discovery from WASM actors.
Uses RegistryFacet which intercepts registry operation messages and uses the real
ObjectRegistry backend (configured via node-config/runtimeconfig, not hardcoded).

Real-world use case: Microservices service discovery, actor discovery, finding
services by type/category (similar to Consul, Eureka, Kubernetes service discovery).

## How It Works

1. RegistryFacet is attached to the actor via app-config.toml
2. External clients send messages with registry operation types
3. RegistryFacet intercepts these messages and handles them using ObjectRegistry
4. The actor's handle() method is never called for registry operations

## RegistryFacet Operations (Intercepted)

- "register_object": Register an object in the registry
- "unregister_object": Unregister an object
- "lookup_object": Lookup an object by ID
- "discover_objects": Discover objects with filters (type, category, labels, etc.)

## Actor Operations (Not Intercepted by Facet)

- "get_state": Get actor state (for persistence)
- "call": Generic call (returns actor info)
"""

import json
from wit_world import exports


class Actor(exports.Actor):
    """Registry actor using RegistryFacet for service discovery.
    
    Note: Registry operations (register_object, unregister_object, etc.) are intercepted
    by RegistryFacet and never reach this class's handle() method.
    """
    
    def init(self, config_json: str) -> str:
        """Initialize registry actor."""
        if config_json:
            try:
                config = json.loads(config_json)
                # Actor can have its own config, but registry operations are handled by facet
                return ""
            except Exception as e:
                return f"ERROR: Failed to parse config: {e}"
        return ""
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """
        Handle non-registry operations.
        
        Note: Registry operations (register_object, unregister_object, lookup_object,
        discover_objects) are intercepted by RegistryFacet and never reach this method.
        This method only handles actor-specific operations.
        
        Message types handled here:
        - "get_state": Get actor state (for persistence)
        - "call": Generic call (returns actor info)
        """
        try:
            if msg_type in ("get_state", "call"):
                # Return actor info
                return json.dumps({
                    "status": "ok",
                    "actor_type": "registry",
                    "note": "Registry operations (register_object, unregister_object, lookup_object, discover_objects) are handled by RegistryFacet"
                })
            
            else:
                # Unknown message type - return error
                # Note: Registry operations should be intercepted by RegistryFacet
                return json.dumps({
                    "status": "error",
                    "error": f"Unknown message type: {msg_type}. Registry operations (register_object, unregister_object, lookup_object, discover_objects) should be intercepted by RegistryFacet."
                })
                
        except Exception as e:
            return json.dumps({
                "status": "error",
                "error": str(e)
            })
    
    def get_state(self) -> str:
        """Get registry actor state as JSON."""
        # Actor doesn't maintain its own registry state - RegistryFacet uses ObjectRegistry backend
        return json.dumps({"actor_type": "registry"})
    
    def set_state(self, state_json: str) -> str:
        """Restore registry actor state from JSON."""
        try:
            # Actor doesn't maintain its own registry state
            return ""
        except Exception as e:
            return f"ERROR: Failed to restore state: {e}"
