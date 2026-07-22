# Cloudflare Workers - Discord-style Guild Chat Server (Rust)

Demonstrates **Cloudflare Workers Durable Objects** pattern for real-time chat using the PlexSpaces Rust SDK macros.

**Real-world use case**: Chat platforms (Discord, Slack), collaborative editors,
multiplayer game lobbies — anywhere you need per-entity stateful coordination
at the edge. Inspired by [Discord's guild process architecture](https://discord.com/blog/how-discord-scaled-elixir-to-5-000-000-concurrent-users)
and the [Cloudflare workers-chat-demo](https://github.com/cloudflare/workers-chat-demo).

See also: [Go version](../../go/apps/migrating_cloudflare_workers/) | [Python version](../../python/apps/migrating_cloudflare_workers/)

## Architecture

```
              ┌──────────────────────────────────┐
              │   ChatRoomActor (Durable Object) │
              │                                  │
              │  ┌────────────────────────────┐  │
              │  │  members      messages     │  │
              │  │  user-1 ──┐  [seq1,seq2..] │  │
              │  │  user-2 ──┤  (ring buffer) │  │
              │  │  user-3 ──┘                │  │
              │  └────────────────────────────┘  │
              └──────────────────────────────────┘
                    │           │           │
              join/leave   send_message  get_history
                    │        (fan-out)      │
              ┌─────▼──────────▼───────────▼──────┐
              │     RateLimiterActor              │
              │     Per-user token bucket         │
              │     user-1: [5 tokens, refill@1s] │
              └───────────────────────────────────┘
              ┌───────────────────────────────────┐
              │     AlarmDemoActor                │
              │     Durable alarm (setAlarm)      │
              │     pending: [task-1, task-2...]  │
              └───────────────────────────────────┘
```

**Chat flow** (like Discord's guild process):
1. Users `join` a room → tracked in `HashMap<String, MemberInfo>`
2. `send_message` → add to history + fan-out to all members via `host::send()`
3. Rate limiter enforces per-user message limits (token bucket)
4. Message history persisted via KV (like DO `state.storage.put`)
5. Alarm demo shows durable batch processing pattern

## Quick Start

```bash
# Terminal 1: Start PlexSpaces node
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091

# Terminal 2: Build and run
cd examples/rust/apps/migrating_cloudflare_workers
./build.sh        # Builds guild_chat.wasm (requires wasm-tools + WASI adapter)
./test.sh 8091    # Deploy + test chat + rate limiting + alarms + benchmarks
```

## PlexSpaces Rust SDK Features

| Feature | How Used |
|---------|----------|
| `wit_bindgen::generate!` | WIT interface binding for WASM host functions |
| `impl Guest` (WIT binding) | Routes init/handle to the correct actor type |
| `host::send()` | Fire-and-forget fan-out to member actors |
| `host::kv_put()/host::kv_get()` | Durable storage (like DO `state.storage.put/get`) |
| `host::kv_multi_get()/kv_multi_put()` | Batch KV (like DO `storage.get([k1,k2,...])`) |
| `host::kv_cas(key, old, new)` | Atomic compare-and-swap for idempotent ops |
| `host::kv_increment(key, delta)` | Atomic distributed counter (rate tracking) |
| `host::alarm_set(timestamp_ms)` | Schedule durable alarm (like DO `storage.setAlarm()`) |
| `host::alarm_get()` | Query pending alarm (like DO `storage.getAlarm()`) |
| `host::alarm_delete()` | Cancel alarm (like DO `storage.deleteAlarm()`) |
| `host::now_ms()` | Timestamps for messages + token bucket refill |
| `host::self_id()` | Actor identity for room ID extraction |
| `host::log()` | Structured logging |
| `OnceLock<Mutex<Option<ActorState>>>` | WASM-safe per-instance state |
| Serde `#[serde(tag = "actor_type")]` | Enum-tagged state for multi-actor routing |

## Comparison: Cloudflare Workers/DO vs PlexSpaces Rust

| Feature | Cloudflare Workers/DO | PlexSpaces Rust |
|---------|----------------------|-----------------|
| Actor class | `export class ChatRoom extends DurableObject` | `struct ChatRoomState` + WIT bindings |
| Get instance | `env.CHAT_ROOM.get(id)` | `host::send(actorID, ...)` |
| Durable storage | `this.state.storage.put/get` | `host::kv_put()/host::kv_get()` |
| Batch storage | `storage.get([k1,k2,...])` / `storage.put({...})` | `host::kv_multi_get()` / `host::kv_multi_put()` |
| Atomic CAS | Transactional storage read-modify-write | `host::kv_cas(key, expected, new)` |
| Atomic counter | KV atomic increment | `host::kv_increment(key, delta)` |
| Request handler | `async fetch(request)` | `impl Guest { fn handle(...) }` |
| Init/migration | `blockConcurrencyWhile()` | State restored from KV in `chatroom_init()` |
| Multi-actor | `wrangler.toml [[bindings]]` | `app-config.toml [[children]]` |
| Fan-out | WebSocket broadcast | `host::send()` fire-and-forget to member actors |
| Rate limiting | Separate DO class per IP | `RateLimiterState` token bucket |
| Set alarm | `this.state.storage.setAlarm(timestamp)` | `host::alarm_set(timestamp_ms)` |
| Get alarm | `this.state.storage.getAlarm()` | `host::alarm_get()` |
| Cancel alarm | `this.state.storage.deleteAlarm()` | `host::alarm_delete()` |
| Alarm callback | `async alarm() { ... }` | `"__alarm__"` handler in `alarmdemo_handle()` |
| Deployment | `npx wrangler deploy` | HTTP multipart deploy API |
| Language | JavaScript/TypeScript | Rust (WASM via cargo + wasm-tools) |
| Distribution | Cloudflare edge network | HTTP/gRPC (multi-cloud) |

## Discord Architecture Parallels

| Discord Pattern | This Example |
|----------------|--------------|
| One Elixir GenServer per guild | One `ChatRoomActor` per room |
| Session process per user | Member tracking in `ChatRoomState.members` |
| Fan-out to connected sessions | `send_message` calls `host::send()` to members |
| ETS for fast member lookups | Rust `HashMap` for O(1) member access |
| Manifold for distributed fan-out | PlexSpaces `host::send()` |
| Supervision trees | `app-config.toml` supervisor |

## Implementation Notes

The three actors share one WASM binary. Actor type routing is done in `init()` via
the `actor_type` field in the config JSON — the framework passes this when starting
each actor instance. The `ActorState` enum (with `#[serde(tag = "actor_type")]`)
safely separates per-instance state for `ChatRoomActor`, `RateLimiterActor`, and
`AlarmDemoActor` within the same WASM module.

State is stored in a `OnceLock<Mutex<Option<ActorState>>>` — the standard pattern
for WASM actors that need a global per-instance state cell.

## Files

| File | Description |
|------|-------------|
| `src/lib.rs` | ChatRoomState + RateLimiterState + AlarmDemoState with WIT guest impl |
| `Cargo.toml` | Package manifest (cdylib + plexspaces-sdk) |
| `.cargo/config.toml` | Shared workspace target dir |
| `app-config.toml` | ApplicationSpec (supervisor + 3 DO actors) |
| `build.sh` | Build WASM component via cargo + wasm-tools |
| `test.sh` | Deploy + test chat + rate limiting + benchmarks |

## Related Examples

- [Go version](../../go/apps/migrating_cloudflare_workers/README.md) — Go WASM implementation
- [Python version](../../python/apps/migrating_cloudflare_workers/README.md) — Python SDK decorators
- [Rust Chat Room](../chat_room/README.md) — Full guild/channel/FSM chat example
- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
