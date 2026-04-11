// SPDX-License-Identifier: LGPL-2.1-or-later
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

// PlexSpaces Common Types API
//
// ## Purpose
// Provides fundamental data types and utilities shared across all PlexSpaces services.
// This is the foundation layer that all other proto files depend on, defining core
// abstractions like actor identity, metadata, error handling, and the Facet system.
//
// ## Architecture Context
// This proto file is foundational to ALL five PlexSpaces pillars:
// - **Pillar 1 (TupleSpace)**: Uses Metadata for tuple annotations, QoSLevel for delivery guarantees
// - **Pillar 2 (Erlang/OTP)**: Uses string actor IDs for identity, RetryPolicy for supervision
// - **Pillar 3 (Durability)**: Uses Metadata timestamps for journal ordering, RetryPolicy for recovery
// - **Pillar 4 (WASM)**: Uses Facets to dynamically add WASM execution capabilities
// - **Pillar 5 (Firecracker)**: Uses ResourceState to track VM lifecycle
//
// ### Core Abstractions Provided
// 1. **Actor IDs**: Plain strings for actor identity (format: "{namespace}/{actor_name}")
// 2. **Metadata**: Standard resource metadata (creation time, labels, annotations)
// 3. **ErrorDetail**: Structured error reporting with extensible details
// 4. **RetryPolicy**: Configurable retry behavior for fault tolerance
// 5. **QoSLevel**: Message delivery guarantees (none, best-effort, at-least-once, exactly-once)
// 6. **ResourceState**: Lifecycle states for managed resources (creating, active, deleting, etc.)
// 7. **Facet System**: Dynamic capability composition (THE key extensibility mechanism)
//
// ## Component Interactions
// - **Used by**: ALL other proto files (actor_runtime.proto, tuplespace.proto, supervision.proto, etc.)
// - **Depends on**: Only Google well-known types (Timestamp, Duration, Any, Struct)
// - **Provides**: Core types that enable distributed actor communication and resource management
//
// ## Design Decisions
// - **Why actor IDs are plain strings**:
//   - Simplicity: No wrapper message, direct string usage
//   - Human-readable: Easy debugging and logging
//   - Wire efficiency: No extra message overhead
//   - Flexibility: Can embed namespace, node_id in string format
//
// - **Why Metadata uses maps for labels and annotations**:
//   - Labels: Simple string key-value pairs for filtering/grouping (Kubernetes-inspired)
//   - Annotations: Complex structured data using google.protobuf.Any (arbitrary payloads)
//   - Enables extensibility without proto changes
//
// - **Why separate QoSLevel and RetryPolicy**:
//   - QoS: Message delivery semantics (fire-and-forget vs guaranteed)
//   - RetryPolicy: Failure recovery behavior (backoff, max attempts)
//   - Orthogonal concerns that compose independently
//
// - **Why ResourceState enum instead of bool flags**:
//   - State machine validation: only valid transitions allowed
//   - Clear semantics: CREATING vs ACTIVE vs DELETING are distinct
//   - Enables supervision logic based on state
//
// - **Why Facet system in common.proto instead of facets.proto**:
//   - CRITICAL DESIGN: Facets are THE extensibility mechanism
//   - Must be available to all components without circular dependencies
//   - Enables "Static for core, Dynamic for extensions" principle
//   - Actors, workflows, nodes all use Facets for runtime composition
//
// ## Facet System (CRITICAL EXTENSIBILITY MECHANISM)
// Facets provide dynamic capabilities to actors and resources WITHOUT changing core abstractions.
// This is the key to PlexSpaces' "one powerful actor" philosophy instead of "20 specialized types".
//
// ### Philosophy
// - **Core = Static**: Identity, state, behavior, mailbox, journal (always present, compiled in)
// - **Extensions = Facets**: Mobility, metrics, tracing, security (optional, runtime-composed)
// - **Pay for what you use**: Only actors that need a capability pay its cost
//
// ### Example: Virtual Actor = Actor + VirtualActorFacet
// ```protobuf
// Actor {
//   id: "user-123"
//   facets: [
//     Facet {
//       type: "virtual_actor"
//       config: { "activation_strategy": "lazy", "deactivation_timeout": "5m" }
//       priority: 100  // Higher priority = runs first in interceptor chain
//     }
//   ]
// }
// ```
//
// ### Example: Mobile Actor = Actor + MobilityFacet
// ```protobuf
// Actor {
//   id: "agent-456"
//   facets: [
//     Facet {
//       type: "mobility"
//       config: { "migration_strategy": "eager", "state_transfer": "checkpoint" }
//       priority: 50
//     }
//   ]
// }
// ```
//
// ### Common Facet Types (defined in facets.proto)
// - **virtual_actor**: Automatic activation/deactivation (Orbit-inspired)
// - **otp_genserver**: GenServer behavior with handle_call/cast/info
// - **durable_execution**: Journaling and deterministic replay (Restate-inspired)
// - **mobility**: Actor migration between nodes
// - **metrics**: Prometheus metrics collection
// - **tracing**: Distributed tracing (OpenTelemetry)
// - **security**: Authorization and authentication
// - **collaboration**: Multi-agent coordination
//
// ### Facet Priority Ranges
// Priority determines execution order in the facet interceptor chain:
// - **1000+**: Security/Auth facets (run first, can block execution)
// - **900-999**: Logging/Tracing facets (capture all events)
// - **800-899**: Metrics facets (measure performance)
// - **100-500**: Domain logic facets (business capabilities)
// - **1-99**: Persistence facets (run last, commit state)

// Code generated by protoc-gen-go. DO NOT EDIT.
// versions:
// 	protoc-gen-go v1.36.11
// 	protoc        (unknown)
// source: plexspaces/v1/common.proto

package commonv1

import (
	_ "buf.build/gen/go/bufbuild/protovalidate/protocolbuffers/go/buf/validate"
	_ "github.com/grpc-ecosystem/grpc-gateway/v2/protoc-gen-openapiv2/options"
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

// Activation strategy for virtual actors (single definition used by actor_runtime and node.release).
//
// Used by VirtualActorConfig (actor_runtime), DefaultVirtualActorConfig (release), and Rust
// VirtualActorFacet. Define here so it is not duplicated across protos or crates.
type ActivationStrategy int32

const (
	ActivationStrategy_ACTIVATION_STRATEGY_UNSPECIFIED ActivationStrategy = 0 // Treated as LAZY
	ActivationStrategy_ACTIVATION_STRATEGY_LAZY        ActivationStrategy = 1 // Activate on first message (default)
	ActivationStrategy_ACTIVATION_STRATEGY_EAGER       ActivationStrategy = 2 // Activate immediately on creation
	ActivationStrategy_ACTIVATION_STRATEGY_PREWARM     ActivationStrategy = 3 // Pre-activate based on schedule
)

// Enum value maps for ActivationStrategy.
var (
	ActivationStrategy_name = map[int32]string{
		0: "ACTIVATION_STRATEGY_UNSPECIFIED",
		1: "ACTIVATION_STRATEGY_LAZY",
		2: "ACTIVATION_STRATEGY_EAGER",
		3: "ACTIVATION_STRATEGY_PREWARM",
	}
	ActivationStrategy_value = map[string]int32{
		"ACTIVATION_STRATEGY_UNSPECIFIED": 0,
		"ACTIVATION_STRATEGY_LAZY":        1,
		"ACTIVATION_STRATEGY_EAGER":       2,
		"ACTIVATION_STRATEGY_PREWARM":     3,
	}
)

func (x ActivationStrategy) Enum() *ActivationStrategy {
	p := new(ActivationStrategy)
	*p = x
	return p
}

func (x ActivationStrategy) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ActivationStrategy) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_common_proto_enumTypes[0].Descriptor()
}

func (ActivationStrategy) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_common_proto_enumTypes[0]
}

func (x ActivationStrategy) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ActivationStrategy.Descriptor instead.
func (ActivationStrategy) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{0}
}

// Quality of Service levels
type QoSLevel int32

const (
	QoSLevel_QOS_LEVEL_UNSPECIFIED QoSLevel = 0
	QoSLevel_QOS_LEVEL_NONE        QoSLevel = 1
	QoSLevel_QOS_LEVEL_BEST_EFFORT QoSLevel = 2
	QoSLevel_QOS_LEVEL_GUARANTEED  QoSLevel = 3
)

// Enum value maps for QoSLevel.
var (
	QoSLevel_name = map[int32]string{
		0: "QOS_LEVEL_UNSPECIFIED",
		1: "QOS_LEVEL_NONE",
		2: "QOS_LEVEL_BEST_EFFORT",
		3: "QOS_LEVEL_GUARANTEED",
	}
	QoSLevel_value = map[string]int32{
		"QOS_LEVEL_UNSPECIFIED": 0,
		"QOS_LEVEL_NONE":        1,
		"QOS_LEVEL_BEST_EFFORT": 2,
		"QOS_LEVEL_GUARANTEED":  3,
	}
)

func (x QoSLevel) Enum() *QoSLevel {
	p := new(QoSLevel)
	*p = x
	return p
}

func (x QoSLevel) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (QoSLevel) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_common_proto_enumTypes[1].Descriptor()
}

func (QoSLevel) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_common_proto_enumTypes[1]
}

func (x QoSLevel) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use QoSLevel.Descriptor instead.
func (QoSLevel) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{1}
}

// Resource states
type ResourceState int32

