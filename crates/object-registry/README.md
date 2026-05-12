# Object Registry - Unified Service Discovery

**Purpose**: Provides unified registration and discovery for all distributed objects in PlexSpaces:
- **Actors**: Stateful computation units (actor model)
- **TupleSpaces**: Coordination primitives (Linda model)
- **Services**: Microservices and gRPC endpoints
- **Nodes**: PlexSpaces node instances
- **Workflows**: Durable workflow definitions
- **Applications**: Deployed applications

## Overview

This crate consolidates three separate registries (ActorRegistry, TupleSpaceRegistry, ServiceRegistry) into ONE unified registry following Proto-First Design principles.

### Architecture

```text
┌─────────────────────────────────────────────────────────┐
│              ObjectRegistryImpl                          │
│  register() / unregister() / lookup() / discover()      │
│  heartbeat() / find_stale() / update_health_status()    │
│  record_heartbeat_failure() / lookup_by_alias()         │
│  register_with_unique_alias()                           │
└────────────────────┬────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────┐
│         ObjectRegistryRepository (trait)                │
│  put() / get() / delete() / discover() / heartbeat()    │
└────────────────────┬────────────────────────────────────┘
                     │
        ┌───────────┼─────────────────────┐
        ▼           ▼                     ▼
   ┌────────┐  ┌────────────┐       ┌────────┐
   │ SQLite │  │ PostgreSQL │       │DynamoDB│
   └────────┘  └────────────┘       └────────┘
       ↑
  Use :memory:
  for testing
```

## Key Features

### Indexed Columns for Fast Queries

Unlike a generic key-value store, this repository uses indexed columns:

| Column | Purpose |
|--------|---------|
| `tenant_id`, `namespace`, `object_id` | Primary key for tenant isolation |
| `object_type` | Fast discover by type (actors, services, nodes) |
| `node_id` | Find all objects on a specific node |
| `health_status` | Filter by health state |
| `last_heartbeat` | Find stale registrations efficiently |
| `object_category` | Sub-type filtering (e.g., "GenServer", "redis") |
| `alias` | Unique identity key for placement (`"{type}:{name}:{ns}:{tenant}"`) |
| `max_heartbeat_failures` | Threshold before transitioning to DEAD (default 3) |
| `heartbeat_failure_count` | Consecutive missed heartbeats (reset on success) |
| `registration_blob` | Full ObjectRegistration protobuf |

### Health Lifecycle

Health transitions are managed automatically:

```
HEALTHY  ──(1st miss)──▶  DEGRADED  ──(max misses)──▶  DEAD
   ▲                                                      │
   └─────────────(successful heartbeat)───────────────────┘

NODE going DEAD ──cascades──▶ all objects on that node → DEAD
```

`max_heartbeat_failures` (proto field 19, default 3) and `heartbeat_failure_count` (proto field 20, OUTPUT_ONLY) are stored as indexed columns for efficient increments.

### Unique Actor Placement (Orleans Grain Directory)

The `alias` field (proto field 18) enables single-active-instance guarantees:

```rust
use plexspaces_object_registry::RegisterResult;

let registration = ObjectRegistration {
    object_id: "counter@node1".to_string(),
    alias: "Counter:my-counter:production:tenant-1".to_string(),
    // ...
    ..Default::default()
};

match registry.register_with_unique_alias(&ctx, registration, true).await? {
    RegisterResult::Registered => { /* spawn succeeded */ }
    RegisterResult::AlreadyExists { grpc_address, object_id } => {
        // Forward to existing instance at grpc_address
    }
}
```

### Performance Characteristics

| Operation | Complexity | Notes |
|-----------|------------|-------|
| `register()` | O(1) | Single repository write |
| `lookup()` | O(1) | Primary key lookup |
| `lookup_by_alias()` | O(1) | Unique index lookup |
| `register_with_unique_alias()` | O(1) | Alias check + insert |
| `discover()` | O(log n + k) | Indexed query + filter |
| `heartbeat()` | O(1) | Column UPDATEs (timestamp + reset failures + health) |
| `record_heartbeat_failure()` | O(1) | Atomic increment + health transition |
| `find_stale()` | O(log n + k) | Uses `last_heartbeat` index |

### Multi-Backend Support

| Backend | Use Case | Feature Flag |
|---------|----------|--------------|
| **SQLite** | Embedded, single-node; use `:memory:` for testing | `sql-backend` |
| **PostgreSQL** | Production, multi-node | `sql-backend` |
| **DynamoDB** | AWS serverless | `ddb-backend` |

> **Note**: In-memory testing uses `SqliteObjectRegistryRepository::new(":memory:")` which provides fast, isolated storage without persistence.

## Usage

### Basic Usage with SQLite In-Memory Backend

