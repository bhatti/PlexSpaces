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

//! Core service traits for PlexSpaces.
//!
//! # Purpose
//! This crate contains the foundational service traits and types that are shared
//! between `plexspaces-actor`, `plexspaces-journaling`, and other crates that would
//! otherwise create circular dependencies through `plexspaces-core`.
//!
//! # What lives here
//! - `ActorId` — structured actor identity (moved from `plexspaces-core`)
//! - `ActorIdError` — validation errors for ActorId
//! - `JournalStorage`, `JournalError`, `JournalResult` — journal storage abstraction
//! - `ActorService` — spawn/send trait for actor operations
//! - `ActorFactory` — trait for spawning actors
//! - `ActorStateChecker` — thin trait for checking actor liveness (avoids dependency on ActorRegistry)
//! - `MessageSender` — trait for sending messages to actors
//! - `ActorStateHandle` — runtime handle for a running actor
//! - `ServiceLocatorBase` — readonly service locator base trait (safe for journaling to depend on)

pub mod actor_id;
pub mod actor_ref;
pub mod actor_state_checker;
pub mod actor_state_handle;
pub mod blob_service;
pub mod channel_service;
pub mod elastic_pool;
pub mod health;
pub mod idempotency_store;
pub mod journal_storage;
pub mod message_sender;
pub mod metrics;
pub mod node_connectivity;
pub mod object_registry;
pub mod outbound_http;
pub mod service_link_access;
pub mod service_locator_base;
pub mod transport_client;
pub mod tuplespace_provider;
pub mod wasm_runtime;

pub use actor_id::{
    wasm_root_supervisor_actor_type_from_application_name,
    wasm_worker_actor_type_from_application_name, ActorId, ActorIdError,
};
pub use actor_ref::ActorRef;
pub use actor_state_checker::ActorStateChecker;
pub use actor_state_handle::ActorStateHandle;
pub use blob_service::BlobServiceTrait;
pub use channel_service::{ChannelService, ProcessGroupService};
pub use elastic_pool::{ElasticPoolService, PoolServiceError};
pub use health::{
    run_health_check, HealthCheckContext, HealthCheckError, HealthCheckResult, HealthChecker,
    HealthReporter,
};
pub use idempotency_store::{IdempotencyError, IdempotencyOutcome, IdempotencyResult, IdempotencyStore};
pub use journal_storage::{JournalError, JournalResult, JournalStorage};
pub use message_sender::MessageSender;
pub use metrics::MetricsServiceAccess;
pub use node_connectivity::{ConnectNodesResult, NodeConnectivity};
pub use object_registry::{ObjectRegistration, ObjectRegistry, RegisterResult};
pub use outbound_http::{
    HttpHeader, OutboundHttpClient, OutboundHttpClientError, OutboundHttpRequest,
    OutboundHttpResponse,
};
pub use service_link_access::ServiceLinkAccess;
pub use service_locator_base::{ActorFactory, ActorService, ServiceLocatorBase};
pub use transport_client::{ActorTransportClient, NodeTransportClient};
pub use tuplespace_provider::TupleSpaceProvider;
pub use wasm_runtime::WasmRuntimeTrait;

/// Temporary sender ID prefix for ask() pattern.
/// Format: "{TEMP_SENDER_PREFIX}_{correlation_id}" in ActorId.name().
pub const TEMP_SENDER_PREFIX: &str = "ask";

/// Internal actor type for temporary senders used by ask/reply routing.
pub const TEMP_SENDER_ACTOR_TYPE: &str = "temporary_sender";
