# AWS Step Functions vs PlexSpaces Comparison

This comparison demonstrates how to implement state machine workflow orchestration in both AWS Step Functions and PlexSpaces.

## Use Case: Order Processing State Machine

A state machine workflow that:
- Defines explicit states (ValidateOrder, ProcessPayment, ShipOrder, OrderCompleted)
- Handles state transitions with error handling
- Supports retry policies and error recovery
- Demonstrates durable execution and state tracking

## PlexSpaces Abstractions Showcased

This example demonstrates:
- ✅ **WorkflowBehavior** - Durable workflow orchestration with state tracking
- ✅ **State Machine Pattern** - Explicit state transitions (AWS Step Functions pattern)
- ✅ **DurabilityFacet** - Journaling and deterministic replay
- ✅ **Query Support** - Query current state and status

## Design Decisions

**Why WorkflowBehavior?**
- Matches AWS Step Functions' workflow orchestration model
- Supports explicit state tracking and transitions
- Enables deterministic replay for exactly-once semantics

**Why State Machine Pattern?**
- AWS Step Functions uses explicit state definitions
- State transitions are clear and traceable
- Error handling per state (Catch blocks in AWS)

---

## AWS Step Functions Implementation

### Native JSON State Machine Definition

See `native/order-state-machine.json` for the complete AWS Step Functions state machine definition.

**Key Features**:
- **Task States**: ValidateOrder, ProcessPayment, ShipOrder
- **Retry Policies**: Automatic retry with exponential backoff
- **Error Handling**: Catch blocks for each state
- **Success/Fail States**: OrderCompleted (Succeed), OrderFailed (Fail)

### State Machine Flow

```
StartAt: ValidateOrder
  ↓
ValidateOrder (Task)
  ├─ Success → ProcessPayment
  └─ Error → OrderFailed
  ↓
ProcessPayment (Task)
  ├─ Success → ShipOrder
  └─ Error → OrderFailed
  ↓
ShipOrder (Task)
  ├─ Success → OrderCompleted
  └─ Error → OrderFailed
  ↓
OrderCompleted (Succeed)
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// Order state machine workflow actor
pub struct OrderStateMachine {
    state: OrderWorkflowState,
}

#[async_trait]
impl WorkflowBehavior for OrderStateMachine {
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        // State 1: ValidateOrder
        self.state.current_state = OrderState::ValidateOrder;
        let validation = self.validate_order(&order).await?;
        if !validation.valid {
            self.state.current_state = OrderState::OrderFailed(...);
            return Err(...);
        }
        
        // State 2: ProcessPayment
        self.state.current_state = OrderState::ProcessPayment;
        let payment = self.process_payment(&order).await?;
        
        // State 3: ShipOrder
        self.state.current_state = OrderState::ShipOrder;
        let shipment = self.ship_order(&order).await?;
        
        // State 4: OrderCompleted
        self.state.current_state = OrderState::OrderCompleted;
        Ok(Message::new(result.into_bytes()))
    }
}
```

---

## Side-by-Side Comparison

| Feature | AWS Step Functions | PlexSpaces |
|---------|-------------------|------------|
| **State Definition** | JSON state machine definition | Rust enum (`OrderState`) |
| **State Transitions** | Implicit via `Next` field | Explicit state assignment |
| **Error Handling** | `Catch` blocks per state | `Result` with error propagation |
| **Retry Policies** | `Retry` configuration in JSON | Automatic retry via `WorkflowBehavior` |
| **Durability** | Built-in (all executions logged) | `DurabilityFacet` (optional) |
| **Query Support** | `DescribeExecution` API | `query()` method |
| **State Tracking** | Execution history in AWS console | Explicit `current_state` field |

---

## Key Differences

### 1. State Definition

**AWS Step Functions**:
```json
{
  "States": {
    "ValidateOrder": {
      "Type": "Task",
      "Resource": "arn:aws:lambda:...",
      "Next": "ProcessPayment"
    }
  }
}
```

**PlexSpaces**:
```rust
enum OrderState {
    ValidateOrder,
    ProcessPayment,
    ShipOrder,
    OrderCompleted,
    OrderFailed(String),
}
```

### 2. Error Handling

**AWS Step Functions**:
```json
{
  "Catch": [{
    "ErrorEquals": ["ValidationError"],
    "Next": "OrderFailed"
  }]
}
```

**PlexSpaces**:
```rust
if !validation.valid {
    self.state.current_state = OrderState::OrderFailed(...);
    return Err(BehaviorError::ProcessingError(...));
}
```

### 3. Query Support

**AWS Step Functions**:
```typescript
const execution = await stepFunctions.describeExecution({
    executionArn: executionArn
}).promise();
```

**PlexSpaces**:
```rust
let query_msg = Message::new(b"status".to_vec())
    .with_message_type("workflow_query:status".to_string());
let status = state_machine.ask(query_msg, timeout).await?;
```

---

## Running the Comparison

### Prerequisites

- Rust 1.70+
- Cargo

### Build and Run

```bash
cd examples/comparison/aws_step_functions
cargo build --release
cargo run --release
```

### Run Tests

```bash
./scripts/test.sh
```

---

## Performance Comparison

| Metric | AWS Step Functions | PlexSpaces |
|--------|-------------------|------------|
| **State Transition Latency** | ~100-200ms (Lambda cold start) | ~1-5ms (in-process) |
| **Overhead** | JSON parsing, Lambda invocation | Minimal (direct function calls) |
| **Durability Cost** | Per-execution pricing | Optional (only if DurabilityFacet attached) |

---

## When to Use Each

### Use AWS Step Functions When:
- You need AWS-native integration (Lambda, SQS, etc.)
- You prefer JSON-based state machine definitions
- You want managed infrastructure (no server management)
- You need AWS console visibility

### Use PlexSpaces When:
- You want type-safe state machines (Rust enums)
- You need low-latency state transitions (in-process)
- You want optional durability (pay for what you use)
- You prefer code-based definitions over JSON

---

## References

- [AWS Step Functions Documentation](https://docs.aws.amazon.com/step-functions/)
- [AWS Step Functions State Machine Language](https://docs.aws.amazon.com/step-functions/latest/dg/concepts-amazon-states-language.html)
- [PlexSpaces WorkflowBehavior](../../../../crates/behavior/src/workflow.rs)
- [PlexSpaces DurabilityFacet](../../../../crates/journaling/src/durability_facet.rs)
