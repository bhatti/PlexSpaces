// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

// PlexSpaces Supervision API
//
// ## Purpose
// Implements Erlang/OTP-style supervision trees for fault-tolerant actor systems.
// Supervisors monitor child actors and automatically restart them on failure,
// implementing the "let it crash" philosophy where failures are expected and handled
// systematically rather than prevented defensively.
//
// ## Architecture Context
// This proto file implements **Pillar 2 (Erlang/OTP Philosophy)** of PlexSpaces.
// It provides the fault tolerance foundation that enables:
// - Hierarchical supervision trees (supervisors can supervise other supervisors)
// - Configurable restart strategies (one-for-one, one-for-all, rest-for-one)
// - Restart intensity limits to prevent restart loops
// - Graceful shutdown with configurable timeouts
// - Child lifecycle management (start, stop, restart)
//
// ### Integration with Other Pillars
// - **Pillar 1 (TupleSpace)**: Supervisors can coordinate recovery across distributed nodes
// - **Pillar 3 (Durability)**: Restarted actors replay journal to restore state
// - **Pillar 4 (WASM)**: WASM actors can be supervised just like native actors
// - **Pillar 5 (Firecracker)**: Supervisors can manage actors across VMs
//
// ## Component Interactions
// - **Used by**: Node managers to ensure actor availability, applications for fault tolerance
// - **Depends on**: common.proto (actor IDs, metadata), actor_runtime.proto (actor lifecycle)
// - **Provides**: Fault tolerance and automatic recovery for all PlexSpaces actors
//
// ## Design Decisions
// - **Why four supervision strategies**:
//   - ONE_FOR_ONE: Isolated failures (most common, restart only failed child)
//   - ONE_FOR_ALL: Dependent children (restart all when one fails, ensure consistency)
//   - REST_FOR_ONE: Ordered dependencies (restart failed + all started after it)
//   - SIMPLE_ONE_FOR_ONE: Dynamic worker pools (all children identical, efficient)
//
// - **Why restart intensity limits (max_restarts/within_period)**:
//   - Prevents infinite restart loops (bad code, missing resource)
//   - Escalates to parent supervisor when limit exceeded
//   - Typical: 3 restarts in 5 seconds, then give up
//
// - **Why three restart strategies per child**:
//   - PERMANENT: Always restart (critical services like databases)
//   - TRANSIENT: Restart only on abnormal exit (workers that might complete)
//   - TEMPORARY: Never restart (one-off tasks, cleanup jobs)
//
// - **Why shutdown_timeout per child**:
//   - Some actors need time to flush buffers, close connections
//   - Prevents hanging shutdowns (timeout = kill forcefully)
//   - Balance: too short = data loss, too long = slow shutdown
//
// ## Supervision Strategies Explained
//
// ### ONE_FOR_ONE (Default, Most Common)
// ```
// Supervisor
//  ├─ Child A (running)
//  ├─ Child B (CRASHED) → Restart only B
//  └─ Child C (running)
//
// Use when: Children are independent (e.g., separate user sessions)
// ```
//
// ### ONE_FOR_ALL (Consistency Critical)
// ```
// Supervisor
//  ├─ Child A (running) → Restart
//  ├─ Child B (CRASHED) → Restart
//  └─ Child C (running) → Restart
//
// Use when: Children must stay consistent (e.g., cache + database connection)
// ```
//
// ### REST_FOR_ONE (Ordered Dependencies)
// ```
// Supervisor
//  ├─ Child A (running)     → Keep running
//  ├─ Child B (CRASHED)     → Restart
//  └─ Child C (depends on B) → Restart (started after B)
//
// Use when: Children have start-order dependencies
// ```
//
// ### SIMPLE_ONE_FOR_ONE (Worker Pools)
// ```
// Supervisor (template: WorkerSpec)
//  ├─ Worker-1 (running)
//  ├─ Worker-2 (CRASHED) → Restart from template
//  ├─ Worker-3 (running)
//  └─ ... (can have thousands of identical workers)
//
// Use when: Many identical workers (e.g., connection pool, request handlers)
// ```
//
// ## Restart Intensity Example
// ```protobuf
// SupervisorSpec {
//   strategy: SUPERVISION_STRATEGY_ONE_FOR_ONE
//   max_restarts: 3
//   max_restart_window: "5s"
// }
//
// Timeline:
// t=0s:  Child crashes, restart #1 ✓
// t=2s:  Child crashes, restart #2 ✓
// t=4s:  Child crashes, restart #3 ✓
// t=6s:  Child crashes, restart #4 ✗ (too many, escalate to parent)
// ```

// Code generated by protoc-gen-go. DO NOT EDIT.
// versions:
// 	protoc-gen-go v1.36.11
// 	protoc        (unknown)
// source: plexspaces/v1/supervision/supervision.proto

package supervisionv1

import (
	_ "buf.build/gen/go/bufbuild/protovalidate/protocolbuffers/go/buf/validate"
	v1 "github.com/bhatti/PlexSpaces/sdks/go/plexspaces/proto/plexspaces/v1"
	_ "github.com/grpc-ecosystem/grpc-gateway/v2/protoc-gen-openapiv2/options"
	_ "google.golang.org/genproto/googleapis/api/annotations"
	protoreflect "google.golang.org/protobuf/reflect/protoreflect"
	protoimpl "google.golang.org/protobuf/runtime/protoimpl"
	durationpb "google.golang.org/protobuf/types/known/durationpb"
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

// Supervision strategy determines how supervisor handles child failures
type SupervisionStrategy int32

const (
	SupervisionStrategy_SUPERVISION_STRATEGY_UNSPECIFIED SupervisionStrategy = 0
	// Restart only the failed child (default, most common)
	SupervisionStrategy_SUPERVISION_STRATEGY_ONE_FOR_ONE SupervisionStrategy = 1
	// Restart all children if one fails
	SupervisionStrategy_SUPERVISION_STRATEGY_ONE_FOR_ALL SupervisionStrategy = 2
	// Restart failed child and all children started after it
	SupervisionStrategy_SUPERVISION_STRATEGY_REST_FOR_ONE SupervisionStrategy = 3
	// Simple one-for-one: all children are identical (worker pools)
	SupervisionStrategy_SUPERVISION_STRATEGY_SIMPLE_ONE_FOR_ONE SupervisionStrategy = 4
	// Adaptive: learns from failure patterns and adapts strategy automatically
	SupervisionStrategy_SUPERVISION_STRATEGY_ADAPTIVE SupervisionStrategy = 5
)

// Enum value maps for SupervisionStrategy.
var (
	SupervisionStrategy_name = map[int32]string{
		0: "SUPERVISION_STRATEGY_UNSPECIFIED",
		1: "SUPERVISION_STRATEGY_ONE_FOR_ONE",
		2: "SUPERVISION_STRATEGY_ONE_FOR_ALL",
		3: "SUPERVISION_STRATEGY_REST_FOR_ONE",
		4: "SUPERVISION_STRATEGY_SIMPLE_ONE_FOR_ONE",
		5: "SUPERVISION_STRATEGY_ADAPTIVE",
	}
	SupervisionStrategy_value = map[string]int32{
		"SUPERVISION_STRATEGY_UNSPECIFIED":        0,
		"SUPERVISION_STRATEGY_ONE_FOR_ONE":        1,
		"SUPERVISION_STRATEGY_ONE_FOR_ALL":        2,
		"SUPERVISION_STRATEGY_REST_FOR_ONE":       3,
		"SUPERVISION_STRATEGY_SIMPLE_ONE_FOR_ONE": 4,
		"SUPERVISION_STRATEGY_ADAPTIVE":           5,
	}
)

func (x SupervisionStrategy) Enum() *SupervisionStrategy {
	p := new(SupervisionStrategy)
	*p = x
	return p
}

func (x SupervisionStrategy) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (SupervisionStrategy) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_supervision_supervision_proto_enumTypes[0].Descriptor()
}

func (SupervisionStrategy) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_supervision_supervision_proto_enumTypes[0]
}

func (x SupervisionStrategy) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use SupervisionStrategy.Descriptor instead.
func (SupervisionStrategy) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{0}
}

// Restart policy for individual children
type RestartPolicy int32

const (
	RestartPolicy_RESTART_POLICY_UNSPECIFIED RestartPolicy = 0
	// Always restart child on failure (critical long-running processes)
	RestartPolicy_RESTART_POLICY_PERMANENT RestartPolicy = 1
	// Restart only on abnormal termination (tasks that may complete)
	RestartPolicy_RESTART_POLICY_TRANSIENT RestartPolicy = 2
	// Never restart child (one-shot tasks)
	RestartPolicy_RESTART_POLICY_TEMPORARY RestartPolicy = 3
	// Exponential backoff: delay restarts with increasing intervals
	RestartPolicy_RESTART_POLICY_EXPONENTIAL_BACKOFF RestartPolicy = 4
)

// Enum value maps for RestartPolicy.
var (
	RestartPolicy_name = map[int32]string{
		0: "RESTART_POLICY_UNSPECIFIED",
		1: "RESTART_POLICY_PERMANENT",
		2: "RESTART_POLICY_TRANSIENT",
		3: "RESTART_POLICY_TEMPORARY",
		4: "RESTART_POLICY_EXPONENTIAL_BACKOFF",
	}
	RestartPolicy_value = map[string]int32{
		"RESTART_POLICY_UNSPECIFIED":         0,
		"RESTART_POLICY_PERMANENT":           1,
		"RESTART_POLICY_TRANSIENT":           2,
		"RESTART_POLICY_TEMPORARY":           3,
		"RESTART_POLICY_EXPONENTIAL_BACKOFF": 4,
	}
)

