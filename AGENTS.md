# PlexSpaces Development Guide (CLAUDE.md)

## 🚨 CRITICAL RULES - READ FIRST

### Rule #0: NEVER BREAK TESTS (ABSOLUTE PRIORITY)
**THIS OVERRIDES EVERYTHING ELSE**

### Rule #0a: REVIEW DESIGN CHANGES WITH ME BEFORE IMPLEMENTATION (ABSOLUTE PRIORITY)

**ANY structural decision that adds, removes, or moves things across crates/modules requires approval BEFORE writing code.**

Examples that REQUIRE approval before touching any file:
- Adding a field or method to an existing struct/trait (Node, ServiceLocator, etc.)
- Adding a new trait or abstraction
- Deciding where something lives (which crate, which struct)
- Adding a new crate dependency
- Choosing between two valid implementation approaches

The rule: **if you are about to make a choice that can't be undone with a single Edit, STOP and ask first.**

❌ **FORBIDDEN**: Writing code that encodes a design decision before presenting the options to the user
❌ **FORBIDDEN**: Picking an approach mid-implementation and continuing without checking
✅ **MANDATORY**: State the decision clearly, present options with tradeoffs, wait for explicit approval

```bash
# BEFORE EVERY COMMIT (MANDATORY):
make build          # Must succeed with zero errors
make test           # ALL tests must pass
make test-examples  # ALL examples must pass

# If ANY test fails: STOP, FIX IMMEDIATELY, do not proceed
```

- ❌ **FORBIDDEN**: Commit code that breaks ANY test
- ❌ **FORBIDDEN**: Skip running tests before committing
- ❌ **FORBIDDEN**: Mark tasks as "done" or "complete" when tests are failing
- ❌ **FORBIDDEN**: Say something is "ready" or "complete" without verifying ALL tests pass
- ❌ **FORBIDDEN**: No timing based flaky test, use condition variables or other robust primitives.
- ❌ **FORBIDDEN**: Do not solve cyclic dependencies with ugly hacks like traits.
- ❌ **FORBIDDEN**: Don't use sleep or flaky setup in tests.
- ❌ **FORBIDDEN**: Avoid using role proper of actor if possible as it's meant to be used only if class-type/actor-type is same and is used for diff purpose like leader/worker. Better way is to have different class/actor-types for diff usages.
- ✅ **MANDATORY**: All tests pass before ANY commit
- ✅ **MANDATORY**: Fix broken tests IMMEDIATELY
- ✅ **MANDATORY**: Verify `make test` AND `make test-examples` pass before claiming completion
- ✅ **MANDATORY**: Test example scripts (e.g., `test.sh`) must pass before marking as done
- ✅ **MANDATORY**: Use simple, reliable, solid and production grade solutions not ugly hacks, work arounds or conditional logic.

**Why #1**: Broken tests = broken system = broken trust. No exceptions.
**Why #2**: Claiming something is "done" when tests fail is misleading and wastes time.

### Rule #0b: NO DUPLICATE STRUCT/TRAIT/IMPL DEFINITIONS (ABSOLUTE PRIORITY)
**Each type must be defined EXACTLY ONCE in the codebase**

```bash
# Detect duplicates before any refactor:
grep -r "struct <TypeName>" crates/ | grep -v target/
grep -r "pub struct <TypeName>" crates/ | grep -v target/

# ❌ FORBIDDEN: Same struct/trait defined in two crates
# ❌ FORBIDDEN: Orphaned .rs files not included in any mod.rs / lib.rs
# ✅ MANDATORY: Each type lives in ONE canonical crate (usually the lowest-level crate)
# ✅ MANDATORY: Other crates re-export via `pub use`, never re-define
# ✅ MANDATORY: Delete orphaned files (not declared in any `mod`) immediately
```

**Why**: Duplicate definitions cause behavioral drift, maintenance burden, and hidden bugs when only one copy gets updated.

---

### Rule #0c: Targeted `cargo test` during iteration (agents and fast feedback)

Rule #0 still requires the **full** `make test` and `make test-examples` before you commit or treat work as complete. During implementation, **do not** run the whole workspace suite in tight loops—it is too slow.

- **Prefer** narrow invocations that cover the code you changed:
  - `cargo test -p <crate> --lib` — unit tests for one crate
  - `cargo test -p <crate> --test <binary> <name_filter>` — one integration binary + optional name substring
