// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

// PlexSpaces Actor Runtime API
//
// ## Purpose
// Defines THE core actor abstraction for PlexSpaces - the fundamental unit of computation
// that unifies Virtual Actors (Orbit/Orleans), OTP GenServers (Erlang), Durable Workflows
// (Restate), and Mobile Agents (Voyager). Rather than creating 20 different actor types,
// PlexSpaces provides ONE powerful Actor with composable capabilities via Facets.
//
// ## Architecture Context
// This proto file is **foundational to ALL PlexSpaces pillars** - it's the substrate upon
// which everything else is built:
// - **Pillar 1 (TupleSpace)**: Actors use TupleSpace for decoupled coordination
// - **Pillar 2 (Erlang/OTP)**: Actors implement GenServer, GenEvent, Supervisor patterns
// - **Pillar 3 (Durability)**: All actor operations journaled for replay after failures
// - **Pillar 4 (WASM)**: Actors execute as WASM modules for language-agnostic behavior
// - **Pillar 5 (Firecracker)**: Actors run in isolated microVMs for security
//
// ### The Actor Philosophy: Generalized Abstractions
// **Problem**: How to support Virtual Actors, Mobile Agents, OTP GenServers, Workflows
// WITHOUT creating 20 different implementations?
//
// **Solution**: ONE powerful Actor type + composable Facets
// ```
// Virtual Actor = Actor + VirtualActorFacet (Orbit-inspired activation)
// Mobile Agent = Actor + MobilityFacet (Voyager-inspired migration)
// GenServer = Actor + OTPGenServerFacet (Erlang patterns)
// Durable Workflow = Actor + DurableExecutionFacet + WorkflowFacet (Restate patterns)
// Stateless Worker = Actor + StatelessWorkerFacet (Orleans-inspired pools)
// Data-Parallel = Actor + DataParallelFacet (NSDI'22 lattice actors)
// ```
//
// ### Integration with Other PlexSpaces Components
// - **Used by**: SupervisionService (fault tolerance), WorkflowService (orchestration),
//   MobilityService (agent migration), TupleSpaceService (coordination), NodeService (placement)
// - **Depends on**: common.proto (Metadata, Facet, RetryPolicy), facets.proto (capability management)
// - **Provides**: Core abstraction for ALL distributed computation in PlexSpaces
//
// ## Design Decisions
// - **Why actor_id remains a string field here**:
//   - Actor identity is defined structurally by plexspaces.common.v1.ActorId
//   - Runtime envelopes still carry the canonical string for storage and wire compatibility
//   - Format: "{name}//{actor_type}::{namespace}@{node_id}"
//
// - **Why separate ActorState enum**:
//   - Enables state machine validation (can't go from TERMINATED to ACTIVE)
//   - Supervision logic uses state for restart decisions
//   - Clear lifecycle tracking for observability
//   - Supports graceful activation/deactivation (Orbit-inspired)
//
// - **Why facets field in Actor message**:
//   - **Core features are static** (lifecycle, supervision, messaging) - always present
//   - **Extensions are dynamic** (mobility, metrics, workflows) - attached via facets
//   - See "Static vs Dynamic Design Principle" in CLAUDE.md
//   - Avoids proto bloat (don't add every possible field to Actor)
//   - Enables user-defined capabilities (custom facets for domain-specific features)
//
// - **Why priority as int32** (not just High/Low):
//   - Allows N-level priorities (100+ levels)
//   - Standard mapping: Signal(100), System(75), High(50), Normal(25), Low(0)
//   - Enables fine-grained control plane vs data plane separation (Quickwit-inspired)
//   - Custom priority schemes possible (e.g., deadline-based priorities)
//
// - **Why TTL on messages**:
//   - Prevents message accumulation in failure scenarios (actor offline for hours)
//   - Enables automatic cleanup of stale messages (e.g., 5-minute-old price quote)
//   - Bounded memory growth in mailboxes
//   - Supports time-sensitive workflows (don't process expired requests)
//
// - **Why resource requirements** (ResourceRequirements message):
//   - Quickwit-inspired resource contracts for intelligent scheduling
//   - Enables bin-packing placement (optimize node utilization)
//   - Prevents OOM by rejecting actors that don't fit
//   - Supports heterogeneous clusters (GPU nodes, high-memory nodes)
//
// - **Why placement hints** (PlacementHint message):
//   - Orleans-inspired placement strategies
//   - Supports affinity (co-locate related actors)
//   - Enables custom placement logic (e.g., data locality)
//   - Balances between random (load) and deterministic (affinity)
//
// - **Why stateless worker config** (StatelessWorkerConfig):
//   - Orleans-inspired worker pools for stateless operations
//   - Scales horizontally without coordination (no single-actor bottleneck)
//   - Load balancing strategies (round-robin, least-loaded, random)
//   - Auto-scaling based on load (min/max instances)
//
// - **Why data-parallel config** (DataParallelConfig):
//   - NSDI'22 lattice actors pattern for coordination-free parallelism
//   - Enables sharding (partition state across actors)
//   - Supports rebalancing when shards added/removed
//   - Use with STATE_MGMT_MODE_LATTICE for CRDT-based state
//
// ## Core Design Principles (Implementation Priorities)
//
// These principles guide ALL implementation decisions in PlexSpaces. They are listed in
// priority order - foundational principles come first, advanced features come later.
//
// ### 1. Remoting First (Erlang/OTP Parity)
// **Why**: Actor-to-actor communication is fundamental to the actor model. Without remoting,
// we cannot properly test distributed supervision, clustering, or any distributed features.
//
// - **Location Transparency**: Same API for local and remote actors
//   - `actor_ref.send(message)` works whether actor is in same process or different node
//   - ActorRef abstracts physical location (like Erlang's `node@actor` addressing)
//   - Example: `send("local-node/actor-123", msg)` vs `send("remote-node/actor-456", msg)`
//
// - **gRPC for All Distribution**: All cross-node communication uses gRPC
//   - ActorService.Send for fire-and-forget messaging
//   - ActorService.Ask for request-reply patterns
//   - ActorService.Stream for bidirectional streaming
//   - See implementation phases in CLAUDE.md (Weeks 1-8)
//
// - **Test-Driven with Byzantine Generals**: Distributed consensus algorithm validates everything
//   - Phase 1: Local actors in single process (4 generals, 1 traitor)
//   - Phase 2: Remote actors across processes/VMs (distributed consensus)
//   - Phase 3: Distributed TupleSpace for coordination
//   - Phase 4: Cross-node supervision and fault tolerance
//
// ### 2. WebAssembly as Universal Runtime
// **CORE PRINCIPLE**: All actors compile to WASM for portability and dynamic deployment.
//
// - **Portable Execution**: Same actor runs on Docker/Kubernetes/Firecracker
//   - Actor code compiled to WASM module (.wasm file)
//   - WASM module executed by wasmtime/wasmer runtime
//   - No platform-specific binaries (works on x86, ARM, etc.)
//
// - **Dynamic Deployment**: Send WASM code to nodes at runtime (like Java classloader)
//   - DeployActorCodeRequest sends WASM bytes over gRPC
//   - Node compiles and caches WASM module
//   - Multiple actors instantiated from same WASM module
//   - Example: Deploy 100 "general" actors from single WASM module
//
// - **Sandboxed Isolation**: WASM provides secure execution boundaries
//   - Memory isolation (actor can't access other actors' memory)
//   - Resource limits (CPU time, memory allocation)
//   - Capability-based security (only allowed host functions)
//   - See Phase 6 in CLAUDE.md (Week 11-12)
//
// ### 3. Service Mesh for Non-Functional Requirements (Istio-Inspired)
// **CORE PRINCIPLE**: Embed Istio-like capabilities for security, reliability, observability.
//
// - **Security**:
//   - mTLS: Mutual TLS for all node-to-node communication
//   - Authentication: Node identity verification via certificates
//   - Authorization: RBAC for actor operations (who can spawn, send, query)
//
// - **Reliability**:
//   - Circuit Breaker: Fail fast on unhealthy nodes (prevent cascading failures)
//   - Retry Policies: Exponential backoff for transient failures
//   - Timeout Policies: Per-operation deadlines (prevent hanging requests)
//   - Load Balancing: Round-robin, least-connections, weighted distribution
//
// - **Observability**:
//   - Distributed Tracing: OpenTelemetry spans track message flow
//   - Metrics: Prometheus format (actor count, message rate, latency percentiles)
//   - Access Logs: Structured JSON logs for all actor operations
//
// - **Deployment Pattern**:
//   - Embedded: Service mesh logic in same process (no sidecar overhead)
//   - Sidecar: Optional Envoy proxy for advanced features (future)
//   - See Phase 8 in CLAUDE.md (Week 15-16)
//
// ### 4. Deployment Flexibility (Docker/Kubernetes/Firecracker)
// **CORE PRINCIPLE**: Actors run anywhere - containers, VMs, or bare metal.
//
// - **Docker/Kubernetes**: Standard container orchestration
//   - Dockerfile for node images
//   - Helm charts for deployment
//   - Service discovery via DNS (k8s services)
//   - See Phase 5 in CLAUDE.md (Week 9-10)
//
// - **Firecracker MicroVMs**: Strong isolation with low overhead
//   - Boot VMs in < 125ms (custom kernel + rootfs)
//   - Run WASM actors inside Firecracker VMs
//   - VM-to-VM networking via TAP devices
//   - See Phase 7 in CLAUDE.md (Week 13-14)
//
// ### 5. Durability AFTER Distributed Foundation (Restate-Inspired)
// **Why**: Journaling only makes sense once remoting and deployment work.
//
// - **Event Sourcing**: All actor operations journaled for replay
// - **Deterministic Replay**: Recover to exact pre-crash state
// - **Side Effect Caching**: External calls cached to avoid duplication
// - **Durable Promises**: Promises that survive actor failures
// - See Phase 9 in CLAUDE.md (Week 17-18)
//
// ### 6. Mobile Agents (Voyager-Inspired)
// **CORE PRINCIPLE**: Actors migrate with state + code + journal.
//
// - **State Migration**: Serialize and transfer actor state
// - **Code Migration**: Transfer WASM module to destination
// - **Journal Migration**: Move execution history for replay
// - **Resume Execution**: Continue from last journal entry after migration
// - See Phase 11 in CLAUDE.md (Week 21-22)
//
// ## Actor Lifecycle Examples
//
// ### Basic Actor Creation
// ```protobuf
// CreateActorRequest {
//   actor_type: "counter"
//   config: {
//     enable_persistence: true
//     checkpoint_interval: { seconds: 60 }
//     supervision_strategy: SUPERVISION_STRATEGY_ONE_FOR_ONE
//   }
// }
// // Result: Actor created in CREATING state, transitions to ACTIVATING -> ACTIVE
// ```
//
// ### Virtual Actor (Orbit-Inspired Auto-Activation)
// ```protobuf
// CreateActorRequest {
//   actor_type: "user-session"
//   config: { /* ... */ }
// }
// // Attach VirtualActorFacet to enable auto-activation
// AttachFacetRequest {
//   actor_id: "user-session-123"
//   facet_type: "virtual_actor"
//   config: {
//     "activation_strategy": "lazy",        // Activate on first message
//     "deactivation_timeout": "5m"          // Deactivate after 5 min idle
//   }
// }
// // Actor auto-activates on first message, auto-deactivates when idle
// ```
//
// ### Stateless Worker Pool (Orleans-Inspired)
// ```protobuf
// CreateActorRequest {
//   actor_type: "image-processor"
//   config: {
//     stateless_worker_config: {
//       max_instances: 100
//       min_instances: 5
//       strategy: LOAD_BALANCING_LEAST_LOADED
//     }
//     placement_hint: {
//       strategy: PLACEMENT_STRATEGY_RESOURCE_BASED
//       requirements: {
//         min_memory_mb: 512
//         min_cpu_cores: 1.0
//       }
//     }
//   }
// }
// // System auto-scales between 5-100 instances based on load
// ```
//
// ### Data-Parallel Actor Group (NSDI'22 Pattern)
// ```protobuf
// CreateActorRequest {
//   actor_type: "counter-shard"
//   config: {
//     data_parallel_config: {
//       group_id: "global-counter"
//       shard_count: 16
//       shard_id: 0  // Create shard 0
//       partition_strategy: PARTITION_STRATEGY_HASH
//       rebalance_policy: REBALANCE_POLICY_LOAD_BASED
//     }
//     state_management_mode: STATE_MGMT_MODE_LATTICE
//     consistency_level: CONSISTENCY_LEVEL_EVENTUAL
//   }
// }
// // Create 16 shards, each handles 1/16th of key space
// // State merges via CRDT (coordination-free)
// ```
//
// ## Message Passing Examples
//
// ### Fire-and-Forget (Async)
// ```protobuf
// SendMessageRequest {
//   message: {
//     id: "msg-001"
//     sender_id: "actor-a"
//     receiver_id: "actor-b"
//     message_type: "increment"
//     payload: [serialized data]
//     priority: 25  // Normal priority
//   }
//   wait_for_response: false
// }
// ```
//
// ### Request-Reply (Sync, Erlang gen_server:call pattern)
// ```protobuf
// SendMessageRequest {
//   message: {
//     id: "msg-002"
//     sender_id: "actor-a"
//     receiver_id: "actor-b"
//     message_type: "get_count"
//     payload: []
//     priority: 50  // High priority
//   }
//   wait_for_response: true
//   timeout: { seconds: 5 }
// }
// // Response: SendMessageResponse { message_id: "msg-002", response: {...} }
// ```
//
// ### Time-Sensitive Message (TTL)
// ```protobuf
// SendMessageRequest {
//   message: {
//     id: "msg-003"
//     receiver_id: "trader"
//     message_type: "price_quote"
//     payload: [quote data]
//     ttl: { seconds: 30 }  // Expire after 30 seconds
//   }
// }
// // Message dropped if not processed within 30s
// ```
//
// ## Actor State Transitions
//
// ### Normal Lifecycle
// ```
// CREATING (spawn)
//    ↓
// ACTIVATING (on_activate hook)
//    ↓
// ACTIVE (processing messages)
//    ↓
// DEACTIVATING (on_deactivate hook)
//    ↓
// INACTIVE (idle but restorable)
//    ↓
// TERMINATED (permanent deletion)
// ```
//
// ### Failure & Recovery
// ```
// ACTIVE (processing message)
//    ↓ [crash]
// FAILED
//    ↓ [supervisor restart]
// ACTIVATING (replay journal)
//    ↓
// ACTIVE (resume execution)
// ```
//
// ### Migration
// ```
// ACTIVE (on node-1)
//    ↓ [migrate request]
// MIGRATING (serialize state + journal)
//    ↓ [transfer to node-2]
// ACTIVATING (on node-2, restore state)
//    ↓
// ACTIVE (on node-2)
// ```

// Code generated by protoc-gen-go. DO NOT EDIT.
// versions:
// 	protoc-gen-go v1.36.11
// 	protoc        (unknown)
// source: plexspaces/v1/actors/actor_runtime.proto

package actorv1

import (
	_ "buf.build/gen/go/bufbuild/protovalidate/protocolbuffers/go/buf/validate"
	_ "github.com/grpc-ecosystem/grpc-gateway/v2/protoc-gen-openapiv2/options"
	v1 "github.com/plexobject/plexspaces/sdks/go/plexspaces/proto/plexspaces/v1"
	supervision "github.com/plexobject/plexspaces/sdks/go/plexspaces/proto/plexspaces/v1/supervision"
	_ "google.golang.org/genproto/googleapis/api/annotations"
	protoreflect "google.golang.org/protobuf/reflect/protoreflect"
	protoimpl "google.golang.org/protobuf/runtime/protoimpl"
	anypb "google.golang.org/protobuf/types/known/anypb"
	durationpb "google.golang.org/protobuf/types/known/durationpb"
	_ "google.golang.org/protobuf/types/known/structpb"
	timestamppb "google.golang.org/protobuf/types/known/timestamppb"
	reflect "reflect"
	sync "sync"
	unsafe "unsafe"
)

const (
	// Verify that this generated code is sufficiently up-to-date.
	_ = protoimpl.EnforceVersion(20 - protoimpl.MinVersion)
	// Verify that runtime/protoimpl is sufficiently up-to-date.
	_ = protoimpl.EnforceVersion(protoimpl.MaxVersion - 20)
)

// Placement strategy for actor activation (Orleans-inspired)
type PlacementStrategy int32

const (
	PlacementStrategy_PLACEMENT_STRATEGY_UNSPECIFIED    PlacementStrategy = 0
	PlacementStrategy_PLACEMENT_STRATEGY_RANDOM         PlacementStrategy = 1  // Random node selection
	PlacementStrategy_PLACEMENT_STRATEGY_PREFER_LOCAL   PlacementStrategy = 2  // Co-locate with caller
	PlacementStrategy_PLACEMENT_STRATEGY_LOAD_BASED     PlacementStrategy = 3  // Balance by load
	PlacementStrategy_PLACEMENT_STRATEGY_RESOURCE_BASED PlacementStrategy = 4  // Based on resource availability
	PlacementStrategy_PLACEMENT_STRATEGY_AFFINITY       PlacementStrategy = 5  // Affinity groups
	PlacementStrategy_PLACEMENT_STRATEGY_CUSTOM         PlacementStrategy = 99 // User-defined
)

// Enum value maps for PlacementStrategy.
var (
	PlacementStrategy_name = map[int32]string{
		0:  "PLACEMENT_STRATEGY_UNSPECIFIED",
		1:  "PLACEMENT_STRATEGY_RANDOM",
		2:  "PLACEMENT_STRATEGY_PREFER_LOCAL",
		3:  "PLACEMENT_STRATEGY_LOAD_BASED",
		4:  "PLACEMENT_STRATEGY_RESOURCE_BASED",
		5:  "PLACEMENT_STRATEGY_AFFINITY",
		99: "PLACEMENT_STRATEGY_CUSTOM",
	}
	PlacementStrategy_value = map[string]int32{
		"PLACEMENT_STRATEGY_UNSPECIFIED":    0,
		"PLACEMENT_STRATEGY_RANDOM":         1,
		"PLACEMENT_STRATEGY_PREFER_LOCAL":   2,
		"PLACEMENT_STRATEGY_LOAD_BASED":     3,
		"PLACEMENT_STRATEGY_RESOURCE_BASED": 4,
		"PLACEMENT_STRATEGY_AFFINITY":       5,
		"PLACEMENT_STRATEGY_CUSTOM":         99,
	}
)

func (x PlacementStrategy) Enum() *PlacementStrategy {
	p := new(PlacementStrategy)
	*p = x
	return p
}

func (x PlacementStrategy) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (PlacementStrategy) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[0].Descriptor()
}

func (PlacementStrategy) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[0]
}

func (x PlacementStrategy) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use PlacementStrategy.Descriptor instead.
func (PlacementStrategy) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{0}
}

type LoadBalancingStrategy int32

const (
	LoadBalancingStrategy_LOAD_BALANCING_UNSPECIFIED  LoadBalancingStrategy = 0
	LoadBalancingStrategy_LOAD_BALANCING_ROUND_ROBIN  LoadBalancingStrategy = 1
	LoadBalancingStrategy_LOAD_BALANCING_LEAST_LOADED LoadBalancingStrategy = 2
	LoadBalancingStrategy_LOAD_BALANCING_RANDOM       LoadBalancingStrategy = 3
)

// Enum value maps for LoadBalancingStrategy.
var (
	LoadBalancingStrategy_name = map[int32]string{
		0: "LOAD_BALANCING_UNSPECIFIED",
		1: "LOAD_BALANCING_ROUND_ROBIN",
		2: "LOAD_BALANCING_LEAST_LOADED",
		3: "LOAD_BALANCING_RANDOM",
	}
	LoadBalancingStrategy_value = map[string]int32{
		"LOAD_BALANCING_UNSPECIFIED":  0,
		"LOAD_BALANCING_ROUND_ROBIN":  1,
		"LOAD_BALANCING_LEAST_LOADED": 2,
		"LOAD_BALANCING_RANDOM":       3,
	}
)

func (x LoadBalancingStrategy) Enum() *LoadBalancingStrategy {
	p := new(LoadBalancingStrategy)
	*p = x
	return p
}

func (x LoadBalancingStrategy) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (LoadBalancingStrategy) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[1].Descriptor()
}

func (LoadBalancingStrategy) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[1]
}

func (x LoadBalancingStrategy) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use LoadBalancingStrategy.Descriptor instead.
func (LoadBalancingStrategy) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{1}
}

// Partition strategy for data-parallel actors
type PartitionStrategy int32

const (
	PartitionStrategy_PARTITION_STRATEGY_UNSPECIFIED     PartitionStrategy = 0
	PartitionStrategy_PARTITION_STRATEGY_HASH            PartitionStrategy = 1  // Hash-based partitioning (default)
	PartitionStrategy_PARTITION_STRATEGY_RANGE           PartitionStrategy = 2  // Range-based partitioning
	PartitionStrategy_PARTITION_STRATEGY_CONSISTENT_HASH PartitionStrategy = 3  // Consistent hashing
	PartitionStrategy_PARTITION_STRATEGY_CUSTOM          PartitionStrategy = 99 // User-defined partitioner
)

// Enum value maps for PartitionStrategy.
var (
	PartitionStrategy_name = map[int32]string{
		0:  "PARTITION_STRATEGY_UNSPECIFIED",
		1:  "PARTITION_STRATEGY_HASH",
		2:  "PARTITION_STRATEGY_RANGE",
		3:  "PARTITION_STRATEGY_CONSISTENT_HASH",
		99: "PARTITION_STRATEGY_CUSTOM",
	}
	PartitionStrategy_value = map[string]int32{
		"PARTITION_STRATEGY_UNSPECIFIED":     0,
		"PARTITION_STRATEGY_HASH":            1,
		"PARTITION_STRATEGY_RANGE":           2,
		"PARTITION_STRATEGY_CONSISTENT_HASH": 3,
		"PARTITION_STRATEGY_CUSTOM":          99,
	}
)

func (x PartitionStrategy) Enum() *PartitionStrategy {
	p := new(PartitionStrategy)
	*p = x
	return p
}

func (x PartitionStrategy) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (PartitionStrategy) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[2].Descriptor()
}

func (PartitionStrategy) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[2]
}

func (x PartitionStrategy) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use PartitionStrategy.Descriptor instead.
func (PartitionStrategy) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{2}
}

// Rebalancing policy for data-parallel actors
type RebalancePolicy int32

const (
	RebalancePolicy_REBALANCE_POLICY_UNSPECIFIED RebalancePolicy = 0
	RebalancePolicy_REBALANCE_POLICY_NONE        RebalancePolicy = 1 // No automatic rebalancing
	RebalancePolicy_REBALANCE_POLICY_ON_SCALE    RebalancePolicy = 2 // Rebalance when shards added/removed
	RebalancePolicy_REBALANCE_POLICY_LOAD_BASED  RebalancePolicy = 3 // Rebalance based on load metrics
)

// Enum value maps for RebalancePolicy.
var (
	RebalancePolicy_name = map[int32]string{
		0: "REBALANCE_POLICY_UNSPECIFIED",
		1: "REBALANCE_POLICY_NONE",
		2: "REBALANCE_POLICY_ON_SCALE",
		3: "REBALANCE_POLICY_LOAD_BASED",
	}
	RebalancePolicy_value = map[string]int32{
		"REBALANCE_POLICY_UNSPECIFIED": 0,
		"REBALANCE_POLICY_NONE":        1,
		"REBALANCE_POLICY_ON_SCALE":    2,
		"REBALANCE_POLICY_LOAD_BASED":  3,
	}
)

func (x RebalancePolicy) Enum() *RebalancePolicy {
	p := new(RebalancePolicy)
	*p = x
	return p
}

func (x RebalancePolicy) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (RebalancePolicy) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[3].Descriptor()
}

func (RebalancePolicy) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[3]
}

func (x RebalancePolicy) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use RebalancePolicy.Descriptor instead.
func (RebalancePolicy) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{3}
}

type ShardGroupState int32

const (
	ShardGroupState_SHARD_GROUP_STATE_UNSPECIFIED ShardGroupState = 0
	ShardGroupState_SHARD_GROUP_STATE_CREATING    ShardGroupState = 1 // Shards being created
	ShardGroupState_SHARD_GROUP_STATE_ACTIVE      ShardGroupState = 2 // All shards active
	ShardGroupState_SHARD_GROUP_STATE_REBALANCING ShardGroupState = 3 // Shards being rebalanced
	ShardGroupState_SHARD_GROUP_STATE_DRAINING    ShardGroupState = 4 // Shards being drained
	ShardGroupState_SHARD_GROUP_STATE_STOPPING    ShardGroupState = 5 // Shards being stopped
	ShardGroupState_SHARD_GROUP_STATE_STOPPED     ShardGroupState = 6 // All shards stopped
)

// Enum value maps for ShardGroupState.
var (
	ShardGroupState_name = map[int32]string{
		0: "SHARD_GROUP_STATE_UNSPECIFIED",
		1: "SHARD_GROUP_STATE_CREATING",
		2: "SHARD_GROUP_STATE_ACTIVE",
		3: "SHARD_GROUP_STATE_REBALANCING",
		4: "SHARD_GROUP_STATE_DRAINING",
		5: "SHARD_GROUP_STATE_STOPPING",
		6: "SHARD_GROUP_STATE_STOPPED",
	}
	ShardGroupState_value = map[string]int32{
		"SHARD_GROUP_STATE_UNSPECIFIED": 0,
		"SHARD_GROUP_STATE_CREATING":    1,
		"SHARD_GROUP_STATE_ACTIVE":      2,
		"SHARD_GROUP_STATE_REBALANCING": 3,
		"SHARD_GROUP_STATE_DRAINING":    4,
		"SHARD_GROUP_STATE_STOPPING":    5,
		"SHARD_GROUP_STATE_STOPPED":     6,
	}
)

func (x ShardGroupState) Enum() *ShardGroupState {
	p := new(ShardGroupState)
	*p = x
	return p
}

func (x ShardGroupState) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ShardGroupState) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[4].Descriptor()
}

func (ShardGroupState) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[4]
}

func (x ShardGroupState) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ShardGroupState.Descriptor instead.
func (ShardGroupState) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{4}
}

// Node placement strategy for ShardGroup creation (leader-worker multi-node).
// When absent or SAME_NODE, all shards are created on the node that receives the RPC.
// FROM_REGISTRY uses NodeRegistry.list_nodes (optional cluster filter); NODE_IDS uses explicit list.
type NodePlacementStrategy int32

const (
	NodePlacementStrategy_NODE_PLACEMENT_STRATEGY_UNSPECIFIED   NodePlacementStrategy = 0 // Same as SAME_NODE: all shards on local node
	NodePlacementStrategy_NODE_PLACEMENT_STRATEGY_SAME_NODE     NodePlacementStrategy = 1
	NodePlacementStrategy_NODE_PLACEMENT_STRATEGY_FROM_REGISTRY NodePlacementStrategy = 2 // Round-robin shards across nodes from NodeRegistry
	NodePlacementStrategy_NODE_PLACEMENT_STRATEGY_NODE_IDS      NodePlacementStrategy = 3 // Round-robin shards across given node_ids
)

// Enum value maps for NodePlacementStrategy.
var (
	NodePlacementStrategy_name = map[int32]string{
		0: "NODE_PLACEMENT_STRATEGY_UNSPECIFIED",
		1: "NODE_PLACEMENT_STRATEGY_SAME_NODE",
		2: "NODE_PLACEMENT_STRATEGY_FROM_REGISTRY",
		3: "NODE_PLACEMENT_STRATEGY_NODE_IDS",
	}
	NodePlacementStrategy_value = map[string]int32{
		"NODE_PLACEMENT_STRATEGY_UNSPECIFIED":   0,
		"NODE_PLACEMENT_STRATEGY_SAME_NODE":     1,
		"NODE_PLACEMENT_STRATEGY_FROM_REGISTRY": 2,
		"NODE_PLACEMENT_STRATEGY_NODE_IDS":      3,
	}
)

func (x NodePlacementStrategy) Enum() *NodePlacementStrategy {
	p := new(NodePlacementStrategy)
	*p = x
	return p
}

func (x NodePlacementStrategy) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (NodePlacementStrategy) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[5].Descriptor()
}

func (NodePlacementStrategy) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[5]
}

func (x NodePlacementStrategy) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use NodePlacementStrategy.Descriptor instead.
func (NodePlacementStrategy) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{5}
}

// Aggregation strategy for scatter-gather results
type ShardGroupAggregationStrategy int32

const (
	ShardGroupAggregationStrategy_SHARD_GROUP_AGGREGATION_UNSPECIFIED ShardGroupAggregationStrategy = 0
	ShardGroupAggregationStrategy_SHARD_GROUP_AGGREGATION_CONCAT      ShardGroupAggregationStrategy = 1
	ShardGroupAggregationStrategy_SHARD_GROUP_AGGREGATION_MERGE       ShardGroupAggregationStrategy = 2
	ShardGroupAggregationStrategy_SHARD_GROUP_AGGREGATION_FIRST       ShardGroupAggregationStrategy = 3
	ShardGroupAggregationStrategy_SHARD_GROUP_AGGREGATION_MAJORITY    ShardGroupAggregationStrategy = 4
)

// Enum value maps for ShardGroupAggregationStrategy.
var (
	ShardGroupAggregationStrategy_name = map[int32]string{
		0: "SHARD_GROUP_AGGREGATION_UNSPECIFIED",
		1: "SHARD_GROUP_AGGREGATION_CONCAT",
		2: "SHARD_GROUP_AGGREGATION_MERGE",
		3: "SHARD_GROUP_AGGREGATION_FIRST",
		4: "SHARD_GROUP_AGGREGATION_MAJORITY",
	}
	ShardGroupAggregationStrategy_value = map[string]int32{
		"SHARD_GROUP_AGGREGATION_UNSPECIFIED": 0,
		"SHARD_GROUP_AGGREGATION_CONCAT":      1,
		"SHARD_GROUP_AGGREGATION_MERGE":       2,
		"SHARD_GROUP_AGGREGATION_FIRST":       3,
		"SHARD_GROUP_AGGREGATION_MAJORITY":    4,
	}
)

func (x ShardGroupAggregationStrategy) Enum() *ShardGroupAggregationStrategy {
	p := new(ShardGroupAggregationStrategy)
	*p = x
	return p
}

func (x ShardGroupAggregationStrategy) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ShardGroupAggregationStrategy) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[6].Descriptor()
}

func (ShardGroupAggregationStrategy) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[6]
}

func (x ShardGroupAggregationStrategy) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ShardGroupAggregationStrategy.Descriptor instead.
func (ShardGroupAggregationStrategy) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{6}
}

type CollectiveReduction int32

const (
	CollectiveReduction_COLLECTIVE_REDUCTION_UNSPECIFIED CollectiveReduction = 0
	CollectiveReduction_COLLECTIVE_REDUCTION_SUM         CollectiveReduction = 1
	CollectiveReduction_COLLECTIVE_REDUCTION_MIN         CollectiveReduction = 2
	CollectiveReduction_COLLECTIVE_REDUCTION_MAX         CollectiveReduction = 3
	CollectiveReduction_COLLECTIVE_REDUCTION_PRODUCT     CollectiveReduction = 4
	CollectiveReduction_COLLECTIVE_REDUCTION_CONCAT      CollectiveReduction = 5
	CollectiveReduction_COLLECTIVE_REDUCTION_BOOL_AND    CollectiveReduction = 6
	CollectiveReduction_COLLECTIVE_REDUCTION_BOOL_OR     CollectiveReduction = 7
)

// Enum value maps for CollectiveReduction.
var (
	CollectiveReduction_name = map[int32]string{
		0: "COLLECTIVE_REDUCTION_UNSPECIFIED",
		1: "COLLECTIVE_REDUCTION_SUM",
		2: "COLLECTIVE_REDUCTION_MIN",
		3: "COLLECTIVE_REDUCTION_MAX",
		4: "COLLECTIVE_REDUCTION_PRODUCT",
		5: "COLLECTIVE_REDUCTION_CONCAT",
		6: "COLLECTIVE_REDUCTION_BOOL_AND",
		7: "COLLECTIVE_REDUCTION_BOOL_OR",
	}
	CollectiveReduction_value = map[string]int32{
		"COLLECTIVE_REDUCTION_UNSPECIFIED": 0,
		"COLLECTIVE_REDUCTION_SUM":         1,
		"COLLECTIVE_REDUCTION_MIN":         2,
		"COLLECTIVE_REDUCTION_MAX":         3,
		"COLLECTIVE_REDUCTION_PRODUCT":     4,
		"COLLECTIVE_REDUCTION_CONCAT":      5,
		"COLLECTIVE_REDUCTION_BOOL_AND":    6,
		"COLLECTIVE_REDUCTION_BOOL_OR":     7,
	}
)

func (x CollectiveReduction) Enum() *CollectiveReduction {
	p := new(CollectiveReduction)
	*p = x
	return p
}

func (x CollectiveReduction) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (CollectiveReduction) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[7].Descriptor()
}

func (CollectiveReduction) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[7]
}

func (x CollectiveReduction) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use CollectiveReduction.Descriptor instead.
func (CollectiveReduction) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{7}
}

// State management mode (lattice-based data-parallel actors)
type StateMgmtMode int32

const (
	StateMgmtMode_STATE_MGMT_MODE_UNSPECIFIED StateMgmtMode = 0
	StateMgmtMode_STATE_MGMT_MODE_TRADITIONAL StateMgmtMode = 1 // Regular mutable state
	StateMgmtMode_STATE_MGMT_MODE_LATTICE     StateMgmtMode = 2 // Coordination-free lattice state (CRDT)
)

// Enum value maps for StateMgmtMode.
var (
	StateMgmtMode_name = map[int32]string{
		0: "STATE_MGMT_MODE_UNSPECIFIED",
		1: "STATE_MGMT_MODE_TRADITIONAL",
		2: "STATE_MGMT_MODE_LATTICE",
	}
	StateMgmtMode_value = map[string]int32{
		"STATE_MGMT_MODE_UNSPECIFIED": 0,
		"STATE_MGMT_MODE_TRADITIONAL": 1,
		"STATE_MGMT_MODE_LATTICE":     2,
	}
)

func (x StateMgmtMode) Enum() *StateMgmtMode {
	p := new(StateMgmtMode)
	*p = x
	return p
}

func (x StateMgmtMode) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (StateMgmtMode) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[8].Descriptor()
}

func (StateMgmtMode) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[8]
}

func (x StateMgmtMode) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use StateMgmtMode.Descriptor instead.
func (StateMgmtMode) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{8}
}

// Consistency level for lattice-based actors
type ConsistencyLevel int32

const (
	ConsistencyLevel_CONSISTENCY_LEVEL_UNSPECIFIED    ConsistencyLevel = 0
	ConsistencyLevel_CONSISTENCY_LEVEL_EVENTUAL       ConsistencyLevel = 1 // No ordering guarantees
	ConsistencyLevel_CONSISTENCY_LEVEL_CAUSAL         ConsistencyLevel = 2 // Causal consistency (vector clocks)
	ConsistencyLevel_CONSISTENCY_LEVEL_READ_COMMITTED ConsistencyLevel = 3 // Read committed isolation
	ConsistencyLevel_CONSISTENCY_LEVEL_LINEARIZABLE   ConsistencyLevel = 4 // Strict consistency (coordination required)
)

// Enum value maps for ConsistencyLevel.
var (
	ConsistencyLevel_name = map[int32]string{
		0: "CONSISTENCY_LEVEL_UNSPECIFIED",
		1: "CONSISTENCY_LEVEL_EVENTUAL",
		2: "CONSISTENCY_LEVEL_CAUSAL",
		3: "CONSISTENCY_LEVEL_READ_COMMITTED",
		4: "CONSISTENCY_LEVEL_LINEARIZABLE",
	}
	ConsistencyLevel_value = map[string]int32{
		"CONSISTENCY_LEVEL_UNSPECIFIED":    0,
		"CONSISTENCY_LEVEL_EVENTUAL":       1,
		"CONSISTENCY_LEVEL_CAUSAL":         2,
		"CONSISTENCY_LEVEL_READ_COMMITTED": 3,
		"CONSISTENCY_LEVEL_LINEARIZABLE":   4,
	}
)

func (x ConsistencyLevel) Enum() *ConsistencyLevel {
	p := new(ConsistencyLevel)
	*p = x
	return p
}

func (x ConsistencyLevel) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ConsistencyLevel) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[9].Descriptor()
}

func (ConsistencyLevel) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[9]
}

func (x ConsistencyLevel) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ConsistencyLevel.Descriptor instead.
func (ConsistencyLevel) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{9}
}

// Request to spawn an actor on a specific remote node (Erlang spawn/4 equivalent)
//
// ## Purpose
// Spawns an actor on a specified remote node using pre-deployed actor type.
// Returns an ActorRef for location-transparent messaging.
//
// ## Erlang Philosophy
// In Erlang:
// ```erlang
// % Local spawn (current node)
// Pid = spawn(Module, Function, Args)
//
// % Remote spawn (specific node)
// Pid = spawn(Node, Module, Function, Args)
// ```
// - Node: Target node (atom, e.g., 'worker@host1')
// - Module: Pre-compiled module on remote node (e.g., 'gen_server')
// - Function: Exported function to run (e.g., 'start_link')
// - Args: Arguments to function (e.g., [initial_state])
// - Returns: Pid that works for local and remote sends (location transparent)
//
// ## PlexSpaces Approach
// ```rust
// // Local spawn (current node)
// let actor = node.spawn_actor("worker", state, config).await?;
//
// // Remote spawn (specific node)
// let actor_ref = node.spawn_remote("node2", "worker", state, config).await?;
// ```
// - target_node_id: Target node (string, e.g., "node2")
// - actor_type: Pre-deployed actor type on remote node (string, e.g., "worker")
// - initial_state: Serialized initial state (bytes)
// - config: Actor configuration
// - Returns: ActorRef in format "actor_id@node_id" for location-transparent messaging
//
// ## Key Assumptions (Erlang-Compatible)
// 1. **Code Pre-Deployed**: The actor_type must already exist on the target node
//   - Like Erlang: Module must be loaded on remote node
//   - Use CreateActor first to deploy actor type, or ensure it's in node's registry
//
// 2. **Location Transparency**: Returned ActorRef works the same for local and remote sends
//   - Like Erlang: Pid works for both local send (Pid ! Msg) and remote send
//   - ActorRef.tell() / ActorRef.ask() automatically routes to correct node
//
// ## Extensibility for WASM (Future - Week 11-12)
// Phase 6 will add dynamic WASM deployment support via reserved fields:
// ```protobuf
//
//	oneof code_source {
//	  string actor_type = 2;        // Pre-deployed type (current)
//	  WasmModule wasm_module = 10;  // Deploy WASM on-the-fly (future)
//	  string wasm_url = 11;         // Fetch WASM from URL (future)
//	}
//
// ```
// This enables:
// - **Pre-deployed**: spawn_remote("node2", "worker", ...) - code already on node2
// - **Dynamic WASM**: spawn_remote_wasm("node2", wasm_bytes, ...) - deploy code + spawn
// - **URL-based**: spawn_remote_url("node2", "https://cdn/worker.wasm", ...) - fetch + deploy + spawn
//
// ## Use Cases
//  1. **Distributed Testing**: Spawn Byzantine generals on specific nodes
//     ```rust
//     for (i, node) in nodes.iter().enumerate() {
//     node.spawn_remote(&node.id, "general", state, config).await?;
//     }
//     ```
//  2. **Load Distribution**: Explicitly place workers across cluster
//     ```rust
//     let worker_node = pick_least_loaded_node();
//     worker_node.spawn_remote(&worker_node.id, "worker", state, config).await?;
//     ```
//  3. **Data Locality**: Spawn actor near data source
//     ```rust
//     let db_node = find_node_with_shard(shard_id);
//     db_node.spawn_remote(&db_node.id, "processor", state, config).await?;
//     ```
//  4. **Affinity**: Co-locate related actors on same node
//     ```rust
//     let parent_node = get_actor_node(&parent_id);
//     parent_node.spawn_remote(&parent_node.id, "child", state, config).await?;
//     ```
//
// ## Comparison to CreateActor
// | Feature | CreateActor | SpawnActor |
// |---------|-------------|------------------|
// | Node selection | Placement strategy / current node | Explicit target node |
// | Use case | General actor creation | Explicit remote placement |
// | Fallback | Can fall back to other nodes | Fails if target unavailable |
// | Erlang equivalent | spawn/3 (local) | spawn/4 (remote) |
// | Code deployment | Any node can have code | Target node must have code |
//
// ## Implementation Flow
// When node1 calls SpawnRemoteActor targeting node2:
// 1. node1 validates target_node_id exists in registry
// 2. node1 sends gRPC SpawnRemoteActor request to node2
// 3. node2 validates actor_type exists in local registry
// 4. node2 spawns actor locally: Actor::spawn(actor_type, state, config)
// 5. node2 returns ActorRef with format "actor-123@node2"
// 6. node1 caches remote ActorRef for future messaging
// 7. Subsequent tell/ask automatically routes via gRPC to node2
//
// Who may invoke tell/ask on the spawned actor (enforced in ActorRef).
// UNSPECIFIED is treated as PUBLIC by the runtime.
type ActorVisibility int32

const (
	ActorVisibility_ACTOR_VISIBILITY_UNSPECIFIED ActorVisibility = 0
	// Any tenant and namespace may message this actor.
	ActorVisibility_ACTOR_VISIBILITY_PUBLIC ActorVisibility = 1
	// Only callers in the same tenant_id may message; namespace may differ within the tenant.
	ActorVisibility_ACTOR_VISIBILITY_PROTECTED ActorVisibility = 2
	// Only callers matching both tenant_id and namespace may message.
	ActorVisibility_ACTOR_VISIBILITY_PRIVATE ActorVisibility = 3
)

// Enum value maps for ActorVisibility.
var (
	ActorVisibility_name = map[int32]string{
		0: "ACTOR_VISIBILITY_UNSPECIFIED",
		1: "ACTOR_VISIBILITY_PUBLIC",
		2: "ACTOR_VISIBILITY_PROTECTED",
		3: "ACTOR_VISIBILITY_PRIVATE",
	}
	ActorVisibility_value = map[string]int32{
		"ACTOR_VISIBILITY_UNSPECIFIED": 0,
		"ACTOR_VISIBILITY_PUBLIC":      1,
		"ACTOR_VISIBILITY_PROTECTED":   2,
		"ACTOR_VISIBILITY_PRIVATE":     3,
	}
)

func (x ActorVisibility) Enum() *ActorVisibility {
	p := new(ActorVisibility)
	*p = x
	return p
}

func (x ActorVisibility) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ActorVisibility) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[10].Descriptor()
}

func (ActorVisibility) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[10]
}

func (x ActorVisibility) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ActorVisibility.Descriptor instead.
func (ActorVisibility) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{10}
}

// / Monitor type (distinguishes Monitor from Link)
// /
// / ## Purpose
// / Distinguishes between one-way monitoring (Monitor) and two-way death propagation (Link).
// / This enables clear API separation and proper handling of each type.
// /
// / ## Erlang Philosophy
// / - **MONITOR**: One-way notification (Erlang `monitor/2`)
// /   - Supervisor gets notified when actor dies
// /   - Supervisor does NOT die when actor dies
// /   - Used for observability, health checks
// /
// / - **LINK**: Two-way death propagation (Erlang `link/1`)
// /   - If actor1 dies, actor2 automatically dies (cascading)
// /   - If actor2 dies, actor1 automatically dies (cascading)
// /   - Used for tight coupling, supervision trees
// /
// / ## Design Notes
// / - Monitors remain separate (existing functionality)
// / - Links enable cascading failures (new functionality)
// / - Supervision uses links internally (cohesive design)
type MonitorType int32

const (
	MonitorType_MONITOR_TYPE_UNSPECIFIED MonitorType = 0
	// / One-way monitoring (get notified, don't die)
	MonitorType_MONITOR_TYPE_MONITOR MonitorType = 1
	// / Two-way link (die together, cascading failures)
	MonitorType_MONITOR_TYPE_LINK MonitorType = 2
)

// Enum value maps for MonitorType.
var (
	MonitorType_name = map[int32]string{
		0: "MONITOR_TYPE_UNSPECIFIED",
		1: "MONITOR_TYPE_MONITOR",
		2: "MONITOR_TYPE_LINK",
	}
	MonitorType_value = map[string]int32{
		"MONITOR_TYPE_UNSPECIFIED": 0,
		"MONITOR_TYPE_MONITOR":     1,
		"MONITOR_TYPE_LINK":        2,
	}
)

func (x MonitorType) Enum() *MonitorType {
	p := new(MonitorType)
	*p = x
	return p
}

func (x MonitorType) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (MonitorType) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[11].Descriptor()
}

func (MonitorType) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[11]
}

func (x MonitorType) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use MonitorType.Descriptor instead.
func (MonitorType) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{11}
}

// Lifecycle event types for filtering
type LifecycleEventType int32

const (
	LifecycleEventType_LIFECYCLE_EVENT_TYPE_UNSPECIFIED  LifecycleEventType = 0
	LifecycleEventType_LIFECYCLE_EVENT_TYPE_CREATED      LifecycleEventType = 1
	LifecycleEventType_LIFECYCLE_EVENT_TYPE_STARTING     LifecycleEventType = 2
	LifecycleEventType_LIFECYCLE_EVENT_TYPE_ACTIVATED    LifecycleEventType = 3
	LifecycleEventType_LIFECYCLE_EVENT_TYPE_DEACTIVATING LifecycleEventType = 4
	LifecycleEventType_LIFECYCLE_EVENT_TYPE_DEACTIVATED  LifecycleEventType = 5
	LifecycleEventType_LIFECYCLE_EVENT_TYPE_TERMINATED   LifecycleEventType = 6
	LifecycleEventType_LIFECYCLE_EVENT_TYPE_FAILED       LifecycleEventType = 7
	LifecycleEventType_LIFECYCLE_EVENT_TYPE_MIGRATING    LifecycleEventType = 8
)

// Enum value maps for LifecycleEventType.
var (
	LifecycleEventType_name = map[int32]string{
		0: "LIFECYCLE_EVENT_TYPE_UNSPECIFIED",
		1: "LIFECYCLE_EVENT_TYPE_CREATED",
		2: "LIFECYCLE_EVENT_TYPE_STARTING",
		3: "LIFECYCLE_EVENT_TYPE_ACTIVATED",
		4: "LIFECYCLE_EVENT_TYPE_DEACTIVATING",
		5: "LIFECYCLE_EVENT_TYPE_DEACTIVATED",
		6: "LIFECYCLE_EVENT_TYPE_TERMINATED",
		7: "LIFECYCLE_EVENT_TYPE_FAILED",
		8: "LIFECYCLE_EVENT_TYPE_MIGRATING",
	}
	LifecycleEventType_value = map[string]int32{
		"LIFECYCLE_EVENT_TYPE_UNSPECIFIED":  0,
		"LIFECYCLE_EVENT_TYPE_CREATED":      1,
		"LIFECYCLE_EVENT_TYPE_STARTING":     2,
		"LIFECYCLE_EVENT_TYPE_ACTIVATED":    3,
		"LIFECYCLE_EVENT_TYPE_DEACTIVATING": 4,
		"LIFECYCLE_EVENT_TYPE_DEACTIVATED":  5,
		"LIFECYCLE_EVENT_TYPE_TERMINATED":   6,
		"LIFECYCLE_EVENT_TYPE_FAILED":       7,
		"LIFECYCLE_EVENT_TYPE_MIGRATING":    8,
	}
)

func (x LifecycleEventType) Enum() *LifecycleEventType {
	p := new(LifecycleEventType)
	*p = x
	return p
}

func (x LifecycleEventType) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (LifecycleEventType) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[12].Descriptor()
}

func (LifecycleEventType) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[12]
}

func (x LifecycleEventType) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use LifecycleEventType.Descriptor instead.
func (LifecycleEventType) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{12}
}

// Drop policy for buffer overflow (JavaNOW-inspired backpressure)
type DropPolicy int32

const (
	DropPolicy_DROP_POLICY_UNSPECIFIED DropPolicy = 0
	DropPolicy_DROP_POLICY_DROP_OLDEST DropPolicy = 1 // Drop oldest events, keep newest (good for real-time)
	DropPolicy_DROP_POLICY_DROP_NEWEST DropPolicy = 2 // Drop newest events, keep oldest (good for audit)
	DropPolicy_DROP_POLICY_BLOCK       DropPolicy = 3 // Block publisher until buffer drains (use carefully!)
)

// Enum value maps for DropPolicy.
var (
	DropPolicy_name = map[int32]string{
		0: "DROP_POLICY_UNSPECIFIED",
		1: "DROP_POLICY_DROP_OLDEST",
		2: "DROP_POLICY_DROP_NEWEST",
		3: "DROP_POLICY_BLOCK",
	}
	DropPolicy_value = map[string]int32{
		"DROP_POLICY_UNSPECIFIED": 0,
		"DROP_POLICY_DROP_OLDEST": 1,
		"DROP_POLICY_DROP_NEWEST": 2,
		"DROP_POLICY_BLOCK":       3,
	}
)

func (x DropPolicy) Enum() *DropPolicy {
	p := new(DropPolicy)
	*p = x
	return p
}

func (x DropPolicy) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (DropPolicy) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[13].Descriptor()
}

func (DropPolicy) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[13]
}

func (x DropPolicy) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use DropPolicy.Descriptor instead.
func (DropPolicy) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{13}
}

// ActorRef error codes
type ActorRefErrorCode int32

const (
	ActorRefErrorCode_ACTOR_REF_ERROR_CODE_UNSPECIFIED            ActorRefErrorCode = 0
	ActorRefErrorCode_ACTOR_REF_ERROR_CODE_ACTOR_NOT_FOUND        ActorRefErrorCode = 1
	ActorRefErrorCode_ACTOR_REF_ERROR_CODE_SEND_FAILED            ActorRefErrorCode = 2
	ActorRefErrorCode_ACTOR_REF_ERROR_CODE_MAILBOX_FULL           ActorRefErrorCode = 3
	ActorRefErrorCode_ACTOR_REF_ERROR_CODE_ACTOR_TERMINATED       ActorRefErrorCode = 4
	ActorRefErrorCode_ACTOR_REF_ERROR_CODE_TIMEOUT                ActorRefErrorCode = 5
	ActorRefErrorCode_ACTOR_REF_ERROR_CODE_REMOTE_NOT_IMPLEMENTED ActorRefErrorCode = 6
)

// Enum value maps for ActorRefErrorCode.
var (
	ActorRefErrorCode_name = map[int32]string{
		0: "ACTOR_REF_ERROR_CODE_UNSPECIFIED",
		1: "ACTOR_REF_ERROR_CODE_ACTOR_NOT_FOUND",
		2: "ACTOR_REF_ERROR_CODE_SEND_FAILED",
		3: "ACTOR_REF_ERROR_CODE_MAILBOX_FULL",
		4: "ACTOR_REF_ERROR_CODE_ACTOR_TERMINATED",
		5: "ACTOR_REF_ERROR_CODE_TIMEOUT",
		6: "ACTOR_REF_ERROR_CODE_REMOTE_NOT_IMPLEMENTED",
	}
	ActorRefErrorCode_value = map[string]int32{
		"ACTOR_REF_ERROR_CODE_UNSPECIFIED":            0,
		"ACTOR_REF_ERROR_CODE_ACTOR_NOT_FOUND":        1,
		"ACTOR_REF_ERROR_CODE_SEND_FAILED":            2,
		"ACTOR_REF_ERROR_CODE_MAILBOX_FULL":           3,
		"ACTOR_REF_ERROR_CODE_ACTOR_TERMINATED":       4,
		"ACTOR_REF_ERROR_CODE_TIMEOUT":                5,
		"ACTOR_REF_ERROR_CODE_REMOTE_NOT_IMPLEMENTED": 6,
	}
)

func (x ActorRefErrorCode) Enum() *ActorRefErrorCode {
	p := new(ActorRefErrorCode)
	*p = x
	return p
}

func (x ActorRefErrorCode) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ActorRefErrorCode) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[14].Descriptor()
}

func (ActorRefErrorCode) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[14]
}

func (x ActorRefErrorCode) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ActorRefErrorCode.Descriptor instead.
func (ActorRefErrorCode) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{14}
}

// Resource profile for an actor
// Indicates what type of resources the actor primarily consumes
type ResourceProfile int32

const (
	ResourceProfile_RESOURCE_PROFILE_UNSPECIFIED       ResourceProfile = 0
	ResourceProfile_RESOURCE_PROFILE_CPU_INTENSIVE     ResourceProfile = 1
	ResourceProfile_RESOURCE_PROFILE_MEMORY_INTENSIVE  ResourceProfile = 2
	ResourceProfile_RESOURCE_PROFILE_IO_INTENSIVE      ResourceProfile = 3
	ResourceProfile_RESOURCE_PROFILE_NETWORK_INTENSIVE ResourceProfile = 4
	ResourceProfile_RESOURCE_PROFILE_BALANCED          ResourceProfile = 5
)

// Enum value maps for ResourceProfile.
var (
	ResourceProfile_name = map[int32]string{
		0: "RESOURCE_PROFILE_UNSPECIFIED",
		1: "RESOURCE_PROFILE_CPU_INTENSIVE",
		2: "RESOURCE_PROFILE_MEMORY_INTENSIVE",
		3: "RESOURCE_PROFILE_IO_INTENSIVE",
		4: "RESOURCE_PROFILE_NETWORK_INTENSIVE",
		5: "RESOURCE_PROFILE_BALANCED",
	}
	ResourceProfile_value = map[string]int32{
		"RESOURCE_PROFILE_UNSPECIFIED":       0,
		"RESOURCE_PROFILE_CPU_INTENSIVE":     1,
		"RESOURCE_PROFILE_MEMORY_INTENSIVE":  2,
		"RESOURCE_PROFILE_IO_INTENSIVE":      3,
		"RESOURCE_PROFILE_NETWORK_INTENSIVE": 4,
		"RESOURCE_PROFILE_BALANCED":          5,
	}
)

func (x ResourceProfile) Enum() *ResourceProfile {
	p := new(ResourceProfile)
	*p = x
	return p
}

func (x ResourceProfile) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ResourceProfile) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[15].Descriptor()
}

func (ResourceProfile) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[15]
}

func (x ResourceProfile) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ResourceProfile.Descriptor instead.
func (ResourceProfile) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{15}
}

// Resource violation error codes
type ResourceViolationCode int32

const (
	ResourceViolationCode_RESOURCE_VIOLATION_CODE_UNSPECIFIED      ResourceViolationCode = 0
	ResourceViolationCode_RESOURCE_VIOLATION_CODE_CPU_EXCEEDED     ResourceViolationCode = 1
	ResourceViolationCode_RESOURCE_VIOLATION_CODE_MEMORY_EXCEEDED  ResourceViolationCode = 2
	ResourceViolationCode_RESOURCE_VIOLATION_CODE_IO_EXCEEDED      ResourceViolationCode = 3
	ResourceViolationCode_RESOURCE_VIOLATION_CODE_NETWORK_EXCEEDED ResourceViolationCode = 4
)

// Enum value maps for ResourceViolationCode.
var (
	ResourceViolationCode_name = map[int32]string{
		0: "RESOURCE_VIOLATION_CODE_UNSPECIFIED",
		1: "RESOURCE_VIOLATION_CODE_CPU_EXCEEDED",
		2: "RESOURCE_VIOLATION_CODE_MEMORY_EXCEEDED",
		3: "RESOURCE_VIOLATION_CODE_IO_EXCEEDED",
		4: "RESOURCE_VIOLATION_CODE_NETWORK_EXCEEDED",
	}
	ResourceViolationCode_value = map[string]int32{
		"RESOURCE_VIOLATION_CODE_UNSPECIFIED":      0,
		"RESOURCE_VIOLATION_CODE_CPU_EXCEEDED":     1,
		"RESOURCE_VIOLATION_CODE_MEMORY_EXCEEDED":  2,
		"RESOURCE_VIOLATION_CODE_IO_EXCEEDED":      3,
		"RESOURCE_VIOLATION_CODE_NETWORK_EXCEEDED": 4,
	}
)

func (x ResourceViolationCode) Enum() *ResourceViolationCode {
	p := new(ResourceViolationCode)
	*p = x
	return p
}

func (x ResourceViolationCode) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ResourceViolationCode) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[16].Descriptor()
}

func (ResourceViolationCode) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[16]
}

func (x ResourceViolationCode) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ResourceViolationCode.Descriptor instead.
func (ResourceViolationCode) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{16}
}

// Actor health status
type ActorHealthStatus int32

const (
	ActorHealthStatus_ACTOR_HEALTH_STATUS_UNSPECIFIED ActorHealthStatus = 0
	ActorHealthStatus_ACTOR_HEALTH_STATUS_HEALTHY     ActorHealthStatus = 1
	ActorHealthStatus_ACTOR_HEALTH_STATUS_DEGRADED    ActorHealthStatus = 2
	ActorHealthStatus_ACTOR_HEALTH_STATUS_STUCK       ActorHealthStatus = 3
	ActorHealthStatus_ACTOR_HEALTH_STATUS_FAILED      ActorHealthStatus = 4
)

// Enum value maps for ActorHealthStatus.
var (
	ActorHealthStatus_name = map[int32]string{
		0: "ACTOR_HEALTH_STATUS_UNSPECIFIED",
		1: "ACTOR_HEALTH_STATUS_HEALTHY",
		2: "ACTOR_HEALTH_STATUS_DEGRADED",
		3: "ACTOR_HEALTH_STATUS_STUCK",
		4: "ACTOR_HEALTH_STATUS_FAILED",
	}
	ActorHealthStatus_value = map[string]int32{
		"ACTOR_HEALTH_STATUS_UNSPECIFIED": 0,
		"ACTOR_HEALTH_STATUS_HEALTHY":     1,
		"ACTOR_HEALTH_STATUS_DEGRADED":    2,
		"ACTOR_HEALTH_STATUS_STUCK":       3,
		"ACTOR_HEALTH_STATUS_FAILED":      4,
	}
)

func (x ActorHealthStatus) Enum() *ActorHealthStatus {
	p := new(ActorHealthStatus)
	*p = x
	return p
}

func (x ActorHealthStatus) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ActorHealthStatus) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[17].Descriptor()
}

func (ActorHealthStatus) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_actors_actor_runtime_proto_enumTypes[17]
}

func (x ActorHealthStatus) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ActorHealthStatus.Descriptor instead.
func (ActorHealthStatus) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{17}
}

// Actor instance representation.
//
// ## Purpose
// Defines the complete state of a durable actor instance including its
// identity, lifecycle state, configuration, resource usage, and attached capabilities.
//
// ## Why This Exists
// - Actors are the fundamental unit of computation in PlexSpaces (Pillar 2: Erlang/OTP)
// - Supports durable actors that survive restarts (Pillar 3: Restate durability)
// - Enables resource-aware scheduling (Quickwit-inspired resource contracts)
// - Provides facet extensibility (Static vs Dynamic principle)
// - Tracks metrics for monitoring and health checks
//
// ## Design Notes
// - actor_id uses ActorId from common.proto for consistency across services
// - state tracks lifecycle for supervision and recovery logic
// - node_id and vm_id support distributed placement and Firecracker isolation
// - facets enable dynamic capability composition without changing core proto
// - metrics enable health monitoring and auto-scaling decisions
type Actor struct {
	state      protoimpl.MessageState `protogen:"open.v1"`
	ActorId    string                 `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	ActorType  string                 `protobuf:"bytes,2,opt,name=actor_type,json=actorType,proto3" json:"actor_type,omitempty"`
	State      ActorState             `protobuf:"varint,3,opt,name=state,proto3,enum=plexspaces.actor.v1.ActorState" json:"state,omitempty"`
	NodeId     string                 `protobuf:"bytes,4,opt,name=node_id,json=nodeId,proto3" json:"node_id,omitempty"`
	VmId       string                 `protobuf:"bytes,5,opt,name=vm_id,json=vmId,proto3" json:"vm_id,omitempty"`
	ActorState []byte                 `protobuf:"bytes,6,opt,name=actor_state,json=actorState,proto3" json:"actor_state,omitempty"`
	Metadata   *v1.Metadata           `protobuf:"bytes,7,opt,name=metadata,proto3" json:"metadata,omitempty"`
	Config     *ActorConfig           `protobuf:"bytes,8,opt,name=config,proto3" json:"config,omitempty"`
	Metrics    *ActorMetrics          `protobuf:"bytes,9,opt,name=metrics,proto3" json:"metrics,omitempty"`
	// FACET EXTENSIBILITY: Attached facets provide additional capabilities
	// Examples: virtual_actor, otp_genserver, otp_supervisor, durable_execution, workflow
	Facets []*v1.Facet `protobuf:"bytes,10,rep,name=facets,proto3" json:"facets,omitempty"`
	// Schema version of actor_state serialization for format evolution
	//
	// ## Purpose
	// Enables safe actor state evolution across deployments and restarts.
	// Actors may be inactive for extended periods (days/weeks/months) and must
	// safely load old state when reactivated with newer actor code.
	//
	// ## Why This Exists
	// - Actor state schemas evolve over time (new fields, removed fields, type changes)
	// - Actors persist state to disk via checkpoints (see journaling.proto)
	// - Actors may migrate between nodes with different code versions
	// - Loading incompatible state causes corruption or panics
	// - Prevents data loss during rolling upgrades
	//
	// ## Version Rules
	// - **Version 0** = unversioned (assume version 1, best-effort deserialization)
	// - **Version >= 1** = explicit schema version tracked with actor state
	// - **Same version** (e.g., both v2) → Load directly, no migration needed
	// - **Older version** (state v1, actor v2) → Attempt migration if migration exists, else reject
	// - **Newer version** (state v3, actor v2) → **REJECT** with error (upgrade actor code first)
	//
	// ## Migration Strategy
	// ```
	// Version 1 → 2: Add new field with default value (backward compatible, no migration)
	// Version 2 → 3: Remove deprecated field (backward compatible, no migration)
	// Version 3 → 4: Change field type (BREAKING - requires explicit migration function)
	// Version 4 → 5: Restructure nested fields (BREAKING - requires migration)
	// ```
	//
	// ## Example Usage
	// ```rust
	// // Actor implementation defines current schema version
	//
	//	impl CounterActor {
	//	    const SCHEMA_VERSION: u32 = 2;
	//
	//	    fn save_state(&self) -> Result<Actor, PersistenceError> {
	//	        Ok(Actor {
	//	            actor_id: self.id.clone(),
	//	            actor_state: serialize(&self.internal_state)?,
	//	            actor_state_schema_version: Self::SCHEMA_VERSION,
	//	            // ... other fields ...
	//	        })
	//	    }
	//
	//	    fn load_state(actor: &Actor) -> Result<Self, PersistenceError> {
	//	        // Version compatibility check
	//	        match actor.actor_state_schema_version {
	//	            // Same version - direct load
	//	            v if v == Self::SCHEMA_VERSION => {
	//	                let state: CounterState = deserialize(&actor.actor_state)?;
	//	                Ok(Self::from_state(state))
	//	            }
	//
	//	            // Older version - migrate forward
	//	            v if v < Self::SCHEMA_VERSION => {
	//	                let migrated = migrate_counter_state(
	//	                    &actor.actor_state,
	//	                    v,
	//	                    Self::SCHEMA_VERSION,
	//	                )?;
	//	                let state: CounterState = deserialize(&migrated)?;
	//	                Ok(Self::from_state(state))
	//	            }
	//
	//	            // Newer version - reject (cannot load future state)
	//	            v => Err(PersistenceError::IncompatibleSchemaVersion {
	//	                actor_version: Self::SCHEMA_VERSION,
	//	                state_version: v,
	//	                message: format!(
	//	                    "Actor state v{} is newer than actor code v{}. Upgrade actor first.",
	//	                    v, Self::SCHEMA_VERSION
	//	                ),
	//	            }),
	//	        }
	//	    }
	//	}
	//
	// ```
	//
	// ## Integration with Checkpointing
	// This field works in conjunction with `Checkpoint.state_schema_version` from journaling.proto:
	// - **Actor.actor_state_schema_version**: Version of in-memory actor state (current)
	// - **Checkpoint.state_schema_version**: Version of checkpointed actor state (persistent)
	//
	// When creating checkpoint:
	// ```rust
	//
	//	let checkpoint = Checkpoint {
	//	    actor_id: actor.actor_id.clone(),
	//	    state_data: actor.actor_state.clone(),
	//	    state_schema_version: actor.actor_state_schema_version,  // Copy version
	//	    // ...
	//	};
	//
	// ```
	//
	// ## See Also
	// - `proto/plexspaces/v1/journaling.proto` - Checkpoint.state_schema_version (lines 241-321)
	// - `docs/SCHEMA_VERSIONING_REVIEW.md` - Complete versioning strategy
	// - Migration registry pattern for centralized v1→v2→v3 transitions
	ActorStateSchemaVersion uint32 `protobuf:"varint,12,opt,name=actor_state_schema_version,json=actorStateSchemaVersion,proto3" json:"actor_state_schema_version,omitempty"`
	// Error message when actor is in FAILED state
	//
	// ## Purpose
	// Provides error details when actor.state == ACTOR_STATE_FAILED.
	// Used for debugging, logging, and supervisor restart decisions.
	//
	// ## Usage
	// - Only populated when state == ACTOR_STATE_FAILED
	// - Empty string when state != ACTOR_STATE_FAILED
	// - Contains error message from actor crash or failure
	ErrorMessage string `protobuf:"bytes,13,opt,name=error_message,json=errorMessage,proto3" json:"error_message,omitempty"`
	// Namespace for this actor's data isolation.
	// When actor is part of an application deployment, namespace must match application namespace.
	// Source of truth for namespace is the application (when deploying) or actor creation request.
	Namespace string `protobuf:"bytes,14,opt,name=namespace,proto3" json:"namespace,omitempty"`
	// User-specified actor name. Together with actor_type, namespace, and node_id
	// this forms the structured ActorId.
	Name          string `protobuf:"bytes,15,opt,name=name,proto3" json:"name,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *Actor) Reset() {
	*x = Actor{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[0]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *Actor) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*Actor) ProtoMessage() {}

func (x *Actor) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[0]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use Actor.ProtoReflect.Descriptor instead.
func (*Actor) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{0}
}

func (x *Actor) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *Actor) GetActorType() string {
	if x != nil {
		return x.ActorType
	}
	return ""
}

func (x *Actor) GetState() ActorState {
	if x != nil {
		return x.State
	}
	return ActorState_ACTOR_STATE_UNSPECIFIED
}

func (x *Actor) GetNodeId() string {
	if x != nil {
		return x.NodeId
	}
	return ""
}

func (x *Actor) GetVmId() string {
	if x != nil {
		return x.VmId
	}
	return ""
}

func (x *Actor) GetActorState() []byte {
	if x != nil {
		return x.ActorState
	}
	return nil
}

func (x *Actor) GetMetadata() *v1.Metadata {
	if x != nil {
		return x.Metadata
	}
	return nil
}

func (x *Actor) GetConfig() *ActorConfig {
	if x != nil {
		return x.Config
	}
	return nil
}

func (x *Actor) GetMetrics() *ActorMetrics {
	if x != nil {
		return x.Metrics
	}
	return nil
}

func (x *Actor) GetFacets() []*v1.Facet {
	if x != nil {
		return x.Facets
	}
	return nil
}

func (x *Actor) GetActorStateSchemaVersion() uint32 {
	if x != nil {
		return x.ActorStateSchemaVersion
	}
	return 0
}

func (x *Actor) GetErrorMessage() string {
	if x != nil {
		return x.ErrorMessage
	}
	return ""
}

func (x *Actor) GetNamespace() string {
	if x != nil {
		return x.Namespace
	}
	return ""
}

func (x *Actor) GetName() string {
	if x != nil {
		return x.Name
	}
	return ""
}

// Actor configuration
type ActorConfig struct {
	state               protoimpl.MessageState          `protogen:"open.v1"`
	MailboxTimeout      *durationpb.Duration            `protobuf:"bytes,1,opt,name=mailbox_timeout,json=mailboxTimeout,proto3" json:"mailbox_timeout,omitempty"`    // Max 1 hour timeout
	MaxMailboxSize      uint32                          `protobuf:"varint,2,opt,name=max_mailbox_size,json=maxMailboxSize,proto3" json:"max_mailbox_size,omitempty"` // 1 to 1M messages
	EnablePersistence   bool                            `protobuf:"varint,3,opt,name=enable_persistence,json=enablePersistence,proto3" json:"enable_persistence,omitempty"`
	CheckpointInterval  *durationpb.Duration            `protobuf:"bytes,4,opt,name=checkpoint_interval,json=checkpointInterval,proto3" json:"checkpoint_interval,omitempty"` // 1 second to 24 hours
	RestartPolicy       *v1.RetryPolicy                 `protobuf:"bytes,5,opt,name=restart_policy,json=restartPolicy,proto3" json:"restart_policy,omitempty"`
	SupervisionStrategy supervision.SupervisionStrategy `protobuf:"varint,6,opt,name=supervision_strategy,json=supervisionStrategy,proto3,enum=plexspaces.supervision.v1.SupervisionStrategy" json:"supervision_strategy,omitempty"`
	Properties          map[string]*anypb.Any           `protobuf:"bytes,7,rep,name=properties,proto3" json:"properties,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Orleans-inspired stateless worker configuration
	StatelessWorkerConfig *StatelessWorkerConfig `protobuf:"bytes,11,opt,name=stateless_worker_config,json=statelessWorkerConfig,proto3" json:"stateless_worker_config,omitempty"`
	// Data-parallel configuration (if part of a shard group)
	DataParallelConfig *DataParallelConfig `protobuf:"bytes,12,opt,name=data_parallel_config,json=dataParallelConfig,proto3" json:"data_parallel_config,omitempty"`
	// State management mode (traditional vs lattice-based)
	StateManagementMode StateMgmtMode `protobuf:"varint,13,opt,name=state_management_mode,json=stateManagementMode,proto3,enum=plexspaces.actor.v1.StateMgmtMode" json:"state_management_mode,omitempty"`
	// Consistency level for lattice-based actors
	ConsistencyLevel ConsistencyLevel `protobuf:"varint,14,opt,name=consistency_level,json=consistencyLevel,proto3,enum=plexspaces.actor.v1.ConsistencyLevel" json:"consistency_level,omitempty"`
	// Resource-aware scheduling: Actor resource requirements
	ResourceRequirements *ActorResourceRequirements `protobuf:"bytes,16,opt,name=resource_requirements,json=resourceRequirements,proto3" json:"resource_requirements,omitempty"`
	// Shard group IDs (for task routing and co-scheduling)
	ActorGroups []string `protobuf:"bytes,17,rep,name=actor_groups,json=actorGroups,proto3" json:"actor_groups,omitempty"`
	// Schema version of properties map for configuration evolution
	//
	// ## Purpose
	// Enables safe evolution of actor configuration across deployments.
	// The `properties` map (field 7) contains opaque configuration that may
	// change as actor types evolve, requiring versioning for compatibility.
	//
	// ## Why This Exists
	// - Actor configuration schemas evolve (new config options, deprecated settings)
	// - Actors may be created with old config and reactivated with new code
	// - Configuration migration needed during rolling upgrades
	// - Prevents invalid configuration causing actor startup failures
	//
	// ## Version Rules
	// - **Version 0** = unversioned (assume version 1)
	// - **Version >= 1** = explicit config schema version
	// - **Same version** → Use config directly
	// - **Older version** → Migrate config forward (e.g., add new defaults, remove deprecated)
	// - **Newer version** → REJECT (upgrade actor code first)
	//
	// ## Example Migration
	// ```rust
	// // Version 1: Simple timeout config
	// properties: {"timeout_ms": 5000}
	//
	// // Version 2: Added retry config (backward compatible)
	//
	//	properties: {
	//	    "timeout_ms": 5000,
	//	    "max_retries": 3,      // NEW: defaults to 3
	//	    "retry_backoff_ms": 100 // NEW: defaults to 100ms
	//	}
	//
	// // Migration v1→v2: Add defaults for new fields
	//
	//	fn migrate_config_v1_to_v2(config: &mut ActorConfig) {
	//	    config.properties.entry("max_retries").or_insert(Any::from(3u32));
	//	    config.properties.entry("retry_backoff_ms").or_insert(Any::from(100u32));
	//	    config.config_schema_version = 2;
	//	}
	//
	// ```
	//
	// ## See Also
	// - `Actor.actor_state_schema_version` - For actor state versioning
	// - `docs/SCHEMA_VERSIONING_REVIEW.md` - Complete versioning strategy
	ConfigSchemaVersion uint32 `protobuf:"varint,15,opt,name=config_schema_version,json=configSchemaVersion,proto3" json:"config_schema_version,omitempty"`
	unknownFields       protoimpl.UnknownFields
	sizeCache           protoimpl.SizeCache
}

func (x *ActorConfig) Reset() {
	*x = ActorConfig{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[1]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorConfig) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorConfig) ProtoMessage() {}

func (x *ActorConfig) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[1]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorConfig.ProtoReflect.Descriptor instead.
func (*ActorConfig) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{1}
}

func (x *ActorConfig) GetMailboxTimeout() *durationpb.Duration {
	if x != nil {
		return x.MailboxTimeout
	}
	return nil
}

func (x *ActorConfig) GetMaxMailboxSize() uint32 {
	if x != nil {
		return x.MaxMailboxSize
	}
	return 0
}

func (x *ActorConfig) GetEnablePersistence() bool {
	if x != nil {
		return x.EnablePersistence
	}
	return false
}

func (x *ActorConfig) GetCheckpointInterval() *durationpb.Duration {
	if x != nil {
		return x.CheckpointInterval
	}
	return nil
}

func (x *ActorConfig) GetRestartPolicy() *v1.RetryPolicy {
	if x != nil {
		return x.RestartPolicy
	}
	return nil
}

func (x *ActorConfig) GetSupervisionStrategy() supervision.SupervisionStrategy {
	if x != nil {
		return x.SupervisionStrategy
	}
	return supervision.SupervisionStrategy(0)
}

func (x *ActorConfig) GetProperties() map[string]*anypb.Any {
	if x != nil {
		return x.Properties
	}
	return nil
}

func (x *ActorConfig) GetStatelessWorkerConfig() *StatelessWorkerConfig {
	if x != nil {
		return x.StatelessWorkerConfig
	}
	return nil
}

func (x *ActorConfig) GetDataParallelConfig() *DataParallelConfig {
	if x != nil {
		return x.DataParallelConfig
	}
	return nil
}

func (x *ActorConfig) GetStateManagementMode() StateMgmtMode {
	if x != nil {
		return x.StateManagementMode
	}
	return StateMgmtMode_STATE_MGMT_MODE_UNSPECIFIED
}

func (x *ActorConfig) GetConsistencyLevel() ConsistencyLevel {
	if x != nil {
		return x.ConsistencyLevel
	}
	return ConsistencyLevel_CONSISTENCY_LEVEL_UNSPECIFIED
}

func (x *ActorConfig) GetResourceRequirements() *ActorResourceRequirements {
	if x != nil {
		return x.ResourceRequirements
	}
	return nil
}

func (x *ActorConfig) GetActorGroups() []string {
	if x != nil {
		return x.ActorGroups
	}
	return nil
}

func (x *ActorConfig) GetConfigSchemaVersion() uint32 {
	if x != nil {
		return x.ConfigSchemaVersion
	}
	return 0
}

// Resource requirements for placement
type ResourceRequirements struct {
	state                protoimpl.MessageState `protogen:"open.v1"`
	MinMemoryMb          uint64                 `protobuf:"varint,1,opt,name=min_memory_mb,json=minMemoryMb,proto3" json:"min_memory_mb,omitempty"`  // Max 1TB
	MinCpuCores          float64                `protobuf:"fixed64,2,opt,name=min_cpu_cores,json=minCpuCores,proto3" json:"min_cpu_cores,omitempty"` // 0.1 to 128 cores
	RequiredCapabilities []string               `protobuf:"bytes,3,rep,name=required_capabilities,json=requiredCapabilities,proto3" json:"required_capabilities,omitempty"`
	CustomRequirements   map[string]string      `protobuf:"bytes,4,rep,name=custom_requirements,json=customRequirements,proto3" json:"custom_requirements,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	unknownFields        protoimpl.UnknownFields
	sizeCache            protoimpl.SizeCache
}

func (x *ResourceRequirements) Reset() {
	*x = ResourceRequirements{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[2]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ResourceRequirements) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ResourceRequirements) ProtoMessage() {}

func (x *ResourceRequirements) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[2]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ResourceRequirements.ProtoReflect.Descriptor instead.
func (*ResourceRequirements) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{2}
}

func (x *ResourceRequirements) GetMinMemoryMb() uint64 {
	if x != nil {
		return x.MinMemoryMb
	}
	return 0
}

func (x *ResourceRequirements) GetMinCpuCores() float64 {
	if x != nil {
		return x.MinCpuCores
	}
	return 0
}

func (x *ResourceRequirements) GetRequiredCapabilities() []string {
	if x != nil {
		return x.RequiredCapabilities
	}
	return nil
}

func (x *ResourceRequirements) GetCustomRequirements() map[string]string {
	if x != nil {
		return x.CustomRequirements
	}
	return nil
}

// Actor placement for resource-aware scheduling. Single source for labels, affinity, and resources.
// Scheduler (crates/scheduler) uses placement.required_labels and placement.resource_requirements to select nodes.
type ActorResourceRequirements struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Unified placement (strategy, required_labels, resource_requirements, preferred/avoid node IDs)
	Placement     *NodePlacement `protobuf:"bytes,1,opt,name=placement,proto3" json:"placement,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorResourceRequirements) Reset() {
	*x = ActorResourceRequirements{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[3]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorResourceRequirements) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorResourceRequirements) ProtoMessage() {}

func (x *ActorResourceRequirements) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[3]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorResourceRequirements.ProtoReflect.Descriptor instead.
func (*ActorResourceRequirements) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{3}
}

func (x *ActorResourceRequirements) GetPlacement() *NodePlacement {
	if x != nil {
		return x.Placement
	}
	return nil
}

// Stateless worker configuration (Orleans-inspired)
type StatelessWorkerConfig struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Maximum number of concurrent instances
	MaxInstances uint32 `protobuf:"varint,1,opt,name=max_instances,json=maxInstances,proto3" json:"max_instances,omitempty"`
	// Minimum number of instances to keep warm
	MinInstances uint32 `protobuf:"varint,2,opt,name=min_instances,json=minInstances,proto3" json:"min_instances,omitempty"`
	// Load balancing strategy
	Strategy      LoadBalancingStrategy `protobuf:"varint,3,opt,name=strategy,proto3,enum=plexspaces.actor.v1.LoadBalancingStrategy" json:"strategy,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *StatelessWorkerConfig) Reset() {
	*x = StatelessWorkerConfig{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[4]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *StatelessWorkerConfig) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*StatelessWorkerConfig) ProtoMessage() {}

func (x *StatelessWorkerConfig) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[4]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use StatelessWorkerConfig.ProtoReflect.Descriptor instead.
func (*StatelessWorkerConfig) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{4}
}

func (x *StatelessWorkerConfig) GetMaxInstances() uint32 {
	if x != nil {
		return x.MaxInstances
	}
	return 0
}

func (x *StatelessWorkerConfig) GetMinInstances() uint32 {
	if x != nil {
		return x.MinInstances
	}
	return 0
}

func (x *StatelessWorkerConfig) GetStrategy() LoadBalancingStrategy {
	if x != nil {
		return x.Strategy
	}
	return LoadBalancingStrategy_LOAD_BALANCING_UNSPECIFIED
}

// Data-parallel (shard group) strategy; same for all shards in the group.
// Shard identity is implicit (index in ShardGroup.shard_actor_ids), not stored per-actor.
type DataParallelConfig struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Shard group ID
	GroupId string `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	// Number of shards in the group (1 to 1 billion)
	ShardCount uint32 `protobuf:"varint,2,opt,name=shard_count,json=shardCount,proto3" json:"shard_count,omitempty"`
	// Partitioning strategy
	PartitionStrategy PartitionStrategy `protobuf:"varint,4,opt,name=partition_strategy,json=partitionStrategy,proto3,enum=plexspaces.actor.v1.PartitionStrategy" json:"partition_strategy,omitempty"`
	// Rebalancing policy
	RebalancePolicy RebalancePolicy `protobuf:"varint,5,opt,name=rebalance_policy,json=rebalancePolicy,proto3,enum=plexspaces.actor.v1.RebalancePolicy" json:"rebalance_policy,omitempty"`
	// Node placement for multi-node leader-worker (same_node, from_registry, node_ids)
	Placement     *NodePlacement `protobuf:"bytes,6,opt,name=placement,proto3" json:"placement,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *DataParallelConfig) Reset() {
	*x = DataParallelConfig{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[5]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *DataParallelConfig) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*DataParallelConfig) ProtoMessage() {}

func (x *DataParallelConfig) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[5]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use DataParallelConfig.ProtoReflect.Descriptor instead.
func (*DataParallelConfig) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{5}
}

func (x *DataParallelConfig) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *DataParallelConfig) GetShardCount() uint32 {
	if x != nil {
		return x.ShardCount
	}
	return 0
}

func (x *DataParallelConfig) GetPartitionStrategy() PartitionStrategy {
	if x != nil {
		return x.PartitionStrategy
	}
	return PartitionStrategy_PARTITION_STRATEGY_UNSPECIFIED
}

func (x *DataParallelConfig) GetRebalancePolicy() RebalancePolicy {
	if x != nil {
		return x.RebalancePolicy
	}
	return RebalancePolicy_REBALANCE_POLICY_UNSPECIFIED
}

func (x *DataParallelConfig) GetPlacement() *NodePlacement {
	if x != nil {
		return x.Placement
	}
	return nil
}

// Shard group: unified config (DataParallelConfig) plus actor refs and state.
type ShardGroup struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Strategy for the group (group_id, shard_count, partition_strategy, rebalance_policy, placement)
	Config          *DataParallelConfig    `protobuf:"bytes,1,opt,name=config,proto3" json:"config,omitempty"`
	ActorType       string                 `protobuf:"bytes,2,opt,name=actor_type,json=actorType,proto3" json:"actor_type,omitempty"`
	ShardActorIds   []string               `protobuf:"bytes,3,rep,name=shard_actor_ids,json=shardActorIds,proto3" json:"shard_actor_ids,omitempty"` // Indexed by shard index 0 to config.shard_count-1
	State           ShardGroupState        `protobuf:"varint,4,opt,name=state,proto3,enum=plexspaces.actor.v1.ShardGroupState" json:"state,omitempty"`
	CreatedAt       *timestamppb.Timestamp `protobuf:"bytes,5,opt,name=created_at,json=createdAt,proto3" json:"created_at,omitempty"`
	Metadata        map[string]string      `protobuf:"bytes,6,rep,name=metadata,proto3" json:"metadata,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	RebalanceStatus *RebalanceStatus       `protobuf:"bytes,7,opt,name=rebalance_status,json=rebalanceStatus,proto3" json:"rebalance_status,omitempty"`
	unknownFields   protoimpl.UnknownFields
	sizeCache       protoimpl.SizeCache
}

func (x *ShardGroup) Reset() {
	*x = ShardGroup{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[6]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ShardGroup) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ShardGroup) ProtoMessage() {}

func (x *ShardGroup) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[6]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ShardGroup.ProtoReflect.Descriptor instead.
func (*ShardGroup) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{6}
}

func (x *ShardGroup) GetConfig() *DataParallelConfig {
	if x != nil {
		return x.Config
	}
	return nil
}

func (x *ShardGroup) GetActorType() string {
	if x != nil {
		return x.ActorType
	}
	return ""
}

func (x *ShardGroup) GetShardActorIds() []string {
	if x != nil {
		return x.ShardActorIds
	}
	return nil
}

func (x *ShardGroup) GetState() ShardGroupState {
	if x != nil {
		return x.State
	}
	return ShardGroupState_SHARD_GROUP_STATE_UNSPECIFIED
}

func (x *ShardGroup) GetCreatedAt() *timestamppb.Timestamp {
	if x != nil {
		return x.CreatedAt
	}
	return nil
}

func (x *ShardGroup) GetMetadata() map[string]string {
	if x != nil {
		return x.Metadata
	}
	return nil
}

func (x *ShardGroup) GetRebalanceStatus() *RebalanceStatus {
	if x != nil {
		return x.RebalanceStatus
	}
	return nil
}

// Rebalancing status for shard groups
type RebalanceStatus struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Is currently rebalancing
	IsRebalancing bool `protobuf:"varint,1,opt,name=is_rebalancing,json=isRebalancing,proto3" json:"is_rebalancing,omitempty"`
	// Old shard count (before rebalancing)
	OldShardCount uint32 `protobuf:"varint,2,opt,name=old_shard_count,json=oldShardCount,proto3" json:"old_shard_count,omitempty"`
	// New shard count (after rebalancing)
	NewShardCount uint32 `protobuf:"varint,3,opt,name=new_shard_count,json=newShardCount,proto3" json:"new_shard_count,omitempty"`
	// Progress (0.0 to 100.0)
	ProgressPercent float64 `protobuf:"fixed64,4,opt,name=progress_percent,json=progressPercent,proto3" json:"progress_percent,omitempty"`
	// When rebalancing started
	StartedAt *timestamppb.Timestamp `protobuf:"bytes,5,opt,name=started_at,json=startedAt,proto3" json:"started_at,omitempty"`
	// Estimated completion time
	EstimatedCompletion *timestamppb.Timestamp `protobuf:"bytes,6,opt,name=estimated_completion,json=estimatedCompletion,proto3" json:"estimated_completion,omitempty"`
	unknownFields       protoimpl.UnknownFields
	sizeCache           protoimpl.SizeCache
}

func (x *RebalanceStatus) Reset() {
	*x = RebalanceStatus{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[7]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *RebalanceStatus) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*RebalanceStatus) ProtoMessage() {}

func (x *RebalanceStatus) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[7]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use RebalanceStatus.ProtoReflect.Descriptor instead.
func (*RebalanceStatus) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{7}
}

func (x *RebalanceStatus) GetIsRebalancing() bool {
	if x != nil {
		return x.IsRebalancing
	}
	return false
}

func (x *RebalanceStatus) GetOldShardCount() uint32 {
	if x != nil {
		return x.OldShardCount
	}
	return 0
}

func (x *RebalanceStatus) GetNewShardCount() uint32 {
	if x != nil {
		return x.NewShardCount
	}
	return 0
}

func (x *RebalanceStatus) GetProgressPercent() float64 {
	if x != nil {
		return x.ProgressPercent
	}
	return 0
}

func (x *RebalanceStatus) GetStartedAt() *timestamppb.Timestamp {
	if x != nil {
		return x.StartedAt
	}
	return nil
}

func (x *RebalanceStatus) GetEstimatedCompletion() *timestamppb.Timestamp {
	if x != nil {
		return x.EstimatedCompletion
	}
	return nil
}

// Unified node placement: strategy, affinity, and resource requirements.
// Replaces PlacementHint, PlacementPreferences, and label/placement fields on ActorResourceRequirements.
// Scheduler (crates/scheduler) matches nodes using required_labels and resource_requirements.
type NodePlacement struct {
	state    protoimpl.MessageState `protogen:"open.v1"`
	Strategy NodePlacementStrategy  `protobuf:"varint,1,opt,name=strategy,proto3,enum=plexspaces.actor.v1.NodePlacementStrategy" json:"strategy,omitempty"`
	// For FROM_REGISTRY: optional cluster name filter (empty = all connected nodes)
	Cluster string `protobuf:"bytes,2,opt,name=cluster,proto3" json:"cluster,omitempty"`
	// For NODE_IDS: explicit node IDs to place shards on (round-robin)
	NodeIds []string `protobuf:"bytes,3,rep,name=node_ids,json=nodeIds,proto3" json:"node_ids,omitempty"`
	// Node must match all labels (Kubernetes-inspired). Used by scheduler for node selection.
	RequiredLabels map[string]string `protobuf:"bytes,4,rep,name=required_labels,json=requiredLabels,proto3" json:"required_labels,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Avoid node IDs (anti-affinity; scheduler excludes these)
	AvoidNodeIds []string `protobuf:"bytes,5,rep,name=avoid_node_ids,json=avoidNodeIds,proto3" json:"avoid_node_ids,omitempty"`
	// CPU, memory, disk, GPU requirements. Scheduler filters by NodeCapacity.available.
	ResourceRequirements *v1.ResourceSpec `protobuf:"bytes,6,opt,name=resource_requirements,json=resourceRequirements,proto3" json:"resource_requirements,omitempty"`
	// Affinity labels (co-location hint; scheduler may prefer nodes with matching labels)
	AffinityLabels map[string]string `protobuf:"bytes,7,rep,name=affinity_labels,json=affinityLabels,proto3" json:"affinity_labels,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	unknownFields  protoimpl.UnknownFields
	sizeCache      protoimpl.SizeCache
}

func (x *NodePlacement) Reset() {
	*x = NodePlacement{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[8]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *NodePlacement) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*NodePlacement) ProtoMessage() {}

func (x *NodePlacement) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[8]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use NodePlacement.ProtoReflect.Descriptor instead.
func (*NodePlacement) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{8}
}

func (x *NodePlacement) GetStrategy() NodePlacementStrategy {
	if x != nil {
		return x.Strategy
	}
	return NodePlacementStrategy_NODE_PLACEMENT_STRATEGY_UNSPECIFIED
}

func (x *NodePlacement) GetCluster() string {
	if x != nil {
		return x.Cluster
	}
	return ""
}

func (x *NodePlacement) GetNodeIds() []string {
	if x != nil {
		return x.NodeIds
	}
	return nil
}

func (x *NodePlacement) GetRequiredLabels() map[string]string {
	if x != nil {
		return x.RequiredLabels
	}
	return nil
}

func (x *NodePlacement) GetAvoidNodeIds() []string {
	if x != nil {
		return x.AvoidNodeIds
	}
	return nil
}

func (x *NodePlacement) GetResourceRequirements() *v1.ResourceSpec {
	if x != nil {
		return x.ResourceRequirements
	}
	return nil
}

func (x *NodePlacement) GetAffinityLabels() map[string]string {
	if x != nil {
		return x.AffinityLabels
	}
	return nil
}

type CreateShardGroupRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Group strategy (group_id, shard_count, partition_strategy, rebalance_policy, placement)
	// Use config.placement.required_labels for node placement; scheduler matches nodes by placement.
	Config    *DataParallelConfig `protobuf:"bytes,1,opt,name=config,proto3" json:"config,omitempty"`
	ActorType string              `protobuf:"bytes,2,opt,name=actor_type,json=actorType,proto3" json:"actor_type,omitempty"`
	// Per-shard ActorConfig (optional data_parallel_config here is ignored; use config above)
	ShardConfig   *ActorConfig      `protobuf:"bytes,3,opt,name=shard_config,json=shardConfig,proto3" json:"shard_config,omitempty"`
	InitialState  []byte            `protobuf:"bytes,4,opt,name=initial_state,json=initialState,proto3" json:"initial_state,omitempty"`
	Metadata      map[string]string `protobuf:"bytes,5,rep,name=metadata,proto3" json:"metadata,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *CreateShardGroupRequest) Reset() {
	*x = CreateShardGroupRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[9]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *CreateShardGroupRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*CreateShardGroupRequest) ProtoMessage() {}

func (x *CreateShardGroupRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[9]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use CreateShardGroupRequest.ProtoReflect.Descriptor instead.
func (*CreateShardGroupRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{9}
}

func (x *CreateShardGroupRequest) GetConfig() *DataParallelConfig {
	if x != nil {
		return x.Config
	}
	return nil
}

func (x *CreateShardGroupRequest) GetActorType() string {
	if x != nil {
		return x.ActorType
	}
	return ""
}

func (x *CreateShardGroupRequest) GetShardConfig() *ActorConfig {
	if x != nil {
		return x.ShardConfig
	}
	return nil
}

func (x *CreateShardGroupRequest) GetInitialState() []byte {
	if x != nil {
		return x.InitialState
	}
	return nil
}

func (x *CreateShardGroupRequest) GetMetadata() map[string]string {
	if x != nil {
		return x.Metadata
	}
	return nil
}

type CreateShardGroupResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Group         *ShardGroup            `protobuf:"bytes,1,opt,name=group,proto3" json:"group,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *CreateShardGroupResponse) Reset() {
	*x = CreateShardGroupResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[10]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *CreateShardGroupResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*CreateShardGroupResponse) ProtoMessage() {}

func (x *CreateShardGroupResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[10]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use CreateShardGroupResponse.ProtoReflect.Descriptor instead.
func (*CreateShardGroupResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{10}
}

func (x *CreateShardGroupResponse) GetGroup() *ShardGroup {
	if x != nil {
		return x.Group
	}
	return nil
}

type DeleteShardGroupRequest struct {
	state           protoimpl.MessageState `protogen:"open.v1"`
	GroupId         string                 `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	Force           bool                   `protobuf:"varint,2,opt,name=force,proto3" json:"force,omitempty"`
	ShutdownTimeout *durationpb.Duration   `protobuf:"bytes,3,opt,name=shutdown_timeout,json=shutdownTimeout,proto3" json:"shutdown_timeout,omitempty"`
	unknownFields   protoimpl.UnknownFields
	sizeCache       protoimpl.SizeCache
}

func (x *DeleteShardGroupRequest) Reset() {
	*x = DeleteShardGroupRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[11]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *DeleteShardGroupRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*DeleteShardGroupRequest) ProtoMessage() {}

func (x *DeleteShardGroupRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[11]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use DeleteShardGroupRequest.ProtoReflect.Descriptor instead.
func (*DeleteShardGroupRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{11}
}

func (x *DeleteShardGroupRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *DeleteShardGroupRequest) GetForce() bool {
	if x != nil {
		return x.Force
	}
	return false
}

func (x *DeleteShardGroupRequest) GetShutdownTimeout() *durationpb.Duration {
	if x != nil {
		return x.ShutdownTimeout
	}
	return nil
}

type GetShardGroupRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	GroupId       string                 `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *GetShardGroupRequest) Reset() {
	*x = GetShardGroupRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[12]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *GetShardGroupRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*GetShardGroupRequest) ProtoMessage() {}

func (x *GetShardGroupRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[12]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use GetShardGroupRequest.ProtoReflect.Descriptor instead.
func (*GetShardGroupRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{12}
}

func (x *GetShardGroupRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

type GetShardGroupResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Group         *ShardGroup            `protobuf:"bytes,1,opt,name=group,proto3" json:"group,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *GetShardGroupResponse) Reset() {
	*x = GetShardGroupResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[13]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *GetShardGroupResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*GetShardGroupResponse) ProtoMessage() {}

func (x *GetShardGroupResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[13]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use GetShardGroupResponse.ProtoReflect.Descriptor instead.
func (*GetShardGroupResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{13}
}

func (x *GetShardGroupResponse) GetGroup() *ShardGroup {
	if x != nil {
		return x.Group
	}
	return nil
}

type ListShardGroupsRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	ActorType     string                 `protobuf:"bytes,1,opt,name=actor_type,json=actorType,proto3" json:"actor_type,omitempty"`
	State         ShardGroupState        `protobuf:"varint,2,opt,name=state,proto3,enum=plexspaces.actor.v1.ShardGroupState" json:"state,omitempty"`
	Page          *v1.PageRequest        `protobuf:"bytes,3,opt,name=page,proto3" json:"page,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ListShardGroupsRequest) Reset() {
	*x = ListShardGroupsRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[14]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ListShardGroupsRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ListShardGroupsRequest) ProtoMessage() {}

func (x *ListShardGroupsRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[14]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ListShardGroupsRequest.ProtoReflect.Descriptor instead.
func (*ListShardGroupsRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{14}
}

func (x *ListShardGroupsRequest) GetActorType() string {
	if x != nil {
		return x.ActorType
	}
	return ""
}

func (x *ListShardGroupsRequest) GetState() ShardGroupState {
	if x != nil {
		return x.State
	}
	return ShardGroupState_SHARD_GROUP_STATE_UNSPECIFIED
}

func (x *ListShardGroupsRequest) GetPage() *v1.PageRequest {
	if x != nil {
		return x.Page
	}
	return nil
}

type ListShardGroupsResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Groups        []*ShardGroup          `protobuf:"bytes,1,rep,name=groups,proto3" json:"groups,omitempty"`
	Page          *v1.PageResponse       `protobuf:"bytes,2,opt,name=page,proto3" json:"page,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ListShardGroupsResponse) Reset() {
	*x = ListShardGroupsResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[15]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ListShardGroupsResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ListShardGroupsResponse) ProtoMessage() {}

func (x *ListShardGroupsResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[15]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ListShardGroupsResponse.ProtoReflect.Descriptor instead.
func (*ListShardGroupsResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{15}
}

func (x *ListShardGroupsResponse) GetGroups() []*ShardGroup {
	if x != nil {
		return x.Groups
	}
	return nil
}

func (x *ListShardGroupsResponse) GetPage() *v1.PageResponse {
	if x != nil {
		return x.Page
	}
	return nil
}

type SendToShardRequest struct {
	state           protoimpl.MessageState `protogen:"open.v1"`
	GroupId         string                 `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	PartitionKey    []byte                 `protobuf:"bytes,2,opt,name=partition_key,json=partitionKey,proto3" json:"partition_key,omitempty"`
	Message         *v1.Message            `protobuf:"bytes,3,opt,name=message,proto3" json:"message,omitempty"`
	WaitForResponse bool                   `protobuf:"varint,4,opt,name=wait_for_response,json=waitForResponse,proto3" json:"wait_for_response,omitempty"`
	Timeout         *durationpb.Duration   `protobuf:"bytes,5,opt,name=timeout,proto3" json:"timeout,omitempty"`
	unknownFields   protoimpl.UnknownFields
	sizeCache       protoimpl.SizeCache
}

func (x *SendToShardRequest) Reset() {
	*x = SendToShardRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[16]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SendToShardRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SendToShardRequest) ProtoMessage() {}

func (x *SendToShardRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[16]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SendToShardRequest.ProtoReflect.Descriptor instead.
func (*SendToShardRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{16}
}

func (x *SendToShardRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *SendToShardRequest) GetPartitionKey() []byte {
	if x != nil {
		return x.PartitionKey
	}
	return nil
}

func (x *SendToShardRequest) GetMessage() *v1.Message {
	if x != nil {
		return x.Message
	}
	return nil
}

func (x *SendToShardRequest) GetWaitForResponse() bool {
	if x != nil {
		return x.WaitForResponse
	}
	return false
}

func (x *SendToShardRequest) GetTimeout() *durationpb.Duration {
	if x != nil {
		return x.Timeout
	}
	return nil
}

type SendToShardResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	ShardId       uint32                 `protobuf:"varint,1,opt,name=shard_id,json=shardId,proto3" json:"shard_id,omitempty"`
	ShardActorId  string                 `protobuf:"bytes,2,opt,name=shard_actor_id,json=shardActorId,proto3" json:"shard_actor_id,omitempty"`
	Response      *v1.Message            `protobuf:"bytes,3,opt,name=response,proto3" json:"response,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SendToShardResponse) Reset() {
	*x = SendToShardResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[17]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SendToShardResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SendToShardResponse) ProtoMessage() {}

func (x *SendToShardResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[17]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SendToShardResponse.ProtoReflect.Descriptor instead.
func (*SendToShardResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{17}
}

func (x *SendToShardResponse) GetShardId() uint32 {
	if x != nil {
		return x.ShardId
	}
	return 0
}

func (x *SendToShardResponse) GetShardActorId() string {
	if x != nil {
		return x.ShardActorId
	}
	return ""
}

func (x *SendToShardResponse) GetResponse() *v1.Message {
	if x != nil {
		return x.Response
	}
	return nil
}

type ScatterGatherRequest struct {
	state         protoimpl.MessageState        `protogen:"open.v1"`
	GroupId       string                        `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	Query         *v1.Message                   `protobuf:"bytes,2,opt,name=query,proto3" json:"query,omitempty"`
	Timeout       *durationpb.Duration          `protobuf:"bytes,3,opt,name=timeout,proto3" json:"timeout,omitempty"`
	Aggregation   ShardGroupAggregationStrategy `protobuf:"varint,4,opt,name=aggregation,proto3,enum=plexspaces.actor.v1.ShardGroupAggregationStrategy" json:"aggregation,omitempty"`
	MinResponses  uint32                        `protobuf:"varint,5,opt,name=min_responses,json=minResponses,proto3" json:"min_responses,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ScatterGatherRequest) Reset() {
	*x = ScatterGatherRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[18]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ScatterGatherRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ScatterGatherRequest) ProtoMessage() {}

func (x *ScatterGatherRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[18]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ScatterGatherRequest.ProtoReflect.Descriptor instead.
func (*ScatterGatherRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{18}
}

func (x *ScatterGatherRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *ScatterGatherRequest) GetQuery() *v1.Message {
	if x != nil {
		return x.Query
	}
	return nil
}

func (x *ScatterGatherRequest) GetTimeout() *durationpb.Duration {
	if x != nil {
		return x.Timeout
	}
	return nil
}

func (x *ScatterGatherRequest) GetAggregation() ShardGroupAggregationStrategy {
	if x != nil {
		return x.Aggregation
	}
	return ShardGroupAggregationStrategy_SHARD_GROUP_AGGREGATION_UNSPECIFIED
}

func (x *ScatterGatherRequest) GetMinResponses() uint32 {
	if x != nil {
		return x.MinResponses
	}
	return 0
}

type ShardQueryResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	ShardId       uint32                 `protobuf:"varint,1,opt,name=shard_id,json=shardId,proto3" json:"shard_id,omitempty"`
	ShardActorId  string                 `protobuf:"bytes,2,opt,name=shard_actor_id,json=shardActorId,proto3" json:"shard_actor_id,omitempty"`
	Response      *v1.Message            `protobuf:"bytes,3,opt,name=response,proto3" json:"response,omitempty"`
	Latency       *durationpb.Duration   `protobuf:"bytes,4,opt,name=latency,proto3" json:"latency,omitempty"`
	Success       bool                   `protobuf:"varint,5,opt,name=success,proto3" json:"success,omitempty"`
	Error         string                 `protobuf:"bytes,6,opt,name=error,proto3" json:"error,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ShardQueryResponse) Reset() {
	*x = ShardQueryResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[19]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ShardQueryResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ShardQueryResponse) ProtoMessage() {}

func (x *ShardQueryResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[19]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ShardQueryResponse.ProtoReflect.Descriptor instead.
func (*ShardQueryResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{19}
}

func (x *ShardQueryResponse) GetShardId() uint32 {
	if x != nil {
		return x.ShardId
	}
	return 0
}

func (x *ShardQueryResponse) GetShardActorId() string {
	if x != nil {
		return x.ShardActorId
	}
	return ""
}

func (x *ShardQueryResponse) GetResponse() *v1.Message {
	if x != nil {
		return x.Response
	}
	return nil
}

func (x *ShardQueryResponse) GetLatency() *durationpb.Duration {
	if x != nil {
		return x.Latency
	}
	return nil
}

func (x *ShardQueryResponse) GetSuccess() bool {
	if x != nil {
		return x.Success
	}
	return false
}

func (x *ShardQueryResponse) GetError() string {
	if x != nil {
		return x.Error
	}
	return ""
}

type ScatterGatherStats struct {
	state           protoimpl.MessageState `protogen:"open.v1"`
	ShardsQueried   uint32                 `protobuf:"varint,1,opt,name=shards_queried,json=shardsQueried,proto3" json:"shards_queried,omitempty"`
	ShardsResponded uint32                 `protobuf:"varint,2,opt,name=shards_responded,json=shardsResponded,proto3" json:"shards_responded,omitempty"`
	ShardsFailed    uint32                 `protobuf:"varint,3,opt,name=shards_failed,json=shardsFailed,proto3" json:"shards_failed,omitempty"`
	MaxLatency      *durationpb.Duration   `protobuf:"bytes,4,opt,name=max_latency,json=maxLatency,proto3" json:"max_latency,omitempty"`
	unknownFields   protoimpl.UnknownFields
	sizeCache       protoimpl.SizeCache
}

func (x *ScatterGatherStats) Reset() {
	*x = ScatterGatherStats{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[20]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ScatterGatherStats) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ScatterGatherStats) ProtoMessage() {}

func (x *ScatterGatherStats) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[20]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ScatterGatherStats.ProtoReflect.Descriptor instead.
func (*ScatterGatherStats) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{20}
}

func (x *ScatterGatherStats) GetShardsQueried() uint32 {
	if x != nil {
		return x.ShardsQueried
	}
	return 0
}

func (x *ScatterGatherStats) GetShardsResponded() uint32 {
	if x != nil {
		return x.ShardsResponded
	}
	return 0
}

func (x *ScatterGatherStats) GetShardsFailed() uint32 {
	if x != nil {
		return x.ShardsFailed
	}
	return 0
}

func (x *ScatterGatherStats) GetMaxLatency() *durationpb.Duration {
	if x != nil {
		return x.MaxLatency
	}
	return nil
}

type ScatterGatherResponse struct {
	state          protoimpl.MessageState `protogen:"open.v1"`
	Result         *v1.Message            `protobuf:"bytes,1,opt,name=result,proto3" json:"result,omitempty"`
	ShardResponses []*ShardQueryResponse  `protobuf:"bytes,2,rep,name=shard_responses,json=shardResponses,proto3" json:"shard_responses,omitempty"`
	Stats          *ScatterGatherStats    `protobuf:"bytes,3,opt,name=stats,proto3" json:"stats,omitempty"`
	unknownFields  protoimpl.UnknownFields
	sizeCache      protoimpl.SizeCache
}

func (x *ScatterGatherResponse) Reset() {
	*x = ScatterGatherResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[21]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ScatterGatherResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ScatterGatherResponse) ProtoMessage() {}

func (x *ScatterGatherResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[21]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ScatterGatherResponse.ProtoReflect.Descriptor instead.
func (*ScatterGatherResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{21}
}

func (x *ScatterGatherResponse) GetResult() *v1.Message {
	if x != nil {
		return x.Result
	}
	return nil
}

func (x *ScatterGatherResponse) GetShardResponses() []*ShardQueryResponse {
	if x != nil {
		return x.ShardResponses
	}
	return nil
}

func (x *ScatterGatherResponse) GetStats() *ScatterGatherStats {
	if x != nil {
		return x.Stats
	}
	return nil
}

// Bulk update: send update messages to multiple shards (DPA UpdateFunction)
// Inspired by NSDI'22 Data-Parallel Actors paper
type BulkUpdateShardGroupRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Group to update
	GroupId string `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	// Update messages: partition_key -> message
	// Messages will be routed to appropriate shards based on partition_key
	Updates map[string]*v1.Message `protobuf:"bytes,2,rep,name=updates,proto3" json:"updates,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Consistency level for updates
	ConsistencyLevel ConsistencyLevel `protobuf:"varint,3,opt,name=consistency_level,json=consistencyLevel,proto3,enum=plexspaces.actor.v1.ConsistencyLevel" json:"consistency_level,omitempty"`
	// Timeout for updates
	Timeout *durationpb.Duration `protobuf:"bytes,4,opt,name=timeout,proto3" json:"timeout,omitempty"`
	// Wait for responses (true = wait for all, false = fire-and-forget)
	WaitForResponses bool `protobuf:"varint,5,opt,name=wait_for_responses,json=waitForResponses,proto3" json:"wait_for_responses,omitempty"`
	unknownFields    protoimpl.UnknownFields
	sizeCache        protoimpl.SizeCache
}

func (x *BulkUpdateShardGroupRequest) Reset() {
	*x = BulkUpdateShardGroupRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[22]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *BulkUpdateShardGroupRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*BulkUpdateShardGroupRequest) ProtoMessage() {}

func (x *BulkUpdateShardGroupRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[22]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use BulkUpdateShardGroupRequest.ProtoReflect.Descriptor instead.
func (*BulkUpdateShardGroupRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{22}
}

func (x *BulkUpdateShardGroupRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *BulkUpdateShardGroupRequest) GetUpdates() map[string]*v1.Message {
	if x != nil {
		return x.Updates
	}
	return nil
}

func (x *BulkUpdateShardGroupRequest) GetConsistencyLevel() ConsistencyLevel {
	if x != nil {
		return x.ConsistencyLevel
	}
	return ConsistencyLevel_CONSISTENCY_LEVEL_UNSPECIFIED
}

func (x *BulkUpdateShardGroupRequest) GetTimeout() *durationpb.Duration {
	if x != nil {
		return x.Timeout
	}
	return nil
}

func (x *BulkUpdateShardGroupRequest) GetWaitForResponses() bool {
	if x != nil {
		return x.WaitForResponses
	}
	return false
}

type BulkUpdateShardGroupResponse struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Number of updates sent
	UpdatesSent uint32 `protobuf:"varint,1,opt,name=updates_sent,json=updatesSent,proto3" json:"updates_sent,omitempty"`
	// Number of successful updates
	UpdatesSucceeded uint32 `protobuf:"varint,2,opt,name=updates_succeeded,json=updatesSucceeded,proto3" json:"updates_succeeded,omitempty"`
	// Number of failed updates
	UpdatesFailed uint32 `protobuf:"varint,3,opt,name=updates_failed,json=updatesFailed,proto3" json:"updates_failed,omitempty"`
	// Per-shard statistics
	ShardStats []*ShardUpdateStats `protobuf:"bytes,4,rep,name=shard_stats,json=shardStats,proto3" json:"shard_stats,omitempty"`
	// Errors (if any)
	Errors        []string `protobuf:"bytes,5,rep,name=errors,proto3" json:"errors,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *BulkUpdateShardGroupResponse) Reset() {
	*x = BulkUpdateShardGroupResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[23]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *BulkUpdateShardGroupResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*BulkUpdateShardGroupResponse) ProtoMessage() {}

func (x *BulkUpdateShardGroupResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[23]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use BulkUpdateShardGroupResponse.ProtoReflect.Descriptor instead.
func (*BulkUpdateShardGroupResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{23}
}

func (x *BulkUpdateShardGroupResponse) GetUpdatesSent() uint32 {
	if x != nil {
		return x.UpdatesSent
	}
	return 0
}

func (x *BulkUpdateShardGroupResponse) GetUpdatesSucceeded() uint32 {
	if x != nil {
		return x.UpdatesSucceeded
	}
	return 0
}

func (x *BulkUpdateShardGroupResponse) GetUpdatesFailed() uint32 {
	if x != nil {
		return x.UpdatesFailed
	}
	return 0
}

func (x *BulkUpdateShardGroupResponse) GetShardStats() []*ShardUpdateStats {
	if x != nil {
		return x.ShardStats
	}
	return nil
}

func (x *BulkUpdateShardGroupResponse) GetErrors() []string {
	if x != nil {
		return x.Errors
	}
	return nil
}

type ShardUpdateStats struct {
	state            protoimpl.MessageState `protogen:"open.v1"`
	ShardId          uint32                 `protobuf:"varint,1,opt,name=shard_id,json=shardId,proto3" json:"shard_id,omitempty"`
	ShardActorId     string                 `protobuf:"bytes,2,opt,name=shard_actor_id,json=shardActorId,proto3" json:"shard_actor_id,omitempty"`
	UpdatesSent      uint32                 `protobuf:"varint,3,opt,name=updates_sent,json=updatesSent,proto3" json:"updates_sent,omitempty"`
	UpdatesSucceeded uint32                 `protobuf:"varint,4,opt,name=updates_succeeded,json=updatesSucceeded,proto3" json:"updates_succeeded,omitempty"`
	UpdatesFailed    uint32                 `protobuf:"varint,5,opt,name=updates_failed,json=updatesFailed,proto3" json:"updates_failed,omitempty"`
	unknownFields    protoimpl.UnknownFields
	sizeCache        protoimpl.SizeCache
}

func (x *ShardUpdateStats) Reset() {
	*x = ShardUpdateStats{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[24]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ShardUpdateStats) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ShardUpdateStats) ProtoMessage() {}

func (x *ShardUpdateStats) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[24]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ShardUpdateStats.ProtoReflect.Descriptor instead.
func (*ShardUpdateStats) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{24}
}

func (x *ShardUpdateStats) GetShardId() uint32 {
	if x != nil {
		return x.ShardId
	}
	return 0
}

func (x *ShardUpdateStats) GetShardActorId() string {
	if x != nil {
		return x.ShardActorId
	}
	return ""
}

func (x *ShardUpdateStats) GetUpdatesSent() uint32 {
	if x != nil {
		return x.UpdatesSent
	}
	return 0
}

func (x *ShardUpdateStats) GetUpdatesSucceeded() uint32 {
	if x != nil {
		return x.UpdatesSucceeded
	}
	return 0
}

func (x *ShardUpdateStats) GetUpdatesFailed() uint32 {
	if x != nil {
		return x.UpdatesFailed
	}
	return 0
}

// Map: apply function to all shards in parallel (DPA Map operator)
// Inspired by NSDI'22 Data-Parallel Actors paper
type MapShardGroupRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Group to map over
	GroupId string `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	// Map function message (sent to each shard)
	MapFunction *v1.Message `protobuf:"bytes,2,opt,name=map_function,json=mapFunction,proto3" json:"map_function,omitempty"`
	// Timeout for map operation
	Timeout *durationpb.Duration `protobuf:"bytes,3,opt,name=timeout,proto3" json:"timeout,omitempty"`
	// Minimum number of shards that must respond (0 = all required)
	MinResponses  uint32 `protobuf:"varint,4,opt,name=min_responses,json=minResponses,proto3" json:"min_responses,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *MapShardGroupRequest) Reset() {
	*x = MapShardGroupRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[25]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *MapShardGroupRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*MapShardGroupRequest) ProtoMessage() {}

func (x *MapShardGroupRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[25]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use MapShardGroupRequest.ProtoReflect.Descriptor instead.
func (*MapShardGroupRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{25}
}

func (x *MapShardGroupRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *MapShardGroupRequest) GetMapFunction() *v1.Message {
	if x != nil {
		return x.MapFunction
	}
	return nil
}

func (x *MapShardGroupRequest) GetTimeout() *durationpb.Duration {
	if x != nil {
		return x.Timeout
	}
	return nil
}

func (x *MapShardGroupRequest) GetMinResponses() uint32 {
	if x != nil {
		return x.MinResponses
	}
	return 0
}

type MapShardGroupResponse struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Mapped results from each shard
	ShardResults []*ShardQueryResponse `protobuf:"bytes,1,rep,name=shard_results,json=shardResults,proto3" json:"shard_results,omitempty"`
	// Statistics
	Stats         *ScatterGatherStats `protobuf:"bytes,2,opt,name=stats,proto3" json:"stats,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *MapShardGroupResponse) Reset() {
	*x = MapShardGroupResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[26]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *MapShardGroupResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*MapShardGroupResponse) ProtoMessage() {}

func (x *MapShardGroupResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[26]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use MapShardGroupResponse.ProtoReflect.Descriptor instead.
func (*MapShardGroupResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{26}
}

func (x *MapShardGroupResponse) GetShardResults() []*ShardQueryResponse {
	if x != nil {
		return x.ShardResults
	}
	return nil
}

func (x *MapShardGroupResponse) GetStats() *ScatterGatherStats {
	if x != nil {
		return x.Stats
	}
	return nil
}

type CollectiveTargetField struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	ValuePath     string                 `protobuf:"bytes,1,opt,name=value_path,json=valuePath,proto3" json:"value_path,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *CollectiveTargetField) Reset() {
	*x = CollectiveTargetField{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[27]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *CollectiveTargetField) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*CollectiveTargetField) ProtoMessage() {}

func (x *CollectiveTargetField) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[27]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use CollectiveTargetField.ProtoReflect.Descriptor instead.
func (*CollectiveTargetField) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{27}
}

func (x *CollectiveTargetField) GetValuePath() string {
	if x != nil {
		return x.ValuePath
	}
	return ""
}

type BroadcastShardGroupRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	GroupId       string                 `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	Message       *v1.Message            `protobuf:"bytes,2,opt,name=message,proto3" json:"message,omitempty"`
	Timeout       *durationpb.Duration   `protobuf:"bytes,3,opt,name=timeout,proto3" json:"timeout,omitempty"`
	MinAcks       uint32                 `protobuf:"varint,4,opt,name=min_acks,json=minAcks,proto3" json:"min_acks,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *BroadcastShardGroupRequest) Reset() {
	*x = BroadcastShardGroupRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[28]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *BroadcastShardGroupRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*BroadcastShardGroupRequest) ProtoMessage() {}

func (x *BroadcastShardGroupRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[28]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use BroadcastShardGroupRequest.ProtoReflect.Descriptor instead.
func (*BroadcastShardGroupRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{28}
}

func (x *BroadcastShardGroupRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *BroadcastShardGroupRequest) GetMessage() *v1.Message {
	if x != nil {
		return x.Message
	}
	return nil
}

func (x *BroadcastShardGroupRequest) GetTimeout() *durationpb.Duration {
	if x != nil {
		return x.Timeout
	}
	return nil
}

func (x *BroadcastShardGroupRequest) GetMinAcks() uint32 {
	if x != nil {
		return x.MinAcks
	}
	return 0
}

type BroadcastShardGroupResponse struct {
	state          protoimpl.MessageState `protogen:"open.v1"`
	ShardResponses []*ShardQueryResponse  `protobuf:"bytes,1,rep,name=shard_responses,json=shardResponses,proto3" json:"shard_responses,omitempty"`
	Stats          *ScatterGatherStats    `protobuf:"bytes,2,opt,name=stats,proto3" json:"stats,omitempty"`
	unknownFields  protoimpl.UnknownFields
	sizeCache      protoimpl.SizeCache
}

func (x *BroadcastShardGroupResponse) Reset() {
	*x = BroadcastShardGroupResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[29]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *BroadcastShardGroupResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*BroadcastShardGroupResponse) ProtoMessage() {}

func (x *BroadcastShardGroupResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[29]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use BroadcastShardGroupResponse.ProtoReflect.Descriptor instead.
func (*BroadcastShardGroupResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{29}
}

func (x *BroadcastShardGroupResponse) GetShardResponses() []*ShardQueryResponse {
	if x != nil {
		return x.ShardResponses
	}
	return nil
}

func (x *BroadcastShardGroupResponse) GetStats() *ScatterGatherStats {
	if x != nil {
		return x.Stats
	}
	return nil
}

type ReduceShardGroupRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	GroupId       string                 `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	MapFunction   *v1.Message            `protobuf:"bytes,2,opt,name=map_function,json=mapFunction,proto3" json:"map_function,omitempty"`
	Timeout       *durationpb.Duration   `protobuf:"bytes,3,opt,name=timeout,proto3" json:"timeout,omitempty"`
	MinResponses  uint32                 `protobuf:"varint,4,opt,name=min_responses,json=minResponses,proto3" json:"min_responses,omitempty"`
	Reduction     CollectiveReduction    `protobuf:"varint,5,opt,name=reduction,proto3,enum=plexspaces.actor.v1.CollectiveReduction" json:"reduction,omitempty"`
	Target        *CollectiveTargetField `protobuf:"bytes,6,opt,name=target,proto3" json:"target,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ReduceShardGroupRequest) Reset() {
	*x = ReduceShardGroupRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[30]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ReduceShardGroupRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ReduceShardGroupRequest) ProtoMessage() {}

func (x *ReduceShardGroupRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[30]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ReduceShardGroupRequest.ProtoReflect.Descriptor instead.
func (*ReduceShardGroupRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{30}
}

func (x *ReduceShardGroupRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *ReduceShardGroupRequest) GetMapFunction() *v1.Message {
	if x != nil {
		return x.MapFunction
	}
	return nil
}

func (x *ReduceShardGroupRequest) GetTimeout() *durationpb.Duration {
	if x != nil {
		return x.Timeout
	}
	return nil
}

func (x *ReduceShardGroupRequest) GetMinResponses() uint32 {
	if x != nil {
		return x.MinResponses
	}
	return 0
}

func (x *ReduceShardGroupRequest) GetReduction() CollectiveReduction {
	if x != nil {
		return x.Reduction
	}
	return CollectiveReduction_COLLECTIVE_REDUCTION_UNSPECIFIED
}

func (x *ReduceShardGroupRequest) GetTarget() *CollectiveTargetField {
	if x != nil {
		return x.Target
	}
	return nil
}

type ReduceShardGroupResponse struct {
	state          protoimpl.MessageState `protogen:"open.v1"`
	Result         *v1.Message            `protobuf:"bytes,1,opt,name=result,proto3" json:"result,omitempty"`
	ShardResponses []*ShardQueryResponse  `protobuf:"bytes,2,rep,name=shard_responses,json=shardResponses,proto3" json:"shard_responses,omitempty"`
	Stats          *ScatterGatherStats    `protobuf:"bytes,3,opt,name=stats,proto3" json:"stats,omitempty"`
	unknownFields  protoimpl.UnknownFields
	sizeCache      protoimpl.SizeCache
}

func (x *ReduceShardGroupResponse) Reset() {
	*x = ReduceShardGroupResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[31]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ReduceShardGroupResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ReduceShardGroupResponse) ProtoMessage() {}

func (x *ReduceShardGroupResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[31]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ReduceShardGroupResponse.ProtoReflect.Descriptor instead.
func (*ReduceShardGroupResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{31}
}

func (x *ReduceShardGroupResponse) GetResult() *v1.Message {
	if x != nil {
		return x.Result
	}
	return nil
}

func (x *ReduceShardGroupResponse) GetShardResponses() []*ShardQueryResponse {
	if x != nil {
		return x.ShardResponses
	}
	return nil
}

func (x *ReduceShardGroupResponse) GetStats() *ScatterGatherStats {
	if x != nil {
		return x.Stats
	}
	return nil
}

type AllReduceShardGroupRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	GroupId       string                 `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	MapFunction   *v1.Message            `protobuf:"bytes,2,opt,name=map_function,json=mapFunction,proto3" json:"map_function,omitempty"`
	Timeout       *durationpb.Duration   `protobuf:"bytes,3,opt,name=timeout,proto3" json:"timeout,omitempty"`
	MinResponses  uint32                 `protobuf:"varint,4,opt,name=min_responses,json=minResponses,proto3" json:"min_responses,omitempty"`
	Reduction     CollectiveReduction    `protobuf:"varint,5,opt,name=reduction,proto3,enum=plexspaces.actor.v1.CollectiveReduction" json:"reduction,omitempty"`
	Target        *CollectiveTargetField `protobuf:"bytes,6,opt,name=target,proto3" json:"target,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *AllReduceShardGroupRequest) Reset() {
	*x = AllReduceShardGroupRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[32]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *AllReduceShardGroupRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*AllReduceShardGroupRequest) ProtoMessage() {}

func (x *AllReduceShardGroupRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[32]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use AllReduceShardGroupRequest.ProtoReflect.Descriptor instead.
func (*AllReduceShardGroupRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{32}
}

func (x *AllReduceShardGroupRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *AllReduceShardGroupRequest) GetMapFunction() *v1.Message {
	if x != nil {
		return x.MapFunction
	}
	return nil
}

func (x *AllReduceShardGroupRequest) GetTimeout() *durationpb.Duration {
	if x != nil {
		return x.Timeout
	}
	return nil
}

func (x *AllReduceShardGroupRequest) GetMinResponses() uint32 {
	if x != nil {
		return x.MinResponses
	}
	return 0
}

func (x *AllReduceShardGroupRequest) GetReduction() CollectiveReduction {
	if x != nil {
		return x.Reduction
	}
	return CollectiveReduction_COLLECTIVE_REDUCTION_UNSPECIFIED
}

func (x *AllReduceShardGroupRequest) GetTarget() *CollectiveTargetField {
	if x != nil {
		return x.Target
	}
	return nil
}

type AllReduceShardGroupResponse struct {
	state          protoimpl.MessageState `protogen:"open.v1"`
	Result         *v1.Message            `protobuf:"bytes,1,opt,name=result,proto3" json:"result,omitempty"`
	ShardResponses []*ShardQueryResponse  `protobuf:"bytes,2,rep,name=shard_responses,json=shardResponses,proto3" json:"shard_responses,omitempty"`
	Stats          *ScatterGatherStats    `protobuf:"bytes,3,opt,name=stats,proto3" json:"stats,omitempty"`
	unknownFields  protoimpl.UnknownFields
	sizeCache      protoimpl.SizeCache
}

func (x *AllReduceShardGroupResponse) Reset() {
	*x = AllReduceShardGroupResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[33]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *AllReduceShardGroupResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*AllReduceShardGroupResponse) ProtoMessage() {}

func (x *AllReduceShardGroupResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[33]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use AllReduceShardGroupResponse.ProtoReflect.Descriptor instead.
func (*AllReduceShardGroupResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{33}
}

func (x *AllReduceShardGroupResponse) GetResult() *v1.Message {
	if x != nil {
		return x.Result
	}
	return nil
}

func (x *AllReduceShardGroupResponse) GetShardResponses() []*ShardQueryResponse {
	if x != nil {
		return x.ShardResponses
	}
	return nil
}

func (x *AllReduceShardGroupResponse) GetStats() *ScatterGatherStats {
	if x != nil {
		return x.Stats
	}
	return nil
}

type BarrierShardGroupRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	GroupId       string                 `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	BarrierId     string                 `protobuf:"bytes,2,opt,name=barrier_id,json=barrierId,proto3" json:"barrier_id,omitempty"`
	Round         uint64                 `protobuf:"varint,3,opt,name=round,proto3" json:"round,omitempty"`
	Timeout       *durationpb.Duration   `protobuf:"bytes,4,opt,name=timeout,proto3" json:"timeout,omitempty"`
	MinAcks       uint32                 `protobuf:"varint,5,opt,name=min_acks,json=minAcks,proto3" json:"min_acks,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *BarrierShardGroupRequest) Reset() {
	*x = BarrierShardGroupRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[34]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *BarrierShardGroupRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*BarrierShardGroupRequest) ProtoMessage() {}

func (x *BarrierShardGroupRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[34]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use BarrierShardGroupRequest.ProtoReflect.Descriptor instead.
func (*BarrierShardGroupRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{34}
}

func (x *BarrierShardGroupRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *BarrierShardGroupRequest) GetBarrierId() string {
	if x != nil {
		return x.BarrierId
	}
	return ""
}

func (x *BarrierShardGroupRequest) GetRound() uint64 {
	if x != nil {
		return x.Round
	}
	return 0
}

func (x *BarrierShardGroupRequest) GetTimeout() *durationpb.Duration {
	if x != nil {
		return x.Timeout
	}
	return nil
}

func (x *BarrierShardGroupRequest) GetMinAcks() uint32 {
	if x != nil {
		return x.MinAcks
	}
	return 0
}

type BarrierShardGroupResponse struct {
	state          protoimpl.MessageState `protogen:"open.v1"`
	ShardResponses []*ShardQueryResponse  `protobuf:"bytes,1,rep,name=shard_responses,json=shardResponses,proto3" json:"shard_responses,omitempty"`
	Stats          *ScatterGatherStats    `protobuf:"bytes,2,opt,name=stats,proto3" json:"stats,omitempty"`
	unknownFields  protoimpl.UnknownFields
	sizeCache      protoimpl.SizeCache
}

func (x *BarrierShardGroupResponse) Reset() {
	*x = BarrierShardGroupResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[35]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *BarrierShardGroupResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*BarrierShardGroupResponse) ProtoMessage() {}

func (x *BarrierShardGroupResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[35]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use BarrierShardGroupResponse.ProtoReflect.Descriptor instead.
func (*BarrierShardGroupResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{35}
}

func (x *BarrierShardGroupResponse) GetShardResponses() []*ShardQueryResponse {
	if x != nil {
		return x.ShardResponses
	}
	return nil
}

func (x *BarrierShardGroupResponse) GetStats() *ScatterGatherStats {
	if x != nil {
		return x.Stats
	}
	return nil
}

// Scale shard group (add/remove shards with rebalancing)
type ScaleShardGroupRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Group to scale
	GroupId string `protobuf:"bytes,1,opt,name=group_id,json=groupId,proto3" json:"group_id,omitempty"`
	// New shard count
	NewShardCount uint32 `protobuf:"varint,2,opt,name=new_shard_count,json=newShardCount,proto3" json:"new_shard_count,omitempty"`
	// Rebalancing policy
	RebalancePolicy RebalancePolicy `protobuf:"varint,3,opt,name=rebalance_policy,json=rebalancePolicy,proto3,enum=plexspaces.actor.v1.RebalancePolicy" json:"rebalance_policy,omitempty"`
	// Configuration for new shards (if scaling up)
	NewShardConfig *ActorConfig `protobuf:"bytes,4,opt,name=new_shard_config,json=newShardConfig,proto3" json:"new_shard_config,omitempty"`
	unknownFields  protoimpl.UnknownFields
	sizeCache      protoimpl.SizeCache
}

func (x *ScaleShardGroupRequest) Reset() {
	*x = ScaleShardGroupRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[36]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ScaleShardGroupRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ScaleShardGroupRequest) ProtoMessage() {}

func (x *ScaleShardGroupRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[36]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ScaleShardGroupRequest.ProtoReflect.Descriptor instead.
func (*ScaleShardGroupRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{36}
}

func (x *ScaleShardGroupRequest) GetGroupId() string {
	if x != nil {
		return x.GroupId
	}
	return ""
}

func (x *ScaleShardGroupRequest) GetNewShardCount() uint32 {
	if x != nil {
		return x.NewShardCount
	}
	return 0
}

func (x *ScaleShardGroupRequest) GetRebalancePolicy() RebalancePolicy {
	if x != nil {
		return x.RebalancePolicy
	}
	return RebalancePolicy_REBALANCE_POLICY_UNSPECIFIED
}

func (x *ScaleShardGroupRequest) GetNewShardConfig() *ActorConfig {
	if x != nil {
		return x.NewShardConfig
	}
	return nil
}

type ScaleShardGroupResponse struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Updated shard group
	Group *ShardGroup `protobuf:"bytes,1,opt,name=group,proto3" json:"group,omitempty"`
	// Rebalancing status
	RebalanceStatus *RebalanceStatus `protobuf:"bytes,2,opt,name=rebalance_status,json=rebalanceStatus,proto3" json:"rebalance_status,omitempty"`
	unknownFields   protoimpl.UnknownFields
	sizeCache       protoimpl.SizeCache
}

func (x *ScaleShardGroupResponse) Reset() {
	*x = ScaleShardGroupResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[37]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ScaleShardGroupResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ScaleShardGroupResponse) ProtoMessage() {}

func (x *ScaleShardGroupResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[37]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ScaleShardGroupResponse.ProtoReflect.Descriptor instead.
func (*ScaleShardGroupResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{37}
}

func (x *ScaleShardGroupResponse) GetGroup() *ShardGroup {
	if x != nil {
		return x.Group
	}
	return nil
}

func (x *ScaleShardGroupResponse) GetRebalanceStatus() *RebalanceStatus {
	if x != nil {
		return x.RebalanceStatus
	}
	return nil
}

// Actor performance metrics
type ActorMetrics struct {
	state                 protoimpl.MessageState `protogen:"open.v1"`
	MessagesProcessed     uint64                 `protobuf:"varint,1,opt,name=messages_processed,json=messagesProcessed,proto3" json:"messages_processed,omitempty"`
	MessagesFailed        uint64                 `protobuf:"varint,2,opt,name=messages_failed,json=messagesFailed,proto3" json:"messages_failed,omitempty"`
	AverageProcessingTime *durationpb.Duration   `protobuf:"bytes,3,opt,name=average_processing_time,json=averageProcessingTime,proto3" json:"average_processing_time,omitempty"` // Max 1 hour per message
	Restarts              uint64                 `protobuf:"varint,4,opt,name=restarts,proto3" json:"restarts,omitempty"`
	LastActivity          *timestamppb.Timestamp `protobuf:"bytes,5,opt,name=last_activity,json=lastActivity,proto3" json:"last_activity,omitempty"`
	MemoryUsageBytes      uint64                 `protobuf:"varint,6,opt,name=memory_usage_bytes,json=memoryUsageBytes,proto3" json:"memory_usage_bytes,omitempty"` // Max 1TB
	CpuUsagePercent       float64                `protobuf:"fixed64,7,opt,name=cpu_usage_percent,json=cpuUsagePercent,proto3" json:"cpu_usage_percent,omitempty"`
	unknownFields         protoimpl.UnknownFields
	sizeCache             protoimpl.SizeCache
}

func (x *ActorMetrics) Reset() {
	*x = ActorMetrics{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[38]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorMetrics) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorMetrics) ProtoMessage() {}

func (x *ActorMetrics) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[38]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorMetrics.ProtoReflect.Descriptor instead.
func (*ActorMetrics) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{38}
}

func (x *ActorMetrics) GetMessagesProcessed() uint64 {
	if x != nil {
		return x.MessagesProcessed
	}
	return 0
}

func (x *ActorMetrics) GetMessagesFailed() uint64 {
	if x != nil {
		return x.MessagesFailed
	}
	return 0
}

func (x *ActorMetrics) GetAverageProcessingTime() *durationpb.Duration {
	if x != nil {
		return x.AverageProcessingTime
	}
	return nil
}

func (x *ActorMetrics) GetRestarts() uint64 {
	if x != nil {
		return x.Restarts
	}
	return 0
}

func (x *ActorMetrics) GetLastActivity() *timestamppb.Timestamp {
	if x != nil {
		return x.LastActivity
	}
	return nil
}

func (x *ActorMetrics) GetMemoryUsageBytes() uint64 {
	if x != nil {
		return x.MemoryUsageBytes
	}
	return 0
}

func (x *ActorMetrics) GetCpuUsagePercent() float64 {
	if x != nil {
		return x.CpuUsagePercent
	}
	return 0
}

type SpawnActorRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Full spawn contract: identity, role, namespace, tenant_id, behavior_kind, args, facets, labels, config, visibility.
	Spec *ActorSpawnSpec `protobuf:"bytes,1,opt,name=spec,proto3" json:"spec,omitempty"`
	// Optional namespace override for this RPC only.
	// If non-empty, the server merges this into spec.namespace for the spawn operation; if empty, spec.namespace is used.
	Namespace string `protobuf:"bytes,2,opt,name=namespace,proto3" json:"namespace,omitempty"`
	// Number of identical replicas to spawn (default: 1 when 0).
	// When > 1, spawns N actors with auto-generated instance names derived from spec.identity.name (prefix pattern).
	InstancesCount uint32 `protobuf:"varint,3,opt,name=instances_count,json=instancesCount,proto3" json:"instances_count,omitempty"`
	unknownFields  protoimpl.UnknownFields
	sizeCache      protoimpl.SizeCache
}

func (x *SpawnActorRequest) Reset() {
	*x = SpawnActorRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[39]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SpawnActorRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SpawnActorRequest) ProtoMessage() {}

func (x *SpawnActorRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[39]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SpawnActorRequest.ProtoReflect.Descriptor instead.
func (*SpawnActorRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{39}
}

func (x *SpawnActorRequest) GetSpec() *ActorSpawnSpec {
	if x != nil {
		return x.Spec
	}
	return nil
}

func (x *SpawnActorRequest) GetNamespace() string {
	if x != nil {
		return x.Namespace
	}
	return ""
}

func (x *SpawnActorRequest) GetInstancesCount() uint32 {
	if x != nil {
		return x.InstancesCount
	}
	return 0
}

// Response from SpawnActor
//
// ## Purpose
// Returns reference to newly spawned actor on the node receiving the request.
//
// ## Design Notes
// - actor_ref: String in format "actor_id@target_node_id"
//   - Can be used immediately for tell/ask operations
//   - Location transparent - same API as local actors
//   - Example: "actor-abc123@node2"
//
// - actor: Full Actor details for inspection
//   - Contains state, config, metrics, etc.
//   - Useful for monitoring and debugging
type SpawnActorResponse struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Reference to spawned actor (format: "actor_id@node_id")
	// Example: "general-1@node2", "worker-abc@prod-7"
	// Use this for messaging: actor_ref.tell(msg), actor_ref.ask(msg)
	ActorRef string `protobuf:"bytes,1,opt,name=actor_ref,json=actorRef,proto3" json:"actor_ref,omitempty"`
	// Full actor details (state, config, metrics)
	// Useful for inspection and monitoring
	Actor         *Actor `protobuf:"bytes,2,opt,name=actor,proto3" json:"actor,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SpawnActorResponse) Reset() {
	*x = SpawnActorResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[40]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SpawnActorResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SpawnActorResponse) ProtoMessage() {}

func (x *SpawnActorResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[40]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SpawnActorResponse.ProtoReflect.Descriptor instead.
func (*SpawnActorResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{40}
}

func (x *SpawnActorResponse) GetActorRef() string {
	if x != nil {
		return x.ActorRef
	}
	return ""
}

func (x *SpawnActorResponse) GetActor() *Actor {
	if x != nil {
		return x.Actor
	}
	return nil
}

type SpawnActorsRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Requests      []*SpawnActorRequest   `protobuf:"bytes,1,rep,name=requests,proto3" json:"requests,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SpawnActorsRequest) Reset() {
	*x = SpawnActorsRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[41]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SpawnActorsRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SpawnActorsRequest) ProtoMessage() {}

func (x *SpawnActorsRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[41]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SpawnActorsRequest.ProtoReflect.Descriptor instead.
func (*SpawnActorsRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{41}
}

func (x *SpawnActorsRequest) GetRequests() []*SpawnActorRequest {
	if x != nil {
		return x.Requests
	}
	return nil
}

type SpawnActorResult struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Success       bool                   `protobuf:"varint,1,opt,name=success,proto3" json:"success,omitempty"`
	Error         string                 `protobuf:"bytes,2,opt,name=error,proto3" json:"error,omitempty"`
	Response      *SpawnActorResponse    `protobuf:"bytes,3,opt,name=response,proto3" json:"response,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SpawnActorResult) Reset() {
	*x = SpawnActorResult{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[42]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SpawnActorResult) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SpawnActorResult) ProtoMessage() {}

func (x *SpawnActorResult) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[42]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SpawnActorResult.ProtoReflect.Descriptor instead.
func (*SpawnActorResult) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{42}
}

func (x *SpawnActorResult) GetSuccess() bool {
	if x != nil {
		return x.Success
	}
	return false
}

func (x *SpawnActorResult) GetError() string {
	if x != nil {
		return x.Error
	}
	return ""
}

func (x *SpawnActorResult) GetResponse() *SpawnActorResponse {
	if x != nil {
		return x.Response
	}
	return nil
}

type SpawnActorsResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Results       []*SpawnActorResult    `protobuf:"bytes,1,rep,name=results,proto3" json:"results,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SpawnActorsResponse) Reset() {
	*x = SpawnActorsResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[43]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SpawnActorsResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SpawnActorsResponse) ProtoMessage() {}

func (x *SpawnActorsResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[43]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SpawnActorsResponse.ProtoReflect.Descriptor instead.
func (*SpawnActorsResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{43}
}

func (x *SpawnActorsResponse) GetResults() []*SpawnActorResult {
	if x != nil {
		return x.Results
	}
	return nil
}

// Request to get an actor
type GetActorRequest struct {
	state   protoimpl.MessageState `protogen:"open.v1"`
	ActorId string                 `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	// Namespace for tenant isolation (tenant_id from JWT)
	Namespace     string `protobuf:"bytes,2,opt,name=namespace,proto3" json:"namespace,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *GetActorRequest) Reset() {
	*x = GetActorRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[44]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *GetActorRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*GetActorRequest) ProtoMessage() {}

func (x *GetActorRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[44]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use GetActorRequest.ProtoReflect.Descriptor instead.
func (*GetActorRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{44}
}

func (x *GetActorRequest) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *GetActorRequest) GetNamespace() string {
	if x != nil {
		return x.Namespace
	}
	return ""
}

type GetActorResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Actor         *Actor                 `protobuf:"bytes,1,opt,name=actor,proto3" json:"actor,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *GetActorResponse) Reset() {
	*x = GetActorResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[45]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *GetActorResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*GetActorResponse) ProtoMessage() {}

func (x *GetActorResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[45]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use GetActorResponse.ProtoReflect.Descriptor instead.
func (*GetActorResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{45}
}

func (x *GetActorResponse) GetActor() *Actor {
	if x != nil {
		return x.Actor
	}
	return nil
}

// Request to list actors
type ListActorsRequest struct {
	state       protoimpl.MessageState `protogen:"open.v1"`
	PageRequest *v1.PageRequest        `protobuf:"bytes,1,opt,name=page_request,json=pageRequest,proto3" json:"page_request,omitempty"`
	ActorType   string                 `protobuf:"bytes,2,opt,name=actor_type,json=actorType,proto3" json:"actor_type,omitempty"`
	State       ActorState             `protobuf:"varint,3,opt,name=state,proto3,enum=plexspaces.actor.v1.ActorState" json:"state,omitempty"`
	NodeId      string                 `protobuf:"bytes,4,opt,name=node_id,json=nodeId,proto3" json:"node_id,omitempty"`
	// Namespace for tenant isolation (tenant_id from JWT)
	// Only actors in this namespace will be returned
	Namespace     string `protobuf:"bytes,5,opt,name=namespace,proto3" json:"namespace,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ListActorsRequest) Reset() {
	*x = ListActorsRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[46]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ListActorsRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ListActorsRequest) ProtoMessage() {}

func (x *ListActorsRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[46]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ListActorsRequest.ProtoReflect.Descriptor instead.
func (*ListActorsRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{46}
}

func (x *ListActorsRequest) GetPageRequest() *v1.PageRequest {
	if x != nil {
		return x.PageRequest
	}
	return nil
}

func (x *ListActorsRequest) GetActorType() string {
	if x != nil {
		return x.ActorType
	}
	return ""
}

func (x *ListActorsRequest) GetState() ActorState {
	if x != nil {
		return x.State
	}
	return ActorState_ACTOR_STATE_UNSPECIFIED
}

func (x *ListActorsRequest) GetNodeId() string {
	if x != nil {
		return x.NodeId
	}
	return ""
}

func (x *ListActorsRequest) GetNamespace() string {
	if x != nil {
		return x.Namespace
	}
	return ""
}

type ListActorsResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Actors        []*Actor               `protobuf:"bytes,1,rep,name=actors,proto3" json:"actors,omitempty"`
	PageResponse  *v1.PageResponse       `protobuf:"bytes,2,opt,name=page_response,json=pageResponse,proto3" json:"page_response,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ListActorsResponse) Reset() {
	*x = ListActorsResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[47]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ListActorsResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ListActorsResponse) ProtoMessage() {}

func (x *ListActorsResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[47]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ListActorsResponse.ProtoReflect.Descriptor instead.
func (*ListActorsResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{47}
}

func (x *ListActorsResponse) GetActors() []*Actor {
	if x != nil {
		return x.Actors
	}
	return nil
}

func (x *ListActorsResponse) GetPageResponse() *v1.PageResponse {
	if x != nil {
		return x.PageResponse
	}
	return nil
}

// Request to send a message via tell semantics
type SendMessageRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Namespace (extracted from path: /api/v1/actors/{namespace}/{actor_type})
	Namespace string `protobuf:"bytes,1,opt,name=namespace,proto3" json:"namespace,omitempty"`
	// Actor type or actor id to target.
	// If this value contains '@', it is treated as a direct actor id first.
	ActorType string `protobuf:"bytes,2,opt,name=actor_type,json=actorType,proto3" json:"actor_type,omitempty"`
	// Optional HTTP method metadata for gateway-originated requests.
	HttpMethod string `protobuf:"bytes,3,opt,name=http_method,json=httpMethod,proto3" json:"http_method,omitempty"`
	// Request payload bytes.
	Payload []byte `protobuf:"bytes,4,opt,name=payload,proto3" json:"payload,omitempty"`
	// Request headers or message metadata.
	Headers map[string]string `protobuf:"bytes,5,rep,name=headers,proto3" json:"headers,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Query parameters from HTTP requests.
	QueryParams map[string]string `protobuf:"bytes,6,rep,name=query_params,json=queryParams,proto3" json:"query_params,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Full request path for gateway-originated requests.
	Path string `protobuf:"bytes,7,opt,name=path,proto3" json:"path,omitempty"`
	// Remaining path segment after actor_type.
	Subpath string `protobuf:"bytes,8,opt,name=subpath,proto3" json:"subpath,omitempty"`
	// Optional sender actor id for remoting and tracing.
	SenderId string `protobuf:"bytes,9,opt,name=sender_id,json=senderId,proto3" json:"sender_id,omitempty"`
	// Optional application or transport message type.
	MessageType string `protobuf:"bytes,10,opt,name=message_type,json=messageType,proto3" json:"message_type,omitempty"`
	// Optional correlation id preserved for remoting.
	CorrelationId string `protobuf:"bytes,11,opt,name=correlation_id,json=correlationId,proto3" json:"correlation_id,omitempty"`
	// Optional reply_to preserved for remoting.
	ReplyTo string `protobuf:"bytes,12,opt,name=reply_to,json=replyTo,proto3" json:"reply_to,omitempty"`
	// Optional client-provided message id.
	MessageId string `protobuf:"bytes,13,opt,name=message_id,json=messageId,proto3" json:"message_id,omitempty"`
	// Optional actor instance name. When set together with actor_type and namespace,
	// the handler constructs the canonical actor ID directly:
	//
	//	actor_name//actor_type::namespace@node_id
	//
	// This avoids ambiguous lookups and makes addressing explicit.
	// If empty, falls back to registry lookup by actor_type within the namespace.
	ActorName     string `protobuf:"bytes,20,opt,name=actor_name,json=actorName,proto3" json:"actor_name,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SendMessageRequest) Reset() {
	*x = SendMessageRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[48]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SendMessageRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SendMessageRequest) ProtoMessage() {}

func (x *SendMessageRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[48]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SendMessageRequest.ProtoReflect.Descriptor instead.
func (*SendMessageRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{48}
}

func (x *SendMessageRequest) GetNamespace() string {
	if x != nil {
		return x.Namespace
	}
	return ""
}

func (x *SendMessageRequest) GetActorType() string {
	if x != nil {
		return x.ActorType
	}
	return ""
}

func (x *SendMessageRequest) GetHttpMethod() string {
	if x != nil {
		return x.HttpMethod
	}
	return ""
}

func (x *SendMessageRequest) GetPayload() []byte {
	if x != nil {
		return x.Payload
	}
	return nil
}

func (x *SendMessageRequest) GetHeaders() map[string]string {
	if x != nil {
		return x.Headers
	}
	return nil
}

func (x *SendMessageRequest) GetQueryParams() map[string]string {
	if x != nil {
		return x.QueryParams
	}
	return nil
}

func (x *SendMessageRequest) GetPath() string {
	if x != nil {
		return x.Path
	}
	return ""
}

func (x *SendMessageRequest) GetSubpath() string {
	if x != nil {
		return x.Subpath
	}
	return ""
}

func (x *SendMessageRequest) GetSenderId() string {
	if x != nil {
		return x.SenderId
	}
	return ""
}

func (x *SendMessageRequest) GetMessageType() string {
	if x != nil {
		return x.MessageType
	}
	return ""
}

func (x *SendMessageRequest) GetCorrelationId() string {
	if x != nil {
		return x.CorrelationId
	}
	return ""
}

func (x *SendMessageRequest) GetReplyTo() string {
	if x != nil {
		return x.ReplyTo
	}
	return ""
}

func (x *SendMessageRequest) GetMessageId() string {
	if x != nil {
		return x.MessageId
	}
	return ""
}

func (x *SendMessageRequest) GetActorName() string {
	if x != nil {
		return x.ActorName
	}
	return ""
}

type SendMessageResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Success       bool                   `protobuf:"varint,1,opt,name=success,proto3" json:"success,omitempty"`
	MessageId     string                 `protobuf:"bytes,2,opt,name=message_id,json=messageId,proto3" json:"message_id,omitempty"`
	ActorId       string                 `protobuf:"bytes,3,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	ErrorMessage  string                 `protobuf:"bytes,4,opt,name=error_message,json=errorMessage,proto3" json:"error_message,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SendMessageResponse) Reset() {
	*x = SendMessageResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[49]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SendMessageResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SendMessageResponse) ProtoMessage() {}

func (x *SendMessageResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[49]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SendMessageResponse.ProtoReflect.Descriptor instead.
func (*SendMessageResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{49}
}

func (x *SendMessageResponse) GetSuccess() bool {
	if x != nil {
		return x.Success
	}
	return false
}

func (x *SendMessageResponse) GetMessageId() string {
	if x != nil {
		return x.MessageId
	}
	return ""
}

func (x *SendMessageResponse) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *SendMessageResponse) GetErrorMessage() string {
	if x != nil {
		return x.ErrorMessage
	}
	return ""
}

// Request for streaming messages (high-throughput)
type StreamMessageRequest struct {
	state   protoimpl.MessageState `protogen:"open.v1"`
	Message *v1.Message            `protobuf:"bytes,1,opt,name=message,proto3" json:"message,omitempty"`
	// Sequence number for ordering (client-generated)
	Sequence      uint64 `protobuf:"varint,2,opt,name=sequence,proto3" json:"sequence,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *StreamMessageRequest) Reset() {
	*x = StreamMessageRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[50]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *StreamMessageRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*StreamMessageRequest) ProtoMessage() {}

func (x *StreamMessageRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[50]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use StreamMessageRequest.ProtoReflect.Descriptor instead.
func (*StreamMessageRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{50}
}

func (x *StreamMessageRequest) GetMessage() *v1.Message {
	if x != nil {
		return x.Message
	}
	return nil
}

func (x *StreamMessageRequest) GetSequence() uint64 {
	if x != nil {
		return x.Sequence
	}
	return 0
}

// Response for streaming messages
type StreamMessageResponse struct {
	state     protoimpl.MessageState `protogen:"open.v1"`
	MessageId string                 `protobuf:"bytes,1,opt,name=message_id,json=messageId,proto3" json:"message_id,omitempty"`
	// Acknowledgement of sequence number
	Sequence uint64 `protobuf:"varint,2,opt,name=sequence,proto3" json:"sequence,omitempty"`
	// Status: "delivered", "failed", "queued"
	Status string `protobuf:"bytes,3,opt,name=status,proto3" json:"status,omitempty"`
	// Optional error message
	Error         string `protobuf:"bytes,4,opt,name=error,proto3" json:"error,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *StreamMessageResponse) Reset() {
	*x = StreamMessageResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[51]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *StreamMessageResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*StreamMessageResponse) ProtoMessage() {}

func (x *StreamMessageResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[51]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use StreamMessageResponse.ProtoReflect.Descriptor instead.
func (*StreamMessageResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{51}
}

func (x *StreamMessageResponse) GetMessageId() string {
	if x != nil {
		return x.MessageId
	}
	return ""
}

func (x *StreamMessageResponse) GetSequence() uint64 {
	if x != nil {
		return x.Sequence
	}
	return 0
}

func (x *StreamMessageResponse) GetStatus() string {
	if x != nil {
		return x.Status
	}
	return ""
}

func (x *StreamMessageResponse) GetError() string {
	if x != nil {
		return x.Error
	}
	return ""
}

// Request to delete actor
type DeleteActorRequest struct {
	state   protoimpl.MessageState `protogen:"open.v1"`
	ActorId string                 `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	Force   bool                   `protobuf:"varint,2,opt,name=force,proto3" json:"force,omitempty"`
	// Namespace for tenant isolation (tenant_id from JWT)
	Namespace     string `protobuf:"bytes,3,opt,name=namespace,proto3" json:"namespace,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *DeleteActorRequest) Reset() {
	*x = DeleteActorRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[52]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *DeleteActorRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*DeleteActorRequest) ProtoMessage() {}

func (x *DeleteActorRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[52]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use DeleteActorRequest.ProtoReflect.Descriptor instead.
func (*DeleteActorRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{52}
}

func (x *DeleteActorRequest) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *DeleteActorRequest) GetForce() bool {
	if x != nil {
		return x.Force
	}
	return false
}

func (x *DeleteActorRequest) GetNamespace() string {
	if x != nil {
		return x.Namespace
	}
	return ""
}

// Actor lifecycle event
//
// ## Purpose
// Represents a state transition in an actor's lifecycle. Used for monitoring,
// observability, and triggering supervision actions.
//
// ## Architecture Context
// - Emitted by Actor implementation when state changes occur
// - Consumed by Node for monitoring callbacks
// - Consumed by observability systems (metrics, tracing, logging)
// - Enables location-transparent monitoring (Erlang-style)
//
// ## Design Decisions
// - Proto-based for distributed type safety across nodes
// - event_type as oneof for type-safe event variants
// - Timestamp for event ordering and time-travel debugging
// - Extensible for future event types without breaking changes
//
// ## Usage
// ```rust
// // Actor emits event
//
//	let event = ActorLifecycleEvent {
//	    actor_id: "worker@node1".to_string(),
//	    timestamp: Some(Timestamp::now()),
//	    event_type: Some(actor_lifecycle_event::EventType::Terminated(
//	        ActorTerminated { reason: "normal".to_string() }
//	    )),
//	};
//
// node.handle_lifecycle_event(event).await?;
// ```
type ActorLifecycleEvent struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Actor that emitted this event
	ActorId string `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	// When event occurred
	Timestamp *timestamppb.Timestamp `protobuf:"bytes,2,opt,name=timestamp,proto3" json:"timestamp,omitempty"`
	// Event-specific data (oneof ensures type safety)
	//
	// Types that are valid to be assigned to EventType:
	//
	//	*ActorLifecycleEvent_Created
	//	*ActorLifecycleEvent_Starting
	//	*ActorLifecycleEvent_Activated
	//	*ActorLifecycleEvent_Deactivating
	//	*ActorLifecycleEvent_Deactivated
	//	*ActorLifecycleEvent_Terminated
	//	*ActorLifecycleEvent_Failed
	//	*ActorLifecycleEvent_Migrating
	EventType     isActorLifecycleEvent_EventType `protobuf_oneof:"event_type"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorLifecycleEvent) Reset() {
	*x = ActorLifecycleEvent{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[53]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorLifecycleEvent) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorLifecycleEvent) ProtoMessage() {}

func (x *ActorLifecycleEvent) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[53]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorLifecycleEvent.ProtoReflect.Descriptor instead.
func (*ActorLifecycleEvent) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{53}
}

func (x *ActorLifecycleEvent) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *ActorLifecycleEvent) GetTimestamp() *timestamppb.Timestamp {
	if x != nil {
		return x.Timestamp
	}
	return nil
}

func (x *ActorLifecycleEvent) GetEventType() isActorLifecycleEvent_EventType {
	if x != nil {
		return x.EventType
	}
	return nil
}

func (x *ActorLifecycleEvent) GetCreated() *ActorCreated {
	if x != nil {
		if x, ok := x.EventType.(*ActorLifecycleEvent_Created); ok {
			return x.Created
		}
	}
	return nil
}

func (x *ActorLifecycleEvent) GetStarting() *ActorStarting {
	if x != nil {
		if x, ok := x.EventType.(*ActorLifecycleEvent_Starting); ok {
			return x.Starting
		}
	}
	return nil
}

func (x *ActorLifecycleEvent) GetActivated() *ActorActivated {
	if x != nil {
		if x, ok := x.EventType.(*ActorLifecycleEvent_Activated); ok {
			return x.Activated
		}
	}
	return nil
}

func (x *ActorLifecycleEvent) GetDeactivating() *ActorDeactivating {
	if x != nil {
		if x, ok := x.EventType.(*ActorLifecycleEvent_Deactivating); ok {
			return x.Deactivating
		}
	}
	return nil
}

func (x *ActorLifecycleEvent) GetDeactivated() *ActorDeactivated {
	if x != nil {
		if x, ok := x.EventType.(*ActorLifecycleEvent_Deactivated); ok {
			return x.Deactivated
		}
	}
	return nil
}

func (x *ActorLifecycleEvent) GetTerminated() *ActorTerminated {
	if x != nil {
		if x, ok := x.EventType.(*ActorLifecycleEvent_Terminated); ok {
			return x.Terminated
		}
	}
	return nil
}

func (x *ActorLifecycleEvent) GetFailed() *ActorFailed {
	if x != nil {
		if x, ok := x.EventType.(*ActorLifecycleEvent_Failed); ok {
			return x.Failed
		}
	}
	return nil
}

func (x *ActorLifecycleEvent) GetMigrating() *ActorMigrating {
	if x != nil {
		if x, ok := x.EventType.(*ActorLifecycleEvent_Migrating); ok {
			return x.Migrating
		}
	}
	return nil
}

type isActorLifecycleEvent_EventType interface {
	isActorLifecycleEvent_EventType()
}

type ActorLifecycleEvent_Created struct {
	Created *ActorCreated `protobuf:"bytes,10,opt,name=created,proto3,oneof"`
}

type ActorLifecycleEvent_Starting struct {
	Starting *ActorStarting `protobuf:"bytes,11,opt,name=starting,proto3,oneof"`
}

type ActorLifecycleEvent_Activated struct {
	Activated *ActorActivated `protobuf:"bytes,12,opt,name=activated,proto3,oneof"`
}

type ActorLifecycleEvent_Deactivating struct {
	Deactivating *ActorDeactivating `protobuf:"bytes,13,opt,name=deactivating,proto3,oneof"`
}

type ActorLifecycleEvent_Deactivated struct {
	Deactivated *ActorDeactivated `protobuf:"bytes,14,opt,name=deactivated,proto3,oneof"`
}

type ActorLifecycleEvent_Terminated struct {
	Terminated *ActorTerminated `protobuf:"bytes,15,opt,name=terminated,proto3,oneof"`
}

type ActorLifecycleEvent_Failed struct {
	Failed *ActorFailed `protobuf:"bytes,16,opt,name=failed,proto3,oneof"`
}

type ActorLifecycleEvent_Migrating struct {
	Migrating *ActorMigrating `protobuf:"bytes,17,opt,name=migrating,proto3,oneof"`
}

func (*ActorLifecycleEvent_Created) isActorLifecycleEvent_EventType() {}

func (*ActorLifecycleEvent_Starting) isActorLifecycleEvent_EventType() {}

func (*ActorLifecycleEvent_Activated) isActorLifecycleEvent_EventType() {}

func (*ActorLifecycleEvent_Deactivating) isActorLifecycleEvent_EventType() {}

func (*ActorLifecycleEvent_Deactivated) isActorLifecycleEvent_EventType() {}

func (*ActorLifecycleEvent_Terminated) isActorLifecycleEvent_EventType() {}

func (*ActorLifecycleEvent_Failed) isActorLifecycleEvent_EventType() {}

func (*ActorLifecycleEvent_Migrating) isActorLifecycleEvent_EventType() {}

// Actor created (construction complete, not yet started)
//
// ## Purpose
// Emitted after Actor::new() completes successfully.
//
// ## State Transition
// [none] -> Created
//
// ## Supervisor Action
// None (waiting for activation)
type ActorCreated struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorCreated) Reset() {
	*x = ActorCreated{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[54]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorCreated) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorCreated) ProtoMessage() {}

func (x *ActorCreated) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[54]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorCreated.ProtoReflect.Descriptor instead.
func (*ActorCreated) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{54}
}

// Actor starting (spawning message processing task)
//
// ## Purpose
// Emitted when Actor::start() is called, before tokio::spawn().
//
// ## State Transition
// Created -> Starting
//
// ## Supervisor Action
// None (normal startup)
type ActorStarting struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorStarting) Reset() {
	*x = ActorStarting{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[55]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorStarting) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorStarting) ProtoMessage() {}

func (x *ActorStarting) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[55]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorStarting.ProtoReflect.Descriptor instead.
func (*ActorStarting) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{55}
}

// Actor activated (ready to process messages)
//
// ## Purpose
// Emitted after actor's message loop starts and on_activate() hook completes.
//
// ## State Transition
// Starting -> Activated
//
// ## Supervisor Action
// None (actor running normally)
type ActorActivated struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorActivated) Reset() {
	*x = ActorActivated{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[56]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorActivated) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorActivated) ProtoMessage() {}

func (x *ActorActivated) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[56]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorActivated.ProtoReflect.Descriptor instead.
func (*ActorActivated) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{56}
}

// Actor deactivating (graceful shutdown initiated)
//
// ## Purpose
// Emitted when actor receives shutdown signal or supervisor requests stop.
//
// ## State Transition
// Activated -> Deactivating
//
// ## Supervisor Action
// None (expected shutdown)
type ActorDeactivating struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Why deactivation was initiated
	// Examples: "supervisor_shutdown", "manual_stop", "timeout_idle"
	Reason        string `protobuf:"bytes,1,opt,name=reason,proto3" json:"reason,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorDeactivating) Reset() {
	*x = ActorDeactivating{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[57]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorDeactivating) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorDeactivating) ProtoMessage() {}

func (x *ActorDeactivating) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[57]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorDeactivating.ProtoReflect.Descriptor instead.
func (*ActorDeactivating) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{57}
}

func (x *ActorDeactivating) GetReason() string {
	if x != nil {
		return x.Reason
	}
	return ""
}

// Actor deactivated (shutdown complete, but not destroyed)
//
// ## Purpose
// Emitted after on_deactivate() hook completes. Actor can be reactivated.
//
// ## State Transition
// Deactivating -> Deactivated
//
// ## Supervisor Action
// None (actor cleanly stopped, can be restarted if needed)
type ActorDeactivated struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Why deactivation occurred
	Reason        string `protobuf:"bytes,1,opt,name=reason,proto3" json:"reason,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorDeactivated) Reset() {
	*x = ActorDeactivated{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[58]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorDeactivated) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorDeactivated) ProtoMessage() {}

func (x *ActorDeactivated) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[58]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorDeactivated.ProtoReflect.Descriptor instead.
func (*ActorDeactivated) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{58}
}

func (x *ActorDeactivated) GetReason() string {
	if x != nil {
		return x.Reason
	}
	return ""
}

// Actor terminated (permanently stopped, not restartable)
//
// ## Purpose
// Emitted when actor's task completes normally (loop exits without error).
// Triggers monitoring callbacks (NotifyActorDown).
//
// ## State Transition
// Activated|Deactivating -> Terminated
//
// ## Supervisor Action
// - If restart strategy allows, restart actor
// - Notify all monitors (local + remote via NotifyActorDown RPC)
//
// ## Design Notes
// - reason="normal": Graceful shutdown completed
// - reason="shutdown": Supervisor-initiated shutdown
// - reason="killed": Forcefully terminated
type ActorTerminated struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Termination reason
	// "normal": Graceful shutdown
	// "shutdown": Supervisor-initiated
	// "killed": Forcefully terminated
	Reason        string `protobuf:"bytes,1,opt,name=reason,proto3" json:"reason,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorTerminated) Reset() {
	*x = ActorTerminated{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[59]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorTerminated) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorTerminated) ProtoMessage() {}

func (x *ActorTerminated) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[59]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorTerminated.ProtoReflect.Descriptor instead.
func (*ActorTerminated) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{59}
}

func (x *ActorTerminated) GetReason() string {
	if x != nil {
		return x.Reason
	}
	return ""
}

// Actor failed (crashed, needs supervision action)
//
// ## Purpose
// Emitted when actor's task panics or returns error. Triggers supervision
// restart logic and monitoring callbacks.
//
// ## State Transition
// Activated -> Failed
//
// ## Supervisor Action
// - Apply restart strategy (OneForOne, OneForAll, RestForOne)
// - Notify all monitors with error details
// - Increment failure counter for escalation
//
// ## Design Notes
// - error: Full panic/error message for debugging
// - Supervision tree decides whether to restart based on restart strategy
type ActorFailed struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Error/panic message
	// Examples:
	// - "panic: index out of bounds"
	// - "error: timeout waiting for response"
	// - "error: connection refused"
	Error string `protobuf:"bytes,1,opt,name=error,proto3" json:"error,omitempty"`
	// Optional: Stack trace for debugging
	StackTrace    string `protobuf:"bytes,2,opt,name=stack_trace,json=stackTrace,proto3" json:"stack_trace,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorFailed) Reset() {
	*x = ActorFailed{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[60]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorFailed) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorFailed) ProtoMessage() {}

func (x *ActorFailed) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[60]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorFailed.ProtoReflect.Descriptor instead.
func (*ActorFailed) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{60}
}

func (x *ActorFailed) GetError() string {
	if x != nil {
		return x.Error
	}
	return ""
}

func (x *ActorFailed) GetStackTrace() string {
	if x != nil {
		return x.StackTrace
	}
	return ""
}

// Actor migrating (moving to different node)
//
// ## Purpose
// Emitted when mobile agent starts migration to another node.
//
// ## State Transition
// Activated -> Migrating
//
// ## Supervisor Action
// - Update actor location in registry
// - If migration fails, restart on original node
//
// ## Design Notes
// - target_node: Destination node address (e.g., "node2")
// - Future: Add migration_id for tracking migration progress
type ActorMigrating struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Target node where actor is migrating
	TargetNode    string `protobuf:"bytes,1,opt,name=target_node,json=targetNode,proto3" json:"target_node,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorMigrating) Reset() {
	*x = ActorMigrating{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[61]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorMigrating) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorMigrating) ProtoMessage() {}

func (x *ActorMigrating) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[61]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorMigrating.ProtoReflect.Descriptor instead.
func (*ActorMigrating) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{61}
}

func (x *ActorMigrating) GetTargetNode() string {
	if x != nil {
		return x.TargetNode
	}
	return ""
}

// Request to monitor an actor (Erlang-style)
//
// ## Purpose
// Establishes a monitor from `supervisor_id` to `actor_id` on the **node that hosts
// `actor_id`**. When that actor terminates, that node runs termination handling and
// delivers a `__DOWN__` **mailbox message** to `supervisor_id` (same Erlang semantics
// as `{'DOWN', Ref, process, Pid, Reason}`), routing remotely via `ActorService` when
// the supervisor lives on another node.
//
// ## Erlang Philosophy
// Equivalent to: Ref = erlang:monitor(process, Pid)
// Works the same for local and remote processes (location transparent).
//
// ## Design Notes
//   - actor_id: Canonical actor ID for the actor to monitor
//   - supervisor_id: Canonical actor ID of the process that receives `__DOWN__` in its mailbox
//   - supervisor_callback: **Reserved / wire compatibility.** The Rust server implementation
//     does not use this field today; DOWN is sent with `ActorRegistry::tell` to `supervisor_id`.
//     Clients should still populate it per validation (e.g. supervisor node's ActorService URL)
//     for forward compatibility with a possible push-style callback path.
type MonitorActorRequest struct {
	state              protoimpl.MessageState `protogen:"open.v1"`
	ActorId            string                 `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	SupervisorId       string                 `protobuf:"bytes,2,opt,name=supervisor_id,json=supervisorId,proto3" json:"supervisor_id,omitempty"`
	SupervisorCallback string                 `protobuf:"bytes,3,opt,name=supervisor_callback,json=supervisorCallback,proto3" json:"supervisor_callback,omitempty"`
	unknownFields      protoimpl.UnknownFields
	sizeCache          protoimpl.SizeCache
}

func (x *MonitorActorRequest) Reset() {
	*x = MonitorActorRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[62]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *MonitorActorRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*MonitorActorRequest) ProtoMessage() {}

func (x *MonitorActorRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[62]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use MonitorActorRequest.ProtoReflect.Descriptor instead.
func (*MonitorActorRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{62}
}

func (x *MonitorActorRequest) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *MonitorActorRequest) GetSupervisorId() string {
	if x != nil {
		return x.SupervisorId
	}
	return ""
}

func (x *MonitorActorRequest) GetSupervisorCallback() string {
	if x != nil {
		return x.SupervisorCallback
	}
	return ""
}

// Response to MonitorActor request
//
// ## Purpose
// Returns a monitor reference that can be used to demonitor in future.
//
// ## Erlang Philosophy
// Equivalent to the Ref returned by erlang:monitor(process, Pid).
//
// ## Design Notes
// - monitor_ref: Unique ID for this monitoring link (ULID)
// - Can be used for future demonitor() operation
type MonitorActorResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	MonitorRef    string                 `protobuf:"bytes,1,opt,name=monitor_ref,json=monitorRef,proto3" json:"monitor_ref,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *MonitorActorResponse) Reset() {
	*x = MonitorActorResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[63]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *MonitorActorResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*MonitorActorResponse) ProtoMessage() {}

func (x *MonitorActorResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[63]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use MonitorActorResponse.ProtoReflect.Descriptor instead.
func (*MonitorActorResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{63}
}

func (x *MonitorActorResponse) GetMonitorRef() string {
	if x != nil {
		return x.MonitorRef
	}
	return ""
}

// Request to remove a monitor (Erlang demonitor/1 equivalent)
//
// ## Purpose
// Cancels a monitor previously established via MonitorActor on the node that hosts
// the **monitored** actor (`actor_id`). The caller must supply the same `monitor_ref`
// returned by MonitorActor.
type DemonitorActorRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	ActorId       string                 `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	SupervisorId  string                 `protobuf:"bytes,2,opt,name=supervisor_id,json=supervisorId,proto3" json:"supervisor_id,omitempty"`
	MonitorRef    string                 `protobuf:"bytes,3,opt,name=monitor_ref,json=monitorRef,proto3" json:"monitor_ref,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *DemonitorActorRequest) Reset() {
	*x = DemonitorActorRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[64]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *DemonitorActorRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*DemonitorActorRequest) ProtoMessage() {}

func (x *DemonitorActorRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[64]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use DemonitorActorRequest.ProtoReflect.Descriptor instead.
func (*DemonitorActorRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{64}
}

func (x *DemonitorActorRequest) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *DemonitorActorRequest) GetSupervisorId() string {
	if x != nil {
		return x.SupervisorId
	}
	return ""
}

func (x *DemonitorActorRequest) GetMonitorRef() string {
	if x != nil {
		return x.MonitorRef
	}
	return ""
}

// Notification that a monitored actor has terminated
//
// ## Purpose
// Sent by the node hosting the actor to the supervisor when actor terminates.
// This is an internal message, not typically sent by user code.
//
// ## Erlang Philosophy
// Equivalent to receiving: {'DOWN', Ref, process, Pid, Reason}
// Supervisor receives this asynchronously when monitored actor exits.
//
// ## Design Notes
// - actor_id: The actor that terminated
// - supervisor_id: The supervisor that was monitoring (for routing)
// - reason: Why actor terminated:
//   - "normal": Graceful shutdown
//   - "shutdown": Supervisor-initiated shutdown
//   - "killed": Forcefully terminated
//   - Error message: Crash reason (e.g., "panic: index out of bounds")
type ActorDownNotification struct {
	state        protoimpl.MessageState `protogen:"open.v1"`
	ActorId      string                 `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	SupervisorId string                 `protobuf:"bytes,2,opt,name=supervisor_id,json=supervisorId,proto3" json:"supervisor_id,omitempty"`
	Reason       string                 `protobuf:"bytes,3,opt,name=reason,proto3" json:"reason,omitempty"`
	// Monitor reference ULID correlating with the original monitor() call.
	// Allows the monitoring actor to identify which monitor fired when monitoring multiple actors.
	MonitorRef string `protobuf:"bytes,4,opt,name=monitor_ref,json=monitorRef,proto3" json:"monitor_ref,omitempty"`
	// When true, this notification is a Link EXIT signal (bidirectional death propagation).
	// The receiving node should kill the supervisor_id actor with a Linked exit reason.
	// When false (default), this is a Monitor DOWN notification delivered to the mailbox.
	IsLinkSignal  bool `protobuf:"varint,5,opt,name=is_link_signal,json=isLinkSignal,proto3" json:"is_link_signal,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorDownNotification) Reset() {
	*x = ActorDownNotification{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[65]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorDownNotification) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorDownNotification) ProtoMessage() {}

func (x *ActorDownNotification) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[65]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorDownNotification.ProtoReflect.Descriptor instead.
func (*ActorDownNotification) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{65}
}

func (x *ActorDownNotification) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *ActorDownNotification) GetSupervisorId() string {
	if x != nil {
		return x.SupervisorId
	}
	return ""
}

func (x *ActorDownNotification) GetReason() string {
	if x != nil {
		return x.Reason
	}
	return ""
}

func (x *ActorDownNotification) GetMonitorRef() string {
	if x != nil {
		return x.MonitorRef
	}
	return ""
}

func (x *ActorDownNotification) GetIsLinkSignal() bool {
	if x != nil {
		return x.IsLinkSignal
	}
	return false
}

// Request to batch-check actor states — used by stale-monitor GC task
//
// ## Purpose
// Efficiently checks the lifecycle state of multiple actors in a single RPC call.
// Used by the background monitor GC task to detect stale monitor entries for actors
// that no longer exist on their hosting node.
type GetActorStatesRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// List of canonical actor IDs to check
	ActorIds      []string `protobuf:"bytes,1,rep,name=actor_ids,json=actorIds,proto3" json:"actor_ids,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *GetActorStatesRequest) Reset() {
	*x = GetActorStatesRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[66]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *GetActorStatesRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*GetActorStatesRequest) ProtoMessage() {}

func (x *GetActorStatesRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[66]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use GetActorStatesRequest.ProtoReflect.Descriptor instead.
func (*GetActorStatesRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{66}
}

func (x *GetActorStatesRequest) GetActorIds() []string {
	if x != nil {
		return x.ActorIds
	}
	return nil
}

// Response for batch actor state check — uses existing ActorState enum
type GetActorStatesResponse struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Map of canonical actor_id -> ActorState (uses existing ActorState enum)
	// ACTOR_STATE_UNSPECIFIED / ACTOR_STATE_TERMINATED / not present = actor not found on this node
	States        map[string]ActorState `protobuf:"bytes,1,rep,name=states,proto3" json:"states,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"varint,2,opt,name=value,enum=plexspaces.actor.v1.ActorState"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *GetActorStatesResponse) Reset() {
	*x = GetActorStatesResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[67]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *GetActorStatesResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*GetActorStatesResponse) ProtoMessage() {}

func (x *GetActorStatesResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[67]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use GetActorStatesResponse.ProtoReflect.Descriptor instead.
func (*GetActorStatesResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{67}
}

func (x *GetActorStatesResponse) GetStates() map[string]ActorState {
	if x != nil {
		return x.States
	}
	return nil
}

// / Actor link for two-way death propagation
// /
// / ## Purpose
// / Represents a link between two actors. When one actor dies, the linked actor
// / automatically dies (cascading failure). This is the foundation for supervision trees.
// /
// / ## Erlang Philosophy
// / Equivalent to Erlang's `link/1` - creates a bidirectional link between processes.
// / If either process dies abnormally, the other dies too.
// /
// / ## Design Notes
// / - Links are bidirectional (if A links to B, B is linked to A)
// / - Links propagate death (if A dies, B dies; if B dies, A dies)
// / - Links are used internally by supervision (parent-child relationships)
// / - Links can also be created explicitly via API
// /
// / ## Example
// / ```rust
// / // Link two actors
// / node.link("actor-1", "actor-2").await?;
// /
// / // If actor-1 dies abnormally, actor-2 automatically dies
// / // If actor-2 dies abnormally, actor-1 automatically dies
// / ```
type ActorLink struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// / First actor in the link
	ActorId string `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	// / Second actor in the link (bidirectional)
	LinkedActorId string `protobuf:"bytes,2,opt,name=linked_actor_id,json=linkedActorId,proto3" json:"linked_actor_id,omitempty"`
	// / Link creation timestamp (for observability)
	CreatedAt *timestamppb.Timestamp `protobuf:"bytes,3,opt,name=created_at,json=createdAt,proto3" json:"created_at,omitempty"`
	// / Metadata (optional, for debugging)
	Metadata      map[string]string `protobuf:"bytes,4,rep,name=metadata,proto3" json:"metadata,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorLink) Reset() {
	*x = ActorLink{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[68]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorLink) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorLink) ProtoMessage() {}

func (x *ActorLink) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[68]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorLink.ProtoReflect.Descriptor instead.
func (*ActorLink) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{68}
}

func (x *ActorLink) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *ActorLink) GetLinkedActorId() string {
	if x != nil {
		return x.LinkedActorId
	}
	return ""
}

func (x *ActorLink) GetCreatedAt() *timestamppb.Timestamp {
	if x != nil {
		return x.CreatedAt
	}
	return nil
}

func (x *ActorLink) GetMetadata() map[string]string {
	if x != nil {
		return x.Metadata
	}
	return nil
}

// / Link two actors (Erlang link/1 equivalent)
// /
// / ## Purpose
// / Creates a bidirectional link between two actors. When one actor dies abnormally,
// / the linked actor automatically dies (cascading failure).
// /
// / ## Erlang Philosophy
// / Equivalent to Erlang's `link(Pid)` - creates bidirectional link.
// / If either process dies abnormally, the other dies too.
// /
// / ## Design Notes
// / - Links are bidirectional (if A links to B, B is linked to A)
// / - Links only propagate abnormal deaths (not "normal" shutdowns)
// / - Links are used internally by supervision (parent-child relationships)
// / - Links can be created explicitly via this API
// /
// / ## Example
// / ```rust
// / // Link two actors
// / node.link("actor-1", "actor-2").await?;
// /
// / // If actor-1 dies abnormally, actor-2 automatically dies
// / // If actor-2 dies abnormally, actor-1 automatically dies
// / ```
type LinkActorRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	ActorId       string                 `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	LinkedActorId string                 `protobuf:"bytes,2,opt,name=linked_actor_id,json=linkedActorId,proto3" json:"linked_actor_id,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *LinkActorRequest) Reset() {
	*x = LinkActorRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[69]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *LinkActorRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*LinkActorRequest) ProtoMessage() {}

func (x *LinkActorRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[69]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use LinkActorRequest.ProtoReflect.Descriptor instead.
func (*LinkActorRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{69}
}

func (x *LinkActorRequest) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *LinkActorRequest) GetLinkedActorId() string {
	if x != nil {
		return x.LinkedActorId
	}
	return ""
}

// / Response to LinkActor request
type LinkActorResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Success       bool                   `protobuf:"varint,1,opt,name=success,proto3" json:"success,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *LinkActorResponse) Reset() {
	*x = LinkActorResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[70]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *LinkActorResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*LinkActorResponse) ProtoMessage() {}

func (x *LinkActorResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[70]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use LinkActorResponse.ProtoReflect.Descriptor instead.
func (*LinkActorResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{70}
}

func (x *LinkActorResponse) GetSuccess() bool {
	if x != nil {
		return x.Success
	}
	return false
}

// / Unlink two actors (Erlang unlink/1 equivalent)
// /
// / ## Purpose
// / Removes the bidirectional link between two actors. After unlinking,
// / actors can die independently without cascading failures.
// /
// / ## Erlang Philosophy
// / Equivalent to Erlang's `unlink(Pid)` - removes bidirectional link.
type UnlinkActorRequest struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	ActorId       string                 `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	LinkedActorId string                 `protobuf:"bytes,2,opt,name=linked_actor_id,json=linkedActorId,proto3" json:"linked_actor_id,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *UnlinkActorRequest) Reset() {
	*x = UnlinkActorRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[71]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *UnlinkActorRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*UnlinkActorRequest) ProtoMessage() {}

func (x *UnlinkActorRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[71]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use UnlinkActorRequest.ProtoReflect.Descriptor instead.
func (*UnlinkActorRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{71}
}

func (x *UnlinkActorRequest) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *UnlinkActorRequest) GetLinkedActorId() string {
	if x != nil {
		return x.LinkedActorId
	}
	return ""
}

// / Response to UnlinkActor request
type UnlinkActorResponse struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Success       bool                   `protobuf:"varint,1,opt,name=success,proto3" json:"success,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *UnlinkActorResponse) Reset() {
	*x = UnlinkActorResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[72]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *UnlinkActorResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*UnlinkActorResponse) ProtoMessage() {}

func (x *UnlinkActorResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[72]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use UnlinkActorResponse.ProtoReflect.Descriptor instead.
func (*UnlinkActorResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{72}
}

func (x *UnlinkActorResponse) GetSuccess() bool {
	if x != nil {
		return x.Success
	}
	return false
}

// Request to check if actor exists
type CheckActorExistsRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Actor ID to check
	ActorId       string `protobuf:"bytes,1,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *CheckActorExistsRequest) Reset() {
	*x = CheckActorExistsRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[73]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *CheckActorExistsRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*CheckActorExistsRequest) ProtoMessage() {}

func (x *CheckActorExistsRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[73]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use CheckActorExistsRequest.ProtoReflect.Descriptor instead.
func (*CheckActorExistsRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{73}
}

func (x *CheckActorExistsRequest) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

// Response to check actor exists request
type CheckActorExistsResponse struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Actor exists (virtual or active)
	Exists bool `protobuf:"varint,1,opt,name=exists,proto3" json:"exists,omitempty"`
	// Actor is currently active (in memory)
	IsActive bool `protobuf:"varint,2,opt,name=is_active,json=isActive,proto3" json:"is_active,omitempty"`
	// Actor has VirtualActorFacet (is virtual)
	IsVirtual     bool `protobuf:"varint,3,opt,name=is_virtual,json=isVirtual,proto3" json:"is_virtual,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *CheckActorExistsResponse) Reset() {
	*x = CheckActorExistsResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[74]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *CheckActorExistsResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*CheckActorExistsResponse) ProtoMessage() {}

func (x *CheckActorExistsResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[74]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use CheckActorExistsResponse.ProtoReflect.Descriptor instead.
func (*CheckActorExistsResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{74}
}

func (x *CheckActorExistsResponse) GetExists() bool {
	if x != nil {
		return x.Exists
	}
	return false
}

func (x *CheckActorExistsResponse) GetIsActive() bool {
	if x != nil {
		return x.IsActive
	}
	return false
}

func (x *CheckActorExistsResponse) GetIsVirtual() bool {
	if x != nil {
		return x.IsVirtual
	}
	return false
}

// Request to ask an actor via HTTP-like interface (FaaS-style)
//
// ## Purpose
// Enables ask-style requests to actors via HTTP GET/POST/PUT routes.
// The tenant_id, namespace, and actor_type are extracted from the HTTP path.
//
// ## HTTP Method Handling
// - GET: Query parameters are converted to JSON and stored in payload as string
// - POST: Request body becomes payload, HTTP headers become headers map
type AskReplyRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Namespace (extracted from path: /api/v1/actors/{namespace}/{actor_type})
	// Can be empty - defaults to empty string if not provided
	// Tenant ID comes from gRPC auth (JWT middleware) or default config, not from request
	Namespace string `protobuf:"bytes,1,opt,name=namespace,proto3" json:"namespace,omitempty"`
	// Actor type (extracted from path: /api/v1/actors/{namespace}/{actor_type})
	// Used to lookup actors via ActorRegistry discover_actors_by_type
	ActorType string `protobuf:"bytes,2,opt,name=actor_type,json=actorType,proto3" json:"actor_type,omitempty"`
	// HTTP method metadata (GET, POST, or PUT)
	HttpMethod string `protobuf:"bytes,3,opt,name=http_method,json=httpMethod,proto3" json:"http_method,omitempty"`
	// Request payload
	// For GET: JSON string of query parameters
	// For POST/PUT: Request body bytes
	Payload []byte `protobuf:"bytes,4,opt,name=payload,proto3" json:"payload,omitempty"`
	// HTTP headers
	// Converted from HTTP request headers
	Headers map[string]string `protobuf:"bytes,5,rep,name=headers,proto3" json:"headers,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Query parameters (for GET requests)
	// Converted to JSON and stored in payload
	QueryParams map[string]string `protobuf:"bytes,6,rep,name=query_params,json=queryParams,proto3" json:"query_params,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Full HTTP path for the request (optional)
	// Example: "/api/v1/actors/default/counter/custom/path"
	// Allows actors to perform custom routing based on the complete URL
	Path string `protobuf:"bytes,7,opt,name=path,proto3" json:"path,omitempty"`
	// Subpath after the actor_type segment (optional)
	// Example: for "/api/v1/actors/default/counter/metrics/latest"
	//
	//	subpath = "metrics/latest"
	//
	// This will be used in future for advanced per-actor routing capabilities.
	Subpath string `protobuf:"bytes,8,opt,name=subpath,proto3" json:"subpath,omitempty"`
	// Optional sender actor id for remoting and tracing.
	SenderId string `protobuf:"bytes,9,opt,name=sender_id,json=senderId,proto3" json:"sender_id,omitempty"`
	// Optional transport message type.
	MessageType string `protobuf:"bytes,10,opt,name=message_type,json=messageType,proto3" json:"message_type,omitempty"`
	// Optional correlation id preserved for remoting.
	CorrelationId string `protobuf:"bytes,11,opt,name=correlation_id,json=correlationId,proto3" json:"correlation_id,omitempty"`
	// Optional reply_to preserved for remoting.
	ReplyTo string `protobuf:"bytes,12,opt,name=reply_to,json=replyTo,proto3" json:"reply_to,omitempty"`
	// Optional client-provided message id.
	MessageId string `protobuf:"bytes,13,opt,name=message_id,json=messageId,proto3" json:"message_id,omitempty"`
	// Optional timeout for request-reply (ask) operations.
	// Defaults to 5 seconds if not specified. Use for long-running operations like training.
	// HTTP gateway extracts from ?timeout=30 query parameter (in seconds).
	Timeout *durationpb.Duration `protobuf:"bytes,14,opt,name=timeout,proto3" json:"timeout,omitempty"`
	// Optional actor instance name. When set together with actor_type and namespace,
	// the handler constructs the canonical actor ID directly:
	//
	//	actor_name//actor_type::namespace@node_id
	//
	// If empty, falls back to registry lookup by actor_type within the namespace.
	ActorName     string `protobuf:"bytes,20,opt,name=actor_name,json=actorName,proto3" json:"actor_name,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *AskReplyRequest) Reset() {
	*x = AskReplyRequest{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[75]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *AskReplyRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*AskReplyRequest) ProtoMessage() {}

func (x *AskReplyRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[75]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use AskReplyRequest.ProtoReflect.Descriptor instead.
func (*AskReplyRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{75}
}

func (x *AskReplyRequest) GetNamespace() string {
	if x != nil {
		return x.Namespace
	}
	return ""
}

func (x *AskReplyRequest) GetActorType() string {
	if x != nil {
		return x.ActorType
	}
	return ""
}

func (x *AskReplyRequest) GetHttpMethod() string {
	if x != nil {
		return x.HttpMethod
	}
	return ""
}

func (x *AskReplyRequest) GetPayload() []byte {
	if x != nil {
		return x.Payload
	}
	return nil
}

func (x *AskReplyRequest) GetHeaders() map[string]string {
	if x != nil {
		return x.Headers
	}
	return nil
}

func (x *AskReplyRequest) GetQueryParams() map[string]string {
	if x != nil {
		return x.QueryParams
	}
	return nil
}

func (x *AskReplyRequest) GetPath() string {
	if x != nil {
		return x.Path
	}
	return ""
}

func (x *AskReplyRequest) GetSubpath() string {
	if x != nil {
		return x.Subpath
	}
	return ""
}

func (x *AskReplyRequest) GetSenderId() string {
	if x != nil {
		return x.SenderId
	}
	return ""
}

func (x *AskReplyRequest) GetMessageType() string {
	if x != nil {
		return x.MessageType
	}
	return ""
}

func (x *AskReplyRequest) GetCorrelationId() string {
	if x != nil {
		return x.CorrelationId
	}
	return ""
}

func (x *AskReplyRequest) GetReplyTo() string {
	if x != nil {
		return x.ReplyTo
	}
	return ""
}

func (x *AskReplyRequest) GetMessageId() string {
	if x != nil {
		return x.MessageId
	}
	return ""
}

func (x *AskReplyRequest) GetTimeout() *durationpb.Duration {
	if x != nil {
		return x.Timeout
	}
	return nil
}

func (x *AskReplyRequest) GetActorName() string {
	if x != nil {
		return x.ActorName
	}
	return ""
}

// Response from asking an actor
//
// ## Purpose
// Returns the result of an actor ask request.
type AskReplyResponse struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Success status
	Success bool `protobuf:"varint,1,opt,name=success,proto3" json:"success,omitempty"`
	// Response payload (for GET/ask requests)
	// Contains the reply message from actor
	Payload []byte `protobuf:"bytes,2,opt,name=payload,proto3" json:"payload,omitempty"`
	// Response headers (optional metadata)
	Headers map[string]string `protobuf:"bytes,3,rep,name=headers,proto3" json:"headers,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Actor ID that was invoked (format: "actor_id@node_id")
	ActorId string `protobuf:"bytes,4,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	// Error message (if success is false)
	ErrorMessage  string `protobuf:"bytes,5,opt,name=error_message,json=errorMessage,proto3" json:"error_message,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *AskReplyResponse) Reset() {
	*x = AskReplyResponse{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[76]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *AskReplyResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*AskReplyResponse) ProtoMessage() {}

func (x *AskReplyResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[76]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use AskReplyResponse.ProtoReflect.Descriptor instead.
func (*AskReplyResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{76}
}

func (x *AskReplyResponse) GetSuccess() bool {
	if x != nil {
		return x.Success
	}
	return false
}

func (x *AskReplyResponse) GetPayload() []byte {
	if x != nil {
		return x.Payload
	}
	return nil
}

func (x *AskReplyResponse) GetHeaders() map[string]string {
	if x != nil {
		return x.Headers
	}
	return nil
}

func (x *AskReplyResponse) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *AskReplyResponse) GetErrorMessage() string {
	if x != nil {
		return x.ErrorMessage
	}
	return ""
}

// Lifecycle event filter for subscribers
//
// ## Purpose
// Defines what events a subscriber wants to receive. Supports filtering by:
// - Event types (Created, Terminated, Failed, etc.)
// - Actor ID patterns (regex)
// - Node ID patterns (regex)
// - Custom tags
//
// ## Examples
// ```
// // Prometheus exporter - only care about actor spawn/terminate for counts
//
//	EventFilter {
//	  event_types: [ACTOR_CREATED, ACTOR_TERMINATED, ACTOR_FAILED]
//	}
//
// // Tracing system - want all events for specific actor group
//
//	EventFilter {
//	  actor_id_pattern: "worker-.*@node1"
//	  event_types: [ACTOR_ACTIVATED, ACTOR_DEACTIVATED]
//	}
//
// // Monitoring dashboard - all critical events across cluster
//
//	EventFilter {
//	  event_types: [ACTOR_FAILED, ACTOR_MIGRATING]
//	}
//
// ```
type LifecycleEventFilter struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Subscription ID (ULID, generated by subscriber)
	// Used for unsubscribe and tracking
	SubscriptionId string `protobuf:"bytes,1,opt,name=subscription_id,json=subscriptionId,proto3" json:"subscription_id,omitempty"`
	// Event types to receive (empty = all types)
	// Maps to ActorLifecycleEvent.event_type oneof
	EventTypes []LifecycleEventType `protobuf:"varint,2,rep,packed,name=event_types,json=eventTypes,proto3,enum=plexspaces.actor.v1.LifecycleEventType" json:"event_types,omitempty"`
	// Actor ID pattern (regex, empty = all actors)
	// Example: "worker-.*@node1" matches all workers on node1
	ActorIdPattern string `protobuf:"bytes,3,opt,name=actor_id_pattern,json=actorIdPattern,proto3" json:"actor_id_pattern,omitempty"`
	// Node ID pattern (regex, empty = all nodes)
	// Example: "prod-.*" matches all production nodes
	NodeIdPattern string `protobuf:"bytes,4,opt,name=node_id_pattern,json=nodeIdPattern,proto3" json:"node_id_pattern,omitempty"`
	// Custom tags filter (AND logic - event must have all tags)
	// Example: {"env": "production", "team": "platform"}
	RequiredTags map[string]string `protobuf:"bytes,5,rep,name=required_tags,json=requiredTags,proto3" json:"required_tags,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Buffer size for slow subscriber (default: 1000 events)
	// When buffer full, drop_policy determines behavior
	BufferSize uint32 `protobuf:"varint,6,opt,name=buffer_size,json=bufferSize,proto3" json:"buffer_size,omitempty"` // Max 100K events
	// Drop policy when buffer full
	DropPolicy    DropPolicy `protobuf:"varint,7,opt,name=drop_policy,json=dropPolicy,proto3,enum=plexspaces.actor.v1.DropPolicy" json:"drop_policy,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *LifecycleEventFilter) Reset() {
	*x = LifecycleEventFilter{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[77]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *LifecycleEventFilter) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*LifecycleEventFilter) ProtoMessage() {}

func (x *LifecycleEventFilter) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[77]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use LifecycleEventFilter.ProtoReflect.Descriptor instead.
func (*LifecycleEventFilter) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{77}
}

func (x *LifecycleEventFilter) GetSubscriptionId() string {
	if x != nil {
		return x.SubscriptionId
	}
	return ""
}

func (x *LifecycleEventFilter) GetEventTypes() []LifecycleEventType {
	if x != nil {
		return x.EventTypes
	}
	return nil
}

func (x *LifecycleEventFilter) GetActorIdPattern() string {
	if x != nil {
		return x.ActorIdPattern
	}
	return ""
}

func (x *LifecycleEventFilter) GetNodeIdPattern() string {
	if x != nil {
		return x.NodeIdPattern
	}
	return ""
}

func (x *LifecycleEventFilter) GetRequiredTags() map[string]string {
	if x != nil {
		return x.RequiredTags
	}
	return nil
}

func (x *LifecycleEventFilter) GetBufferSize() uint32 {
	if x != nil {
		return x.BufferSize
	}
	return 0
}

func (x *LifecycleEventFilter) GetDropPolicy() DropPolicy {
	if x != nil {
		return x.DropPolicy
	}
	return DropPolicy_DROP_POLICY_UNSPECIFIED
}

// Virtual Actor Lifecycle (Orleans-inspired)
//
// ## Purpose
// Tracks activation/deactivation state for virtual actors (actors that exist virtually,
// activated on-demand). Virtual actors are always addressable but not always in memory.
//
// ## When Used
// Only present when actor has VirtualActorFacet attached (opt-in pattern).
// Regular actors (explicit creation) don't have this lifecycle tracking.
//
// ## Design Decision
// Virtual actor lifecycle is tracked separately from core ActorState to maintain
// simplicity: core actors are explicit, virtual actors are opt-in via facet.
//
// ## Example
// ```protobuf
//
//	Actor {
//	  actor_id: "user-123"
//	  state: ACTOR_STATE_INACTIVE  // Virtual actor, not in memory
//	  facets: [
//	    Facet {
//	      type: "virtual_actor"
//	      config: {
//	        "idle_timeout": "5m",
//	        "activation_strategy": "lazy"
//	      }
//	    }
//	  ]
//	}
//
// ```
// Virtual Actor Lifecycle Metadata
//
// ## Purpose
// Provides metadata about virtual actor activation/deactivation (timestamps, counts, etc.).
// This is METADATA only - the actual lifecycle state is tracked in Actor.state (ActorState enum).
//
// ## Design Decision
// All actors (virtual or not) use the same ActorState enum for state consistency.
// VirtualActorLifecycle provides additional metadata useful for virtual actors:
// - Activation timestamps (for monitoring/debugging)
// - Activation counts (for metrics)
// - Idle timeout configuration
// - Pending message counts
//
// ## Usage
// - Actor.state: Source of truth for lifecycle state (CREATING, ACTIVE, INACTIVE, etc.)
// - VirtualActorLifecycle: Optional metadata for virtual actors only
type VirtualActorLifecycle struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Last time actor was activated (loaded into memory)
	LastActivated *timestamppb.Timestamp `protobuf:"bytes,1,opt,name=last_activated,json=lastActivated,proto3" json:"last_activated,omitempty"`
	// Last time actor received a message or performed an operation
	LastAccessed *timestamppb.Timestamp `protobuf:"bytes,2,opt,name=last_accessed,json=lastAccessed,proto3" json:"last_accessed,omitempty"`
	// Idle timeout before deactivation (from facet config)
	IdleTimeout *durationpb.Duration `protobuf:"bytes,3,opt,name=idle_timeout,json=idleTimeout,proto3" json:"idle_timeout,omitempty"`
	// Number of times this actor has been activated
	ActivationCount uint32 `protobuf:"varint,4,opt,name=activation_count,json=activationCount,proto3" json:"activation_count,omitempty"`
	// Is currently activating (prevents duplicate activations)
	IsActivating bool `protobuf:"varint,5,opt,name=is_activating,json=isActivating,proto3" json:"is_activating,omitempty"`
	// Messages queued during activation (processed after activation completes)
	// Note: This is a simplified representation - actual queue is in Node
	PendingMessageCount uint32 `protobuf:"varint,6,opt,name=pending_message_count,json=pendingMessageCount,proto3" json:"pending_message_count,omitempty"`
	unknownFields       protoimpl.UnknownFields
	sizeCache           protoimpl.SizeCache
}

func (x *VirtualActorLifecycle) Reset() {
	*x = VirtualActorLifecycle{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[78]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *VirtualActorLifecycle) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*VirtualActorLifecycle) ProtoMessage() {}

func (x *VirtualActorLifecycle) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[78]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use VirtualActorLifecycle.ProtoReflect.Descriptor instead.
func (*VirtualActorLifecycle) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{78}
}

func (x *VirtualActorLifecycle) GetLastActivated() *timestamppb.Timestamp {
	if x != nil {
		return x.LastActivated
	}
	return nil
}

func (x *VirtualActorLifecycle) GetLastAccessed() *timestamppb.Timestamp {
	if x != nil {
		return x.LastAccessed
	}
	return nil
}

func (x *VirtualActorLifecycle) GetIdleTimeout() *durationpb.Duration {
	if x != nil {
		return x.IdleTimeout
	}
	return nil
}

func (x *VirtualActorLifecycle) GetActivationCount() uint32 {
	if x != nil {
		return x.ActivationCount
	}
	return 0
}

func (x *VirtualActorLifecycle) GetIsActivating() bool {
	if x != nil {
		return x.IsActivating
	}
	return false
}

func (x *VirtualActorLifecycle) GetPendingMessageCount() uint32 {
	if x != nil {
		return x.PendingMessageCount
	}
	return 0
}

// Virtual Actor Configuration (for VirtualActorFacet)
//
// ## Purpose
// Configuration for virtual actor behavior (activation strategy, idle timeout, etc.)
//
// ## Usage
// This config is stored in the VirtualActorFacet's config map:
// ```protobuf
//
//	Facet {
//	  type: "virtual_actor"
//	  config: {
//	    "idle_timeout": "5m",
//	    "activation_strategy": "lazy"
//	  }
//	}
//
// ```
//
// ## Design Decision
// Config is stored as string map in Facet (simplicity, flexibility).
// This message is for documentation and type safety in Rust code.
type VirtualActorConfig struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Activation strategy (from common.proto)
	ActivationStrategy v1.ActivationStrategy `protobuf:"varint,1,opt,name=activation_strategy,json=activationStrategy,proto3,enum=plexspaces.common.v1.ActivationStrategy" json:"activation_strategy,omitempty"`
	// Idle timeout before deactivation
	IdleTimeout *durationpb.Duration `protobuf:"bytes,2,opt,name=idle_timeout,json=idleTimeout,proto3" json:"idle_timeout,omitempty"`
	// Should actor persist state on deactivation?
	PersistOnDeactivation bool `protobuf:"varint,3,opt,name=persist_on_deactivation,json=persistOnDeactivation,proto3" json:"persist_on_deactivation,omitempty"`
	unknownFields         protoimpl.UnknownFields
	sizeCache             protoimpl.SizeCache
}

func (x *VirtualActorConfig) Reset() {
	*x = VirtualActorConfig{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[79]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *VirtualActorConfig) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*VirtualActorConfig) ProtoMessage() {}

func (x *VirtualActorConfig) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[79]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use VirtualActorConfig.ProtoReflect.Descriptor instead.
func (*VirtualActorConfig) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{79}
}

func (x *VirtualActorConfig) GetActivationStrategy() v1.ActivationStrategy {
	if x != nil {
		return x.ActivationStrategy
	}
	return v1.ActivationStrategy(0)
}

func (x *VirtualActorConfig) GetIdleTimeout() *durationpb.Duration {
	if x != nil {
		return x.IdleTimeout
	}
	return nil
}

func (x *VirtualActorConfig) GetPersistOnDeactivation() bool {
	if x != nil {
		return x.PersistOnDeactivation
	}
	return false
}

// ActorRef error message
type ActorRefError struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Code          ActorRefErrorCode      `protobuf:"varint,1,opt,name=code,proto3,enum=plexspaces.actor.v1.ActorRefErrorCode" json:"code,omitempty"`
	Message       string                 `protobuf:"bytes,2,opt,name=message,proto3" json:"message,omitempty"`
	ActorId       string                 `protobuf:"bytes,3,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"` // Optional: actor ID that caused error
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorRefError) Reset() {
	*x = ActorRefError{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[80]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorRefError) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorRefError) ProtoMessage() {}

func (x *ActorRefError) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[80]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorRefError.ProtoReflect.Descriptor instead.
func (*ActorRefError) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{80}
}

func (x *ActorRefError) GetCode() ActorRefErrorCode {
	if x != nil {
		return x.Code
	}
	return ActorRefErrorCode_ACTOR_REF_ERROR_CODE_UNSPECIFIED
}

func (x *ActorRefError) GetMessage() string {
	if x != nil {
		return x.Message
	}
	return ""
}

func (x *ActorRefError) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

// Resource contract for an actor
// Declares resource requirements and limits upfront
type ResourceContract struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Maximum CPU usage as percentage (0.0 - 100.0)
	MaxCpuPercent float32 `protobuf:"fixed32,1,opt,name=max_cpu_percent,json=maxCpuPercent,proto3" json:"max_cpu_percent,omitempty"`
	// Maximum memory usage in bytes
	MaxMemoryBytes uint64 `protobuf:"varint,2,opt,name=max_memory_bytes,json=maxMemoryBytes,proto3" json:"max_memory_bytes,omitempty"`
	// Maximum I/O operations per second
	MaxIoOpsPerSec *uint32 `protobuf:"varint,3,opt,name=max_io_ops_per_sec,json=maxIoOpsPerSec,proto3,oneof" json:"max_io_ops_per_sec,omitempty"`
	// Guaranteed network bandwidth in Mbps
	GuaranteedBandwidthMbps *uint32 `protobuf:"varint,4,opt,name=guaranteed_bandwidth_mbps,json=guaranteedBandwidthMbps,proto3,oneof" json:"guaranteed_bandwidth_mbps,omitempty"`
	// Maximum execution time per message
	MaxExecutionTime *durationpb.Duration `protobuf:"bytes,5,opt,name=max_execution_time,json=maxExecutionTime,proto3,oneof" json:"max_execution_time,omitempty"`
	unknownFields    protoimpl.UnknownFields
	sizeCache        protoimpl.SizeCache
}

func (x *ResourceContract) Reset() {
	*x = ResourceContract{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[81]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ResourceContract) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ResourceContract) ProtoMessage() {}

func (x *ResourceContract) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[81]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ResourceContract.ProtoReflect.Descriptor instead.
func (*ResourceContract) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{81}
}

func (x *ResourceContract) GetMaxCpuPercent() float32 {
	if x != nil {
		return x.MaxCpuPercent
	}
	return 0
}

func (x *ResourceContract) GetMaxMemoryBytes() uint64 {
	if x != nil {
		return x.MaxMemoryBytes
	}
	return 0
}

func (x *ResourceContract) GetMaxIoOpsPerSec() uint32 {
	if x != nil && x.MaxIoOpsPerSec != nil {
		return *x.MaxIoOpsPerSec
	}
	return 0
}

func (x *ResourceContract) GetGuaranteedBandwidthMbps() uint32 {
	if x != nil && x.GuaranteedBandwidthMbps != nil {
		return *x.GuaranteedBandwidthMbps
	}
	return 0
}

func (x *ResourceContract) GetMaxExecutionTime() *durationpb.Duration {
	if x != nil {
		return x.MaxExecutionTime
	}
	return nil
}

// Current resource usage of an actor
type ResourceUsage struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Current CPU usage percentage
	CpuPercent float32 `protobuf:"fixed32,1,opt,name=cpu_percent,json=cpuPercent,proto3" json:"cpu_percent,omitempty"`
	// Current memory usage in bytes
	MemoryBytes uint64 `protobuf:"varint,2,opt,name=memory_bytes,json=memoryBytes,proto3" json:"memory_bytes,omitempty"`
	// Current I/O operations per second
	IoOpsPerSec uint32 `protobuf:"varint,3,opt,name=io_ops_per_sec,json=ioOpsPerSec,proto3" json:"io_ops_per_sec,omitempty"`
	// Current network bandwidth in Mbps
	NetworkMbps   uint32 `protobuf:"varint,4,opt,name=network_mbps,json=networkMbps,proto3" json:"network_mbps,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ResourceUsage) Reset() {
	*x = ResourceUsage{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[82]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ResourceUsage) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ResourceUsage) ProtoMessage() {}

func (x *ResourceUsage) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[82]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ResourceUsage.ProtoReflect.Descriptor instead.
func (*ResourceUsage) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{82}
}

func (x *ResourceUsage) GetCpuPercent() float32 {
	if x != nil {
		return x.CpuPercent
	}
	return 0
}

func (x *ResourceUsage) GetMemoryBytes() uint64 {
	if x != nil {
		return x.MemoryBytes
	}
	return 0
}

func (x *ResourceUsage) GetIoOpsPerSec() uint32 {
	if x != nil {
		return x.IoOpsPerSec
	}
	return 0
}

func (x *ResourceUsage) GetNetworkMbps() uint32 {
	if x != nil {
		return x.NetworkMbps
	}
	return 0
}

// Resource violation error message
type ResourceViolation struct {
	state   protoimpl.MessageState `protogen:"open.v1"`
	Code    ResourceViolationCode  `protobuf:"varint,1,opt,name=code,proto3,enum=plexspaces.actor.v1.ResourceViolationCode" json:"code,omitempty"`
	Message string                 `protobuf:"bytes,2,opt,name=message,proto3" json:"message,omitempty"`
	// Allowed value (e.g., max_cpu_percent)
	AllowedValue *float32 `protobuf:"fixed32,3,opt,name=allowed_value,json=allowedValue,proto3,oneof" json:"allowed_value,omitempty"`
	// Actual value (e.g., current cpu_percent)
	ActualValue   *float32 `protobuf:"fixed32,4,opt,name=actual_value,json=actualValue,proto3,oneof" json:"actual_value,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ResourceViolation) Reset() {
	*x = ResourceViolation{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[83]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ResourceViolation) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ResourceViolation) ProtoMessage() {}

func (x *ResourceViolation) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[83]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ResourceViolation.ProtoReflect.Descriptor instead.
func (*ResourceViolation) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{83}
}

func (x *ResourceViolation) GetCode() ResourceViolationCode {
	if x != nil {
		return x.Code
	}
	return ResourceViolationCode_RESOURCE_VIOLATION_CODE_UNSPECIFIED
}

func (x *ResourceViolation) GetMessage() string {
	if x != nil {
		return x.Message
	}
	return ""
}

func (x *ResourceViolation) GetAllowedValue() float32 {
	if x != nil && x.AllowedValue != nil {
		return *x.AllowedValue
	}
	return 0
}

func (x *ResourceViolation) GetActualValue() float32 {
	if x != nil && x.ActualValue != nil {
		return *x.ActualValue
	}
	return 0
}

// Actor health message
type ActorHealth struct {
	state  protoimpl.MessageState `protogen:"open.v1"`
	Status ActorHealthStatus      `protobuf:"varint,1,opt,name=status,proto3,enum=plexspaces.actor.v1.ActorHealthStatus" json:"status,omitempty"`
	// For STUCK status: how long actor has been stuck
	StuckSince *durationpb.Duration `protobuf:"bytes,2,opt,name=stuck_since,json=stuckSince,proto3,oneof" json:"stuck_since,omitempty"`
	// For FAILED status: reason for failure
	FailureReason *string `protobuf:"bytes,3,opt,name=failure_reason,json=failureReason,proto3,oneof" json:"failure_reason,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorHealth) Reset() {
	*x = ActorHealth{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[84]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorHealth) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorHealth) ProtoMessage() {}

func (x *ActorHealth) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[84]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorHealth.ProtoReflect.Descriptor instead.
func (*ActorHealth) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{84}
}

func (x *ActorHealth) GetStatus() ActorHealthStatus {
	if x != nil {
		return x.Status
	}
	return ActorHealthStatus_ACTOR_HEALTH_STATUS_UNSPECIFIED
}

func (x *ActorHealth) GetStuckSince() *durationpb.Duration {
	if x != nil {
		return x.StuckSince
	}
	return nil
}

func (x *ActorHealth) GetFailureReason() string {
	if x != nil && x.FailureReason != nil {
		return *x.FailureReason
	}
	return ""
}

// Complete specification for spawning or reactivating any actor.
//
// ## Purpose
// Single source of truth for all information needed to spawn an actor, whether
// from TOML app-config, SDK annotations, gRPC, or virtual actor reactivation.
// Replaces the fragmented triple of (init_config_template, initial_state, labels)
// and the VirtualActorDefinitionRegistration intermediary.
//
// ## Design
// - node_id is NOT included: ActorFactory always resolves local_node_id at spawn time.
// - tenant_id is overridden from JWT at request time if available.
// - args map is the canonical user-supplied init payload: becomes "args" key in WASM init().
// - facets carry the full facet declaration (type, config, priority) verbatim from ChildSpec.
//
// ## Usage
// - TOML ChildSpec → ActorSpawnSpec via actor_spawn_spec_from_child_spec()
// - SDK annotations → ActorSpawnSpec built from behavior + declared facets
// - VirtualActorMetadata stores ActorSpawnSpec as its spec field
// - ActorBuilder.from_spec() accepts ActorSpawnSpec
// - ActorFactory.spawn_actor() accepts ActorSpawnSpec
type ActorSpawnSpec struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Instance name + behavior class (namespace and tenant added in fields below).
	Identity *v1.ActorIdentity `protobuf:"bytes,1,opt,name=identity,proto3" json:"identity,omitempty"`
	// Role of the actor within its application (e.g. "worker", "leader").
	// Maps 1:1 to ChildSpec.role (TOML `role` field in [[supervisor.children]]).
	// Used by BehaviorRegistry to dispatch the correct spec when multiple children
	// share the same actor_type (behavior class).
	Role string `protobuf:"bytes,2,opt,name=role,proto3" json:"role,omitempty"`
	// Namespace for actor isolation (required at spawn time).
	Namespace string `protobuf:"bytes,3,opt,name=namespace,proto3" json:"namespace,omitempty"`
	// Tenant ID for multi-tenancy isolation (empty if auth disabled).
	// Overridden from JWT claims at request time when auth is enabled.
	TenantId string `protobuf:"bytes,4,opt,name=tenant_id,json=tenantId,proto3" json:"tenant_id,omitempty"`
	// Tell/ask isolation for cross-tenant/cross-namespace callers (see ActorVisibility).
	// UNSPECIFIED is treated as PUBLIC.
	Visibility ActorVisibility `protobuf:"varint,5,opt,name=visibility,proto3,enum=plexspaces.actor.v1.ActorVisibility" json:"visibility,omitempty"`
	// OTP-style behavior kind for logging and observability.
	// Examples: "GenServer", "GenEvent", "GenStateMachine", "Workflow".
	BehaviorKind string `protobuf:"bytes,6,opt,name=behavior_kind,json=behaviorKind,proto3" json:"behavior_kind,omitempty"`
	// User-supplied initialization arguments.
	// These become the "args" field in the WASM init() payload so TypeScript/Python/Go
	// actors can read them via host.config("initial_count"), etc.
	// Also used by Rust embedded actors as configuration.
	Args map[string]string `protobuf:"bytes,7,rep,name=args,proto3" json:"args,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Facet declarations attached to this actor.
	// Carries virtual_actor, durability, timer, etc. facets verbatim from ChildSpec.
	// ActorFactory instantiates these at spawn time via create_facets_from_config().
	Facets []*v1.Facet `protobuf:"bytes,8,rep,name=facets,proto3" json:"facets,omitempty"`
	// Observability labels propagated to metrics and traces.
	Labels map[string]string `protobuf:"bytes,9,rep,name=labels,proto3" json:"labels,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Actor runtime configuration (mailbox, restart policy, etc.).
	// Optional — defaults apply when absent.
	Config        *ActorConfig `protobuf:"bytes,10,opt,name=config,proto3" json:"config,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorSpawnSpec) Reset() {
	*x = ActorSpawnSpec{}
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[85]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorSpawnSpec) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorSpawnSpec) ProtoMessage() {}

func (x *ActorSpawnSpec) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[85]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorSpawnSpec.ProtoReflect.Descriptor instead.
func (*ActorSpawnSpec) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP(), []int{85}
}

func (x *ActorSpawnSpec) GetIdentity() *v1.ActorIdentity {
	if x != nil {
		return x.Identity
	}
	return nil
}

func (x *ActorSpawnSpec) GetRole() string {
	if x != nil {
		return x.Role
	}
	return ""
}

func (x *ActorSpawnSpec) GetNamespace() string {
	if x != nil {
		return x.Namespace
	}
	return ""
}

func (x *ActorSpawnSpec) GetTenantId() string {
	if x != nil {
		return x.TenantId
	}
	return ""
}

func (x *ActorSpawnSpec) GetVisibility() ActorVisibility {
	if x != nil {
		return x.Visibility
	}
	return ActorVisibility_ACTOR_VISIBILITY_UNSPECIFIED
}

func (x *ActorSpawnSpec) GetBehaviorKind() string {
	if x != nil {
		return x.BehaviorKind
	}
	return ""
}

func (x *ActorSpawnSpec) GetArgs() map[string]string {
	if x != nil {
		return x.Args
	}
	return nil
}

func (x *ActorSpawnSpec) GetFacets() []*v1.Facet {
	if x != nil {
		return x.Facets
	}
	return nil
}

func (x *ActorSpawnSpec) GetLabels() map[string]string {
	if x != nil {
		return x.Labels
	}
	return nil
}

func (x *ActorSpawnSpec) GetConfig() *ActorConfig {
	if x != nil {
		return x.Config
	}
	return nil
}

var File_plexspaces_v1_actors_actor_runtime_proto protoreflect.FileDescriptor

const file_plexspaces_v1_actors_actor_runtime_proto_rawDesc = "" +
	"\n" +
	"(plexspaces/v1/actors/actor_runtime.proto\x12\x13plexspaces.actor.v1\x1a\x1bbuf/validate/validate.proto\x1a\x1cgoogle/api/annotations.proto\x1a\x1fgoogle/api/field_behavior.proto\x1a\x19google/protobuf/any.proto\x1a\x1egoogle/protobuf/duration.proto\x1a\x1cgoogle/protobuf/struct.proto\x1a\x1fgoogle/protobuf/timestamp.proto\x1a plexspaces/v1/actors/types.proto\x1a\x1aplexspaces/v1/common.proto\x1a+plexspaces/v1/supervision/supervision.proto\x1a.protoc-gen-openapiv2/options/annotations.proto\"\x8a\x06\n" +
	"\x05Actor\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId\x12@\n" +
	"\n" +
	"actor_type\x18\x02 \x01(\tB!\xe0A\x02\xbaH\x1br\x19\x10\x01\x18\x80\x012\x12^[a-z][a-z0-9_-]*$R\tactorType\x12:\n" +
	"\x05state\x18\x03 \x01(\x0e2\x1f.plexspaces.actor.v1.ActorStateB\x03\xe0A\x03R\x05state\x12$\n" +
	"\anode_id\x18\x04 \x01(\tB\v\xe0A\x03\xbaH\x05r\x03\x18\xff\x01R\x06nodeId\x12 \n" +
	"\x05vm_id\x18\x05 \x01(\tB\v\xe0A\x03\xbaH\x05r\x03\x18\xff\x01R\x04vmId\x12$\n" +
	"\vactor_state\x18\x06 \x01(\fB\x03\xe0A\x03R\n" +
	"actorState\x12:\n" +
	"\bmetadata\x18\a \x01(\v2\x1e.plexspaces.common.v1.MetadataR\bmetadata\x128\n" +
	"\x06config\x18\b \x01(\v2 .plexspaces.actor.v1.ActorConfigR\x06config\x12@\n" +
	"\ametrics\x18\t \x01(\v2!.plexspaces.actor.v1.ActorMetricsB\x03\xe0A\x03R\ametrics\x123\n" +
	"\x06facets\x18\n" +
	" \x03(\v2\x1b.plexspaces.common.v1.FacetR\x06facets\x12;\n" +
	"\x1aactor_state_schema_version\x18\f \x01(\rR\x17actorStateSchemaVersion\x12(\n" +
	"\rerror_message\x18\r \x01(\tB\x03\xe0A\x03R\ferrorMessage\x12\x1c\n" +
	"\tnamespace\x18\x0e \x01(\tR\tnamespace\x12;\n" +
	"\x04name\x18\x0f \x01(\tB'\xbaH$r\"\x10\x01\x18\x80\x012\x1b^[a-zA-Z0-9][a-zA-Z0-9_-]*$R\x04name:<\x92A9\n" +
	"7*\x05Actor2\x16Durable actor instance\xd2\x01\bactor_id\xd2\x01\n" +
	"actor_type\"\xe5\t\n" +
	"\vActorConfig\x12O\n" +
	"\x0fmailbox_timeout\x18\x01 \x01(\v2\x19.google.protobuf.DurationB\v\xbaH\b\xaa\x01\x05\"\x03\b\x90\x1cR\x0emailboxTimeout\x125\n" +
	"\x10max_mailbox_size\x18\x02 \x01(\rB\v\xbaH\b*\x06\x18\xc0\x84=(\x01R\x0emaxMailboxSize\x12-\n" +
	"\x12enable_persistence\x18\x03 \x01(\bR\x11enablePersistence\x12\\\n" +
	"\x13checkpoint_interval\x18\x04 \x01(\v2\x19.google.protobuf.DurationB\x10\xbaH\r\xaa\x01\n" +
	"\"\x04\b\x80\xa3\x052\x02\b\x01R\x12checkpointInterval\x12H\n" +
	"\x0erestart_policy\x18\x05 \x01(\v2!.plexspaces.common.v1.RetryPolicyR\rrestartPolicy\x12a\n" +
	"\x14supervision_strategy\x18\x06 \x01(\x0e2..plexspaces.supervision.v1.SupervisionStrategyR\x13supervisionStrategy\x12P\n" +
	"\n" +
	"properties\x18\a \x03(\v20.plexspaces.actor.v1.ActorConfig.PropertiesEntryR\n" +
	"properties\x12b\n" +
	"\x17stateless_worker_config\x18\v \x01(\v2*.plexspaces.actor.v1.StatelessWorkerConfigR\x15statelessWorkerConfig\x12Y\n" +
	"\x14data_parallel_config\x18\f \x01(\v2'.plexspaces.actor.v1.DataParallelConfigR\x12dataParallelConfig\x12V\n" +
	"\x15state_management_mode\x18\r \x01(\x0e2\".plexspaces.actor.v1.StateMgmtModeR\x13stateManagementMode\x12R\n" +
	"\x11consistency_level\x18\x0e \x01(\x0e2%.plexspaces.actor.v1.ConsistencyLevelR\x10consistencyLevel\x12c\n" +
	"\x15resource_requirements\x18\x10 \x01(\v2..plexspaces.actor.v1.ActorResourceRequirementsR\x14resourceRequirements\x122\n" +
	"\factor_groups\x18\x11 \x03(\tB\x0f\xbaH\f\x92\x01\t\"\ar\x05\x10\x01\x18\xff\x01R\vactorGroups\x122\n" +
	"\x15config_schema_version\x18\x0f \x01(\rR\x13configSchemaVersion\x1aS\n" +
	"\x0fPropertiesEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12*\n" +
	"\x05value\x18\x02 \x01(\v2\x14.google.protobuf.AnyR\x05value:\x028\x01:5\x92A2\n" +
	"0*\fActor Config2 Configuration for actor behavior\"\x83\x03\n" +
	"\x14ResourceRequirements\x12-\n" +
	"\rmin_memory_mb\x18\x01 \x01(\x04B\t\xbaH\x062\x04\x18\x80\x80@R\vminMemoryMb\x12;\n" +
	"\rmin_cpu_cores\x18\x02 \x01(\x01B\x17\xbaH\x14\x12\x12\x19\x00\x00\x00\x00\x00\x00`@)\x9a\x99\x99\x99\x99\x99\xb9?R\vminCpuCores\x12D\n" +
	"\x15required_capabilities\x18\x03 \x03(\tB\x0f\xbaH\f\x92\x01\t\"\ar\x05\x10\x01\x18\x80\x01R\x14requiredCapabilities\x12r\n" +
	"\x13custom_requirements\x18\x04 \x03(\v2A.plexspaces.actor.v1.ResourceRequirements.CustomRequirementsEntryR\x12customRequirements\x1aE\n" +
	"\x17CustomRequirementsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\"]\n" +
	"\x19ActorResourceRequirements\x12@\n" +
	"\tplacement\x18\x01 \x01(\v2\".plexspaces.actor.v1.NodePlacementR\tplacement\"\xbf\x01\n" +
	"\x15StatelessWorkerConfig\x12/\n" +
	"\rmax_instances\x18\x01 \x01(\rB\n" +
	"\xbaH\a*\x05\x18\x90N(\x01R\fmaxInstances\x12-\n" +
	"\rmin_instances\x18\x02 \x01(\rB\b\xbaH\x05*\x03\x18\x90NR\fminInstances\x12F\n" +
	"\bstrategy\x18\x03 \x01(\x0e2*.plexspaces.actor.v1.LoadBalancingStrategyR\bstrategy\"\xe5\x02\n" +
	"\x12DataParallelConfig\x12%\n" +
	"\bgroup_id\x18\x01 \x01(\tB\n" +
	"\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x12.\n" +
	"\vshard_count\x18\x02 \x01(\rB\r\xbaH\n" +
	"*\b\x18\x80\x94\xeb\xdc\x03(\x01R\n" +
	"shardCount\x12U\n" +
	"\x12partition_strategy\x18\x04 \x01(\x0e2&.plexspaces.actor.v1.PartitionStrategyR\x11partitionStrategy\x12O\n" +
	"\x10rebalance_policy\x18\x05 \x01(\x0e2$.plexspaces.actor.v1.RebalancePolicyR\x0frebalancePolicy\x12@\n" +
	"\tplacement\x18\x06 \x01(\v2\".plexspaces.actor.v1.NodePlacementR\tplacementJ\x04\b\x03\x10\x04R\bshard_id\"\xe4\x03\n" +
	"\n" +
	"ShardGroup\x12?\n" +
	"\x06config\x18\x01 \x01(\v2'.plexspaces.actor.v1.DataParallelConfigR\x06config\x12\x1d\n" +
	"\n" +
	"actor_type\x18\x02 \x01(\tR\tactorType\x12&\n" +
	"\x0fshard_actor_ids\x18\x03 \x03(\tR\rshardActorIds\x12:\n" +
	"\x05state\x18\x04 \x01(\x0e2$.plexspaces.actor.v1.ShardGroupStateR\x05state\x129\n" +
	"\n" +
	"created_at\x18\x05 \x01(\v2\x1a.google.protobuf.TimestampR\tcreatedAt\x12I\n" +
	"\bmetadata\x18\x06 \x03(\v2-.plexspaces.actor.v1.ShardGroup.MetadataEntryR\bmetadata\x12O\n" +
	"\x10rebalance_status\x18\a \x01(\v2$.plexspaces.actor.v1.RebalanceStatusR\x0frebalanceStatus\x1a;\n" +
	"\rMetadataEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\"\xbd\x02\n" +
	"\x0fRebalanceStatus\x12%\n" +
	"\x0eis_rebalancing\x18\x01 \x01(\bR\risRebalancing\x12&\n" +
	"\x0fold_shard_count\x18\x02 \x01(\rR\roldShardCount\x12&\n" +
	"\x0fnew_shard_count\x18\x03 \x01(\rR\rnewShardCount\x12)\n" +
	"\x10progress_percent\x18\x04 \x01(\x01R\x0fprogressPercent\x129\n" +
	"\n" +
	"started_at\x18\x05 \x01(\v2\x1a.google.protobuf.TimestampR\tstartedAt\x12M\n" +
	"\x14estimated_completion\x18\x06 \x01(\v2\x1a.google.protobuf.TimestampR\x13estimatedCompletion\"\xe4\x04\n" +
	"\rNodePlacement\x12F\n" +
	"\bstrategy\x18\x01 \x01(\x0e2*.plexspaces.actor.v1.NodePlacementStrategyR\bstrategy\x12\x18\n" +
	"\acluster\x18\x02 \x01(\tR\acluster\x12\x19\n" +
	"\bnode_ids\x18\x03 \x03(\tR\anodeIds\x12_\n" +
	"\x0frequired_labels\x18\x04 \x03(\v26.plexspaces.actor.v1.NodePlacement.RequiredLabelsEntryR\x0erequiredLabels\x125\n" +
	"\x0eavoid_node_ids\x18\x05 \x03(\tB\x0f\xbaH\f\x92\x01\t\"\ar\x05\x10\x01\x18\xff\x01R\favoidNodeIds\x12W\n" +
	"\x15resource_requirements\x18\x06 \x01(\v2\".plexspaces.common.v1.ResourceSpecR\x14resourceRequirements\x12_\n" +
	"\x0faffinity_labels\x18\a \x03(\v26.plexspaces.actor.v1.NodePlacement.AffinityLabelsEntryR\x0eaffinityLabels\x1aA\n" +
	"\x13RequiredLabelsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\x1aA\n" +
	"\x13AffinityLabelsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\"\xec\x03\n" +
	"\x17CreateShardGroupRequest\x12J\n" +
	"\x06config\x18\x01 \x01(\v2'.plexspaces.actor.v1.DataParallelConfigB\t\xe0A\x02\xbaH\x03\xc8\x01\x01R\x06config\x12,\n" +
	"\n" +
	"actor_type\x18\x02 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\x80\x01R\tactorType\x12C\n" +
	"\fshard_config\x18\x03 \x01(\v2 .plexspaces.actor.v1.ActorConfigR\vshardConfig\x12#\n" +
	"\rinitial_state\x18\x04 \x01(\fR\finitialState\x12V\n" +
	"\bmetadata\x18\x05 \x03(\v2:.plexspaces.actor.v1.CreateShardGroupRequest.MetadataEntryR\bmetadata\x1a;\n" +
	"\rMetadataEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01:X\x92AU\n" +
	"S*\x1aCreate Shard Group Request2\x1fRequest to create a shard group\xd2\x01\x06config\xd2\x01\n" +
	"actor_type\"\x98\x01\n" +
	"\x18CreateShardGroupResponse\x12:\n" +
	"\x05group\x18\x01 \x01(\v2\x1f.plexspaces.actor.v1.ShardGroupB\x03\xe0A\x02R\x05group:@\x92A=\n" +
	";*\x1bCreate Shard Group Response2\x1cCreated shard group metadata\"\xee\x01\n" +
	"\x17DeleteShardGroupRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x12\x14\n" +
	"\x05force\x18\x02 \x01(\bR\x05force\x12D\n" +
	"\x10shutdown_timeout\x18\x03 \x01(\v2\x19.google.protobuf.DurationR\x0fshutdownTimeout:M\x92AJ\n" +
	"H*\x1aDelete Shard Group Request2\x1fRequest to delete a shard group\xd2\x01\bgroup_id\"\x8b\x01\n" +
	"\x14GetShardGroupRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId:I\x92AF\n" +
	"D*\x17Get Shard Group Request2\x1eRequest to fetch a shard group\xd2\x01\bgroup_id\"\x89\x01\n" +
	"\x15GetShardGroupResponse\x12:\n" +
	"\x05group\x18\x01 \x01(\v2\x1f.plexspaces.actor.v1.ShardGroupB\x03\xe0A\x02R\x05group:4\x92A1\n" +
	"/*\x18Get Shard Group Response2\x13Shard group details\"\xf6\x01\n" +
	"\x16ListShardGroupsRequest\x12)\n" +
	"\n" +
	"actor_type\x18\x01 \x01(\tB\n" +
	"\xbaH\ar\x05\x10\x01\x18\x80\x01R\tactorType\x12:\n" +
	"\x05state\x18\x02 \x01(\x0e2$.plexspaces.actor.v1.ShardGroupStateR\x05state\x125\n" +
	"\x04page\x18\x03 \x01(\v2!.plexspaces.common.v1.PageRequestR\x04page:>\x92A;\n" +
	"9*\x19List Shard Groups Request2\x1cRequest to list shard groups\"\xc9\x01\n" +
	"\x17ListShardGroupsResponse\x127\n" +
	"\x06groups\x18\x01 \x03(\v2\x1f.plexspaces.actor.v1.ShardGroupR\x06groups\x126\n" +
	"\x04page\x18\x02 \x01(\v2\".plexspaces.common.v1.PageResponseR\x04page:=\x92A:\n" +
	"8*\x1aList Shard Groups Response2\x1aPaged list of shard groups\"\x8f\x03\n" +
	"\x12SendToShardRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x12/\n" +
	"\rpartition_key\x18\x02 \x01(\fB\n" +
	"\xe0A\x02\xbaH\x04z\x02\x10\x01R\fpartitionKey\x12B\n" +
	"\amessage\x18\x03 \x01(\v2\x1d.plexspaces.common.v1.MessageB\t\xe0A\x02\xbaH\x03\xc8\x01\x01R\amessage\x12*\n" +
	"\x11wait_for_response\x18\x04 \x01(\bR\x0fwaitForResponse\x123\n" +
	"\atimeout\x18\x05 \x01(\v2\x19.google.protobuf.DurationR\atimeout:y\x92Av\n" +
	"t*\x15Send To Shard Request26Request to route a message to a shard by partition key\xd2\x01\bgroup_id\xd2\x01\rpartition_key\xd2\x01\amessage\"\xc6\x01\n" +
	"\x13SendToShardResponse\x12\x19\n" +
	"\bshard_id\x18\x01 \x01(\rR\ashardId\x12$\n" +
	"\x0eshard_actor_id\x18\x02 \x01(\tR\fshardActorId\x129\n" +
	"\bresponse\x18\x03 \x01(\v2\x1d.plexspaces.common.v1.MessageR\bresponse:3\x92A0\n" +
	".*\x16Send To Shard Response2\x14Shard routing result\"\x95\x03\n" +
	"\x14ScatterGatherRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x12>\n" +
	"\x05query\x18\x02 \x01(\v2\x1d.plexspaces.common.v1.MessageB\t\xe0A\x02\xbaH\x03\xc8\x01\x01R\x05query\x123\n" +
	"\atimeout\x18\x03 \x01(\v2\x19.google.protobuf.DurationR\atimeout\x12T\n" +
	"\vaggregation\x18\x04 \x01(\x0e22.plexspaces.actor.v1.ShardGroupAggregationStrategyR\vaggregation\x12#\n" +
	"\rmin_responses\x18\x05 \x01(\rR\fminResponses:c\x92A`\n" +
	"^*\x16Scatter Gather Request21Request to query all shards and aggregate results\xd2\x01\bgroup_id\xd2\x01\x05query\"\xf5\x01\n" +
	"\x12ShardQueryResponse\x12\x19\n" +
	"\bshard_id\x18\x01 \x01(\rR\ashardId\x12$\n" +
	"\x0eshard_actor_id\x18\x02 \x01(\tR\fshardActorId\x129\n" +
	"\bresponse\x18\x03 \x01(\v2\x1d.plexspaces.common.v1.MessageR\bresponse\x123\n" +
	"\alatency\x18\x04 \x01(\v2\x19.google.protobuf.DurationR\alatency\x12\x18\n" +
	"\asuccess\x18\x05 \x01(\bR\asuccess\x12\x14\n" +
	"\x05error\x18\x06 \x01(\tR\x05error\"\xc7\x01\n" +
	"\x12ScatterGatherStats\x12%\n" +
	"\x0eshards_queried\x18\x01 \x01(\rR\rshardsQueried\x12)\n" +
	"\x10shards_responded\x18\x02 \x01(\rR\x0fshardsResponded\x12#\n" +
	"\rshards_failed\x18\x03 \x01(\rR\fshardsFailed\x12:\n" +
	"\vmax_latency\x18\x04 \x01(\v2\x19.google.protobuf.DurationR\n" +
	"maxLatency\"\xa1\x02\n" +
	"\x15ScatterGatherResponse\x125\n" +
	"\x06result\x18\x01 \x01(\v2\x1d.plexspaces.common.v1.MessageR\x06result\x12P\n" +
	"\x0fshard_responses\x18\x02 \x03(\v2'.plexspaces.actor.v1.ShardQueryResponseR\x0eshardResponses\x12=\n" +
	"\x05stats\x18\x03 \x01(\v2'.plexspaces.actor.v1.ScatterGatherStatsR\x05stats:@\x92A=\n" +
	";*\x17Scatter Gather Response2 Aggregated scatter-gather result\"\xac\x04\n" +
	"\x1bBulkUpdateShardGroupRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x12\\\n" +
	"\aupdates\x18\x02 \x03(\v2=.plexspaces.actor.v1.BulkUpdateShardGroupRequest.UpdatesEntryB\x03\xe0A\x02R\aupdates\x12R\n" +
	"\x11consistency_level\x18\x03 \x01(\x0e2%.plexspaces.actor.v1.ConsistencyLevelR\x10consistencyLevel\x123\n" +
	"\atimeout\x18\x04 \x01(\v2\x19.google.protobuf.DurationR\atimeout\x12,\n" +
	"\x12wait_for_responses\x18\x05 \x01(\bR\x10waitForResponses\x1aY\n" +
	"\fUpdatesEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x123\n" +
	"\x05value\x18\x02 \x01(\v2\x1d.plexspaces.common.v1.MessageR\x05value:\x028\x01:s\x92Ap\n" +
	"n*\x1fBulk Update Shard Group Request26Request to apply multiple updates across a shard group\xd2\x01\bgroup_id\xd2\x01\aupdates\"\xd2\x02\n" +
	"\x1cBulkUpdateShardGroupResponse\x12!\n" +
	"\fupdates_sent\x18\x01 \x01(\rR\vupdatesSent\x12+\n" +
	"\x11updates_succeeded\x18\x02 \x01(\rR\x10updatesSucceeded\x12%\n" +
	"\x0eupdates_failed\x18\x03 \x01(\rR\rupdatesFailed\x12F\n" +
	"\vshard_stats\x18\x04 \x03(\v2%.plexspaces.actor.v1.ShardUpdateStatsR\n" +
	"shardStats\x12\x16\n" +
	"\x06errors\x18\x05 \x03(\tR\x06errors:[\x92AX\n" +
	"V* Bulk Update Shard Group Response22Update statistics for a bulk shard-group operation\"\xca\x01\n" +
	"\x10ShardUpdateStats\x12\x19\n" +
	"\bshard_id\x18\x01 \x01(\rR\ashardId\x12$\n" +
	"\x0eshard_actor_id\x18\x02 \x01(\tR\fshardActorId\x12!\n" +
	"\fupdates_sent\x18\x03 \x01(\rR\vupdatesSent\x12+\n" +
	"\x11updates_succeeded\x18\x04 \x01(\rR\x10updatesSucceeded\x12%\n" +
	"\x0eupdates_failed\x18\x05 \x01(\rR\rupdatesFailed\"\xcc\x02\n" +
	"\x14MapShardGroupRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x12K\n" +
	"\fmap_function\x18\x02 \x01(\v2\x1d.plexspaces.common.v1.MessageB\t\xe0A\x02\xbaH\x03\xc8\x01\x01R\vmapFunction\x123\n" +
	"\atimeout\x18\x03 \x01(\v2\x19.google.protobuf.DurationR\atimeout\x12#\n" +
	"\rmin_responses\x18\x04 \x01(\rR\fminResponses:c\x92A`\n" +
	"^*\x17Map Shard Group Request2)Request to apply a function to all shards\xd2\x01\bgroup_id\xd2\x01\fmap_function\"\xed\x01\n" +
	"\x15MapShardGroupResponse\x12L\n" +
	"\rshard_results\x18\x01 \x03(\v2'.plexspaces.actor.v1.ShardQueryResponseR\fshardResults\x12=\n" +
	"\x05stats\x18\x02 \x01(\v2'.plexspaces.actor.v1.ScatterGatherStatsR\x05stats:G\x92AD\n" +
	"B*\x18Map Shard Group Response2&Results from mapping across all shards\"6\n" +
	"\x15CollectiveTargetField\x12\x1d\n" +
	"\n" +
	"value_path\x18\x01 \x01(\tR\tvaluePath\"\xc3\x02\n" +
	"\x1aBroadcastShardGroupRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x12B\n" +
	"\amessage\x18\x02 \x01(\v2\x1d.plexspaces.common.v1.MessageB\t\xe0A\x02\xbaH\x03\xc8\x01\x01R\amessage\x123\n" +
	"\atimeout\x18\x03 \x01(\v2\x19.google.protobuf.DurationR\atimeout\x12\x19\n" +
	"\bmin_acks\x18\x04 \x01(\rR\aminAcks:g\x92Ad\n" +
	"b*\x1dBroadcast Shard Group Request2,Request to broadcast a message to all shards\xd2\x01\bgroup_id\xd2\x01\amessage\"\x84\x02\n" +
	"\x1bBroadcastShardGroupResponse\x12P\n" +
	"\x0fshard_responses\x18\x01 \x03(\v2'.plexspaces.actor.v1.ShardQueryResponseR\x0eshardResponses\x12=\n" +
	"\x05stats\x18\x02 \x01(\v2'.plexspaces.actor.v1.ScatterGatherStatsR\x05stats:T\x92AQ\n" +
	"O*\x1eBroadcast Shard Group Response2-Per-shard responses for a broadcast operation\"\xe2\x03\n" +
	"\x17ReduceShardGroupRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x12K\n" +
	"\fmap_function\x18\x02 \x01(\v2\x1d.plexspaces.common.v1.MessageB\t\xe0A\x02\xbaH\x03\xc8\x01\x01R\vmapFunction\x123\n" +
	"\atimeout\x18\x03 \x01(\v2\x19.google.protobuf.DurationR\atimeout\x12#\n" +
	"\rmin_responses\x18\x04 \x01(\rR\fminResponses\x12F\n" +
	"\treduction\x18\x05 \x01(\x0e2(.plexspaces.actor.v1.CollectiveReductionR\treduction\x12B\n" +
	"\x06target\x18\x06 \x01(\v2*.plexspaces.actor.v1.CollectiveTargetFieldR\x06target:j\x92Ag\n" +
	"e*\x1aReduce Shard Group Request2-Request to reduce values across a shard group\xd2\x01\bgroup_id\xd2\x01\fmap_function\"\xae\x02\n" +
	"\x18ReduceShardGroupResponse\x125\n" +
	"\x06result\x18\x01 \x01(\v2\x1d.plexspaces.common.v1.MessageR\x06result\x12P\n" +
	"\x0fshard_responses\x18\x02 \x03(\v2'.plexspaces.actor.v1.ShardQueryResponseR\x0eshardResponses\x12=\n" +
	"\x05stats\x18\x03 \x01(\v2'.plexspaces.actor.v1.ScatterGatherStatsR\x05stats:J\x92AG\n" +
	"E*\x1bReduce Shard Group Response2&Reduced result and per-shard responses\"\xea\x03\n" +
	"\x1aAllReduceShardGroupRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x12K\n" +
	"\fmap_function\x18\x02 \x01(\v2\x1d.plexspaces.common.v1.MessageB\t\xe0A\x02\xbaH\x03\xc8\x01\x01R\vmapFunction\x123\n" +
	"\atimeout\x18\x03 \x01(\v2\x19.google.protobuf.DurationR\atimeout\x12#\n" +
	"\rmin_responses\x18\x04 \x01(\rR\fminResponses\x12F\n" +
	"\treduction\x18\x05 \x01(\x0e2(.plexspaces.actor.v1.CollectiveReductionR\treduction\x12B\n" +
	"\x06target\x18\x06 \x01(\v2*.plexspaces.actor.v1.CollectiveTargetFieldR\x06target:o\x92Al\n" +
	"j*\x1eAll Reduce Shard Group Request2.Request to run all-reduce across a shard group\xd2\x01\bgroup_id\xd2\x01\fmap_function\"\xb8\x02\n" +
	"\x1bAllReduceShardGroupResponse\x125\n" +
	"\x06result\x18\x01 \x01(\v2\x1d.plexspaces.common.v1.MessageR\x06result\x12P\n" +
	"\x0fshard_responses\x18\x02 \x03(\v2'.plexspaces.actor.v1.ShardQueryResponseR\x0eshardResponses\x12=\n" +
	"\x05stats\x18\x03 \x01(\v2'.plexspaces.actor.v1.ScatterGatherStatsR\x05stats:Q\x92AN\n" +
	"L*\x1fAll Reduce Shard Group Response2)All-reduce result and per-shard responses\"\xc8\x02\n" +
	"\x18BarrierShardGroupRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x12,\n" +
	"\n" +
	"barrier_id\x18\x02 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\tbarrierId\x12\x14\n" +
	"\x05round\x18\x03 \x01(\x04R\x05round\x123\n" +
	"\atimeout\x18\x04 \x01(\v2\x19.google.protobuf.DurationR\atimeout\x12\x19\n" +
	"\bmin_acks\x18\x05 \x01(\rR\aminAcks:n\x92Ak\n" +
	"i*\x1bBarrier Shard Group Request22Request to synchronize a shard group barrier round\xd2\x01\bgroup_id\xd2\x01\n" +
	"barrier_id\"\xf4\x01\n" +
	"\x19BarrierShardGroupResponse\x12P\n" +
	"\x0fshard_responses\x18\x01 \x03(\v2'.plexspaces.actor.v1.ShardQueryResponseR\x0eshardResponses\x12=\n" +
	"\x05stats\x18\x02 \x01(\v2'.plexspaces.actor.v1.ScatterGatherStatsR\x05stats:F\x92AC\n" +
	"A*\x1cBarrier Shard Group Response2!Barrier synchronization responses\"\xf2\x02\n" +
	"\x16ScaleShardGroupRequest\x12(\n" +
	"\bgroup_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\agroupId\x122\n" +
	"\x0fnew_shard_count\x18\x02 \x01(\rB\n" +
	"\xe0A\x02\xbaH\x04*\x02(\x01R\rnewShardCount\x12O\n" +
	"\x10rebalance_policy\x18\x03 \x01(\x0e2$.plexspaces.actor.v1.RebalancePolicyR\x0frebalancePolicy\x12J\n" +
	"\x10new_shard_config\x18\x04 \x01(\v2 .plexspaces.actor.v1.ActorConfigR\x0enewShardConfig:]\x92AZ\n" +
	"X*\x19Scale Shard Group Request2\x1eRequest to scale a shard group\xd2\x01\bgroup_id\xd2\x01\x0fnew_shard_count\"\xee\x01\n" +
	"\x17ScaleShardGroupResponse\x125\n" +
	"\x05group\x18\x01 \x01(\v2\x1f.plexspaces.actor.v1.ShardGroupR\x05group\x12O\n" +
	"\x10rebalance_status\x18\x02 \x01(\v2$.plexspaces.actor.v1.RebalanceStatusR\x0frebalanceStatus:K\x92AH\n" +
	"F*\x1aScale Shard Group Response2(Updated shard group and rebalance status\"\xdc\x03\n" +
	"\fActorMetrics\x12-\n" +
	"\x12messages_processed\x18\x01 \x01(\x04R\x11messagesProcessed\x12'\n" +
	"\x0fmessages_failed\x18\x02 \x01(\x04R\x0emessagesFailed\x12^\n" +
	"\x17average_processing_time\x18\x03 \x01(\v2\x19.google.protobuf.DurationB\v\xbaH\b\xaa\x01\x05\"\x03\b\x90\x1cR\x15averageProcessingTime\x12\x1a\n" +
	"\brestarts\x18\x04 \x01(\x04R\brestarts\x12?\n" +
	"\rlast_activity\x18\x05 \x01(\v2\x1a.google.protobuf.TimestampR\flastActivity\x12:\n" +
	"\x12memory_usage_bytes\x18\x06 \x01(\x04B\f\xbaH\t2\a\x18\x80\x80\x80\x80\x80 R\x10memoryUsageBytes\x12C\n" +
	"\x11cpu_usage_percent\x18\a \x01(\x01B\x17\xbaH\x14\x12\x12\x19\x00\x00\x00\x00\x00\x00Y@)\x00\x00\x00\x00\x00\x00\x00\x00R\x0fcpuUsagePercent:6\x92A3\n" +
	"1*\rActor Metrics2 Performance metrics for an actor\"\xf1\x02\n" +
	"\x11SpawnActorRequest\x12<\n" +
	"\x04spec\x18\x01 \x01(\v2#.plexspaces.actor.v1.ActorSpawnSpecB\x03\xe0A\x02R\x04spec\x12)\n" +
	"\tnamespace\x18\x02 \x01(\tB\v\xe0A\x01\xbaH\x05r\x03\x18\x80\x01R\tnamespace\x126\n" +
	"\x0finstances_count\x18\x03 \x01(\rB\r\xe0A\x01\xbaH\a*\x05\x18\x80\b(\x00R\x0einstancesCount:\xba\x01\x92A\xb6\x01\n" +
	"\xb3\x01*\x1aSpawn Remote Actor Request2\x8d\x01Spawn actor on the node receiving this gRPC request using ActorSpawnSpec as the single contract (identity, facets, args, config, visibility).\xd2\x01\x04spec\"\xc7\x01\n" +
	"\x12SpawnActorResponse\x12*\n" +
	"\tactor_ref\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\x80\x04R\bactorRef\x120\n" +
	"\x05actor\x18\x02 \x01(\v2\x1a.plexspaces.actor.v1.ActorR\x05actor:S\x92AP\n" +
	"N*\x1bSpawn Remote Actor Response2#Reference to remotely spawned actor\xd2\x01\tactor_ref\"X\n" +
	"\x12SpawnActorsRequest\x12B\n" +
	"\brequests\x18\x01 \x03(\v2&.plexspaces.actor.v1.SpawnActorRequestR\brequests\"\x87\x01\n" +
	"\x10SpawnActorResult\x12\x18\n" +
	"\asuccess\x18\x01 \x01(\bR\asuccess\x12\x14\n" +
	"\x05error\x18\x02 \x01(\tR\x05error\x12C\n" +
	"\bresponse\x18\x03 \x01(\v2'.plexspaces.actor.v1.SpawnActorResponseR\bresponse\"V\n" +
	"\x13SpawnActorsResponse\x12?\n" +
	"\aresults\x18\x01 \x03(\v2%.plexspaces.actor.v1.SpawnActorResultR\aresults\"\xaf\x01\n" +
	"\x0fGetActorRequest\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId\x12)\n" +
	"\tnamespace\x18\x02 \x01(\tB\v\xe0A\x01\xbaH\x05r\x03\x18\x80\x01R\tnamespace:G\x92AD\n" +
	"B*\x11Get Actor Request2\"Request to retrieve an actor by ID\xd2\x01\bactor_id\"D\n" +
	"\x10GetActorResponse\x120\n" +
	"\x05actor\x18\x01 \x01(\v2\x1a.plexspaces.actor.v1.ActorR\x05actor\"\xca\x02\n" +
	"\x11ListActorsRequest\x12D\n" +
	"\fpage_request\x18\x01 \x01(\v2!.plexspaces.common.v1.PageRequestR\vpageRequest\x12'\n" +
	"\n" +
	"actor_type\x18\x02 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x01R\tactorType\x125\n" +
	"\x05state\x18\x03 \x01(\x0e2\x1f.plexspaces.actor.v1.ActorStateR\x05state\x12!\n" +
	"\anode_id\x18\x04 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\x06nodeId\x12)\n" +
	"\tnamespace\x18\x05 \x01(\tB\v\xe0A\x01\xbaH\x05r\x03\x18\x80\x01R\tnamespace:A\x92A>\n" +
	"<*\x13List Actors Request2%Request to list actors with filtering\"\x91\x01\n" +
	"\x12ListActorsResponse\x122\n" +
	"\x06actors\x18\x01 \x03(\v2\x1a.plexspaces.actor.v1.ActorR\x06actors\x12G\n" +
	"\rpage_response\x18\x02 \x01(\v2\".plexspaces.common.v1.PageResponseR\fpageResponse\"\xa6\a\n" +
	"\x12SendMessageRequest\x12)\n" +
	"\tnamespace\x18\x01 \x01(\tB\v\xe0A\x01\xbaH\x05r\x03\x18\x80\x01R\tnamespace\x12,\n" +
	"\n" +
	"actor_type\x18\x02 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\tactorType\x12(\n" +
	"\vhttp_method\x18\x03 \x01(\tB\a\xbaH\x04r\x02\x18\x10R\n" +
	"httpMethod\x12\x18\n" +
	"\apayload\x18\x04 \x01(\fR\apayload\x12N\n" +
	"\aheaders\x18\x05 \x03(\v24.plexspaces.actor.v1.SendMessageRequest.HeadersEntryR\aheaders\x12[\n" +
	"\fquery_params\x18\x06 \x03(\v28.plexspaces.actor.v1.SendMessageRequest.QueryParamsEntryR\vqueryParams\x12\x1c\n" +
	"\x04path\x18\a \x01(\tB\b\xbaH\x05r\x03\x18\x80\x10R\x04path\x12\"\n" +
	"\asubpath\x18\b \x01(\tB\b\xbaH\x05r\x03\x18\x80\x10R\asubpath\x12%\n" +
	"\tsender_id\x18\t \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\bsenderId\x12U\n" +
	"\fmessage_type\x18\n" +
	" \x01(\tB2\xbaH/r-R\x00R\x04castR\x04infoR\x06signalR\x05eventR\acommandR\x05queryR\vmessageType\x12/\n" +
	"\x0ecorrelation_id\x18\v \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\rcorrelationId\x12#\n" +
	"\breply_to\x18\f \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\areplyTo\x12'\n" +
	"\n" +
	"message_id\x18\r \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\tmessageId\x12'\n" +
	"\n" +
	"actor_name\x18\x14 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\tactorName\x1a:\n" +
	"\fHeadersEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\x1a>\n" +
	"\x10QueryParamsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01:b\x92A_\n" +
	"]*\x14Send Message Request28Request to send a tell message to an actor or actor type\xd2\x01\n" +
	"actor_type\"\xea\x01\n" +
	"\x13SendMessageResponse\x12\x18\n" +
	"\asuccess\x18\x01 \x01(\bR\asuccess\x12)\n" +
	"\n" +
	"message_id\x18\x02 \x01(\tB\n" +
	"\xbaH\ar\x05\x10\x01\x18\xff\x01R\tmessageId\x12#\n" +
	"\bactor_id\x18\x03 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\aactorId\x12-\n" +
	"\rerror_message\x18\x04 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x10R\ferrorMessage::\x92A7\n" +
	"5*\x15Send Message Response2\x1cTell request acknowledgement\"\xc2\x01\n" +
	"\x14StreamMessageRequest\x12<\n" +
	"\amessage\x18\x01 \x01(\v2\x1d.plexspaces.common.v1.MessageB\x03\xe0A\x02R\amessage\x12\x1a\n" +
	"\bsequence\x18\x02 \x01(\x04R\bsequence:P\x92AM\n" +
	"K*\x16Stream Message Request2'Request to stream a message to an actor\xd2\x01\amessage\"\xa1\x01\n" +
	"\x15StreamMessageResponse\x12)\n" +
	"\n" +
	"message_id\x18\x01 \x01(\tB\n" +
	"\xbaH\ar\x05\x10\x01\x18\xff\x01R\tmessageId\x12\x1a\n" +
	"\bsequence\x18\x02 \x01(\x04R\bsequence\x12!\n" +
	"\x06status\x18\x03 \x01(\tB\t\xbaH\x06r\x04\x10\x01\x18 R\x06status\x12\x1e\n" +
	"\x05error\x18\x04 \x01(\tB\b\xbaH\x05r\x03\x18\x80\bR\x05error\"\xc3\x01\n" +
	"\x12DeleteActorRequest\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId\x12\x14\n" +
	"\x05force\x18\x02 \x01(\bR\x05force\x12)\n" +
	"\tnamespace\x18\x03 \x01(\tB\v\xe0A\x01\xbaH\x05r\x03\x18\x80\x01R\tnamespace:B\x92A?\n" +
	"=*\x14Delete Actor Request2\x1aRequest to delete an actor\xd2\x01\bactor_id\"\xb5\x06\n" +
	"\x13ActorLifecycleEvent\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId\x12=\n" +
	"\ttimestamp\x18\x02 \x01(\v2\x1a.google.protobuf.TimestampB\x03\xe0A\x02R\ttimestamp\x12=\n" +
	"\acreated\x18\n" +
	" \x01(\v2!.plexspaces.actor.v1.ActorCreatedH\x00R\acreated\x12@\n" +
	"\bstarting\x18\v \x01(\v2\".plexspaces.actor.v1.ActorStartingH\x00R\bstarting\x12C\n" +
	"\tactivated\x18\f \x01(\v2#.plexspaces.actor.v1.ActorActivatedH\x00R\tactivated\x12L\n" +
	"\fdeactivating\x18\r \x01(\v2&.plexspaces.actor.v1.ActorDeactivatingH\x00R\fdeactivating\x12I\n" +
	"\vdeactivated\x18\x0e \x01(\v2%.plexspaces.actor.v1.ActorDeactivatedH\x00R\vdeactivated\x12F\n" +
	"\n" +
	"terminated\x18\x0f \x01(\v2$.plexspaces.actor.v1.ActorTerminatedH\x00R\n" +
	"terminated\x12:\n" +
	"\x06failed\x18\x10 \x01(\v2 .plexspaces.actor.v1.ActorFailedH\x00R\x06failed\x12C\n" +
	"\tmigrating\x18\x11 \x01(\v2#.plexspaces.actor.v1.ActorMigratingH\x00R\tmigrating:\x7f\x92A|\n" +
	"z*\x15Actor Lifecycle Event2=Actor state transition event for monitoring and observability\xd2\x01\bactor_id\xd2\x01\ttimestamp\xd2\x01\n" +
	"event_typeB\f\n" +
	"\n" +
	"event_type\"\x0e\n" +
	"\fActorCreated\"\x0f\n" +
	"\rActorStarting\"\x10\n" +
	"\x0eActorActivated\"5\n" +
	"\x11ActorDeactivating\x12 \n" +
	"\x06reason\x18\x01 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x02R\x06reason\"4\n" +
	"\x10ActorDeactivated\x12 \n" +
	"\x06reason\x18\x01 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x02R\x06reason\"8\n" +
	"\x0fActorTerminated\x12%\n" +
	"\x06reason\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\x80\x02R\x06reason\"]\n" +
	"\vActorFailed\x12#\n" +
	"\x05error\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\x80\x10R\x05error\x12)\n" +
	"\vstack_trace\x18\x02 \x01(\tB\b\xbaH\x05r\x03\x18\x80@R\n" +
	"stackTrace\"@\n" +
	"\x0eActorMigrating\x12.\n" +
	"\vtarget_node\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\n" +
	"targetNode\"\xaf\x02\n" +
	"\x13MonitorActorRequest\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId\x122\n" +
	"\rsupervisor_id\x18\x02 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\fsupervisorId\x12>\n" +
	"\x13supervisor_callback\x18\x03 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\x80\x04R\x12supervisorCallback:z\x92Aw\n" +
	"u*\x15Monitor Actor Request2+Request to monitor an actor for termination\xd2\x01\bactor_id\xd2\x01\rsupervisor_id\xd2\x01\x13supervisor_callback\"\x94\x01\n" +
	"\x14MonitorActorResponse\x12.\n" +
	"\vmonitor_ref\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\n" +
	"monitorRef:L\x92AI\n" +
	"G*\x16Monitor Actor Response2\x1fResponse with monitor reference\xd2\x01\vmonitor_ref\"\xa9\x02\n" +
	"\x15DemonitorActorRequest\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId\x122\n" +
	"\rsupervisor_id\x18\x02 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\fsupervisorId\x12.\n" +
	"\vmonitor_ref\x18\x03 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\n" +
	"monitorRef:\x81\x01\x92A~\n" +
	"|*\x17Demonitor Actor Request28Remove a monitor on the node hosting the monitored actor\xd2\x01\bactor_id\xd2\x01\rsupervisor_id\xd2\x01\vmonitor_ref\"\xdf\x02\n" +
	"\x15ActorDownNotification\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId\x122\n" +
	"\rsupervisor_id\x18\x02 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\fsupervisorId\x12%\n" +
	"\x06reason\x18\x03 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\x80\x02R\x06reason\x12)\n" +
	"\vmonitor_ref\x18\x04 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\n" +
	"monitorRef\x12$\n" +
	"\x0eis_link_signal\x18\x05 \x01(\bR\fisLinkSignal:p\x92Am\n" +
	"k*\x17Actor Down Notification2,Notification that monitored actor terminated\xd2\x01\bactor_id\xd2\x01\rsupervisor_id\xd2\x01\x06reason\"\x9d\x01\n" +
	"\x15GetActorStatesRequest\x12 \n" +
	"\tactor_ids\x18\x01 \x03(\tB\x03\xe0A\x02R\bactorIds:b\x92A_\n" +
	"]*\x18Get Actor States Request25Batch check of actor states for stale monitor cleanup\xd2\x01\tactor_ids\"\x8e\x02\n" +
	"\x16GetActorStatesResponse\x12O\n" +
	"\x06states\x18\x01 \x03(\v27.plexspaces.actor.v1.GetActorStatesResponse.StatesEntryR\x06states\x1aZ\n" +
	"\vStatesEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x125\n" +
	"\x05value\x18\x02 \x01(\x0e2\x1f.plexspaces.actor.v1.ActorStateR\x05value:\x028\x01:G\x92AD\n" +
	"B*\x19Get Actor States Response2%Map of actor_id to current ActorState\"\x92\x03\n" +
	"\tActorLink\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId\x125\n" +
	"\x0flinked_actor_id\x18\x02 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\rlinkedActorId\x129\n" +
	"\n" +
	"created_at\x18\x03 \x01(\v2\x1a.google.protobuf.TimestampR\tcreatedAt\x12H\n" +
	"\bmetadata\x18\x04 \x03(\v2,.plexspaces.actor.v1.ActorLink.MetadataEntryR\bmetadata\x1a;\n" +
	"\rMetadataEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01:b\x92A_\n" +
	"]*\n" +
	"Actor Link22Two-way link between actors for cascading failures\xd2\x01\bactor_id\xd2\x01\x0flinked_actor_id\"\xde\x01\n" +
	"\x10LinkActorRequest\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId\x125\n" +
	"\x0flinked_actor_id\x18\x02 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\rlinkedActorId:i\x92Af\n" +
	"d*\x12Link Actor Request21Request to link two actors for cascading failures\xd2\x01\bactor_id\xd2\x01\x0flinked_actor_id\"l\n" +
	"\x11LinkActorResponse\x12\x18\n" +
	"\asuccess\x18\x01 \x01(\bR\asuccess:=\x92A:\n" +
	"8*\x13Link Actor Response2!Response confirming link creation\"\xcd\x01\n" +
	"\x12UnlinkActorRequest\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId\x125\n" +
	"\x0flinked_actor_id\x18\x02 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\rlinkedActorId:V\x92AS\n" +
	"Q*\x14Unlink Actor Request2\x1cRequest to unlink two actors\xd2\x01\bactor_id\xd2\x01\x0flinked_actor_id\"o\n" +
	"\x13UnlinkActorResponse\x12\x18\n" +
	"\asuccess\x18\x01 \x01(\bR\asuccess:>\x92A;\n" +
	"9*\x15Unlink Actor Response2 Response confirming link removal\"\x9b\x01\n" +
	"\x17CheckActorExistsRequest\x12(\n" +
	"\bactor_id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\aactorId:V\x92AS\n" +
	"Q*\x1aCheck Actor Exists Request2(Request to check if virtual actor exists\xd2\x01\bactor_id\"\xc5\x01\n" +
	"\x18CheckActorExistsResponse\x12\x16\n" +
	"\x06exists\x18\x01 \x01(\bR\x06exists\x12\x1b\n" +
	"\tis_active\x18\x02 \x01(\bR\bisActive\x12\x1d\n" +
	"\n" +
	"is_virtual\x18\x03 \x01(\bR\tisVirtual:U\x92AR\n" +
	"P*\x1bCheck Actor Exists Response21Response indicating if actor exists and is active\"\xa4\a\n" +
	"\x0fAskReplyRequest\x12)\n" +
	"\tnamespace\x18\x01 \x01(\tB\v\xe0A\x01\xbaH\x05r\x03\x18\x80\x01R\tnamespace\x12,\n" +
	"\n" +
	"actor_type\x18\x02 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\x80\x01R\tactorType\x12\x1f\n" +
	"\vhttp_method\x18\x03 \x01(\tR\n" +
	"httpMethod\x12\x18\n" +
	"\apayload\x18\x04 \x01(\fR\apayload\x12K\n" +
	"\aheaders\x18\x05 \x03(\v21.plexspaces.actor.v1.AskReplyRequest.HeadersEntryR\aheaders\x12X\n" +
	"\fquery_params\x18\x06 \x03(\v25.plexspaces.actor.v1.AskReplyRequest.QueryParamsEntryR\vqueryParams\x12\x12\n" +
	"\x04path\x18\a \x01(\tR\x04path\x12\x18\n" +
	"\asubpath\x18\b \x01(\tR\asubpath\x12%\n" +
	"\tsender_id\x18\t \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\bsenderId\x12@\n" +
	"\fmessage_type\x18\n" +
	" \x01(\tB\x1d\xbaH\x1ar\x18R\x00R\x04callR\x05queryR\acommandR\vmessageType\x12/\n" +
	"\x0ecorrelation_id\x18\v \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\rcorrelationId\x12#\n" +
	"\breply_to\x18\f \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\areplyTo\x12'\n" +
	"\n" +
	"message_id\x18\r \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\tmessageId\x12C\n" +
	"\atimeout\x18\x0e \x01(\v2\x19.google.protobuf.DurationB\x0e\xe0A\x01\xbaH\b\xaa\x01\x05\"\x03\b\x90\x1cR\atimeout\x12'\n" +
	"\n" +
	"actor_name\x18\x14 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\tactorName\x1a:\n" +
	"\fHeadersEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\x1a>\n" +
	"\x10QueryParamsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01:V\x92AS\n" +
	"Q*\x11Ask Reply Request2/Request to ask an actor via HTTP-like interface\xd2\x01\n" +
	"actor_type\"\xcf\x02\n" +
	"\x10AskReplyResponse\x12\x18\n" +
	"\asuccess\x18\x01 \x01(\bR\asuccess\x12\x18\n" +
	"\apayload\x18\x02 \x01(\fR\apayload\x12L\n" +
	"\aheaders\x18\x03 \x03(\v22.plexspaces.actor.v1.AskReplyResponse.HeadersEntryR\aheaders\x12\x19\n" +
	"\bactor_id\x18\x04 \x01(\tR\aactorId\x12#\n" +
	"\rerror_message\x18\x05 \x01(\tR\ferrorMessage\x1a:\n" +
	"\fHeadersEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01:=\x92A:\n" +
	"8*\x12Ask Reply Response2\"Response from an actor ask request\"\xdc\x04\n" +
	"\x14LifecycleEventFilter\x121\n" +
	"\x0fsubscription_id\x18\x01 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\x0esubscriptionId\x12H\n" +
	"\vevent_types\x18\x02 \x03(\x0e2'.plexspaces.actor.v1.LifecycleEventTypeR\n" +
	"eventTypes\x122\n" +
	"\x10actor_id_pattern\x18\x03 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x04R\x0eactorIdPattern\x120\n" +
	"\x0fnode_id_pattern\x18\x04 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x04R\rnodeIdPattern\x12`\n" +
	"\rrequired_tags\x18\x05 \x03(\v2;.plexspaces.actor.v1.LifecycleEventFilter.RequiredTagsEntryR\frequiredTags\x12*\n" +
	"\vbuffer_size\x18\x06 \x01(\rB\t\xbaH\x06*\x04\x18\xa0\x8d\x06R\n" +
	"bufferSize\x12@\n" +
	"\vdrop_policy\x18\a \x01(\x0e2\x1f.plexspaces.actor.v1.DropPolicyR\n" +
	"dropPolicy\x1a?\n" +
	"\x11RequiredTagsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01:P\x92AM\n" +
	"K*\x16Lifecycle Event Filter21Filter criteria for lifecycle event subscriptions\"\xd6\x03\n" +
	"\x15VirtualActorLifecycle\x12A\n" +
	"\x0elast_activated\x18\x01 \x01(\v2\x1a.google.protobuf.TimestampR\rlastActivated\x12?\n" +
	"\rlast_accessed\x18\x02 \x01(\v2\x1a.google.protobuf.TimestampR\flastAccessed\x12<\n" +
	"\fidle_timeout\x18\x03 \x01(\v2\x19.google.protobuf.DurationR\vidleTimeout\x12)\n" +
	"\x10activation_count\x18\x04 \x01(\rR\x0factivationCount\x12#\n" +
	"\ris_activating\x18\x05 \x01(\bR\fisActivating\x122\n" +
	"\x15pending_message_count\x18\x06 \x01(\rR\x13pendingMessageCount:w\x92At\n" +
	"r* Virtual Actor Lifecycle Metadata2NActivation/deactivation metadata for virtual actors (timestamps, counts, etc.)\"\xe5\x01\n" +
	"\x12VirtualActorConfig\x12Y\n" +
	"\x13activation_strategy\x18\x01 \x01(\x0e2(.plexspaces.common.v1.ActivationStrategyR\x12activationStrategy\x12<\n" +
	"\fidle_timeout\x18\x02 \x01(\v2\x19.google.protobuf.DurationR\vidleTimeout\x126\n" +
	"\x17persist_on_deactivation\x18\x03 \x01(\bR\x15persistOnDeactivation\"\x80\x01\n" +
	"\rActorRefError\x12:\n" +
	"\x04code\x18\x01 \x01(\x0e2&.plexspaces.actor.v1.ActorRefErrorCodeR\x04code\x12\x18\n" +
	"\amessage\x18\x02 \x01(\tR\amessage\x12\x19\n" +
	"\bactor_id\x18\x03 \x01(\tR\aactorId\"\xf0\x02\n" +
	"\x10ResourceContract\x12&\n" +
	"\x0fmax_cpu_percent\x18\x01 \x01(\x02R\rmaxCpuPercent\x12(\n" +
	"\x10max_memory_bytes\x18\x02 \x01(\x04R\x0emaxMemoryBytes\x12/\n" +
	"\x12max_io_ops_per_sec\x18\x03 \x01(\rH\x00R\x0emaxIoOpsPerSec\x88\x01\x01\x12?\n" +
	"\x19guaranteed_bandwidth_mbps\x18\x04 \x01(\rH\x01R\x17guaranteedBandwidthMbps\x88\x01\x01\x12L\n" +
	"\x12max_execution_time\x18\x05 \x01(\v2\x19.google.protobuf.DurationH\x02R\x10maxExecutionTime\x88\x01\x01B\x15\n" +
	"\x13_max_io_ops_per_secB\x1c\n" +
	"\x1a_guaranteed_bandwidth_mbpsB\x15\n" +
	"\x13_max_execution_time\"\x9b\x01\n" +
	"\rResourceUsage\x12\x1f\n" +
	"\vcpu_percent\x18\x01 \x01(\x02R\n" +
	"cpuPercent\x12!\n" +
	"\fmemory_bytes\x18\x02 \x01(\x04R\vmemoryBytes\x12#\n" +
	"\x0eio_ops_per_sec\x18\x03 \x01(\rR\vioOpsPerSec\x12!\n" +
	"\fnetwork_mbps\x18\x04 \x01(\rR\vnetworkMbps\"\xe2\x01\n" +
	"\x11ResourceViolation\x12>\n" +
	"\x04code\x18\x01 \x01(\x0e2*.plexspaces.actor.v1.ResourceViolationCodeR\x04code\x12\x18\n" +
	"\amessage\x18\x02 \x01(\tR\amessage\x12(\n" +
	"\rallowed_value\x18\x03 \x01(\x02H\x00R\fallowedValue\x88\x01\x01\x12&\n" +
	"\factual_value\x18\x04 \x01(\x02H\x01R\vactualValue\x88\x01\x01B\x10\n" +
	"\x0e_allowed_valueB\x0f\n" +
	"\r_actual_value\"\xdd\x01\n" +
	"\vActorHealth\x12>\n" +
	"\x06status\x18\x01 \x01(\x0e2&.plexspaces.actor.v1.ActorHealthStatusR\x06status\x12?\n" +
	"\vstuck_since\x18\x02 \x01(\v2\x19.google.protobuf.DurationH\x00R\n" +
	"stuckSince\x88\x01\x01\x12*\n" +
	"\x0efailure_reason\x18\x03 \x01(\tH\x01R\rfailureReason\x88\x01\x01B\x0e\n" +
	"\f_stuck_sinceB\x11\n" +
	"\x0f_failure_reason\"\xff\x04\n" +
	"\x0eActorSpawnSpec\x12D\n" +
	"\bidentity\x18\x01 \x01(\v2#.plexspaces.common.v1.ActorIdentityB\x03\xe0A\x02R\bidentity\x12\x12\n" +
	"\x04role\x18\x02 \x01(\tR\x04role\x12\x1c\n" +
	"\tnamespace\x18\x03 \x01(\tR\tnamespace\x12\x1b\n" +
	"\ttenant_id\x18\x04 \x01(\tR\btenantId\x12D\n" +
	"\n" +
	"visibility\x18\x05 \x01(\x0e2$.plexspaces.actor.v1.ActorVisibilityR\n" +
	"visibility\x12#\n" +
	"\rbehavior_kind\x18\x06 \x01(\tR\fbehaviorKind\x12A\n" +
	"\x04args\x18\a \x03(\v2-.plexspaces.actor.v1.ActorSpawnSpec.ArgsEntryR\x04args\x123\n" +
	"\x06facets\x18\b \x03(\v2\x1b.plexspaces.common.v1.FacetR\x06facets\x12G\n" +
	"\x06labels\x18\t \x03(\v2/.plexspaces.actor.v1.ActorSpawnSpec.LabelsEntryR\x06labels\x128\n" +
	"\x06config\x18\n" +
	" \x01(\v2 .plexspaces.actor.v1.ActorConfigR\x06config\x1a7\n" +
	"\tArgsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\x1a9\n" +
	"\vLabelsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01*\x85\x02\n" +
	"\x11PlacementStrategy\x12\"\n" +
	"\x1ePLACEMENT_STRATEGY_UNSPECIFIED\x10\x00\x12\x1d\n" +
	"\x19PLACEMENT_STRATEGY_RANDOM\x10\x01\x12#\n" +
	"\x1fPLACEMENT_STRATEGY_PREFER_LOCAL\x10\x02\x12!\n" +
	"\x1dPLACEMENT_STRATEGY_LOAD_BASED\x10\x03\x12%\n" +
	"!PLACEMENT_STRATEGY_RESOURCE_BASED\x10\x04\x12\x1f\n" +
	"\x1bPLACEMENT_STRATEGY_AFFINITY\x10\x05\x12\x1d\n" +
	"\x19PLACEMENT_STRATEGY_CUSTOM\x10c*\x93\x01\n" +
	"\x15LoadBalancingStrategy\x12\x1e\n" +
	"\x1aLOAD_BALANCING_UNSPECIFIED\x10\x00\x12\x1e\n" +
	"\x1aLOAD_BALANCING_ROUND_ROBIN\x10\x01\x12\x1f\n" +
	"\x1bLOAD_BALANCING_LEAST_LOADED\x10\x02\x12\x19\n" +
	"\x15LOAD_BALANCING_RANDOM\x10\x03*\xb9\x01\n" +
	"\x11PartitionStrategy\x12\"\n" +
	"\x1ePARTITION_STRATEGY_UNSPECIFIED\x10\x00\x12\x1b\n" +
	"\x17PARTITION_STRATEGY_HASH\x10\x01\x12\x1c\n" +
	"\x18PARTITION_STRATEGY_RANGE\x10\x02\x12&\n" +
	"\"PARTITION_STRATEGY_CONSISTENT_HASH\x10\x03\x12\x1d\n" +
	"\x19PARTITION_STRATEGY_CUSTOM\x10c*\x8e\x01\n" +
	"\x0fRebalancePolicy\x12 \n" +
	"\x1cREBALANCE_POLICY_UNSPECIFIED\x10\x00\x12\x19\n" +
	"\x15REBALANCE_POLICY_NONE\x10\x01\x12\x1d\n" +
	"\x19REBALANCE_POLICY_ON_SCALE\x10\x02\x12\x1f\n" +
	"\x1bREBALANCE_POLICY_LOAD_BASED\x10\x03*\xf4\x01\n" +
	"\x0fShardGroupState\x12!\n" +
	"\x1dSHARD_GROUP_STATE_UNSPECIFIED\x10\x00\x12\x1e\n" +
	"\x1aSHARD_GROUP_STATE_CREATING\x10\x01\x12\x1c\n" +
	"\x18SHARD_GROUP_STATE_ACTIVE\x10\x02\x12!\n" +
	"\x1dSHARD_GROUP_STATE_REBALANCING\x10\x03\x12\x1e\n" +
	"\x1aSHARD_GROUP_STATE_DRAINING\x10\x04\x12\x1e\n" +
	"\x1aSHARD_GROUP_STATE_STOPPING\x10\x05\x12\x1d\n" +
	"\x19SHARD_GROUP_STATE_STOPPED\x10\x06*\xb8\x01\n" +
	"\x15NodePlacementStrategy\x12'\n" +
	"#NODE_PLACEMENT_STRATEGY_UNSPECIFIED\x10\x00\x12%\n" +
	"!NODE_PLACEMENT_STRATEGY_SAME_NODE\x10\x01\x12)\n" +
	"%NODE_PLACEMENT_STRATEGY_FROM_REGISTRY\x10\x02\x12$\n" +
	" NODE_PLACEMENT_STRATEGY_NODE_IDS\x10\x03*\xd8\x01\n" +
	"\x1dShardGroupAggregationStrategy\x12'\n" +
	"#SHARD_GROUP_AGGREGATION_UNSPECIFIED\x10\x00\x12\"\n" +
	"\x1eSHARD_GROUP_AGGREGATION_CONCAT\x10\x01\x12!\n" +
	"\x1dSHARD_GROUP_AGGREGATION_MERGE\x10\x02\x12!\n" +
	"\x1dSHARD_GROUP_AGGREGATION_FIRST\x10\x03\x12$\n" +
	" SHARD_GROUP_AGGREGATION_MAJORITY\x10\x04*\x9d\x02\n" +
	"\x13CollectiveReduction\x12$\n" +
	" COLLECTIVE_REDUCTION_UNSPECIFIED\x10\x00\x12\x1c\n" +
	"\x18COLLECTIVE_REDUCTION_SUM\x10\x01\x12\x1c\n" +
	"\x18COLLECTIVE_REDUCTION_MIN\x10\x02\x12\x1c\n" +
	"\x18COLLECTIVE_REDUCTION_MAX\x10\x03\x12 \n" +
	"\x1cCOLLECTIVE_REDUCTION_PRODUCT\x10\x04\x12\x1f\n" +
	"\x1bCOLLECTIVE_REDUCTION_CONCAT\x10\x05\x12!\n" +
	"\x1dCOLLECTIVE_REDUCTION_BOOL_AND\x10\x06\x12 \n" +
	"\x1cCOLLECTIVE_REDUCTION_BOOL_OR\x10\a*n\n" +
	"\rStateMgmtMode\x12\x1f\n" +
	"\x1bSTATE_MGMT_MODE_UNSPECIFIED\x10\x00\x12\x1f\n" +
	"\x1bSTATE_MGMT_MODE_TRADITIONAL\x10\x01\x12\x1b\n" +
	"\x17STATE_MGMT_MODE_LATTICE\x10\x02*\xbd\x01\n" +
	"\x10ConsistencyLevel\x12!\n" +
	"\x1dCONSISTENCY_LEVEL_UNSPECIFIED\x10\x00\x12\x1e\n" +
	"\x1aCONSISTENCY_LEVEL_EVENTUAL\x10\x01\x12\x1c\n" +
	"\x18CONSISTENCY_LEVEL_CAUSAL\x10\x02\x12$\n" +
	" CONSISTENCY_LEVEL_READ_COMMITTED\x10\x03\x12\"\n" +
	"\x1eCONSISTENCY_LEVEL_LINEARIZABLE\x10\x04*\x8e\x01\n" +
	"\x0fActorVisibility\x12 \n" +
	"\x1cACTOR_VISIBILITY_UNSPECIFIED\x10\x00\x12\x1b\n" +
	"\x17ACTOR_VISIBILITY_PUBLIC\x10\x01\x12\x1e\n" +
	"\x1aACTOR_VISIBILITY_PROTECTED\x10\x02\x12\x1c\n" +
	"\x18ACTOR_VISIBILITY_PRIVATE\x10\x03*\\\n" +
	"\vMonitorType\x12\x1c\n" +
	"\x18MONITOR_TYPE_UNSPECIFIED\x10\x00\x12\x18\n" +
	"\x14MONITOR_TYPE_MONITOR\x10\x01\x12\x15\n" +
	"\x11MONITOR_TYPE_LINK\x10\x02*\xda\x02\n" +
	"\x12LifecycleEventType\x12$\n" +
	" LIFECYCLE_EVENT_TYPE_UNSPECIFIED\x10\x00\x12 \n" +
	"\x1cLIFECYCLE_EVENT_TYPE_CREATED\x10\x01\x12!\n" +
	"\x1dLIFECYCLE_EVENT_TYPE_STARTING\x10\x02\x12\"\n" +
	"\x1eLIFECYCLE_EVENT_TYPE_ACTIVATED\x10\x03\x12%\n" +
	"!LIFECYCLE_EVENT_TYPE_DEACTIVATING\x10\x04\x12$\n" +
	" LIFECYCLE_EVENT_TYPE_DEACTIVATED\x10\x05\x12#\n" +
	"\x1fLIFECYCLE_EVENT_TYPE_TERMINATED\x10\x06\x12\x1f\n" +
	"\x1bLIFECYCLE_EVENT_TYPE_FAILED\x10\a\x12\"\n" +
	"\x1eLIFECYCLE_EVENT_TYPE_MIGRATING\x10\b*z\n" +
	"\n" +
	"DropPolicy\x12\x1b\n" +
	"\x17DROP_POLICY_UNSPECIFIED\x10\x00\x12\x1b\n" +
	"\x17DROP_POLICY_DROP_OLDEST\x10\x01\x12\x1b\n" +
	"\x17DROP_POLICY_DROP_NEWEST\x10\x02\x12\x15\n" +
	"\x11DROP_POLICY_BLOCK\x10\x03*\xae\x02\n" +
	"\x11ActorRefErrorCode\x12$\n" +
	" ACTOR_REF_ERROR_CODE_UNSPECIFIED\x10\x00\x12(\n" +
	"$ACTOR_REF_ERROR_CODE_ACTOR_NOT_FOUND\x10\x01\x12$\n" +
	" ACTOR_REF_ERROR_CODE_SEND_FAILED\x10\x02\x12%\n" +
	"!ACTOR_REF_ERROR_CODE_MAILBOX_FULL\x10\x03\x12)\n" +
	"%ACTOR_REF_ERROR_CODE_ACTOR_TERMINATED\x10\x04\x12 \n" +
	"\x1cACTOR_REF_ERROR_CODE_TIMEOUT\x10\x05\x12/\n" +
	"+ACTOR_REF_ERROR_CODE_REMOTE_NOT_IMPLEMENTED\x10\x06*\xe8\x01\n" +
	"\x0fResourceProfile\x12 \n" +
	"\x1cRESOURCE_PROFILE_UNSPECIFIED\x10\x00\x12\"\n" +
	"\x1eRESOURCE_PROFILE_CPU_INTENSIVE\x10\x01\x12%\n" +
	"!RESOURCE_PROFILE_MEMORY_INTENSIVE\x10\x02\x12!\n" +
	"\x1dRESOURCE_PROFILE_IO_INTENSIVE\x10\x03\x12&\n" +
	"\"RESOURCE_PROFILE_NETWORK_INTENSIVE\x10\x04\x12\x1d\n" +
	"\x19RESOURCE_PROFILE_BALANCED\x10\x05*\xee\x01\n" +
	"\x15ResourceViolationCode\x12'\n" +
	"#RESOURCE_VIOLATION_CODE_UNSPECIFIED\x10\x00\x12(\n" +
	"$RESOURCE_VIOLATION_CODE_CPU_EXCEEDED\x10\x01\x12+\n" +
	"'RESOURCE_VIOLATION_CODE_MEMORY_EXCEEDED\x10\x02\x12'\n" +
	"#RESOURCE_VIOLATION_CODE_IO_EXCEEDED\x10\x03\x12,\n" +
	"(RESOURCE_VIOLATION_CODE_NETWORK_EXCEEDED\x10\x04*\xba\x01\n" +
	"\x11ActorHealthStatus\x12#\n" +
	"\x1fACTOR_HEALTH_STATUS_UNSPECIFIED\x10\x00\x12\x1f\n" +
	"\x1bACTOR_HEALTH_STATUS_HEALTHY\x10\x01\x12 \n" +
	"\x1cACTOR_HEALTH_STATUS_DEGRADED\x10\x02\x12\x1d\n" +
	"\x19ACTOR_HEALTH_STATUS_STUCK\x10\x03\x12\x1e\n" +
	"\x1aACTOR_HEALTH_STATUS_FAILED\x10\x042\x9f4\n" +
	"\fActorService\x12\xa1\x02\n" +
	"\n" +
	"SpawnActor\x12&.plexspaces.actor.v1.SpawnActorRequest\x1a'.plexspaces.actor.v1.SpawnActorResponse\"\xc1\x01\x92A\x9e\x01\n" +
	"\x06Actors\x12\vSpawn Actor\x1a\x86\x01Spawn actor on the node receiving this request (target node implicit from endpoint). gRPC is already remote, so 'remote' is redundant.\x82\xd3\xe4\x93\x02\x19:\x01*\"\x14/api/v1/actors/spawn\x12\x94\x02\n" +
	"\vSpawnActors\x12'.plexspaces.actor.v1.SpawnActorsRequest\x1a(.plexspaces.actor.v1.SpawnActorsResponse\"\xb1\x01\x92A\x88\x01\n" +
	"\x06Actors\x12\fSpawn Actors\x1apSpawn multiple actors on the node receiving this request using the same canonical spawn semantics as SpawnActor.\x82\xd3\xe4\x93\x02\x1f:\x01*\"\x1a/api/v1/actors/spawn-batch\x12\xad\x01\n" +
	"\bGetActor\x12$.plexspaces.actor.v1.GetActorRequest\x1a%.plexspaces.actor.v1.GetActorResponse\"T\x92A0\n" +
	"\x06Actors\x12\tGet Actor\x1a\x1bRetrieve an actor by its ID\x82\xd3\xe4\x93\x02\x1b\x12\x19/api/v1/actors/{actor_id}\x12\xb2\x01\n" +
	"\n" +
	"ListActors\x12&.plexspaces.actor.v1.ListActorsRequest\x1a'.plexspaces.actor.v1.ListActorsResponse\"S\x92A:\n" +
	"\x06Actors\x12\vList Actors\x1a#List actors with optional filtering\x82\xd3\xe4\x93\x02\x10\x12\x0e/api/v1/actors\x12\x8d\x02\n" +
	"\vSendMessage\x12'.plexspaces.actor.v1.SendMessageRequest\x1a(.plexspaces.actor.v1.SendMessageResponse\"\xaa\x01\x92AG\n" +
	"\bMessages\x12\fSend Message\x1a-Send a tell message to an actor or actor type\x82\xd3\xe4\x93\x02Z:\x01*Z,:\x01*\x1a'/api/v1/actors/{namespace}/{actor_type}\"'/api/v1/actors/{namespace}/{actor_type}\x12\xc8\x01\n" +
	"\x0eStreamMessages\x12).plexspaces.actor.v1.StreamMessageRequest\x1a*.plexspaces.actor.v1.StreamMessageResponse\"[\x92AX\n" +
	"\bMessages\x12\x0fStream Messages\x1a;Bidirectional streaming for high-throughput message passing(\x010\x01\x12\xa9\x01\n" +
	"\vDeleteActor\x12'.plexspaces.actor.v1.DeleteActorRequest\x1a\x1b.plexspaces.common.v1.Empty\"T\x92A0\n" +
	"\x06Actors\x12\fDelete Actor\x1a\x18Delete an actor instance\x82\xd3\xe4\x93\x02\x1b*\x19/api/v1/actors/{actor_id}\x12\xb7\x01\n" +
	"\fMonitorActor\x12(.plexspaces.actor.v1.MonitorActorRequest\x1a).plexspaces.actor.v1.MonitorActorResponse\"R\x92AO\n" +
	"\vSupervision\x12\rMonitor Actor\x1a1Establish monitoring link to actor (Erlang-style)\x12\xb6\x01\n" +
	"\x0eDemonitorActor\x12*.plexspaces.actor.v1.DemonitorActorRequest\x1a\x1b.plexspaces.common.v1.Empty\"[\x92AX\n" +
	"\vSupervision\x12\x0fDemonitor Actor\x1a8Cancel a monitor on the node hosting the monitored actor\x12\xe1\x01\n" +
	"\tLinkActor\x12%.plexspaces.actor.v1.LinkActorRequest\x1a&.plexspaces.actor.v1.LinkActorResponse\"\x84\x01\x92AW\n" +
	"\vSupervision\x12\n" +
	"Link Actor\x1a<Create bidirectional link between two actors (Erlang link/1)\x82\xd3\xe4\x93\x02$:\x01*\"\x1f/api/v1/actors/{actor_id}/links\x12\xfa\x01\n" +
	"\vUnlinkActor\x12'.plexspaces.actor.v1.UnlinkActorRequest\x1a(.plexspaces.actor.v1.UnlinkActorResponse\"\x97\x01\x92A[\n" +
	"\vSupervision\x12\fUnlink Actor\x1a>Remove bidirectional link between two actors (Erlang unlink/1)\x82\xd3\xe4\x93\x023*1/api/v1/actors/{actor_id}/links/{linked_actor_id}\x12\xbc\x01\n" +
	"\x0fNotifyActorDown\x12*.plexspaces.actor.v1.ActorDownNotification\x1a\x1b.plexspaces.common.v1.Empty\"`\x92A]\n" +
	"\vSupervision\x12\x11Notify Actor Down\x1a;Internal: Notify supervisor that monitored actor terminated\x12\xf3\x01\n" +
	"\x10CheckActorExists\x12,.plexspaces.actor.v1.CheckActorExistsRequest\x1a-.plexspaces.actor.v1.CheckActorExistsResponse\"\x81\x01\x92AV\n" +
	"\x0eVirtual Actors\x12\x12Check Actor Exists\x1a0Check if virtual actor exists without activating\x82\xd3\xe4\x93\x02\"\x12 /api/v1/actors/{actor_id}/exists\x12\xd8\x01\n" +
	"\x0eGetActorStates\x12*.plexspaces.actor.v1.GetActorStatesRequest\x1a+.plexspaces.actor.v1.GetActorStatesResponse\"m\x92AJ\n" +
	"\x06Actors\x12\x10Get Actor States\x1a.Batch check actor states for monitoring and GC\x82\xd3\xe4\x93\x02\x1a:\x01*\"\x15/api/v1/actors/states\x12\xe5\x02\n" +
	"\bAskReply\x12$.plexspaces.actor.v1.AskReplyRequest\x1a%.plexspaces.actor.v1.AskReplyResponse\"\x8b\x02\x92AE\n" +
	"\x06Actors\x12\tAsk Reply\x1a0Ask an actor via HTTP GET/POST/PUT ask endpoints\x82\xd3\xe4\x93\x02\xbc\x01Z-\x12+/api/v1/actors/{namespace}/{actor_type}/askZ0:\x01*\"+/api/v1/actors/{namespace}/{actor_type}/askZ0:\x01*\x1a+/api/v1/actors/{namespace}/{actor_type}/ask\x12'/api/v1/actors/{namespace}/{actor_type}\x12\xdf\x01\n" +
	"\x10CreateShardGroup\x12,.plexspaces.actor.v1.CreateShardGroupRequest\x1a-.plexspaces.actor.v1.CreateShardGroupResponse\"n\x92AL\n" +
	"\vShardGroups\x12\x12Create Shard Group\x1a)Create a shard group and spawn its shards\x82\xd3\xe4\x93\x02\x19:\x01*\"\x14/api/v1/shard-groups\x12\xe5\x01\n" +
	"\x10DeleteShardGroup\x12,.plexspaces.actor.v1.DeleteShardGroupRequest\x1a\x1b.plexspaces.common.v1.Empty\"\x85\x01\x92A[\n" +
	"\vShardGroups\x12\x12Delete Shard Group\x1a8Delete a shard group and optionally force shard shutdown\x82\xd3\xe4\x93\x02!*\x1f/api/v1/shard-groups/{group_id}\x12\xdf\x01\n" +
	"\rGetShardGroup\x12).plexspaces.actor.v1.GetShardGroupRequest\x1a*.plexspaces.actor.v1.GetShardGroupResponse\"w\x92AM\n" +
	"\vShardGroups\x12\x0fGet Shard Group\x1a-Get shard group metadata and shard membership\x82\xd3\xe4\x93\x02!\x12\x1f/api/v1/shard-groups/{group_id}\x12\xeb\x01\n" +
	"\x0fListShardGroups\x12+.plexspaces.actor.v1.ListShardGroupsRequest\x1a,.plexspaces.actor.v1.ListShardGroupsResponse\"}\x92A^\n" +
	"\vShardGroups\x12\x11List Shard Groups\x1a<List shard groups with optional actor-type and state filters\x82\xd3\xe4\x93\x02\x16\x12\x14/api/v1/shard-groups\x12\xeb\x01\n" +
	"\x0fScaleShardGroup\x12+.plexspaces.actor.v1.ScaleShardGroupRequest\x1a,.plexspaces.actor.v1.ScaleShardGroupResponse\"}\x92AJ\n" +
	"\vShardGroups\x12\x11Scale Shard Group\x1a(Scale a shard group to a new shard count\x82\xd3\xe4\x93\x02*:\x01*\"%/api/v1/shard-groups/{group_id}:scale\x12\xe7\x01\n" +
	"\vSendToShard\x12'.plexspaces.actor.v1.SendToShardRequest\x1a(.plexspaces.actor.v1.SendToShardResponse\"\x84\x01\x92AR\n" +
	"\vShardGroups\x12\rSend To Shard\x1a4Route a message to a shard selected by partition key\x82\xd3\xe4\x93\x02):\x01*\"$/api/v1/shard-groups/{group_id}:send\x12\x87\x02\n" +
	"\x13BroadcastShardGroup\x12/.plexspaces.actor.v1.BroadcastShardGroupRequest\x1a0.plexspaces.actor.v1.BroadcastShardGroupResponse\"\x8c\x01\x92AU\n" +
	"\vShardGroups\x12\x15Broadcast Shard Group\x1a/Broadcast a message to every shard in the group\x82\xd3\xe4\x93\x02.:\x01*\")/api/v1/shard-groups/{group_id}:broadcast\x12\x82\x02\n" +
	"\x10ReduceShardGroup\x12,.plexspaces.actor.v1.ReduceShardGroupRequest\x1a-.plexspaces.actor.v1.ReduceShardGroupResponse\"\x90\x01\x92A\\\n" +
	"\vShardGroups\x12\x12Reduce Shard Group\x1a9Run a shard-group reduction and return the reduced result\x82\xd3\xe4\x93\x02+:\x01*\"&/api/v1/shard-groups/{group_id}:reduce\x12\x81\x02\n" +
	"\x13AllReduceShardGroup\x12/.plexspaces.actor.v1.AllReduceShardGroupRequest\x1a0.plexspaces.actor.v1.AllReduceShardGroupResponse\"\x86\x01\x92AO\n" +
	"\vShardGroups\x12\x16All Reduce Shard Group\x1a(Run an all-reduce across the shard group\x82\xd3\xe4\x93\x02.:\x01*\")/api/v1/shard-groups/{group_id}:allReduce\x12\xf2\x01\n" +
	"\x11BarrierShardGroup\x12-.plexspaces.actor.v1.BarrierShardGroupRequest\x1a..plexspaces.actor.v1.BarrierShardGroupResponse\"~\x92AI\n" +
	"\vShardGroups\x12\x13Barrier Shard Group\x1a%Synchronize shards in a barrier round\x82\xd3\xe4\x93\x02,:\x01*\"'/api/v1/shard-groups/{group_id}:barrier\x12\xf9\x01\n" +
	"\rScatterGather\x12).plexspaces.actor.v1.ScatterGatherRequest\x1a*.plexspaces.actor.v1.ScatterGatherResponse\"\x90\x01\x92AU\n" +
	"\vShardGroups\x12\x0eScatter Gather\x1a6Send a query to all shards and aggregate the responses\x82\xd3\xe4\x93\x022:\x01*\"-/api/v1/shard-groups/{group_id}:scatterGather\x12\x95\x02\n" +
	"\x14BulkUpdateShardGroup\x120.plexspaces.actor.v1.BulkUpdateShardGroupRequest\x1a1.plexspaces.actor.v1.BulkUpdateShardGroupResponse\"\x97\x01\x92A_\n" +
	"\vShardGroups\x12\x17Bulk Update Shard Group\x1a7Apply multiple partitioned updates across a shard group\x82\xd3\xe4\x93\x02/:\x01*\"*/api/v1/shard-groups/{group_id}:bulkUpdate\x12\xe3\x01\n" +
	"\rMapShardGroup\x12).plexspaces.actor.v1.MapShardGroupRequest\x1a*.plexspaces.actor.v1.MapShardGroupResponse\"{\x92AJ\n" +
	"\vShardGroups\x12\x0fMap Shard Group\x1a*Apply a function to all shards in parallel\x82\xd3\xe4\x93\x02(:\x01*\"#/api/v1/shard-groups/{group_id}:map\x1a(\x92A%\x12#Service for managing durable actors2\xee\x03\n" +
	"\x15LifecycleEventChannel\x12\xd8\x01\n" +
	"\x18SubscribeLifecycleEvents\x12).plexspaces.actor.v1.LifecycleEventFilter\x1a(.plexspaces.actor.v1.ActorLifecycleEvent\"e\x92Ab\n" +
	"\rObservability\x12\x1dSubscribe to Lifecycle Events\x1a2Stream of actor lifecycle events for observability0\x01\x12\xc1\x01\n" +
	"\x15PublishLifecycleEvent\x12(.plexspaces.actor.v1.ActorLifecycleEvent\x1a\x1b.plexspaces.common.v1.Empty\"a\x92A^\n" +
	"\rObservability\x12\x17Publish Lifecycle Event\x1a4Internal: Publish lifecycle event to all subscribers\x1a6\x92A3\x121Event streaming for actor lifecycle observabilityB\xf6\x02\x92A\x80\x01\x12W\n" +
	"\x1bPlexSpace Actor Runtime API\x123API for managing durable actors and their lifecycle2\x031.0*\x01\x022\x10application/json:\x10application/json\n" +
	"\x17com.plexspaces.actor.v1B\x11ActorRuntimeProtoP\x01ZVgithub.com/plexobject/plexspaces/sdks/go/plexspaces/proto/plexspaces/v1/actors;actorv1\xa2\x02\x03PAX\xaa\x02\x13Plexspaces.Actor.V1\xca\x02\x13Plexspaces\\Actor\\V1\xe2\x02\x1fPlexspaces\\Actor\\V1\\GPBMetadata\xea\x02\x15Plexspaces::Actor::V1b\x06proto3"

var (
	file_plexspaces_v1_actors_actor_runtime_proto_rawDescOnce sync.Once
	file_plexspaces_v1_actors_actor_runtime_proto_rawDescData []byte
)

func file_plexspaces_v1_actors_actor_runtime_proto_rawDescGZIP() []byte {
	file_plexspaces_v1_actors_actor_runtime_proto_rawDescOnce.Do(func() {
		file_plexspaces_v1_actors_actor_runtime_proto_rawDescData = protoimpl.X.CompressGZIP(unsafe.Slice(unsafe.StringData(file_plexspaces_v1_actors_actor_runtime_proto_rawDesc), len(file_plexspaces_v1_actors_actor_runtime_proto_rawDesc)))
	})
	return file_plexspaces_v1_actors_actor_runtime_proto_rawDescData
}

var file_plexspaces_v1_actors_actor_runtime_proto_enumTypes = make([]protoimpl.EnumInfo, 18)
var file_plexspaces_v1_actors_actor_runtime_proto_msgTypes = make([]protoimpl.MessageInfo, 103)
var file_plexspaces_v1_actors_actor_runtime_proto_goTypes = []any{
	(PlacementStrategy)(0),               // 0: plexspaces.actor.v1.PlacementStrategy
	(LoadBalancingStrategy)(0),           // 1: plexspaces.actor.v1.LoadBalancingStrategy
	(PartitionStrategy)(0),               // 2: plexspaces.actor.v1.PartitionStrategy
	(RebalancePolicy)(0),                 // 3: plexspaces.actor.v1.RebalancePolicy
	(ShardGroupState)(0),                 // 4: plexspaces.actor.v1.ShardGroupState
	(NodePlacementStrategy)(0),           // 5: plexspaces.actor.v1.NodePlacementStrategy
	(ShardGroupAggregationStrategy)(0),   // 6: plexspaces.actor.v1.ShardGroupAggregationStrategy
	(CollectiveReduction)(0),             // 7: plexspaces.actor.v1.CollectiveReduction
	(StateMgmtMode)(0),                   // 8: plexspaces.actor.v1.StateMgmtMode
	(ConsistencyLevel)(0),                // 9: plexspaces.actor.v1.ConsistencyLevel
	(ActorVisibility)(0),                 // 10: plexspaces.actor.v1.ActorVisibility
	(MonitorType)(0),                     // 11: plexspaces.actor.v1.MonitorType
	(LifecycleEventType)(0),              // 12: plexspaces.actor.v1.LifecycleEventType
	(DropPolicy)(0),                      // 13: plexspaces.actor.v1.DropPolicy
	(ActorRefErrorCode)(0),               // 14: plexspaces.actor.v1.ActorRefErrorCode
	(ResourceProfile)(0),                 // 15: plexspaces.actor.v1.ResourceProfile
	(ResourceViolationCode)(0),           // 16: plexspaces.actor.v1.ResourceViolationCode
	(ActorHealthStatus)(0),               // 17: plexspaces.actor.v1.ActorHealthStatus
	(*Actor)(nil),                        // 18: plexspaces.actor.v1.Actor
	(*ActorConfig)(nil),                  // 19: plexspaces.actor.v1.ActorConfig
	(*ResourceRequirements)(nil),         // 20: plexspaces.actor.v1.ResourceRequirements
	(*ActorResourceRequirements)(nil),    // 21: plexspaces.actor.v1.ActorResourceRequirements
	(*StatelessWorkerConfig)(nil),        // 22: plexspaces.actor.v1.StatelessWorkerConfig
	(*DataParallelConfig)(nil),           // 23: plexspaces.actor.v1.DataParallelConfig
	(*ShardGroup)(nil),                   // 24: plexspaces.actor.v1.ShardGroup
	(*RebalanceStatus)(nil),              // 25: plexspaces.actor.v1.RebalanceStatus
	(*NodePlacement)(nil),                // 26: plexspaces.actor.v1.NodePlacement
	(*CreateShardGroupRequest)(nil),      // 27: plexspaces.actor.v1.CreateShardGroupRequest
	(*CreateShardGroupResponse)(nil),     // 28: plexspaces.actor.v1.CreateShardGroupResponse
	(*DeleteShardGroupRequest)(nil),      // 29: plexspaces.actor.v1.DeleteShardGroupRequest
	(*GetShardGroupRequest)(nil),         // 30: plexspaces.actor.v1.GetShardGroupRequest
	(*GetShardGroupResponse)(nil),        // 31: plexspaces.actor.v1.GetShardGroupResponse
	(*ListShardGroupsRequest)(nil),       // 32: plexspaces.actor.v1.ListShardGroupsRequest
	(*ListShardGroupsResponse)(nil),      // 33: plexspaces.actor.v1.ListShardGroupsResponse
	(*SendToShardRequest)(nil),           // 34: plexspaces.actor.v1.SendToShardRequest
	(*SendToShardResponse)(nil),          // 35: plexspaces.actor.v1.SendToShardResponse
	(*ScatterGatherRequest)(nil),         // 36: plexspaces.actor.v1.ScatterGatherRequest
	(*ShardQueryResponse)(nil),           // 37: plexspaces.actor.v1.ShardQueryResponse
	(*ScatterGatherStats)(nil),           // 38: plexspaces.actor.v1.ScatterGatherStats
	(*ScatterGatherResponse)(nil),        // 39: plexspaces.actor.v1.ScatterGatherResponse
	(*BulkUpdateShardGroupRequest)(nil),  // 40: plexspaces.actor.v1.BulkUpdateShardGroupRequest
	(*BulkUpdateShardGroupResponse)(nil), // 41: plexspaces.actor.v1.BulkUpdateShardGroupResponse
	(*ShardUpdateStats)(nil),             // 42: plexspaces.actor.v1.ShardUpdateStats
	(*MapShardGroupRequest)(nil),         // 43: plexspaces.actor.v1.MapShardGroupRequest
	(*MapShardGroupResponse)(nil),        // 44: plexspaces.actor.v1.MapShardGroupResponse
	(*CollectiveTargetField)(nil),        // 45: plexspaces.actor.v1.CollectiveTargetField
	(*BroadcastShardGroupRequest)(nil),   // 46: plexspaces.actor.v1.BroadcastShardGroupRequest
	(*BroadcastShardGroupResponse)(nil),  // 47: plexspaces.actor.v1.BroadcastShardGroupResponse
	(*ReduceShardGroupRequest)(nil),      // 48: plexspaces.actor.v1.ReduceShardGroupRequest
	(*ReduceShardGroupResponse)(nil),     // 49: plexspaces.actor.v1.ReduceShardGroupResponse
	(*AllReduceShardGroupRequest)(nil),   // 50: plexspaces.actor.v1.AllReduceShardGroupRequest
	(*AllReduceShardGroupResponse)(nil),  // 51: plexspaces.actor.v1.AllReduceShardGroupResponse
	(*BarrierShardGroupRequest)(nil),     // 52: plexspaces.actor.v1.BarrierShardGroupRequest
	(*BarrierShardGroupResponse)(nil),    // 53: plexspaces.actor.v1.BarrierShardGroupResponse
	(*ScaleShardGroupRequest)(nil),       // 54: plexspaces.actor.v1.ScaleShardGroupRequest
	(*ScaleShardGroupResponse)(nil),      // 55: plexspaces.actor.v1.ScaleShardGroupResponse
	(*ActorMetrics)(nil),                 // 56: plexspaces.actor.v1.ActorMetrics
	(*SpawnActorRequest)(nil),            // 57: plexspaces.actor.v1.SpawnActorRequest
	(*SpawnActorResponse)(nil),           // 58: plexspaces.actor.v1.SpawnActorResponse
	(*SpawnActorsRequest)(nil),           // 59: plexspaces.actor.v1.SpawnActorsRequest
	(*SpawnActorResult)(nil),             // 60: plexspaces.actor.v1.SpawnActorResult
	(*SpawnActorsResponse)(nil),          // 61: plexspaces.actor.v1.SpawnActorsResponse
	(*GetActorRequest)(nil),              // 62: plexspaces.actor.v1.GetActorRequest
	(*GetActorResponse)(nil),             // 63: plexspaces.actor.v1.GetActorResponse
	(*ListActorsRequest)(nil),            // 64: plexspaces.actor.v1.ListActorsRequest
	(*ListActorsResponse)(nil),           // 65: plexspaces.actor.v1.ListActorsResponse
	(*SendMessageRequest)(nil),           // 66: plexspaces.actor.v1.SendMessageRequest
	(*SendMessageResponse)(nil),          // 67: plexspaces.actor.v1.SendMessageResponse
	(*StreamMessageRequest)(nil),         // 68: plexspaces.actor.v1.StreamMessageRequest
	(*StreamMessageResponse)(nil),        // 69: plexspaces.actor.v1.StreamMessageResponse
	(*DeleteActorRequest)(nil),           // 70: plexspaces.actor.v1.DeleteActorRequest
	(*ActorLifecycleEvent)(nil),          // 71: plexspaces.actor.v1.ActorLifecycleEvent
	(*ActorCreated)(nil),                 // 72: plexspaces.actor.v1.ActorCreated
	(*ActorStarting)(nil),                // 73: plexspaces.actor.v1.ActorStarting
	(*ActorActivated)(nil),               // 74: plexspaces.actor.v1.ActorActivated
	(*ActorDeactivating)(nil),            // 75: plexspaces.actor.v1.ActorDeactivating
	(*ActorDeactivated)(nil),             // 76: plexspaces.actor.v1.ActorDeactivated
	(*ActorTerminated)(nil),              // 77: plexspaces.actor.v1.ActorTerminated
	(*ActorFailed)(nil),                  // 78: plexspaces.actor.v1.ActorFailed
	(*ActorMigrating)(nil),               // 79: plexspaces.actor.v1.ActorMigrating
	(*MonitorActorRequest)(nil),          // 80: plexspaces.actor.v1.MonitorActorRequest
	(*MonitorActorResponse)(nil),         // 81: plexspaces.actor.v1.MonitorActorResponse
	(*DemonitorActorRequest)(nil),        // 82: plexspaces.actor.v1.DemonitorActorRequest
	(*ActorDownNotification)(nil),        // 83: plexspaces.actor.v1.ActorDownNotification
	(*GetActorStatesRequest)(nil),        // 84: plexspaces.actor.v1.GetActorStatesRequest
	(*GetActorStatesResponse)(nil),       // 85: plexspaces.actor.v1.GetActorStatesResponse
	(*ActorLink)(nil),                    // 86: plexspaces.actor.v1.ActorLink
	(*LinkActorRequest)(nil),             // 87: plexspaces.actor.v1.LinkActorRequest
	(*LinkActorResponse)(nil),            // 88: plexspaces.actor.v1.LinkActorResponse
	(*UnlinkActorRequest)(nil),           // 89: plexspaces.actor.v1.UnlinkActorRequest
	(*UnlinkActorResponse)(nil),          // 90: plexspaces.actor.v1.UnlinkActorResponse
	(*CheckActorExistsRequest)(nil),      // 91: plexspaces.actor.v1.CheckActorExistsRequest
	(*CheckActorExistsResponse)(nil),     // 92: plexspaces.actor.v1.CheckActorExistsResponse
	(*AskReplyRequest)(nil),              // 93: plexspaces.actor.v1.AskReplyRequest
	(*AskReplyResponse)(nil),             // 94: plexspaces.actor.v1.AskReplyResponse
	(*LifecycleEventFilter)(nil),         // 95: plexspaces.actor.v1.LifecycleEventFilter
	(*VirtualActorLifecycle)(nil),        // 96: plexspaces.actor.v1.VirtualActorLifecycle
	(*VirtualActorConfig)(nil),           // 97: plexspaces.actor.v1.VirtualActorConfig
	(*ActorRefError)(nil),                // 98: plexspaces.actor.v1.ActorRefError
	(*ResourceContract)(nil),             // 99: plexspaces.actor.v1.ResourceContract
	(*ResourceUsage)(nil),                // 100: plexspaces.actor.v1.ResourceUsage
	(*ResourceViolation)(nil),            // 101: plexspaces.actor.v1.ResourceViolation
	(*ActorHealth)(nil),                  // 102: plexspaces.actor.v1.ActorHealth
	(*ActorSpawnSpec)(nil),               // 103: plexspaces.actor.v1.ActorSpawnSpec
	nil,                                  // 104: plexspaces.actor.v1.ActorConfig.PropertiesEntry
	nil,                                  // 105: plexspaces.actor.v1.ResourceRequirements.CustomRequirementsEntry
	nil,                                  // 106: plexspaces.actor.v1.ShardGroup.MetadataEntry
	nil,                                  // 107: plexspaces.actor.v1.NodePlacement.RequiredLabelsEntry
	nil,                                  // 108: plexspaces.actor.v1.NodePlacement.AffinityLabelsEntry
	nil,                                  // 109: plexspaces.actor.v1.CreateShardGroupRequest.MetadataEntry
	nil,                                  // 110: plexspaces.actor.v1.BulkUpdateShardGroupRequest.UpdatesEntry
	nil,                                  // 111: plexspaces.actor.v1.SendMessageRequest.HeadersEntry
	nil,                                  // 112: plexspaces.actor.v1.SendMessageRequest.QueryParamsEntry
	nil,                                  // 113: plexspaces.actor.v1.GetActorStatesResponse.StatesEntry
	nil,                                  // 114: plexspaces.actor.v1.ActorLink.MetadataEntry
	nil,                                  // 115: plexspaces.actor.v1.AskReplyRequest.HeadersEntry
	nil,                                  // 116: plexspaces.actor.v1.AskReplyRequest.QueryParamsEntry
	nil,                                  // 117: plexspaces.actor.v1.AskReplyResponse.HeadersEntry
	nil,                                  // 118: plexspaces.actor.v1.LifecycleEventFilter.RequiredTagsEntry
	nil,                                  // 119: plexspaces.actor.v1.ActorSpawnSpec.ArgsEntry
	nil,                                  // 120: plexspaces.actor.v1.ActorSpawnSpec.LabelsEntry
	(ActorState)(0),                      // 121: plexspaces.actor.v1.ActorState
	(*v1.Metadata)(nil),                  // 122: plexspaces.common.v1.Metadata
	(*v1.Facet)(nil),                     // 123: plexspaces.common.v1.Facet
	(*durationpb.Duration)(nil),          // 124: google.protobuf.Duration
	(*v1.RetryPolicy)(nil),               // 125: plexspaces.common.v1.RetryPolicy
	(supervision.SupervisionStrategy)(0), // 126: plexspaces.supervision.v1.SupervisionStrategy
	(*timestamppb.Timestamp)(nil),        // 127: google.protobuf.Timestamp
	(*v1.ResourceSpec)(nil),              // 128: plexspaces.common.v1.ResourceSpec
	(*v1.PageRequest)(nil),               // 129: plexspaces.common.v1.PageRequest
	(*v1.PageResponse)(nil),              // 130: plexspaces.common.v1.PageResponse
	(*v1.Message)(nil),                   // 131: plexspaces.common.v1.Message
	(v1.ActivationStrategy)(0),           // 132: plexspaces.common.v1.ActivationStrategy
	(*v1.ActorIdentity)(nil),             // 133: plexspaces.common.v1.ActorIdentity
	(*anypb.Any)(nil),                    // 134: google.protobuf.Any
	(*v1.Empty)(nil),                     // 135: plexspaces.common.v1.Empty
}
var file_plexspaces_v1_actors_actor_runtime_proto_depIdxs = []int32{
	121, // 0: plexspaces.actor.v1.Actor.state:type_name -> plexspaces.actor.v1.ActorState
	122, // 1: plexspaces.actor.v1.Actor.metadata:type_name -> plexspaces.common.v1.Metadata
	19,  // 2: plexspaces.actor.v1.Actor.config:type_name -> plexspaces.actor.v1.ActorConfig
	56,  // 3: plexspaces.actor.v1.Actor.metrics:type_name -> plexspaces.actor.v1.ActorMetrics
	123, // 4: plexspaces.actor.v1.Actor.facets:type_name -> plexspaces.common.v1.Facet
	124, // 5: plexspaces.actor.v1.ActorConfig.mailbox_timeout:type_name -> google.protobuf.Duration
	124, // 6: plexspaces.actor.v1.ActorConfig.checkpoint_interval:type_name -> google.protobuf.Duration
	125, // 7: plexspaces.actor.v1.ActorConfig.restart_policy:type_name -> plexspaces.common.v1.RetryPolicy
	126, // 8: plexspaces.actor.v1.ActorConfig.supervision_strategy:type_name -> plexspaces.supervision.v1.SupervisionStrategy
	104, // 9: plexspaces.actor.v1.ActorConfig.properties:type_name -> plexspaces.actor.v1.ActorConfig.PropertiesEntry
	22,  // 10: plexspaces.actor.v1.ActorConfig.stateless_worker_config:type_name -> plexspaces.actor.v1.StatelessWorkerConfig
	23,  // 11: plexspaces.actor.v1.ActorConfig.data_parallel_config:type_name -> plexspaces.actor.v1.DataParallelConfig
	8,   // 12: plexspaces.actor.v1.ActorConfig.state_management_mode:type_name -> plexspaces.actor.v1.StateMgmtMode
	9,   // 13: plexspaces.actor.v1.ActorConfig.consistency_level:type_name -> plexspaces.actor.v1.ConsistencyLevel
	21,  // 14: plexspaces.actor.v1.ActorConfig.resource_requirements:type_name -> plexspaces.actor.v1.ActorResourceRequirements
	105, // 15: plexspaces.actor.v1.ResourceRequirements.custom_requirements:type_name -> plexspaces.actor.v1.ResourceRequirements.CustomRequirementsEntry
	26,  // 16: plexspaces.actor.v1.ActorResourceRequirements.placement:type_name -> plexspaces.actor.v1.NodePlacement
	1,   // 17: plexspaces.actor.v1.StatelessWorkerConfig.strategy:type_name -> plexspaces.actor.v1.LoadBalancingStrategy
	2,   // 18: plexspaces.actor.v1.DataParallelConfig.partition_strategy:type_name -> plexspaces.actor.v1.PartitionStrategy
	3,   // 19: plexspaces.actor.v1.DataParallelConfig.rebalance_policy:type_name -> plexspaces.actor.v1.RebalancePolicy
	26,  // 20: plexspaces.actor.v1.DataParallelConfig.placement:type_name -> plexspaces.actor.v1.NodePlacement
	23,  // 21: plexspaces.actor.v1.ShardGroup.config:type_name -> plexspaces.actor.v1.DataParallelConfig
	4,   // 22: plexspaces.actor.v1.ShardGroup.state:type_name -> plexspaces.actor.v1.ShardGroupState
	127, // 23: plexspaces.actor.v1.ShardGroup.created_at:type_name -> google.protobuf.Timestamp
	106, // 24: plexspaces.actor.v1.ShardGroup.metadata:type_name -> plexspaces.actor.v1.ShardGroup.MetadataEntry
	25,  // 25: plexspaces.actor.v1.ShardGroup.rebalance_status:type_name -> plexspaces.actor.v1.RebalanceStatus
	127, // 26: plexspaces.actor.v1.RebalanceStatus.started_at:type_name -> google.protobuf.Timestamp
	127, // 27: plexspaces.actor.v1.RebalanceStatus.estimated_completion:type_name -> google.protobuf.Timestamp
	5,   // 28: plexspaces.actor.v1.NodePlacement.strategy:type_name -> plexspaces.actor.v1.NodePlacementStrategy
	107, // 29: plexspaces.actor.v1.NodePlacement.required_labels:type_name -> plexspaces.actor.v1.NodePlacement.RequiredLabelsEntry
	128, // 30: plexspaces.actor.v1.NodePlacement.resource_requirements:type_name -> plexspaces.common.v1.ResourceSpec
	108, // 31: plexspaces.actor.v1.NodePlacement.affinity_labels:type_name -> plexspaces.actor.v1.NodePlacement.AffinityLabelsEntry
	23,  // 32: plexspaces.actor.v1.CreateShardGroupRequest.config:type_name -> plexspaces.actor.v1.DataParallelConfig
	19,  // 33: plexspaces.actor.v1.CreateShardGroupRequest.shard_config:type_name -> plexspaces.actor.v1.ActorConfig
	109, // 34: plexspaces.actor.v1.CreateShardGroupRequest.metadata:type_name -> plexspaces.actor.v1.CreateShardGroupRequest.MetadataEntry
	24,  // 35: plexspaces.actor.v1.CreateShardGroupResponse.group:type_name -> plexspaces.actor.v1.ShardGroup
	124, // 36: plexspaces.actor.v1.DeleteShardGroupRequest.shutdown_timeout:type_name -> google.protobuf.Duration
	24,  // 37: plexspaces.actor.v1.GetShardGroupResponse.group:type_name -> plexspaces.actor.v1.ShardGroup
	4,   // 38: plexspaces.actor.v1.ListShardGroupsRequest.state:type_name -> plexspaces.actor.v1.ShardGroupState
	129, // 39: plexspaces.actor.v1.ListShardGroupsRequest.page:type_name -> plexspaces.common.v1.PageRequest
	24,  // 40: plexspaces.actor.v1.ListShardGroupsResponse.groups:type_name -> plexspaces.actor.v1.ShardGroup
	130, // 41: plexspaces.actor.v1.ListShardGroupsResponse.page:type_name -> plexspaces.common.v1.PageResponse
	131, // 42: plexspaces.actor.v1.SendToShardRequest.message:type_name -> plexspaces.common.v1.Message
	124, // 43: plexspaces.actor.v1.SendToShardRequest.timeout:type_name -> google.protobuf.Duration
	131, // 44: plexspaces.actor.v1.SendToShardResponse.response:type_name -> plexspaces.common.v1.Message
	131, // 45: plexspaces.actor.v1.ScatterGatherRequest.query:type_name -> plexspaces.common.v1.Message
	124, // 46: plexspaces.actor.v1.ScatterGatherRequest.timeout:type_name -> google.protobuf.Duration
	6,   // 47: plexspaces.actor.v1.ScatterGatherRequest.aggregation:type_name -> plexspaces.actor.v1.ShardGroupAggregationStrategy
	131, // 48: plexspaces.actor.v1.ShardQueryResponse.response:type_name -> plexspaces.common.v1.Message
	124, // 49: plexspaces.actor.v1.ShardQueryResponse.latency:type_name -> google.protobuf.Duration
	124, // 50: plexspaces.actor.v1.ScatterGatherStats.max_latency:type_name -> google.protobuf.Duration
	131, // 51: plexspaces.actor.v1.ScatterGatherResponse.result:type_name -> plexspaces.common.v1.Message
	37,  // 52: plexspaces.actor.v1.ScatterGatherResponse.shard_responses:type_name -> plexspaces.actor.v1.ShardQueryResponse
	38,  // 53: plexspaces.actor.v1.ScatterGatherResponse.stats:type_name -> plexspaces.actor.v1.ScatterGatherStats
	110, // 54: plexspaces.actor.v1.BulkUpdateShardGroupRequest.updates:type_name -> plexspaces.actor.v1.BulkUpdateShardGroupRequest.UpdatesEntry
	9,   // 55: plexspaces.actor.v1.BulkUpdateShardGroupRequest.consistency_level:type_name -> plexspaces.actor.v1.ConsistencyLevel
	124, // 56: plexspaces.actor.v1.BulkUpdateShardGroupRequest.timeout:type_name -> google.protobuf.Duration
	42,  // 57: plexspaces.actor.v1.BulkUpdateShardGroupResponse.shard_stats:type_name -> plexspaces.actor.v1.ShardUpdateStats
	131, // 58: plexspaces.actor.v1.MapShardGroupRequest.map_function:type_name -> plexspaces.common.v1.Message
	124, // 59: plexspaces.actor.v1.MapShardGroupRequest.timeout:type_name -> google.protobuf.Duration
	37,  // 60: plexspaces.actor.v1.MapShardGroupResponse.shard_results:type_name -> plexspaces.actor.v1.ShardQueryResponse
	38,  // 61: plexspaces.actor.v1.MapShardGroupResponse.stats:type_name -> plexspaces.actor.v1.ScatterGatherStats
	131, // 62: plexspaces.actor.v1.BroadcastShardGroupRequest.message:type_name -> plexspaces.common.v1.Message
	124, // 63: plexspaces.actor.v1.BroadcastShardGroupRequest.timeout:type_name -> google.protobuf.Duration
	37,  // 64: plexspaces.actor.v1.BroadcastShardGroupResponse.shard_responses:type_name -> plexspaces.actor.v1.ShardQueryResponse
	38,  // 65: plexspaces.actor.v1.BroadcastShardGroupResponse.stats:type_name -> plexspaces.actor.v1.ScatterGatherStats
	131, // 66: plexspaces.actor.v1.ReduceShardGroupRequest.map_function:type_name -> plexspaces.common.v1.Message
	124, // 67: plexspaces.actor.v1.ReduceShardGroupRequest.timeout:type_name -> google.protobuf.Duration
	7,   // 68: plexspaces.actor.v1.ReduceShardGroupRequest.reduction:type_name -> plexspaces.actor.v1.CollectiveReduction
	45,  // 69: plexspaces.actor.v1.ReduceShardGroupRequest.target:type_name -> plexspaces.actor.v1.CollectiveTargetField
	131, // 70: plexspaces.actor.v1.ReduceShardGroupResponse.result:type_name -> plexspaces.common.v1.Message
	37,  // 71: plexspaces.actor.v1.ReduceShardGroupResponse.shard_responses:type_name -> plexspaces.actor.v1.ShardQueryResponse
	38,  // 72: plexspaces.actor.v1.ReduceShardGroupResponse.stats:type_name -> plexspaces.actor.v1.ScatterGatherStats
	131, // 73: plexspaces.actor.v1.AllReduceShardGroupRequest.map_function:type_name -> plexspaces.common.v1.Message
	124, // 74: plexspaces.actor.v1.AllReduceShardGroupRequest.timeout:type_name -> google.protobuf.Duration
	7,   // 75: plexspaces.actor.v1.AllReduceShardGroupRequest.reduction:type_name -> plexspaces.actor.v1.CollectiveReduction
	45,  // 76: plexspaces.actor.v1.AllReduceShardGroupRequest.target:type_name -> plexspaces.actor.v1.CollectiveTargetField
	131, // 77: plexspaces.actor.v1.AllReduceShardGroupResponse.result:type_name -> plexspaces.common.v1.Message
	37,  // 78: plexspaces.actor.v1.AllReduceShardGroupResponse.shard_responses:type_name -> plexspaces.actor.v1.ShardQueryResponse
	38,  // 79: plexspaces.actor.v1.AllReduceShardGroupResponse.stats:type_name -> plexspaces.actor.v1.ScatterGatherStats
	124, // 80: plexspaces.actor.v1.BarrierShardGroupRequest.timeout:type_name -> google.protobuf.Duration
	37,  // 81: plexspaces.actor.v1.BarrierShardGroupResponse.shard_responses:type_name -> plexspaces.actor.v1.ShardQueryResponse
	38,  // 82: plexspaces.actor.v1.BarrierShardGroupResponse.stats:type_name -> plexspaces.actor.v1.ScatterGatherStats
	3,   // 83: plexspaces.actor.v1.ScaleShardGroupRequest.rebalance_policy:type_name -> plexspaces.actor.v1.RebalancePolicy
	19,  // 84: plexspaces.actor.v1.ScaleShardGroupRequest.new_shard_config:type_name -> plexspaces.actor.v1.ActorConfig
	24,  // 85: plexspaces.actor.v1.ScaleShardGroupResponse.group:type_name -> plexspaces.actor.v1.ShardGroup
	25,  // 86: plexspaces.actor.v1.ScaleShardGroupResponse.rebalance_status:type_name -> plexspaces.actor.v1.RebalanceStatus
	124, // 87: plexspaces.actor.v1.ActorMetrics.average_processing_time:type_name -> google.protobuf.Duration
	127, // 88: plexspaces.actor.v1.ActorMetrics.last_activity:type_name -> google.protobuf.Timestamp
	103, // 89: plexspaces.actor.v1.SpawnActorRequest.spec:type_name -> plexspaces.actor.v1.ActorSpawnSpec
	18,  // 90: plexspaces.actor.v1.SpawnActorResponse.actor:type_name -> plexspaces.actor.v1.Actor
	57,  // 91: plexspaces.actor.v1.SpawnActorsRequest.requests:type_name -> plexspaces.actor.v1.SpawnActorRequest
	58,  // 92: plexspaces.actor.v1.SpawnActorResult.response:type_name -> plexspaces.actor.v1.SpawnActorResponse
	60,  // 93: plexspaces.actor.v1.SpawnActorsResponse.results:type_name -> plexspaces.actor.v1.SpawnActorResult
	18,  // 94: plexspaces.actor.v1.GetActorResponse.actor:type_name -> plexspaces.actor.v1.Actor
	129, // 95: plexspaces.actor.v1.ListActorsRequest.page_request:type_name -> plexspaces.common.v1.PageRequest
	121, // 96: plexspaces.actor.v1.ListActorsRequest.state:type_name -> plexspaces.actor.v1.ActorState
	18,  // 97: plexspaces.actor.v1.ListActorsResponse.actors:type_name -> plexspaces.actor.v1.Actor
	130, // 98: plexspaces.actor.v1.ListActorsResponse.page_response:type_name -> plexspaces.common.v1.PageResponse
	111, // 99: plexspaces.actor.v1.SendMessageRequest.headers:type_name -> plexspaces.actor.v1.SendMessageRequest.HeadersEntry
	112, // 100: plexspaces.actor.v1.SendMessageRequest.query_params:type_name -> plexspaces.actor.v1.SendMessageRequest.QueryParamsEntry
	131, // 101: plexspaces.actor.v1.StreamMessageRequest.message:type_name -> plexspaces.common.v1.Message
	127, // 102: plexspaces.actor.v1.ActorLifecycleEvent.timestamp:type_name -> google.protobuf.Timestamp
	72,  // 103: plexspaces.actor.v1.ActorLifecycleEvent.created:type_name -> plexspaces.actor.v1.ActorCreated
	73,  // 104: plexspaces.actor.v1.ActorLifecycleEvent.starting:type_name -> plexspaces.actor.v1.ActorStarting
	74,  // 105: plexspaces.actor.v1.ActorLifecycleEvent.activated:type_name -> plexspaces.actor.v1.ActorActivated
	75,  // 106: plexspaces.actor.v1.ActorLifecycleEvent.deactivating:type_name -> plexspaces.actor.v1.ActorDeactivating
	76,  // 107: plexspaces.actor.v1.ActorLifecycleEvent.deactivated:type_name -> plexspaces.actor.v1.ActorDeactivated
	77,  // 108: plexspaces.actor.v1.ActorLifecycleEvent.terminated:type_name -> plexspaces.actor.v1.ActorTerminated
	78,  // 109: plexspaces.actor.v1.ActorLifecycleEvent.failed:type_name -> plexspaces.actor.v1.ActorFailed
	79,  // 110: plexspaces.actor.v1.ActorLifecycleEvent.migrating:type_name -> plexspaces.actor.v1.ActorMigrating
	113, // 111: plexspaces.actor.v1.GetActorStatesResponse.states:type_name -> plexspaces.actor.v1.GetActorStatesResponse.StatesEntry
	127, // 112: plexspaces.actor.v1.ActorLink.created_at:type_name -> google.protobuf.Timestamp
	114, // 113: plexspaces.actor.v1.ActorLink.metadata:type_name -> plexspaces.actor.v1.ActorLink.MetadataEntry
	115, // 114: plexspaces.actor.v1.AskReplyRequest.headers:type_name -> plexspaces.actor.v1.AskReplyRequest.HeadersEntry
	116, // 115: plexspaces.actor.v1.AskReplyRequest.query_params:type_name -> plexspaces.actor.v1.AskReplyRequest.QueryParamsEntry
	124, // 116: plexspaces.actor.v1.AskReplyRequest.timeout:type_name -> google.protobuf.Duration
	117, // 117: plexspaces.actor.v1.AskReplyResponse.headers:type_name -> plexspaces.actor.v1.AskReplyResponse.HeadersEntry
	12,  // 118: plexspaces.actor.v1.LifecycleEventFilter.event_types:type_name -> plexspaces.actor.v1.LifecycleEventType
	118, // 119: plexspaces.actor.v1.LifecycleEventFilter.required_tags:type_name -> plexspaces.actor.v1.LifecycleEventFilter.RequiredTagsEntry
	13,  // 120: plexspaces.actor.v1.LifecycleEventFilter.drop_policy:type_name -> plexspaces.actor.v1.DropPolicy
	127, // 121: plexspaces.actor.v1.VirtualActorLifecycle.last_activated:type_name -> google.protobuf.Timestamp
	127, // 122: plexspaces.actor.v1.VirtualActorLifecycle.last_accessed:type_name -> google.protobuf.Timestamp
	124, // 123: plexspaces.actor.v1.VirtualActorLifecycle.idle_timeout:type_name -> google.protobuf.Duration
	132, // 124: plexspaces.actor.v1.VirtualActorConfig.activation_strategy:type_name -> plexspaces.common.v1.ActivationStrategy
	124, // 125: plexspaces.actor.v1.VirtualActorConfig.idle_timeout:type_name -> google.protobuf.Duration
	14,  // 126: plexspaces.actor.v1.ActorRefError.code:type_name -> plexspaces.actor.v1.ActorRefErrorCode
	124, // 127: plexspaces.actor.v1.ResourceContract.max_execution_time:type_name -> google.protobuf.Duration
	16,  // 128: plexspaces.actor.v1.ResourceViolation.code:type_name -> plexspaces.actor.v1.ResourceViolationCode
	17,  // 129: plexspaces.actor.v1.ActorHealth.status:type_name -> plexspaces.actor.v1.ActorHealthStatus
	124, // 130: plexspaces.actor.v1.ActorHealth.stuck_since:type_name -> google.protobuf.Duration
	133, // 131: plexspaces.actor.v1.ActorSpawnSpec.identity:type_name -> plexspaces.common.v1.ActorIdentity
	10,  // 132: plexspaces.actor.v1.ActorSpawnSpec.visibility:type_name -> plexspaces.actor.v1.ActorVisibility
	119, // 133: plexspaces.actor.v1.ActorSpawnSpec.args:type_name -> plexspaces.actor.v1.ActorSpawnSpec.ArgsEntry
	123, // 134: plexspaces.actor.v1.ActorSpawnSpec.facets:type_name -> plexspaces.common.v1.Facet
	120, // 135: plexspaces.actor.v1.ActorSpawnSpec.labels:type_name -> plexspaces.actor.v1.ActorSpawnSpec.LabelsEntry
	19,  // 136: plexspaces.actor.v1.ActorSpawnSpec.config:type_name -> plexspaces.actor.v1.ActorConfig
	134, // 137: plexspaces.actor.v1.ActorConfig.PropertiesEntry.value:type_name -> google.protobuf.Any
	131, // 138: plexspaces.actor.v1.BulkUpdateShardGroupRequest.UpdatesEntry.value:type_name -> plexspaces.common.v1.Message
	121, // 139: plexspaces.actor.v1.GetActorStatesResponse.StatesEntry.value:type_name -> plexspaces.actor.v1.ActorState
	57,  // 140: plexspaces.actor.v1.ActorService.SpawnActor:input_type -> plexspaces.actor.v1.SpawnActorRequest
	59,  // 141: plexspaces.actor.v1.ActorService.SpawnActors:input_type -> plexspaces.actor.v1.SpawnActorsRequest
	62,  // 142: plexspaces.actor.v1.ActorService.GetActor:input_type -> plexspaces.actor.v1.GetActorRequest
	64,  // 143: plexspaces.actor.v1.ActorService.ListActors:input_type -> plexspaces.actor.v1.ListActorsRequest
	66,  // 144: plexspaces.actor.v1.ActorService.SendMessage:input_type -> plexspaces.actor.v1.SendMessageRequest
	68,  // 145: plexspaces.actor.v1.ActorService.StreamMessages:input_type -> plexspaces.actor.v1.StreamMessageRequest
	70,  // 146: plexspaces.actor.v1.ActorService.DeleteActor:input_type -> plexspaces.actor.v1.DeleteActorRequest
	80,  // 147: plexspaces.actor.v1.ActorService.MonitorActor:input_type -> plexspaces.actor.v1.MonitorActorRequest
	82,  // 148: plexspaces.actor.v1.ActorService.DemonitorActor:input_type -> plexspaces.actor.v1.DemonitorActorRequest
	87,  // 149: plexspaces.actor.v1.ActorService.LinkActor:input_type -> plexspaces.actor.v1.LinkActorRequest
	89,  // 150: plexspaces.actor.v1.ActorService.UnlinkActor:input_type -> plexspaces.actor.v1.UnlinkActorRequest
	83,  // 151: plexspaces.actor.v1.ActorService.NotifyActorDown:input_type -> plexspaces.actor.v1.ActorDownNotification
	91,  // 152: plexspaces.actor.v1.ActorService.CheckActorExists:input_type -> plexspaces.actor.v1.CheckActorExistsRequest
	84,  // 153: plexspaces.actor.v1.ActorService.GetActorStates:input_type -> plexspaces.actor.v1.GetActorStatesRequest
	93,  // 154: plexspaces.actor.v1.ActorService.AskReply:input_type -> plexspaces.actor.v1.AskReplyRequest
	27,  // 155: plexspaces.actor.v1.ActorService.CreateShardGroup:input_type -> plexspaces.actor.v1.CreateShardGroupRequest
	29,  // 156: plexspaces.actor.v1.ActorService.DeleteShardGroup:input_type -> plexspaces.actor.v1.DeleteShardGroupRequest
	30,  // 157: plexspaces.actor.v1.ActorService.GetShardGroup:input_type -> plexspaces.actor.v1.GetShardGroupRequest
	32,  // 158: plexspaces.actor.v1.ActorService.ListShardGroups:input_type -> plexspaces.actor.v1.ListShardGroupsRequest
	54,  // 159: plexspaces.actor.v1.ActorService.ScaleShardGroup:input_type -> plexspaces.actor.v1.ScaleShardGroupRequest
	34,  // 160: plexspaces.actor.v1.ActorService.SendToShard:input_type -> plexspaces.actor.v1.SendToShardRequest
	46,  // 161: plexspaces.actor.v1.ActorService.BroadcastShardGroup:input_type -> plexspaces.actor.v1.BroadcastShardGroupRequest
	48,  // 162: plexspaces.actor.v1.ActorService.ReduceShardGroup:input_type -> plexspaces.actor.v1.ReduceShardGroupRequest
	50,  // 163: plexspaces.actor.v1.ActorService.AllReduceShardGroup:input_type -> plexspaces.actor.v1.AllReduceShardGroupRequest
	52,  // 164: plexspaces.actor.v1.ActorService.BarrierShardGroup:input_type -> plexspaces.actor.v1.BarrierShardGroupRequest
	36,  // 165: plexspaces.actor.v1.ActorService.ScatterGather:input_type -> plexspaces.actor.v1.ScatterGatherRequest
	40,  // 166: plexspaces.actor.v1.ActorService.BulkUpdateShardGroup:input_type -> plexspaces.actor.v1.BulkUpdateShardGroupRequest
	43,  // 167: plexspaces.actor.v1.ActorService.MapShardGroup:input_type -> plexspaces.actor.v1.MapShardGroupRequest
	95,  // 168: plexspaces.actor.v1.LifecycleEventChannel.SubscribeLifecycleEvents:input_type -> plexspaces.actor.v1.LifecycleEventFilter
	71,  // 169: plexspaces.actor.v1.LifecycleEventChannel.PublishLifecycleEvent:input_type -> plexspaces.actor.v1.ActorLifecycleEvent
	58,  // 170: plexspaces.actor.v1.ActorService.SpawnActor:output_type -> plexspaces.actor.v1.SpawnActorResponse
	61,  // 171: plexspaces.actor.v1.ActorService.SpawnActors:output_type -> plexspaces.actor.v1.SpawnActorsResponse
	63,  // 172: plexspaces.actor.v1.ActorService.GetActor:output_type -> plexspaces.actor.v1.GetActorResponse
	65,  // 173: plexspaces.actor.v1.ActorService.ListActors:output_type -> plexspaces.actor.v1.ListActorsResponse
	67,  // 174: plexspaces.actor.v1.ActorService.SendMessage:output_type -> plexspaces.actor.v1.SendMessageResponse
	69,  // 175: plexspaces.actor.v1.ActorService.StreamMessages:output_type -> plexspaces.actor.v1.StreamMessageResponse
	135, // 176: plexspaces.actor.v1.ActorService.DeleteActor:output_type -> plexspaces.common.v1.Empty
	81,  // 177: plexspaces.actor.v1.ActorService.MonitorActor:output_type -> plexspaces.actor.v1.MonitorActorResponse
	135, // 178: plexspaces.actor.v1.ActorService.DemonitorActor:output_type -> plexspaces.common.v1.Empty
	88,  // 179: plexspaces.actor.v1.ActorService.LinkActor:output_type -> plexspaces.actor.v1.LinkActorResponse
	90,  // 180: plexspaces.actor.v1.ActorService.UnlinkActor:output_type -> plexspaces.actor.v1.UnlinkActorResponse
	135, // 181: plexspaces.actor.v1.ActorService.NotifyActorDown:output_type -> plexspaces.common.v1.Empty
	92,  // 182: plexspaces.actor.v1.ActorService.CheckActorExists:output_type -> plexspaces.actor.v1.CheckActorExistsResponse
	85,  // 183: plexspaces.actor.v1.ActorService.GetActorStates:output_type -> plexspaces.actor.v1.GetActorStatesResponse
	94,  // 184: plexspaces.actor.v1.ActorService.AskReply:output_type -> plexspaces.actor.v1.AskReplyResponse
	28,  // 185: plexspaces.actor.v1.ActorService.CreateShardGroup:output_type -> plexspaces.actor.v1.CreateShardGroupResponse
	135, // 186: plexspaces.actor.v1.ActorService.DeleteShardGroup:output_type -> plexspaces.common.v1.Empty
	31,  // 187: plexspaces.actor.v1.ActorService.GetShardGroup:output_type -> plexspaces.actor.v1.GetShardGroupResponse
	33,  // 188: plexspaces.actor.v1.ActorService.ListShardGroups:output_type -> plexspaces.actor.v1.ListShardGroupsResponse
	55,  // 189: plexspaces.actor.v1.ActorService.ScaleShardGroup:output_type -> plexspaces.actor.v1.ScaleShardGroupResponse
	35,  // 190: plexspaces.actor.v1.ActorService.SendToShard:output_type -> plexspaces.actor.v1.SendToShardResponse
	47,  // 191: plexspaces.actor.v1.ActorService.BroadcastShardGroup:output_type -> plexspaces.actor.v1.BroadcastShardGroupResponse
	49,  // 192: plexspaces.actor.v1.ActorService.ReduceShardGroup:output_type -> plexspaces.actor.v1.ReduceShardGroupResponse
	51,  // 193: plexspaces.actor.v1.ActorService.AllReduceShardGroup:output_type -> plexspaces.actor.v1.AllReduceShardGroupResponse
	53,  // 194: plexspaces.actor.v1.ActorService.BarrierShardGroup:output_type -> plexspaces.actor.v1.BarrierShardGroupResponse
	39,  // 195: plexspaces.actor.v1.ActorService.ScatterGather:output_type -> plexspaces.actor.v1.ScatterGatherResponse
	41,  // 196: plexspaces.actor.v1.ActorService.BulkUpdateShardGroup:output_type -> plexspaces.actor.v1.BulkUpdateShardGroupResponse
	44,  // 197: plexspaces.actor.v1.ActorService.MapShardGroup:output_type -> plexspaces.actor.v1.MapShardGroupResponse
	71,  // 198: plexspaces.actor.v1.LifecycleEventChannel.SubscribeLifecycleEvents:output_type -> plexspaces.actor.v1.ActorLifecycleEvent
	135, // 199: plexspaces.actor.v1.LifecycleEventChannel.PublishLifecycleEvent:output_type -> plexspaces.common.v1.Empty
	170, // [170:200] is the sub-list for method output_type
	140, // [140:170] is the sub-list for method input_type
	140, // [140:140] is the sub-list for extension type_name
	140, // [140:140] is the sub-list for extension extendee
	0,   // [0:140] is the sub-list for field type_name
}

func init() { file_plexspaces_v1_actors_actor_runtime_proto_init() }
func file_plexspaces_v1_actors_actor_runtime_proto_init() {
	if File_plexspaces_v1_actors_actor_runtime_proto != nil {
		return
	}
	file_plexspaces_v1_actors_types_proto_init()
	file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[53].OneofWrappers = []any{
		(*ActorLifecycleEvent_Created)(nil),
		(*ActorLifecycleEvent_Starting)(nil),
		(*ActorLifecycleEvent_Activated)(nil),
		(*ActorLifecycleEvent_Deactivating)(nil),
		(*ActorLifecycleEvent_Deactivated)(nil),
		(*ActorLifecycleEvent_Terminated)(nil),
		(*ActorLifecycleEvent_Failed)(nil),
		(*ActorLifecycleEvent_Migrating)(nil),
	}
	file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[81].OneofWrappers = []any{}
	file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[83].OneofWrappers = []any{}
	file_plexspaces_v1_actors_actor_runtime_proto_msgTypes[84].OneofWrappers = []any{}
	type x struct{}
	out := protoimpl.TypeBuilder{
		File: protoimpl.DescBuilder{
			GoPackagePath: reflect.TypeOf(x{}).PkgPath(),
			RawDescriptor: unsafe.Slice(unsafe.StringData(file_plexspaces_v1_actors_actor_runtime_proto_rawDesc), len(file_plexspaces_v1_actors_actor_runtime_proto_rawDesc)),
			NumEnums:      18,
			NumMessages:   103,
			NumExtensions: 0,
			NumServices:   2,
		},
		GoTypes:           file_plexspaces_v1_actors_actor_runtime_proto_goTypes,
		DependencyIndexes: file_plexspaces_v1_actors_actor_runtime_proto_depIdxs,
		EnumInfos:         file_plexspaces_v1_actors_actor_runtime_proto_enumTypes,
		MessageInfos:      file_plexspaces_v1_actors_actor_runtime_proto_msgTypes,
	}.Build()
	File_plexspaces_v1_actors_actor_runtime_proto = out.File
	file_plexspaces_v1_actors_actor_runtime_proto_goTypes = nil
	file_plexspaces_v1_actors_actor_runtime_proto_depIdxs = nil
}
