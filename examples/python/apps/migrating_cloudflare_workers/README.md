# Cloudflare Workers - Discord-style Guild Chat Server (Python)

Demonstrates **Cloudflare Workers Durable Objects** pattern for real-time chat using the PlexSpaces Python SDK.

**Real-world use case**: Chat platforms (Discord, Slack), collaborative editors,
multiplayer game lobbies — anywhere you need per-entity stateful coordination
at the edge. Inspired by [Discord's guild process architecture](https://discord.com/blog/how-discord-scaled-elixir-to-5-000-000-concurrent-users)
and the [Cloudflare workers-chat-demo](https://github.com/cloudflare/workers-chat-demo).

See also: [Go version](../../go/apps/migrating_cloudflare_workers/) | [Rust version](../../rust/apps/migrating_cloudflare_workers/)

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
1. Users `join` a room → tracked in member dict
2. `send_message` → add to history + fan-out to all members via `host.send()`
3. Rate limiter enforces per-user message limits (token bucket)
4. Message history persisted via KV (like DO `state.storage.put`)
5. Alarm demo shows durable batch processing pattern

## Quick Start

```bash
# Terminal 1: Start PlexSpaces node
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8094

# Terminal 2: Build and run
cd examples/python/apps/migrating_cloudflare_workers
./build.sh        # Builds guild_chat.wasm (requires plexspaces Python SDK)
./test.sh 8094    # Deploy + test chat + rate limiting + alarms + benchmarks
```

## PlexSpaces Python SDK Features

| Feature | How Used |
|---------|----------|
| `@actor(facets=[...])` | Decorator declares virtual actor + facets (like DO export class) |
| `@handler("op")` | Routes messages by op name (like DO `fetch(request)`) |
| `state(default_factory=...)` | Persistent state fields (like DO `this.state.storage`) |
| `host.send(actorId, ...)` | Fire-and-forget fan-out to member actors |
| `host.kv.put()/host.kv.get()` | Durable storage (like DO `state.storage.put/get`) |
| `host.kv.multi_get()/multi_put()` | Batch KV (like DO `storage.get([k1,k2,...])`) |
| `host.kv.cas(key, old, new)` | Atomic compare-and-swap for idempotent ops |
| `host.kv.increment(key, delta)` | Atomic distributed counter (rate tracking) |
| `host.alarm.set(timestamp_ms)` | Schedule durable alarm (like DO `storage.setAlarm()`) |
| `host.alarm.get()` | Query pending alarm (like DO `storage.getAlarm()`) |
| `host.alarm.delete()` | Cancel alarm (like DO `storage.deleteAlarm()`) |
| `host.now_ms()` | Timestamps for messages + token bucket refill |
| `host.self_id()` | Actor identity for room ID extraction |
| `host.info(msg)` | Structured logging |

## Comparison: Cloudflare Workers/DO vs PlexSpaces Python

| Feature | Cloudflare Workers/DO | PlexSpaces Python |
|---------|----------------------|-------------------|
| Actor class | `export class ChatRoom extends DurableObject` | `@actor(facets=["virtual_actor"])` |
| Get instance | `env.CHAT_ROOM.get(id)` | `host.send(actorID, ...)` |
| Durable storage | `this.state.storage.put/get` | `host.kv.put()/host.kv.get()` |
| Batch storage | `storage.get([k1,k2,...])` / `storage.put({...})` | `host.kv.multi_get()` / `host.kv.multi_put()` |
| Atomic CAS | Transactional storage read-modify-write | `host.kv.cas(key, expected, new)` |
| Atomic counter | KV atomic increment | `host.kv.increment(key, delta)` |
| Request handler | `async fetch(request)` | `@handler("op")` method |
| Init/migration | `blockConcurrencyWhile()` | State restored from `host.kv` in init |
| Multi-actor | `wrangler.toml [[bindings]]` | `app-config.toml [[children]]` |
| Fan-out | WebSocket broadcast | `host.send()` fire-and-forget to member actors |
| Rate limiting | Separate DO class per IP | `RateLimiterActor` with token bucket |
| Set alarm | `this.state.storage.setAlarm(timestamp)` | `host.alarm.set(timestamp_ms)` |
| Get alarm | `this.state.storage.getAlarm()` | `host.alarm.get()` |
| Cancel alarm | `this.state.storage.deleteAlarm()` | `host.alarm.delete()` |
| Alarm callback | `async alarm() { ... }` | `@handler("__alarm__")` method |
| Deployment | `npx wrangler deploy` | HTTP multipart deploy API |
| Language | JavaScript/TypeScript | Python (WASM via componentize-py) |
| Distribution | Cloudflare edge network | HTTP/gRPC (multi-cloud) |

## Discord Architecture Parallels

| Discord Pattern | This Example |
|----------------|--------------|
| One Elixir GenServer per guild | One `ChatRoomActor` per room |
| Session process per user | Member tracking in `ChatRoomActor.members` |
| Fan-out to connected sessions | `send_message` calls `host.send()` to members |
| ETS for fast member lookups | Python dict for O(1) member access |
| Manifold for distributed fan-out | PlexSpaces `host.send()` |
| Supervision trees | `app-config.toml` supervisor |

## Files

| File | Description |
|------|-------------|
| `guild_chat.py` | ChatRoomActor + RateLimiterActor + AlarmDemoActor |
| `app-config.toml` | ApplicationSpec (supervisor + 3 DO actors) |
| `build.sh` | Build WASM module via plexspaces-py SDK CLI |
| `test.sh` | Deploy + test chat + rate limiting + benchmarks |

## Related Examples

- [Go version](../../go/apps/migrating_cloudflare_workers/README.md) — Go WASM implementation
- [Rust version](../../rust/apps/migrating_cloudflare_workers/README.md) — Rust WASM with SDK macros
- [Python WebSocket Chat Room](../ws_chat_room/README.md) — WebSocket-based chat variant
- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
