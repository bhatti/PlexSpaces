# Job Queue Outbox — Flow-Controlled Messaging (TypeScript WASM)

Demonstrates the outbox pattern with AIMD concurrency control for reliable,
flow-controlled message delivery from a coordinator to worker actors.

## Features

- **Outbox Dispatch** — Send messages to multiple targets via `host.outbox.dispatch()`
- **AIMD Flow Control** — Adaptive concurrency: additive increase on success, multiplicative decrease on RESOURCE_EXHAUSTED
- **Bounded mailbox** — Receiver-side backpressure gate; `MailboxFull` → `DeliveryError::Capacity` → AIMD decrease
- **Dispatcher Status** — Monitor pending/delivered/failed counts via `host.outbox.status()`
- **Batch Dispatch** — Generate target IDs and dispatch in bulk
- **In-Process Verification** — `verify.mjs` tests actor logic without a running node

## Architecture

```
Coordinator → host.outbox.dispatch(targets, payload)
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
# Build JS (for verification)
./build.sh

# In-process verification (no server needed)
node verify.mjs

# Full test with deployed WASM (requires running PlexSpaces node)
./test.sh [port]
```

## What it demonstrates

1. **Outbox dispatch** — `host.outbox.dispatch(ctx, dispatcherID, targets, payload, msgType, key)`
2. **Batch dispatch** — Generate N target IDs and dispatch in one call
3. **Status monitoring** — `host.outbox.status(ctx, dispatcherID)` for delivery metrics
4. **Flow control** — Bounded mailbox on workers; `MailboxFull` → `DeliveryError::Capacity` → AIMD decrease

## SDK API

```typescript
import { Host } from "@plexspaces/sdk";
const host = new Host();

// Dispatch to specific targets
const [accepted, rejected] = host.outbox.dispatch(
  ctx, "dispatcher-1",
  ["worker-0", "worker-1", "worker-2"],
  new TextEncoder().encode(JSON.stringify({ task: "process" })),
  "cast", ""
);

// Check delivery status (JSON string)
const statusJson = host.outbox.status(ctx, "dispatcher-1");
const status = JSON.parse(statusJson);
```

## Configuration

See `app-config.toml` for the supervisor tree:
- 1 coordinator (`JobQueueOutboxCoordinator`)
- 4 workers (`OutboxWorker`, bounded mailbox for backpressure)

## References

- [Architecture — Outbox & Flow Control](../../../../docs/architecture.md)
- [Detailed Design — Flow-Controlled Messaging](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Rust Job Queue Example](../../../rust/embedded/job_queue/)