func (x RestartPolicy) Enum() *RestartPolicy {
	p := new(RestartPolicy)
	*p = x
	return p
}

func (x RestartPolicy) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (RestartPolicy) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_supervision_supervision_proto_enumTypes[1].Descriptor()
}

func (RestartPolicy) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_supervision_supervision_proto_enumTypes[1]
}

func (x RestartPolicy) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use RestartPolicy.Descriptor instead.
func (RestartPolicy) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{1}
}

type ChildStatus int32

const (
	ChildStatus_CHILD_STATUS_UNSPECIFIED ChildStatus = 0
	ChildStatus_CHILD_STATUS_STARTING    ChildStatus = 1
	ChildStatus_CHILD_STATUS_RUNNING     ChildStatus = 2
	ChildStatus_CHILD_STATUS_STOPPING    ChildStatus = 3
	ChildStatus_CHILD_STATUS_STOPPED     ChildStatus = 4
	ChildStatus_CHILD_STATUS_FAILED      ChildStatus = 5
	ChildStatus_CHILD_STATUS_RESTARTING  ChildStatus = 6
)

// Enum value maps for ChildStatus.
var (
	ChildStatus_name = map[int32]string{
		0: "CHILD_STATUS_UNSPECIFIED",
		1: "CHILD_STATUS_STARTING",
		2: "CHILD_STATUS_RUNNING",
		3: "CHILD_STATUS_STOPPING",
		4: "CHILD_STATUS_STOPPED",
		5: "CHILD_STATUS_FAILED",
		6: "CHILD_STATUS_RESTARTING",
	}
	ChildStatus_value = map[string]int32{
		"CHILD_STATUS_UNSPECIFIED": 0,
		"CHILD_STATUS_STARTING":    1,
		"CHILD_STATUS_RUNNING":     2,
		"CHILD_STATUS_STOPPING":    3,
		"CHILD_STATUS_STOPPED":     4,
		"CHILD_STATUS_FAILED":      5,
		"CHILD_STATUS_RESTARTING":  6,
	}
)

func (x ChildStatus) Enum() *ChildStatus {
	p := new(ChildStatus)
	*p = x
	return p
}

func (x ChildStatus) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (ChildStatus) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_supervision_supervision_proto_enumTypes[2].Descriptor()
}

func (ChildStatus) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_supervision_supervision_proto_enumTypes[2]
}

func (x ChildStatus) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use ChildStatus.Descriptor instead.
func (ChildStatus) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{2}
}

// Event propagation policy for hierarchical supervision trees
//
// ## Purpose
// Defines how events from child supervisors propagate to parent supervisors
// in supervision trees, enabling configurable monitoring and observability.
//
// ## Why This Exists
// In hierarchical supervision trees, parent supervisors need to know about
// child supervisor events for:
// - Monitoring child supervisor health
// - Detecting failure patterns across tree levels
// - Implementing escalation policies
// - Providing unified observability
//
// ## Design Decisions
// - **FORWARD_ALL**: Default Erlang/OTP behavior - transparent event flow
// - **FILTER_CRITICAL**: Reduce noise for high-level supervisors
// - **NO_PROPAGATION**: Isolate branches (child supervisor is autonomous)
//
// ## Usage
// ```rust
// let child_supervisor = Supervisor::new(
//
//	"child",
//	SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 60 }
//
// );
//
// // Parent supervisor receives all child events
// parent.add_supervisor_child(
//
//	child_supervisor,
//	EventPropagation::FORWARD_ALL
//
// );
// ```
type EventPropagation int32

const (
	// Forward all events from child supervisor to parent
	// (ChildStarted, ChildStopped, ChildFailed, ChildRestarted, etc.)
	EventPropagation_EVENT_PROPAGATION_FORWARD_ALL EventPropagation = 0
	// Only forward critical events (failures, max restarts exceeded)
	// Filters out routine events (ChildStarted, ChildStopped)
	EventPropagation_EVENT_PROPAGATION_FILTER_CRITICAL EventPropagation = 1
	// No event propagation - child supervisor is completely autonomous
	// Parent only knows if child supervisor itself fails
	EventPropagation_EVENT_PROPAGATION_NONE EventPropagation = 2
)

// Enum value maps for EventPropagation.
var (
	EventPropagation_name = map[int32]string{
		0: "EVENT_PROPAGATION_FORWARD_ALL",
		1: "EVENT_PROPAGATION_FILTER_CRITICAL",
		2: "EVENT_PROPAGATION_NONE",
	}
	EventPropagation_value = map[string]int32{
		"EVENT_PROPAGATION_FORWARD_ALL":     0,
		"EVENT_PROPAGATION_FILTER_CRITICAL": 1,
		"EVENT_PROPAGATION_NONE":            2,
	}
)

func (x EventPropagation) Enum() *EventPropagation {
	p := new(EventPropagation)
	*p = x
	return p
}

func (x EventPropagation) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (EventPropagation) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_supervision_supervision_proto_enumTypes[3].Descriptor()
}

func (EventPropagation) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_supervision_supervision_proto_enumTypes[3]
}

func (x EventPropagation) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use EventPropagation.Descriptor instead.
func (EventPropagation) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{3}
}

// Event type emitted by a running supervisor
type SupervisorEventType int32

const (
	SupervisorEventType_SUPERVISOR_EVENT_UNSPECIFIED           SupervisorEventType = 0
	SupervisorEventType_SUPERVISOR_EVENT_CHILD_STARTED         SupervisorEventType = 1
	SupervisorEventType_SUPERVISOR_EVENT_CHILD_STOPPED         SupervisorEventType = 2
	SupervisorEventType_SUPERVISOR_EVENT_CHILD_RESTARTED       SupervisorEventType = 3
	SupervisorEventType_SUPERVISOR_EVENT_CHILD_FAILED          SupervisorEventType = 4
	SupervisorEventType_SUPERVISOR_EVENT_MAX_RESTARTS_EXCEEDED SupervisorEventType = 5
	SupervisorEventType_SUPERVISOR_EVENT_STRATEGY_ADAPTED      SupervisorEventType = 6
)

// Enum value maps for SupervisorEventType.
var (
	SupervisorEventType_name = map[int32]string{
		0: "SUPERVISOR_EVENT_UNSPECIFIED",
		1: "SUPERVISOR_EVENT_CHILD_STARTED",
		2: "SUPERVISOR_EVENT_CHILD_STOPPED",
		3: "SUPERVISOR_EVENT_CHILD_RESTARTED",
		4: "SUPERVISOR_EVENT_CHILD_FAILED",
		5: "SUPERVISOR_EVENT_MAX_RESTARTS_EXCEEDED",
		6: "SUPERVISOR_EVENT_STRATEGY_ADAPTED",
	}
	SupervisorEventType_value = map[string]int32{
		"SUPERVISOR_EVENT_UNSPECIFIED":           0,
		"SUPERVISOR_EVENT_CHILD_STARTED":         1,
		"SUPERVISOR_EVENT_CHILD_STOPPED":         2,
		"SUPERVISOR_EVENT_CHILD_RESTARTED":       3,
		"SUPERVISOR_EVENT_CHILD_FAILED":          4,
		"SUPERVISOR_EVENT_MAX_RESTARTS_EXCEEDED": 5,
		"SUPERVISOR_EVENT_STRATEGY_ADAPTED":      6,
	}
)

func (x SupervisorEventType) Enum() *SupervisorEventType {
	p := new(SupervisorEventType)
	*p = x
	return p
}

func (x SupervisorEventType) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (SupervisorEventType) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_supervision_supervision_proto_enumTypes[4].Descriptor()
}

func (SupervisorEventType) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_supervision_supervision_proto_enumTypes[4]
}

func (x SupervisorEventType) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use SupervisorEventType.Descriptor instead.
func (SupervisorEventType) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{4}
}

// / Supervision error types
// /
// / ## Design
// / All supervision errors are defined in proto for:
// / - Wire compatibility (gRPC error responses)
// / - Language-agnostic error handling
// / - Consistent error semantics across implementations
type SupervisionErrorCode int32

const (
	SupervisionErrorCode_SUPERVISION_ERROR_UNSPECIFIED SupervisionErrorCode = 0
	// Child management errors
	SupervisionErrorCode_CHILD_NOT_FOUND      SupervisionErrorCode = 1
	SupervisionErrorCode_CHILD_ALREADY_EXISTS SupervisionErrorCode = 2
	SupervisionErrorCode_CHILD_START_FAILED   SupervisionErrorCode = 3
	SupervisionErrorCode_CHILD_STOP_FAILED    SupervisionErrorCode = 4
	// Restart errors
	SupervisionErrorCode_MAX_RESTARTS_EXCEEDED SupervisionErrorCode = 5
	SupervisionErrorCode_RESTART_FAILED        SupervisionErrorCode = 6
	// Strategy errors
	SupervisionErrorCode_INVALID_STRATEGY   SupervisionErrorCode = 7
	SupervisionErrorCode_INVALID_CHILD_SPEC SupervisionErrorCode = 8
	// Supervisor errors
	SupervisionErrorCode_SUPERVISOR_NOT_FOUND      SupervisionErrorCode = 9
	SupervisionErrorCode_SUPERVISOR_ALREADY_EXISTS SupervisionErrorCode = 10
	SupervisionErrorCode_SUPERVISOR_NOT_ACTIVE     SupervisionErrorCode = 11
)

