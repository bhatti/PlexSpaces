# PlexSpaces

<div align="center">

**A unified distributed actor framework for building scalable, fault-tolerant systems**

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/plexobject/plexspaces)
[![API Docs](https://img.shields.io/badge/API-Swagger-green.svg)](https://petstore.swagger.io/?url=https://raw.githubusercontent.com/bhatti/tspaces/main/docs/openapi.json)

[Features](#features) • [Quick Start](#quick-start) • [Documentation](#documentation) • [Examples](#examples) • [Comparison](#comparison)

</div>

---

## Overview

PlexSpaces is a distributed actor framework that unifies the best patterns from Erlang/OTP, Orleans, Temporal, and modern serverless architectures. It provides a single, powerful abstraction for building durable workflows, stateful microservices, distributed ML workloads, and edge computing applications.

### What Makes PlexSpaces Unique

- **Five Foundational Pillars**: TupleSpace coordination, Erlang/OTP supervision, durable execution, WASM runtime, and Firecracker isolation
- **Composable Abstractions**: One powerful actor model with dynamic facets instead of multiple specialized types
- **Location Transparency**: Actors work seamlessly across local processes, containers, and cloud regions
- **Polyglot Support**: Write actors in Rust, Python, TypeScript, Go, or any language that compiles to WebAssembly
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
- **Python WASM durability**: Use `@actor(facets=["durability"])` and enable via `WasmConfig.durability_enabled` in release/node config — see [Durability](docs/durability.md#durability-facet-parameter-python-sdk) and [Bank Account example](examples/python/apps/bank_account/README.md)
- **Capabilities**: HttpClientFacet, KeyValueFacet, BlobStorageFacet
- **Timers/Reminders**: TimerFacet, ReminderFacet
- **Observability**: MetricsFacet, TracingFacet, LoggingFacet
- **Security**: AuthenticationFacet, AuthorizationFacet
- **Events**: EventEmitterFacet

**Primitives**:
- **ActorRef**: Location-transparent actor references (spawn **visibility** on **`tell` / `ask`**; **link/monitor** validate **local** operands against **`RequestContext`** namespace and tenant scope when auth is on, then **`ActorService`** for remote halves)
- **ActorContext**: Service access for actors
- **TupleSpace**: Linda-style coordination
- **Channels**: Queue and topic patterns (InMemory, Redis, Kafka, SQLite, NATS, UDP)
- **Process Groups**: Group communication
- **Journaling**: Event sourcing and replay

### 🚀 Advanced Features

- **Data-Parallel Actors (DPA-inspired)**: ShardGroup for data-parallel sharding with bulk updates, parallel map, and scatter-gather operations
  - **ShardGroup**: Partition data across multiple shards/actors with hash/consistent-hash/range strategies
  - **BulkUpdateShardGroup**: Bulk writes with eventual consistency (DPA UpdateFunction)
  - **MapShardGroup**: Parallel queries across all shards (DPA Map operator)
  - **ScatterGather**: Aggregation queries with fault tolerance (DPA Scatter-Gather)
  - **Unified SDK**: `ShardGroupClient` and `UnifiedShardGroupClient` for both WASM/internal and gRPC (optional feature)
  - **Resource-Based Routing**: Labels flow through to ActorResourceRequirements for intelligent node placement
- **FaaS-Style Invocation**: HTTP-based actor invocation via `AskReply` and `SendMessage`
  - **RESTful API**: `GET /api/v1/actors/{namespace}/{actor_type}` and `GET|POST|PUT /api/v1/actors/{namespace}/{actor_type}/ask` for request-reply, `POST`/`PUT /api/v1/actors/{namespace}/{actor_type}` for fire-and-forget
  - **Namespace Support**: Organize actors by namespace for better isolation (defaults to "default")
  - **Tenant Identity**: Tenant identity is derived from validated auth context when authentication is enabled
  - **AWS Lambda URL Support**: Ready for integration with AWS Lambda Function URLs
  - **Serverless Patterns**: Invoke actors like serverless functions with automatic load balancing
- **Resource-Aware Scheduling**: Intelligent placement based on CPU, memory, and I/O profiles
- **Multi-Tenancy**: Two-level isolation (tenant-id from auth + namespace from application/actor) for secure multi-tenant deployments
- **Event Sourcing**: Complete audit trail with time-travel debugging
- **Distributed Coordination**: Actor groups, process groups, and distributed locks
- **Observability**: Built-in metrics (unified Prometheus pipeline; see [Metrics](docs/metrics.md)), tracing, and health checks
- **gRPC-First**: All APIs defined in Protocol Buffers for type safety and multi-language support
- **Capability Providers**: HTTP, KeyValue, BlobStorage facets for I/O operations
- **Security Facets**: Authentication, authorization, and encryption support
- **Event-Driven**: EventEmitter facet for reactive programming patterns
- **Graceful Shutdown**: Actors using non-memory channels stop accepting new messages but complete in-progress work
- **UDP Multicast Channels**: Low-latency pub/sub for cluster-wide messaging
- **Cluster Configuration**: Node grouping via `cluster_name` for shared channels

### 🔧 Infrastructure Services

- **NodeService**: Comprehensive node management with metrics, health checks, and capacity calculation
- **NodeRegistry**: TTL-based caching with gossip protocol for efficient node discovery
- **Channel Factory**: Priority-based backend selection (Kafka → NATS → SQS → ProcessGroup → InMemory)
- **SecretMasker**: Automatic masking of passwords, API keys, and tokens in API responses
- **ServiceLocator**: Centralized service discovery with typed accessors for all services
- **BlobServiceTrait**: Type-safe blob storage access via ServiceLocator

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
docker run -p 8000:8000 plexspaces/node:latest

# Or build from source
git clone https://github.com/plexobject/plexspaces.git
cd plexspaces && make build
```

### 2. Your First Actor (Rust SDK)

```rust
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    NodeBuilder, RequestContext, ActorId,
    spawn_with_facets, call_message, json,
};
use std::sync::Arc;
use std::time::Duration;

// Define actor with SDK annotations (like Python decorators)
#[gen_server_actor]
struct Counter {
    count: i32,
}

impl Counter {
    fn new() -> Self { Self { count: 0 } }
}

// Define handlers - GenServer defaults to "call" (request-reply)
#[plexspaces_handlers]
impl Counter {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &plexspaces_sdk::ActorContext,
        msg: &plexspaces_sdk::Message,
    ) -> Result<serde_json::Value, plexspaces_sdk::BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)?;
        self.count += payload["amount"].as_i64().unwrap_or(1) as i32;
        Ok(json!({ "count": self.count }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create and start node
    let node = Arc::new(NodeBuilder::new("node1").build().await);
    let service_locator = node.service_locator();
    
    // Spawn node in background
    let node_clone = node.clone();
    tokio::spawn(async move { node_clone.start().await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Create request context (tenant isolation required)
    let ctx = RequestContext::new_without_auth("my-tenant".into(), "default".into());
    
    // Spawn actor using SDK helper
    let actor_ref = spawn_with_facets(
        &ctx, service_locator.clone(),
        "counter", "default",
        Counter::new(), vec![],
    ).await?;
    
    // Send request-reply message using SDK helper
    let request = call_message(json!({ "action": "increment", "amount": 5 }));
    let reply = actor_ref.ask(request, Duration::from_secs(5)).await?;
    let result: serde_json::Value = serde_json::from_slice(&reply.payload)?;
    println!("Count: {}", result["count"]);
    
    Ok(())
}
```

**That's it!** You've created your first actor. See the [Getting Started Guide](docs/getting-started.md) for more examples.

### 3. Next Steps

- 📖 [Concepts Guide](docs/concepts.md) - Learn Actors, Behaviors, Facets, and more
- 🚀 [Examples](examples/README.md) - Explore real-world patterns
- 🏗️ [Architecture](docs/architecture.md) - Understand the system design

### Python Actor Example (SDK)

```python
# counter_actor.py - Build to WASM with: plexspaces-py build counter_actor.py -o counter.wasm
from plexspaces import actor, state, handler

@actor
class CounterActor:
    count: int = state(default=0)
    
    @handler("increment")
    def increment(self, amount: int = 1) -> dict:
        self.count += amount
        return {"count": self.count}
    
    @handler("get")
    def get(self) -> dict:
        return {"count": self.count}
```

```bash
# Build and deploy
plexspaces-py build counter_actor.py -o counter.wasm
curl -X POST http://localhost:8094/api/v1/deploy \
  -F "namespace=default" -F "actor_type=counter" -F "wasm=@counter.wasm"

# Invoke via HTTP
curl "http://localhost:8094/api/v1/actors/default/counter/invoke?msg_type=increment" \
  -d '{"amount": 5}'
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
  - **HTTP-Based Invocation**: Invoke actors via REST API (`GET /api/v1/actors/{namespace}/{actor_type}` for request-reply and `POST`/`PUT /api/v1/actors/{namespace}/{actor_type}` for fire-and-forget)
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

### WASM Polyglot Apps
Real-world use cases deployed as WASM actors (Python, TypeScript, Go):
- **[Ray Parameter Server](examples/python/apps/migrating_ray/)** (Python): Distributed ML training with gradient aggregation
- **[Erlang/OTP Rate Limiter](examples/go/apps/migrating_erlang_otp/)** (Go): Sliding window rate limiting service
- **[Bank Account](examples/python/apps/bank_account/)** (Python): Durable actors with checkpointing
- **[Orleans Batch Predictor](examples/typescript/apps/migrating_orleans/)** (TypeScript): Virtual actor ML inference

### Framework Comparisons
Side-by-side comparisons with 24+ frameworks:
- Erlang/OTP, Orleans, Temporal, Restate
- Ray, Cloudflare Workers, Azure Durable Functions
- AWS Step Functions, wasmCloud, Dapr
- And many more...

See [Examples](examples/README.md) for the complete list.

## Documentation

- **[Actor System](docs/actor-system.md)**: Comprehensive guide to the unified actor system - actors, supervisors, applications, facets, behaviors, lifecycle, linking/monitoring, and observability
- **[Metrics](docs/metrics.md)**: Unified Prometheus pipeline, global recorder crate, and `MetricsService` alignment with the metrics unification plan
- **[Getting Started](docs/getting-started.md)**: Quick start guide and tutorials
- **[Concepts](docs/concepts.md)**: Core concepts explained (Actors, Behaviors, Facets, TupleSpace, FaaS-Style Invocation, etc.)
- **[Architecture](docs/architecture.md)**: System design, abstractions, and primitives (including FaaS Invocation)
- **[Detailed Design](docs/detailed-design.md)**: Comprehensive component documentation with all facets, behaviors, APIs, and primitives (including AskReply and SendMessage)
- **[Security](docs/security.md)**: Authentication (JWT for HTTP, mTLS for gRPC), tenant isolation, JWT claims and CLI token creation, middleware, and local testing (`PLEXSPACES_DISABLE_AUTH`)
- **[Installation](docs/installation.md)**: Docker, Kubernetes, and manual setup
- **[Testing](docs/testing.md)**: How to run unit tests, integration tests, and example tests
- **[WASM Deployment](docs/wasm-deployment.md)**: Deploy polyglot WASM applications (Rust, Python, TypeScript, Go)
- **[Use Cases](docs/use-cases.md)**: Real-world application patterns and use cases (including FaaS Platforms)
- **[Examples](examples/README.md)**: Example gallery by language
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
# Workspace crates + polyglot SDK builds; excludes embedded comparison crates in examples/ (Makefile: CARGO_EXCLUDE_EXAMPLES)
make build

# Run tests
make test  # Same workspace scope as make build + Go / TypeScript / Python SDK tests (see docs/testing.md)

# Run examples
make test-examples

# Check code coverage
cargo tarpaulin --lib --fail-under 95
```

### Testing Backend Initialization Logging

PlexSpaces logs detailed information when initializing storage backends. To see these logs:

```bash
# Start a node with INFO logging enabled
RUST_LOG=info cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8090

# Run an example with logging
cd examples/rust/embedded/durable_actor
RUST_LOG=info cargo run
```

Example output showing backend initialization:

```
INFO plexspaces_keyvalue: KeyValue storage initialized db_path="/tmp/plexspaces-test-node.db" table="kv_store" backend="SQLite"
INFO plexspaces_locks: Locks storage initialized db_url="sqlite:///tmp/plexspaces.db" table="locks" backend="SQLite"
INFO plexspaces_journaling: Journal storage initialized backend="InMemory"
INFO plexspaces_blob: Blob storage initialized backend="embedded" bucket="plexspaces" endpoint="http://localhost:9000"
```

Supported backends: SQLite, PostgreSQL, Redis, DynamoDB, embedded object store (rustfs)/S3/GCP/Azure (blob), InMemory.

## License

PlexSpaces is licensed under the [![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0).

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
