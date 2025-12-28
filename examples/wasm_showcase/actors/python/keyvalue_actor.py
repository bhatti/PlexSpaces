#!/usr/bin/env python3
"""
Python KeyValue Actor Example

Demonstrates using KeyValue storage from WASM actors.
This actor stores and retrieves key-value pairs using the KeyValue host interface.
"""

# Actor state
_state = {}

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with state."""
    global _state
    try:
        if initial_state:
            # Could load state from KeyValue here
            pass
        return None, None
    except Exception as e:
        return None, f"Init failed: {str(e)}"

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle messages using KeyValue storage."""
    global _state
    
    try:
        if message_type == "store":
            # Store key-value pair
            # Note: In real implementation, this would call host.keyvalue.put()
            # For now, this is a placeholder demonstrating the pattern
            key = payload[:payload.find(b'\0')].decode('utf-8') if b'\0' in payload else payload.decode('utf-8')
            value = payload[payload.find(b'\0')+1:] if b'\0' in payload else b''
            _state[key] = value
            return b"stored", None
            
        elif message_type == "retrieve":
            # Retrieve value by key
            key = payload.decode('utf-8')
            if key in _state:
                return _state[key], None
            else:
                return b"", f"Key not found: {key}"
                
        elif message_type == "delete":
            # Delete key
            key = payload.decode('utf-8')
            if key in _state:
                del _state[key]
                return b"deleted", None
            else:
                return b"", f"Key not found: {key}"
                
        elif message_type == "list":
            # List all keys
            keys = list(_state.keys())
            return '\n'.join(keys).encode('utf-8'), None
            
        else:
            return b"", f"Unknown message type: {message_type}"
    except Exception as e:
        return b"", f"Handle message failed: {str(e)}"

def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle synchronous request (GenServer pattern)."""
    return handle_message(from_actor, message_type, payload)






