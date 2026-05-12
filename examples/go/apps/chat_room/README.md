# Chat Room (Go WASM)

A large-scale chat example implemented as a Go WASM actor application.  This is
a faithful port of the [Python chat_room](../../../python/apps/chat_room/) example,
demonstrating the same high-level patterns using the Go SDK.

## What it demonstrates

- **Durable virtual actors** — `SessionActor`, `GuildActor`, `ChannelActor`,
  `MessageStoreActor`, `PresenceActor` persist state across restarts via the
  durability facet.
- **Process groups** — `SessionActor` joins per-user and per-channel groups;
  `FanoutActor` broadcasts messages to all members of a channel group.
- **Timer-based expiry** — `PresenceActor` uses `SendAfter` to expire presence
  entries; `ChannelActor` uses timers to clear typing indicators.
- **FSM actor** — `ConnectionFSM` models the session lifecycle with explicit
  state transitions (offline → connected → joined → idle / disconnected).
- **Workflow actor** — `ModerationWorkflow` is a durable workflow that can be
  started, signalled, and queried independently of normal message handling.
- **Shared services** — KV store, TupleSpace (audit log), application metrics,
  and the object registry are all exercised.

## Actors

| Actor | Behavior | Description |
|-------|----------|-------------|
| `SessionActor` | GenServer | One per connected client session |
| `PresenceActor` | GenServer | Per-user online/offline status with TTL expiry |
| `ConnectionFSM` | GenFSM | Session lifecycle state machine |
| `GuildActor` | GenServer | Guild/server membership and channel index |
| `ChannelActor` | GenServer | Text channel: members, typing, message routing |
| `MessageStoreActor` | GenServer | Durable per-channel message storage |
| `FanoutActor` | GenServer | Broadcasts channel events to process groups |
| `AuditEventActor` | GenEvent | Append-only audit event log |
| `ModerationWorkflow` | Workflow | Durable moderation review flow |

## Build

```bash
./build.sh
```

Output: `<repo-root>/target/examples/go/chat_room/chat_room_actor.wasm`

## References

- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
- [Python source](../../../python/apps/chat_room/)
