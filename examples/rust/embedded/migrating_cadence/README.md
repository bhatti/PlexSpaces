# Cadence vs PlexSpaces Comparison

This comparison demonstrates how to implement Cadence-style workflow orchestration (Temporal's predecessor) in both Cadence and PlexSpaces.

## Use Case: Workflow Orchestration with Activities

A workflow system that:
- Orchestrates workflows with activity execution
- Provides durable execution (Temporal's predecessor)
- Demonstrates workflow + activity patterns

## PlexSpaces Abstractions Showcased

- ✅ **WorkflowBehavior** - Workflow orchestration
- ✅ **DurabilityFacet** - Durable execution
- ✅ **Activity Patterns** - ExecuteActivity for workflow steps

## Design Decisions

**Why WorkflowBehavior?**
- Cadence orchestrates workflows with activities
- WorkflowBehavior provides durable execution
- Matches Cadence's workflow model

**Why DurabilityFacet?**
- Cadence workflows are durable
- State persistence for long-running workflows
- Journaling enables replay on failure

---

## Cadence Implementation

### Native Go Code

See `native/order_workflow.go` for the complete Cadence implementation.

Key features:
- **Workflow Orchestration**: Durable workflows with activity execution
- **Activity Options**: Configurable timeouts and retry policies
- **Durable Execution**: State persistence for long-running workflows
- **Activity Patterns**: ExecuteActivity for workflow steps

```go
// Usage:
workflowClient.ExecuteWorkflow(ctx, workflowOptions, OrderWorkflow, orderID, amount)
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// Workflow with activities (Cadence pattern)
let behavior = Box::new(OrderWorkflowActor::new());
let mut actor = ActorBuilder::new(behavior)
    .with_id(actor_id.clone())
    .build()
    .await;

// Attach DurabilityFacet
let durability_facet = Box::new(DurabilityFacet::new(storage, config));
actor.attach_facet(durability_facet, 50, serde_json::json!({})).await?;

// Execute workflow (Cadence: workflow.ExecuteWorkflow)
workflow.ask(Start { order_id, amount }).await?;
```

---

## Side-by-Side Comparison

| Feature | Cadence | PlexSpaces |
|---------|---------|------------|
| **Workflow** | Built-in | WorkflowBehavior |
| **Activities** | ExecuteActivity | Activity execution in workflow |
| **Durability** | Built-in | DurabilityFacet |
| **Protocol** | Thrift/TChannel | gRPC (Temporal uses gRPC) |

---

## Running the Comparison

```bash
cd examples/comparison/cadence
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Cadence Documentation](https://github.com/uber/cadence)
- [Temporal vs Cadence](https://temporal.io/temporal-versus/cadence)
- [PlexSpaces WorkflowBehavior](../../../../crates/behavior/src/workflow.rs)
