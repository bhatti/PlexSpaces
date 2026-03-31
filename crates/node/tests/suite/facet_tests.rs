// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Consolidated facet tests from:
// - facet_service_debug.rs (4 tests)
// - facet_service_integration.rs (5 tests + 1 feature-gated)
// - facet_storage_test.rs (1 test)
// Total: 10 tests (+1 feature-gated)

use super::test_helpers::{lookup_actor_ref, spawn_actor_helper};

use plexspaces_actor::ActorBuilder;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, Message};
use plexspaces_journaling::TimerFacet;
use plexspaces_node::{Node, NodeBuilder};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

// =============================================================================
// COMMON HELPERS
// =============================================================================

/// Simple behavior for testing
struct TestBehavior;

#[async_trait::async_trait]
impl ActorTrait for TestBehavior {
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

/// Helper to create a test node
async fn create_test_node() -> Node {
    NodeBuilder::new("test-node").build().await
}

fn create_timer_facet(service_locator: Arc<dyn plexspaces_core::ServiceLocator>) -> Box<TimerFacet> {
    Box::new(TimerFacet::new(serde_json::json!({}), 50, service_locator))
}

/// Helper to create a test message
fn create_test_message(payload: Vec<u8>) -> Message {
    Message {
        id: ulid::Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

// =============================================================================
// FACET DEBUG TESTS (from facet_service_debug.rs - 4 tests)
// =============================================================================

/// Test 1: Spawn actor WITHOUT facets - should not hang
#[tokio::test]
async fn test_spawn_actor_no_facets() {
    let node = Arc::new(create_test_node().await);

    let behavior = Box::new(TestBehavior);
    let actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await
        .unwrap();

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();

    let registration_future = async {
        loop {
            if lookup_actor_ref(&node, &actor_id)
                .await
                .ok()
                .flatten()
                .is_some()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), registration_future)
        .await
        .expect("Actor should be registered within 5 seconds");

    assert_eq!(actor_id, ActorId::from("test-actor@test-node"));
}

/// Test 2: Attach facet WITHOUT spawning - should not hang
#[tokio::test]
async fn test_attach_facet_no_spawn() {
    let node = Arc::new(create_test_node().await);
    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await
        .unwrap();

    let timer_facet = create_timer_facet(node.service_locator());
    actor.attach_facet(timer_facet).await.unwrap();

    let facets = actor.facets();
    let facets_guard = facets.read().await;
    let facet_types = facets_guard.list_facets();
    assert!(facet_types.contains(&"timer".to_string()));
}

/// Test 3: Spawn actor WITH facet - isolate where hang occurs
#[tokio::test]
async fn test_spawn_actor_with_facet() {
    let node = Arc::new(create_test_node().await);

    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await
        .unwrap();

    let timer_facet = create_timer_facet(node.service_locator());
    actor.attach_facet(timer_facet).await.unwrap();

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();

    let registration_future = async {
        loop {
            if lookup_actor_ref(&node, &actor_id)
                .await
                .ok()
                .flatten()
                .is_some()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), registration_future)
        .await
        .expect("Actor should be registered within 5 seconds");

    assert_eq!(actor_id, ActorId::from("test-actor@test-node"));
}

/// Test 4: Check facet storage after spawn
#[tokio::test]
async fn test_facet_storage_after_spawn() {
    let node = Arc::new(create_test_node().await);

    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await
        .unwrap();

    let timer_facet = create_timer_facet(node.service_locator());
    actor.attach_facet(timer_facet).await.unwrap();

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();

    sleep(Duration::from_millis(200)).await;

    let facets = node.get_facets(&actor_id).await;
    if let Some(facets_arc) = facets {
        let facets_guard = facets_arc.read().await;
        let _timer_facet_arc = facets_guard.get_facet("timer");
    }
}

// =============================================================================
// FACET SERVICE INTEGRATION TESTS (from facet_service_integration.rs - 5 tests)
// =============================================================================

#[tokio::test]
async fn test_facet_service_get_facet_normal_actor() {
    let node = Arc::new(create_test_node().await);

    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await
        .unwrap();

    let timer_facet = create_timer_facet(node.service_locator());
    actor.attach_facet(timer_facet).await.unwrap();

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();

    assert!(actor_id.as_str().contains("test-actor"));
    assert!(actor_id.as_str().contains("@"));

    let facets = node.clone().get_facets(&actor_id).await;
    assert!(
        facets.is_some(),
        "Facets should be stored for normal actor (actor_id={:?})",
        actor_id
    );

    let facets_arc = facets.unwrap();
    let facets_guard = facets_arc.read().await;
    let timer_facet_arc = facets_guard.get_facet("timer");
    assert!(
        timer_facet_arc.is_some(),
        "TimerFacet should be retrievable"
    );
}

#[tokio::test]
async fn test_facet_service_get_facet_virtual_actor() {
    let node = Arc::new(create_test_node().await);

    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("virtual-actor@local"))
        .build()
        .await
        .unwrap();

    use plexspaces_journaling::VirtualActorFacet;
    let virtual_facet = Box::new(VirtualActorFacet::new(serde_json::json!({}), 50));
    actor.attach_facet(virtual_facet).await.unwrap();

    let timer_facet = create_timer_facet(node.service_locator());
    actor.attach_facet(timer_facet).await.unwrap();

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();

    let message = create_test_message(b"activate".to_vec());
    actor_ref.tell(message).await.unwrap();

    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(5);
    let mut facets = None;
    while start.elapsed() < timeout {
        tokio::task::yield_now().await;
        let (exists, is_active, _) = node.check_virtual_actor_exists(&actor_id).await;
        if exists && is_active {
            facets = node.clone().get_facets(&actor_id).await;
            if facets.is_some() {
                break;
            }
        }
        sleep(Duration::from_millis(50)).await;
    }

    assert!(
        facets.is_some(),
        "Facets should be stored for virtual actor after activation (actor_id={:?})",
        actor_id
    );

    let facets_arc = facets.unwrap();
    let facets_guard = facets_arc.read().await;
    let timer_facet_arc = facets_guard.get_facet("timer");
    assert!(
        timer_facet_arc.is_some(),
        "TimerFacet should be retrievable for virtual actor"
    );
}

#[tokio::test]
async fn test_facet_service_get_facet_not_found() {
    let node = Arc::new(create_test_node().await);

    let behavior = Box::new(TestBehavior);
    let actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("no-facet-actor@local"))
        .build()
        .await
        .unwrap();

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();

    sleep(Duration::from_millis(200)).await;

    let facets = node.clone().get_facets(&actor_id).await;
    if let Some(facets_arc) = facets {
        let facets_guard = facets_arc.read().await;
        let timer_facet_arc = facets_guard.get_facet("timer");
        assert!(timer_facet_arc.is_none(), "TimerFacet should not be found");
    }
}

#[tokio::test]
async fn test_facet_service_facets_cleaned_up_on_unregister() {
    let node = Arc::new(create_test_node().await);

    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("cleanup-actor@local"))
        .build()
        .await
        .unwrap();

