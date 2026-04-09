# Python WASM Actors for PlexSpaces

This directory contains Python actors compiled to WebAssembly Components using `componentize-py`.

## Prerequisites

### 1. Python 3.12+ Virtual Environment

```bash
# Create and activate virtual environment
python3.12 -m venv ~/venv
source ~/venv/bin/activate

# Install componentize-py
pip install componentize-py
```

**Note**: We recommend Python 3.12 for stability. Newer versions (3.14+) may have issues with pyo3 string conversion.

### 2. wasm-tools (optional, for inspection)

```bash
cargo install wasm-tools
```

## Architecture

Python WASM actors use the **WebAssembly Component Model** with **WIT (WebAssembly Interface Types)** for structured communication.

### Component Model Stack

```
┌─────────────────────────────────────────────────────────────┐
│                    PlexSpaces Runtime (Rust)                 │
├─────────────────────────────────────────────────────────────┤
│  WASM Component Host (wasmtime)                              │
│  ├── WASI Preview 2 Bindings                                 │
│  │   ├── wasi:cli/environment (read-only)                   │
│  │   ├── wasi:cli/exit                                       │
│  │   ├── wasi:io/streams                                     │
│  │   ├── wasi:clocks/*                                       │
│  │   └── wasi:random/*                                       │
│  └── PlexSpaces Host Functions                               │
│      ├── plexspaces:actor/host@0.1.0                 │
│      │   ├── send(to, msg_type, payload: bytes) -> result   │
│      │   ├── log(level, message)                             │
│      │   └── now_ms() -> u64                                 │
├─────────────────────────────────────────────────────────────┤
│  Python Component (componentize-py)                          │
│  └── plexspaces:actor/actor@0.1.0                    │
│      ├── init(config: bytes) -> result                       │
│      ├── handle(from, msg_type, payload: bytes) -> result    │
│      ├── get_state() -> result<bytes, actor-error>           │
│      └── set_state(state: bytes) -> result                   │
└─────────────────────────────────────────────────────────────┘
```

### WIT Interface (Actor World)

Located at `wit/plexspaces-actor/world.wit`:

```wit
package plexspaces:actor@0.1.0;

interface actor {
    init: func(config: list<u8>) -> result<_, actor-error>;
    handle: func(from-actor: string, msg-type: string, payload: list<u8>) -> result<list<u8>, actor-error>;
    get-state: func() -> result<list<u8>, actor-error>;
    set-state: func(state: list<u8>) -> result<_, actor-error>;
}

interface host {
    // Send message to another actor
    send: func(to: string, msg-type: string, payload: list<u8>) -> result<_, actor-error>;
    
    // Log message
    log: func(level: string, message: string);
    
    // Get current timestamp in milliseconds
    now-ms: func() -> u64;
}

world actor-world {
    import host;
    export actor;
}
```

**Key Design Choice**: The actor-world ABI is protobuf-first. Python SDK decorators and generated protobuf models own encode/decode so Python application code stays typed while the host contract stays consistent with Rust, Go, and TypeScript.

## Building Python Actors

### Step 1: Create Python Actor

```python
# my_actor.py
from wit_world import exports
from generated.ping_pb2 import PingRequest, PingResponse, ActorState

class Actor(exports.Actor):
    def __init__(self):
        self._state = ActorState()
    
    def init(self, config: bytes) -> None:
        """Initialize actor from protobuf config bytes."""
        if config:
            self._state.ParseFromString(config)
    
    def handle(self, from_actor: str, msg_type: str, payload: bytes) -> bytes:
        """Handle message with protobuf request/response types."""
        request = PingRequest()
        request.ParseFromString(payload)
        if msg_type == "ping":
            return PingResponse(response="pong").SerializeToString()
        raise ValueError(f"Unknown operation: {msg_type}")
    
    def get_state(self) -> bytes:
        """Get state as protobuf bytes."""
        return self._state.SerializeToString()
    
    def set_state(self, state: bytes) -> None:
        """Restore state from protobuf bytes."""
        self._state.ParseFromString(state)
```

### Step 2: Build Script

```bash
#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$PROJECT_ROOT/wit/plexspaces-actor"
ACTOR_NAME="my_actor"

source "$HOME/venv/bin/activate"
cd "$SCRIPT_DIR"

# Clean previous artifacts
rm -rf wit_world componentize_py_* poll_loop.py 2>/dev/null || true

# Generate bindings
componentize-py -d "$WIT_DIR" -w "actor-world" bindings .

# Build component
componentize-py -d "$WIT_DIR" -w "actor-world" componentize -o "${ACTOR_NAME}.wasm" "$ACTOR_NAME"

echo "✅ Built: ${ACTOR_NAME}.wasm"
```

