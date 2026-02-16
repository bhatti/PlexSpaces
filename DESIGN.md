# PlexSpaces WASM & SDK Design Document

## Implementation Status

| Item | Status | Notes |
|------|--------|-------|
| Re-instantiation fix (state preservation) | **Done** | get_state/set_state cycle in `handle_message_component()` |
| WIT handle() return type fix | **Done** | Reverted to `string` to match compiled components |
| WIT host extensions (12 functions) | **Done** | ask, self-id, spawn, stop, link, unlink, monitor, demonitor, send-after, pg-join, pg-leave, pg-members, pg-broadcast |
| Rust host bindings (SimpleHostImpl) | **Done** | All host functions implemented with metrics/tracing |
| Python SDK parity | **Done** | Host class + MockHost with all functions |
| TypeScript SDK parity | **Done** | host.ts with Host class + ProcessGroups |
| Go SDK foundation | **Done** | actor.go, host.go, host_imports.go (//go:wasmimport) |
| Test suite fixes | **Done** | Fixed max_fuel param in all test files |
| send-after JoinHandle tracking | **Done** | Tracked in `pending_timers` with self-cleanup |
| Bank account WASM validation | **Done** | Builds and validates with updated WIT |
| PlexspacesActor state preservation | Pending | Needs set-state in plexspaces-actor WIT |
| Polyglot examples | Pending | Port bank_account/migrating examples |

### Design Decisions

| Decision | Rationale |
|----------|-----------|
| **Removed `parent-id`** | Framework uses Erlang-style supervisor trees, not explicit parent/child tracking for actors. Supervision hierarchy is managed by `ActorRegistry.register_parent_child()` at the framework level, not exposed to individual WASM actors. |
| **Removed `cancel-timer`** | Timers are managed by the framework's `TimerFacet`/`ReminderFacet` (actor facets), not by individual actors. Actors can be stopped to cancel their pending timers. `TimerFacet::unregister_timer()` exists for framework-level timer management. |
| **`send-after` returns timer-id with tracked JoinHandle** | Timer tasks are stored in `SimpleHostImpl::pending_timers` for cleanup when the actor stops. Self-cleanup removes entries after delivery. Timer-ids are for observability, not cancellation. |
| **SDKs as thin decorators over framework** | WIT/SDK layer delegates to `HostFunctions` → `MessageSender` → framework services. No business logic in the SDK layer. |

## Table of Contents
1. [Architecture Overview](#1-architecture-overview)
2. [Re-instantiation Analysis & Fix](#2-re-instantiation-analysis--fix)
3. [WIT Extension Design](#3-wit-extension-design)
4. [SDK Parity Design](#4-sdk-parity-design)
5. [Behaviors & Facets in WASM](#5-behaviors--facets-in-wasm)
6. [Polyglot Example Strategy](#6-polyglot-example-strategy)
7. [Execution Plan](#7-execution-plan)

---

## 1. Architecture Overview

### WASM Runtime Stack

```
┌────────────────────────────────────────────────┐
│  SDK Layer (Python/TypeScript/Go/Rust)         │
│  @actor, @handler, PlexSpacesActor, BaseActor  │
├────────────────────────────────────────────────┤
│  WIT Interface (Contract)                      │
│  simple-actor (JSON) │ plexspaces-actor (typed)│
├────────────────────────────────────────────────┤
│  Host Bindings                                 │
│  SimpleHostImpl     │ ComponentHost            │
├────────────────────────────────────────────────┤
│  HostFunctions (Service Gateway)               │
│  MessageSender, KV, TupleSpace, Locks, etc.   │
├────────────────────────────────────────────────┤
│  Framework Services                            │
│  ActorFactory, ActorRef, ActorRegistry,        │
│  Supervisor, FacetManager, JournalStorage      │
└────────────────────────────────────────────────┘
```

### Key Insight: MessageSender Already Has Full API

The `MessageSender` trait for WASM (`crates/wasm-runtime/src/host_functions.rs`) already defines:

```rust
pub trait MessageSender: Send + Sync {
    async fn send_message(&self, from: &str, to: &str, message: &str) -> Result<(), String>;
    async fn ask(&self, from: &str, to: &str, message_type: &str, payload: Vec<u8>, timeout_ms: u64) -> Result<Vec<u8>, String>;
    async fn spawn_actor(&self, from: &str, module_ref: &str, initial_state: Vec<u8>, actor_id: Option<String>, labels: Vec<(String, String)>, durable: bool) -> Result<String, String>;
    async fn stop_actor(&self, from: &str, actor_id: &str, timeout_ms: u64) -> Result<(), String>;
    async fn link_actor(&self, from: &str, actor_id: &str, linked_actor_id: &str) -> Result<(), String>;
    async fn unlink_actor(&self, from: &str, actor_id: &str, linked_actor_id: &str) -> Result<(), String>;
    async fn monitor_actor(&self, from: &str, actor_id: &str) -> Result<u64, String>;
    async fn demonitor_actor(&self, from: &str, actor_id: &str, monitor_ref: u64) -> Result<(), String>;
}
```

And `ActorServiceMessageSender` (`crates/node/src/wasm_message_sender.rs`) implements all of these by
delegating to `ActorService`, `ActorFactory`, and `ActorRef`. The `HostFunctions` struct holds an
`Option<Arc<dyn MessageSender>>` and delegates to it.

**This means**: The host-side plumbing already exists. We only need to:
1. Add the WIT function signatures to `simple-actor/world.wit`
2. Wire them in `SimpleHostImpl` to delegate to `HostFunctions`/`MessageSender`
3. Add SDK wrappers in Python/TypeScript/Go

---

## 2. Re-instantiation Analysis & Fix

### Current Problem

After every `handle()` call, `instance.rs` creates a fresh `Store<ComponentContext>` + WASM instance:

```
handle() → [state changes] → drop Store → new Store → init(original_config) → [state LOST]
```

The code comments reference "wasmtime#8943" but the actual underlying issue is the Component Model's
re-entrancy guard (`may_enter` flag). The component spec says:
- **Re-entrant calls**: Always trap (guest→host→guest callback while first call active)
- **Sequential calls**: Should work (first call completes fully, then second call)

### Investigation Plan

**Step 1**: Write integration test calling `handle()` twice on the same SimpleActor instance:

```rust
#[tokio::test]
async fn test_sequential_handle_calls_without_reinstantiation() {
    // 1. Load a Python-built .wasm (e.g., calculator_actor.wasm from test fixtures)
    // 2. Instantiate SimpleActor component
    // 3. Call init()
    // 4. Call handle("", "call", '{"op":"add","a":1,"b":2}')
    // 5. Call handle("", "call", '{"op":"add","a":3,"b":4}') on SAME store
    // 6. If step 5 succeeds: sequential calls work, remove re-instantiation
    // 7. If step 5 traps: document the exact error
}
```

**Step 2 (if re-instantiation needed)**: Fix state preservation:

```
handle() → get_state() → drop Store → new Store → init(config) → set_state(saved) → [state PRESERVED]
```

Implementation in `instance.rs`:

```rust
// In handle_message_component(), SimpleActor path, after handle() succeeds:
let saved_state = simple_bindings.plexspaces_simple_actor_actor()
    .call_get_state(&mut *store)
    .await
    .unwrap_or_default();

let instance_ctx = store.data().instance_ctx.clone();
drop(state);

// Pass saved_state to re-instantiation
match self.create_fresh_simple_actor_state(&instance_ctx, Some(&saved_state)).await {
    Ok(new_state) => { /* replace */ }
    Err(e) => { /* log warning */ }
}
```

In `create_fresh_simple_actor_state()`:

```rust
async fn create_fresh_simple_actor_state(
    &self,
    instance_ctx: &InstanceContext,
    restored_state: Option<&str>,  // NEW parameter
) -> WasmResult<ComponentState> {
    // ... create ComponentContext, Store, linker, bindings (same as now) ...

    // Always call init() first (sets up internal structure)
    let init_config = self.original_init_config.as_deref().unwrap_or("");
    let result = simple_bindings.plexspaces_simple_actor_actor()
        .call_init(&mut component_store, init_config)
        .await?;

    // If we have saved state, restore it via set_state()
    if let Some(state_json) = restored_state {
        if !state_json.is_empty() && state_json != "{}" {
            let set_result = simple_bindings.plexspaces_simple_actor_actor()
                .call_set_state(&mut component_store, state_json)
                .await
                .map_err(|e| WasmError::ActorFunctionError(
                    format!("set_state() after re-instantiation failed: {}", e)
                ))?;
            if !set_result.is_empty() {
                tracing::warn!(actor_id = %self.actor_id, error = %set_result,
                    "set_state() returned error during re-instantiation");
            }
        }
    }

    Ok(ComponentState { store: component_store, bindings: ComponentBindings::SimpleActor(simple_bindings) })
}
```

Same pattern for PlexspacesActor using `call_snapshot_state()` / `call_init(state_bytes)`.

### Deduplication

Extract shared helper for ComponentContext creation (both SimpleActor and PlexspacesActor paths
construct identical ComponentContext):

```rust
fn create_component_context(&self, instance_ctx: &InstanceContext) -> ComponentContext {
    let tuplespace_provider = self.tuplespace_provider.clone();
    ComponentContext {
        instance_ctx: instance_ctx.clone(),
        wasi_ctx: Self::build_wasi_context(),
        resource_table: wasmtime_wasi::ResourceTable::new(),
        plexspaces_host: PlexspacesHost::new(self.actor_id.clone(), instance_ctx.host_functions.clone()),
        logging_impl: LoggingImpl { actor_id: self.actor_id.clone() },
        messaging_impl: MessagingImpl::new(self.actor_id.clone(), instance_ctx.host_functions.clone()),
        tuplespace_impl: TuplespaceImpl::new(tuplespace_provider.clone(), self.actor_id.clone()),
        channels_impl: ChannelsImpl::new(instance_ctx.host_functions.clone()),
        durability_impl: DurabilityImpl::new(self.actor_id.clone(), instance_ctx.host_functions.clone()),
        workflow_impl: WorkflowImpl,
        blob_impl: BlobImpl { actor_id: self.actor_id.clone(), host_functions: instance_ctx.host_functions.clone() },
        keyvalue_impl: KeyValueImpl { actor_id: self.actor_id.clone(), host_functions: instance_ctx.host_functions.clone() },
        process_groups_impl: ProcessGroupsImpl { actor_id: self.actor_id.clone(), host_functions: instance_ctx.host_functions.clone() },
        locks_impl: LocksImpl { actor_id: self.actor_id.clone(), host_functions: instance_ctx.host_functions.clone() },
        registry_impl: RegistryImpl { actor_id: self.actor_id.clone(), host_functions: instance_ctx.host_functions.clone() },
        simple_host_impl: SimpleHostImpl::new(self.actor_id.clone(), instance_ctx.host_functions.clone(), tuplespace_provider),
    }
}

fn build_wasi_context() -> wasmtime_wasi::WasiCtx {
    wasmtime_wasi::WasiCtxBuilder::new()
        .inherit_stdio()
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUNBUFFERED", "1")
        .env("HOME", "/")
        .env("PATH", "/")
        .build()
}

async fn wire_component_linker(engine: &Engine) -> WasmResult<ComponentLinker<ComponentContext>> {
    let mut linker = ComponentLinker::new(engine);
    wasmtime_wasi::add_to_linker_async(&mut linker)
        .map_err(|e| WasmError::InstantiationError(format!("WASI: {}", e)))?;
    crate::component_host::add_plexspaces_host_to_linker(&mut linker)
        .map_err(|e| WasmError::InstantiationError(format!("PlexSpaces host: {}", e)))?;
    crate::simple_component_host::plexspaces::simple_actor::host::add_to_linker(
        &mut linker, |ctx: &mut ComponentContext| &mut ctx.simple_host_impl,
    ).map_err(|e| WasmError::InstantiationError(format!("SimpleActor host: {}", e)))?;
    Ok(linker)
}
```

---

## 3. WIT Extension Design

### 3.1 New simple-actor Host Functions

All new functions follow the existing convention: JSON-string-in/JSON-string-out,
empty string = success, "ERROR:" prefix = error.

```wit
// wit/plexspaces-simple-actor/world.wit — additions to host interface

interface host {
    // ========== Existing (unchanged) ==========
    send: func(to: string, msg-type: string, payload-json: string) -> string;
    log: func(level: string, message: string);
    now-ms: func() -> u64;
    kv-get: func(key: string) -> string;
    kv-put: func(key: string, value: string) -> string;
    kv-delete: func(key: string) -> string;
    kv-list: func(prefix: string) -> string;
    ts-write: func(tuple-json: string) -> string;
    ts-read: func(pattern-json: string) -> string;
    ts-take: func(pattern-json: string) -> string;
    ts-read-all: func(pattern-json: string) -> string;
    lock-acquire: func(tenant-id: string, namespace: string, holder-id: string,
                       lock-name: string, lease-duration-secs: u32, timeout-ms: u64) -> string;
    lock-release: func(lock-id: string, tenant-id: string, namespace: string,
                       holder-id: string, lock-version: string) -> string;
    lock-renew: func(lock-id: string, tenant-id: string, namespace: string,
                     holder-id: string, lock-version: string, lease-duration-secs: u32) -> string;
    blob-upload: func(blob-id: string, data: string, content-type: string) -> string;
    blob-download: func(blob-id: string) -> string;
    blob-delete: func(blob-id: string) -> string;
    blob-list: func(prefix: string) -> string;

    // ========== New: Messaging ==========

    /// Request-reply (GenServer call pattern).
    /// Blocks until response or timeout. Returns JSON response or "ERROR:...".
    /// Delegates to ActorRef::ask() via ActorServiceMessageSender.
    ask: func(to: string, msg-type: string, payload-json: string, timeout-ms: u64) -> string;

    /// Get own actor ID. Returns actor ID string (e.g., "account-alice@node-1").
    /// Trivial: returns SimpleHostImpl.actor_id.
    self-id: func() -> string;

    // ========== New: Actor Lifecycle ==========

    /// Spawn a child actor from a WASM module.
    /// module-ref: Module reference (name@version or hash, must be deployed)
    /// actor-id: Desired actor ID (empty = auto-generated ULID)
    /// init-config-json: JSON config passed to new actor's init()
    /// Returns new actor ID or "ERROR:...".
    /// Delegates to ActorServiceMessageSender::spawn_actor() → ActorFactory::spawn_actor().
    spawn: func(module-ref: string, actor-id: string, init-config-json: string) -> string;

    /// Stop actor gracefully. Delegates to ActorFactory::stop_actor().
    /// Returns empty on success, "ERROR:..." on failure.
    stop: func(actor-id: string, timeout-ms: u64) -> string;

    // ========== New: Supervision (Erlang-style) ==========

    /// Bidirectional link. If linked actor dies, this actor receives EXIT.
    /// Delegates to ActorServiceMessageSender::link_actor().
    link: func(actor-id: string) -> string;
    unlink: func(actor-id: string) -> string;

    /// Unidirectional monitor. If monitored actor dies, this actor receives DOWN.
    /// Returns monitor reference (as string number) or "ERROR:...".
    /// Delegates to ActorServiceMessageSender::monitor_actor().
    monitor: func(actor-id: string) -> string;
    demonitor: func(monitor-ref: string) -> string;

    // ========== New: Timers ==========

    /// Schedule message to self after delay. Returns timer ID (string) or "ERROR:...".
    /// Timer cancellation is managed by the framework's TimerFacet/ReminderFacet.
    send-after: func(delay-ms: u64, msg-type: string, payload-json: string) -> string;

    // ========== New: Process Groups (Erlang pg2-style) ==========

    /// Join a named group. topics-json: JSON array of topics (empty = all).
    pg-join: func(group-name: string, topics-json: string) -> string;
    pg-leave: func(group-name: string) -> string;
    /// Returns JSON array of actor IDs in group.
    pg-members: func(group-name: string) -> string;
    /// Publish message to all group members. Returns JSON array of recipient actor IDs.
    pg-publish: func(group-name: string, topic: string, payload-json: string) -> string;
}
```

### 3.2 Host Binding Implementation

Each new function in `SimpleHostImpl` follows the exact same pattern as existing `send()`:

```rust
// ask — request-reply
async fn ask(&mut self, to: String, msg_type: String, payload_json: String, timeout_ms: u64) -> String {
    let sender = match &self.host_functions.message_sender {
        Some(s) => s,
        None => return "ERROR:MessageSender not available".to_string(),
    };
    match sender.ask(&self.actor_id, &to, &msg_type, payload_json.into_bytes(), timeout_ms).await {
        Ok(response_bytes) => String::from_utf8_lossy(&response_bytes).to_string(),
        Err(e) => format!("ERROR:{}", e),
    }
}

// self-id — trivial, returns ActorId assigned during registration
fn self_id(&mut self) -> String {
    self.actor_id.clone()
}

// spawn — delegates to MessageSender::spawn_actor
async fn spawn(&mut self, module_ref: String, actor_id: String, init_config_json: String) -> String {
    let sender = match &self.host_functions.message_sender {
        Some(s) => s,
        None => return "ERROR:MessageSender not available".to_string(),
    };
    let id = if actor_id.is_empty() { None } else { Some(actor_id) };
    match sender.spawn_actor(&self.actor_id, &module_ref, init_config_json.into_bytes(), id, vec![], false).await {
        Ok(new_id) => new_id,
        Err(e) => format!("ERROR:{}", e),
    }
}

// stop — delegates to MessageSender::stop_actor
async fn stop(&mut self, actor_id: String, timeout_ms: u64) -> String {
    let sender = match &self.host_functions.message_sender {
        Some(s) => s,
        None => return "ERROR:MessageSender not available".to_string(),
    };
    match sender.stop_actor(&self.actor_id, &actor_id, timeout_ms).await {
        Ok(()) => String::new(),
        Err(e) => format!("ERROR:{}", e),
    }
}

// link/unlink — delegates to MessageSender::link_actor/unlink_actor
async fn link(&mut self, actor_id: String) -> String {
    let sender = match &self.host_functions.message_sender {
        Some(s) => s,
        None => return "ERROR:MessageSender not available".to_string(),
    };
    match sender.link_actor(&self.actor_id, &self.actor_id, &actor_id).await {
        Ok(()) => String::new(),
        Err(e) => format!("ERROR:{}", e),
    }
}

// monitor — delegates to MessageSender::monitor_actor, returns ref as string
async fn monitor(&mut self, actor_id: String) -> String {
    let sender = match &self.host_functions.message_sender {
        Some(s) => s,
        None => return "ERROR:MessageSender not available".to_string(),
    };
    match sender.monitor_actor(&self.actor_id, &actor_id).await {
        Ok(ref_id) => ref_id.to_string(),
        Err(e) => format!("ERROR:{}", e),
    }
}

// pg-join — delegates to ProcessGroupRegistry
async fn pg_join(&mut self, group_name: String, topics_json: String) -> String {
    let pg = match &self.host_functions.process_group_registry {
        Some(pg) => pg,
        None => return "ERROR:ProcessGroupRegistry not available".to_string(),
    };
    let topics: Vec<String> = serde_json::from_str(&topics_json).unwrap_or_default();
    match pg.join(&group_name, &self.actor_id, &topics).await {
        Ok(()) => String::new(),
        Err(e) => format!("ERROR:{}", e),
    }
}

// send-after — deferred self-send via tokio delay
async fn send_after(&mut self, delay_ms: u64, msg_type: String, payload_json: String) -> String {
    let host_functions = self.host_functions.clone();
    let from = self.actor_id.clone();
    let timer_id = ulid::Ulid::new().to_string();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        let _ = host_functions.send_message(&from, &from, &payload_json).await;
    });
    timer_id
}
```

### 3.3 Add `context` to `messaging` Interface (Full Actor World)

Since backward compatibility is not required, add `ctx: context` as first parameter to all
functions in `wit/plexspaces-actor/messaging.wit`:

```wit
interface messaging {
    use types.{actor-id, payload, message-id, correlation-id, duration-ms, spawn-options, actor-error, context};

    tell: func(ctx: context, to: actor-id, msg-type: string, payload: payload) -> result<message-id, actor-error>;
    ask: func(ctx: context, to: actor-id, msg-type: string, payload: payload, timeout-ms: duration-ms) -> result<payload, actor-error>;
    reply: func(ctx: context, correlation-id: correlation-id, payload: payload) -> result<_, actor-error>;
    forward: func(ctx: context, to: actor-id, msg-type: string, payload: payload, original-sender: actor-id, correlation-id: option<correlation-id>) -> result<message-id, actor-error>;
    spawn: func(ctx: context, module-ref: string, initial-state: payload, options: spawn-options) -> result<actor-id, actor-error>;
    stop: func(ctx: context, actor-id: actor-id, timeout-ms: duration-ms) -> result<_, actor-error>;
    link: func(ctx: context, actor-id: actor-id) -> result<_, actor-error>;
    unlink: func(ctx: context, actor-id: actor-id) -> result<_, actor-error>;
    monitor: func(ctx: context, actor-id: actor-id) -> result<u64, actor-error>;
    demonitor: func(ctx: context, monitor-ref: u64) -> result<_, actor-error>;
    self-id: func() -> actor-id;
    parent-id: func() -> option<actor-id>;
    now: func() -> u64;
    sleep: func(duration-ms: duration-ms);
    send-after: func(ctx: context, delay-ms: duration-ms, msg-type: string, payload: payload) -> result<u64, actor-error>;
    cancel-timer: func(ctx: context, timer-id: u64) -> result<_, actor-error>;
}
```

Note: `self-id`, `now`, `sleep` don't need context (they're actor-intrinsic).
Note: `parent-id` and `cancel-timer` exist in the typed plexspaces-actor WIT (for Rust)
but are **not exposed** in the simple-actor WIT (for Python/TypeScript/Go) because:
- Parent/child hierarchy is managed by the framework's supervisor tree, not by individual actors.
- Timer cancellation is managed by TimerFacet/ReminderFacet (actor facets), not by the WIT host API.

---

## 4. SDK Parity Design

### 4.1 Python SDK Additions

**File: `sdks/python/plexspaces/host.py`**

Currently has: `send`, `log`, `now_ms`, `kv_*`, `ts_*`, `lock_*`, `blob_*`, `process_groups.*`

Add:

```python
class Host:
    # === New Messaging ===
    def ask(self, to: str, msg_type: str, payload: Any, timeout_ms: int = 5000) -> Any:
        """Request-reply (GenServer call pattern).
        Sends message and blocks until reply or timeout.
        Returns parsed JSON response.
        Raises PlexSpacesError on failure."""
        payload_json = json.dumps(payload) if not isinstance(payload, str) else payload
        result = _host.ask(to, msg_type, payload_json, timeout_ms)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])
        return json.loads(result) if result else None

    def self_id(self) -> str:
        """Get this actor's ID (e.g., 'account-alice@node-1')."""
        return _host.self_id()

    # === Actor Lifecycle ===
    def spawn(self, module_ref: str, actor_id: str = "", config: dict = None) -> str:
        """Spawn a child actor.
        module_ref: Deployed module (name@version or hash)
        actor_id: Desired ID (empty = auto-generated)
        config: JSON config for new actor's init()
        Returns new actor ID."""
        result = _host.spawn(module_ref, actor_id, json.dumps(config or {}))
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])
        return result

    def stop(self, actor_id: str, timeout_ms: int = 5000) -> None:
        """Stop actor gracefully."""
        result = _host.stop(actor_id, timeout_ms)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])

    # === New Supervision ===
    def link(self, actor_id: str) -> None:
        """Bidirectional link (Erlang-style). If linked actor dies, this actor gets EXIT."""
        result = _host.link(actor_id)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])

    def unlink(self, actor_id: str) -> None:
        result = _host.unlink(actor_id)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])

    def monitor(self, actor_id: str) -> str:
        """Unidirectional monitor. Returns monitor reference."""
        result = _host.monitor(actor_id)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])
        return result

    def demonitor(self, monitor_ref: str) -> None:
        result = _host.demonitor(monitor_ref)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])

    # === New Timers ===
    def send_after(self, delay_ms: int, msg_type: str, payload: Any) -> str:
        """Schedule message to self after delay. Returns timer ID."""
        payload_json = json.dumps(payload) if not isinstance(payload, str) else payload
        result = _host.send_after(delay_ms, msg_type, payload_json)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])
        return result

    # === Process Groups ===
    def pg_join(self, group: str, topics: list = None) -> None:
        result = _host.pg_join(group, json.dumps(topics or []))
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])

    def pg_leave(self, group: str) -> None:
        result = _host.pg_leave(group)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])

    def pg_members(self, group: str) -> list:
        result = _host.pg_members(group)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])
        return json.loads(result) if result else []

    def pg_publish(self, group: str, topic: str, payload: Any) -> list:
        payload_json = json.dumps(payload) if not isinstance(payload, str) else payload
        result = _host.pg_publish(group, topic, payload_json)
        if result.startswith("ERROR:"):
            raise PlexSpacesError(result[6:])
        return json.loads(result) if result else []
```

### 4.2 TypeScript SDK Additions

**File: `sdks/typescript/src/host.ts`** (new file)

```typescript
// Import WIT-generated host bindings
// jco generates these from plexspaces:simple-actor/host
declare const hostBindings: {
  send(to: string, msgType: string, payloadJson: string): string;
  ask(to: string, msgType: string, payloadJson: string, timeoutMs: bigint): string;
  selfId(): string;
  spawn(moduleRef: string, actorId: string, initConfigJson: string): string;
  stop(actorId: string): string;
  link(actorId: string): string;
  unlink(actorId: string): string;
  monitor(actorId: string): string;
  demonitor(monitorRef: string): string;
  sendAfter(delayMs: bigint, msgType: string, payloadJson: string): string;
  log(level: string, message: string): void;
  nowMs(): bigint;
  kvGet(key: string): string;
  kvPut(key: string, value: string): string;
  kvDelete(key: string): string;
  kvList(prefix: string): string;
  tsWrite(tupleJson: string): string;
  tsRead(patternJson: string): string;
  tsTake(patternJson: string): string;
  tsReadAll(patternJson: string): string;
  lockAcquire(tenantId: string, ns: string, holderId: string, name: string, leaseSecs: number, timeoutMs: bigint): string;
  lockRelease(lockId: string, tenantId: string, ns: string, holderId: string, version: string): string;
  lockRenew(lockId: string, tenantId: string, ns: string, holderId: string, version: string, leaseSecs: number): string;
  blobUpload(blobId: string, data: string, contentType: string): string;
  blobDownload(blobId: string): string;
  blobDelete(blobId: string): string;
  blobList(prefix: string): string;
  pgJoin(group: string, topicsJson: string): string;
  pgLeave(group: string): string;
  pgMembers(group: string): string;
  pgPublish(group: string, topic: string, payloadJson: string): string;
};

function checkError(result: string): string {
  if (result.startsWith('ERROR:')) {
    throw new Error(result.substring(6));
  }
  return result;
}

function parseJson(result: string): unknown {
  if (!result || result === '') return null;
  return JSON.parse(result);
}

export const host = {
  // --- Messaging ---
  send(to: string, msgType: string, payload: unknown): void {
    checkError(hostBindings.send(to, msgType, JSON.stringify(payload)));
  },
  ask(to: string, msgType: string, payload: unknown, timeoutMs = 5000): unknown {
    const result = checkError(hostBindings.ask(to, msgType, JSON.stringify(payload), BigInt(timeoutMs)));
    return parseJson(result);
  },
  selfId(): string { return hostBindings.selfId(); },

  // --- Lifecycle ---
  spawn(moduleRef: string, actorId = '', config: Record<string, unknown> = {}): string {
    return checkError(hostBindings.spawn(moduleRef, actorId, JSON.stringify(config)));
  },
  stop(actorId: string, timeoutMs = 5000): void {
    checkError(hostBindings.stop(actorId, BigInt(timeoutMs)));
  },

  // --- Supervision ---
  link(actorId: string): void { checkError(hostBindings.link(actorId)); },
  unlink(actorId: string): void { checkError(hostBindings.unlink(actorId)); },
  monitor(actorId: string): string { return checkError(hostBindings.monitor(actorId)); },
  demonitor(monitorRef: string): void { checkError(hostBindings.demonitor(monitorRef)); },

  // --- Timers ---
  sendAfter(delayMs: number, msgType: string, payload: unknown): string {
    return checkError(hostBindings.sendAfter(BigInt(delayMs), msgType, JSON.stringify(payload)));
  },
  // --- Logging ---
  log(level: string, message: string): void { hostBindings.log(level, message); },
  debug(message: string): void { hostBindings.log('debug', message); },
  info(message: string): void { hostBindings.log('info', message); },
  warn(message: string): void { hostBindings.log('warn', message); },
  error(message: string): void { hostBindings.log('error', message); },
  nowMs(): number { return Number(hostBindings.nowMs()); },

  // --- Key-Value ---
  kvGet(key: string): string | null { const r = hostBindings.kvGet(key); return r === '' ? null : checkError(r); },
  kvPut(key: string, value: string): void { checkError(hostBindings.kvPut(key, value)); },
  kvDelete(key: string): void { checkError(hostBindings.kvDelete(key)); },
  kvList(prefix: string): string[] { return JSON.parse(checkError(hostBindings.kvList(prefix))); },

  // --- TupleSpace ---
  tsWrite(tuple: unknown[]): void { checkError(hostBindings.tsWrite(JSON.stringify(tuple))); },
  tsRead(pattern: unknown[]): unknown[] | null { const r = hostBindings.tsRead(JSON.stringify(pattern)); return r === '' ? null : JSON.parse(checkError(r)); },
  tsTake(pattern: unknown[]): unknown[] | null { const r = hostBindings.tsTake(JSON.stringify(pattern)); return r === '' ? null : JSON.parse(checkError(r)); },
  tsReadAll(pattern: unknown[]): unknown[][] { const r = hostBindings.tsReadAll(JSON.stringify(pattern)); return r === '' ? [] : JSON.parse(checkError(r)); },

  // --- Locks ---
  lockAcquire(tenantId: string, ns: string, holderId: string, name: string, leaseSecs: number, timeoutMs: number): Record<string, unknown> {
    return JSON.parse(checkError(hostBindings.lockAcquire(tenantId, ns, holderId, name, leaseSecs, BigInt(timeoutMs))));
  },
  lockRelease(lockId: string, tenantId: string, ns: string, holderId: string, version: string): void {
    checkError(hostBindings.lockRelease(lockId, tenantId, ns, holderId, version));
  },
  lockRenew(lockId: string, tenantId: string, ns: string, holderId: string, version: string, leaseSecs: number): string {
    return checkError(hostBindings.lockRenew(lockId, tenantId, ns, holderId, version, leaseSecs));
  },

  // --- Blob ---
  blobUpload(blobId: string, data: string, contentType: string): void { checkError(hostBindings.blobUpload(blobId, data, contentType)); },
  blobDownload(blobId: string): string | null { const r = hostBindings.blobDownload(blobId); return r === '' ? null : checkError(r); },
  blobDelete(blobId: string): void { checkError(hostBindings.blobDelete(blobId)); },
  blobList(prefix: string): string[] { return JSON.parse(checkError(hostBindings.blobList(prefix))); },

  // --- Process Groups ---
  pgJoin(group: string, topics: string[] = []): void { checkError(hostBindings.pgJoin(group, JSON.stringify(topics))); },
  pgLeave(group: string): void { checkError(hostBindings.pgLeave(group)); },
  pgMembers(group: string): string[] { return JSON.parse(checkError(hostBindings.pgMembers(group))); },
  pgPublish(group: string, topic: string, payload: unknown): string[] {
    return JSON.parse(checkError(hostBindings.pgPublish(group, topic, JSON.stringify(payload))));
  },
};
```

### 4.3 Go SDK Design

**Architecture**: Struct embedding (Go's composition pattern) + Handle<Op> method dispatch via reflection.

```
sdks/go/
├── plexspaces/
│   ├── actor.go       # BaseActor, dispatch, WIT export glue
│   ├── host.go        # Host function wrappers (send, ask, kv, ts, etc.)
│   └── state.go       # State management (JSON marshal/unmarshal)
├── examples/
│   └── bank_account/
│       └── main.go    # Bank account actor
├── go.mod
└── README.md
```

```go
package plexspaces

// BaseActor provides state management and handler dispatch.
// Actors embed BaseActor and define Handle<Op>(payload) methods.
type BaseActor struct {
    state     interface{}     // User-defined state struct
    self      interface{}     // Reference to the embedding struct (for reflection)
    initFn    func(config map[string]interface{})
}

// Init initializes the actor. Called by WIT init export.
func (a *BaseActor) Init(configJson string) string { ... }

// Handle dispatches to Handle<Op> methods via reflection.
// E.g., payload.op="deposit" → actor.HandleDeposit(payload)
func (a *BaseActor) Handle(from, msgType, payloadJson string) (string, error) { ... }

// GetState serializes state to JSON.
func (a *BaseActor) GetState() string { ... }

// SetState restores state from JSON.
func (a *BaseActor) SetState(stateJson string) string { ... }

// --- Example usage ---
type BankAccountState struct {
    AccountID    string        `json:"account_id"`
    Balance      int           `json:"balance"`
    Transactions []Transaction `json:"transactions"`
}

type BankAccountActor struct {
    plexspaces.BaseActor
}

func NewBankAccountActor() *BankAccountActor {
    a := &BankAccountActor{}
    a.BaseActor = plexspaces.NewBaseActor(a, &BankAccountState{})
    return a
}

func (a *BankAccountActor) HandleDeposit(payload map[string]interface{}) (interface{}, error) {
    amount := int(payload["amount"].(float64))
    state := a.State().(*BankAccountState)
    state.Balance += amount
    return map[string]interface{}{"status": "ok", "balance": state.Balance}, nil
}
```

Build with TinyGo: `tinygo build -target=wasip2 -o bank_account.wasm .`

---

## 5. Behaviors & Facets in WASM

### How Behaviors Work for WASM Actors

Behaviors are configured in `app-config.toml` via `behavior_kind` on each child spec:

```toml
[[supervisor.children]]
id = "counter"
type = "worker"
behavior_kind = "GenServer"      # Routes "call" → handle (expect reply)
```

The `WasmActorBehavior` in `wasm_application.rs` stores `behavior_kind` from the config. The
runtime then uses `behavior_kind` for:
1. **Logging**: Spans show `behavior=GenServer` instead of actor ID
2. **Message routing**: For PlexspacesActor world, "call"→`handle_call()`, "cast"→`handle_cast()`
3. **Dashboard display**: Groups actors by behavior type

For the simple-actor world, ALL messages go to `handle()`. The SDK (Python/TypeScript/Go) does its
own dispatch based on `msg_type` → `@handler("op")` / `on<Op>()` / `Handle<Op>()`.

### How Facets Work for WASM Actors

Facets are configured in `app-config.toml` per child:

```toml
[[supervisor.children]]
id = "account-alice"
type = "worker"
restart = "permanent"
facets = [
    { type = "durability", priority = 100, config = {} },
    { type = "virtual_actor", priority = 90, config = { idle_timeout = "5m" } },
    { type = "timer", priority = 80, config = { interval = "30s" } },
    { type = "reminder", priority = 70, config = { database_url = "sqlite://reminders.db" } },
]
```

The framework:
1. Creates facet instances from config during `create_wasm_actor_child_spec()`
2. Attaches them to the actor after spawn via `FacetContainer`
3. Facets intercept `before_method`/`after_method` on handle_message
4. DurabilityFacet: Saves checkpoint (calls `get_state()`) on configurable intervals
5. VirtualActorFacet: Deactivates idle actors, reactivates on message
6. TimerFacet: Schedules periodic messages to the actor

### Durability in WASM

For the **simple-actor world** (Python/TypeScript/Go), durability works via:
1. DurabilityFacet configured in `app-config.toml`
2. Framework periodically calls `get_state()` on the WASM instance → saves to checkpoint store
3. On restart, framework calls `init(config)` then `set_state(checkpoint)` to restore
4. **This is exactly the same flow we need for re-instantiation state preservation**

For the **plexspaces-actor world** (Rust), durability uses the full `durability` WIT interface
with `persist()`, `checkpoint()`, `cache_side_effect()`, `is_replaying()` etc.

---

## 6. Polyglot Example Strategy

### Start with bank_account (Validate Re-instantiation Fix)

Use `examples/python/apps/bank_account/` as the test bed for re-instantiation:
1. Deploy bank_account actor
2. Deposit 100 → should return balance=100
3. Deposit 200 → should return balance=300 (proves state preserved across calls)
4. Get balance → should return 300

Then create equivalent:
- `examples/typescript/apps/bank_account/` (already exists, verify it works)
- `examples/go/apps/bank_account/` (new)

### Then Port Migrating Examples

Priority order (simpler → complex):
1. `migrating_erlang_otp` — GenServer counter with supervision (basic OTP)
2. `migrating_restate` — Durable payment processing (durability facet)
3. `migrating_temporal` — Order workflow (workflow behavior)
4. `migrating_ray` — Python already exists, add TypeScript + Go
5. `migrating_orleans` — TypeScript already exists, add Python + Go

---

## 7. Execution Plan

Each step = one small commit with integration test + validation.

| # | Step | Deliverable | Test |
|---|------|-------------|------|
| 1 | Test re-instantiation necessity | Integration test in wasm-runtime | `test_sequential_handle_calls` |
| 2 | Fix state preservation OR remove re-instantiation | Changes to `instance.rs` | Bank account: deposit+deposit=sum |
| 3 | Deduplicate ComponentContext creation | Refactor `instance.rs` | Existing tests pass |
| 4 | Add new WIT functions to simple-actor | `world.wit` changes | Compilation check |
| 5 | Implement host bindings | `simple_component_host.rs` | Unit tests per function |
| 6 | Regenerate component bindings | `generated/plexspaces_actor.rs` | Build passes |
| 7 | Python SDK: add ask, spawn, self_id, etc. | `host.py` additions | Python example tests |
| 8 | TypeScript SDK: add host.ts | New `host.ts` file | TS example tests |
| 9 | TypeScript bank_account: verify parity | Verify existing example | deploy+test |
| 10 | Go SDK: BaseActor + host | New `sdks/go/` | Go bank_account |
| 11 | Add context to messaging WIT | `messaging.wit` + component_host | Build passes |
| 12 | Port migrating_erlang_otp | Python + TS + Go versions | deploy+test each |
| 13 | Port migrating_restate | Python + TS + Go versions | durability test |
| 14 | Port migrating_temporal | Python + TS + Go versions | workflow test |
