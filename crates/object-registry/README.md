# Object Registry - Unified Service Discovery

**Purpose**: Provides unified registration and discovery for all distributed objects in PlexSpaces:
- **Actors**: Stateful computation units (actor model)
- **TupleSpaces**: Coordination primitives (Linda model)
- **Services**: Microservices and gRPC endpoints

## Overview

This crate consolidates three separate registries (ActorRegistry, TupleSpaceRegistry, ServiceRegistry) into ONE unified registry following Proto-First Design principles.

### Component Diagram

```text
┌─────────────────────────────────────────────────────────┐
│                 ObjectRegistry                           │
│  register() / unregister() / lookup() / discover()      │
└────────────────────┬────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────┐
│              KeyValueStore Backend                       │
│  (InMemory, SQLite, Redis, PostgreSQL)                  │
│  Key: {tenant}:{namespace}:{type}:{object_id}           │
│  Value: ObjectRegistration (proto serialized)           │
└─────────────────────────────────────────────────────────┘
```

## Key Components

### ObjectRegistry

Main registry struct with KeyValueStore backend:

```rust
pub struct ObjectRegistry {
    kv_store: Arc<dyn KeyValueStore>,
}
```

### ObjectRegistration

Registration information:

```rust
pub struct ObjectRegistration {
    pub object_id: String,
    pub object_type: ObjectType,
    pub object_category: String,
    pub grpc_address: String,
    pub metadata: HashMap<String, String>,
}
```

## Usage Examples

### Register Actor

```rust
use plexspaces_object_registry::ObjectRegistry;
use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
use plexspaces_keyvalue::InMemoryKVStore;

let kv = Arc::new(InMemoryKVStore::new());
let registry = ObjectRegistry::new(kv);

// Register actor
let registration = ObjectRegistration {
    object_id: "counter@node1".to_string(),
    object_type: ObjectType::ObjectTypeActor as i32,
    object_category: "GenServer".to_string(),
    grpc_address: "http://node1:9001".to_string(),
    ..Default::default()
};

registry.register(registration).await?;
```

### Discover Objects by Type

```rust
// Discover all actors
let actors = registry.discover(
    Some(ObjectType::ObjectTypeActor),
    None,
    None,
    None,
    None,
    100
).await?;
```

## Dependencies

This crate depends on:
- `plexspaces_proto`: Protocol buffer definitions
- `plexspaces_keyvalue`: Key-value storage backend

## Dependents

This crate is used by:
- `plexspaces_node`: Node uses registry for discovery
- `plexspaces_tuplespace`: TupleSpace uses registry for distributed coordination
- All services: For service discovery

## References

- Implementation: `crates/object-registry/src/`
- Tests: `crates/object-registry/tests/`
- Proto definitions: `proto/plexspaces/v1/object_registry.proto`

