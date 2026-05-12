# PlexSpaces Chat Room Example (TypeScript)

TypeScript WASM port of the Python `chat_room` large-scale chat example.
All nine actor classes are implemented in a single module (`chat_room_actor.ts`).

## Actors

| Actor | File section | Behavior |
|---|---|---|
| `SessionActor` | sessions | Per-client session: connect, send messages, deliver events, inbox |
| `PresenceActor` | sessions | User presence tracking with timer-based expiry |
| `ConnectionFSM` | sessions | Explicit FSM: offline → connected → joined → idle/disconnected |
| `GuildActor` | routing | Server/guild: member registry, channel list, session index |
| `ChannelActor` | routing | Text channel: member join, typing indicators, message posting |
| `MessageStoreActor` | routing | Durable per-channel message storage |
| `FanoutActor` | routing | Broadcasts channel events to all session members via process groups |
| `AuditEventActor` | routing | Append-only audit log with tuplespace writes |
| `ModerationWorkflow` | workflows | Durable moderation review with signal/query support |

## Build

```bash
cd examples/typescript/apps/chat_room
./build.sh
```

The compiled WASM is written to `<repo-root>/target/examples/typescript/chat_room/chat_room_actor.wasm`.

## Configuration

`app-config.toml` declares all nine actor types under the `ts-chat-room-large-scale` namespace with appropriate facets (virtual_actor, durability, timer, reminder, process_group).

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Python chat_room example](../../../python/apps/chat_room/)
