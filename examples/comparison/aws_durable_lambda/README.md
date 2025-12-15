# AWS Durable Lambda vs PlexSpaces Comparison

This comparison demonstrates how to implement durable execution with idempotency in both AWS Durable Lambda and PlexSpaces.

## Use Case: Payment Processing with Idempotency

A durable function that:
- Processes payments with idempotency keys
- Persists state across invocations
- Handles duplicate requests gracefully
- Demonstrates durable execution patterns

## PlexSpaces Abstractions Showcased

This example demonstrates:
- ✅ **DurabilityFacet** - Journaling and deterministic replay
- ✅ **Idempotency** - Duplicate request handling
- ✅ **State Persistence** - Actor state survives restarts
- ✅ **GenServerBehavior** - Request-reply pattern

## Design Decisions

**Why DurabilityFacet?**
- Matches AWS Durable Lambda's durable execution model
- Automatically journals all operations
- Enables deterministic replay for exactly-once semantics

**Why Idempotency?**
- Prevents duplicate payments
- Handles retries gracefully
- Essential for payment processing

---

## AWS Durable Lambda Implementation

### Native Python Code

See `native/order_handler.py` for the complete AWS Durable Lambda implementation.

Key features:
- **Durable Execution**: State persists across invocations
- **Idempotency**: Automatic handling of duplicate requests
- **Idempotency Keys**: Prevent duplicate operations
- **State Persistence**: Actor state survives restarts

```python
# Usage:
result = lambda_handler({
    'body': json.dumps({
        'order_id': 'order-123',
        'amount': 99.99
    })
}, context)
```

---

## PlexSpaces Implementation

### Rust Code

```rust
impl GenServerBehavior for PaymentProcessor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        let request: PaymentRequest = serde_json::from_slice(msg.payload())?;
        
        // Check idempotency
        if self.state.processed_orders.contains(&request.idempotency_key) {
            return Ok(cached_result);
        }
        
        // Process payment (journaled automatically via DurabilityFacet)
        let result = self.process_payment(&request).await?;
        
        // Update state (persisted automatically)
        self.state.processed_orders.push(request.idempotency_key);
        
        Ok(Message::new(serde_json::to_vec(&result)?))
    }
}
```

---

## Side-by-Side Comparison

| Feature | AWS Durable Lambda | PlexSpaces |
|---------|-------------------|------------|
| **Durability** | Built-in (all executions logged) | `DurabilityFacet` (optional) |
| **Idempotency** | Automatic via execution context | Manual check + state tracking |
| **State Persistence** | Automatic (all state persisted) | Via `DurabilityFacet` |
| **Replay** | Automatic on restart | Deterministic replay via journal |
| **Caching** | Built-in result caching | Manual state tracking |

---

## Running the Comparison

```bash
cd examples/comparison/aws_durable_lambda
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [AWS Durable Lambda Documentation](https://docs.aws.com/lambda/latest/dg/durable-functions.html)
- [PlexSpaces DurabilityFacet](../../../../crates/journaling/src/durability_facet.rs)
