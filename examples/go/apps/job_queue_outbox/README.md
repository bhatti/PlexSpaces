# Job Queue Outbox — Flow-Controlled Messaging (Go WASM)

Demonstrates the outbox pattern with AIMD concurrency control for reliable,
flow-controlled message delivery from a coordinator to worker actors.

## Features

- **Outbox Dispatch** — Send messages to multiple targets via `host.Outbox().Dispatch()`
- **AIMD Flow Control** — Adaptive concurrency: additive increase on success, multiplicative decrease on RESOURCE_EXHAUSTED
- **AdmissionControlFacet** — Receiver-side flow control on workers
- **Dispatcher Status** — Monitor pending/delivered/failed counts via `host.Outbox().Status()`
- **Batch Dispatch** — Generate target IDs and dispatch in bulk

## Architecture

```
Coordinator → host.Outbox().Dispatch(targets, payload)
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
# Build WASM component (requires tinygo + wasm-tools)
./build.sh

# Run test (requires running PlexSpaces node)
./test.sh [port]
```

## What it demonstrates

1. **Outbox dispatch** — `host.Outbox().Dispatch(ctx, dispatcherID, targets, payload, msgType, key)`
2. **Batch dispatch** — Generate N target IDs and dispatch in one call
3. **Status monitoring** — `host.Outbox().Status(ctx, dispatcherID)` for delivery metrics
4. **Flow control** — AdmissionControlFacet on workers limits concurrent processing

## SDK API

```go
host := plexspaces.NewHost()

// Dispatch to specific targets (use DispatchWithTargets — avoids marshalPayload)
result, err := host.Outbox().DispatchWithTargets(
    ctx, "dispatcher-1",
    []string{"worker-0", "worker-1", "worker-2"},
    `{"task":"process"}`,  // pre-encoded JSON string
    "cast", "",
)
// result.Accepted, result.Rejected

// Check delivery status
status, err := host.Outbox().Status(ctx, "dispatcher-1")
// status["pending"], status["delivered"], status["failed"]
```

## ⚠️ TinyGo WASM JSON Rules

This example deliberately avoids patterns that corrupt TinyGo's WASM function table:

```go
// ❌ NEVER — links fmtsort, corrupts actor dispatch (call_indirect trap)
json.Marshal(map[string]any{"count": n})
fmt.Sprintf("accepted=%d", n)

// ❌ NEVER — adds ~60 encoding/json.state* entries to the element table
json.Unmarshal([]byte(payloadJSON), &req)

// ✅ USE INSTEAD — type-switch encoder, no fmtsort
plexspaces.SafeMarshal(map[string]any{"count": n})

// ✅ USE INSTEAD — zero-alloc JSON helpers (no encoding/json)
targets := extractStringArray(payloadJSON, "targets")
batchSize := plexspaces.WasmJSONExtractInt(payloadJSON, "batch_size")
actorID := plexspaces.WasmJSONExtractStr(configJSON, "actor_id")
```

See [TinyGo WASM Safety Rules](../../../../docs/polyglot.md#️-tinygo-wasm-json-and-fmt-safety-rules) for the full guide.

## State Persistence

TinyGo WASM actors are re-instantiated after EVERY `handle()` call. State is preserved
via `get-state`/`set-state` WIT exports. Implement `WASMStateful` to persist fields:

```go
func (j *JobQueueOutbox) MarshalStateWASM() string {
    return plexspaces.SafeMarshal(map[string]any{
        "total_dispatched": j.TotalDispatched,
        // ...
    })
}

func (j *JobQueueOutbox) UnmarshalStateWASM(stateJSON string) bool {
    j.TotalDispatched = plexspaces.WasmJSONExtractInt(stateJSON, "total_dispatched")
    // ...
    return true
}
```

## Target Address Format

Use `"ActorType:actor-name"` format for outbox targets (not bare names):

```go
// ✅ CORRECT — type:name addressing
targets := []string{"OutboxWorker:worker-0", "OutboxWorker:worker-1"}

// ❌ WRONG — bare name treated as actor type, not instance name
targets := []string{"worker-0", "worker-1"}
```

## Configuration

See `app-config.toml` for the supervisor tree:
- 1 coordinator (`JobQueueOutboxCoordinator`)
- 4 workers (`OutboxWorker`, each with `admission_control` facet)

## References

- [Architecture — Outbox & Flow Control](../../../../docs/architecture.md)
- [Detailed Design — Flow-Controlled Messaging](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Rust Job Queue Example](../../../rust/embedded/job_queue/)
