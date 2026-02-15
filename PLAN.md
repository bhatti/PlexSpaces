# PlexSpaces WASM & SDK Improvement Plan

## Architecture Review

### Current WASM Runtime Design

PlexSpaces has two WIT worlds reflecting a sound architectural split:

1. **`plexspaces-actor`** (full typed world) — 15 WIT interfaces with structured types (`payload = list<u8>`, `tuple-data`, `pattern`, `context`, etc.). Used by Rust components targeting `wasm32-wasip2`. Exports the `actor` interface with `init()`, `handle-message()`, `snapshot-state()`, `shutdown()`, plus behavior-specific handlers (`handle-call`, `handle-cast`, `handle-event`, `handle-transition`).

2. **`plexspaces:simple-actor`** (JSON string world) — Simplified interface for Python/TypeScript/Go compatibility. All payloads are JSON strings. Host provides `send`, `log`, `now-ms`, kv, tuplespace, locks, blob. Actor exports `init`, `handle`, `get-state`, `set-state`.

This split is architecturally correct: the Component Model's typed interfaces work well with Rust's type system but cause friction with Python (componentize-py) and JavaScript (jco componentize) — particularly around recursion in structured types. The `simple-actor` world with JSON strings is the pragmatic right answer.

### The Re-instantiation Problem

**Root cause**: After every `handle()` call, PlexSpaces creates a fresh `Store<ComponentContext>` + component instance to avoid wasmtime's "cannot enter component instance" trap.

**What the code does** (`instance.rs:1491-1603`):
1. Lock `Arc<Mutex<ComponentState>>`
2. Call `handle()` on the WASM component
3. Drop the lock
4. Create entirely new `ComponentContext`, `Store`, WASI context, and component bindings
5. Call `init(original_init_config)` on the fresh instance
6. Replace the old `ComponentState` with the new one

**What goes wrong**: State changes from `handle()` are lost — only the original init config is replayed. The code explicitly documents this:
> "State changes from handle() are lost on re-instantiation; proper state persistence requires fix #2 (return state from handle) or fix #3 (host function persistence)."

**Is re-instantiation actually needed?** The wasmtime Component Model spec distinguishes:
- **Re-entrant calls** (guest→host→guest while first call is active): Always traps. This is correct behavior.
- **Sequential calls** (first call completes fully, then second call): Should work per spec.

PlexSpaces host functions do NOT re-enter the component — `send()` defers self-messages via `tokio::spawn()`, and all other host ops (kv, tuplespace, locks) are pure Rust calls. So re-entrancy is not triggered.

However, `componentize-py` and `jco componentize` may generate components with internal lifecycle state (e.g., WASI stdio stream consumption, Python interpreter init) that prevents sequential calls even though the Component Model spec allows them. This needs empirical verification.

### WIT Consistency Issues

1. **`context` parameter inconsistency**: `tuplespace`, `keyvalue`, `channels`, `durability`, `workflow`, `blob`, `locks`, `registry`, `process-groups` all take `ctx: context` as first parameter. `messaging` does not. For multi-tenancy, messaging should also accept context.

2. **simple-actor host gaps**: The `simple-actor/host` interface provides `send` (fire-and-forget) but lacks:
   - `ask` (request-reply) — critical for GenServer pattern
   - `spawn` — actors can't create child actors
   - `self-id` / `parent-id` — actors don't know their own identity
   - `link` / `monitor` — no supervision from guest
   - `send-after` — no timer scheduling
   - Process group operations — no pub/sub coordination

3. **PlexspacesActor `get-state`/`set-state` missing from WIT**: The `actor.wit` interface has `snapshot-state` but no `set-state` equivalent. For PlexspacesActor components, state restoration after re-instantiation requires `init(state_bytes)` rather than a dedicated `set-state`. The SimpleActor WIT correctly has both `get-state` and `set-state`.

### SDK Feature Parity

