// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Minimal tests to isolate deadlock issues using bisect approach

use plexspaces_actor::ActorBuilder;
use plexspaces_core::{Actor, ActorContext, ActorId};
use plexspaces_journaling::TimerFacet;
use plexspaces_mailbox::Message;
use plexspaces_node::{Node, NodeBuilder};
use std::sync::Arc;
use std::time::Duration;
mod test_helpers;
use test_helpers::{lookup_actor_ref, activate_virtual_actor};


/// Simple behavior for testing
struct TestBehavior;

#[async_trait::async_trait]
#[async_trait::async_trait]
impl Actor for TestBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _message: Message,
        _reply: &dyn plexspaces_core::Reply,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        Ok(())
    }
    
    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

/// Helper to create a test node
fn create_test_node() -> Node {
    NodeBuilder::new("test-node")
        .build()
}

/// Test 1: Spawn actor WITHOUT facets - should not hang
#[tokio::test]
async fn test_01_spawn_actor_no_facets() {
    let node = Arc::new(create_test_node());
    
    let behavior = Box::new(TestBehavior);
    let actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await;
    
    // Spawn actor without facets
    let actor_ref = node.clone().spawn_actor(actor).await.unwrap();
    let actor_id = actor_ref.id().clone();
    
    // Wait for actor to be fully registered instead of using sleep
    let registration_future = async {
        loop {
            if lookup_actor_ref(&node, &actor_id).await.ok().flatten().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), registration_future)
        .await
        .expect("Actor should be registered within 5 seconds");
    
    // Should complete without hanging
    // Note: spawn_actor fixes the actor ID to include the node ID
    assert_eq!(actor_id, ActorId::from("test-actor@test-node"));
}

/// Test 2: Attach facet WITHOUT spawning - should not hang
#[tokio::test]
async fn test_02_attach_facet_no_spawn() {
    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await;
    
    // Attach facet
    let timer_facet = Box::new(TimerFacet::new());
    actor
        .attach_facet(timer_facet, 50, serde_json::json!({}))
        .await
        .unwrap();
    
    // Should complete without hanging
    // Verify facet is attached
    let facets = actor.facets();
    let facets_guard = facets.read().await;
    let facet_types = facets_guard.list_facets();
    assert!(facet_types.contains(&"timer".to_string()));
}

/// Test 3: Spawn actor WITH facet - isolate where hang occurs
#[tokio::test]
async fn test_03_spawn_actor_with_facet() {
    let node = Arc::new(create_test_node());
    
    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await;
    
    // Attach facet
    let timer_facet = Box::new(TimerFacet::new());
    actor
        .attach_facet(timer_facet, 50, serde_json::json!({}))
        .await
        .unwrap();
    
    // Spawn actor - this is where hang might occur
    println!("Before spawn_actor");
    let actor_ref = node.clone().spawn_actor(actor).await.unwrap();
    println!("After spawn_actor");
    
    let actor_id = actor_ref.id().clone();
    
    // Wait for actor to be fully registered instead of using sleep
    let registration_future = async {
        loop {
            if lookup_actor_ref(&node, &actor_id).await.ok().flatten().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), registration_future)
        .await
        .expect("Actor should be registered within 5 seconds");
    
    // Should complete without hanging
    // Note: spawn_actor fixes the actor ID to include the node ID
    assert_eq!(actor_id, ActorId::from("test-actor@test-node"));
}

/// Test 4: Check facet storage after spawn
#[tokio::test]
async fn test_04_facet_storage_after_spawn() {
    let node = Arc::new(create_test_node());
    
    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await;
    
    // Attach facet
    let timer_facet = Box::new(TimerFacet::new());
    actor
        .attach_facet(timer_facet, 50, serde_json::json!({}))
        .await
        .unwrap();
    
    // Spawn actor
    let actor_ref = node.clone().spawn_actor(actor).await.unwrap();
    let actor_id = actor_ref.id().clone();
    
    // Wait for initialization
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Check if facets are stored
    let facets = node.clone().get_facets(&actor_id).await;
    println!("Facets stored: {}", facets.is_some());
    
    if let Some(facets_arc) = facets {
        let facets_guard = facets_arc.read().await;
        let timer_facet_arc = facets_guard.get_facet("timer");
        println!("Timer facet found: {}", timer_facet_arc.is_some());
    }
}

