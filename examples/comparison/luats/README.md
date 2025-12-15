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
let behavior = Box::new(EventCoordinatorActor::new());
let coordinator = node.spawn_actor(actor).await?;

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
