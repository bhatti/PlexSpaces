# Timers Example

## Purpose

This example demonstrates **non-durable, in-memory timers** using the `TimerFacet` in PlexSpaces. Timers are ideal for transient, high-frequency operations that don't need to survive actor deactivation.

## How Timers Work

### Architecture

Timers are implemented as an **opt-in facet** (`TimerFacet`) that can be attached to any actor. This follows the **cohesive design principle** where capabilities are added via facets rather than being built into every actor.

```
┌─────────────────────────────────────────────────────────┐
│ Actor                                                    │
│   └─ TimerFacet (opt-in)                                │
│       ├─ register_timer()                                │
│       ├─ unregister_timer()                              │
│       └─ Background tasks (tokio::spawn)                │
└─────────────────────────────────────────────────────────┘
                    │
                    v
            TimerFired Message
            (sent to actor's mailbox)
```

### Key Characteristics

1. **Non-Durable**: Timers are stored in-memory only (not persisted)
2. **Lost on Deactivation**: If the actor deactivates, all timers are lost
3. **High-Frequency**: Suitable for operations with intervals < 1 second
4. **Lightweight**: No I/O overhead (pure in-memory operations)

### Timer Lifecycle

1. **Registration**: Actor calls `timer_facet.register_timer(registration)`
2. **Background Task**: TimerFacet spawns a tokio task that waits for the timer interval
3. **Firing**: When timer fires, a `TimerFired` message is sent to the actor's mailbox
4. **Processing**: Actor's behavior handles the `TimerFired` message
5. **Cleanup**: On actor deactivation, all timers are automatically cancelled

### Timer Types

- **One-time Timer**: Fires once after a delay (`due_time` specified, `periodic: false`)
- **Periodic Timer**: Fires repeatedly at intervals (`interval` specified, `periodic: true`)

## Example Scenarios

### Scenario 1: Heartbeat Timer (Periodic)

**Use Case**: Send heartbeat every 2 seconds to keep connection alive

```rust
// Using simplified API via FacetService
let timer_facet = ctx.facet_service.get_facet(&ctx.actor_id, "timer").await?;
let mut timer_facet_guard = timer_facet.write().await;
if let Some(timer_facet) = timer_facet_guard.as_any_mut().downcast_mut::<TimerFacet>() {
    timer_facet.register_periodic(
        "heartbeat",
        Duration::from_secs(2),
        None, // No distributed lock for single-node
    ).await?;
}
```

**Validation**: 
- Timer fires every 2 seconds
- Actor receives `TimerFired` messages with `timer_name = "heartbeat"`
- Timer continues until actor deactivates or timer is unregistered

### Scenario 2: One-time Cleanup Timer

**Use Case**: Perform cleanup after 5 seconds

```rust
// Using simplified API via FacetService
let timer_facet = ctx.facet_service.get_facet(&ctx.actor_id, "timer").await?;
let mut timer_facet_guard = timer_facet.write().await;
if let Some(timer_facet) = timer_facet_guard.as_any_mut().downcast_mut::<TimerFacet>() {
    timer_facet.register_one_shot(
        "cleanup",
        Duration::from_secs(5),
        None, // No distributed lock for single-node
    ).await?;
}
```

**Validation**:
- Timer fires once after 5 seconds
- Actor receives one `TimerFired` message
- Timer is automatically removed after firing

### Scenario 3: High-Frequency Polling Timer

**Use Case**: Poll external resource every 500ms

```rust
// Using simplified API via FacetService
let timer_facet = ctx.facet_service.get_facet(&ctx.actor_id, "timer").await?;
let mut timer_facet_guard = timer_facet.write().await;
if let Some(timer_facet) = timer_facet_guard.as_any_mut().downcast_mut::<TimerFacet>() {
    timer_facet.register_periodic(
        "poll",
        Duration::from_millis(500),
        None, // No distributed lock for single-node
    ).await?;
}
```

