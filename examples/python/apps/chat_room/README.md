# Large-Scale Chat App (Python WASM)

This example shows how to grow a tiny chat room into the shape of a
Discord/WhatsApp-style system using PlexSpaces actors and built-in
abstractions. Instead of putting routing, storage, fan-out, and lifecycle in
one class, the app composes focused actors that communicate only by messages.

It stays intentionally small enough to read, but it demonstrates the same
patterns that large actor-based chat systems rely on:

- session actors for connected clients
- guild and channel routers
- dedicated fan-out offload
- durable per-channel message stores
- presence with expiry timers
- explicit connection lifecycle FSM
- durable moderation workflow
- append-only audit events and tuple-space traces

## Actors

| File | Actor | Behavior | Demonstrates |
|------|-------|----------|--------------|
| `sessions.py` | `SessionActor` | GenServer | Connected client session, channel joins, delivery inbox |
| `sessions.py` | `PresenceActor` | GenServer | Presence state, reminder-style expiry, KV + TupleSpace |
| `sessions.py` | `ConnectionFSM` | GenStateMachine | Explicit session lifecycle |
| `routing.py` | `GuildActor` | GenServer | Guild topology, membership, channel catalog |
| `routing.py` | `ChannelActor` | GenServer | Message routing, typing indicators, fan-out delegation |
| `routing.py` | `MessageStoreActor` | GenServer | Durable per-channel history, KV + TupleSpace indexing |
| `routing.py` | `FanoutActor` | GenServer | Object Registry + process-group broadcast offload |
| `routing.py` | `AuditEventActor` | GenEvent | Object Registry + fire-and-forget audit stream |
| `workflows.py` | `ModerationWorkflow` | Workflow | Durable report/review lifecycle |

## Architecture

```mermaid
graph TB
    ClientA["Alice Client"] --> SessionA["SessionActor<br/>alice-mobile"]
    ClientB["Bob Client"] --> SessionB["SessionActor<br/>bob-desktop"]

    subgraph Edge["Client Edge"]
        SessionA
        SessionB
        FSM["ConnectionFSM<br/>per session"]
        Presence["PresenceActor<br/>per user"]
    end

    subgraph Routing["Chat Routing"]
        Guild["GuildActor<br/>per guild"]
        Channel["ChannelActor<br/>per channel"]
        Fanout["FanoutActor<br/>broadcast relay"]
    end

    subgraph Data["Durable Data"]
        Store["MessageStoreActor<br/>per channel"]
        WF["ModerationWorkflow<br/>per report"]
        Audit["AuditEventActor<br/>append-only stream"]
    end

    SessionA --> Guild
    SessionA --> Channel
    SessionA --> FSM
    SessionA --> Presence
    SessionB --> Guild
    SessionB --> Channel
    SessionB --> FSM
    SessionB --> Presence

    Channel --> Store
    Channel --> Fanout
    Channel --> Audit
    Audit -.-> TS["TupleSpace"]
    Store -.-> KV["KV"]
    Store -.-> TS
    Presence -.-> KV
    WF -.-> KV

    Fanout --> SessionA
    Fanout --> SessionB
    Channel --> WF

    style ClientA fill:#C62828,color:#FFFFFF,stroke:#7F0000,stroke-width:2px
    style ClientB fill:#EF6C00,color:#FFFFFF,stroke:#A84300,stroke-width:2px
    style SessionA fill:#1565C0,color:#FFFFFF,stroke:#0D47A1,stroke-width:2px
    style SessionB fill:#1565C0,color:#FFFFFF,stroke:#0D47A1,stroke-width:2px
    style FSM fill:#6A1B9A,color:#FFFFFF,stroke:#4A148C,stroke-width:2px
    style Presence fill:#00838F,color:#FFFFFF,stroke:#005662,stroke-width:2px
    style Guild fill:#2E7D32,color:#FFFFFF,stroke:#1B5E20,stroke-width:2px
    style Channel fill:#388E3C,color:#FFFFFF,stroke:#1B5E20,stroke-width:2px
    style Fanout fill:#F9A825,color:#111111,stroke:#F57F17,stroke-width:2px
    style Store fill:#283593,color:#FFFFFF,stroke:#1A237E,stroke-width:2px
    style WF fill:#AD1457,color:#FFFFFF,stroke:#880E4F,stroke-width:2px
    style Audit fill:#4E342E,color:#FFFFFF,stroke:#3E2723,stroke-width:2px
    style KV fill:#455A64,color:#FFFFFF,stroke:#263238,stroke-width:2px
    style TS fill:#37474F,color:#FFFFFF,stroke:#263238,stroke-width:2px
    style Edge fill:#E3F2FD,stroke:#1565C0,stroke-width:2px
    style Routing fill:#E8F5E9,stroke:#2E7D32,stroke-width:2px
    style Data fill:#FFF3E0,stroke:#EF6C00,stroke-width:2px
```

## Message Path

```mermaid
sequenceDiagram
    participant A as Alice SessionActor
    participant C as ChannelActor
    participant S as MessageStoreActor
    participant F as FanoutActor
    participant B as Bob SessionActor

    A->>C: send_channel_message("general", "hello")
    C->>S: append_message(...)
    S-->>C: message_id + stored_at
    C->>F: deliver_channel_event(...)
    F->>A: deliver_channel_event
    F->>B: deliver_channel_event
    C->>A: ack(message_id)
```

## Abstractions In Play

- **Virtual actors** let us address `guild:guild-acme`, `channel:guild-acme__general`, or `session:alice-mobile` directly without manual process management.
- **Object Registry** is used for singleton service discovery: `FanoutActor` and `AuditEventActor` register on init with `host.registry.register()`, and `helpers.singleton_actor_id()` discovers them via `host.registry.discover(object_category=role)`. This replaces the older `pg_first("svc:{role}")` pattern.
- **Process groups** are still used for channel fan-out broadcast: session actors join per-channel groups so `FanoutActor` can broadcast a message to all subscribers in one call.
- **Durability** keeps guild, channel, store, session, and workflow state recoverable after reactivation.
- **KV + TupleSpace** give the example both durable lookups and searchable coordination traces.
- **Timers/reminders** make typing indicators and presence expiry deterministic and actor-owned.
- **Workflow + FSM** show that long-running moderation and short-lived connection lifecycle are both first-class, but modeled differently.

## Usage

```bash
./build.sh
./test.sh 8091
```

`test.sh` deploys the app with `app-config.toml`, connects two sessions, sends a
message through the channel router, verifies history and fan-out, checks typing
expiry and presence expiry, and exercises the moderation workflow.

## Project structure

```text
chat_room/
├── chat_room_actor.py  # entrypoint + ACTOR_ROLES
├── sessions.py         # SessionActor, PresenceActor, ConnectionFSM
├── routing.py          # GuildActor, ChannelActor, MessageStoreActor, FanoutActor, AuditEventActor
├── workflows.py        # ModerationWorkflow
├── helpers.py          # shared naming/discovery/metrics helpers
├── app-config.toml     # supervisor tree + facets
├── verify.py           # native deterministic verification
├── build.sh
└── test.sh
```

## Why this example matters

The point is not that a chat app must have exactly these actors. The point is
that actor boundaries stay stable as the system grows. The same design that
starts as one channel with two sessions can scale into many guilds, many
channels, many sessions, and many background workflows without falling back to
shared mutable state.

## Related docs

- [Actor System](../../../../docs/actor-system.md)
- [Architecture](../../../../docs/architecture.md)
- [Detailed Design](../../../../docs/detailed-design.md)
- [Python SDK](../../../../sdks/python/README.md)
