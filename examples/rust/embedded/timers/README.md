# Timers Example (TimerFacet) - SDK Annotations Demo

**Purpose**: Demonstrate in-memory timers using PlexSpaces `TimerFacet` with **SDK annotations** and comprehensive metrics.

**PlexSpaces APIs**: `TimerFacet`, `register_once()`, `register_periodic()`, `cancel()`, **SDK annotations**, `CoordinationComputeTracker`

## Quick Start

```bash
cd examples/rust/embedded/timers
cargo build
cargo run
```

## What It Demonstrates

1. **SDK Annotations**: `#[gen_server_actor]`, `#[plexspaces_handlers]`, `#[handler]` for clean actor definitions
2. **TimerFacet attachment**: Attach timer capability to actors via `spawn_with_facets`
3. **One-shot timers**: `register_once(name, delay)` - fire once after delay
4. **Periodic timers**: `register_periodic(name, interval)` - fire repeatedly
5. **Timer cancellation**: `cancel(name)` - stop a timer
6. **Performance Metrics**: `CoordinationComputeTracker` tracks coordination overhead vs timer processing

## Use Case

**Session Management**: Manage multiple user sessions with idle timeout, heartbeat, and retry timers.

- **80 sessions** with various timer patterns
- **Idle timeout**: 30 seconds (one-shot timer)
- **Heartbeat**: 5 seconds (periodic timer)
- **Retry**: 2 seconds (one-shot timer for some sessions)
- **Activity simulation**: Cancel and re-register timers for some sessions

## PlexSpaces SDK Usage

### Define Actor with SDK Annotations

```rust
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, spawn_with_facets, TimerFacet,
};

// Step 1: Annotate struct with #[gen_server_actor(...)]
#[gen_server_actor(name = "session_actor", facets = ["timer"])]
struct SessionActor {
    user_id: String,
    is_active: bool,
    activity_count: u32,
    timer_fire_count: u32,
}

// Step 2: Annotate impl with #[plexspaces_handlers]
#[plexspaces_handlers]
impl SessionActor {
    #[handler("timer_fired", cast)]  // fire-and-forget
    async fn handle_timer_fired(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let timer_name = String::from_utf8_lossy(&msg.payload);
        match timer_name.as_ref() {
            "idle_timeout" => self.is_active = false,
            "heartbeat" => {}, // Heartbeat received
            "retry" => {}, // Retry timer fired
            _ => {}
        }
        Ok(())
    }
}
```

### Spawn Actor with TimerFacet

```rust
use plexspaces_sdk::{spawn_with_facets, RequestContext};
use plexspaces_node::NodeBuilder;
use plexspaces_core::ActorId;

// Create TimerFacet
let timer_facet = Box::new(TimerFacet::new(json!({}), 50, service_locator.clone()));

// Spawn with SDK helper and attach the timer facet
let actor_ref = spawn_with_facets(
    &ctx,
    service_locator,
    "session-user-123".to_string(),
    "sessions",
    SessionActor::new("user-123"),
    vec![timer_facet],
).await?;
```

### Register Timers

```rust
// Register idle timeout (one-shot)
timer_facet.register_once("idle_timeout", Duration::from_secs(30)).await?;

// Register heartbeat (periodic)
timer_facet.register_periodic("heartbeat", Duration::from_secs(5)).await?;

// Register retry timer (one-shot)
timer_facet.register_once("retry", Duration::from_secs(2)).await?;
```

### Cancel Timer

```rust
// Cancel on user activity (reset timeout)
timer_facet.cancel("idle_timeout").await?;

// Re-register idle_timeout (reset timer)
timer_facet.register_once("idle_timeout", Duration::from_secs(30)).await?;
```

## Key Code Patterns

### RequestContext Usage

```rust
// ✅ CORRECT - Explicit tenant/namespace
let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "sessions".to_string());

// ❌ FORBIDDEN - Never use internal() except for system init
// let ctx = RequestContext::internal();
```

### CoordinationComputeTracker Usage

```rust
use plexspaces_node::CoordinationComputeTracker;

let mut metrics_tracker = CoordinationComputeTracker::new("timers".to_string());

// Track coordination (spawning actors, registering timers)
metrics_tracker.start_coordinate();
// ... spawn actors, register timers ...
metrics_tracker.end_coordinate();

// Track computation (timer processing)
metrics_tracker.start_compute();
// ... simulate timer processing ...
metrics_tracker.end_compute();

// Get final metrics
let metrics = metrics_tracker.finalize();
```

