# PlexSpaces Services Reference

This document provides detailed documentation for all PlexSpaces gRPC services and APIs.

## Table of Contents

1. [Overview](#overview)
2. [Core Services](#core-services)
   - [ActorService](#actorservice)
   - [NodeService](#nodeservice)
   - [ApplicationService](#applicationservice)
3. [Coordination Services](#coordination-services)
   - [TuplePlexSpaceService](#tupleplexspaceservice)
   - [ProcessGroupService](#processgroupservice)
   - [ChannelService](#channelservice)
4. [Workflow Services](#workflow-services)
   - [WorkflowService](#workflowservice)
   - [JournalService](#journalservice)
   - [TimerService](#timerservice)
5. [Infrastructure Services](#infrastructure-services)
   - [BlobService](#blobservice)
   - [KeyValueService](#keyvalueservice)
   - [ObjectRegistry](#objectregistry)
6. [Operational Services](#operational-services)
   - [MetricsService](#metricsservice)
   - [DashboardService](#dashboardservice)
   - [SchedulingService](#schedulingservice)
   - [SecurityService](#securityservice)
7. [Runtime Services](#runtime-services)
   - [WasmRuntimeService](#wasmruntimeservice)
   - [FirecrackerVmService](#firecrackerervmservice)
   - [PoolService](#poolservice)
8. [API Access](#api-access)

---

## Overview

PlexSpaces exposes remote functionality through gRPC services, with optional REST/HTTP gateway support via grpc-gateway annotations. The service layer is intentionally thin: framework behavior lives in the core crates and services translate HTTP/gRPC requests into calls on those shared implementations. All services follow these principles:

- **Proto-First Design**: All contracts defined in Protocol Buffers
- **Tenant Isolation**: All operations require `RequestContext` with tenant/namespace
- **Observability**: Built-in metrics, tracing, and health checks
- **Security**: mTLS support with configurable authentication
- **Thin Controllers**: gRPC/HTTP layers orchestrate request handling and delegate business behavior to the framework crates and ServiceLocator-registered services

### Service Architecture

```mermaid
graph TB
    subgraph Clients["Clients"]
        CLI["CLI"]
        Dashboard["Dashboard"]
        SDK["SDK / Local Decorators"]
        REST["REST/HTTP"]
    end
    
    subgraph Gateway["API Gateway"]
        gRPC["gRPC Server"]
        HTTP["HTTP Gateway"]
    end
    
    subgraph Core["Core Services"]
        Actor["ActorService"]
        Node["NodeService"]
        App["ApplicationService"]
    end
    
    subgraph Coordination["Coordination"]
        TS["TupleSpaceService"]
        PG["ProcessGroupService"]
        Ch["ChannelService"]
    end
    
    subgraph Workflow["Workflow"]
        WF["WorkflowService"]
        Journal["JournalService"]
        Timer["TimerService"]
    end
    
    subgraph Infrastructure["Infrastructure"]
        Blob["BlobService"]
        KV["KeyValueService"]
        OR["ObjectRegistry"]
    end
    
    CLI --> gRPC
    Dashboard --> HTTP
    SDK --> Core
    REST --> HTTP
    
    HTTP --> gRPC
    gRPC --> Core
    gRPC --> Coordination
    gRPC --> Workflow
    gRPC --> Infrastructure
    
    style Clients fill:#3b82f6,stroke:#60a5fa,stroke-width:2px,color:#fff
    style Gateway fill:#10b981,stroke:#34d399,stroke-width:2px,color:#000
    style Core fill:#7c3aed,stroke:#a78bfa,stroke-width:2px,color:#fff
    style Coordination fill:#ea580c,stroke:#fb923c,stroke-width:2px,color:#fff
    style Workflow fill:#dc2626,stroke:#ef4444,stroke-width:2px,color:#fff
    style Infrastructure fill:#0891b2,stroke:#22d3ee,stroke-width:2px,color:#000
```

---

## Core Services

### ActorService

**Proto**: `proto/plexspaces/v1/actors/actor_runtime.proto`

The ActorService provides comprehensive actor lifecycle management, messaging, and coordination.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `CreateActor` | Create a new actor | `CreateActorRequest` | `CreateActorResponse` |
| `GetActor` | Get actor state and metadata | `GetActorRequest` | `GetActorResponse` |
| `DeleteActor` | Delete/stop an actor | `DeleteActorRequest` | `DeleteActorResponse` |
| `SendMessage` | HTTP-style tell to actor (fire-and-forget) | `SendMessageRequest` | `SendMessageResponse` |
| `AskReply` | HTTP-style ask to actor (request-reply) | `AskReplyRequest` | `AskReplyResponse` |
| `LinkActor` | Create Erlang-style link | `LinkActorRequest` | `LinkActorResponse` |
| `UnlinkActor` | Remove link | `UnlinkActorRequest` | `UnlinkActorResponse` |
| `MonitorActor` | Monitor actor lifecycle | `MonitorActorRequest` | `MonitorActorResponse` |
| `DemonitorActor` | Remove monitor | `DemonitorActorRequest` | `DemonitorActorResponse` |
| `ListActors` | List actors with filtering | `ListActorsRequest` | `ListActorsResponse` |
| `StreamActors` | Stream actor updates | `StreamActorsRequest` | `stream Actor` |

#### FaaS-Style Invocation

The actor runtime exposes separate RPCs for ask and tell semantics:

- **`AskReply`** handles request-reply over `GET /api/v1/actors/{namespace}/{actor_type}`, `GET /api/v1/actors/{namespace}/{actor_type}/ask`, and `POST`/`PUT` on the `/ask` path.
- **`SendMessage`** handles fire-and-forget delivery over `POST`/`PUT /api/v1/actors/{namespace}/{actor_type}`.
- When authentication is enabled, `tenant_id` is derived from the JWT and is not part of the actor HTTP path.
- `GET` is not supported for `SendMessage`.

```
GET  /api/v1/actors/{namespace}/{actor_type}         - AskReply
GET  /api/v1/actors/{namespace}/{actor_type}/ask     - AskReply
POST /api/v1/actors/{namespace}/{actor_type}         - SendMessage
PUT  /api/v1/actors/{namespace}/{actor_type}         - SendMessage
POST /api/v1/actors/{namespace}/{actor_type}/ask     - AskReply
PUT  /api/v1/actors/{namespace}/{actor_type}/ask     - AskReply
```

### NodeService

**Proto**: `proto/plexspaces/v1/node/node.proto`

Node management, cluster operations, and health monitoring.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `GetReleaseSpec` | Get node configuration (secrets masked) | `GetReleaseSpecRequest` | `GetReleaseSpecResponse` |
| `RegisterNodes` | Register multiple nodes (batch) | `RegisterNodesRequest` | `RegisterNodesResponse` |
| `UnregisterNode` | Remove node from cluster | `UnregisterNodeRequest` | `UnregisterNodeResponse` |
| `ListConnectedNodes` | Paginated node list | `ListConnectedNodesRequest` | `ListConnectedNodesResponse` |
| `StreamConnectedNodes` | Stream for large clusters | `StreamConnectedNodesRequest` | `stream Node` |
| `GetMetrics` | Node CPU, memory, operational metrics | `GetMetricsRequest` | `NodeMetrics` |
| `CalculateCapacity` | Resource capacity calculation | `CalculateCapacityRequest` | `NodeCapacity` |
| `ListNodeApplications` | Applications on node | `ListNodeApplicationsRequest` | `ListNodeApplicationsResponse` |
| `GetHealth` | Node health status | `GetHealthRequest` | `GetHealthResponse` |
| `SendHeartbeat` | Heartbeat with capacity info | `SendHeartbeatRequest` | `SendHeartbeatResponse` |
| `ConnectNodes` | Connect to remote nodes (Erlang-style net_adm:ping) | `ConnectNodesRequest` | `ConnectNodesResponse` |
| `DisconnectNodes` | Disconnect from nodes | `DisconnectNodesRequest` | `DisconnectNodesResponse` |
| `Ping` | SWIM protocol ping for failure detection | `PingRequest` | `PingResponse` |

#### Node connectivity and seed nodes

**Where it lives**: Node connectivity is implemented and owned by **NodeServiceImpl**, not by ServiceLocator. The node creates `NodeServiceImpl`, registers itself as the connectivity instance via `register_node_connectivity(self)`, and passes it into **ApplicationServiceImpl** at construction so that application deploy can connect to `ApplicationSpec.seed_nodes`.

**Automatic connection**:

- **Node startup**: When the node starts, it connects to all addresses in `ReleaseSpec.node.cluster_seed_nodes` using `NodeServiceImpl::connect_to_node_addresses`. This runs after the gRPC server is up.
- **Application deploy**: When an application is deployed with `ApplicationSpec.seed_nodes` set, the deploy path uses the injected `NodeConnectivity` (the same NodeServiceImpl) to call `connect_to_node_addresses(seed_nodes)` so the node registry is populated before the app runs.
- **Seed-node registration model**: `connect_to_node_addresses` registers seed addresses synchronously in the node registry. SWIM direct ping then reconciles placeholder node IDs to the remote node's reported identity and updates the heartbeat timestamp. Cluster mismatches are removed during that reconciliation step.
- **Source of truth**: when shared DB mode is enabled, `NodeRegistry` treats `ObjectRegistry` as authoritative and only returns nodes from the same cluster whose `last_heartbeat` or `updated_at` is within the configured active-node window (default: 24 hours). SWIM supplements this with faster liveness updates but does not replace the backing-store view.
- **Context ownership**: node-registry uses system-owned request context for membership reads and writes so user tenant/namespace context cannot leak into cluster membership state.
- **Cluster labels**: node registrations persist cluster and placement labels in object metadata, and node-registry reconstructs `NodeRegistration.capabilities` from those metadata labels when it reads membership back from object-registry.

**Connect timeout**: For `connect_to_node_addresses` (and when the trait method is called with no timeout), the timeout is **1 second per address**, with a **minimum of 5 seconds** and **maximum of 5 minutes**. Explicit timeout on the gRPC `ConnectNodes` request is unchanged.

**Cluster matching**: NodeServiceImpl is constructed with a **local cluster name** (`local_cluster`): from `ReleaseSpec.node.cluster_name` when using `with_release_spec`, or empty when using `new()`. When connecting to an address by ping, the node is **registered only if** the remote node’s cluster (from the ping response) matches the effective local cluster (from release spec or `local_cluster`). If both are non-empty and different, the connection is rejected and not added to the registry.

**Already-registered check**: Before pinging, the node service checks whether an address or node ID is already registered (by node_id via `NodeRegistry::lookup_node`, or by exact address match via listing and comparing `node_address`). Already-registered entries are skipped. Address matching is **exact** (no scheme normalization).

#### Health-Aware Connection

The SDK provides `NodeClient` with production-grade health-aware connection:

- **Liveness Checks**: Uses `SystemService.liveness_probe()` before connecting
- **Readiness Checks**: Uses `SystemService.readiness_probe()` after connecting
- **Exponential Backoff**: Retry with jitter (prevents thundering herd)
- **Parallel Health Checks**: Efficient multi-node connection
- **Graceful Degradation**: Handles partial success gracefully

**SDK Usage**:
```rust
use plexspaces_sdk::NodeClient;

// Health-aware connection (checks liveness, waits for readiness)
let mut node_client = NodeClient::connect("http://localhost:8000").await?;

// Connect multiple nodes with health checks
let resp = node_client.connect_nodes(
    vec!["http://localhost:8001".to_string()],
    None,
    30,
).await?;
```

See [SDK Documentation](sdk.md#node-connectivity-health-aware-connection) for details.

#### Security Features

- `GetReleaseSpec` automatically masks sensitive fields (passwords, API keys, tokens)
- All operations require valid `RequestContext` with tenant isolation

### ApplicationService

**Proto**: `proto/plexspaces/v1/application/application.proto`

Application lifecycle management and deployment.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `DeployApplication` | Deploy application to node | `DeployApplicationRequest` | `DeployApplicationResponse` |
| `UndeployApplication` | Remove application | `UndeployApplicationRequest` | `UndeployApplicationResponse` |
| `GetApplicationStatus` | Get deployment status | `GetApplicationStatusRequest` | `GetApplicationStatusResponse` |
| `ListApplications` | List all applications | `ListApplicationsRequest` | `ListApplicationsResponse` |
| `StartApplication` | Start stopped application | `StartApplicationRequest` | `StartApplicationResponse` |
| `StopApplication` | Stop running application | `StopApplicationRequest` | `StopApplicationResponse` |
| `RestartApplication` | Restart application | `RestartApplicationRequest` | `RestartApplicationResponse` |
| `GetApplicationLogs` | Stream application logs | `GetApplicationLogsRequest` | `stream LogEntry` |

**Listing from clients**: Use gRPC `ListApplications` (for example `plexspaces list --node <gRPC host:port> --json`). Proto `google.api.http` on this RPC feeds generated OpenAPI (`make proto-openapi` → `docs/openapi.json`) and gateway-style tooling; the embedded node’s Axum gateway wires deploy/undeploy (and related) HTTP routes, not a separate list handler—list flows go through gRPC or the CLI.

---

## Coordination Services

### TuplePlexSpaceService

**Proto**: `proto/plexspaces/v1/tuplespace/tuplespace.proto`

Linda-style TupleSpace coordination for decoupled communication.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `Write` | Write tuple to space | `WriteRequest` | `WriteResponse` |
| `Read` | Read tuple (blocking) | `ReadRequest` | `ReadResponse` |
| `Take` | Take tuple (blocking, removes) | `TakeRequest` | `TakeResponse` |
| `ReadIfExists` | Non-blocking read | `ReadIfExistsRequest` | `ReadIfExistsResponse` |
| `TakeIfExists` | Non-blocking take | `TakeIfExistsRequest` | `TakeIfExistsResponse` |
| `Scan` | Scan tuples matching pattern | `ScanRequest` | `ScanResponse` |
| `Count` | Count matching tuples | `CountRequest` | `CountResponse` |
| `Subscribe` | Subscribe to tuple events | `SubscribeRequest` | `stream TupleEvent` |

#### Coordination Patterns

- **Spatial Decoupling**: Actors don't need to know each other
- **Temporal Decoupling**: Actors don't need to be active simultaneously
- **Pattern Matching**: Flexible tuple retrieval with wildcards

### ProcessGroupService

**Proto**: `proto/plexspaces/v1/process_groups/process_groups.proto`

OTP-style process groups for group communication.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `CreateGroup` | Create process group | `CreateGroupRequest` | `CreateGroupResponse` |
| `DeleteGroup` | Delete group | `DeleteGroupRequest` | `DeleteGroupResponse` |
| `JoinGroup` | Add actor to group | `JoinGroupRequest` | `JoinGroupResponse` |
| `LeaveGroup` | Remove actor from group | `LeaveGroupRequest` | `LeaveGroupResponse` |
| `GetGroupMembers` | List group members | `GetGroupMembersRequest` | `GetGroupMembersResponse` |
| `BroadcastToGroup` | Send to all members | `BroadcastToGroupRequest` | `BroadcastToGroupResponse` |
| `ListGroups` | List all groups | `ListGroupsRequest` | `ListGroupsResponse` |

### ChannelService

**Proto**: `proto/plexspaces/v1/channel/channel.proto`

Queue and topic messaging with multiple backends.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `CreateChannel` | Create channel | `CreateChannelRequest` | `CreateChannelResponse` |
| `DeleteChannel` | Delete channel | `DeleteChannelRequest` | `DeleteChannelResponse` |
| `Publish` | Publish message | `PublishRequest` | `PublishResponse` |
| `Subscribe` | Subscribe to channel | `SubscribeRequest` | `stream ChannelMessage` |
| `Acknowledge` | Acknowledge message | `AcknowledgeRequest` | `AcknowledgeResponse` |
| `GetChannelStats` | Get channel statistics | `GetChannelStatsRequest` | `GetChannelStatsResponse` |

#### Supported Backends

- **InMemory**: Local development, single-node
- **Redis**: Distributed, pub/sub
- **Kafka**: High-throughput streaming
- **NATS**: Lightweight messaging
- **SQS**: AWS managed queues
- **UDP Multicast**: Cluster-wide low-latency

---

## Workflow Services

### WorkflowService

**Proto**: `proto/plexspaces/v1/workflow/workflow.proto`

Temporal-style durable workflow orchestration.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `StartWorkflow` | Start new workflow | `StartWorkflowRequest` | `StartWorkflowResponse` |
| `GetWorkflowStatus` | Get workflow state | `GetWorkflowStatusRequest` | `GetWorkflowStatusResponse` |
| `CancelWorkflow` | Cancel running workflow | `CancelWorkflowRequest` | `CancelWorkflowResponse` |
| `TerminateWorkflow` | Force terminate | `TerminateWorkflowRequest` | `TerminateWorkflowResponse` |
| `SignalWorkflow` | Send signal to workflow | `SignalWorkflowRequest` | `SignalWorkflowResponse` |
| `QueryWorkflow` | Query workflow state | `QueryWorkflowRequest` | `QueryWorkflowResponse` |
| `ListWorkflows` | List workflows | `ListWorkflowsRequest` | `ListWorkflowsResponse` |
| `GetWorkflowHistory` | Get execution history | `GetWorkflowHistoryRequest` | `GetWorkflowHistoryResponse` |

### JournalService

**Proto**: `proto/plexspaces/v1/journaling/journaling.proto`

Event sourcing and replay for durable execution.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `AppendEntry` | Append journal entry | `AppendEntryRequest` | `AppendEntryResponse` |
| `ReadEntries` | Read journal entries | `ReadEntriesRequest` | `ReadEntriesResponse` |
| `GetLatestCheckpoint` | Get checkpoint | `GetLatestCheckpointRequest` | `GetLatestCheckpointResponse` |
| `CreateCheckpoint` | Create checkpoint | `CreateCheckpointRequest` | `CreateCheckpointResponse` |
| `TruncateJournal` | Truncate old entries | `TruncateJournalRequest` | `TruncateJournalResponse` |

### TimerService

**Proto**: `proto/plexspaces/v1/journaling/timer.proto`

Durable timers and reminders.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `CreateTimer` | Create one-shot timer | `CreateTimerRequest` | `CreateTimerResponse` |
| `CreateReminder` | Create recurring reminder | `CreateReminderRequest` | `CreateReminderResponse` |
| `CancelTimer` | Cancel timer | `CancelTimerRequest` | `CancelTimerResponse` |
| `GetTimerStatus` | Get timer status | `GetTimerStatusRequest` | `GetTimerStatusResponse` |
| `ListTimers` | List actor timers | `ListTimersRequest` | `ListTimersResponse` |

---

## Infrastructure Services

### BlobService

**Proto**: `proto/plexspaces/v1/storage/blob.proto`

Binary large object storage with streaming support.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `Upload` | Upload blob | `UploadRequest` | `UploadResponse` |
| `Download` | Download blob | `DownloadRequest` | `DownloadResponse` |
| `Delete` | Delete blob | `DeleteRequest` | `DeleteResponse` |
| `GetMetadata` | Get blob metadata | `GetMetadataRequest` | `GetMetadataResponse` |
| `ListBlobs` | List blobs | `ListBlobsRequest` | `ListBlobsResponse` |
| `StreamUpload` | Streaming upload | `stream UploadChunk` | `UploadResponse` |
| `StreamDownload` | Streaming download | `DownloadRequest` | `stream DownloadChunk` |

#### Supported Backends

- **Local**: File system storage
- **S3**: AWS S3 / MinIO
- **GCS**: Google Cloud Storage
- **Azure**: Azure Blob Storage

### KeyValueService

**Proto**: `proto/plexspaces/v1/keyvalue/keyvalue.proto`

Multi-backend key-value storage for actor state, configuration, registries, leases, and metrics.

#### Architecture

The KeyValueStore has a layered trait design:

- **`plexspaces_common::KeyValueStore`**: Base trait for actors, facets, and WASM host (CRUD + TTL + CAS + increment). Defined in `common` crate to avoid circular dependencies.
- **`plexspaces_keyvalue::KeyValueStore`**: Extended trait for backend implementations (adds watch, multi_get, stats). Defined in the `keyvalue` crate.
- **`plexspaces_core`** re-exports the base trait from `common` for convenience.

Backends: SQLite (`:memory:` or file), PostgreSQL, Redis, DynamoDB, S3/Blob (all via feature flags).

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `Get` | Get value | `GetRequest` | `GetResponse` |
| `Put` | Put value | `PutRequest` | `PutResponse` |
| `Delete` | Delete key | `DeleteRequest` | `DeleteResponse` |
| `Exists` | Check if key exists | `ExistsRequest` | `ExistsResponse` |
| `List` | List keys with prefix | `ListRequest` | `ListResponse` |
| `MultiGet` | Batch get | `MultiGetRequest` | `MultiGetResponse` |
| `MultiPut` | Batch put | `MultiPutRequest` | `MultiPutResponse` |
| `PutWithTtl` | Put with TTL | `PutWithTtlRequest` | `PutWithTtlResponse` |
| `Cas` | Compare-and-swap | `CasRequest` | `CasResponse` |
| `Increment` | Atomic increment | `IncrementRequest` | `IncrementResponse` |

### ObjectRegistry

**Proto**: `proto/plexspaces/v1/registry/object_registry.proto`

Distributed object registry for service discovery.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `Register` | Register object | `RegisterRequest` | `RegisterResponse` |
| `Unregister` | Unregister object | `UnregisterRequest` | `UnregisterResponse` |
| `Lookup` | Lookup object | `LookupRequest` | `LookupResponse` |
| `List` | List objects | `ListRequest` | `ListResponse` |
| `Watch` | Watch for changes | `WatchRequest` | `stream WatchEvent` |

---

## Operational Services

### MetricsService

**Proto**: `proto/plexspaces/v1/metrics/metrics.proto`

Metrics collection and aggregation.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `RecordMetric` | Record single metric | `RecordMetricRequest` | `RecordMetricResponse` |
| `RecordMetrics` | Record batch | `RecordMetricsRequest` | `RecordMetricsResponse` |
| `GetMetrics` | Get metrics | `GetMetricsRequest` | `GetMetricsResponse` |
| `GetActorMetrics` | Get actor metrics | `GetActorMetricsRequest` | `GetActorMetricsResponse` |
| `StreamMetrics` | Stream metrics | `StreamMetricsRequest` | `stream MetricEvent` |

### DashboardService

**Proto**: `proto/plexspaces/v1/dashboard/dashboard.proto`

Dashboard and monitoring UI backend.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `GetClusterOverview` | Cluster summary | `GetClusterOverviewRequest` | `ClusterOverview` |
| `GetNodeDetails` | Node details | `GetNodeDetailsRequest` | `NodeDetails` |
| `GetActorDetails` | Actor details | `GetActorDetailsRequest` | `ActorDetails` |
| `GetSystemHealth` | System health | `GetSystemHealthRequest` | `SystemHealth` |

### SchedulingService

**Proto**: `proto/plexspaces/v1/scheduling/scheduling.proto`

Resource-aware actor scheduling and placement.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `ScheduleActor` | Schedule actor placement | `ScheduleActorRequest` | `ScheduleActorResponse` |
| `GetPlacement` | Get current placement | `GetPlacementRequest` | `GetPlacementResponse` |
| `RebalanceCluster` | Trigger rebalancing | `RebalanceClusterRequest` | `RebalanceClusterResponse` |

### SecurityService

**Proto**: `proto/plexspaces/v1/security/security.proto`

Security, authentication, and authorization.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `ValidateToken` | Validate JWT token | `ValidateTokenRequest` | `ValidateTokenResponse` |
| `RefreshToken` | Refresh token | `RefreshTokenRequest` | `RefreshTokenResponse` |
| `GetPermissions` | Get user permissions | `GetPermissionsRequest` | `GetPermissionsResponse` |
| `CheckPermission` | Check specific permission | `CheckPermissionRequest` | `CheckPermissionResponse` |

---

## Runtime Services

### WasmRuntimeService

**Proto**: `proto/plexspaces/v1/wasm/wasm.proto`

WebAssembly module deployment and execution.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `LoadModule` | Load WASM module | `LoadModuleRequest` | `LoadModuleResponse` |
| `UnloadModule` | Unload module | `UnloadModuleRequest` | `UnloadModuleResponse` |
| `ListModules` | List loaded modules | `ListModulesRequest` | `ListModulesResponse` |
| `GetModuleInfo` | Get module metadata | `GetModuleInfoRequest` | `GetModuleInfoResponse` |
| `InvokeFunction` | Invoke WASM function | `InvokeFunctionRequest` | `InvokeFunctionResponse` |

### FirecrackerVmService

**Proto**: `proto/plexspaces/v1/firecracker/firecracker.proto`

Firecracker microVM management for strong isolation.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `CreateVM` | Create microVM | `CreateVMRequest` | `CreateVMResponse` |
| `StartVM` | Start VM | `StartVMRequest` | `StartVMResponse` |
| `StopVM` | Stop VM | `StopVMRequest` | `StopVMResponse` |
| `DestroyVM` | Destroy VM | `DestroyVMRequest` | `DestroyVMResponse` |
| `GetVMStatus` | Get VM status | `GetVMStatusRequest` | `GetVMStatusResponse` |
| `ListVMs` | List all VMs | `ListVMsRequest` | `ListVMsResponse` |

### PoolService

**Proto**: `proto/plexspaces/v1/pool/pool.proto`

Actor pool management for load balancing.

#### RPCs

| Method | Description | Request | Response |
|--------|-------------|---------|----------|
| `CreatePool` | Create actor pool | `CreatePoolRequest` | `CreatePoolResponse` |
| `DeletePool` | Delete pool | `DeletePoolRequest` | `DeletePoolResponse` |
| `GetPoolStatus` | Get pool status | `GetPoolStatusRequest` | `GetPoolStatusResponse` |
| `ResizePool` | Resize pool | `ResizePoolRequest` | `ResizePoolResponse` |
| `RouteToPool` | Route message to pool | `RouteToPoolRequest` | `RouteToPoolResponse` |

---

## API Access

### Interactive API Documentation

Explore the PlexSpaces API interactively using Swagger UI:

[![API Documentation](https://img.shields.io/badge/API-Swagger%20UI-85EA2D?style=for-the-badge&logo=swagger)](https://petstore.swagger.io/?url=https://raw.githubusercontent.com/bhatti/PlexSpaces/refs/heads/master/docs/openapi.json)

### gRPC Client

```rust
use plexspaces_proto::actor::v1::actor_service_client::ActorServiceClient;

let mut client = ActorServiceClient::connect("http://localhost:50051").await?;

let request = tonic::Request::new(CreateActorRequest {
    actor_type: "counter".to_string(),
    tenant_id: "my-tenant".to_string(),
    namespace: "default".to_string(),
    ..Default::default()
});

let response = client.create_actor(request).await?;
```

### REST/HTTP Gateway

When HTTP gateway is enabled, all gRPC services are accessible via REST:

```bash
# Create actor via REST
curl -X POST http://localhost:8080/v1/actors \
  -H "Content-Type: application/json" \
  -H "X-Tenant-Id: my-tenant" \
  -d '{"actor_type": "counter", "namespace": "default"}'

# Invoke actor (FaaS-style)
curl -H "Authorization: Bearer $TOKEN" http://localhost:8080/api/v1/actors/default/counter
```

---

## See Also

- [Architecture Overview](architecture.md)
- [Getting Started](getting-started.md)
- [Detailed Design](detailed-design.md)
- [Use Cases](use-cases.md)
