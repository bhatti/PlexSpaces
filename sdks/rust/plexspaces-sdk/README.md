# PlexSpaces Rust SDK

High-level API and annotations for building PlexSpaces actors in Rust, inspired by Erlang/OTP patterns and industry best practices.

## Table of Contents

- [Overview](#overview)
- [Quick Start](#quick-start)
- [Actor Annotations](#actor-annotations)
- [Handler Annotations](#handler-annotations)
- [Message Types and Ask/Tell Semantics](#message-types-and-asktell-semantics)
- [Handler Dispatch Mechanism](#handler-dispatch-mechanism)
- [Spawning Actors](#spawning-actors)
- [Message Creation Helpers](#message-creation-helpers)
- [Communication Patterns](#communication-patterns)
- [Parallel Operations](#parallel-operations)
- [Leader-worker (multi-node)](#leader-worker-multi-node)
- [Best Practices](#best-practices)
- [Examples](#examples)

## Tests

This crate is a **workspace member**. Full verification from the repository root:

```bash
make test   # includes plexspaces-sdk and plexspaces-sdk-macros with the rest of the workspace
```

For a fast loop on this crate only: `cargo test -p plexspaces-sdk --all-features`.

## Overview

The PlexSpaces Rust SDK provides:

- **Attribute macros** (like Python decorators) to reduce boilerplate
- **Erlang/OTP-inspired behaviors**: GenServer (request-reply), GenEvent (fire-and-forget), GenStateMachine (FSM), Workflow (durable)
- **Type-safe message passing** with `ActorRef::ask()` and `ActorRef::tell()`
- **Automatic handler dispatch** based on operation names
- **Unified shard group client** for data-parallel operations

## Quick Start

```rust
use plexspaces_sdk::*;

#[gen_server_actor]
struct Counter {
    count: i32,
}

#[plexspaces_handlers]
impl Counter {
    #[handler("increment")]
    async fn increment(&mut self, _ctx: &ActorContext, _msg: &Message) 
        -> Result<Value, BehaviorError> 
    {
        self.count += 1;
        Ok(json!({ "count": self.count }))
    }
    
    #[handler("get")]
    async fn get(&mut self, _ctx: &ActorContext, _msg: &Message) 
        -> Result<Value, BehaviorError> 
    {
        Ok(json!({ "count": self.count }))
    }
}

// Spawn actor
let ctx = RequestContext::new_without_auth("tenant".into(), "namespace".into());
let actor_ref = spawn(&ctx, service_locator, actor_id, "ns", Counter { count: 0 }).await?;

// Request-reply (ask)
let request = call_message(json!({ "action": "increment" }));
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;

// Fire-and-forget (tell)
let event = cast_message(json!({ "action": "increment" }));
actor_ref.tell(event).await?;
```

## Actor Annotations

Actor annotations define the behavior type and optional facets:

| Annotation | Behavior Type | Use Case | Default Pattern |
|------------|---------------|----------|-----------------|
| `#[gen_server_actor]` | GenServer | Request-reply actors | call |
| `#[gen_server_actor(facets = ["timer", "durability"])]` | GenServer with facets | Request-reply with capabilities | call |
| `#[event_actor]` | GenEvent | Fire-and-forget events | cast |
| `#[fsm_actor]` | GenStateMachine | State machine workflows | call |
| `#[workflow_actor]` | Workflow | Durable orchestrations | call |
| `#[actor]` | Custom | User-defined behavior | varies |

### Facets

Facets add capabilities to actors:

- `"virtual_actor"` - Virtual actor (suspends when idle, reactivates on message). With `#[gen_server_actor(facets = ["virtual_actor", ...])]`, **`spawn_with_facets`** registers the actor definition through the shared core registration path, preserving behavior kind plus proto-shaped facet metadata/config so reactivation matches the original registration after vacation.
- `"durability"` - Durable state (persisted to storage)
- `"timer"` - Timer support (periodic tasks, timeouts)
- `"supervisor"` - Supervisor tree (fault tolerance)

**Example:**

```rust
#[gen_server_actor(facets = ["virtual_actor", "durability", "timer")]
struct BankAccount {
    balance: i64,
}
```

## Handler Annotations

Handler annotations mark methods as message handlers and control dispatch:

| Annotation | Semantics | Return Type | Use With |
|------------|-----------|-------------|----------|
| `#[handler("op")]` | call (GenServer default) | `Result<Value, BehaviorError>` | Request-reply |
| `#[handler("op", call)]` | Explicit call | `Result<Value, BehaviorError>` | Request-reply |
| `#[handler("op", cast)]` | Fire-and-forget | `Result<(), BehaviorError>` | Fire-and-forget |
| `#[handler("*")]` | Catch-all | `Result<Value, BehaviorError>` | Any operation |

### Handler Dispatch Rules

1. **GenServer actors**: Handlers default to `call` (request-reply) unless `cast` is specified
2. **GenEvent actors**: Handlers default to `cast` (fire-and-forget) unless `call` is specified
3. **Catch-all handler**: `#[handler("*")]` matches any operation not matched by specific handlers

**Example:**

```rust
#[plexspaces_handlers]
impl MyActor {
    // Request-reply (default for GenServer)
    #[handler("get_balance")]
    async fn get_balance(&mut self, _ctx: &ActorContext, msg: &Message) 
        -> Result<Value, BehaviorError> 
    {
        Ok(json!({ "balance": self.balance }))
    }
    
    // Fire-and-forget (explicit cast)
    #[handler("log_event", cast)]
    async fn log_event(&mut self, _ctx: &ActorContext, _msg: &Message) 
        -> Result<(), BehaviorError> 
    {
        // Log event, no reply needed
        Ok(())
    }
    
    // Catch-all handler (for worker actors)
    #[handler("*")]
    async fn process(&mut self, _ctx: &ActorContext, msg: &Message) 
        -> Result<Value, BehaviorError> 
    {
        let payload: Value = serde_json::from_slice(&msg.payload)?;
        let action = payload["action"].as_str().unwrap_or("unknown");
        match action {
            "task1" => { /* ... */ }
            "task2" => { /* ... */ }
            _ => Err(BehaviorError::ProcessingError(format!("Unknown action: {}", action)))
        }
    }
}
```

## Message Types and Ask/Tell Semantics

PlexSpaces follows Erlang/OTP patterns for message passing:

### Message Patterns

| Pattern | Erlang Equivalent | Semantics | Use With | Handler Method |
|------------|-------------------|-----------|----------|----------------|
| `call` | `gen_server:call/2` | Request-reply (synchronous, reply expected) | `ActorRef::ask()` | `handle_request()` |
| `cast` | `gen_server:cast/2` | Fire-and-forget (asynchronous, reply optional) | `ActorRef::tell()` | `handle_request()` |
| `info` | `gen_server:handle_info/2` | Async message (no reply) | Internal use | `handle_info()` |

**Note**: GenServer uses a single `handle_request()` method for both `call` and `cast`. The difference is:
- **call**: Reply is expected and sent automatically by the SDK macro
- **cast**: Reply is optional (fire-and-forget semantics)

### Message Type Field

The `Message.message_type` field serves two purposes:

1. **Message pattern** (when set to `"call"` or `"cast"`): Controls routing to `handle_request()` vs `handle_cast()` in GenServer
2. **Operation name** (when set to other values): Used directly as the operation name for handler dispatch

### HTTP Gateway Mapping

The HTTP Gateway now separates ask and tell behavior by endpoint:

| HTTP Endpoint | `message_type` | Behavior |
|-------------|----------------|----------|
| `GET /api/v1/actors/{namespace}/{actor_type}` | `"call"` | Request-reply |
| `GET /api/v1/actors/{namespace}/{actor_type}/ask` | `"call"` | Request-reply |
| `POST/PUT /api/v1/actors/{namespace}/{actor_type}` | `"cast"` | Fire-and-forget |
| `POST/PUT /api/v1/actors/{namespace}/{actor_type}/ask` | `"call"` | Request-reply |

**Example HTTP requests:**

```bash
# Request-reply
GET /api/v1/actors/default/my-actor?action=get_balance
POST /api/v1/actors/default/my-actor/ask -d '{"action": "get_balance"}'

# Fire-and-forget
POST /api/v1/actors/default/my-actor -d '{"action": "log_event"}'
```

## Handler Dispatch Mechanism

The SDK macro (`#[plexspaces_handlers]`) extracts the operation name from the message payload and dispatches to the appropriate handler.

### Operation Extraction

When `message_type` is `"call"` or `"cast"` (message invocations), the operation name is extracted from the payload. **Canonical key across all SDKs is `message_type`**; aliases `op` and `msg_type` (order: message_type → op → msg_type). Rust SDK also accepts:

1. `payload.action` (preferred in Rust)
2. `payload.op` (fallback)
3. `payload.msg_type` (fallback)

When `message_type` is NOT `"call"` or `"cast"`, `message_type` itself is used as the operation name.

### Dispatch Flow

```
Message arrives
    ↓
Check message_type:
    ├─ "call" or "cast" → Extract operation from payload.action/op/msg_type
    └─ Other → Use message_type as operation name
    ↓
Match handler:
    ├─ #[handler("extracted_op")] → Call handler
    ├─ #[handler("*")] → Call catch-all handler
    └─ No match → Return BehaviorError::UnsupportedMessage
    ↓
Handler executes:
    ├─ call semantics → Serialize return value, send reply via ctx.send_reply()
    └─ cast semantics → No reply sent
```

### Example Dispatch

```rust
// Message: message_type="call", payload={"action": "increment", "amount": 10}
// → Extracts operation: "increment"
// → Matches: #[handler("increment")]
// → Calls: handle_increment()

#[handler("increment")]
async fn handle_increment(&mut self, _ctx: &ActorContext, msg: &Message) 
    -> Result<Value, BehaviorError> 
{
    let payload: Value = serde_json::from_slice(&msg.payload)?;
    let amount = payload["amount"].as_i64().unwrap_or(1);
    self.count += amount;
    Ok(json!({ "count": self.count }))
}
```

## Spawning Actors

The SDK provides helper functions for spawning actors:

### Basic Spawn

```rust
use plexspaces_sdk::{spawn, RequestContext};

let ctx = RequestContext::new_without_auth("tenant".into(), "namespace".into());
let actor_ref = spawn(&ctx, service_locator, actor_id, "ns", Counter { count: 0 }).await?;
```

Uses facets declared in `#[gen_server_actor(facets = [...])]` annotation.

### Spawn with Explicit Facets

```rust
use plexspaces_sdk::{spawn_with_facets, create_facets};

let facets = create_facets(&["timer", "durability"], &config)?;
let actor_ref = spawn_with_facets(
    &ctx, 
    service_locator, 
    actor_id, 
    "ns", 
    actor, 
    facets
).await?;
```

### Spawn with Storage

```rust
use plexspaces_sdk::spawn_with_storage;
use plexspaces_journaling::SqliteJournalStorage;

let storage = Arc::new(SqliteJournalStorage::new(":memory:").await?);
let actor_ref = spawn_with_storage(
    &ctx,
    service_locator,
    actor_id,
    "ns",
    actor,
    storage,
).await?;
```

### Virtual Actor Auto-Registration

When spawning an actor with the `virtual_actor` facet, the SDK automatically registers that actor type for automatic activation. This enables Orleans-style virtual actor behavior where any actor ID matching the type pattern can be activated on-demand.

**How it works:**

1. When you spawn an actor with `virtual_actor` facet (via `spawn()`, `spawn_with_facets()`, or `spawn_with_storage()`), the SDK:
   - Detects the `virtual_actor` facet
   - Extracts the actor type from `behavior.behavior_type()` (e.g., `"GenServer"`, `"CustomActor"`)
   - Registers the actor type with `VirtualActorManager` for automatic activation

2. After registration, any message sent to an actor ID of that type will automatically activate the actor if it's not already active.

**Example:**

```rust
#[gen_server_actor(facets = ["virtual_actor"])]
struct UserProfile {
    user_id: String,
    preferences: HashMap<String, String>,
}

// Spawn first instance - automatically registers "UserProfile" type
let ctx = RequestContext::new_without_auth("tenant".into(), "namespace".into());
let actor_ref1 = spawn(
    &ctx,
    service_locator,
    "user-123",
    "ns",
    UserProfile::new("user-123"),
).await?;

// Later, spawn a different instance of the same type
// This actor will also be automatically activated on-demand
let actor_ref2 = spawn(
    &ctx,
    service_locator,
    "user-456",  // Different name, same type
    "ns",
    UserProfile::new("user-456"),
).await?;

// Any message to "user-789" (same type, different name) will automatically
// activate a new UserProfile actor for that user ID
```

**Note:** Type registration is idempotent - registering the same type multiple times is safe and overwrites previous registration. Registration failures are logged but don't prevent actor spawning (the actor will still work, but auto-activation may fail).

## Message Creation Helpers

The SDK provides helper functions for creating messages with correct call or cast semantics:

| Function | Description | Use With |
|----------|-------------|----------|
| `call_message(payload)` | Create request-reply message (`message_type = "call"`) | `actor_ref.ask()` |
| `cast_message(payload)` | Create fire-and-forget message (`message_type = "cast"`) | `actor_ref.tell()` |
| `new_message(invocation, payload)` | Create message with custom invocation type | Either |

**Example:**

```rust
use plexspaces_sdk::{call_message, cast_message, json};
use std::time::Duration;

// Request-reply: use call_message() with ask()
let request = call_message(json!({ "action": "get_balance" }));
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;

// Fire-and-forget: use cast_message() with tell()
let event = cast_message(json!({ "event": "user_login", "user_id": "123" }));
actor_ref.tell(event).await?;
```

## Communication Patterns

### Request-Reply (Ask Pattern)

```rust
// Create request message
let request = call_message(json!({ "action": "get_balance" }));

// Send and wait for reply
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;

// Parse reply
let result: Value = serde_json::from_slice(&reply.payload)?;
```

**Handler side:**

```rust
#[handler("get_balance")]
async fn get_balance(&mut self, _ctx: &ActorContext, _msg: &Message) 
    -> Result<Value, BehaviorError> 
{
    // Return value is automatically serialized and sent as reply
    Ok(json!({ "balance": self.balance }))
}
```

### Fire-and-Forget (Tell Pattern)

```rust
// Create event message
let event = cast_message(json!({ "event": "user_login", "user_id": "123" }));

// Send without waiting
actor_ref.tell(event).await?;
```

**Handler side:**

```rust
#[handler("log_event", cast)]
async fn log_event(&mut self, _ctx: &ActorContext, msg: &Message) 
    -> Result<(), BehaviorError> 
{
    // Process event, no reply sent
    let payload: Value = serde_json::from_slice(&msg.payload)?;
    // ... log event ...
    Ok(())
}
```

## ShardGroup Operations

The SDK uses the shard-group client APIs directly for data-parallel operations:

```rust
use plexspaces_sdk::{ShardGroupClientLocal, ShardGroupClientTrait};
use plexspaces_proto::actor::v1::PartitionStrategy;

let mut client = ShardGroupClientLocal::new(service_locator).await?;

let group = client.create_shard_group(
    "worker-pool".to_string(),
    "worker".to_string(),
    4,
    PartitionStrategy::PartitionStrategyHash,
    None,
).await?;

let results = client.map(
    "worker-pool".to_string(),
    json!({ "action": "process", "data": items }),
).await?;

// Reduce operation (aggregate results)
let final_result = client.reduce(
    &ctx,
    "worker-pool",
    "namespace",
    json!({ "action": "aggregate" }),
    results,
    Duration::from_secs(30),
).await?;
```

See [examples/rust/apps/data_parallel_worker](../examples/rust/apps/data_parallel_worker) for a deployable WASM example (shard group + scatter/gather, SDK + WIT).

## Leader-worker (multi-node)

For **one logical run** with work split across nodes, the **first node** is the leader. Use `plexspaces_sdk::leader_worker` (feature `grpc`):

- **`list_worker_node_ids(ctx, service_locator, cluster, page_size)`** — Node IDs from the registry (after ConnectNodes). Leader uses this to distribute work.
- **Virtual actors are lazy** — No explicit ensure. Deploy the worker type as virtual on all nodes; the leader sends to `worker/chunk@node_id` and the target node creates the actor on first message receive. Same in all SDKs (Rust, Python, TypeScript, Go).
- **`spawn_actor_on_node(...)`** — Spawn a non-virtual worker on a given node (calls that node’s SpawnActor). Use only when not using virtual actors.

Core logic is in main crates (NodeRegistry, ActorService); the SDK wraps it. See [docs/sdk.md](../../../docs/sdk.md#leader-worker-multi-node-one-run) and [examples/README.md](../../../examples/README.md) for cross-language semantics and patterns.

## Best Practices

### 1. Use SDK Helpers

✅ **DO**: Use `call_message()` and `cast_message()` helpers

```rust
let request = call_message(json!({ "action": "get_balance" }));
```

❌ **DON'T**: Manually construct messages

```rust
let request = Message {
    message_type: "call".to_string(),
    payload: serde_json::to_vec(&json!({ "action": "get_balance" }))?,
    ..Default::default()
};
```

### 2. Consistent Operation Names

✅ **DO**: Use `payload.action` consistently

```rust
let request = call_message(json!({ "action": "increment" }));
```

❌ **DON'T**: Mix `action`, `op`, and `msg_type` inconsistently

### 3. Handler Semantics

✅ **DO**: Use `call` for request-reply, `cast` for fire-and-forget

```rust
#[handler("get_balance")]  // Defaults to call
#[handler("log_event", cast)]  // Explicit cast
```

❌ **DON'T**: Use `cast` handlers with `ask()` or `call` handlers with `tell()`

### 4. Error Handling

✅ **DO**: Return `BehaviorError` from handlers

```rust
#[handler("divide")]
async fn divide(&mut self, _ctx: &ActorContext, msg: &Message) 
    -> Result<Value, BehaviorError> 
{
    let payload: Value = serde_json::from_slice(&msg.payload)?;
    let divisor = payload["divisor"].as_i64().unwrap_or(0);
    if divisor == 0 {
        return Err(BehaviorError::ProcessingError("Division by zero".into()));
    }
    Ok(json!({ "result": self.value / divisor }))
}
```

### 5. RequestContext

✅ **DO**: Always use proper `RequestContext` with tenant/namespace

```rust
let ctx = RequestContext::new_without_auth("tenant".into(), "namespace".into());
```

❌ **DON'T**: Use `RequestContext::internal()` except for system initialization

## Examples

- [Entity Recognition](../examples/rust/apps/entity_recognition) - GenServer with specific handlers
- [Data Parallel Worker](../examples/rust/apps/data_parallel_worker) - WASM leader/worker, scatter/gather over shard group
- [Bank Account](../examples/rust/apps/bank_account) - Durable actor with storage

## Design Philosophy

The SDK design follows Erlang/OTP principles and industry best practices:

### Erlang/OTP Alignment

1. **GenServer Pattern**: 
   - `call` = `gen_server:call/2` (request-reply, synchronous)
   - `cast` = `gen_server:cast/2` (fire-and-forget, asynchronous)
   - Both route to `handle_request()` (like Erlang's `handle_call/3`), but `cast` doesn't require a reply

2. **Message-Passing**: 
   - All communication via messages, no shared state
   - Messages are immutable and serializable
   - Location transparency (actors don't know if target is local or remote)

3. **Fault Tolerance**: 
   - "Let it crash" philosophy with supervisors
   - Actor isolation (failure in one actor doesn't affect others)

4. **Separation of Concerns**: 
   - Behaviors (GenServer, GenEvent, GenStateMachine, Workflow) separate from actor implementation
   - Facets add capabilities (durability, timers, virtual actors) without changing core behavior

### Industry Best Practices

1. **Type Safety**: 
   - Strong typing with Rust's type system
   - Compile-time guarantees for message passing

2. **Observability**: 
   - Structured logging with `tracing`
   - Metrics for message counts, latencies, errors
   - Request/response correlation via `correlation_id`

3. **Error Handling**: 
   - Explicit error types (`BehaviorError`)
   - No silent failures
   - Errors propagate through the actor system

4. **Performance**: 
   - Zero-copy message passing where possible
   - Async/await for non-blocking I/O
   - Efficient serialization (Protocol Buffers, JSON)

### Design Decisions

**Why `message_type` serves dual purpose?**

- **Message pattern** (`"call"`/`"cast"`): Controls routing to request-reply vs fire-and-forget
- **Operation name** (other values): Used directly as handler name for backward compatibility

This design allows:
- Clear ask/tell semantics (`call` vs `cast`)
- Flexible operation naming (`payload.action`, `payload.op`, or `message_type`)
- Backward compatibility with existing code that uses `message_type` as operation name

**Why GenServer uses single `handle_request()` for both call and cast?**

- Simpler API: One method to implement instead of two
- Consistent handler logic: Same code path for both patterns
- Reply semantics handled by SDK macro: Automatically sends reply for `call`, optional for `cast`
- Aligns with Erlang: `gen_server:cast/2` still calls `handle_call/3`, caller just doesn't wait

## Reference

- [Main SDK Documentation](../../../docs/sdk.md) - Comprehensive SDK reference
- [Architecture Documentation](../../../docs/architecture.md) - System architecture
- [Concepts Documentation](../../../docs/concepts.md) - Core concepts and patterns
- [Behavior Documentation](../../../crates/behavior/README.md) - Behavior implementation details
