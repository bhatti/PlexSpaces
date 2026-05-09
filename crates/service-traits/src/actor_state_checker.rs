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

//! Thin actor liveness check trait.
//!
//! Allows `ServiceLocatorBase` to expose actor state queries without creating
//! a dependency on the full `ActorRegistry` type from `plexspaces-core`.

use async_trait::async_trait;

use crate::ActorId;

/// Thin trait for checking whether a local actor is currently active.
///
/// # Purpose
/// `ReminderFacet` (in `plexspaces-journaling`) needs to check whether the
/// target actor is running before firing a reminder. It previously depended on
/// `ActorRegistry` directly, which would force `plexspaces-journaling` to pull
/// in `plexspaces-core` (creating a cycle). This thin trait breaks the cycle:
/// `plexspaces-core`'s `ActorRegistry` implements `ActorStateChecker`, and
/// `ServiceLocatorBase::get_actor_state_checker()` returns it.
#[async_trait]
pub trait ActorStateChecker: Send + Sync {
    /// Returns `true` when the actor identified by `actor_id` is currently in
    /// an active/running state on this node.
    async fn is_actor_state_active(&self, actor_id: &ActorId) -> bool;
}
