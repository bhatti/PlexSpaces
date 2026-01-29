# Subscription Billing Example (ReminderFacet)

**Purpose**: Demonstrate DURABLE reminders using PlexSpaces `ReminderFacet`.

**PlexSpaces APIs**: `ReminderFacet`, `ReminderRegistration`, `JournalStorage`

## Quick Start

```bash
cd examples/rust/embedded/reminders
cargo build
cargo run
```

## What It Demonstrates

1. **ReminderFacet attachment**: Attach durable reminder capability to actors
2. **ReminderRegistration**: Configure reminder with interval, first fire time, etc.
3. **Persistence**: Reminders survive crashes (backed by JournalStorage)
4. **Actor callback**: Reminder fires message to `handle_message()`

## PlexSpaces API Usage

### Create Actor with ReminderFacet

```rust
use plexspaces_journaling::{MemoryJournalStorage, ReminderFacet};

// ReminderFacet requires JournalStorage for persistence
let storage = Arc::new(MemoryJournalStorage::new());
let reminder_facet = ReminderFacet::new(storage, json!({}), 50);

// Build actor with facet
let actor = ActorBuilder::new(Box::new(SubscriptionActor::new("user-456")))
    .with_id("subscription-user-456@node")
    .with_namespace("billing")
    .with_facet(Box::new(reminder_facet))
    .spawn(&ctx, service_locator)
    .await?;
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

### Handle Reminder Events

```rust
async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
    // Reminder events have message_type = "reminder_fired"
    if msg.message_type == "reminder_fired" {
        let reminder_name = String::from_utf8_lossy(&msg.payload);
        match reminder_name.as_ref() {
            "monthly_billing" => {
                println!("Processing monthly payment...");
            }
            "trial_expired" => {
                self.is_active = false;
            }
            _ => {}
        }
    }
    Ok(())
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

## See Also

- [Timers](../timers/) - In-memory timers with TimerFacet
- [Durable Actor](../durable_actor/) - Journaling and durability
- [Architecture Docs](../../../../docs/architecture.md)
