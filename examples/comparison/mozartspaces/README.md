# MozartSpaces vs PlexSpaces Comparison

This comparison demonstrates how to implement MozartSpaces-style XVSM (eXtended Virtual Shared Memory) with coordinator objects in both MozartSpaces and PlexSpaces.

## Use Case: XVSM with Coordinator Objects (FIFO, LIFO, Label-based Retrieval)

An extended TupleSpace system that:
- Uses coordinator objects to define tuple storage/retrieval patterns
- Supports FIFO (queue), LIFO (stack), Label-based, and Priority patterns
- Demonstrates extended Linda coordination model

## PlexSpaces Abstractions Showcased

- ✅ **TupleSpace Coordination** - Extended Linda model
- ✅ **Coordinator Objects** - FIFO, LIFO, Label-based, Priority patterns
- ✅ **GenServerBehavior** - Request-reply pattern for coordinator operations
- ✅ **Pattern Matching** - Flexible tuple retrieval

## Design Decisions

**Why Coordinator Objects?**
- MozartSpaces extends Linda with coordinators
- Defines how tuples are stored and fetched
- Enables queue, stack, and label-based patterns

**Why XVSM?**
- Extends basic TupleSpace with retrieval patterns
- More flexible than basic Linda model
- Supports complex coordination scenarios

---

## MozartSpaces Implementation

### Native Java Code

See `native/xvsm_coordinator.java` for the complete MozartSpaces implementation.

Key features:
- **XVSM**: eXtended Virtual Shared Memory
- **Coordinator Objects**: Define tuple storage/retrieval patterns
- **FIFO/LIFO**: Queue and stack patterns
- **Label-Based**: Label-based tuple retrieval

```java
// Usage:
XVSMCoordinator coordinator = new XVSMCoordinator();
coordinator.writeFIFO(new Tuple("item", "data"));
Tuple result = coordinator.takeFIFO();
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// XVSM coordinator actor
use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl, Actor};
use plexspaces_mailbox::{mailbox_config_default, Mailbox};
use std::sync::Arc;

let behavior = Box::new(XVSMCoordinatorActor::new());
let actor_id = "coordinator@node1".to_string();
let mut mailbox_config = mailbox_config_default();
mailbox_config.storage_strategy = plexspaces_mailbox::StorageStrategy::Memory as i32;
let mailbox = Mailbox::new(mailbox_config, format!("{}:mailbox", actor_id)).await?;
let actor = Actor::new(actor_id.clone(), behavior, mailbox, "default".to_string(), None);

let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
    .ok_or_else(|| "ActorFactory not found")?;
let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await?;
let coordinator = plexspaces_core::ActorRef::new(actor_id)?;

// Write with FIFO coordinator
coordinator.ask(Write {
    tuple: labeled_tuple,
    coordinator: CoordinatorType::FIFO,
}).await?;

// Take with FIFO coordinator (gets first item)
coordinator.ask(Take {
    pattern: "".to_string(),
    coordinator: CoordinatorType::FIFO,
}).await?;
```

---

## Side-by-Side Comparison

| Feature | MozartSpaces | PlexSpaces |
|---------|--------------|------------|
| **XVSM** | Built-in | Coordinator actors |
| **FIFO** | FIFOCoordinator | CoordinatorType::FIFO |
| **LIFO** | LIFOCoordinator | CoordinatorType::LIFO |
| **Label-based** | LabelCoordinator | CoordinatorType::Label |
| **Pattern Matching** | Built-in | Pattern matching |

---

## Running the Comparison

```bash
cd examples/comparison/mozartspaces
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [MozartSpaces XVSM](https://link.springer.com/chapter/10.1007/978-3-319-39519-7_4)
- [PlexSpaces TupleSpace](../../../../crates/tuplespace/src/mod.rs)
