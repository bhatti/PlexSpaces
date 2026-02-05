"""
Chat Room Actor - Real-time Chat with Process Groups (Python WASM with SDK)

Demonstrates using ProcessGroups for real-time chat coordination.
Users join rooms, send messages, and messages are broadcast to all members.

Real-world use case: Chat applications, live notifications, collaborative editing.

## SDK Features Used

- @actor: Marks class as PlexSpaces actor
- state(): Defines persistent chat room state
- @handler(): Routes chat operations
- host.process_groups: Access to ProcessGroups API for broadcast
"""

from plexspaces import actor, state, handler, init_handler, host


@actor
class ChatRoom:
    """Chat room actor using ProcessGroups for broadcast messaging."""
    
    # Room name and members
    room_name: str = state(default="")
    members: list = state(default_factory=list)
    messages: list = state(default_factory=list)
    
    @init_handler
    def on_init(self, config: dict):
        """Initialize chat room from config."""
        self.room_name = config.get("room_name", "general")
        self.members = config.get("members", [])
        self.messages = []
        host.info(f"Chat room '{self.room_name}' initialized")
    
    @handler("join")
    def join_room(self, user: str = "") -> dict:
        """User joins the chat room."""
        if not user:
            return {"status": "error", "error": "User name is required"}
        
        if user not in self.members:
            self.members.append(user)
            host.info(f"{user} joined #{self.room_name}")
            
            # Notify others via ProcessGroups
            try:
                host.process_groups.join(self.room_name, user)
            except Exception:
                pass  # Host API may not be available
        
        return {
            "status": "ok", 
            "user": user, 
            "room": self.room_name,
            "member_count": len(self.members)
        }
    
    @handler("leave")
    def leave_room(self, user: str = "") -> dict:
        """User leaves the chat room."""
        if user in self.members:
            self.members.remove(user)
            host.info(f"{user} left #{self.room_name}")
            
            try:
                host.process_groups.leave(self.room_name, user)
            except Exception:
                pass
            
            return {"status": "ok", "user": user, "left": self.room_name}
        else:
            return {"status": "error", "error": f"User not in room: {user}"}
    
    @handler("send")
    def send_message(self, user: str = "", text: str = "") -> dict:
        """Send a message to the chat room (broadcast to all members)."""
        if not user:
            return {"status": "error", "error": "User is required"}
        if not text:
            return {"status": "error", "error": "Message text is required"}
        
        msg = {
            "from": user,
            "text": text,
            "room": self.room_name
        }
        self.messages.append(msg)
        
        host.info(f"[{user}] {text}")
        
        # Broadcast to all members via ProcessGroups
        try:
            host.process_groups.publish(self.room_name, msg)
        except Exception:
            pass
        
        return {
            "status": "ok",
            "delivered_to": len(self.members) - 1,  # Exclude sender
            "message_id": len(self.messages) - 1
        }
    
    @handler("members")
    def get_members(self) -> dict:
        """Get list of room members."""
        return {"room": self.room_name, "members": self.members, "count": len(self.members)}
    
    @handler("history")
    def get_history(self, limit: int = 50) -> dict:
        """Get recent chat messages."""
        n = int(limit) if limit is not None else 50
        recent = self.messages[-n:] if n > 0 else self.messages
        return {"room": self.room_name, "messages": recent, "count": len(recent)}
    
    @handler("call", "get_state")
    def get_state_handler(self) -> dict:
        """Get room state for persistence."""
        return {
            "room_name": self.room_name,
            "members": self.members,
            "message_count": len(self.messages)
        }
