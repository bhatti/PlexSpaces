# Alarms API — Python WASM

Demonstrates the Cloudflare Durable Objects `setAlarm()` / `alarm()` pattern using PlexSpaces Python WASM actors.

A `RequestQueueActor` batches incoming requests and processes them 10 seconds after the **first** write — using a durable alarm that survives actor deactivation and node restarts.

## Cloudflare DO vs PlexSpaces Python

| Cloudflare DO                              | PlexSpaces Python                           |
|--------------------------------------------|---------------------------------------------|
| `export class RequestQueue extends DO`     | `@actor class RequestQueueActor`            |
| `this.ctx.storage.get('count')`            | `self.count` (state field, auto-persisted)  |
| `this.ctx.storage.put('count', n)`         | `self.count = n` (auto-persisted)           |
| `this.ctx.storage.setAlarm(Date.now()+10s)`| `host.alarm.set(host.now_ms() + 10_000)`    |
| `this.ctx.storage.getAlarm()`              | `host.alarm.get()`                          |
| `async alarm() { ... }`                    | `@handler("__alarm__")`                     |
| `new Response(JSON.stringify(result))`     | `return {"status": "ok", ...}`              |
| `wrangler.toml [[durable_objects]]`        | `app-config.toml [[supervisor.children]]`   |

## Handlers

| Handler       | Description                                               |
|---------------|-----------------------------------------------------------|
| `enqueue`     | Add item to queue; set alarm on first item                |
| `status`      | Return queue depth and alarm timestamp                    |
| `reset`       | Clear queue and cancel alarm (for testing)                |
| `__alarm__`   | Process all queued items and clear queue                  |

## Build

```bash
# Activate Python virtualenv if using one
source ~/venv/bin/activate
./build.sh
```

Requires: PlexSpaces Python SDK (`pip install -e ../../../../sdks/python`).

## Run

```bash
# Start PlexSpaces node (separate terminal)
PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091

# Run test
./test.sh
```

## Related

- [TypeScript version](../../../typescript/apps/alarms_api/)
- [Go version](../../../go/apps/alarms_api/)
- [Rust version](../../../rust/apps/alarms_api/)
- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
