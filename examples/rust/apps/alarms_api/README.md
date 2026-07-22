# Alarms API — Rust WASM

Demonstrates the Cloudflare Durable Objects `setAlarm()` / `alarm()` pattern using PlexSpaces Rust WASM actors.

A `RequestQueueActor` batches incoming requests and processes them 10 seconds after the **first** write — using a durable alarm that survives actor deactivation and node restarts.

## Cloudflare DO vs PlexSpaces Rust

| Cloudflare DO                              | PlexSpaces Rust                             |
|--------------------------------------------|---------------------------------------------|
| `export class RequestQueue extends DO`     | `struct RequestQueueState` (WASM actor)     |
| `this.ctx.storage.get('count')`            | `host::kv_get("count")`                     |
| `this.ctx.storage.put('count', n)`         | `host::kv_put("count", ...)`                |
| `this.ctx.storage.setAlarm(Date.now()+10s)`| `host::alarm_set(now_ms + 10_000)`          |
| `this.ctx.storage.getAlarm()`              | `host::alarm_get()`                         |
| `async alarm() { ... }`                    | `"__alarm__"` message handler               |
| `new Response(JSON.stringify(result))`     | `serde_json::to_vec(&json!({...}))`         |
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

Requires: Rust `wasm32-wasip1` target, `wasm-tools`, WASI adapter (jco).

```bash
rustup target add wasm32-wasip1
cargo install wasm-tools
npm install -g @bytecodealliance/jco
```

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
- [Python version](../../../python/apps/alarms_api/)
- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
