# WebSocket Chat Room (Go WASM)

A Go port of the [TypeScript ws_chat_room](../../typescript/apps/ws_chat_room/README.md)
example. Two actors manage a real-time chat room and per-user presence, compiled to
WASM with TinyGo and deployed to a PlexSpaces node.

## What It Demonstrates

| PlexSpaces Feature | Role |
|---|---|
| `host.Send()` | Routing-transparent fan-out to member actors; ActorRegistry routes to thin nodes via WsActorTransportClient |
| `host.KVPutWithTTL()` | Per-user presence persistence with 2-hour TTL — entries expire automatically |
| `host.KVCAS()` | Idempotent join check: first write wins, reconnects clean up stale entries |
| `host.AlarmSet()` / `host.AlarmGet()` | Periodic room stats flush in ChatRoomActor (5 min interval) |
| `host.SendAfter()` | Reminder-based idle timeout in PresenceActor (60s after going online) |
| Virtual actor facet | ChatRoomActor and PresenceActor activated on first message, idle-deactivated |
| process_group facet | ChatRoomActor joins a process group for multi-room discovery |
| reminder facet | PresenceActor uses reminder facet to power the SendAfter idle check |
| `plexspaces.ActorRouter` | Multi-actor routing — one WASM module hosts both actor types |
| `plexspaces.BaseActor` | JSON state serialization via `GetState()` / `SetState()` |

## Actors

| Struct | Behavior | Facets | Role |
|---|---|---|---|
| `ChatRoomActor` | GenServer | `virtual_actor`, `process_group` | Per-room member registry; fans out `chat_message`, `member_joined`, `member_left` |
| `PresenceActor` | GenServer | `virtual_actor`, `reminder` | Per-user online/offline state with KV persistence and idle timeout |

## Architecture

```
HTTP Client (curl / thin-node)
  │  POST /api/v1/actors/go-ws-chat-room/ChatRoomActor:{room}/ask
  ▼
PlexSpaces Node
  │  dispatch → ChatRoomActor:{room}  (WASM Go)
  ▼
ChatRoomActor
  │  for each member_actor_id:
  │    host.Send(member_actor_id, "chat_message", {...})
  │    → ActorRegistry routes:
  │      server actor   → direct mailbox
  │      thin-node actor → WsActorTransportClient → WsRegistry → WS session
  ▼
Member Actor (thin-node client / another server actor)
```

## Comparison to Cloudflare Durable Objects

| Cloudflare Workers/DO | PlexSpaces Go |
|---|---|
| `export class ChatRoom extends DurableObject` | `ChatRoomActor struct + BaseActor` |
| WebSocket `accept()` / `server.send()` | `host.Send(actorID, "chat_message", event)` |
| `this.state.storage.put()` | `GetState()` / `SetState()` + `host.KVPutWithTTL()` |
| `this.state.storage.setAlarm(ts)` | `host.AlarmSet(ts)` |
| `this.state.storage.getAlarm()` | `host.AlarmGet()` |
| `alarm()` callback | `"__alarm__"` message handler |
| `this.ctx.waitUntil(delay)` | `host.SendAfter(delayMs, ...)` |

## Message Handlers

### ChatRoomActor

| Handler | Payload | Response |
|---|---|---|
| `join` | `{actor_id, username}` | `{success, members, member_info, room_id, history}` |
| `leave` | `{actor_id}` | `{success}` |
| `send` | `{sender_actor_id, text}` | `{success, members_notified}` |
| `members` | `{}` | `{members, usernames, room_id}` |
| `status` | `{}` | `{room_id, member_count, history_size, msg_seq, alarm_at_ms}` |
| `__alarm__` | `{}` | `{status, action, member_count, history_size, msg_seq}` |

### PresenceActor

| Handler | Payload | Response |
|---|---|---|
| `online` | `{}` | `{success, online, user_id}` |
| `offline` | `{}` | `{success, online, user_id}` |
| `timeout_check` | `{}` | `{checked, idle_ms, online, user_id}` |
| `status` | `{}` | `{user_id, online, last_seen}` |

## Build and Run

Prerequisites: TinyGo, wasm-tools, and a PlexSpaces node running on port 8093.

```bash
cd examples/go/apps/ws_chat_room

# Build the WASM actor
bash build.sh

# Run automated integration test (join, send, leave, presence)
bash test.sh

# Use a different port
HTTP_PORT=8091 bash test.sh

# Keep the app deployed after the test
KEEP_DEPLOYED=1 bash test.sh
```

To undeploy:
```bash
bash undeploy.sh
```

## Build Dependencies

- TinyGo 0.33+: `brew install tinygo` (macOS)
- wasm-tools: `cargo install wasm-tools`
- wasm-opt (binaryen): `brew install binaryen` (macOS) or bundled with jco
- WASI adapter: install jco globally `npm install -g @bytecodealliance/jco`

## Configuration

| Variable | Default | Description |
|---|---|---|
| `HTTP_PORT` | `8093` | PlexSpaces node HTTP port |
| `APP_ID` | `go-ws-chat-room` | Application ID for deploy/undeploy |
| `TIMEOUT` | `60` | Test timeout in seconds |
| `KEEP_DEPLOYED` | `0` | Set to `1` to skip undeploy after test |

## References

- [Architecture — WebSocket thin nodes](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Go SDK README](../../../../sdks/go/README.md)
- [TypeScript ws_chat_room](../../typescript/apps/ws_chat_room/README.md) — TypeScript variant of the same concept
- [migrating_cloudflare_workers](../migrating_cloudflare_workers/README.md) — Go WASM pattern reference
