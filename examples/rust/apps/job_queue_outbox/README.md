# Job Queue Outbox — Flow-Controlled Messaging (Rust WASM)

Demonstrates the outbox pattern with AIMD concurrency control for reliable,
flow-controlled message delivery from a coordinator to worker actors.

## Features

- **Outbox Dispatch** — Send messages to multiple targets via `outbox_dispatch` WIT call
- **AIMD Flow Control** — Adaptive concurrency: additive increase on success, multiplicative decrease on RESOURCE_EXHAUSTED
- **Bounded mailbox** — Receiver-side backpressure gate; `MailboxFull` → `DeliveryError::Capacity` → AIMD decrease
- **Dispatcher Status** — Monitor pending/delivered/failed counts via `outbox_status` WIT call
- **Batch Dispatch** — Generate target IDs and dispatch in bulk
- **State Persistence** — get-state/set-state via serde_json across re-instantiation

## Architecture

```
Coordinator → outbox_dispatch(targets, payload)
                  │
                  ▼
              OutboxRelay (AIMD concurrency limiter)
                  │
          ┌───────┼───────┐───────┐
          ▼       ▼       ▼       ▼
      worker-0  worker-1  worker-2  worker-3
      (bounded mailbox — MailboxFull → AIMD decrease)
```

## Running

```bash
# Build WASM component (requires cargo, wasm-tools)
./build.sh

# Run test (requires running PlexSpaces node)
HTTP_PORT=8091 ./test.sh
```

## What it demonstrates

1. **Outbox dispatch** — `outbox_dispatch(ctx, dispatcher_id, targets, payload, msg_type, key)`
2. **Batch dispatch** — Generate N target IDs and dispatch in one call
3. **Status monitoring** — `outbox_status(ctx, dispatcher_id)` for delivery metrics
4. **Flow control** — Bounded mailbox on workers; `MailboxFull` → `DeliveryError::Capacity` → AIMD decrease

## WIT API

```rust
// Dispatch messages to target actors via the outbox relay
use plexspaces::actor::outbox::{outbox_dispatch, outbox_status};

let targets = vec!["OutboxWorker:worker-0".to_string(), ...];
let payload_bytes = r#"{"task":"process"}"#.as_bytes().to_vec();
match outbox_dispatch(ctx, dispatcher_id, &targets, &payload_bytes, "cast", "") {
    Ok(result_str) => {
        // result_str is JSON: {"accepted":N,"rejected":M}
    }
    Err(e) => { ... }
}

// Get dispatcher status
let status_json = outbox_status(ctx, dispatcher_id).unwrap_or_default();
```

## Target Address Format

Use `"ActorType:actor-name"` format for outbox targets:

```rust
// ✅ CORRECT — type:name addressing
let targets = vec![
    "OutboxWorker:worker-0".to_string(),
    "OutboxWorker:worker-1".to_string(),
];

// ❌ WRONG — bare name treated as actor type, not instance name
let targets = vec!["worker-0".to_string()];
```

## Configuration

See `app-config.toml` for the supervisor tree:
- 1 coordinator (`JobQueueOutboxCoordinator`)
- 4 workers (`OutboxWorker`, bounded mailbox for backpressure)

## References

- [Architecture — Outbox & Flow Control](../../../../docs/architecture.md)
- [Detailed Design — Flow-Controlled Messaging](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Go Job Queue Outbox Example](../../../go/apps/job_queue_outbox/)
- [TypeScript Job Queue Outbox Example](../../../typescript/apps/job_queue_outbox/)
- [Python Job Queue Outbox Example](../../../python/apps/job_queue_outbox/)
