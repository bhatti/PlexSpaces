# Session Store Service - wasmCloud-style Capability-Based Design

Demonstrates **wasmCloud's capability-based security model** for distributed session management.

**Real-world use case**: Distributed session store (Redis Session Store, AWS ElastiCache) where sessions are stored in distributed KV, validated via external services, and cleaned up periodically.

## Architecture

```
┌─────────────┐
│ Web App     │
└──────┬──────┘
       │ POST /session/create
       │ GET  /session/{id}
       │ POST /session/{id}/refresh
       │ DELETE /session/{id}
       ▼
┌─────────────────────────────────┐
│  SessionStore Actor              │
│  ┌───────────────────────────┐ │
│  │ Session CRUD               │ │ ← KeyValue capability
│  │   - create()               │ │   (host.kv_get/put/delete)
│  │   - get()                  │ │
│  │   - refresh()              │ │
│  │   - delete()               │ │
│  └───────────────────────────┘ │
│  ┌───────────────────────────┐ │
│  │ User Validation           │ │ ← Inter-actor communication
│  │   - host.ask(auth_service) │ │   (simulates HTTP capability)
│  └───────────────────────────┘ │
│  ┌───────────────────────────┐ │
│  │ Cleanup Timer             │ │ ← Timer capability
│  │   - host.send_after()      │ │   (periodic cleanup)
│  │   - Every 60s             │ │
│  └───────────────────────────┘ │
└─────────────────────────────────┘
```

**wasmCloud capabilities showcased**:
1. **KeyValue Capability**: Distributed session storage (`session:{id}` → JSON)
2. **Timer Capability**: Periodic cleanup job (every 60s) removes expired sessions
3. **Inter-Actor Communication**: Validate sessions via `host.ask()` to auth service
   (simulates HTTP capability for calling external services)

## Quick Start

```bash
# Terminal 1: Start PlexSpaces node
PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:7992

# Terminal 2: Build and run
cd examples/python/apps/migrating_wasmcloud
./build.sh        # Builds session_store.wasm
./test.sh 7993    # Deploy + test sessions + benchmarks
```

## PlexSpaces SDK Features

| Feature | How Used |
|---------|----------|
| `@actor` | Marks `SessionStore` as a PlexSpaces actor |
| `state()` | Persistent state (stats, config, timer ID) |
| `@handler()` | Routes `create`, `get`, `refresh`, `delete`, `cleanup_expired`, `stats` |
| `@init_handler` | Initialize from framework config (child_spec args) |
| `host.kv_get()` / `host.kv_put()` / `host.kv_delete()` / `host.kv_list()` | **KeyValue capability** (wasmCloud-style) |
| `KeyValue` helper class | Convenient KV operations with auto JSON encoding/decoding |
| `host.ask()` | **Inter-actor communication** (simulates HTTP capability) |
| `host.send_after()` | **Timer capability** (periodic cleanup) |
| `host.now_ms()` | Timestamp for TTL checks |

## wasmCloud Capability-Based Design

### KeyValue Capability

Sessions are stored in distributed KV store:

```python
# Store session
session_key = f"session:{session_id}"
session_data = {
    "session_id": session_id,
    "user_id": user_id,
    "created_at_ms": now_ms,
    "expires_at_ms": expires_at_ms,
    "ttl_sec": ttl,
    "metadata": {}
}
host.kv_put(session_key, json.dumps(session_data))

# Retrieve session
session_json = host.kv_get(session_key)
session_data = json.loads(session_json)
```

**Benefits**:
- Distributed storage (shared across nodes)
- Persistent (survives actor restarts)
- Fast lookups (O(1) key access)

### Timer Capability

Periodic cleanup of expired sessions:

```python
# Start cleanup timer (every 60s)
delay_ms = cleanup_interval_sec * 1000
payload = json.dumps({"op": "cleanup_expired"})
timer_id = host.send_after(delay_ms, "cleanup_expired", payload)
```

