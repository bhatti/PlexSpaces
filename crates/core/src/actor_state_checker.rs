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

//! Actor lifecycle/state handle trait
//!
//! Provides the `ActorStateHandle` trait, implemented by `Actor` in `plexspaces_actor`.
//! `ActorRegistry` uses this trait to check state, access the mailbox, and stop actors
//! without creating a circular dependency on `plexspaces_actor`.

use async_trait::async_trait;
use plexspaces_proto::v1::actor::ActorState;

/// Runtime handle for a running actor.
///
/// `ActorRegistry` stores local lifecycle/state access through `ActorRef` instead of `Any`
/// downcasts or a parallel instance map.
///
/// The trait is internal to the framework and is implemented by local actor runtimes.
/// Remote actor references never carry a state handle.
///
/// `ActorRegistry` and other core runtime paths use `Arc<dyn ActorStateHandle>` so that
/// state queries and graceful shutdown can be done through a typed interface without
/// downcasting. Implemented by `Actor` in the `plexspaces_actor` crate.
#[async_trait]
pub trait ActorStateHandle: Send + Sync {
    /// Returns the actor's current state as a proto `ActorState` enum value.
    async fn actor_state(&self) -> ActorState;

    /// Stops the actor gracefully (sends shutdown signal).
    async fn stop_actor(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
