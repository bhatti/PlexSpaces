# Alarms API — Go WASM

Demonstrates the Cloudflare Durable Objects `setAlarm()` / `alarm()` pattern using PlexSpaces Go WASM actors.

A `RequestQueueActor` batches incoming requests and processes them 10 seconds after the **first** write — using a durable alarm that survives actor deactivation and node restarts.

## Cloudflare DO vs PlexSpaces Go

| Cloudflare DO                              | PlexSpaces Go                               |
|--------------------------------------------|---------------------------------------------|
| `export class RequestQueue extends DO`     | `RequestQueue struct + BaseActor`           |
| `this.ctx.storage.get('count')`            | `host.KV().Get("count")`                    |
| `this.ctx.storage.put('count', n)`         | `host.KV().Put("count", val)`               |
| `this.ctx.storage.setAlarm(Date.now()+10s)`| `host.Alarm().Set(nowMs + 10_000)`          |
| `this.ctx.storage.getAlarm()`              | `host.Alarm().Get()`                        |
| `async alarm() { ... }`                    | `case "__alarm__":`                         |
| `new Response(JSON.stringify(result))`     | `return marshal(map[string]any{...})`       |
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
./build.sh
```

Requires: `tinygo`, `wasm-tools`, `wasm-opt` (binaryen), WASI adapter (jco).

## Run

```bash
# Start PlexSpaces node (separate terminal)
PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091

# Run test
./test.sh
```

## Related

- [TypeScript version](../../../typescript/apps/alarms_api/)
- [Python version](../../../python/apps/alarms_api/)
- [Rust version](../../../rust/apps/alarms_api/)
- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
