#!/usr/bin/env python3
"""
Python Service Demo Actor

Demonstrates accessing PlexSpaces services from Python WASM actor:
- ActorService: Spawn actors, send messages
- TupleSpace: Write/read tuples
- ChannelService: Send to queues, publish to topics (via host functions)
"""

# Actor state
_spawned_actors = []
_messages_sent = 0
_queue_messages = []
_topic_messages = []

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with state."""
    global _spawned_actors, _messages_sent, _queue_messages, _topic_messages
    _spawned_actors = []
    _messages_sent = 0
    _queue_messages = []
    _topic_messages = []
    return None, None

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle incoming messages demonstrating service access."""
    global _spawned_actors, _messages_sent, _queue_messages, _topic_messages
    
    try:
        if message_type == "spawn_actor":
            # Spawn a child actor (requires allow_spawn_actors capability)
            # Note: This would call host::spawn_actor() in real implementation
            # For now, we simulate it
            child_id = f"child-{len(_spawned_actors)}"
            _spawned_actors.append(child_id)
            return child_id.encode('utf-8'), None
            
        elif message_type == "send_message":
            # Send message to another actor (requires allow_send_messages capability)
            # Note: This would call host::send_message() in real implementation
            _messages_sent += 1
            return b"sent", None
            
        elif message_type == "tuplespace_write":
            # Write tuple to TupleSpace (requires allow_tuplespace capability)
            # Note: This would call host::tuplespace_write() in real implementation
            return b"written", None
            
        elif message_type == "tuplespace_read":
            # Read tuple from TupleSpace (requires allow_tuplespace capability)
            # Note: This would call host::tuplespace_read() in real implementation
            return b"read", None
            
        elif message_type == "send_to_queue":
            # Send message to queue (requires channel service)
            # Note: This would call host::send_to_queue() in real implementation
            # For demo, we simulate receiving it
            queue_name = payload.decode('utf-8', errors='ignore')
            _queue_messages.append(queue_name)
            return b"queued", None
            
        elif message_type == "publish_to_topic":
            # Publish message to topic (requires channel service)
            # Note: This would call host::publish_to_topic() in real implementation
            # For demo, we simulate receiving it
            topic_name = payload.decode('utf-8', errors='ignore')
            _topic_messages.append(topic_name)
            return b"published", None
            
        elif message_type == "receive_from_queue":
            # Receive message from queue (requires channel service)
            # Note: This would call host::receive_from_queue() in real implementation
            # For demo, we return a simulated message
            if _queue_messages:
                msg = _queue_messages.pop(0)
                return msg.encode('utf-8'), None
            return b"", None  # No message available
            
        elif message_type == "get_stats":
            # Return statistics
            import json
            stats = {
                "spawned_actors": len(_spawned_actors),
                "messages_sent": _messages_sent,
                "queue_messages": len(_queue_messages),
                "topic_messages": len(_topic_messages)
            }
            return json.dumps(stats).encode('utf-8'), None
            
        else:
            return b"", f"Unknown message type: {message_type}"
    except Exception as e:
        return b"", f"Handle message failed: {str(e)}"

def snapshot_state() -> tuple[bytes, str | None]:
    """Snapshot actor state for persistence."""
    global _spawned_actors, _messages_sent, _queue_messages, _topic_messages
    try:
        import json
        state = {
            "spawned_actors": _spawned_actors,
            "messages_sent": _messages_sent,
            "queue_messages": _queue_messages,
            "topic_messages": _topic_messages
        }
        return json.dumps(state).encode('utf-8'), None
    except Exception as e:
        return b"", f"Snapshot failed: {str(e)}"

def shutdown() -> tuple[None, str | None]:
    """Graceful shutdown."""
    global _spawned_actors, _messages_sent, _queue_messages, _topic_messages
    _spawned_actors.clear()
    _messages_sent = 0
    _queue_messages.clear()
    _topic_messages.clear()
    return None, None
