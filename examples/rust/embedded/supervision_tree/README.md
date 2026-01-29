# Supervision Tree Example

**Purpose**: Demonstrate Erlang/OTP-style fault tolerance with supervision trees.

**Pattern**: Supervisor manages child actors, automatically restarts on failures.

## Quick Start

```bash
cd examples/rust/embedded/supervision_tree

# Build
cargo build

# Run
cargo run

# Run with debug logging
RUST_LOG=debug cargo run

# Run in release mode
cargo run --release
```

## What It Demonstrates

1. **OneForOne Strategy**: If a child fails, only that child is restarted
2. **OneForAll Strategy**: If one child fails, ALL children are restarted
3. **RestForOne Strategy**: If a child fails, restart it AND all children started after it
4. **Failure Recovery**: Automatic restart on crashes with statistics
5. **Restart Policies**: Permanent, Transient, Temporary

## Architecture

```
                  ┌───────────────────────┐
                  │      Supervisor       │
                  │  (Strategy: OneFor*)  │
                  └───────────────────────┘
                             │
        ┌────────────────────┼────────────────────┐
        ▼                    ▼                    ▼
   ┌─────────┐          ┌─────────┐          ┌─────────┐
   │ Worker1 │          │ Worker2 │          │ Worker3 │
   │Permanent│          │Permanent│          │Transient│
   └─────────┘          └─────────┘          └─────────┘
```

## Supervision Strategies

| Strategy | Behavior |
|----------|----------|
| **OneForOne** | Only restart the failed child (most common) |
| **OneForAll** | Restart ALL children if one fails (tightly coupled) |
| **RestForOne** | Restart failed child + all started after it (ordered deps) |

## Restart Policies

| Policy | Behavior |
|--------|----------|
| **Permanent** | Always restart, regardless of exit reason |
| **Transient** | Only restart on abnormal exit (crash), not normal exit |
| **Temporary** | Never restart |

## Key Code Patterns

### Creating a Supervisor

```rust
use plexspaces_actor::supervisor::{Supervisor, SupervisionStrategy};

let (supervisor, mut event_rx) = Supervisor::new(
    "my-supervisor".to_string(),
    SupervisionStrategy::OneForOne {
        max_restarts: 5,      // Max restarts before giving up
        within_seconds: 60,   // Time window for restart counting
    },
    service_locator.clone(),
);
```

### Adding Workers

```rust
use plexspaces_actor::child_spec::{ChildSpec, RestartStrategy, StartFn, StartedChild};

let start_fn: StartFn = Arc::new(move || {
    let actor_id = worker_id.clone();
    Box::pin(async move {
        let mailbox = Mailbox::new(MailboxConfig::default(), actor_id.clone()).await?;
        let actor = Actor::new(actor_id.clone(), behavior, mailbox, tenant, namespace, None);
        let actor_ref = CoreActorRef::new(actor_id)?;
        Ok(StartedChild::Worker { actor, actor_ref })
    })
});

let spec = ChildSpec::worker(id.clone(), id, start_fn)
    .with_restart(RestartStrategy::Permanent);

supervisor.add_child(spec).await?;
```

### Handling Events

```rust
while let Some(event) = event_rx.recv().await {
    match event {
        SupervisorEvent::ChildStarted(id) => println!("Started: {}", id),
        SupervisorEvent::ChildFailed(id, reason) => println!("Failed: {}", id),
        SupervisorEvent::ChildRestarted(id, count) => println!("Restarted: {}", id),
        SupervisorEvent::MaxRestartsExceeded(id) => println!("Gave up on: {}", id),
        _ => {}
    }
}
```

### Simulating Failures (Testing)

```rust
supervisor.handle_failure(&worker_id, "crash reason".to_string(), None).await?;
```

## Expected Output

```
╔════════════════════════════════════════════════════════════════╗
║           Supervision Tree Example                             ║
╚════════════════════════════════════════════════════════════════╝

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Example 1: Basic Supervisor (OneForOne Strategy)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

OneForOne: If a child fails, only that child is restarted.

  ✓ Worker started: worker-1@supervision-node
  ✓ Worker started: worker-2@supervision-node
  ✓ Worker started: worker-3@supervision-node

  Supervisor stats:
    - Total restarts: 0
    - Successful restarts: 0

  ✓ Supervisor shutdown complete

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Example 2: Failure Recovery (Automatic Restart)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Simulating a failure and observing automatic restart...

  ✓ Worker started: crashable-worker@supervision-node
  → Simulating crash...
  ✗ Worker failed: crashable-worker@supervision-node (reason: simulated crash)
  ✓ Worker restarted: crashable-worker@supervision-node (restart #1)

...

...
```

## Use Cases

**Use Supervision Trees when:**
- Building fault-tolerant systems that must recover from crashes
- Managing stateful actors that need isolation
- Implementing "let it crash" philosophy (Erlang/OTP style)
- Services with dependencies (database → cache → API)

**Choose your strategy:**
- **OneForOne**: Independent workers (web request handlers)
- **OneForAll**: Tightly coupled workers (database connection pool)
- **RestForOne**: Ordered dependencies (startup sequence)

## See Also

- [Actor Groups (Sharding)](../actor_groups_sharding/) - For horizontal scaling
- [Durable Actor](../durable_actor/) - For persistent state
- [Architecture Docs](../../../../docs/architecture.md)