// Enum value maps for SupervisionErrorCode.
var (
	SupervisionErrorCode_name = map[int32]string{
		0:  "SUPERVISION_ERROR_UNSPECIFIED",
		1:  "CHILD_NOT_FOUND",
		2:  "CHILD_ALREADY_EXISTS",
		3:  "CHILD_START_FAILED",
		4:  "CHILD_STOP_FAILED",
		5:  "MAX_RESTARTS_EXCEEDED",
		6:  "RESTART_FAILED",
		7:  "INVALID_STRATEGY",
		8:  "INVALID_CHILD_SPEC",
		9:  "SUPERVISOR_NOT_FOUND",
		10: "SUPERVISOR_ALREADY_EXISTS",
		11: "SUPERVISOR_NOT_ACTIVE",
	}
	SupervisionErrorCode_value = map[string]int32{
		"SUPERVISION_ERROR_UNSPECIFIED": 0,
		"CHILD_NOT_FOUND":               1,
		"CHILD_ALREADY_EXISTS":          2,
		"CHILD_START_FAILED":            3,
		"CHILD_STOP_FAILED":             4,
		"MAX_RESTARTS_EXCEEDED":         5,
		"RESTART_FAILED":                6,
		"INVALID_STRATEGY":              7,
		"INVALID_CHILD_SPEC":            8,
		"SUPERVISOR_NOT_FOUND":          9,
		"SUPERVISOR_ALREADY_EXISTS":     10,
		"SUPERVISOR_NOT_ACTIVE":         11,
	}
)

func (x SupervisionErrorCode) Enum() *SupervisionErrorCode {
	p := new(SupervisionErrorCode)
	*p = x
	return p
}

func (x SupervisionErrorCode) String() string {
	return protoimpl.X.EnumStringOf(x.Descriptor(), protoreflect.EnumNumber(x))
}

func (SupervisionErrorCode) Descriptor() protoreflect.EnumDescriptor {
	return file_plexspaces_v1_supervision_supervision_proto_enumTypes[5].Descriptor()
}

func (SupervisionErrorCode) Type() protoreflect.EnumType {
	return &file_plexspaces_v1_supervision_supervision_proto_enumTypes[5]
}

func (x SupervisionErrorCode) Number() protoreflect.EnumNumber {
	return protoreflect.EnumNumber(x)
}

// Deprecated: Use SupervisionErrorCode.Descriptor instead.
func (SupervisionErrorCode) EnumDescriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{5}
}

// Configuration for adaptive supervision strategy
type AdaptiveConfig struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Learning rate (0.0–1.0): how quickly to adapt to failure patterns
	LearningRate  float64 `protobuf:"fixed64,1,opt,name=learning_rate,json=learningRate,proto3" json:"learning_rate,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *AdaptiveConfig) Reset() {
	*x = AdaptiveConfig{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[0]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *AdaptiveConfig) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*AdaptiveConfig) ProtoMessage() {}

func (x *AdaptiveConfig) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[0]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use AdaptiveConfig.ProtoReflect.Descriptor instead.
func (*AdaptiveConfig) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{0}
}

func (x *AdaptiveConfig) GetLearningRate() float64 {
	if x != nil {
		return x.LearningRate
	}
	return 0
}

// Configuration for exponential backoff restart policy
type ExponentialBackoffConfig struct {
	state          protoimpl.MessageState `protogen:"open.v1"`
	InitialDelayMs uint64                 `protobuf:"varint,1,opt,name=initial_delay_ms,json=initialDelayMs,proto3" json:"initial_delay_ms,omitempty"`
	MaxDelayMs     uint64                 `protobuf:"varint,2,opt,name=max_delay_ms,json=maxDelayMs,proto3" json:"max_delay_ms,omitempty"`
	Factor         float64                `protobuf:"fixed64,3,opt,name=factor,proto3" json:"factor,omitempty"`
	unknownFields  protoimpl.UnknownFields
	sizeCache      protoimpl.SizeCache
}

func (x *ExponentialBackoffConfig) Reset() {
	*x = ExponentialBackoffConfig{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[1]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ExponentialBackoffConfig) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ExponentialBackoffConfig) ProtoMessage() {}

func (x *ExponentialBackoffConfig) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[1]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ExponentialBackoffConfig.ProtoReflect.Descriptor instead.
func (*ExponentialBackoffConfig) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{1}
}

func (x *ExponentialBackoffConfig) GetInitialDelayMs() uint64 {
	if x != nil {
		return x.InitialDelayMs
	}
	return 0
}

func (x *ExponentialBackoffConfig) GetMaxDelayMs() uint64 {
	if x != nil {
		return x.MaxDelayMs
	}
	return 0
}

func (x *ExponentialBackoffConfig) GetFactor() float64 {
	if x != nil {
		return x.Factor
	}
	return 0
}

// Child specification
//
// ## Erlang/OTP Equivalent
// This maps to Erlang's child_spec:
// ```erlang
// #{id => ChildId,
//
//	start => {Module, Function, Args},
//	restart => permanent | temporary | transient,
//	shutdown => brutal_kill | Timeout | infinity,
//	type => worker | supervisor,
//	modules => [Module]}
//
// ```
//
// Identity uses `ActorIdentity` (name + actor_type). The canonical `ActorId`
// string is derived at runtime when namespace and node_id are known.
//
// This is the canonical merged definition covering both supervision (metadata)
// and application-level deployment (role, args, behavior_kind, nested supervisor).
type ChildSpec struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Instance name + behavior class for this supervised process.
	ActorIdentity *v1.ActorIdentity `protobuf:"bytes,1,opt,name=actor_identity,json=actorIdentity,proto3" json:"actor_identity,omitempty"`
	// Role of this child within the application (e.g. "worker", "leader", "supervisor").
	// Used for BehaviorRegistry dispatch when multiple children share the same actor_type.
	Role string `protobuf:"bytes,2,opt,name=role,proto3" json:"role,omitempty"`
	// Arguments to pass to start function
	Args map[string]string `protobuf:"bytes,3,rep,name=args,proto3" json:"args,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// How to handle child failures
	Restart RestartPolicy `protobuf:"varint,4,opt,name=restart,proto3,enum=plexspaces.supervision.v1.RestartPolicy" json:"restart,omitempty"`
	// Shutdown timeout for graceful termination
	// - None/0 = brutal_kill (immediate)
	// - Some(ms) = graceful shutdown with timeout
	// - For supervisors: typically set high or infinity to allow children to shutdown
	ShutdownTimeout *durationpb.Duration `protobuf:"bytes,5,opt,name=shutdown_timeout,json=shutdownTimeout,proto3" json:"shutdown_timeout,omitempty"` // Max 5 minutes
	// Nested supervisor spec (when role = "supervisor")
	Supervisor *SupervisorSpec `protobuf:"bytes,6,opt,name=supervisor,proto3,oneof" json:"supervisor,omitempty"`
	// Facet configuration (for automatic attachment during actor creation)
	// Facets are attached in priority order (high priority first) before actor.init() is called
	// All facets are automatically restored during supervisor restart
	Facets []*v1.Facet `protobuf:"bytes,7,rep,name=facets,proto3" json:"facets,omitempty"`
	// OTP-style behavior kind for logging and observability (e.g. "GenServer", "GenEvent").
	// When set, process_message spans and actor registration logs show this instead of the child id.
	BehaviorKind *string `protobuf:"bytes,8,opt,name=behavior_kind,json=behaviorKind,proto3,oneof" json:"behavior_kind,omitempty"`
	// Metadata for child configuration
	// Can include: "start_module", "start_function", "supervisor_strategy"
	Metadata map[string]string `protobuf:"bytes,10,rep,name=metadata,proto3" json:"metadata,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Exponential backoff config (when restart = RESTART_POLICY_EXPONENTIAL_BACKOFF)
	ExponentialBackoff *ExponentialBackoffConfig `protobuf:"bytes,11,opt,name=exponential_backoff,json=exponentialBackoff,proto3,oneof" json:"exponential_backoff,omitempty"`
	unknownFields      protoimpl.UnknownFields
	sizeCache          protoimpl.SizeCache
}

func (x *ChildSpec) Reset() {
	*x = ChildSpec{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[2]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ChildSpec) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ChildSpec) ProtoMessage() {}

func (x *ChildSpec) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[2]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ChildSpec.ProtoReflect.Descriptor instead.
func (*ChildSpec) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{2}
}

func (x *ChildSpec) GetActorIdentity() *v1.ActorIdentity {
	if x != nil {
		return x.ActorIdentity
	}
	return nil
}

func (x *ChildSpec) GetRole() string {
	if x != nil {
		return x.Role
	}
	return ""
}

func (x *ChildSpec) GetArgs() map[string]string {
	if x != nil {
		return x.Args
	}
	return nil
}

func (x *ChildSpec) GetRestart() RestartPolicy {
	if x != nil {
		return x.Restart
	}
	return RestartPolicy_RESTART_POLICY_UNSPECIFIED
}

func (x *ChildSpec) GetShutdownTimeout() *durationpb.Duration {
	if x != nil {
		return x.ShutdownTimeout
	}
	return nil
}

func (x *ChildSpec) GetSupervisor() *SupervisorSpec {
	if x != nil {
		return x.Supervisor
	}
	return nil
}

