# Order Processing Microservices Example

## Overview

This example demonstrates a complete order processing microservices architecture using PlexSpaces, showcasing:

- **Hybrid Behavior Pattern**: Use what makes sense logically
  - **GenServerBehavior** for queries (request/reply) - `GetOrder`, `GetStatus`
  - **ActorBehavior** for commands (fire-and-forget) - `CreateOrder`, `CancelOrder`
  - **GenEventBehavior** for event handling - Order lifecycle events
  - **GenStateMachine** for stateful workflows - Order state transitions
- **Typed Messages**: Using `ActorMessage` enum for type-safe messaging
- **Config Bootstrap**: Erlang/OTP-style configuration loading (env > release.toml > defaults)
- **Coordination vs Compute Metrics**: Tracking framework efficiency (granularity ratio)
- **Event-Driven Architecture**: Distributed pub/sub via channels (InMemory/Redis/Kafka)
- **Supervision Trees**: Hierarchical fault tolerance with OneForOne/OneForAll strategies

## Architecture

```
RootSupervisor (OneForOne)
  ├─ ServiceSupervisor (OneForOne)
  │   ├─ PaymentServiceActor (Permanent, CircuitBreaker)
  │   ├─ InventoryServiceActor (Permanent)
  │   └─ ShippingServiceActor (Permanent, CircuitBreaker)
  └─ OrderProcessorActor (Permanent)
```

## Behavior Selection Guide

### Hybrid Approach: Use What Makes Sense Logically

This example uses a **hybrid behavior approach** - selecting the right behavior for each operation based on semantics, not convenience. This follows the principle: **use what makes sense logically**.

#### GenServerBehavior (Request/Reply)
**Use for**: Operations that need to return data synchronously
- `GetOrder` - Returns order details
- `GetOrderStatus` - Returns current state
- `ListOrders` - Returns order list

**Pattern**: Request → Process → Reply
```rust
impl GenServerBehavior for OrderProcessor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        // MUST return a reply
        let order = self.orders.get(&order_id)?;
        Ok(Message::new(serde_json::to_vec(&order)?))
    }
}
```

#### ActorBehavior (Fire-and-Forget)
**Use for**: Operations that don't need immediate response
- `CreateOrder` - Creates order, publishes event
- `CancelOrder` - Cancels order, publishes event
- `ProcessPayment` - Processes payment, publishes event

**Pattern**: Command → Process → Publish Event
```rust
impl ActorBehavior for OrderProcessor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // No reply needed - fire-and-forget
        self.handle_create_order(ctx, customer_id, items).await?;
        Ok(())
    }
}
```

#### GenEventBehavior (Event Handling)
**Use for**: Event subscriptions and notifications
- Order lifecycle events
- Payment notifications
- Shipping updates

#### GenStateMachine (FSM)
**Use for**: Stateful workflows with explicit state transitions
- Order state machine (Pending → PaymentProcessing → Reserved → Shipped → Completed)

### Decision Matrix

| Operation Type | Pattern | Behavior | Example |
|---------------|---------|----------|---------|
| Query | Request/Reply | GenServerBehavior | GetOrder, GetStatus |
| Command | Fire-and-Forget | ActorBehavior | CreateOrder, CancelOrder |
| Event | Fire-and-Forget | ActorBehavior/GenEvent | OrderCreated, PaymentCompleted |
| State Machine | State Transitions | GenStateMachine | Order lifecycle |
| Service API | Request/Reply | GenServerBehavior | REST-like endpoints |

## Domain Model

### Order States (FSM)
- `Pending` → `PaymentProcessing` → `Reserved` → `Shipped` → `Completed`
- Error states: `Cancelled`, `Failed`

### Services
1. **OrderProcessorActor**: Coordinates the end-to-end order workflow
2. **PaymentServiceActor**: Processes payments with circuit breaker and idempotency
3. **InventoryServiceActor**: Manages stock with optimistic locking
4. **ShippingServiceActor**: Creates shipment labels with circuit breaker

## Configuration

### Erlang/OTP-Style Configuration

Uses `ConfigBootstrap` for layered configuration (Erlang/OTP-inspired):

**Precedence** (highest to lowest):
1. Environment variables
2. `release.toml` (application config)
3. Default values

**Example `release.toml`**:
```toml
[order_processor]
max_orders = 1000
enable_metrics = true

[channel]
use_in_memory = true
use_redis = false
redis_url = "redis://localhost:6379"

[node]
node_id = "order-node-1"
grpc_port = 8000
```

**Environment Variables**:
```bash
export ORDER_PROCESSOR_MAX_ORDERS=2000
export ORDER_PROCESSOR_ENABLE_METRICS=true
export CHANNEL_USE_REDIS=true
export REDIS_URL=redis://localhost:6379
```

## Event-Driven Architecture

The order processing workflow publishes events at key stages via the **ChannelManager**, supporting multiple backend types:

### Channel Backends

1. **InMemory Channel**: Fast local pub/sub within same node
   - Use case: Development, single-node deployments
   - Delivery: At-most-once (fire-and-forget)
   - Ordering: FIFO

2. **Redis Streams Channel**: Distributed coordination across nodes
   - Use case: Multi-node coordination, distributed events
   - Delivery: At-least-once (consumer groups)
   - Ordering: FIFO with partition keys (order ID)

