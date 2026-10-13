# Job Queue Outbox — Flow-Controlled Messaging (Python WASM)

Demonstrates the outbox pattern with AIMD concurrency control for reliable,
flow-controlled message delivery from a coordinator to worker actors.

## Features

- **Outbox Dispatch** — Send messages to multiple targets via `host.outbox.dispatch()`
- **AIMD Flow Control** — Adaptive concurrency: additive increase on success, multiplicative decrease on RESOURCE_EXHAUSTED
- **AdmissionControlFacet** — Receiver-side flow control on workers
- **Dispatcher Status** — Monitor pending/delivered/failed counts via `host.outbox.status()`
- **Batch Dispatch** — Generate target IDs and dispatch in bulk

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
      (AdmissionControlFacet on each)
```

## Running

```bash
# Build WASM actor
./build.sh

# Run test (requires running PlexSpaces node)
./test.sh [port]
```

## What it demonstrates

1. **Outbox dispatch** — `host.outbox.dispatch(ctx, dispatcher_id, targets, payload, msg_type, key)`
2. **Batch dispatch** — Generate N target IDs and dispatch in one call
3. **Status monitoring** — `host.outbox.status(ctx, dispatcher_id)` for delivery metrics
4. **Flow control** — AdmissionControlFacet on workers limits concurrent processing

## SDK API

```python
from plexspaces import host

# Dispatch to specific targets
accepted, rejected = host.outbox.dispatch(
    ctx, "dispatcher-1",
    ["worker-0", "worker-1", "worker-2"],
    {"task": "process"},
    "cast", ""
)

# Check delivery status
status = host.outbox.status(ctx, "dispatcher-1")
# => {"pending": 0, "delivered": 3, "failed": 0, ...}
```

## Configuration

See `app-config.toml` for the supervisor tree:
- 1 coordinator (dispatches via outbox)
- 4 workers (each with `admission_control` facet for backpressure)

## References

- [Architecture — Outbox & Flow Control](../../../../docs/architecture.md)
- [Detailed Design — Flow-Controlled Messaging](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Rust Job Queue Example](../../../rust/embedded/job_queue/)
