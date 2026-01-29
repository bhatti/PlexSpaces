# Restate vs PlexSpaces Comparison

This comparison demonstrates how to implement durable execution with deterministic replay and journaling in both Restate and PlexSpaces.

## Use Case: Payment Processing with Idempotency

A payment processing service that:
- Processes payments idempotently
- Journals all operations for deterministic replay
- Handles failures with automatic recovery
- Demonstrates durable execution and exactly-once semantics

## PlexSpaces Abstractions Showcased

This example demonstrates:
- ✅ **DurabilityFacet** - Journaling and deterministic replay
- ✅ **Journaling** - Event sourcing and checkpoints
- ✅ **Deterministic Replay** - Exactly-once execution
- ✅ **Side Effect Caching** - Prevent duplicate external calls
- ✅ **Checkpoints** - Fast recovery (90%+ faster than full replay)

## Design Decisions

**Why DurabilityFacet?**
- Optional capability - actors without durability have zero overhead
- Matches Restate's durable execution model
- Enables time-travel debugging and exactly-once semantics

**Why Journaling?**
- Event sourcing enables deterministic replay
- Checkpoints provide fast recovery
- Side effect caching prevents duplicate external calls

---

## Restate Implementation

### Native TypeScript Code

See `native/order_service.ts` for the complete Restate implementation.

Key features:
- **Durable Execution**: All operations automatically journaled
- **Deterministic Replay**: Functions called again with same inputs on replay
- **Side Effect Caching**: External API calls cached during replay
- **Exactly-Once Semantics**: Idempotent operations with idempotency keys

```typescript
// Usage:
const result = await orderService.processOrder({
    id: "order-123",
    amount: 99.99,
    items: [...]
});
```
        const charge = await ctx.run("charge", async () => {
            // External API call with idempotency key
            const response = await fetch('https://api.payment.com/charge', {
                method: 'POST',
                headers: {
                    'Idempotency-Key': ctx.idempotencyKey(),
                },
                body: JSON.stringify({
                    orderId: request.orderId,
                    amount: request.amount,
                    paymentMethod: request.paymentMethod,
                }),
            });
            return response.json();
        });
        
        // Step 3: Record transaction (idempotent)
        await ctx.run("record", async () => {
            // Database write - idempotent
            await db.transactions.upsert({
                where: { transactionId: charge.transactionId },
                create: {
                    transactionId: charge.transactionId,
                    orderId: request.orderId,
                    amount: request.amount,
                    status: charge.status,
                },
                update: {
                    status: charge.status,
                },
            });
        });
        
        return {
            transactionId: charge.transactionId,
            status: charge.status,
        };
    },
});

// Start Restate service
restate.endpoint().bind(paymentService);
```

### Running Restate Example

```bash
# Start Restate server
restate dev

# Run the service
npm run start
```

---

## PlexSpaces Implementation

### Rust Implementation

See `src/main.rs` for the complete implementation.

### Key Differences

| Feature | Restate | PlexSpaces |
|---------|---------|------------|
| **Language** | TypeScript | Rust |
| **Durability** | Built-in | DurabilityFacet (optional) |
| **Journaling** | Automatic | Configurable |
| **Replay** | Automatic | Deterministic replay |
| **Side Effects** | ctx.run() caching | ExecutionContext caching |
| **Checkpoints** | Automatic | Configurable interval |

### Architecture Comparison

**Restate**:
```
Service Function (TypeScript)
  └─ ctx.run() (journaled operations)
      └─ External APIs (cached during replay)
```

**PlexSpaces**:
```
Actor (Rust)
  └─ DurabilityFacet (Journaling)
      └─ ExecutionContext (replay mode + side effect cache)
          └─ External APIs (cached during replay)
```

---

## Side-by-Side Code Comparison

### Durable Function

**Restate (TypeScript)**:
```typescript
paymentService.handler({
    name: "processPayment",
    handler: async (ctx: restate.Context, request: PaymentRequest) => {
        const validation = await ctx.run("validate", async () => {
            // External API call - cached during replay
            return await validatePayment(request);
        });
        
        const charge = await ctx.run("charge", async () => {
            // Idempotent charge with idempotency key
            return await chargePayment(request, ctx.idempotencyKey());
        });
        
        return charge;
    },
});
```

**PlexSpaces (Rust)**:
```rust
#[async_trait]
impl ActorBehavior for PaymentActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: PaymentRequest = serde_json::from_slice(msg.payload())?;
        
        // All operations are journaled via DurabilityFacet
        // During replay, side effects are loaded from cache
        
        // Step 1: Validate payment (cached during replay)
        let validation = ctx.durability_facet()
            .execute_with_cache("validate", || async {
                validate_payment(&request).await
            })
            .await?;
        
        // Step 2: Charge payment (idempotent)
        let charge = ctx.durability_facet()
            .execute_with_idempotency("charge", &request.idempotency_key, || async {
                charge_payment(&request).await
            })
            .await?;
        
        Ok(())
    }
}
```

### Idempotency

**Restate (TypeScript)**:
```typescript
const charge = await ctx.run("charge", async () => {
    return await fetch('https://api.payment.com/charge', {
        headers: {
            'Idempotency-Key': ctx.idempotencyKey(),
        },
        // ...
    });
});
```

**PlexSpaces (Rust)**:
```rust
let charge = ctx.durability_facet()
    .execute_with_idempotency(
        "charge",
        &request.idempotency_key,
        || async {
            charge_payment(&request).await
        },
    )
    .await?;