```rust
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
use plexspaces_common::RequestContext;
use std::sync::Arc;

// Create repository with in-memory SQLite (for testing)
let repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await?);
let registry = ObjectRegistryImpl::new(repo);

// Create context for tenant isolation
let ctx = RequestContext::new_without_auth("tenant-1".to_string(), "production".to_string());

// Register actor
let registration = ObjectRegistration {
    object_id: "counter//gen_server::production@node1".to_string(),
    object_type: ObjectType::ObjectTypeActor as i32,
    object_category: "GenServer".to_string(),
    grpc_address: "http://node1:8000".to_string(),
    ..Default::default()
};

registry.register(&ctx, registration).await?;
```

### Using Configuration

```rust
use plexspaces_object_registry::{ObjectRegistryImpl, create_repository_from_shared_db};
use plexspaces_proto::storage::v1::SharedDbConfig;

let config = SharedDbConfig {
    connection_string: "sqlite:///tmp/registry.db".to_string(),
    ..Default::default()
};
let repo = create_repository_from_shared_db(&config).await?;
let registry = ObjectRegistryImpl::new(repo);
```
| `PLEXSPACES_OBJECT_REGISTRY_POSTGRES_URL` | PostgreSQL connection string | - |
| `PLEXSPACES_OBJECT_REGISTRY_DDB_TABLE` | DynamoDB table name | `plexspaces-object-registry` |
| `PLEXSPACES_OBJECT_REGISTRY_DDB_REGION` | DynamoDB AWS region | `us-east-1` |
| `PLEXSPACES_OBJECT_REGISTRY_DDB_ENDPOINT` | DynamoDB endpoint (for local) | - |

### Discover Objects

```rust
// Discover all actors
let actors = registry.discover(
    &ctx,
    Some(ObjectType::ObjectTypeActor),  // Filter by type
    None,  // object_category
    None,  // capabilities
    None,  // labels
    None,  // health_status
    0,     // offset
    100    // limit
).await?;

// Find stale registrations (no heartbeat in 60 seconds)
let stale = registry.find_stale(&ctx, 60, None, 100).await?;
```

### Heartbeat Updates

```rust
// Efficient heartbeat - single column UPDATE, no blob read/write
registry
    .heartbeat(
        &ctx,
        ObjectType::ObjectTypeActor,
        "counter//gen_server::production@node1",
    )
    .await?;

// Record a missed heartbeat (increments failure count, transitions health)
let new_status = registry
    .record_heartbeat_failure(&ctx, "counter//gen_server::production@node1")
    .await?;
// new_status == DEGRADED (count < max) or DEAD (count >= max)

// Update health status directly
registry
    .update_health_status(
        &ctx,
        "counter//gen_server::production@node1",
        HealthStatus::HealthStatusDead,
    )
    .await?;
```

## Migrations

Database schema is managed through SQL migrations:

```
crates/object-registry/migrations/
├── postgres/
│   ├── 001_object_registrations.up.sql
│   └── 001_object_registrations.down.sql
└── sqlite/
    ├── 001_object_registrations.up.sql
    └── 001_object_registrations.down.sql
```

Migrations run automatically when creating a SQL repository.

The registry also supports distinct tenant discovery for a given object type. Dashboard tenant
inventory uses application registrations as the source of truth, with repository-backed offset/limit
pagination and total-count queries so tenant listing does not depend on higher-level response merges.

## WIT Interface (WASM Actors)

WASM actors access the Object Registry through the `plexspaces:actor/registry@0.1.0` WIT interface
defined in `wit/plexspaces-actor/registry.wit`. The interface provides:

| WIT Function | Description |
|---|---|
| `register` | Register an object with optional alias |
| `unregister` | Remove a registration |
| `lookup` | Fetch by object ID |
| `lookup-by-alias` | Fetch by alias (grain directory lookup) |
| `discover` | Paginated discovery with filters |
| `heartbeat` | Update liveness timestamp |

The `object-registration` record includes an `alias: option<string>` field for identity-based lookup.

## SDK Access

All SDK languages expose `host.registry` (or `host.Registry()` in Go):

| Language | Access |
|---|---|
| Python | `host.registry.lookup_by_alias(ctx, alias)` |
| TypeScript | `host.registry.lookupByAlias(ctx, alias)` |
| Go | `host.Registry().LookupByAlias(ctx, alias)` |
| Rust | `plexspaces_sdk::object_registry::lookup_actor_by_identity(...)` |

## Dependencies

This crate depends on:
- `plexspaces-proto`: Protocol buffer definitions (object_registry.proto)
- `plexspaces-common`: RequestContext for multi-tenancy
- `plexspaces-core`: Service trait and ObjectRegistry trait

## Dependents

This crate is used by:
- `plexspaces-services`: ServiceLocator initializes ObjectRegistry
- `plexspaces-node`: Node uses registry for discovery
- All gRPC services: For service discovery

## References

- [Architecture](../../docs/architecture.md): Object Registry section
- [Database Models](../../docs/detailed-design.md#database-models-and-er-diagram): ER diagram and schema details
- Proto definitions: `proto/plexspaces/v1/registry/object_registry.proto`
