# Polyglot WASM Development Guide

## Overview

PlexSpaces supports **polyglot development** - you can write actors, applications, services, and workflows in multiple languages (Python, TypeScript/JavaScript, Rust, Go) and deploy them as WebAssembly (WASM) modules to PlexSpaces nodes. All languages share the same **WIT (WebAssembly Interface Types)** abstractions, ensuring consistent behavior across languages.

**Key Benefits:**
- ✅ **Language Choice**: Use the best language for each task (Python for ML, TypeScript for web integration, Rust for performance)
- ✅ **Unified Abstractions**: All languages use the same WIT interfaces for framework services
- ✅ **Type Safety**: WIT provides type-safe interfaces across languages
- ✅ **Isolation**: Each WASM module runs in isolation with resource limits
- ✅ **Dynamic Deployment**: Deploy applications at runtime without restarting nodes

## Supported Languages

| Language | Compiler | WASM Size | Runtime Performance | Best For |
|----------|----------|-----------|---------------------|----------|
| **Python** | `componentize-py` | 30-40MB | Moderate | ML, data processing, rapid prototyping |
| **TypeScript/JavaScript** | `javy` | 500KB-2MB | Good | Web integration, rapid development |
| **Rust** | `cargo` (wasm32-wasip2) | 100KB-1MB | Excellent | Production, performance-critical |
| **Go** | `tinygo` | 2-5MB | Good | Good balance, fast iteration |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│              PlexSpaces Node                                │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  WASM Runtime (wasmtime)                             │  │
│  │  - Loads and executes WASM modules                 │  │
│  │  - Provides WIT host functions                       │  │
│  │  - Enforces resource limits                         │  │
│  └──────────────────────────────────────────────────────┘  │
│                          │                                   │
│                          ▼                                   │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Polyglot WASM Applications                          │  │
│  │                                                      │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────┐  │  │
│  │  │ Python Actor │  │ TypeScript   │  │ Rust     │  │  │
│  │  │ (Python)     │  │ Actor (TS)   │  │ Actor    │  │  │
│  │  └──────────────┘  └──────────────┘  └──────────┘  │  │
│  │         │                  │                 │        │  │
│  │         └──────────────────┼─────────────────┘        │  │
│  │                            │                            │  │
│  │                            ▼                            │  │
│  │  ┌──────────────────────────────────────────────┐     │  │
│  │  │  WIT Host Functions (Unified Interface)     │     │  │
│  │  │  - messaging, tuplespace, keyvalue, etc.    │     │  │
│  │  └──────────────────────────────────────────────┘     │  │
│  └──────────────────────────────────────────────────────────┘  │
│                          │                                   │
│                          ▼                                   │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Framework Services                                  │  │
│  │  - ActorService, TupleSpace, KeyValue, Blob, etc.   │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## WIT Abstractions

All polyglot applications use the same **WIT (WebAssembly Interface Types)** interfaces, defined in `wit/plexspaces-actor/`. These provide a unified, type-safe API across all languages.

### Core Abstractions

#### 1. **Messaging** (`messaging.wit`)
Actor-to-actor communication:
- `send_message(to: actor-id, msg-type: string, payload: bytes)`
- `ask_message(to: actor-id, msg-type: string, payload: bytes, timeout: duration-ms) -> result`
- `spawn_actor(behavior: string, options: spawn-options) -> actor-id`

**Use Cases**: Request-reply patterns, actor spawning, message routing

#### 2. **TupleSpace** (`tuplespace.wit`)
Linda-style distributed coordination:
- `write(ctx: context, tuple: tuple-data)`
- `read(ctx: context, pattern: pattern) -> option<tuple-data>`
- `take(ctx: context, pattern: pattern) -> option<tuple-data>`
- `read_all(ctx: context, pattern: pattern) -> list<tuple-data>`
- `count(ctx: context, pattern: pattern) -> u32`

**Use Cases**: Distributed coordination, result sharing, pattern matching

