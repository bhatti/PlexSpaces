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

//! # Supervisor Builder
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
//! - Uses `plexspaces.node.v1.SupervisorSpec` (application.proto) not supervision.proto
//! - Why: Applications spawn their own actors, not supervise existing ones
//! - Local supervision only (follows user requirement for simplicity)
//! - Converts proto enums to Rust enums for type safety

use super::ApplicationError;
use plexspaces_proto::application::v1::{
    ChildSpec as ProtoChildSpec, ChildType as ProtoChildType, RestartPolicy as ProtoRestartPolicy,
    SupervisionStrategy as ProtoSupervisionStrategy, SupervisorSpec as ProtoSupervisorSpec,
};
use plexspaces_supervisor::{ChildType, RestartPolicy, SupervisionStrategy, Supervisor};
use std::sync::Arc;
use std::time::Duration;
use ulid::Ulid;

/// Supervisor builder for application-embedded supervision trees
///
/// ## Purpose
/// Converts proto SupervisorSpec to runtime Supervisor instance.
///
/// ## Examples
/// ```rust,no_run
/// use plexspaces::application::SupervisorBuilder;
/// use plexspaces_proto::node::v1::{SupervisorSpec, SupervisionStrategy};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let proto_spec = SupervisorSpec {
///     strategy: SupervisionStrategy::SupervisionStrategyOneForOne as i32,
///     max_restarts: 3,
///     max_restart_window: None,
///     children: vec![],
/// };
/// let supervisor = SupervisorBuilder::from_proto_spec(&proto_spec)
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct SupervisorBuilder;

impl SupervisorBuilder {
    /// Build supervisor from proto SupervisorSpec
    ///
    /// ## Arguments
    /// * `spec` - Proto supervisor specification from application.proto
    ///
    /// ## Returns
    /// Configured Supervisor instance ready to start
    ///
    /// ## Errors
    /// - `ApplicationError::InvalidConfig` if spec is invalid
    ///
    /// ## Design Notes
    /// - Uses application.proto's SupervisorSpec (has start_module)
    /// - Not supervision.proto's SupervisorSpec (has actor_id)
    /// - Local supervision only (no remote actor spawning yet)
    pub async fn from_proto_spec(
        spec: &ProtoSupervisorSpec,
    ) -> Result<Arc<Supervisor>, ApplicationError> {
        // Convert proto supervision strategy to Rust enum
        let strategy = Self::convert_supervision_strategy(spec)?;

        // Create supervisor with strategy
        let (supervisor, _event_rx) =
            Supervisor::new(format!("app-supervisor-{}", Ulid::new()), strategy);

        // Add children from spec
        for child_spec in &spec.children {
            Self::add_child_to_supervisor(&supervisor, child_spec).await?;
        }

        Ok(Arc::new(supervisor))
    }

