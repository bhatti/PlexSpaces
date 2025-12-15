# Netflix Conductor/Orkes vs PlexSpaces Comparison

This comparison demonstrates how to implement Netflix Conductor/Orkes-style JSON-based workflow DSL for e-commerce order processing in both Conductor and PlexSpaces.

## Use Case: E-commerce Order Processing Workflow

A microservices orchestration workflow that:
- Processes payments via HTTP microservice
- Updates inventory via HTTP microservice
- Checks discount eligibility (decision task)
- Creates shipments via HTTP microservice
- Publishes events to event bus
- Demonstrates JSON-based workflow DSL for microservices orchestration

**Workflow Steps**: Payment → Inventory → Discount Check → Shipping → Notification

---

## Conductor Implementation

### Native JSON Workflow Definition

See `native/order_workflow.json` for the complete Conductor workflow definition.

Key features:
- **JSON-based DSL**: Declarative workflow definition
- **HTTP Tasks**: Call external microservices
- **EVENT Tasks**: Publish events to event bus
- **DECISION Tasks**: Conditional workflow logic
- **Task Orchestration**: Sequential task execution

The workflow defines:
1. ProcessPayment (HTTP task)
2. UpdateInventory (HTTP task)
3. CheckDiscountEligibility (DECISION task)
4. CreateShipment (HTTP task)
5. PublishOrderProcessed (EVENT task)

---

## PlexSpaces Implementation

### Rust Code

```rust
// E-commerce order processing workflow (Conductor pattern)
let order = OrderRequest {
    order_id: "order-12345".to_string(),
    customer_id: "customer-789".to_string(),
    items: vec![...],
    total_amount: 109.97,
    shipping_address: "123 Main St".to_string(),
};

let tasks = vec![
    WorkflowTask {
        task_id: "task-1".to_string(),
        task_type: "HTTP".to_string(),
        name: "ProcessPayment".to_string(),
        input_parameters: json!({
            "url": "https://api.payment.com/charge",
            "method": "POST",
            "amount": order.total_amount
        }),
        status: "SCHEDULED".to_string(),
    },
    WorkflowTask {
        task_id: "task-2".to_string(),
        task_type: "HTTP".to_string(),
        name: "UpdateInventory".to_string(),
        // ...
    },
    // ... more tasks
];

let workflow_def = WorkflowDefinition {
    workflow_id: "order-processing-workflow".to_string(),
    name: "E-commerce Order Processing".to_string(),
    version: 1,
    tasks,
    status: "RUNNING".to_string(),
};

workflow.ask(StartWorkflow { definition: workflow_def, order }).await?;
```

---

## Side-by-Side Comparison

| Feature | Conductor | PlexSpaces |
|---------|-----------|------------|
| **Workflow DSL** | JSON-based | WorkflowDefinition struct |
| **Task Types** | HTTP, EVENT, DECISION, SUB_WORKFLOW | Task type enum |
| **Microservices** | HTTP tasks | HTTP microservice calls |
| **Event Bus** | EVENT tasks | Event publishing |
| **Conditional Logic** | DECISION tasks | Decision logic |
| **Durability** | Built-in | DurabilityFacet |

---

## Key Features

### JSON-based Workflow DSL
- **Declarative**: Workflows defined in JSON
- **Versioned**: Workflow definitions can be versioned
- **Flexible**: Supports multiple task types (HTTP, EVENT, DECISION, SUB_WORKFLOW)

### Microservices Orchestration
- **HTTP Tasks**: Call external microservices
- **Event Tasks**: Publish events to event bus
- **Decision Tasks**: Conditional workflow logic
- **Sub-workflows**: Nested workflow execution

### Task Execution
- **Worker Tasks**: Executed by external worker applications
- **System Tasks**: Managed internally by Conductor
- **Task Status**: SCHEDULED → IN_PROGRESS → COMPLETED/FAILED

---

## Running the Comparison

```bash
cd examples/comparison/conductor
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Netflix Conductor](https://github.com/Netflix/conductor)
- [Orkes Platform](https://orkes.io/)
- [PlexSpaces WorkflowBehavior](../../../../crates/behavior/src/workflow.rs)