func (x *ChildSpec) GetFacets() []*v1.Facet {
	if x != nil {
		return x.Facets
	}
	return nil
}

func (x *ChildSpec) GetBehaviorKind() string {
	if x != nil && x.BehaviorKind != nil {
		return *x.BehaviorKind
	}
	return ""
}

func (x *ChildSpec) GetMetadata() map[string]string {
	if x != nil {
		return x.Metadata
	}
	return nil
}

func (x *ChildSpec) GetExponentialBackoff() *ExponentialBackoffConfig {
	if x != nil {
		return x.ExponentialBackoff
	}
	return nil
}

// Supervisor specification (canonical definition)
//
// Used everywhere supervision trees are configured: application deployment,
// runtime supervisor creation, and gRPC supervisor management API.
type SupervisorSpec struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Supervision strategy
	Strategy SupervisionStrategy `protobuf:"varint,1,opt,name=strategy,proto3,enum=plexspaces.supervision.v1.SupervisionStrategy" json:"strategy,omitempty"`
	// Maximum restart intensity (max restarts in period)
	MaxRestarts uint32 `protobuf:"varint,2,opt,name=max_restarts,json=maxRestarts,proto3" json:"max_restarts,omitempty"` // 1 to 1000 restarts
	// Time window for restart counting
	MaxRestartWindow *durationpb.Duration `protobuf:"bytes,3,opt,name=max_restart_window,json=maxRestartWindow,proto3" json:"max_restart_window,omitempty"` // 1 second to 1 hour
	// Child specifications
	Children []*ChildSpec `protobuf:"bytes,4,rep,name=children,proto3" json:"children,omitempty"` // Max 1000 children
	// Supervisor metadata
	Metadata map[string]string `protobuf:"bytes,5,rep,name=metadata,proto3" json:"metadata,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Adaptive strategy configuration (when strategy = SUPERVISION_STRATEGY_ADAPTIVE)
	Adaptive      *AdaptiveConfig `protobuf:"bytes,6,opt,name=adaptive,proto3,oneof" json:"adaptive,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SupervisorSpec) Reset() {
	*x = SupervisorSpec{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[3]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SupervisorSpec) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SupervisorSpec) ProtoMessage() {}

func (x *SupervisorSpec) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[3]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SupervisorSpec.ProtoReflect.Descriptor instead.
func (*SupervisorSpec) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{3}
}

func (x *SupervisorSpec) GetStrategy() SupervisionStrategy {
	if x != nil {
		return x.Strategy
	}
	return SupervisionStrategy_SUPERVISION_STRATEGY_UNSPECIFIED
}

func (x *SupervisorSpec) GetMaxRestarts() uint32 {
	if x != nil {
		return x.MaxRestarts
	}
	return 0
}

func (x *SupervisorSpec) GetMaxRestartWindow() *durationpb.Duration {
	if x != nil {
		return x.MaxRestartWindow
	}
	return nil
}

func (x *SupervisorSpec) GetChildren() []*ChildSpec {
	if x != nil {
		return x.Children
	}
	return nil
}

func (x *SupervisorSpec) GetMetadata() map[string]string {
	if x != nil {
		return x.Metadata
	}
	return nil
}

func (x *SupervisorSpec) GetAdaptive() *AdaptiveConfig {
	if x != nil {
		return x.Adaptive
	}
	return nil
}

// Supervisor state
type SupervisorState struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Supervisor ID
	SupervisorId string `protobuf:"bytes,1,opt,name=supervisor_id,json=supervisorId,proto3" json:"supervisor_id,omitempty"`
	// Specification
	Spec *SupervisorSpec `protobuf:"bytes,2,opt,name=spec,proto3" json:"spec,omitempty"`
	// Current children
	Children []*ChildState `protobuf:"bytes,3,rep,name=children,proto3" json:"children,omitempty"` // Max 1000 children
	// Restart history
	RestartHistory []*RestartEvent `protobuf:"bytes,4,rep,name=restart_history,json=restartHistory,proto3" json:"restart_history,omitempty"` // Max 10K events
	// Is supervisor active
	IsActive      bool `protobuf:"varint,5,opt,name=is_active,json=isActive,proto3" json:"is_active,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SupervisorState) Reset() {
	*x = SupervisorState{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[4]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SupervisorState) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SupervisorState) ProtoMessage() {}

func (x *SupervisorState) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[4]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SupervisorState.ProtoReflect.Descriptor instead.
func (*SupervisorState) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{4}
}

func (x *SupervisorState) GetSupervisorId() string {
	if x != nil {
		return x.SupervisorId
	}
	return ""
}

func (x *SupervisorState) GetSpec() *SupervisorSpec {
	if x != nil {
		return x.Spec
	}
	return nil
}

func (x *SupervisorState) GetChildren() []*ChildState {
	if x != nil {
		return x.Children
	}
	return nil
}

func (x *SupervisorState) GetRestartHistory() []*RestartEvent {
	if x != nil {
		return x.RestartHistory
	}
	return nil
}

func (x *SupervisorState) GetIsActive() bool {
	if x != nil {
		return x.IsActive
	}
	return false
}

// Child state
type ChildState struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Child specification
	Spec *ChildSpec `protobuf:"bytes,1,opt,name=spec,proto3" json:"spec,omitempty"`
	// Current state
	Status ChildStatus `protobuf:"varint,2,opt,name=status,proto3,enum=plexspaces.supervision.v1.ChildStatus" json:"status,omitempty"`
	// When child was started
	StartedAt *timestamppb.Timestamp `protobuf:"bytes,3,opt,name=started_at,json=startedAt,proto3" json:"started_at,omitempty"`
	// Number of restarts
	RestartCount uint32 `protobuf:"varint,4,opt,name=restart_count,json=restartCount,proto3" json:"restart_count,omitempty"` // Max 10K restarts
	// Last restart time
	LastRestart   *timestamppb.Timestamp `protobuf:"bytes,5,opt,name=last_restart,json=lastRestart,proto3" json:"last_restart,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ChildState) Reset() {
	*x = ChildState{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[5]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ChildState) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ChildState) ProtoMessage() {}

func (x *ChildState) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[5]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ChildState.ProtoReflect.Descriptor instead.
func (*ChildState) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{5}
}

func (x *ChildState) GetSpec() *ChildSpec {
	if x != nil {
		return x.Spec
	}
	return nil
}

func (x *ChildState) GetStatus() ChildStatus {
	if x != nil {
		return x.Status
	}
	return ChildStatus_CHILD_STATUS_UNSPECIFIED
}

func (x *ChildState) GetStartedAt() *timestamppb.Timestamp {
	if x != nil {
		return x.StartedAt
	}
	return nil
}

func (x *ChildState) GetRestartCount() uint32 {
	if x != nil {
		return x.RestartCount
	}
	return 0
}

func (x *ChildState) GetLastRestart() *timestamppb.Timestamp {
	if x != nil {
		return x.LastRestart
	}
	return nil
}

