# Azure Durable Functions vs PlexSpaces Comparison

This comparison demonstrates how to implement serverless workflow orchestration in both Azure Durable Functions and PlexSpaces.

## Use Case: Order Processing Orchestration

A durable orchestration that:
- Coordinates multiple activities (validate, payment, shipping)
- Handles failures with retries
- Supports sub-orchestrations
- Demonstrates durable execution and activity patterns

## PlexSpaces Abstractions Showcased

This example demonstrates:
- ✅ **WorkflowBehavior** - Durable workflow orchestration
- ✅ **Activity Patterns** - External task execution
- ✅ **Retry Policies** - Automatic retry with exponential backoff
- ✅ **Sub-workflows** - Nested workflow execution
- ✅ **DurabilityFacet** - Journaling and deterministic replay

## Design Decisions

**Why WorkflowBehavior?**
- Matches Azure Durable Functions' orchestration model
- Separates orchestration logic from activity execution
- Enables deterministic replay for exactly-once semantics

**Why Activity Patterns?**
- Activities are external tasks that can fail
- Retry policies handle transient failures
- Idempotency keys prevent duplicate operations

---

## Azure Durable Functions Implementation

### Native TypeScript Code

See `native/order_orchestration.ts` for the complete Azure Durable Functions implementation.

Key features:
- **Durable Orchestration**: Workflow orchestration with automatic replay
- **Activities**: External tasks with retry policies
- **Sub-orchestrations**: Nested workflow execution
- **State Management**: Automatic state persistence and recovery

```typescript
// Usage:
const client = new DurableOrchestrationClient();
const instanceId = await client.startNew("OrderWorkflow", orderId);
const result = await client.waitForCompletionOrCreateCheckStatusResponse(instanceId);
```
}

// validate-order.ts (Activity)
export async function validateOrder(input: OrderRequest): Promise<ValidationResult> {
    // External API call
    const response = await fetch('https://api.example.com/validate', {
        method: 'POST',
        body: JSON.stringify(input),
    });
    return response.json();
}

// function.json
{
    "bindings": [
        {
            "name": "context",
            "type": "orchestrationTrigger",
            "direction": "in"
        }
    ]
}
```

---

## PlexSpaces Implementation

### Rust Implementation

See `src/main.rs` for the complete implementation.

### Key Differences

| Feature | Azure Durable Functions | PlexSpaces |
|---------|------------------------|------------|
| **Language** | TypeScript/C# | Rust |
| **Orchestration** | Built-in | WorkflowBehavior |
| **Activities** | callActivity() | Activity patterns |
| **Retry** | Built-in policies | Configurable retry |
| **Sub-orchestrations** | callSubOrchestrator() | Nested workflows |
| **Durability** | Automatic | DurabilityFacet (optional) |

---

## Running the Comparison

### Prerequisites

```bash
# For Azure Durable Functions example (optional)
npm install @azure/functions

# For PlexSpaces example
cargo build --release --features sqlite-backend
```

### Run PlexSpaces Example

```bash
# Run the example
cargo run --release --features sqlite-backend

# Run tests
cargo test --features sqlite-backend

# Run test script
./scripts/test.sh
```

---

## Performance Comparison

### Benchmarks

| Metric | Azure Durable Functions | PlexSpaces | Notes |
|--------|------------------------|------------|-------|
| **Orchestration Start** | <50ms | <20ms | Cold start |
| **Activity Execution** | <100ms | <30ms | Local activities |
| **Replay Time** | <200ms | <100ms | With checkpoints |
| **Throughput** | 5K+ orch/s | 20K+ orch/s | Single node |

*Note: Benchmarks are approximate. See `metrics/benchmark_results.json` for detailed results.*

---

## Feature Comparison

| Feature | Azure Durable Functions | PlexSpaces | Notes |
|---------|------------------------|------------|-------|
| **Durable Execution** | ✅ Built-in | ✅ WorkflowBehavior | Similar |
| **Activity Patterns** | ✅ callActivity() | ✅ Activity patterns | Similar |
| **Retry Policies** | ✅ Built-in | ✅ Configurable | Similar |
| **Sub-orchestrations** | ✅ Built-in | ✅ Nested workflows | Similar |
| **Human Tasks** | ✅ Built-in | ⚠️ Planned | Not yet implemented |
| **Event Sourcing** | ✅ Built-in | ✅ Journaling | Similar |

---

## References

- [Azure Durable Functions Documentation](https://learn.microsoft.com/en-us/azure/azure-functions/durable/durable-functions-overview)
- [PlexSpaces WorkflowBehavior](../../../../crates/behavior)
- [PlexSpaces DurabilityFacet](../../../../crates/journaling)
