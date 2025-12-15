#!/usr/bin/env python3
"""
Python FSM Actor Example (GenFSM Behavior)

Demonstrates a GenFSM behavior pattern actor in Python.
This actor implements a simple state machine (Idle -> Processing -> Done) using handle_transition().
"""

# Actor state
_state = "idle"  # idle, processing, done
_data = None

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with state."""
    global _state, _data
    try:
        if initial_state:
            _state = initial_state.decode('utf-8', errors='ignore')
        else:
            _state = "idle"
        _data = None
        return None, None
    except Exception as e:
        return None, f"Init failed: {str(e)}"

def handle_transition(from_actor: str, message_type: str, payload: bytes) -> tuple[str, str | None]:
    """Handle state transition (GenFSM pattern - returns new state name)."""
    global _state, _data
    
    try:
        if message_type == "start":
            if _state == "idle":
                _state = "processing"
                _data = payload.decode('utf-8', errors='ignore')
                return "processing", None
            else:
                return "", f"Invalid state transition: {_state} -> processing"
        elif message_type == "complete":
            if _state == "processing":
                _state = "done"
                return "done", None
            else:
                return "", f"Invalid state transition: {_state} -> done"
        elif message_type == "reset":
            _state = "idle"
            _data = None
            return "idle", None
        elif message_type == "get_state":
            # Return current state (no transition)
            return _state, None
        else:
            return "", f"Unknown message type: {message_type} (current state: {_state})"
    except Exception as e:
        return "", f"Handle transition failed: {str(e)}"

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Fallback generic handler (for backward compatibility)."""
    # For GenFSM, delegate to handle_transition
    new_state, err = handle_transition(from_actor, message_type, payload)
    if err:
        return b"", err
    return new_state.encode('utf-8'), None

def snapshot_state() -> tuple[bytes, str | None]:
    """Snapshot actor state for persistence."""
    global _state, _data
    try:
        state_data = {
            "state": _state,
            "data": _data
        }
        import json
        return json.dumps(state_data).encode('utf-8'), None
    except Exception as e:
        return b"", f"Snapshot failed: {str(e)}"

def shutdown() -> tuple[None, str | None]:
    """Graceful shutdown."""
    global _state, _data
    _state = "idle"
    _data = None
    return None, None
