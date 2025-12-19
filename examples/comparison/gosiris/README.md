# Gosiris vs PlexSpaces Comparison

This comparison demonstrates how to implement Gosiris-style actors (Go actor framework) in both Gosiris and PlexSpaces.

## Use Case: Counter Actor with Message Passing

A simple counter actor that:
- Receives messages (Increment, Decrement, Get)
- Maintains state (count)
- Demonstrates actor model and message passing

## PlexSpaces Abstractions Showcased

- ✅ **GenServerBehavior** - Request-reply pattern
- ✅ **Actor Model** - Message passing with state isolation
- ✅ **ActorRef** - Location-transparent messaging

## Design Decisions

**Why GenServerBehavior?**
- Gosiris uses request-reply pattern
- Matches Go's message passing style
- Simple and straightforward

**Why Actor Model?**
- Gosiris is built on actor model
- Message passing for communication
- State isolation by design

---

## Gosiris Implementation

### Native Go Code

See `native/counter.go` for the complete Gosiris implementation.

Key features:
- **Actor Model**: Message passing with state isolation
- **Type Safety**: Go's type system for message handling
- **Request-Reply**: Synchronous message passing pattern
- **Actor Spawning**: Dynamic actor creation

```go
// Usage:
system := gosiris.NewActorSystem()
counter := system.ActorOf("counter", &CounterActor{count: 0})
system.Tell(counter, Increment{Sender: system.Self()})
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// Counter actor with message passing
let behavior = Box::new(CounterActor::new());
let actor = ActorBuilder::new(behavior)
    .with_id(actor_id.clone())
    .build()
    .await;

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
    "GenServer", // actor_type
    vec![], // initial_state
    None, // config
    std::collections::HashMap::new(), // labels
).await?;
let actor_ref = plexspaces_core::ActorRef::new(actor_id)?;

// Send message (Gosiris: actor.Send(ctx, Increment{}))
let result = counter.ask(Increment).await?;
```

---

## Side-by-Side Comparison

| Feature | Gosiris | PlexSpaces |
|---------|---------|------------|
| **Actor Model** | Built-in | GenServerBehavior |
| **Message Passing** | Send/Reply | ask/tell |
| **State Isolation** | Actor boundaries | Actor boundaries |
| **Language** | Go | Rust |

---

## Running the Comparison

```bash
cd examples/comparison/gosiris
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Gosiris Documentation](https://github.com/teivah/gosiris)
- [PlexSpaces GenServerBehavior](../../../../crates/behavior/src/genserver.rs)
