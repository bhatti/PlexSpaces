"""
Registry Actor - Service Discovery (Python WASM with SDK)

Demonstrates using object registry for service discovery from WASM actors.
Uses RegistryFacet which intercepts registry operation messages and uses the real
ObjectRegistry backend.

Real-world use case: Microservices service discovery, actor discovery, finding
services by type/category (similar to Consul, Eureka, Kubernetes service discovery).

## How It Works

1. RegistryFacet is attached to the actor via app-config.toml
2. External clients send messages with registry operation types
3. RegistryFacet intercepts these messages and handles them using ObjectRegistry
4. The actor's handlers are only called for non-registry operations

## RegistryFacet Operations (Intercepted)

- "register_object": Register an object in the registry
- "unregister_object": Unregister an object
- "lookup_object": Lookup an object by ID
- "discover_objects": Discover objects with filters

## SDK Features Used

- @actor: Marks class as PlexSpaces actor
- @handler(): Routes non-intercepted messages
"""

from plexspaces import actor, handler, init_handler


@actor
class RegistryActor:
    """Registry actor using RegistryFacet for service discovery.
    
    Note: Registry operations (register_object, unregister_object, etc.) are intercepted
    by RegistryFacet and never reach this class's handlers.
    """
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize registry actor."""
        # Actor can have its own config, but registry operations are handled by facet
        pass
    
    @handler("get_state", "call")
    def get_info(self) -> dict:
        """Return actor info for non-registry operations."""
        return {
            "status": "ok",
            "actor_type": "registry",
            "note": "Registry operations (register_object, unregister_object, lookup_object, discover_objects) are handled by RegistryFacet"
        }
