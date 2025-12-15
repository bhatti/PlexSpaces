# Facet - Dynamic Behavior Composition

**Purpose**: Provides a facet system for dynamic behavior composition, allowing runtime attachment of cross-cutting concerns to actors.

## Overview

Facets allow runtime attachment of cross-cutting concerns to actors. This follows the "Static for core, dynamic for extensions" principle where core actor functionality is static, but additional capabilities can be added dynamically via facets.

## Key Components

### Facet Trait

```rust
pub trait Facet: Send + Sync {
    fn name(&self) -> &str;
    async fn on_attach(&mut self, actor_id: &str) -> Result<(), FacetError>;
    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError>;
    async fn on_message(&mut self, msg: &Message) -> Result<(), FacetError>;
}
```

### FacetContainer

Manages multiple facets attached to an actor:

```rust
pub struct FacetContainer {
    facets: HashMap<String, Box<dyn Facet>>,
}

impl FacetContainer {
    pub fn attach(&mut self, facet: Box<dyn Facet>) -> Result<(), FacetError>;
    pub fn detach(&mut self, name: &str) -> Result<(), FacetError>;
    pub fn get<T: Facet>(&self, name: &str) -> Option<&T>;
}
```

## Built-in Facets

### Capabilities

- **WASM Migration**: State-only migration via WASM runtime (see `wasm.proto`)
- **KeyValueFacet**: Key-value storage access
- **MetricsFacet**: Prometheus metrics integration
- **TracingFacet**: Distributed tracing
- **SecurityFacet**: Authorization and authentication

### EventEmitter

Event-driven communication:

```rust
pub struct EventEmitter {
    listeners: HashMap<String, Vec<Box<dyn EventListener>>>,
}

impl EventEmitter {
    pub fn emit(&self, event: &str, data: Vec<u8>) -> Result<(), EventError>;
    pub fn on(&mut self, event: &str, listener: Box<dyn EventListener>);
}
```

## Design Principles

### Static vs Dynamic

- **Static (Core)**: Actor ID, state, behavior, mailbox, journal (always present)
- **Dynamic (Facets)**: Mobility, metrics, tracing, security (optional, runtime attachment)

### Separation of Concerns

Facets handle cross-cutting concerns without modifying core actor logic:
- **Mobility**: Handles actor migration logic
- **Metrics**: Collects and exports metrics
- **Tracing**: Adds distributed tracing spans
- **Security**: Enforces authorization policies

## Usage Examples

### Attaching Facets

```rust
use plexspaces_facet::{FacetContainer, MetricsFacet, TracingFacet};

let mut facets = FacetContainer::new();

// Attach metrics facet
let metrics = MetricsFacet::new();
facets.attach(Box::new(metrics))?;

// Attach tracing facet
let tracing = TracingFacet::new();
facets.attach(Box::new(tracing))?;
```

### Using Facets in Actor

```rust
use plexspaces_facet::FacetContainer;

pub struct MyActor {
    facets: FacetContainer,
    state: MyState,
}

impl MyActor {
    async fn handle_message(&mut self, msg: Message) -> Result<(), Error> {
        // Facets can intercept messages
        for facet in self.facets.iter() {
            facet.on_message(&msg).await?;
        }
        
        // Process message
        self.process_message(msg).await?;
        
        Ok(())
    }
}
```

### Custom Facet Implementation

```rust
use plexspaces_facet::{Facet, FacetError};
use plexspaces_mailbox::Message;

pub struct MyCustomFacet {
    config: MyConfig,
}

#[async_trait]
impl Facet for MyCustomFacet {
    fn name(&self) -> &str {
        "my_custom_facet"
    }

    async fn on_attach(&mut self, actor_id: &str) -> Result<(), FacetError> {
        // Initialize facet for actor
        Ok(())
    }

    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError> {
        // Cleanup
        Ok(())
    }

    async fn on_message(&mut self, msg: &Message) -> Result<(), FacetError> {
        // Process message (e.g., logging, metrics, etc.)
        Ok(())
    }
}
```

## Dependencies

This crate depends on:
- `plexspaces_core`: Common types (ActorId, Message)
- `async-trait`: For async trait methods
- `tokio`: Async runtime

## Dependents

This crate is used by:
- `plexspaces_actor`: Actors use FacetContainer
- `plexspaces_node`: Node manages facets for actors

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design - Facets](../../docs/detailed-design.md#facets) - Comprehensive facet documentation with all facet types
- [Getting Started Guide](../../docs/getting-started.md) - Quick start with facets
- Implementation: `crates/facet/src/`
- Tests: `crates/facet/src/` (unit tests)
- Proto definitions: `proto/plexspaces/v1/actors/facets.proto`

