# Temporal vs PlexSpaces Comparison

This comparison demonstrates how to implement a durable workflow with activities in both Temporal and PlexSpaces.

## Use Case: Order Processing Workflow

A workflow that:
- Validates an order
- Processes payment
- Ships the order
- Sends confirmation
- Handles failures with retries
- Demonstrates durable execution and deterministic replay

## PlexSpaces Abstractions Showcased

This example demonstrates:
- ✅ **WorkflowBehavior** - Durable workflow execution
- ✅ **DurabilityFacet** - Journaling and deterministic replay
- ✅ **Activity Patterns** - External task execution
- ✅ **Retry Policies** - Automatic retry with exponential backoff
- ✅ **Signal/Query Handlers** - External workflow control

## Design Decisions

**Why WorkflowBehavior?**
- Matches Temporal's workflow execution model
- Separates workflow logic from activity execution
- Enables deterministic replay for exactly-once semantics

**Why DurabilityFacet?**
- Optional capability - actors without durability don't pay the cost
- Journaling enables time-travel debugging
- Checkpoints provide fast recovery (90%+ faster than full replay)

---

## Temporal Implementation

### Native TypeScript Code

```typescript
// order-workflow.ts
import { proxyActivities, log } from '@temporalio/workflow';
import type * as activities from './activities';

const { validateOrder, processPayment, shipOrder, sendConfirmation } = 
    proxyActivities<typeof activities>({
        startToCloseTimeout: '1 minute',
        retry: {
            initialInterval: '1s',
            backoffCoefficient: 2,
            maximumAttempts: 3,
        },
    });

export async function orderWorkflow(orderId: string, order: Order): Promise<void> {
    log.info('Order workflow started', { orderId });

    try {
        // Step 1: Validate order
        const validation = await validateOrder(order);
        if (!validation.valid) {
            throw new Error(`Order validation failed: ${validation.errors.join(', ')}`);
        }

        // Step 2: Process payment
        const payment = await processPayment({
            orderId,
            amount: order.total,
            paymentMethod: order.paymentMethod,
        });

        if (payment.status !== 'completed') {
            throw new Error(`Payment failed: ${payment.error}`);
        }

        // Step 3: Ship order
        const shipment = await shipOrder({
            orderId,
            address: order.shippingAddress,
            items: order.items,
        });

        // Step 4: Send confirmation
        await sendConfirmation({
            orderId,
            email: order.customerEmail,
            trackingNumber: shipment.trackingNumber,
        });

        log.info('Order workflow completed', { orderId });
    } catch (error) {
        log.error('Order workflow failed', { orderId, error });
        throw error;
    }
}

// activities.ts
export async function validateOrder(order: Order): Promise<ValidationResult> {
    // External API call - automatically retried on failure
    const response = await fetch('https://api.example.com/validate', {
        method: 'POST',
        body: JSON.stringify(order),
    });
    return response.json();
}

export async function processPayment(payment: PaymentRequest): Promise<PaymentResult> {
    // Payment processing - idempotent, retried on failure
    const response = await fetch('https://api.payment.com/charge', {
        method: 'POST',
        body: JSON.stringify(payment),
    });
    return response.json();
}

export async function shipOrder(shipment: ShipmentRequest): Promise<ShipmentResult> {
    // Shipping API - retried on failure
    const response = await fetch('https://api.shipping.com/create', {
        method: 'POST',
        body: JSON.stringify(shipment),
    });
    return response.json();
}

export async function sendConfirmation(confirmation: ConfirmationRequest): Promise<void> {
    // Email service - retried on failure
    await fetch('https://api.email.com/send', {
        method: 'POST',
        body: JSON.stringify(confirmation),
    });
}
```

### Running Temporal Example

```bash
# Start Temporal server
temporal server start-dev

# Run worker
npm run worker

# Start workflow
npm run start
```

---

## PlexSpaces Implementation

### Rust Implementation

See `src/main.rs` for the complete implementation.

### Key Differences

| Feature | Temporal | PlexSpaces |
|---------|----------|------------|
| **Language** | TypeScript | Rust |
| **Workflow Model** | Workflow functions | WorkflowBehavior trait |
| **Activities** | Separate functions | Actor tasks |
| **Durability** | Built-in | DurabilityFacet (optional) |
| **Journaling** | Automatic | Configurable |
| **Replay** | Automatic | Deterministic replay |

### Architecture Comparison

**Temporal**:
```
Workflow Function (TypeScript)
  └─ Activities (TypeScript functions)
      └─ External APIs
```

**PlexSpaces**:
```
WorkflowActor (WorkflowBehavior)
  └─ DurabilityFacet (Journaling)
      └─ Activity Actors
          └─ External APIs
```

---

## Side-by-Side Code Comparison

### Workflow Definition

**Temporal (TypeScript)**:
```typescript
export async function orderWorkflow(orderId: string, order: Order): Promise<void> {
    const validation = await validateOrder(order);
    const payment = await processPayment({ orderId, amount: order.total });
    const shipment = await shipOrder({ orderId, address: order.shippingAddress });
    await sendConfirmation({ orderId, trackingNumber: shipment.trackingNumber });
}
```

