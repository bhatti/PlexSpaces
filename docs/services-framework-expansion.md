# Services Framework Expansion - Design Document

## Status: PROPOSED
## Author: Engineering Team
## Date: 2026-02-24

---

## 1. Executive Summary

This document proposes expanding PlexSpaces' services framework with three major improvements:

1. **Capability Provider Architecture** - A plugin-based model (inspired by wasmCloud) for registering service capabilities dynamically
2. **Missing Service Implementations** - Fill gaps in HttpClient, SchedulerService, and CronService
3. **Service Mesh Primitives** - Add lattice-style service-to-service invocation with built-in observability

These changes will make PlexSpaces more extensible, bring parity with wasmCloud/Temporal/Akka in service orchestration, and provide a clean abstraction for multi-cloud and edge deployments.

---

## 2. Current State Analysis

### 2.1 What Exists

The `plexspaces-services` crate provides centralized service infrastructure:

```
plexspaces-core    (traits: ServiceLocator, Service, ActorService, etc.)
       ↓
plexspaces-services (implementations: ServiceLocatorImpl, ActorServiceImpl, etc.)
       ↓
plexspaces-node    (wire-up: NodeBuilder, gRPC server, HTTP gateway)
```

**Currently implemented services** (12 total):

| Service | Crate Location | gRPC? | WIT Interface? |
|---------|---------------|-------|----------------|
| ActorService | `actor_service/` | Yes | messaging.wit |
| ApplicationService | `application_service.rs` | Yes | N/A (HTTP API) |
| TupleService | `tuple_service/` | Yes | tuplespace.wit |
| BlobService | `blob_service/` | Yes | blob.wit |
| WorkflowService | `workflow_service/` | Yes | workflow.wit |
| SystemService | `system_service/` | Yes | N/A |
| MetricsService | `metrics_service/` | Yes | N/A |
| NodeService | `node_service/` | Yes | N/A |
| NodeRegistry | `node_registry/` | No | N/A |
| DashboardService | `dashboard_service/` | Yes | N/A |
| ProcessGroupService | `process_group_service/` | Yes | process-groups.wit |
| WasmFileSaver | `wasm_file_saver.rs` | No | N/A |

**ServiceLocator typed accessors** (20 services registered):
- ActorRegistry, VirtualActorManager, ReplyWaiterRegistry
- ActorService, ChannelService, TupleSpaceProvider
- ObjectRegistry, JournalStorage, LockManager
- NodeMetricsAccessor, NodeConnectionInfo
- ActorFactory, ApplicationManager, BehaviorRegistry
- GrpcConnectionManager, WasmRuntime
- ProcessGroupService, BlobService, NodeRegistry
- KeyValueStore, ProcessGroupRegistry, TaskRouter

### 2.2 Gaps Identified

From PLAN.md "Abstraction Coverage Matrix" and analysis:

| Gap | Impact | Priority |
|-----|--------|----------|
| **No HttpClient service** | WASM actors cannot make outbound HTTP calls | HIGH |
| **No CronService** | No durable scheduled jobs with cron expressions | HIGH |
| **No ServiceMesh/Lattice** | No transparent cross-node service invocation | MEDIUM |
| **No capability provider plugin model** | Hard to extend with custom service types | MEDIUM |
| **No config-driven service registration** | All services hard-coded in initialize_services_impl | LOW |

### 2.3 Architectural Principles (Preserved)

These existing principles remain unchanged:
- **ServiceLocator-First**: Services depend on ServiceLocator, not Node
- **Proto-First**: Data in proto, behavior in Rust traits
- **No circular deps**: Services crate doesn't depend on node crate
- **Typed accessors**: Object-safe trait methods for Arc<dyn ServiceLocator>

---

## 3. Proposed Changes

### 3.1 Capability Provider Architecture

**Goal**: Allow dynamic service registration through a plugin interface, inspired by wasmCloud's capability providers.

#### 3.1.1 CapabilityProvider Trait (in plexspaces-core)

