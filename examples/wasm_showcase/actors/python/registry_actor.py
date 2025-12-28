#!/usr/bin/env python3
"""
Python Registry Actor Example

Demonstrates using object registry for service discovery from WASM actors.
This actor registers itself and discovers other services.
"""

# Actor state
_registered_objects = {}

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with state."""
    global _registered_objects
    try:
        # In real implementation, this would call host.registry.register()
        # to register this actor as a service
        return None, None
    except Exception as e:
        return None, f"Init failed: {str(e)}"

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle messages using object registry."""
    global _registered_objects
    
    try:
        if message_type == "register":
            # Register an object
            # Note: In real implementation, this would call host.registry.register()
            # Format: object_id:object_type:grpc_address:category
            parts = payload.split(b':', 3)
            if len(parts) >= 3:
                object_id = parts[0].decode('utf-8')
                object_type = parts[1].decode('utf-8')
                grpc_address = parts[2].decode('utf-8')
                category = parts[3].decode('utf-8') if len(parts) > 3 else ''
                _registered_objects[object_id] = {
                    'object_type': object_type,
                    'grpc_address': grpc_address,
                    'category': category
                }
                return b"registered", None
            else:
                return b"", "Invalid register format: object_id:object_type:grpc_address:category"
                
        elif message_type == "lookup":
            # Lookup an object
            # Note: In real implementation, this would call host.registry.lookup()
            parts = payload.split(b':', 1)
            if len(parts) >= 2:
                object_type = parts[0].decode('utf-8')
                object_id = parts[1].decode('utf-8')
                if object_id in _registered_objects:
                    obj = _registered_objects[object_id]
                    return f"{obj['grpc_address']}:{obj['category']}".encode('utf-8'), None
                else:
                    return b"", f"Object not found: {object_id}"
            else:
                return b"", "Invalid lookup format: object_type:object_id"
                
        elif message_type == "discover":
            # Discover objects
            # Note: In real implementation, this would call host.registry.discover()
            object_type = payload.decode('utf-8')
            matching = [obj_id for obj_id, obj in _registered_objects.items() 
                       if obj['object_type'] == object_type]
            return '\n'.join(matching).encode('utf-8'), None
            
        elif message_type == "unregister":
            # Unregister an object
            # Note: In real implementation, this would call host.registry.unregister()
            parts = payload.split(b':', 1)
            if len(parts) >= 2:
                object_type = parts[0].decode('utf-8')
                object_id = parts[1].decode('utf-8')
                if object_id in _registered_objects:
                    del _registered_objects[object_id]
                    return b"unregistered", None
                else:
                    return b"", f"Object not found: {object_id}"
            else:
                return b"", "Invalid unregister format: object_type:object_id"
                
        else:
            return b"", f"Unknown message type: {message_type}"
    except Exception as e:
        return b"", f"Handle message failed: {str(e)}"

def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle synchronous request (GenServer pattern)."""
    return handle_message(from_actor, message_type, payload)






