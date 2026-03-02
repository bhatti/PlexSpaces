# Orbit vs PlexSpaces Comparison - Discord-style Read State Tracker (TypeScript)

Demonstrates **Orbit-style virtual actors** with durability for per-user message read tracking.

**Real-world use case**: Discord read receipts, Slack unread indicators, chat applications — anywhere you need per-user read state tracking with automatic lifecycle management. Inspired by [Discord's read state architecture](https://discord.com/blog/how-discord-stores-billions-of-messages) and [Orbit's virtual actor model](https://www.orbit.cloud/orbit/).

## Architecture

```
              ┌──────────────────────────────────┐
              │   ReadStateTracker (Virtual Actor) │
              │   Per User (auto-activated)        │
              │                                    │
              │  ┌────────────────────────────┐  │
              │  │  Channels                  │  │
              │  │  channel-1: msg-123        │  │
              │  │  channel-2: msg-456        │  │
              │  │  channel-3: msg-789        │  │
              │  └────────────────────────────┘  │
              └──────────────────────────────────┘
                    │           │           │
              mark_read   get_read_state  get_all_read_states
                    │           │           │
              ┌─────▼───────────▼──────────▼──────┐
              │   DurabilityFacet (persistence)   │
              │   State survives deactivation      │
              └────────────────────────────────────┘
```

**Read state flow** (like Discord's read receipts):
1. User reads a message → `mark_read(channel_id, message_id)` → actor activates (if virtual)
2. Read state persisted via DurabilityFacet (survives crashes and deactivation)
3. Virtual actor deactivates after idle timeout (saves memory)
4. Next read → actor reactivates with persisted state

## Quick Start

```bash
# Terminal 1: Start PlexSpaces node
./scripts/server.sh  # (from repo root)

# Terminal 2: Build and test
cd examples/typescript/apps/migrating_orbit
./build.sh
./test.sh 8092
```

## SDK Features Used

- **VirtualActorFacet**: Automatic activation/deactivation (lazy activation on first message)
- **DurabilityFacet**: State persistence (read states survive crashes and deactivation)
- **PlexSpacesActor<T>**: Actor base with typed state + JSON serialization
- **Host**: Host function wrappers (`nowMs`, `kvPut`, `kvGet`, etc.)
- **onInit()**: Actor initialization from framework config
- **on<Op>()**: Message handlers dispatched by `payload.op`
- **getState()/setState()**: Checkpoint-based state persistence

## Comparison to Orbit

| Orbit (Java)                    | PlexSpaces TypeScript          |
|--------------------------------|--------------------------------|
| `Actor.getReference(UserActor.class, "user-123")` | Virtual actor ID: `read-state-tracker:user-123` |
| `await user.markRead(channelId, messageId)` | `host.ask(actorId, {"op":"mark_read", ...})` |
| Automatic activation/deactivation | `VirtualActorFacet` (configured in app-config.toml) |
| State persistence (built-in)   | `DurabilityFacet` (optional, configured) |
| Virtual actor lifecycle        | Same pattern (lazy activation) |

## Use Case: Discord-style Read Receipts

**Problem**: Track which messages each user has read in each channel, with millions of users and thousands of channels.

**Solution**: Virtual actors (one per user) with durability:
- **Memory efficient**: Only active users consume memory (virtual actors deactivate after idle)
- **Durable**: Read states persist across crashes and deactivation
- **Scalable**: Each user is an independent actor (no shared state)
- **Fast**: In-memory updates for active users, persistence is transparent

## Operations

### Mark Read
```typescript
// Mark a message as read in a channel
POST /api/v1/actors/orbit-read-state-ts/read-state-tracker:user-123
{
  "op": "mark_read",
  "channel_id": "channel-1",
  "message_id": "msg-123",
  "timestamp": 1234567890
}
```

### Get Read State
```typescript
// Get read state for a specific channel
POST /api/v1/actors/orbit-read-state-ts/read-state-tracker:user-123
{
  "op": "get_read_state",
  "channel_id": "channel-1"
}
```

### Get All Read States
```typescript
// Get all read states for a user
POST /api/v1/actors/orbit-read-state-ts/read-state-tracker:user-123
{
  "op": "get_all_read_states"
}
```

### Batch Mark Read
```typescript
// Batch update multiple channels (for performance testing)
POST /api/v1/actors/orbit-read-state-ts/read-state-tracker:user-123
{
  "op": "batch_mark_read",
  "updates": [
    {"channel_id": "channel-1", "message_id": "msg-1", "timestamp": 1234567890},
    {"channel_id": "channel-2", "message_id": "msg-2", "timestamp": 1234567891}
  ]
}
```

## Metrics

The example tracks:
- **Coordination vs Computation**: Wall clock time vs WASM compute time
- **Throughput**: Updates per second (WASM compute)
- **Virtual Actor Lifecycle**: Activation/deactivation with state persistence
- **Memory Efficiency**: Only active users consume memory

## Benchmark Output Example

```
Step 6: Batch Update Benchmark (500 updates in single WASM call)
----------------------------------------------------------------

  Total updates:     500
  Channels updated:  450
  Channels created:  50
  Total channels:    5
  Updates/sec (WASM): 12,500

  Coordination Cost Analysis
  ────────────────────────────────────────────
  Wall clock:        45.2ms
  Compute (WASM):    40.0ms (88.5%)
  Coordination:     5.2ms (11.5%)
  Granularity:      7.7x (compute/coordinate)
```

## Design Decisions

**Why Virtual Actors?**
- Millions of users, but only thousands active at once
- Virtual actors deactivate after idle timeout (saves memory)
- Automatic reactivation on next message (transparent to user)

**Why Durability?**
- Read states must survive crashes and deactivation
- DurabilityFacet provides transparent persistence
- Checkpoint-based recovery (fast, not full replay)

**Why Per-User Actors?**
- Independent state per user (no shared state)
- Natural sharding (each user is a separate actor)
- Horizontal scaling (users distributed across nodes)

## PlexSpaces Abstractions Showcased

- ✅ **VirtualActorFacet** - Automatic activation/deactivation, lifecycle management
- ✅ **DurabilityFacet** - State persistence (journaling + checkpointing)
- ✅ **Virtual Actor Pattern** - Automatic activation on first message
- ✅ **GenServerBehavior** - Request-reply pattern (via SDK)
- ✅ **WASM Actors** - Polyglot actor support (TypeScript → WASM)

## References

- [Orbit Documentation](https://www.orbit.cloud/orbit/)
- [Discord's Read State Architecture](https://discord.com/blog/how-discord-stores-billions-of-messages)
- [PlexSpaces VirtualActorFacet](../../../../crates/journaling/src/virtual_actor_facet.rs)
- [PlexSpaces DurabilityFacet](../../../../crates/journaling/src/durability_facet.rs)
- [Getting Started Guide](../../../../docs/getting-started.md)
