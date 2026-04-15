# Use Cases

PlexSpaces excels at building scalable, fault-tolerant distributed systems. This document outlines the primary use cases and application patterns.


## Durable Workflows

Long-running business processes with automatic recovery:

- **Order Processing**: Validate → Payment → Ship → Confirm
- **Data Pipelines**: Extract → Transform → Load
- **Approval Workflows**: Multi-step approval processes
- **Saga Patterns**: Distributed transactions with compensation

**Key Features**:
- Exactly-once execution
- Automatic recovery from failures
- Time-travel debugging
- Human-in-the-loop support

**Example**: See [Order Processing](../examples/domains/order-processing/) example.

## Stateful Microservices

Services that maintain state across requests:

- **Shopping Carts**: Per-user cart state
- **Game Sessions**: Player state and game logic
- **User Sessions**: Authentication and authorization state
- **Real-Time Collaboration**: Shared document state

**Key Features**:
- Automatic persistence
- Location transparency
- Fault tolerance
- Horizontal scaling

**Example**: See [WASM Calculator](../examples/simple/wasm_calculator/) example.

## Distributed ML Workloads

Machine learning training and inference:

- **Parameter Servers**: Distributed model training
- **Data Parallelism**: Parallel data processing
- **Model Serving**: Distributed inference
- **Hyperparameter Tuning**: Parallel experiment execution

**Key Features**:
- Resource-aware scheduling
- Elastic worker pools (checkout/checkin via host pool API; see [Parameter sweep (migrating_merlin)](../examples/python/apps/migrating_merlin/README.md))
- Fault tolerance
- Checkpointing and recovery

**Examples**: [Parameter sweep (migrating_merlin)](../examples/python/apps/migrating_merlin/README.md) (Python, Go, TypeScript, Rust) — elastic pool + tuple space work queue; [Ray Comparison](../examples/comparison/ray/) for Ray-style patterns.

## AI Agents and Workloads

Production AI agent systems with stateful, fault-tolerant, and multi-tenant execution:

- **Agentic RAG Pipelines**: Retrieve → Generate → Validate → Retry loops as durable workflows, with sharded indexing and guardrail validation
- **MCP Tool Calling**: Actors as Model Context Protocol (MCP) tool servers — tool discovery (`tools/list`), validated execution (`tools/call`), JSON-RPC 2.0 gateway with tenant isolation
- **Multi-Agent A2A Collaboration**: Orchestrator decomposes tasks, discovers specialist agents by capability, delegates via message passing, coordinates results via TupleSpace
- **LLM Orchestration**: Prompt chaining, routing (classify → dispatch), reflection loops (generate → judge → refine), LLM-as-Judge quality gates, and Evol-Instruct prompt mutation
- **Parallel Inference**: Shard-group scatter-gather, elastic pool checkout/checkin, MPI collectives (broadcast, reduce, allreduce, barrier) for distributed model serving and training
- **Resource-Aware Inference**: Label-based model routing (GPU/memory tier), per-tenant budget enforcement, cost-aware tier selection with GenFSM budget state machine
- **Circuit Breakers**: GenFSM-based resilience for LLM call failures — closed/open/half-open with automatic recovery via `send_after`

**Key Features**:
- Actor-per-agent isolation — each agent has dedicated state, mailbox, and failure boundary
- `virtual_actor` facet — agents activate on demand, deactivate when idle (zero cost at rest)
- `durability` facet — agents survive node restarts with per-stage checkpointing
- `timer` facet — scheduled retries, heartbeats, circuit-breaker timeouts
- `metrics` facet — every agent interaction auto-instrumented in Prometheus
- All four behavior types: GenServer (stateful tools), GenEvent (fire-and-forget audit), GenFSM (quality/budget gates), WorkflowActor (durable orchestration)
- Multi-tenant JWT isolation — tenant namespace enforced at every `host.Ask()` boundary
- Polyglot — Python, Go, TypeScript, Rust agents on the same runtime

**Examples**:
- [parallel_ai_inference](../examples/python/apps/parallel_ai_inference/README.md) — all 4 parallelization mechanisms + MPI collectives (Python)
- [mcp_tool_server](../examples/python/apps/mcp_tool_server/README.md) — MCP tool discovery and execution (Python)
- [agentic_rag_pipeline](../examples/go/apps/agentic_rag_pipeline/README.md) — durable RAG with guardrails (Go)
- [a2a_multi_agent](../examples/go/apps/a2a_multi_agent/README.md) — multi-agent A2A collaboration (Go)
- [resource_aware_inference](../examples/go/apps/resource_aware_inference/README.md) — cost-aware model routing (Go)
- [llm_workflow_orchestrator](../examples/typescript/apps/llm_workflow_orchestrator/README.md) — prompt chaining, routing, reflection, LLM-as-Judge (TypeScript)

See also: [AI Agents at Scale: 20 Production Patterns with PlexSpaces](../blog/ai-agents-at-scale-with-plexspaces.md)

## Event Processing

Real-time stream processing with exactly-once semantics:

- **Event Sourcing**: Complete audit trail
- **CQRS**: Command/Query separation
- **Stream Processing**: Real-time data transformation
- **Event-Driven Architecture**: Reactive systems

**Key Features**:
- Exactly-once processing
- Event replay
- Backpressure handling
- Scalable processing

**Example**: See [GenEvent Behavior](../examples/simple/gen_event_example/) example.

## Game Servers

Stateful game sessions with automatic migration:

- **Multiplayer Games**: Player state synchronization
- **Game Sessions**: Match state management
- **Leaderboards**: Real-time ranking updates
- **Inventory Systems**: Player item management

