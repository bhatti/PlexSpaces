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

use super::*;
use crate::behavior::MockBehavior;
use crate::core::ActorId;
use plexspaces_mailbox::{Mailbox, MailboxConfig};

fn test_actor_id(name: &str) -> ActorId {
    ActorId::new(name, "worker", "default", "localhost").expect("test actor id must be valid")
}

fn test_actor_id_string(name: &str) -> String {
    test_actor_id(name).to_string()
}

/// Helper function to create a ChildSpec for tests
/// Uses async factory pattern (ChildSpec standard)
#[allow(dead_code)]
async fn create_child_spec(
    id: String,
    restart: crate::child_spec::ProtoRestartPolicy,
) -> crate::ChildSpec {
    use crate::child_spec::{ChildSpec, StartedChild};
    use crate::ActorInstance;
    use std::sync::Arc;
    use std::time::Duration;

    let child_actor_id = test_actor_id(&id);
    let id_for_factory = id.clone();
    let start_fn: crate::child_spec::StartFn = Arc::new(move || {
        let child_name = id_for_factory.clone();
        Box::pin(async move {
            let actor_id = test_actor_id(&child_name);
            let mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
                .await
                .map_err(|e| crate::ActorError::InvalidState(e.to_string()))?;
            let actor = ActorInstance::new(
                actor_id.clone(),
                Box::new(MockBehavior::new()),
                mailbox,
                "test-tenant".to_string(),
                "test".to_string(),
                None,
            );
            let actor_mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
                .await
                .map_err(|e| crate::ActorError::InvalidState(e.to_string()))?;
            let service_locator: Arc<dyn crate::core::ServiceLocator> =
                Arc::new(crate::TestServiceLocatorStub::new());
            let actor_ref = crate::actor_ref::ActorRef::local(
                actor_id,
                "test-tenant".to_string(),
                "test".to_string(),
                Arc::new(actor_mailbox),
                service_locator,
                plexspaces_proto::actor::v1::ActorVisibility::ActorVisibilityPublic,
            );
            Ok(StartedChild::Worker { actor, actor_ref })
        })
    });

    ChildSpec::worker(child_actor_id, start_fn)
        .with_restart(restart)
        .with_shutdown(crate::child_spec::ShutdownSpec::Timeout(
            Duration::from_secs(5),
        ))
}

/// Helper function to create a test supervisor with ServiceLocator
async fn create_test_supervisor(
    id: String,
    strategy: SupervisionStrategy,
) -> (Supervisor, tokio::sync::mpsc::Receiver<SupervisorEvent>) {
    let service_locator: Arc<dyn crate::core::ServiceLocator> =
        Arc::new(crate::TestServiceLocatorStub::new());
    Supervisor::new(test_actor_id_string(&id), strategy, service_locator)
}

/// Helper function to create a ChildSpec with an async factory (wraps the sync actor creation).
fn create_child_spec_sync(id: String, restart: RestartPolicy) -> crate::ChildSpec {
    use crate::child_spec::{ChildSpec, ProtoRestartPolicy, StartedChild};
    use crate::ActorInstance;
    use std::sync::Arc;

    let child_actor_id = test_actor_id(&id);
    let id_for_factory = id.clone();
    let start_fn: crate::child_spec::StartFn = Arc::new(move || {
        let child_name = id_for_factory.clone();
        Box::pin(async move {
            let actor_id = test_actor_id(&child_name);
            let actor_mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
                .await
                .map_err(|e| crate::ActorError::InvalidState(e.to_string()))?;
            let ref_mailbox = Arc::new(
                Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
                    .await
                    .map_err(|e| crate::ActorError::InvalidState(e.to_string()))?,
            );
            let actor = ActorInstance::new(
                actor_id.clone(),
                Box::new(MockBehavior::new()),
                actor_mailbox,
                "test-tenant".to_string(),
                "test".to_string(),
                None,
            );
            let service_locator: Arc<dyn crate::core::ServiceLocator> =
                Arc::new(crate::TestServiceLocatorStub::new());
            let actor_ref = crate::actor_ref::ActorRef::local(
                actor_id,
                "test-tenant".to_string(),
                "test".to_string(),
                ref_mailbox,
                service_locator,
                plexspaces_proto::actor::v1::ActorVisibility::ActorVisibilityPublic,
            );
            Ok(StartedChild::Worker { actor, actor_ref })
        })
    });

    // Map supervisor RestartPolicy to proto RestartPolicy for ChildSpec
    let restart_policy = match restart {
        RestartPolicy::Permanent | RestartPolicy::ExponentialBackoff { .. } => {
            ProtoRestartPolicy::RestartPolicyPermanent
        }
        RestartPolicy::Transient => ProtoRestartPolicy::RestartPolicyTransient,
        RestartPolicy::Temporary => ProtoRestartPolicy::RestartPolicyTemporary,
    };

    ChildSpec::worker(child_actor_id, start_fn).with_restart(restart_policy)
}

