# Message Routing Design

## Overview

This document describes the message routing design for the PlexSpaces actor system, covering `tell()` and `ask()` patterns for local and remote actors.

## Design Principles

1. **Location Transparency**: Actors communicate without knowing if the target is local or remote.
2. **Request-Reply Pattern**: `ask()` provides synchronous request-reply semantics over async messaging.
3. **Zero-allocation reply routing**: `ask()` uses a ULID-keyed oneshot channel in `PendingAsks`. No actor object, mailbox, or HashMap lock is created per ask.
4. **ActorRegistry as Routing Authority**: All routing logic is centralized in `ActorRegistry::tell()`/`ask()`/`ask_with_sender()`.
5. **Dynamic Locality**: Locality is determined dynamically by comparing node_id via `ActorId::is_on_node()`, not by ActorRef variant.
6. **Tenant ID Propagation**: `tenant_id` flows from API → JWT/header → `RequestContext` → all routing calls.
7. **Parallel Operations**: `ask_with_sender()` enables fanout: one caller supplies N distinct correlation IDs, each registered independently in `PendingAsks`.
8. **Scoped Lookup**: Runtime paths use `(tenant_id, namespace, actor_id)` and fail closed on ambiguous flat IDs.
9. **Link and monitor**: `ActorRegistry::{link, unlink, monitor, demonitor}` are location-transparent: local targets run `validate_link_monitor_operand_scope` then `ActorMonitor` updates; remote targets use `ActorService` gRPC only.

## gRPC spawn contract (`SpawnActorRequest`)

- **Single source of truth**: `SpawnActorRequest` carries `spec` (`ActorSpawnSpec` in `proto/plexspaces/v1/actors/actor_runtime.proto`) — identity, role, args, facets, labels, config, `visibility`, etc.
- **Namespace merge**: If `SpawnActorRequest.namespace` is non-empty, the server merges it into `spec.namespace` for that RPC only (override). Otherwise `spec.namespace` is used. For WASM deployments, namespace aligns with the application id / app namespace from `application-spec.toml`.
- **Tenant**: When authentication is enabled, the authoritative tenant is taken from JWT claims via `request_context_from_grpc_request` — same rule as all other ActorService RPCs. Use `RequestContext` from the gateway, not ad-hoc string formatting.
- **Replicas**: `instances_count > 1` on a single `SpawnActor` RPC is rejected; use `SpawnActors` (batch) for N identical instances.
- **Canonical IDs**: Construct and compare actors with `ActorId::new`, `ActorId::from_canonical`, and `to_string()` — do not concatenate `name//type::ns@node` by hand.

## `ActorVisibility` (tell / ask)

Spawn-time `ActorVisibility` on `ActorSpawnSpec` constrains who may tell and ask via `ActorRef`. Call sites pass `RequestContext` into `ActorRef::tell` / `ask`; enforcement uses `enforce_visibility_for_actor_ref_messaging`. `ActorRegistry::tell` / `ask` do not duplicate visibility checks; local delivery goes through registered senders which enforce on send.

## tell() Pattern

### Local Actor (Same Node)

```
Caller → ActorRef::tell() → ActorRegistry lookup → Local ActorRef → Actor runtime
```

1. `ActorRef::tell(ctx, message)` is called.
2. If local: route through the local actor ref directly.
3. If remote pointing to local: look up the scoped local actor ref in `ActorRegistry`.
4. Deliver through the local actor runtime.
5. Actor processes the message.

### Remote Actor (Different Node)

```
Caller → ActorRef::tell() → gRPC → Remote Node → ActorRegistry → Local ActorRef → Actor runtime
```

Uses gRPC for network transport; message serialization is handled automatically.

## ask() Pattern

### Overview

`ask()` provides request-reply semantics via `PendingAsks` — a DashMap of ULID-keyed oneshot channels. No actor object or mailbox is created per ask.

```
ask() call:
  1. Generate ULID correlation_id
  2. Build temporary-sender ActorId  (ActorId::temporary_sender)
  3. Set message.sender_id = temp_sender_id, message.correlation_id = correlation_id
  4. Register oneshot channel: PendingAsks::register(correlation_id, timeout) → Receiver<Message>
  5. dispatch_local_message(actor_id, message)
  6. tokio::time::timeout(timeout, rx.await)

Reply delivery:
  1. Actor calls ctx.send_reply(correlation_id, reply_to, from, reply_msg)
  2. reply_to is the temp_sender_id from the original message
  3. dispatch_local_message(temp_sender_id, reply_msg)
  4. Guard at top of dispatch_local_message: actor_id.is_temporary_sender() == true
  5. PendingAsks::resolve(correlation_id, reply_msg) → sends on oneshot channel
  6. ask() rx.await returns reply
```

**Tenant isolation**: Correlation IDs are ULIDs (128-bit globally unique). The isolation guarantee is structural — two asks never share a key regardless of tenant. There is no tenant-scoped composite key because `resolve()` is called with the *replying actor's* context, not the *caller's* context.

### Local Actor ask() Flow

```
Actor A → ActorRegistry::ask(ctx, actor_id, msg, timeout)
        → PendingAsks::register(corr_id)
        → dispatch_local_message(actor_id, msg)
        → Actor B runtime
              ↓ handle_request()
              ↓ ctx.send_reply(corr_id, temp_sender_id, ...)
              ↓ dispatch_local_message(temp_sender_id, reply)
              ↓ PendingAsks::resolve(corr_id, reply)  ← intercept before mailbox lookup
              ↓ oneshot channel → ask() returns reply
```

### Remote Actor ask() Flow

For remote actors, `ask()` delegates directly to gRPC `AskReply` — no `PendingAsks` entry is created. The gRPC transport handles the request-reply inline.

