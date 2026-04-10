# Subscription Billing Example (ReminderFacet) - SDK Annotations Demo

**Purpose**: Demonstrate DURABLE reminders using PlexSpaces `ReminderFacet` with **SDK annotations**, showing the industry-standard distinction between transient timers and durable reminders.

**PlexSpaces APIs**: `ReminderFacet`, `TimerFacet`, `ReminderRegistration`, `JournalStorage`, **SDK annotations**, `CoordinationComputeTracker`

## Quick Start

```bash
cd examples/rust/embedded/reminders
cargo build
cargo run
```

## What It Demonstrates

1. **SDK Annotations**: `#[actor]`, `#[plexspaces_handlers]`, `#[handler]` for clean actor definitions
2. **TimerFacet vs ReminderFacet**: Demonstrates the Orleans model distinction (transient vs durable)
3. **ReminderFacet attachment**: Attach durable reminder capability to actors via `spawn_with_facets`
4. **ReminderRegistration**: Configure reminder with interval, first fire time, persist_across_activations
5. **Persistence**: Reminders survive crashes (backed by JournalStorage)
6. **CoordinationComputeTracker**: Metrics for coordination vs computation overhead analysis
7. **Real-world scenario**: Subscription billing system with 50 subscriptions, trial management, monthly billing, and health monitoring

## Use Case

**Subscription Billing System**: Manages subscriptions with:
- **Transient timers** (TimerFacet): Heartbeat monitoring (in-memory, lost on crash)
- **Durable reminders** (ReminderFacet): Monthly billing, trial warnings, trial expiration (persisted, survives crashes)

This demonstrates the industry-standard Orleans model where:
- **Timer** = transient, fast, no persistence overhead
- **Reminder** = durable, requires storage, survives crashes

## PlexSpaces SDK Usage

### Define Actor with SDK Annotations

```rust
use plexspaces_sdk::{
    actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, spawn_with_facets,
};

// Step 1: Annotate struct with #[actor(facets = ["timer", "reminder"])]
#[actor(facets = ["timer", "reminder"])]
struct SubscriptionActor {
    user_id: String,
    plan: SubscriptionPlan,
    is_active: bool,
}

// Step 2: Annotate impl with #[plexspaces_handlers(custom)]
#[plexspaces_handlers(custom)]
impl SubscriptionActor {
    #[handler("timer_fired", cast)]  // Transient timer events
    async fn handle_timer_fired(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        // Handle heartbeat, session checks
        Ok(())
    }
    
    #[handler("reminder_fired", cast)]  // Durable reminder events
    async fn handle_reminder_fired(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        // Handle billing, trial expiration
        Ok(())
    }
}
```

### Spawn Actor with Both Facets

```rust
use plexspaces_journaling::{SqliteJournalStorage, ReminderFacet, TimerFacet};
use plexspaces_facet::Facet;

// ReminderFacet requires JournalStorage for persistence
let storage = Arc::new(SqliteJournalStorage::new(":memory:").await?);
let timer_facet = Box::new(TimerFacet::new(json!({}), 50)) as Box<dyn Facet>;
let reminder_facet = Box::new(ReminderFacet::new(storage, json!({}), 50)) as Box<dyn Facet>;

// Spawn with SDK helper
let actor_ref = spawn_with_facets(
    &ctx,
    service_locator,
    "subscription-user-456",
    "billing",
    SubscriptionActor::new("user-456", plan),
    vec![timer_facet, reminder_facet],
).await?;
```

### Register Timers and Reminders

```rust
use plexspaces_journaling::ReminderRegistration;
use plexspaces_proto::prost_types;

// Get facets from node
let facets_arc = node.get_facets(actor_id).await?;
let facets_guard = facets_arc.read().await;

// Register TimerFacet (transient heartbeat)
if let Some(timer_facet_arc) = facets_guard.get_facet("timer") {
    let timer_facet_guard = timer_facet_arc.read().await;
    if let Some(timer_facet) = timer_facet_guard.as_any().downcast_ref::<TimerFacet>() {
        timer_facet.register_periodic("heartbeat", Duration::from_secs(5)).await?;
    }
}

// Register ReminderFacet (durable billing)
if let Some(reminder_facet_arc) = facets_guard.get_facet("reminder") {
    let reminder_facet_guard = reminder_facet_arc.read().await;
    if let Some(reminder_facet) = reminder_facet_guard.as_any().downcast_ref::<ReminderFacet>() {
        let registration = ReminderRegistration {
            actor_id: actor_id.clone(),
            reminder_name: "monthly_billing".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 30 * 24 * 60 * 60, // 30 days
                nanos: 0,
            }),
            first_fire_time: None,
            callback_data: vec![],
            persist_across_activations: true, // KEY: Survives crashes!
            max_occurrences: 0, // Unlimited
        };
        reminder_facet.register_reminder(registration).await?;
    }
}
```