**Key Features**:
- Stateful actors per player/session
- Automatic migration for load balancing
- Fault tolerance (no lost progress)
- Low latency messaging

**Example**: See [Cloudflare Workers Comparison](../examples/comparison/cloudflare_workers/) example.

## Edge Computing

Deploy actors to edge locations with automatic synchronization:

- **CDN Logic**: Edge-side computation
- **IoT Coordination**: Device state management
- **Geographic Distribution**: Low-latency regional processing
- **Offline-First**: Sync when online

**Key Features**:
- Edge deployment
- Automatic synchronization
- Conflict resolution
- Low latency

**Example**: See [Edge Computing](../examples/advanced/edge_computing/) example.

## FaaS Platforms

Build serverless platforms with durable execution:

- **HTTP-Based Invocation**: Invoke actors via `AskReply` and `SendMessage` over REST paths such as `GET /api/v1/actors/{namespace}/{actor_type}` and `POST /api/v1/actors/{namespace}/{actor_type}`
- **AWS Lambda Integration**: Ready for AWS Lambda Function URLs and API Gateway
- **Serverless Functions**: Treat actors as serverless functions with automatic scaling
- **Multi-Tenant Isolation**: Built-in tenant-based access control
- **Load Balancing**: Automatic load distribution across actor instances

**Key Features**:
- FaaS-style HTTP invocation with explicit ask and tell endpoints
- AWS Lambda Function URL support
- API Gateway integration
- Automatic actor discovery by type
- Random load balancing when multiple actors exist
- Tenant isolation with JWT authentication

**Example**: See [Webhook Handler](../examples/rust/embedded/webhook_handler/) example.

**Documentation**:
- [Concepts: FaaS-Style Invocation](concepts.md#faas-style-invocation) - Core concepts
- [Architecture: FaaS Invocation](architecture.md#faas-invocation) - System design
- [Detailed Design: AskReply and SendMessage Services](detailed-design.md#askreply-and-sendmessage-services) - Implementation details

- **Function Orchestration**: Multi-function workflows
- **Stateful Functions**: Functions with memory
- **Long-Running Functions**: Beyond typical timeout limits
- **Function Composition**: Complex function pipelines

**Key Features**:
- Durable execution
- Automatic scaling
- Pay-per-use
- Multi-language support

**Example**: See [Azure Durable Functions Comparison](../examples/comparison/azure_durable_functions/) example.

## Scientific Computing

HPC workflows and simulations:

- **Workflow Orchestration**: Multi-step scientific pipelines
- **Distributed Simulations**: Parallel physics simulations
- **Data Processing**: Large-scale data analysis
- **ML Training**: Distributed model training

**Key Features**:
- Workflow orchestration
- Resource scheduling
- Checkpointing
- Fault tolerance

**Example**: See [Merlin Comparison](../examples/comparison/merlin/) and [eFlows4HPC Comparison](../examples/comparison/eflows4hpc/) examples.

## Microservice Orchestration

Coordinate multiple microservices:

- **Service Mesh**: Inter-service communication
- **Circuit Breakers**: Fault tolerance patterns
- **Load Balancing**: Request distribution
- **Service Discovery**: Dynamic service location

**Key Features**:
- Service coordination
- Fault tolerance
- Load balancing
- Health monitoring

**Example**: See [Dapr Comparison](../examples/comparison/dapr/) example.

## Real-Time Systems

Low-latency real-time applications:

- **Chat Systems**: Real-time messaging
- **Collaboration Tools**: Shared state synchronization
- **Live Updates**: Real-time data streaming
- **Gaming**: Low-latency game state

**Key Features**:
- Low latency messaging
- Real-time synchronization
- WebSocket support
- Pub/sub patterns

**Key Features**:
- Multiple backends: InMemory, Redis, Kafka, SQLite, NATS, UDP
- ACK/NACK semantics for reliable processing
- Dead Letter Queue (DLQ) for poisonous messages
- Graceful shutdown for non-memory channels
- Message recovery on restart
- UDP multicast for low-latency cluster messaging

**Example**: See [Channel Examples](../examples/simple/channel_example/) example.

## Comparison with Other Solutions

| Use Case | Traditional Solution | PlexSpaces Advantage |
|----------|---------------------|---------------------|
| AI Agents / MCP | LangChain, AutoGen, standalone MCP servers | Stateful agents, durable workflows, multi-tenancy, MPI collectives, polyglot — all in one framework |
| LLM Orchestration | LangGraph, Temporal | Four behavior types (GenServer/GenEvent/GenFSM/Workflow) composable via config; per-stage checkpointing |
| Parallel Inference | Ray, Triton | Native MPI collectives, shard-group scatter-gather, elastic pool — ~50μs WASM cold start vs ~100ms Ray |
| Distributed Training | Horovod, PyTorch DDP | Ring AllReduce native, parameter server built-in, no separate training cluster |
| Workflows | Temporal, AWS Step Functions | Unified with actors, better integration |
| Stateful Services | Redis, Memcached | Automatic persistence, fault tolerance |
| ML Workloads | Ray, Spark | Better resource scheduling, fault tolerance |
| Event Processing | Kafka, Pulsar | Exactly-once built-in, simpler model |
| Game Servers | Custom solutions | Automatic migration, fault tolerance |
| Edge Computing | Cloudflare Workers | Better state management, synchronization |
| FAAS | AWS Lambda, Azure Functions | Durable execution, longer timeouts |
| Scientific Computing | Slurm, HTCondor | Better workflow orchestration, fault tolerance |

## Getting Started

- [Getting Started Guide](getting-started.md): Learn the basics
- [Concepts Guide](concepts.md): Understand core concepts
- [Examples](../examples/README.md): Explore example applications
- [Architecture](architecture.md): Understand the system design