#[tokio::test]
async fn test_supervisor_creation() {
    let (supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    )
    .await;

    // Add a child
    let spec = create_child_spec_sync("test-child".to_string(), RestartPolicy::Permanent);

    let actor_ref = supervisor.add_child(spec).await.unwrap();
    assert_eq!(actor_ref.id(), &test_actor_id("test-child"));

    // Check event
    if let Some(event) = event_rx.recv().await {
        assert_eq!(
            event.event_type,
            plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildStarted
                as i32
        );
        assert_eq!(
            ActorId::from(event.actor_id.clone()),
            test_actor_id("test-child")
        );
    }
}

#[test]
fn test_backoff_calculation() {
    use super::restart::calculate_backoff_delay;
    assert_eq!(calculate_backoff_delay(0, 100, 10000, 2.0), 100);
    assert_eq!(calculate_backoff_delay(1, 100, 10000, 2.0), 200);
    assert_eq!(calculate_backoff_delay(2, 100, 10000, 2.0), 400);
    assert_eq!(calculate_backoff_delay(10, 100, 10000, 2.0), 10000); // Capped at max
}

#[tokio::test]
async fn test_supervisor_builder() {
    let spec = create_child_spec_sync("worker-1".to_string(), RestartPolicy::Transient);
    let service_locator: Arc<dyn crate::core::ServiceLocator> =
        Arc::new(crate::TestServiceLocatorStub::new());
    let (_supervisor, _event_rx) = SupervisorBuilder::new(test_actor_id_string("root"))
        .with_strategy(SupervisionStrategy::OneForAll {
            max_restarts: 5,
            within_seconds: 30,
        })
        .add_child(spec)
        .build(service_locator)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_remove_child() {
    let (supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    )
    .await;

    // Add a child
    let spec = create_child_spec_sync("removable-child".to_string(), RestartPolicy::Permanent);

    supervisor.add_child(spec).await.unwrap();
    let _ = event_rx.recv().await; // Consume ChildStarted event

    // Remove the child
    supervisor
        .remove_child(&test_actor_id("removable-child"))
        .await
        .unwrap();

    // Check event
    if let Some(event) = event_rx.recv().await {
        assert_eq!(
            event.event_type,
            plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildStopped
                as i32
        );
        assert_eq!(
            ActorId::from(event.actor_id.clone()),
            test_actor_id("removable-child")
        );
    }
}

#[tokio::test]
async fn test_remove_nonexistent_child() {
    let (supervisor, _event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    )
    .await;

    // Try to remove a child that doesn't exist
    let result = supervisor.remove_child(&test_actor_id("nonexistent")).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        SupervisorError::ChildNotFound(_) => (),
        _ => panic!("Expected ChildNotFound error"),
    }
}

