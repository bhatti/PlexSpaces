# API Improvements Plan

## Goal
Simplify example code without hiding framework APIs or breaking tenant isolation.

---

## Change 1: `Node::spawn()` - Delegate to ActorFactory

**Location**: `crates/node/src/mod.rs`

**Purpose**: Convenience method - same signature as `ActorFactory::spawn_actor()`, just avoids the `service_locator().get_actor_factory()` dance.

```rust
impl Node {
    /// Spawn an actor on this node
    ///
    /// Delegates to ActorFactory::spawn_actor() - same parameters.
    /// This is a convenience method that avoids getting ActorFactory from ServiceLocator.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant/namespace isolation (REQUIRED, explicit)
    /// * `actor_id` - Actor ID (format: "name@node_id")
    /// * `actor_type` - Type of actor (e.g., "GenServer")
    /// * `initial_state` - Initial state bytes
    /// * `config` - Optional actor configuration
    /// * `labels` - Optional labels
    /// * `facets` - Optional facets
    ///
    /// ## Returns
    /// Arc<dyn MessageSender> for the spawned actor
    pub async fn spawn(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        actor_type: &str,
        initial_state: Vec<u8>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        labels: HashMap<String, String>,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<Arc<dyn MessageSender>, NodeError> {
        let actor_factory = self.service_locator().get_actor_factory().await
            .ok_or_else(|| NodeError::ConfigError("ActorFactory not found in ServiceLocator".to_string()))?;
        
        actor_factory.spawn_actor(ctx, actor_id, actor_type, initial_state, config, labels, facets)
            .await
            .map_err(|e| NodeError::ActorError(e.to_string()))
    }
}
```

**Key Points**:
- `ctx: &RequestContext` is **REQUIRED and explicit** - caller must provide tenant/namespace
- Same params as `ActorFactory::spawn_actor()` - no hiding
- Returns same type as ActorFactory
- NO `request_context_for_system_operations()` - that's for internal/system use only

---

## Change 2: `Message::json()` - JSON Serialization Builder

**Location**: `crates/mailbox/src/lib.rs` (or wherever Message is defined)

**Purpose**: Convenience for serializing messages, follows existing builder pattern.

```rust
impl Message {
    /// Create a Message from a JSON-serializable value
    ///
    /// ## Example
    /// ```rust
    /// let msg = Message::json(&CounterMessage::Increment { amount: 1 })?
    ///     .with_message_type("increment")
    ///     .with_sender("sender@node");
    /// ```
    pub fn json<T: serde::Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        let payload = serde_json::to_vec(value)?;
        Ok(Self::new(payload))
    }
}
```

**Key Points**:
- Returns `Result` (serialization can fail)
- Works with existing `.with_*()` builder methods
- No magic - just wraps `serde_json::to_vec()`

---

## What NOT to Change

### RequestContext Rules

1. **User-facing APIs**: `ctx: &RequestContext` is always an **explicit parameter**
2. **System operations only**: `request_context_for_system_operations()` is ONLY for:
   - Node initialization
   - Internal service setup
   - System-level operations that don't access tenant data
3. **Examples must show proper context**: All examples must create proper RequestContext with tenant/namespace

### Example Pattern

```rust
// CORRECT - explicit RequestContext with tenant/namespace, ActorId type
let ctx = RequestContext::new_without_auth("my-tenant".to_string(), "my-namespace".to_string());
let actor_id = ActorId::from("counter@node");
let actor = node.spawn(&ctx, &actor_id, "GenServer", vec![], None, HashMap::new(), vec![]).await?;

