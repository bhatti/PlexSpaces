# Actor - Core Actor Implementation

**Purpose**: Provides the foundational actor abstraction with lifecycle management, behavior composition, and resource-aware scheduling. This is the foundational building block for all distributed computation in PlexSpaces.

## Overview

This crate implements **Pillar 2 (Erlang/OTP Philosophy)** of PlexSpaces. It provides the unified actor model where one powerful actor type has composable capabilities instead of multiple specialized types.

### Architecture Context

This module supports all 5 pillars:
- **Pillar 1 (TupleSpace)**: Actors can coordinate via TupleSpace for decoupled communication
- **Pillar 2 (Erlang/OTP)**: Core implementation of actor model, lifecycle, supervision
- **Pillar 3 (Durability)**: Integration with journaling for durable execution
- **Pillar 4 (WASM)**: Actors can execute as WASM modules (via behavior abstraction)
- **Pillar 5 (Firecracker)**: Actors can run in isolated microVMs (via node placement)

### Component Diagram

```text
plexspaces_proto (definitions)
       |
       v
plexspaces_actor (this module) <--- plexspaces_supervision
       |                                     |
       +---> plexspaces_mailbox              |
       +---> plexspaces_journal              |
       +---> plexspaces_behavior             |
       +---> plexspaces_facet                |
       |                                     |
       +-------------------------------------+
                     |
                     v
             plexspaces_node
```

## Key Components

- **Actor**: The unified actor implementation with static lifecycle and dynamic facets
- **ActorRef**: Lightweight, cloneable handle for sending messages to actors
- **ActorBuilder**: Fluent API for creating actors with configuration
- **ActorState**: Lifecycle state enum (Creating, Inactive, Active, Terminated, Failed) - unified with proto definitions
- **ResourceProfile**: Resource-aware scheduling profiles (CpuIntensive, IoIntensive, Balanced, etc.)
- **ResourceContract**: QoS guarantees for actors (max CPU, memory, IO, etc.)
- **ActorHealth**: Health status tracking (Healthy, Degraded, Stuck, Failed)

## Design Principles

### Static vs Dynamic Design

This module follows PlexSpaces' "Static for core, dynamic for extensions" principle:

**Static (Core, Always Present)**:
- `Actor.id`: Every actor needs unique identity
- `Actor.state`: Every actor has lifecycle state
- `Actor.behavior`: Every actor processes messages
- `Actor.mailbox`: Every actor receives messages
- `Actor.journal`: Every actor is durable (Pillar 3)
- Lifecycle hooks: `on_activate()`, `on_deactivate()`, `on_timer()`

**Dynamic (Extensions via Facets)**:
- WASM Migration: State-only migration via WASM runtime (see `wasm.proto`)
- Metrics: `MetricsFacet` for Prometheus integration
- Tracing: `TracingFacet` for distributed tracing
- Security: `SecurityFacet` for authorization

### Resource Awareness (Quickwit-Inspired)

Actors declare resource profiles and contracts upfront:
- **ResourceProfile**: Type of resources consumed (CPU, Memory, IO, Network, Balanced)
- **ResourceContract**: QoS guarantees (max CPU%, memory bytes, IO ops/sec)
- System uses these for intelligent placement and scheduling

### Proto-First Design

All actor state and messages use Protocol Buffer definitions:
- `proto/plexspaces/v1/actor_runtime.proto` defines Actor, Message, ActorState
- This module implements the logic using generated proto types
- Ensures wire compatibility for distributed actor communication

## Actor Lifecycle

```rust
Creating → Inactive → Active → Terminated
                            ↓
                          Failed
```

### Lifecycle States

- **Creating**: Actor is being created (replaces "Initializing")
- **Inactive**: Actor is inactive and not processing messages (replaces "Suspended")
- **Active**: Actor is running and processing messages
- **Terminated**: Actor has stopped gracefully (replaces "Stopped")
- **Failed**: Actor has crashed with error message (includes `error_message` field)

**Note**: As of Phase 4, the lifecycle states have been unified with proto definitions. The `error_message` field in the `Actor` struct stores detailed error information when the actor is in the `Failed` state.

## Usage Examples

### Basic Actor Creation

