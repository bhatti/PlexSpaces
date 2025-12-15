# Reminders Example

## Purpose

This example demonstrates **durable, persistent reminders** using the `ReminderFacet` in PlexSpaces. Reminders survive actor deactivation and crashes, making them ideal for critical operations like billing, SLA enforcement, and scheduled tasks.

## How Reminders Work

### Architecture

Reminders are implemented as an **opt-in facet** (`ReminderFacet`) that integrates with:
- **JournalStorage**: For persistence (SQLite, PostgreSQL, or Memory)
- **VirtualActorFacet**: For auto-activation when reminders fire

```
┌─────────────────────────────────────────────────────────┐
│ Actor                                                    │
│   ├─ ReminderFacet (opt-in)                            │
│   │   ├─ register_reminder()                           │
│   │   ├─ Background task (checks due reminders)        │
│   │   └─ Persists to JournalStorage                     │
│   └─ VirtualActorFacet (optional, for auto-activation) │
└─────────────────────────────────────────────────────────┘
                    │
                    v
            ┌───────────────┐
            │ JournalStorage│
            │ (SQLite/PG)   │
            └───────────────┘
```

### Key Characteristics

1. **Durable**: Reminders are persisted to storage (survive crashes)
2. **Survives Deactivation**: Reminders persist across actor lifecycle
3. **Auto-Activation**: Can trigger actor activation when due (with VirtualActorFacet)
4. **Max Occurrences**: Can auto-delete after N fires
5. **Long-Term Scheduling**: Suitable for intervals of hours, days, or months

### Reminder Lifecycle

1. **Registration**: Actor calls `reminder_facet.register_reminder(registration)`
2. **Persistence**: Reminder is saved to `JournalStorage` (SQLite/PostgreSQL)
3. **Background Task**: ReminderFacet spawns a task that periodically checks for due reminders
4. **Firing**: When reminder is due:
   - If actor is deactivated → Trigger activation (via VirtualActorFacet)
   - Send `ReminderFired` message to actor's mailbox
   - Update `fire_count` and `next_fire_time`
5. **Auto-Deletion**: If `max_occurrences` reached, reminder is automatically deleted
6. **Survival**: Reminders persist even if actor deactivates or node crashes

### Reminder Configuration

- **interval**: Time between fires (for periodic reminders)
- **first_fire_time**: When to fire the first time
- **max_occurrences**: Maximum number of fires before auto-deletion (-1 for infinite)
- **persist_across_activations**: Whether reminder survives deactivation (always true for reminders)

## Example Scenarios

### Scenario 1: Monthly Billing Reminder (Infinite)

**Use Case**: Send monthly billing reminder every 30 days, indefinitely

```rust
let registration = ReminderRegistration {
    actor_id: "billing-actor@local".to_string(),
    reminder_name: "monthly_billing".to_string(),
    interval: Some(Duration { seconds: 2_592_000, nanos: 0 }), // 30 days
    first_fire_time: Some(Timestamp::from(SystemTime::now() + Duration::from_secs(2_592_000))),
    callback_data: b"billing-data".to_vec(),
    persist_across_activations: true,
    max_occurrences: -1, // Infinite
};
reminder_facet.register_reminder(registration).await?;
```

**Validation**:
- Reminder fires every 30 days (2 seconds in demo)
- Reminder persists across actor deactivation
- Reminder continues indefinitely
- If actor deactivates, reminder triggers auto-activation

### Scenario 2: SLA Check Reminder (Max Occurrences)

**Use Case**: Check SLA compliance every hour, max 24 times (daily monitoring)

```rust
let registration = ReminderRegistration {
    actor_id: "sla-actor@local".to_string(),
    reminder_name: "sla_check".to_string(),
    interval: Some(Duration { seconds: 3_600, nanos: 0 }), // 1 hour
    first_fire_time: Some(Timestamp::from(SystemTime::now())), // Fire immediately
    callback_data: b"sla-check-data".to_vec(),
    persist_across_activations: true,
    max_occurrences: 24, // Auto-delete after 24 fires
};
reminder_facet.register_reminder(registration).await?;
```

**Validation**:
- Reminder fires every hour (1 second in demo)
- After 24 fires, reminder is automatically deleted
- Reminder persists across deactivation until max_occurrences reached

### Scenario 3: Retry Reminder (Limited Retries)

**Use Case**: Retry failed operation every minute, max 3 times

```rust
let registration = ReminderRegistration {
    actor_id: "retry-actor@local".to_string(),
    reminder_name: "retry_send".to_string(),
    interval: Some(Duration { seconds: 60, nanos: 0 }), // 1 minute
    first_fire_time: Some(Timestamp::from(SystemTime::now())), // Fire immediately
    callback_data: b"retry-data".to_vec(),
    persist_across_activations: true,
    max_occurrences: 3, // Auto-delete after 3 fires
};
reminder_facet.register_reminder(registration).await?;
```

**Validation**:
- Reminder fires every minute (1.5 seconds in demo)
- After 3 fires, reminder is automatically deleted
- Demonstrates retry pattern with automatic cleanup

## How the Example Validates Features

### 1. Facet Attachment