## Timer vs Reminder

| Feature | TimerFacet | ReminderFacet |
|---------|-----------|---------------|
| Persistence | In-memory | Durable (DB) |
| Survives crash | No | Yes |
| Storage required | No | Yes (JournalStorage) |
| Performance | Fast | Slower (I/O) |
| Use case | Heartbeat, retry | Billing, notifications |
| Message type | `timer_fired` | `reminder_fired` |

## Expected Output

The example creates 50 subscriptions with:
- 50 transient timers (heartbeat monitoring)
- 67 durable reminders (monthly billing + trial warnings)

**Sample Output**:
```
Execution Summary
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Total subscriptions: 50
  Timers registered: 50 (transient heartbeats)
  Reminders registered: 67 (durable billing)
  Total operations: 117

Coordination vs Computation Breakdown:
  Coordination time: 21.00ms (0.2%)
  Computation time: 12001.00ms (99.8%)
  Total time: 12022.43ms (12.02s)
  Efficiency (compute/total): 99.8%

Benchmark Metrics:
  Operations per second: 9.7
  Subscriptions per second: 4.2

Granularity Analysis:
  Granularity ratio (compute/coordinate): 571.48
  ✅ Good granularity (coordination overhead is low)
```

## Performance Metrics

The example includes comprehensive metrics:

- **Coordination vs Computation**: Breakdown of time spent on coordination (spawning, registration) vs computation (processing)
- **Efficiency**: Percentage of time spent on actual computation
- **Granularity Ratio**: Compute time / coordinate time (should be >= 10× for good granularity)
- **Operations per second**: Throughput metrics for timer/reminder operations
- **Subscriptions per second**: Throughput for subscription management

## Use Cases

- **Monthly/annual billing**: Charge on renewal date (durable reminder)
- **Trial expiration**: Notify and downgrade (durable reminder)
- **Renewal reminders**: Send notice before expiry (durable reminder)
- **Scheduled reports**: Daily/weekly report generation (durable reminder)
- **SLA reminders**: Escalation timers (durable reminder)
- **Health monitoring**: Heartbeat checks (transient timer)
- **Session timeouts**: Idle timeout management (transient timer)

## SDK Annotations Reference

| Annotation | Description |
|------------|-------------|
| `#[actor(facets = ["timer", "reminder"])]` | Marks struct as actor with both facets |
| `#[plexspaces_handlers(custom)]` | Generates Actor impl with dispatch |
| `#[handler("timer_fired", cast)]` | Fire-and-forget handler for transient timer events |
| `#[handler("reminder_fired", cast)]` | Fire-and-forget handler for durable reminder events |
| `spawn_with_facets(..., facets)` | Spawn actor with facets attached |

## Key Code Patterns

### RequestContext Usage

```rust
// NEVER use RequestContext::internal() - use proper tenant/namespace
let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "billing".to_string());
```

### CoordinationComputeTracker Usage

```rust
use plexspaces_node::CoordinationComputeTracker;

let mut metrics_tracker = CoordinationComputeTracker::new("reminders".to_string());
let total_start = Instant::now();

// Coordination phase
metrics_tracker.start_coordinate();
// ... spawn actors, register timers/reminders ...
metrics_tracker.end_coordinate();

// Computation phase
metrics_tracker.start_compute();
// ... simulate processing ...
metrics_tracker.end_compute();

// Get metrics
let total_time = total_start.elapsed();
let metrics = metrics_tracker.finalize();
```

## Design Principles

- **Industry Standard**: Follows Orleans model (Timer = transient, Reminder = durable)
- **No Configuration Flags**: The naming convention IS the API contract
- **Proper Tenant Isolation**: Uses `RequestContext` with explicit tenant/namespace
- **Observability**: Comprehensive metrics via `CoordinationComputeTracker`
- **Production-Grade**: Simple, robust, well-tested patterns

## See Also

- [Timers](../timers/) - In-memory timers with TimerFacet
- [Bank Account](../bank_account/) - Durable actors with journaling
- [SDK Documentation](../../../../docs/sdk.md) - Full SDK reference
- [Architecture Docs](../../../../docs/architecture.md)
