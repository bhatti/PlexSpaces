# Chat Room (Rust WASM)

Large-scale chat example built from multiple PlexSpaces WASM actors — a faithful Rust port of the Python/Go/TypeScript chat_room examples.

## What it demonstrates

- **Multi-actor WASM binary**: nine actor types (`SessionActor`, `GuildActor`, `ChannelActor`, `PresenceActor`, `MessageStoreActor`, `FanoutActor`, `AuditEventActor`, `ConnectionFSM`, `ModerationWorkflow`) compiled into one WASM component, routed by `actor_type` from the init config.
- **Durable virtual actors**: automatic activation on first message, idle timeout, checkpoint-based durability.
- **Process groups**: channel subscriptions and fan-out delivery using `host::pg_join` / `host::pg_broadcast`.
- **Timers**: typing indicator expiry and presence TTL via `host::send_after`.
- **FSM actors**: `ConnectionFSM` demonstrates explicit state transitions (offline → connected → joined → idle).
- **Workflow actors**: `ModerationWorkflow` demonstrates long-running workflows with signals and queries.
- **Object Registry**: `FanoutActor` and `AuditEventActor` register themselves at startup using proto-encoded WIT registry calls.
- **Metrics**: per-actor application metrics via `host::application_metrics_add`.
- **KV storage**: guild channel list and channel members persisted via `host::kv_put`.

## Actor types

| Actor | Behavior | Key operations |
|-------|----------|----------------|
| `SessionActor` | GenServer, virtual, durable | `connect`, `send_channel_message`, `set_typing`, `inbox` |
| `GuildActor` | GenServer, virtual, durable | `create_channel`, `register_session`, `topology` |
| `ChannelActor` | GenServer, virtual, durable, timer | `join_member`, `mark_typing`, `post_message`, `history`, `status` |
| `PresenceActor` | GenServer, virtual, durable, reminder | `set_presence`, `expire_presence`, `status` |
| `MessageStoreActor` | GenServer, virtual, durable | `append_message`, `history` |
| `FanoutActor` | GenServer, virtual, singleton | `deliver_channel_event`, `stats` |
| `AuditEventActor` | GenEvent, virtual, singleton, durable | `record_event`, `stats` |
| `ConnectionFSM` | GenFSM, virtual, durable | `transition`, `status` |
| `ModerationWorkflow` | Workflow, virtual, durable | `workflow_run`, `workflow_signal:*`, `workflow_query:*` |

## Running

```bash
# Build WASM component
./build.sh

# Run full E2E test against a live node on port 8091
./test.sh 8091
```

Prerequisites: `wasm-tools`, `cargo` with `wasm32-wasip1` target, `@bytecodealliance/jco` (for WASI adapter).

## See also

- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
- [Go chat_room](../../../go/apps/chat_room/README.md)
- [TypeScript chat_room](../../../typescript/apps/chat_room/README.md)
- [SDK Reference](../../../../sdks/rust/plexspaces-sdk/README.md)
