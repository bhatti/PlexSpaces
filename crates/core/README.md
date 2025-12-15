# Core - Fundamental Types and Traits

**Purpose**: Provides fundamental types and traits shared between actor and behavior modules to break circular dependencies.

## Overview

This crate contains the foundational types that are used throughout PlexSpaces. It serves as a dependency-free core that other crates can depend on without creating circular dependencies.

## Key Components

### ActorContext

Enhanced context providing actors with access to all system services:

```rust
pub trait ActorContext {
    // Actor operations
    fn actor_service(&self) -> &dyn ActorService;
    
    // Service discovery
    fn object_registry(&self) -> &dyn ObjectRegistry;
    
    // Coordination
    fn tuplespace(&self) -> &dyn TupleSpaceProvider;
    
    // Channels
    fn channel_service(&self) -> &dyn ChannelService;
    
    // Process groups
    fn process_group_service(&self) -> &dyn ProcessGroupService;
    
    // Facets
    fn facet_service(&self) -> &dyn FacetService;
    
    // Node operations
    fn node_operations(&self) -> &dyn NodeOperations;
}
```

**Design Philosophy**: Actors receive this context in all their methods, giving them full access to the system without needing to pass services around manually.

**Note**: As of Phase 4, `ActorContext` is primarily a data structure. Convenience methods like `reply()`, `join_group()`, etc. have been removed. Actors should access services directly via the context (e.g., `ctx.actor_service().send(...)`, `ctx.process_group_service().join(...)`).

### ServiceLocator

Centralized service registration and gRPC client caching:

```rust
use plexspaces_core::ServiceLocator;
use std::sync::Arc;

let service_locator = Arc::new(ServiceLocator::new());

// Register services
let actor_registry = Arc::new(ActorRegistry::new());
service_locator.register_service(actor_registry.clone()).await;

// Retrieve services
let registry: Arc<ActorRegistry> = service_locator.get_service().await
    .ok_or("ActorRegistry not registered")?;

// Get gRPC client for remote node (with caching)
let client = service_locator.get_node_client("remote-node").await?;
```

**Design Philosophy**:
- **Centralized Management**: Single place to register/get services
- **gRPC Client Pooling**: Reuse connections across ActorRefs (one client per node)
- **Type Safety**: Type-based service lookup using `TypeId`
- **Thread Safety**: Uses `Arc<RwLock<...>>` for read-heavy workloads

**Benefits**:
- Eliminates need to pass individual services to every component
- Efficient connection reuse (scalable to hundreds of thousands of ActorRefs)
- Lightweight `Arc<ServiceLocator>` can be stored in ActorRef for remote messaging

### Service Traits

- **ActorService**: Spawn and communicate with actors (local and remote)
- **ObjectRegistry**: Service discovery and registration
- **TupleSpaceProvider**: Coordination via TupleSpace
- **ChannelService**: Queue and topic patterns
- **ProcessGroupService**: Group communication
- **FacetService**: Dynamic capability composition
- **NodeOperations**: Node-level operations

### Application Trait

Erlang/OTP-inspired application lifecycle:

```rust
#[async_trait]
pub trait Application {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    
    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError>;
    async fn stop(&mut self) -> Result<(), ApplicationError>;
    
    async fn health_check(&self) -> HealthStatus;
}
```

**Design Principles**:
- **Proto-First**: Data models defined in `proto/plexspaces/v1/application/application.proto`
- **Trait in Rust**: Application trait defines behavior (Rust-specific)
- **One app per node**: Default to single application per container
- **Clear separation**: Node provides infrastructure, Application implements logic

### Common Types

- **ActorId**: String type for actor identifiers
- **ActorRef**: Reference to an actor (for sending messages)
- **ActorLocation**: Location of an actor (local or remote)
- **Message**: Re-exported from `plexspaces_mailbox`
- **Mailbox**: Re-exported from `plexspaces_mailbox`

## Architecture Context

In PlexSpaces (following Erlang/OTP principles):
- **Node** = Infrastructure layer (gRPC, actor registry, remoting, health checks)
- **Application** = Business logic layer (supervision trees, workers, domain actors)
- **Release** = Docker image with Node binary as entry point

## Usage Examples

### Using ActorContext

```rust
use plexspaces_core::{ActorContext, ActorId};

async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), Error> {
    // Spawn a new actor
    let child_id = "child@node1".to_string();
    ctx.actor_service().spawn_actor(child_id.clone(), behavior).await?;
    
    // Send message to another actor
    let target_ref = ctx.object_registry().lookup_actor(&"target@node1".to_string()).await?;
    target_ref.tell(msg).await?;
    
    // Write to TupleSpace
    let tuple = Tuple::new(vec![/* fields */]);
    ctx.tuplespace().write(tuple).await?;
    
    // Publish to channel
    ctx.channel_service().publish("topic", msg.payload()).await?;
    
    Ok(())
}
```

### Implementing Application

```rust
use plexspaces_core::application::{Application, ApplicationNode, ApplicationError};
use std::sync::Arc;

pub struct MyApplication {
    config: ApplicationConfig,
}

#[async_trait]
impl Application for MyApplication {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn version(&self) -> &str {
        &self.config.version
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        // Spawn supervision tree, workers, etc.
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        // Graceful shutdown: drain work, save state
        Ok(())
    }
}
```

## Dependencies

This crate depends on:
- `plexspaces_proto`: Protocol buffer definitions
- `plexspaces_mailbox`: Message types
- `plexspaces_persistence`: Persistence traits
- `plexspaces_tuplespace`: TupleSpace traits
- `plexspaces_facet`: Facet traits
- `async-trait`: For async trait methods
- `tokio`: Async runtime
- `serde`: Serialization

## Dependents

This crate is used by:
- `plexspaces_actor`: Actors use ActorContext
- `plexspaces_behavior`: Behaviors use ActorContext
- `plexspaces_node`: Node provides ActorContext implementation
- All other crates: For common types

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design](../../docs/detailed-design.md) - Component details
- [Getting Started Guide](../../docs/getting-started.md) - Quick start guide
- Implementation: `crates/core/src/`
- Tests: `crates/core/src/` (unit tests)
- Proto definitions: `proto/plexspaces/v1/application/application.proto`