#### 3. **Key-Value Store** (`keyvalue.wit`)
Distributed key-value storage:
- `get(ctx: context, key: string) -> option<bytes>`
- `put(ctx: context, key: string, value: bytes)`
- `put_with_ttl(ctx: context, key: string, value: bytes, ttl: duration-ms)`
- `delete(ctx: context, key: string)`
- `exists(ctx: context, key: string) -> bool`
- `list_keys(ctx: context, prefix: string) -> list<string>`
- `compare_and_swap(ctx: context, key: string, expected: bytes, new: bytes) -> bool`
- `increment(ctx: context, key: string, delta: s64) -> s64`

**Use Cases**: State storage, caching, distributed counters

#### 4. **Blob Storage** (`blob.wit`)
Large object storage:
- `upload(ctx: context, blob_id: string, data: bytes, metadata: blob-metadata) -> string`
- `download(ctx: context, blob_id: string) -> bytes`
- `delete(ctx: context, blob_id: string)`
- `exists(ctx: context, blob_id: string) -> bool`
- `metadata(ctx: context, blob_id: string) -> option<blob-metadata>`
- `list_blobs(ctx: context, prefix: string) -> list<string>`
- `copy(ctx: context, source_id: string, dest_id: string) -> string`

**Use Cases**: File storage, media files, large data objects

#### 5. **Channels** (`channels.wit`)
Queues and pub/sub:
- `send_to_queue(ctx: context, queue: string, message: bytes)`
- `receive_from_queue(ctx: context, queue: string, timeout: duration-ms) -> option<bytes>`
- `publish_to_topic(ctx: context, topic: string, message: bytes)`
- `subscribe_to_topic(ctx: context, topic: string)`
- `create_queue(ctx: context, queue: string)`
- `queue_depth(ctx: context, queue: string) -> u32`

**Use Cases**: Task queues, event streaming, pub/sub patterns

#### 6. **Durability** (`durability.wit`)
State persistence and recovery:
- `persist(ctx: context, key: string, value: bytes)`
- `persist_batch(ctx: context, entries: list<tuple<string, bytes>>)`
- `checkpoint(ctx: context)`
- `get_sequence(ctx: context) -> u64`
- `is_replaying(ctx: context) -> bool`
- `read_journal(ctx: context, from: u64, to: u64) -> list<bytes>`

**Use Cases**: Fault tolerance, state recovery, event sourcing

#### 7. **Locks** (`locks.wit`)
Distributed locking:
- `acquire(ctx: context, lock_id: string, timeout: duration-ms) -> bool`
- `release(ctx: context, lock_id: string)`
- `renew(ctx: context, lock_id: string, duration: duration-ms)`
- `try_acquire(ctx: context, lock_id: string) -> bool`

**Use Cases**: Mutual exclusion, distributed coordination

#### 8. **Process Groups** (`process-groups.wit`)
Actor group management:
- `create_group(ctx: context, group_name: string)`
- `join_group(ctx: context, group_name: string)`
- `leave_group(ctx: context, group_name: string)`
- `get_members(ctx: context, group_name: string) -> list<actor-id>`
- `publish_to_group(ctx: context, group_name: string, message: bytes)`

**Use Cases**: Actor clustering, group messaging, sharding

#### 9. **Registry** (`registry.wit`)
Service discovery:
- `register(ctx: context, name: string, address: string, metadata: map<string, string>)`
- `lookup(ctx: context, name: string) -> option<registry-entry>`
- `unregister(ctx: context, name: string)`
- `discover(ctx: context, pattern: string) -> list<registry-entry>`
- `heartbeat(ctx: context, name: string)`

**Use Cases**: Service discovery, node registration, health checking

#### 10. **Workflow** (`workflow.wit`)
Durable orchestration:
- `start_workflow(ctx: context, workflow_id: string, input: bytes)`
- `signal_workflow(ctx: context, workflow_id: string, signal: string, data: bytes)`
- `query_workflow(ctx: context, workflow_id: string) -> bytes`
- `await_workflow(ctx: context, workflow_id: string, timeout: duration-ms) -> bytes`
- `schedule_activity(ctx: context, activity: string, input: bytes) -> bytes`
- `sleep(ctx: context, duration: duration-ms)`

