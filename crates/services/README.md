# Services - Centralized Service Infrastructure

**Purpose**: Provides centralized service infrastructure and implementations for PlexSpaces, consolidating all service-related code in one place.

## Overview

The `plexspaces-services` crate consolidates all service implementations and infrastructure that was previously scattered across multiple crates. This eliminates circular dependencies and provides a clean, centralized location for managing services.

## Architecture Context

```
plexspaces-core → plexspaces-services ← plexspaces-application
                      ↑
                      └── All service implementations
```

**Design Philosophy**:
- **ServiceLocator-First**: Services depend on `ServiceLocator`, not `Node`
- **Clean Separation**: Services are independent, testable, and composable
- **No Circular Dependencies**: Services don't depend on `plexspaces-node`
- **Trait-Based Design**: Services use traits from `plexspaces-core` for abstraction

## Key Components

### ServiceLocator

Centralized service registration and gRPC client caching:

```rust
use plexspaces_services::ServiceLocator;
use std::sync::Arc;

let service_locator = Arc::new(ServiceLocator::new());

// Register services
service_locator.register_service(actor_registry.clone()).await;

// Retrieve services
let registry = service_locator.actor_registry().await?;
```

**Features**:
- Type-based service registration and retrieval
- gRPC client pooling for remote node communication
- Thread-safe concurrent access
- Service lifecycle management

### Service Implementations

All gRPC service implementations are consolidated here:

- **ActorService**: Actor lifecycle and messaging (`crates/services/src/actor_service/`)
- **ApplicationService**: Application deployment and management (`crates/services/src/application_service.rs`)
- **TupleService**: Distributed TupleSpace operations (`crates/services/src/tuple_service/`)
- **BlobService**: Blob storage operations (`crates/services/src/blob_service/`)
- **WorkflowService**: Workflow orchestration (`crates/services/src/workflow_service/`)
- **SystemService**: System information and health checks (`crates/services/src/system_service/`)
- **MetricsService**: Metrics collection and export (`crates/services/src/metrics_service/`)
- **FirecrackerService**: Firecracker VM management (`crates/services/src/firecracker_service/`) - Optional feature
- **NodeService**: Node management, metrics, and capacity (`crates/services/src/node_service/`)
- **NodeRegistry**: TTL-cached node discovery with gossip (`crates/services/src/node_registry/`)
- **DashboardService**: Aggregated cluster dashboard (`crates/services/src/dashboard_service/`)

### Service Wrappers

Service wrappers for registering services in `ServiceLocator`:

- `ActorServiceWrapper`
- `FirecrackerVmServiceWrapper`
- `NodeConnectionInfoWrapper`
- And more...

## Usage Examples

### Creating Services

```rust
use plexspaces_services::{ServiceLocator, ActorServiceImpl};
use std::sync::Arc;

// Create service locator
let service_locator = Arc::new(ServiceLocator::new());

// Create and register actor service
let actor_service = Arc::new(ActorServiceImpl::new(
    service_locator.clone(),
    "node1".to_string(),
));
service_locator.register_service(actor_service.clone()).await;
```

### Using ServiceLocator in Services

```rust
use plexspaces_core::ServiceLocator;
use std::sync::Arc;

pub struct MyService {
    service_locator: Arc<dyn ServiceLocator>,
}

impl MyService {
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }
    
    async fn do_work(&self) -> Result<(), Error> {
        // Get required services from ServiceLocator
        let actor_registry = self.service_locator.actor_registry().await?;
        let tuplespace = self.service_locator.tuplespace_provider().await?;
        
        // Use services...
        Ok(())
    }
}
```

## Design Principles

### No `Arc<dyn Any>` Types

All services use proper traits instead of `Arc<dyn Any>`:
- `WasmRuntimeTrait` for WASM runtime
- `KeyValueStore` trait for key-value operations
- `LockManager` trait for distributed locks
- `MessageSender` trait for message sending

### Proto-First Design

Data models are defined in Protocol Buffers:
- `Lock`, `AcquireLockOptions`, `RenewLockOptions`, `ReleaseLockOptions` from proto
- Rust traits define behavior, proto defines data

### ServiceLocator-First Architecture

Services depend on `ServiceLocator`, not `Node`:
- Services get dependencies from `ServiceLocator`
- No direct `Node` dependencies
- Enables testing with mock `ServiceLocator`

## Features

### Optional Features

- **firecracker**: Enable Firecracker VM service support
  - Requires `plexspaces-firecracker` dependency
  - Adds `FirecrackerVmService` implementation

## Testing

```bash
# Run all service tests
cargo test -p plexspaces-services

# Run with firecracker feature
cargo test -p plexspaces-services --features firecracker
```

## Dependencies

This crate depends on:
- `plexspaces-core`: Core types and traits
- `plexspaces-proto`: Protocol buffer definitions
- `plexspaces-application`: Application management
- `plexspaces-actor`: Actor types
- `plexspaces-tuplespace`: TupleSpace types
- `plexspaces-blob`: Blob storage types
- `plexspaces-workflow`: Workflow types
- `plexspaces-firecracker`: Firecracker VM types (optional)
- `tonic`: gRPC framework
- `tokio`: Async runtime

## Dependents

This crate is used by:
- `plexspaces-node`: Node uses services for gRPC endpoints
- All applications: Applications use services via `ServiceLocator`

## Migration Notes

### From Node-Based Services

Services that previously depended on `Arc<Node>` should now:
1. Accept `Arc<dyn ServiceLocator>` instead
2. Get required services from `ServiceLocator`
3. Use traits from `plexspaces-core` for abstraction

### Example Migration

**Before**:
```rust
pub struct MyService {
    node: Arc<Node>,
}

impl MyService {
    pub fn new(node: Arc<Node>) -> Self {
        Self { node }
    }
}
```

**After**:
```rust
use plexspaces_core::ServiceLocator;

pub struct MyService {
    service_locator: Arc<dyn ServiceLocator>,
}

impl MyService {
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }
}
```

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [ServiceLocator Design](../../crates/core/README.md#servicelocator) - ServiceLocator documentation
- [HIGHEST_PRIORITY_PLAN.md](../../HIGHEST_PRIORITY_PLAN.md#task-45) - Task 4.5 details
- Implementation: `crates/services/src/`
- Tests: `crates/services/src/` (unit tests)

