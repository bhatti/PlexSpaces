# Cloudflare Workers - Discord-style Guild Chat Server (TypeScript)

Demonstrates **full feature parity** with Cloudflare Workers Durable Objects
for real-time chat, including durable alarms, atomic KV operations, batch
reads/writes, and real fan-out via actor messaging.

**Real-world use case**: Chat platforms (Discord, Slack), collaborative editors,
multiplayer game lobbies — anywhere you need per-entity stateful coordination
at the edge. Inspired by [Discord's guild process architecture](https://discord.com/blog/how-discord-scaled-elixir-to-5-000-000-concurrent-users)
and the [Cloudflare workers-chat-demo](https://github.com/cloudflare/workers-chat-demo).

## Architecture

```
              ┌──────────────────────────────────┐
              │   ChatRoom (Durable Object)      │
              │                                  │
              │  ┌────────────────────────────┐  │
              │  │  Members      Messages     │  │
              │  │  alice ──┐   [seq1,seq2..] │  │
              │  │  bob   ──┤   (ring buffer) │  │
              │  │  carol ──┘                 │  │
              │  └────────────────────────────┘  │
              │  kvMultiGet/Put: batch history   │
              │  host.send(): real fan-out       │
              └──────────────────────────────────┘
                    │           │           │
              join/leave   send_message  get_history
                    │      (real fan-out)   │
              ┌─────▼───────────▼──────────▼──────┐
              │         Worker Router             │
              └─────────────┬────────┬────────────┘
                            │        │
              ┌─────────────▼──┐  ┌──▼─────────────────┐
              │  RateLimiter   │  │    AlarmDemo        │
              │  Token bucket  │  │  Deferred batch     │
              │  kvIncrement() │  │  alarmSet/alarm()   │
              └────────────────┘  └─────────────────────┘
```

**Chat flow** (like Discord's guild process):
1. Users `join` a room -> tracked in member list
2. `send_message` -> add to history + real `host.send()` fan-out to all members
3. Rate limiter enforces per-user message limits (token bucket + `kvIncrement`)
4. Message history persisted via `kvMultiPut` batch write (like DO `storage.put(map)`)
5. History restored on init via `kvMultiGet` batch read (like DO `storage.get(keys[])`)
6. `AlarmDemoActor`: enqueue items, schedule durable alarm, process batch on fire

## Quick Start

```bash
# Terminal 1: Start PlexSpaces node
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:7992

# Terminal 2: Build and run
cd examples/typescript/apps/migrating_cloudflare_workers
./build.sh        # Builds guild_chat_actor.wasm (TS -> ESM -> WASM component)
./test.sh 7993    # Deploy + test chat + rate limiting + benchmarks
```

## PlexSpaces SDK Features

| Feature | How Used |
|---------|----------|
| `ActorRouter` | Routes to ChatRoom, RateLimiter, or AlarmDemo by actor ID prefix |
| `PlexSpacesActor<T>` | TypeScript actor base with typed state + JSON serialization |
| `host.kvPut()/kvGet()` | Durable message history (like DO `state.storage`) |
| `host.kvMultiGet()` | Batch-fetch history + metadata on init (like DO `storage.get(keys[])`) |
| `host.kvMultiPut()` | Batch-write history + metadata on persist (like DO `storage.put(map)`) |
| `host.kvIncrement()` | Atomic distributed counter for rate limiting (cross-node safe) |
| `host.kvCas()` | Atomic compare-and-swap for distributed state coordination |
| `host.alarmSet()` | Schedule durable alarm (like DO `storage.setAlarm(timestamp)`) |
| `host.alarmGet()` | Read scheduled alarm timestamp (like DO `storage.getAlarm()`) |
| `on__alarm__()` | Alarm handler dispatched by reminder facet (like DO `alarm()`) |
| `host.send()` | Real fan-out to member actors (like DO WebSocket broadcast) |
| `host.nowMs()` | Timestamps for messages + token bucket refill |
| `host.info()` | Structured logging |
| `onInit()` | Initialize + restore persisted state (`blockConcurrencyWhile`) |
| `on<Op>()` | Message handlers dispatched by payload.op |
| `getState()/setState()` | Checkpoint-based state persistence |

## Comparison: Cloudflare Workers/DO vs PlexSpaces

| Feature | Cloudflare Workers/DO | PlexSpaces TypeScript |
|---------|----------------------|----------------------|
| Actor model | `export class ChatRoom extends DurableObject` | `ChatRoomActor extends PlexSpacesActor` |
| Get instance | `env.CHAT_ROOM.get(id)` | `host.ask(actorID, ...)` |
| Durable storage | `this.state.storage.put/get` | `host.kvPut()/kvGet()` + getState |
| Batch KV write | `storage.put(new Map([...]))` | `host.kvMultiPut(entries)` |
| Batch KV read | `storage.get(["k1","k2"])` | `host.kvMultiGet(keys)` |
| Atomic counter | N/A (manual CAS) | `host.kvIncrement(key, delta)` |
| Atomic CAS | N/A (manual retry) | `host.kvCas(key, expected, new)` |
| Request handler | `async fetch(request)` | `on<Op>(payload)` handlers |
| Init/migration | `blockConcurrencyWhile()` | `onInit()` (runs before any handle) |
| Multi-actor | `wrangler.toml [[bindings]]` | `app-config.toml [[children]]` |
| Routing | Worker fetch + URL path | `ActorRouter` prefix matching |
| Fan-out | WebSocket broadcast | `host.send()` to member actors |
| Rate limiting | Separate DO class per IP | RateLimiter actor (kvIncrement) |
| Schedule alarm | `storage.setAlarm(timestamp)` | `host.alarmSet(timestampMs)` |
| Read alarm | `storage.getAlarm()` | `host.alarmGet()` |
| Alarm handler | `async alarm() { ... }` | `on__alarm__(payload)` handler |
| Deployment | `npx wrangler deploy` | HTTP multipart deploy API |
| Language | JavaScript/TypeScript | TypeScript, Go, Python, Rust |
| Distribution | Cloudflare edge network | HTTP/gRPC (multi-cloud) |

## Discord Architecture Parallels

This example mirrors Discord's production architecture:

| Discord Pattern | This Example |
|----------------|--------------|
| One Elixir GenServer per guild | One ChatRoom actor per room |
| Session process per user | Member tracking in ChatRoom |
| Fan-out to connected sessions | `send_message` broadcasts to members |
| ETS for fast member lookups | Object map for O(1) member access |
| Manifold for distributed fan-out | PlexSpaces host.send() |
| Supervision trees | `app-config.toml` supervisor |

## Metrics & Benchmarks

The `test.sh` script runs two in-WASM batch benchmarks with coordination cost analysis:

### Batch Message Benchmark (send_message_batch)

Processes 200+ messages in a single WASM request cycle with 10+ members for fan-out:

| Metric | Description |
|--------|-------------|
| **Messages/sec** | In-WASM throughput (JSON parse + map lookup + history + fan-out) |
| **Compute time** | Pure WASM execution time (message processing) |
| **Coordination time** | HTTP round-trip + WASM instantiation overhead |
| **Granularity ratio** | `compute_time / coordination_time` (target: >= 10x) |

### Batch Rate Limit Benchmark (check_rate_batch)

Processes 5000 rate checks across 50 users in a single WASM call:

| Metric | Description |
|--------|-------------|
| **Ops/sec** | Token bucket checks per second (in-WASM) |
| **Deny rate** | Percentage of requests exceeding rate limit |
| **Coordination cost** | % of wall clock spent on HTTP vs computation |

### Coordination Cost Analysis

Each benchmark reports:
- **Wall clock**: Total time including HTTP coordination
- **Compute (WASM)**: Pure computation inside WASM actor
- **Coordination**: `wall_clock - compute` (HTTP + serialization + dispatch)
- **Granularity**: `compute / coordination` ratio

Higher granularity = more compute per coordination overhead = better efficiency.

## Files

| File | Description |
|------|-------------|
| `guild_chat_actor.ts` | ChatRoom + RateLimiter + AlarmDemo actors (multi-actor router) |
| `package.json` | npm dependencies + build scripts |
| `tsconfig.json` | TypeScript configuration |
| `build-bundle.mjs` | esbuild bundler (TS + SDK -> ESM) |
| `app-config.toml` | ApplicationSpec (supervisor + 3 DO actors) |
| `build.sh` | Build WASM component (TS -> JS -> ESM -> WASM) |
| `test.sh` | Deploy + test chat + rate limiting + alarm + benchmarks |
| `verify.mjs` | In-process Node.js verification (no server) |
| `native/guild_chat.js` | Native Cloudflare Worker/DO reference |