**Benefits**:
- Automatic cleanup (no manual triggers)
- Efficient (batch processing)
- Reliable (timer fires even if actor deactivates)

### Inter-Actor Communication (Simulates HTTP Capability)

Validate sessions via inter-actor calls:

```python
# Simulate HTTP capability via host.ask()
response_json = host.ask(
    auth_service_actor,
    "validate_user",
    json.dumps({"user_id": user_id}),
    5000  # 5s timeout
)
response = json.loads(response_json)
valid = response.get("valid", False)
```

**Benefits**:
- Location-transparent (works for local/remote actors)
- Request-reply pattern (synchronous validation)
- Timeout handling (prevents hanging)

## Comparison: wasmCloud vs PlexSpaces

| Feature | wasmCloud | PlexSpaces |
|---------|-----------|------------|
| **Capability Model** | Explicit capability providers (HTTP, KeyValue, Timer) | Host functions (host.kv_*, host.send_after, host.ask) |
| **KeyValue** | KeyValue capability provider | `host.kv_get()` / `host.kv_put()` / `host.kv_delete()` |
| **HTTP Client** | HTTP capability provider | `host.ask()` to other actors (simulates HTTP) |
| **Timers** | Timer capability provider | `host.send_after()` for periodic tasks |
| **Security** | Capability-based (explicit grants) | Capability-based (host functions) |
| **Polyglot** | Any WASM language | Python, TypeScript, Go, Rust |
| **State** | External storage (KV, Blob) | External storage (KV) + actor state |

## Session Operations

### Create Session

```bash
curl -X POST "http://localhost:7993/api/v1/actors/session-store/session-store?timeout=5" \
  -H "Content-Type: application/json" \
  -d '{"op":"create","user_id":"user-123","ttl_sec":3600}'
```

Response:
```json
{
  "status": "ok",
  "session_id": "sess-1234567890-user-123",
  "expires_at_ms": 1234567890000,
  "ttl_sec": 3600
}
```

### Get Session

```bash
curl -X POST "http://localhost:7993/api/v1/actors/session-store/session-store?timeout=5" \
  -H "Content-Type: application/json" \
  -d '{"op":"get","session_id":"sess-1234567890-user-123"}'
```

### Refresh Session (Extend TTL)

```bash
curl -X POST "http://localhost:7993/api/v1/actors/session-store/session-store?timeout=5" \
  -H "Content-Type: application/json" \
  -d '{"op":"refresh","session_id":"sess-1234567890-user-123","extend_ttl_sec":3600}'
```

### Delete Session

```bash
curl -X POST "http://localhost:7993/api/v1/actors/session-store/session-store?timeout=5" \
  -H "Content-Type: application/json" \
  -d '{"op":"delete","session_id":"sess-1234567890-user-123"}'
```

### Get Statistics

```bash
curl -X POST "http://localhost:7993/api/v1/actors/session-store/session-store?timeout=5" \
  -H "Content-Type: application/json" \
  -d '{"op":"stats"}'
```

## Metrics & Benchmarks

The test script (`test.sh`) runs benchmarks showing:

### Coordination vs Computation

| Metric | Value | Description |
|--------|-------|-------------|
| **Coordination** | KV lookups, timer scheduling, inter-actor calls | Overhead from distributed operations |
| **Computation** | JSON serialization, TTL checks, session validation | Actual business logic |

### Performance Metrics

| Metric | Typical Value | Description |
|--------|---------------|-------------|
| **Create rate** | 500-1000 sessions/sec | Throughput for session creation |
| **Get rate** | 2000-5000 ops/sec | Throughput for session lookups |
| **Refresh rate** | 1000-2000 ops/sec | Throughput for TTL extension |
| **Cleanup** | 100-500 sessions/sec | Expired session cleanup rate |

### Benchmark Output Example