```
ActorRegistry::ask(ctx, remote_actor_id, msg, timeout)
  → actor_id.is_on_node(local_node_id) == false
  → ActorService::send_and_wait(ctx, actor_id, msg, Some(timeout))
  → gRPC AskReply → Remote Node → Actor B → gRPC reply
  → returns reply inline
```

### Fanout (ask_with_sender)

`ask_with_sender()` enables shard-group parallel asks: the caller supplies a pre-generated ULID correlation_id per shard, each registered independently in `PendingAsks`.

```rust
// In partition.rs shard fanout:
let futures: Vec<_> = shard_ids.iter().map(|shard_id| {
    let correlation_id = Ulid::new().to_string();
    registry.ask_with_sender(&ctx, shard_id, msg.clone(), timeout, correlation_id)
}).collect();
let results = join_all(futures).await;
```

No shared temporary actor object is created — each shard ask has its own oneshot channel entry.

## PendingAsks Implementation

```
DashMap<correlation_id: String, PendingAskEntry {
    tx: oneshot::Sender<Message>,
    expires_at: Instant,
}>
```

- 16 shards for concurrent writes under high load.
- Background GC task sweeps expired entries every 5 seconds.
- Metrics: `plexspaces_pending_asks_inflight` (gauge), `plexspaces_pending_asks_expired_total` (counter), `plexspaces_ask_errors_total` labeled by reason (`dispatch`, `timeout`, `channel_closed`).
- Configurable default timeout via `PLEXSPACES_ASK_TIMEOUT_MS` env var (default 30s).

## Temporary Sender ActorId

The temporary sender is a virtual routing identity — not a live actor. It is an `ActorId` with:
- `actor_type = "temporary_sender"`
- `name = "ask_{correlation_id}"`
- `node_id = local_node_id`
- `namespace = ctx.namespace()`

`ActorId::is_temporary_sender()` is used by `dispatch_local_message` to intercept replies before any mailbox lookup.

## Tenant ID Propagation

### Flow: API → JWT → RequestContext → routing

1. **API Layer**: Extracts `tenant_id` from JWT claims (auth enabled) or request namespace param.
2. **RequestContext**: Created with `tenant_id` and `namespace` from the gateway/gRPC metadata.
3. **ask()**: Passes `ctx` to all routing functions; `temp_sender_id` carries the namespace.
4. **Routing**: All routing functions require explicit `RequestContext`; no global state.

## ActorRegistry as Routing Authority

All routing logic is centralized in `ActorRegistry` (`crates/actor/src/actor_registry.rs`):

- **`ActorRegistry::tell(ctx, actor_id, message)`**: Fire-and-forget; local or remote via ActorService gRPC.
- **`ActorRegistry::ask(ctx, actor_id, message, timeout)`**: Request-reply; uses `PendingAsks` for local, gRPC AskReply for remote.
- **`ActorRegistry::ask_with_sender(ctx, actor_id, message, timeout, correlation_id)`**: Ask with caller-supplied ULID for fanout.
- **`ActorId::is_on_node(node_id)`**: Determines locality by comparing node_id suffix.

## Error Handling

- **Dispatch error**: `PendingAsks::cancel(corr_id)` called immediately; metrics counter incremented with `reason=dispatch`.
- **Timeout**: `PendingAsks::cancel(corr_id)` called; metrics counter with `reason=timeout`. Background GC also sweeps expired entries.
- **Channel closed**: Receiver dropped before reply arrived; metrics counter with `reason=channel_closed`.
- **Actor Not Found**: `ask()` returns `ActorRegistryError::ActorNotFound` immediately (before waiting).

## Message Flow Diagrams

### Local ask()

```
┌─────────────┐   ask(ctx, actor_id, msg)   ┌──────────────────┐
│  Caller     ├───────────────────────────►  │  ActorRegistry   │
│             │                              │  PendingAsks     │
│             │◄──────────────────────────── │  register(cid)   │
│             │   rx = oneshot channel       └────────┬─────────┘
│             │                                       │ dispatch_local_message
│             │                              ┌────────▼─────────┐
│             │                              │    Actor B       │
│             │                              │  handle_request  │
│             │                              └────────┬─────────┘
│             │                                       │ ctx.send_reply
│             │                              ┌────────▼─────────┐
│             │                              │  dispatch_local_ │
│             │                              │  message(temp_   │
│             │                              │  sender_id, rpl) │
│             │                              │  is_temp → true  │
│             │                              │  PendingAsks.    │
│             │                              │  resolve(cid)    │
│             │◄──────────────────────────── └──────────────────┘
│             │   rx.await → reply
└─────────────┘
```

### Remote ask()

```
Node1                              Node2
┌─────────┐                       ┌──────────────┐
│ Caller  │  gRPC AskReply        │  Actor B     │
│         ├──────────────────────►│              │
│         │                       │ handle_req() │
│         │                       │ send_reply() │
│         │◄──────────────────────│ gRPC reply   │
│ ask()   │                       └──────────────┘
│ returns │
└─────────┘
```

## Observability

All ask() calls include:
- `tracing::debug!` at dispatch and resolve points (gated).
- `plexspaces_pending_asks_inflight` gauge: current in-flight asks.
- `plexspaces_pending_asks_expired_total` counter: GC-swept entries.
- `plexspaces_ask_errors_total{reason}` counter: errors by reason (dispatch, timeout, channel_closed).

## References

- [Akka Ask Pattern](https://doc.akka.io/docs/akka/current/typed/interaction-patterns.html#request-response)
- [Erlang gen_server:call](https://www.erlang.org/doc/man/gen_server.html#call-2)
- [Orleans Request-Response](https://dotnet.github.io/orleans/docs/grains/communication/index.html)
