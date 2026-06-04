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

//! Actor Factory trait - for spawning and activating actors
//!
//! ## Purpose
//! Provides a trait for spawning actors without depending on Node directly.
//! This allows VirtualActorManager, ActorService, and other components to spawn actors
//! without tight coupling to Node.
//!
//! ## Design
//! - Trait defined in actor crate (can return ActorRef directly, no circular dependency)
//! - Implementation (ActorFactoryImpl) lives in this crate
//! - ServiceLocator stores `Arc<dyn ActorFactory>` directly
//!
//! ## Note on spawn_built_actor
//! The `spawn_built_actor` method is NOT part of this trait because it requires
//! the concrete Actor type. Instead, it's available as a
//! method on `ActorFactoryImpl` directly. Use `get_actor_factory_impl()` helper
//! if you need to call `spawn_built_actor`.

pub use plexspaces_service_traits::ActorFactory;