**Use Cases**: Long-running workflows, orchestration, state machines

### Context Parameter

All WIT host functions require a `context` parameter as the first argument:

```wit
record context {
    /// Tenant ID for multi-tenancy (empty string for default/internal)
    tenant-id: string,
    /// Namespace for resource isolation (empty string for default)
    namespace: string,
}
```

**Usage**:
- Empty strings (`""`) use default/internal context
- Non-empty values enable multi-tenant isolation
- All operations are scoped to the provided context

## Core Functionality Patterns

### 1. Basic Actor (Message Handling)

**Pattern**: Fire-and-forget message processing

**Python Example**:
```python
# calculator_actor.py
def handle_message(from_actor: str, message_type: str, payload: bytes) -> bytes:
    """Basic actor - handles messages asynchronously"""
    import json
    
    if message_type == "calculate":
        request = json.loads(payload.decode('utf-8'))
        result = request.get('a', 0) + request.get('b', 0)
        return json.dumps({'result': result}).encode('utf-8')
    
    return b'{"error": "unknown message type"}'
```

**TypeScript Example**:
```typescript
// calculator_actor.ts
export function handleMessage(
    fromActor: string,
    messageType: string,
    payload: Uint8Array
): Uint8Array {
    if (messageType === "calculate") {
        const request = JSON.parse(new TextDecoder().decode(payload));
        const result = request.a + request.b;
        return new TextEncoder().encode(JSON.stringify({ result }));
    }
    return new TextEncoder().encode('{"error": "unknown message type"}');
}
```

**See**: `examples/simple/wasm_calculator/actors/python/calculator_actor.py`

### 2. GenServer (Request-Reply Pattern)

**Pattern**: Synchronous request-reply with return value

**Python Example**:
```python
# calculator_actor.py - GenServer behavior
def handle_request(from_actor: str, message_type: str, payload: bytes) -> bytes:
    """
    GenServer behavior - request-reply pattern.
    MUST return a response (unlike handle_message which is fire-and-forget).
    """
    import json
    
    request = json.loads(payload.decode('utf-8'))
    operation = request.get('operation', '')
    operands = request.get('operands', [])
    
    # Perform calculation
    if operation == 'add':
        result = sum(operands)
    elif operation == 'multiply':
        result = 1
        for op in operands:
            result *= op
    else:
        return json.dumps({'error': f'Unknown operation: {operation}'}).encode('utf-8')
    
    # Return result (required for GenServer)
    return json.dumps({'result': result, 'operation': operation}).encode('utf-8')
```

**Key Difference**: `handle_request()` returns a value (GenServer), while `handle_message()` is fire-and-forget (Actor).

**See**: `examples/simple/wasm_calculator/actors/python/calculator_actor.py` (lines 6-49)

### 3. Durable Actor (State Persistence)

**Pattern**: State persistence with journaling and recovery

**Python Example**:
```python
# durable_calculator_actor.py
_calculator_state = {
    'last_operation': None,
    'last_result': 0.0,
    'operation_count': 0,
    'history': []
}

def init(initial_state: bytes) -> tuple[None, str | None]:
    """Initialize actor with persisted state (called on recovery)"""
    global _calculator_state
    if initial_state:
        import json
        _calculator_state = json.loads(initial_state.decode('utf-8'))
    return None, None

def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Handle requests - state is automatically journaled"""
    global _calculator_state
    import json
    
    request = json.loads(payload.decode('utf-8'))
    operation = request.get('operation', '')
    operands = request.get('operands', [])
    
    # Perform calculation
    result = sum(operands) if operation == 'add' else 0
    
    # Update state (automatically journaled by DurabilityFacet)
    _calculator_state['last_operation'] = operation
    _calculator_state['last_result'] = result
    _calculator_state['operation_count'] += 1
    _calculator_state['history'].append({'operation': operation, 'result': result})
    
    return json.dumps({'result': result, 'durable': True}).encode('utf-8'), None

def snapshot_state() -> tuple[bytes, str | None]:
    """Snapshot state for checkpointing"""
    global _calculator_state
    import json
    return json.dumps(_calculator_state).encode('utf-8'), None
```

