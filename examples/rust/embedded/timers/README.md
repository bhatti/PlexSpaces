# Session Manager Example (TimerFacet)

**Purpose**: Demonstrate in-memory timers using PlexSpaces `TimerFacet`.

**PlexSpaces APIs**: `TimerFacet`, `register_once()`, `register_periodic()`, `cancel()`

## Quick Start

```bash
cd examples/rust/embedded/timers
cargo build
cargo run
```

## What It Demonstrates

1. **TimerFacet attachment**: Attach timer capability to actors
2. **One-shot timers**: `register_once(name, delay)` - fire once after delay
3. **Periodic timers**: `register_periodic(name, interval)` - fire repeatedly
4. **Timer cancellation**: `cancel(name)` - stop a timer
5. **Timer listing**: `list_active_timers()` - see all active timers

## PlexSpaces API Usage

### Create Actor with TimerFacet

```rust
use plexspaces_journaling::TimerFacet;
use plexspaces_facet::Facet;

// Create TimerFacet
let timer_facet = TimerFacet::new(json!({}), 50);

// Build actor
let mut actor = ActorBuilder::new(Box::new(SessionActor::new("user-123")))
    .with_id("session-user-123@node")
    .with_namespace("sessions")
    .build(&ctx, service_locator)
    .await?;

// Attach facet
actor.attach_facet(Box::new(timer_facet)).await?;
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

### Handle Timer Events

```rust
#[async_trait]
impl ActorTrait for SessionActor {
    async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        // Timer events have message_type = "timer_fired"
        if msg.message_type == "timer_fired" {
            let timer_name = String::from_utf8_lossy(&msg.payload);
            match timer_name.as_ref() {
                "idle_timeout" => {
                    self.is_active = false;
                    println!("Session expired!");
                }
                "heartbeat" => {
                    println!("Heartbeat");
                }
                _ => {}
            }
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

## See Also

- [Reminders](../reminders/) - Durable reminders with ReminderFacet
- [Durable Actor](../durable_actor/) - Journaling and durability
- [Architecture Docs](../../../../docs/architecture.md)