**Validation**:
- Timer fires every 500ms
- Actor receives frequent `TimerFired` messages
- Demonstrates high-frequency capability

## How the Example Validates Features

### 1. Facet Attachment

**What it validates**: Timers are opt-in via facet attachment

```rust
let timer_facet = Box::new(TimerFacet::new());
actor.attach_facet(timer_facet, 50, serde_json::json!({})).await?;
```

**Expected**: Facet is successfully attached, actor can now use timers

### 2. Timer Registration via FacetService

**What it validates**: Timers can be registered using FacetService from ActorContext

**Implementation**:
- Actor receives a `RegisterTimers` message
- Actor uses `ctx.facet_service.get_facet()` to retrieve TimerFacet
- Actor calls simplified APIs: `register_periodic()` and `register_one_shot()`

**Expected**: 
- Periodic timers continue firing
- One-time timers fire once and stop
- Timer names are unique per actor

### 3. Typed Message Delivery

**What it validates**: Timer fires send typed `ActorMessage::TimerFired` messages to actor's mailbox

**Expected**:
- Messages use `ActorMessage::TimerFired` enum variant
- Messages contain `timer_name` field for pattern matching
- Actor's behavior uses type-safe pattern matching on `ActorMessage` enum

### 4. Timer Cleanup

**What it validates**: Timers are automatically cleaned up on actor deactivation

**Expected**:
- All timer tasks are cancelled
- No memory leaks
- Actor can be reactivated without stale timers

## Running the Example

### Quick Run
```bash
cd examples/simple/timers_example
cargo run
```

### Test Script (Recommended)
```bash
cd examples/simple/timers_example
./test.sh
```

The test script will:
- Build the example in release mode
- Run it for a configured duration
- Validate expected behavior
- Show metrics (timer fire counts, timing)
- Exit with appropriate status codes for CI/CD

## Expected Output

```
[INFO] ╔════════════════════════════════════════════════════════════════╗
[INFO] ║  Timers Example - Non-Durable, In-Memory Timers               ║
[INFO] ╚════════════════════════════════════════════════════════════════╝
[INFO] ✅ Actor spawned: timer-actor@local
[INFO] ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
[INFO] Scenario 1: Heartbeat Timer (Periodic, 2s interval)
[INFO] ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
[INFO] 💓 Heartbeat timer fired count=1
[INFO] 💓 Heartbeat timer fired count=2
[INFO] 💓 Heartbeat timer fired count=3
[INFO] 💓 Heartbeat timer fired count=4
```

## Design Notes

### Why Non-Durable?

- **Performance**: No I/O overhead for high-frequency operations
- **Simplicity**: No need to persist transient operations
- **Use Case Fit**: Heartbeats, polling, and cleanup don't need persistence

### When to Use Timers vs Reminders

| Feature | Timers | Reminders |
|---------|--------|-----------|
| **Durability** | ❌ In-memory only | ✅ Persisted |
| **Survives Deactivation** | ❌ No | ✅ Yes |
| **Survives Crash** | ❌ No | ✅ Yes |
| **Min Interval** | Milliseconds | Seconds |
| **Use Cases** | Heartbeats, polling | Billing, SLA, cron jobs |
| **Performance** | Fast (no I/O) | Slower (disk writes) |

### Integration with VirtualActorFacet

Timers **do not** trigger actor activation. If an actor with timers deactivates:
- All timers are lost
- Actor must be manually reactivated
- Timers must be re-registered

For auto-activation, use **ReminderFacet** instead.

## See Also

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design - Facets](../../docs/detailed-design.md#timer-and-reminder-facets) - TimerFacet documentation
- [Getting Started Guide](../../docs/getting-started.md) - Quick start guide
- `examples/simple/reminders_example` - Durable reminders that survive deactivation
- `crates/journaling/src/timer_facet.rs` - Implementation details
