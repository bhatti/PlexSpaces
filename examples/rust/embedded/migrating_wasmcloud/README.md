# wasmCloud vs PlexSpaces Comparison

This comparison demonstrates how to implement wasmCloud-style WASM actors with capability providers in both wasmCloud and PlexSpaces.

## Use Case: WASM Actor with HTTP and KeyValue Capability Providers

A WASM actor that:
- Uses HTTP capability provider for external calls
- Uses KeyValue capability provider for caching
- Demonstrates polyglot support (WASM from any language)
- Showcases capability-based security model

## PlexSpaces Abstractions Showcased

- ✅ **WASM Actors** - Polyglot actor support via WASM
- ✅ **Capability Providers** - HTTP, KeyValue, and more
- ✅ **GenServerBehavior** - Request-reply pattern
- ✅ **Actor Model** - Stateful computation with capabilities

## Design Decisions

**Why Capability Providers?**
- wasmCloud uses capability-based security
- Actors request capabilities explicitly
- Secure by default (no implicit access)

**Why WASM Actors?**
- Polyglot support: Write actors in any language
- Portable: Run anywhere WASM runs
- Isolation: WASM sandbox provides security

---

## wasmCloud Implementation

### Native Rust Code (compiled to WASM)

See `native/wasm_actor.rs` for the complete wasmCloud implementation.

Key features:
- **WASM Actors**: Polyglot support (any language that compiles to WASM)
- **Capability Providers**: HTTP, KeyValue, and more
- **Capability-Based Security**: Actors request capabilities explicitly
- **Portable**: Run anywhere WASM runs

```rust
// Usage:
// Compile to WASM and deploy to wasmCloud host
// Actor automatically receives HTTP and KeyValue capabilities
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// WASM actor with capability providers
let behavior = Box::new(WasmActor::new(actor_id.clone()));
let mut actor = ActorBuilder::new(behavior)
    .with_id(actor_id.clone())
    .build()
    .await;

// Capability providers are accessed via actor methods
// HTTP capability: actor.http_call()
// KeyValue capability: actor.kv_get() / kv_set()
```

---

## Side-by-Side Comparison

| Feature | wasmCloud | PlexSpaces |
|---------|-----------|------------|
| **WASM Actors** | Built-in | Via WASM runtime |
| **Capability Providers** | HTTP, KeyValue, etc. | Simulated (can be real) |
| **Polyglot Support** | Any WASM language | Via WASM compilation |
| **Security Model** | Capability-based | Capability-based |

---

## Running the Comparison

```bash
cd examples/comparison/wasmcloud
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [wasmCloud Documentation](https://wasmcloud.dev/)
- [PlexSpaces WASM Support](../../../../docs/WASM_INTEGRATION.md)