#[tokio::test]
async fn test_handle_failure_one_for_one() {
    let (supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    )
    .await;

    // Add a child
    let spec = create_child_spec_sync("failing-child".to_string(), RestartPolicy::Permanent);

    supervisor.add_child(spec).await.unwrap();
    let _ = event_rx.recv().await; // Consume ChildStarted

    // Handle failure
    supervisor
        .handle_failure(
            &test_actor_id("failing-child"),
            "test error".to_string(),
            None, // Will be parsed from reason string
        )
        .await
        .unwrap();

    // Check for ChildFailed event
    let event = event_rx.recv().await.unwrap();
    assert_eq!(
        event.event_type,
        plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildFailed as i32,
        "Expected ChildFailed event, got event_type={}",
        event.event_type
    );
    assert_eq!(
        ActorId::from(event.actor_id.clone()),
        test_actor_id("failing-child")
    );
    assert_eq!(event.reason, "test error");

    // Check for ChildRestarted event
    let event = event_rx.recv().await.unwrap();
    assert_eq!(
        event.event_type,
        plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildRestarted
            as i32,
        "Expected ChildRestarted event, got event_type={}",
        event.event_type
    );
    assert_eq!(
        ActorId::from(event.actor_id.clone()),
        test_actor_id("failing-child")
    );
    assert_eq!(event.restart_count, 1); // First restart
}

#[tokio::test]
async fn test_temporary_restart_policy() {
    let (supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    )
    .await;

    // Add a temporary child (should not restart)
    let spec = create_child_spec_sync("temp-child".to_string(), RestartPolicy::Temporary);

    supervisor.add_child(spec).await.unwrap();
    let _ = event_rx.recv().await; // Consume ChildStarted

    // Handle failure
    supervisor
        .handle_failure(
            &test_actor_id("temp-child"),
            "test error".to_string(),
            None, // Will be parsed from reason string
        )
        .await
        .unwrap();

    // Should get ChildFailed but NOT ChildRestarted
    let event = event_rx.recv().await.unwrap();
    assert_eq!(
        event.event_type,
        plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildFailed as i32,
        "Expected ChildFailed event"
    );

    // No restart event should follow for Temporary (try_recv should fail immediately)
    assert!(
        event_rx.try_recv().is_err(),
        "Should not restart temporary actor"
    );
}

#[tokio::test]
async fn test_max_restarts_exceeded() {
    let (supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 2, // Low limit to test
            within_seconds: 60,
        },
    )
    .await;

    let spec = create_child_spec_sync("crash-child".to_string(), RestartPolicy::Permanent);

    supervisor.add_child(spec).await.unwrap();
    let _ = event_rx.recv().await; // ChildStarted

    // Trigger failures until max restarts exceeded
    for i in 0..3 {
        let result = supervisor
            .handle_failure(&test_actor_id("crash-child"), format!("crash {}", i), None)
            .await;

        let _ = event_rx.recv().await; // ChildFailed

        if i < 2 {
            // Should succeed
            assert!(result.is_ok());
            let _ = event_rx.recv().await; // ChildRestarted
        } else {
            // Third failure should exceed limit
            assert!(result.is_err());
            match result.unwrap_err() {
                SupervisorError::MaxRestartsExceeded => (),
                e => panic!("Expected MaxRestartsExceeded, got {:?}", e),
            }

            // Should get MaxRestartsExceeded event
            let event = event_rx.recv().await.unwrap();
            assert_eq!(
                event.event_type,
                plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventMaxRestartsExceeded as i32,
                "Expected MaxRestartsExceeded event"
            );
            assert_eq!(
                ActorId::from(event.actor_id.clone()),
                test_actor_id("crash-child")
            );
        }
    }
}