**Features**:
- `init()` restores state from checkpoint
- `snapshot_state()` serializes state for persistence
- State automatically journaled by framework

**See**: `examples/simple/wasm_calculator/actors/python/durable_calculator_actor.py`

### 4. TupleSpace Coordination

**Pattern**: Distributed coordination via associative memory

**Python Example**:
```python
# tuplespace_calculator_actor.py
def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Use TupleSpace for distributed coordination"""
    import json
    
    request = json.loads(payload.decode('utf-8'))
    operation = request.get('operation', '')
    operands = request.get('operands', [])
    
    # Perform calculation
    result = sum(operands) if operation == 'add' else 0
    
    # Write result to TupleSpace (using WIT host function)
    # Note: Actual WIT binding would be: host.tuplespace_write(ctx, tuple_fields)
    tuple_fields = [
        {"string-val": "calculator_result"},
        {"string-val": operation},
        {"float-val": result},
        {"string-val": from_actor},
    ]
    # host.tuplespace_write(ctx, tuple_fields)  # Bound via WIT
    
    # Read results from other actors
    pattern = [
        {"exact": {"string-val": "calculator_result"}},
        {"any": None},  # Match any operation
        {"any": None},  # Match any result
    ]
    # results = host.tuplespace_read(ctx, pattern)  # Bound via WIT
    
    return json.dumps({'result': result, 'tuplespace_used': True}).encode('utf-8'), None
```

**Use Cases**: Result sharing, distributed coordination, pattern matching

**See**: `examples/simple/wasm_calculator/actors/python/tuplespace_calculator_actor.py`

### 5. Workflow Orchestration

**Pattern**: Long-running durable workflows with activities

**Python Example**:
```python
# workflow_actor.py
def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Orchestrate multi-step workflow"""
    import json
    
    if message_type == "start_workflow":
        request = json.loads(payload.decode('utf-8'))
        workflow_id = request.get('workflow_id')
        input_data = request.get('input')
        
        # Step 1: Start workflow (using WIT host function)
        # host.workflow_start_workflow(ctx, workflow_id, json.dumps(input_data).encode())
        
        # Step 2: Schedule activities
        # result1 = host.workflow_schedule_activity(ctx, "step1", input_data)
        # result2 = host.workflow_schedule_activity(ctx, "step2", result1)
        
        # Step 3: Wait for completion
        # final_result = host.workflow_await_workflow(ctx, workflow_id, timeout_ms=60000)
        
        return json.dumps({'workflow_id': workflow_id, 'status': 'started'}).encode('utf-8'), None
    
    elif message_type == "query_workflow":
        request = json.loads(payload.decode('utf-8'))
        workflow_id = request.get('workflow_id')
        
        # Query workflow state
        # state = host.workflow_query_workflow(ctx, workflow_id)
        
        return json.dumps({'workflow_id': workflow_id, 'state': 'running'}).encode('utf-8'), None
```

**Use Cases**: Multi-step processes, long-running tasks, state machines

**See**: `examples/domains/genomics-pipeline/` for complete workflow example

### 6. Service Pattern (Key-Value, Blob, Channels)

**Pattern**: Using framework services via WIT host functions

**Python Example - Key-Value Store**:
```python
# service_actor.py
def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Use Key-Value store for state management"""
    import json
    
    ctx = {"tenant-id": "", "namespace": ""}  # Default context
    
    if message_type == "store":
        request = json.loads(payload.decode('utf-8'))
        key = request.get('key')
        value = request.get('value')
        
        # Store value (using WIT host function)
        # host.keyvalue_put(ctx, key, json.dumps(value).encode())
        
        return json.dumps({'status': 'stored'}).encode('utf-8'), None
    
    elif message_type == "retrieve":
        request = json.loads(payload.decode('utf-8'))
        key = request.get('key')
        
        # Retrieve value
        # value_bytes = host.keyvalue_get(ctx, key)
        # value = json.loads(value_bytes.decode()) if value_bytes else None
        
        return json.dumps({'key': key, 'value': None}).encode('utf-8'), None
```

