# Orbit vs PlexSpaces Comparison

This comparison demonstrates how to implement Orbit-style virtual actors (JVM-based virtual actor framework) in both Orbit and PlexSpaces.

## Use Case: User Profile Actor with Lifecycle Management

A virtual actor that:
- Manages user profile state
- Automatically activates on first message
- Persists state across requests
- Demonstrates lifecycle management

## PlexSpaces Abstractions Showcased

- ✅ **VirtualActorFacet** - Automatic activation/deactivation, lifecycle management
- ✅ **DurabilityFacet** - State persistence
- ✅ **Virtual Actor Pattern** - Automatic activation on first message
- ✅ **GenServerBehavior** - Request-reply pattern

## Design Decisions

**Why VirtualActorFacet?**
- Matches Orbit's virtual actor model
- Automatic lifecycle management
- Configurable idle timeout

**Why VirtualActorFacet?**
- Orbit actors use lazy activation
- Actors are always addressable
- First message triggers activation

---

## Orbit Implementation

### Native Java Code

See `native/user_actor.java` for the complete Orbit implementation.

Key features:
- **Virtual Actors**: All actors are virtual (automatic lifecycle)
- **Lifecycle Management**: Automatic activation/deactivation
- **State Persistence**: State persisted across deactivation/reactivation

```java
// Usage:
UserActor user = Actor.getReference(UserActor.class, "user-123");
await user.updateProfile("Alice", "alice@example.com");
UserProfile profile = await user.getProfile();
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// Create user actor with VirtualActorFacet (Orbit-style)
let behavior = Box::new(UserActor::new());
let mut actor = ActorBuilder::new(behavior)
    .with_id(actor_id.clone())
    .build()
    .await;

// Attach VirtualActorFacet (lifecycle management)
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
let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await?;
let user_ref = plexspaces_core::ActorRef::new(actor_id)?;
let user = create_actor_ref(user_ref, node).await?;
```

---

## Side-by-Side Comparison

| Feature | Orbit | PlexSpaces |
|---------|-------|------------|
| **Virtual Actors** | Built-in (all actors are virtual) | `VirtualActorFacet` (optional) |
| **Lifecycle** | Automatic management | Configurable via `VirtualActorFacet` |
| **State Persistence** | Automatic | `DurabilityFacet` (optional) |
| **Activation** | Lazy (on first message) | `get_or_activate_actor` pattern |

---

## Running the Comparison

```bash
cd examples/comparison/orbit
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Orbit Documentation](https://www.orbit.cloud/orbit/)
- [PlexSpaces VirtualActorFacet](../../../../crates/journaling/src/virtual_actor_facet.rs)
