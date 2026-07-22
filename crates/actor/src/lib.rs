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

//! Core actor implementation for PlexSpaces
//!
//! This crate provides the foundational actor abstraction including:
//! - Actor lifecycle management
//! - Actor references (ActorRef) for location-transparent messaging
//! - Actor registry for service discovery
//! - Resource contracts and health monitoring
//!
//! Previously split between plexspaces-core and plexspaces-actor; now unified here.

#![warn(missing_docs)]
#![warn(clippy::all)]

// ---------------------------------------------------------------------------
// Modules merged from plexspaces-core (Phase 9)
// These are declared first so that crate::core and crate::behavior can use them.
// ---------------------------------------------------------------------------

pub mod actor_types;
pub use actor_types::{Actor, ActorError, BehaviorContext, BehaviorError, BehaviorType};

pub mod behavior_factory;
pub use behavior_factory::{BehaviorFactory, BehaviorFactoryError, BehaviorRegistry};

pub mod actor_context;
pub use actor_context::LinkProvider;
pub use actor_context::{
    ActorContext, ActorService, ChannelService, DiscoverOptions, FacetService, ObjectRegistration,
    ObjectRegistryHealthStatus, ObjectRegistry, ProcessGroupService, RegisterResult,
    TupleSpaceProvider,
};

pub mod actor_monitor;
pub use actor_monitor::{
    create_down_message, create_exit_message, create_ping_message, create_pong_message,
    exit_reason_to_string, is_ctrl_message, start_monitor_gc_task, ActorMonitor,
    LocalOnlyActorService, MonitorLink, CTRL_MSG_PREFIX,
};

pub mod actor_visibility;
pub use actor_visibility::{
    check_actor_visibility_for_messaging, enforce_visibility_for_actor_ref_messaging,
};

pub mod actor_registry;
pub use actor_registry::{ActorRegistrationParams, ActorRegistry, ActorRegistryError, TemporarySenderEntry};

pub mod service_trait;
pub use service_trait::{Service, ServiceName};

pub mod service_wrappers;

pub mod service_locator_trait;
pub use service_locator_trait::{
    ApplicationManager, BlobServiceTrait, InitializableServiceLocator, NodeRegistryTrait,
    ServiceLocator, WasmRuntimeTrait, WsRegistryTrait,
};

pub mod outbound_http_client;
pub use outbound_http_client::{
    HttpHeader, OutboundHttpClient, OutboundHttpClientError, OutboundHttpRequest,
    OutboundHttpResponse,
};

pub mod node_connectivity;
pub use node_connectivity::{ConnectNodesResult, NodeConnectivity};

pub mod keyvalue_store;
pub use keyvalue_store::{KeyValueStore, KeyValueStoreError, KeyValueStoreResult};

pub mod service_locator;
pub use service_locator::{
    apply_request_context_to_grpc_metadata, request_context_from_grpc_request,
};

pub mod application_node_trait;
pub use application_node_trait::ApplicationNode;

pub mod grpc_connection_manager;
pub use grpc_connection_manager::{GrpcConnectionManager, ServiceType};

pub mod grpc_transport_client;
pub use grpc_transport_client::{GrpcActorTransportClient, GrpcNodeTransportClient};

pub mod elastic_pool_service;
pub use elastic_pool_service::{ElasticPoolService, PoolServiceError};

pub mod object_registry_helpers;

pub mod actor_trait;
pub use actor_trait::MessageSender;

pub mod exit_reason;
pub use exit_reason::{ExitAction, ExitReason};

pub mod virtual_actor_lifecycle_facet;
pub use virtual_actor_lifecycle_facet::{VirtualActorLifecycleFacet, VirtualActorLifecycleState};

pub mod virtual_actor_manager;
pub use virtual_actor_manager::{
    wasm_init_payload, VirtualActorError, VirtualActorManager, VirtualActorMetadata,
};

pub mod virtual_actor_registration;
pub use virtual_actor_registration::{
    proto_facets_for_registration, register_virtual_actor_definition,
    register_virtual_actor_type_consistent, VirtualActorTypeSpec,
};

pub mod actor_state_checker;
pub use actor_state_checker::ActorStateHandle;

pub mod facet_service_wrapper;
pub use facet_service_wrapper::{FacetManagerServiceWrapper, FacetRegistryServiceWrapper};

pub mod message_metrics;
pub use message_metrics::ActorMetrics;

pub mod metrics_renderer;
pub use metrics_renderer::MetricsPrometheusRenderer;

pub mod metrics_service_access;
pub use metrics_service_access::MetricsServiceAccess;

pub mod monitoring;
pub use monitoring::NodeConnectionInfo;

pub mod process_metrics;
pub use process_metrics::{ProcessResourceSampler, ProcessResourceSnapshot};