#[tokio::test]
async fn test_supervisor_stats() {
    let (supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 5,
            within_seconds: 60,
        },
    )
    .await;

    let spec = create_child_spec_sync("stats-child".to_string(), RestartPolicy::Permanent);

    supervisor.add_child(spec).await.unwrap();
    let _ = event_rx.recv().await; // ChildStarted

    // Trigger a failure and restart
    supervisor
        .handle_failure(
            &test_actor_id("stats-child"),
            "test error".to_string(),
            None, // Will be parsed from reason string
        )
        .await
        .unwrap();

    let _ = event_rx.recv().await; // ChildFailed
    let _ = event_rx.recv().await; // ChildRestarted

    // Check stats
    let stats = supervisor.stats().await;
    assert_eq!(stats.total_restarts, 1);
    assert_eq!(stats.successful_restarts, 1);
    assert_eq!(stats.failed_restarts, 0);
    assert_eq!(stats.failure_patterns.get("test error"), Some(&1));
}

#[tokio::test]
async fn test_shutdown() {
    let (mut supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    )
    .await;

    // Add multiple children (reduced to 2 for faster tests)
    for i in 0..2 {
        let id = format!("child-{}", i);
        let spec = create_child_spec_sync(id.clone(), RestartPolicy::Permanent);

        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // ChildStarted
    }

    // Shutdown all children
    supervisor.shutdown().await.unwrap();

    // Should get ChildStopped for all 2 children
    for _ in 0..2 {
        let event = event_rx.recv().await.unwrap();
        assert_eq!(
            event.event_type,
            plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildStopped
                as i32,
            "Expected ChildStopped event"
        );
    }
}

#[test]
fn test_supervision_strategy_serialization() {
    // Test OneForOne
    let strategy = SupervisionStrategy::OneForOne {
        max_restarts: 3,
        within_seconds: 60,
    };
    let json = serde_json::to_string(&strategy).unwrap();
    let deserialized: SupervisionStrategy = serde_json::from_str(&json).unwrap();
    match deserialized {
        SupervisionStrategy::OneForOne {
            max_restarts,
            within_seconds,
        } => {
            assert_eq!(max_restarts, 3);
            assert_eq!(within_seconds, 60);
        }
        _ => panic!("Wrong strategy type"),
    }

    // Test OneForAll
    let strategy = SupervisionStrategy::OneForAll {
        max_restarts: 5,
        within_seconds: 30,
    };
    let json = serde_json::to_string(&strategy).unwrap();
    let _: SupervisionStrategy = serde_json::from_str(&json).unwrap();

    // Test RestForOne
    let strategy = SupervisionStrategy::RestForOne {
        max_restarts: 2,
        within_seconds: 120,
    };
    let json = serde_json::to_string(&strategy).unwrap();
    let _: SupervisionStrategy = serde_json::from_str(&json).unwrap();
}

#[test]
fn test_restart_policy_serialization() {
    // Test all restart policy variants
    let policies = vec![
        RestartPolicy::Permanent,
        RestartPolicy::Transient,
        RestartPolicy::Temporary,
        RestartPolicy::ExponentialBackoff {
            initial_delay_ms: 100,
            max_delay_ms: 10000,
            factor: 2.0,
        },
    ];

    for policy in policies {
        let json = serde_json::to_string(&policy).unwrap();
        let _: RestartPolicy = serde_json::from_str(&json).unwrap();
    }
}

#[tokio::test]
async fn test_one_for_all_strategy() {
    let (supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForAll {
            max_restarts: 3,
            within_seconds: 60,
        },
    )
    .await;

    // Add 3 children
    for i in 0..3 {
        let id = format!("child-{}", i);
        let spec = create_child_spec_sync(id.clone(), RestartPolicy::Permanent);
        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // Consume ChildStarted
    }

    // Trigger failure on child-1
    supervisor
        .handle_failure(&test_actor_id("child-1"), "test error".to_string(), None)
        .await
        .unwrap();

    // Should get ChildFailed for child-1
    let event = event_rx.recv().await.unwrap();
    assert_eq!(
        event.event_type,
        plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildFailed as i32,
        "Expected ChildFailed event"
    );
    assert_eq!(
        ActorId::from(event.actor_id.clone()),
        test_actor_id("child-1")
    );

    // OneForAll should restart ALL children (0, 1, 2)
    // Collect all restart events
    let mut restarted_ids = Vec::new();
    for _ in 0..3 {
        let event = event_rx.recv().await.unwrap();
        assert_eq!(
            event.event_type,
            plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildRestarted
                as i32,
            "Expected ChildRestarted event"
        );
        restarted_ids.push(ActorId::from(event.actor_id.clone()));
    }

    // All 3 children should be restarted
    assert_eq!(restarted_ids.len(), 3);
    assert!(restarted_ids.contains(&test_actor_id("child-0")));
    assert!(restarted_ids.contains(&test_actor_id("child-1")));
    assert!(restarted_ids.contains(&test_actor_id("child-2")));
}