- **Examples** (adjust to your change):
  - `cargo test -p plexspaces-wasm-runtime --lib`
  - `cargo test -p plexspaces-application --lib`
  - `cargo test -p plexspaces-node --test node_integration_tests wasm_application`
  - `cargo test -p plexspaces-node --test node_integration_tests test_go_wasm_controller_stop`
- **AI assistants**: Run **only** targeted tests relevant to the edit; avoid `make test` and full-repo `cargo test` unless the user explicitly asks for a full run or the change is broad.
- **Humans**: Run the full Rule #0 commands before push or release.

---

### Rule #1: Proto-First Design
**ALL data models MUST be in Protocol Buffers FIRST**

```bash
# Correct workflow:
1. Define in .proto → buf generate → Implement → Test
2. NEVER write Rust code before proto definitions

# What goes in Proto:
✅ Data structures (Message, ActorId, Config types)
✅ Error enums (WorkflowError, MailboxError)
✅ gRPC services (for remote communication ONLY)

# What stays in Rust:
✅ Traits (ActorBehavior, JournalStorage)
✅ Implementation details (Actor, Mailbox, internal state)
✅ Helper functions
```

**Key**: Proto = contract (what), Rust = implementation (how)

---

### Rule #2: Test-Driven Development (95%+ Coverage)
**Every line of code must have a test BEFORE it's written**

```bash
# TDD workflow:
1. Write failing test (RED)
2. Write minimal code to pass (GREEN)
3. Refactor while keeping green
4. Verify: cargo tarpaulin --lib --fail-under 95

# Fast iteration during development:
cargo test -p <crate-name> --lib    # Test specific crate (fast)
make test                            # Full suite periodically (slow)
```

**Coverage targets**: Core modules 98%, Critical 95%, Extensions 85%

---

### Rule #3: DON"T REFORMAT CODE WITHOUT MY APPROVAL (ABSOLUTE PRIORITY)


---

### Rule #4: Not spawning multiple background processes

---

### Rule #5: NEVER Use Git Commands - Git is FORBIDDEN
**Git operations are handled by the user, not by the AI assistant**

```bash
# ❌ FORBIDDEN - Never run any git commands
git add
git commit
git push
git branch
git checkout
git status
git log
git diff
# ... or ANY git command

# ✅ CORRECT - User handles all git operations manually
# AI assistant only modifies files, never runs git
```

**Why**: Git operations require user oversight and approval. The user maintains full control over version control.

---

### Rule #6: NEVER Use `RequestContext::internal()` - Tenant Isolation is Mandatory
**Tenant isolation is REQUIRED for all operations**

```rust
// ❌ FORBIDDEN - Never use internal() context
let ctx = RequestContext::internal();
node.link(actor1.id(), actor2.id(), &ctx).await?;

// ✅ CORRECT - Always use proper tenant/namespace context
use plexspaces_core::RequestContext;
let ctx = RequestContext::new_without_auth("tenant-id".to_string(), "namespace".to_string());
node.link(actor1.id(), actor2.id(), &ctx).await?;

// ✅ CORRECT - Extract from gRPC request
let ctx = extract_request_context(&request)?;
node.link(actor1.id(), actor2.id(), &ctx).await?;

// ✅ CORRECT - Pass as parameter from caller
async fn my_function(&self, ctx: &RequestContext) -> Result<()> {
    node.link(actor1.id(), actor2.id(), ctx).await?;
}
```

**Why**: 
- Tenant isolation is a **security requirement** - all operations must be scoped to a tenant
- `RequestContext::internal()` bypasses tenant isolation and is only for system initialization
- Tests must use proper tenant/namespace contexts to verify tenant isolation works correctly
- Production code must extract context from requests (gRPC metadata, HTTP headers, etc.)

**When `internal()` is acceptable** (rare, system initialization only):
- During `Node::new()` or `ServiceLocator::new()` initialization
- System-level operations that don't access tenant-specific data
- Must be documented with a comment explaining why it's necessary

**All other cases**: Use `RequestContext::new_without_auth()` with explicit tenant/namespace, or extract from request.

---

### Rule #7: Update PROJECT_TRACKER.md Always
**Source of truth for project status**

```bash
# After completing meaningful work:
1. Run verification (make build && make test && make test-examples)
2. Update PROJECT_TRACKER.md:
   - Mark completed tasks ✅
   - Add new todos discovered
   - Update current status
3. User will handle git commit/push (AI never runs git commands)

# When updates are needed:
✅ After completing a feature
✅ After fixing a bug with tests
✅ After adding test suite
❌ NOT after every single file edit
❌ NOT after minor formatting
```