// WRONG - don't use system operations context for user code
let ctx = node.service_locator().request_context_for_system_operations().await; // NO!
```

---

## Implementation Order

1. [x] Add `Node::spawn()` delegate method
2. [x] Add `Message::json()` builder
3. [x] Verify framework compiles: `cargo check -p plexspaces-node -p plexspaces-mailbox`
4. [ ] Update examples to use new APIs with proper RequestContext

---

## Review Checklist

- [ ] `Node::spawn()` has same params as `ActorFactory::spawn_actor()`
- [ ] `RequestContext` is always explicit, never defaulted
- [ ] No `request_context_for_system_operations()` in user-facing code
- [ ] `Message::json()` follows existing builder pattern
- [ ] All examples show proper tenant/namespace context

---

## Design Decisions (Finalized)

1. **`Node::spawn()` uses `&ActorId`** - Same as `ActorFactory::spawn_actor()`. No `&str` variant.

2. **`Message::json()` does NOT infer message_type** - Always explicit via `.with_message_type()`.

---

## Rule: Always Explicit

**No magic, no inference, no defaults that hide intent.**

- `RequestContext` - Always explicit, caller provides tenant/namespace
- `ActorId` - Use the type, not raw strings
- `message_type` - Always explicit via `.with_message_type()`
- Parameters - Same as underlying API, no "simplified" variants that hide params

This ensures:
- Code is readable and self-documenting
- Multi-tenancy is enforced
- No surprises from inferred values
- Examples teach the real API

---

## Change 3: `impl Into<String>` for Builder Methods

**Location**: `crates/mailbox/src/mod.rs`

**Completed**: Message builder methods now accept `impl Into<String>`:
- [x] `with_message_type`
- [x] `with_sender`
- [x] `with_correlation_id`
- [x] `with_idempotency_key`
- [x] `with_reply_to`
- [x] `with_metadata` (both key and value)

**Usage**:
```rust
// Now works without .to_string()
Message::json(&msg)?
    .with_message_type("increment")
    .with_sender("actor@node")