const (
	ResourceState_RESOURCE_STATE_UNSPECIFIED ResourceState = 0
	ResourceState_RESOURCE_STATE_CREATING    ResourceState = 1
	ResourceState_RESOURCE_STATE_ACTIVE      ResourceState = 2
	ResourceState_RESOURCE_STATE_INACTIVE    ResourceState = 3
	ResourceState_RESOURCE_STATE_UPDATING    ResourceState = 4
	ResourceState_RESOURCE_STATE_DELETING    ResourceState = 5
	ResourceState_RESOURCE_STATE_FAILED      ResourceState = 6
	ResourceState_RESOURCE_STATE_UNKNOWN     ResourceState = 7
)

// Enum value maps for ResourceState.
var (
	ResourceState_name = map[int32]string{
		0: "RESOURCE_STATE_UNSPECIFIED",
		1: "RESOURCE_STATE_CREATING",
		2: "RESOURCE_STATE_ACTIVE",
		3: "RESOURCE_STATE_INACTIVE",
		4: "RESOURCE_STATE_UPDATING",
		5: "RESOURCE_STATE_DELETING",
		6: "RESOURCE_STATE_FAILED",
		7: "RESOURCE_STATE_UNKNOWN",
	}
	ResourceState_value = map[string]int32{
		"RESOURCE_STATE_UNSPECIFIED": 0,
		"RESOURCE_STATE_CREATING":    1,
		"RESOURCE_STATE_ACTIVE":      2,
		"RESOURCE_STATE_INACTIVE":    3,
		"RESOURCE_STATE_UPDATING":    4,
		"RESOURCE_STATE_DELETING":    5,
		"RESOURCE_STATE_FAILED":      6,
		"RESOURCE_STATE_UNKNOWN":     7,
	}
)

func (x ResourceState) Enum() *ResourceState {
	p := new(ResourceState)
	*p = x
	return p
}

func (x ResourceState) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ResourceState) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_common_proto_enumTypes[2].Descriptor()
}

func (ResourceState) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_common_proto_enumTypes[2]
}

func (x ResourceState) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ResourceState.Descriptor instead.
func (ResourceState) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{2}
}

// Empty message (replacement for plexspaces.common.v1.Empty)
//
// Used for RPC methods that don't return a meaningful value.
// We define our own instead of using plexspaces.common.v1.Empty because
// prost_types doesn't include it.
type Empty struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *Empty) Reset() {
	*x = Empty{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[0]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *Empty) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*Empty) ProtoMessage() {}

func (x *Empty) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[0]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use Empty.ProtoReflect.Descriptor instead.
func (*Empty) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{0}
}

// Standard metadata for resources
type Metadata struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	CreateTime    *timestamppb.Timestamp `protobuf:"bytes,1,opt,name=create_time,json=createTime,proto3" json:"create_time,omitempty"`
	UpdateTime    *timestamppb.Timestamp `protobuf:"bytes,2,opt,name=update_time,json=updateTime,proto3" json:"update_time,omitempty"`
	CreatedBy     string                 `protobuf:"bytes,3,opt,name=created_by,json=createdBy,proto3" json:"created_by,omitempty"`
	UpdatedBy     string                 `protobuf:"bytes,4,opt,name=updated_by,json=updatedBy,proto3" json:"updated_by,omitempty"`
	Labels        map[string]string      `protobuf:"bytes,5,rep,name=labels,proto3" json:"labels,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	Annotations   map[string]*anypb.Any  `protobuf:"bytes,6,rep,name=annotations,proto3" json:"annotations,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *Metadata) Reset() {
	*x = Metadata{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[1]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *Metadata) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*Metadata) ProtoMessage() {}

func (x *Metadata) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[1]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use Metadata.ProtoReflect.Descriptor instead.
func (*Metadata) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{1}
}

func (x *Metadata) GetCreateTime() *timestamppb.Timestamp {
	if x != nil {
		return x.CreateTime
	}
	return nil
}

func (x *Metadata) GetUpdateTime() *timestamppb.Timestamp {
	if x != nil {
		return x.UpdateTime
	}
	return nil
}

func (x *Metadata) GetCreatedBy() string {
	if x != nil {
		return x.CreatedBy
	}
	return ""
}

func (x *Metadata) GetUpdatedBy() string {
	if x != nil {
		return x.UpdatedBy
	}
	return ""
}

func (x *Metadata) GetLabels() map[string]string {
	if x != nil {
		return x.Labels
	}
	return nil
}

func (x *Metadata) GetAnnotations() map[string]*anypb.Any {
	if x != nil {
		return x.Annotations
	}
	return nil
}

// Structured actor identity.
//
// The canonical string form is `{name}//{actor_type}::{namespace}@{node_id}`.
// Construct actor IDs from fields, and only derive the canonical string for
// storage or display.
type ActorId struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// User-specified actor name. Must be unique within the actor type,
	// namespace, and node scope.
	Name string `protobuf:"bytes,1,opt,name=name,proto3" json:"name,omitempty"`
	// Actor type from behavior registration.
	ActorType string `protobuf:"bytes,2,opt,name=actor_type,json=actorType,proto3" json:"actor_type,omitempty"`
	// Namespace for tenancy and application isolation.
	Namespace string `protobuf:"bytes,3,opt,name=namespace,proto3" json:"namespace,omitempty"`
	// Node where the actor currently resides.
	NodeId        string `protobuf:"bytes,4,opt,name=node_id,json=nodeId,proto3" json:"node_id,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ActorId) Reset() {
	*x = ActorId{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[2]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ActorId) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ActorId) ProtoMessage() {}

func (x *ActorId) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[2]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ActorId.ProtoReflect.Descriptor instead.
func (*ActorId) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{2}
}

func (x *ActorId) GetName() string {
	if x != nil {
		return x.Name
	}
	return ""
}

func (x *ActorId) GetActorType() string {
	if x != nil {
		return x.ActorType
	}
	return ""
}

func (x *ActorId) GetNamespace() string {
	if x != nil {
		return x.Namespace
	}
	return ""
}

func (x *ActorId) GetNodeId() string {
	if x != nil {
		return x.NodeId
	}
	return ""
}

// Standard error details
type ErrorDetail struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Code          string                 `protobuf:"bytes,1,opt,name=code,proto3" json:"code,omitempty"`
	Message       string                 `protobuf:"bytes,2,opt,name=message,proto3" json:"message,omitempty"`
	Details       map[string]*anypb.Any  `protobuf:"bytes,3,rep,name=details,proto3" json:"details,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ErrorDetail) Reset() {
	*x = ErrorDetail{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[3]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ErrorDetail) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ErrorDetail) ProtoMessage() {}

func (x *ErrorDetail) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[3]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ErrorDetail.ProtoReflect.Descriptor instead.
func (*ErrorDetail) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{3}
}

func (x *ErrorDetail) GetCode() string {
	if x != nil {
		return x.Code
	}
	return ""
}

func (x *ErrorDetail) GetMessage() string {
	if x != nil {
		return x.Message
	}
	return ""
}

func (x *ErrorDetail) GetDetails() map[string]*anypb.Any {
	if x != nil {
		return x.Details
	}
	return nil
}

// Retry policy configuration
type RetryPolicy struct {
	state             protoimpl.MessageState `protogen:"open.v1"`
	MaxAttempts       uint32                 `protobuf:"varint,1,opt,name=max_attempts,json=maxAttempts,proto3" json:"max_attempts,omitempty"`
	BackoffMultiplier float64                `protobuf:"fixed64,2,opt,name=backoff_multiplier,json=backoffMultiplier,proto3" json:"backoff_multiplier,omitempty"`
	InitialDelay      *durationpb.Duration   `protobuf:"bytes,3,opt,name=initial_delay,json=initialDelay,proto3" json:"initial_delay,omitempty"` // >= 1ms
	MaxDelay          *durationpb.Duration   `protobuf:"bytes,4,opt,name=max_delay,json=maxDelay,proto3" json:"max_delay,omitempty"`             // <= 1 hour
	unknownFields     protoimpl.UnknownFields
	sizeCache         protoimpl.SizeCache
}

func (x *RetryPolicy) Reset() {
	*x = RetryPolicy{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[4]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *RetryPolicy) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*RetryPolicy) ProtoMessage() {}

func (x *RetryPolicy) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[4]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use RetryPolicy.ProtoReflect.Descriptor instead.
func (*RetryPolicy) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{4}
}

func (x *RetryPolicy) GetMaxAttempts() uint32 {
	if x != nil {
		return x.MaxAttempts
	}
	return 0
}

func (x *RetryPolicy) GetBackoffMultiplier() float64 {
	if x != nil {
		return x.BackoffMultiplier
	}
	return 0
}

func (x *RetryPolicy) GetInitialDelay() *durationpb.Duration {
	if x != nil {
		return x.InitialDelay
	}
	return nil
}

func (x *RetryPolicy) GetMaxDelay() *durationpb.Duration {
	if x != nil {
		return x.MaxDelay
	}
	return nil
}

