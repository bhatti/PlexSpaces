#!/usr/bin/env python3
"""
Event Actor - Simple Actor Interface (GenEvent Behavior)

Demonstrates a GenEvent behavior pattern actor in Python.
Handles event notifications (fire-and-forget).
Uses the simplified string-only WIT interface for componentize-py compatibility.
"""

import json
from wit_world import exports

# Actor state
_events = []
_max_events = 100


class Actor(exports.Actor):
    """Event actor implementing simple-actor interface."""
    
    def init(self, config_json: str) -> str:
        """Initialize event actor with optional config."""
        global _events, _max_events
        if config_json:
            try:
                config = json.loads(config_json)
                _max_events = config.get("max_events", 100)
                _events = config.get("events", [])
            except Exception as e:
                return f"ERROR: Failed to parse config: {e}"
        else:
            _events = []
        return ""
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        """
        Handle event notifications.
        
        Message types:
        - "notify": Add event to list
        - "clear": Clear all events
        - "get_events": Get list of events
        - "get_count": Get event count
        """
        global _events
        
        try:
            data = json.loads(payload_json) if payload_json else {}
            
            if msg_type == "notify":
                event = {
                    "from": from_actor,
                    "type": data.get("type", "unknown"),
                    "data": data.get("data", {}),
                    "timestamp": data.get("timestamp", None)
                }
                _events.append(event)
                # Keep only last N events
                if len(_events) > _max_events:
                    _events = _events[-_max_events:]
                return json.dumps({"status": "ok", "event_count": len(_events)})
            
            elif msg_type == "clear":
                _events.clear()
                return json.dumps({"status": "ok", "cleared": True})
            
            elif msg_type == "get_events":
                limit = data.get("limit", _max_events)
                return json.dumps({"events": _events[-limit:]})
            
            elif msg_type in ("get_count", "call", "get_state"):
                return json.dumps({"event_count": len(_events)})
            
            else:
                return json.dumps({
                    "status": "unknown_message_type",
                    "msg_type": msg_type
                })
                
        except Exception as e:
            return f"ERROR: {e}"
    
    def get_state(self) -> str:
        """Get event actor state as JSON."""
        global _events, _max_events
        return json.dumps({"events": _events, "max_events": _max_events})
    
    def set_state(self, state_json: str) -> str:
        """Restore event actor state from JSON."""
        global _events, _max_events
        try:
            state = json.loads(state_json)
            _events = state.get("events", [])
            _max_events = state.get("max_events", 100)
            return ""
        except Exception as e:
            return f"ERROR: Failed to restore state: {e}"