| Capability | Rust SDK | Python SDK | TypeScript SDK | simple-actor WIT |
|-----------|----------|------------|----------------|-----------------|
| Actor definition | `#[actor]` macro | `@actor` decorator | `extends PlexSpacesActor` | `export actor` |
| Handler routing | `#[handler("op")]` | `@handler("op")` | `on<Op>()` method | `handle()` dispatch |
| GenServer | `#[gen_server_actor]` | `@gen_server_actor` | implicit | via msg_type |
| GenEvent | `#[event_actor]` | `@event_actor` | **missing** | via msg_type |
| GenFSM | `#[fsm_actor]` | `@fsm_actor` | **missing** | via msg_type |
| Workflow | `#[workflow_actor]` | **missing** | **missing** | not in simple WIT |
| send (tell) | via ActorRef | `host.send()` | **missing** | `host.send()` |
| ask (call) | GenServerRef | **missing** | **missing** | **not in WIT** |
| spawn | `spawn_*()` | **missing** | **missing** | **not in WIT** |
| self-id | via ActorContext | **missing** | **missing** | **not in WIT** |
| KV store | via core | `host.kv_*()` | **missing** | `host.kv-*()` |
| TupleSpace | via core | `host.ts_*()` | **missing** | `host.ts-*()` |
| Locks | via facet | `host.lock_*()` | **missing** | `host.lock-*()` |
| Blob storage | via core | `host.blob_*()` | **missing** | `host.blob-*()` |
| Process groups | via core | `host.process_groups.*` | **missing** | **not in WIT** |
| Logging | tracing | `host.log()` | `hostLog()` | `host.log()` |
| Time | via core | `host.now_ms()` | **missing** | `host.now-ms()` |

---

## Phase 1: Fix WASM Runtime (Highest Priority)

### Step 1.1: Empirically Verify Re-instantiation Necessity

**Goal**: Determine if per-invocation re-instantiation is actually required.

**Method**: Write a targeted integration test that:
1. Instantiates a SimpleActor component (Python-built `.wasm`)
2. Calls `handle()` on it
3. Calls `handle()` again on the SAME store/instance WITHOUT re-instantiation
4. Checks whether it traps with "cannot enter component instance"

**If sequential calls work**: Remove re-instantiation entirely. This is a massive performance improvement (no Store recreation, no WASI context rebuild, no component re-linking, no `init()` replay per message).

**If sequential calls trap**: The limitation is in how `componentize-py`/`jco` generate component lifecycle code. Keep re-instantiation but fix state preservation (Step 1.2).

**Deliverable**: Test in `crates/wasm-runtime/tests/suite/` + decision document.

### Step 1.2: Fix State Preservation During Re-instantiation

**Only needed if Step 1.1 confirms re-instantiation is required.**

**Current flow** (broken):
```
handle() → drop old state → init(original_config)  [state lost]
```

**Fixed flow**:
```
handle() → get_state() → drop old state → init(original_config) → set_state(saved_state)
```

**Implementation in `instance.rs`**:

For **SimpleActor** path in `handle_message_component()`:
1. After `handle()` succeeds, before `drop(state)`, call `call_get_state(&mut *store)` to capture current state as JSON string
2. Pass this state to `create_fresh_simple_actor_state()`
3. In `create_fresh_simple_actor_state()`, after `call_init()`, call `call_set_state(&mut component_store, &saved_state)` to restore

For **PlexspacesActor** path:
1. After `handle_message` or `handle_event` succeeds, call `call_snapshot_state(&mut *store)` to get state bytes
2. In `create_fresh_plexspaces_actor_state()`, pass the snapshot as `initial_state` to `call_init()`

**Why `get_state/set_state` and not "return state from handle"**: The WIT already has the right abstractions. Adding state to handle's return type would break the interface and require SDK changes. Using the existing `get_state()`/`set_state()` cycle is the clean approach that works with all existing SDKs.

**Test**: Integration test that deposits 100, then deposits 200, verifies balance is 300 (state survived re-instantiation).

### Step 1.3: Deduplicate Re-instantiation Code

**Problem**: `create_fresh_simple_actor_state()` (lines 1491-1603) and `create_fresh_plexspaces_actor_state()` (lines 1605-1699) share ~100 lines of identical `ComponentContext` construction and linker wiring.

