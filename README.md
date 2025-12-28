# PlexSpaces

<div align="center">

**A unified distributed actor framework for building scalable, fault-tolerant systems**

[![License: LGPL v2.1](https://img.shields.io/badge/License-LGPL%20v2.1-blue.svg)](https://www.gnu.org/licenses/lgpl-2.1)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/plexobject/plexspaces)

[Features](#features) • [Quick Start](#quick-start) • [Documentation](#documentation) • [Examples](#examples) • [Comparison](#comparison)

</div>

---

## Overview

PlexSpaces is a distributed actor framework that unifies the best patterns from Erlang/OTP, Orleans, Temporal, and modern serverless architectures. It provides a single, powerful abstraction for building durable workflows, stateful microservices, distributed ML workloads, and edge computing applications.

### What Makes PlexSpaces Unique

- **Five Foundational Pillars**: TupleSpace coordination, Erlang/OTP supervision, durable execution, WASM runtime, and Firecracker isolation
- **Composable Abstractions**: One powerful actor model with dynamic facets instead of multiple specialized types
- **Location Transparency**: Actors work seamlessly across local processes, containers, and cloud regions
- **Polyglot Support**: Write actors in Rust, Python, JavaScript, or any language that compiles to WebAssembly
- **Production Ready**: Built-in observability, fault tolerance, and resource-aware scheduling

## Features

### 🎯 Core Capabilities

- **Durable Actors**: Stateful actors with automatic persistence and fault recovery
- **Virtual Actors**: Orleans-style activation/deactivation with automatic lifecycle management
- **Workflows**: Temporal-style durable workflows with exactly-once execution
- **TupleSpace Coordination**: Linda-style associative memory for decoupled communication
- **Supervision Trees**: Erlang/OTP-inspired fault tolerance with restart strategies
- **WASM Runtime**: Deploy actors written in any language that compiles to WebAssembly
- **Firecracker Isolation**: Run actors in lightweight microVMs for strong isolation

### 🧩 Abstractions

**Behaviors** (Compile-time patterns):
- **GenServerBehavior**: Erlang/OTP-style request/reply
- **GenFSMBehavior**: Finite state machine
- **GenEventBehavior**: Event-driven processing
- **WorkflowBehavior**: Durable workflow orchestration

**Facets** (Runtime capabilities):
- **Infrastructure**: VirtualActorFacet, DurabilityFacet, MobilityFacet
- **Capabilities**: HttpClientFacet, KeyValueFacet, BlobStorageFacet
- **Timers/Reminders**: TimerFacet, ReminderFacet
- **Observability**: MetricsFacet, TracingFacet, LoggingFacet
- **Security**: AuthenticationFacet, AuthorizationFacet
- **Events**: EventEmitterFacet

**Primitives**:
- **ActorRef**: Location-transparent actor references
- **ActorContext**: Service access for actors
- **TupleSpace**: Linda-style coordination
- **Channels**: Queue and topic patterns (InMemory, Redis, Kafka, SQLite, NATS, UDP)
- **Process Groups**: Group communication
- **Journaling**: Event sourcing and replay

### 🚀 Advanced Features

- **FaaS-Style Invocation**: HTTP-based actor invocation via `InvokeActor` RPC (GET for reads, POST/PUT for updates, DELETE for deletes)
  - **RESTful API**: `/api/v1/actors/{tenant_id}/{namespace}/{actor_type}` endpoint (or `/api/v1/actors/{namespace}/{actor_type}` without tenant_id)
  - **Namespace Support**: Organize actors by namespace for better isolation (defaults to "default")
  - **Tenant Defaulting**: Tenant ID defaults to "default" if not provided in path
  - **AWS Lambda URL Support**: Ready for integration with AWS Lambda Function URLs
  - **Serverless Patterns**: Invoke actors like serverless functions with automatic load balancing
- **Resource-Aware Scheduling**: Intelligent placement based on CPU, memory, and I/O profiles
- **Multi-Tenancy**: Built-in isolation contexts for secure multi-tenant deployments
- **Event Sourcing**: Complete audit trail with time-travel debugging
- **Distributed Coordination**: Actor groups, process groups, and distributed locks
- **Observability**: Built-in metrics, tracing, and health checks
- **gRPC-First**: All APIs defined in Protocol Buffers for type safety and multi-language support
- **Capability Providers**: HTTP, KeyValue, BlobStorage facets for I/O operations
- **Security Facets**: Authentication, authorization, and encryption support
- **Event-Driven**: EventEmitter facet for reactive programming patterns
- **Graceful Shutdown**: Actors using non-memory channels stop accepting new messages but complete in-progress work
- **UDP Multicast Channels**: Low-latency pub/sub for cluster-wide messaging
- **Cluster Configuration**: Node grouping via `cluster_name` for shared channels

## Design Philosophy

PlexSpaces follows three core principles:

1. **One Powerful Abstraction**: A unified actor model with composable capabilities beats multiple specialized types
2. **Elevate Research to Production**: Generalize proven research concepts (Linda, OTP, virtual actors) into production abstractions
3. **Composable Over Specialized**: Dynamic facets enable capabilities without creating new actor types

### Core Tenets

- **Proto-First**: All contracts defined in Protocol Buffers for cross-language compatibility
- **Location Transparency**: Actors work seamlessly across local processes, containers, and cloud regions
- **Fault Tolerance**: "Let it crash" philosophy with automatic recovery via supervision trees
- **Exactly-Once Semantics**: Durable execution with deterministic replay guarantees
- **Resource Awareness**: Intelligent scheduling based on declared resource profiles
- **Observability-First**: Built-in metrics, tracing, and health checks for production operations

### Five Foundational Pillars

1. **TupleSpace Coordination** (Linda Model): Decoupled communication via associative memory
2. **Erlang/OTP Philosophy**: Supervision trees, behaviors, and "let it crash" fault tolerance
3. **Durable Execution**: Restate-inspired journaling for exactly-once semantics and fault recovery
4. **WASM Runtime**: Portable, secure actors that run anywhere
5. **Firecracker Isolation**: MicroVM-level isolation for security and resource management

## Quick Start

Get PlexSpaces running in under 5 minutes:

### 1. Install

```bash
# Using Docker (recommended)
docker run -p 8080:8080 -p 8000:8000 -p 8001:8001 plexspaces/node:latest

# Or build from source
git clone https://github.com/plexobject/plexspaces.git
cd plexspaces && make build
```

### 2. Your First Actor

```rust
use plexspaces::*;
use plexspaces_behavior::GenServerBehavior;

struct Counter {
    count: i32,
}

#[async_trait]
impl GenServerBehavior for Counter {
    type Request = i32;
    type Reply = i32;

    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        request: Self::Request,
    ) -> Result<Self::Reply, BehaviorError> {
        self.count += request;
        Ok(self.count)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a node
    let node = PlexSpacesNode::new("node1".to_string()).await?;
    
    // Spawn a counter actor using ActorFactory
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    
    let actor_id = "counter@node1".to_string();
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| "ActorFactory not found")?;
    let ctx = plexspaces_core::RequestContext::internal();
    let _message_sender = actor_factory.spawn_actor(
        &ctx,
        &actor_id,
        "Counter", // actor_type
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![], // facets (empty for simple actor)
    ).await?;
    
    // Get actor reference (location-transparent)
    let actor_ref = plexspaces_actor::ActorRef::remote(
        actor_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );
    let reply = actor_ref.ask(5, Duration::from_secs(5)).await?;
    println!("Count: {}", reply);
    
    Ok(())
}
```

**That's it!** You've created your first actor. See the [Getting Started Guide](docs/getting-started.md) for more examples.

### 3. Next Steps

- 📖 [Concepts Guide](docs/concepts.md) - Learn Actors, Behaviors, Facets, and more
- 🚀 [Examples](examples/README.md) - Explore real-world patterns
- 🏗️ [Architecture](docs/architecture.md) - Understand the system design

### Python Actor Example

```python
# counter_actor.py
from plexspaces import Actor, tell, ask

class CounterActor(Actor):
    def __init__(self):
        self.count = 0
    
    async def handle_increment(self, amount: int):
        self.count += amount
        return self.count

# Deploy and use
actor = CounterActor()
await actor.start("counter@node1")
result = await ask(actor, "increment", 5)
print(f"Count: {result}")
```

See the [Getting Started Guide](docs/getting-started.md) for detailed tutorials and the [Concepts Guide](docs/concepts.md) to understand the fundamentals.

## Use Cases

PlexSpaces excels at:

- **Durable Workflows**: Long-running business processes with automatic recovery
- **Stateful Microservices**: Services that maintain state across requests
- **Distributed ML Workloads**: Parameter servers, distributed training, and inference pipelines
- **Event Processing**: Real-time stream processing with exactly-once semantics
- **Game Servers**: Stateful game sessions with automatic migration and fault tolerance
- **Edge Computing**: Deploy actors to edge locations with automatic synchronization
- **FaaS Platforms**: Build serverless platforms with durable execution
  - **HTTP-Based Invocation**: Invoke actors via REST API (`GET /api/v1/actors/{tenant_id}/{namespace}/{actor_type}` or `/api/v1/actors/{namespace}/{actor_type}`)
  - **Namespace Support**: Organize actors by namespace within tenants for better isolation
  - **AWS Lambda Integration**: Ready for AWS Lambda Function URLs and API Gateway
  - **Serverless Functions**: Treat actors as serverless functions with automatic scaling

See [Use Cases](docs/use-cases.md) for detailed examples.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    PlexSpaces Node                      │
├─────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │   Actors     │  │  Workflows   │  │ TupleSpaces  │ │
│  │  (GenServer) │  │  (Durable)   │  │  (Linda)     │ │
│  └──────────────┘  └──────────────┘  └──────────────┘ │
│         │                  │                  │         │
│  ┌──────────────────────────────────────────────────┐  │
│  │         Actor Runtime & Supervision              │  │
│  └──────────────────────────────────────────────────┘  │
│         │                  │                  │         │
│  ┌──────────────────────────────────────────────────┐  │
│  │  Journaling  │  WASM Runtime  │  Firecracker    │  │
│  └──────────────────────────────────────────────────┘  │
│         │                  │                  │         │
│  ┌──────────────────────────────────────────────────┐  │
│  │         gRPC Services & Service Mesh             │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

See [Architecture Guide](docs/architecture.md) for detailed design.

## Examples

PlexSpaces includes comprehensive examples organized by complexity:

### Simple Examples
- **Timers**: Scheduled tasks and periodic operations
- **Durable Actors**: State persistence and recovery
- **WASM Calculator**: Polyglot actors in Python/JavaScript
- **Actor Groups**: Sharding and load distribution

### Intermediate Examples
- **Matrix Multiply**: Distributed computation
- **Heat Diffusion**: Multi-actor simulation
- **MPI Patterns**: Scatter-gather coordination

### Advanced Examples
- **Byzantine Generals**: Consensus algorithms
- **N-Body Simulation**: Complex physics simulation
- **Order Processing**: Real-world workflow orchestration

### Framework Comparisons
Side-by-side comparisons with 24+ frameworks:
- Erlang/OTP, Orleans, Temporal, Restate
- Ray, Cloudflare Workers, Azure Durable Functions
- AWS Step Functions, wasmCloud, Dapr
- And many more...

See [Examples](examples/README.md) for the complete list.

## Documentation

- **[Actor System](docs/actor-system.md)**: Comprehensive guide to the unified actor system - actors, supervisors, applications, facets, behaviors, lifecycle, linking/monitoring, and observability
- **[Getting Started](docs/getting-started.md)**: Quick start guide and tutorials
- **[Concepts](docs/concepts.md)**: Core concepts explained (Actors, Behaviors, Facets, TupleSpace, FaaS-Style Invocation, etc.)
- **[Architecture](docs/architecture.md)**: System design, abstractions, and primitives (including FaaS Invocation)
- **[Detailed Design](docs/detailed-design.md)**: Comprehensive component documentation with all facets, behaviors, APIs, and primitives (including InvokeActor Service)
- **[Installation](docs/installation.md)**: Docker, Kubernetes, and manual setup
- **[Testing](docs/testing.md)**: How to run unit tests, integration tests, and example tests
- **[WASM Deployment](docs/wasm-deployment.md)**: Deploy polyglot WASM applications (Rust, Python, TypeScript, Go)
- **[Use Cases](docs/use-cases.md)**: Real-world application patterns and use cases (including FaaS Platforms)
- **[Examples](docs/examples.md)**: Example gallery with feature matrix
- **[CLI Reference](docs/cli.md)**: Command-line tools and operations
- **[API Reference](https://docs.rs/plexspaces/)**: Full API documentation

**Complete Documentation**: All documentation is in the `docs/` directory. Crate-specific documentation is in `crates/*/README.md` with references to main docs.

## Comparison with Other Frameworks

PlexSpaces unifies patterns from multiple frameworks:

| Framework | Pattern | PlexSpaces Abstraction |
|-----------|---------|----------------------|
| Erlang/OTP | GenServer, Supervision | `GenServerBehavior`, `Supervisor` |
| Akka | Actor Model, Message Passing | `Actor`, `ActorRef`, `tell()`/`ask()` |
| Orleans | Virtual Actors | `VirtualActorFacet` |
| Temporal | Durable Workflows | `WorkflowBehavior`, `DurabilityFacet` |
| Restate | Durable Execution | `DurabilityFacet`, `Journaling` |
| Ray | Distributed ML | `GenServerBehavior`, `TupleSpace` |
| Cloudflare Workers | Durable Objects | `VirtualActorFacet`, `DurabilityFacet` |

See [Framework Comparisons](examples/comparison/README.md) for detailed side-by-side examples.

## Installation

### Docker (Recommended)

```bash
docker pull plexspaces/node:latest
docker run -p 8080:8080 plexspaces/node:latest
```

### Kubernetes

```bash
kubectl apply -f k8s/deployment.yaml
```

### From Source

```bash
git clone https://github.com/plexobject/plexspaces.git
cd plexspaces
cargo build --release
```

See [Installation Guide](docs/installation.md) for detailed instructions.

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Setup

```bash
# Clone and build
git clone https://github.com/plexobject/plexspaces.git
cd plexspaces
make build

# Run tests
make test  # Includes unit tests and integration tests (see docs/testing.md)

# Run examples
make test-examples

# Check code coverage
cargo tarpaulin --lib --fail-under 95
```

## License

PlexSpaces is licensed under the [GNU Lesser General Public License v2.1](LICENSE).

## Acknowledgments

PlexSpaces is the evolution of **JavaNow** (based on Actors/Linda memory model and MPI), a comprehensive parallel computing framework developed for my post-graduate research in the late 1990s. 

PlexSpaces incorporates patterns from:

- **Erlang/OTP**: Supervision trees and fault tolerance
- **Akka**: Actor model and message passing
- **Microsoft Orleans**: Virtual actors and activation patterns
- **Temporal**: Durable workflows and exactly-once execution
- **Restate**: Durable execution and journaling
- **Ray**: Distributed ML and resource scheduling
- **Cloudflare Workers**: Edge computing and Durable Objects

## Community

- **GitHub Issues**: [Report bugs and request features](https://github.com/plexobject/plexspaces/issues)
- **Discussions**: [Ask questions and share ideas](https://github.com/plexobject/plexspaces/discussions)
- **Documentation**: [Full documentation](https://plexspaces.readthedocs.io/)

---

<div align="center">

**Built with ❤️ by the PlexSpaces team**

[Website](https://plexspaces.io) • [Documentation](docs/) • [Examples](examples/) • [GitHub](https://github.com/plexobject/plexspaces)

</div>