pub mod prometheus_text;
pub use prometheus_text::{
    actor_metrics_from_exposition_for_namespace, local_prometheus_recorder_chart_summary,
    max_histogram_bucket_upper_bound_for_labels, overlay_node_operational_counters_from_exposition,
    sum_counter_all_label_sets, sum_counter_for_labels, sum_sample_values_all_series,
    sum_sample_values_for_labels, LocalPrometheusRecorderChartSummary,
};

pub mod reply_waiter;
pub use reply_waiter::{ReplyWaiter, ReplyWaiterError, ReplyWaiterRegistry};

pub mod journal_storage;
pub use journal_storage::{JournalError, JournalResult, JournalStorage};

pub mod secret_masker;
pub use secret_masker::{mask_map_secrets, mask_release_spec, SecretMasker, DEFAULT_MASK};

/// Runtime constants for actor type names and prefixes.
pub mod constants;
pub use constants::{TEMP_SENDER_ACTOR_TYPE, TEMP_SENDER_PREFIX};

pub mod actor_id;
pub use actor_id::{
    wasm_root_supervisor_actor_type_from_application_name,
    wasm_worker_actor_type_from_application_name, ActorId, ActorIdError,
};

pub mod facet_helpers;
pub use facet_helpers::{create_facet_from_proto, create_facets_from_proto};

pub mod facet_factories;
pub use facet_factories::{
    CachingFacetFactory, EventEmitterFacetFactory, HttpClientFacetFactory, KeyValueFacetFactory,
    LockFacetFactory, LoggingFacetFactory, MetricsFacetFactory, ProcessGroupFacetFactory,
    RegistryFacetFactory,
};

/// Health module - consolidated health checking, reporting, and service functionality.
pub mod health;
pub use health::checker as health_checker;
pub use health::checker::{
    run_health_check, HealthCheckContext, HealthCheckError, HealthCheckResult, HealthChecker,
};
pub use health::reporter as health_reporter;
pub use health::reporter::HealthReporter;
pub use health::service as health_service;
pub use health::service::PlexSpacesHealthReporter;

pub mod test_helpers;

// Re-export from external crates (same as plexspaces-core did)
pub use plexspaces_common::{RequestContext, RequestContextError, RequestContextExt};
pub use plexspaces_facet::FacetManager;
pub use plexspaces_locks::{LockError, LockManager, LockResult};
pub use plexspaces_proto::actor::v1::ActorSpawnSpec;
pub use plexspaces_proto::common::v1::Message;
pub use plexspaces_proto::locks::prv::{
    AcquireLockOptions, Lock, ReleaseLockOptions, RenewLockOptions,
};
pub use plexspaces_service_traits::{
    ActorRef as ServiceTraitsActorRef, ActorStateChecker, ServiceLocatorBase,
};

/// Core types module — re-exports all merged types so `crate::core::X` paths work.
pub mod core;

// ---------------------------------------------------------------------------
// OTP-style behaviors (GenServer, GenEvent, GenStateMachine, Workflow)
// ---------------------------------------------------------------------------
/// OTP-style actor behaviors (GenServer, GenEvent, GenStateMachine, Workflow).
pub mod behavior;
pub use behavior::*;

// ---------------------------------------------------------------------------
// Main actor module
// ---------------------------------------------------------------------------
mod r#mod;
pub use r#mod::*;
// ActorInstance is the runtime actor container; Actor (above) is the behavior trait.
pub use r#mod::ActorInstance;

// Resource management
pub mod resource;

// Actor reference (overrides ServiceTraitsActorRef with the full-featured version)
pub mod actor_ref;
pub use actor_ref::{ActorRef, ActorRefError};

// High-level typed actor references
pub mod typed_refs;
pub use typed_refs::{
    EventError, EventRef, FsmError, FsmRef, GenServerError, GenServerRef, WorkflowRef,
    WorkflowRefError, DEFAULT_CALL_TIMEOUT, DEFAULT_FSM_TIMEOUT, DEFAULT_OPERATION_TIMEOUT,
    DEFAULT_RUN_TIMEOUT,
};

// TTL tests
#[cfg(test)]
mod actor_ref_ttl_tests;

// Actor builder
pub mod builder;
pub use builder::ActorBuilder;

// Actor factory for spawning actors
pub mod actor_factory;
pub use actor_factory::ActorFactory;
pub mod actor_factory_impl;
pub use actor_factory_impl::ActorFactoryImpl;

// Test stub for ServiceLocator
mod test_service_locator;
pub use test_service_locator::TestServiceLocatorStub;

// Supervision tree implementation
pub mod supervisor;
pub use supervisor::*;

// Proto-based supervisor builder
pub mod supervisor_builder_proto;
pub use supervisor_builder_proto::ProtoSupervisorBuilder;

// Child specification module
pub mod child_spec;
pub use child_spec::{ChildSpec, ProtoRestartPolicy, ShutdownSpec, StartFn, StartedChild};

// Stateless helpers for parallel / collective shard-group operations
pub mod parallel;

// Re-export SupervisorStats from proto (for public API)
pub use plexspaces_proto::supervision::v1::SupervisorStats;
