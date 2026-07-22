# WebSocket Chat Room (TypeScript WASM + WsThinClient)

A real-time chat application where browser tabs connect as **thin nodes** via
binary WebSocket frames, join named rooms managed by a WASM actor, and receive
messages through the framework's Process Group broadcast mechanism.

Demonstrates the full round-trip: `WsThinClient (browser)` → binary `WsFrame`
→ PlexSpaces node → `ChatRoomActor` WASM → `host.processGroups.broadcast()` →
`WsActorTransportClient` → `WsRegistry` → thin-node WebSocket session.

## What It Demonstrates

| PlexSpaces Feature | Role |
|---|---|
| `WsThinClient` | Zero-dep TypeScript thin-node SDK for browsers and Node.js |
| Binary `WsFrame` protocol | Hand-rolled protobuf wire encoding (`ws-frame-wire.ts`) |
| Thin-node registration | Browser tab registers as `NODE_ROLE_THIN` with stable `<username>.io` node_id |
| `PlexSpacesActor<State>` | Typed actor base class with `getDefaultState()` / `on*()` handlers |
| `ActorRouter` | Dispatch to `ChatRoomActor` and `PresenceActor` from one WASM |
| `host.send(actorId, ...)` | Per-member fan-out; ActorRegistry routes to thin nodes via WsActorTransportClient |
| `host.sendAfter` | Reminder-based idle timeout in `PresenceActor` |
| `host.kvPutJson` | Per-user presence persistence in KV store |
| Virtual actor facet | Room and presence actors activated on first message |

## Actors

| Class | Behavior | Facets | Role |
|---|---|---|---|
| `ChatRoomActor` | GenServer | `virtual_actor`, `process_group` | Manages PG join/leave; broadcasts `chat_message` to all room members |
| `PresenceActor` | GenServer | `virtual_actor`, `reminder` | Tracks online/offline state; auto-expires after 60s idle |

## Architecture

```
Browser Tab A  (WsThinClient, stable node_id: alice.io)
  │   binary WsFrame tell (protobuf, wire type 2)
  ▼
PlexSpaces Node  /ws  (ws_routes.rs)
  │   WsFrame → ActorRegistry.tell("ChatRoomActor:lobby//…")
  ▼
ChatRoomActor:{room}  (WASM TypeScript)
  │   for each member_actor_id in state.members:
  │     host.send(member_actor_id, "chat_message", {sender, text, ts})
  │     → ActorRegistry routes: server actor = direct mailbox
  │                             thin-node actor = WsActorTransportClient
  ▼
WsActorTransportClient  →  WsRegistry.get_sender(thin_node_id)
  │   WsFrame incoming_tell → Browser Tab B WebSocket session
  ▼
Browser Tab B  WsThinClient.onMessage(actorId, "chat_message", payload)
```

`ChatRoomActor` stores member actor_ids in its durable state.  `host.send()`
is routing-transparent: the ActorRegistry delivers to local actors directly,
and to thin nodes via `WsActorTransportClient → WsRegistry → WS session`.

## Browser UI

Connect panel → join room → WhatsApp-style message bubbles → send/leave.

```
┌─────────────────────────────────────────────────────────────┐
│  PlexSpaces WebSocket Chat                                  │
│  URL: [ws://localhost:8091/ws]  Token:[]  User:[alice]      │
│  Room: [lobby]  Leader Node: [test-node-8091]  [Join][Leave]│
├─────────────────────────────────────────────────────────────┤
│                              alice: Hello!       10:05 ✓✓  │
│  bob: Hey there!             10:06 ✓✓                       │
├─────────────────────────────────────────────────────────────┤
│  [Type a message…                              ] [➤]        │
└─────────────────────────────────────────────────────────────┘
```

## End-to-End Latency Benchmark

`test.sh` measures browser→ChatRoomActor→browser latency over N messages:

```
┌────────────────┬──────────┬──────────┬──────────┬──────────────┐
│   Messages (N) │  avg ms  │  p50 ms  │  p95 ms  │  min / max   │
├────────────────┼──────────┼──────────┼──────────┼──────────────┤
│              5 │       3  │       2  │       8  │    1 / 12    │
└────────────────┴──────────┴──────────┴──────────┴──────────────┘

Routing path: WsThinClient → WS endpoint → ChatRoomActor (WASM)
             → host.processGroups.broadcast() → WsActorTransportClient
             → WsRegistry → thin-node WS session
```

*(Numbers measured on a local single-node PlexSpaces instance.)*

## Build & Run

Prerequisites: PlexSpaces server running on port 8091.

```bash
cd examples/typescript/apps/ws_chat_room

# Run automated integration test (alice + bob), then keep app deployed for browser use:
bash test.sh
```

After `test.sh` completes, open the browser UI at:

**`http://localhost:8091/apps/ts-ws-chat-room/`**

Connect with two browser tabs using different usernames (e.g. `alice` and `bob`).
Each tab uses a stable node ID derived from the username (`alice.io`, `bob.io`) so
virtual actors route consistently across page refreshes.

> Do NOT open `static/index.html` directly from the filesystem — the browser
> blocks WebSocket connections from `file://` origins. Always use the HTTP URL above.

The static files are bundled into the deploy zip (WAR-style). `test.sh` packages
`app.wasm`, `app-config.toml`, and the `static/` directory together and POSTs
them to `/api/v1/applications/deploy`. The node extracts and serves the static
files immediately — no server restart needed.

To undeploy after you're done:
```bash
bash undeploy.sh
```

To use a different port:
```bash
WS_PORT=8093 bash test.sh
```

To force undeploy at the end of the test:
```bash
KEEP_DEPLOYED=0 bash test.sh
```

## Configuration

| Variable | Default | Description |
|---|---|---|
| `WS_PORT` | `8091` | PlexSpaces node WS port |
| `LEADER_NODE_ID` | `test-node-8091` | Node ID where actors are deployed |
| `TIMEOUT` | `60` | Test timeout in seconds |
| `KEEP_DEPLOYED` | `1` | Set to `0` to undeploy after test completes |

## References

- [Architecture — WebSocket thin nodes](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [TypeScript SDK README](../../../../sdks/typescript/README.md)
- [chat_room example](../chat_room/README.md) — HTTP API variant of the same concept
