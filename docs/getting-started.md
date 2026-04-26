# Getting Started with PlexSpaces

This guide will help you get started with PlexSpaces in minutes. You'll learn how to create your first actor, send messages, and use key features.

**New to PlexSpaces?** Start here, then read the [Concepts Guide](concepts.md) to understand the fundamentals.

> **📖 For comprehensive actor system documentation**, see [Actor System Guide](actor-system.md) which covers actors, supervisors, applications, facets, behaviors, lifecycle, linking/monitoring, and observability in detail.


## Prerequisites

- **Rust 1.70+**: [Install Rust](https://www.rust-lang.org/tools/install)
- **Docker** (optional): For containerized deployment
- **Protocol Buffers compiler** (optional): For proto generation (`buf` CLI recommended)

## Installation

### Quick Install (Docker)

```bash
# Pull the latest image
docker pull plexspaces/node:latest

# Run a single node
docker run -p 8080:8080 plexspaces/node:latest
```

### Build from Source

```bash
# Clone the repository
git clone https://github.com/plexobject/plexspaces.git
cd plexspaces

# Build the project
make build

# Run tests
make test  # Rust workspace + optional polyglot SDK tests — see docs/testing.md
```

See [Installation Guide](installation.md) for detailed setup instructions.
See [Testing Guide](testing.md) for how to run unit tests, integration tests, Rust SDK crates, and Python/TypeScript/Go SDK tests.

**Note**: For actors using non-memory channels (Redis, Kafka, SQLite, NATS), graceful shutdown is automatically handled. When an actor stops, it completes all in-progress messages before terminating. See [Durability Guide](durability.md) for details on graceful shutdown and message recovery.

## Your First Actor

Let's create a simple counter actor that demonstrates the core concepts. This example shows:
- Creating an actor
- Sending messages (tell and ask)
- Getting replies
- Basic actor lifecycle

### Rust Actor (SDK Annotations)

The [PlexSpaces Rust SDK](sdk.md#rust-sdk) provides decorator-style annotations for minimal boilerplate:

```rust
use plexspaces_node::NodeBuilder;
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    RequestContext,
    spawn_with_facets, call_message, json,
};
use std::time::Duration;

// Step 1: Define actor with #[gen_server_actor] annotation
#[gen_server_actor]
struct Counter {
    count: i32,
}

impl Counter {
    fn new() -> Self {
        Self { count: 0 }
    }
}

// Step 2: Define handlers with #[plexspaces_handlers]
// GenServer defaults to "call" semantics (request-reply)
#[plexspaces_handlers]
impl Counter {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &plexspaces_sdk::ActorContext,
        msg: &plexspaces_sdk::Message,
    ) -> Result<serde_json::Value, plexspaces_sdk::BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)?;
        let amount = payload["amount"].as_i64().unwrap_or(1) as i32;
        self.count += amount;
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create and start node using the unified embedded startup path.
    let node = NodeBuilder::new("node1")
        .with_clustering_enabled(false)
        .build_started()
        .await;
    let service_locator = node.service_locator();

    // Create request context with tenant/namespace (required for tenant isolation)
    let ctx = RequestContext::new_without_auth(
        "my-tenant".to_string(),
        "default".to_string(),
    );
    
    // Spawn actor using SDK helper
    let actor_ref = spawn_with_facets(
        &ctx,
        service_locator.clone(),
        "counter",
        "default",
        Counter::new(),
        vec![], // facets
    ).await?;
    
    // Send messages using SDK message helpers
    // call_message() creates a message with "call" semantics for ask()
    let increment_msg = call_message(json!({ "action": "increment", "amount": 5 }));
    let reply = actor_ref.ask(increment_msg, Duration::from_secs(5)).await?;
    let result: serde_json::Value = serde_json::from_slice(&reply.payload)?;
    println!("Count after increment: {}", result["count"]);
    
    let get_msg = call_message(json!({ "action": "get" }));
    let reply = actor_ref.ask(get_msg, Duration::from_secs(5)).await?;
    let result: serde_json::Value = serde_json::from_slice(&reply.payload)?;
    println!("Current count: {}", result["count"]);
    
    // Shutdown
    node.shutdown(Duration::from_secs(3)).await?;
    Ok(())
}
```

**Key Points:**
- Use `#[gen_server_actor]` annotation instead of implementing traits manually
- Use `#[plexspaces_handlers]` with `#[handler("op")]` for message routing
- Use `spawn_with_facets()` from SDK to spawn actors
- Use `call_message()` for request-reply messages (with `ask()`)
- Use `cast_message()` for fire-and-forget messages (with `tell()`)
- Always use `RequestContext::new_without_auth()` with tenant/namespace (never `internal()`)

### Python Actor (WASM with SDK)

The [PlexSpaces Python SDK](sdk.md) provides decorator-based actor development with minimal boilerplate:

```python
# counter_actor.py
from plexspaces import actor, state, handler

@actor
class CounterActor:
    count: int = state(default=0)
    
    @handler("increment")
    def increment(self, amount: int = 1) -> dict:
        self.count += amount
        return {"count": self.count}
    
    @handler("get")
    def get(self) -> dict:
        return {"count": self.count}
```

Build and deploy:

```bash
# Build to WASM
plexspaces-py build counter_actor.py -o counter_actor.wasm

# Deploy (via HTTP API)
curl -X POST http://localhost:8094/api/v1/deploy \
  -F "namespace=default" \
  -F "actor_type=counter" \
  -F "wasm=@counter_actor.wasm"
```

### TypeScript Actor (WASM with SDK)

The [PlexSpaces TypeScript SDK](sdk.md#typescript-sdk) uses inheritance: extend `PlexSpacesActor<TState>` and implement `on<Op>(payload)` handlers. Same WIT world as Python (`plexspaces-actor`). Build with `jco componentize ... --disable all`. 

**SDK Simplification**: The SDK automatically generates WIT TypeScript types during build - you don't need to run `jco types` or import generated files. The SDK uses iterative JSON serialization to avoid WASM recursion issues. Just extend the base class and implement handlers - the SDK handles all WIT details.

See [examples/typescript/apps/bank_account](../examples/typescript/apps/bank_account/README.md) for a full example and E2E test.

See [SDK Guide](sdk.md) for complete Python and TypeScript SDK documentation.
See [WASM Deployment](wasm-deployment.md) and [Examples](../examples/README.md) for Python, TypeScript, Go, and Rust WASM actor examples. For parameter sweep with elastic pool (checkout/checkin) and tuple space, see [Parameter sweep (migrating_merlin)](../examples/python/apps/migrating_merlin/README.md) (available in all four languages).

## Key Concepts

This section provides a brief overview. For detailed explanations, see the [Concepts Guide](concepts.md).

### Actors

Actors are the fundamental unit of computation in PlexSpaces:

- **Stateful**: Each actor maintains private state
- **Sequential**: Messages processed one at a time
- **Location-Transparent**: Work the same locally or remotely
- **Fault-Tolerant**: Automatic recovery and supervision

### ActorRef

An `ActorRef` is a lightweight handle to an actor:

```rust
// Get actor reference by logical actor name
let actor_ref = node.get_actor_ref("counter").await?;

// Fire-and-forget (tell)
actor_ref.tell(message).await?;

// Request-reply (ask)
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;
```

### Behaviors

Behaviors define how actors process messages:

- **GenServerBehavior**: Erlang/OTP-style request/reply
- **GenFSMBehavior**: Finite state machine
- **GenEventBehavior**: Event-driven processing
- **WorkflowBehavior**: Durable workflow orchestration

### Facets

Facets add dynamic capabilities to actors:

- **VirtualActorFacet**: Orleans-style activation/deactivation
- **DurabilityFacet**: Automatic persistence and recovery
- **TimerFacet**: Scheduled tasks and periodic operations
- **ReminderFacet**: Persistent scheduled reminders

**Learn More**: See the [Concepts Guide](concepts.md) for comprehensive explanations of all core concepts.

## Next Steps

1. **Learn Core Concepts**: Read the [concepts guide](concepts.md) to understand Actors, Behaviors, Facets, and more
2. **Configure Security**: Set up mTLS, JWT, and tenant isolation - see [Security Guide](security.md)
3. **Usage Patterns**: Learn practical usage patterns with [Usage Guide](usage.md)
4. **Explore Examples**: Check out the [examples directory](../examples/README.md) for more patterns
5. **Read Architecture**: Understand the [system design](architecture.md)
6. **FaaS Invocation**: Learn how to invoke actors via HTTP: `GET /api/v1/actors/{namespace}/{actor_type}` and `/ask` use `AskReply`; `POST`/`PUT /api/v1/actors/{namespace}/{actor_type}` use `SendMessage`; `POST`/`PUT /ask` use `AskReply` with a request body. See [Concepts: FaaS-Style Invocation](concepts.md#faas-style-invocation)
7. **Deploy to Production**: Follow the [installation guide](installation.md)
8. **Learn Use Cases**: See [real-world applications](use-cases.md)

## Common Patterns

### Durable Actor

```rust
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    spawn_with_storage, DurabilityFacet, SqliteJournalStorage,
    RequestContext, json,
};
use std::sync::Arc;

// Define durable actor with facets annotation
#[gen_server_actor(facets = ["durability"])]
struct DurableCounter {
    count: i32,
}

#[plexspaces_handlers]
impl DurableCounter {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &plexspaces_sdk::ActorContext,
        _msg: &plexspaces_sdk::Message,
    ) -> Result<serde_json::Value, plexspaces_sdk::BehaviorError> {
        self.count += 1;
        Ok(json!({ "count": self.count }))
    }
}

// Spawn with storage backend
let storage = Arc::new(SqliteJournalStorage::new(":memory:").await?);
let ctx = RequestContext::new_without_auth("tenant".to_string(), "ns".to_string());

let actor_ref = spawn_with_storage(
    &ctx,
    service_locator.clone(),
    "durable-counter",
    "default",
    DurableCounter { count: 0 },
    storage,
).await?;
```

### Virtual Actor

```rust
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    spawn, VirtualActorFacet,
    RequestContext, json,
};

// Define virtual actor with facets annotation
// Virtual actors are activated on-demand and deactivated after idle timeout
#[gen_server_actor(facets = ["virtual_actor"])]
struct VirtualCounter {
    count: i32,
}

#[plexspaces_handlers]
impl VirtualCounter {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &plexspaces_sdk::ActorContext,
        _msg: &plexspaces_sdk::Message,
    ) -> Result<serde_json::Value, plexspaces_sdk::BehaviorError> {
        self.count += 1;
        Ok(json!({ "count": self.count }))
    }
}

// Spawn using SDK spawn() - facets are auto-created from annotation
let ctx = RequestContext::new_without_auth("tenant".to_string(), "ns".to_string());

let actor_ref = spawn(
    &ctx,
    service_locator.clone(),
    "virtual-counter",
    "default",
    VirtualCounter { count: 0 },
).await?;
```

### Workflow

```rust
use plexspaces_sdk::{
    workflow_actor, plexspaces_handlers, run_handler, signal_handler, query_handler,
    spawn_workflow_actor, WorkflowRef,
    RequestContext, json,
};

// Define workflow actor with annotations
#[workflow_actor(facets = ["durability"])]
struct OrderWorkflow {
    order_id: String,
    status: String,
}

#[plexspaces_handlers(workflow)]
impl OrderWorkflow {
    // Main workflow execution
    #[run_handler]
    async fn run(
        &mut self,
        ctx: &plexspaces_sdk::ActorContext,
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
    
    // Signal handler for external events
    #[signal_handler("cancel")]
    async fn on_cancel(
        &mut self,
        _ctx: &plexspaces_sdk::ActorContext,
        _data: plexspaces_sdk::Message,
    ) -> Result<(), plexspaces_sdk::BehaviorError> {
        self.status = "cancelled".to_string();
        Ok(())
    }
    
    // Query handler (read-only)
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

// Spawn and interact with workflow
let ctx = RequestContext::new_without_auth("tenant".to_string(), "ns".to_string());

let workflow: WorkflowRef = spawn_workflow_actor(
    &ctx,
    service_locator.clone(),
    "order-workflow-123",
    OrderWorkflow { order_id: String::new(), status: "pending".to_string() },
    vec![],
).await?;

// Run workflow
let result = workflow.run(&json!({ "order_id": "ORD-123" })).await?;

// Query status
let status: serde_json::Value = workflow.query("status").await?;

// Send signal
workflow.signal("cancel", &json!({})).await?;
```

## Troubleshooting

### Actor Not Found

If you get an "actor not found" error:

1. Check the actor ID format: `name//actor_type::namespace@node_id`
2. Verify the actor was spawned with the intended unique `name`
3. If you are writing client code, do not construct the full actor ID when spawning; use the unique actor name and let the runtime create the canonical ID
4. Ensure the actor was spawned before sending messages
5. For virtual actors, the first message will auto-activate

### Connection Errors

If you see connection errors:

1. **Verify node health**: The SDK uses health-aware connection (checks liveness/readiness)
   ```rust
   use plexspaces_sdk::NodeClient;
   
   // SDK automatically checks liveness and waits for readiness
   let mut node_client = NodeClient::connect("http://localhost:8000").await?;
   ```

2. **Check node status**: Verify the node is running and healthy
   ```bash
   # HTTP health endpoint
   curl http://localhost:8080/health
   
   # gRPC health (if grpc_health_probe available)
   grpc_health_probe -addr=localhost:8000
   ```

3. **Review connection details**: SDK provides detailed error messages
   - Liveness check failures: Node not alive yet (may need to wait)
   - Readiness timeout: Node alive but not ready (check dependencies)
   - Connection failures: Network/firewall issues

4. **Check network connectivity**: Ensure nodes can reach each other
   ```bash
   # Test gRPC port
   nc -z localhost 8000
   
   # Check firewall rules for gRPC port (default: 8000)
   ```

5. **Multi-node connection**: SDK handles partial success gracefully
   ```rust
   let resp = node_client.connect_nodes(
       vec!["http://localhost:8001".to_string()],
       None,
       30,
   ).await?;
   
   // Check which nodes connected and which failed
   println!("Connected: {:?}", resp.connected);
   println!("Failed: {:?}", resp.failed);
   ```

See [SDK Documentation](sdk.md#node-connectivity-health-aware-connection) for health-aware connection details.

### Build Errors

If you encounter build errors:

1. Ensure Rust 1.70+ is installed: `rustc --version`
2. Update dependencies: `cargo update`
3. Clean and rebuild: `cargo clean && cargo build`

## Resources

- **Documentation**: [Full API docs](https://docs.rs/plexspaces/)
- **Examples**: [Example gallery](../examples/README.md)
- **Community**: [GitHub Discussions](https://github.com/plexobject/plexspaces/discussions)
- **Issues**: [Report bugs](https://github.com/plexobject/plexspaces/issues)

---

**Ready to build?** Check out the [examples](../examples/README.md) or read the [architecture guide](architecture.md)!