3. **Kafka Channel**: Audit logging and event sourcing
   - Use case: Permanent event log, compliance, replay
   - Delivery: At-least-once (consumer groups)
   - Ordering: FIFO per partition (order ID as key)

### Order Events Flow

```
OrderCreated
  ↓
PaymentCompleted (or PaymentFailed)
  ↓
InventoryReserved (or InventoryFailed)
  ↓
ShipmentCreated (or ShipmentFailed)
  ↓
OrderCompleted
```

## Metrics: Coordination vs Compute

Tracks coordination overhead vs actual computation time for HPC workloads.

**Golden Rule**: `compute_time / coordinate_time >= 10x` (minimum), ideally `>= 100x`

**Usage**:
```rust
let mut metrics = CoordinationComputeTracker::new("order-processor");

// Track computation
metrics.start_compute();
// ... do actual work ...
metrics.end_compute();

// Track coordination
metrics.start_coordinate();
// ... send messages, wait for barriers ...
metrics.end_coordinate();

// Get final metrics
let report = metrics.finalize();
println!("Granularity ratio: {:.2}", report.granularity_ratio);
```

## Running Locally

### Prerequisites
- Rust 1.83+
- Docker (optional, for Redis/Kafka)

### Quick Start

```bash
# Build
cargo build --release

# Run (uses new framework features)
cargo run --bin order-coordinator

# Or run with environment variables
ORDER_PROCESSOR_MAX_ORDERS=1000 \
CHANNEL_USE_IN_MEMORY=true \
cargo run --bin order-coordinator
```

### Multi-Node Setup

```bash
# Terminal 1: Node 1
NODE_ID=node1 NODE_ADDRESS=localhost:8000 cargo run

# Terminal 2: Node 2
NODE_ID=node2 NODE_ADDRESS=localhost:8001 cargo run
```

## Implementation

The example demonstrates:
- Hybrid behavior approach (GenServer + ActorBehavior)
- Typed messages (`ActorMessage` enum)
- Config bootstrap
- Coordination vs compute metrics

**File**: `src/actors/order_processor.rs`

**Usage**:
```rust
use order_processing::actors::order_processor::OrderProcessorBehavior;

let behavior = Box::new(OrderProcessorBehavior::new());
let actor = ActorBuilder::new(behavior)
    .with_name("order-processor")
    .build();
// Spawn using ActorFactory
use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
use std::sync::Arc;

let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
    .ok_or_else(|| "ActorFactory not found")?;
let actor_id = actor.id().clone();
let ctx = plexspaces_core::RequestContext::internal();
let _message_sender = actor_factory.spawn_actor(
    &ctx,
    &actor_id,
    "Workflow", // actor_type
    vec![], // initial_state
    None, // config
    std::collections::HashMap::new(), // labels
).await?;
let actor_ref = plexspaces_core::ActorRef::new(actor_id)?;
```

## Testing

```bash
# Run all tests
cargo test

# Run integration tests
cargo test --test integration_order_flow

# Run with logging
RUST_LOG=info cargo test
```

## Key Framework Features Used

1. **ConfigBootstrap**: Erlang/OTP-style configuration loading
2. **CoordinationComputeTracker**: Metrics for coordination vs compute
3. **ActorMessage Enum**: Type-safe message handling
4. **FacetService**: Access facets via `ActorContext::facet_service`
5. **NodeBuilder**: Fluent API for node creation
6. **ActorBuilder**: Fluent API for actor creation

## Migration Notes

### Deprecated APIs
- `ActorRegistry` → Use `ObjectRegistry` instead

### Breaking Changes
- Channel enum names updated: `ChannelBackend::InMemory` → `ChannelBackend::ChannelBackendInMemory`
- Delivery guarantee enum names updated: `DeliveryGuarantee::AtMostOnce` → `DeliveryGuarantee::DeliveryGuaranteeAtMostOnce`

## Implementation Details

### Order Processor Actor

The order processor demonstrates the hybrid behavior approach:

**File**: `src/actors/order_processor.rs`

**Key Features**:
- Implements both `ActorBehavior` and `GenServerBehavior`
- Commands (`CreateOrder`, `CancelOrder`) handled by `ActorBehavior`
- Queries (`GetOrder`) handled by `GenServerBehavior`
- Uses `ConfigBootstrap` for configuration
- Tracks coordination vs compute metrics

**Example Usage**:
```rust
use order_processing::actors::order_processor::OrderProcessorBehavior;

let behavior = Box::new(OrderProcessorBehavior::new());
let actor = ActorBuilder::new(behavior)
    .with_name("order-processor")
    .build();
// Spawn using ActorFactory
use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
use std::sync::Arc;

let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
    .ok_or_else(|| "ActorFactory not found")?;
let actor_id = actor.id().clone();
let ctx = plexspaces_core::RequestContext::internal();
let _message_sender = actor_factory.spawn_actor(
    &ctx,
    &actor_id,
    "Workflow", // actor_type
    vec![], // initial_state
    None, // config
    std::collections::HashMap::new(), // labels
).await?;
let actor_ref = plexspaces_core::ActorRef::new(actor_id)?;
```

## Further Reading

- Framework behavior documentation: `crates/behavior/src/mod.rs`
- Config bootstrap: `crates/node/src/config_bootstrap.rs`
- Metrics helper: `crates/node/src/metrics_helper.rs`

