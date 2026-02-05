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

use crate::{ChildType, RestartPolicy, SupervisionStrategy, Supervisor, SupervisorError};
use plexspaces_proto::application::v1::{
    ChildSpec as ProtoChildSpec, ChildType as ProtoChildType, RestartPolicy as ProtoRestartPolicy,
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
/// # async fn example(service_locator: Arc<dyn plexspaces_core::ServiceLocator>) -> Result<(), Box<dyn std::error::Error>> {
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
        service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
    ) -> Result<Arc<Supervisor>, SupervisorError> {
        // Convert proto supervision strategy to Rust enum
        let strategy = Self::convert_supervision_strategy(spec)?;

        // Create supervisor with strategy and service_locator
        let (supervisor, _event_rx) =
            Supervisor::new(format!("app-supervisor-{}", Ulid::new()), strategy, service_locator);

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
            Ok(ProtoSupervisionStrategy::SupervisionStrategyUnspecified) | Ok(ProtoSupervisionStrategy::SupervisionStrategyOneForOne) => {
                Ok(SupervisionStrategy::OneForOne {
                    max_restarts,
                    within_seconds,
                })
            }
            Ok(ProtoSupervisionStrategy::SupervisionStrategyOneForAll) => Ok(SupervisionStrategy::OneForAll {
                max_restarts,
                within_seconds,
            }),
            Ok(ProtoSupervisionStrategy::SupervisionStrategyRestForOne) => Ok(SupervisionStrategy::RestForOne {
                max_restarts,
                within_seconds,
            }),
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

        // Convert proto child type to Rust enum
        let child_type = Self::convert_child_type(child_spec)?;

        // Extract shutdown timeout
        let _shutdown_timeout = child_spec
            .shutdown_timeout
            .as_ref()
            .map(|d| Duration::from_secs(d.seconds as u64))
            .unwrap_or(Duration::from_secs(30));

        // TODO: Actually spawn the actor
        // For now, we just validate the spec
        // Future: Use ActorFactory to spawn actors

        // Validate child spec
        if child_spec.id.is_empty() {
            return Err(SupervisorError::ConfigError(
                "Child spec must have non-empty ID".to_string(),
            ));
        }

        // Log child spec for now (actual spawning in next phase)
        tracing::debug!(
            child_id = %child_spec.id,
            restart_policy = ?restart,
            child_type = ?child_type,
            "Validated child spec (spawning not yet implemented)"
        );

        Ok(())
    }

    /// Convert proto RestartPolicy to Rust RestartPolicy
    fn convert_restart_policy(
        child_spec: &ProtoChildSpec,
    ) -> Result<RestartPolicy, SupervisorError> {
        match ProtoRestartPolicy::try_from(child_spec.restart) {
            Ok(ProtoRestartPolicy::RestartPolicyUnspecified) | Ok(ProtoRestartPolicy::RestartPolicyPermanent) => {
                Ok(RestartPolicy::Permanent)
            }
            Ok(ProtoRestartPolicy::RestartPolicyTransient) => Ok(RestartPolicy::Transient),
            Ok(ProtoRestartPolicy::RestartPolicyTemporary) => Ok(RestartPolicy::Temporary),
            Err(_) => Err(SupervisorError::ConfigError(format!(
                "Unknown restart policy for child '{}': {}",
                child_spec.id, child_spec.restart
            ))),
        }
    }

    /// Convert proto ChildType to Rust ChildType
    fn convert_child_type(child_spec: &ProtoChildSpec) -> Result<ChildType, SupervisorError> {
        match ProtoChildType::try_from(child_spec.r#type) {
            Ok(ProtoChildType::ChildTypeUnspecified) | Ok(ProtoChildType::ChildTypeWorker) => Ok(ChildType::Worker),
            Ok(ProtoChildType::ChildTypeSupervisor) => Ok(ChildType::Supervisor),
            Err(_) => Err(SupervisorError::ConfigError(format!(
                "Unknown child type for child '{}': {}",
                child_spec.id, child_spec.r#type
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
    use plexspaces_proto::application::v1::{
        ChildSpec, ChildType, RestartPolicy, 
        SupervisionStrategy as ProtoSupervisionStrategy, 
        SupervisorSpec,
    };
    use plexspaces_services::ServiceLocatorImpl;
    use crate::SupervisionStrategy;

    /// Helper function to create a basic supervisor spec
    fn create_test_supervisor_spec() -> SupervisorSpec {
        SupervisorSpec {
            strategy: ProtoSupervisionStrategy::SupervisionStrategyOneForOne as i32,
            max_restarts: 3,
            max_restart_window: Some(prost_types::Duration {
                seconds: 5,
                nanos: 0,
            }),
            children: vec![
                ChildSpec {
                    id: "worker1".to_string(),
                    r#type: ChildType::ChildTypeWorker as i32,
                    args: std::collections::HashMap::new(),
                    restart: RestartPolicy::RestartPolicyPermanent as i32,
                    shutdown_timeout: Some(prost_types::Duration {
                        seconds: 30,
                        nanos: 0,
                    }),
                    supervisor: None,
                    facets: vec![],
                    behavior_kind: None,
                },
            ],
        }
    }

    /// Test: Build supervisor from proto spec
    #[tokio::test]
    async fn test_build_supervisor_from_proto() {
        let spec = create_test_supervisor_spec();
        let service_locator: Arc<dyn plexspaces_core::ServiceLocator> = Arc::new(ServiceLocatorImpl::new());

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
        };

        let result = ProtoSupervisorBuilder::convert_supervision_strategy(&spec);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SupervisorError::ConfigError(_)
        ));
    }
}
