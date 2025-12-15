# Process Groups - Distributed Pub/Sub

**Purpose**: Distributed pub/sub and broadcast messaging for actor coordination (Erlang pg/pg2-inspired). Enables named groups where actors subscribe to topics and receive broadcast messages.

## Overview

Process groups provide lightweight, distributed pub/sub coordination for actors across nodes. Unlike high-throughput messaging systems (Channels), Process Groups focus on **coordination and membership tracking** with minimal dependencies.

### Key Features

- ✅ **Topic-based pub/sub**: Actors subscribe to specific topics within groups
- ✅ **Erlang pg2-compatible**: Multiple joins, get_members, get_local_members
- ✅ **Multi-tenancy**: Groups scoped by tenant_id + namespace
- ✅ **Lightweight**: No external pub/sub infrastructure required (uses KeyValueStore)
- ✅ **Observability**: Metrics, tracing, structured logging
- ✅ **Scalable**: Optimized for large clusters (100+ members per group)

## Architecture

### Design Decisions

**Why KeyValueStore-based (not Channels)?**
- **Lightweight**: Works with in-memory KV (no Redis/Kafka/NATS required)
- **Different use case**: Process Groups = coordination; Channels = high-throughput messaging
- **Flexibility**: Can integrate with Channels internally if needed, but not required
- **Erlang pg2 model**: Membership tracking is core; messaging is secondary

**Why Topic-based Pub/Sub?**
- Enables fine-grained subscriptions within groups
- Actors can subscribe to specific topics (e.g., "user.login", "config.database")
- Empty topic list = receive all messages (broadcast)
- Topic merging on multiple joins

### Integration Points

Process groups integrate with PlexSpaces core components:
- **ActorRegistry**: Validates actor IDs, uses same KeyValueStore backend
- **Multi-Tenancy**: Groups scoped by tenant_id + namespace
- **Distributed Storage**: Group membership in KeyValueStore (Redis, PostgreSQL, etc.)
- **Messaging**: PublishToGroup broadcasts using Message from actor_runtime.proto

## Key Components

### ProcessGroupRegistry

Main service for group operations:

```rust
pub struct ProcessGroupRegistry {
    storage: Arc<dyn KeyValueStore>,
    node_id: String,
}
```

### Storage Schema

- **Group metadata**: `tenant:{tenant_id}:group:{group_name}`
- **Membership**: `tenant:{tenant_id}:group:{group_name}:member:{actor_id}`
- **Messages**: `tenant:{tenant_id}:group:{group_name}:message:{message_id}`

## Usage Examples

### Basic Broadcast (All Members)

```rust
use plexspaces_process_groups::*;
use plexspaces_keyvalue::InMemoryKVStore;

let kv_store = Arc::new(InMemoryKVStore::new());
let registry = ProcessGroupRegistry::new("node-1", kv_store);

// Create group
registry.create_group("config-updates", "tenant-1", "default").await?;

// Actors join group (empty topics = receive all messages)
registry.join_group("config-updates", "tenant-1", &"actor-1".to_string(), vec![]).await?;
registry.join_group("config-updates", "tenant-1", &"actor-2".to_string(), vec![]).await?;

// Publish to all members
let message = b"New config deployed".to_vec();
let recipients = registry.publish_to_group("config-updates", "tenant-1", None, message).await?;
// recipients = ["actor-1", "actor-2"]
```

### Topic-based Pub/Sub

```rust
// Create event group
registry.create_group("events", "tenant-1", "default").await?;

// Actor 1 subscribes to "user.login" topic
registry.join_group("events", "tenant-1", &"actor-1".to_string(), vec!["user.login".to_string()]).await?;

// Actor 2 subscribes to "user.logout" topic
registry.join_group("events", "tenant-1", &"actor-2".to_string(), vec!["user.logout".to_string()]).await?;

// Actor 3 subscribes to all topics (empty list)
registry.join_group("events", "tenant-1", &"actor-3".to_string(), vec![]).await?;

// Publish to "user.login" topic - only actor-1 and actor-3 receive
let message = b"User logged in".to_vec();
let recipients = registry.publish_to_group("events", "tenant-1", Some("user.login"), message).await?;
// recipients = ["actor-1", "actor-3"]

// Publish to "user.logout" topic - only actor-2 and actor-3 receive
let message = b"User logged out".to_vec();
let recipients = registry.publish_to_group("events", "tenant-1", Some("user.logout"), message).await?;
// recipients = ["actor-2", "actor-3"]
```

### Multiple Joins (Erlang pg2 Semantics)

```rust
// Actor can join same group multiple times
registry.join_group("events", "tenant-1", &"actor-1".to_string(), vec!["user.login".to_string()]).await?;
registry.join_group("events", "tenant-1", &"actor-1".to_string(), vec!["user.logout".to_string()]).await?;

// Topics are merged: actor-1 now subscribed to both "user.login" and "user.logout"
// Must leave equal number of times to fully remove
registry.leave_group("events", "tenant-1", &"actor-1".to_string()).await?;
registry.leave_group("events", "tenant-1", &"actor-1".to_string()).await?;
// Now actor-1 is fully removed
```

### Local vs Cluster-wide Members

```rust
// Get all members across cluster
let all_members = registry.get_members("events", "tenant-1").await?;

// Get only local members (this node)
let local_members = registry.get_local_members("events", "tenant-1").await?;
```

## API Reference

### Core Operations

