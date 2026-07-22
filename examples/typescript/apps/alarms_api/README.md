# Alarms API — TypeScript WASM

Demonstrates the Cloudflare Durable Objects `setAlarm()` / `alarm()` pattern using PlexSpaces TypeScript WASM actors.

A `RequestQueueActor` batches incoming requests and processes them 10 seconds after the **first** write — using a durable alarm that survives actor deactivation and node restarts.

## Cloudflare DO vs PlexSpaces

| Cloudflare DO                              | PlexSpaces TypeScript                       |
|--------------------------------------------|---------------------------------------------|
| `export class RequestQueue extends DO`     | `class RequestQueueActor extends PlexSpacesActor` |
| `this.ctx.storage.get('count')`            | `host.kvGet('count')` / `getState()`        |
| `this.ctx.storage.put('count', n)`         | `host.kvPut('count', ...)` / `setState()`   |
| `this.ctx.storage.setAlarm(Date.now()+10s)`| `host.alarm.set(host.nowMs() + 10_000)`     |
| `this.ctx.storage.getAlarm()`              | `host.alarm.get()`                          |
| `async alarm() { ... }`                    | `on__alarm__()` handler                     |
| `new Response(JSON.stringify(result))`     | `return { ...result }`                      |
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
npm install
./build.sh
```

## Run

```bash
# Start PlexSpaces node (separate terminal)
PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091

# Run test
./test.sh
```

## Related

- [Go version](../../../go/apps/alarms_api/)
- [Python version](../../../python/apps/alarms_api/)
- [Rust version](../../../rust/apps/alarms_api/)
- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