```

---

## DONE: `ActorRef::tell()` now accepts mailbox Message directly

Implemented:
- [x] `From<mailbox::Message> for proto::Message` in mailbox crate
- [x] `ActorRef::tell(impl Into<Message>)` accepts both types

```rust
// Now works without .to_proto()
let msg = Message::json(&data)?.with_message_type("foo");
actor_ref.tell(msg).await?;  // Auto-converts via Into
```

---

## TODO: Remove `.to_proto()` from existing tests

Search and update existing tests that use the old pattern:
```bash
# Find usages to update
rg "\.to_proto\(\)" crates/*/tests/ crates/*/src/*_test*.rs
```

Pattern to replace:
```rust
// Old
actor_ref.tell(msg.to_proto()).await?;

// New
actor_ref.tell(msg).await?;
```

---

## TODO: Ensure all examples use shared target directory

Each example must have `.cargo/config.toml` with:
```toml
[build]
target-dir = "../../../../target"  # Adjust path depth based on example location
```

This prevents each example from creating its own `target/` directory and shares build artifacts.

**Completed:**
- [x] `examples/rust/embedded/actor_groups_sharding/.cargo/config.toml`

**Pending:** All other examples when rewritten

---

## TODO: Apply `impl Into<String>` Pattern Elsewhere

Review and apply `impl Into<String>` pattern to other builder methods in the framework:

- [ ] `NodeBuilder::new()` - currently takes `impl Into<NodeId>`
- [ ] `ActorBuilder::with_name()` / `with_id()`
- [ ] `RequestContext::new_without_auth()` - takes `String, String`
- [ ] `ActorId::from()` - already works via From trait
- [ ] Config builders in various crates
- [ ] Error message constructors

**Pattern to follow**:
```rust
// Before
pub fn with_foo(mut self, foo: String) -> Self

// After
pub fn with_foo(mut self, foo: impl Into<String>) -> Self {
    self.foo = foo.into();
    self
}
```

---

## Examples Strategy

### Phase 1: Core Rust Embedded Examples (COMPLETE)

Simple, use-case driven examples that demonstrate one concept each:

| Example | Use Case | Concept | Status |
|---------|----------|---------|--------|
| `actor_groups_sharding` | User counters | Horizontal scaling | ✅ |
| `supervision_tree` | Worker management | Fault tolerance | ✅ |
| `durable_actor` | Counter recovery | Journaling/replay | ✅ |
| `timers` | Session timeout | In-memory timers | ✅ |
| `reminders` | Subscription billing | Durable reminders | ✅ |
| `chat_room` | Real-time chat | Pub/Sub messaging | ✅ |
| `feature_flags` | Config propagation | Distributed config | ✅ |
| `webhook_handler` | GitHub/Stripe/Slack | HTTP invocation | ✅ |

### Phase 2: WASM Examples (After Core)

Reuse use cases from Phase 1, implement as WASM actors:

**Reference for WASM patterns**:
- `crates/wasm-runtime/tests/suite/wasm_component_integration.rs`
- `crates/wasm-runtime/tests/suite/shared_wasm_module.rs`
- `examples/python/apps/` - Existing Python WASM actors

| Example | Language | Based On |
|---------|----------|----------|
| `python/apps/chat_room` | Python | `process_groups_pubsub` |
| `python/apps/session_manager` | Python | `timers` |
| `python/apps/billing` | Python | `reminders` |
| `typescript/apps/chat_room` | TypeScript | `process_groups_pubsub` |

### Phase 3: Cross-Language Examples

Show same use case implemented in multiple languages:

```
examples/
├── rust/embedded/chat_room/      # Full Rust crate
├── python/apps/chat_room/        # Python WASM actor
└── typescript/apps/chat_room/    # TypeScript WASM actor
```

### WASM Build Pattern

From existing tests (`get_calculator_wasm_path`):

```rust
// Locate WASM file
fn get_wasm_path(name: &str) -> PathBuf {
    // Try: crates/wasm-runtime/tests/fixtures/{name}.wasm
    // Fallback: examples/python/apps/{name}/build/{name}.wasm
}

// Load and cache (40MB files are slow to compile)
static SHARED_MODULE: OnceLock<Mutex<WasmModule>> = OnceLock::new();
```

---

## Framework Simplification Opportunities

Identified during example rewrites:

| Issue | Current | Proposed | Priority |
|-------|---------|----------|----------|
| ProcessGroupRegistry API | Inconsistent - some take ctx, some take tenant/namespace strings | Unify to always take `&RequestContext` | ✅ Done |
| ActorBuilder::spawn | Returns `ActorRef` | Already good | ✅ Done |
| Node::spawn | Added as delegate | Done | ✅ Done |
| Message::json | Added builder | Done | ✅ Done |
| TimerFacet setup | Requires actor_ref + actor_service | Add `TimerFacet::standalone()` | Medium |
| ReminderFacet generic | `ReminderFacet<S>` hard to downcast | Consider type erasure | Low |
| NodeBuilder::new() | Two variants: `new()` vs `new(id)` | Unify to single pattern | Medium |
| Error types | `Box<dyn StdError + Send + Sync>` vs `Box<dyn std::error::Error>` | Unify error types for easier `?` usage | Medium |

### ProcessGroupRegistry API Inconsistency (FIXED)

**Before** - methods had inconsistent signatures:

```rust
// Takes strings directly
create_group(group_name, tenant_id, namespace)
join_group(group_name, tenant_id, namespace, actor_id, topics)

// Takes RequestContext
leave_group(ctx, group_name, actor_id)
get_members(ctx, group_name)
publish_to_group(ctx, group_name, topic, message)
delete_group(ctx, group_name)
```

**After** - All methods now take `&RequestContext` for consistency:

```rust
// Unified API
create_group(ctx, group_name)
join_group(ctx, group_name, actor_id, topics)
leave_group(ctx, group_name, actor_id)
get_members(ctx, group_name)
publish_to_group(ctx, group_name, topic, message)
delete_group(ctx, group_name)
```

**Files updated**:
- `crates/process-groups/src/lib.rs` - Core implementation
- `crates/node/src/service_wrappers.rs` - Node service wrapper
- `crates/application/src/service_wrappers.rs` - Application service wrapper
- `crates/wasm-runtime/src/component_host.rs` - WASM host functions
- `examples/rust/embedded/chat_room/src/main.rs`
- `examples/rust/embedded/feature_flags/src/main.rs`

### Error Type Unification (Medium Priority)

Currently, `ActorRef::tell()` returns `Result<_, Box<dyn StdError + Send + Sync>>` which requires `.map_err()` conversion when main uses `Box<dyn std::error::Error>`.

**Workaround in examples**:
```rust
handler.tell(msg).await.map_err(|e| format!("Send error: {}", e))?;
```

**Proposed**: Define a unified error type or ensure all errors implement `std::error::Error`.
