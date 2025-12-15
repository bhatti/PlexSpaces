#!/usr/bin/env python3
"""
Python Event Actor Example (GenEvent Behavior)

Demonstrates a GenEvent behavior pattern actor in Python.
This actor handles event notifications (fire-and-forget) using handle_event().
"""

# Actor state
_events = []
_max_events = 100

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with state."""
    global _events
    try:
        if initial_state:
            # Deserialize events from state
            import struct
            count = struct.unpack('<I', initial_state[:4])[0]
            _events = list(initial_state[4:4+count])
        else:
            _events = []
        return None, None
    except Exception as e:
        return None, f"Init failed: {str(e)}"

def handle_event(from_actor: str, message_type: str, payload: bytes) -> tuple[None, str | None]:
    """Handle event notification (GenEvent pattern - no reply needed)."""
    global _events
    
    try:
        if message_type == "notify":
            # Add event to list
            event_data = payload.decode('utf-8', errors='ignore')
            _events.append(event_data)
            # Keep only last N events
            if len(_events) > _max_events:
                _events = _events[-_max_events:]
            return None, None  # No response for events
        elif message_type == "clear":
            _events.clear()
            return None, None
        else:
            return None, f"Unknown message type: {message_type}"
    except Exception as e:
        return None, f"Handle event failed: {str(e)}"

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Fallback generic handler (for backward compatibility)."""
    # For GenEvent, delegate to handle_event (but return empty response)
    result, err = handle_event(from_actor, message_type, payload)
    if err:
        return b"", err
    return b"", None  # GenEvent doesn't return responses

def snapshot_state() -> tuple[bytes, str | None]:
    """Snapshot actor state for persistence."""
    global _events
    try:
        import struct
        count = len(_events)
        state = struct.pack('<I', count)
        state += b''.join(e.encode('utf-8') for e in _events)
        return state, None
    except Exception as e:
        return b"", f"Snapshot failed: {str(e)}"

def shutdown() -> tuple[None, str | None]:
    """Graceful shutdown."""
    global _events
    _events.clear()
    return None, None
