#!/usr/bin/env python3
"""
ProcessGroups Actor - Simple Actor Interface

Demonstrates using ProcessGroups for pub/sub coordination from WASM actors.
Joins groups and publishes/subscribes to topics.
Uses the simplified string-only WIT interface for componentize-py compatibility.
"""

import json
from wit_world import exports

# Actor state
_group_memberships = []
_messages = []


class Actor(exports.Actor):
    """ProcessGroups actor implementing simple-actor interface."""
    
    def init(self, config_json: str) -> str:
        """Initialize process groups actor."""
        global _group_memberships, _messages
        if config_json:
            try:
                config = json.loads(config_json)
                _group_memberships = config.get("groups", [])
                _messages = config.get("messages", [])
            except Exception as e:
                return f"ERROR: Failed to parse config: {e}"
        else:
            _group_memberships = []
            _messages = []
        return ""
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """
        Handle process groups operations.
        
        Message types:
        - "join_group": Join a process group {"group": "..."}
        - "leave_group": Leave a process group {"group": "..."}
        - "publish": Publish message to group {"group": "...", "topic": "...", "message": {...}}
        - "subscribe": Subscribe to topic {"group": "...", "topic": "..."}
        - "list_groups": List joined groups
        - "get_messages": Get received messages
        """
        global _group_memberships, _messages
        
        try:
            data = json.loads(payload_json) if payload_json else {}
            
            if msg_type == "join_group":
                group = data.get("group", "")
                if not group:
                    return json.dumps({"status": "error", "error": "Group name is required"})
                
                if group not in _group_memberships:
                    _group_memberships.append(group)
                return json.dumps({
                    "status": "ok",
                    "joined": group,
                    "groups": _group_memberships
                })
            
            elif msg_type == "leave_group":
                group = data.get("group", "")
                if group in _group_memberships:
                    _group_memberships.remove(group)
                    return json.dumps({"status": "ok", "left": group})
                else:
                    return json.dumps({"status": "error", "error": f"Not in group: {group}"})
            
            elif msg_type == "publish":
                group = data.get("group", "")
                topic = data.get("topic", "")
                message = data.get("message", {})
                
                # In real implementation, this would call host API
                # For now, simulate by storing locally
                _messages.append({
                    "group": group,
                    "topic": topic,
                    "message": message,
                    "from": from_actor
                })
                return json.dumps({
                    "status": "ok",
                    "published": True,
                    "group": group,
                    "topic": topic
                })
            
            elif msg_type == "subscribe":
                group = data.get("group", "")
                topic = data.get("topic", "")
                # In real implementation, this would call host API
                return json.dumps({
                    "status": "ok",
                    "subscribed": True,
                    "group": group,
                    "topic": topic
                })
            
            elif msg_type == "list_groups":
                return json.dumps({"groups": _group_memberships})
            
            elif msg_type == "get_messages":
                limit = data.get("limit", 100)
                return json.dumps({"messages": _messages[-limit:]})
            
            elif msg_type in ("call", "get_state"):
                return json.dumps({
                    "groups": _group_memberships,
                    "message_count": len(_messages)
                })
            
            else:
                return json.dumps({
                    "status": "unknown_message_type",
                    "msg_type": msg_type
                })
                
        except Exception as e:
            return f"ERROR: {e}"
    
    def get_state(self) -> str:
        """Get process groups actor state as JSON."""
        global _group_memberships, _messages
        return json.dumps({
            "groups": _group_memberships,
            "messages": _messages
        })
    
    def set_state(self, state_json: str) -> str:
        """Restore process groups actor state from JSON."""
        global _group_memberships, _messages
        try:
            state = json.loads(state_json)
            _group_memberships = state.get("groups", [])
            _messages = state.get("messages", [])
            return ""
        except Exception as e:
            return f"ERROR: Failed to restore state: {e}"
