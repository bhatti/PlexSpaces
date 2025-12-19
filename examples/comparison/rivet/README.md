# Rivet vs PlexSpaces Comparison

This comparison demonstrates how to implement Rivet-style actors (Cloudflare Durable Objects pattern) in both Rivet and PlexSpaces.

## Use Case: Counter Actor with Virtual Actor Lifecycle

A virtual actor that:
- Automatically activates on first message
- Persists state across requests
- Demonstrates virtual actor lifecycle management

## PlexSpaces Abstractions Showcased

This example demonstrates:
- ✅ **VirtualActorFacet** - Automatic activation/deactivation
- ✅ **DurabilityFacet** - State persistence
- ✅ **Virtual Actor Pattern** - Automatic activation on first message
- ✅ **GenServerBehavior** - Request-reply pattern

## Design Decisions

**Why VirtualActorFacet?**
- Matches Rivet's virtual actor model
- Automatic activation on first message
- Automatic deactivation after idle timeout

**Why VirtualActorFacet?**
- Rivet actors use lazy activation pattern
- Actors are always addressable but not always in memory
- First message triggers activation automatically

---

## Rivet Implementation

### Native TypeScript Code

See `native/counter.ts` for the complete Rivet implementation.

Key features:
- **Virtual Actors**: All actors are virtual (automatic activation)
- **State Persistence**: State automatically persisted via Durable Objects
- **Lifecycle Management**: Automatic activation/deactivation

```typescript
// Usage:
const counter = await rivet.actors.getOrActivate('counter', 'rivet-1');
await counter.increment();
const count = await counter.get();
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// Create counter actor with VirtualActorFacet (Rivet-style)
let behavior = Box::new(CounterActor::new());
let mut actor = ActorBuilder::new(behavior)
    .with_id(actor_id.clone())
    .build()
    .await;

// Attach VirtualActorFacet (Rivet actors are virtual)
let virtual_facet = Box::new(VirtualActorFacet::new(config));
actor.attach_facet(virtual_facet, 100, config).await?;

// Attach DurabilityFacet (state persistence)
let durability_facet = Box::new(DurabilityFacet::new(storage, config));
actor.attach_facet(durability_facet, 50, serde_json::json!({})).await?;

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
let counter_ref = plexspaces_core::ActorRef::new(actor_id)?;
let counter = create_actor_ref(counter_ref, node).await?;
```

---

## Side-by-Side Comparison

| Feature | Rivet | PlexSpaces |
|---------|-------|------------|
| **Virtual Actors** | Built-in (all actors are virtual) | `VirtualActorFacet` (optional) |
| **Activation** | Automatic on first message | VirtualActorFacet pattern |
| **State Persistence** | Automatic | `DurabilityFacet` (optional) |
| **Lifecycle** | Automatic deactivation | Configurable idle timeout |

---

## Running the Comparison

```bash
cd examples/comparison/rivet
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Rivet Documentation](https://github.com/rivet-dev/rivet)
- [PlexSpaces VirtualActorFacet](../../../../crates/journaling/src/virtual_actor_facet.rs)
