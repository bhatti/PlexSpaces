#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Chat Room Example - ProcessGroups Demo
#
# A real-time chat room using PlexSpaces ProcessGroups for pub/sub messaging.
# This demonstrates the SDK's minimal boilerplate approach.
#
# Build:
#     plexspaces-py build chat_room.py -o chat_room_actor.wasm
#
# Real-world use cases:
#   - Chat applications (Slack, Discord)
#   - Live notifications
#   - Collaborative editing
#   - Config update broadcasts

"""
Chat Room Actor - ProcessGroups Demo

Demonstrates:
- @actor decorator for minimal boilerplate
- state() for persistent room state
- @handler() for message routing
- host.process_groups for pub/sub
"""

from plexspaces import actor, state, handler, host


@actor
class ChatRoom:
    """
    A chat room using ProcessGroups for real-time messaging.
    
    Features:
    - Users can join/leave the room
    - Messages are broadcast to all members
    - Message history is persisted
    """
    
    # Persistent state fields (auto-serialized)
    room_name: str = state(default="general")
    members: list = state(default_factory=list)
    messages: list = state(default_factory=list)
    
    @handler("join")
    def join(self, user_id: str) -> dict:
        """
        User joins the chat room.
        
        Args:
            user_id: ID of the user joining
        
        Returns:
            Status and current member count
        """
        if user_id in self.members:
            return {"status": "already_joined", "members": len(self.members)}
        
        # Join the ProcessGroup for pub/sub
        host.process_groups.join(self.room_name, user_id)
        self.members.append(user_id)
        
        host.info(f"User {user_id} joined room {self.room_name}")
        
        # Broadcast join notification
        host.process_groups.publish(self.room_name, {
            "type": "user_joined",
            "user_id": user_id
        })
        
        return {
            "status": "joined",
            "room": self.room_name,
            "members": len(self.members)
        }
    
    @handler("leave")
    def leave(self, user_id: str) -> dict:
        """
        User leaves the chat room.
        
        Args:
            user_id: ID of the user leaving
        
        Returns:
            Status and remaining member count
        """
        if user_id not in self.members:
            return {"status": "not_in_room"}
        
        # Leave the ProcessGroup
        host.process_groups.leave(self.room_name, user_id)
        self.members.remove(user_id)
        
        host.info(f"User {user_id} left room {self.room_name}")
        
        # Broadcast leave notification
        host.process_groups.publish(self.room_name, {
            "type": "user_left",
            "user_id": user_id
        })
        
        return {
            "status": "left",
            "room": self.room_name,
            "members": len(self.members)
        }
    
    @handler("send", "message")
    def send_message(self, from_user: str, text: str) -> dict:
        """
        Send a message to the chat room.
        
        Args:
            from_user: User ID of the sender
            text: Message text
        
        Returns:
            Status and recipient count
        """
        if from_user not in self.members:
            return {"error": "not_in_room"}
        
        # Create message record
        message = {
            "from": from_user,
            "text": text,
            "timestamp": host.now_ms()
        }
        
        # Store in history (keep last 100)
        self.messages.append(message)
        if len(self.messages) > 100:
            self.messages = self.messages[-100:]
        
        # Broadcast to all members
        recipients = host.process_groups.publish(self.room_name, {
            "type": "chat_message",
            **message
        })
        
        host.debug(f"Message from {from_user} broadcast to {len(recipients)} recipients")
        
        return {
            "status": "sent",
            "recipients": len(recipients)
        }
    
    @handler("history")
    def get_history(self, limit: int = 10) -> dict:
        """
        Get recent message history.
        
        Args:
            limit: Maximum number of messages to return (default: 10)
        
        Returns:
            Recent messages
        """
        return {
            "messages": self.messages[-limit:],
            "total": len(self.messages)
        }
    
    @handler("members", "list")
    def list_members(self) -> dict:
        """
        List current room members.
        
        Returns:
            List of member IDs
        """
        return {
            "room": self.room_name,
            "members": self.members,
            "count": len(self.members)
        }
    
    @handler("info", "get_state", "call")
    def get_info(self) -> dict:
        """
        Get room information.
        
        Returns:
            Room state summary
        """
        return {
            "room": self.room_name,
            "member_count": len(self.members),
            "message_count": len(self.messages)
        }
