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

//! Comprehensive tests for Supervisor integration with Unified Lifecycle (Phase 1)
//!
//! Tests cover:
//! - Supervisor start_child with facets from ChildSpec
//! - Supervisor restart with facet restoration
//! - Facet lifecycle hooks during supervisor operations
//! - Multiple facets per actor in supervisor context

use plexspaces_supervisor::{Supervisor, SupervisionStrategy, ChildSpec, StartedChild};
use plexspaces_actor::{Actor, ActorRef as ActorActorRef};
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorError, BehaviorError, Message, ServiceLocator};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use std::sync::Arc;
use async_trait::async_trait;

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

async fn create_test_supervisor() -> (Supervisor, tokio::sync::mpsc::Receiver<plexspaces_supervisor::SupervisorEvent>) {
    let service_locator = Arc::new(ServiceLocator::new());
    let (mut supervisor, event_rx) = Supervisor::new(
        "test-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
    );
    supervisor = supervisor.with_service_locator(service_locator);
    (supervisor, event_rx)
}

/// Test S-I-1: Supervisor start_child with facets from ChildSpec
#[tokio::test]
async fn test_supervisor_start_child_with_facets() {
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
                let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", actor_id.clone())).await.unwrap();
                let actor = Actor::new(
                    actor_id.clone(),
                    Box::new(TestActor::new(actor_id.clone())),
                    mailbox,
                    "tenant".to_string(),
                    "namespace".to_string(),
                    None,
                );
                let actor_ref = ActorActorRef::local(
                    actor_id.clone(),
                    actor.mailbox().clone(),
                    Arc::new(ServiceLocator::new()),
                );
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
    assert_eq!(spec.facets.len(), 1);
    assert_eq!(spec.facets[0].r#type, "test");
    
    let result = supervisor.start_child(spec).await;
    assert!(result.is_ok(), "start_child should succeed");
    assert_eq!(result.unwrap(), actor_id);
    
    // Verify child is in supervisor
    let count = supervisor.count_children().await;
    assert_eq!(count.actors, 1);
    assert_eq!(count.total, 1);
}

/// Test S-I-2: Supervisor restart preserves facets
#[tokio::test]
async fn test_supervisor_restart_preserves_facets() {
    // TODO: Implement test
    // 1. Create supervisor with child having facets
    // 2. Start child
    // 3. Verify facets attached
    // 4. Simulate child crash
    // 5. Supervisor restarts child
    // 6. Verify facets restored
    todo!("Implement test")
}


