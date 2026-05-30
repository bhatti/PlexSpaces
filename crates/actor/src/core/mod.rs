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

//! Core types module — re-exports all types from the merged modules at the actor crate root.
//!
//! Previously this module was a thin wrapper over `plexspaces_core`. After the merge,
//! all core source modules live directly in `crates/actor/src/` and this module
//! re-exports them so callers using `crate::core::X` (or `plexspaces_actor::core::X`)
//! continue to work without changes.

// Actor trait and core error types
pub use crate::actor_types::{Actor, ActorError, BehaviorContext, BehaviorError, BehaviorType};

// Re-export all flat items from the merged core modules.
pub use crate::actor_context::{
    ActorContext, ActorService, ChannelService, FacetService, LinkProvider, ObjectRegistration,
    ObjectRegistry, ProcessGroupService, TupleSpaceProvider,
};
pub use crate::actor_factory::ActorFactory;
pub use crate::actor_id::{
    wasm_root_supervisor_actor_type_from_application_name,
    wasm_worker_actor_type_from_application_name, ActorId, ActorIdError,
};
pub use crate::actor_monitor::{
    create_down_message, create_exit_message, create_ping_message, create_pong_message,
    exit_reason_to_string, is_ctrl_message, start_monitor_gc_task, ActorMonitor,
    LocalOnlyActorService, MonitorLink, CTRL_MSG_PREFIX,
};
pub use crate::actor_registry::{ActorRegistry, ActorRegistryError, TemporarySenderEntry};
pub use crate::actor_state_checker::ActorStateHandle;
pub use crate::actor_trait::MessageSender;
pub use crate::actor_visibility::{
    check_actor_visibility_for_messaging, enforce_visibility_for_actor_ref_messaging,
};
pub use crate::application_node_trait::ApplicationNode;
pub use crate::behavior_factory::{BehaviorFactory, BehaviorFactoryError, BehaviorRegistry};
pub use crate::constants::{TEMP_SENDER_ACTOR_TYPE, TEMP_SENDER_PREFIX};
pub use crate::elastic_pool_service::{ElasticPoolService, PoolServiceError};
pub use crate::exit_reason::{ExitAction, ExitReason};
pub use crate::facet_factories::{
    CachingFacetFactory, EventEmitterFacetFactory, HttpClientFacetFactory, KeyValueFacetFactory,
    LockFacetFactory, LoggingFacetFactory, MetricsFacetFactory, ProcessGroupFacetFactory,
    RegistryFacetFactory,
};
pub use crate::facet_helpers::{create_facet_from_proto, create_facets_from_proto};
pub use crate::facet_service_wrapper::{FacetManagerServiceWrapper, FacetRegistryServiceWrapper};
pub use crate::grpc_connection_manager::{GrpcConnectionManager, ServiceType};
pub use crate::journal_storage::{JournalError, JournalResult, JournalStorage};
pub use crate::keyvalue_store::{KeyValueStore, KeyValueStoreError, KeyValueStoreResult};
pub use crate::message_metrics::ActorMetrics;
pub use crate::metrics_renderer::MetricsPrometheusRenderer;
pub use crate::metrics_service_access::MetricsServiceAccess;
pub use crate::monitoring::NodeConnectionInfo;
pub use crate::node_connectivity::{ConnectNodesResult, NodeConnectivity};
pub use crate::outbound_http_client::{
    HttpHeader, OutboundHttpClient, OutboundHttpClientError, OutboundHttpRequest,
    OutboundHttpResponse,
};
pub use crate::process_metrics::{ProcessResourceSampler, ProcessResourceSnapshot};
pub use crate::prometheus_text::{
    actor_metrics_from_exposition_for_namespace, local_prometheus_recorder_chart_summary,
    max_histogram_bucket_upper_bound_for_labels, overlay_node_operational_counters_from_exposition,
    sum_counter_all_label_sets, sum_counter_for_labels, sum_sample_values_all_series,
    sum_sample_values_for_labels, LocalPrometheusRecorderChartSummary,
};
pub use crate::reply_waiter::{ReplyWaiter, ReplyWaiterError, ReplyWaiterRegistry};
pub use crate::secret_masker::{mask_map_secrets, mask_release_spec, SecretMasker, DEFAULT_MASK};
pub use crate::service_locator::{
    apply_request_context_to_grpc_metadata, request_context_from_grpc_request,
};
pub use crate::service_locator_trait::{
    ApplicationManager, BlobServiceTrait, InitializableServiceLocator, NodeRegistryTrait,
    ServiceLocator, WasmRuntimeTrait,
};
pub use crate::service_trait::{Service, ServiceName};
pub use crate::spawn_init_parse::legacy_spawn_init_json_to_role_and_args;
pub use crate::virtual_actor_lifecycle_facet::{
    VirtualActorLifecycleFacet, VirtualActorLifecycleState,
};
pub use crate::virtual_actor_manager::{
    wasm_init_payload, VirtualActorError, VirtualActorManager, VirtualActorMetadata,
};
pub use crate::virtual_actor_registration::{
    proto_facets_for_registration, register_virtual_actor_definition,
    register_virtual_actor_type_consistent, VirtualActorTypeSpec,
};

// Sub-module paths needed by callers
pub use crate::actor_context;
pub use crate::actor_types;
pub use crate::actor_visibility;
pub use crate::behavior_factory;
pub use crate::facet_factories;
pub use crate::facet_helpers;
pub use crate::facet_service_wrapper;
pub use crate::health_checker;
pub use crate::health_reporter;
pub use crate::health_service;
pub use crate::monitoring;

// Re-export from external crates (same as core did)
pub use plexspaces_common::{RequestContext, RequestContextError, RequestContextExt};
pub use plexspaces_facet::FacetManager;
pub use plexspaces_locks::{LockError, LockManager, LockResult};
pub use plexspaces_proto::actor::v1::ActorSpawnSpec;
pub use plexspaces_proto::common::v1::Message;
pub use plexspaces_proto::locks::prv::{
    AcquireLockOptions, Lock, ReleaseLockOptions, RenewLockOptions,
};
pub use plexspaces_service_traits::{ActorRef, ActorStateChecker, ServiceLocatorBase};

// Health re-exports
pub use crate::health::checker::{
    run_health_check, HealthCheckContext, HealthCheckError, HealthCheckResult, HealthChecker,
};
pub use crate::health::reporter::HealthReporter;
pub use crate::health::service::PlexSpacesHealthReporter;