// Restart event
type RestartEvent struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Child that was restarted
	ChildId string `protobuf:"bytes,1,opt,name=child_id,json=childId,proto3" json:"child_id,omitempty"`
	// When restart occurred
	Timestamp *timestamppb.Timestamp `protobuf:"bytes,2,opt,name=timestamp,proto3" json:"timestamp,omitempty"`
	// Reason for restart
	Reason string `protobuf:"bytes,3,opt,name=reason,proto3" json:"reason,omitempty"` // Error messages can be long
	// Which strategy was applied
	Strategy      SupervisionStrategy `protobuf:"varint,4,opt,name=strategy,proto3,enum=plexspaces.supervision.v1.SupervisionStrategy" json:"strategy,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *RestartEvent) Reset() {
	*x = RestartEvent{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[6]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *RestartEvent) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*RestartEvent) ProtoMessage() {}

func (x *RestartEvent) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[6]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use RestartEvent.ProtoReflect.Descriptor instead.
func (*RestartEvent) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{6}
}

func (x *RestartEvent) GetChildId() string {
	if x != nil {
		return x.ChildId
	}
	return ""
}

func (x *RestartEvent) GetTimestamp() *timestamppb.Timestamp {
	if x != nil {
		return x.Timestamp
	}
	return nil
}

func (x *RestartEvent) GetReason() string {
	if x != nil {
		return x.Reason
	}
	return ""
}

func (x *RestartEvent) GetStrategy() SupervisionStrategy {
	if x != nil {
		return x.Strategy
	}
	return SupervisionStrategy_SUPERVISION_STRATEGY_UNSPECIFIED
}

// Supervisor statistics for monitoring and observability
//
// ## Purpose
// Provides runtime metrics for supervisor behavior, enabling:
// - **Production Monitoring**: Track restart rates, failure patterns
// - **Debugging**: Identify problematic actors or recurring failures
// - **Capacity Planning**: Understand resource usage and stability
// - **Alerting**: Detect when restart rates exceed thresholds
//
// ## Why This Exists
// In production distributed systems, supervisors are the fault tolerance backbone.
// Without metrics, operators cannot answer critical questions:
// - "Is this actor constantly restarting?"
// - "What are the most common failure reasons?"
// - "Is the system stable or thrashing?"
// - "Do we need to adjust max_restarts thresholds?"
//
// ## Metrics Tracked
//
// ### Restart Counts
// - **total_restarts**: All restart attempts (successful + failed)
// - **successful_restarts**: Restarts that succeeded (actor back online)
// - **failed_restarts**: Restarts that failed (couldn't bring actor back)
//
// ### Success Rate Calculation
// ```
// success_rate = successful_restarts / total_restarts
// Example: 95 successful / 100 total = 0.95 (95% success rate)
// ```
//
// ### Strategy Adaptations (Adaptive Strategy Only)
// - **strategy_adaptations**: How many times adaptive strategy changed
// - Tracks learning behavior of Adaptive supervision strategy
// - High adaptation count indicates unstable failure patterns
//
// ### Failure Patterns
// - **failure_patterns**: Map of error reasons → occurrence count
// - Enables root cause analysis: "80% of failures are 'database timeout'"
// - Helps prioritize fixes for most common failures
//
// ## Usage Examples
//
// ### Alerting on High Restart Rate
// ```rust
// let stats = supervisor.stats().await;
//
//	if stats.total_restarts > 100 && stats.success_rate() < 0.90 {
//	    alert("High restart rate with low success!");
//	}
//
// ```
//
// ### Debugging Failure Patterns
// ```rust
// let stats = supervisor.stats().await;
//
//	for (reason, count) in stats.failure_patterns {
//	    if count > 10 {
//	        println!("Common failure: {} ({} times)", reason, count);
//	    }
//	}
//
// ```
//
// ### Capacity Planning
// ```rust
// let stats = supervisor.stats().await;
// let restart_rate = stats.total_restarts as f64 / uptime_seconds;
// if restart_rate > 1.0 {  // More than 1 restart/sec
//
//	    scale_cluster();  // Need more capacity
//	}
//
// ```
//
// ## Integration with Metrics Systems
// These stats can be exported to:
// - **Prometheus**: `supervisor_restarts_total{supervisor="foo"}`
// - **StatsD**: `supervisor.foo.restarts.total:100|c`
// - **CloudWatch**: Custom metrics for AWS deployments
// - **DataDog**: APM metrics for distributed tracing
type SupervisorStats struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Total number of restart attempts (successful + failed)
	//
	// Monotonically increasing counter tracking all restart attempts
	// since supervisor creation. Includes both successful restarts
	// (actor back online) and failed restarts (couldn't recover).
	TotalRestarts uint64 `protobuf:"varint,1,opt,name=total_restarts,json=totalRestarts,proto3" json:"total_restarts,omitempty"`
	// Number of successful restarts
	//
	// Counts restarts where the actor successfully came back online
	// and resumed processing messages. Always <= total_restarts.
	SuccessfulRestarts uint64 `protobuf:"varint,2,opt,name=successful_restarts,json=successfulRestarts,proto3" json:"successful_restarts,omitempty"`
	// Number of failed restarts
	//
	// Counts restarts that failed (e.g., actor factory error, timeout).
	// Always <= total_restarts.
	// Invariant: total_restarts = successful_restarts + failed_restarts
	FailedRestarts uint64 `protobuf:"varint,3,opt,name=failed_restarts,json=failedRestarts,proto3" json:"failed_restarts,omitempty"`
	// Number of times adaptive strategy changed (Adaptive strategy only)
	//
	// Tracks how many times the Adaptive supervision strategy learned
	// from failure patterns and switched strategies. Only increments
	// for Adaptive strategy; always 0 for static strategies.
	StrategyAdaptations uint32 `protobuf:"varint,4,opt,name=strategy_adaptations,json=strategyAdaptations,proto3" json:"strategy_adaptations,omitempty"` // Max 10K adaptations
	// Failure pattern histogram: error reason → count
	//
	// Maps failure reasons (error messages) to their occurrence count.
	// Enables root cause analysis:
	// - "database_timeout": 50 → Database connection issues
	// - "out_of_memory": 10 → Resource exhaustion
	// - "panic": 5 → Code bugs
	//
	// Use this to prioritize fixes for most common failure modes.
	FailurePatterns map[string]uint32 `protobuf:"bytes,5,rep,name=failure_patterns,json=failurePatterns,proto3" json:"failure_patterns,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"varint,2,opt,name=value"`
	unknownFields   protoimpl.UnknownFields
	sizeCache       protoimpl.SizeCache
}

func (x *SupervisorStats) Reset() {
	*x = SupervisorStats{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[7]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SupervisorStats) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SupervisorStats) ProtoMessage() {}

func (x *SupervisorStats) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[7]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SupervisorStats.ProtoReflect.Descriptor instead.
func (*SupervisorStats) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{7}
}

func (x *SupervisorStats) GetTotalRestarts() uint64 {
	if x != nil {
		return x.TotalRestarts
	}
	return 0
}

func (x *SupervisorStats) GetSuccessfulRestarts() uint64 {
	if x != nil {
		return x.SuccessfulRestarts
	}
	return 0
}

func (x *SupervisorStats) GetFailedRestarts() uint64 {
	if x != nil {
		return x.FailedRestarts
	}
	return 0
}

func (x *SupervisorStats) GetStrategyAdaptations() uint32 {
	if x != nil {
		return x.StrategyAdaptations
	}
	return 0
}

func (x *SupervisorStats) GetFailurePatterns() map[string]uint32 {
	if x != nil {
		return x.FailurePatterns
	}
	return nil
}

// Supervisor event with associated data (emitted to the event channel)
type SupervisorEvent struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	EventType     SupervisorEventType    `protobuf:"varint,1,opt,name=event_type,json=eventType,proto3,enum=plexspaces.supervision.v1.SupervisorEventType" json:"event_type,omitempty"`
	ActorId       string                 `protobuf:"bytes,2,opt,name=actor_id,json=actorId,proto3" json:"actor_id,omitempty"`
	RestartCount  uint32                 `protobuf:"varint,3,opt,name=restart_count,json=restartCount,proto3" json:"restart_count,omitempty"`
	Reason        string                 `protobuf:"bytes,4,opt,name=reason,proto3" json:"reason,omitempty"`
	NewStrategy   SupervisionStrategy    `protobuf:"varint,5,opt,name=new_strategy,json=newStrategy,proto3,enum=plexspaces.supervision.v1.SupervisionStrategy" json:"new_strategy,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SupervisorEvent) Reset() {
	*x = SupervisorEvent{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[8]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SupervisorEvent) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SupervisorEvent) ProtoMessage() {}

func (x *SupervisorEvent) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[8]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SupervisorEvent.ProtoReflect.Descriptor instead.
func (*SupervisorEvent) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{8}
}

func (x *SupervisorEvent) GetEventType() SupervisorEventType {
	if x != nil {
		return x.EventType
	}
	return SupervisorEventType_SUPERVISOR_EVENT_UNSPECIFIED
}

func (x *SupervisorEvent) GetActorId() string {
	if x != nil {
		return x.ActorId
	}
	return ""
}

func (x *SupervisorEvent) GetRestartCount() uint32 {
	if x != nil {
		return x.RestartCount
	}
	return 0
}

func (x *SupervisorEvent) GetReason() string {
	if x != nil {
		return x.Reason
	}
	return ""
}

func (x *SupervisorEvent) GetNewStrategy() SupervisionStrategy {
	if x != nil {
		return x.NewStrategy
	}
	return SupervisionStrategy_SUPERVISION_STRATEGY_UNSPECIFIED
}

// Child information returned by which_children()
type ChildInfo struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	ChildId       string                 `protobuf:"bytes,1,opt,name=child_id,json=childId,proto3" json:"child_id,omitempty"`
	Role          string                 `protobuf:"bytes,2,opt,name=role,proto3" json:"role,omitempty"`
	Status        ChildStatus            `protobuf:"varint,3,opt,name=status,proto3,enum=plexspaces.supervision.v1.ChildStatus" json:"status,omitempty"`
	RestartCount  uint32                 `protobuf:"varint,4,opt,name=restart_count,json=restartCount,proto3" json:"restart_count,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ChildInfo) Reset() {
	*x = ChildInfo{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[9]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ChildInfo) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ChildInfo) ProtoMessage() {}

func (x *ChildInfo) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[9]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ChildInfo.ProtoReflect.Descriptor instead.
func (*ChildInfo) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{9}
}

func (x *ChildInfo) GetChildId() string {
	if x != nil {
		return x.ChildId
	}
	return ""
}

func (x *ChildInfo) GetRole() string {
	if x != nil {
		return x.Role
	}
	return ""
}

func (x *ChildInfo) GetStatus() ChildStatus {
	if x != nil {
		return x.Status
	}
	return ChildStatus_CHILD_STATUS_UNSPECIFIED
}

func (x *ChildInfo) GetRestartCount() uint32 {
	if x != nil {
		return x.RestartCount
	}
	return 0
}

