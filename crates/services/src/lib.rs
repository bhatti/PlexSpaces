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

//! # PlexSpaces Services Crate
//!
//! ## Purpose
//! Centralized service infrastructure and implementations for PlexSpaces.
//! This crate consolidates all service-related code including:
//! - ServiceLocator (service registration and retrieval)
//! - ApplicationManager (application lifecycle management)
//! - All gRPC service implementations
//!
//! ## Architecture
//! ```
//! plexspaces-core → plexspaces-services ← plexspaces-application
//!                      ↑
//!                      └── All service implementations
//! ```
//!
//! ## Design Notes
//! - All gRPC services consolidated here for easier management

pub mod actor_factory_helpers;
pub mod service_locator;
pub mod service_wrappers;

// Re-export ServiceLocatorImpl and related types
/// Single source of truth: RequestContext from gRPC metadata (tenant/namespace propagation).
pub use plexspaces_core::{
    apply_request_context_to_grpc_metadata, request_context_from_grpc_request,
};
pub use service_locator::{ServiceLocatorImpl, ServiceStorage};
// ActorFactory is now in core crate - use ServiceLocator methods directly:
// Example: service_locator.get_actor_factory().await
// Re-export Service trait from core
pub use plexspaces_core::Service;
// Re-export ServiceLocator trait from core (trait)
pub use plexspaces_core::ServiceLocator as ServiceLocatorTrait;
pub use service_wrappers::*;

// Type alias for convenience (ServiceLocatorImpl implements ServiceLocator trait)
pub type ServiceLocator = ServiceLocatorImpl;

// Service implementations
pub mod actor_service;
pub mod application_service;
pub mod blob_service;
pub mod dashboard_service;
#[cfg(feature = "firecracker")]
pub mod firecracker_service;
pub mod metrics_service;
pub mod node_address;
pub mod node_registry;
pub mod node_service;
pub mod process_group_service;
pub mod service_link_service;
pub mod system_service;
pub mod tuple_service;
pub mod wasm_file_saver;
pub mod workflow_service;

// Re-export ProcessGroupServiceImpl for convenience
pub use process_group_service::ProcessGroupServiceImpl;

// Re-export NodeRegistry for convenience
pub use node_registry::NodeRegistry;

// Re-export NodeServiceImpl for convenience
pub use node_service::NodeServiceImpl;

// Re-export application deployment helpers for consistent behavior
pub use application_service::create_default_application_spec;