**Solution**: Extract `create_component_context()` helper:
```rust
fn create_component_context(&self, instance_ctx: &InstanceContext) -> ComponentContext {
    let tuplespace_provider = self.tuplespace_provider.clone();
    ComponentContext {
        instance_ctx: instance_ctx.clone(),
        wasi_ctx: Self::create_wasi_context(),
        resource_table: wasmtime_wasi::ResourceTable::new(),
        plexspaces_host: PlexspacesHost::new(self.actor_id.clone(), instance_ctx.host_functions.clone()),
        logging_impl: LoggingImpl { actor_id: self.actor_id.clone() },
        messaging_impl: MessagingImpl::new(self.actor_id.clone(), instance_ctx.host_functions.clone()),
        // ... all other impls
    }
}

fn wire_component_linker(engine: &Engine) -> WasmResult<ComponentLinker<ComponentContext>> {
    let mut linker = ComponentLinker::new(engine);
    wasmtime_wasi::add_to_linker_async(&mut linker)?;
    add_plexspaces_host_to_linker(&mut linker)?;
    plexspaces::simple_actor::host::add_to_linker(&mut linker, |ctx| &mut ctx.simple_host_impl)?;
    Ok(linker)
}
```

---

## Phase 2: WIT Consistency & Extensions

### Step 2.1: Add Missing Host Functions to simple-actor WIT

Extend `wit/plexspaces-simple-actor/world.wit` host interface:

```wit
interface host {
    // === Existing ===
    send: func(to: string, msg-type: string, payload-json: string) -> string;
    log: func(level: string, message: string);
    now-ms: func() -> u64;
    // kv-*, ts-*, lock-*, blob-* (existing)

    // === New: Messaging ===
    /// Request-reply (blocks until response or timeout)
    ask: func(to: string, msg-type: string, payload-json: string, timeout-ms: u64) -> string;

    /// Get own actor ID
    self-id: func() -> string;

    /// Get parent/supervisor actor ID (empty if none)
    parent-id: func() -> string;

    // === New: Actor Lifecycle ===
    /// Spawn child actor
    spawn: func(module-ref: string, actor-id: string, init-config-json: string) -> string;

    /// Stop actor gracefully
    stop: func(actor-id: string, timeout-ms: u64) -> string;

    // === New: Supervision ===
    /// Link to another actor (bidirectional, Erlang-style)
    link: func(actor-id: string) -> string;
    unlink: func(actor-id: string) -> string;

    /// Monitor another actor (unidirectional)
    monitor: func(actor-id: string) -> string;
    demonitor: func(monitor-ref: string) -> string;

    // === New: Timers ===
    /// Schedule message to self after delay
    send-after: func(delay-ms: u64, msg-type: string, payload-json: string) -> string;
    cancel-timer: func(timer-id: string) -> string;

    // === New: Process Groups ===
    pg-join: func(group-name: string, topics-json: string) -> string;
    pg-leave: func(group-name: string) -> string;
    pg-members: func(group-name: string) -> string;
    pg-publish: func(group-name: string, topic: string, payload-json: string) -> string;
}
```

**Design rationale**: All new functions follow the existing simple-actor convention of JSON-string-in/JSON-string-out with "ERROR:" prefix for errors. This maintains compatibility with `componentize-py` and `jco componentize`.

### Step 2.2: Implement Host Bindings for New WIT Functions

In `crates/wasm-runtime/src/simple_component_host.rs`, implement each new function by delegating to the existing `HostFunctions` services:

- `ask` → `host_functions.ask_message()` (already exists in MessagingImpl for full actor world)
- `self_id` → return `self.actor_id.clone()`
- `spawn` → `host_functions.spawn_actor()` (needs new method on HostFunctions)
- `link`/`monitor` → delegate to actor system supervision
- `send_after` → delegate to timer service
- `pg_*` → delegate to `process_group_registry`

### Step 2.3: Add `context` to `messaging` Interface (Full Actor World)

Add `ctx: context` as first parameter to `tell`, `ask`, `reply`, `forward`, `spawn`, `stop`, `link`, `unlink`, `monitor`, `demonitor`, `send-after`, `cancel-timer` in `wit/plexspaces-actor/messaging.wit`.

This is a breaking change for full actor world but aligns with every other interface. No existing non-Rust WASM actors use the full actor world so impact is contained to Rust SDK.

**Note**: This must be coordinated with regenerating the component bindings (`generated/plexspaces_actor.rs`) and updating the host implementation in `component_host.rs`.

---

## Phase 3: SDK Parity

### Step 3.1: Python SDK — Add `ask`, `spawn`, `self_id`