#[tokio::test]
async fn test_rest_for_one_strategy() {
    let (supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::RestForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    )
    .await;

    // Add 3 children
    for i in 0..3 {
        let id = format!("child-{}", i);
        let spec = create_child_spec_sync(id.clone(), RestartPolicy::Permanent);
        supervisor.add_child(spec).await.unwrap();
        let _ = event_rx.recv().await; // Consume ChildStarted
    }

    // Trigger failure on child-1
    supervisor
        .handle_failure(&test_actor_id("child-1"), "test error".to_string(), None)
        .await
        .unwrap();

    // Should get ChildFailed for child-1
    let event = event_rx.recv().await.unwrap();
    assert_eq!(
        event.event_type,
        plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildFailed as i32,
        "Expected ChildFailed event"
    );
    assert_eq!(
        ActorId::from(event.actor_id.clone()),
        test_actor_id("child-1")
    );

    // RestForOne should restart the failed child (for now, until we implement child ordering)
    // TODO: When child ordering (Vec) is implemented, this will restart child-1 and all after it
    let event = event_rx.recv().await.unwrap();
    assert_eq!(
        event.event_type,
        plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildRestarted
            as i32,
        "Expected ChildRestarted event"
    );
    assert_eq!(
        ActorId::from(event.actor_id.clone()),
        test_actor_id("child-1")
    );
    assert_eq!(event.restart_count, 1);
}

#[tokio::test]
async fn test_adaptive_strategy() {
    let (supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::Adaptive {
            initial_strategy: Box::new(SupervisionStrategy::OneForOne {
                max_restarts: 5,
                within_seconds: 60,
            }),
            learning_rate: 0.1,
        },
    )
    .await;

    // Add a child
    let spec = create_child_spec_sync("adaptive-child".to_string(), RestartPolicy::Permanent);

    supervisor.add_child(spec).await.unwrap();
    let _ = event_rx.recv().await; // Consume ChildStarted

    // Trigger failure with adaptive strategy
    supervisor
        .handle_failure(
            &test_actor_id("adaptive-child"),
            "test error".to_string(),
            None, // Will be parsed from reason string
        )
        .await
        .unwrap();

    // Should get ChildFailed
    let event = event_rx.recv().await.unwrap();
    assert_eq!(
        event.event_type,
        plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildFailed as i32,
        "Expected ChildFailed event"
    );

    // Should get ChildRestarted (adaptive strategy uses initial OneForOne)
    let event = event_rx.recv().await.unwrap();
    assert_eq!(
        event.event_type,
        plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildRestarted
            as i32,
        "Expected ChildRestarted event"
    );
    assert_eq!(
        ActorId::from(event.actor_id.clone()),
        test_actor_id("adaptive-child")
    );
    assert_eq!(event.restart_count, 1);
}

#[tokio::test]
async fn test_transient_restart_normal_exit() {
    // TODO: Implement when we can distinguish normal vs abnormal exit
    // For now, transient policy restarts on all failures
}

// Note: ChildSpec uses proto RestartPolicy directly (Permanent, Transient, Temporary).
// ExponentialBackoff is only in Supervisor's own RestartPolicy (for supervisor-level behavior).