```rust
/// A capability provider that can be dynamically registered with the ServiceLocator.
///
/// Capability providers are plugins that extend PlexSpaces with new service types.
/// They follow the wasmCloud pattern: host-side implementations that fulfill
/// WIT interface contracts for WASM actors.
///
/// Lifecycle:
/// 1. Provider is created with configuration
/// 2. Provider is registered with ServiceLocator via register_capability_provider()
/// 3. Provider.start() is called during node startup
/// 4. Provider handles requests from actors via its WIT interface
/// 5. Provider.stop() is called during node shutdown
#[async_trait]
pub trait CapabilityProvider: Send + Sync {
    /// Unique provider identifier (e.g., "plexspaces:http-client", "plexspaces:redis")
    fn provider_id(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// WIT interface this provider fulfills (if any)
    /// e.g., "plexspaces:actor/http-client@0.1.0"
    fn wit_interface(&self) -> Option<&str> { None }

    /// Start the provider (called after registration)
    async fn start(&self, config: &ProviderConfig) -> Result<(), ProviderError>;

    /// Stop the provider (called during shutdown)
    async fn stop(&self) -> Result<(), ProviderError>;

    /// Health check
    async fn health_check(&self) -> Result<ProviderHealth, ProviderError>;

    /// Handle a capability invocation from a WASM actor
    /// This is the core dispatch method - routes WIT function calls to provider logic
    async fn handle_invocation(
        &self,
        ctx: &RequestContext,
        operation: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, ProviderError>;
}

/// Configuration for a capability provider
pub struct ProviderConfig {
    /// Provider-specific configuration (JSON or key-value pairs)
    pub config: HashMap<String, String>,
    /// Link configuration (which actors are linked to this provider)
    pub links: Vec<ProviderLink>,
}

/// A link between an actor and a capability provider
pub struct ProviderLink {
    pub actor_id: String,
    pub link_name: String,
    pub config: HashMap<String, String>,
}

/// Provider health status
pub struct ProviderHealth {
    pub healthy: bool,
    pub message: String,
}

/// Provider errors
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("Provider not started: {0}")]
    NotStarted(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Invocation error: {0}")]
    InvocationError(String),
    #[error("Provider error: {0}")]
    Other(String),
}
```

#### 3.1.2 ServiceLocator Extensions

Add to `ServiceLocator` trait:

```rust
/// Register a capability provider
async fn register_capability_provider(&self, provider: Arc<dyn CapabilityProvider>);

/// Get a capability provider by ID
async fn get_capability_provider(&self, provider_id: &str) -> Option<Arc<dyn CapabilityProvider>>;

/// List all registered capability providers
async fn list_capability_providers(&self) -> Vec<String>;
```

Add to `ServiceLocatorImpl`:

```rust
/// Registered capability providers (provider_id -> provider)
capability_providers: Arc<RwLock<HashMap<String, Arc<dyn CapabilityProvider>>>>,
```

### 3.2 HttpClient Service

**Goal**: Allow WASM actors to make outbound HTTP requests through a host-provided capability.

#### 3.2.1 WIT Interface (new file: wit/plexspaces-actor/http-client.wit)

```wit
interface http-client {
    use types.{payload, actor-error, duration-ms, context};

    /// HTTP method
    enum http-method {
        get,
        post,
        put,
        delete,
        patch,
        head,
        options,
    }

    /// HTTP header
    record http-header {
        name: string,
        value: string,
    }

    /// HTTP request
    record http-request {
        method: http-method,
        url: string,
        headers: list<http-header>,
        body: option<payload>,
        timeout-ms: duration-ms,
    }

    /// HTTP response
    record http-response {
        status: u16,
        headers: list<http-header>,
        body: payload,
    }

    /// Send HTTP request
    request: func(ctx: context, req: http-request) -> result<http-response, actor-error>;

    /// Convenience: GET request
    get: func(ctx: context, url: string, headers: list<http-header>, timeout-ms: duration-ms) -> result<http-response, actor-error>;

    /// Convenience: POST request
    post: func(ctx: context, url: string, headers: list<http-header>, body: payload, timeout-ms: duration-ms) -> result<http-response, actor-error>;
}
```

#### 3.2.2 HttpClientProvider Implementation

Located at: `crates/services/src/http_client_service/`

```rust
pub struct HttpClientProvider {
    client: reqwest::Client,
    config: RwLock<HttpClientConfig>,
}

pub struct HttpClientConfig {
    /// Maximum concurrent requests per actor
    pub max_concurrent_requests: usize,
    /// Default timeout (ms)
    pub default_timeout_ms: u64,
    /// Allowed URL patterns (regex). Empty = allow all.
    pub allowed_url_patterns: Vec<String>,
    /// Denied URL patterns (regex). Checked first.
    pub denied_url_patterns: Vec<String>,
    /// Maximum response body size (bytes)
    pub max_response_size: usize,
}
```