In `sdks/python/plexspaces/host.py`:
```python
class PlexSpacesHost:
    def ask(self, to: str, msg_type: str, payload: dict, timeout_ms: int = 5000) -> dict:
        """Request-reply messaging (GenServer call pattern)"""
        result = _host.ask(to, msg_type, json.dumps(payload), timeout_ms)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])
        return json.loads(result) if result else {}

    def self_id(self) -> str:
        """Get this actor's ID"""
        return _host.self_id()

    def spawn(self, module_ref: str, actor_id: str, config: dict = None) -> str:
        """Spawn a child actor"""
        result = _host.spawn(module_ref, actor_id, json.dumps(config or {}))
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])
        return result
```

### Step 3.2: TypeScript SDK — Add Host Function Wrappers

Create `sdks/typescript/src/host.ts` wrapping all simple-actor host imports:

```typescript
import * as witHost from 'plexspaces:simple-actor/host@0.1.0';

export const host = {
  send(to: string, msgType: string, payload: object): void { ... },
  ask(to: string, msgType: string, payload: object, timeoutMs?: number): object { ... },
  selfId(): string { return witHost.selfId(); },
  kvGet(key: string): string | null { ... },
  kvPut(key: string, value: string): void { ... },
  tsWrite(tuple: any[]): void { ... },
  tsRead(pattern: any[]): any[] | null { ... },
  lockAcquire(tenantId: string, ns: string, holderId: string, name: string, leaseSecs: number, timeoutMs: number): object { ... },
  blobUpload(blobId: string, data: string, contentType: string): void { ... },
  pgJoin(group: string, topics?: string[]): void { ... },
  pgPublish(group: string, topic: string, payload: object): string[] { ... },
  // ... etc
};
```

### Step 3.3: TypeScript SDK — Add Behavior Type Classes

Extend the inheritance hierarchy to support OTP behaviors:

```typescript
// GenServer: typed request-reply
abstract class GenServerActor<TState> extends PlexSpacesActor<TState> {
  // Dispatches "call" messages to onCall<Op>() methods with expected reply
  // Dispatches "cast" messages to onCast<Op>() methods (fire-and-forget)
}

// EventActor: fire-and-forget event handler
abstract class EventActor<TState> extends PlexSpacesActor<TState> {
  // All handlers are fire-and-forget, no reply expected
}

// FsmActor: state machine with transitions
abstract class FsmActor<TState> extends PlexSpacesActor<TState> {
  abstract getInitialFsmState(): string;
  // onTransition<Event>() methods return new state name
}
```

### Step 3.4: Go SDK — New Implementation

Using TinyGo targeting `wasm32-wasip2` with the simple-actor world:

```go
package plexspaces

// BaseActor provides state management and message dispatch via reflection
type BaseActor struct {
    state    interface{}
    handlers map[string]HandlerFunc
}

// Actors embed BaseActor and define Handle<Op> methods
// Dispatch uses reflection to find methods matching "Handle" + op name
```

Build: `tinygo build -target=wasip2 -o actor.wasm ./...`

---

## Phase 4: Polyglot Examples

### Step 4.1: Port migrating_erlang_otp to Python + TypeScript

The Rust version demonstrates GenServer (counter), supervision trees, and link/monitor. The polyglot versions will use app-config.toml for supervisor tree definition and the respective SDKs.

### Step 4.2: Port migrating_restate to Python + TypeScript

Demonstrates durable execution with journaling. Uses `host.kv_put/kv_get` for side-effect caching in simple-actor world (since full durability WIT isn't in simple-actor).

### Step 4.3: Port migrating_temporal to Python + TypeScript

Demonstrates workflow orchestration pattern.

---

## Execution Order

**Principle**: Each step is one small commit with tests. Validate before moving to next.

1. **Step 1.1** — Test re-instantiation necessity (small test, high-impact finding)
2. **Step 1.2** — Fix state preservation via get_state/set_state cycle
3. **Step 1.3** — Deduplicate ComponentContext creation
4. **Step 2.1** — Add missing WIT functions to simple-actor host
5. **Step 2.2** — Implement host bindings for new WIT functions
6. **Step 3.1** — Python SDK: add `ask`, `spawn`, `self_id`
7. **Step 3.2** — TypeScript SDK: host function wrappers
8. **Step 3.3** — TypeScript SDK: behavior type classes
9. **Step 4.1** — First polyglot example (migrating_erlang_otp)
10. **Step 2.3** — Add context to messaging WIT (deferred, breaking change)
11. **Step 3.4** — Go SDK
12. **Step 4.2-4.3** — More polyglot examples