#[tokio::test]
async fn test_with_parent_supervisor() {
    let (parent, _parent_rx) = create_test_supervisor(
        "parent-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    )
    .await;

    let parent = Arc::new(parent);

    let (mut child_supervisor, _child_rx) = create_test_supervisor(
        "child-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    )
    .await;

    child_supervisor = child_supervisor.with_parent(parent.clone());

    // Verify parent is set (we can't directly access it due to privacy, but test compiles)
    let strategy = child_supervisor.strategy.read().await.clone();
    match strategy {
        SupervisionStrategy::OneForOne {
            max_restarts,
            within_seconds,
        } => {
            assert_eq!(max_restarts, 3);
            assert_eq!(within_seconds, 60);
        }
        _ => panic!("Wrong strategy type"),
    }
}

#[tokio::test]
async fn test_restart_intensity_window_reset() {
    use std::time::Duration;

    let (supervisor, mut event_rx) = create_test_supervisor(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 2,
            within_seconds: 1, // 1 second window
        },
    )
    .await;

    let spec = create_child_spec_sync("window-child".to_string(), RestartPolicy::Permanent);

    supervisor.add_child(spec).await.unwrap();
    let _ = event_rx.recv().await; // ChildStarted

    // First restart
    supervisor
        .handle_failure(&test_actor_id("window-child"), "error 1".to_string(), None)
        .await
        .unwrap();
    let _ = event_rx.recv().await; // ChildFailed
    let _ = event_rx.recv().await; // ChildRestarted

    // Second restart (within window)
    supervisor
        .handle_failure(&test_actor_id("window-child"), "error 2".to_string(), None)
        .await
        .unwrap();
    let _ = event_rx.recv().await; // ChildFailed
    let _ = event_rx.recv().await; // ChildRestarted

    // Wait for window to expire
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Third restart (outside window - should reset counter)
    let result = supervisor
        .handle_failure(&test_actor_id("window-child"), "error 3".to_string(), None)
        .await;

    // Should succeed because counter was reset
    assert!(
        result.is_ok(),
        "Expected restart to succeed after window reset"
    );
    let _ = event_rx.recv().await; // ChildFailed
    let _ = event_rx.recv().await; // ChildRestarted
}

// ========================================================================
// Tests for extract_namespace_from_actor_id helper
// ========================================================================

/// Extract namespace from an actor ID string.
///
/// Actor IDs follow the format `name:namespace@node`. This function extracts
/// the namespace portion between the colon and the at-sign. Returns an empty
/// string if the ID does not contain both delimiters in the correct order.
///
/// ## Examples
/// - `"worker-0:my-ns@node1"` -> `"my-ns"`
/// - `"worker-0@node1"` -> `""`
/// - `"worker-0:my-ns"` -> `""`
fn extract_namespace_from_actor_id(actor_id: &str) -> String {
    if let Some(colon_pos) = actor_id.find(':') {
        if let Some(at_pos) = actor_id.find('@') {
            if colon_pos < at_pos {
                return actor_id[colon_pos + 1..at_pos].to_string();
            }
        }
    }
    String::new()
}

#[test]
fn test_extract_namespace_with_namespace() {
    assert_eq!(
        extract_namespace_from_actor_id("worker-0:my-ns@node1"),
        "my-ns"
    );
}

#[test]
fn test_extract_namespace_without_namespace() {
    assert_eq!(extract_namespace_from_actor_id("worker-0@node1"), "");
}

#[test]
fn test_extract_namespace_no_node() {
    assert_eq!(extract_namespace_from_actor_id("worker-0:my-ns"), "");
}

#[test]
fn test_extract_namespace_empty() {
    assert_eq!(extract_namespace_from_actor_id(""), "");
}

#[test]
fn test_extract_namespace_colon_after_at() {
    assert_eq!(extract_namespace_from_actor_id("worker-0@node1:port"), "");
}

#[test]
fn test_extract_namespace_complex() {
    assert_eq!(
        extract_namespace_from_actor_id("data-worker-3:ray-ps@test-node"),
        "ray-ps"
    );
}
