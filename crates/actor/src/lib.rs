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
pub use actor_factory::ActorFactory;
pub use actor_factory_impl::ActorFactoryImpl;
pub use virtual_actor_wrapper::VirtualActorWrapper;

// Actor registry
// pub mod registry; // TEMPORARILY DISABLED - awaiting migration to object_registry proto