    /// Convert proto SupervisionStrategy to Rust SupervisionStrategy
    fn convert_supervision_strategy(
        spec: &ProtoSupervisorSpec,
    ) -> Result<SupervisionStrategy, ApplicationError> {
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
            Err(_) => Err(ApplicationError::InvalidConfig(format!(
                "Unknown supervision strategy: {}",
                spec.strategy
            ))),
        }
    }

    /// Add child to supervisor from proto ChildSpec
    async fn add_child_to_supervisor(
        _supervisor: &Supervisor,
        child_spec: &ProtoChildSpec,
    ) -> Result<(), ApplicationError> {
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

        // TODO: Actually spawn the actor based on start_module
        // For now, we just validate the spec
        // Future: Use ActorFactory to spawn from start_module string

        // Validate child spec
        if child_spec.id.is_empty() {
            return Err(ApplicationError::InvalidConfig(
                "Child spec must have non-empty ID".to_string(),
            ));
        }

        if child_spec.start_module.is_empty() {
            return Err(ApplicationError::InvalidConfig(format!(
                "Child '{}' must have start_module",
                child_spec.id
            )));
        }

        // Log child spec for now (actual spawning in next phase)
        tracing::debug!(
            child_id = %child_spec.id,
            start_module = %child_spec.start_module,
            restart_policy = ?restart,
            child_type = ?child_type,
            "Validated child spec (spawning not yet implemented)"
        );

        Ok(())
    }

    /// Convert proto RestartPolicy to Rust RestartPolicy
    fn convert_restart_policy(
        child_spec: &ProtoChildSpec,
    ) -> Result<RestartPolicy, ApplicationError> {
        match ProtoRestartPolicy::try_from(child_spec.restart) {
            Ok(ProtoRestartPolicy::RestartPolicyUnspecified) | Ok(ProtoRestartPolicy::RestartPolicyPermanent) => {
                Ok(RestartPolicy::Permanent)
            }
            Ok(ProtoRestartPolicy::RestartPolicyTransient) => Ok(RestartPolicy::Transient),
            Ok(ProtoRestartPolicy::RestartPolicyTemporary) => Ok(RestartPolicy::Temporary),
            Err(_) => Err(ApplicationError::InvalidConfig(format!(
                "Unknown restart policy for child '{}': {}",
                child_spec.id, child_spec.restart
            ))),
        }
    }

    /// Convert proto ChildType to Rust ChildType
    fn convert_child_type(child_spec: &ProtoChildSpec) -> Result<ChildType, ApplicationError> {
        match ProtoChildType::try_from(child_spec.r#type) {
            Ok(ProtoChildType::ChildTypeUnspecified) | Ok(ProtoChildType::ChildTypeWorker) => Ok(ChildType::Worker),
            Ok(ProtoChildType::ChildTypeSupervisor) => Ok(ChildType::Supervisor),
            Err(_) => Err(ApplicationError::InvalidConfig(format!(
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
        ChildSpec, ChildType, RestartPolicy, SupervisionStrategy, SupervisorSpec,
    };
    use plexspaces_proto::prost_types::Duration as ProtoDuration;

    /// Create test proto SupervisorSpec
    fn create_test_supervisor_spec() -> SupervisorSpec {
        SupervisorSpec {
            strategy: ProtoSupervisionStrategy::SupervisionStrategyOneForOne as i32,
            max_restarts: 3,
            max_restart_window: Some(ProtoDuration {
                seconds: 5,
                nanos: 0,
            }),
            children: vec![
                ChildSpec {
                    id: "worker1".to_string(),
                    r#type: ProtoChildType::ChildTypeWorker as i32,
                    start_module: "test::Worker".to_string(),
                    args: Default::default(),
                    restart: ProtoRestartPolicy::RestartPolicyPermanent as i32,
                    shutdown_timeout: Some(ProtoDuration {
                        seconds: 10,
                        nanos: 0,
                    }),
                    supervisor: None,
                },
                ChildSpec {
                    id: "worker2".to_string(),
                    r#type: ProtoChildType::ChildTypeWorker as i32,
                    start_module: "test::Worker".to_string(),
                    args: Default::default(),
                    restart: ProtoRestartPolicy::RestartPolicyTransient as i32,
                    shutdown_timeout: Some(ProtoDuration {
                        seconds: 5,
                        nanos: 0,
                    }),
                    supervisor: None,
                },
            ],
        }
    }

    /// Test: Build supervisor from proto spec
    #[tokio::test]
    async fn test_build_supervisor_from_proto() {
        let spec = create_test_supervisor_spec();

        let result = SupervisorBuilder::from_proto_spec(&spec).await;
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
            max_restart_window: Some(ProtoDuration {
                seconds: 10,
                nanos: 0,
            }),
            children: vec![],
        };

        let strategy = SupervisorBuilder::convert_supervision_strategy(&spec);
        assert!(strategy.is_ok());

        match strategy.unwrap() {
            plexspaces_supervisor::SupervisionStrategy::OneForOne {
                max_restarts,
                within_seconds,
            } => {
                assert_eq!(max_restarts, 5);
                assert_eq!(within_seconds, 10);
            }
            _ => panic!("Expected OneForOne strategy"),
        }
    }

    /// Test: Convert supervision strategy ONE_FOR_ALL
    #[tokio::test]
    async fn test_convert_supervision_strategy_one_for_all() {
        let spec = SupervisorSpec {
            strategy: ProtoSupervisionStrategy::SupervisionStrategyOneForAll as i32,
            max_restarts: 3,
            max_restart_window: Some(ProtoDuration {
                seconds: 5,
                nanos: 0,
            }),
            children: vec![],
        };

        let strategy = SupervisorBuilder::convert_supervision_strategy(&spec);
        assert!(strategy.is_ok());

        match strategy.unwrap() {
            plexspaces_supervisor::SupervisionStrategy::OneForAll {
                max_restarts,
                within_seconds,
            } => {
                assert_eq!(max_restarts, 3);
                assert_eq!(within_seconds, 5);
            }
            _ => panic!("Expected OneForAll strategy"),
        }
    }

    /// Test: Convert supervision strategy REST_FOR_ONE
    #[tokio::test]
    async fn test_convert_supervision_strategy_rest_for_one() {
        let spec = SupervisorSpec {
            strategy: ProtoSupervisionStrategy::SupervisionStrategyRestForOne as i32,
            max_restarts: 2,
            max_restart_window: Some(ProtoDuration {
                seconds: 3,
                nanos: 0,
            }),
            children: vec![],
        };

        let strategy = SupervisorBuilder::convert_supervision_strategy(&spec);
        assert!(strategy.is_ok());

        match strategy.unwrap() {
            plexspaces_supervisor::SupervisionStrategy::RestForOne {
                max_restarts,
                within_seconds,
            } => {
                assert_eq!(max_restarts, 2);
                assert_eq!(within_seconds, 3);
            }
            _ => panic!("Expected RestForOne strategy"),
        }
    }

    /// Test: Convert restart policy PERMANENT
    #[tokio::test]
    async fn test_convert_restart_policy_permanent() {
        let child_spec = ChildSpec {
            id: "test".to_string(),
            r#type: ProtoChildType::ChildTypeWorker as i32,
            start_module: "test::Worker".to_string(),
            args: Default::default(),
            restart: ProtoRestartPolicy::RestartPolicyPermanent as i32,
            shutdown_timeout: None,
            supervisor: None,
        };

        let policy = SupervisorBuilder::convert_restart_policy(&child_spec);
        assert!(policy.is_ok());
        assert!(matches!(
            policy.unwrap(),
            plexspaces_supervisor::RestartPolicy::Permanent
        ));
    }

    /// Test: Convert restart policy TRANSIENT
    #[tokio::test]
    async fn test_convert_restart_policy_transient() {
        let child_spec = ChildSpec {
            id: "test".to_string(),
            r#type: ProtoChildType::ChildTypeWorker as i32,
            start_module: "test::Worker".to_string(),
            args: Default::default(),
            restart: ProtoRestartPolicy::RestartPolicyTransient as i32,
            shutdown_timeout: None,
            supervisor: None,
        };

        let policy = SupervisorBuilder::convert_restart_policy(&child_spec);
        assert!(policy.is_ok());
        assert!(matches!(
            policy.unwrap(),
            plexspaces_supervisor::RestartPolicy::Transient
        ));
    }

    /// Test: Convert restart policy TEMPORARY
    #[tokio::test]
    async fn test_convert_restart_policy_temporary() {
        let child_spec = ChildSpec {
            id: "test".to_string(),
            r#type: ProtoChildType::ChildTypeWorker as i32,
            start_module: "test::Worker".to_string(),
            args: Default::default(),
            restart: ProtoRestartPolicy::RestartPolicyTemporary as i32,
            shutdown_timeout: None,
            supervisor: None,
        };

        let policy = SupervisorBuilder::convert_restart_policy(&child_spec);
        assert!(policy.is_ok());
        assert!(matches!(
            policy.unwrap(),
            plexspaces_supervisor::RestartPolicy::Temporary
        ));
    }

    /// Test: Convert child type WORKER
    #[tokio::test]
    async fn test_convert_child_type_worker() {
        let child_spec = ChildSpec {
            id: "test".to_string(),
            r#type: ProtoChildType::ChildTypeWorker as i32,
            start_module: "test::Worker".to_string(),
            args: Default::default(),
            restart: ProtoRestartPolicy::RestartPolicyPermanent as i32,
            shutdown_timeout: None,
            supervisor: None,
        };

        let child_type = SupervisorBuilder::convert_child_type(&child_spec);
        assert!(child_type.is_ok());
        assert!(matches!(
            child_type.unwrap(),
            plexspaces_supervisor::ChildType::Worker
        ));
    }

    /// Test: Convert child type SUPERVISOR
    #[tokio::test]
    async fn test_convert_child_type_supervisor() {
        let child_spec = ChildSpec {
            id: "test".to_string(),
            r#type: ProtoChildType::ChildTypeSupervisor as i32,
            start_module: "test::Supervisor".to_string(),
            args: Default::default(),
            restart: ProtoRestartPolicy::RestartPolicyPermanent as i32,
            shutdown_timeout: None,
            supervisor: None,
        };

        let child_type = SupervisorBuilder::convert_child_type(&child_spec);
        assert!(child_type.is_ok());
        assert!(matches!(
            child_type.unwrap(),
            plexspaces_supervisor::ChildType::Supervisor
        ));
    }

    /// Test: Validate child spec with empty ID fails
    #[tokio::test]
    async fn test_validate_child_spec_empty_id() {
        let (supervisor, _event_rx) = Supervisor::new(
            "test-supervisor".to_string(),
            plexspaces_supervisor::SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 5,
            },
        );

        let child_spec = ChildSpec {
            id: "".to_string(), // Empty ID
            r#type: ProtoChildType::ChildTypeWorker as i32,
            start_module: "test::Worker".to_string(),
            args: Default::default(),
            restart: ProtoRestartPolicy::RestartPolicyPermanent as i32,
            shutdown_timeout: None,
            supervisor: None,
        };

        let result = SupervisorBuilder::add_child_to_supervisor(&supervisor, &child_spec).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ApplicationError::InvalidConfig(_)
        ));
    }

    /// Test: Validate child spec with empty start_module fails
    #[tokio::test]
    async fn test_validate_child_spec_empty_start_module() {
        let (supervisor, _event_rx) = Supervisor::new(
            "test-supervisor".to_string(),
            plexspaces_supervisor::SupervisionStrategy::OneForOne {
                max_restarts: 3,
                within_seconds: 5,
            },
        );

        let child_spec = ChildSpec {
            id: "worker1".to_string(),
            r#type: ProtoChildType::ChildTypeWorker as i32,
            start_module: "".to_string(), // Empty module
            args: Default::default(),
            restart: ProtoRestartPolicy::RestartPolicyPermanent as i32,
            shutdown_timeout: None,
            supervisor: None,
        };

        let result = SupervisorBuilder::add_child_to_supervisor(&supervisor, &child_spec).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ApplicationError::InvalidConfig(_)
        ));
    }
}
