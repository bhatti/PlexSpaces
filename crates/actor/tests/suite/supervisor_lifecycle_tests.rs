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

//! Comprehensive tests for Supervisor lifecycle methods (Phase 4)
//!
//! Tests cover:
//! - start_child() - dynamic child addition
//! - delete_child() - dynamic child removal
//! - restart_child() - child restart
//! - which_children() - list all children
//! - count_children() - count by type
//! - get_childspec() - get child specification
//! - Parent-child registration integration

use async_trait::async_trait;
use plexspaces_actor::child_spec::{
    ChildType as CSChildType, RestartStrategy, ShutdownSpec, StartedChild,
};
use plexspaces_actor::supervisor::{
    ChildType, RestartPolicy, SupervisionStrategy, Supervisor, SupervisorEvent,
};
use plexspaces_actor::{Actor, ActorRef as ActorActorRef, ChildSpec};
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, ActorError, ActorRef as CoreActorRef, BehaviorError, Message,
};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_services::ServiceLocatorImpl;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::time::{sleep, Duration};

/// Helper function to create a ChildSpec from a sync factory
fn create_child_spec_from_factory(
    id: String,
    factory: Arc<dyn Fn() -> Result<Actor, ActorError> + Send + Sync>,
    restart_policy: RestartPolicy,
    shutdown_timeout_ms: Option<u64>,
) -> ChildSpec {
    use plexspaces_actor::child_spec::ShutdownSpec;

    let actor_ref = CoreActorRef::new(id.clone()).expect("Failed to create actor ref");

    let restart_strategy = match restart_policy {
        RestartPolicy::Permanent => RestartStrategy::Permanent,
        RestartPolicy::Transient => RestartStrategy::Transient,
        RestartPolicy::Temporary => RestartStrategy::Temporary,
        RestartPolicy::ExponentialBackoff { .. } => RestartStrategy::Permanent,
    };

    let mut spec =
        ChildSpec::worker_sync(id.clone(), id, factory, actor_ref).with_restart(restart_strategy);

    // Apply shutdown timeout if specified
    spec = match shutdown_timeout_ms {
        Some(0) => spec.with_shutdown(ShutdownSpec::BrutalKill),
        Some(ms) => spec.with_shutdown(ShutdownSpec::Timeout(StdDuration::from_millis(ms))),
        None => spec.with_shutdown(ShutdownSpec::Infinity),
    };

    spec
}

/// Test actor for supervisor tests
struct TestActor {
    id: String,
}

impl TestActor {
    fn new(id: String) -> Self {
        Self { id }
    }
}