## Expected Output

```
╔════════════════════════════════════════════════════════════════╗
║           Timers Example (TimerFacet)                         ║
╚════════════════════════════════════════════════════════════════╝

Configuration:
  Sessions: 80
  Idle timeout: 30s
  Heartbeat interval: 5s
  Retry delay: 2s

Step 1: Create PlexSpaces Node
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Node: timers-node
  Created in 45.23ms

Step 2: Create 80 session actors with TimerFacet
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Spawned session actor: session-user-0@timers-node
  Spawned session actor: session-user-1@timers-node
  ...
  Created 80 session actors in 234.56ms
  Average spawn time: 2.93ms per actor

Step 3: Register timers for all sessions
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Registered timers for session-user-0@timers-node
  ...
  Registered 213 timers in 156.78ms
  Average registration time: 0.74ms per timer

Step 4: Simulate timer processing
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Simulated timer processing for 2.50s

Step 5: Cancel timers for some sessions (simulate activity)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Cancelled and re-registered 20 timers in 45.12ms

Step 6: Performance Metrics
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Execution Summary:
  Total execution time: 2878.45ms (2.88s)
  Sessions: 80
  Timers registered: 213
  Timers cancelled: 20

Coordination vs Computation Breakdown:
  Coordination time: 436.45ms (15.2%)
  Computation time: 2500.00ms (86.9%)
  Efficiency (compute/total): 86.9%

Timer Operations Metrics:
  Total timer operations: 293
  Average latency per operation: 1.49ms
  Timer operations per second: 101.8
  Sessions per second: 27.8

Benchmark Metrics:
  Timer operations per second: 101.8
  Sessions per second: 27.8

Granularity Analysis:
  Granularity ratio (compute/coordinate): 5.73
  ⚠️  Moderate granularity (coordination overhead is noticeable)
```

## Key Concepts

| Concept | Description |
|---------|-------------|
| **TimerFacet** | Facet that adds timer capability to actors |
| **In-memory** | Timers are NOT persisted (lost on crash) |
| **Fire message** | Timer fires message to `handle_message()` |
| **register_once** | One-shot timer (idle timeout, retry) |
| **register_periodic** | Repeating timer (heartbeat, polling) |
| **cancel** | Stop an active timer |

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

## Performance Metrics

The example uses `CoordinationComputeTracker` to measure:
- **Coordination time**: Actor spawning, timer registration, cancellation
- **Computation time**: Timer processing (simulated)
- **Granularity ratio**: `compute_time / coordinate_time` (should be >= 10×)
- **Timer operations per second**: Throughput of timer operations
- **Sessions per second**: Throughput of session management

### Metrics Interpretation

- **Granularity ratio >= 100×**: Excellent (coordination overhead negligible)
- **Granularity ratio >= 10×**: Good (coordination overhead low)
- **Granularity ratio >= 1×**: Moderate (coordination overhead noticeable)
- **Granularity ratio < 1×**: Poor (coordination overhead dominates)

For timer workloads, coordination overhead includes actor spawning and timer registration, while computation includes timer processing. The granularity ratio helps identify if timer operations are efficient enough.

## SDK Annotations Reference

| Annotation | Description |
|------------|-------------|
| `#[gen_server_actor(name = "session_actor", facets = ["timer"])]` | Declares the runtime actor type and attached facets |
| `#[plexspaces_handlers]` | Generates Actor impl with dispatch |
| `#[handler("op", cast)]` | Fire-and-forget handler (no reply) |
| `spawn_with_facets(..., facets)` | Spawn actor with facets attached |

## Design Principles

- **Core Functionality**: `TimerFacet` lives in main crates (`sdks/rust/plexspaces-sdk`)
- **No Hacks**: Proper trait usage, no cyclic dependencies
- **Observability**: `CoordinationComputeTracker` for metrics
- **Tenant Isolation**: Explicit `RequestContext` with tenant/namespace (no `internal()`)
- **Shared Target Directory**: Uses workspace shared target (`target/` at root)

## See Also

- [Reminders Example](../reminders/) - Durable reminders with ReminderFacet
- [Bank Account Example](../bank_account/) - Durable actors with journaling
- [SDK Documentation](../../../../docs/sdk.md) - Full SDK reference
- [Architecture Docs](../../../../docs/architecture.md)