// Child counts returned by count_children()
type ChildCount struct {
	state         protoimpl.MessageState `protogen:"open.v1"`
	Actors        uint32                 `protobuf:"varint,1,opt,name=actors,proto3" json:"actors,omitempty"`
	Supervisors   uint32                 `protobuf:"varint,2,opt,name=supervisors,proto3" json:"supervisors,omitempty"`
	Total         uint32                 `protobuf:"varint,3,opt,name=total,proto3" json:"total,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *ChildCount) Reset() {
	*x = ChildCount{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[10]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *ChildCount) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*ChildCount) ProtoMessage() {}

func (x *ChildCount) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[10]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use ChildCount.ProtoReflect.Descriptor instead.
func (*ChildCount) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{10}
}

func (x *ChildCount) GetActors() uint32 {
	if x != nil {
		return x.Actors
	}
	return 0
}

func (x *ChildCount) GetSupervisors() uint32 {
	if x != nil {
		return x.Supervisors
	}
	return 0
}

func (x *ChildCount) GetTotal() uint32 {
	if x != nil {
		return x.Total
	}
	return 0
}

// / Supervision error details
type SupervisionError struct {
	state protoimpl.MessageState `protogen:"open.v1"`
	// Error code
	Code SupervisionErrorCode `protobuf:"varint,1,opt,name=code,proto3,enum=plexspaces.supervision.v1.SupervisionErrorCode" json:"code,omitempty"`
	// Human-readable error message
	Message string `protobuf:"bytes,2,opt,name=message,proto3" json:"message,omitempty"`
	// Additional context (child_id, supervisor_id, etc.)
	Context map[string]string `protobuf:"bytes,3,rep,name=context,proto3" json:"context,omitempty" protobuf_key:"bytes,1,opt,name=key" protobuf_val:"bytes,2,opt,name=value"`
	// Timestamp when error occurred
	Timestamp     *timestamppb.Timestamp `protobuf:"bytes,4,opt,name=timestamp,proto3" json:"timestamp,omitempty"`
	unknownFields protoimpl.UnknownFields
	sizeCache     protoimpl.SizeCache
}

func (x *SupervisionError) Reset() {
	*x = SupervisionError{}
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[11]
	ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
	ms.StoreMessageInfo(mi)
}

func (x *SupervisionError) String() string {
	return protoimpl.X.MessageStringOf(x)
}

func (*SupervisionError) ProtoMessage() {}

func (x *SupervisionError) ProtoReflect() protoreflect.Message {
	mi := &file_plexspaces_v1_supervision_supervision_proto_msgTypes[11]
	if x != nil {
		ms := protoimpl.X.MessageStateOf(protoimpl.Pointer(x))
		if ms.LoadMessageInfo() == nil {
			ms.StoreMessageInfo(mi)
		}
		return ms
	}
	return mi.MessageOf(x)
}

// Deprecated: Use SupervisionError.ProtoReflect.Descriptor instead.
func (*SupervisionError) Descriptor() ([]byte, []int) {
	return file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP(), []int{11}
}

func (x *SupervisionError) GetCode() SupervisionErrorCode {
	if x != nil {
		return x.Code
	}
	return SupervisionErrorCode_SUPERVISION_ERROR_UNSPECIFIED
}

func (x *SupervisionError) GetMessage() string {
	if x != nil {
		return x.Message
	}
	return ""
}

func (x *SupervisionError) GetContext() map[string]string {
	if x != nil {
		return x.Context
	}
	return nil
}

func (x *SupervisionError) GetTimestamp() *timestamppb.Timestamp {
	if x != nil {
		return x.Timestamp
	}
	return nil
}

var File_plexspaces_v1_supervision_supervision_proto protoreflect.FileDescriptor

const file_plexspaces_v1_supervision_supervision_proto_rawDesc = "" +
	"\n" +
	"+plexspaces/v1/supervision/supervision.proto\x12\x19plexspaces.supervision.v1\x1a\x1bbuf/validate/validate.proto\x1a\x1cgoogle/api/annotations.proto\x1a\x1fgoogle/api/field_behavior.proto\x1a\x1egoogle/protobuf/duration.proto\x1a\x1fgoogle/protobuf/timestamp.proto\x1a\x1aplexspaces/v1/common.proto\x1a.protoc-gen-openapiv2/options/annotations.proto\"5\n" +
	"\x0eAdaptiveConfig\x12#\n" +
	"\rlearning_rate\x18\x01 \x01(\x01R\flearningRate\"~\n" +
	"\x18ExponentialBackoffConfig\x12(\n" +
	"\x10initial_delay_ms\x18\x01 \x01(\x04R\x0einitialDelayMs\x12 \n" +
	"\fmax_delay_ms\x18\x02 \x01(\x04R\n" +
	"maxDelayMs\x12\x16\n" +
	"\x06factor\x18\x03 \x01(\x01R\x06factor\"\xe9\x06\n" +
	"\tChildSpec\x12J\n" +
	"\x0eactor_identity\x18\x01 \x01(\v2#.plexspaces.common.v1.ActorIdentityR\ractorIdentity\x12\x1c\n" +
	"\x04role\x18\x02 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x01R\x04role\x12B\n" +
	"\x04args\x18\x03 \x03(\v2..plexspaces.supervision.v1.ChildSpec.ArgsEntryR\x04args\x12B\n" +
	"\arestart\x18\x04 \x01(\x0e2(.plexspaces.supervision.v1.RestartPolicyR\arestart\x12Q\n" +
	"\x10shutdown_timeout\x18\x05 \x01(\v2\x19.google.protobuf.DurationB\v\xbaH\b\xaa\x01\x05\"\x03\b\xac\x02R\x0fshutdownTimeout\x12N\n" +
	"\n" +
	"supervisor\x18\x06 \x01(\v2).plexspaces.supervision.v1.SupervisorSpecH\x00R\n" +
	"supervisor\x88\x01\x01\x123\n" +
	"\x06facets\x18\a \x03(\v2\x1b.plexspaces.common.v1.FacetR\x06facets\x12(\n" +
	"\rbehavior_kind\x18\b \x01(\tH\x01R\fbehaviorKind\x88\x01\x01\x12N\n" +
	"\bmetadata\x18\n" +
	" \x03(\v22.plexspaces.supervision.v1.ChildSpec.MetadataEntryR\bmetadata\x12i\n" +
	"\x13exponential_backoff\x18\v \x01(\v23.plexspaces.supervision.v1.ExponentialBackoffConfigH\x02R\x12exponentialBackoff\x88\x01\x01\x1a7\n" +
	"\tArgsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01\x1a;\n" +
	"\rMetadataEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01B\r\n" +
	"\v_supervisorB\x10\n" +
	"\x0e_behavior_kindB\x16\n" +
	"\x14_exponential_backoff\"\x9d\x04\n" +
	"\x0eSupervisorSpec\x12J\n" +
	"\bstrategy\x18\x01 \x01(\x0e2..plexspaces.supervision.v1.SupervisionStrategyR\bstrategy\x12-\n" +
	"\fmax_restarts\x18\x02 \x01(\rB\n" +
	"\xbaH\a*\x05\x18\xe8\a(\x01R\vmaxRestarts\x12X\n" +
	"\x12max_restart_window\x18\x03 \x01(\v2\x19.google.protobuf.DurationB\x0f\xbaH\f\xaa\x01\t\"\x03\b\x90\x1c2\x02\b\x01R\x10maxRestartWindow\x12K\n" +
	"\bchildren\x18\x04 \x03(\v2$.plexspaces.supervision.v1.ChildSpecB\t\xbaH\x06\x92\x01\x03\x10\xe8\aR\bchildren\x12S\n" +
	"\bmetadata\x18\x05 \x03(\v27.plexspaces.supervision.v1.SupervisorSpec.MetadataEntryR\bmetadata\x12J\n" +
	"\badaptive\x18\x06 \x01(\v2).plexspaces.supervision.v1.AdaptiveConfigH\x00R\badaptive\x88\x01\x01\x1a;\n" +
	"\rMetadataEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01B\v\n" +
	"\t_adaptive\"\xc7\x02\n" +
	"\x0fSupervisorState\x12-\n" +
	"\rsupervisor_id\x18\x01 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\fsupervisorId\x12=\n" +
	"\x04spec\x18\x02 \x01(\v2).plexspaces.supervision.v1.SupervisorSpecR\x04spec\x12L\n" +
	"\bchildren\x18\x03 \x03(\v2%.plexspaces.supervision.v1.ChildStateB\t\xbaH\x06\x92\x01\x03\x10\xe8\aR\bchildren\x12[\n" +
	"\x0frestart_history\x18\x04 \x03(\v2'.plexspaces.supervision.v1.RestartEventB\t\xbaH\x06\x92\x01\x03\x10\x90NR\x0erestartHistory\x12\x1b\n" +
	"\tis_active\x18\x05 \x01(\bR\bisActive\"\xaf\x02\n" +
	"\n" +
	"ChildState\x128\n" +
	"\x04spec\x18\x01 \x01(\v2$.plexspaces.supervision.v1.ChildSpecR\x04spec\x12>\n" +
	"\x06status\x18\x02 \x01(\x0e2&.plexspaces.supervision.v1.ChildStatusR\x06status\x129\n" +
	"\n" +
	"started_at\x18\x03 \x01(\v2\x1a.google.protobuf.TimestampR\tstartedAt\x12-\n" +
	"\rrestart_count\x18\x04 \x01(\rB\b\xbaH\x05*\x03\x18\x90NR\frestartCount\x12=\n" +
	"\flast_restart\x18\x05 \x01(\v2\x1a.google.protobuf.TimestampR\vlastRestart\"\xdb\x01\n" +
	"\fRestartEvent\x12#\n" +
	"\bchild_id\x18\x01 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\achildId\x128\n" +
	"\ttimestamp\x18\x02 \x01(\v2\x1a.google.protobuf.TimestampR\ttimestamp\x12 \n" +
	"\x06reason\x18\x03 \x01(\tB\b\xbaH\x05r\x03\x18\x80\bR\x06reason\x12J\n" +
	"\bstrategy\x18\x04 \x01(\x0e2..plexspaces.supervision.v1.SupervisionStrategyR\bstrategy\"\xff\x02\n" +
	"\x0fSupervisorStats\x12%\n" +
	"\x0etotal_restarts\x18\x01 \x01(\x04R\rtotalRestarts\x12/\n" +
	"\x13successful_restarts\x18\x02 \x01(\x04R\x12successfulRestarts\x12'\n" +
	"\x0ffailed_restarts\x18\x03 \x01(\x04R\x0efailedRestarts\x12;\n" +
	"\x14strategy_adaptations\x18\x04 \x01(\rB\b\xbaH\x05*\x03\x18\x90NR\x13strategyAdaptations\x12j\n" +
	"\x10failure_patterns\x18\x05 \x03(\v2?.plexspaces.supervision.v1.SupervisorStats.FailurePatternsEntryR\x0ffailurePatterns\x1aB\n" +
	"\x14FailurePatternsEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\rR\x05value:\x028\x01\"\x9f\x02\n" +
	"\x0fSupervisorEvent\x12M\n" +
	"\n" +
	"event_type\x18\x01 \x01(\x0e2..plexspaces.supervision.v1.SupervisorEventTypeR\teventType\x12#\n" +
	"\bactor_id\x18\x02 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x04R\aactorId\x12#\n" +
	"\rrestart_count\x18\x03 \x01(\rR\frestartCount\x12 \n" +
	"\x06reason\x18\x04 \x01(\tB\b\xbaH\x05r\x03\x18\x80\bR\x06reason\x12Q\n" +
	"\fnew_strategy\x18\x05 \x01(\x0e2..plexspaces.supervision.v1.SupervisionStrategyR\vnewStrategy\"\xb3\x01\n" +
	"\tChildInfo\x12#\n" +
	"\bchild_id\x18\x01 \x01(\tB\b\xbaH\x05r\x03\x18\xff\x01R\achildId\x12\x1c\n" +
	"\x04role\x18\x02 \x01(\tB\b\xbaH\x05r\x03\x18\x80\x01R\x04role\x12>\n" +
	"\x06status\x18\x03 \x01(\x0e2&.plexspaces.supervision.v1.ChildStatusR\x06status\x12#\n" +
	"\rrestart_count\x18\x04 \x01(\rR\frestartCount\"\\\n" +
	"\n" +
	"ChildCount\x12\x16\n" +
	"\x06actors\x18\x01 \x01(\rR\x06actors\x12 \n" +
	"\vsupervisors\x18\x02 \x01(\rR\vsupervisors\x12\x14\n" +
	"\x05total\x18\x03 \x01(\rR\x05total\"\xc5\x02\n" +
	"\x10SupervisionError\x12C\n" +
	"\x04code\x18\x01 \x01(\x0e2/.plexspaces.supervision.v1.SupervisionErrorCodeR\x04code\x12\"\n" +
	"\amessage\x18\x02 \x01(\tB\b\xbaH\x05r\x03\x18\x80\bR\amessage\x12R\n" +
	"\acontext\x18\x03 \x03(\v28.plexspaces.supervision.v1.SupervisionError.ContextEntryR\acontext\x128\n" +
	"\ttimestamp\x18\x04 \x01(\v2\x1a.google.protobuf.TimestampR\ttimestamp\x1a:\n" +
	"\fContextEntry\x12\x10\n" +
	"\x03key\x18\x01 \x01(\tR\x03key\x12\x14\n" +
	"\x05value\x18\x02 \x01(\tR\x05value:\x028\x01*\xfe\x01\n" +
	"\x13SupervisionStrategy\x12$\n" +
	" SUPERVISION_STRATEGY_UNSPECIFIED\x10\x00\x12$\n" +
	" SUPERVISION_STRATEGY_ONE_FOR_ONE\x10\x01\x12$\n" +
	" SUPERVISION_STRATEGY_ONE_FOR_ALL\x10\x02\x12%\n" +
	"!SUPERVISION_STRATEGY_REST_FOR_ONE\x10\x03\x12+\n" +
	"'SUPERVISION_STRATEGY_SIMPLE_ONE_FOR_ONE\x10\x04\x12!\n" +
	"\x1dSUPERVISION_STRATEGY_ADAPTIVE\x10\x05*\xb1\x01\n" +
	"\rRestartPolicy\x12\x1e\n" +
	"\x1aRESTART_POLICY_UNSPECIFIED\x10\x00\x12\x1c\n" +
	"\x18RESTART_POLICY_PERMANENT\x10\x01\x12\x1c\n" +
	"\x18RESTART_POLICY_TRANSIENT\x10\x02\x12\x1c\n" +
	"\x18RESTART_POLICY_TEMPORARY\x10\x03\x12&\n" +
	"\"RESTART_POLICY_EXPONENTIAL_BACKOFF\x10\x04*\xcb\x01\n" +
	"\vChildStatus\x12\x1c\n" +
	"\x18CHILD_STATUS_UNSPECIFIED\x10\x00\x12\x19\n" +
	"\x15CHILD_STATUS_STARTING\x10\x01\x12\x18\n" +
	"\x14CHILD_STATUS_RUNNING\x10\x02\x12\x19\n" +
	"\x15CHILD_STATUS_STOPPING\x10\x03\x12\x18\n" +
	"\x14CHILD_STATUS_STOPPED\x10\x04\x12\x17\n" +
	"\x13CHILD_STATUS_FAILED\x10\x05\x12\x1b\n" +
	"\x17CHILD_STATUS_RESTARTING\x10\x06*x\n" +
	"\x10EventPropagation\x12!\n" +
	"\x1dEVENT_PROPAGATION_FORWARD_ALL\x10\x00\x12%\n" +
	"!EVENT_PROPAGATION_FILTER_CRITICAL\x10\x01\x12\x1a\n" +
	"\x16EVENT_PROPAGATION_NONE\x10\x02*\x9b\x02\n" +
	"\x13SupervisorEventType\x12 \n" +
	"\x1cSUPERVISOR_EVENT_UNSPECIFIED\x10\x00\x12\"\n" +
	"\x1eSUPERVISOR_EVENT_CHILD_STARTED\x10\x01\x12\"\n" +
	"\x1eSUPERVISOR_EVENT_CHILD_STOPPED\x10\x02\x12$\n" +
	" SUPERVISOR_EVENT_CHILD_RESTARTED\x10\x03\x12!\n" +
	"\x1dSUPERVISOR_EVENT_CHILD_FAILED\x10\x04\x12*\n" +
	"&SUPERVISOR_EVENT_MAX_RESTARTS_EXCEEDED\x10\x05\x12%\n" +
	"!SUPERVISOR_EVENT_STRATEGY_ADAPTED\x10\x06*\xc8\x02\n" +
	"\x14SupervisionErrorCode\x12!\n" +
	"\x1dSUPERVISION_ERROR_UNSPECIFIED\x10\x00\x12\x13\n" +
	"\x0fCHILD_NOT_FOUND\x10\x01\x12\x18\n" +
	"\x14CHILD_ALREADY_EXISTS\x10\x02\x12\x16\n" +
	"\x12CHILD_START_FAILED\x10\x03\x12\x15\n" +
	"\x11CHILD_STOP_FAILED\x10\x04\x12\x19\n" +
	"\x15MAX_RESTARTS_EXCEEDED\x10\x05\x12\x12\n" +
	"\x0eRESTART_FAILED\x10\x06\x12\x14\n" +
	"\x10INVALID_STRATEGY\x10\a\x12\x16\n" +
	"\x12INVALID_CHILD_SPEC\x10\b\x12\x18\n" +
	"\x14SUPERVISOR_NOT_FOUND\x10\t\x12\x1d\n" +
	"\x19SUPERVISOR_ALREADY_EXISTS\x10\n" +
	"\x12\x19\n" +
	"\x15SUPERVISOR_NOT_ACTIVE\x10\vB\x90\x05\x92A\xf6\x02\x12\x9a\x01\n" +
	"\x1aPlexSpaces Supervision API\x12CErlang/OTP-style supervision trees for fault-tolerant actor systems\"2\n" +
	"\n" +
	"PlexSpaces\x12$https://github.com/bhatti/plexspaces2\x031.0*\x02\x02\x012\x10application/json:\x10application/jsonR9\n" +
	"\x03400\x122\n" +
	"\x0fInvalid request\x12\x1f\n" +
	"\x1d\x1a\x1b.plexspaces.common.v1.ErrorR3\n" +
	"\x03404\x12,\n" +
	"\tNot found\x12\x1f\n" +
	"\x1d\x1a\x1b.plexspaces.common.v1.ErrorR?\n" +
	"\x03500\x128\n" +
	"\x15Internal server error\x12\x1f\n" +
	"\x1d\x1a\x1b.plexspaces.common.v1.Error\n" +
	"\x1dcom.plexspaces.supervision.v1B\x10SupervisionProtoP\x01Z]github.com/bhatti/PlexSpaces/sdks/go/plexspaces/proto/plexspaces/v1/supervision;supervisionv1\xa2\x02\x03PSX\xaa\x02\x19Plexspaces.Supervision.V1\xca\x02\x19Plexspaces\\Supervision\\V1\xe2\x02%Plexspaces\\Supervision\\V1\\GPBMetadata\xea\x02\x1bPlexspaces::Supervision::V1b\x06proto3"

var (
	file_plexspaces_v1_supervision_supervision_proto_rawDescOnce sync.Once
	file_plexspaces_v1_supervision_supervision_proto_rawDescData []byte
)

func file_plexspaces_v1_supervision_supervision_proto_rawDescGZIP() []byte {
	file_plexspaces_v1_supervision_supervision_proto_rawDescOnce.Do(func() {
		file_plexspaces_v1_supervision_supervision_proto_rawDescData = protoimpl.X.CompressGZIP(unsafe.Slice(unsafe.StringData(file_plexspaces_v1_supervision_supervision_proto_rawDesc), len(file_plexspaces_v1_supervision_supervision_proto_rawDesc)))
	})
	return file_plexspaces_v1_supervision_supervision_proto_rawDescData
}

var file_plexspaces_v1_supervision_supervision_proto_enumTypes = make([]protoimpl.EnumInfo, 6)
var file_plexspaces_v1_supervision_supervision_proto_msgTypes = make([]protoimpl.MessageInfo, 17)
var file_plexspaces_v1_supervision_supervision_proto_goTypes = []any{
	(SupervisionStrategy)(0),         // 0: plexspaces.supervision.v1.SupervisionStrategy
	(RestartPolicy)(0),               // 1: plexspaces.supervision.v1.RestartPolicy
	(ChildStatus)(0),                 // 2: plexspaces.supervision.v1.ChildStatus
	(EventPropagation)(0),            // 3: plexspaces.supervision.v1.EventPropagation
	(SupervisorEventType)(0),         // 4: plexspaces.supervision.v1.SupervisorEventType
	(SupervisionErrorCode)(0),        // 5: plexspaces.supervision.v1.SupervisionErrorCode
	(*AdaptiveConfig)(nil),           // 6: plexspaces.supervision.v1.AdaptiveConfig
	(*ExponentialBackoffConfig)(nil), // 7: plexspaces.supervision.v1.ExponentialBackoffConfig
	(*ChildSpec)(nil),                // 8: plexspaces.supervision.v1.ChildSpec
	(*SupervisorSpec)(nil),           // 9: plexspaces.supervision.v1.SupervisorSpec
	(*SupervisorState)(nil),          // 10: plexspaces.supervision.v1.SupervisorState
	(*ChildState)(nil),               // 11: plexspaces.supervision.v1.ChildState
	(*RestartEvent)(nil),             // 12: plexspaces.supervision.v1.RestartEvent
	(*SupervisorStats)(nil),          // 13: plexspaces.supervision.v1.SupervisorStats
	(*SupervisorEvent)(nil),          // 14: plexspaces.supervision.v1.SupervisorEvent
	(*ChildInfo)(nil),                // 15: plexspaces.supervision.v1.ChildInfo
	(*ChildCount)(nil),               // 16: plexspaces.supervision.v1.ChildCount
	(*SupervisionError)(nil),         // 17: plexspaces.supervision.v1.SupervisionError
	nil,                              // 18: plexspaces.supervision.v1.ChildSpec.ArgsEntry
	nil,                              // 19: plexspaces.supervision.v1.ChildSpec.MetadataEntry
	nil,                              // 20: plexspaces.supervision.v1.SupervisorSpec.MetadataEntry
	nil,                              // 21: plexspaces.supervision.v1.SupervisorStats.FailurePatternsEntry
	nil,                              // 22: plexspaces.supervision.v1.SupervisionError.ContextEntry
	(*v1.ActorIdentity)(nil),         // 23: plexspaces.common.v1.ActorIdentity
	(*durationpb.Duration)(nil),      // 24: google.protobuf.Duration
	(*v1.Facet)(nil),                 // 25: plexspaces.common.v1.Facet
	(*timestamppb.Timestamp)(nil),    // 26: google.protobuf.Timestamp
}
var file_plexspaces_v1_supervision_supervision_proto_depIdxs = []int32{
	23, // 0: plexspaces.supervision.v1.ChildSpec.actor_identity:type_name -> plexspaces.common.v1.ActorIdentity
	18, // 1: plexspaces.supervision.v1.ChildSpec.args:type_name -> plexspaces.supervision.v1.ChildSpec.ArgsEntry
	1,  // 2: plexspaces.supervision.v1.ChildSpec.restart:type_name -> plexspaces.supervision.v1.RestartPolicy
	24, // 3: plexspaces.supervision.v1.ChildSpec.shutdown_timeout:type_name -> google.protobuf.Duration
	9,  // 4: plexspaces.supervision.v1.ChildSpec.supervisor:type_name -> plexspaces.supervision.v1.SupervisorSpec
	25, // 5: plexspaces.supervision.v1.ChildSpec.facets:type_name -> plexspaces.common.v1.Facet
	19, // 6: plexspaces.supervision.v1.ChildSpec.metadata:type_name -> plexspaces.supervision.v1.ChildSpec.MetadataEntry
	7,  // 7: plexspaces.supervision.v1.ChildSpec.exponential_backoff:type_name -> plexspaces.supervision.v1.ExponentialBackoffConfig
	0,  // 8: plexspaces.supervision.v1.SupervisorSpec.strategy:type_name -> plexspaces.supervision.v1.SupervisionStrategy
	24, // 9: plexspaces.supervision.v1.SupervisorSpec.max_restart_window:type_name -> google.protobuf.Duration
	8,  // 10: plexspaces.supervision.v1.SupervisorSpec.children:type_name -> plexspaces.supervision.v1.ChildSpec
	20, // 11: plexspaces.supervision.v1.SupervisorSpec.metadata:type_name -> plexspaces.supervision.v1.SupervisorSpec.MetadataEntry
	6,  // 12: plexspaces.supervision.v1.SupervisorSpec.adaptive:type_name -> plexspaces.supervision.v1.AdaptiveConfig
	9,  // 13: plexspaces.supervision.v1.SupervisorState.spec:type_name -> plexspaces.supervision.v1.SupervisorSpec
	11, // 14: plexspaces.supervision.v1.SupervisorState.children:type_name -> plexspaces.supervision.v1.ChildState
	12, // 15: plexspaces.supervision.v1.SupervisorState.restart_history:type_name -> plexspaces.supervision.v1.RestartEvent
	8,  // 16: plexspaces.supervision.v1.ChildState.spec:type_name -> plexspaces.supervision.v1.ChildSpec
	2,  // 17: plexspaces.supervision.v1.ChildState.status:type_name -> plexspaces.supervision.v1.ChildStatus
	26, // 18: plexspaces.supervision.v1.ChildState.started_at:type_name -> google.protobuf.Timestamp
	26, // 19: plexspaces.supervision.v1.ChildState.last_restart:type_name -> google.protobuf.Timestamp
	26, // 20: plexspaces.supervision.v1.RestartEvent.timestamp:type_name -> google.protobuf.Timestamp
	0,  // 21: plexspaces.supervision.v1.RestartEvent.strategy:type_name -> plexspaces.supervision.v1.SupervisionStrategy
	21, // 22: plexspaces.supervision.v1.SupervisorStats.failure_patterns:type_name -> plexspaces.supervision.v1.SupervisorStats.FailurePatternsEntry
	4,  // 23: plexspaces.supervision.v1.SupervisorEvent.event_type:type_name -> plexspaces.supervision.v1.SupervisorEventType
	0,  // 24: plexspaces.supervision.v1.SupervisorEvent.new_strategy:type_name -> plexspaces.supervision.v1.SupervisionStrategy
	2,  // 25: plexspaces.supervision.v1.ChildInfo.status:type_name -> plexspaces.supervision.v1.ChildStatus
	5,  // 26: plexspaces.supervision.v1.SupervisionError.code:type_name -> plexspaces.supervision.v1.SupervisionErrorCode
	22, // 27: plexspaces.supervision.v1.SupervisionError.context:type_name -> plexspaces.supervision.v1.SupervisionError.ContextEntry
	26, // 28: plexspaces.supervision.v1.SupervisionError.timestamp:type_name -> google.protobuf.Timestamp
	29, // [29:29] is the sub-list for method output_type
	29, // [29:29] is the sub-list for method input_type
	29, // [29:29] is the sub-list for extension type_name
	29, // [29:29] is the sub-list for extension extendee
	0,  // [0:29] is the sub-list for field type_name
}

func init() { file_plexspaces_v1_supervision_supervision_proto_init() }
func file_plexspaces_v1_supervision_supervision_proto_init() {
	if File_plexspaces_v1_supervision_supervision_proto != nil {
		return
	}
	file_plexspaces_v1_supervision_supervision_proto_msgTypes[2].OneofWrappers = []any{}
	file_plexspaces_v1_supervision_supervision_proto_msgTypes[3].OneofWrappers = []any{}
	type x struct{}
	out := protoimpl.TypeBuilder{
		File: protoimpl.DescBuilder{
			GoPackagePath: reflect.TypeOf(x{}).PkgPath(),
			RawDescriptor: unsafe.Slice(unsafe.StringData(file_plexspaces_v1_supervision_supervision_proto_rawDesc), len(file_plexspaces_v1_supervision_supervision_proto_rawDesc)),
			NumEnums:      6,
			NumMessages:   17,
			NumExtensions: 0,
			NumServices:   0,
		},
		GoTypes:           file_plexspaces_v1_supervision_supervision_proto_goTypes,
		DependencyIndexes: file_plexspaces_v1_supervision_supervision_proto_depIdxs,
		EnumInfos:         file_plexspaces_v1_supervision_supervision_proto_enumTypes,
		MessageInfos:      file_plexspaces_v1_supervision_supervision_proto_msgTypes,
	}.Build()
	File_plexspaces_v1_supervision_supervision_proto = out.File
	file_plexspaces_v1_supervision_supervision_proto_goTypes = nil
	file_plexspaces_v1_supervision_supervision_proto_depIdxs = nil
}