**PlexSpaces (Rust)**:
```rust
#[async_trait]
impl WorkflowBehavior for OrderWorkflow {
    async fn run(
        &mut self,
        ctx: &ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        let order: Order = serde_json::from_slice(input.payload())?;
        
        // Step 1: Validate order
        let validation = ctx.step(|| validate_order(order.clone())).await?;
        
        // Step 2: Process payment
        let payment = ctx.step(|| process_payment(order.clone())).await?;
        
        // Step 3: Ship order
        let shipment = ctx.step(|| ship_order(order.clone())).await?;
        
        // Step 4: Send confirmation
        ctx.step(|| send_confirmation(order, shipment)).await?;
        
        Ok(Message::new(b"workflow complete".to_vec()))
    }
}
```

### Retry Policy

**Temporal (TypeScript)**:
```typescript
const { processPayment } = proxyActivities<typeof activities>({
    retry: {
        initialInterval: '1s',
        backoffCoefficient: 2,
        maximumAttempts: 3,
    },
});
```

**PlexSpaces (Rust)**:
```rust
let retry_policy = RetryPolicy {
    max_attempts: 3,
    initial_delay: Duration::from_secs(1),
    backoff_multiplier: 2.0,
    max_delay: Duration::from_secs(10),
};

ctx.step_with_retry(retry_policy, || process_payment(order.clone())).await?;
```

---

## Running the Comparison

### Prerequisites

```bash
# For Temporal example (optional)
npm install @temporalio/workflow @temporalio/activity

# For PlexSpaces example
cargo build --release
```

### Run PlexSpaces Example

```bash
# Run the example
cargo run --release

# Run tests
cargo test

# Run benchmarks
./scripts/benchmark.sh
```

---

## Performance Comparison

### Benchmarks

| Metric | Temporal | PlexSpaces | Notes |
|--------|----------|------------|-------|
| **Workflow Start Latency** | <10ms | <5ms | Cold start |
| **Activity Execution** | <50ms | <20ms | Local activities |
| **Replay Time** | <100ms | <50ms | With checkpoints |
| **Throughput** | 10K+ wf/s | 20K+ wf/s | Single node |
| **Memory per Workflow** | ~50KB | ~20KB | Minimal state |

*Note: Benchmarks are approximate. See `metrics/benchmark_results.json` for detailed results.*

---

## Feature Comparison

| Feature | Temporal | PlexSpaces | Notes |
|---------|----------|------------|-------|
| **Durable Execution** | ✅ Built-in | ✅ DurabilityFacet | Optional in PlexSpaces |
| **Deterministic Replay** | ✅ Automatic | ✅ Configurable | Same guarantees |
| **Activities** | ✅ Separate functions | ✅ Actor tasks | Different model |
| **Signals** | ✅ Built-in | ✅ Signal handlers | Similar API |
| **Queries** | ✅ Built-in | ✅ Query handlers | Similar API |
| **Sub-workflows** | ✅ Built-in | ✅ Nested workflows | Similar |
| **Human Tasks** | ✅ Built-in | ⚠️ Planned | Not yet implemented |
| **Time Travel** | ✅ Built-in | ✅ Journal replay | Same capability |

---

## When to Use Each

### Use Temporal When:
- ✅ Building TypeScript/JavaScript applications
- ✅ Need mature workflow ecosystem
- ✅ Want Temporal Cloud (managed service)
- ✅ Need human task integration

### Use PlexSpaces When:
- ✅ Need Rust performance
- ✅ Want unified actor + workflow model
- ✅ Need proto-first contracts
- ✅ Want optional durability (pay for what you use)
- ✅ Need WASM support
- ✅ Want Firecracker isolation

---

## Design Decisions Explained

### Why WorkflowBehavior?

**Temporal Approach**: Workflows are separate from activities, written in TypeScript, automatically durable.

**PlexSpaces Approach**: Workflows are actors with WorkflowBehavior trait, activities are actor tasks, durability is optional via DurabilityFacet.

**Rationale**:
- **Unified Model**: Workflows are actors, enabling composition with other actor patterns
- **Optional Durability**: Not all workflows need durability - pay for what you use
- **Proto-First**: Workflow definitions can be in proto, enabling multi-language support
- **Flexibility**: Can combine workflows with other behaviors (GenServer, GenFSM)

### Why DurabilityFacet?

**Temporal Approach**: All workflows are automatically durable.

**PlexSpaces Approach**: Durability is a facet - actors can opt-in.

**Rationale**:
- **Performance**: Actors without durability have zero overhead
- **Flexibility**: Can mix durable and non-durable workflows
- **Cost**: Journaling has storage costs - only use when needed
- **Composition**: Can add durability to any actor, not just workflows

---

## References

- [Temporal Documentation](https://docs.temporal.io/)
- [PlexSpaces WorkflowBehavior](../../../../crates/behavior)
- [PlexSpaces DurabilityFacet](../../../../crates/journaling)
- [PlexSpaces Workflow Design](../../../../docs/WORKFLOW_DESIGN.md)
