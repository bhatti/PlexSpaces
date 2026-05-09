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

//! Lightweight actor reference.
//!
//! `ActorRef` is a pure data type — it carries only the structured identity
//! of an actor and has no service dependencies. Methods that need to actually
//! communicate with the actor live in `plexspaces-actor`'s richer `ActorRef`
//! implementation (which wraps a mailbox and message sender).

use crate::{ActorId, ActorIdError};
use serde::{Deserialize, Serialize};

/// Lightweight actor reference — pure data, no methods, no service dependencies.
///
/// # Purpose
/// Returned by `ActorService::spawn_actor` so callers can track the spawned
/// actor's identity without depending on the full actor runtime.
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorRef {
    /// Structured actor identity.
    pub id: ActorId,
}

impl ActorRef {
    /// Create an ActorRef from an identity.
    pub fn new(id: ActorId) -> Result<Self, ActorIdError> {
        Ok(ActorRef { id })
    }

    /// Returns `true` when the actor is on a different node than `current_node_id`.
    pub fn is_remote(&self, current_node_id: &str) -> bool {
        self.id.node_id() != current_node_id
    }

    /// Get actor ID.
    pub fn id(&self) -> &ActorId {
        &self.id
    }

    /// Get actor name (without node ID).
    pub fn actor_name(&self) -> &str {
        self.id.name()
    }

    /// Get node ID.
    pub fn node_id(&self) -> &str {
        self.id.node_id()
    }

    /// Get actor type.
    pub fn actor_type(&self) -> &str {
        self.id.actor_type()
    }

    /// Get namespace.
    pub fn namespace(&self) -> &str {
        self.id.namespace()
    }
}
