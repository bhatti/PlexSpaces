# Chat Room Example (Process Groups)

**Purpose**: Demonstrate process groups for broadcast messaging.

**Use Case**: Real-time chat application with pub/sub.

## Quick Start

```bash
cd examples/rust/embedded/chat_room

# Build
cargo build

# Run
cargo run
```

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

### Create a Group (Room)

```rust
use plexspaces_process_groups::ProcessGroupRegistry;

let registry = ProcessGroupRegistry::new("chat-server", kv_store);
let room = registry.create_group("general", "acme-corp", "chat").await?;
```

### Join a Group

```rust
use plexspaces_core::ActorId;

let user = ActorId::from("alice@chat-server");
registry.join_group("general", "acme-corp", "chat", &user, vec![]).await?;
```

### Get Members

```rust
use plexspaces_core::RequestContext;

let ctx = RequestContext::new_without_auth("acme-corp".into(), "chat".into());
let members = registry.get_members(&ctx, "general").await?;
```

### Leave a Group

```rust
registry.leave_group(&ctx, "general", &user).await?;
```

### Broadcast Message (Real Implementation)

```rust
// In actual PlexSpaces, use publish_to_group
registry.publish_to_group(&ctx, "general", message).await?;
```

## Expected Output

```
Step 1: Create chat room
  Room created: #general
  Tenant: acme-corp

Step 2: Users join the room
  alice@chat-server joined #general
  bob@chat-server joined #general
  charlie@chat-server joined #general
  Room members: 3

Step 3: Alice sends a message
  [alice]: Hey everyone! How's it going?
  Delivered to:
    -> bob@chat-server
    -> charlie@chat-server

Step 5: Charlie leaves the room
  charlie left #general
  Remaining: 2 members

Step 6: Alice sends another message
  [alice]: Charlie left, it's just us now!
  Delivered to:
    -> bob@chat-server
  (charlie did NOT receive - left room)
```

## Use Cases

- **Chat rooms / channels** - Slack, Discord-style messaging
- **Live notifications** - Push updates to all subscribers
- **Collaborative editing** - Real-time document sync
- **Config update broadcasts** - Notify all services of changes
- **Game lobbies** - Player coordination

## Process Groups vs Sharding

| Feature | Process Groups | Actor Groups (Sharding) |
|---------|---------------|------------------------|
| **Pattern** | Broadcast (1-to-many) | Partition (key-to-one) |
| **Use Case** | Chat, notifications | User counters, sessions |
| **Routing** | All members receive | Key-based routing |
| **Membership** | Dynamic join/leave | Fixed shard count |

## See Also

- [Actor Groups (Sharding)](../actor_groups_sharding/) - For partitioned data
- [Architecture Docs](../../../../docs/architecture.md)

## WASM Version

This example will also be available as a Python WASM actor:
- `examples/python/apps/chat_room/` (planned)