**Python Example - Blob Storage**:
```python
def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Use Blob storage for large objects"""
    import json
    
    ctx = {"tenant-id": "", "namespace": ""}
    
    if message_type == "upload":
        request = json.loads(payload.decode('utf-8'))
        blob_id = request.get('blob_id')
        data = request.get('data').encode()
        
        # Upload blob
        # blob_id = host.blob_upload(ctx, blob_id, data, metadata)
        
        return json.dumps({'blob_id': blob_id}).encode('utf-8'), None
    
    elif message_type == "download":
        request = json.loads(payload.decode('utf-8'))
        blob_id = request.get('blob_id')
        
        # Download blob
        # data = host.blob_download(ctx, blob_id)
        
        return json.dumps({'blob_id': blob_id, 'size': 0}).encode('utf-8'), None
```

**Python Example - Channels (Queues)**:
```python
def handle_request(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:
    """Use Channels for task queues"""
    import json
    
    ctx = {"tenant-id": "", "namespace": ""}
    
    if message_type == "enqueue":
        request = json.loads(payload.decode('utf-8'))
        queue = request.get('queue')
        message = request.get('message')
        
        # Send to queue
        # host.channels_send_to_queue(ctx, queue, json.dumps(message).encode())
        
        return json.dumps({'status': 'enqueued'}).encode('utf-8'), None
    
    elif message_type == "dequeue":
        request = json.loads(payload.decode('utf-8'))
        queue = request.get('queue')
        
        # Receive from queue
        # message_bytes = host.channels_receive_from_queue(ctx, queue, timeout_ms=5000)
        # message = json.loads(message_bytes.decode()) if message_bytes else None
        
        return json.dumps({'queue': queue, 'message': None}).encode('utf-8'), None
```

**See**: `examples/wasm_showcase/` for comprehensive service examples

## Language-Specific Guides

### Python

**Compiler**: `componentize-py`  
**WASM Size**: 30-40MB (includes Python runtime)  
**Best For**: ML, data processing, rapid prototyping

#### Setup

```bash
pip install componentize-py
```

#### Complete Example: Calculator Actor with GenServer

```python
# calculator_actor.py
"""
Calculator Actor - Demonstrates GenServer behavior (request-reply pattern).
See: examples/simple/wasm_calculator/actors/python/calculator_actor.py
"""

def handle_request(from_actor: str, message_type: str, payload: bytes) -> bytes:
    """
    GenServer behavior - request-reply pattern.
    Returns response synchronously.
    """
    import json
    
    try:
        request = json.loads(payload.decode('utf-8'))
        operation = request.get('operation', '')
        operands = request.get('operands', [])
        
        # Perform calculation
        if operation == 'add':
            result = sum(operands)
        elif operation == 'multiply':
            result = 1
            for op in operands:
                result *= op
        else:
            return json.dumps({'error': f'Unknown operation: {operation}'}).encode('utf-8')
        
        # Return result (required for GenServer)
        return json.dumps({'result': result, 'operation': operation}).encode('utf-8')
        
    except Exception as e:
        return json.dumps({'error': str(e)}).encode('utf-8')

def snapshot_state() -> bytes:
    """Snapshot state for checkpointing"""
    import json
    return json.dumps({'last_operation': None}).encode('utf-8')

# componentize-py expects an Actor class
class Actor:
    @staticmethod
    def handle_request(from_actor: str, message_type: str, payload: bytes) -> bytes:
        return handle_request(from_actor, message_type, payload)
    
    @staticmethod
    def snapshot_state() -> bytes:
        return snapshot_state()
```

#### Building

```bash
componentize-py calculator_actor.py \
    -o calculator_actor.wasm \
    --wit ../../../wit/plexspaces-actor/actor.wit
```

**See**: 
- `examples/simple/wasm_calculator/actors/python/calculator_actor.py` - GenServer example
- `examples/simple/wasm_calculator/actors/python/durable_calculator_actor.py` - Durable actor
- `examples/simple/wasm_calculator/actors/python/tuplespace_calculator_actor.py` - TupleSpace coordination