The HttpClientProvider implements `CapabilityProvider` and provides the host-side
implementation of the `http-client` WIT interface. It includes:

- URL allowlist/denylist for security (prevent SSRF)
- Rate limiting per actor
- Response size limits
- Configurable timeouts
- Connection pooling via reqwest

#### 3.2.3 ServiceLocator Integration

Add to `ServiceLocator` trait:

```rust
/// Get HttpClient capability
async fn get_http_client(&self) -> Option<Arc<dyn HttpClientTrait>>;
/// Register HttpClient capability
async fn register_http_client(&self, client: Arc<dyn HttpClientTrait>);
```

### 3.3 CronService (Durable Scheduled Jobs)

**Goal**: Provide durable cron-based job scheduling with exactly-once execution.

#### 3.3.1 WIT Interface (new file: wit/plexspaces-actor/cron.wit)

```wit
interface cron {
    use types.{actor-id, payload, actor-error, duration-ms, context};

    /// Cron schedule specification
    record cron-schedule {
        /// Cron expression (e.g., "0 */5 * * * *" for every 5 minutes)
        expression: string,
        /// Timezone (e.g., "UTC", "America/New_York")
        timezone: string,
    }

    /// Scheduled job definition
    record scheduled-job {
        /// Unique job ID
        job-id: string,
        /// Target actor to receive the trigger message
        target-actor: actor-id,
        /// Message type to send
        msg-type: string,
        /// Message payload
        payload: payload,
        /// Cron schedule
        schedule: cron-schedule,
        /// Whether job is currently enabled
        enabled: bool,
    }

    /// Job status information
    record job-status {
        job-id: string,
        enabled: bool,
        last-run-at: option<u64>,
        next-run-at: option<u64>,
        run-count: u64,
        last-error: option<string>,
    }

    /// Register a cron job
    register-job: func(ctx: context, job: scheduled-job) -> result<string, actor-error>;

    /// Unregister a cron job
    unregister-job: func(ctx: context, job-id: string) -> result<_, actor-error>;

    /// Enable/disable a job
    set-job-enabled: func(ctx: context, job-id: string, enabled: bool) -> result<_, actor-error>;

    /// Get job status
    get-job-status: func(ctx: context, job-id: string) -> result<option<job-status>, actor-error>;

    /// List all jobs for this actor
    list-jobs: func(ctx: context) -> result<list<job-status>, actor-error>;
}
```

#### 3.3.2 CronService Implementation

Located at: `crates/services/src/cron_service/`

```rust
pub struct CronServiceImpl {
    service_locator: Arc<dyn ServiceLocator>,
    node_id: String,
    /// Registered jobs (job_id -> CronJob)
    jobs: Arc<RwLock<HashMap<String, CronJob>>>,
    /// Background scheduler handle
    scheduler_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

struct CronJob {
    definition: ScheduledJob,
    next_fire_time: DateTime<Utc>,
    run_count: u64,
    last_run_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
}
```

Key features:
- Uses distributed lock (LockManager) for leader election - only one node fires each job
- Jobs persisted in KeyValueStore for durability across restarts
- Cron expression parsing via `cron` crate
- Exactly-once delivery: lock + idempotency key per execution
- Fires by sending a message to the target actor via ActorService

### 3.4 Service Lifecycle Management

**Goal**: Formalize service startup/shutdown ordering and health checking.

#### 3.4.1 ServiceLifecycle Trait (in plexspaces-core)

```rust
/// Lifecycle hooks for services that need ordered startup/shutdown.
///
/// Services that implement this trait participate in the node's
/// ordered startup and graceful shutdown sequences.
#[async_trait]
pub trait ServiceLifecycle: Send + Sync {
    /// Called during node startup, after all services are registered.
    /// Use this for initialization that depends on other services being available.
    async fn on_start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    /// Called during graceful shutdown, before services are deregistered.
    /// Services should stop accepting new work but finish in-progress operations.
    async fn on_stop(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    /// Return startup priority (lower = starts first, higher = starts later).
    /// Default: 100. Infrastructure services (DB, KV) should use 10-50.
    /// Application services should use 100+.
    fn startup_priority(&self) -> u32 { 100 }

    /// Return shutdown priority (lower = stops first, higher = stops later).
    /// Typically reverse of startup: application services stop first (low priority),
    /// infrastructure stops last (high priority).
    fn shutdown_priority(&self) -> u32 { 100 }
}
```

