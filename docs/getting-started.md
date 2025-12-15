# Getting Started with PlexSpaces

This guide will help you get started with PlexSpaces in minutes. You'll learn how to create your first actor, send messages, and use key features.

**New to PlexSpaces?** Start here, then read the [Concepts Guide](concepts.md) to understand the fundamentals.


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
make test
```

See [Installation Guide](installation.md) for detailed setup instructions.

## Your First Actor

Let's create a simple counter actor that demonstrates the core concepts. This example shows:
- Creating an actor
- Sending messages (tell and ask)
- Getting replies
- Basic actor lifecycle

### Rust Actor

```rust
use plexspaces::*;
use plexspaces_behavior::GenServerBehavior;
use std::time::Duration;

struct Counter {
    count: i32,
}

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

enum CounterRequest {
    Increment(i32),
    Get,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a node
    let node = PlexSpacesNode::new("node1".to_string()).await?;
    
    // Spawn a counter actor
    let actor_id = "counter@node1".to_string();
    let counter = Counter { count: 0 };
    node.spawn_actor(actor_id.clone(), counter).await?;
    
    // Get actor reference
    let actor_ref = node.get_actor_ref(&actor_id).await?;
    
    // Send messages
    let reply1 = actor_ref.ask(
        CounterRequest::Increment(5),
        Duration::from_secs(5)
    ).await?;
    println!("Count after increment: {}", reply1);
    
    let reply2 = actor_ref.ask(
        CounterRequest::Get,
        Duration::from_secs(5)
    ).await?;
    println!("Current count: {}", reply2);
    
    Ok(())
}
```

### Python Actor (WASM)

```python
# counter_actor.py
from plexspaces import Actor, tell, ask

class CounterActor(Actor):
    def __init__(self):
        self.count = 0
    
    async def handle_increment(self, amount: int) -> int:
        self.count += amount
        return self.count
    
    async def handle_get(self) -> int:
        return self.count

# Deploy and use
async def main():
    actor = CounterActor()
    await actor.start("counter@node1")
    
    result = await ask(actor, "increment", 5)
    print(f"Count: {result}")
    
    count = await ask(actor, "get")
    print(f"Current count: {count}")
```

See [WASM Examples](../examples/wasm_showcase/) for more Python/JavaScript actor examples.

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
// Get actor reference
let actor_ref = node.get_actor_ref(&"counter@node1".to_string()).await?;

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
2. **Explore Examples**: Check out the [examples directory](../examples/) for more patterns
3. **Read Architecture**: Understand the [system design](architecture.md)
4. **FaaS Invocation**: Learn how to invoke actors via HTTP (GET/POST) for serverless patterns - see [Concepts: FaaS-Style Invocation](concepts.md#faas-style-invocation)
5. **Deploy to Production**: Follow the [installation guide](installation.md)
6. **Learn Use Cases**: See [real-world applications](use-cases.md)

## Common Patterns

### Durable Actor

```rust
use plexspaces_facet::DurabilityFacet;

let actor = ActorBuilder::new("durable-counter@node1".to_string())
    .with_behavior(Counter { count: 0 })
    .with_facet(DurabilityFacet::new())  // Automatic persistence
    .build()?;
```

### Virtual Actor

```rust
use plexspaces_facet::VirtualActorFacet;

let actor = ActorBuilder::new("virtual-counter@node1".to_string())
    .with_behavior(Counter { count: 0 })
    .with_facet(VirtualActorFacet::new())  // Auto activation/deactivation
    .build()?;
```

### Workflow

```rust
use plexspaces_behavior::WorkflowBehavior;

struct OrderWorkflow {
    order_id: String,
}

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

## Troubleshooting

### Actor Not Found

If you get an "actor not found" error:

1. Check the actor ID format: `name@node_id`
2. Ensure the actor was spawned before sending messages
3. For virtual actors, the first message will auto-activate

### Connection Errors

If you see connection errors:

1. Verify the node is running: `curl http://localhost:8080/health`
2. Check network connectivity between nodes
3. Review firewall rules for gRPC port (default: 9001)

### Build Errors

If you encounter build errors:

1. Ensure Rust 1.70+ is installed: `rustc --version`
2. Update dependencies: `cargo update`
3. Clean and rebuild: `cargo clean && cargo build`

## Resources

- **Documentation**: [Full API docs](https://docs.rs/plexspaces/)
- **Examples**: [Example gallery](../examples/)
- **Community**: [GitHub Discussions](https://github.com/plexobject/plexspaces/discussions)
- **Issues**: [Report bugs](https://github.com/plexobject/plexspaces/issues)

---

**Ready to build?** Check out the [examples](../examples/) or read the [architecture guide](architecture.md)!