#### `create_group(group_name, tenant_id, namespace) -> ProcessGroup`
Creates an empty process group. Returns error if group already exists.

#### `delete_group(group_name, tenant_id) -> ()`
Deletes a group and all its members. Idempotent (no error if group doesn't exist).

#### `join_group(group_name, tenant_id, actor_id, topics) -> ()`
Adds actor to group with optional topic subscriptions.
- `topics`: Vec of topic strings (empty = receive all messages)
- Increments join_count (Erlang pg2 semantics)
- Topics are merged if actor already in group

#### `leave_group(group_name, tenant_id, actor_id) -> ()`
Removes actor from group. Decrements join_count. Actor fully removed when join_count reaches 0.

#### `get_members(group_name, tenant_id) -> Vec<ActorId>`
Returns all group members across cluster.

#### `get_local_members(group_name, tenant_id) -> Vec<ActorId>`
Returns only members on current node (faster, no network).

#### `list_groups(tenant_id) -> Vec<String>`
Lists all groups for tenant.

#### `publish_to_group(group_name, tenant_id, topic, message) -> Vec<ActorId>`
Publishes message to group members.
- `topic`: Optional topic filter (None = all members, Some(topic) = only subscribers)
- Returns list of actor IDs that received the message

## Observability

### Metrics

All operations emit Prometheus-compatible metrics:

- `plexspaces_process_groups_created_total` - Groups created (counter)
- `plexspaces_process_groups_deleted_total` - Groups deleted (counter)
- `plexspaces_process_groups_joins_total` - Actors joined (counter, labels: group, tenant)
- `plexspaces_process_groups_leaves_total` - Actors left (counter, labels: group, tenant)
- `plexspaces_process_groups_publish_total` - Messages published (counter, labels: group, tenant)
- `plexspaces_process_groups_publish_duration_seconds` - Publish latency (histogram)
- `plexspaces_process_groups_fanout_size` - Number of recipients per publish (histogram)
- `plexspaces_process_groups_*_errors_total` - Error counts (counter, label: error)

### Tracing

All operations create tracing spans with:
- Operation name (e.g., `process_group.create`, `process_group.join`)
- Group name, tenant ID, actor ID (where applicable)
- Duration tracking

### Logging

Structured logging at appropriate levels:
- `DEBUG`: Normal operations (join, leave, publish)
- `WARN`: Errors (group not found, actor not in group)
- `TRACE`: Detailed operation flow

## Performance Characteristics

### Scalability

- **Membership queries**: O(n) where n = total members (optimized with prefix scans)
- **Publish**: O(n) where n = members matching topic filter
- **Join/Leave**: O(1) per operation (optimized: only updates member_count if new/removed)
- **Delete**: O(n) with batch operations (parallel deletes for members/messages)

### Optimizations

1. **Batch operations**: Delete operations use parallel tasks
2. **Efficient membership tracking**: Only updates member_count when membership changes
3. **Topic filtering**: Early filtering during publish (avoids unnecessary message storage)
4. **Local member queries**: `get_local_members` avoids network round-trips

### Large Cluster Support

Tested with 100+ members per group. For larger clusters:
- Use topic-based filtering to reduce fanout
- Consider using distributed KeyValueStore (Redis, PostgreSQL) for better performance
- Monitor `plexspaces_process_groups_fanout_size` metric

## Design Principles

### Proto-First

All data models defined in `process_groups.proto` first, Rust implements the contract.

### Erlang pg2 Compatibility

Matches pg2 semantics:
- Multiple joins (must leave equally)
- `get_members` (cluster-wide)
- `get_local_members` (local node only)
- Group lifecycle (explicit create/delete)

### Test-Driven Development

All features developed with tests first (RED → GREEN → REFACTOR). 21+ comprehensive tests covering:
- Basic operations (create, join, leave, publish)
- Topic-based pub/sub
- Multiple joins
- Large clusters (100+ members)
- Error paths
- Edge cases

## Dependencies

This crate depends on:
- `plexspaces_proto`: Protocol buffer definitions
- `plexspaces_keyvalue`: Key-value storage backend
- `plexspaces_core`: Common types
- `metrics`: Observability metrics
- `tracing`: Structured logging and tracing

## Dependents

This crate is used by:
- `plexspaces_node`: Node provides process groups to actors
- `plexspaces_core`: ActorContext includes ProcessGroupService

## Comparison: Process Groups vs Channels

| Feature | Process Groups | Channels |
|---------|---------------|----------|
| **Purpose** | Coordination, membership | High-throughput messaging |
| **Dependencies** | KeyValueStore only | Redis/Kafka/NATS (optional) |
| **Use Case** | Actor coordination, pub/sub | Message queues, streams |
| **Topics** | ✅ Built-in | ✅ Built-in |
| **Membership** | ✅ Core feature | ❌ Not applicable |
| **Lightweight** | ✅ Yes | ⚠️ Depends on backend |

**When to use Process Groups:**
- Need actor coordination and membership tracking
- Want lightweight solution (no external infrastructure)
- Erlang pg2-style patterns

**When to use Channels:**
- Need high-throughput message queues
- Want persistent, at-least-once delivery
- Need consumer groups for load balancing

## References

- Implementation: `crates/process-groups/src/`
- Tests: `crates/process-groups/src/lib.rs` (21+ tests)
- Proto definitions: `proto/plexspaces/v1/process_groups/process_groups.proto`
- Design document: `docs/PROCESS_GROUPS_DESIGN.md`
- Inspiration: Erlang pg/pg2
