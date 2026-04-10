# Node - Node Management and Distribution

**Purpose**: Provides location transparency and distribution capabilities, inspired by Erlang's node system but elevated for modern needs.

## Overview

The Node crate is the central infrastructure layer that hosts actors, manages distribution, and provides all system services. It implements the Erlang/OTP node concept with modern enhancements.

### Architecture Context

In PlexSpaces (following Erlang/OTP principles):
- **Node** = Infrastructure layer (gRPC, actor registry, remoting, health checks)
- **Application** = Business logic layer (supervision trees, workers, domain actors)
- **Release** = Docker image with Node binary as entry point

## Key Components

### Node

Main node structure managing all system services:

```rust
pub struct Node {
    id: NodeId,
    service_locator: Arc<ServiceLocator>,  // Centralized service registration
    config: NodeConfig,
    // ... other fields ...
}
```

**Note**: Actor-related data (instances, facets, monitors, links, etc.) is now stored in `ActorRegistry` 
(accessed via `ServiceLocator`) for better separation of concerns and encapsulation. The `Node` no longer 
directly exposes internal actor storage maps.

**ServiceLocator Integration**: The `Node` creates and manages a `ServiceLocator` that registers core services (`ActorRegistry`, `ReplyWaiterRegistry`) and provides gRPC client caching for remote node communication. This enables efficient connection reuse across all `ActorRef`s.

### NodeBuilder

Fluent API for creating nodes:

```rust
let node = NodeBuilder::new("my-node")
    .with_listen_address("0.0.0.0:9000")
    .with_max_connections(200)
    .with_heartbeat_interval_ms(10000)
    .build();
```

### ConfigBootstrap

Erlang/OTP-inspired configuration loading:

```rust
// Load from release.toml with environment variable overrides
let config = ConfigBootstrap::load()?;
```

### CoordinationComputeTracker

Performance metrics tracking:

```rust
let mut tracker = CoordinationComputeTracker::new("operation-name");
tracker.start_coordinate();
// ... coordination work ...
tracker.end_coordinate();
let report = tracker.finalize();
```

## Services

### gRPC Services

All gRPC services are implemented in the `plexspaces-services` crate:

- **ActorService**: Remote actor communication (`plexspaces-services::actor_service`)
- **ApplicationService**: Application deployment and management (`plexspaces-services::application_service`)
- **HealthService**: Health checks for Kubernetes probes (`plexspaces-core::health_service`)
- **SystemService**: System information and statistics (`plexspaces-services::system_service`)
- **TupleSpaceService**: Distributed TupleSpace operations (`plexspaces-services::tuple_service`)
- **BlobService**: Blob storage operations (`plexspaces-services::blob_service`)
- **WorkflowService**: Workflow orchestration (`plexspaces-services::workflow_service`)
- **MetricsService**: Metrics collection and export (`plexspaces-services::metrics_service`)
- **FirecrackerService**: Firecracker VM management (`plexspaces-services::firecracker_service`) - Optional

**Note**: Services are accessed via `ServiceLocator` and don't depend directly on `Node`. This design eliminates circular dependencies and enables better testability. See [Services Crate README](../services/README.md) for details.

### Application Management

Erlang/OTP-style application lifecycle:

```rust
// Start application
node.start_application(application).await?;

// Stop application
node.stop_application("my-app").await?;
```

### WASM Application Deployment

Deploy WASM modules as applications:

```rust
// Deploy WASM application
let wasm_bytes = std::fs::read("app.wasm")?;
node.deploy_wasm_application("my-app", wasm_bytes).await?;
```

## Usage Examples

### Creating and Starting a Node

```rust
use plexspaces_node::{Node, NodeBuilder};

// Create node with builder
let node = NodeBuilder::new("node1")
    .with_listen_address("0.0.0.0:9000")
    .build();

// Start node (starts gRPC server)
node.start().await?;

// Node is now running and accepting connections
```

### Spawning Actors

```rust
use plexspaces_node::ActorBuilder;

// Spawn actor on node
let actor_ref = ActorBuilder::new(MyBehavior {})
    .with_name("counter")
    .spawn(&ctx, node.service_locator().clone())
    .await?;

// Send message to actor
actor_ref.tell(message).await?;
```

Use a unique actor name when spawning. The node/runtime constructs the canonical actor ID; client code should not prebuild full actor IDs for local spawns.

### Remote Actor Communication

```rust
// Send message to remote actor (location transparent)
let remote_actor = "counter//counter::default@node2".to_string();
node.send_message(remote_actor, message).await?;
```

### Application Deployment

```rust
use plexspaces_core::application::Application;

struct MyApplication {
    // ...
}

#[async_trait]
impl Application for MyApplication {
    // ...
}

// Deploy application
node.start_application(Arc::new(MyApplication {})).await?;
```

### Health Checks

```rust
// Check node health
let health = node.health_check().await?;

// Check specific dependency
let db_health = node.check_dependency("database").await?;
```

