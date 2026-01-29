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

//! Core actor implementation for PlexSpaces
//!
//! This crate provides the foundational actor abstraction including:
//! - Actor lifecycle management
//! - Actor references (ActorRef) for location-transparent messaging
//! - Actor registry for service discovery
//! - Resource contracts and health monitoring

#![warn(missing_docs)]
#![warn(clippy::all)]

// Main actor module
mod r#mod;
pub use r#mod::*;

// Resource management
pub mod resource;

// Actor reference
pub mod actor_ref;
pub use actor_ref::{ActorRef, ActorRefError};

// TTL tests
#[cfg(test)]
mod actor_ref_ttl_tests;

// Actor builder (Option C: Unified Actor Design)
pub mod builder;
pub use builder::ActorBuilder;

// Actor factory for spawning actors
pub mod actor_factory;
pub mod actor_factory_impl;
// regular_actor_wrapper removed - ActorRef now implements MessageSender directly
pub mod virtual_actor_wrapper;
pub mod service_locator_helpers;
pub use actor_factory::ActorFactory;
pub use actor_factory_impl::ActorFactoryImpl;
pub use virtual_actor_wrapper::VirtualActorWrapper;
// Helper functions for ActorFactory are in plexspaces-services crate to avoid circular dependencies
// pub use service_locator_helpers::get_actor_factory; // Moved to plexspaces-services

// Re-export register_state_fetcher_callback for tests
pub use r#mod::register_state_fetcher_callback;

// Actor registry
// pub mod registry; // TEMPORARILY DISABLED - awaiting migration to object_registry proto

// Test stub for ServiceLocator (avoids dependency on services crate)
mod test_service_locator;
pub use test_service_locator::TestServiceLocatorStub;

// Supervision tree implementation (merged from supervisor crate)
pub mod supervisor;
pub use supervisor::*;

// Proto-based supervisor builder (moved from application crate)
pub mod supervisor_builder_proto;
pub use supervisor_builder_proto::ProtoSupervisorBuilder;

// Child specification module
pub mod child_spec;
pub use child_spec::{ChildSpec, StartedChild, StartFn, ShutdownSpec};

// Facet helpers module
pub mod facet_helpers;
pub use facet_helpers::{
    create_facet_from_proto, create_facets_from_proto,
    LockFacetFactory, RegistryFacetFactory, ProcessGroupFacetFactory,
};

// Re-export SupervisorStats from proto (for public API)
pub use plexspaces_proto::supervision::v1::SupervisorStats;