### TypeScript/JavaScript

**Compiler**: `javy`  
**WASM Size**: 500KB-2MB  
**Best For**: Web integration, rapid development

#### Setup

```bash
# Install Javy
cargo install javy-cli

# Or download from GitHub releases
curl -L https://github.com/bytecodealliance/javy/releases/download/v8.0.0/javy-arm-macos-v8.0.0.gz | gunzip > ~/.local/bin/javy
chmod +x ~/.local/bin/javy
```

#### Example Actor

```typescript
// greeter.ts
export function init(initialState: Uint8Array): void {
    // Initialize actor state
}

export function handleMessage(
    fromActor: string,
    messageType: string,
    payload: Uint8Array
): Uint8Array {
    if (messageType === "greet") {
        const name = new TextDecoder().decode(payload);
        const greeting = `Hello, ${name}!`;
        return new TextEncoder().encode(greeting);
    }
    return new TextEncoder().encode('{"error": "unknown message type"}');
}

export function snapshotState(): Uint8Array {
    // Serialize state for checkpointing
    return new TextEncoder().encode('{}');
}
```

#### Building

```bash
# Compile TypeScript to JavaScript
npx tsc greeter.ts --target ES2020 --module commonjs

# Compile JavaScript to WASM
javy compile greeter.js -o greeter.wasm
```

**See**: `examples/simple/polyglot_wasm_deployment/actors/typescript/` for complete TypeScript examples

### Rust

**Compiler**: `cargo` (wasm32-wasip2 target)  
**WASM Size**: 100KB-1MB  
**Best For**: Production, performance-critical

#### Setup

```bash
rustup target add wasm32-wasip2
```

#### Example Actor

```rust
// calculator_actor.rs
use std::collections::HashMap;

#[export_name = "init"]
pub extern "C" fn init(initial_state: *const u8, len: usize) {
    // Initialize actor state
}

#[export_name = "handle_message"]
pub extern "C" fn handle_message(
    from: *const u8, from_len: usize,
    msg_type: *const u8, msg_type_len: usize,
    payload: *const u8, payload_len: usize,
) -> *const u8 {
    // Handle incoming messages
    // Use WIT host functions via wasmtime::component::bindgen! macro
}

#[export_name = "snapshot_state"]
pub extern "C" fn snapshot_state() -> *const u8 {
    // Serialize state for checkpointing
}
```

#### Building

```bash
cargo build --target wasm32-wasip2 --release
```

**See**: `examples/simple/polyglot_wasm_deployment/actors/rust/` for complete Rust examples

### Go

**Compiler**: `tinygo`  
**WASM Size**: 2-5MB  
**Best For**: Good balance, fast iteration

#### Setup

```bash
# Install TinyGo
# macOS: brew install tinygo
# Linux: See https://tinygo.org/getting-started/install/
```

#### Example Actor

```go
// calculator_actor.go
package main

//export init
func init(initialState *byte, len uint32) {
    // Initialize actor state
}

//export handle_message
func handleMessage(
    from *byte, fromLen uint32,
    msgType *byte, msgTypeLen uint32,
    payload *byte, payloadLen uint32,
) *byte {
    // Handle incoming messages
    // Use WIT host functions via Go bindings
}

//export snapshot_state
func snapshotState() *byte {
    // Serialize state for checkpointing
}

func main() {}
```

#### Building

```bash
tinygo build -target=wasip2 -o calculator_actor.wasm calculator_actor.go
```

## Deployment

### HTTP Multipart Upload (Recommended)

For large WASM files (>5MB), use HTTP multipart upload:

```bash
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@calculator_actor.wasm"
```

### CLI Tool

For small WASM files (<5MB), use the CLI:

```bash
./target/release/plexspaces deploy \
  --node localhost:8000 \
  --app-id calculator-app \
  --name calculator \
  --version 1.0.0 \
  --wasm calculator_actor.wasm
```

**Note**: CLI automatically uses HTTP for files >5MB.

See [WASM Deployment Guide](wasm-deployment.md) for complete deployment instructions.