#### 3.4.2 ServiceLocator Extensions

```rust
/// Register a service that participates in lifecycle management
async fn register_lifecycle_service(
    &self,
    name: &str,
    service: Arc<dyn ServiceLifecycle>,
);

/// Start all lifecycle services in priority order
async fn start_lifecycle_services(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

/// Stop all lifecycle services in reverse priority order
async fn stop_lifecycle_services(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
```

### 3.5 Service Invocation Layer (Lattice-Inspired)

**Goal**: Provide transparent cross-node service invocation with built-in retries, timeouts, and observability.

#### 3.5.1 ServiceInvoker Trait

```rust
/// Cross-node service invocation with automatic routing, retries, and observability.
///
/// Inspired by wasmCloud's lattice RPC and Dapr's service invocation.
/// Uses ObjectRegistry for service discovery and GrpcConnectionManager for transport.
#[async_trait]
pub trait ServiceInvoker: Send + Sync {
    /// Invoke a service operation on any available node
    ///
    /// The invoker handles:
    /// 1. Service discovery via ObjectRegistry
    /// 2. Load balancing across available nodes
    /// 3. Automatic retries with exponential backoff
    /// 4. Timeout management
    /// 5. Distributed tracing propagation
    async fn invoke(
        &self,
        ctx: &RequestContext,
        service_name: &str,
        operation: &str,
        payload: &[u8],
        options: InvocationOptions,
    ) -> Result<Vec<u8>, InvocationError>;
}

pub struct InvocationOptions {
    /// Timeout for the entire invocation (including retries)
    pub timeout: Duration,
    /// Maximum number of retries
    pub max_retries: u32,
    /// Retry backoff strategy
    pub backoff: BackoffStrategy,
    /// Target node preference (None = any available)
    pub target_node: Option<String>,
}

pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed(Duration),
    /// Exponential backoff with jitter
    Exponential { initial: Duration, max: Duration },
}
```

This builds on the existing `GrpcConnectionManager` and `ObjectRegistry` but adds:
- Automatic load balancing across nodes
- Configurable retry policies
- Circuit breaker integration (existing `circuit-breaker` crate)
- Distributed tracing context propagation

---

## 4. Implementation Plan

### Phase 1: Core Infrastructure (Service Lifecycle + Capability Provider)

| Step | Description | Files Changed |
|------|-------------|---------------|
| 1.1 | Add `ServiceLifecycle` trait to `plexspaces-core` | `crates/core/src/service_lifecycle.rs`, `crates/core/src/lib.rs` |
| 1.2 | Add `CapabilityProvider` trait to `plexspaces-core` | `crates/core/src/capability_provider.rs`, `crates/core/src/lib.rs` |
| 1.3 | Add `ProviderError` types to `plexspaces-core` | `crates/core/src/capability_provider.rs` |
| 1.4 | Extend `ServiceLocator` trait with new methods | `crates/core/src/service_locator_trait.rs` |
| 1.5 | Implement new methods in `ServiceLocatorImpl` | `crates/services/src/service_locator.rs` |
| 1.6 | Add lifecycle management to `initialize_services_impl` | `crates/services/src/service_locator.rs` |
| 1.7 | Unit tests for lifecycle and capability provider | `crates/services/tests/` |

### Phase 2: HttpClient Service

| Step | Description | Files Changed |
|------|-------------|---------------|
| 2.1 | Create `http-client.wit` WIT interface | `wit/plexspaces-actor/http-client.wit` |
| 2.2 | Add `http-client` import to `world.wit` | `wit/plexspaces-actor/world.wit` |
| 2.3 | Create `HttpClientTrait` in core | `crates/core/src/http_client_trait.rs` |
| 2.4 | Implement `HttpClientProvider` | `crates/services/src/http_client_service/mod.rs` |
| 2.5 | Add host bindings for WASM | `crates/wasm-runtime/src/simple_component_host.rs` |
| 2.6 | Register in `ServiceLocatorImpl` | `crates/services/src/service_locator.rs` |
| 2.7 | Add to `ServiceLocator` trait | `crates/core/src/service_locator_trait.rs` |
| 2.8 | SDK support (Python, TypeScript, Go) | `sdks/*/` |
| 2.9 | Tests and example | `crates/services/tests/`, `examples/` |

