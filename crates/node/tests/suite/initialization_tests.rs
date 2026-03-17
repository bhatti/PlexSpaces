// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Minimal tests to verify Node initialization and service setup

use super::test_helpers::spawn_actor_helper;

use plexspaces_node::{Node, NodeBuilder};
use std::sync::Arc;

/// Test 1: Node creation and basic initialization
#[tokio::test]
async fn test_01_node_creation() {
    let node = NodeBuilder::new("test-node").build().await;
    assert_eq!(node.id().as_str(), "test-node");
}

/// Test 2: Node with Arc - should not hang
#[tokio::test]
async fn test_02_node_arc_creation() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    assert_eq!(node.id().as_str(), "test-node");
}

/// Test 3: Node clone - should not hang
#[tokio::test]
async fn test_03_node_clone() {
    let node = NodeBuilder::new("test-node").build().await;
    let node_clone = node.clone();
    assert_eq!(node_clone.id().as_str(), "test-node");
}

/// Test 4: Node Arc clone - should not hang
#[tokio::test]
async fn test_04_node_arc_clone() {
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let node_clone = node.clone();
    assert_eq!(node_clone.id().as_str(), "test-node");
}

/// Test 5: Access facet_storage via get_facets - should not hang
#[tokio::test]
async fn test_05_facet_storage_access() {
    use plexspaces_core::ActorId;
    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let actor_id = ActorId::from("test-actor@local");
    let facets: Option<Arc<tokio::sync::RwLock<plexspaces_facet::FacetContainer>>> =
        node.get_facets(&actor_id).await;
    assert!(facets.is_none()); // No facets stored yet
}

/// Test 6: Spawn actor without facets - isolate potential hang
#[tokio::test]
async fn test_06_spawn_actor_no_facets() {
    use plexspaces_actor::ActorBuilder;
    use plexspaces_core::Message;
    use plexspaces_core::{Actor, ActorContext, ActorId};

    struct TestBehavior;

    #[async_trait::async_trait]
    impl Actor for TestBehavior {
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

    let node = Arc::new(NodeBuilder::new("test-node").build().await);
    let behavior = Box::new(TestBehavior);
    let actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("test-actor@local"))
        .build()
        .await
        .unwrap();

    // This is where the hang might occur
    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    // spawn_actor_helper normalizes the actor ID to include the node ID
    assert!(
        actor_ref.id().as_str().contains("test-actor"),
        "Actor ID should contain 'test-actor'"
    );
    assert!(
        actor_ref.id().as_str().contains("@"),
        "Actor ID should contain '@'"
    );
    assert!(
        actor_ref.id().as_str().contains("test-node"),
        "Actor ID should contain node ID 'test-node'"
    );
}