```rust
use plexspaces_actor::{Actor, ActorBuilder, ActorState};
use plexspaces_behavior::GenServerBehavior;
use plexspaces_core::{ActorContext, ActorBehavior};

struct CounterActor {
    count: i32,
}

#[async_trait]
impl ActorBehavior for CounterActor {
    async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        // Handle message
        Ok(())
    }
}

// Create actor using builder
let actor = ActorBuilder::new("counter@node1".to_string())
    .with_behavior(CounterActor { count: 0 })
    .build()?;
```

### Actor Reference (ActorRef)

```rust
use plexspaces_actor::ActorRef;
use plexspaces_core::ServiceLocator;
use std::sync::Arc;

// Create local actor reference
let actor_ref = ActorRef::local("counter@node1", mailbox);

// Create remote actor reference (uses ServiceLocator for gRPC client caching)
let service_locator = Arc::new(ServiceLocator::new());
// ... register services in service_locator ...
let remote_ref = ActorRef::remote("counter@node2", "node2", service_locator);

// Send message (fire-and-forget) - same API for local and remote
let message = Message::new(b"increment".to_vec());
actor_ref.tell(message).await?;
remote_ref.tell(message).await?;

// Ask pattern (request-reply)
let reply = remote_ref.ask(message, Duration::from_secs(5)).await?;
```

**Design**: `ActorRef` is location-transparent. Remote `ActorRef`s use `ServiceLocator` for efficient gRPC client caching (one client per node, shared across all ActorRefs to that node). This scales to hundreds of thousands of ActorRefs without creating excessive connections.

### Resource-Aware Actor

```rust
use plexspaces_actor::{ActorBuilder, ResourceProfile, ResourceContract};

let actor = ActorBuilder::new("cpu-intensive-task@node1".to_string())
    .with_behavior(MyBehavior {})
    .with_resource_profile(ResourceProfile::CpuIntensive)
    .with_resource_contract(ResourceContract {
        max_cpu_percent: 80,
        max_memory_bytes: 1024 * 1024 * 100, // 100MB
        max_io_ops_per_sec: 1000,
    })
    .build()?;
```

## Performance Characteristics

- **Actor creation**: < 1ms (in-memory)
- **Message delivery**: < 10μs (local), < 1ms (remote)
- **State transition**: < 100μs (Activating -> Active)
- **Memory per actor**: < 1KB (idle), < 10KB (active with state)

## Testing

```bash
# Run all actor tests
cargo test -p plexspaces-actor

# Check coverage
cargo tarpaulin -p plexspaces-actor --out Html
open tarpaulin-report.html

# Run doc tests
cargo test --doc -p plexspaces-actor
```

## Dependencies

This crate depends on:
- `plexspaces_proto`: Actor message definitions, state enums
- `plexspaces_behavior`: OTP-style behaviors (GenServer, GenEvent, etc.)
- `plexspaces_mailbox`: Message queue abstraction with priority and backpressure
- `plexspaces_journaling`: Durable execution and replay for fault tolerance
- `plexspaces_facet`: Dynamic capability composition (mobility, metrics, etc.)
- `plexspaces_core`: Common types (ActorId, ActorContext, errors)

## Dependents

This crate is used by:
- `plexspaces_supervisor`: Supervises actors and handles restart policies
- `plexspaces_node`: Hosts actors on compute nodes with resource isolation
- `plexspaces_mobility`: Migrates actors between nodes with state preservation
- `plexspaces_workflow`: Orchestrates multi-actor workflows

## Features

### TTL Support
Messages support Time-To-Live (TTL) for automatic expiration:

```rust
use std::time::Duration;
let message = Message::new(b"data".to_vec())
    .with_ttl(Duration::from_secs(30));

// Check if expired
if message.is_expired() {
    // Message has expired
}
```

### Unified ActorRef API
Location-transparent messaging without ActorContext:

```rust
// Local or remote - same API
actor_ref.tell(message).await?;
let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;
```

## Known Limitations

- **Actor groups**: ✅ Already implemented
- **Hot code swapping**: Not needed - use blue/green deployment
- **Stateless workers**: ✅ Implemented via elastic pool pattern
- **Distributed placement**: Currently random, planned intelligent placement

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design - Actors](../../docs/detailed-design.md#actors) - Comprehensive actor documentation
- [Getting Started Guide](../../docs/getting-started.md) - Quick start with actors
- Implementation: `crates/actor/src/`
- Tests: `crates/actor/src/` (unit tests)
- Proto definitions: `proto/plexspaces/v1/actor_runtime.proto`

