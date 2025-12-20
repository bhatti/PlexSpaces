# Core Concepts

This document explains the fundamental concepts you need to understand to use PlexSpaces effectively.

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
11. [FaaS-Style Invocation](#faas-style-invocation)

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

### Example

```rust
struct Counter {
    count: i32,
}

#[async_trait]
impl ActorBehavior for Counter {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message
    ) -> Result<(), BehaviorError> {
        // Process message
        self.count += 1;
        Ok(())
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
// Get actor reference
let actor_ref = node.get_actor_ref(&"counter@node1".to_string()).await?;

// Fire-and-forget (tell)
actor_ref.tell(message).await?;

// Request-reply (ask)
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;
```

## Behaviors

**Behaviors** define how actors process messages. They are compile-time traits (zero overhead):

- **GenServerBehavior**: Erlang/OTP-style request/reply (synchronous)
- **GenFSMBehavior**: Finite state machine (state transitions)
- **GenEventBehavior**: Event-driven processing (fire-and-forget)
- **WorkflowBehavior**: Durable workflow orchestration (Temporal/Restate-inspired)

### GenServerBehavior Example

```rust
#[async_trait]
impl GenServerBehavior for Counter {
    type Request = CounterRequest;
    type Reply = i32;

    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        request: Self::Request,
    ) -> Result<Self::Reply, BehaviorError> {
        match request {
            CounterRequest::Increment(amount) => {
                self.count += amount;
                Ok(self.count)
            }
            CounterRequest::Get => Ok(self.count),
        }
    }
}
```

## Facets

**Facets** add dynamic capabilities to actors at runtime. They follow the "Static for core, Dynamic for extensions" principle:

- **Infrastructure Facets**: VirtualActorFacet, DurabilityFacet, MobilityFacet
- **Capability Facets**: HttpClientFacet, KeyValueFacet, BlobStorageFacet
- **Timer/Reminder Facets**: TimerFacet, ReminderFacet
- **Observability Facets**: MetricsFacet, TracingFacet, LoggingFacet
- **Security Facets**: AuthenticationFacet, AuthorizationFacet
- **Event Facets**: EventEmitterFacet

### Example

```rust
use plexspaces_journaling::{VirtualActorFacet, DurabilityFacet, SqliteJournalStorage};

// Create facets
let storage = SqliteJournalStorage::new(":memory:").await?;
let virtual_facet = Box::new(VirtualActorFacet::new(serde_json::json!({}), 100));
let durability_facet = Box::new(DurabilityFacet::new(
    storage,
    serde_json::json!({
        "checkpoint_interval": 100,
        "replay_on_activation": true,
    }),
    50,
));

// Spawn actor with facets
let _message_sender = actor_factory.spawn_actor(
    &ctx,
    &actor_id,
    "MyActor",
    vec![],
    None,
    std::collections::HashMap::new(),
    vec![virtual_facet, durability_facet], // facets
).await?;
```

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

### Example

```rust
#[async_trait]
impl WorkflowBehavior for OrderWorkflow {
    async fn execute(&mut self, ctx: &WorkflowContext) -> Result<(), WorkflowError> {
        // Step 1: Validate order
        ctx.step("validate", || validate_order(&self.order_id)).await?;
        
        // Step 2: Process payment
        ctx.step("payment", || process_payment(&self.order_id)).await?;
        
        // Step 3: Ship order
        ctx.step("ship", || ship_order(&self.order_id)).await?;
        
        Ok(())
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
// Tell (fire-and-forget)
actor_ref.tell(Message::new(b"increment".to_vec())).await?;

// Ask (request-reply)
let reply = actor_ref.ask(
    CounterRequest::Get,
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

// Spawn actor with durability facet
let _message_sender = actor_factory.spawn_actor(
    &ctx,
    &actor_id,
    "MyActor",
    vec![],
    None,
    std::collections::HashMap::new(),
    vec![durability_facet], // facets
).await?;
```

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

- **RESTful API**: Standard HTTP GET/POST methods for actor invocation
- **Path-Based Routing**: `/api/v1/actors/{tenant_id}/{actor_type}` endpoint
- **GET for Reads**: Uses `ask()` pattern (request-reply) with query parameters
- **POST for Updates**: Uses `tell()` pattern (fire-and-forget) with request body
- **Multi-Tenant Isolation**: Built-in tenant-based access control
- **Load Balancing**: Automatic distribution across actor instances
- **AWS Lambda Ready**: Designed for integration with AWS Lambda Function URLs

### HTTP Methods

**GET - Read Operations (Ask Pattern)**:
```bash
# Get counter value (with tenant_id and namespace)
curl "http://localhost:8080/api/v1/actors/default/default/counter?action=get"

# Get counter value (without tenant_id, defaults to "default")
curl "http://localhost:8080/api/v1/actors/default/counter?action=get"
```

- Query parameters converted to JSON payload
- Actor's `handle_request()` called (GenServer pattern)
- Actor sends reply via `ctx.send_reply()`
- Response contains actor's reply payload
- `message.uri_path` and `message.uri_method` populated

**POST/PUT - Update Operations (Tell Pattern)**:
```bash
# Increment counter (POST) - with tenant_id and namespace
curl -X POST "http://localhost:8080/api/v1/actors/default/default/counter" \
  -H "Content-Type: application/json" \
  -d '{"action":"increment"}'

# Increment counter (POST) - without tenant_id
curl -X POST "http://localhost:8080/api/v1/actors/default/counter" \
  -H "Content-Type: application/json" \
  -d '{"action":"increment"}'

# Update counter (PUT)
curl -X PUT "http://localhost:8080/api/v1/actors/default/default/counter" \
  -H "Content-Type: application/json" \
  -d '{"action":"set","value":42}'
```

- Request body becomes message payload
- HTTP headers preserved as message metadata
- Actor's `handle_message()` called (fire-and-forget)
- Response returns immediately
- `message.uri_path` and `message.uri_method` populated

**DELETE - Delete Operations (Ask Pattern)**:
```bash
# Delete resource (with tenant_id and namespace)
curl -X DELETE "http://localhost:8080/api/v1/actors/default/default/counter?confirm=true"

# Delete resource (without tenant_id)
curl -X DELETE "http://localhost:8080/api/v1/actors/default/counter?confirm=true"
```

- Query parameters converted to JSON payload
- Actor's `handle_request()` called (GenServer pattern)
- Actor sends reply via `ctx.send_reply()`
- Response contains actor's reply payload
- `message.uri_path` and `message.uri_method` populated

### Actor Lookup

Actors are discovered by `actor_type` using efficient O(1) hashmap lookup:

1. **Type-Based Discovery**: Actors registered with `actor_type` are indexed
2. **Random Selection**: If multiple actors exist, one is randomly selected (load balancing)
3. **404 Not Found**: Returns 404 if no actors of the specified type are found

### Path and Subpath Routing

For advanced routing, actors receive:

- **URI Path**: Available in `message.uri_path` (full HTTP path)
- **URI Method**: Available in `message.uri_method` (GET, POST, PUT, DELETE)
- **Full HTTP Path**: Also available in `message.metadata["http_path"]` (for backward compatibility)
- **Subpath**: Available in `message.metadata["http_subpath"]` (everything after actor_type)

This enables custom routing within actors (e.g., `/metrics`, `/health`, `/actions/{name}`).

### Routing Patterns

PlexSpaces supports multiple routing patterns for actor invocation:

#### 1. HTTP to gRPC Routing

The HTTP gateway translates HTTP requests to gRPC `InvokeActor` calls:

```
HTTP Request → HTTP Gateway (Axum) → gRPC InvokeActor → ActorService → Actor
```

**Pattern Flow**:
1. **HTTP Request**: Client sends HTTP GET/POST to `/api/v1/actors/{tenant_id}/{namespace}/{actor_type}`
2. **HTTP Gateway**: Axum server parses path parameters, query params, and body
3. **gRPC Translation**: Gateway constructs `InvokeActorRequest` with:
   - `tenant_id`, `namespace`, `actor_type` from path
   - `payload` from request body (POST/PUT) or query params (GET)
   - `message_type` set to `"call"` for GET/DELETE (ask pattern) or `"cast"` for POST/PUT (tell pattern)
4. **Actor Service**: `ActorServiceImpl::invoke_actor` handles the gRPC request
5. **Actor Discovery**: Service looks up actors by type using `ActorRegistry::discover_actors_by_type`
6. **Message Delivery**: Selected actor receives message via mailbox
7. **Response**: For ask pattern (GET), actor sends reply via `ctx.send_reply()`
8. **HTTP Response**: Gateway converts gRPC response back to HTTP/JSON

#### 2. Actor Discovery and Selection

When multiple actors of the same type exist, the system uses:

- **Random Selection**: Picks one actor randomly from discovered actors
- **Load Distribution**: Natural load balancing across actor instances
- **Type-Based Lookup**: `ActorRegistry::discover_actors_by_type(tenant_id, namespace, actor_type)`

**Example**:
```rust
// Multiple counter actors registered
// GET /api/v1/actors/default/default/counter
// → ActorService discovers all actors with type="counter"
// → Randomly selects one (e.g., "counter-1@node1")
// → Routes message to selected actor
```

#### 3. Message Type Routing

Different HTTP methods map to different message patterns:

- **GET/DELETE** → `MessageType::Call` (ask pattern, expects reply)
- **POST/PUT** → `MessageType::Cast` (tell pattern, fire-and-forget)

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

- **Path Parameter**: `tenant_id` in URL path
- **JWT Authentication**: When enabled, `tenant_id` extracted from JWT claims
- **Access Control**: JWT `tenant_id` must match requested `tenant_id`
- **Default Tenant**: If no auth provided or not in path, defaults to "default"
- **Default Namespace**: If not in path, defaults to "default"
- **Path Formats**:
  - `/api/v1/actors/{tenant_id}/{namespace}/{actor_type}` - Full path with tenant_id
  - `/api/v1/actors/{namespace}/{actor_type}` - Path without tenant_id (defaults to "default")

### Example

```rust
// Register actor with type, tenant_id, and namespace for InvokeActor lookup
actor_registry.register_actor(
    actor_id.clone(),
    sender,
    Some("counter".to_string()),  // actor_type
    Some("default".to_string()),   // tenant_id (defaults to "default" if None)
    Some("default".to_string()),   // namespace (defaults to "default" if None)
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
    
    // Also available in metadata for backward compatibility
    if let Some(subpath) = msg.metadata.get("http_subpath") {
        // Custom routing based on subpath
    }
    
    Ok(())
}
```

### AWS Lambda Integration

The `InvokeActor` endpoint is designed for AWS Lambda Function URLs:

1. Deploy PlexSpaces Node as Lambda function
2. Enable Lambda Function URL for HTTP access
3. Route requests to `/api/v1/actors/{tenant_id}/{namespace}/{actor_type}` or `/api/v1/actors/{namespace}/{actor_type}`
4. Lambda automatically scales based on request volume

See [Architecture](architecture.md#faas-invocation) and [Detailed Design](detailed-design.md#invokeactor-service) for more details.

## Next Steps

- [Getting Started](getting-started.md): Learn how to build your first actor
- [Architecture](architecture.md): Understand the system design
- [Detailed Design](detailed-design.md): Deep dive into components
- [Use Cases](use-cases.md): Explore real-world applications