---

## 🏗️ Core Workflow (Muscle Memory)

```bash
# Daily development loop:

1. Proto first
   vim proto/plexspaces/v1/feature.proto
   buf format -w proto/ && buf lint
   # (User runs: buf generate)

2. Test first (RED)
   vim crates/module/tests/feature_test.rs
   cargo test -p module --lib  # Must FAIL

3. Implement (GREEN)
   vim crates/module/src/feature.rs
   cargo test -p module --lib  # Must PASS

4. Verify (MANDATORY after ANY change)
   make build && make test && make test-examples

5. Update tracker
   vim PROJECT_TRACKER.md

6. Commit & push
   git commit -m "feat(module): feature X"
   git push
```

---

## 🎯 Design Principles (Non-Negotiable)

### 0. No `Any` Types - Use Traits Instead

**NEVER use `Arc<dyn Any>` or `Box<dyn Any>` for service dependencies.**

```rust
// ❌ FORBIDDEN - Using Any types
struct Service {
    node: Arc<dyn Any + Send + Sync>,
}

// ✅ CORRECT - Using proper traits
struct Service {
    service_locator: Arc<dyn ServiceLocator>,
}

// Access node capabilities through traits:
let connection_info = service_locator.get_node_connection_info().await?;
let connected_nodes = connection_info.connected_nodes().await;

let config = service_locator.get_node_config().await?;
```

**Available Node Capability Traits:**
- `NodeConnectionInfo`: Access node connection information (connected nodes list)
- `ServiceLocator::get_node_config()`: Access node configuration
- `ServiceLocator::get_metrics_prometheus_renderer()` / `MetricsServiceAccess`: In-process Prometheus exposition for node and actor counters

**Why**: `Any` types require unsafe downcasting, break type safety, and make code harder to understand and maintain. Traits provide clear contracts and compile-time guarantees.

### 1. No Global State
**Everything flows through dependency injection**

```rust
✅ ActorContext with injected services (Arc<ServiceLocator>)
✅ Node-scoped registries (ActorRegistry, ReplyTracker)
✅ Pass dependencies via constructors

❌ Global static singletons
❌ lazy_static! for state
```

### 2. Location Transparency
```rust
// Actor doesn't know if target is local or remote
ctx.tell("actor-id", message).await;  // Automatic routing

// Local: Direct mailbox
// Remote: gRPC call
```

### 3. Architecture Patterns

**Pattern 1: Proto for Data, Traits for Behavior**
```rust
// Proto: Messages only (serializable)
message GenServerRequest { ... }

// Rust: Behavior interface (in-process)
#[async_trait]
trait GenServer {
    async fn handle_request(&mut self, req: GenServerRequest, ctx: &ActorContext) -> Result<Response>;
}
```

**Pattern 2: WIT for WASM, Proto for Payloads**
```wit
// WIT: Host function signatures
tell: func(target: string, payload: list<u8>) -> result<_, string>

// Proto: Message payloads across boundary
```

**Pattern 3: ActorContext as Capability Provider**
```rust
impl ActorContext {
    // Spawning
    pub async fn spawn_local(&self, ...) -> ActorRef;
    pub async fn spawn_remote(&self, node_id, ...) -> ActorRef;
    
    // Messaging
    pub async fn tell(&self, target, msg) -> Result<()>;
    pub async fn ask(&self, target, msg, timeout) -> Result<Reply>;
    
    // Coordination
    pub async fn tuplespace(&self, name) -> Arc<TupleSpace>;
    pub async fn ts_write(&self, space, tuple) -> Result<()>;
}
```

---

## 🎨 SDK Patterns (MANDATORY for Examples and Docs)

### Rule #9: Use SDK, Not Low-Level APIs

**All examples, documentation, and new code MUST use SDK patterns:**

```rust
// ❌ FORBIDDEN - Low-level ActorFactory
let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().actor_factory_impl().await?;
let _message_sender = actor_factory.spawn_actor(&ctx, &actor_id, "Counter", vec![], None, HashMap::new(), vec![]).await?;

// ✅ CORRECT - SDK spawn helpers
use plexspaces_sdk::{spawn, spawn_with_facets, spawn_with_storage};
let actor_ref = spawn_with_facets(&ctx, service_locator, actor_id, "namespace", actor, vec![]).await?;
```

