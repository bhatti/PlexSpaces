# Chat Room Example (Python WASM with SDK)

Real-time chat application using ProcessGroups for broadcast messaging.

## Overview

This example demonstrates:
- **Process Groups**: Named groups of actors (chat rooms)
- **Pub/Sub**: Broadcast messages to all room members
- **Dynamic membership**: Users join/leave at runtime
- **Message history**: Track conversation history

## Use Cases

- Chat rooms / channels (Slack, Discord)
- Live notifications
- Collaborative editing
- Config update broadcasts

## SDK Features Used

```python
from plexspaces import actor, state, handler, host

@actor
class ChatRoom:
    room_name: str = state(default="")
    members: list = state(default_factory=list)
    messages: list = state(default_factory=list)
    
    @handler("join")
    def join_room(self, user: str = "") -> dict:
        self.members.append(user)
        host.process_groups.join(self.room_name, user)
        return {"status": "ok", "user": user}
    
    @handler("send")
    def send_message(self, user: str = "", text: str = "") -> dict:
        # Broadcast to all members via ProcessGroups
        host.process_groups.publish(self.room_name, {"from": user, "text": text})
        return {"status": "ok", "delivered_to": len(self.members) - 1}
```

## Build

```bash
./build.sh
```

## Test

```bash
# Start PlexSpaces server first
./test.sh 8092
```

## API

| Handler | Description |
|---------|-------------|
| `join` | User joins the chat room |
| `leave` | User leaves the chat room |
| `send` | Send message (broadcast to all) |
| `members` | Get list of room members |
| `history` | Get recent chat messages |

## Message Flow

```
┌─────────┐     ┌─────────────┐     ┌─────────┐
│  Alice  │     │  Chat Room  │     │   Bob   │
└────┬────┘     └──────┬──────┘     └────┬────┘
     │                 │                  │
     │─── join("alice") ──→              │
     │                 │                  │
     │                 │←── join("bob") ──│
     │                 │                  │
     │── send("Hi!") ──→                 │
     │                 │── broadcast ────→│
     │                 │                  │
```

## Related

- [Rust Chat Room Example](../../../rust/embedded/chat_room/)
- [ProcessGroups Documentation](../../../../docs/detailed-design.md#process-groups)
