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

//! Actor lifecycle state enum and proto conversion helpers.

/// Actor state - matches proto ActorState enum exactly
///
/// ## State Transitions
/// ```
/// CREATING -> ACTIVATING -> ACTIVE -> DEACTIVATING -> INACTIVE
///                        \-> MIGRATING -> ACTIVE (on new node)
///                        \-> FAILED -> (supervisor restarts)
///                        \-> TERMINATED (permanent stop)
/// ```
///
/// ## Design Notes
/// - This enum matches proto `ActorState` exactly for consistency
/// - FAILED state includes error message for debugging
/// - All actors (virtual or not) use the same ActorState
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorState {
    /// Unspecified state (should never occur in valid states)
    Unspecified,
    /// Actor is created but not yet started (replaces "Initializing")
    Creating,
    /// Actor is running and processing messages
    Active,
    /// Actor is suspended/not processing messages (replaces "Suspended")
    Inactive,
    /// Actor is activating (loading state, running on_activate)
    Activating,
    /// Actor is deactivating (saving state, running on_deactivate)
    Deactivating,
    /// Actor is stopping (shutdown in progress) - NEW
    Stopping,
    /// Actor is migrating to another node
    Migrating,
    /// Actor has crashed (includes error message)
    Failed(String),
    /// Actor has permanently stopped (replaces "Stopped")
    Terminated,
}

// NOTE: ActorLifecycle trait REMOVED - lifecycle is now STATIC (always present)
// This was a design flaw: lifecycle hooks are CORE to every actor, not optional.
// The new design has lifecycle methods directly on Actor (see impl Actor below)

impl ActorState {
    /// Convert to proto ActorState enum
    pub fn to_proto(&self) -> plexspaces_proto::v1::actor::ActorState {
        use plexspaces_proto::v1::actor::ActorState as ProtoState;
        match self {
            ActorState::Unspecified => ProtoState::ActorStateUnspecified,
            ActorState::Creating => ProtoState::ActorStateCreating,
            ActorState::Active => ProtoState::ActorStateActive,
            ActorState::Inactive => ProtoState::ActorStateInactive,
            ActorState::Activating => ProtoState::ActorStateActivating,
            ActorState::Deactivating => ProtoState::ActorStateDeactivating,
            ActorState::Stopping => ProtoState::ActorStateStopping,
            ActorState::Migrating => ProtoState::ActorStateMigrating,
            ActorState::Failed(_) => ProtoState::ActorStateFailed,
            ActorState::Terminated => ProtoState::ActorStateTerminated,
        }
    }

    /// Convert from proto ActorState enum
    ///
    /// Note: For FAILED state, error message should be retrieved from Actor.error_message field
    pub fn from_proto(
        proto: plexspaces_proto::v1::actor::ActorState,
        error_message: Option<String>,
    ) -> Self {
        use plexspaces_proto::v1::actor::ActorState as ProtoState;
        match proto {
            ProtoState::ActorStateUnspecified => ActorState::Unspecified,
            ProtoState::ActorStateCreating => ActorState::Creating,
            ProtoState::ActorStateActive => ActorState::Active,
            ProtoState::ActorStateInactive => ActorState::Inactive,
            ProtoState::ActorStateActivating => ActorState::Activating,
            ProtoState::ActorStateDeactivating => ActorState::Deactivating,
            ProtoState::ActorStateStopping => ActorState::Stopping,
            ProtoState::ActorStateMigrating => ActorState::Migrating,
            ProtoState::ActorStateFailed => ActorState::Failed(error_message.unwrap_or_default()),
            ProtoState::ActorStateTerminated => ActorState::Terminated,
        }
    }
}
