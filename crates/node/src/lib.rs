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

//! Node management and distribution for PlexSpaces
//!
//! Provides location transparency and distribution capabilities,
//! inspired by Erlang's node system but elevated for modern needs.

#![warn(missing_docs)]
#![warn(clippy::all)]

// Main node module

mod r#mod;
pub use r#mod::*;

// gRPC client for remote actor communication
pub mod grpc_client;

// HTTP router for blob service endpoints
pub mod blob_http_router;

// JWT validation for HTTP gateway (tenant_id from claims when auth enabled)
pub mod http_jwt;

// HTTP gateway module (types, helpers, middleware for actor ask/tell via HTTP)
pub mod http_gateway;

// Modular HTTP REST bridge route handlers
pub mod http_routes;

// Wrapper so NodeServiceServer and NodeConnectivity share the same Arc<NodeServiceImpl>
mod node_service_handler;

/// Health module - consolidated health checking and service functionality
pub mod health;
pub use health::circuit_breaker as health_checker_circuit_breaker;

// OpenTelemetry tracing setup (Phase 5)
pub mod tracing_setup;

// Standard gRPC Health Service implementation (Phase 5)
pub mod standard_health_service;

// gRPC health service with dependency checks
pub mod grpc_health_service;

// Automatic dependency registration (includes built-in dependencies)
pub mod dependency_registration;

// External dependency health checkers (embedded object store, DynamoDB, SQS)
pub mod external_dependency_checkers;

// Graceful shutdown coordinator (Phase 5)
pub mod shutdown_coordinator;

// Actor builder for fluent actor creation API

// Node builder for fluent node creation API
pub mod node_builder;
pub mod service_wrappers;
// TODO: regular_actor_wrapper module file missing - commented out until file is created
// pub mod regular_actor_wrapper;
pub use node_builder::NodeBuilder;

// Make Node implement Service trait for ServiceLocator
impl plexspaces_actor::Service for Node {
    fn service_name(&self) -> String {
        use plexspaces_common::ServiceNameExt;
        plexspaces_actor::ServiceName::ServiceNameNode
            .as_str()
            .to_string()
    }
}

/// Config module - consolidated configuration loading and bootstrapping
pub mod config;
pub use config::bootstrap::{ConfigBootstrap, ConfigError};
pub use config::loader::ConfigLoader;
// Backward compatibility aliases
pub use config::bootstrap as config_bootstrap;
pub use config::loader as config_loader;

// WASM applications auto-deploy loader (Tomcat-style webapps)
pub mod wasm_apps_loader;

pub mod metrics_helper;
pub(crate) mod tls_server;
pub mod service_locator_helpers;
pub use metrics_helper::CoordinationComputeTracker;
pub use service_locator_helpers::create_default_service_locator;

// Health service helpers (uses health module)
pub use health::helpers as health_service_helpers;
pub use health::helpers::{create_and_register_health_service, create_default_health_service};

// Object registry helper functions
pub mod object_registry_helpers;

// Re-export for convenience
pub use plexspaces_proto::node::v1::{NodeConfig, ReleaseSpec};