## Key Learnings & Fixes

### 1. PyObject_SetItem Error Fix

**Problem**: componentize-py's pyo3 runtime crashes with `PyObject_SetItem` during component initialization when Python tries to call `os.putenv()`.

**Root Cause**: WASI doesn't support runtime environment variable modification. When we use `.inherit_env()` in WASI context, Python's runtime tries to sync its `os.environ` dict with the OS environment, which fails.

**Fix**: Don't inherit environment variables. Instead, explicitly set only required env vars:

```rust
// In instance.rs
let wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new()
    .inherit_stdio()
    // Don't use inherit_env() - causes PyObject_SetItem errors
    // Set minimal env vars Python needs
    .env("PYTHONDONTWRITEBYTECODE", "1")
    .env("PYTHONUNBUFFERED", "1")
    .env("HOME", "/")
    .env("PATH", "/")
    .build();
```

### 2. Use Generated Protobuf Models for Shared Contracts

**Problem**: Pushing application structs directly through Canonical ABI types makes polyglot parity harder and creates a second model alongside the Rust framework types.

**Solution**: Keep actor-world on `bytes` + `result`, compile the repo protos for Python, and let the SDK/decorators map protobuf models to the WIT boundary:

```wit
// Canonical actor-world boundary
handle: func(from-actor: string, msg-type: string, payload: list<u8>) -> result<list<u8>, actor-error>;
```

This keeps Python aligned with Rust, Go, and TypeScript and avoids ad hoc JSON schemas drifting away from the framework protos.

### 3. componentize-py Versions

| Version | Python | Status |
|---------|--------|--------|
| 0.19.3 | 3.14 | Works with our fixes |
| 0.17.2 | 3.12 | Works with our fixes |
| < 0.14 | varies | May have different issues |

Use the latest version (0.19.3) for best compatibility.

### 4. Resource Limits for Python

Python WASM components require more resources than Rust:

```rust
// In lib.rs defaults
max_memory_bytes: 64 * 1024 * 1024,  // 64MB (Python needs more)
max_stack_bytes: 8 * 1024 * 1024,     // 8MB
```

### 5. Python 3.14 WASM Memory Bugs (Critical)

The Python 3.14 runtime in componentize-py has memory management bugs that cause crashes.
These manifest as WASM trap errors with backtraces showing `*_dealloc` functions:

| Error | Symptom | Cause |
|-------|---------|-------|
| `match_dealloc` | Crash when using pattern matching or hashlib | `hashlib.md5()` and similar functions |
| `tuple_dealloc` | Crash when returning from functions | Complex return values, `json.dumps()` |
| `func_dealloc` | Crash during function cleanup | Helper function calls |

**Workarounds:**

```python
# ❌ CRASHES - hashlib causes match_dealloc
import hashlib
h = hashlib.md5((flag + user).encode()).hexdigest()

# ✅ WORKS - simple inline hash
h = 0
for c in (flag + user):
    h = (h + ord(c)) % 100

# ❌ MAY CRASH - json.dumps with complex nested data
return json.dumps({"status": "ok", "data": {"nested": "value"}})

# ✅ WORKS - string literal for simple responses
return '{"status":"ok"}'

# ❌ MAY CRASH - helper function call
def my_hash(s):
    return sum(ord(c) for c in s) % 100
h = my_hash(flag + user)

# ✅ WORKS - inline the logic
h = 0
for c in (flag + user):
    h = (h + ord(c)) % 100
```

**Best Practices for Stable Python WASM Actors:**

1. **Avoid hashlib entirely** - Use simple arithmetic hash functions
2. **Use string literals for simple JSON** - `'{"ok":true}'` instead of `json.dumps({"ok": True})`
3. **Inline calculations** - Don't extract logic into helper functions
4. **Flat control flow** - Avoid nested try-except blocks
5. **Simple return values** - Keep data structures flat and simple
6. **Use json.dumps() only for complex/dynamic data** - Not for static responses

See `apps/feature_flags/` for a working example with all workarounds applied.

## Testing

### Deploy and Test

