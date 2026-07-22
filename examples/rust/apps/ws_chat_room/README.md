# ws_chat_room (Rust WASM)

Rust port of [examples/typescript/apps/ws_chat_room](../../../typescript/apps/ws_chat_room).

Two WASM actors that together power a WebSocket-based chat room:

- **ChatRoomActor** — per-room member registry and message fan-out. Stores `actorId → username` in memory, fans out `chat_message` events to all member actor IDs via `host::send`. Messages are routed through `WsActorTransportClient → WsRegistry` to thin-node WebSocket sessions.
- **PresenceActor** — per-user online/offline tracking. Persists state in KV and schedules an idle-timeout check via `host::send_after` (reminder facet).

## Features demonstrated

- Virtual actors with `idle_timeout` (ChatRoomActor: 30 m, PresenceActor: 20 m)
- Process-group fan-out via `host::send` for thin-node WS routing
- Reminder facet via `host::send_after` for idle-timeout expiry
- KV persistence for presence state
- WIT-bindgen WASM component model (same interface as TypeScript example)
- Unit tests with pure logic (no host calls)

## Comparison with TypeScript version

| Feature | TypeScript | Rust |
|---------|-----------|------|
| ChatRoomActor | ✓ | ✓ |
| PresenceActor | ✓ | ✓ |
| Reconnect deduplication | ✓ | ✓ |
| History trimming (50 msgs) | ✓ | ✓ |
| Idle timeout (60 s) | ✓ | ✓ |
| KV presence persistence | ✓ | ✓ |
| WS thin-client routing | ✓ | ✓ |

## Prerequisites

- Rust toolchain with `wasm32-wasip1` target: `rustup target add wasm32-wasip1`
- `wasm-tools`: `cargo install wasm-tools`
- WASI adapter from `@bytecodealliance/jco`: `npm install -g @bytecodealliance/jco`
- A running PlexSpaces node (default port 8095)
- TypeScript SDK built: `cd sdks/typescript && npm run build`

## Build

```bash
bash build.sh
```

Produces `ws_chat_room_actor.wasm` — a WASM component embedding the PlexSpaces actor WIT world.

## Run tests

```bash
bash test.sh           # builds, deploys, runs WebSocket integration test
bash test.sh 8095      # specify port
KEEP_DEPLOYED=1 bash test.sh  # keep deployed after test
```

The test:
1. Builds the WASM component
2. Deploys `rust-ws-chat-room` to the node
3. Opens two WsThinClient sessions (alice, bob) via WebSocket
4. Alice joins the room; bob joins the room
5. Alice sends a message; asserts bob receives it via `chat_message` push
6. Checks `status` returns 2 members
7. Both clients leave and disconnect

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [TypeScript ws_chat_room](../../../typescript/apps/ws_chat_room)
- [Rust chat_room (large-scale)](../chat_room)
