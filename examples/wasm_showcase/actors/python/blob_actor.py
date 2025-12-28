#!/usr/bin/env python3
"""
Python Blob Actor Example

Demonstrates using blob storage from WASM actors.
This actor uploads, downloads, and manages blobs using the blob host interface.
"""

# Actor state
_state = {}

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with state."""
    global _state
    try:
        if initial_state:
            # Could load state from blob storage here
            pass
        return None, None
    except Exception as e:
        return None, f"Init failed: {str(e)}"

def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle messages using blob storage."""
    global _state
    
    try:
        if message_type == "upload":
            # Upload blob
            # Format: bucket|key|content-type|data
            parts = payload.split(b'|', 3)
            if len(parts) < 4:
                return b"", "Invalid upload format: bucket|key|content-type|data"
            
            bucket = parts[0].decode('utf-8')
            key = parts[1].decode('utf-8')
            content_type = parts[2].decode('utf-8')
            data = parts[3]
            
            # Note: In real implementation, this would call host.blob.upload()
            # For now, this is a placeholder demonstrating the pattern
            _state[f"{bucket}/{key}"] = {
                "data": data,
                "content_type": content_type,
                "size": len(data),
            }
            return f"Uploaded {key} ({len(data)} bytes)".encode('utf-8'), None
            
        elif message_type == "download":
            # Download blob
            # Format: bucket|key
            parts = payload.split(b'|', 1)
            if len(parts) < 2:
                return b"", "Invalid download format: bucket|key"
            
            bucket = parts[0].decode('utf-8')
            key = parts[1].decode('utf-8')
            blob_key = f"{bucket}/{key}"
            
            if blob_key in _state:
                return _state[blob_key]["data"], None
            else:
                return b"", f"Blob not found: {key}"
                
        elif message_type == "delete":
            # Delete blob
            # Format: bucket|key
            parts = payload.split(b'|', 1)
            if len(parts) < 2:
                return b"", "Invalid delete format: bucket|key"
            
            bucket = parts[0].decode('utf-8')
            key = parts[1].decode('utf-8')
            blob_key = f"{bucket}/{key}"
            
            if blob_key in _state:
                del _state[blob_key]
                return b"deleted", None
            else:
                return b"", f"Blob not found: {key}"
                
        elif message_type == "exists":
            # Check if blob exists
            # Format: bucket|key
            parts = payload.split(b'|', 1)
            if len(parts) < 2:
                return b"", "Invalid exists format: bucket|key"
            
            bucket = parts[0].decode('utf-8')
            key = parts[1].decode('utf-8')
            blob_key = f"{bucket}/{key}"
            
            exists = blob_key in _state
            return b"true" if exists else b"false", None
            
        elif message_type == "list":
            # List blobs with prefix
            # Format: bucket|prefix
            parts = payload.split(b'|', 1)
            if len(parts) < 2:
                return b"", "Invalid list format: bucket|prefix"
            
            bucket = parts[0].decode('utf-8')
            prefix = parts[1].decode('utf-8')
            
            matching_keys = [k for k in _state.keys() if k.startswith(f"{bucket}/{prefix}")]
            result = "\n".join(matching_keys).encode('utf-8')
            return result, None
            
        elif message_type == "copy":
            # Copy blob
            # Format: source-bucket|source-key|dest-bucket|dest-key
            parts = payload.split(b'|', 3)
            if len(parts) < 4:
                return b"", "Invalid copy format: source-bucket|source-key|dest-bucket|dest-key"
            
            source_bucket = parts[0].decode('utf-8')
            source_key = parts[1].decode('utf-8')
            dest_bucket = parts[2].decode('utf-8')
            dest_key = parts[3].decode('utf-8')
            
            source_blob_key = f"{source_bucket}/{source_key}"
            dest_blob_key = f"{dest_bucket}/{dest_key}"
            
            if source_blob_key in _state:
                _state[dest_blob_key] = _state[source_blob_key].copy()
                return f"Copied to {dest_key}".encode('utf-8'), None
            else:
                return b"", f"Source blob not found: {source_key}"
        else:
            return b"", f"Unknown message type: {message_type}"
            
    except Exception as e:
        return b"", f"Handle message failed: {str(e)}"