```

---

## Running the Comparison

### Prerequisites

```bash
# For Restate example (optional)
npm install @restate/sdk

# For PlexSpaces example
cargo build --release --features sqlite-backend
```

### Run PlexSpaces Example

```bash
# Run the example
cargo run --release --features sqlite-backend

# Run tests
cargo test --features sqlite-backend

# Run benchmarks
./scripts/benchmark.sh
```

---

## Performance Comparison

### Benchmarks

| Metric | Restate | PlexSpaces | Notes |
|--------|---------|------------|-------|
| **Operation Latency** | <5ms | <2ms | Local execution |
| **Replay Time** | <50ms | <25ms | With checkpoints |
| **Journal Overhead** | <10% | <5% | Per operation |
| **Checkpoint Time** | <100ms | <50ms | Snapshot creation |
| **Throughput** | 20K+ ops/s | 50K+ ops/s | Single node |

*Note: Benchmarks are approximate. See `metrics/benchmark_results.json` for detailed results.*

---

## Feature Comparison

| Feature | Restate | PlexSpaces | Notes |
|---------|---------|------------|-------|
| **Durable Execution** | ✅ Built-in | ✅ DurabilityFacet | Optional in PlexSpaces |
| **Deterministic Replay** | ✅ Automatic | ✅ Configurable | Same guarantees |
| **Side Effect Caching** | ✅ ctx.run() | ✅ ExecutionContext | Similar |
| **Checkpoints** | ✅ Automatic | ✅ Configurable | Similar |
| **Idempotency** | ✅ Built-in | ✅ Idempotency keys | Similar |
| **Time Travel** | ✅ Built-in | ✅ Journal replay | Same capability |
| **Exactly-Once** | ✅ Guaranteed | ✅ Guaranteed | Same semantics |

---

## When to Use Each

### Use Restate When:
- ✅ Building TypeScript/JavaScript applications
- ✅ Need automatic durability for all functions
- ✅ Want Restate Cloud (managed service)
- ✅ Prefer function-based model

### Use PlexSpaces When:
- ✅ Need Rust performance
- ✅ Want optional durability (pay for what you use)
- ✅ Need unified actor + workflow model
- ✅ Want proto-first contracts
- ✅ Need WASM support
- ✅ Want Firecracker isolation

---

## Design Decisions Explained

### Why DurabilityFacet?

**Restate Approach**: All service functions are automatically durable.

**PlexSpaces Approach**: Durability is a facet - actors can opt-in.

**Rationale**:
- **Performance**: Actors without durability have zero overhead
- **Flexibility**: Can mix durable and non-durable actors
- **Cost**: Journaling has storage costs - only use when needed
- **Composition**: Can add durability to any actor, not just services

### Why Journaling?

**Restate Approach**: All operations automatically journaled.

**PlexSpaces Approach**: Journaling via DurabilityFacet, configurable.

**Rationale**:
- **Event Sourcing**: Complete audit trail of all operations
- **Deterministic Replay**: Can replay from any point
- **Time-Travel Debugging**: Inspect state at any point in history
- **Exactly-Once**: Guaranteed exactly-once execution

### Why Checkpoints?

**Restate Approach**: Automatic periodic checkpoints.

**PlexSpaces Approach**: Configurable checkpoint interval.

**Rationale**:
- **Fast Recovery**: 90%+ faster than full replay
- **Storage Trade-off**: More checkpoints = faster recovery but more storage
- **Configurable**: Tune based on workload characteristics

---

## References

- [Restate Documentation](https://docs.restate.dev/)
- [PlexSpaces DurabilityFacet](../../../../crates/journaling)
- [PlexSpaces Journaling](../../../../crates/journaling)
- [Durable Execution Design](../../../../docs/DURABILITY_DESIGN.md)
