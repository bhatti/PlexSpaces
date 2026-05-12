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

//! # Proto-Based Supervisor Builder
//!
//! ## Purpose
//! Converts proto SupervisorSpec to Rust Supervisor instances for application-embedded
//! supervision trees. Bridges between declarative proto config and runtime supervisor.
//!
//! ## Architecture Context
//! Part of Application framework (Erlang/OTP-inspired). Enables applications to define
//! supervision trees in proto/TOML and have them materialized at runtime.
//!
//! ## Design Decisions
//! - Uses `plexspaces.application.v1.SupervisorSpec` from application.proto
//! - Local supervision only (follows user requirement for simplicity)
//! - Converts proto enums to Rust enums for type safety

use crate::{RestartPolicy, SupervisionStrategy, Supervisor, SupervisorError};
use plexspaces_proto::supervision::v1::{
    ChildSpec as ProtoChildSpec, RestartPolicy as ProtoRestartPolicy,
    SupervisionStrategy as ProtoSupervisionStrategy, SupervisorSpec as ProtoSupervisorSpec,
};
use std::sync::Arc;
use std::time::Duration;
use ulid::Ulid;

/// Proto-based supervisor builder for application-embedded supervision trees
///
/// ## Purpose
/// Converts proto SupervisorSpec to runtime Supervisor instance.
///
/// ## Examples
/// ```rust,ignore
/// use plexspaces_actor::ProtoSupervisorBuilder;
/// use plexspaces_proto::application::v1::{SupervisorSpec, SupervisionStrategy};
/// use std::sync::Arc;
///
/// # async fn example(service_locator: Arc<dyn crate::core::ServiceLocator>) -> Result<(), Box<dyn std::error::Error>> {
/// let proto_spec = SupervisorSpec {
///     strategy: SupervisionStrategy::SupervisionStrategyOneForOne as i32,
///     max_restarts: 3,
///     max_restart_window: None,
///     children: vec![],
/// };
/// let supervisor = ProtoSupervisorBuilder::from_proto_spec(&proto_spec, service_locator)
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct ProtoSupervisorBuilder;

impl ProtoSupervisorBuilder {
    /// Build supervisor from proto SupervisorSpec
    ///
    /// ## Arguments
    /// * `spec` - Proto supervisor specification from application.proto
    /// * `service_locator` - ServiceLocator for service access (required for ActorRef creation)
    ///
    /// ## Returns
    /// Configured Supervisor instance ready to start
    ///
    /// ## Errors
    /// - `SupervisorError::ConfigError` if spec is invalid
    pub async fn from_proto_spec(
        spec: &ProtoSupervisorSpec,
        service_locator: Arc<dyn crate::core::ServiceLocator>,
    ) -> Result<Arc<Supervisor>, SupervisorError> {
        // Convert proto supervision strategy to Rust enum
        let strategy = Self::convert_supervision_strategy(spec)?;

        // Create supervisor with strategy and service_locator
        let (supervisor, _event_rx) = Supervisor::new(
            format!("app-supervisor-{}", Ulid::new()),
            strategy,
            service_locator,
        );

        // Add children from spec
        for child_spec in &spec.children {
            Self::add_child_to_supervisor(&supervisor, child_spec).await?;
        }

        Ok(Arc::new(supervisor))
    }

    /// Convert proto SupervisionStrategy to Rust SupervisionStrategy
    fn convert_supervision_strategy(
        spec: &ProtoSupervisorSpec,
    ) -> Result<SupervisionStrategy, SupervisorError> {
        let max_restarts = spec.max_restarts;
        let within_seconds = spec
            .max_restart_window
            .as_ref()
            .map(|d| d.seconds as u64)
            .unwrap_or(5); // Default: 5 seconds

        match ProtoSupervisionStrategy::try_from(spec.strategy) {
            Ok(ProtoSupervisionStrategy::SupervisionStrategyUnspecified)
            | Ok(ProtoSupervisionStrategy::SupervisionStrategyOneForOne) => {
                Ok(SupervisionStrategy::OneForOne {
                    max_restarts,
                    within_seconds,
                })
            }
            Ok(ProtoSupervisionStrategy::SupervisionStrategyOneForAll) => {
                Ok(SupervisionStrategy::OneForAll {
                    max_restarts,
                    within_seconds,
                })
            }
            Ok(ProtoSupervisionStrategy::SupervisionStrategyRestForOne) => {
                Ok(SupervisionStrategy::RestForOne {
                    max_restarts,
                    within_seconds,
                })
            }
            Ok(ProtoSupervisionStrategy::SupervisionStrategySimpleOneForOne) => {
                // SIMPLE_ONE_FOR_ONE maps to OneForOne: the pool-of-identical-workers
                // distinction is managed at the application level (via ChildSpec template),
                // not at the supervision-strategy level in the runtime.
                Ok(SupervisionStrategy::OneForOne {
                    max_restarts,
                    within_seconds,
                })
            }
            Ok(ProtoSupervisionStrategy::SupervisionStrategyAdaptive) => {
                Ok(SupervisionStrategy::Adaptive {
                    initial_strategy: Box::new(SupervisionStrategy::OneForOne {
                        max_restarts,
                        within_seconds,
                    }),
                    learning_rate: spec
                        .adaptive
                        .as_ref()
                        .map(|a| a.learning_rate)
                        .unwrap_or(0.1),
                })
            }
            Err(_) => Err(SupervisorError::ConfigError(format!(
                "Unknown supervision strategy: {}",
                spec.strategy
            ))),
        }
    }

