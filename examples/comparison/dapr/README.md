# Dapr vs PlexSpaces Comparison

This comparison demonstrates how to implement Dapr-style unified durable workflows (combining actors and workflows) in both Dapr and PlexSpaces.

## Use Case: Order Processing Workflow with Unified Actor + Workflow Model

A unified workflow system that:
- Combines actor model with durable workflows
- Processes orders through multiple steps (payment, notification, completion)
- Demonstrates unified durable execution
- Showcases actor + workflow integration

## PlexSpaces Abstractions Showcased

- ✅ **WorkflowBehavior** - Durable workflow orchestration
- ✅ **DurabilityFacet** - State persistence and replay
- ✅ **Unified Model** - Actor + Workflow in single abstraction
- ✅ **GenServerBehavior** - Request-reply pattern for workflow steps

## Design Decisions

**Why Unified Actor + Workflow Model?**
- Dapr combines actors with durable workflows
- Single abstraction for both stateful actors and workflows
- Simplifies development by unifying patterns

**Why WorkflowBehavior?**
- Durable execution with automatic replay
- State machine orchestration
- Fault tolerance through journaling

---

## Dapr Implementation

### Native TypeScript Code

See `native/order_workflow.ts` for the complete Dapr implementation.

Key features:
- **Unified Model**: Combines actors with durable workflows
- **Actor Integration**: Workflows can call actors for stateful operations
- **Durable Execution**: Automatic state persistence and replay
- **Multi-Step Orchestration**: Sequential workflow execution

```typescript
// Usage:
const result = await orderWorkflow({
    id: "order-123",
    amount: 99.99,
    items: [...]
});
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// Unified actor + workflow model
let mut actor = ActorBuilder::new(behavior)
    .with_id(actor_id.clone())
    .build()
    .await;

// Attach DurabilityFacet (unified durable execution)
let durability_facet = Box::new(DurabilityFacet::new(storage, config));
actor.attach_facet(durability_facet, 50, serde_json::json!({})).await?;

// WorkflowBehavior handles durable execution
// Actor model provides stateful computation
```

---

## Side-by-Side Comparison

| Feature | Dapr | PlexSpaces |
|---------|------|------------|
| **Unified Model** | Actors + Workflows | WorkflowBehavior + Actor |
| **Durability** | Built-in | DurabilityFacet |
| **State Persistence** | Automatic | Journaling via DurabilityFacet |
| **Workflow Orchestration** | Durable Functions | WorkflowBehavior |

---

## Running the Comparison

```bash
cd examples/comparison/dapr
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Dapr Documentation](https://docs.dapr.io/)
- [PlexSpaces WorkflowBehavior](../../../../crates/behavior/src/workflow.rs)
