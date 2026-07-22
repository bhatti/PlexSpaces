# Python WebSocket Chat Room

A Python port of the TypeScript `ws_chat_room` example demonstrating real-time
chat using PlexSpaces virtual actors with WebSocket thin-node delivery.

## Overview

Two actors collaborate to power a multi-user chat room:

| Actor | Responsibility |
|---|---|
| `ChatRoomActor` | Per-room member registry; fans out `chat_message` events to all member actor IDs |
| `PresenceActor` | Per-user online/offline tracking with 60-second idle timeout via reminder facet |

Messages are routed to browser thin-node clients via
`host.send(actorId, ...)` — the ActorRegistry routes each send through
`WsActorTransportClient` and `WsRegistry` to the correct WebSocket session.

## Python SDK Actor Pattern

Actors are defined with the `@actor` decorator and handlers with `@handler`.
State fields are declared at class level using `state()`.

```python
from plexspaces import actor, handler, host, state

@actor(facets=["virtual_actor", "process_group"])
class ChatRoomActor:
    members: dict = state(default_factory=dict)   # actorId -> username
    history: list = state(default_factory=list)
    msg_seq: int = state(default=0)

    @handler("join")
    def join(self, actor_id: str = "", username: str = "") -> dict:
        self.members[actor_id] = username
        for mid in self.members:
            host.send(mid, "member_joined", {"username": username})
        return {"success": True, "members": list(self.members)}

    @handler("send")
    def send(self, sender_actor_id: str = "", text: str = "") -> dict:
        event = {"sender": sender_actor_id, "text": text, "ts": host.now_ms()}
        for mid in self.members:
            host.send(mid, "chat_message", event)
        return {"success": True}
```

## Comparison with Cloudflare Durable Objects

| Feature | PlexSpaces | Cloudflare DO |
|---|---|---|
| Per-room state | Virtual actor (lazy activation) | Durable Object |
| Fan-out delivery | `host.send()` via ActorRegistry | WebSocket broadcast in DO |
| Idle eviction | `idle_timeout = "30m"` facet config | DO hibernation API |
| Presence tracking | `PresenceActor` + reminder facet | Custom timer / Alarm API |
| Multi-node routing | Transparent via WsActorTransportClient | Single-region by default |
| Persistence | Durability facet (optional) | Durable storage (built-in) |
| Language | Python (WASM via componentize-py) | JavaScript / TypeScript |

## Features Demonstrated

- `@actor` / `@handler` decorator pattern (no boilerplate class hierarchy)
- `state()` fields with `default` and `default_factory`
- `host.send()` for fan-out to thin-node WebSocket clients
- `host.now_ms()` for message timestamps
- `host.send_after()` for idle-timeout scheduling (PresenceActor)
- `host.kv_put_json()` for presence key-value persistence
- `virtual_actor` facet for lazy activation and automatic eviction
- `process_group` facet for cluster-wide member broadcast
- `reminder` facet for durable scheduled callbacks
- Bounded history ring buffer (last 50 messages)
- Reconnect handling (stale actor ID eviction on username collision)

## Build

```bash
cd examples/python/apps/ws_chat_room
bash build.sh
```

Requires:
- Python 3.11+
- PlexSpaces Python SDK (`sdks/python`)
- `componentize-py` (installed via SDK)

## Run

Start a PlexSpaces node (default port 8094 for this example):

```bash
./scripts/server.sh --port 8094
```

Then deploy and test:

```bash
bash test.sh 8094
```

Or deploy manually:

```bash
zip -j app.zip ws_chat_actor.wasm app-config.toml
curl -X POST http://localhost:8094/api/v1/applications/deploy \
  -F application_id=py-ws-chat-room \
  -F name=py-ws-chat-room \
  -F version=1.0.0 \
  -F "app_file=@app.zip"
```

## API

All operations go through the HTTP ask API:

```
POST /api/v1/actors/{app_id}/ChatRoomActor:{roomId}/ask
```

### join

```json
{ "actor_id": "alice//ChatClient::ns@node", "username": "alice" }
```

Returns: `{ "success": true, "members": [...], "member_info": {...}, "history": [...] }`

### leave

```json
{ "actor_id": "alice//ChatClient::ns@node" }
```

### send

```json
{ "sender_actor_id": "alice//ChatClient::ns@node", "text": "Hello!" }
```

### members

```json
{ "op": "members" }
```

Returns: `{ "members": [...], "usernames": {...}, "room_id": "..." }`

### status

```json
{ "op": "status" }
```

Returns: `{ "room_id": "...", "member_count": 2, "messages": 5 }`

## Related

- TypeScript version: [`examples/typescript/apps/ws_chat_room`](../../typescript/apps/ws_chat_room/)
- Large-scale chat: [`examples/python/apps/chat_room`](../chat_room/)
- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
