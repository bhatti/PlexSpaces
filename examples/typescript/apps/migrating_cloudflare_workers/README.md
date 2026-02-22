# Cloudflare Workers - Discord-style Guild Chat Server (TypeScript)

Demonstrates **Cloudflare Workers Durable Objects** pattern for real-time chat.

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
              └──────────────────────────────────┘
                    │           │           │
              join/leave   send_message  get_history
                    │        (fan-out)      │
              ┌─────▼───────────▼──────────▼──────┐
              │         Worker Router             │
              └───────────────┬───────────────────┘
                              │
              ┌───────────────▼───────────────────┐
              │   RateLimiter (Durable Object)    │
              │   Per-user token bucket           │
              │   user-1: [5 tokens, refill@1/s]  │
              │   user-2: [3 tokens, refill@1/s]  │
              └───────────────────────────────────┘
```

**Chat flow** (like Discord's guild process):
1. Users `join` a room -> tracked in member list
2. `send_message` -> add to history + fan-out to all members
3. Rate limiter enforces per-user message limits (token bucket)
4. Message history persisted via KV (like DO `state.storage.put`)

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
| `ActorRouter` | Routes to ChatRoom or RateLimiter by actor ID prefix |
| `PlexSpacesActor<T>` | TypeScript actor base with typed state + JSON serialization |
| `host.kvPut()/kvGet()` | Durable message history (like DO `state.storage`) |
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
| Request handler | `async fetch(request)` | `on<Op>(payload)` handlers |
| Init/migration | `blockConcurrencyWhile()` | `onInit()` (runs before any handle) |
| Multi-actor | `wrangler.toml [[bindings]]` | `app-config.toml [[children]]` |
| Routing | Worker fetch + URL path | `ActorRouter` prefix matching |
| Fan-out | WebSocket broadcast | `host.send()` to member actors |
| Rate limiting | Separate DO class per IP | RateLimiter actor per user |
| Timer/alarm | `this.state.storage.setAlarm()` | `host.sendAfter()` |
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

Processes 200+ messages in a single WASM invocation with 10+ members for fan-out:

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
| `guild_chat_actor.ts` | ChatRoom + RateLimiter actors (multi-actor router) |
| `package.json` | npm dependencies + build scripts |
| `tsconfig.json` | TypeScript configuration |
| `build-bundle.mjs` | esbuild bundler (TS + SDK -> ESM) |
| `app-config.toml` | ApplicationSpec (supervisor + 2 DO actors) |
| `build.sh` | Build WASM component (TS -> JS -> ESM -> WASM) |
| `test.sh` | Deploy + test chat + rate limiting + benchmarks |
| `verify.mjs` | In-process Node.js verification (no server) |
| `native/guild_chat.js` | Native Cloudflare Worker/DO reference |
