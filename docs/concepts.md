# Core Concepts

This document explains the fundamental concepts you need to understand to use PlexSpaces effectively.

> **📖 For comprehensive actor system documentation**, see [Actor System Guide](actor-system.md) which covers actors, supervisors, applications, facets, behaviors, lifecycle, linking/monitoring, and observability in detail.

## Table of Contents

1. [Actors](#actors)
2. [ActorRef](#actorref)
3. [Behaviors](#behaviors)
4. [Facets](#facets)
5. [TupleSpace](#tuplespace)
6. [Workflows](#workflows)
7. [Supervision](#supervision)
8. [Location Transparency](#location-transparency)
9. [Message Passing](#message-passing)
10. [Durability](#durability)
11. [Channels](#channels)
12. [FaaS-Style Invocation](#faas-style-invocation)

## Actors

**Actors** are the fundamental unit of computation in PlexSpaces. Each actor:

- Has a **unique ID** in format `name@node_id` (e.g., `counter@node1`)
- Processes **messages sequentially** (single-threaded execution)
- Maintains **private state** (no shared state between actors)
- Communicates via **message passing** (tell/ask patterns)
- Is **location-transparent** (works the same locally or remotely)
- Is **fault-tolerant** (automatic recovery via supervision)

### Actor Lifecycle

```
Creating → Inactive → Active → Terminated
                            ↓
                          Failed
```

**States**:
- **Creating**: Actor is being initialized
- **Inactive**: Actor is inactive (virtual actors)
- **Active**: Actor is processing messages
- **Terminated**: Actor has stopped gracefully
- **Failed**: Actor has crashed with error

### Example (SDK Annotations)

```rust
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers, handler, json};

// Define actor with SDK annotation (like Python @actor decorator)
#[gen_server_actor]
struct Counter {
    count: i32,
}

// Define handlers - GenServer defaults to "call" (request-reply)
#[plexspaces_handlers]
impl Counter {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &plexspaces_sdk::ActorContext,
        msg: &plexspaces_sdk::Message,
    ) -> Result<serde_json::Value, plexspaces_sdk::BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)?;
        self.count += payload["amount"].as_i64().unwrap_or(1) as i32;
        Ok(json!({ "count": self.count }))
    }
}
```

## ActorRef

**ActorRef** is a lightweight, location-transparent handle to an actor. It provides:

- **Location Transparency**: Same API for local and remote actors
- **Cloneable**: Share references safely across threads
- **Message Passing**: `tell()` and `ask()` methods
- **Automatic Routing**: Handles local vs remote communication automatically

### Example

```rust
use plexspaces_sdk::{call_message, cast_message, json};
use std::time::Duration;

// Fire-and-forget (tell) - use cast_message()
let event = cast_message(json!({ "event": "user_login" }));
actor_ref.tell(event).await?;

// Request-reply (ask) - use call_message()
let request = call_message(json!({ "action": "get_balance" }));
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;
```

## Behaviors

**Behaviors** define how actors process messages. Use SDK annotations for declarative definition:

| Annotation | Behavior | Use Case |
|------------|----------|----------|
| `#[gen_server_actor]` | GenServer | Request-reply (call by default) |
| `#[event_actor]` | GenEvent | Fire-and-forget events (cast) |
| `#[fsm_actor]` | GenStateMachine | State machine transitions |
| `#[workflow_actor]` | Workflow | Durable workflow orchestration |

### GenServer Example (SDK)

```rust
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers, handler, json};

#[gen_server_actor]
struct Counter {
    count: i32,
}

#[plexspaces_handlers]
impl Counter {
    // GenServer handlers default to "call" - returns reply automatically
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &plexspaces_sdk::ActorContext,
        msg: &plexspaces_sdk::Message,
    ) -> Result<serde_json::Value, plexspaces_sdk::BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)?;
        self.count += payload["amount"].as_i64().unwrap_or(1) as i32;
        Ok(json!({ "count": self.count }))
    }
    
    #[handler("get")]
    async fn get(
        &mut self,
        _ctx: &plexspaces_sdk::ActorContext,
        _msg: &plexspaces_sdk::Message,
    ) -> Result<serde_json::Value, plexspaces_sdk::BehaviorError> {
        Ok(json!({ "count": self.count }))
    }
}
```

## Facets

**Facets** add dynamic capabilities to actors at runtime. They follow the "Static for core, Dynamic for extensions" principle:

- **Infrastructure Facets**: VirtualActorFacet, DurabilityFacet, MobilityFacet
- **Capability Facets**: 
  - **LockFacet**: Distributed lock coordination (task queues, resource coordination)
  - **ProcessGroupFacet**: Distributed pub/sub and group messaging (Erlang pg2-style)
  - **RegistryFacet**: Service discovery and object registration
  - **HttpClientFacet**: HTTP client capabilities
  - **KeyValueFacet**: Key-value store operations
  - **BlobStorageFacet**: Blob storage operations
- **Timer/Reminder Facets**: TimerFacet, ReminderFacet
- **Observability Facets**: MetricsFacet, TracingFacet, LoggingFacet
- **Security Facets**: AuthenticationFacet, AuthorizationFacet
- **Event Facets**: EventEmitterFacet

### Capability Facets (Message Interception Pattern)

Capability facets (LockFacet, ProcessGroupFacet, RegistryFacet) use **message interception** to provide capabilities:

1. **Facet attached** to actor via `app-config.toml` or programmatically
2. **Facet intercepts** messages with specific types (e.g., `"acquire_lock"`, `"join_group"`, `"register_object"`)
3. **Facet handles** the operation using real backend services from ServiceLocator
4. **Actor's handle()** method is never called for intercepted messages
5. **Backend configured** via node-config/runtimeconfig (not hardcoded)

This pattern works for both Rust and WASM actors - they all send messages, and facets handle them uniformly.

### Example

```rust
use plexspaces_sdk::{spawn_with_storage, RequestContext};
use plexspaces_journaling::SqliteJournalStorage;

// Define actor with annotations (recommended)
#[gen_server_actor(facets = ["virtual_actor", "durability"])]
struct MyActor {
    // actor state
}

// Spawn actor with storage (SDK pattern - recommended for examples)
let storage = Arc::new(SqliteJournalStorage::new(":memory:").await?);
let ctx = RequestContext::new_without_auth("tenant".to_string(), "namespace".to_string());
let actor_ref = spawn_with_storage(
    &ctx,
    service_locator,
    actor_id,
    "namespace",
    MyActor::new(),
    storage,
).await?;

// Alternative: Manual facet creation (for advanced use cases)
use plexspaces_sdk::{spawn_with_facets, VirtualActorFacet, DurabilityFacet};
let storage = Arc::new(SqliteJournalStorage::new(":memory:").await?);
let virtual_facet = Box::new(VirtualActorFacet::new(serde_json::json!({}), 100));
let durability_facet = Box::new(DurabilityFacet::new(storage, serde_json::json!({}), 50));
let actor_ref = spawn_with_facets(
    &ctx,
    service_locator,
    actor_id,
    "namespace",
    MyActor::new(),
    vec![virtual_facet, durability_facet],
).await?;
```

**Note**: For examples and user code, use Node and SDK patterns (`spawn`, `spawn_with_facets`, `spawn_with_storage`, `call_message`, `cast_message`). The framework uses ActorFactory internally for gRPC spawn.

## TupleSpace

**TupleSpace** provides Linda-style coordination for decoupled communication:

- **Spatial Decoupling**: Actors don't need to know each other
- **Temporal Decoupling**: Actors don't need to be active simultaneously
- **Pattern Matching**: Flexible tuple retrieval with wildcards
- **Blocking Operations**: `read()` and `take()` wait for matching tuples
- **Non-blocking Operations**: `read_if_exists()` and `take_if_exists()` for non-blocking access

### Example

```rust
// Write tuple
let tuple = Tuple::new(vec![
    TupleField::String("order".to_string()),
    TupleField::String(order_id),
    TupleField::String("pending".to_string()),
]);
ctx.tuplespace().write(tuple).await?;

// Read tuple (blocking)
let pattern = Pattern::new(vec![
    PatternField::Exact(TupleField::String("order".to_string())),
    PatternField::Wildcard,
    PatternField::Exact(TupleField::String("pending".to_string())),
]);
let tuple = ctx.tuplespace().read(pattern).await?;
```

## Workflows

**Workflows** are durable, long-running processes with automatic recovery:

- **Exactly-Once Execution**: Guaranteed execution semantics
- **Automatic Recovery**: Resume from last checkpoint on failure
- **Step-by-Step Execution**: Sequential or parallel steps
- **Signals and Queries**: External control and read-only queries
- **Time-Travel Debugging**: Replay past executions

### Example (SDK Annotations)

```rust
use plexspaces_sdk::{
    workflow_actor, plexspaces_handlers, run_handler, signal_handler, query_handler,
    json,
};

#[workflow_actor(facets = ["durability"])]
struct OrderWorkflow {
    order_id: String,
    status: String,
}

#[plexspaces_handlers(workflow)]
impl OrderWorkflow {
    #[run_handler]
    async fn run(
        &mut self,
        _ctx: &plexspaces_sdk::ActorContext,
        input: plexspaces_sdk::Message,
    ) -> Result<plexspaces_sdk::Message, plexspaces_sdk::BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&input.payload)?;
        self.order_id = payload["order_id"].as_str().unwrap_or("").to_string();
        
        // Step 1: Validate order
        self.status = "validating".to_string();
        // ... validation logic ...
        
        // Step 2: Process payment
        self.status = "processing_payment".to_string();
        // ... payment logic ...
        
        // Step 3: Ship order
        self.status = "shipping".to_string();
        // ... shipping logic ...
        
        self.status = "completed".to_string();
        Ok(plexspaces_sdk::Message {
            payload: serde_json::to_vec(&json!({ "status": "completed" }))?,
            ..Default::default()
        })
    }
    
    #[signal_handler("cancel")]
    async fn on_cancel(
        &mut self,
        _ctx: &plexspaces_sdk::ActorContext,
        _data: plexspaces_sdk::Message,
    ) -> Result<(), plexspaces_sdk::BehaviorError> {
        self.status = "cancelled".to_string();
        Ok(())
    }
    
    #[query_handler("status")]
    async fn get_status(
        &self,
        _ctx: &plexspaces_sdk::ActorContext,
        _params: plexspaces_sdk::Message,
    ) -> Result<plexspaces_sdk::Message, plexspaces_sdk::BehaviorError> {
        Ok(plexspaces_sdk::Message {
            payload: serde_json::to_vec(&json!({
                "order_id": self.order_id,
                "status": self.status,
            }))?,
            ..Default::default()
        })
    }
}
```

## Supervision

**Supervision** provides fault tolerance through hierarchical supervision trees:

- **Supervision Strategies**: OneForOne, OneForAll, RestForOne, SimpleOneForOne
- **Restart Policies**: Always, Transient, Temporary
- **Restart Intensity**: Maximum restarts within a time window
- **"Let It Crash"**: Failure isolation and automatic recovery

### Example

```rust
let supervisor = Supervisor::new()
    .with_strategy(SupervisionStrategy::OneForOne)
    .with_max_restarts(5)
    .with_restart_window(Duration::from_secs(60))
    .build();

supervisor.add_child(ChildSpec::new("worker")
    .with_restart_policy(RestartPolicy::Always)
).await?;
```

## Location Transparency

**Location Transparency** means actors work the same whether they're local or remote:

- **Same API**: `tell()` and `ask()` work identically for local and remote actors
- **Automatic Routing**: System handles local vs remote communication
- **Actor IDs**: Format `name@node_id` enables location transparency
- **Service Discovery**: Automatic actor location via ObjectRegistry

### Example

```rust
// Local actor
let local_ref = node.get_actor_ref(&"counter@node1".to_string()).await?;
local_ref.tell(message).await?;

// Remote actor (same API!)
let remote_ref = node.get_actor_ref(&"counter@node2".to_string()).await?;
remote_ref.tell(message).await?;
```

## Message Passing

**Message Passing** is the primary communication mechanism:

- **Tell (Fire-and-Forget)**: Asynchronous, no reply expected
- **Ask (Request-Reply)**: Synchronous, reply expected with timeout
- **Correlation IDs**: Automatic tracking for reply matching
- **Message Types**: Typed messages via enums or structs

### Example

```rust
use plexspaces_sdk::{cast_message, call_message, json};

// Tell (fire-and-forget) - use cast_message()
actor_ref.tell(cast_message(json!({ "action": "increment" }))).await?;

// Ask (request-reply) - use call_message()
let reply = actor_ref.ask(
    call_message(json!({ "action": "get" })),
    Duration::from_secs(5)
).await?;
```

## Durability

**Durability** provides automatic persistence and recovery:

- **Event Sourcing**: Complete audit trail of all state changes
- **Checkpointing**: Periodic snapshots for fast recovery
- **Deterministic Replay**: Replay from any point in history
- **Exactly-Once Semantics**: Guaranteed message processing
- **Time-Travel Debugging**: Replay past executions
- **Channel-Based Mailbox**: Durable channels (Kafka, Redis, SQLite, NATS) as actor mailboxes with ACK/NACK
- **Dead Letter Queue (DLQ)**: Automatic handling of poisonous messages
- **Graceful Shutdown**: Actors using non-memory channels stop accepting new messages but complete in-progress work

### Example

```rust
use plexspaces_journaling::{DurabilityFacet, SqliteJournalStorage};

let storage = SqliteJournalStorage::new(":memory:").await?;
let durability_facet = Box::new(DurabilityFacet::new(
    storage,
    serde_json::json!({
        "checkpoint_interval": 100,
        "replay_on_activation": true,
    }),
    50, // priority
));

// Spawn actor with durability facet using SDK helper
use plexspaces_sdk::spawn_with_facets;
let actor_ref = spawn_with_facets(
    &ctx,
    service_locator,
    actor_id,
    "default", // namespace
    MyActor::new(), // actor instance
    vec![durability_facet], // facets
).await?;
```

For comprehensive documentation on durability, including recovery scenarios, edge cases, channel-based mailboxes, and DLQ patterns, see [Durability Documentation](durability.md).

## Channels

**Channels** provide queue and topic patterns for message passing between actors and services. Channels can serve as actor mailboxes, enabling durable message processing with ACK/NACK semantics.

### Channel Backends

PlexSpaces supports multiple channel backends:
- **InMemory**: Fast, same-node MPSC channels
- **Redis**: Distributed messaging with Redis Streams
- **Kafka**: High-throughput streaming pipelines
- **NATS**: Lightweight distributed pub/sub
- **ProcessGroup**: Distributed pub/sub using Erlang pg/pg2-style process groups (no external dependencies)
- **SQLite**: Single-node durability for testing
- **UDP**: Multicast pub/sub within cluster
- **SQS**: AWS SQS integration

- **InMemory**: Fast, non-persistent (testing only)
- **Redis**: Distributed, durable (Redis Streams with consumer groups)
- **Kafka**: High-throughput, durable (production-grade)
- **SQLite**: File-based, durable (single-node persistence)
- **NATS**: Lightweight pub/sub (multi-node)
- **UDP**: Low-latency multicast (best-effort, cluster-wide messaging)

### Channel Features

- **Durability**: Durable backends (Redis, Kafka, SQLite) persist messages across restarts
- **ACK/NACK**: Acknowledge successful processing or requeue failed messages
- **Dead Letter Queue (DLQ)**: Automatic handling of messages that fail repeatedly
- **Graceful Shutdown**: Stop accepting new messages but complete in-progress work
- **Message Recovery**: Unacked messages are automatically recovered on restart
- **Pub/Sub**: Publish/subscribe patterns for topic-based messaging
- **Observability**: Comprehensive metrics and logging for all operations

### UDP Multicast Channels

UDP channels provide low-latency, high-throughput pub/sub messaging within a cluster:

- **Multicast Support**: Uses UDP multicast for efficient cluster-wide broadcasting
- **Cluster Name**: Nodes with the same `cluster_name` can communicate via UDP
- **Best-Effort Delivery**: No ACK/NACK (messages may be lost)
- **Non-Durable**: Messages lost on restart (use for real-time, non-critical messaging)
- **Low Latency**: Sub-millisecond message delivery within cluster
- **High Throughput**: Supports high message rates

**Configuration**:
```rust
let udp_config = UdpConfig {
    multicast_address: "239.255.0.1".to_string(),
    multicast_port: 9999,
    bind_address: "0.0.0.0".to_string(),
    ttl: 1, // Local network only
    max_message_size: 1400, // Ethernet MTU
    cluster_name: "my-cluster".to_string(), // Required
    ..Default::default()
};
```

### Graceful Shutdown

Actors using non-memory channels support graceful shutdown:

- **Stop Accepting New Messages**: `enqueue()` rejects new messages during shutdown
- **Complete In-Progress**: Waits for all in-progress messages to complete (with timeout)
- **Stop Receiving**: `dequeue()` stops receiving from channel backend
- **Close Channel**: Underlying channel is explicitly closed
- **ACK/NACK**: In-progress messages can still be ACKed/NACKed

**Example**:
```rust
// Actor shutdown automatically calls mailbox.graceful_shutdown()
actor.stop().await?;

// Or manually shutdown mailbox
mailbox.graceful_shutdown(Some(Duration::from_secs(30))).await?;
```

### Channel as Mailbox

Channels can serve as actor mailboxes, providing durable message processing:

```rust
use plexspaces_mailbox::MailboxBuilder;

// Create mailbox with Redis channel backend
let mailbox = MailboxBuilder::new()
    .with_redis("redis://localhost:6379".to_string())
    .build("actor-mailbox".to_string())
    .await?;

// Messages are automatically ACKed on successful processing
// NACKed messages are requeued or sent to DLQ
```

For comprehensive channel documentation, including ACK/NACK patterns, DLQ configuration, and graceful shutdown, see [Durability Documentation](durability.md).

## Key Design Principles

### 1. Proto-First

All contracts defined in Protocol Buffers for cross-language compatibility.

### 2. Location Transparency

Actors work seamlessly across local processes, containers, and cloud regions.

### 3. Composable Abstractions

One powerful actor model with dynamic facets instead of multiple specialized types.

### 4. Single-Threaded Execution

Each actor processes messages sequentially for predictable behavior.

### 5. Failure Isolation

Actors are isolated - one actor's failure doesn't affect others.

## FaaS-Style Invocation

**FaaS-Style Invocation** enables HTTP-based actor invocation, treating actors like serverless functions:

- **Endpoint-Based Semantics**: `AskReply` handles request-reply and `SendMessage` handles fire-and-forget delivery
- **Path-Based Routing**: `/api/v1/actors/{namespace}/{actor_type}` and `/api/v1/actors/{namespace}/{actor_type}/ask`
- **GET for Ask**: `GET` routes to `AskReply` and converts query parameters into payload
- **POST/PUT Split**: `POST` and `PUT` on the base actor path use `SendMessage`; `POST` and `PUT` on `/ask` use `AskReply`
- **Multi-Tenant Isolation**: Built-in tenant-based access control
- **Load Balancing**: Automatic distribution across actor instances
- **AWS Lambda Ready**: Designed for integration with AWS Lambda Function URLs

### HTTP Endpoints

**GET - AskReply**:
```bash
curl "http://localhost:8080/api/v1/actors/default/counter?action=get"
```

- Query parameters converted to JSON payload
- Delivered through `AskReply`
- Actor's `handle_request()` called (GenServer pattern)
- Actor sends reply via `ctx.send_reply()`
- Response contains actor's reply payload
- `message.uri_path` and `message.uri_method` populated

**POST/PUT - SendMessage**:
```bash
curl -X POST "http://localhost:8080/api/v1/actors/default/counter" \
  -H "Content-Type: application/json" \
  -d '{"action":"increment"}'

# Update counter (PUT)
curl -X PUT "http://localhost:8080/api/v1/actors/default/counter" \
  -H "Content-Type: application/json" \
  -d '{"action":"set","value":42}'
```

- Request body becomes message payload
- HTTP headers preserved as message metadata
- Delivered through `SendMessage`
- Actor's `handle_message()` called (fire-and-forget)
- Response returns immediately
- `message.uri_path` and `message.uri_method` populated

**POST/PUT /ask - AskReply With Body**:
```bash
curl -X POST "http://localhost:8080/api/v1/actors/default/counter/ask" \
  -H "Content-Type: application/json" \
  -d '{"action":"get"}'
```

- Request body becomes message payload
- Delivered through `AskReply`
- Actor replies in the HTTP response

### Actor Lookup

Actors are discovered by `actor_type` using efficient O(1) hashmap lookup:

1. **Type-Based Discovery**: Actors registered with `actor_type` are indexed
2. **Random Selection**: If multiple actors exist, one is randomly selected (load balancing)
3. **404 Not Found**: Returns 404 if no actors of the specified type are found

### Path and Subpath Routing

For advanced routing, actors receive:

- **URI Path**: Available in `message.uri_path` (full HTTP path)
- **URI Method**: Available in `message.uri_method` (GET, POST, PUT, DELETE)
- **Subpath**: Available in `message.metadata["http_subpath"]` (everything after actor_type)

This enables custom routing within actors (e.g., `/metrics`, `/health`, `/actions/{name}`).

### Routing Patterns

PlexSpaces supports multiple routing patterns for actor invocation:

#### 1. HTTP to gRPC Routing

The HTTP gateway translates HTTP requests to gRPC `AskReply` or `SendMessage` calls:

```
HTTP Request → HTTP Gateway (Axum) → gRPC AskReply/SendMessage → ActorService → Actor
```

**Pattern Flow**:
1. **HTTP Request**: Client sends `GET /api/v1/actors/{namespace}/{actor_type}`, `GET|POST|PUT /api/v1/actors/{namespace}/{actor_type}/ask`, or `POST|PUT /api/v1/actors/{namespace}/{actor_type}`
2. **HTTP Gateway**: Axum server parses path parameters, query params, and body
3. **gRPC Translation**: Gateway constructs `AskReplyRequest` or `SendMessageRequest` with:
   - `namespace`, `actor_type` from path
   - `tenant_id` from JWT claims when authentication is enabled
   - `payload` from request body (POST/PUT) or query params (GET)
   - request metadata such as headers, path, and subpath
4. **Actor Service**: `ActorServiceImpl::ask_reply` or `ActorServiceImpl::send_message` handles the gRPC request
5. **Actor Discovery**: Service looks up actors by type using `ActorRegistry::discover_actors_by_type`
6. **Message Delivery**: Selected actor receives message via mailbox
7. **Response**: For `AskReply`, actor sends reply via `ctx.send_reply()`
8. **HTTP Response**: Gateway converts gRPC response back to HTTP/JSON

#### 2. Actor Discovery and Selection

When multiple actors of the same type exist, the system uses:

- **Random Selection**: Picks one actor randomly from discovered actors
- **Load Distribution**: Natural load balancing across actor instances
- **Type-Based Lookup**: `ActorRegistry::discover_actors_by_type(tenant_id, namespace, actor_type)`

**Example**:
```rust
// Multiple counter actors registered
// GET /api/v1/actors/default/counter
// → ActorService discovers all actors with type="counter"
// → Randomly selects one (e.g., "counter-1@node1")
// → Routes message to selected actor
```

#### 3. Endpoint Routing

Different HTTP endpoints map directly to the actor runtime services:

- **`GET /api/v1/actors/...`** and **`GET|POST|PUT /api/v1/actors/.../ask`** → `AskReply`
- **`POST|PUT /api/v1/actors/...`** → `SendMessage`

**Behavior Handling**:
- `GenServer::route_message` routes `Call` messages to `handle_request` (expects reply)
- `GenServer::route_message` routes `Cast` messages to `handle_request` (no reply required)

#### 4. Path-Based Actor Routing

Actors can implement custom routing based on HTTP path:

```rust
async fn handle_request(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
    if let Some(path) = &msg.uri_path {
        if path.ends_with("/metrics") {
            return self.handle_metrics(ctx, msg).await;
        }
        if path.ends_with("/health") {
            return self.handle_health(ctx, msg).await;
        }
        if let Some(subpath) = msg.metadata.get("http_subpath") {
            // Handle custom subpath routing
            if subpath.starts_with("/actions/") {
                let action = subpath.strip_prefix("/actions/").unwrap();
                return self.handle_action(ctx, msg, action).await;
            }
        }
    }
    // Default handling
    Ok(())
}
```

#### 5. Multi-Node Routing

For distributed systems, routing automatically handles:

- **Local Actors**: Direct mailbox delivery (same node)
- **Remote Actors**: gRPC client routing (different node)
- **Location Transparency**: Same API works for local and remote actors

**Routing Decision**:
```rust
if actor_id.node_id == current_node_id {
    // Local routing: direct mailbox enqueue
} else {
    // Remote routing: gRPC client call to remote node
}
```

### Multi-Tenancy

PlexSpaces implements **two-level isolation** for multi-tenancy:

**Tenant-id (Primary isolation)**:
- **Source of Truth**: JWT token (HTTP) or mTLS certificate (gRPC)
- **Purpose**: Tenant-level data isolation
- **When empty**: Only allowed when auth is disabled (`PLEXSPACES_DISABLE_AUTH=1`)

**Namespace (Sub-tenant isolation)**:
- **Source of Truth**: Application (when actor is deployed as part of app) or Actor creation request
- **Purpose**: Allows tenants to create isolated environments (e.g., "prod", "staging")
- **Storage**: Stored in `ActorRef.namespace` and `Actor.namespace`
- **When empty**: Represents default namespace within tenant

**RequestContext**:
- Carries both `tenant_id` and `namespace` through the call chain
- `tenant_id` from auth, `namespace` from application/actor
- All repository/service methods require RequestContext

**Path Formats**:
- `/api/v1/actors/{namespace}/{actor_type}` - Tenant from JWT auth
- JWT's `tenant_id` claim is the source of truth for tenant isolation

### Example

```rust
// Register actor with type, tenant_id, and namespace for AskReply/SendMessage lookup
actor_registry.register_actor(
    actor_id.clone(),
    sender,
    "counter".to_string(),        // actor_type
    Some("tenant-1".to_string()),  // tenant_id (from RequestContext or node config)
    Some("ns-1".to_string()),     // namespace (from RequestContext or node config, can be empty)
).await;

// Actor can access URI path and method
async fn handle_request(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
    // Access URI information directly from message
    if let Some(uri_path) = &msg.uri_path {
        if uri_path.contains("/metrics") {
            // Handle metrics endpoint
        }
    }
    
    // Access HTTP method
    if let Some(method) = &msg.uri_method {
        match method.as_str() {
            "GET" => self.handle_get(ctx, msg).await?,
            "POST" => self.handle_post(ctx, msg).await?,
            "PUT" => self.handle_put(ctx, msg).await?,
            "DELETE" => self.handle_delete(ctx, msg).await?,
            _ => {}
        }
    }
    
    if let Some(subpath) = msg.metadata.get("http_subpath") {
        // Custom routing based on subpath
    }
    
    Ok(())
}
```

### AWS Lambda Integration

The actor HTTP endpoints are designed for AWS Lambda Function URLs:

1. Deploy PlexSpaces Node as Lambda function
2. Enable Lambda Function URL for HTTP access
3. Route requests to `/api/v1/actors/{namespace}/{actor_type}`
4. Lambda automatically scales based on request volume

See [Architecture](architecture.md#faas-invocation) and [Detailed Design](detailed-design.md#askreply-and-sendmessage-services) for more details.

## Next Steps

- [Getting Started](getting-started.md): Learn how to build your first actor
- [Architecture](architecture.md): Understand the system design
- [Detailed Design](detailed-design.md): Deep dive into components
- [Use Cases](use-cases.md): Explore real-world applications