// Standard pagination request
type PageRequest struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Offset for pagination (0-based, default: 0)
	Offset int32 `protobuf:"varint,1,opt,name=offset,proto3" json:"offset,omitempty"`
	// Limit/Page size (default: 50, max: 1000)
	Limit int32 `protobuf:"varint,2,opt,name=limit,proto3" json:"limit,omitempty"`
	// Filter string (optional)
	Filter string `protobuf:"bytes,3,opt,name=filter,proto3" json:"filter,omitempty"`
	// Order by field (optional)
	OrderBy       string `protobuf:"bytes,4,opt,name=order_by,json=orderBy,proto3" json:"order_by,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *PageRequest) Reset() {
	*x = PageRequest{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[5]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *PageRequest) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*PageRequest) ProtoMessage() {}

func (x *PageRequest) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[5]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use PageRequest.ProtoReflect.Descriptor instead.
func (*PageRequest) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{5}
}

func (x *PageRequest) GetOffset() int32 {
	if x != nil {
		return x.Offset
	}
	return 0
}

func (x *PageRequest) GetLimit() int32 {
	if x != nil {
		return x.Limit
	}
	return 0
}

func (x *PageRequest) GetFilter() string {
	if x != nil {
		return x.Filter
	}
	return ""
}

func (x *PageRequest) GetOrderBy() string {
	if x != nil {
		return x.OrderBy
	}
	return ""
}

// Standard pagination response
type PageResponse struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Total number of items (across all pages)
	TotalSize int32 `protobuf:"varint,1,opt,name=total_size,json=totalSize,proto3" json:"total_size,omitempty"`
	// Current offset
	Offset int32 `protobuf:"varint,2,opt,name=offset,proto3" json:"offset,omitempty"`
	// Current limit/page size
	Limit int32 `protobuf:"varint,3,opt,name=limit,proto3" json:"limit,omitempty"`
	// Whether there are more pages (has_next = offset + limit < total_size)
	HasNext       bool `protobuf:"varint,4,opt,name=has_next,json=hasNext,proto3" json:"has_next,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *PageResponse) Reset() {
	*x = PageResponse{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[6]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *PageResponse) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*PageResponse) ProtoMessage() {}

func (x *PageResponse) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[6]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use PageResponse.ProtoReflect.Descriptor instead.
func (*PageResponse) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{6}
}

func (x *PageResponse) GetTotalSize() int32 {
	if x != nil {
		return x.TotalSize
	}
	return 0
}

func (x *PageResponse) GetOffset() int32 {
	if x != nil {
		return x.Offset
	}
	return 0
}

func (x *PageResponse) GetLimit() int32 {
	if x != nil {
		return x.Limit
	}
	return 0
}

func (x *PageResponse) GetHasNext() bool {
	if x != nil {
		return x.HasNext
	}
	return false
}

// Facet provides capabilities to actors and other resources
// Philosophy: Facets augment core functionality without replacing it
type Facet struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Facet type identifier (e.g., "virtual_actor", "otp_genserver", "durable_execution")
	Type string `protobuf:"bytes,1,opt,name=type,proto3" json:"type,omitempty"`
	// Configuration as key-value pairs (all values are strings for simplicity)
	Config map[string]string `protobuf:"bytes,2,rep,name=config,proto3" json:"config,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Priority for facet execution ordering (higher = runs first)
	// Common ranges:
	//
	//	1000+: Security/Auth facets
	//	900-999: Logging/Tracing facets
	//	800-899: Metrics facets
	//	100-500: Domain logic facets
	//	1-99: Persistence facets
	Priority int32 `protobuf:"varint,3,opt,name=priority,proto3" json:"priority,omitempty"`
	// Facet state (for stateful facets)
	State map[string]*anypb.Any `protobuf:"bytes,4,rep,name=state,proto3" json:"state,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Metadata for facet instance
	Metadata      *Metadata `protobuf:"bytes,5,opt,name=metadata,proto3" json:"metadata,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *Facet) Reset() {
	*x = Facet{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[7]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *Facet) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*Facet) ProtoMessage() {}

func (x *Facet) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[7]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use Facet.ProtoReflect.Descriptor instead.
func (*Facet) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{7}
}

func (x *Facet) GetType() string {
	if x != nil {
		return x.Type
	}
	return ""
}

func (x *Facet) GetConfig() map[string]string {
	if x != nil {
		return x.Config
	}
	return nil
}

func (x *Facet) GetPriority() int32 {
	if x != nil {
		return x.Priority
	}
	return 0
}

func (x *Facet) GetState() map[string]*anypb.Any {
	if x != nil {
		return x.State
	}
	return nil
}

func (x *Facet) GetMetadata() *Metadata {
	if x != nil {
		return x.Metadata
	}
	return nil
}

// Facet descriptor for registry/discovery
type FacetDescriptor struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Type          string                 `protobuf:"bytes,1,opt,name=type,proto3" json:"type,omitempty"`
	Description   string                 `protobuf:"bytes,2,opt,name=description,proto3" json:"description,omitempty"`
	Category      string                 `protobuf:"bytes,3,opt,name=category,proto3" json:"category,omitempty"` // e.g., "infrastructure", "virtual_actor", "otp", "workflow"
	ConfigOptions []*ConfigOption        `protobuf:"bytes,4,rep,name=config_options,json=configOptions,proto3" json:"config_options,omitempty"`
	Dependencies  []string               `protobuf:"bytes,5,rep,name=dependencies,proto3" json:"dependencies,omitempty"` // Other facets this depends on
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *FacetDescriptor) Reset() {
	*x = FacetDescriptor{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[8]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *FacetDescriptor) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*FacetDescriptor) ProtoMessage() {}

func (x *FacetDescriptor) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[8]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use FacetDescriptor.ProtoReflect.Descriptor instead.
func (*FacetDescriptor) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{8}
}

func (x *FacetDescriptor) GetType() string {
	if x != nil {
		return x.Type
	}
	return ""
}

func (x *FacetDescriptor) GetDescription() string {
	if x != nil {
		return x.Description
	}
	return ""
}

func (x *FacetDescriptor) GetCategory() string {
	if x != nil {
		return x.Category
	}
	return ""
}

func (x *FacetDescriptor) GetConfigOptions() []*ConfigOption {
	if x != nil {
		return x.ConfigOptions
	}
	return nil
}

func (x *FacetDescriptor) GetDependencies() []string {
	if x != nil {
		return x.Dependencies
	}
	return nil
}

// Configuration option for a facet
type ConfigOption struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Key           string                 `protobuf:"bytes,1,opt,name=key,proto3" json:"key,omitempty"`
	Description   string                 `protobuf:"bytes,2,opt,name=description,proto3" json:"description,omitempty"`
	DefaultValue  string                 `protobuf:"bytes,3,opt,name=default_value,json=defaultValue,proto3" json:"default_value,omitempty"`
	Required      bool                   `protobuf:"varint,4,opt,name=required,proto3" json:"required,omitempty"`
	ValueType     string                 `protobuf:"bytes,5,opt,name=value_type,json=valueType,proto3" json:"value_type,omitempty"` // "string", "int", "bool", "duration", etc.
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ConfigOption) Reset() {
	*x = ConfigOption{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[9]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ConfigOption) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ConfigOption) ProtoMessage() {}

func (x *ConfigOption) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[9]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ConfigOption.ProtoReflect.Descriptor instead.
func (*ConfigOption) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{9}
}

func (x *ConfigOption) GetKey() string {
	if x != nil {
		return x.Key
	}
	return ""
}

func (x *ConfigOption) GetDescription() string {
	if x != nil {
		return x.Description
	}
	return ""
}

func (x *ConfigOption) GetDefaultValue() string {
	if x != nil {
		return x.DefaultValue
	}
	return ""
}

func (x *ConfigOption) GetRequired() bool {
	if x != nil {
		return x.Required
	}
	return false
}

func (x *ConfigOption) GetValueType() string {
	if x != nil {
		return x.ValueType
	}
	return ""
}

// Security policy for tenant isolation
type SecurityPolicy struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Can actors in this tenant send messages to actors in different namespaces?
	// Example: production=false (strict isolation), staging=true (allow cross-talk)
	AllowCrossNamespace bool `protobuf:"varint,1,opt,name=allow_cross_namespace,json=allowCrossNamespace,proto3" json:"allow_cross_namespace,omitempty"`
	// Can actors in this tenant read from TupleSpace?
	AllowTuplespaceRead bool `protobuf:"varint,2,opt,name=allow_tuplespace_read,json=allowTuplespaceRead,proto3" json:"allow_tuplespace_read,omitempty"`
	// Can actors in this tenant write to TupleSpace?
	AllowTuplespaceWrite bool `protobuf:"varint,3,opt,name=allow_tuplespace_write,json=allowTuplespaceWrite,proto3" json:"allow_tuplespace_write,omitempty"`
	// Can actors in this tenant make remote calls to actors on other nodes?
	AllowRemoteCalls bool `protobuf:"varint,4,opt,name=allow_remote_calls,json=allowRemoteCalls,proto3" json:"allow_remote_calls,omitempty"`
	// Maximum message size this tenant can send (bytes)
	// Prevents DoS attacks via large messages
	MaxMessageSizeBytes uint64 `protobuf:"varint,5,opt,name=max_message_size_bytes,json=maxMessageSizeBytes,proto3" json:"max_message_size_bytes,omitempty"` // 1KB to 100MB
	// Allowed facet types this tenant can use
	// Empty = all facets allowed, Non-empty = whitelist only
	AllowedFacetTypes []string `protobuf:"bytes,6,rep,name=allowed_facet_types,json=allowedFacetTypes,proto3" json:"allowed_facet_types,omitempty"`
	// Custom security rules as key-value pairs
	// Example: {"allow_wasm": "true", "encryption_required": "true"}
	CustomRules   map[string]string `protobuf:"bytes,7,rep,name=custom_rules,json=customRules,proto3" json:"custom_rules,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SecurityPolicy) Reset() {
	*x = SecurityPolicy{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[10]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SecurityPolicy) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SecurityPolicy) ProtoMessage() {}

func (x *SecurityPolicy) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[10]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SecurityPolicy.ProtoReflect.Descriptor instead.
func (*SecurityPolicy) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{10}
}

