# Service Registry - Service Discovery Example (Python WASM with SDK)

Demonstrates **service discovery** using RegistryFacet for microservices coordination.

**Real-world use case**: Microservices service discovery - services register themselves on startup, clients discover services by type/category/region (similar to Consul, Eureka, Kubernetes service discovery).

## PlexSpaces Python SDK

This example uses the [PlexSpaces Python SDK](../../../../sdks/python/README.md):

```python
from plexspaces import actor, handler, init_handler

@actor
class RegistryActor:
    @handler("get_state", "call")
    def get_info(self) -> dict:
        return {"status": "ok", "actor_type": "registry"}
```

**Before SDK**: 101 lines with manual WIT interface  
**After SDK**: 55 lines with decorators

## How RegistryFacet Works

RegistryFacet uses **message interception** to provide service discovery:

1. **Facet attached** to actor via `app-config.toml`
2. **Facet intercepts** messages with registry operation types
3. **Facet handles** operations using ObjectRegistry backend
4. **Actor's handlers** are only called for non-intercepted messages

```
Client sends message → RegistryFacet intercepts → ObjectRegistry → Response
                                    ↓
                        Actor.handle() NOT called
```

### RegistryFacet Operations (Intercepted)

- `"register_object"`: Register a service/actor in the registry
- `"unregister_object"`: Unregister a service/actor
- `"lookup_object"`: Lookup a specific service/actor by ID
- `"discover_objects"`: Discover services/actors with filters (`offset`, then `limit`)

## Quick Start

```bash
./build.sh  # Build WASM actor
./test.sh   # Run tests (requires PlexSpaces node)
```

### Start Node

```bash
# Terminal 1: Start node
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093

# Terminal 2: Run tests
cd examples/python/apps/registry
./test.sh 8094  # HTTP gateway port
```

## Operations

### Register Service

```json
{
  "msg_type": "register_object",
  "payload": {
    "object_id": "payment-service-1",
    "object_type": "Service",
    "object_category": "payment",
    "grpc_address": "http://payment-service:50051",
    "labels": ["production", "us-east"]
  }
}
```

### Discover Services

```json
{
  "msg_type": "discover_objects",
  "payload": {
    "object_type": "Service",
    "labels": ["us-east"],
    "offset": 0,
    "limit": 10
  }
}
```

## Configuration

The `app-config.toml` attaches RegistryFacet to the actor:

```toml
[[supervisor.children]]
id = "registry"
role = "worker"
facets = [
  { type = "registry", priority = 50, config = {} }
]
```

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@actor` | Marks `RegistryActor` as PlexSpaces actor |
| `@handler()` | Routes non-intercepted messages |
| `@init_handler` | Initializes actor from config |

## Files

| File | Description |
|------|-------------|
| `registry_actor.py` | Registry actor using SDK |
| `app-config.toml` | ApplicationSpec with RegistryFacet |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Integration test |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - Complete SDK reference
- [RegistryFacet Documentation](../../../../crates/facet/src/capabilities/registry.rs)
- [Actor System Documentation](../../../../docs/actor-system.md#registryfacet)