### Graceful Shutdown

```rust
// Initiate graceful shutdown
node.shutdown().await?;

// Wait for shutdown to complete
node.wait_for_shutdown().await?;
```

## Configuration

### Configuration File (release.toml)

```toml
[node]
id = "node1"
listen_address = "0.0.0.0:9000"
max_connections = 200
heartbeat_interval_ms = 10000

[application]
name = "my-app"
version = "1.0.0"

[firecracker]
binary_path = "/usr/bin/firecracker"
kernel_path = "/var/lib/firecracker/vmlinux"
rootfs_path = "/var/lib/firecracker/rootfs.ext4"
```

### Environment Variable Overrides

All configuration can be overridden via environment variables:

```bash
export PLEXSPACES_NODE_ID=node1
export PLEXSPACES_NODE_LISTEN_ADDRESS=0.0.0.0:9000
export PLEXSPACES_FIRECRACKER_BIN=/usr/bin/firecracker
```

## Features

### Location Transparency

Actors can communicate transparently across nodes:

```rust
// Same API for local and remote actors
node.send_message("actor//gen_server::default@node1", msg).await?;  // Local
node.send_message("actor//gen_server::default@node2", msg).await?;  // Remote (via gRPC)
```

### Actor Monitoring

Erlang-style monitoring (one-way notifications):

```rust
// Monitor actor for termination
node.monitor_actor("actor//gen_server::default@node1", supervisor_id).await?;
```

### Actor Linking

Erlang-style linking (bidirectional death propagation):

```rust
// Link two actors (if one dies, the other dies too)
node.link_actors(
    "actor1//gen_server::default@node1",
    "actor2//gen_server::default@node1",
).await?;
```

### Virtual Actors and Automatic Activation

Orleans-inspired virtual actors with automatic activation:

```rust
use plexspaces_node::Node;

// Get or activate actor (Orleans-style convenience)
// If actor exists, returns existing ActorRef
// If not, creates and activates actor
let actor_ref = node.get_or_activate_actor(
    "virtual-actor".to_string(),
    || async {
        // Actor factory - only called if actor doesn't exist
        Ok(Actor::new(/* ... */))
    }
).await?;

// Virtual actor is automatically activated on first message
// or via explicit get_or_activate_actor() call
node.send_message("virtual-actor//virtual-actor::default@node1", msg).await?;
```

**Design**: `get_or_activate_actor()` provides a convenient pattern for virtual actors. It checks if the actor exists (locally or remotely) and returns its `ActorRef`, or creates and spawns a new actor if it doesn't exist. This eliminates the need for explicit existence checks before messaging.

### Process Groups

Group communication for actor sets:

```rust
// Create process group
node.create_process_group("my-group").await?;

// Add actors to group
node.add_to_process_group("my-group", "actor1//gen_server::default@node1").await?;
node.add_to_process_group("my-group", "actor2//gen_server::default@node1").await?;

// Send message to all actors in group
node.send_to_process_group("my-group", msg).await?;
```

## Performance Characteristics

- **Actor spawn**: < 1ms (local)
- **Message delivery (local)**: < 10μs
- **Message delivery (remote)**: < 1ms (gRPC)
- **Node startup**: < 100ms
- **Health check**: < 1ms

## Testing

```bash
# Run all node tests
cargo test -p plexspaces-node

# Run integration tests
cargo test -p plexspaces-node --test integration

# Check coverage
cargo tarpaulin -p plexspaces-node --out Html
```

## Dependencies

This crate depends on:
- `plexspaces-core`: Core types and traits
- `plexspaces-actor`: Actor implementation
- `plexspaces-behavior`: Actor behaviors
- `plexspaces-journaling`: Durable execution
- `plexspaces-tuplespace`: Coordination
- `plexspaces-mailbox`: Message queues
- `plexspaces-keyvalue`: Key-value storage
- `plexspaces-object-registry`: Service discovery
- `plexspaces-channel`: Channel abstractions
- `plexspaces-process-groups`: Group communication
- `plexspaces-supervisor`: Supervision trees
- `plexspaces-facet`: Dynamic capabilities
- `plexspaces-scheduler`: Task scheduling
- `plexspaces-locks`: Distributed locks
- `plexspaces-wasm-runtime`: WASM execution
- `tonic`: gRPC framework
- `tokio`: Async runtime

## Dependents

This crate is used by:
- All applications: Applications run on nodes
- CLI tools: CLI manages nodes
- Examples: All examples use nodes

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design](../../docs/detailed-design.md) - Component details
- [Installation Guide](../../docs/installation.md) - Node deployment instructions
- [Getting Started Guide](../../docs/getting-started.md) - Quick start with nodes
- Implementation: `crates/node/src/`
- Tests: `crates/node/tests/`
- Proto definitions: `proto/plexspaces/v1/node.proto`
- Inspiration: Erlang/OTP node system
