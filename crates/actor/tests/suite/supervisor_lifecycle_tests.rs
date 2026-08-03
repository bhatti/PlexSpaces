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

use super::test_actor_helpers::actor_with_default_service_locator;
use async_trait::async_trait;
use plexspaces_actor::child_spec::{ProtoRestartPolicy, ShutdownSpec, StartedChild};
use plexspaces_actor::supervisor::{
    RestartPolicy, SupervisionStrategy, Supervisor, SupervisorEvent,
};
use plexspaces_actor::{
    Actor as ActorTrait, ActorContext, ActorError, ActorRef as CoreActorRef, BehaviorError, Message,
};
use plexspaces_actor::{ActorInstance, ActorRef as ActorActorRef, ChildSpec};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::time::{sleep, Duration};

fn test_actor_id(name: &str) -> plexspaces_actor::ActorId {
    plexspaces_actor::ActorId::new(name, "gen_server", "namespace", "test-node")
        .expect("valid test actor id")
}

fn actor_id_from_legacy(id: &str) -> plexspaces_actor::ActorId {
    if let Ok(actor_id) = plexspaces_actor::ActorId::from_canonical(id) {
        return actor_id;
    }
    let name = id.split('@').next().unwrap_or(id);
    test_actor_id(name)
}

/// Helper function to create a ChildSpec from a sync factory
fn create_child_spec_from_factory(
    id: String,
    factory: Arc<dyn Fn() -> Result<ActorInstance, ActorError> + Send + Sync>,
    restart_policy: RestartPolicy,
    shutdown_timeout_ms: Option<u64>,
) -> ChildSpec {
    use plexspaces_actor::child_spec::ShutdownSpec;

    let actor_ref =
        CoreActorRef::new(actor_id_from_legacy(&id)).expect("Failed to create actor ref");

    let restart_policy_proto = match restart_policy {
        RestartPolicy::Permanent | RestartPolicy::ExponentialBackoff { .. } => {
            ProtoRestartPolicy::RestartPolicyPermanent
        }
        RestartPolicy::Transient => ProtoRestartPolicy::RestartPolicyTransient,
        RestartPolicy::Temporary => ProtoRestartPolicy::RestartPolicyTemporary,
    };

    let mut spec = ChildSpec::worker_sync(actor_id_from_legacy(&id), factory, actor_ref)
        .with_restart(restart_policy_proto);

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
impl plexspaces_actor::Actor for TestActor {
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

    fn behavior_type(&self) -> plexspaces_actor::BehaviorType {
        plexspaces_actor::BehaviorType::GenServer
    }
}

async fn create_test_supervisor() -> (Supervisor, tokio::sync::mpsc::Receiver<SupervisorEvent>) {
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None).await;
    Supervisor::new(
        test_actor_id("test-supervisor").to_string(),
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
    let actor_id = test_actor_id(&child_id).to_string();

    let actor_id_for_closure = actor_id.clone();
    let spec = ChildSpec::worker(
        test_actor_id(&child_id),
        Arc::new(move || {
            let actor_id = actor_id_for_closure.clone();
            Box::pin(async move {
                let mailbox = Mailbox::new(
                    MailboxConfig::default(), format!("mailbox-{}", actor_id.clone()), String::new(), String::new(), None,
                )
                .await
                .unwrap();
                let actor = actor_with_default_service_locator(
                    actor_id.clone(),
                    Box::new(TestActor::new(actor_id.clone())),
                    mailbox,
                    "tenant".to_string(),
                    "namespace".to_string(),
                )
                .await;
                let actor_ref = CoreActorRef::new(actor_id_from_legacy(&actor_id))
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
    let actor_id = test_actor_id(&child_id).to_string();

    let actor_id_for_closure = actor_id.clone();
    let spec = ChildSpec::worker(
        test_actor_id(&child_id),
        Arc::new(move || {
            let actor_id = actor_id_for_closure.clone();
            Box::pin(async move {
                let mailbox = Mailbox::new(
                    MailboxConfig::default(), format!("mailbox-{}", actor_id.clone()), String::new(), String::new(), None,
                )
                .await
                .unwrap();
                let actor = actor_with_default_service_locator(
                    actor_id.clone(),
                    Box::new(TestActor::new(actor_id.clone())),
                    mailbox,
                    "tenant".to_string(),
                    "namespace".to_string(),
                )
                .await;
                let actor_ref = CoreActorRef::new(actor_id_from_legacy(&actor_id))
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
        let actor_id = test_actor_id(&child_id).to_string();

        let spec = ChildSpec::worker(
            test_actor_id(&child_id),
            Arc::new(move || {
                let actor_id = actor_id.clone();
                Box::pin(async move {
                    let mailbox = Mailbox::new(
                        MailboxConfig::default(), format!("mailbox-{}", actor_id.clone()), String::new(), String::new(), None,
                    )
                    .await
                    .unwrap();
                    let actor = actor_with_default_service_locator(
                        actor_id.clone(),
                        Box::new(TestActor::new(actor_id.clone())),
                        mailbox,
                        "tenant".to_string(),
                        "namespace".to_string(),
                    )
                    .await;
                    let actor_ref = CoreActorRef::new(actor_id_from_legacy(&actor_id))
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
    assert!(child_ids.contains(&test_actor_id("worker1").to_string()));
    assert!(child_ids.contains(&test_actor_id("worker2").to_string()));
    assert!(child_ids.contains(&test_actor_id("worker3").to_string()));
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
        let actor_id = test_actor_id(&child_id).to_string();

        let spec = ChildSpec::worker(
            test_actor_id(&child_id),
            Arc::new(move || {
                let actor_id = actor_id.clone();
                Box::pin(async move {
                    let mailbox = Mailbox::new(
                        MailboxConfig::default(), format!("mailbox-{}", actor_id.clone()), String::new(), String::new(), None,
                    )
                    .await
                    .unwrap();
                    let actor = actor_with_default_service_locator(
                        actor_id.clone(),
                        Box::new(TestActor::new(actor_id.clone())),
                        mailbox,
                        "tenant".to_string(),
                        "namespace".to_string(),
                    )
                    .await;
                    let actor_ref = CoreActorRef::new(actor_id_from_legacy(&actor_id))
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
    let actor_id = test_actor_id(&child_id).to_string();

    let actor_id_for_closure = actor_id.clone();
    let original_spec = ChildSpec::worker(
        test_actor_id(&child_id),
        Arc::new(move || {
            let actor_id = actor_id_for_closure.clone();
            Box::pin(async move {
                let mailbox = Mailbox::new(
                    MailboxConfig::default(), format!("mailbox-{}", actor_id.clone()), String::new(), String::new(), None,
                )
                .await
                .unwrap();
                let actor = actor_with_default_service_locator(
                    actor_id.clone(),
                    Box::new(TestActor::new(actor_id.clone())),
                    mailbox,
                    "tenant".to_string(),
                    "namespace".to_string(),
                )
                .await;
                let actor_ref = CoreActorRef::new(actor_id_from_legacy(&actor_id))
                    .map_err(|e| ActorError::InvalidState(e.to_string()))?;
                Ok(StartedChild::Worker { actor, actor_ref })
            })
        }),
    );
    // The important part is that get_childspec() returns the spec

    let _ = supervisor
        .start_child(original_spec.clone())
        .await
        .expect("start_child should succeed");

    // Get child spec
    let retrieved_spec = supervisor.get_childspec(&actor_id).await;
    assert!(retrieved_spec.is_some(), "get_childspec should return spec");

    let spec = retrieved_spec.unwrap();
    assert_eq!(spec.actor_id.to_string(), actor_id);
}

#[tokio::test]
async fn test_restart_child() {
    let (mut supervisor, _event_rx) = create_test_supervisor().await;

    // Add a child using ChildSpec
    let child_id = "worker1".to_string();
    let actor_id = test_actor_id(&child_id).to_string();

    let child_spec = create_child_spec_from_factory(
        actor_id.clone(),
        Arc::new({
            let actor_id = actor_id.clone();
            move || {
                let actor_id_for_thread = actor_id.clone();
                let actor = std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to create runtime for mailbox");
                    let mailbox = rt
                        .block_on(Mailbox::new(
                            MailboxConfig::default(), format!("mailbox-{}", actor_id_for_thread.clone()), String::new(), String::new(), None,
                        ))
                        .expect("Failed to create mailbox in factory");
                    rt.block_on(actor_with_default_service_locator(
                        actor_id_for_thread.clone(),
                        Box::new(TestActor::new(actor_id_for_thread.clone())),
                        mailbox,
                        "tenant".to_string(),
                        "namespace".to_string(),
                    ))
                })
                .join()
                .expect("Thread panicked");
                Ok(actor)
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
