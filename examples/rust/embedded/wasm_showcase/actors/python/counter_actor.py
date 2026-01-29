#!/usr/bin/env python3
"""
Python Counter Actor Example (GenServer Behavior)

Demonstrates a GenServer behavior pattern actor in Python using componentize-py.
This actor implements request-reply pattern using handle_request().
"""

# Actor state
_counter = 0

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with state."""
    global _counter
    try:
        if initial_state:
            _counter = int.from_bytes(initial_state, byteorder='little')
        else:
            _counter = 0
        return None, None
    except Exception as e:
        return None, f"Init failed: {str(e)}"

def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle synchronous request (GenServer pattern - MUST return response)."""
    global _counter
    
    try:
        if message_type == "increment":
            _counter += 1
            return _counter.to_bytes(8, byteorder='little'), None  # Return new count
        elif message_type == "get_count":
            # Return current count as response
            return _counter.to_bytes(8, byteorder='little'), None
        elif message_type == "add":
            # Add value from payload
            value = int.from_bytes(payload, byteorder='little')
            _counter += value
            return _counter.to_bytes(8, byteorder='little'), None  # Return new count
        else:
            return b"", f"Unknown message type: {message_type}"
    except Exception as e:
        return b"", f"Handle request failed: {str(e)}"

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Fallback generic handler (for backward compatibility)."""
    # For GenServer, delegate to handle_request
    return handle_request(from_actor, message_type, payload)

def snapshot_state() -> tuple[bytes, str | None]:
    """Snapshot actor state for persistence."""
    global _counter
    try:
        return _counter.to_bytes(8, byteorder='little'), None
    except Exception as e:
        return b"", f"Snapshot failed: {str(e)}"

def shutdown() -> tuple[None, str | None]:
    """Graceful shutdown."""
    global _counter
    _counter = 0
    return None, None