**What it validates**: Reminders are opt-in via facet attachment

```rust
let reminder_facet = Box::new(ReminderFacet::with_activation_provider(
    storage.clone(),
    activation_provider,
));
actor.attach_facet(reminder_facet, 50, serde_json::json!({})).await?;
```

**Expected**: Facet is successfully attached, actor can now use reminders

### 2. Storage Persistence

**What it validates**: Reminders are persisted to storage backend

**Expected**:
- Reminders survive actor deactivation
- Reminders survive node crashes (with SQLite/PostgreSQL)
- Reminders are loaded on actor activation

### 3. VirtualActorFacet Integration

**What it validates**: Reminders can trigger actor auto-activation

```rust
let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config));
actor.attach_facet(virtual_facet, 100, virtual_facet_config).await?;
```

**Expected**:
- If actor is deactivated when reminder fires, actor is automatically activated
- ReminderFired message is sent after activation
- Actor processes reminder even if it was deactivated

### 4. Message Delivery

**What it validates**: Reminder fires send `ReminderFired` messages to actor's mailbox

**Expected**:
- Messages have `metadata["type"] = "ReminderFired"`
- Messages have `metadata["reminder_name"] = <reminder_name>`
- Messages include `fire_count` in payload
- Actor's behavior receives and processes messages

### 5. Max Occurrences Auto-Deletion

**What it validates**: Reminders auto-delete after max_occurrences fires

**Expected**:
- Reminder fires exactly `max_occurrences` times
- After last fire, reminder is automatically deleted from storage
- No memory leaks or orphaned reminders

### 6. Background Task Scheduling

**What it validates**: Background task efficiently queries due reminders

**Expected**:
- Task polls storage periodically (every 100ms by default)
- Only due reminders are queried (efficient index usage)
- Multiple reminders can fire simultaneously

## Running the Example

### Quick Run
```bash
cd examples/simple/reminders_example
cargo run
```

### Test Script (Recommended)
```bash
cd examples/simple/reminders_example
./test.sh
```

The test script will:
- Build the example in release mode
- Run it for a configured duration
- Validate expected behavior (facet attachment, storage setup, etc.)
- Show metrics (reminder fire counts, facet status)
- Exit with appropriate status codes for CI/CD

## Expected Output

```
[INFO] ╔════════════════════════════════════════════════════════════════╗
[INFO] ║  Reminders Example - Durable, Persistent Reminders            ║
[INFO] ║  with SQLite Backend & VirtualActorFacet Integration         ║
[INFO] ╚════════════════════════════════════════════════════════════════╝
[INFO] 📦 Creating journal storage (MemoryJournalStorage)...
[INFO] ✅ Storage created
[INFO] ✅ Node created
[INFO] 🔌 Attaching VirtualActorFacet for auto-activation...
[INFO] ✅ VirtualActorFacet attached
[INFO] 🔌 Attaching ReminderFacet with SQLite storage...
[INFO] ✅ ReminderFacet attached
[INFO] ✅ Actor spawned: reminder-actor@local
[INFO] 💰 Monthly billing reminder fired count=1
[INFO] 📊 SLA check reminder fired count=1
[INFO] 🔄 Retry reminder fired count=1
```

## Design Notes

### Why Durable?

- **Reliability**: Critical operations (billing, SLA) must not be lost
- **Crash Recovery**: Reminders survive node crashes and restarts
- **Long-Term Scheduling**: Operations scheduled for hours/days/months need persistence

### Storage Backends

- **MemoryJournalStorage**: For testing (not persistent)
- **SqliteJournalStorage**: For edge deployments (file-based)
- **PostgresJournalStorage**: For production (distributed, scalable)

### Integration with VirtualActorFacet

Reminders **do** trigger actor activation. When a reminder fires:
1. ReminderFacet checks if actor is active
2. If deactivated, calls `ActivationProvider.activate_actor()`
3. Waits for activation to complete
4. Sends `ReminderFired` message to actor's mailbox

This enables **serverless-like** behavior where actors are only active when needed.

### Performance Considerations

- **Background Task Polling**: Default 100ms interval (configurable)
- **Storage Queries**: Indexed by `next_fire_time` for efficient querying
- **Batch Processing**: Multiple due reminders can fire in one poll cycle

## When to Use Reminders vs Timers

| Feature | Timers | Reminders |
|---------|--------|-----------|
| **Durability** | ❌ In-memory only | ✅ Persisted |
| **Survives Deactivation** | ❌ No | ✅ Yes |
| **Survives Crash** | ❌ No | ✅ Yes |
| **Triggers Auto-Activation** | ❌ No | ✅ Yes |
| **Min Interval** | Milliseconds | Seconds |
| **Use Cases** | Heartbeats, polling | Billing, SLA, cron jobs |
| **Performance** | Fast (no I/O) | Slower (disk writes) |

## See Also

- `examples/simple/timers_example` - Non-durable timers for high-frequency operations
- `docs/TIMERS_REMINDERS_DESIGN.md` - Complete design documentation
- `crates/journaling/src/reminder_facet.rs` - Implementation details
- `crates/journaling/src/storage/sql.rs` - SQL backend implementation