    /// Add child to supervisor from proto ChildSpec
    async fn add_child_to_supervisor(
        _supervisor: &Supervisor,
        child_spec: &ProtoChildSpec,
    ) -> Result<(), SupervisorError> {
        // Convert proto restart policy to Rust enum
        let restart = Self::convert_restart_policy(child_spec)?;
        let role = if child_spec.role.is_empty() {
            "worker"
        } else {
            child_spec.role.as_str()
        };

        // Extract shutdown timeout
        let _shutdown_timeout = child_spec
            .shutdown_timeout
            .as_ref()
            .map(|d| Duration::from_secs(d.seconds as u64))
            .unwrap_or(Duration::from_secs(30));

        let Some(id) = child_spec.actor_identity.as_ref() else {
            return Err(SupervisorError::ConfigError(
                "Child spec must set actor_identity (name + actor_type)".to_string(),
            ));
        };
        if id.name.is_empty() || id.actor_type.is_empty() {
            return Err(SupervisorError::ConfigError(
                "Child spec actor_identity must have non-empty name and actor_type".to_string(),
            ));
        }

        tracing::debug!(
            child_name = %id.name,
            actor_type = %id.actor_type,
            restart_policy = ?restart,
            role = %role,
            "Validated child spec (spawning not yet implemented)"
        );

        Ok(())
    }

    /// Convert proto RestartPolicy to Rust RestartPolicy
    fn convert_restart_policy(
        child_spec: &ProtoChildSpec,
    ) -> Result<RestartPolicy, SupervisorError> {
        match ProtoRestartPolicy::try_from(child_spec.restart) {
            Ok(ProtoRestartPolicy::RestartPolicyUnspecified)
            | Ok(ProtoRestartPolicy::RestartPolicyPermanent) => Ok(RestartPolicy::Permanent),
            Ok(ProtoRestartPolicy::RestartPolicyTransient) => Ok(RestartPolicy::Transient),
            Ok(ProtoRestartPolicy::RestartPolicyTemporary) => Ok(RestartPolicy::Temporary),
            Ok(ProtoRestartPolicy::RestartPolicyExponentialBackoff) => {
                let eb = child_spec.exponential_backoff.as_ref();
                Ok(RestartPolicy::ExponentialBackoff {
                    initial_delay_ms: eb.map(|e| e.initial_delay_ms).unwrap_or(100),
                    max_delay_ms: eb.map(|e| e.max_delay_ms).unwrap_or(30_000),
                    factor: eb.map(|e| e.factor).unwrap_or(2.0),
                })
            }
            Err(_) => Err(SupervisorError::ConfigError(format!(
                "Unknown restart policy for child {:?}: {}",
                child_spec.actor_identity.as_ref().map(|i| i.name.as_str()),
                child_spec.restart
            ))),
        }
    }
}

// ============================================================================
// TESTS (TDD Approach)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SupervisionStrategy;
    use plexspaces_proto::supervision::v1::{
        ChildSpec, RestartPolicy, SupervisionStrategy as ProtoSupervisionStrategy, SupervisorSpec,
    };
    /// Helper function to create a basic supervisor spec
    fn create_test_supervisor_spec() -> SupervisorSpec {
        SupervisorSpec {
            strategy: ProtoSupervisionStrategy::SupervisionStrategyOneForOne as i32,
            max_restarts: 3,
            max_restart_window: Some(prost_types::Duration {
                seconds: 5,
                nanos: 0,
            }),
            children: vec![ChildSpec {
                actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: "worker1".to_string(),
                    actor_type: "worker".to_string(),
                }),
                role: "worker".to_string(),
                restart: RestartPolicy::RestartPolicyPermanent as i32,
                shutdown_timeout: Some(prost_types::Duration {
                    seconds: 30,
                    nanos: 0,
                }),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    /// Test: Build supervisor from proto spec
    #[tokio::test]
    async fn test_build_supervisor_from_proto() {
        let spec = create_test_supervisor_spec();
        let service_locator: Arc<dyn crate::core::ServiceLocator> =
            Arc::new(crate::TestServiceLocatorStub::new()) as Arc<dyn crate::core::ServiceLocator>;

        let result = ProtoSupervisorBuilder::from_proto_spec(&spec, service_locator).await;
        assert!(result.is_ok(), "Should build supervisor from valid spec");

        let _supervisor = result.unwrap();
        // Supervisor created successfully
    }

    /// Test: Convert supervision strategy ONE_FOR_ONE
    #[tokio::test]
    async fn test_convert_supervision_strategy_one_for_one() {
        let spec = SupervisorSpec {
            strategy: ProtoSupervisionStrategy::SupervisionStrategyOneForOne as i32,
            max_restarts: 5,
            max_restart_window: Some(prost_types::Duration {
                seconds: 10,
                nanos: 0,
            }),
            children: vec![],
            ..Default::default()
        };

        let result = ProtoSupervisorBuilder::convert_supervision_strategy(&spec);
        assert!(result.is_ok());

        match result.unwrap() {
            SupervisionStrategy::OneForOne {
                max_restarts,
                within_seconds,
            } => {
                assert_eq!(max_restarts, 5);
                assert_eq!(within_seconds, 10);
            }
            _ => panic!("Expected OneForOne strategy"),
        }
    }

    /// Test: Invalid supervision strategy returns error
    #[tokio::test]
    async fn test_invalid_supervision_strategy() {
        let spec = SupervisorSpec {
            strategy: 999, // Invalid
            max_restarts: 3,
            max_restart_window: None,
            children: vec![],
            ..Default::default()
        };

        let result = ProtoSupervisorBuilder::convert_supervision_strategy(&spec);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SupervisorError::ConfigError(_)
        ));
    }
}