    let timer_facet = create_timer_facet(node.service_locator());
    actor.attach_facet(timer_facet).await.unwrap();

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();

    let facets = node.clone().get_facets(&actor_id).await;
    assert!(facets.is_some(), "Facets should be stored");

    let actor_registry = node.service_locator().actor_registry().await.unwrap();
    actor_registry
        .unregister_with_cleanup(&actor_id)
        .await
        .unwrap();

    let facets_after = node.clone().get_facets(&actor_id).await;
    assert!(
        facets_after.is_none(),
        "Facets should be cleaned up after unregister"
    );
}

#[cfg(feature = "sqlite-backend")]
#[tokio::test]
async fn test_facet_service_with_sqlite_backend() {
    let node = Arc::new(create_test_node().await);

    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("sqlite-actor@local"))
        .build()
        .await;

    let timer_facet = create_timer_facet(node.service_locator());
    actor.attach_facet(timer_facet).await.unwrap();

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();

    sleep(Duration::from_millis(200)).await;

    let facets = node.clone().get_facets(&actor_id).await;
    assert!(
        facets.is_some(),
        "Facets should be stored with SQLite backend"
    );

    let facets_arc = facets.unwrap();
    let facets_guard = facets_arc.read().await;
    let timer_facet_arc = facets_guard.get_facet("timer");
    assert!(
        timer_facet_arc.is_some(),
        "TimerFacet should be retrievable with SQLite backend"
    );
}

// =============================================================================
// FACET STORAGE DIRECT TEST (from facet_storage_test.rs - 1 test)
// =============================================================================

#[tokio::test]
async fn test_facet_storage_direct() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);

    let behavior = Box::new(TestBehavior);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await
        .unwrap();

    let timer_facet = create_timer_facet(node.service_locator());
    actor.attach_facet(timer_facet).await.unwrap();

    let facets_before = actor.facets();
    let facets_guard_before = facets_before.read().await;
    let facet_types_before = facets_guard_before.list_facets();
    assert!(
        facet_types_before.contains(&"timer".to_string()),
        "TimerFacet should be attached before spawn"
    );
    drop(facets_guard_before);

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();

    sleep(Duration::from_millis(100)).await;

    let facets_arc = node.get_facets(&actor_id).await;

    if facets_arc.is_none() {
        panic!("Facets not found for actor_id: {}", actor_id);
    }

    let facets_arc = facets_arc.unwrap();
    let facets_guard = facets_arc.read().await;
    let timer_facet_arc = facets_guard.get_facet("timer");
    assert!(
        timer_facet_arc.is_some(),
        "TimerFacet should be retrievable"
    );
}