### Actor Definition (SDK Annotations)

```rust
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers, handler, json};

// Define actor with annotation (like Python @actor)
#[gen_server_actor]
struct Counter { count: i32 }

// Define handlers - GenServer defaults to "call" (request-reply)
#[plexspaces_handlers]
impl Counter {
    #[handler("increment")]
    async fn increment(&mut self, _ctx: &ActorContext, msg: &Message) 
        -> Result<serde_json::Value, BehaviorError> {
        self.count += 1;
        Ok(json!({ "count": self.count }))
    }
}
```

### Spawning Actors (SDK Helpers)

```rust
use plexspaces_sdk::{spawn, spawn_with_facets, spawn_with_storage, RequestContext, ActorId};

// NEVER use RequestContext::internal() - use proper tenant/namespace
let ctx = RequestContext::new_without_auth("tenant".into(), "namespace".into());

// Option 1: Spawn with declared facets (from annotation)
let actor_ref = spawn(&ctx, service_locator, actor_id, "ns", actor).await?;

// Option 2: Spawn with explicit facets
let actor_ref = spawn_with_facets(&ctx, service_locator, actor_id, "ns", actor, vec![Box::new(timer_facet)]).await?;

// Option 3: Spawn durable actor with storage
let storage = Arc::new(SqliteJournalStorage::new(":memory:").await?);
let actor_ref = spawn_with_storage(&ctx, service_locator, actor_id, "ns", actor, storage).await?;
```

### Message Creation (SDK Helpers)

```rust
use plexspaces_sdk::{call_message, cast_message, json};
use std::time::Duration;

// ❌ FORBIDDEN - Manual message construction
let msg = Message { id: ulid::Ulid::new().to_string(), message_type: "call".into(), payload: ..., ..Default::default() };

// ✅ CORRECT - SDK message helpers
// Request-reply: use call_message() with ask()
let request = call_message(json!({ "action": "get_balance" }));
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;

// Fire-and-forget: use cast_message() with tell()
let event = cast_message(json!({ "event": "user_login" }));
actor_ref.tell(event).await?;
```

### Available SDK Annotations

| Annotation | Behavior | Use Case |
|------------|----------|----------|
| `#[gen_server_actor]` | GenServer | Request-reply (call by default) |
| `#[event_actor]` | GenEvent | Fire-and-forget events (cast) |
| `#[fsm_actor]` | GenStateMachine | State machine transitions |
| `#[fsm_actor(states = ["a","b"], initial = "a")]` | GenStateMachine | FSM with explicit state list and initial state |
| `#[workflow_actor]` | Workflow | Durable workflow orchestration |
| `#[actor(facets = ["timer"])]` | Custom | Custom behavior with facets |

### Handler Annotations

| Annotation | Semantics | Return Type |
|------------|-----------|-------------|
| `#[handler("op")]` | call (GenServer default) | `Result<Value, BehaviorError>` |
| `#[handler("op", call)]` | Explicit call | `Result<Value, BehaviorError>` |
| `#[handler("op", cast)]` | Fire-and-forget | `Result<(), BehaviorError>` |

---

## 📋 Quick Reference

### Communication Patterns (SDK)
```rust
use plexspaces_sdk::{call_message, cast_message, json};

// Fire-and-forget (tell) - use cast_message()
let event = cast_message(json!({ "event": "user_login" }));
actor_ref.tell(event).await?;

// Request-reply (ask) - use call_message()
let request = call_message(json!({ "action": "get_balance" }));
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;

// Spawn actor - use SDK helpers
let actor_ref = spawn_with_facets(&ctx, service_locator, actor_id, "ns", actor, facets).await?;
```

### Project Structure
```
PlexSpaces/
├── proto/plexspaces/
│   ├── v1/              # Public APIs
│   └── prv/             # Private/internal
├── crates/
│   ├── core/            # ActorId, ActorContext, ActorRef
│   ├── actor/           # Actor implementation
│   ├── behavior/        # GenServer, workflows
│   ├── supervisor/      # Fault tolerance
│   └── tuplespace/      # Coordination
├── examples/            # Usage examples (NOT framework code)
├── target/              # Single shared build output (see below)
└── PROJECT_TRACKER.md   # ⭐ SOURCE OF TRUTH
```