### Phase 3: CronService

| Step | Description | Files Changed |
|------|-------------|---------------|
| 3.1 | Create `cron.wit` WIT interface | `wit/plexspaces-actor/cron.wit` |
| 3.2 | Add `cron` import to `world.wit` | `wit/plexspaces-actor/world.wit` |
| 3.3 | Create `CronServiceTrait` in core | `crates/core/src/cron_service_trait.rs` |
| 3.4 | Implement `CronServiceImpl` | `crates/services/src/cron_service/mod.rs` |
| 3.5 | Add host bindings for WASM | `crates/wasm-runtime/src/simple_component_host.rs` |
| 3.6 | Register in `ServiceLocatorImpl` | `crates/services/src/service_locator.rs` |
| 3.7 | Add distributed lock-based leader election | `crates/services/src/cron_service/leader.rs` |
| 3.8 | Persistence via KeyValueStore | `crates/services/src/cron_service/persistence.rs` |
| 3.9 | Tests and cron_scheduler example | `crates/services/tests/`, `examples/` |

### Phase 4: Service Invocation Layer

| Step | Description | Files Changed |
|------|-------------|---------------|
| 4.1 | Create `ServiceInvoker` trait in core | `crates/core/src/service_invoker.rs` |
| 4.2 | Implement `ServiceInvokerImpl` | `crates/services/src/service_invoker/mod.rs` |
| 4.3 | Integrate with circuit breaker | `crates/services/src/service_invoker/circuit_breaker.rs` |
| 4.4 | Add retry policies | `crates/services/src/service_invoker/retry.rs` |
| 4.5 | Register in ServiceLocator | `crates/services/src/service_locator.rs` |
| 4.6 | Integration tests | `crates/services/tests/` |

---

## 5. Dependency Impact

### New External Dependencies

| Dependency | Version | Purpose | Crate |
|-----------|---------|---------|-------|
| `reqwest` | 0.12 | HTTP client (HttpClientProvider) | plexspaces-services |
| `cron` | 0.13 | Cron expression parsing (CronService) | plexspaces-services |
| `chrono-tz` | 0.10 | Timezone support (CronService) | plexspaces-services |

### Crate Dependency Changes

```
plexspaces-core (new traits: CapabilityProvider, ServiceLifecycle, HttpClientTrait, CronServiceTrait, ServiceInvoker)
       ↓
plexspaces-services (new impls: HttpClientProvider, CronServiceImpl, ServiceInvokerImpl)
       ↓
plexspaces-node (wire-up: register new services during startup)
```

No new circular dependencies introduced. All new traits go in core, all implementations in services.

---

## 6. Migration / Backward Compatibility

- All changes are **additive** - no existing APIs are modified
- `ServiceLocator` trait gets new methods with default implementations returning `None`
- Existing `initialize_services_impl` gains optional new service creation
- New WIT interfaces are only imported in the `plexspaces-actor` world, not `simple-actor`
- Existing tests remain unchanged

---

## 7. Testing Strategy

1. **Unit tests** for each new trait and implementation
2. **Integration tests** for service lifecycle ordering
3. **WASM actor tests** for HttpClient and Cron WIT interfaces
4. **Example apps** demonstrating each new capability:
   - `examples/python/http_gateway/` - HttpClient usage
   - `examples/python/cron_scheduler/` - CronService usage
5. **Backward compatibility tests** - existing tests must pass unchanged

---

## 8. Risks and Mitigations

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| ServiceLocator trait grows too large | Medium | Low | Group related methods behind sub-traits |
| HttpClient SSRF vulnerabilities | Medium | High | URL allowlist/denylist, no localhost by default |
| CronService clock skew across nodes | Low | Medium | UTC-only internally, single leader via lock |
| Breaking SDK changes from new WIT interfaces | Low | Low | New interfaces are opt-in, only in full actor world |

---

## 9. Future Work (Out of Scope)

- **NATS-based transport**: Replace gRPC for inter-node communication (major undertaking)
- **wRPC protocol support**: WebAssembly-native RPC (requires WASI 0.3)
- **Hot-reload for capability providers**: Swap providers without node restart
- **Distributed configuration service**: Centralized config management
