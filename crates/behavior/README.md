# Behavior - OTP-Style Behaviors for PlexSpaces Actors

**Purpose**: Provides Erlang/OTP-inspired behaviors for actors, enabling common patterns like request/reply, event handling, finite state machines, and durable workflows.

## Overview

This crate implements composable behaviors that actors can use to handle different types of messages and state transitions. Behaviors are traits that actors implement, following the Erlang/OTP philosophy of separating concerns.

## Key Behaviors

### GenServer

Request/reply patterns (synchronous communication):

```rust
use plexspaces_behavior::GenServerBehavior;
use plexspaces_core::{ActorBehavior, ActorContext};
use plexspaces_mailbox::Message;

struct CounterGenServer {
    count: i32,
}

#[async_trait]
impl ActorBehavior for CounterGenServer {
    async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }
}

#[async_trait]
impl GenServerBehavior for CounterGenServer {
    async fn handle_request(&mut self, msg: Message, ctx: &ActorContext) -> Result<Message, BehaviorError> {
        // Synchronous - MUST return a reply
        // Like: gen_server:call, Orleans Task<T>, Cloudflare fetch()
        match msg.payload() {
            b"increment" => {
                self.count += 1;
                Ok(Message::new(self.count.to_string().into_bytes()))
            }
            b"get" => Ok(Message::new(self.count.to_string().into_bytes())),
            _ => Err(BehaviorError::UnknownMessage),
        }
    }
}
```

**Design Philosophy**:
- GenServer handles ONLY synchronous requests (request-reply pattern)
- For fire-and-forget events, use GenEvent instead
- Single handler: `handle_request` (expects response)
- Matches Erlang/OTP: `gen_server:call` vs `gen_event:notify`

### GenEvent

Event handling (fire-and-forget):

```rust
use plexspaces_behavior::GenEventBehavior;

#[async_trait]
impl GenEventBehavior for MyEventActor {
    async fn handle_event(&mut self, msg: Message, ctx: &ActorContext) -> Result<(), BehaviorError> {
        // Fire-and-forget - no return value
        // Like: gen_event:notify, pub/sub events
        self.process_event(msg).await
    }
}
```

**Design Philosophy**:
- GenEvent handles fire-and-forget events
- No return value (asynchronous)
- Multiple handlers can subscribe to events

### GenStateMachine (GenFSM)

Finite state machines:

```rust
use plexspaces_behavior::GenStateMachineBehavior;

enum MyState {
    Idle,
    Processing,
    Done,
}

#[async_trait]
impl GenStateMachineBehavior for MyFSM {
    async fn handle_state_transition(
        &mut self,
        current_state: &str,
        event: Message,
        ctx: &ActorContext,
    ) -> Result<String, BehaviorError> {
        match (current_state, event.payload()) {
            ("Idle", b"start") => Ok("Processing".to_string()),
            ("Processing", b"complete") => Ok("Done".to_string()),
            _ => Err(BehaviorError::InvalidStateTransition),
        }
    }
}
```

**Design Philosophy**:
- Explicit state transitions
- State machine pattern for complex workflows
- Matches Erlang/OTP: `gen_fsm` behavior

### Workflow

Restate-inspired durable workflows:

```rust
use plexspaces_behavior::WorkflowBehavior;

#[async_trait]
impl WorkflowBehavior for MyWorkflow {
    async fn run(
        &mut self,
        ctx: &plexspaces_core::ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        // Main workflow execution (exclusive, one at a time)
        // Like: Restate workflow.run(), Temporal workflow execution
        Ok(Message::new(b"workflow complete".to_vec()))
    }

    async fn signal(
        &mut self,
        ctx: &plexspaces_core::ActorContext,
        name: String,
        data: Message,
    ) -> Result<(), BehaviorError> {
        // External events that modify workflow state
        // Like: Restate workflow.signal(), Temporal signal handlers
        Ok(())
    }

    async fn query(
        &mut self,
        ctx: &plexspaces_core::ActorContext,
        name: String,
        args: Message,
    ) -> Result<Message, BehaviorError> {
        // Read-only queries (does not modify state)
        // Like: Restate workflow.query(), Temporal query handlers
        Ok(Message::new(b"query result".to_vec()))
    }
}
```

**Design Philosophy**:
- **run()**: Main workflow execution (exclusive, one at a time)
- **signal()**: External events that modify workflow state (fire-and-forget)
- **query()**: Read-only queries (concurrent, does not modify state)
- Matches Restate and Temporal patterns

