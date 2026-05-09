# PlexSpaces SDKs

PlexSpaces provides language-specific SDKs for building actors with minimal boilerplate. The SDKs are inspired by industry-leading frameworks like [Ray](https://docs.ray.io/en/latest/ray-core/api/doc/ray.remote.html), [Temporal](https://docs.temporal.io/), and [Orleans](https://learn.microsoft.com/en-us/dotnet/orleans/).

## SDK Architecture

The SDK is a **thin decorator layer** over the core framework crates. Core functionality -- actor registry, message routing, supervision, and state management -- lives in the main crates (`crates/behavior`, `crates/services`, `crates/actor`). The SDK simplifies the developer experience by removing boilerplate: decorators like `@actor` and `@handler` generate the WIT interface glue, state serialization, and message dispatch that you would otherwise write by hand. This means the SDK adds no new runtime capabilities; it is purely a developer ergonomics layer that compiles down to the same WIT exports the framework already expects.

**Design Principles**:
- **Core Functionality**: All business logic and framework capabilities are in main crates
- **SDK as Decorator**: SDK provides annotations and helpers to reduce boilerplate
- **WASM Support**: SDK provides WASM wrappers for integration without RPC
- **gRPC APIs**: Can be built separately for remote access
- **No Duplication**: SDK doesn't reimplement core functionality, only simplifies usage

Across languages, the authoring vocabulary is shared deliberately: `actor`, `gen_server_actor`, `event_actor`, `fsm_actor`, `workflow_actor`, plus `handler`, `init_handler`, `run_handler`, `signal_handler`, and `query_handler`. Rust exposes these as macros, Python and TypeScript as decorators, and Go as typed definition helpers.

## Available SDKs

| Language | Status | Location | Build Target |
|----------|--------|----------|--------------|
| **Python** | ✅ Available | `sdks/python/` | WASM actors (componentize-py) |
| **TypeScript** | ✅ Available | `sdks/typescript/` | WASM actors (jco componentize) |
| **Rust** | ✅ Available | `sdks/rust/plexspaces-sdk` | Native (embedded) actors; annotations + spawn / spawn_with_facets + facets |
| **Go** | ✅ Available | `sdks/go/` | WASM actors (TinyGo) |

### Proto generation and typed SDK models

Contracts live under `proto/`. **`make proto`** regenerates **Rust** (prost/tonic via `buf`) **and** polyglot SDK outputs (Python, TypeScript, Go). Use **`make proto-install-deps`** once to install local plugins (`betterproto` in a Python venv, `ts-proto`, `protoc-gen-go`). Set **`VENV_PATH`** if the venv is not `~/venv`.

| Target | Purpose |
|--------|---------|
| `make proto` | Rust + Python + TypeScript + Go (full pipeline) |
| `make proto-buf` | Rust only |
| `make proto-polyglot` | Python + TypeScript + Go only (`buf.gen.python.yaml`, `buf.gen.typescript.yaml`, `buf.gen.go.yaml`) |

Generated code is **checked in** (same idea as `crates/proto/src/generated/`): e.g. `sdks/python/plexspaces/generated/`, `sdks/typescript/src/generated/proto/`, `sdks/go/plexspaces/proto/`.

- **Python**: Optional betterproto models; `workflow` imports `RetryConfig` from generated modules when present, with a small dataclass fallback for WASM guests that do not bundle `generated/`.
- **TypeScript**: `src/proto.ts` re-exports selected proto types for callers; the WIT JSON boundary at runtime is unchanged.
- **Go**: `HostError.ParseErrorDetail()` parses structured JSON from host errors into `ErrorDetail` when the payload is JSON.

### Virtual actor type registration (SDK parity)

For native Rust, **`spawn_with_facets`** calls **`register_virtual_actor_type_consistent`** so virtual actor metadata (including all facet configs) is registered for reactivation. WASM application deploy does the same from `app-config.toml` / child specs. **`VirtualActorManager::get_virtual_actor_type`** returns type-level metadata; it **persists** when an instance is deactivated (vacation) and is removed when the application is **undeployed**.

### Event actors as the channel abstraction

For application code, prefer **`event_actor`** as the primary abstraction for channel-style workloads:

- Use `event_actor` plus `handler(..., "cast")` for fire-and-forget event handling.
- Use directed messaging for point-to-point delivery: `host.send(...)` in Python and TypeScript, `host.Send(...)` in Go, and the native actor APIs in Rust.
- Use process-group broadcast for publish-style fan-out: `host.process_groups.broadcast(...)` in Python, `host.processGroups.broadcast(...)` in TypeScript, and `host.PG().Broadcast(...)` in Go.
- Keep queue/topic implementation details in framework services and host bindings. SDKs should present event-oriented APIs rather than separate business-logic implementations of channels.

### Cross-SDK consistency: TupleSpace and Process Groups

All language SDKs expose the same semantics for TupleSpace and process groups so that examples and docs can be translated 1:1.

**TupleSpace (list-in, list-out)** — Use a high-level helper so callers pass native lists and get native lists back; use `null`/`nil` in patterns for wildcards. Low-level string APIs remain for advanced use.

| Operation | Python | TypeScript | Go |
|-----------|--------|------------|-----|
| Write tuple | `host.ts.write([a, b, c])` | `host.ts.write([a, b, c])` | `host.TS().Write([]any{a, b, c})` |
| Take (destructive) | `host.ts.take(pattern)` → list or None | `host.ts.take(pattern)` → array or null | `host.TS().Take(pattern)` → ([]any, bool) |
| Read (non-destructive) | `host.ts.read(pattern)` → list or None | `host.ts.read(pattern)` → array or null | `host.TS().Read(pattern)` → ([]any, bool) |
| Read all | `host.ts.read_all(pattern)` → list of lists | `host.ts.readAll(pattern)` → array[] | `host.TS().ReadAll(pattern)` → [][]any |
| Wildcards | `None` in pattern | `null` in pattern | `nil` in pattern |

**Process group broadcast** — The host uses the `msg_type` argument for routing. Payload can be data-only (no need to put `op` or `msg_type` inside the payload).

| Language | API |
|----------|-----|
| Python | `host.process_groups.broadcast(group, "tasks_ready", {"ensemble_id": "e1", "num_tasks": 10})` |
| TypeScript | `host.processGroups.broadcast(group, "tasks_ready", { ensembleId: "e1", numTasks: 10 })` |
| Go | `host.PG().Broadcast(group, "tasks_ready", map[string]any{"ensemble_id": "e1", "num_tasks": 10})` |

**Rust**: Native (embedded) actors use `ActorContext::get_tuplespace()` and process group services from the node; WASM Rust actors use the same WIT host interface via `simple_actor::pg_first`.

### Cross-SDK consistency: Tier 1 Ergonomics Helpers

All language SDKs expose the following convenience helpers so that common patterns — finding a service actor, reading/writing structured KV data, and recording metrics — require no boilerplate.

#### `PG.First` / `pg_first` / `processGroups.first` / `first`

Return the first member of a named process group, or an error/null/None when the group is empty.

| Language | API |
|----------|-----|
| Python | `host.process_groups.first("svc:llm_router")` → `str \| None` |
| Python (strict) | `host.process_groups.first_or_raise("svc:llm_router")` → `str` or raises `RuntimeError` |
| TypeScript | `host.processGroups.first("svc:llm_router")` → `string \| null` |
| TypeScript (strict) | `host.processGroups.firstOrThrow("svc:llm_router")` → `string` |
| Go | `host.PG().First("svc:llm_router")` → `(string, error)` |
| Rust WASM | `pg_first("svc:llm_router")` → `Result<String, String>` |

```python
# Python — route to the first available LLM router
router_id = host.process_groups.first_or_raise("svc:llm_router")
host.send(router_id, "route", json.dumps({"prompt": prompt}))
```

#### `KVGetJSON` / `kv_get_json` / `kvGetJson`

Retrieve a key from the KV store and deserialize it from JSON. Returns `None`/`null`/`Ok(None)` when the key is missing or the stored value is corrupt JSON.

| Language | API |
|----------|-----|
| Python | `host.kv_get_json(key)` → `Any \| None` |
| TypeScript | `host.kvGetJson<T>(key)` → `T \| null` |
| Go | `host.KVGetJSON(key, &dest)` → `(bool, error)` |
| Rust WASM | `kv_get_json::<T>(key)` → `Result<Option<T>, String>` |

#### `KVPutJSON` / `kv_put_json` / `kvPutJson`

Serialize a value to JSON and store it under `key`. Raises/throws/returns an error on write failure.

| Language | API |
|----------|-----|
| Python | `host.kv_put_json(key, value)` |
| TypeScript | `host.kvPutJson(key, value)` |
| Go | `host.KVPutJSON(key, value)` → `error` |
| Rust WASM | `kv_put_json(key, &value)` → `Result<(), String>` |

```go
// Go — store and retrieve a typed task record
type Task struct { Seq int `json:"seq"`; Kind string `json:"kind"` }
host.KVPutJSON("task:pending:1", Task{Seq: 1, Kind: "summarize"})
var t Task
ok, _ := host.KVGetJSON("task:pending:1", &t)
```

#### `IncrCounter` / `incr_counter` / `incrCounter`

Increment a single named application metric counter by 1. Errors are swallowed (metrics must not crash actors).

| Language | API |
|----------|-----|
| Python | `host.incr_counter(application_id, name)` |
| TypeScript | `host.incrCounter(applicationId, name)` |
| Go | `b.IncrCounter(host, name)` (on `*BaseActor`) |
| Rust WASM | `incr_counter(application_id, name)` |

#### `IncrCounters` / `incr_counters` / `incrCounters`

Increment one or more named counters in a single host call. `message_count` is set to the number of distinct counter names.

| Language | API |
|----------|-----|
| Python | `host.incr_counters(application_id, {"cache_hits": 5, "cache_misses": 2})` |
| TypeScript | `host.incrCounters(applicationId, { cacheHits: 5, cacheMisses: 2 })` |
| Go | `b.IncrCounters(host, map[string]int{"cache_hits": 5, "cache_misses": 2})` |
| Rust WASM | `incr_counters(application_id, &[("cache_hits", 5), ("cache_misses", 2)])` |

### Cross-SDK consistency: Channel

`Channel` is the host-provided queue and pub/sub primitive. It exposes two patterns over the same named channel:

- **Queue (point-to-point)** — `Send` / `Receive` / `Ack` / `Nack`. One consumer receives each message; unacked messages are redelivered.
- **Pub/sub (fan-out)** — `Publish` / `Subscribe` / `Unsubscribe`. All active subscribers receive each message.

The `ctx` parameter is a JSON string that carries the tenant/namespace context for isolation.

| Operation | Python | Go |
|-----------|--------|----|
| Enqueue | `host.channel.send(ctx, name, msg_type, payload)` → `str` | `host.Ch().Send(ctx, name, msgType, payload)` → `(string, error)` |
| Dequeue | `host.channel.receive(ctx, name, timeout_ms)` → `dict \| None` | `host.Ch().Receive(ctx, name, timeoutMs)` → `(map, bool, error)` |
| Ack | `host.channel.ack(ctx, name, msg_id)` | `host.Ch().Ack(ctx, name, msgID)` |
| Nack | `host.channel.nack(ctx, name, msg_id, requeue)` | `host.Ch().Nack(ctx, name, msgID, requeue)` |
| Publish | `host.channel.publish(ctx, name, msg_type, payload)` → `str` | `host.Ch().Publish(ctx, name, msgType, payload)` → `(string, error)` |
| Subscribe | `host.channel.subscribe(ctx, name, filter)` → `str` | `host.Ch().Subscribe(ctx, name, filter)` → `(string, error)` |
| Unsubscribe | `host.channel.unsubscribe(subscription_id)` | `host.Ch().Unsubscribe(subscriptionID)` |
| Depth | `host.channel.depth(ctx, name)` → `int` | `host.Ch().Depth(ctx, name)` → `(uint64, error)` |

```go
// Go — task queue producer / consumer pattern
ctx := `{"tenant_id":"t1","namespace":"app"}`

// Producer: enqueue a task
msgID, err := host.Ch().Send(ctx, "tasks:analyze", "analyze", map[string]any{
    "doc_id": "d42",
    "model":  "summarizer",
})

// Consumer: dequeue, process, ack
msg, ok, err := host.Ch().Receive(ctx, "tasks:analyze", 5000)
if ok {
    // process msg["payload"] …
    _ = host.Ch().Ack(ctx, "tasks:analyze", msg["id"].(string))
}
```

```python
# Python — pub/sub fan-out to multiple subscribers
ctx = '{"tenant_id":"t1","namespace":"app"}'
host.channel.publish(ctx, "events:agent", "agent_chat", {"session": sid})
```

Channels are auto-created on first use; explicit `Create` is only needed to set capacity or TTL limits.

### Cross-SDK consistency: EventLog

`EventLog` is a monotonic, append-only, two-cursor log backed by the KV store. Embed it in actor state (it holds only a `watermark` integer) so it survives restarts. Multiple independent consumers each track their own read cursor in KV under `<prefix>cursor:<consumer_id>`.

**Append** — increments `watermark`, writes the entry as JSON to `<prefix>seq:<watermark>`. Rolls back `watermark` on KV write failure.

**Poll** — reads entries from `(cursor+1)..watermark` for a named consumer up to `limit`, then persists the new cursor. Returns `(events, new_cursor)`. Idempotent: a second call with the same consumer returns nothing new.

| Language | Embed | Append | Poll |
|----------|-------|--------|------|
| Python | `self.log = EventLog()` | `seq = self.log.append(host, "audit:", entry)` | `events, cur = self.log.poll(host, "audit:", "c1", limit=20)` |
| TypeScript | `log = new EventLog()` | `seq = this.log.append(host, "audit:", entry)` | `[events, cur] = this.log.poll(host, "audit:", "c1", 20)` |
| Go | `Log EventLog` (in state struct) | `seq, err := s.Log.Append(host, "audit:", entry)` | `events, cur, err := s.Log.Poll(host, "audit:", "c1", 20)` |
| Rust WASM | `log: EventLog` (in state struct, derive Serialize) | `let seq = state.log.append("audit:", &entry)?;` | `let (events, cur) = state.log.poll("audit:", "consumer-1", 20)?;` |

```python
# Python — audit log with two independent consumers
from plexspaces.host import EventLog

class MyActor:
    log: EventLog = state(default_factory=EventLog)

    @handler("record")
    def record(self, event: dict) -> dict:
        seq = self.log.append(host, "audit:", event)
        return {"seq": seq}

    @handler("poll_audit")
    def poll_audit(self, consumer_id: str) -> dict:
        events, cursor = self.log.poll(host, "audit:", consumer_id, limit=50)
        return {"events": events, "cursor": cursor}
```

```rust
// Rust WASM — EventLog embedded in actor state
use plexspaces_sdk::simple_actor::{EventLog, kv_put_json, kv_get_json};

#[derive(Serialize, Deserialize, Default)]
struct MyState {
    log: EventLog,
}

// append
let seq = state.log.append("audit:", &entry)?;

// poll (returns all new events since last call for this consumer)
let (events, new_cursor) = state.log.poll::<serde_json::Value>("audit:", "consumer-1", 20)?;
```

## Python SDK

### Installation

```bash
# From source (development)
cd sdks/python
pip install -e ".[dev]"

# Required for building WASM
pip install componentize-py
```

### Quick Start

**1. Write your actor (no boilerplate!)**

```python
# bank_account.py
from plexspaces import actor, state, handler

@actor
class BankAccount:
    balance: int = state(default=0)
    account_id: str = state(default="")
    
    @handler("deposit")
    def deposit(self, amount: int) -> dict:
        self.balance += amount
        return {"balance": self.balance}
    
    @handler("withdraw")
    def withdraw(self, amount: int) -> dict:
        if amount > self.balance:
            return {"error": "insufficient_funds"}
        self.balance -= amount
        return {"balance": self.balance}
    
    @handler("balance", "get")
    def get_balance(self) -> dict:
        return {"balance": self.balance}
```

**2. Build to WASM**

```bash
plexspaces-py build bank_account.py -o bank_account_actor.wasm
```

**3. Deploy**

```bash
curl -X POST http://localhost:8094/api/v1/deploy \
  -F "namespace=default" \
  -F "actor_type=bank_account" \
  -F "wasm=@bank_account_actor.wasm"
```

### Actor ID Format

All actors use the standardized format: `{id}//{actor_type}::{namespace}@{node_id}`
Client code should usually pass only the actor name to builder and SDK spawn helpers; the runtime constructs this canonical ID.

**Components**:
- `id`: Base actor identifier (can be ULID, client-provided, or empty)
- `actor_type`: Actor type from proto (required, e.g., "read-state-tracker", "GenServer")
- `namespace`: Optional namespace for multi-tenancy (required for WASM deployment)
- `node_id`: Node identifier (required)

**Delimiters**:
- `//`: Separates base ID from actor_type
- `::`: Separates actor_type from namespace
- `@`: Separates namespace from node_id

**Examples**:
- `user-123//read-state-tracker::orbit-read-state-ts@node-1` (full format)
- `account-alice//account::default@node-abc123`

**Internal Structured Form**:
```rust
use plexspaces_actor::ActorId;

let actor_id = ActorId::new(
    "user-123",
    "read-state-tracker",
    "orbit-read-state-ts",
    "node-1",
)?;
```

Client code should usually provide only the actor name or logical key and let the framework build the structured actor ID. The canonical string form is primarily for storage, routing boundaries, observability, and inter-node APIs.

### API Reference

#### Decorators

| Decorator | Description | Example |
|-----------|-------------|---------|
| `@actor` | Define a PlexSpaces actor class (GenServer) | `@actor class MyActor:` |
| `@actor(facets=[...])` | Actor with facet declaration | `@actor(facets=["durability"]) class DurableActor:` |
| `@event_actor` | Event-handler (GenEvent): fire-and-forget, no request-reply; ideal for channel-style consumers | `@event_actor class AuditLog:` |
| `@fsm_actor` | FSM actor (GenStateMachine): stateful transitions | `@fsm_actor class OrderFSM:` |
| `@fsm_actor(states=[...], initial="...")` | FSM actor with explicit state list and initial state | `@fsm_actor(states=["idle","running","done"], initial="idle") class OrderFSM:` |
| `@gen_server_actor` | Explicit GenServer (same as `@actor`) | `@gen_server_actor class Worker:` |
| `@workflow_actor` | Workflow/orchestration actor | `@workflow_actor class Pipeline:` |
| `@handler(*msg_types)` | Route messages to this method | `@handler("deposit")` |
| `state(default=None, default_factory=None)` | Define persistent state field | `balance: int = state(default=0)` |
| `@init_handler` | Custom initialization handler | `@init_handler def on_init(self, config):` |
| `@run_handler` / `@signal_handler` / `@query_handler` | Workflow run, signal, and query entrypoints | `@signal_handler("cancel")` |

#### Behavior Types

All behavior decorators support an optional `facets` parameter:

| Decorator | Behavior | Use Case | Invocation |
|-----------|----------|----------|------------|
| `@actor` | GenServer | Request-reply actors (default) | Auto `call` |
| `@gen_server_actor` | GenServer | Explicit GenServer | Auto `call` |
| `@event_actor` | GenEvent | Fire-and-forget event handlers | `cast` |
| `@fsm_actor` | GenStateMachine | State machine workflows | Auto `call` |
| `@workflow_actor` | Workflow | Long-running orchestrations | Auto `call` |

**GenServer Auto-Invocation**: When using `@actor` or `@gen_server_actor`, request-reply handlers use call semantics automatically. You don't need to specify that explicitly:

```python
@gen_server_actor
class PaymentHandler:
    @handler("process_payment")  # Automatically uses call semantics
    def process_payment(self, amount: int) -> dict:
        return {"status": "ok", "amount": amount}
```

#### Facets

Facets declare what capabilities an actor expects. Use the `facets` parameter on any behavior decorator:

```python
@actor(facets=["durability"])
class DurableAccount:
    balance: int = state(default=0)

@fsm_actor(states=["idle", "processing", "done", "error"], initial="idle",
           facets=["durability", "registry"])
class OrderWorkflow:
    fsm_state: str = state(default="idle")  # auto-initialised from initial= if omitted
```

| Facet | WASM Behavior | Rust Behavior |
|-------|---------------|---------------|
| `durability` | `DurabilityFacet` with `get_state` / `set_state` adapter | `DurabilityFacet` attachment |
| `registry` | Service discovery via `RegistryFacet` in app-config | `RegistryFacet` attachment |

**WASM Durability**: WASM actors use the same `DurabilityFacet` lifecycle as native actors. The runtime automatically bridges the facet to the behavior's `get_state()` / `set_state()` implementation, so durable stop/reactivate restores the last checkpoint while non-durable stop/reactivate rebuilds from init-config. For full details, see [Durability: WASM Actor Durability](durability.md#wasm-actor-durability).

#### State Persistence

Fields decorated with `state()` are automatically:
- Serialized in `get_state()` (for checkpointing)
- Restored in `set_state()` (after restart)
- Persisted across actor restarts

```python
@actor
class Counter:
    count: int = state(default=0)           # Immutable default
    history: list = state(default_factory=list)  # Mutable default (use factory)
```

**State Serialization Safety (WASM JSON):** The SDK automatically handles float-to-string conversion for WASM JSON safety. When state is serialized via `get_state()`, the internal `_sanitize_payload_for_wasm` function converts float values to string representations to avoid JSON precision issues in WASM runtimes. When state is restored via `set_state()`, the `_desanitize_from_wasm` function converts them back. This is fully transparent to user code -- you read and write normal Python floats, and the SDK handles the round-trip safety automatically.

#### Message Handlers

Use `@handler()` to route messages to methods:

```python
@actor
class Calculator:
    @handler("add")
    def add(self, a: int, b: int) -> dict:
        return {"result": a + b}
    
    @handler("sub", "subtract")  # Multiple message types
    def subtract(self, a: int, b: int) -> dict:
        return {"result": a - b}
```

#### Host Functions

Access PlexSpaces capabilities via `host`:

```python
from plexspaces import actor, handler, host

@actor
class ChatRoom:
    @handler("send")
    def send_message(self, text: str) -> dict:
        # Log message
        host.info(f"Sending: {text}")
        
        # Broadcast to group
        host.process_groups.publish("chat-room", {"text": text})
        
        return {"status": "sent"}
```

| Function | Description |
|----------|-------------|
| **Messaging** | |
| `host.send(to, msg_type, payload)` | Send message to another actor (fire-and-forget) |
| `host.ask(to, msg_type, payload, timeout_ms)` | Request-reply: send and wait for response. Raises RuntimeError on timeout/error. |
| **Actor Identity** | |
| `host.self_id()` | Get own actor ID (e.g., `"account-alice"`) |
| **Actor Lifecycle** | |
| `host.spawn(module_ref, actor_id, init_config)` | Spawn a new actor. Returns spawned actor ID (auto-generated ULID if actor_id is empty). |
| `host.stop(actor_id)` | Stop an actor gracefully |
| **Linking & Monitoring (Erlang/OTP)** | |
| `host.link(actor_id)` | Bidirectional link: if either actor crashes, the other is notified |
| `host.unlink(actor_id)` | Remove a bidirectional link |
| `host.monitor(actor_id)` | Unidirectional monitor: receive DOWN notification when target exits. Returns monitor ref. |
| `host.demonitor(monitor_ref)` | Cancel a monitor |
| **Timers** | |
| `host.send_after(delay_ms, msg_type, payload)` | Send message to self after delay. Returns timer-id for tracking. |
| **Logging & Time** | |
| `host.log(level, message)` | Log a message |
| `host.info(message)` | Log info message |
| `host.debug(message)` | Log debug message |
| `host.warn(message)` | Log warning message |
| `host.error(message)` | Log error message |
| `host.now_ms()` | Get current timestamp (ms) |
| **Key-Value Storage** | |
| `host.kv_get(key)` | Get value bytes by key. SDKs decode protobuf or app-owned payloads from the returned bytes. |
| `host.kv_put(key, value)` | Store key-value bytes. SDKs typically encode/decode protobuf messages for shared models. |
| `host.kv_delete(key)` | Delete key. Returns success/error from the actor-world result. |
| `host.kv_list(prefix)` | List keys with prefix. Returns string keys. |
| **TupleSpace** | |
| `host.ts.write(tuple)` | Write a tuple using protobuf `WriteRequest` / SDK tuple helpers. |
| `host.ts.read(pattern)` | Read one match (non-destructive) using protobuf `ReadRequest`. |
| `host.ts.take(pattern)` | Take one match (destructive) using protobuf `ReadRequest`. |
| `host.ts.read_all(pattern)` | Read all matches using protobuf tuple/pattern models. |
| **Distributed Locks** | |
| `host.lock_acquire(tenant_id, namespace, holder_id, lock_name, lease_secs, timeout_ms)` | Acquire lock. Returns the shared lock protobuf model. |
| `host.lock_release(lock_id, tenant_id, namespace, holder_id, lock_version)` | Release lock. Returns empty on success. |
| `host.lock_renew(lock_id, tenant_id, namespace, holder_id, lock_version, lease_secs)` | Renew lock lease. Returns the shared lock protobuf model. |
| **Blob Storage** | |
| `host.blob_upload(path, data, content_type)` | Upload blob bytes. Returns the stored blob id. |
| `host.blob_download(path)` | Download blob bytes. |
| `host.blob_delete(path)` | Delete blob. Returns success/error from the actor-world result. |
| `host.blob_list(prefix)` | List blob ids by prefix. |
| **Key-Value JSON helpers** | |
| `host.kv_get_json(key)` | Deserialize a JSON value from KV. Returns `None` if missing or corrupt. |
| `host.kv_put_json(key, value)` | Serialize a value to JSON and store it. Raises on write failure. |
| **Metrics helpers** | |
| `host.incr_counter(application_id, name)` | Increment a named counter by 1. Errors are swallowed. |
| `host.incr_counters(application_id, counters)` | Increment multiple counters: `{"cache_hits": 5, "cache_misses": 2}`. Errors are swallowed. |
| **Process Groups** | |
| `host.process_groups.join(group)` | Join a process group (uses self actor ID) |
| `host.process_groups.leave(group)` | Leave a process group |
| `host.process_groups.broadcast(group, msg_type, payload)` | Broadcast to all group members; `msg_type` is used for routing so payload can be data-only. |
| `host.process_groups.members(group)` | Get group member IDs |
| `host.process_groups.first(group)` | Return the first member of the group, or `None` if empty. |
| `host.process_groups.first_or_raise(group)` | Return the first member; raise `RuntimeError` if empty. |
| **EventLog** | |
| `EventLog()` | Monotonic append-only log backed by KV. Embed in actor state. |
| `log.append(host, prefix, entry)` | Append JSON entry; returns sequence number. Rolls back on KV failure. |
| `log.poll(host, prefix, consumer_id, limit)` | Return up to `limit` new events for a consumer; advances the consumer cursor. |
| **Channel (queue + pub/sub)** | |
| `host.channel.send(ctx, name, msg_type, payload)` | Enqueue (queue semantics). Returns message ID string. |
| `host.channel.send_with_options(ctx, name, msg_type, payload, delay_ms, ttl_ms, headers)` | Enqueue with delay, TTL, and custom headers. |
| `host.channel.receive(ctx, name, timeout_ms)` | Dequeue one message. Returns `dict` or `None` on timeout. |
| `host.channel.ack(ctx, name, msg_id)` | Acknowledge successful processing; prevents redelivery. |
| `host.channel.nack(ctx, name, msg_id, requeue)` | Negative-acknowledge; `requeue=True` retries, `False` dead-letters. |
| `host.channel.publish(ctx, name, msg_type, payload)` | Publish (pub/sub — all subscribers). Returns message ID. |
| `host.channel.subscribe(ctx, name, filter)` | Subscribe; empty filter matches all. Returns subscription ID. |
| `host.channel.unsubscribe(subscription_id)` | Cancel a subscription. |
| `host.channel.depth(ctx, name)` | Return count of pending (unacked) messages. |
| **Elastic pool** | |
| `host.pool_checkout(pool_name, timeout_ms)` (Python) / `host.PoolCheckout` (Go) / `host.poolCheckout` (TS) | Checkout an actor from a named pool. Returns the shared `ActorHandle` protobuf model. |
| `host.pool_checkin(pool_name, actor_id, checkout_id, healthy)` | Checkin an actor to the pool. Use values from the handle returned by checkout. |
| `host.pool_get_metrics(pool_name)` | Get pool metrics as the shared `PoolMetrics` protobuf model. |
| **ShardGroup / Application Metrics** | |
| `host.create_shard_group(request)` | Create a shard group. Request uses proto field names such as `group_id`, `actor_type`, `shard_count`, and `placement`. |
| `host.bulk_update_shard_group(request)` | Bulk update shards. Request uses proto field names such as `group_id`, `updates`, `consistency_level`, `timeout_ms`, and `wait_for_responses`. |
| `host.map_shard_group(request)` | Map a query across shards. Request uses proto field names such as `group_id`, `query`, and `timeout_ms`. |
| `host.scatter_gather(request)` | Scatter/gather across shards. Request uses proto field names such as `group_id`, `query`, `aggregation`, `min_responses`, and `timeout_ms`. |
| `host.application_metrics_add(application_id, metrics)` | Merge a node-local application metrics delta using proto field names. |
| `host.application_get_status(application_id, node_id)` | Get application status and per-node metrics for a participating node. |

**Example**: [Parameter sweep (migrating_merlin)](../examples/python/apps/migrating_merlin/README.md) uses the pool API with tuple space (work queue) and fallback to process group; available in Python, Go, TypeScript, and Rust.

#### Ask Pattern (Request-Reply Between WASM Actors)

The `host.ask(to, msg_type, payload, timeout_ms)` function enables synchronous request-reply communication between WASM actors. Unlike `host.send()` (fire-and-forget), `host.ask()` blocks the caller until a response is received or the timeout expires.

```python
import json
from plexspaces import actor, handler, host, state

@actor
class TrainingWorker:
    worker_id: str = state(default="")

    @handler("train_step")
    def train_step(self) -> dict:
        # Request current weights from the parameter server
        result = host.ask(
            "parameter-server:ml-training",  # target actor (name:namespace)
            "get_weights",                    # message type
            json.dumps({"worker_id": self.worker_id}),  # payload
            5000                              # timeout in milliseconds
        )
        weights = json.loads(result)

        # ... perform training step with weights ...

        # Push gradient update (fire-and-forget)
        host.send("parameter-server:ml-training", "push_gradient",
                   json.dumps({"worker_id": self.worker_id, "gradient": [0.1, -0.2]}))

        return {"status": "step_complete"}
```

**Behavior:**
- Returns the response payload as a string on success.
- Raises `RuntimeError` if the target actor does not respond within `timeout_ms`.
- Raises `RuntimeError` if the target actor returns an error.
- The caller actor is blocked during the ask; other messages to it are queued.

#### Key-Value Storage (WASM)

WASM actors can persist data via **`host.kv_get`** and **`host.kv_put`**. Keys are scoped per actor. The node provides an in-memory keyvalue store for WASM by default. Use this for sensor buffers, caches, or any key-value state without relying on in-actor state serialization. Full keyvalue API (TTL, list-keys, etc.) will be added to the SDKs later. See [WASM Deployment: Key-Value Storage](wasm-deployment.md#key-value-storage-wasm).

#### TupleSpace (WASM)

WASM actors use **`host.ts`** for list-in, list-out TupleSpace coordination. Use `None` in patterns for wildcards:

```python
from plexspaces import actor, handler, host

@actor
class JobCoordinator:
    @handler("submit")
    def submit_job(self, job_id: str, tasks: list) -> dict:
        # Scatter: host.ts.write accepts Python list
        for i, task in enumerate(tasks):
            host.ts.write(["job", job_id, "task", i, task])
        return {"job_id": job_id, "tasks": len(tasks)}
    
    @handler("claim")
    def claim_task(self, job_id: str) -> dict:
        # Take one match; returns list or None
        t = host.ts.take(["job", job_id, "task", None, None])
        if t:
            return {"task_id": t[3], "data": t[4]}
        return {"task": None}
```

#### Blob Storage (WASM)

WASM actors can store binary data (images, documents, etc.) via S3-compatible blob storage:

```python
import base64
from plexspaces import actor, handler, host

@actor
class CdnCache:
    @handler("upload")
    def upload(self, path: str, data: str, content_type: str) -> dict:
        result = host.blob_upload(path, data, content_type)
        if result and result.startswith("ERROR"):
            return {"error": result}
        return {"status": "uploaded", "path": path}
    
    @handler("download")
    def download(self, path: str) -> dict:
        data = host.blob_download(path)
        if not data:
            return {"error": "not_found"}
        return {"data": data}  # base64-encoded
```

#### Distributed Locks (WASM)

WASM actors can use distributed locks for critical sections:

```python
from plexspaces import gen_server_actor, handler, host

@gen_server_actor
class PaymentHandler:
    @handler("refund")
    def process_refund(self, tx_id: str, amount: int) -> dict:
        lock_version = host.lock_acquire(f"refund:{tx_id}", 5000)
        if not lock_version or lock_version.startswith("ERROR"):
            return {"error": "lock_failed"}
        try:
            # Critical section - only one refund at a time
            return {"status": "refunded", "amount": amount}
        finally:
            host.lock_release(f"refund:{tx_id}", lock_version)
```

### Local Blob Storage

The checked-in `release.yaml` used by [`scripts/server.sh`](/Users/shahzadbhatti/workspace/myspaces/scripts/server.sh) now uses the built-in local blob backend, so SDK examples can exercise blob APIs without extra services:

```yaml
runtime:
  blob:
    backend: local
    bucket: plexspaces-blobs
    endpoint: ""
    region: ""
    access_key_id: ""
    secret_access_key: ""
    use_ssl: false
    prefix: "/tmp/plexspaces-blobs"
```

### Optional MinIO for S3-Compatible Testing

For S3-compatible blob storage testing, run MinIO locally:

```bash
# Start MinIO
docker run -d \
  -p 9000:9000 \
  -p 9090:9090 \
  --name minio_server \
  -e MINIO_ROOT_USER=minioadmin \
  -e MINIO_ROOT_PASSWORD=minioadmin \
  -v ./data:/data \
  quay.io/minio/minio server /data --console-address :9090

# Access MinIO Console at http://localhost:9090
# Create bucket: plexspaces-blobs
```

Configure in `release.yaml`:

```yaml
runtime:
  blob:
    backend: minio
    bucket: plexspaces-blobs  # Must create this bucket in MinIO first
    endpoint: http://localhost:9000
    region: us-east-1
    access_key_id: minioadmin
    secret_access_key: minioadmin
    use_ssl: false
    prefix: "/plexspaces"
```

### Migration from Legacy Examples

If you have existing actors using the WIT interface directly:

**Before (Low-level WIT - 150+ lines of boilerplate)**
```python
from wit_world import exports
from generated.bank_account_pb2 import AccountState, DepositRequest, DepositResponse

class Actor(exports.Actor):
    def __init__(self):
        self.state = AccountState()
    
    def handle(self, from_actor: str, msg_type: str, payload: bytes) -> bytes:
        request = DepositRequest()
        request.ParseFromString(payload)
        if msg_type == "deposit":
            self.state.balance += request.amount
            return DepositResponse(balance=self.state.balance).SerializeToString()
        raise ValueError(f"unknown operation: {msg_type}")
    
    def get_state(self) -> bytes:
        return self.state.SerializeToString()
    
    def set_state(self, state: bytes) -> None:
        self.state.ParseFromString(state)
```

**After (SDK - ~20 lines)**
```python
from plexspaces import actor, state, handler

@actor
class BankAccount:
    balance: int = state(default=0)
    
    @handler("deposit")
    def deposit(self, amount: int) -> dict:
        self.balance += amount
        return {"balance": self.balance}
```

### Build Tool

The `plexspaces-py` CLI replaces manual `build.sh` scripts:

```bash
# Build single actor
plexspaces-py build myactor.py -o myactor.wasm

# Verbose output
plexspaces-py build myactor.py -v

# Custom WIT directory
plexspaces-py build myactor.py --wit-dir /path/to/wit
```

### Multi-Actor Modules

A single WASM module can contain multiple actor classes using the `ACTOR_ROLES` mapping. This allows you to deploy related actors together (e.g., a parameter server and its workers) while keeping them as separate logical actors.

```python
# ml_actors.py
import json
from plexspaces import actor, handler, host, state

@actor
class ParameterServer:
    weights: dict = state(default_factory=dict)

    @handler("get_weights")
    def get_weights(self, worker_id: str) -> dict:
        return {"weights": self.weights}

    @handler("push_gradient")
    def push_gradient(self, worker_id: str, gradient: list) -> dict:
        # Apply gradient update
        return {"status": "applied"}

@actor
class TrainingWorker:
    worker_id: str = state(default="")
    epoch: int = state(default=0)

    @handler("train_step")
    def train_step(self) -> dict:
        result = host.ask(
            "parameter-server:ml-training",
            "get_weights",
            GetWeightsRequest(worker_id=self.worker_id).SerializeToString(),
            5000,
        )
        # ... train ...
        return {"epoch": self.epoch}

# Map role names to actor classes
ACTOR_ROLES = {
    "parameter-server": ParameterServer,
    "training-worker": TrainingWorker,
}
```

When the WASM module is loaded, the runtime inspects `ACTOR_ROLES` to determine which actor class to instantiate based on the role specified at spawn time. Each role is an independent actor with its own state and message handlers, but they share the same WASM binary.

### HTTP Invocation with Timeout

When invoking actors via the HTTP API, you can specify a `timeout` query parameter for long-running operations. This is particularly useful for actors that perform expensive computation (e.g., ML training steps, data aggregation).

```bash
# Default timeout: 5 seconds
curl -X POST http://localhost:8094/api/v1/actors/my-actor:default/invoke \
  -H "Content-Type: application/json" \
  -d '{"msg_type": "train", "payload": {"epochs": 10}}'

# Extended timeout: 30 seconds for long-running operations
curl -X POST "http://localhost:8094/api/v1/actors/my-actor:default/invoke?timeout=30" \
  -H "Content-Type: application/json" \
  -d '{"msg_type": "train", "payload": {"epochs": 10}}'
```

| Parameter | Default | Max | Description |
|-----------|---------|-----|-------------|
| `timeout` | 5 seconds | 3600 seconds (1 hour) | How long the HTTP gateway waits for the actor response |

If the actor does not respond within the specified timeout, the HTTP API returns a `504 Gateway Timeout`. The actor itself continues running -- only the HTTP caller's wait is bounded.

### Leader-worker (multi-node)

Same API surface as Rust/TypeScript/Go. Use when driving multi-node from a Python script (entry node HTTP URL).

```python
from plexspaces import LeaderWorkerClient, list_worker_node_ids

# One-off list
node_ids = list_worker_node_ids("http://localhost:8091", page_size=100)

# Or use a client (required for spawn_actor_on_node)
client = LeaderWorkerClient("http://localhost:8091")
node_ids = client.list_worker_node_ids(cluster=None, page_size=100)
# Virtual actors: send to the canonical actor ID string (lazy); no ensure.
# Non-virtual: spawn on a specific node
actor_ref = client.spawn_actor_on_node(node_ids[0], "worker", "w-1")
```

---

## TypeScript SDK

The TypeScript SDK uses **inheritance** instead of decorators: extend `PlexSpacesActor<TState>` and implement `getDefaultState()` plus `on<Op>(payload)` handlers. The SDK owns actor-world protobuf encode/decode so TypeScript code stays aligned with the same contract used by Rust, Python, and Go.

### Installation

```bash
cd sdks/typescript
npm install
npm run build
```

In an example or app, add a dependency:

```json
"dependencies": {
  "@plexspaces/sdk": "file:../../../../sdks/typescript"
}
```

### Quick Start

**1. Extend `PlexSpacesActor` and add handlers**

```ts
import { PlexSpacesActor } from "@plexspaces/sdk";

interface MyState { count: number; }

export class CounterActor extends PlexSpacesActor<MyState> {
  getDefaultState(): MyState { return { count: 0 }; }

  onIncrement(payload: Record<string, unknown>) {
    const amount = Number(payload.amount ?? 1);
    this.state.count += amount;
    return { count: this.state.count };
  }

  onGet() { return { count: this.state.count }; }
}

const instance = new CounterActor();
export const actor = {
  init: (c: string) => instance.init(c),
  handle: (from: string, msg: string, payload: string) => instance.handle(from, msg, payload),
  getState: () => instance.getState(),
  setState: (s: string) => instance.setState(s),
};
```

**2. Build to WASM**

- Compile: `tsc`
- Bundle actor + SDK into one ESM file (e.g. with esbuild)
- Build WASM: `jco componentize your-bundle.mjs --wit wit/plexspaces-actor -o actor.wasm --disable all`

The `--disable all` flag ensures the component only imports `plexspaces:actor/host` (no WASI), matching the PlexSpaces runtime linker.

**Note**: WIT TypeScript types are automatically generated by the SDK during build (`npm run build`). Client code doesn't need to generate or import these types - the SDK abstracts all WIT details away.

### API (TypeScript)

| API | Description |
|-----|-------------|
| `PlexSpacesActor<TState>` | Base class. `TState` is your state shape (plain object). |
| `getDefaultState(): TState` | Override to return initial state. |
| `onInit(config)` | Optional. Called from `init()` with parsed config. |
| `on<Op>(payload)` | Handler for message op (e.g. `onDeposit`, `onBalance`). SDK decorators map payload bytes to generated models or plain objects. |
| `protected state: TState` | Current state; read/write in handlers. |
| `protected encode(message)`, `decode(bytes, ctor)` | Helpers for protobuf-backed actor-world payloads. |

**FSM Actors (TypeScript)**: The TypeScript SDK offers two patterns for FSM actors. The decorator form requires `experimentalDecorators` in tsconfig; the class-based form uses static properties:

```typescript
// Decorator form (requires experimentalDecorators: true in tsconfig)
@fsm_actor({ states: ["idle", "processing", "done", "error"], initial: "idle" })
class OrderFSMActor { /* ... */ }

// Class-based form (no decorator required)
class OrderFSMActor extends PlexSpacesActor<State> {
  static readonly FSM_STATES = ["idle", "processing", "done", "error"] as const;
  static readonly FSM_INITIAL = "idle";

  getDefaultState(): State { return { fsmState: OrderFSMActor.FSM_INITIAL, ... }; }
}
```

Both forms document valid states and the initial state for observability and tooling.

**Host Functions**: The TypeScript SDK uses WIT virtual imports for host functions. jco componentize wires up `plexspaces:actor/host@0.1.0` imports at build time. The SDK uses generated protobuf models for shared contracts and keeps actor-world encoding at the decorator layer. See [TypeScript SDK README](../sdks/typescript/README.md#host-functions) for details.

**Tier 1 helpers** — convenience wrappers available on the `host` singleton:

| Helper | API | Notes |
|--------|-----|-------|
| `host.kvGetJson<T>(key)` | `→ T \| null` | Returns `null` if missing or corrupt JSON |
| `host.kvPutJson(key, value)` | `→ void` (throws on error) | Serializes to JSON and stores |
| `host.incrCounter(appId, name)` | `→ void` | Increments one counter by 1; errors swallowed |
| `host.incrCounters(appId, counters)` | `→ void` | Increments multiple counters; errors swallowed |
| `host.processGroups.first(group)` | `→ string \| null` | First member of the group |
| `host.processGroups.firstOrThrow(group)` | `→ string` | Throws if the group is empty |
| `new EventLog(watermark?)` | — | Embed in actor state (serializable) |
| `log.append(host, prefix, entry)` | `→ number` | Sequence number; rolls back on KV failure |
| `log.poll(host, prefix, consumerId, limit?)` | `→ [events, cursor]` | Advances consumer cursor in KV |

**Serialization**: The actor-world boundary is protobuf-first. SDK decorators marshal generated protobuf messages to bytes and unmarshal replies so application code stays typed while the runtime stays aligned with the shared host contract.

Observability (metrics, tracing) for WASM actors is provided by the PlexSpaces runtime; the TypeScript SDK does not add its own. See [sdks/typescript/README.md](../sdks/typescript/README.md) and [examples/typescript/apps/bank_account](../examples/typescript/apps/bank_account/README.md) for a full example and E2E test.

### Leader-worker (multi-node)

Same API surface as Rust/Python/Go. Use when driving multi-node from Node or browser (entry node HTTP URL).

```ts
import { LeaderWorkerClient, listWorkerNodeIds } from "@plexspaces/sdk";

// One-off list
const nodeIds = await listWorkerNodeIds("http://localhost:8091", null, 100);

// Or use a client (required for spawnActorOnNode)
const client = new LeaderWorkerClient("http://localhost:8091");
const ids = await client.listWorkerNodeIds(undefined, 100);
// Virtual actors: send to the canonical actor ID string (lazy); no ensure.
const actorRef = await client.spawnActorOnNode(ids[0], "worker", "w-1");
```

---

## Rust SDK

The Rust SDK provides **Python-style annotations** to eliminate boilerplate. Use the same core macros across native Rust and deployable Rust WASM: `#[gen_server_actor]`, `#[handler("op")]`, and `#[plexspaces_handlers]`. For deployable Rust WASM actors on the `plexspaces:actor` WIT surface, use the WASM mode forms `#[gen_server_actor(wasm)]` and `#[plexspaces_handlers(wasm)]`.

**Location**: `sdks/rust/plexspaces-sdk` and `sdks/rust/plexspaces-sdk-macros`.

### Handler dispatching (call/cast semantics)

- **Call** = request-reply (GET/ask): client expects a response.
- **Cast** = fire-and-forget (POST/tell): no response required.
- **GenServer uses call by default**: For `#[gen_server_actor]`, handlers automatically use "call" semantics - no second parameter needed. Handlers return `Result<Value, BehaviorError>` and SDK auto-sends reply.

### Annotations (mirroring Python SDK)

#### Actor Type Annotations

| Rust Annotation | Python Equivalent | Generated Code |
|-----------------|-------------------|----------------|
| `#[actor]` | `@actor` | `const FACETS`; use `#[plexspaces_handlers(custom)]` for dispatch |
| `#[actor(facets = ["durability"])]` | `@actor(facets=[...])` | Same + `FACETS` const for documentation |
| `#[actor(name = "custom_name")]` | `@actor` | Custom behavior type name for HTTP routing |
| `#[gen_server_actor]` | `@gen_server_actor` | `impl Actor` + delegates to `GenServer::route_message` |
| `#[gen_server_actor(facets = ["timer"])]` | `@gen_server_actor(facets=[...])` | Same with facets |
| `#[gen_server_actor(name = "webhook")]` | N/A | Custom type name for HTTP gateway routing |
| `#[event_actor]` | `@event_actor` | `impl Actor` with GenEvent behavior |
| `#[event_actor(name = "audit")]` | N/A | Custom type name for routing |
| `#[fsm_actor]` | `@fsm_actor` | `impl Actor` with GenStateMachine behavior |
| `#[fsm_actor(states = ["a","b"], initial = "a")]` | `@fsm_actor(states=[...], initial="...")` | Same + `FSM_STATES` and `FSM_INITIAL` consts |
| `#[workflow_actor]` | `@workflow_actor` | `impl Actor` with Workflow behavior |
| `#[gen_server_actor(wasm)]` | `@gen_server_actor` for Rust WASM apps | Marks a deployable Rust WASM request-reply handler |

#### Handler Annotations

| Rust Annotation | Python Equivalent | Generated Code |
|-----------------|-------------------|----------------|
| `#[handler("op")]` | `@handler("op")` | Route operation "op" to this method. Operation extracted from `payload.action`, `payload.op`, or `payload.msg_type` when `message_type` is "call"/"cast". When `message_type` is not "call"/"cast", uses `message_type` directly (GenServer=call) |
| `#[handler("op", call)]` | `@handler("op", "call")` | Explicit call semantics (request-reply) |
| `#[handler("op", cast)]` | `@handler("op", "cast")` | Explicit cast semantics (fire-and-forget) |
| `#[handler("*")]` | N/A | Catch-all handler (matches any operation). Useful for worker actors that process tasks based on `payload.action` |
| `#[init_handler]` | `@init_handler` | Called on actor initialization |
| `#[run_handler]` | N/A | Workflow main execution handler |
| `#[signal_handler("name")]` | N/A | Workflow signal handler |
| `#[query_handler("name")]` | N/A | Workflow query handler (read-only) |

#### Deployable Rust WASM Annotations

For deployable Rust WASM apps, keep the WIT guest wrapper thin and put role logic in annotated
leader/worker structs:

| Rust Annotation | Purpose |
|-----------------|---------|
| `#[gen_server_actor(wasm)]` | Marks a leader/worker struct as a deployable Rust WASM request-reply handler |
| `#[plexspaces_handlers(wasm)]` | Generates `SimpleActorHandlers` dispatch from `#[handler]` methods |

These macros are intended for the same role-oriented patterns used by the Rust WASM app examples
such as `heat_diffusion` and `genomics_pipeline`. The outer app still chooses the role, but the
role-specific message handling uses the same annotation style as the native SDK instead of
hand-written operation matching in each module.

#### Dispatch Annotations

| Rust Annotation | Generated Code |
|-----------------|----------------|
| `#[plexspaces_handlers]` | `impl GenServer` dispatch from `#[handler]` methods |
| `#[plexspaces_handlers(gen_server)]` | Same as above (explicit) |
| `#[plexspaces_handlers(event)]` | `impl EventHandler` dispatch for GenEvent actors |
| `#[plexspaces_handlers(custom)]` | `impl Actor` dispatch for Custom behavior actors |
| `#[plexspaces_handlers(fsm)]` | FSM dispatch based on state and event |
| `#[plexspaces_handlers(workflow)]` | `impl Workflow` with run/signal/query handlers |

**Note**: For `#[gen_server_actor]`, all handlers default to "call" - no second parameter needed (like Python).

### API Summary

| API | Description |
|-----|-------------|
| `#[gen_server_actor]` | Attribute on struct: generates `impl Actor` with GenServer behavior |
| `#[actor]` | Attribute on struct: generates `impl Actor` with Custom behavior |
| `#[handler("op")]` | Attribute on method: marks as handler (GenServer defaults to call) |
| `#[plexspaces_handlers]` | Attribute on impl block: generates dispatch from `#[handler]` methods |
| `spawn(ctx, sl, id, ns, actor)` | Spawn actor using declared facets from annotation |
| `spawn_with_facets(ctx, sl, id, ns, actor, facets)` | Spawn actor with explicit facets |
| `spawn_with_storage(ctx, sl, id, ns, actor, storage)` | Spawn durable actor with storage backend |
| `create_facets(&["timer", "durability"], &config)` | Create facet instances from names (convenience helper) |

### Tier 1 WASM Actor Helpers (`simple_actor`)

The following free functions and types are available in `plexspaces_sdk::simple_actor` for deployable Rust WASM actors. They wrap the WIT `host::*` functions with ergonomic Rust types. None of these require the `native` feature flag — they compile for WASM targets.

| API | Description |
|-----|-------------|
| `pg_first(group)` | `→ Result<String, String>` — first member of the process group |
| `kv_get_json::<T>(key)` | `→ Result<Option<T>, String>` — deserialize JSON from KV; `None` if missing |
| `kv_put_json(key, &value)` | `→ Result<(), String>` — serialize to JSON and store |
| `incr_counter(app_id, name)` | `→ ()` — increment one counter by 1; errors are logged and swallowed |
| `incr_counters(app_id, &[("name", delta)])` | `→ ()` — increment multiple counters; errors are logged and swallowed |
| `EventLog { watermark: i64 }` | Monotonic append-only log backed by KV. Derive `Serialize`/`Deserialize`. |
| `log.append(prefix, &entry)` | `→ Result<i64, String>` — append JSON entry; rolls back watermark on failure |
| `log.poll::<T>(prefix, consumer_id, limit)` | `→ Result<(Vec<T>, i64), String>` — return new events for a consumer |

```rust
// Rust WASM — Tier 1 helpers in a deployable actor
use plexspaces_sdk::simple_actor::{
    pg_first, kv_get_json, kv_put_json, incr_counter, incr_counters, EventLog,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
struct MyState {
    audit_log: EventLog,
}

// Route to the first available LLM router
let router = pg_first("svc:llm_router")?;

// Store and retrieve a typed record
kv_put_json("task:1", &Task { seq: 1, kind: "summarize".into() })?;
let task: Option<Task> = kv_get_json("task:1")?;

// Record metrics (never panics)
incr_counter("my-app", "tasks_processed");
incr_counters("my-app", &[("cache_hits", 5), ("cache_misses", 2)]);

// Append to an audit log and poll events
let seq = state.audit_log.append("audit:", &entry)?;
let (events, cursor) = state.audit_log.poll::<serde_json::Value>("audit:", "consumer-1", 20)?;
```

**Note**: For examples and production code, prefer using `Node` or SDK spawn helpers instead of calling `ActorFactory` directly. The SDK helpers delegate to the framework-owned actor spawn path and return canonical typed refs without exposing mailbox or registry internals to application code.

### Workflow retry (all SDKs)

Retry behavior is defined by proto `RetryConfig` (plexspaces.workflow.v1). Single unified run with reasonable defaults and overrides.

- **Contract**: One run method/helper. `max_attempts` 0 or unset = 1 attempt; otherwise retry up to `max_attempts`. **Exponential backoff with jitter**: between attempts, delay = `min(initial_interval_ms * backoff_rate^(attempt-1), max_interval_ms)` with full jitter (multiply by random in (0, 1]). Defaults when unset: `initial_interval_ms` 100, `backoff_rate` 2, `max_interval_ms` 30000.
- **Rust (crates/behavior)**: `ctx.run(name, retry, operation)` with `retry: Option<&RetryConfig>`. `None` or `default_retry_config()` = one attempt; `Some(&RetryConfig { max_attempts, initial_interval_ms, backoff_rate, max_interval_ms, .. })` for retries with exponential backoff and jitter. Exports: `ExecutionContext`, `default_retry_config`, `RetryConfig`.
- **TypeScript**: `withRetry(fn, config?)` with `RetryConfig`. Immediate retries (no delay in WASM); full backoff+jitter in Rust `run()`. Omitted config = 3 attempts; `max_attempts: 1` = one attempt. `defaultRetryConfig()` returns single-attempt config.
- **Python**: `with_retry(fn, retry_config=None)`. Immediate retries; full backoff+jitter in Rust `run()`. `default_retry_config()` returns single-attempt dict.
- **Go**: `WithRetry(fn, config)`. Immediate retries; full backoff+jitter in Rust `run()`. `DefaultRetryConfig()` returns single-attempt config.

### Message Creation Helpers

The SDK provides helper functions for creating messages with correct invocation semantics:

| Function | Description | Use With |
|----------|-------------|----------|
| `call_message(payload)` | Create request-reply message (`message_type = "call"`) | `actor_ref.ask(&ctx, …)` |
| `cast_message(payload)` | Create fire-and-forget message (`message_type = "cast"`) | `actor_ref.tell(&ctx, …)` |
| `new_message(invocation, payload)` | Create message with custom invocation type | Either |

**Example:**
```rust
use plexspaces_actor::RequestContext;
use plexspaces_sdk::{call_message, cast_message, json};
use std::time::Duration;

let ctx = RequestContext::new_without_auth("tenant-id".into(), "namespace".into());

// Request-reply: use call_message() with ask()
let request = call_message(json!({ "action": "get_balance" }));
let reply = actor_ref
    .ask(&ctx, request, Duration::from_secs(5))
    .await?;

// Fire-and-forget: use cast_message() with tell()
let event = cast_message(json!({ "event": "user_login", "user_id": "123" }));
actor_ref.tell(&ctx, event).await?;
```

**Message Routing Design:**
- `message_type` = "call" or "cast" (invocation type, set by the API or SDK helper you choose)
- When `message_type` is "call"/"cast", operation is extracted from `payload.action`, `payload.op`, or `payload.msg_type`
- When `message_type` is not "call"/"cast", operation is `message_type` itself
- Handlers match on extracted operation name, not on `message_type`

**Why invocation matters:**
- GenServer's `route_message()` dispatches based on `message_type`
- `"call"` routes to `handle_request()` (request-reply, reply expected)
- `"cast"` routes to `handle_request()` (fire-and-forget, reply optional)
- `"info"` routes to `handle_info()` (async message, no reply)
- **Note**: GenServer uses a single `handle_request()` method for both call and cast; the difference is whether a reply is expected (call) or optional (cast)

### Unified ShardGroup Client (Data-Parallel Actors)

The Rust SDK provides unified abstractions for ShardGroup (data-parallel actors) inspired by the [Data-Parallel Actors (DPA) paper](https://www.micahlerner.com/2022/06/04/data-parallel-actors-a-programming-model-for-scalable-query-serving-systems.html). The primary SDK path is in-process and ServiceLocator-backed so embedded and WASM usage stays on the framework-owned implementation instead of making remote calls.

**Key Features**:
- **Unified API**: Same high-level API for embedded and WASM/local usage
- **Boilerplate Removal**: Auto RequestContext, JSON conversion, error handling
- **Resource-Based Routing**: Labels flow to DataParallelConfig.placement.required_labels (NodePlacement) for scheduler node matching
- **Canonical API**: `ShardGroupClient` and `UnifiedShardGroupClient` provide map, scatter-gather, and bulk update operations over the same core service traits

**Example: Unified ShardGroup Client**

```rust
use plexspaces_sdk::{UnifiedShardGroupClient, PartitionStrategy};
use std::collections::HashMap;
use plexspaces_proto::actor::v1::{NodePlacement, NodePlacementStrategy};

// In-process / WASM-local path (uses ServiceLocator directly)
let mut client = UnifiedShardGroupClient::from_service_locator(service_locator).await?;

// Create ShardGroup (worker pool)
let mut labels = HashMap::new();
labels.insert("cluster".to_string(), "prod".to_string());
labels.insert("zone".to_string(), "us-west-1".to_string());
let placement = NodePlacement {
    strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
    cluster: "prod".to_string(),
    node_ids: vec![],
    required_labels: labels,
    avoid_node_ids: vec![],
    resource_requirements: None,
    affinity_labels: HashMap::new(),
};

let group = client.create_shard_group(
    "worker-pool-1".to_string(),
    "worker".to_string(),
    4, // 4 shards
    PartitionStrategy::PartitionStrategyHash,
    Some(placement),
).await?;

// Bulk update: route tasks to workers based on partition key
let mut updates = HashMap::new();
updates.insert("key-1".to_string(), json!({"action": "increment", "value": 1}));
updates.insert("key-2".to_string(), json!({"action": "increment", "value": 2}));

let bulk_resp = client.bulk_update(
    "worker-pool-1".to_string(),
    updates,
    ConsistencyLevel::ConsistencyLevelEventual,
    false, // fire-and-forget
).await?;

// Parallel map: query all workers
let map_resp = client.map(
    "worker-pool-1".to_string(),
    json!({"action": "get_all_keys"}),
).await?;

// Scatter-gather: aggregate results
let scatter_resp = client.scatter_gather(
    "worker-pool-1".to_string(),
    json!({"action": "get_total_count"}),
    ShardGroupAggregationStrategy::ShardGroupAggregationConcat,
    2, // min responses
).await?;
```

**Example: ShardGroup Operations**

```rust
use plexspaces_sdk::{ShardGroupClient, ShardGroupClientTrait};
use plexspaces_proto::actor::v1::{
    ConsistencyLevel, PartitionStrategy, ShardGroupAggregationStrategy,
};

let mut client = ShardGroupClient::from_service_locator(service_locator.clone()).await?;

let group = client.create_shard_group(
    "pool-1".to_string(),
    "worker",
    4,
    PartitionStrategy::PartitionStrategyHash,
    None,
).await?;
let pool_id = group.config.expect("config").group_id;

let map_resp = client.map(
    pool_id.clone(),
    json!({"action": "get_all"}),
).await?;

let aggregated = client.scatter_gather(
    pool_id.clone(),
    json!({"action": "get_total"}),
    ShardGroupAggregationStrategy::ShardGroupAggregationConcat,
    2,
).await?;

let mut updates = HashMap::new();
updates.insert("key-1".to_string(), json!({"action": "increment"}));
let stats = client.bulk_update(
    pool_id,
    updates,
    ConsistencyLevel::ConsistencyLevelEventual,
    false,
).await?;
```

**Architecture**:
- **Core Functionality**: Lives in `crates/services/src/actor_service/mod.rs` (ActorService trait)
- **SDK Decorators**: `UnifiedShardGroupClient` wraps ActorService and removes boilerplate
- **Transport Boundary**: Local SDK usage stays in-process via ServiceLocator; remote gRPC APIs are built separately on top of the same proto/service contracts
- **Labels Flow**: ShardGroup config.placement.required_labels (NodePlacement) → ActorResourceRequirements.placement → NodeSelector → Node placement

For WASM apps using the actor-world WIT world, node-local benchmark counters should be recorded
through `application-metrics-add` and read back with `application-get-status`. The SDK/WIT layer is
only the decorator; the authoritative per-node application metrics live in the application manager
inside the main Rust framework crates.

**Cross-SDK WASM parity**: Python, TypeScript, Go, and Rust WASM all expose the same shard-group
host surface through the actor-world WIT world:
- `create-shard-group`
- `bulk-update-shard-group`
- `map-shard-group`
- `scatter-gather`
- `broadcast-shard-group`
- `reduce-shard-group`
- `all-reduce-shard-group`
- `barrier-shard-group`
- `spawn-actors`
- `application-metrics-add`
- `application-get-status`
- `http-fetch`

These WASM-facing SDK wrappers are transport decorators only. They encode/decode the generated
protobuf request and response models and delegate to the underlying framework `ActorService` /
application-manager implementations through the WIT host boundary.

**Batching for efficiency**: Because each `scatter-gather` call is one gRPC round-trip shared across
all shards, processing multiple logical work items per call amortises coordination overhead. Pass a
`batch_size` field in your scatter-gather payload so each shard processes `N` items per message.
This is the primary lever for improving the granularity ratio (`Gran = compute_time / coord_time`).
See [Scaling Benchmarks](detailed-design.md#scaling-benchmarks-and-parallel-efficiency) for
details on Gran, Eff%, and the strong vs. weak scaling model.

See [Firecracker Multi-Tenant Example](../examples/rust/embedded/firecracker_multi_tenant/README.md) for a complete data-parallel actors demonstration.

### Leader-worker (multi-node, one run)

Multi-node parallelization is **one logical run** with work **split across nodes**. The **first node** that receives the run is the **leader**; it distributes work to workers on the same or other nodes.

**Rust SDK** (host-side, in-process): `plexspaces_sdk::leader_worker` provides:

| API | Purpose |
|-----|---------|
| `list_worker_node_ids(ctx, service_locator, cluster, page_size)` | Returns node IDs from the registry (after ConnectNodes). Leader uses this to distribute work. |
| `spawn_actor_on_node(ctx, service_locator, node_id, actor_type, actor_id, initial_state, config, labels)` | Calls the target node’s **`SpawnActor`** RPC with **`SpawnActorRequest.spec`** (`ActorSpawnSpec`). Legacy **`initial_state`** bytes (JSON in the WASM init shape) are mapped into **`spec.role`** / **`spec.args`** via **`plexspaces_actor::legacy_spawn_init_json_to_role_and_args`**. Prefer building **`ActorSpawnSpec`** directly when you already have structured args. |

**Virtual actors are lazy**: They are created on first message receive. The leader does not call any “ensure” or pre-create step. Deploy the worker type as virtual on all nodes, then send directly to the canonical actor ID for the shard, such as `chunk-1//worker::default@node-B`; the target node creates the actor when it receives the first message. This is consistent across all runtimes (Rust, Python, TypeScript, Go).

Core lives in main crates: NodeRegistry, ActorService, scheduling/placement, and the node registry. The SDK is a thin wrapper over these framework-owned services.

**Cross-SDK parity**: All four SDKs expose the same leader-worker API. **Rust** (in-process): `list_worker_node_ids(ctx, service_locator, cluster, page_size)`, `spawn_actor_on_node(ctx, service_locator, node_id, ...)`. **Python**: `LeaderWorkerClient(entry_http_url)`, `client.list_worker_node_ids(cluster=..., page_size=...)`, `client.spawn_actor_on_node(node_id, actor_type, ...)`, plus `list_worker_node_ids(entry_http_url, ...)`. **TypeScript**: `LeaderWorkerClient`, `listWorkerNodeIds()`, `spawnActorOnNode()`. **Go**: `NewLeaderWorkerClient(entryHTTPURL)`, `ListWorkerNodeIds()`, `SpawnActorOnNode()` (and `ListWorkerNodeIds` convenience). Virtual actors are lazy in all; no ensure step.

### Elastic pool (checkout/checkin)

The framework provides a single unified elastic pool: the **ElasticPool** implementation in `crates/elastic-pool`, exposed via the **ElasticPoolService** trait in core. The node (or app) registers a **PoolRegistry** (which holds named ElasticPool instances) with ServiceLocator. The SDK decorator **ElasticPoolClient** obtains the service from ServiceLocator and exposes checkout/checkin, metrics, and scale without adding new business logic.

- **Core**: `ElasticPoolService` trait and `PoolServiceError` in `plexspaces-actor`; ServiceLocator has `get_elastic_pool_service` / `register_elastic_pool_service`.
- **Implementation**: `crates/elastic-pool` — `ElasticPool` (one pool), `PoolRegistry` (implements ElasticPoolService, holds multiple named pools).
- **SDK**: `ElasticPoolClient::from_service_locator(service_locator)` — thin wrapper that calls the registered service.

```rust
use plexspaces_sdk::ElasticPoolClient;
use std::time::Duration;

let client = ElasticPoolClient::from_service_locator(service_locator.clone());
let handle = client.checkout("my-pool", Duration::from_secs(5)).await?;
// use actor via handle.actor_id...
client.checkin("my-pool", &handle.actor_id, &handle.checkout_id, true).await?;
```

Node setup: register `PoolRegistry` (from `plexspaces_elastic_pool`), then create pools via the registry or `registry.register_pool(name, pool)` before starting.

**WASM host API**: The actor-world WIT exposes `pool-checkout`, `pool-checkin`, and `pool-get-metrics` so that Python, Go, TypeScript, and Rust WASM actors can use the same pool service. The same host also exposes shard-group helpers for deployable apps: `create-shard-group`, `bulk-update-shard-group`, `map-shard-group`, `broadcast-shard-group`, `reduce-shard-group`, `all-reduce-shard-group`, `barrier-shard-group`, `scatter-gather`, `spawn-actors`, `application-metrics-add`, `application-get-status`, and `http-fetch`. The runtime injects the framework `ActorService`, application manager, outbound HTTP client, and `ElasticPoolService` into `HostFunctions`, so the WIT surface stays a thin decorator over the core crates. Failures use WIT `result<_, actor-error>` and successful payloads are protobuf wire bytes for the generated SDK models.

### Example: GenServer with annotations (webhook_handler-style)

```rust
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, spawn_with_facets, call_message, json, Value,
};

// Step 1: Annotate struct with #[gen_server_actor]
// Generates: impl Actor { behavior_type() = GenServer; handle_message() -> route_message() }
#[gen_server_actor]
struct WebhookHandler {
    deliveries: Vec<String>,
}

// Step 2: Annotate impl with #[plexspaces_handlers]
// Generates: impl GenServer { handle_request() = dispatch by payload.action }
#[plexspaces_handlers]
impl WebhookHandler {
    // GenServer handlers default to "call" - no second param needed
    // Return Result<Value, ...>, SDK serializes and sends reply automatically
    #[handler("deliver")]
    async fn deliver(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        let id = ulid::Ulid::new().to_string();
        self.deliveries.push(id.clone());
        Ok(json!({ "id": id, "action": "delivered" }))
    }

    #[handler("list")]
    async fn list(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        Ok(json!({ "deliveries": self.deliveries, "total": self.deliveries.len() }))
    }
}

// Spawn with facets using SDK helper
let actor_ref = spawn_with_facets(&ctx, service_locator, actor_id, "webhooks", WebhookHandler::new(), vec![]).await?;

// Send request-reply message using call_message()
let request = call_message(json!({ "action": "deliver", "url": "https://example.com" }));
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;
```

### Example: Custom actor with fire-and-forget handlers (session_manager-style)

```rust
use plexspaces_sdk::{
    actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, spawn_with_facets, cast_message, TimerFacet, json,
};

// Step 1: Annotate struct with #[actor]
// Generates: impl Actor { behavior_type() = Custom("SessionActor") }
#[actor(facets = ["timer"])]
struct SessionActor {
    user_id: String,
    is_active: bool,
}

// Step 2: Annotate impl with #[plexspaces_handlers(custom)]
// Generates: impl Actor { handle_message() = dispatch by msg.message_type }
#[plexspaces_handlers(custom)]
impl SessionActor {
    // cast semantics: fire-and-forget, no reply
    #[handler("timer_fired", cast)]
    async fn handle_timer_fired(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let timer_name = String::from_utf8_lossy(&msg.payload);
        if timer_name == "idle_timeout" {
            self.is_active = false;
        }
        Ok(())
    }

    #[handler("activity", cast)]
    async fn handle_activity(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<(), BehaviorError> {
        // Reset idle timer, etc.
        Ok(())
    }
}

// Spawn with TimerFacet using SDK helper
let timer_facet = TimerFacet::new(json!({}), 50);
let actor_ref = spawn_with_facets(&ctx, sl, "user-123", "sessions", SessionActor::new("user-123"), vec![Box::new(timer_facet)]).await?;

// Send fire-and-forget message using cast_message()
let activity_event = cast_message(json!({ "event": "user_activity" }));
actor_ref.tell(&ctx, activity_event).await?;
```

### Timer vs Reminder: Transient vs Durable Scheduling

PlexSpaces follows the **Orleans model** for time-based operations:

| Facet | Durability | Storage | Message Type | Use Case |
|-------|------------|---------|--------------|----------|
| `TimerFacet` | **Transient** (in-memory) | None | `timer_fired` | Heartbeats, timeouts, health checks |

---

## Node Connectivity (Health-Aware Connection)

The SDK provides `NodeClient` for production-grade node connectivity with health checks, retry logic, and graceful error handling. For server-side behavior (node startup `cluster_seed_nodes`, application deploy `seed_nodes`, connect timeout, and cluster matching), see [Services Reference](services.md#node-connectivity-and-seed-nodes) and [Architecture](architecture.md#node-connectivity-and-seed-nodes).

### Health-Aware Connection (Kubernetes + Erlang-inspired)

**Architecture**:
- **Core Logic**: `NodeService` and `SystemService` in `crates/services` (core APIs)
- **SDK Wrapper**: `NodeClient` in `sdks/rust/plexspaces-sdk` (convenience with retry/backoff)

**Health Check Flow**:
1. **Liveness Check** (optional): Uses core `SystemService.liveness_probe()` API
2. **Connection**: Uses core `NodeServiceClient.connect()` API  
3. **Ping Verification**: Uses core `NodeService.ping()` API
4. **Readiness Wait** (optional): Uses core `SystemService.readiness_probe()` API

**Production-Grade Features**:
- ✅ Exponential backoff with jitter (prevents thundering herd)
- ✅ Health-aware retries (checks liveness before retrying)
- ✅ Readiness polling (waits for node to be ready)
- ✅ Parallel health checks for multiple nodes
- ✅ Graceful degradation (partial success handling)
- ✅ Detailed error messages for troubleshooting

### Basic Usage

```rust
use plexspaces_sdk::NodeClient;
use std::time::Duration;

// Simple: Uses defaults (checks liveness, waits for readiness)
let mut node_client = NodeClient::connect("http://localhost:8000").await?;

// Connect to multiple nodes with health checks
let resp = node_client.connect_nodes(
    vec![
        "http://localhost:8092".to_string(),
        "http://localhost:8093".to_string(),
    ],
    None, // cluster name (optional)
    30,   // timeout seconds
).await?;

// Check results
println!("Connected: {:?}", resp.connected);
println!("Failed: {:?}", resp.failed);

// List connected nodes
let list_resp = node_client.list_connected_nodes(None).await?;
for node in list_resp.nodes {
    println!("Node: {} @ {}", node.node_id, node.node_address);
}
```

### Advanced Configuration

```rust
use plexspaces_sdk::{NodeClient, HealthCheckConfig};
use std::time::Duration;

// Custom health check configuration
let config = HealthCheckConfig {
    max_retries: 10,
    initial_delay: Duration::from_millis(500),
    max_delay: Duration::from_secs(10),
    health_check_timeout: Duration::from_secs(5),
    check_liveness: true,        // Check liveness before connecting
    wait_for_readiness: true,    // Wait for readiness after connecting
    readiness_timeout: Duration::from_secs(60),
    readiness_poll_interval: Duration::from_millis(500),
};

let mut node_client = NodeClient::connect_with_health_check(
    "http://localhost:8000",
    config,
).await?;

// Connect multiple nodes with custom config
let resp = node_client.connect_nodes_with_health_check(
    vec!["http://localhost:8092".to_string()],
    None,
    30,
    config,
).await?;
```

### Health Check Semantics (Kubernetes-inspired)

- **Liveness**: Is the node alive? (should we retry?)
  - Checks: gRPC server responsive, not deadlocked
  - Used for: Retry decisions, failure detection
  - Endpoint: `SystemService.liveness_probe()`

- **Readiness**: Is the node ready? (can we use it?)
  - Checks: Critical dependencies healthy, not overloaded
  - Used for: Connection decisions, load balancing
  - Endpoint: `SystemService.readiness_probe()`

### Retry Strategy (Erlang-inspired)

- **Exponential Backoff**: `delay = min(initial * 2^attempt, max) + jitter`
- **Jitter**: 0-25% of delay (prevents thundering herd)
- **Parallel Health Checks**: Uses `futures::join_all` for efficiency
- **Graceful Degradation**: Continues with available nodes if some fail

### Error Handling

```rust
match NodeClient::connect("http://localhost:8000").await {
    Ok(client) => {
        println!("Connected successfully");
    }
    Err(e) => {
        eprintln!("Connection failed: {}", e);
        // Error messages include:
        // - Liveness check failures
        // - Connection timeouts
        // - Readiness timeout details
    }
}
```

### Multi-Node Connection Flow

When connecting to multiple nodes, the SDK:

1. **Pre-checks liveness** for all nodes in parallel (avoids unnecessary attempts)
2. **Filters out dead nodes** before calling core API
3. **Calls core ConnectNodes API** for alive nodes (registers seed addresses immediately and lets SWIM reconcile node identity + heartbeat)
4. **Returns combined results** (connected + failed with detailed reasons)

This production-grade approach ensures:
- ✅ Efficient connection attempts (only alive nodes)
- ✅ Detailed error reporting (why each node failed)
- ✅ Partial success handling (some nodes may connect while others fail)
- ✅ Better user experience (clear error messages for troubleshooting)

See [Firecracker Multi-Tenant Example](../examples/rust/apps/firecracker_multi_tenant/README.md) for a complete demonstration of health-aware node connectivity.
| `ReminderFacet` | **Durable** (persisted) | `Arc<dyn JournalStorage>` | `reminder_fired` | Billing, SLA, scheduled tasks |

**The naming convention IS the API contract:**
- **Timer** = transient, fast, no persistence overhead, lost on crash
- **Reminder** = durable, requires storage, survives crashes

```rust
use plexspaces_journaling::{TimerFacet, ReminderFacet, SqliteJournalStorage};
use plexspaces_actor::JournalStorage;

// TimerFacet - TRANSIENT (no storage required)
let timer_facet = TimerFacet::new(json!({}), 50);

// ReminderFacet - DURABLE (requires storage)
let storage: Arc<dyn JournalStorage> = Arc::new(
    SqliteJournalStorage::new(":memory:").await?
);
let reminder_facet = ReminderFacet::new(storage, json!({}), 50);

// Spawn actor with both facets
let actor_ref = spawn_with_facets(
    &ctx, sl, actor_id, "billing", BillingActor::new(),
    vec![Box::new(timer_facet), Box::new(reminder_facet)]
).await?;
```

**When to use Timer vs Reminder:**

| Use Timer (transient) | Use Reminder (durable) |
|-----------------------|------------------------|
| Heartbeats / health checks | Billing cycles |
| Session timeouts | SLA enforcement |
| Debouncing / throttling | Trial expiration |
| High-frequency events | Scheduled reports |
| Non-critical operations | Any operation that MUST happen |

### Example: Workflow actor (durable workflows)

```rust
use plexspaces_sdk::{
    workflow_actor, plexspaces_handlers, run_handler, signal_handler, query_handler,
    ActorContext, BehaviorError, Message, json,
};

// Step 1: Annotate struct with #[workflow_actor]
// Generates: impl Actor { behavior_type() = Workflow; handle_message() -> route_workflow_message() }
#[workflow_actor(facets = ["durability"])]
struct PaymentWorkflow {
    order_id: String,
    status: String,
    amount: i64,
}

// Step 2: Annotate impl with #[plexspaces_handlers(workflow)]
// Generates: impl Workflow { run(), signal(), query() }
#[plexspaces_handlers(workflow)]
impl PaymentWorkflow {
    // Main workflow execution (exclusive, one at a time)
    #[run_handler]
    async fn run(&mut self, ctx: &ActorContext, input: Message) -> Result<Message, BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&input.payload)?;
        self.order_id = payload["order_id"].as_str().unwrap_or("").to_string();
        self.amount = payload["amount"].as_i64().unwrap_or(0);
        self.status = "processing".to_string();
        
        // Workflow execution with durable operations via ExecutionContext
        // ctx.run(name, retry, || ...) with retry = None or Some(&RetryConfig), ctx.sleep(), ctx.promise(), etc.
        
        self.status = "completed".to_string();
        Ok(Message {
            payload: serde_json::to_vec(&json!({ "status": "completed", "order_id": self.order_id }))?,
            ..Default::default()
        })
    }
    
    // Signal handler: external events that modify state
    #[signal_handler("cancel")]
    async fn on_cancel(&mut self, _ctx: &ActorContext, _data: Message) -> Result<(), BehaviorError> {
        self.status = "cancelled".to_string();
        Ok(())
    }
    
    // Query handler: read-only (can be concurrent)
    #[query_handler("status")]
    async fn get_status(&self, _ctx: &ActorContext, _params: Message) -> Result<Message, BehaviorError> {
        Ok(Message {
            payload: serde_json::to_vec(&json!({
                "order_id": self.order_id,
                "status": self.status,
                "amount": self.amount,
            }))?,
            ..Default::default()
        })
    }
}
```

### Example: Event actor (fire-and-forget)

```rust
use plexspaces_sdk::{
    event_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message,
};

// Step 1: Annotate struct with #[event_actor]
// Generates: impl Actor { behavior_type() = GenEvent; handle_message() -> handle_event() }
#[event_actor]
struct AuditLogger {
    logs: Vec<String>,
}

// Step 2: Annotate impl with #[plexspaces_handlers(event)]
// Generates: impl EventHandler { handle_event() = dispatch by event type }
#[plexspaces_handlers(event)]
impl AuditLogger {
    // Event handlers are fire-and-forget (no reply)
    #[handler("user_login", cast)]
    async fn on_user_login(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)?;
        let user_id = payload["user_id"].as_str().unwrap_or("unknown");
        self.logs.push(format!("User {} logged in", user_id));
        Ok(())
    }
    
    #[handler("user_logout", cast)]
    async fn on_user_logout(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)?;
        let user_id = payload["user_id"].as_str().unwrap_or("unknown");
        self.logs.push(format!("User {} logged out", user_id));
        Ok(())
    }
}
```

### Rust examples using the SDK

- **webhook_handler** — `#[gen_server_actor]`, `#[plexspaces_handlers]`, HTTP deliver/list; `examples/rust/embedded/webhook_handler/`
- **session_manager** — `#[actor]`, `#[plexspaces_handlers(custom)]`, TimerFacet; `examples/rust/apps/session_manager/`
- **timeseries_forecasting** — uses `spawn_with_behavior_type` for BehaviorRegistry-based actors (pipeline by type name); see its README for when to use BehaviorRegistry vs SDK `spawn`

Going forward, new Rust examples should use these SDK annotations and `spawn` / `spawn_with_facets` where applicable. See [Examples](../examples/README.md) and [Detailed Design - Behaviors](detailed-design.md#behaviors).

---

## Go SDK

The Go SDK uses **TinyGo** to compile actors to WASM components. Actors implement a simple `Actor` interface with JSON-based state management. The SDK provides `BaseActor` for zero-boilerplate state serialization and `ActorRouter` for multi-actor modules.

**Location**: `sdks/go/plexspaces/`

### Leader-worker (multi-node)

Same API surface as Rust/Python/TypeScript. Use when driving multi-node from a Go program (entry node HTTP URL). The `leader_worker` package is excluded from WASM builds (`//go:build !tinygo.wasm`).

```go
import "github.com/plexspaces/plexspaces/sdks/go/plexspaces"

client := plexspaces.NewLeaderWorkerClient("http://localhost:8091")
ids, err := client.ListWorkerNodeIds("", 100, "")
// Virtual actors: send to the canonical actor ID string (lazy); no ensure.
actorRef, err := client.SpawnActorOnNode(ids[0], "worker", "w-1", nil, nil, nil)

// Or one-off list:
ids, err := plexspaces.ListWorkerNodeIds("http://localhost:8091", "", 100)
```

### Installation

```bash
# TinyGo required (0.36+)
brew install tinygo  # macOS
# or see https://tinygo.org/getting-started/install/

# wasm-tools required for Component Model
cargo install wasm-tools
```

### Quick Start

**1. Write your actor**

```go
package main

import "github.com/example/plexspaces/sdks/go/plexspaces"

type Counter struct {
    plexspaces.BaseActor
    Count int `json:"count"`
}

func (c *Counter) Handle(from, msgType, payloadJSON string) string {
    switch msgType {
    case "increment":
        c.Count++
        return fmt.Sprintf(`{"count":%d}`, c.Count)
    case "get":
        return fmt.Sprintf(`{"count":%d}`, c.Count)
    default:
        return `{"error":"unknown operation"}`
    }
}

func init() {
    plexspaces.Register(&Counter{})
}

func main() {}
```

**2. Build to WASM**

```bash
tinygo build -target=wasi -o counter_core.wasm .
wasm-tools component embed wit/ -w actor-world counter_core.wasm -o counter_embedded.wasm
wasm-tools component new counter_embedded.wasm --adapt wasi_snapshot_preview1.reactor.wasm -o counter.wasm
```

**3. Deploy**

```bash
curl -X POST http://localhost:8094/api/v1/deploy \
  -F "namespace=default" \
  -F "actor_type=counter" \
  -F "wasm=@counter.wasm"
```

### Actor Interface

```go
type Actor interface {
    Init(configJSON string) string
    Handle(fromActor, msgType, payloadJSON string) string
    GetState() string
    SetState(stateJSON string) string
}
```

`BaseActor` provides default `Init`, `GetState`, and `SetState` implementations using JSON serialization. You only need to implement `Handle`.

### Multi-Actor Modules (ActorRouter)

```go
func init() {
    router := plexspaces.NewActorRouter()
    router.Route("chat-room", NewChatRoom)
    router.Route("rate-limiter", NewRateLimiter)
    plexspaces.Register(router)
}
```

The router selects the actor by longest-prefix match on the actor ID. For example, `"chat-room-lobby:default"` matches `"chat-room"`.

**Actor Definition Helpers**

Use `RouteDefinition` with typed definition builders when you need explicit behavior metadata (e.g., FSM state lists, facets):

| Helper | Behavior | Description |
|--------|----------|-------------|
| `DefineActor(factory, facets...)` | GenServer | Default actor with optional facets |
| `GenServerActor(factory, facets...)` | GenServer | Explicit GenServer |
| `EventActor(factory, facets...)` | GenEvent | Fire-and-forget event handler |
| `FSMActor(factory, facets...)` | GenStateMachine | FSM actor |
| `FSMActorDef(factory, FSMOpts{...})` | GenStateMachine | FSM actor with state list and initial state |
| `WorkflowActorDefinition(factory, facets...)` | Workflow | Durable workflow actor |

```go
// Register FSM actor with explicit state metadata
router.RouteDefinition("order_fsm", plexspaces.FSMActorDef(
    NewOrderFSM,
    plexspaces.FSMOpts{
        States:  []string{"idle", "processing", "done", "error"},
        Initial: "idle",
        Facets:  []string{"durability"},
    },
))
```

### Host Functions

Access PlexSpaces capabilities via the `Host` singleton:

| Function | Description |
|----------|-------------|
| **Messaging** | |
| `host.Send(to, msgType, payload)` | Fire-and-forget message |
| `host.Ask(to, msgType, payload, timeoutMs)` | Request-reply |
| `host.SelfID()` | Get own actor ID |
| **Actor Lifecycle** | |
| `host.Spawn(moduleRef, actorID, config)` | Create actor |
| `host.Stop(actorID)` | Terminate actor |
| `host.Link(actorID)` / `host.Unlink(actorID)` | Erlang-style linking |
| `host.Monitor(actorID)` / `host.Demonitor(ref)` | Unidirectional monitoring |
| **Timers** | |
| `host.SendAfter(delayMs, msgType, payload)` | Delayed message |
| **Logging & Time** | |
| `host.Log(level, msg)` / `host.Info(msg)` / `host.Warn(msg)` / `host.Error(msg)` | Structured logging |
| `host.NowMs()` | Current timestamp (ms) |
| **Key-Value Storage** | |
| `host.KVGet(key)` / `host.KVPut(key, value)` / `host.KVDelete(key)` / `host.KVList(prefix)` | Key-value operations |
| `host.KVGetJSON(key, &dest)` | Deserialize a JSON value from KV. Returns `(bool, error)` — false when missing. |
| `host.KVPutJSON(key, value)` | Serialize a value to JSON and store it. Returns `error`. |
| **TupleSpace** | |
| `host.TSWrite(tupleJSON)` / `host.TSRead(patternJSON)` / `host.TSTake(patternJSON)` / `host.TSReadAll(patternJSON)` | Linda-style coordination |
| **Distributed Locks** | |
| `host.LockAcquire(...)` / `host.LockRelease(...)` / `host.LockRenew(...)` | Distributed locks with leases |
| **Blob Storage** | |
| `host.BlobUpload(id, data, contentType)` / `host.BlobDownload(id)` / `host.BlobDelete(id)` / `host.BlobList(prefix)` | S3-compatible storage |
| **Process Groups** | |
| `host.PG().Join(group)` / `host.PG().Leave(group)` / `host.PG().Members(group)` / `host.PG().Broadcast(group, msgType, payload)` | Distributed pub/sub |
| `host.PG().First(group)` | Return the first member of the group, or error if empty. |
| **Metrics helpers** | |
| `b.IncrCounter(host, name)` | Increment a named counter by 1 (on `*BaseActor`). Errors are swallowed. |
| `b.IncrCounters(host, counters)` | Increment multiple counters: `map[string]int{"cache_hits": 5}`. Errors are swallowed. |
| **EventLog** | |
| `EventLog{}` | Embed in actor state struct. Holds only `Watermark int64`. |
| `log.Append(host, prefix, entry)` | Append JSON entry; returns sequence number. Rolls back on KV failure. |
| `log.Poll(host, prefix, consumerID, limit)` | Return up to `limit` new events for a consumer; advances the consumer cursor. |
| **Channel (queue + pub/sub)** | |
| `host.Ch().Send(ctx, name, msgType, payload)` | Enqueue a message (queue semantics — one consumer receives it). Returns message ID. |
| `host.Ch().SendWithOptions(ctx, name, msgType, payload, delayMs, ttlMs, headers)` | Enqueue with delay, TTL, and custom headers. |
| `host.Ch().Receive(ctx, name, timeoutMs)` | Dequeue one message (blocks up to `timeoutMs`). Returns `(msg, ok, err)`. |
| `host.Ch().Ack(ctx, name, msgID)` | Acknowledge successful processing; prevents redelivery. |
| `host.Ch().Nack(ctx, name, msgID, requeue)` | Negative-acknowledge; `requeue=true` retries, `false` dead-letters. |
| `host.Ch().Publish(ctx, name, msgType, payload)` | Publish (pub/sub — all subscribers receive it). Returns message ID. |
| `host.Ch().Subscribe(ctx, name, filter)` | Subscribe; `filter` is a message-type pattern (empty = all). Returns subscription ID. |
| `host.Ch().Unsubscribe(subscriptionID)` | Cancel a subscription. |
| `host.Ch().Create(ctx, name, maxSize, ttlMs)` | Create a channel (no-op if already exists). |
| `host.Ch().Delete(ctx, name)` | Delete a channel and all pending messages. |
| `host.Ch().Depth(ctx, name)` | Return number of pending (unacked) messages. |

### WASM Component Model Architecture

The Go SDK implements the WASM Component Model canonical ABI directly in `exports.go`:

- **`cabi_realloc`**: Memory allocation for host-to-guest string passing
- **Qualified export names**: `plexspaces:actor/actor@0.1.0#init`, `#handle`, `#get-state`, `#set-state`
- **Raw uint32 signatures**: String parameters as `(ptr, len)` pairs, returns via 8-byte return area
- **`cabi_post_*` cleanup functions**: Called by host after reading return values

This eliminates the need for `--dummy-names legacy` in `wasm-tools component embed` and ensures proper matching between TinyGo exports and the WIT interface.

### Testing

The SDK includes comprehensive tests (`plexspaces_test.go`) that run natively (not in WASM) using stub implementations of all host functions. Tests cover:
- Actor interface and BaseActor state round-trip
- ActorRouter prefix matching and longest-match-wins logic
- Host function stubs for all capabilities (KV, PG, Locks, Blobs, TupleSpace)
- Error handling and HostError type

### Examples

| Example | Pattern | Actors | Features |
|---------|---------|--------|----------|
| `migrating_erlang_otp` | Erlang GenServer | 1 (RateLimiter) | Sliding window algorithm, benchmarking |
| `migrating_cloudflare_workers` | Cloudflare Durable Objects | 2 (ChatRoom, RateLimiter) | ActorRouter, KV store, fan-out |
| `migrating_gosiris` | Gosiris Actor Model | 2 (Sensor, Aggregator) | Process groups, polling, anomaly detection |

---

## Collective / Parallel Shard-Group APIs

All SDKs expose five collective operations that map directly to MPI-style
primitives.  These are high-level wrappers over shard groups; the framework
handles fan-out, reduction, and broadcast internally.  Business logic stays in
`crates/actor/src/parallel.rs`; the SDK layers pass through to the actor
service without duplicating reduction logic.

### MPI → PlexSpaces API Mapping

| MPI Operation | PlexSpaces API | Semantics |
|---------------|---------------|-----------|
| `MPI_Bcast` | `BroadcastShardGroup` | Leader → all shards (fan-out) |
| `MPI_Scatter` + `MPI_Gather` | `ScatterGather` | Per-shard query, aggregate results |
| `MPI_Reduce` | `ReduceShardGroup` | Map + built-in reduction; result at leader |
| `MPI_Allreduce` | `AllReduceShardGroup` | Reduce + broadcast result to ALL shards |
| `MPI_Barrier` | `BarrierShardGroup` | Synchronise all shards at a named point |
| `MPI_Comm_spawn` | `SpawnActors` | Batch actor creation (N replicas via `instances_count`) |

### Built-in Reductions

`ReduceShardGroup` and `AllReduceShardGroup` accept these `reduction` strings:
`"sum"`, `"min"`, `"max"`, `"product"`, `"concat"`, `"bool_and"`, `"bool_or"`.

### Cross-SDK Examples

The same benchmark across all four SDKs — see
`examples/go/apps/mpi_collectives/README.md` for the full architecture and
sequence diagrams.

**Go (WASM)**
```go
// Phase 1: MPI_Bcast
host.BroadcastShardGroup(map[string]any{
    "group_id": groupID, "message_type": "apply_broadcast",
    "message": map[string]any{"round": r, "scale": scale},
    "min_acks": workerCount, "timeout_ms": 30000,
})

// Phase 3: MPI_Reduce
resp, _ := host.ReduceShardGroup(map[string]any{
    "group_id": groupID, "message_type": "partial_reduce",
    "map_function": map[string]any{"round": r},
    "target": "partial_sum", "reduction": "sum",
    "min_responses": workerCount, "timeout_ms": 30000,
})
total := resp["result"].(float64)

// Phase 4: MPI_Allreduce (reduce + broadcast "event" to all workers)
host.AllReduceShardGroup(map[string]any{
    "group_id": groupID, "message_type": "partial_reduce",
    "map_function": map[string]any{"round": r},
    "target": "partial_sum", "reduction": "sum",
    "min_responses": workerCount, "timeout_ms": 30000,
})
// Workers receive message_type="event" with payload=global_sum

// Phase 5: MPI_Barrier
host.BarrierShardGroup(map[string]any{
    "group_id": groupID, "barrier_id": "barrier-round-0",
    "round": uint64(r), "min_acks": workerCount, "timeout_ms": 30000,
})
```

**Python (WASM)**
```python
# MPI_Reduce
resp = host.reduce_shard_group({
    "group_id": group_id, "message_type": "partial_reduce",
    "map_function": {"round": r},
    "target": "partial_sum", "reduction": "sum",
    "min_responses": worker_count,
})
total = resp["result"]
```

**TypeScript (WASM)**
```typescript
const resp = host.reduceShardGroup({
    groupId: groupId, messageType: "partial_reduce",
    mapFunction: { round: r },
    target: "partial_sum", reduction: "sum",
    minResponses: workerCount,
});
const total = resp.result;
```

**Rust (SDK)**
```rust
let resp = shard_client.reduce(
    &group_id,
    call_message(json!({ "round": r })),
    CollectiveReduction::CollectiveReductionSum,
    Some("partial_sum"),
    worker_count,
).await?;
```

### Worker-side Dispatch

Workers dispatch on `message_type` in their `Handle` method:

```go
func (w *WorkerActor) Handle(from, msgType, payload string) string {
    switch msgType {
    case "apply_broadcast":  // BroadcastShardGroup
        ...
    case "process_scatter_chunk": // ScatterGather
        ...
    case "partial_reduce":   // ReduceShardGroup / AllReduceShardGroup map phase
        return marshal(map[string]any{"partial_sum": w.LastPartialSum, ...})
    case "event":            // AllReduceShardGroup broadcast-back
        // payload is the JSON-encoded reduced value (e.g. 42.5)
        w.LastReducedSum = parseFloat(payload)
        ...
    }
}
```

`AllReduceShardGroup` broadcasts the reduction result back to all workers with
`message_type="event"` and header `plexspaces-collective-op=all-reduce-result`.

---

## Remote API Boundary

Remote access is provided by the framework's proto-first HTTP/gRPC APIs, not by reimplementing framework behavior inside the SDKs. The intended layering is:

- **Core crates** own business logic, actor lifecycle, routing, durability, placement, and coordination.
- **SDKs** are local decorators that remove boilerplate for embedded and WASM usage through ServiceLocator and WIT.
- **Remote clients** are generated or hand-authored against the proto contracts and call the thin HTTP/gRPC service layer when out-of-process access is required.

This keeps local SDK use fast and consistent, preserves one implementation of the framework semantics, and lets Python, TypeScript, Go, and Rust share the same proto-first data model for remote interoperability.

This allows external applications to interact with PlexSpaces without being WASM actors.

---

## Comparison with Other Frameworks

| Feature | PlexSpaces SDK | Ray | Temporal | Orleans |
|---------|---------------|-----|----------|---------|
| Decorator | `@actor` | `@ray.remote` | `@workflow.defn` | Interface |
| State | `state()` | Class attrs | N/A | Grain state |
| Handlers | `@handler()` | Methods | `@activity.defn` | Methods |
| Build | WASM | Python | Docker | .NET |
| Runtime | PlexSpaces | Ray Cluster | Temporal Server | Orleans Silo |
| Multi-language | ✅ WASM | Python only | ✅ | .NET only |

---

## SDK Directory Structure

```
sdks/
├── README.md              # Overview
├── python/
│   ├── plexspaces/        # SDK package
│   │   ├── generated/     # Proto-generated (betterproto); from make proto / make proto-python
│   │   ├── __init__.py    # Exports
│   │   ├── decorators.py  # @actor, @handler, state()
│   │   ├── workflow.py    # RetryConfig + helpers (uses generated when available)
│   │   ├── runtime.py     # WIT wrapper generator
│   │   └── host.py        # Host function wrappers
│   ├── plexspaces_cli/    # CLI tool
│   │   └── build.py       # plexspaces-py build
│   ├── examples/          # SDK examples
│   ├── tests/             # Unit tests (pytest)
│   ├── pyproject.toml     # Package config
│   └── README.md          # Python SDK docs
├── typescript/            # TypeScript SDK (inheritance-based)
│   ├── src/
│   │   ├── generated/proto/  # ts-proto output; from make proto / make proto-typescript
│   │   ├── proto.ts       # Re-exports selected proto types
│   │   ├── actor.ts       # PlexSpacesActor base class
│   │   ├── host.ts        # Host function wrappers
│   │   └── index.ts       # Exports
│   ├── package.json
│   └── README.md          # TypeScript SDK docs
├── rust/                  # Rust SDK (native and WASM actor decorators; workspace members)
│   ├── plexspaces-sdk/    # Re-exports, spawn helpers, typed refs, WASM-safe SDK surface
│   └── plexspaces-sdk-macros/  # #[actor], #[gen_server_actor], #[plexspaces_handlers], etc.
└── go/                    # Go SDK (TinyGo WASM actors)
    └── plexspaces/
        ├── proto/         # Generated Go packages (make proto / make proto-go)
        ├── actor.go       # Actor interface + BaseActor
        ├── host.go        # Host function wrappers (Host singleton); ParseErrorDetail
        ├── host_imports.go # WIT wasmimport directives (TinyGo)
        ├── host_stubs.go  # Native/test stub implementations
        ├── exports.go     # WASM Component Model canonical ABI exports
        ├── router.go      # ActorRouter for multi-actor modules
        └── plexspaces_test.go # Unit tests
```

---

## See Also

- [Python SDK README](../sdks/python/README.md) - Python SDK details
- [TypeScript SDK README](../sdks/typescript/README.md) - TypeScript SDK details
- [Go SDK README](../sdks/go/README.md) - Go SDK entry point
- [Testing Guide](testing.md) - `make test` (workspace + polyglot SDK tests)
- [WASM Deployment Guide](wasm-deployment.md) - Deploying WASM actors (Python, TypeScript, Rust, Go)
- [Polyglot Development Guide](polyglot.md) - WASM development in multiple languages
- [Getting Started](getting-started.md) - Quick start guide
- [Examples](../examples/README.md) - Example gallery (including Rust SDK examples)
- [Architecture](architecture.md) - System architecture
- [Detailed Design](detailed-design.md) - Behaviors (crates/behavior), facets (BuiltInFacetType, impl locations)
- [Behavior crate README](../crates/behavior/README.md) - All behaviors defined in mod.rs; call/cast and GenServer default
