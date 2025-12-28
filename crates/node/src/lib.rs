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

//! Node management and distribution for PlexSpaces
//!
//! Provides location transparency and distribution capabilities,
//! inspired by Erlang's node system but elevated for modern needs.

#![warn(missing_docs)]
#![warn(clippy::all)]

// Main node module
mod r#mod;
pub use r#mod::*;

// Application manager moved to core crate to break cyclic dependency
// Re-export for backward compatibility
pub use plexspaces_core::ApplicationManager;

// Extension trait for node-specific ApplicationManager functionality
pub mod application_manager_ext;
pub use application_manager_ext::ApplicationManagerExt;

// gRPC service for remote actor communication
pub mod grpc_service;

// gRPC client for remote actor communication
pub mod grpc_client;

// gRPC service for distributed TupleSpace operations (Phase 3)
pub mod tuplespace_service;

// HTTP router for blob service endpoints
pub mod blob_http_router;

// HTTP router for gRPC-Gateway routes (actor invocation, etc.)
// TODO: Implement HTTP gateway for InvokeActor routes
// pub mod http_router;

// Node registry removed - replaced by object-registry

// Health service for K8s probes (Phase 5)
pub mod health_service;

// Health checker trait for dependency checks
pub mod health_checker;

// Circuit breaker wrapper for health checkers
pub mod health_checker_circuit_breaker;

// Metrics service for Prometheus export (Phase 5)
pub mod metrics_service;

// OpenTelemetry tracing setup (Phase 5)
pub mod tracing_setup;

// Standard gRPC Health Service implementation (Phase 5)
pub mod standard_health_service;

// gRPC health service with dependency checks
pub mod grpc_health_service;

// System service implementation
pub mod system_service;

// Automatic dependency registration (includes built-in dependencies)
pub mod dependency_registration;

// External dependency health checkers (MinIO, DynamoDB, SQS)
pub mod external_dependency_checkers;

// Application service for application-level deployment
pub mod application_service;
pub mod application_impl;
pub mod wasm_application;
pub mod wasm_message_sender;

// Firecracker VM service for VM management and application deployment
#[cfg(feature = "firecracker")]
pub mod firecracker_service;

// Graceful shutdown coordinator (Phase 5)
pub mod shutdown_coordinator;

// Actor builder for fluent actor creation API

// Node builder for fluent node creation API
pub mod node_builder;
pub mod service_wrappers;
pub mod virtual_actor_wrapper;
// TODO: regular_actor_wrapper module file missing - commented out until file is created
// pub mod regular_actor_wrapper;
pub use node_builder::NodeBuilder;

// Make Node implement Service trait for ServiceLocator
impl plexspaces_core::Service for Node {}

// Configuration bootstrap (Erlang/OTP-inspired)
pub mod config_bootstrap;
// TODO: Complete config_loader implementation - temporarily disabled due to proto mismatches
pub mod config_loader;
pub use config_loader::ConfigLoader;
mod config_loader_yaml;
mod config_loader_convert;
pub mod metrics_helper;
pub mod service_locator_helpers;
pub use config_bootstrap::{ConfigBootstrap, ConfigError};
pub use metrics_helper::CoordinationComputeTracker;
pub use service_locator_helpers::create_default_service_locator;
pub mod health_service_helpers;
pub use health_service_helpers::{create_and_register_health_service, create_default_health_service};

// Object registry helper functions
pub mod object_registry_helpers;

// Re-export for convenience
pub use plexspaces_proto::node::v1::{NodeConfig, ReleaseSpec};
