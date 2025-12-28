#!/usr/bin/env python3
"""
Python Durability Actor Example

Demonstrates using durability/journaling from WASM actors.
This actor persists events and creates checkpoints for crash recovery.
"""

# Actor state
_counter = 0
_event_count = 0

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with state."""
    global _counter, _event_count
    try:
        if initial_state:
            # Could replay events from journal here
            _counter = int.from_bytes(initial_state, byteorder='little') if len(initial_state) >= 4 else 0
        return None, None
    except Exception as e:
        return None, f"Init failed: {str(e)}"

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle messages using durability/journaling."""
    global _counter, _event_count
    
    try:
        if message_type == "increment":
            # Persist event before updating state
            # Note: In real implementation, this would call host.durability.persist()
            _event_count += 1
            _counter += 1
            # In real implementation: host.durability.persist('incremented', payload)
            return _counter.to_bytes(8, byteorder='little'), None
            
        elif message_type == "get_count":
            # Return current count
            return _counter.to_bytes(8, byteorder='little'), None
            
        elif message_type == "checkpoint":
            # Create checkpoint
            # Note: In real implementation, this would call host.durability.checkpoint()
            # In real implementation: host.durability.checkpoint()
            return b"checkpointed", None
            
        elif message_type == "get_sequence":
            # Get current event sequence
            # Note: In real implementation, this would call host.durability.get_sequence()
            return _event_count.to_bytes(8, byteorder='little'), None
            
        elif message_type == "persist_batch":
            # Persist multiple events atomically
            # Note: In real implementation, this would call host.durability.persist_batch()
            # Parse events from payload (simplified)
            _event_count += 3  # Assume 3 events in batch
            return b"batch_persisted", None
            
        else:
            return b"", f"Unknown message type: {message_type}"
    except Exception as e:
        return b"", f"Handle message failed: {str(e)}"

def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle synchronous request (GenServer pattern)."""
    return handle_message(from_actor, message_type, payload)

def snapshot_state() -> tuple[bytes, str | None]:
    """Snapshot actor state for persistence."""
    global _counter
    try:
        return _counter.to_bytes(8, byteorder='little'), None
    except Exception as e:
        return b"", f"Snapshot failed: {str(e)}"