## Simplified Behavior (Experimental)

A unified behavior trait that replaces all different handler traits with a pure functional approach:

```rust
use plexspaces_behavior::simplified::{Behavior, Input, Output};

struct MyBehavior;

impl Behavior for MyBehavior {
    async fn handle(&self, input: Input) -> Result<Output, BehaviorError> {
        // Pure functional approach
        // Input contains: message, state, context
        // Output contains: response, effects, state_updates
        Ok(Output {
            response: Some(b"result".to_vec()),
            effects: vec![],
            state_updates: vec![],
        })
    }
}
```

## Design Principles

### Separation of Concerns

- **GenServer**: Synchronous requests (request-reply)
- **GenEvent**: Asynchronous events (fire-and-forget)
- **GenStateMachine**: State transitions (FSM pattern)
- **Workflow**: Durable workflows (Restate/Temporal pattern)

### Location-Transparent Reply Routing

All behaviors use `ActorService.send()` for reply routing, which automatically handles local/remote detection:
- **Local actors**: ActorService routes to local mailbox (fast, microseconds)
- **Remote actors**: ActorService routes via gRPC (network, milliseconds)
- **Unified API**: Same `ActorService.send()` call works for both

**Example**:
```rust
// GenServer automatically routes replies using ActorService
// ActorService checks node_id and routes appropriately
ctx.actor_service
    .send(sender_id, reply_msg)
    .await?;
```

### Proto-First Design

Behavior traits implement contracts defined in proto files:
- `proto/plexspaces/v1/behaviors.proto` defines GenServer, GenEvent, GenFSM
- `proto/plexspaces/v1/workflow.proto` defines Workflow behavior
- Traits map to proto service definitions

### Inspiration from Multiple Frameworks

- **Erlang/OTP**: `gen_server:call`, `gen_event:notify`, `gen_fsm`
- **Orleans**: Grain methods returning `Task<T>` (request)
- **Restate**: Exclusive handlers with return value, workflow patterns
- **Temporal**: Workflow execution, signals, queries
- **Cloudflare Durable Objects**: `fetch()` HTTP request handler

## Usage Examples

### Combining Behaviors

Actors can implement multiple behaviors:

```rust
struct MyActor {
    state: MyState,
}

#[async_trait]
impl ActorBehavior for MyActor {
    async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        // Route to appropriate behavior
        if msg.is_request() {
            self.handle_request(msg, ctx).await?;
        } else {
            self.handle_event(msg, ctx).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl GenServerBehavior for MyActor {
    async fn handle_request(&mut self, msg: Message, ctx: &ActorContext) -> Result<Message, BehaviorError> {
        // Handle synchronous requests
        Ok(Message::new(b"response".to_vec()))
    }
}

#[async_trait]
impl GenEventBehavior for MyActor {
    async fn handle_event(&mut self, msg: Message, ctx: &ActorContext) -> Result<(), BehaviorError> {
        // Handle asynchronous events
        Ok(())
    }
}
```

## Testing

```bash
# Run all behavior tests
cargo test -p plexspaces-behavior

# Run specific behavior tests
cargo test -p plexspaces-behavior genserver
cargo test -p plexspaces-behavior genevent
cargo test -p plexspaces-behavior genfsm
cargo test -p plexspaces-behavior workflow

# Check coverage
cargo tarpaulin -p plexspaces-behavior --out Html
```

## Dependencies

This crate depends on:
- `plexspaces_core`: Common types (ActorBehavior, ActorContext, BehaviorError)
- `plexspaces_mailbox`: Message types
- `async-trait`: For async trait methods

## Dependents

This crate is used by:
- `plexspaces_actor`: Actors implement behavior traits
- `plexspaces_workflow`: Workflow execution engine
- `plexspaces_node`: Node hosts actors with behaviors

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design - Behaviors](../../docs/detailed-design.md#behaviors) - Comprehensive behavior documentation
- [Getting Started Guide](../../docs/getting-started.md) - Quick start with behaviors
- Implementation: `crates/behavior/src/`
- Tests: `crates/behavior/src/` (genserver_tests, genevent_tests, genfsm_tests)
- Proto definitions: `proto/plexspaces/v1/behaviors.proto`, `proto/plexspaces/v1/workflow.proto`

