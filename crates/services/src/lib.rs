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

pub mod service_locator;
pub mod service_wrappers;

// Re-export ServiceLocatorImpl and related types
pub use service_locator::{ServiceLocatorImpl, ServiceStorage, request_context_from_grpc_request};
// Re-export Service trait from core
pub use plexspaces_core::Service;
// Re-export ServiceLocator trait from core (trait)
pub use plexspaces_core::ServiceLocator as ServiceLocatorTrait;
pub use service_wrappers::*;

// Type alias for convenience (ServiceLocatorImpl implements ServiceLocator trait)
pub type ServiceLocator = ServiceLocatorImpl;


// Service implementations
pub mod actor_service;
pub mod tuplespace_service;
pub mod blob_service;
pub mod application_service;
pub mod tuple_service;
pub mod workflow_service;
pub mod system_service;
#[cfg(feature = "firecracker")]
pub mod firecracker_service;
pub mod metrics_service;
pub mod dashboard_service;

