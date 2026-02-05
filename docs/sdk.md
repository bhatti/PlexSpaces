# PlexSpaces SDKs

PlexSpaces provides language-specific SDKs for building actors with minimal boilerplate. The SDKs are inspired by industry-leading frameworks like [Ray](https://docs.ray.io/en/latest/ray-core/api/doc/ray.remote.html), [Temporal](https://docs.temporal.io/), and [Orleans](https://learn.microsoft.com/en-us/dotnet/orleans/).

## Available SDKs

| Language | Status | Location | Build Target |
|----------|--------|----------|--------------|
| **Python** | ✅ Available | `sdks/python/` | WASM actors (componentize-py) |
| **TypeScript** | ✅ Available | `sdks/typescript/` | WASM actors (jco componentize) |
| **Rust** | ✅ Available | `sdks/rust/plexspaces-sdk` | Native (embedded) actors; annotations + spawn_actor + facets |
| Go | 📋 Planned | `sdks/go/` | WASM actors |

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

### API Reference

#### Decorators

| Decorator | Description | Example |
|-----------|-------------|---------|
| `@actor` | Define a PlexSpaces actor class (GenServer) | `@actor class MyActor:` |
| `@actor(facets=[...])` | Actor with facet declaration | `@actor(facets=["durability"]) class DurableActor:` |
| `@event_actor` | Event-handler (GenEvent): fire-and-forget, no request-reply | `@event_actor class AuditLog:` |
| `@fsm_actor` | FSM actor (GenStateMachine): stateful transitions | `@fsm_actor class OrderFSM:` |
| `@gen_server_actor` | Explicit GenServer (same as `@actor`) | `@gen_server_actor class Worker:` |
| `@workflow_actor` | Workflow/orchestration actor | `@workflow_actor class Pipeline:` |
| `@handler(*msg_types)` | Route messages to this method | `@handler("deposit")` |
| `state(default=None, default_factory=None)` | Define persistent state field | `balance: int = state(default=0)` |
| `@init_handler` | Custom initialization handler | `@init_handler def on_init(self, config):` |

#### Behavior Types

All behavior decorators support an optional `facets` parameter:

| Decorator | Behavior | Use Case | Invocation |
|-----------|----------|----------|------------|
| `@actor` | GenServer | Request-reply actors (default) | Auto `call` |
| `@gen_server_actor` | GenServer | Explicit GenServer | Auto `call` |
| `@event_actor` | GenEvent | Fire-and-forget event handlers | `cast` |
| `@fsm_actor` | GenStateMachine | State machine workflows | Auto `call` |
| `@workflow_actor` | Workflow | Long-running orchestrations | Auto `call` |

**GenServer Auto-Invocation**: When using `@actor` or `@gen_server_actor`, all handlers automatically use `invocation="call"` (request-reply). You don't need to specify it explicitly:

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

@fsm_actor(facets=["durability", "registry"])
class OrderWorkflow:
    current_state: str = state(default="idle")
```

| Facet | WASM Behavior | Rust Behavior |
|-------|---------------|---------------|
| `durability` | Checkpoint-based persistence via `WasmConfig.durability_enabled` | `DurabilityFacet` attachment |
| `registry` | Service discovery via `RegistryFacet` in app-config | `RegistryFacet` attachment |

**WASM Durability**: WASM actors use checkpoint-based persistence (get_state/set_state pattern), not the Rust `DurabilityFacet`. The `facets=["durability"]` declaration documents that the actor expects durability to be enabled via `durability_enabled: true` in release.yaml or WasmConfig. For full mapping (facets → WasmConfig, where to set it), see [Durability: WASM Actor Durability and Durability Facet Parameter](durability.md#durability-facet-parameter-python-sdk).

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
| `host.send(to, msg_type, payload)` | Send message to another actor |
| `host.log(level, message)` | Log a message |
| `host.info(message)` | Log info message |
| `host.debug(message)` | Log debug message |
| `host.warn(message)` | Log warning message |
| `host.error(message)` | Log error message |
| `host.now_ms()` | Get current timestamp (ms) |
| **Key-Value Storage** | |
| `host.kv_get(key)` | Get value by key. Returns value string or empty if not found. |
| `host.kv_put(key, value)` | Store key-value pair. Returns empty on success. |
| `host.kv_delete(key)` | Delete key. Returns empty on success. |
| `host.kv_list(prefix)` | List keys with prefix. Returns JSON array of keys. |
| **TupleSpace** | |
| `host.ts_write(tuple_json)` | Write tuple (JSON array). Returns empty on success. |
| `host.ts_read(pattern_json)` | Read tuple (non-destructive). Returns matched tuple or empty. |
| `host.ts_take(pattern_json)` | Take tuple (destructive). Returns and removes matched tuple. |
| `host.ts_read_all(pattern_json)` | Read all matching tuples. Returns JSON array of tuples. |
| **Distributed Locks** | |
| `host.lock_acquire(lock_id, timeout_ms)` | Acquire lock. Returns lock version on success. |
| `host.lock_release(lock_id, lock_version)` | Release lock. Returns empty on success. |
| **Blob Storage** | |
| `host.blob_upload(path, data, content_type)` | Upload blob (base64 data). Returns empty on success. |
| `host.blob_download(path)` | Download blob. Returns base64 data or empty if not found. |
| `host.blob_delete(path)` | Delete blob. Returns empty on success. |
| `host.blob_list(prefix)` | List blobs by prefix. Returns JSON array of blob IDs. |
| **Process Groups** | |
| `host.process_groups.join(group, actor_id)` | Join a process group |
| `host.process_groups.leave(group, actor_id)` | Leave a process group |
| `host.process_groups.publish(group, message)` | Broadcast to group |
| `host.process_groups.get_members(group)` | Get group members |

#### Key-Value Storage (WASM)

WASM actors can persist data via **`host.kv_get`** and **`host.kv_put`**. Keys are scoped per actor. The node provides an in-memory keyvalue store for WASM by default. Use this for sensor buffers, caches, or any key-value state without relying on in-actor state serialization. Full keyvalue API (TTL, list-keys, etc.) will be added to the SDKs later. See [WASM Deployment: Key-Value Storage](wasm-deployment.md#key-value-storage-wasm).

#### TupleSpace (WASM)

WASM actors can use TupleSpace for coordination via Linda-style primitives:

```python
import json
from plexspaces import actor, handler, host

@actor
class JobCoordinator:
    @handler("submit")
    def submit_job(self, job_id: str, tasks: list) -> dict:
        # Scatter: Write tasks to TupleSpace
        for i, task in enumerate(tasks):
            tuple_json = json.dumps(["job", job_id, "task", i, task])
            host.ts_write(tuple_json)
        return {"job_id": job_id, "tasks": len(tasks)}
    
    @handler("claim")
    def claim_task(self, job_id: str) -> dict:
        # Atomic claim: ts_take removes the tuple
        pattern = json.dumps(["job", job_id, "task", None, None])
        result = host.ts_take(pattern)
        if result:
            task = json.loads(result)
            return {"task_id": task[3], "data": task[4]}
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

### Local Development with MinIO

For blob storage testing, run MinIO locally:

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
    storage_type: s3
    bucket: plexspaces-blobs  # Must create this bucket in MinIO first
    endpoint: http://localhost:9000
    region: us-east-1
    access_key_id: minioadmin
    secret_access_key: minioadmin
    force_path_style: true
```

### Migration from Legacy Examples

If you have existing actors using the WIT interface directly:

**Before (Legacy - 150+ lines of boilerplate)**
```python
from wit_world import exports
import json

_balance = 0

class Actor(exports.Actor):
    def init(self, config_json: str) -> str:
        global _balance
        _balance = 0
        return ""
    
    def handle(self, from_actor: str, msg_type: str, payload_json: str) -> str:
        global _balance
        data = json.loads(payload_json)
        if msg_type == "deposit":
            _balance += data["amount"]
            return json.dumps({"balance": _balance})
        # ... lots of boilerplate
    
    def get_state(self) -> str:
        return json.dumps({"balance": _balance})
    
    def set_state(self, state_json: str) -> str:
        global _balance
        _balance = json.loads(state_json)["balance"]
        return ""
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

---

## TypeScript SDK

The TypeScript SDK uses **inheritance** instead of decorators: extend `PlexSpacesActor<TState>` and implement `getDefaultState()` plus `on<Op>(payload)` handlers. Same WIT world as Python (`plexspaces-simple-actor`).

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
- Build WASM: `jco componentize your-bundle.mjs --wit wit/plexspaces-simple-actor -o actor.wasm --disable all`

The `--disable all` flag ensures the component only imports `plexspaces:simple-actor/host` (no WASI), matching the PlexSpaces runtime linker.

### API (TypeScript)

| API | Description |
|-----|-------------|
| `PlexSpacesActor<TState>` | Base class. `TState` is your state shape (plain object). |
| `getDefaultState(): TState` | Override to return initial state. |
| `onInit(config)` | Optional. Called from `init()` with parsed config. |
| `on<Op>(payload)` | Handler for message op (e.g. `onDeposit`, `onBalance`). Dispatch is by `payload.op`. |
| `protected state: TState` | Current state; read/write in handlers. |
| `protected json(obj)`, `error(msg)` | Helpers for returning JSON or error strings. |

Observability (metrics, tracing) for WASM actors is provided by the PlexSpaces runtime; the TypeScript SDK does not add its own. See [sdks/typescript/README.md](../sdks/typescript/README.md) and [examples/typescript/apps/bank_account](../examples/typescript/apps/bank_account/README.md) for a full example and E2E test.

---

## Rust SDK

The Rust SDK provides **Python-style annotations** to eliminate boilerplate. Use attribute macros like `#[gen_server_actor]`, `#[handler("op")]`, and `#[plexspaces_handlers]` for clean, declarative actor definitions. Use it for native (embedded) Rust actors; WASM Rust actors follow the same WIT world as Python/TypeScript.

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
| `#[workflow_actor]` | `@workflow_actor` | `impl Actor` with Workflow behavior |

#### Handler Annotations

| Rust Annotation | Python Equivalent | Generated Code |
|-----------------|-------------------|----------------|
| `#[handler("op")]` | `@handler("op")` | Route `msg_type == "op"` to this method (GenServer=call) |
| `#[handler("op", call)]` | `@handler("op", "call")` | Explicit call semantics (request-reply) |
| `#[handler("op", cast)]` | `@handler("op", "cast")` | Explicit cast semantics (fire-and-forget) |
| `#[init_handler]` | `@init_handler` | Called on actor initialization |
| `#[run_handler]` | N/A | Workflow main execution handler |
| `#[signal_handler("name")]` | N/A | Workflow signal handler |
| `#[query_handler("name")]` | N/A | Workflow query handler (read-only) |

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
| `spawn_actor(ctx, sl, id, ns, actor, facets)` | Spawn actor with facets (like Python `@actor(facets=[...])`) |
| `create_facets(&["timer", "durability"], &config)` | Create facet instances from names (convenience helper) |

### Example: GenServer with annotations (webhook_handler-style)

```rust
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, spawn_actor, json, Value,
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

// Spawn with facets
let actor_ref = spawn_actor(&ctx, service_locator, actor_id, "webhooks", WebhookHandler::new(), vec![]).await?;
```

### Example: Custom actor with fire-and-forget handlers (session_manager-style)

```rust
use plexspaces_sdk::{
    actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, spawn_actor, TimerFacet, json,
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

// Spawn with TimerFacet
let timer_facet = TimerFacet::new(json!({}), 50);
let actor_ref = spawn_actor(&ctx, sl, actor_id, "sessions", SessionActor::new("user-123"), vec![Box::new(timer_facet)]).await?;
```

### Timer vs Reminder: Transient vs Durable Scheduling

PlexSpaces follows the **Orleans model** for time-based operations:

| Facet | Durability | Storage | Message Type | Use Case |
|-------|------------|---------|--------------|----------|
| `TimerFacet` | **Transient** (in-memory) | None | `timer_fired` | Heartbeats, timeouts, health checks |
| `ReminderFacet` | **Durable** (persisted) | `Arc<dyn JournalStorage>` | `reminder_fired` | Billing, SLA, scheduled tasks |

**The naming convention IS the API contract:**
- **Timer** = transient, fast, no persistence overhead, lost on crash
- **Reminder** = durable, requires storage, survives crashes

```rust
use plexspaces_journaling::{TimerFacet, ReminderFacet, SqliteJournalStorage};
use plexspaces_core::JournalStorage;

// TimerFacet - TRANSIENT (no storage required)
let timer_facet = TimerFacet::new(json!({}), 50);

// ReminderFacet - DURABLE (requires storage)
let storage: Arc<dyn JournalStorage> = Arc::new(
    SqliteJournalStorage::new(":memory:").await?
);
let reminder_facet = ReminderFacet::new(storage, json!({}), 50);

// Spawn actor with both facets
let actor_ref = spawn_actor(
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
        // ctx.run(|| ...), ctx.sleep(), ctx.promise(), etc.
        
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

### Legacy macros (backward compatibility)

The following are still supported but prefer the new annotations:

| Legacy API | Replacement |
|------------|-------------|
| `#[derive(PlexSpacesActor)]` | `#[actor]` or `#[gen_server_actor]` |
| `plexspaces_impl_handlers!(Actor, behavior, ...)` | `#[plexspaces_handlers]` + `#[handler(...)]` |

### Rust examples using the SDK

- **webhook_handler** — `#[gen_server_actor]`, `#[plexspaces_handlers]`, HTTP deliver/list; `examples/rust/embedded/webhook_handler/`
- **session_manager** — `#[actor]`, `#[plexspaces_handlers(custom)]`, TimerFacet; `examples/rust/apps/session_manager/`
- **timeseries_forecasting** — uses ActorFactory (pipeline by type name); see its README for when to use Factory vs SDK `spawn_actor`

Going forward, new Rust examples should use these SDK annotations and `spawn_actor` with facets where applicable. See [Examples](examples.md) and [Detailed Design - Behaviors](detailed-design.md#behaviors).

---

## Future: gRPC Client SDK

The SDK will be extended to support gRPC API clients (not just WASM actors):

**Why gRPC?**
- Proto-first design means we already have message definitions
- Can auto-generate clients for Python, TypeScript, Go, etc.
- Same API for both WASM actors and external clients

**Planned Features**:
```python
# Future: gRPC client mode
from plexspaces import Client

async def main():
    client = Client("localhost:8094")
    
    # Invoke actor (GET = request-reply; POST/PUT/DELETE = fire-and-forget unless invocation=call). Valid invocation: call, cast, info.
    result = await client.actors.invoke(
        namespace="default",
        actor_type="bank_account", 
        msg_type="deposit",
        payload={"amount": 100}
    )
    
    # Use TupleSpace
    await client.tuplespace.write("orders", {"id": "123", "status": "pending"})
    order = await client.tuplespace.read("orders", {"id": "123"})
```

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
│   │   ├── __init__.py    # Exports
│   │   ├── decorators.py  # @actor, @handler, state()
│   │   ├── runtime.py     # WIT wrapper generator
│   │   └── host.py        # Host function wrappers
│   ├── plexspaces_cli/    # CLI tool
│   │   └── build.py       # plexspaces-py build
│   ├── examples/          # SDK examples
│   ├── tests/             # Unit tests
│   ├── pyproject.toml     # Package config
│   └── README.md          # Python SDK docs
├── typescript/            # TypeScript SDK (inheritance-based)
│   ├── src/
│   │   ├── actor.ts       # PlexSpacesActor base class
│   │   └── index.ts       # Exports
│   ├── package.json
│   └── README.md          # TypeScript SDK docs
├── rust/                  # Rust SDK (native embedded actors)
│   ├── plexspaces-sdk/    # Re-exports, spawn_actor, plexspaces_impl_handlers!
│   └── plexspaces-sdk-macros/  # #[derive(PlexSpacesActor)]
└── go/                    # Planned
```

---

## See Also

- [Python SDK README](../sdks/python/README.md) - Python SDK details
- [TypeScript SDK README](../sdks/typescript/README.md) - TypeScript SDK details
- [WASM Deployment Guide](wasm-deployment.md) - Deploying WASM actors (Python, TypeScript, Rust, Go)
- [Polyglot Development Guide](polyglot.md) - WASM development in multiple languages
- [Getting Started](getting-started.md) - Quick start guide
- [Examples](examples.md) - Example gallery (including Rust SDK examples)
- [Architecture](architecture.md) - System architecture
- [Detailed Design](detailed-design.md) - Behaviors (crates/behavior), facets (BuiltInFacetType, impl locations)
- [Behavior crate README](../crates/behavior/README.md) - All behaviors defined in mod.rs; call/cast and GenServer default