### Examples: Shared target directory (mandatory)
All examples MUST use the **workspace shared target directory**. Do NOT create a separate `target/` inside any example.
- **Canonical path**: `<workspace>/target` (e.g. `tspaces/target` at repo root).
- **Per example**: Each example MUST have `.cargo/config.toml` with `[build] target-dir = "<relative to workspace>/target"` (e.g. for `examples/rust/embedded/webhook_handler` use `target-dir = "../../../../target"`).
- **Test scripts**: Must set `CARGO_TARGET_DIR` to the workspace target when invoking cargo, or rely on `.cargo/config.toml`.
- **Rationale**: Single build artifact tree; avoids rebuilding the framework for every example.

### Test Organization
```bash
# Unit tests: At bottom of source file
#[cfg(test)]
mod tests { ... }

# Integration: crates/{name}/tests/
# E2E: tests/ (sparingly)
# Examples: examples/{name}/tests/
```

### Documentation Requirements
```rust
// GNU AFFERO GENERAL PUBLIC LICENSE header on EVERY file
// SPDX-License-Identifier: AGPL-3.0-or-later

// Every public item needs:
/// # Purpose
/// What problem does this solve?
///
/// # Architecture Context
/// How does it fit in PlexSpaces?
///
/// # Examples
/// ```rust
/// // Compilable example
/// ```

