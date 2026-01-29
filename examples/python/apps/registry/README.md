# Service Registry - Service Discovery Example (Python WASM)

Demonstrates **service discovery** using RegistryFacet for microservices coordination.

**Real-world use case**: Microservices service discovery - services register themselves on startup, clients discover services by type/category/region (similar to Consul, Eureka, Kubernetes service discovery, AWS Service Discovery).

## How RegistryFacet Works

RegistryFacet uses **message interception** to provide service discovery capabilities:

1. **Facet attached** to actor via `app-config.toml`
2. **Facet intercepts** messages with registry operation types (`register_object`, `lookup_object`, etc.)
3. **Facet handles** operations using real ObjectRegistry backend from ServiceLocator
4. **Actor's handle()** method is never called for intercepted messages
5. **Backend configured** via node-config/runtimeconfig (not hardcoded)

### Message Interception Pattern

```
Client sends message → RegistryFacet intercepts → ObjectRegistry backend → Response
                                    ↓
                        Actor.handle() is NOT called
```

### RegistryFacet Operations (Intercepted)

- `"register_object"`: Register a service/actor in the registry
- `"unregister_object"`: Unregister a service/actor
- `"lookup_object"`: Lookup a specific service/actor by ID
- `"discover_objects"`: Discover services/actors with filters (type, category, labels, health status)

## Real-World Scenario

**E-commerce Microservices Architecture:**

```
┌─────────────────────────────────────────────────────────────┐
│                    Service Registry                           │
│              (RegistryFacet + ObjectRegistry)                 │
└───────┬───────────────┬───────────────┬─────────────────────┘
        │               │               │
        ▼               ▼               ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│   Payment    │ │     User     │ │    Order     │
│   Service    │ │   Service    │ │   Service    │
│              │ │              │ │              │
│ Registers on │ │ Registers on │ │ Registers on │
│  startup     │ │  startup     │ │  startup     │
└──────────────┘ └──────────────┘ └──────────────┘
        │               │               │
        └───────────────┴───────────────┘
                      │
                      ▼
            ┌──────────────────┐
            │   API Gateway    │
            │  Discovers and   │
            │  routes requests │
            └──────────────────┘
```

**Flow:**
1. Services start up and register themselves with metadata (type, category, capabilities, region)
2. API Gateway discovers services by type/category when routing requests
3. Services can discover each other for inter-service communication
4. Services unregister on graceful shutdown

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
./test.sh 8094  # HTTP gateway port (not gRPC 8093)
```

### Manual testing (auth disabled – default for local)

Use this when you want to run the registry example without JWT/mTLS:

```bash
# Terminal 1: Disable auth and start node
export PLEXSPACES_DISABLE_AUTH=1
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093

# Terminal 2: Run registry test (no token needed)
cd examples/python/apps/registry
./test.sh 8094
```

### Manual testing (auth enabled – JWT)

When auth is enabled, `/api/v1/actors/...` requires a valid JWT. Create a token and pass it to the test script:

```bash
# Terminal 1: Enable auth and start node (set JWT secret)
export PLEXSPACES_JWT_SECRET=your-secret-key
# Do NOT set PLEXSPACES_DISABLE_AUTH
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093

# Terminal 2: Create JWT and run registry test
cargo run -p plexspaces-cli -- jwt create --tenant-id internal --sub system --roles admin --exp-hours 1 --secret your-secret-key
# Copy the printed token, then:
export PLEXSPACES_AUTH_TOKEN="<paste-token-here>"
cd examples/python/apps/registry
./test.sh 8094
```

The registry example uses path `/api/v1/actors/internal/system/registry`; the JWT must have `tenant_id` matching (e.g. `internal`) or use the same tenant in the path.

## Operations

### Register Service

Services register themselves with metadata:

```json
{
  "msg_type": "register_object",
  "payload": {
    "object_id": "payment-service-1",
    "object_type": "Service",
    "object_category": "payment",
    "grpc_address": "http://payment-service:50051",
    "capabilities": ["process_payment", "refund"],
    "labels": ["production", "us-east"],
    "health_status": "Healthy"
  }
}
```

### Lookup Service

Find a specific service by ID:

```json
{
  "msg_type": "lookup_object",
  "payload": {
    "object_id": "payment-service-1",
    "object_type": "Service"
  }
}
```

### Discover Services

Find services by filters (type, category, labels, health status):

```json
{
  "msg_type": "discover_objects",
  "payload": {
    "object_type": "Service",
    "labels": ["us-east"],
    "limit": 10,
    "offset": 0
  }
}
```

### Unregister Service

Services unregister on graceful shutdown:

```json
{
  "msg_type": "unregister_object",
  "payload": {
    "object_id": "payment-service-1",
    "object_type": "Service"
  }
}
```

## Configuration

The `app-config.toml` attaches RegistryFacet to the actor:

```toml
[[supervisor.children]]
id = "registry"
type = "worker"
facets = [
  { type = "registry", priority = 50, config = {} }
]
```

## Key Points

1. **RegistryFacet intercepts** all registry operation messages
2. **Actor's handle()** is never called for registry operations
3. **ObjectRegistry backend** handles all storage (configured via node config)
4. **No actor state** - registry data is stored in ObjectRegistry, not in actor
5. **Works for Rust and WASM** - same message interception pattern

## Use Cases

- **Microservices service discovery**: Services register, clients discover
- **Actor discovery**: Find actors by type/category
- **Load balancing**: Discover healthy services for routing
- **Multi-region**: Discover services by region labels
- **Capability-based routing**: Find services with specific capabilities

## See Also

- [RegistryFacet Documentation](../../../../crates/facet/src/capabilities/registry.rs) - Full RegistryFacet implementation
- [ObjectRegistry Documentation](../../../../crates/object-registry/README.md) - ObjectRegistry backend
- [Actor System Documentation](../../../../docs/actor-system.md#registryfacet) - RegistryFacet usage patterns
- [Task Queue Example](../task-queue/) - Another facet example (LockFacet)
