# Chat Room Example (Process Groups)

**Purpose**: Demonstrate process groups for distributed broadcast messaging with comprehensive performance metrics.

**Use Case**: Real-time chat application with pub/sub (Slack, Discord-style messaging).

## Quick Start

```bash
cd examples/rust/embedded/chat_room

# Build (uses shared workspace target directory)
cargo build

# Run
cargo run
```

## What It Demonstrates

1. **Process Groups**: Named groups of actors for broadcast messaging
2. **Pub/Sub**: Broadcast messages to all group members via `publish_to_group`
3. **Dynamic Membership**: Join/leave at runtime
4. **Multi-tenancy**: Groups scoped to tenant/namespace via `RequestContext`
5. **Performance Metrics**: `CoordinationComputeTracker` tracks coordination overhead vs message processing

## What It Demonstrates

1. **Process Groups**: Named groups of actors
2. **Pub/Sub**: Broadcast messages to all group members
3. **Dynamic Membership**: Join/leave at runtime
4. **Multi-tenancy**: Groups scoped to tenant/namespace

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ ProcessGroupRegistry                                            │
│   └─ Group: #general (tenant: acme-corp)                       │
│        ├─ alice@chat-server                                     │
│        ├─ bob@chat-server                                       │
│        └─ charlie@chat-server                                   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
            [alice sends message] ──→ broadcast to all
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
     alice                   bob                 charlie
   (sender)              (receives)            (receives)
```

## Key Code Patterns

### Setup ProcessGroupRegistry

```rust
use plexspaces_keyvalue::SqliteKVStore;
use plexspaces_process_groups::ProcessGroupRegistry;
use plexspaces_core::RequestContext;

// Create backend (SQLite :memory: for demo, use Redis/PostgreSQL for production)
let kv_store = Arc::new(SqliteKVStore::new(":memory:").await?);
let registry = ProcessGroupRegistry::new("chat-server", kv_store);

// Create RequestContext for multi-tenancy (no internal())
let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "chat".to_string());
```

### Create a Group (Room)

```rust
let room = registry.create_group(&ctx, "general").await?;
```

### Join a Group

```rust
use plexspaces_core::ActorId;

let user = ActorId::from("alice@chat-server");
registry.join_group(&ctx, "general", &user, vec![]).await?;
```

### Get Members

```rust
let members = registry.get_members(&ctx, "general").await?;
```

### Broadcast Message

```rust
use serde::Serialize;

#[derive(Serialize)]
struct ChatMessage {
    from: String,
    text: String,
}

let msg = ChatMessage::new("alice", "Hello everyone!");
let payload = serde_json::to_vec(&msg)?;
let recipients = registry.publish_to_group(&ctx, "general", None, payload).await?;
```

### Leave a Group

```rust
registry.leave_group(&ctx, "general", &user).await?;
```

### Metrics Tracking

```rust
use plexspaces_node::CoordinationComputeTracker;

let mut metrics_tracker = CoordinationComputeTracker::new("chat-room".to_string());

// Track coordination (group operations, message sending)
metrics_tracker.start_coordinate();
registry.publish_to_group(&ctx, "general", None, payload).await?;
metrics_tracker.end_coordinate();
metrics_tracker.increment_message();

// Track computation (message processing)
metrics_tracker.start_compute();
// ... process message ...
metrics_tracker.end_compute();

// Get final metrics
let metrics = metrics_tracker.finalize();
println!("Granularity ratio: {:.2}", metrics.granularity_ratio);
```

## Expected Output

The example demonstrates a realistic chat scenario with:
- **3 chat rooms** (room-0, room-1, room-2)
- **8 users per room** (24 total users)
- **20 messages per room** (60 total messages)
- **Comprehensive metrics** showing coordination vs computation overhead

```
Step 1: Create 3 chat rooms
  Room created: #room-0
  Created 3 rooms in 2.34ms

Step 2: Users join rooms (8 users per room)
  user-0-0@chat-server joined #room-0
  user-0-1@chat-server joined #room-0
  ...
  24 users joined rooms in 15.67ms

Step 3: Send 20 messages per room
  [user-0-0@chat-server]: Message 1 from user-0-0@chat-server in room-0 -> 8 recipients
  ...
  Total messages sent: 60
  Total recipients: 480 (avg 8.0 per message)

Step 5: Performance Metrics
  Execution Summary:
    Total execution time: 234.56ms (0.23s)
    Rooms: 3
    Users per room: 8
    Messages per room: 20
    Total messages: 60
    Total recipients: 480

  Coordination vs Computation Breakdown:
    Coordination time: 45.23ms (19.3%)
    Computation time: 6.00ms (2.6%)
    Efficiency (compute/total): 2.6%

  Message & Broadcast Metrics:
    Total messages sent: 60
    Average latency per message: 0.75ms
    Message throughput: 255.8 msg/s
    Recipient throughput: 2046.4 recipients/s

  Benchmark Metrics:
    Throughput: 0.01 MB/s
    Messages per second: 255.8
    Recipients per second: 2046.4

  Granularity Analysis:
    Granularity ratio (compute/coordinate): 0.13
    ⚠️  Moderate granularity (coordination overhead is noticeable)
```

## Use Cases

- **Chat rooms / channels** - Slack, Discord-style messaging
- **Live notifications** - Push updates to all subscribers
- **Collaborative editing** - Real-time document sync
- **Config update broadcasts** - Notify all services of changes
- **Game lobbies** - Player coordination

## Performance Metrics

The example uses `CoordinationComputeTracker` to measure:
- **Coordination time**: Group operations (create, join, leave, publish)
- **Computation time**: Message processing (simulated)
- **Granularity ratio**: `compute_time / coordinate_time` (should be >= 10×)
- **Message throughput**: Messages per second
- **Recipient throughput**: Recipients per second (accounts for broadcast)

### Metrics Interpretation

- **Granularity ratio >= 100×**: Excellent (coordination overhead negligible)
- **Granularity ratio >= 10×**: Good (coordination overhead low)
- **Granularity ratio >= 1×**: Moderate (coordination overhead noticeable)
- **Granularity ratio < 1×**: Poor (coordination overhead dominates)

For pub/sub workloads, coordination overhead is expected to be higher than computation since broadcasting involves network I/O and message routing.

## Process Groups vs Sharding

| Feature | Process Groups | Actor Groups (Sharding) |
|---------|---------------|------------------------|
| **Pattern** | Broadcast (1-to-many) | Partition (key-to-one) |
| **Use Case** | Chat, notifications | User counters, sessions |
| **Routing** | All members receive | Key-based routing |
| **Membership** | Dynamic join/leave | Fixed shard count |

## Design Principles

- **Core Functionality**: `ProcessGroupRegistry` lives in main crates (`crates/process-groups`)
- **No Hacks**: Proper trait usage, no cyclic dependencies
- **Observability**: `CoordinationComputeTracker` for metrics
- **Tenant Isolation**: Explicit `RequestContext` with tenant/namespace (no `internal()`)
- **Shared Target Directory**: Uses workspace shared target (`target/` at root)

## See Also

- [Event Analytics Example](../event_analytics/) - Shard Groups (hash-based routing)
- [Architecture Docs](../../../../docs/architecture.md)
- [Process Groups Documentation](../../../../crates/process-groups/README.md)

## Python WASM Version

A Python WASM version is available:
- `examples/python/apps/chat_room/` - Uses Python SDK with `@actor`, `@handler`, `host.process_groups`