```bash
# Start PlexSpaces node
cargo run -p plexspaces -- start --node-id test-node --listen-addr '0.0.0.0:8090'

# In another terminal, deploy Python actor
cargo run -p plexspaces-cli -- deploy \
    --node localhost:8090 \
    -i my-app \
    -n my-actor \
    -w examples/python/apps/my_actor/my_actor.wasm

# Send message (POST = fire-and-forget)
curl -X POST "http://localhost:8091/api/v1/actors/{namespace}/{actor-type}" \
    -H "Content-Type: application/json" \
    -d '{"operation": "ping"}'
```

### HTTP API Patterns

| Endpoint | Service | Description |
|--------|---------|-------------|
| `GET /api/v1/actors/{namespace}/{actor_type}` | `AskReply` | Request-reply, query params become payload |
| `GET /api/v1/actors/{namespace}/{actor_type}/ask` | `AskReply` | Request-reply |
| `POST /api/v1/actors/{namespace}/{actor_type}` | `SendMessage` | Fire-and-forget, body becomes payload |
| `PUT /api/v1/actors/{namespace}/{actor_type}` | `SendMessage` | Fire-and-forget, body becomes payload |
| `POST /api/v1/actors/{namespace}/{actor_type}/ask` | `AskReply` | Request-reply with request body |
| `PUT /api/v1/actors/{namespace}/{actor_type}/ask` | `AskReply` | Request-reply with request body |

## Examples

### SDK Examples (Recommended)

The [PlexSpaces Python SDK](../../docs/sdk.md) provides decorator-based actor development with minimal boilerplate:

| Example | Description | Features | Status |
|---------|-------------|----------|--------|
| `apps/bank_account/` | Bank account with durability | `@actor(facets=["durability"])`, state(), checkpoint via WasmConfig.durability_enabled | ✅ Complete (2.2) |
| `apps/payment_handler/` | Payment microservice | GenServer, KV idempotency, locks, `@gen_server_actor(facets=["durability"])` | ✅ Complete (2.5 microservices) |
| `apps/job_processing/` | Distributed job processing | TupleSpace scatter/gather, ts_write/ts_take/ts_read_all | ✅ Complete (2.5 MPI-like) |
| `apps/cdn_cache/` | Blob storage for assets | blob_upload/download/list/delete, MinIO/S3 | ✅ Complete (2.7) |
| `apps/leader_election/` | **Locks (2.8)** | host.lock_acquire/lock_release, leader election (cron singleton, task scheduler) | ✅ Complete |
| `apps/audit_log/` | Event-driven audit log | @event_actor, host.log | Pending (2.3) |
| `apps/feature_flags/` | Gradual rollout | KV, consistent hashing | Pending |
| `apps/fsm/` | Order workflow state machine | State transitions, @fsm_actor | Pending (2.6) |
| `apps/registry/` | Service discovery | RegistryFacet | Pending (2.4) |
| `apps/calculator/` | Simple math operations | @handler with multiple ops | Existing (not in reorg plan) |
| `apps/receipt_storage/` | Expense tracking | CRUD, filtering, aggregation | Pending |
| `apps/chat_room/` | Real-time chat | ProcessGroups, pub/sub | Pending |
| `apps/task-queue/` | Distributed task queue | LockFacet, heartbeats | Pending |
| `apps/nbody/` | Physics simulation | Multi-actor coordination | Pending |

**Durability facet (WASM):** Use `@actor(facets=["durability"])` in Python; enable checkpoint persistence via `durability_enabled: true` in release.yaml or app-config. See [Durability: Durability Facet Parameter](docs/durability.md#durability-facet-parameter-python-sdk).

**Quick Start:**
```bash
cd apps/bank_account
./build.sh           # Build WASM
./test.sh 8092       # Test (server must be running on port 8091)
```

## Troubleshooting

### "PyObject_SetItem" Error

Ensure you're NOT using `.inherit_env()` in the WASI context. This is fixed in the codebase.

### Component Not Found

Check that:
1. WIT interface matches what's in `wit/plexspaces-actor/`
2. Python class exports `Actor` implementing `exports.Actor`
3. All methods match WIT signatures exactly

### Large Component Size (30-40MB)

Normal for Python WASM. The component includes the CPython interpreter. Consider:
- Using Rust for performance-critical actors
- Caching compiled modules (already done by PlexSpaces)

## References

- [WebAssembly Component Model](https://component-model.bytecodealliance.org/)
- [componentize-py](https://github.com/bytecodealliance/componentize-py)
- [WASI Preview 2](https://github.com/WebAssembly/WASI/tree/main/preview2)
- [wasmtime](https://wasmtime.dev/)