```
Step 4: Create 200 Sessions (wasmCloud KeyValue capability)
----------------------------------------------------------------
  Created: 200 sessions
  Failed:  0 sessions
  Time:    500ms
  Rate:    400 sessions/sec

Step 5: Access Sessions (1000 operations)
----------------------------------------------------------------
  Accessed:    900 sessions
  Not found:   80
  Expired:     20
  Time:        800ms
  Rate:        1250 ops/sec
```

## Files

| File | Description |
|------|-------------|
| `session_store.py` | SessionStore actor with wasmCloud-style capabilities |
| `app-config.toml` | ApplicationSpec (supervisor + session store) |
| `build.sh` | Build WASM module |
| `test.sh` | Deploy + test sessions + benchmarks |
| `native/session_store.py` | Native Python reference (optional) |

## Design Decisions

### Why Capability-Based Design?

**wasmCloud philosophy**: Actors request capabilities explicitly (HTTP, KeyValue, Timer). This provides:
- **Security**: No implicit access (secure by default)
- **Clarity**: Explicit dependencies (easy to understand)
- **Flexibility**: Swap implementations (Redis ↔ SQLite ↔ DynamoDB)

**PlexSpaces equivalent**: Host functions (`host.kv_*`, `host.send_after`, `host.ask`) provide the same capability-based model.

### Why External Storage (KV)?

Sessions are stored in KV (not actor state) because:
- **Durability**: Survives actor restarts
- **Distribution**: Shared across nodes
- **Scalability**: Handle millions of sessions
- **wasmCloud pattern**: External storage for large/durable data

### Why Timer-Based Cleanup?

Periodic cleanup (not on-demand) because:
- **Efficiency**: Batch processing (check 1000s at once)
- **Reliability**: Timer fires even if actor deactivates
- **wasmCloud pattern**: Timer capability for periodic tasks

## Capability Comparison: PlexSpaces vs wasmCloud

| Capability | wasmCloud | PlexSpaces | Status | Notes |
|------------|-----------|------------|--------|-------|
| **HTTP Client** | ✅ `wasi:http` | ⚠️ Simulated via `host.ask()` | Partial | Use `host.ask()` to call HTTP gateway actors |
| **HTTP Server** | ✅ `wasi:http` | ❌ Not available | Missing | Not implemented |
| **KeyValue Storage** | ✅ `wasi:keyvalue` | ✅ `host.kv_*` + `KeyValue` helper | ✅ Complete | Redis, SQLite, DynamoDB, PostgreSQL |
| **Blob Storage** | ✅ `wasi:blobstore` | ✅ `host.blob_*` | ✅ Complete | S3, Azure, GCP, MinIO, Filesystem |
| **Messaging** | ✅ `wasmcloud:messaging` | ✅ `host.send/ask` | ✅ Complete | Inter-actor messaging (Kafka/NATS via actors) |
| **Logging** | ✅ `wasi:logging` | ✅ `host.log` | ✅ Complete | Built-in logging |
| **Process Groups** | ❌ Not available | ✅ `host.pg_*` | ✅ Extra | Pub/sub coordination (Erlang pg2-style) |
| **TupleSpace** | ❌ Not available | ✅ `host.ts_*` | ✅ Extra | Linda-style coordination |
| **Distributed Locks** | ❌ Not available | ✅ `host.lock_*` | ✅ Extra | Distributed locking |
| **Timers** | ❌ Not available | ✅ `host.send_after()` | ✅ Extra | Delayed messaging |

**This Example Uses:**
- ✅ **KeyValue** - `KeyValue` helper class for session storage
- ✅ **Timer** - `host.send_after()` for periodic cleanup
- ⚠️ **HTTP** - Simulated via `host.ask()` for user validation

## References

- [wasmCloud Documentation](https://wasmcloud.dev/)
- [PlexSpaces WASM Support](../../../../docs/WASM_INTEGRATION.md)
- [PlexSpaces Python SDK](../../../../sdks/python/README.md)
- [SDK Improvements](./TODO.md) - Future enhancements and TODOs
