// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Test to verify facet storage works correctly

mod test_helpers;
use test_helpers::spawn_actor_helper;

use plexspaces_actor::ActorBuilder;
use plexspaces_core::{ActorBehavior, ActorContext, ActorId};
use plexspaces_journaling::TimerFacet;
use plexspaces_mailbox::Message;
use plexspaces_node::{Node, NodeBuilder};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

struct TestBehavior;

#[async_trait::async_trait]
impl ActorBehavior for TestBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _message: Message,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        Ok(())
    }
    
    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

#[tokio::test]
async fn test_facet_storage_direct() {
    let node = Arc::new(NodeBuilder::new("test-node").build());
    
    // Create actor with TimerFacet
    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await;
    
    // Attach TimerFacet
    let timer_facet = Box::new(TimerFacet::new(serde_json::json!({}), 50));
    actor
        .attach_facet(timer_facet)
        .await
        .unwrap();
    
    // Verify facet is attached before spawning
    let facets_before = actor.facets();
    let facets_guard_before = facets_before.read().await;
    let facet_types_before = facets_guard_before.list_facets();
    assert!(facet_types_before.contains(&"timer".to_string()), "TimerFacet should be attached before spawn");
    drop(facets_guard_before);
    
    // Spawn actor
    let actor_id = ActorId::from("test-actor@local");
    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    
    // Wait a bit
    sleep(Duration::from_millis(100)).await;
    
    // Check if facets are stored
    let facets_arc = node.clone().get_facets(&actor_id).await;
    
    if facets_arc.is_none() {
        // Debug: Check what actor_ids are in storage
        // This requires making facet_storage accessible or adding a debug method
        panic!("Facets not found for actor_id: {}", actor_id);
    }
    
    let facets_arc = facets_arc.unwrap();
    let facets_guard = facets_arc.read().await;
    let timer_facet_arc = facets_guard.get_facet("timer");
    assert!(timer_facet_arc.is_some(), "TimerFacet should be retrievable");
}