func (x *SecurityPolicy) GetAllowCrossNamespace() bool {
	if x != nil {
		return x.AllowCrossNamespace
	}
	return false
}

func (x *SecurityPolicy) GetAllowTuplespaceRead() bool {
	if x != nil {
		return x.AllowTuplespaceRead
	}
	return false
}

func (x *SecurityPolicy) GetAllowTuplespaceWrite() bool {
	if x != nil {
		return x.AllowTuplespaceWrite
	}
	return false
}

func (x *SecurityPolicy) GetAllowRemoteCalls() bool {
	if x != nil {
		return x.AllowRemoteCalls
	}
	return false
}

func (x *SecurityPolicy) GetMaxMessageSizeBytes() uint64 {
	if x != nil {
		return x.MaxMessageSizeBytes
	}
	return 0
}

func (x *SecurityPolicy) GetAllowedFacetTypes() []string {
	if x != nil {
		return x.AllowedFacetTypes
	}
	return nil
}

func (x *SecurityPolicy) GetCustomRules() map[string]string {
	if x != nil {
		return x.CustomRules
	}
	return nil
}

// Per-tenant resource quotas
type ResourceQuota struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Maximum number of actors this tenant can create
	// 0 = unlimited (use with caution!)
	MaxActors uint32 `protobuf:"varint,1,opt,name=max_actors,json=maxActors,proto3" json:"max_actors,omitempty"` // Max 1 million actors
	// Maximum memory this tenant can consume (MB)
	// Enforced at actor creation and runtime monitoring
	MaxMemoryMb uint64 `protobuf:"varint,2,opt,name=max_memory_mb,json=maxMemoryMb,proto3" json:"max_memory_mb,omitempty"` // Max 1TB
	// Maximum CPU percentage this tenant can consume (0-100)
	// Example: 50.0 = tenant limited to 50% of node CPU
	MaxCpuPercent float64 `protobuf:"fixed64,3,opt,name=max_cpu_percent,json=maxCpuPercent,proto3" json:"max_cpu_percent,omitempty"`
	// Maximum disk space for journals/snapshots (MB)
	MaxDiskMb uint64 `protobuf:"varint,4,opt,name=max_disk_mb,json=maxDiskMb,proto3" json:"max_disk_mb,omitempty"` // Max 10TB
	// Maximum message throughput (messages per second)
	// Enforced via rate limiting at message send
	RateLimitMsgPerSec uint64 `protobuf:"varint,5,opt,name=rate_limit_msg_per_sec,json=rateLimitMsgPerSec,proto3" json:"rate_limit_msg_per_sec,omitempty"` // Max 1M msgs/sec
	// Maximum concurrent operations
	// Limits concurrent handler executions across all actors
	MaxConcurrentOperations uint32 `protobuf:"varint,6,opt,name=max_concurrent_operations,json=maxConcurrentOperations,proto3" json:"max_concurrent_operations,omitempty"` // Max 100K concurrent
	// Custom quota limits as key-value pairs
	// Example: {"max_tuplespace_entries": "10000"}
	CustomQuotas  map[string]string `protobuf:"bytes,7,rep,name=custom_quotas,json=customQuotas,proto3" json:"custom_quotas,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ResourceQuota) Reset() {
	*x = ResourceQuota{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[11]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ResourceQuota) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ResourceQuota) ProtoMessage() {}

func (x *ResourceQuota) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[11]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ResourceQuota.ProtoReflect.Descriptor instead.
func (*ResourceQuota) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{11}
}

func (x *ResourceQuota) GetMaxActors() uint32 {
	if x != nil {
		return x.MaxActors
	}
	return 0
}

func (x *ResourceQuota) GetMaxMemoryMb() uint64 {
	if x != nil {
		return x.MaxMemoryMb
	}
	return 0
}

func (x *ResourceQuota) GetMaxCpuPercent() float64 {
	if x != nil {
		return x.MaxCpuPercent
	}
	return 0
}

func (x *ResourceQuota) GetMaxDiskMb() uint64 {
	if x != nil {
		return x.MaxDiskMb
	}
	return 0
}

func (x *ResourceQuota) GetRateLimitMsgPerSec() uint64 {
	if x != nil {
		return x.RateLimitMsgPerSec
	}
	return 0
}

func (x *ResourceQuota) GetMaxConcurrentOperations() uint32 {
	if x != nil {
		return x.MaxConcurrentOperations
	}
	return 0
}

func (x *ResourceQuota) GetCustomQuotas() map[string]string {
	if x != nil {
		return x.CustomQuotas
	}
	return nil
}

// Resource specification (CPU, memory, disk, GPU)
// Shared by nodes, actors, and scheduling components
type ResourceSpec struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// CPU cores (fractional allowed, e.g., 0.5 = half core)
	// Example: 2.5 = two and a half CPU cores
	CpuCores float64 `protobuf:"fixed64,1,opt,name=cpu_cores,json=cpuCores,proto3" json:"cpu_cores,omitempty"`
	// Memory in bytes
	// Example: 1073741824 = 1GB
	MemoryBytes uint64 `protobuf:"varint,2,opt,name=memory_bytes,json=memoryBytes,proto3" json:"memory_bytes,omitempty"`
	// Disk space in bytes
	// Example: 10737418240 = 10GB
	DiskBytes uint64 `protobuf:"varint,3,opt,name=disk_bytes,json=diskBytes,proto3" json:"disk_bytes,omitempty"`
	// GPU count (0 = no GPU)
	// Example: 1 = one GPU
	GpuCount uint32 `protobuf:"varint,4,opt,name=gpu_count,json=gpuCount,proto3" json:"gpu_count,omitempty"`
	// GPU type (e.g., "H100", "A100", "L4", "T4")
	// Empty string = any GPU type
	GpuType       string `protobuf:"bytes,5,opt,name=gpu_type,json=gpuType,proto3" json:"gpu_type,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ResourceSpec) Reset() {
	*x = ResourceSpec{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[12]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ResourceSpec) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ResourceSpec) ProtoMessage() {}

func (x *ResourceSpec) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[12]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ResourceSpec.ProtoReflect.Descriptor instead.
func (*ResourceSpec) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{12}
}

func (x *ResourceSpec) GetCpuCores() float64 {
	if x != nil {
		return x.CpuCores
	}
	return 0
}

func (x *ResourceSpec) GetMemoryBytes() uint64 {
	if x != nil {
		return x.MemoryBytes
	}
	return 0
}

func (x *ResourceSpec) GetDiskBytes() uint64 {
	if x != nil {
		return x.DiskBytes
	}
	return 0
}

func (x *ResourceSpec) GetGpuCount() uint32 {
	if x != nil {
		return x.GpuCount
	}
	return 0
}

func (x *ResourceSpec) GetGpuType() string {
	if x != nil {
		return x.GpuType
	}
	return ""
}

