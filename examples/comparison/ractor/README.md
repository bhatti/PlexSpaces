# Ractor vs PlexSpaces Comparison

This comparison demonstrates how to implement Ractor-style Rust-native actors in both Ractor and PlexSpaces.

## Use Case: Calculator Actor with Type-Safe Message Passing

A calculator actor that:
- Performs arithmetic operations (Add, Subtract, Multiply, Divide)
- Maintains operation count
- Demonstrates Rust-native actor model with type safety

## PlexSpaces Abstractions Showcased

- ✅ **GenServerBehavior** - Request-reply pattern
- ✅ **Actor Model** - Rust-native actors with type safety
- ✅ **Message Passing** - Type-safe message handling

## Design Decisions

**Why GenServerBehavior?**
- Ractor uses request-reply pattern
- Matches Rust's type-safe message passing
- Simple and straightforward

**Why Rust-Native Actors?**
- Ractor is built for Rust
- Type safety at compile time
- Performance benefits of native Rust

---

## Ractor Implementation

### Native Rust Code

See `native/calculator.rs` for the complete Ractor implementation.

Key features:
- **Rust-Native**: Built for Rust with type safety
- **Type-Safe Messages**: Compile-time message type checking
- **Request-Reply**: RpcReplyPort for synchronous communication
- **Actor Spawning**: Dynamic actor creation with handles

```rust
// Usage:
let (actor, handle) = Actor::spawn(None, CalculatorActor, ()).await?;
let result = actor.call(CalculatorMessage::Add { a: 10.0, b: 5.0, reply: port }).await?;
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// Calculator actor with type-safe message passing
let behavior = Box::new(CalculatorActor::new());
let actor = ActorBuilder::new(behavior)
    .with_id(actor_id.clone())
    .build()
    .await;

let actor_ref = node.spawn_actor(actor).await?;

// Send message (Ractor: actor.send_message(Add { a: 10, b: 5 }))
let result = calculator.ask(Add { a: 10.0, b: 5.0 }).await?;
```

---

## Side-by-Side Comparison

| Feature | Ractor | PlexSpaces |
|---------|--------|------------|
| **Language** | Rust | Rust |
| **Actor Model** | Built-in | GenServerBehavior |
| **Message Passing** | Type-safe | Type-safe (via serde) |
| **State Isolation** | Actor boundaries | Actor boundaries |

---

## Running the Comparison

```bash
cd examples/comparison/ractor
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Ractor Documentation](https://github.com/slawlor/ractor)
- [PlexSpaces GenServerBehavior](../../../../crates/behavior/src/genserver.rs)
