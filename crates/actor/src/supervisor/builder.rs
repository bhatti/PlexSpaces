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

//! Supervisor errors, builder, and utility types.

use std::sync::Arc;
use tokio::sync::mpsc;

use crate::core::ActorId;

use super::{SupervisionStrategy, Supervisor, SupervisorEvent};

/// Supervisor errors
///
/// ## Error Types
/// All errors are returned when supervisor operations fail.
/// Errors are designed to be actionable and include context.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    /// Actor creation failed
    ///
    /// ## When This Occurs
    /// - Factory function returns an error
    /// - Actor initialization fails
    ///
    /// ## Context
    /// The error message includes the original error from the factory.
    #[error("Actor creation failed: {0}")]
    ActorCreationFailed(String),

    /// Child not found
    ///
    /// ## When This Occurs
    /// - Attempting to remove a child that doesn't exist
    /// - Attempting to restart a child that doesn't exist
    ///
    /// ## Context
    /// The `ActorId` of the missing child is included.
    #[error("Child not found: {0:?}")]
    ChildNotFound(ActorId),

    /// Maximum restarts exceeded
    ///
    /// ## When This Occurs
    /// - Child has been restarted more than `max_restarts` times
    /// - Restarts occurred within the `within_seconds` time window
    ///
    /// ## Behavior
    /// When this error occurs, the supervisor stops attempting to restart
    /// the child and may escalate to the parent supervisor (if present).
    #[error("Max restarts exceeded")]
    MaxRestartsExceeded,

    /// Restart failed
    ///
    /// ## When This Occurs
    /// - Actor factory fails during restart
    /// - Actor start fails during restart
    ///
    /// ## Context
    /// The error message includes the original error from the restart attempt.
    #[error("Restart failed: {0}")]
    RestartFailed(String),

    /// Invalid supervision strategy
    ///
    /// ## When This Occurs
    /// - Unknown strategy type is provided
    /// - Strategy configuration is invalid
    ///
    /// ## Context
    /// The error message includes the invalid strategy identifier or description.
    #[error("Invalid strategy: {0}")]
    InvalidStrategy(String),

    /// Configuration error
    ///
    /// ## When This Occurs
    /// - Invalid supervisor configuration from proto/TOML
    /// - Missing required fields
    /// - Invalid enum values
    ///
    /// ## Context
    /// The error message includes details about what is invalid.
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

impl SupervisorError {
    /// Returns the proto error code corresponding to this error variant.
    pub fn code(&self) -> plexspaces_proto::actor::v1::SupervisorErrorCode {
        use plexspaces_proto::actor::v1::SupervisorErrorCode;
        match self {
            SupervisorError::ActorCreationFailed(_) => {
                SupervisorErrorCode::SupervisorErrorChildStartFailed
            }
            SupervisorError::ChildNotFound(_) => SupervisorErrorCode::SupervisorErrorChildNotFound,
            SupervisorError::MaxRestartsExceeded => {
                SupervisorErrorCode::SupervisorErrorMaxRestartsExceeded
            }
            SupervisorError::RestartFailed(_) => {
                SupervisorErrorCode::SupervisorErrorChildStartFailed
            }
            SupervisorError::InvalidStrategy(_) => SupervisorErrorCode::SupervisorErrorConfigError,
            SupervisorError::ConfigError(_) => SupervisorErrorCode::SupervisorErrorConfigError,
        }
    }
}

/// Supervisor builder for fluent API
///
/// ## Example
/// ```rust,ignore
/// let (supervisor, event_rx) = SupervisorBuilder::new("my-supervisor-label".to_string())
///     .with_strategy(SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 60 })
///     .add_child(ChildSpec::worker_sync(
///         child_actor_id,
///         Arc::new(|| Ok(actor)),
///         actor_ref,
///     ))
///     .build(service_locator)
///     .await?;
/// ```
pub struct SupervisorBuilder {
    /// Opaque supervisor label (same semantics as [`Supervisor::new`] first argument).
    id: String,
    /// Supervision strategy
    strategy: SupervisionStrategy,
    /// Children to add (ChildSpec - proto-first design)
    children: Vec<crate::ChildSpec>,
    /// Parent supervisor (for hierarchical trees)
    parent: Option<Arc<Supervisor>>,
}

impl SupervisorBuilder {
    /// Create a new supervisor builder.
    ///
    /// `id` is the same **opaque supervisor label** as [`Supervisor::new`]'s first argument (logging / metrics), not a child [`ActorId`].
    pub fn new(id: String) -> Self {
        SupervisorBuilder {
            id,
            strategy: SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 60,
            },
            children: Vec::new(),
            parent: None,
        }
    }

    /// Set supervision strategy
    pub fn with_strategy(mut self, strategy: SupervisionStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Add a child specification (ChildSpec - proto-first design)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let child_actor_id = ActorId::new("worker1", "worker", "ns", "node1").unwrap();
    /// builder.add_child(ChildSpec::worker_sync(
    ///     child_actor_id,
    ///     Arc::new(|| Ok(actor)),
    ///     actor_ref,
    /// ))
    /// ```
    pub fn add_child(mut self, spec: crate::ChildSpec) -> Self {
        self.children.push(spec);
        self
    }

    /// Set parent supervisor
    pub fn with_parent(mut self, parent: Arc<Supervisor>) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Build the supervisor with the given ServiceLocator.
    ///
    /// The ServiceLocator is required so that actors started by this supervisor can
    /// register in the ActorRegistry and access FacetRegistry during restarts.
    pub async fn build(
        self,
        service_locator: Arc<dyn crate::core::ServiceLocator>,
    ) -> Result<(Supervisor, mpsc::Receiver<SupervisorEvent>), SupervisorError> {
        let (mut supervisor, event_rx) = Supervisor::new(self.id, self.strategy, service_locator);

        if let Some(parent) = self.parent {
            supervisor = supervisor.with_parent(parent);
        }

        // Add all children
        for spec in self.children {
            supervisor.add_child(spec).await?;
        }

        Ok((supervisor, event_rx))
    }
}
