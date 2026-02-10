# Order Processing Example

**Real-World Use Case**: E-commerce order management with CRUD operations.

## Quick Start

```bash
cd examples/rust/embedded/order_processing
cargo run
```

## What It Demonstrates

1. **SDK Annotations** - Zero-boilerplate actor definition
2. **GenServer Pattern** - Request-reply messaging
3. **Actor Spawning** - Simple `spawn_actor()` helper
4. **Message Handling** - Typed handlers with `#[handler("op")]`

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     OrderProcessor Actor                        │
├─────────────────────────────────────────────────────────────────┤
│  State: HashMap<order_id, Order>                                │
│                                                                 │
│  Handlers:                                                      │
│    #[handler("create")] → Create new order                     │
│    #[handler("get")]    → Get order by ID                      │
│    #[handler("cancel")] → Cancel pending order                 │
│    #[handler("list")]   → List all orders                      │
└─────────────────────────────────────────────────────────────────┘
```

## SDK Pattern

```rust
use plexspaces_sdk::*;

// 1. Define actor with annotation
#[gen_server_actor]
struct OrderProcessor {
    orders: HashMap<String, Order>,
}

// 2. Add handlers
#[plexspaces_handlers(gen_server)]
impl OrderProcessor {
    #[handler("create")]
    async fn handle_create(&mut self, ctx: &ActorContext, msg: &Message) 
        -> Result<Value, BehaviorError> {
        // Handle create order
        Ok(json!({ "order_id": "ORD-001" }))
    }
}

// 3. Spawn and use
let actor_ref = spawn_actor(&ctx, service_locator, "order-processor", "orders", 
    OrderProcessor::new(), vec![]).await?;

// Create message with "op" field in payload for routing
let msg = Message {
    id: ulid::Ulid::new().to_string(),
    message_type: "call".to_string(),
    payload: serde_json::to_vec(&json!({ "op": "create", "customer_id": "alice" }))?,
    ..Default::default()
};
let response = actor_ref.ask(msg, timeout).await?;
```

## Key APIs

| API | Purpose |
|-----|---------|
| `#[gen_server_actor]` | Mark struct as GenServer actor |
| `#[plexspaces_handlers(gen_server)]` | Generate handler dispatch |
| `#[handler("op")]` | Route message type to method |
| `spawn_actor()` | Create actor instance |
| `actor_ref.ask()` | Send request, await reply |

## Use Cases

- E-commerce order management
- Shopping cart services
- Inventory tracking
- Payment processing
- Booking systems

## See Also

- [SDK Documentation](../../../../docs/sdk.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