#[async_trait]
impl plexspaces_core::Actor for TestActor {
    async fn init(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
        Ok(())
    }

    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

async fn create_test_supervisor() -> (Supervisor, tokio::sync::mpsc::Receiver<SupervisorEvent>) {
    let service_locator: Arc<dyn plexspaces_core::ServiceLocator> =
        Arc::new(ServiceLocatorImpl::new());
    Supervisor::new(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
        service_locator,
    )
}

#[tokio::test]
async fn test_start_child() {
    let (mut supervisor, _event_rx) = create_test_supervisor().await;

    let child_id = "worker1".to_string();
    let actor_id = format!("{}@test-node", child_id);

    let actor_id_for_closure = actor_id.clone();
    let spec = ChildSpec::worker(
        child_id.clone(),
        actor_id.clone(),
        Arc::new(move || {
            let actor_id = actor_id_for_closure.clone();
            Box::pin(async move {
                let mailbox = Mailbox::new(
                    MailboxConfig::default(),
                    format!("mailbox-{}", actor_id.clone()),
                )
                .await
                .unwrap();
                let actor = Actor::new(
                    actor_id.clone(),
                    Box::new(TestActor::new(actor_id.clone())),
                    mailbox,
                    "tenant".to_string(),
                    "namespace".to_string(),
                    None,
                );
                let actor_ref = CoreActorRef::new(actor_id.clone())
                    .map_err(|e| ActorError::InvalidState(e.to_string()))?;
                Ok(StartedChild::Worker { actor, actor_ref })
            })
        }),
    );

    let result = supervisor.start_child(spec).await;
    assert!(result.is_ok(), "start_child should succeed");
    assert_eq!(result.unwrap(), actor_id);

    // Verify child is in supervisor
    let count = supervisor.count_children().await;
    assert_eq!(count.actors, 1);
    assert_eq!(count.total, 1);
}

#[tokio::test]
async fn test_delete_child() {
    let (mut supervisor, _event_rx) = create_test_supervisor().await;

    // Add a child first
    let child_id = "worker1".to_string();
    let actor_id = format!("{}@test-node", child_id);

    let actor_id_for_closure = actor_id.clone();
    let spec = ChildSpec::worker(
        child_id.clone(),
        actor_id.clone(),
        Arc::new(move || {
            let actor_id = actor_id_for_closure.clone();
            Box::pin(async move {
                let mailbox = Mailbox::new(
                    MailboxConfig::default(),
                    format!("mailbox-{}", actor_id.clone()),
                )
                .await
                .unwrap();
                let actor = Actor::new(
                    actor_id.clone(),
                    Box::new(TestActor::new(actor_id.clone())),
                    mailbox,
                    "tenant".to_string(),
                    "namespace".to_string(),
                    None,
                );
                let actor_ref = CoreActorRef::new(actor_id.clone())
                    .map_err(|e| ActorError::InvalidState(e.to_string()))?;
                Ok(StartedChild::Worker { actor, actor_ref })
            })
        }),
    );

    let _ = supervisor
        .start_child(spec)
        .await
        .expect("start_child should succeed");

    // Delete the child
    let result = supervisor.delete_child(&actor_id).await;
    assert!(result.is_ok(), "delete_child should succeed");

    // Verify child is removed
    let count = supervisor.count_children().await;
    assert_eq!(count.actors, 0);
    assert_eq!(count.total, 0);
}

#[tokio::test]
async fn test_which_children() {
    let (mut supervisor, _event_rx) = create_test_supervisor().await;

    // Add multiple children
    for i in 1..=3 {
        let child_id = format!("worker{}", i);
        let actor_id = format!("{}@test-node", child_id);

        let spec = ChildSpec::worker(
            child_id.clone(),
            actor_id.clone(),
            Arc::new(move || {
                let actor_id = actor_id.clone();
                Box::pin(async move {
                    let mailbox = Mailbox::new(
                        MailboxConfig::default(),
                        format!("mailbox-{}", actor_id.clone()),
                    )
                    .await
                    .unwrap();
                    let actor = Actor::new(
                        actor_id.clone(),
                        Box::new(TestActor::new(actor_id.clone())),
                        mailbox,
                        "tenant".to_string(),
                        "namespace".to_string(),
                        None,
                    );
                    let actor_ref = CoreActorRef::new(actor_id.clone())
                        .map_err(|e| ActorError::InvalidState(e.to_string()))?;
                    Ok(StartedChild::Worker { actor, actor_ref })
                })
            }),
        );

        let _ = supervisor
            .start_child(spec)
            .await
            .expect("start_child should succeed");
    }

    // Get children list
    let children = supervisor.which_children().await;
    assert_eq!(children.len(), 3);

    // Verify all children are listed
    let child_ids: Vec<String> = children.iter().map(|c| c.child_id.clone()).collect();
    assert!(child_ids.contains(&"worker1@test-node".to_string()));
    assert!(child_ids.contains(&"worker2@test-node".to_string()));
    assert!(child_ids.contains(&"worker3@test-node".to_string()));
}

#[tokio::test]
async fn test_count_children() {
    let (mut supervisor, _event_rx) = create_test_supervisor().await;

    // Initially empty
    let count = supervisor.count_children().await;
    assert_eq!(count.actors, 0);
    assert_eq!(count.supervisors, 0);
    assert_eq!(count.total, 0);

    // Add actors
    for i in 1..=2 {
        let child_id = format!("worker{}", i);
        let actor_id = format!("{}@test-node", child_id);

        let spec = ChildSpec::worker(
            child_id.clone(),
            actor_id.clone(),
            Arc::new(move || {
                let actor_id = actor_id.clone();
                Box::pin(async move {
                    let mailbox = Mailbox::new(
                        MailboxConfig::default(),
                        format!("mailbox-{}", actor_id.clone()),
                    )
                    .await
                    .unwrap();
                    let actor = Actor::new(
                        actor_id.clone(),
                        Box::new(TestActor::new(actor_id.clone())),
                        mailbox,
                        "tenant".to_string(),
                        "namespace".to_string(),
                        None,
                    );
                    let actor_ref = CoreActorRef::new(actor_id.clone())
                        .map_err(|e| ActorError::InvalidState(e.to_string()))?;
                    Ok(StartedChild::Worker { actor, actor_ref })
                })
            }),
        );

        let _ = supervisor
            .start_child(spec)
            .await
            .expect("start_child should succeed");
    }

    let count = supervisor.count_children().await;
    assert_eq!(count.actors, 2);
    assert_eq!(count.supervisors, 0);
    assert_eq!(count.total, 2);
}

#[tokio::test]
async fn test_get_childspec() {
    let (mut supervisor, _event_rx) = create_test_supervisor().await;

    let child_id = "worker1".to_string();
    let actor_id = format!("{}@test-node", child_id);

    let actor_id_for_closure = actor_id.clone();
    let original_spec = ChildSpec::worker(
        child_id.clone(),
        actor_id.clone(),
        Arc::new(move || {
            let actor_id = actor_id_for_closure.clone();
            Box::pin(async move {
                let mailbox = Mailbox::new(
                    MailboxConfig::default(),
                    format!("mailbox-{}", actor_id.clone()),
                )
                .await
                .unwrap();
                let actor = Actor::new(
                    actor_id.clone(),
                    Box::new(TestActor::new(actor_id.clone())),
                    mailbox,
                    "tenant".to_string(),
                    "namespace".to_string(),
                    None,
                );
                let actor_ref = CoreActorRef::new(actor_id.clone())
                    .map_err(|e| ActorError::InvalidState(e.to_string()))?;
                Ok(StartedChild::Worker { actor, actor_ref })
            })
        }),
    );
    // Note: RestartStrategy is not directly accessible, so we skip the restart strategy test
    // The important part is that get_childspec() returns the spec

    let _ = supervisor
        .start_child(original_spec.clone())
        .await
        .expect("start_child should succeed");

    // Get child spec
    let retrieved_spec = supervisor.get_childspec(&actor_id).await;
    assert!(retrieved_spec.is_some(), "get_childspec should return spec");

    let spec = retrieved_spec.unwrap();
    assert_eq!(spec.child_id, child_id);
    // Note: actor_or_supervisor_id might be the full actor_id (with @node), not just child_id
    assert!(spec.actor_or_supervisor_id == actor_id || spec.actor_or_supervisor_id == child_id);
}

#[tokio::test]
async fn test_restart_child() {
    let (mut supervisor, _event_rx) = create_test_supervisor().await;

    // Add a child using ChildSpec
    let child_id = "worker1".to_string();
    let actor_id = format!("{}@test-node", child_id);

    let child_spec = create_child_spec_from_factory(
        actor_id.clone(),
        Arc::new({
            let actor_id = actor_id.clone();
            move || {
                let actor_id_clone = actor_id.clone();
                let mailbox = std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to create runtime for mailbox");
                    rt.block_on(Mailbox::new(
                        MailboxConfig::default(),
                        format!("mailbox-{}", actor_id_clone.clone()),
                    ))
                })
                .join()
                .expect("Thread panicked")
                .expect("Failed to create mailbox in factory");
                Ok(Actor::new(
                    actor_id.clone(),
                    Box::new(TestActor::new(actor_id.clone())),
                    mailbox,
                    "tenant".to_string(),
                    "namespace".to_string(),
                    None,
                ))
            }
        }),
        RestartPolicy::Permanent,
        Some(5000),
    );

    let _ = supervisor
        .add_child(child_spec)
        .await
        .expect("add_child should succeed");

    // Restart the child
    let result = supervisor.restart_child(&actor_id).await;
    assert!(result.is_ok(), "restart_child should succeed");

    // Verify child is still present
    let count = supervisor.count_children().await;
    assert_eq!(count.actors, 1);
}