// Enforce:
RUSTDOCFLAGS="-D missing_docs" cargo doc --no-deps
```

---

## ⚙️ Parallel and Collective Operations

- **Pure logic in actor crate**: Stateless parallel helpers (reduction operations, stats computation, collective message construction, result conversion) live in `crates/actor/src/parallel.rs`. This module has no service state, no gRPC dependencies, and is independently testable.
- **Services are thin controllers**: The actor-service (`crates/services/src/actor_service/`) handles gRPC request/response, request context extraction, shard group state (`RwLock`), and parallel fan-out orchestration (temp sender lifecycle). It delegates pure computation to `plexspaces_actor::parallel`.
- **No MPI naming in core APIs**: Public API names use shard-group vocabulary (`BroadcastShardGroup`, `ReduceShardGroup`, etc.). MPI conceptual mapping is documentation-only, never in type/function names.
- **Built-in reductions only**: Collective reductions use the proto `CollectiveReduction` enum (SUM, PRODUCT, MAX, MIN, BOOL_AND, BOOL_OR, CONCAT). Do not introduce arbitrary user-defined reducer functions over RPC.
- **Framework-owned barriers**: `BarrierShardGroup` is built on broadcast semantics with round tracking. Do not revive or depend on tuplespace barrier APIs.

---

## ✅ Pre-Commit Checklist

Before considering any work done:

- [ ] `make build` and full `make test` pass before merge; during work rely on targeted `cargo test` until the final check
- [ ] `make test-examples` passes (where applicable)
- [ ] No temporary or debug-only logging left in code (`eprintln!`, ungated debug statements)
- [ ] No hacks, duplicate abstractions, or unresolved cyclic dependencies
- [ ] New/changed code has tests and meets the coverage bar (90%+ where applicable)
- [ ] Public APIs and non-obvious logic are commented; `docs/*.md` (and related READMEs) are updated
- [ ] No backward-compat, legacy, or dead code introduced; any such code in the change set is removed
- [ ] SDK/docs use node-based spawn, annotation-based actors, and the canonical message constructors; docs no longer reference deprecated patterns (e.g. `ActorTrait`, `actor-factory`, `Message::new`) where the new approach applies
- [ ] `PROJECT_TRACKER.md` updated for meaningful feature/bug work

---

## ⚠️ Common Mistakes to Avoid

1. ❌ Code before proto
2. ❌ Code before tests
3. ❌ Using `uuid` (use `ulid`)
4. ❌ Ignoring test failures
5. ❌ Not updating PROJECT_TRACKER.md
6. ❌ Large commits with broken tests
7. ❌ Coverage below 90%
8. ❌ Global state (use dependency injection)
9. ❌ Committing after every tiny change
10. ❌ Adding features without updating documentation
11. ❌ Breaking links in documentation
12. ❌ Not referencing main docs from crate/example READMEs
13. ❌ **Using `RequestContext::internal()` - FORBIDDEN**
14. ❌ **Using ActorFactory directly** - use SDK `spawn_with_facets()` instead
15. ❌ **Manual Message construction** - use `call_message()` / `cast_message()`
16. ❌ **Implementing traits manually** - use SDK annotations (`#[gen_server_actor]`, etc.)
17. ❌ **MPI naming in public APIs** - use shard-group vocabulary
18. ❌ **Direct gRPC client creation** - use `GrpcConnectionManager`
19. ❌ **`eprintln!` or ungated debug logs** in committed code
20. ❌ **Manual `ActorId` string formatting** - use `ActorId::build()` / `ActorId::parse()`

---

## 🎓 Core Philosophy

> **"Tests are not optional. Code without tests is legacy code."**

> **"Proto-first ensures we design contracts before implementations."**

> **"PROJECT_TRACKER.md is the single source of truth."**

> **"Every commit must leave the system working."**

**Investment Formula**: 20% more time on proto/tests/docs = 80% less debugging time

---

## 📖 Extended Documentation

See detailed docs:
- `PROJECT_TRACKER.md` - Status, todos, blockers **(check first)**
- `PROJECT_CONTEXT.md` - Full project context, architecture, and design reference **(read at session start)**
- `docs/architecture.md` - System architecture and design
- `docs/detailed-design.md` - Component deep-dives
- `docs/getting-started.md` - Quick start guide
- `docs/use-cases.md` - Real-world applications
- `docs/installation.md` - Deployment instructions
- `examples/README.md` - Example gallery
- `docs/cli.md` - CLI reference

---

## 📚 Documentation Maintenance

### Rule #8: Keep Documentation Up-to-Date

**When adding new features or making changes, you MUST update documentation:**

```bash
# Documentation update checklist:
1. Update relevant docs/ files (architecture.md, detailed-design.md, etc.)
2. Update crate README.md if adding new crate features
3. Update example README.md if adding new examples
4. Update main README.md if adding major features
5. Add cross-references between docs
6. Verify all links work
```

### Documentation Structure

**Main Documentation** (in `docs/` directory):
- `README.md` - Project overview (root)
- `docs/architecture.md` - High-level architecture
- `docs/detailed-design.md` - Component details (all facets, behaviors, primitives)
- `docs/getting-started.md` - Quick start guide
- `docs/installation.md` - Docker/Kubernetes installation
- `docs/use-cases.md` - Real-world applications
- `examples/README.md` - Example gallery
- `docs/cli.md` - CLI reference

**Crate Documentation** (in `crates/*/README.md`):
- Each crate MUST have a README.md
- Include: Purpose, Overview, Key Components, Usage Examples, References to main docs
- Reference main docs: `[Architecture](../../docs/architecture.md)`

**Example Documentation** (in `examples/*/README.md`):
- Each example MUST have a README.md
- Include: Overview, Features, Usage, What it Demonstrates, References to main docs
- Reference main docs: `[Getting Started](../../docs/getting-started.md)`

### When to Update Documentation

**MANDATORY updates when:**
- ✅ Adding new facet types → Update `docs/detailed-design.md#facets`
- ✅ Adding new behaviors → Update `docs/detailed-design.md#behaviors`
- ✅ Adding new features → Update `docs/architecture.md` and `docs/detailed-design.md`
- ✅ Adding new use cases → Update `docs/use-cases.md`
- ✅ Adding new examples → Update `examples/README.md` and example README
- ✅ Changing APIs → Update relevant docs and crate READMEs
- ✅ Adding new crates → Create crate README.md with references to main docs

**Documentation Review Checklist:**
- [ ] All facets documented in `docs/detailed-design.md`
- [ ] All behaviors documented in `docs/detailed-design.md`
- [ ] All primitives documented (Actors, ActorRef, TupleSpace, etc.)
- [ ] All APIs documented with examples
- [ ] All use cases covered in `docs/use-cases.md`
- [ ] All examples listed in `examples/README.md`
- [ ] Cross-references added between docs
- [ ] Crate READMEs reference main docs
- [ ] Example READMEs reference main docs
- [ ] No broken links

### Documentation Standards

**Every documentation file should:**
- Start with purpose/overview
- Include architecture context
- Provide usage examples
- Reference related docs
- Be kept up-to-date with code changes

**Crate READMEs should:**
- Explain the crate's purpose
- Show key components and usage
- Reference main architecture docs
- Include dependencies and dependents

**Example READMEs should:**
- Explain what the example demonstrates
- Show how to run it
- Reference relevant main docs
- List key PlexSpaces features showcased