## Multi-Tenancy

All WIT host functions support multi-tenancy via the `context` parameter:

```python
# Python example
ctx = {"tenant-id": "tenant-123", "namespace": "production"}

# Use context in host function calls
# host.keyvalue_put(ctx, "key", "value")
# host.tuplespace_write(ctx, tuple_data)
```

**Best Practices**:
- Use empty strings for default/internal context
- Use explicit tenant/namespace for multi-tenant isolation
- All operations are scoped to the provided context

## Examples

### Complete Examples

1. **Python Calculator** (`examples/simple/wasm_calculator/`)
   - Python actors with durability and tuplespace
   - Demonstrates state persistence and coordination

2. **Polyglot Deployment** (`examples/simple/polyglot_wasm_deployment/`)
   - Multi-language actors (Rust, TypeScript, Go, Python)
   - Firecracker VM deployment

3. **WASM Showcase** (`examples/wasm_showcase/`)
   - Comprehensive WASM capabilities demonstration
   - All WIT interfaces in action

### Quick Start

```bash
# 1. Build Python actor
cd examples/simple/wasm_calculator
./scripts/build_python_actors.sh

# 2. Start node
cargo run --release --bin plexspaces -- start \
  --node-id test-node \
  --listen-addr 0.0.0.0:8000

# 3. Deploy actor
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=calculator-app" \
  -F "name=calculator" \
  -F "version=1.0.0" \
  -F "wasm_file=@wasm-modules/calculator_actor.wasm"
```

## Best Practices

### 1. Language Selection

- **Python**: Use for ML, data processing, rapid prototyping
- **TypeScript**: Use for web integration, rapid development
- **Rust**: Use for production, performance-critical applications
- **Go**: Use for good balance, fast iteration

### 2. WASM Size Optimization

- Use `wasm-opt` to reduce size:
  ```bash
  wasm-opt -Oz --strip-debug actor.wasm -o actor_opt.wasm
  ```
- Consider Rust/Go for smaller files
- Python WASM files are large (30-40MB) but acceptable for ML workloads

### 3. Error Handling

Always handle errors from WIT host functions:

```python
# Python example
try:
    result = host.tuplespace_read(ctx, pattern)
    if result is None:
        # Handle not found
        pass
except Exception as e:
    # Handle error
    pass
```

### 4. State Management

- Use `snapshot_state()` for checkpointing
- Use `init(initial_state)` for state recovery
- Use key-value store for persistent state
- Use durability host functions for fault tolerance

### 5. Testing

- Test locally before deployment
- Use integration tests with real WASM modules
- Verify WIT interface compatibility
- Test multi-tenant isolation

## Troubleshooting

### Build Issues

**Python (`componentize-py` not found)**:
```bash
pip install componentize-py
```

**TypeScript (`javy` not found)**:
```bash
cargo install javy-cli
# Or download from GitHub releases
```

**Rust (target not installed)**:
```bash
rustup target add wasm32-wasip2
```

**Go (`tinygo` not found)**:
```bash
# macOS
brew install tinygo
# Linux: See https://tinygo.org/getting-started/install/
```

### Runtime Issues

**"WASM module not found"**:
- Verify WASM file exists and is valid
- Check deployment logs for errors
- Ensure WIT interface matches

**"Host function not available"**:
- Verify WIT interface is imported correctly
- Check that host functions are registered in runtime
- Ensure context parameter is provided

**"Multi-tenant isolation error"**:
- Verify context parameter is provided
- Check tenant/namespace values
- Ensure operations are scoped correctly

## References

- [WASM Deployment Guide](wasm-deployment.md) - Complete deployment instructions
- [WIT Specification](https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md) - WebAssembly Interface Types
- [componentize-py](https://github.com/bytecodealliance/componentize-py) - Python to WASM compiler
- [Javy](https://github.com/bytecodealliance/javy) - JavaScript/TypeScript to WASM compiler
- [TinyGo](https://tinygo.org/) - Go to WASM compiler
- [wasmtime](https://docs.wasmtime.dev/) - WASM runtime documentation

