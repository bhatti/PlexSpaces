#!/usr/bin/env python3
"""
Python ProcessGroups Actor Example

Demonstrates using ProcessGroups for pub/sub coordination from WASM actors.
This actor joins groups and publishes/subscribes to topics.
"""

# Actor state
_group_memberships = []

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with state."""
    global _group_memberships
    try:
        # In real implementation, this would call host.process_groups.join_group()
        # For now, this is a placeholder demonstrating the pattern
        return None, None
    except Exception as e:
        return None, f"Init failed: {str(e)}"

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle messages using ProcessGroups."""
    global _group_memberships
    
    try:
        if message_type == "join_group":
            # Join a process group
            # Note: In real implementation, this would call host.process_groups.join_group()
            group_name = payload.decode('utf-8')
            _group_memberships.append(group_name)
            return b"joined", None
            
        elif message_type == "publish":
            # Publish message to group
            # Note: In real implementation, this would call host.process_groups.publish_to_group()
            parts = payload.split(b':', 2)
            if len(parts) >= 2:
                group_name = parts[0].decode('utf-8')
                topic = parts[1].decode('utf-8') if len(parts) > 1 else None
                message = parts[2] if len(parts) > 2 else b''
                return b"published", None
            else:
                return b"", "Invalid publish format: group:topic:message"
                
        elif message_type == "leave_group":
            # Leave a process group
            group_name = payload.decode('utf-8')
            if group_name in _group_memberships:
                _group_memberships.remove(group_name)
                return b"left", None
            else:
                return b"", f"Not in group: {group_name}"
                
        else:
            return b"", f"Unknown message type: {message_type}"
    except Exception as e:
        return b"", f"Handle message failed: {str(e)}"

def handle_event(from_actor: str, message_type: str, payload: bytes) -> tuple[None, str | None]:
    """Handle asynchronous event (GenEvent pattern)."""
    # ProcessGroups messages are typically fire-and-forget
    handle_message(from_actor, message_type, payload)
    return None, None






