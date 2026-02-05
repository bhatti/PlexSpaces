# Session Manager Example (TimerFacet) - SDK Annotations Demo

**Purpose**: Demonstrate in-memory timers using PlexSpaces `TimerFacet` with **SDK annotations**.

**PlexSpaces APIs**: `TimerFacet`, `register_once()`, `register_periodic()`, `cancel()`, **SDK annotations**

## Quick Start

```bash
cd examples/rust/embedded/timers
cargo build
cargo run
```

## What It Demonstrates

1. **SDK Annotations**: `#[actor]`, `#[plexspaces_handlers]`, `#[handler]` for clean actor definitions
2. **TimerFacet attachment**: Attach timer capability to actors via `spawn_actor`
3. **One-shot timers**: `register_once(name, delay)` - fire once after delay
4. **Periodic timers**: `register_periodic(name, interval)` - fire repeatedly
5. **Timer cancellation**: `cancel(name)` - stop a timer
6. **Timer listing**: `list_active_timers()` - see all active timers

## PlexSpaces SDK Usage

### Define Actor with SDK Annotations

```rust
use plexspaces_sdk::{
    actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, spawn_actor, TimerFacet,
};

// Step 1: Annotate struct with #[actor(facets = ["timer"])]
// Generates: FACETS const for documentation
#[actor(facets = ["timer"])]
struct SessionActor {
    user_id: String,
    is_active: bool,
    activity_count: u32,
}

// Step 2: Annotate impl with #[plexspaces_handlers(custom)]
// Generates: impl Actor with handle_message dispatch
#[plexspaces_handlers(custom)]
impl SessionActor {
    #[handler("timer_fired", cast)]  // fire-and-forget
    async fn handle_timer_fired(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let timer_name = String::from_utf8_lossy(&msg.payload);
        match timer_name.as_ref() {
            "idle_timeout" => self.is_active = false,
            "heartbeat" => println!("Heartbeat"),
            _ => {}
        }
        Ok(())
    }
}
```

### Spawn Actor with TimerFacet

```rust
// Create TimerFacet
let timer_facet = TimerFacet::new(json!({}), 50);

// Spawn with SDK helper (like Python @actor(facets=["timer"]))
let actor_ref = spawn_actor(
    &ctx,
    service_locator,
    "session-user-123@node",
    "sessions",
    SessionActor::new("user-123"),
    vec![Box::new(timer_facet)],
).await?;
```

### Register Timers

```rust
// Get facet from actor
if let Some(facet) = actor.get_facet::<TimerFacet>().await {
    // One-shot timer (fires once after 30 seconds)
    facet.register_once("idle_timeout", Duration::from_secs(30)).await?;
    
    // Periodic timer (fires every 5 seconds)
    facet.register_periodic("heartbeat", Duration::from_secs(5)).await?;
}
```

### Handle Timer Events (SDK style)

```rust
// With SDK annotations, handlers are clean methods:
#[plexspaces_handlers(custom)]
impl SessionActor {
    #[handler("timer_fired", cast)]
    async fn handle_timer_fired(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let timer_name = String::from_utf8_lossy(&msg.payload);
        match timer_name.as_ref() {
            "idle_timeout" => {
                self.is_active = false;
                println!("Session expired!");
            }
            "heartbeat" => println!("Heartbeat"),
            _ => {}
        }
        Ok(())
    }
}
```

### Cancel Timer

```rust
// Cancel on user activity (reset timeout)
facet.cancel("idle_timeout").await?;
facet.register_once("idle_timeout", Duration::from_secs(30)).await?;
```

## Key Concepts

| Concept | Description |
|---------|-------------|
| **TimerFacet** | Facet that adds timer capability to actors |
| **In-memory** | Timers are NOT persisted (lost on crash) |
| **Fire message** | Timer fires message to `handle_message()` |
| **register_once** | One-shot timer (idle timeout, retry) |
| **register_periodic** | Repeating timer (heartbeat, polling) |

## Use Cases

- **Session idle timeout**: Disconnect after inactivity
- **Heartbeat**: Keep-alive pings
- **Retry with backoff**: Exponential backoff on failure
- **Debounce**: Rate-limit rapid events
- **Scheduled tasks**: Periodic cleanup

## Timers vs Reminders

| Feature | TimerFacet | ReminderFacet |
|---------|-----------|---------------|
| Persistence | In-memory | Durable (DB) |
| Survives crash | ❌ No | ✅ Yes |
| Use case | Short-lived, high-frequency | Long-lived, critical |
| Example | Heartbeat, retry | Billing, notifications |

## SDK Annotations Reference

| Annotation | Description |
|------------|-------------|
| `#[actor(facets = ["timer"])]` | Marks struct as actor with facets (documentation) |
| `#[plexspaces_handlers(custom)]` | Generates Actor impl with dispatch |
| `#[handler("op", cast)]` | Fire-and-forget handler (no reply) |
| `spawn_actor(..., facets)` | Spawn actor with facets attached |

## See Also

- [Reminders](../reminders/) - Durable reminders with ReminderFacet
- [Durable Actor](../durable_actor/) - Journaling and durability
- [SDK Documentation](../../../../docs/sdk.md) - Full SDK reference
- [Architecture Docs](../../../../docs/architecture.md)
