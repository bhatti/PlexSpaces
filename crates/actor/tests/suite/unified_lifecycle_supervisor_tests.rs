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

//! Comprehensive tests for Supervisor integration with Unified Lifecycle (Phase 1)
//!
//! Tests cover:
//! - Supervisor start_child with facets from ChildSpec
//! - Supervisor restart with facet restoration
//! - Facet lifecycle hooks during supervisor operations
//! - Multiple facets per actor in supervisor context

use super::test_actor_helpers::actor_with_default_service_locator;
use async_trait::async_trait;
use plexspaces_actor::child_spec::StartedChild;
use plexspaces_actor::supervisor::{SupervisionStrategy, Supervisor, SupervisorEvent};
use plexspaces_actor::{
    Actor as ActorTrait, ActorContext, ActorError, ActorRef as CoreActorRef, BehaviorError, Message,
};
use plexspaces_actor::ChildSpec;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use std::sync::Arc;

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

struct InitObservingActor {
    observed_ids: Arc<tokio::sync::Mutex<Vec<plexspaces_actor::ActorId>>>,
}

#[async_trait]
impl plexspaces_actor::Actor for InitObservingActor {
    async fn init(&mut self, ctx: &ActorContext) -> Result<(), ActorError> {
        self.observed_ids.lock().await.push(ctx.actor_id().clone());
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

/// Test S-I-1: Supervisor start_child with facets from ChildSpec
#[tokio::test]
async fn test_supervisor_start_child_with_facets() {
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

    // Add facets to ChildSpec (Phase 1: Unified Lifecycle)
    // Note: For now, facets should be attached in the factory function
    // This test verifies that ChildSpec can store facets
    let mut facet_config = std::collections::HashMap::new();
    facet_config.insert("test_key".to_string(), "test_value".to_string());
    let proto_facet = plexspaces_proto::common::v1::Facet {
        r#type: "test".to_string(),
        config: facet_config,
        priority: 100,
        state: std::collections::HashMap::new(),
        metadata: None,
    };
    let spec = spec.with_facet(proto_facet);

    // Verify facets are stored in ChildSpec
    assert_eq!(spec.proto.facets.len(), 1);
    assert_eq!(spec.proto.facets[0].r#type, "test");

    let result = supervisor.start_child(spec).await;
    assert!(result.is_ok(), "start_child should succeed");
    assert_eq!(result.unwrap(), actor_id);

    // Verify child is in supervisor
    let count = supervisor.count_children().await;
    assert_eq!(count.actors, 1);
    assert_eq!(count.total, 1);
}

/// Test S-I-2: Supervisor restart preserves facets
///
/// This test verifies that when a supervisor restarts a child actor,
/// the facets from the ChildSpec are preserved and reattached to the new actor.
#[tokio::test]
async fn test_supervisor_restart_preserves_facets() {
    // Create supervisor (default ServiceLocator so child actors can register on start)
    let (mut supervisor, mut event_rx) = create_test_supervisor().await;

    // Create ChildSpec with facets
    let child_id = "faceted-worker".to_string();
    let actor_id = test_actor_id(&child_id).to_string();

    let spec = ChildSpec::worker(
        test_actor_id(&child_id),
        Arc::new({
            let actor_id = actor_id.clone();
            move || {
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
            }
        }),
    );

    // Add facet to spec (using proto Facet type)
    let facet = plexspaces_proto::common::v1::Facet {
        r#type: "test_facet".to_string(),
        config: std::collections::HashMap::new(),
        priority: 1,
        metadata: None,
        state: std::collections::HashMap::new(),
    };
    let spec = spec.with_facet(facet.clone());

    // Start child
    let result = supervisor.start_child(spec).await;
    assert!(result.is_ok(), "start_child should succeed");

    // Consume ChildStarted event
    let _ = event_rx.recv().await;

    // Verify child is present
    let count = supervisor.count_children().await;
    assert_eq!(count.actors, 1, "Should have 1 actor child");

    // Get the child spec to verify facets are stored
    let retrieved_spec = supervisor.get_childspec(&actor_id).await;
    assert!(retrieved_spec.is_some(), "Should be able to get child spec");
    let retrieved_spec = retrieved_spec.unwrap();
    assert_eq!(
        retrieved_spec.proto.facets.len(),
        1,
        "Spec should have 1 facet stored"
    );
    assert_eq!(
        retrieved_spec.proto.facets[0].r#type, "test_facet",
        "Facet type should match"
    );

    // Trigger restart via handle_failure
    let result = supervisor
        .handle_failure(
            &actor_id_from_legacy(&actor_id),
            "simulated crash".to_string(),
            Some(plexspaces_actor::ExitReason::Error(
                "test error".to_string(),
            )),
        )
        .await;
    assert!(result.is_ok(), "handle_failure should succeed");

    // Consume events (ChildFailed, ChildRestarted)
    let _ = event_rx.recv().await;
    let _ = event_rx.recv().await;

    // Verify child was restarted (still present)
    let count = supervisor.count_children().await;
    assert_eq!(
        count.actors, 1,
        "Should still have 1 actor child after restart"
    );

    // Verify facets are still in the spec (preserved during restart)
    let retrieved_spec = supervisor.get_childspec(&actor_id).await;
    assert!(
        retrieved_spec.is_some(),
        "Should be able to get child spec after restart"
    );
    let retrieved_spec = retrieved_spec.unwrap();
    assert_eq!(
        retrieved_spec.proto.facets.len(),
        1,
        "Spec should still have 1 facet after restart"
    );
    assert_eq!(
        retrieved_spec.proto.facets[0].r#type, "test_facet",
        "Facet type should still match after restart"
    );
}

#[tokio::test]
async fn test_supervisor_restart_sets_self_ref_before_init() {
    let (mut supervisor, mut event_rx) = create_test_supervisor().await;
    let observed_ids = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let child_name = "self-ref-worker";
    let actor_id = test_actor_id(child_name);
    let actor_id_string = actor_id.to_string();
    let actor_id_for_factory = actor_id.clone();
    let spec = ChildSpec::worker(
        actor_id.clone(),
        Arc::new({
            let observed_ids = observed_ids.clone();
            let actor_id_for_factory = actor_id_for_factory.clone();
            move || {
                let observed_ids = observed_ids.clone();
                let actor_id_string = actor_id_string.clone();
                let actor_id_for_factory = actor_id_for_factory.clone();
                Box::pin(async move {
                    let mailbox = Mailbox::new(
                        MailboxConfig::default(),
                        format!("mailbox-{}", actor_id_string),
                        String::new(),
                        String::new(),
                        None,
                    )
                    .await
                    .unwrap();
                    let actor = actor_with_default_service_locator(
                        actor_id_string.clone(),
                        Box::new(InitObservingActor {
                            observed_ids: observed_ids.clone(),
                        }),
                        mailbox,
                        "tenant".to_string(),
                        "namespace".to_string(),
                    )
                    .await;
                    let actor_ref = CoreActorRef::new(actor_id_for_factory.clone())
                        .map_err(|e| ActorError::InvalidState(e.to_string()))?;
                    Ok(StartedChild::Worker { actor, actor_ref })
                })
            }
        }),
    );

    supervisor
        .start_child(spec)
        .await
        .expect("start_child should succeed");
    let _ = event_rx.recv().await;

    supervisor
        .restart_child(actor_id.as_str())
        .await
        .expect("restart_child should succeed");

    let observed_ids = observed_ids.lock().await.clone();
    assert_eq!(
        observed_ids,
        vec![actor_id.clone(), actor_id],
        "both initial start and supervisor restart should inject self_ref before init()"
    );
}
