# WIT Interface Documentation

## Overview

The PlexSpaces WASM actor interface is defined using WebAssembly Interface Types (WIT), providing a type-safe, language-agnostic contract between WASM actors and the PlexSpaces runtime.

## Location

The WIT interface is defined in: `wit/plexspaces-actor/actor.wit`

## Key Features

### 1. Behavior-Specific Exports

Actors can export behavior-specific handlers for optimized message routing:

- **`handle_request()`** - GenServer behavior (request-reply)
  - Message type "call" routes here
  - MUST return a response payload
  - Example: Counter actor, stateful services

- **`handle_event()`** - GenEvent behavior (fire-and-forget)
  - Message types "cast" or "info" route here
  - No response needed
  - Example: Event logger, metrics collector

- **`handle_transition()`** - GenFSM behavior (state machine)
  - Any message type can route here
  - Returns new state name (empty if no transition)
  - Example: Workflow actor, protocol state machine

- **`handle_message()`** - Generic handler (backward compatibility)
  - Fallback if behavior-specific handlers not exported
  - Supports all message types

### 2. Channel Host Functions

WASM actors can access ChannelService via host functions:

- **`send_to_queue(queue_name, message_type, payload)`**
  - Send message to queue (load-balanced to one consumer)
  - Returns message ID on success

- **`publish_to_topic(topic_name, message_type, payload)`**
  - Publish message to topic (all subscribers receive)
  - Returns message ID on success

- **`receive_from_queue(queue_name, timeout_ms)`**
  - Receive message from queue (blocking, with timeout)
  - Returns (message_type, payload) tuple or None if timeout
  - Note: Requires async host functions (placeholder for now)

### 3. Other Host Functions

- **`send_message(to, message_type, payload)`** - Send message to actor
- **`spawn_actor(module_ref, initial_state)`** - Spawn child actor
- **`tuplespace_write(tuple)`** - Write tuple to TupleSpace
- **`tuplespace_read(pattern)`** - Read tuple (non-destructive)
- **`tuplespace_take(pattern)`** - Take tuple (destructive)
- **`tuplespace_count(pattern)`** - Count matching tuples
- **`log(level, message)`** - Emit log message
- **`now_millis()`** - Get current timestamp
- **`sleep_millis(millis)`** - Sleep for duration

## Usage Examples

### Python Actor (GenServer)

```python
def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    if message_type == "get_count":
        return _counter.to_bytes(8, byteorder='little'), None
    return b"", f"Unknown message type: {message_type}"
```

### Python Actor (GenEvent)

```python
def handle_event(from_actor: str, message_type: str, payload: bytes) -> tuple[None, str | None]:
    if message_type == "notify":
        _events.append(payload.decode('utf-8'))
        return None, None
    return None, f"Unknown message type: {message_type}"
```

### Python Actor (Channel Usage)

```python
def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    if message_type == "queue_task":
        # Note: In real implementation, this would call host::send_to_queue()
        # For now, this is simulated in service_demo_actor.py
        return b"queued", None
    return b"", None
```

## Behavior Routing

The WASM runtime automatically routes messages based on:

1. **Message type**:
   - "call" → `handle_request()` (if exported)
   - "cast" or "info" → `handle_event()` (if exported)
   - Any → `handle_transition()` (if exported)

2. **Exported functions**:
   - Runtime checks for behavior-specific handlers first
   - Falls back to `handle_message()` if not found

3. **Backward compatibility**:
   - Actors exporting only `handle_message()` continue to work
   - Behavior-specific handlers are optional

## Channel Service Integration

ChannelService is automatically passed from the node when creating WASM instances:

- Node creates ChannelServiceWrapper
- ChannelService is passed to WasmInstance::new()
- Host functions can access ChannelService via HostFunctions
- Python actors can use channels via host function calls (when bindings are generated)

## See Also

- `wit/plexspaces-actor/actor.wit` - Full WIT interface definition
- `examples/wasm_showcase/actors/python/` - Python actor examples
- `crates/wasm-runtime/src/host_functions.rs` - Host function implementation
- `crates/wasm-runtime/src/instance.rs` - Message routing implementation