// Request Context (Go-style context.Context)
//
// ## Purpose
// Provides request-scoped context similar to Go's context.Context.
// Carries tenant isolation, tracing, and request metadata through the call chain.
//
// ## Design Philosophy
// - **Tenant Isolation**: tenant_id is REQUIRED for all operations
// - **Tracing**: request_id and correlation_id for distributed tracing
// - **Extensible**: metadata map for additional context
// - **Immutable**: Context should be passed by reference, not mutated
//
// ## Usage Pattern
// ```rust
// // Create context from request
// let ctx = RequestContext::new("tenant-123".to_string())
//
//	.with_namespace("production".to_string())
//	.with_user_id("user-456".to_string());
//
// // Pass to repository/service
// let result = repository.get(&ctx, "resource-id").await?;
// ```
type RequestContext struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Tenant ID (REQUIRED for all operations)
	//
	// All operations are scoped to this tenant.
	// Must be validated at service boundaries.
	TenantId string `protobuf:"bytes,1,opt,name=tenant_id,json=tenantId,proto3" json:"tenant_id,omitempty"`
	// Namespace within tenant (optional, can be empty)
	//
	// Used for further isolation within a tenant. Can be empty string.
	// Common values: "production", "staging", "dev", "test"
	// For admin/internal contexts with empty namespace, repository lookups
	// bypass namespace filtering to allow cross-namespace queries.
	Namespace string `protobuf:"bytes,2,opt,name=namespace,proto3" json:"namespace,omitempty"`
	// User ID (from JWT, optional)
	//
	// Extracted from JWT claims for audit logging and authorization.
	UserId string `protobuf:"bytes,3,opt,name=user_id,json=userId,proto3" json:"user_id,omitempty"`
	// Request ID (for tracing)
	//
	// Unique identifier for this request (ULID).
	// Used for request tracing and correlation.
	RequestId string `protobuf:"bytes,4,opt,name=request_id,json=requestId,proto3" json:"request_id,omitempty"`
	// Correlation ID (for distributed tracing)
	//
	// Links related requests across services.
	// Propagated through gRPC metadata.
	CorrelationId string `protobuf:"bytes,5,opt,name=correlation_id,json=correlationId,proto3" json:"correlation_id,omitempty"`
	// Request timestamp
	Timestamp *timestamppb.Timestamp `protobuf:"bytes,6,opt,name=timestamp,proto3" json:"timestamp,omitempty"`
	// Metadata (extensible key-value pairs)
	//
	// Additional context for request processing.
	// Examples: "source_ip", "user_agent", "api_version"
	Metadata map[string]string `protobuf:"bytes,7,rep,name=metadata,proto3" json:"metadata,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// HTTP-style headers for auth credential propagation (OpenAPI securitySchemes pattern)
	//
	// Carries authorization headers and security credentials through the call chain.
	// Header names are stored lowercase (HTTP/2 convention). Common entries:
	//
	// | Header              | OpenAPI Equivalent                    |
	// |---------------------|---------------------------------------|
	// | authorization       | bearerAuth (type: http, scheme: bearer) |
	// | x-api-key           | apiKey (in: header)                   |
	// | apikey-query:<name> | apiKey (in: query)                    |
	// | any custom header   | custom securityScheme                 |
	//
	// Security: When auth is enabled, the 'authorization' header is set from
	// validated JWT only — never from client-supplied headers.
	Headers map[string]string `protobuf:"bytes,11,rep,name=headers,proto3" json:"headers,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Admin flag (from JWT, optional)
	//
	// When true, indicates the user has admin privileges.
	// Admin users with empty namespace can bypass namespace filtering for
	// administrative operations (see should_skip_namespace_filter()).
	// Extracted from JWT claims (e.g., "admin" role or "is_admin" claim).
	Admin bool `protobuf:"varint,8,opt,name=admin,proto3" json:"admin,omitempty"`
	// Internal flag (for system operations)
	//
	// When true, indicates this is an internal system operation.
	// Internal operations bypass authn/authz and tenant filtering.
	// Internal contexts with empty namespace can bypass namespace filtering
	// for system operations (see should_skip_namespace_filter()).
	// Used for system-level operations like heartbeats, node registration, etc.
	Internal bool `protobuf:"varint,9,opt,name=internal,proto3" json:"internal,omitempty"`
	// Auth enabled flag (from SecurityConfig)
	//
	// When true, indicates authentication is enabled.
	// If auth is enabled and tenant_id is empty, RequestContext creation will fail.
	// If auth is disabled, tenant_id can be empty.
	AuthEnabled   bool `protobuf:"varint,10,opt,name=auth_enabled,json=authEnabled,proto3" json:"auth_enabled,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *RequestContext) Reset() {
	*x = RequestContext{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[13]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *RequestContext) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*RequestContext) ProtoMessage() {}

func (x *RequestContext) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[13]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use RequestContext.ProtoReflect.Descriptor instead.
func (*RequestContext) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{13}
}

func (x *RequestContext) GetTenantId() string {
	if x != nil {
		return x.TenantId
	}
	return ""
}

func (x *RequestContext) GetNamespace() string {
	if x != nil {
		return x.Namespace
	}
	return ""
}

func (x *RequestContext) GetUserId() string {
	if x != nil {
		return x.UserId
	}
	return ""
}

func (x *RequestContext) GetRequestId() string {
	if x != nil {
		return x.RequestId
	}
	return ""
}

func (x *RequestContext) GetCorrelationId() string {
	if x != nil {
		return x.CorrelationId
	}
	return ""
}

func (x *RequestContext) GetTimestamp() *timestamppb.Timestamp {
	if x != nil {
		return x.Timestamp
	}
	return nil
}

func (x *RequestContext) GetMetadata() map[string]string {
	if x != nil {
		return x.Metadata
	}
	return nil
}

func (x *RequestContext) GetHeaders() map[string]string {
	if x != nil {
		return x.Headers
	}
	return nil
}

func (x *RequestContext) GetAdmin() bool {
	if x != nil {
		return x.Admin
	}
	return false
}

func (x *RequestContext) GetInternal() bool {
	if x != nil {
		return x.Internal
	}
	return false
}

func (x *RequestContext) GetAuthEnabled() bool {
	if x != nil {
		return x.AuthEnabled
	}
	return false
}

// Unified message envelope for all PlexSpaces communication
type Message struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Unique message identifier (ULID format)
	// Generated by sender if not provided
	// Example: "01HN9QV1W6EZGQC0P9XYZMR4M1"
	Id string `protobuf:"bytes,1,opt,name=id,proto3" json:"id,omitempty"`
	// Canonical ActorId string or temporary sender routing ID.
	// Temporary senders use canonical ActorId strings; there is no separate temp syntax.
	SenderId string `protobuf:"bytes,2,opt,name=sender_id,json=senderId,proto3" json:"sender_id,omitempty"`
	// Canonical ActorId string or temporary sender routing ID for replies.
	// Note: For pub/sub, use 'channel' field instead
	ReceiverId string `protobuf:"bytes,3,opt,name=receiver_id,json=receiverId,proto3" json:"receiver_id,omitempty"`
	// Channel/topic name (for pub/sub and channel messaging)
	// Example: "orders.created", "user-events", "chat-room-42"
	// Note: For actor messaging, use 'receiver_id' instead
	Channel string `protobuf:"bytes,4,opt,name=channel,proto3" json:"channel,omitempty"`
	// Message type discriminator
	// Common values: "call", "cast", "info", "signal", "event", "command", "query"
	// Example: "call" for request/reply, "cast" for fire-and-forget
	// Empty = unset. Validated to allowed values only.
	MessageType string `protobuf:"bytes,5,opt,name=message_type,json=messageType,proto3" json:"message_type,omitempty"`
	// Message payload (opaque bytes)
	// Encoding is application-specific (JSON, protobuf, msgpack, etc.)
	Payload []byte `protobuf:"bytes,6,opt,name=payload,proto3" json:"payload,omitempty"`
	// Message timestamp (when created)
	// Auto-populated if not provided
	Timestamp *timestamppb.Timestamp `protobuf:"bytes,7,opt,name=timestamp,proto3" json:"timestamp,omitempty"`
	// Custom headers/metadata (extensible key-value pairs)
	// Examples: "content-type", "x-trace-id", "x-request-id"
	Headers map[string]string `protobuf:"bytes,8,rep,name=headers,proto3" json:"headers,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Message priority (0=Low, 25=Normal, 50=High, 75=System, 100=Signal)
	// Higher priority messages are delivered first
	Priority int32 `protobuf:"varint,9,opt,name=priority,proto3" json:"priority,omitempty"`
	// Time-to-live for message expiration
	// Message is discarded if not delivered within TTL (max 24 hours)
	Ttl *durationpb.Duration `protobuf:"bytes,10,opt,name=ttl,proto3" json:"ttl,omitempty"`
	// Delivery attempt count (for retry tracking)
	// Incremented by message brokers on each delivery attempt
	DeliveryCount uint32 `protobuf:"varint,11,opt,name=delivery_count,json=deliveryCount,proto3" json:"delivery_count,omitempty"`
	// Idempotency key for message deduplication
	// Messages with same key within time window are de-duplicated
	// Example: "payment-request-abc-xyz"
	IdempotencyKey string `protobuf:"bytes,12,opt,name=idempotency_key,json=idempotencyKey,proto3" json:"idempotency_key,omitempty"`
	// Correlation ID for request/reply patterns
	// Links response to original request (distributed tracing)
	// Example: "req-01HN9QV1W6EZGQC0P9XYZMR4M1"
	CorrelationId string `protobuf:"bytes,13,opt,name=correlation_id,json=correlationId,proto3" json:"correlation_id,omitempty"`
	// Reply-to address (channel or actor ID for responses)
	// Tells receiver where to send the response
	// Example: "response-queue-42", "callback-actor"
	ReplyTo string `protobuf:"bytes,14,opt,name=reply_to,json=replyTo,proto3" json:"reply_to,omitempty"`
	// Partition key for ordered delivery (Kafka, Redis Streams)
	// Messages with same partition_key go to same partition (FIFO within partition)
	// Example: "user-123" (all messages for user-123 delivered in order)
	PartitionKey string `protobuf:"bytes,15,opt,name=partition_key,json=partitionKey,proto3" json:"partition_key,omitempty"`
	// URI path for HTTP-based requests (optional)
	// Populated when message originates from HTTP gateway
	// Example: "/api/v1/actors/default/counter/metrics"
	UriPath string `protobuf:"bytes,16,opt,name=uri_path,json=uriPath,proto3" json:"uri_path,omitempty"`
	// HTTP method for HTTP-based requests (optional)
	// Example: "GET", "POST", "PUT", "DELETE"
	UriMethod     string `protobuf:"bytes,17,opt,name=uri_method,json=uriMethod,proto3" json:"uri_method,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *Message) Reset() {
	*x = Message{}
	mi := &file_plexspaces_v1_common_proto_msgTypes[14]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *Message) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*Message) ProtoMessage() {}

func (x *Message) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_common_proto_msgTypes[14]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use Message.ProtoReflect.Descriptor instead.
func (*Message) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_common_proto_rawDescGZIP(), []int{14}
}

func (x *Message) GetId() string {
	if x != nil {
		return x.Id
	}
	return ""
}

func (x *Message) GetSenderId() string {
	if x != nil {
		return x.SenderId
	}
	return ""
}

func (x *Message) GetReceiverId() string {
	if x != nil {
		return x.ReceiverId
	}
	return ""
}

func (x *Message) GetChannel() string {
	if x != nil {
		return x.Channel
	}
	return ""
}

func (x *Message) GetMessageType() string {
	if x != nil {
		return x.MessageType
	}
	return ""
}

func (x *Message) GetPayload() []byte {
	if x != nil {
		return x.Payload
	}
	return nil
}

func (x *Message) GetTimestamp() *timestamppb.Timestamp {
	if x != nil {
		return x.Timestamp
	}
	return nil
}

func (x *Message) GetHeaders() map[string]string {
	if x != nil {
		return x.Headers
	}
	return nil
}

func (x *Message) GetPriority() int32 {
	if x != nil {
		return x.Priority
	}
	return 0
}

func (x *Message) GetTtl() *durationpb.Duration {
	if x != nil {
		return x.Ttl
	}
	return nil
}

func (x *Message) GetDeliveryCount() uint32 {
	if x != nil {
		return x.DeliveryCount
	}
	return 0
}

func (x *Message) GetIdempotencyKey() string {
	if x != nil {
		return x.IdempotencyKey
	}
	return ""
}

func (x *Message) GetCorrelationId() string {
	if x != nil {
		return x.CorrelationId
	}
	return ""
}

func (x *Message) GetReplyTo() string {
	if x != nil {
		return x.ReplyTo
	}
	return ""
}

func (x *Message) GetPartitionKey() string {
	if x != nil {
		return x.PartitionKey
	}
	return ""
}

func (x *Message) GetUriPath() string {
	if x != nil {
		return x.UriPath
	}
	return ""
}

func (x *Message) GetUriMethod() string {
	if x != nil {
		return x.UriMethod
	}
	return ""
}

var File_plexspaces_v1_common_proto protoreflect.FileDescriptor

const file_plexspaces_v1_common_proto_rawDesc = "" +
	"\n" +
	"\x1aplexspaces/v1/common.proto\x12\x14plexspaces.common.v1\x1a\x1bbuf/validate/validate.proto\x1a\x1fgoogle/api/field_behavior.proto\x1a\x19google/protobuf/any.proto\x1a\x1egoogle/protobuf/duration.proto\x1a\x1cgoogle/protobuf/struct.proto\x1a\x1fgoogle/protobuf/timestamp.proto\x1a.protoc-gen-openapiv2/options/annotations.proto\"\a\n" +
	"\x05Empty\"\xb7\x04\n" +
	"\bMetadata\x12@\n" +
	"\vcreate_time\x18\x01 \x01(\v2\x1a.google.protobuf.TimestampB\x03\xe0A\x03R\n" +
	"createTime\x12@\n" +
	"\vupdate_time\x18\x02 \x01(\v2\x1a.google.protobuf.TimestampB\x03\xe0A\x03R\n" +
	"updateTime\x12\"\n" +
	"\n" +
	"created_by\x18\x03 \x01(\tB\x03\xe0A\x03R\tcreatedBy\x12\"\n" +
	"\n" +
	"updated_by\x18\x04 \x01(\tB\x03\xe0A\x03R\tupdatedBy\x12B\n" +
	"\x06labels\x18\x05 \x03(\v2*.plexspaces.common.v1.Metadata.LabelsEntryR\x06labels\x12Q\n" +
	"\vannotations\x18\x06 \x03(\v2/.plexspaces.common.v1.Metadata.AnnotationsEntryR\vannotations\x1a9\n" +
	"\vLabelsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\x1aT\n" +
	"\x10AnnotationsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12*\n" +
	"\x05value\x18\x02 \x01(\v2\x14.google.protobuf.AnyR\x05value:\x028\x01:7\x92A4\n" +
	"2*\bMetadata2&Standard metadata fields for resources\"\xe0\x01\n" +
	"\aActorId\x12>\n" +
	"\x04name\x18\x01 \x01(\tB*\xe0A\x02\xbaH$r\"\x10\x01\x18\x80\x012\x1b^[a-zA-Z0-9][a-zA-Z0-9_-]*$R\x04name\x12@\n" +
	"\n" +
	"actor_type\x18\x02 \x01(\tB!\xe0A\x02\xbaH\x1br\x19\x10\x01\x18\x80\x012\x12^[a-z][a-z0-9_-]*$R\tactorType\x12+\n" +
	"\tnamespace\x18\x03 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\x80\x01R\tnamespace\x12&\n" +
	"\anode_id\x18\x04 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\x06nodeId\"\xf9\x01\n" +
	"\vErrorDetail\x12(\n" +
	"\x04code\x18\x01 \x01(\tB\x14\xbaH\x11r\x0f\x10\x01\x18@2\t^[A-Z_]+$R\x04code\x12$\n" +
	"\amessage\x18\x02 \x01(\tB\n" +
	"\xbaH\ar\x05\x10\x01\x18\x80\bR\amessage\x12H\n" +
	"\adetails\x18\x03 \x03(\v2..plexspaces.common.v1.ErrorDetail.DetailsEntryR\adetails\x1aP\n" +
	"\fDetailsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12*\n" +
	"\x05value\x18\x02 \x01(\v2\x14.google.protobuf.AnyR\x05value:\x028\x01\"\xd0\x02\n" +
	"\vRetryPolicy\x12/\n" +
	"\fmax_attempts\x18\x01 \x01(\rB\f\xe0A\x02\xbaH\x06*\x04\x18d(\x01R\vmaxAttempts\x12F\n" +
	"\x12backoff_multiplier\x18\x02 \x01(\x01B\x17\xbaH\x14\x12\x12\x19\x00\x00\x00\x00\x00\x00$@)\x00\x00\x00\x00\x00\x00\xf0?R\x11backoffMultiplier\x12L\n" +
	"\rinitial_delay\x18\x03 \x01(\v2\x19.google.protobuf.DurationB\f\xbaH\t\xaa\x01\x062\x04\x10\xc0\x84=R\finitialDelay\x12C\n" +
	"\tmax_delay\x18\x04 \x01(\v2\x19.google.protobuf.DurationB\v\xbaH\b\xaa\x01\x05\"\x03\b\x90\x1cR\bmaxDelay:5\x92A2\n" +
	"0*\fRetry Policy2 Configuration for retry behavior\"\xe2\x01\n" +
	"\vPageRequest\x12\x1f\n" +
	"\x06offset\x18\x01 \x01(\x05B\a\xbaH\x04\x1a\x02(\x00R\x06offset\x12 \n" +
	"\x05limit\x18\x02 \x01(\x05B\n" +
	"\xbaH\a\x1a\x05\x18\xe8\a(\x01R\x05limit\x12 \n" +
	"\x06filter\x18\x03 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x10R\x06filter\x12#\n" +
	"\border_by\x18\x04 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x02R\aorderBy:I\x92AF\n" +
	"D*\fPage Request24Standard pagination parameters with offset and limit\"\xb3\x01\n" +
	"\fPageResponse\x12\x1d\n" +
	"\n" +
	"total_size\x18\x01 \x01(\x05R\ttotalSize\x12\x16\n" +
	"\x06offset\x18\x02 \x01(\x05R\x06offset\x12\x14\n" +
	"\x05limit\x18\x03 \x01(\x05R\x05limit\x12\x19\n" +
	"\bhas_next\x18\x04 \x01(\bR\ahasNext:;\x92A8\n" +
	"6*\rPage Response2%Standard pagination response metadata\"\xf3\x03\n" +
	"\x05Facet\x124\n" +
	"\x04type\x18\x01 \x01(\tB \xe0A\x02\xbaH\x1ar\x18\x10\x01\x18\xff\x012\x11^[a-z][a-z0-9_]*$R\x04type\x12?\n" +
	"\x06config\x18\x02 \x03(\v2'.plexspaces.common.v1.Facet.ConfigEntryR\x06config\x12&\n" +
	"\bpriority\x18\x03 \x01(\x05B\n" +
	"\xbaH\a\x1a\x05\x18\x90N(\x00R\bpriority\x12<\n" +
	"\x05state\x18\x04 \x03(\v2&.plexspaces.common.v1.Facet.StateEntryR\x05state\x12:\n" +
	"\bmetadata\x18\x05 \x01(\v2\x1e.plexspaces.common.v1.MetadataR\bmetadata\x1a9\n" +
	"\vConfigEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\x1aN\n" +
	"\n" +
	"StateEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12*\n" +
	"\x05value\x18\x02 \x01(\v2\x14.google.protobuf.AnyR\x05value:\x028\x01:F\x92AC\n" +
	"A*\x05Facet21Composable capability that extends actor behavior\xd2\x01\x04type\"\xe4\x02\n" +
	"\x0fFacetDescriptor\x121\n" +
	"\x04type\x18\x01 \x01(\tB\x1d\xbaH\x1ar\x18\x10\x01\x18\xff\x012\x11^[a-z][a-z0-9_]*$R\x04type\x12*\n" +
	"\vdescription\x18\x02 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x10R\vdescription\x12$\n" +
	"\bcategory\x18\x03 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x01R\bcategory\x12I\n" +
	"\x0econfig_options\x18\x04 \x03(\v2\".plexspaces.common.v1.ConfigOptionR\rconfigOptions\x12F\n" +
	"\fdependencies\x18\x05 \x03(\tB\"\xbaH\x1f\x92\x01\x1c\"\x1ar\x18\x10\x01\x18\xff\x012\x11^[a-z][a-z0-9_]*$R\fdependencies:9\x92A6\n" +
	"4*\x10Facet Descriptor2 Metadata describing a facet type\"\xe0\x01\n" +
	"\fConfigOption\x12/\n" +
	"\x03key\x18\x01 \x01(\tB\x1d\xbaH\x1ar\x18\x10\x01\x18\x80\x012\x11^[a-z][a-z0-9_]*$R\x03key\x12*\n" +
	"\vdescription\x18\x02 \x01(\tB\b\xbaH\x05r\x03\x18\x80\bR\vdescription\x12-\n" +
	"\rdefault_value\x18\x03 \x01(\tB\b\xbaH\x05r\x03\x18\x80\bR\fdefaultValue\x12\x1a\n" +
	"\brequired\x18\x04 \x01(\bR\brequired\x12(\n" +
	"\n" +
	"value_type\x18\x05 \x01(\tB\t\xbaH\x06r\x04\x10\x01\x18@R\tvalueType\"\xd1\x04\n" +
	"\x0eSecurityPolicy\x122\n" +
	"\x15allow_cross_namespace\x18\x01 \x01(\bR\x13allowCrossNamespace\x122\n" +
	"\x15allow_tuplespace_read\x18\x02 \x01(\bR\x13allowTuplespaceRead\x124\n" +
	"\x16allow_tuplespace_write\x18\x03 \x01(\bR\x14allowTuplespaceWrite\x12,\n" +
	"\x12allow_remote_calls\x18\x04 \x01(\bR\x10allowRemoteCalls\x12B\n" +
	"\x16max_message_size_bytes\x18\x05 \x01(\x04B\r\xbaH\n" +
	"2\b\x18\x80\x80\x802(\x80\bR\x13maxMessageSizeBytes\x12R\n" +
	"\x13allowed_facet_types\x18\x06 \x03(\tB\"\xbaH\x1f\x92\x01\x1c\"\x1ar\x18\x10\x01\x18\xff\x012\x11^[a-z][a-z0-9_]*$R\x11allowedFacetTypes\x12X\n" +
	"\fcustom_rules\x18\a \x03(\v25.plexspaces.common.v1.SecurityPolicy.CustomRulesEntryR\vcustomRules\x1a>\n" +
	"\x10CustomRulesEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01:A\x92A>\n" +
	"<*\x0fSecurity Policy2)Per-tenant security rules and permissions\"\xc1\x04\n" +
	"\rResourceQuota\x12(\n" +
	"\n" +
	"max_actors\x18\x01 \x01(\rB\t\xbaH\x06*\x04\x18\xc0\x84=R\tmaxActors\x12-\n" +
	"\rmax_memory_mb\x18\x02 \x01(\x04B\t\xbaH\x062\x04\x18\x80\x80@R\vmaxMemoryMb\x12?\n" +
	"\x0fmax_cpu_percent\x18\x03 \x01(\x01B\x17\xbaH\x14\x12\x12\x19\x00\x00\x00\x00\x00\x00Y@)\x00\x00\x00\x00\x00\x00\x00\x00R\rmaxCpuPercent\x12*\n" +
	"\vmax_disk_mb\x18\x04 \x01(\x04B\n" +
	"\xbaH\a2\x05\x18\x80\x80\x80\x05R\tmaxDiskMb\x12=\n" +
	"\x16rate_limit_msg_per_sec\x18\x05 \x01(\x04B\t\xbaH\x062\x04\x18\xc0\x84=R\x12rateLimitMsgPerSec\x12E\n" +
	"\x19max_concurrent_operations\x18\x06 \x01(\rB\t\xbaH\x06*\x04\x18\xa0\x8d\x06R\x17maxConcurrentOperations\x12Z\n" +
	"\rcustom_quotas\x18\a \x03(\v25.plexspaces.common.v1.ResourceQuota.CustomQuotasEntryR\fcustomQuotas\x1a?\n" +
	"\x11CustomQuotasEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01:G\x92AD\n" +
	"B*\x0eResource Quota20Per-tenant resource limits to prevent exhaustion\"\x90\x02\n" +
	"\fResourceSpec\x12+\n" +
	"\tcpu_cores\x18\x01 \x01(\x01B\x0e\xbaH\v\x12\t)\x00\x00\x00\x00\x00\x00\x00\x00R\bcpuCores\x12!\n" +
	"\fmemory_bytes\x18\x02 \x01(\x04R\vmemoryBytes\x12\x1d\n" +
	"\n" +
	"disk_bytes\x18\x03 \x01(\x04R\tdiskBytes\x12\x1b\n" +
	"\tgpu_count\x18\x04 \x01(\rR\bgpuCount\x12\"\n" +
	"\bgpu_type\x18\x05 \x01(\tB\a\xbaH\x04r\x02\x18@R\agpuType:P\x92AM\n" +
	"K*\x16Resource Specification21CPU, memory, disk, and GPU resource specification\"\xe1\x05\n" +
	"\x0eRequestContext\x12)\n" +
	"\ttenant_id\x18\x01 \x01(\tB\f\xe0A\x02\xbaH\x06r\x04\x10\x01\x18@R\btenantId\x12%\n" +
	"\tnamespace\x18\x02 \x01(\tB\a\xbaH\x04r\x02\x18 R\tnamespace\x12!\n" +
	"\auser_id\x18\x03 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x01R\x06userId\x12(\n" +
	"\n" +
	"request_id\x18\x04 \x01(\tB\t\xbaH\x06r\x04\x10\x01\x18@R\trequestId\x12.\n" +
	"\x0ecorrelation_id\x18\x05 \x01(\tB\a\xbaH\x04r\x02\x18@R\rcorrelationId\x128\n" +
	"\ttimestamp\x18\x06 \x01(\v2\x1a.google.protobuf.TimestampR\ttimestamp\x12N\n" +
	"\bmetadata\x18\a \x03(\v22.plexspaces.common.v1.RequestContext.MetadataEntryR\bmetadata\x12K\n" +
	"\aheaders\x18\v \x03(\v21.plexspaces.common.v1.RequestContext.HeadersEntryR\aheaders\x12\x14\n" +
	"\x05admin\x18\b \x01(\bR\x05admin\x12\x1a\n" +
	"\binternal\x18\t \x01(\bR\binternal\x12!\n" +
	"\fauth_enabled\x18\n" +
	" \x01(\bR\vauthEnabled\x1a;\n" +
	"\rMetadataEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\x1a:\n" +
	"\fHeadersEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01:[\x92AX\n" +
	"V*\x0fRequest Context27Request-scoped context for tenant isolation and tracing\xd2\x01\ttenant_id\"\xd2\a\n" +
	"\aMessage\x12\x1d\n" +
	"\x02id\x18\x01 \x01(\tB\r\xe0A\x02\xbaH\ar\x05\x10\x01\x18\xff\x01R\x02id\x12%\n" +
	"\tsender_id\x18\x02 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\bsenderId\x12)\n" +
	"\vreceiver_id\x18\x03 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\n" +
	"receiverId\x12\"\n" +
	"\achannel\x18\x04 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\achannel\x12[\n" +
	"\fmessage_type\x18\x05 \x01(\tB8\xbaH5r3R\x00R\x04callR\x04castR\x04infoR\x06signalR\x05eventR\acommandR\x05queryR\vmessageType\x12\x1d\n" +
	"\apayload\x18\x06 \x01(\fB\x03\xe0A\x02R\apayload\x12=\n" +
	"\ttimestamp\x18\a \x01(\v2\x1a.google.protobuf.TimestampB\x03\xe0A\x03R\ttimestamp\x12D\n" +
	"\aheaders\x18\b \x03(\v2*.plexspaces.common.v1.Message.HeadersEntryR\aheaders\x12%\n" +
	"\bpriority\x18\t \x01(\x05B\t\xbaH\x06\x1a\x04\x18d(\x00R\bpriority\x129\n" +
	"\x03ttl\x18\n" +
	" \x01(\v2\x19.google.protobuf.DurationB\f\xbaH\t\xaa\x01\x06\"\x04\b\x80\xa3\x05R\x03ttl\x12%\n" +
	"\x0edelivery_count\x18\v \x01(\rR\rdeliveryCount\x121\n" +
	"\x0fidempotency_key\x18\f \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\x0eidempotencyKey\x12/\n" +
	"\x0ecorrelation_id\x18\r \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\rcorrelationId\x12#\n" +
	"\breply_to\x18\x0e \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\areplyTo\x12-\n" +
	"\rpartition_key\x18\x0f \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\fpartitionKey\x12#\n" +
	"\buri_path\x18\x10 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x10R\auriPath\x12&\n" +
	"\n" +
	"uri_method\x18\x11 \x01(\tB\a\xbaH\x04r\x02\x18\x10R\turiMethod\x1a:\n" +
	"\fHeadersEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01:g\x92Ad\n" +
	"b*\aMessage2HUniversal message envelope for actor, channel, and pub/sub communication\xd2\x01\x02id\xd2\x01\apayload*\x97\x01\n" +
	"\x12ActivationStrategy\x12#\n" +
	"\x1fACTIVATION_STRATEGY_UNSPECIFIED\x10\x00\x12\x1c\n" +
	"\x18ACTIVATION_STRATEGY_LAZY\x10\x01\x12\x1d\n" +
	"\x19ACTIVATION_STRATEGY_EAGER\x10\x02\x12\x1f\n" +
	"\x1bACTIVATION_STRATEGY_PREWARM\x10\x03*n\n" +
	"\bQoSLevel\x12\x19\n" +
	"\x15QOS_LEVEL_UNSPECIFIED\x10\x00\x12\x12\n" +
	"\x0eQOS_LEVEL_NONE\x10\x01\x12\x19\n" +
	"\x15QOS_LEVEL_BEST_EFFORT\x10\x02\x12\x18\n" +
	"\x14QOS_LEVEL_GUARANTEED\x10\x03*\xf5\x01\n" +
	"\rResourceState\x12\x1e\n" +
	"\x1aRESOURCE_STATE_UNSPECIFIED\x10\x00\x12\x1b\n" +
	"\x17RESOURCE_STATE_CREATING\x10\x01\x12\x19\n" +
	"\x15RESOURCE_STATE_ACTIVE\x10\x02\x12\x1b\n" +
	"\x17RESOURCE_STATE_INACTIVE\x10\x03\x12\x1b\n" +
	"\x17RESOURCE_STATE_UPDATING\x10\x04\x12\x1b\n" +
	"\x17RESOURCE_STATE_DELETING\x10\x05\x12\x19\n" +
	"\x15RESOURCE_STATE_FAILED\x10\x06\x12\x1a\n" +
	"\x16RESOURCE_STATE_UNKNOWN\x10\aB\xa9\x06\x92A\xba\x04\x12Z\n" +
	"\x16PlexSpace Common Types\x12;Common data types and utilities for the PlexSpace framework2\x031.0*\x01\x022\x10application/json:\x10application/jsonZ\xa2\x03\n" +
	"q\n" +
	"\fApiKeyHeader\x12a\b\x02\x12PAPI key authentication via header. Service-scoped keys with configurable scopes.\x1a\tX-API-Key \x02\n" +
	"n\n" +
	"\vApiKeyQuery\x12_\b\x02\x12PAPI key authentication via query parameter. Use only when headers cannot be set.\x1a\aapi_key \x01\n" +
	"\xbc\x01\n" +
	"\n" +
	"BearerAuth\x12\xad\x01\b\x02\x12\x97\x01JWT Bearer token. Format: `Bearer <token>`. Must contain a `tenant_id` claim for multi-tenant isolation. Create tokens via `plexspaces-cli jwt create`.\x1a\rAuthorization \x02b\x10\n" +
	"\x0e\n" +
	"\n" +
	"BearerAuth\x12\x00\n" +
	"\x18com.plexspaces.common.v1B\vCommonProtoP\x01ZPgithub.com/plexobject/plexspaces/sdks/go/plexspaces/proto/plexspaces/v1;commonv1\xa2\x02\x03PCX\xaa\x02\x14Plexspaces.Common.V1\xca\x02\x14Plexspaces\\Common\\V1\xe2\x02 Plexspaces\\Common\\V1\\GPBMetadata\xea\x02\x16Plexspaces::Common::V1b\x06proto3"

var (
	file_plexspaces_v1_common_proto_rawDescOnce sync.Once
	file_plexspaces_v1_common_proto_rawDescData []byte
)

func file_plexspaces_v1_common_proto_rawDescGZIP() []byte {
	file_plexspaces_v1_common_proto_rawDescOnce.Do(func() {
		file_plexspaces_v1_common_proto_rawDescData = protoimpl.X.CompressGZIP(unsafe.Slice(unsafe.StringData(file_plexspaces_v1_common_proto_rawDesc), len(file_plexspaces_v1_common_proto_rawDesc)))
	})
	return file_plexspaces_v1_common_proto_rawDescData
}

var file_plexspaces_v1_common_proto_enumTypes = make([]protoimpl.EnumInfo, 3)
var file_plexspaces_v1_common_proto_msgTypes = make([]protoimpl.MessageInfo, 25)
var file_plexspaces_v1_common_proto_goTypes = []any{
	(ActivationStrategy)(0),       // 0: plexspaces.common.v1.ActivationStrategy
	(QoSLevel)(0),                 // 1: plexspaces.common.v1.QoSLevel
	(ResourceState)(0),            // 2: plexspaces.common.v1.ResourceState
	(*Empty)(nil),                 // 3: plexspaces.common.v1.Empty
	(*Metadata)(nil),              // 4: plexspaces.common.v1.Metadata
	(*ActorId)(nil),               // 5: plexspaces.common.v1.ActorId
	(*ErrorDetail)(nil),           // 6: plexspaces.common.v1.ErrorDetail
	(*RetryPolicy)(nil),           // 7: plexspaces.common.v1.RetryPolicy
	(*PageRequest)(nil),           // 8: plexspaces.common.v1.PageRequest
	(*PageResponse)(nil),          // 9: plexspaces.common.v1.PageResponse
	(*Facet)(nil),                 // 10: plexspaces.common.v1.Facet
	(*FacetDescriptor)(nil),       // 11: plexspaces.common.v1.FacetDescriptor
	(*ConfigOption)(nil),          // 12: plexspaces.common.v1.ConfigOption
	(*SecurityPolicy)(nil),        // 13: plexspaces.common.v1.SecurityPolicy
	(*ResourceQuota)(nil),         // 14: plexspaces.common.v1.ResourceQuota
	(*ResourceSpec)(nil),          // 15: plexspaces.common.v1.ResourceSpec
	(*RequestContext)(nil),        // 16: plexspaces.common.v1.RequestContext
	(*Message)(nil),               // 17: plexspaces.common.v1.Message
	nil,                           // 18: plexspaces.common.v1.Metadata.LabelsEntry
	nil,                           // 19: plexspaces.common.v1.Metadata.AnnotationsEntry
	nil,                           // 20: plexspaces.common.v1.ErrorDetail.DetailsEntry
	nil,                           // 21: plexspaces.common.v1.Facet.ConfigEntry
	nil,                           // 22: plexspaces.common.v1.Facet.StateEntry
	nil,                           // 23: plexspaces.common.v1.SecurityPolicy.CustomRulesEntry
	nil,                           // 24: plexspaces.common.v1.ResourceQuota.CustomQuotasEntry
	nil,                           // 25: plexspaces.common.v1.RequestContext.MetadataEntry
	nil,                           // 26: plexspaces.common.v1.RequestContext.HeadersEntry
	nil,                           // 27: plexspaces.common.v1.Message.HeadersEntry
	(*timestamppb.Timestamp)(nil), // 28: google.protobuf.Timestamp
	(*durationpb.Duration)(nil),   // 29: google.protobuf.Duration
	(*anypb.Any)(nil),             // 30: google.protobuf.Any
}
var file_plexspaces_v1_common_proto_depIdxs = []int32{
	28, // 0: plexspaces.common.v1.Metadata.create_time:type_name -> google.protobuf.Timestamp
	28, // 1: plexspaces.common.v1.Metadata.update_time:type_name -> google.protobuf.Timestamp
	18, // 2: plexspaces.common.v1.Metadata.labels:type_name -> plexspaces.common.v1.Metadata.LabelsEntry
	19, // 3: plexspaces.common.v1.Metadata.annotations:type_name -> plexspaces.common.v1.Metadata.AnnotationsEntry
	20, // 4: plexspaces.common.v1.ErrorDetail.details:type_name -> plexspaces.common.v1.ErrorDetail.DetailsEntry
	29, // 5: plexspaces.common.v1.RetryPolicy.initial_delay:type_name -> google.protobuf.Duration
	29, // 6: plexspaces.common.v1.RetryPolicy.max_delay:type_name -> google.protobuf.Duration
	21, // 7: plexspaces.common.v1.Facet.config:type_name -> plexspaces.common.v1.Facet.ConfigEntry
	22, // 8: plexspaces.common.v1.Facet.state:type_name -> plexspaces.common.v1.Facet.StateEntry
	4,  // 9: plexspaces.common.v1.Facet.metadata:type_name -> plexspaces.common.v1.Metadata
	12, // 10: plexspaces.common.v1.FacetDescriptor.config_options:type_name -> plexspaces.common.v1.ConfigOption
	23, // 11: plexspaces.common.v1.SecurityPolicy.custom_rules:type_name -> plexspaces.common.v1.SecurityPolicy.CustomRulesEntry
	24, // 12: plexspaces.common.v1.ResourceQuota.custom_quotas:type_name -> plexspaces.common.v1.ResourceQuota.CustomQuotasEntry
	28, // 13: plexspaces.common.v1.RequestContext.timestamp:type_name -> google.protobuf.Timestamp
	25, // 14: plexspaces.common.v1.RequestContext.metadata:type_name -> plexspaces.common.v1.RequestContext.MetadataEntry
	26, // 15: plexspaces.common.v1.RequestContext.headers:type_name -> plexspaces.common.v1.RequestContext.HeadersEntry
	28, // 16: plexspaces.common.v1.Message.timestamp:type_name -> google.protobuf.Timestamp
	27, // 17: plexspaces.common.v1.Message.headers:type_name -> plexspaces.common.v1.Message.HeadersEntry
	29, // 18: plexspaces.common.v1.Message.ttl:type_name -> google.protobuf.Duration
	30, // 19: plexspaces.common.v1.Metadata.AnnotationsEntry.value:type_name -> google.protobuf.Any
	30, // 20: plexspaces.common.v1.ErrorDetail.DetailsEntry.value:type_name -> google.protobuf.Any
	30, // 21: plexspaces.common.v1.Facet.StateEntry.value:type_name -> google.protobuf.Any
	22, // [22:22] is the sub-list for method output_type
	22, // [22:22] is the sub-list for method input_type
	22, // [22:22] is the sub-list for extension type_name
	22, // [22:22] is the sub-list for extension extendee
	0,  // [0:22] is the sub-list for field type_name
}

func init() { file_plexspaces_v1_common_proto_init() }
func file_plexspaces_v1_common_proto_init() {
	if File_plexspaces_v1_common_proto != nil {
		return
	}
	type x struct{}
	out := protoimpl.TypeBuilder{
		File: protoimpl.DescBuilder{
			GoPackagePath: reflect.TypeOf(x{}).PkgPath(),
			RawDescriptor: unsafe.Slice(unsafe.StringData(file_plexspaces_v1_common_proto_rawDesc), len(file_plexspaces_v1_common_proto_rawDesc)),
			NumEnums:      3,
			NumMessages:   25,
			NumExtensions: 0,
			NumServices:   0,
		},
		GoTypes:           file_plexspaces_v1_common_proto_goTypes,
		DependencyIndexes: file_plexspaces_v1_common_proto_depIdxs,
		EnumInfos:         file_plexspaces_v1_common_proto_enumTypes,
		MessageInfos:      file_plexspaces_v1_common_proto_msgTypes,
	}.Build()
	File_plexspaces_v1_common_proto = out.File
	file_plexspaces_v1_common_proto_goTypes = nil
	file_plexspaces_v1_common_proto_depIdxs = nil
}
