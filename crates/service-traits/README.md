# service-traits — Cross-Crate Trait Definitions

**Purpose**: Canonical home for all cross-crate traits in PlexSpaces. Separating trait definitions from their implementations breaks circular dependencies, allows mock injection in tests, and enforces the separation between *what* (trait) and *how* (implementation).

## Overview

This crate defines the **contract layer** of PlexSpaces. Every trait here has exactly one canonical definition; implementations live in the appropriate feature crate; consumers depend only on this crate's traits, not on concrete implementations.

### Architecture Context

```text
proto (data contracts)
       |
       v
service-traits (this crate — behavior contracts)
       |
       +---> actor (ActorId, MessageSender, ActorService, ActorFactory)
       +---> journaling (JournalStorage)
       +---> node (HealthChecker, HealthReporter, NodeConnectivity)
       +---> channel (ChannelService)
       +---> blob (BlobServiceTrait)
       +---> tuplespace (TupleSpaceProvider)
       +---> wasm-runtime (WasmRuntimeTrait)
       +---> elastic-pool (ElasticPoolService)
       +---> object-registry (ObjectRegistry)
       +---> metrics (MetricsServiceAccess)
```

**Design principle**: No crate in the graph above depends on another at the same or higher level — they all point *down* to `service-traits` for contracts. This eliminates circular dependencies.

## Trait Reference

### Actor Identity & Messaging

| Trait / Type | File | Purpose |
|---|---|---|
| `ActorId` | `actor_id.rs` | Canonical actor identity (name, type, namespace, node). Wraps proto `ActorIdentity`. Use `ActorId::new()` / `ActorId::from_canonical()`. |
| `ActorRef` | `actor_ref.rs` | Lightweight reference for sending messages to an actor. Not bound to a live mailbox. |
| `MessageSender` | `message_sender.rs` | `tell()` / `ask()` abstractions over actor message delivery. |
| `ActorService` | (re-exported from `actor_ref.rs`) | Spawn actors, send messages via gRPC or local dispatch. |
| `ActorFactory` | (re-exported from `actor_ref.rs`) | Create actor instances with full spawn specs. |
| `ActorStateHandle` | `actor_state_handle.rs` | Runtime handle to an actor's live state (used by facets). |
| `ActorStateChecker` | `actor_state_checker.rs` | Query liveness of an actor without sending a message. |

### Service Discovery & Lifecycle

| Trait / Type | File | Purpose |
|---|---|---|
| `ServiceLocatorBase` | `service_locator_base.rs` | Read-only accessor for registered services. All runtime code uses this — not the mutable `InitializableServiceLocator`. |
| `ElasticPoolService` | `elastic_pool.rs` | Acquire / release actor instances from an elastic pool. Erlang-style worker pool. |
| `PoolServiceError` | `elastic_pool.rs` | Typed error for pool operations with `code() -> PoolServiceErrorCode`. |

### Infrastructure

| Trait / Type | File | Purpose |
|---|---|---|
| `HealthChecker` | `health.rs` | Check the health of a single component (async). |
| `HealthReporter` | `health.rs` | Register and query system-wide health status. |
| `HealthCheckContext` | `health.rs` | Context for a health check (component name, timeout, metadata). Re-exported from proto. |
| `HealthCheckError` | `health.rs` | Typed error with `code() -> HealthCheckErrorCode`. |
| `NodeConnectivity` | `node_connectivity.rs` | Connect nodes, list connected peers, disconnect. |
| `MetricsServiceAccess` | `metrics.rs` | Expose Prometheus metrics from the actor runtime. |

### Storage & Persistence

| Trait / Type | File | Purpose |
|---|---|---|
| `JournalStorage` | `journal_storage.rs` | Append-only event journal with snapshot support (Erlang-style persistent_term). |
| `ObjectRegistry` | `object_registry.rs` | Distributed object registration and lookup (key → value, with TTL). |
| `BlobServiceTrait` | `blob_service.rs` | Upload, download, list, and delete binary blobs. |

### Communication

| Trait / Type | File | Purpose |
|---|---|---|
| `ChannelService` | `channel_service.rs` | Create and manage named channels (queues/pub-sub). |
| `TupleSpaceProvider` | `tuplespace_provider.rs` | Access the TupleSpace coordination primitive. |
| `OutboundHttpClient` | `outbound_http.rs` | Make outbound HTTP calls (used by actors for external integrations). |

### Execution Environments

| Trait / Type | File | Purpose |
|---|---|---|
| `WasmRuntimeTrait` | `wasm_runtime.rs` | Execute a WASM module inside the actor runtime. |

## Usage Patterns

### Depending on service-traits in a new crate

```toml
# Cargo.toml
[dependencies]
plexspaces-service-traits = { path = "../../crates/service-traits" }
```

### Implementing a trait (example: HealthChecker)

```rust
use plexspaces_service_traits::health::{HealthCheckContext, HealthCheckError, HealthChecker};
use async_trait::async_trait;

pub struct DatabaseHealthChecker { db_url: String }

#[async_trait]
impl HealthChecker for DatabaseHealthChecker {
    async fn check(&self, ctx: &HealthCheckContext) -> Result<(), HealthCheckError> {
        // ping database within ctx.timeout
        Ok(())
    }

    fn name(&self) -> &str { "database" }
}
```

### Accepting a trait in a function (dependency injection)

```rust
use plexspaces_service_traits::service_locator_base::ServiceLocatorBase;
use std::sync::Arc;

async fn start_worker(locator: Arc<dyn ServiceLocatorBase>) {
    let channel = locator.get_channel_service().await
        .expect("channel service must be registered");
    // ...
}
```

## Crate Dependencies

This crate intentionally has **minimal dependencies** — only `plexspaces-proto` (for proto-generated types in trait signatures) and standard async/error utilities:

```
plexspaces-proto     — proto-generated data types used in trait signatures
async-trait          — #[async_trait] macro for async trait methods
thiserror            — error type derivation for HealthCheckError, PoolServiceError
```

No crate in the workspace that *implements* a trait defined here should be in this crate's dependency tree.

## References

- [Architecture](../../docs/architecture.md)
- [Detailed Design](../../docs/detailed-design.md)
- [Actor crate](../actor/README.md) — implements ActorService, ActorFactory, MessageSender
- [Node crate](../node/README.md) — implements HealthChecker, NodeConnectivity
