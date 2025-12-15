# Process Groups (Pub/Sub) Example

**Purpose**: Demonstrate Process Groups for pub/sub coordination

**Pattern**: Broadcast messaging - all group members receive messages

## Overview

This example demonstrates the **Process Groups (Pub/Sub)** pattern for coordination. Messages are broadcast to all group members, enabling pub/sub-style communication.

## Features Demonstrated

1. **Process Group Creation**: Create a named group for coordination
2. **Dynamic Membership**: Actors can join and leave groups at runtime
3. **Broadcast Messaging**: Messages sent to a group are received by all members
4. **Group Lifecycle**: Create, manage, and delete groups

## Quick Start

```bash
# Run the example directly
cargo run --release --bin process_groups_pubsub

# Or use the run script
./scripts/run.sh

# Run tests and validation
./scripts/test.sh
```

## Architecture

```
Process Group: "notifications"
    │
    ├─→ actor-1 (member)
    ├─→ actor-2 (member)
    └─→ actor-3 (member)

Broadcast: "Hello from broadcaster!"
    │
    ├─→ actor-1 receives
    ├─→ actor-2 receives
    └─→ actor-3 receives
```

## What It Shows

1. **Creating a Process Group**: Named group with tenant/namespace isolation
2. **Joining a Group**: Actors join to receive broadcasts
3. **Broadcasting Messages**: Send message to all group members
4. **Leaving a Group**: Actors can leave, no longer receive messages
5. **Group Cleanup**: Delete groups when no longer needed

## Use Cases

✅ **Use Process Groups when:**
- Need to broadcast to multiple actors (pub/sub)
- Config updates to all instances
- Event notifications to subscribers
- Coordination between multiple actors
- Service discovery and health checks

❌ **Don't use when:**
- Need 1-to-1 routing → Use Actor Groups (Sharding) instead
- Need request-reply → Use direct actor messaging
- Small number of actors → Direct messaging may be simpler

## Key Takeaways

- **Process Groups**: Pub/sub pattern for broadcast messaging
- **All members receive**: Every member gets every message
- **Dynamic membership**: Join/leave at runtime
- **Use for**: Config updates, event notifications, coordination

## See Also

- `docs/GROUPS_COMPARISON.md` - When to use Process Groups vs Actor Groups
- `examples/simple/actor_groups_sharding` - Sharding pattern example

