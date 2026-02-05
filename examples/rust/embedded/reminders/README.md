# Subscription Billing Example (ReminderFacet) - SDK Annotations Demo

**Purpose**: Demonstrate DURABLE reminders using PlexSpaces `ReminderFacet` with **SDK annotations**.

**PlexSpaces APIs**: `ReminderFacet`, `ReminderRegistration`, `JournalStorage`, **SDK annotations**

## Quick Start

```bash
cd examples/rust/embedded/reminders
cargo build
cargo run
```

## What It Demonstrates

1. **SDK Annotations**: `#[actor]`, `#[plexspaces_handlers]`, `#[handler]` for clean actor definitions
2. **ReminderFacet attachment**: Attach durable reminder capability to actors via `spawn_actor`
3. **ReminderRegistration**: Configure reminder with interval, first fire time, etc.
4. **Persistence**: Reminders survive crashes (backed by JournalStorage)
5. **Actor callback**: Reminder fires message to handler method

## PlexSpaces SDK Usage

### Define Actor with SDK Annotations

```rust
use plexspaces_sdk::{
    actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, spawn_actor,
};

// Step 1: Annotate struct with #[actor(facets = ["durability"])]
// Generates: FACETS const for documentation
#[actor(facets = ["durability"])]
struct SubscriptionActor {
    user_id: String,
    plan: String,
    is_active: bool,
}

// Step 2: Annotate impl with #[plexspaces_handlers(custom)]
// Generates: impl Actor with handle_message dispatch
#[plexspaces_handlers(custom)]
impl SubscriptionActor {
    #[handler("reminder_fired", cast)]  // fire-and-forget
    async fn handle_reminder_fired(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let reminder_name = String::from_utf8_lossy(&msg.payload);
        match reminder_name.as_ref() {
            "monthly_billing" => println!("Processing monthly payment..."),
            "trial_expired" => self.is_active = false,
            _ => {}
        }
        Ok(())
    }
}
```

### Spawn Actor with ReminderFacet

```rust
use plexspaces_journaling::{MemoryJournalStorage, ReminderFacet};

// ReminderFacet requires JournalStorage for persistence
let storage = Arc::new(MemoryJournalStorage::new());
let reminder_facet = ReminderFacet::new(storage, json!({}), 50);

// Spawn with SDK helper (like Python @actor(facets=["durability"]))
let actor_ref = spawn_actor(
    &ctx,
    service_locator,
    "subscription-user-456@node",
    "billing",
    SubscriptionActor::new("user-456", "trial"),
    vec![Box::new(reminder_facet)],
).await?;
```

### Register a Reminder

```rust
use plexspaces_journaling::ReminderRegistration;
use plexspaces_proto::prost_types;

let registration = ReminderRegistration {
    actor_id: "subscription-user-456@node".to_string(),
    reminder_name: "monthly_billing".to_string(),
    interval: Some(prost_types::Duration {
        seconds: 30 * 24 * 60 * 60, // 30 days
        nanos: 0,
    }),
    first_fire_time: Some(prost_types::Timestamp::from(now + Duration::from_days(30))),
    callback_data: vec![],
    persist_across_activations: true, // DURABLE!
    max_occurrences: 0, // 0 = unlimited
};

facet.register_reminder(registration).await?;
```

### Handle Reminder Events (SDK style)

```rust
// With SDK annotations, handlers are clean methods:
#[plexspaces_handlers(custom)]
impl SubscriptionActor {
    #[handler("reminder_fired", cast)]
    async fn handle_reminder_fired(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let reminder_name = String::from_utf8_lossy(&msg.payload);
        match reminder_name.as_ref() {
            "monthly_billing" => println!("Processing monthly payment..."),
            "trial_expired" => self.is_active = false,
            _ => {}
        }
        Ok(())
    }
}
```

## Timer vs Reminder

| Feature | TimerFacet | ReminderFacet |
|---------|-----------|---------------|
| Persistence | In-memory | Durable (DB) |
| Survives crash | No | Yes |
| Use case | Heartbeat, retry | Billing, notifications |
| Backend | None | JournalStorage |

## Use Cases

- **Monthly/annual billing**: Charge on renewal date
- **Trial expiration**: Notify and downgrade
- **Renewal reminders**: Send notice before expiry
- **Scheduled reports**: Daily/weekly report generation
- **SLA reminders**: Escalation timers

## SDK Annotations Reference

| Annotation | Description |
|------------|-------------|
| `#[actor(facets = ["durability"])]` | Marks struct as actor with facets (documentation) |
| `#[plexspaces_handlers(custom)]` | Generates Actor impl with dispatch |
| `#[handler("op", cast)]` | Fire-and-forget handler (no reply) |
| `spawn_actor(..., facets)` | Spawn actor with facets attached |

## See Also

- [Timers](../timers/) - In-memory timers with TimerFacet
- [Durable Actor](../durable_actor/) - Journaling and durability
- [SDK Documentation](../../../../docs/sdk.md) - Full SDK reference
- [Architecture Docs](../../../../docs/architecture.md)
