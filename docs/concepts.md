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
let actor = ActorBuilder::new("actor@node1".to_string())
    .with_behavior(MyBehavior {})
    .with_facet(VirtualActorFacet::new())  // Auto activation/deactivation
    .with_facet(DurabilityFacet::new(storage, config))  // Automatic persistence
    .build()?;
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
let storage = SqliteJournalStorage::new(":memory:").await?;
let durability = DurabilityFacet::new(
    Arc::new(storage),
    DurabilityConfig {
        checkpoint_interval: 100,
        replay_on_activation: true,
        ..Default::default()
    }
);

let actor = ActorBuilder::new("durable-actor@node1".to_string())
    .with_behavior(MyBehavior {})
    .with_facet(durability)
    .build()?;
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

## Next Steps

- [Getting Started](getting-started.md): Learn how to build your first actor
- [Architecture](architecture.md): Understand the system design
- [Detailed Design](detailed-design.md): Deep dive into components
- [Use Cases](use-cases.md): Explore real-world applications
