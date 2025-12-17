# LuaTS vs PlexSpaces Comparison

This comparison demonstrates how to implement LuaTS-style Linda + event-driven programming in both LuaTS and PlexSpaces.

## Use Case: Event-Driven Coordination with TupleSpace (Linda + Events)

A coordination system that:
- Combines Linda TupleSpace with event-driven programming
- Simplifies multi-thread development
- Demonstrates reactive coordination patterns

## PlexSpaces Abstractions Showcased

- ✅ **GenEventBehavior** - Event-driven coordination
- ✅ **TupleSpace** - Linda-style coordination
- ✅ **Combined Model** - Events + TupleSpace for reactive patterns

## Design Decisions

**Why Combine Linda + Events?**
- LuaTS simplifies multi-thread development
- Events provide reactive patterns
- TupleSpace provides decoupled coordination
- Best of both worlds

**Why GenEventBehavior?**
- Event-driven programming pattern
- Subscribe/publish model
- Reactive coordination

---

## LuaTS Implementation

### Native Lua Code

See `native/event_coordination.lua` for the complete LuaTS implementation.

Key features:
- **Linda + Events**: Combines TupleSpace with event-driven programming
- **Subscribe/Publish**: Event-driven coordination patterns
- **TupleSpace**: Linda-style coordination primitives
- **Multi-Thread Simplification**: Simplifies multi-thread development

```lua
-- Usage:
luats.subscribe("data_ready", function(event) ... end)
luats.publish("data_ready", { payload = "data" })
local tuple = luats.read({"event", "data_ready", _})
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// Event coordinator (LuaTS-style: Linda + Events)
use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl, Actor};
use plexspaces_mailbox::{mailbox_config_default, Mailbox};
use std::sync::Arc;

let behavior = Box::new(EventCoordinatorActor::new());
let actor_id = "coordinator@node1".to_string();
let mut mailbox_config = mailbox_config_default();
mailbox_config.storage_strategy = plexspaces_mailbox::StorageStrategy::Memory as i32;
let mailbox = Mailbox::new(mailbox_config, format!("{}:mailbox", actor_id)).await?;
let actor = Actor::new(actor_id.clone(), behavior, mailbox, "default".to_string(), None);

let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
    .ok_or_else(|| "ActorFactory not found")?;
let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await?;
let coordinator = plexspaces_core::ActorRef::new(actor_id)?;

// Subscribe to events (GenEventBehavior)
coordinator.ask(Subscribe { event_type: "data_ready" }).await?;

// Publish event (also writes to TupleSpace)
coordinator.ask(Publish { event }).await?;

// Read from TupleSpace (Linda pattern)
coordinator.ask(ReadTuple { pattern }).await?;
```

---

## Side-by-Side Comparison

| Feature | LuaTS | PlexSpaces |
|---------|------|------------|
| **Linda** | Built-in | TupleSpace |
| **Events** | Built-in | GenEventBehavior |
| **Combined** | Unified API | Actor + TupleSpace |
| **Multi-thread** | Simplified | Actor model |

---

## Running the Comparison

```bash
cd examples/comparison/luats
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [LuaTS Documentation](https://link.springer.com/chapter/10.1007/978-3-319-39519-7_4)
- [PlexSpaces GenEventBehavior](../../../../crates/behavior/src/genevent.rs)
- [PlexSpaces TupleSpace](../../../../crates/tuplespace/src/mod.rs)
