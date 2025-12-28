#!/usr/bin/env python3
"""
Python Locks Actor Example

Demonstrates using distributed locks from WASM actors.
This actor acquires, renews, and releases locks for coordination.
"""

# Actor state
_held_locks = {}

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with state."""
    global _held_locks
    try:
        return None, None
    except Exception as e:
        return None, f"Init failed: {str(e)}"

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle messages using distributed locks."""
    global _held_locks
    
    try:
        if message_type == "acquire":
            # Acquire a lock
            # Note: In real implementation, this would call host.locks.acquire()
            parts = payload.split(b':', 2)
            if len(parts) >= 2:
                lock_key = parts[0].decode('utf-8')
                holder_id = parts[1].decode('utf-8')
                lease_duration = int(parts[2].decode('utf-8')) if len(parts) > 2 else 30000
                _held_locks[lock_key] = {
                    'holder_id': holder_id,
                    'version': '1',
                    'locked': True
                }
                return b"acquired", None
            else:
                return b"", "Invalid acquire format: lock_key:holder_id:lease_duration"
                
        elif message_type == "release":
            # Release a lock
            # Note: In real implementation, this would call host.locks.release()
            parts = payload.split(b':', 2)
            if len(parts) >= 2:
                lock_key = parts[0].decode('utf-8')
                holder_id = parts[1].decode('utf-8')
                version = parts[2].decode('utf-8') if len(parts) > 2 else '1'
                if lock_key in _held_locks:
                    del _held_locks[lock_key]
                    return b"released", None
                else:
                    return b"", f"Lock not held: {lock_key}"
            else:
                return b"", "Invalid release format: lock_key:holder_id:version"
                
        elif message_type == "try_acquire":
            # Try to acquire lock (non-blocking)
            # Note: In real implementation, this would call host.locks.try_acquire()
            parts = payload.split(b':', 2)
            if len(parts) >= 2:
                lock_key = parts[0].decode('utf-8')
                holder_id = parts[1].decode('utf-8')
                if lock_key not in _held_locks:
                    _held_locks[lock_key] = {
                        'holder_id': holder_id,
                        'version': '1',
                        'locked': True
                    }
                    return b"acquired", None
                else:
                    return b"", "Lock already held"
            else:
                return b"", "Invalid try_acquire format: lock_key:holder_id"
                
        else:
            return b"", f"Unknown message type: {message_type}"
    except Exception as e:
        return b"", f"Handle message failed: {str(e)}"

def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle synchronous request (GenServer pattern)."""
    return handle_message(from_actor, message_type, payload)






