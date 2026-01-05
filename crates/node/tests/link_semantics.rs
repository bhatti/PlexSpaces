// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for Link Semantics (Erlang link/1 pattern)

use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::{Node, NodeConfig, NodeId};
use plexspaces_core::ExitReason;
use std::sync::Arc;

#[path = "test_helpers.rs"]
mod test_helpers;
use test_helpers::{find_actor_helper, unregister_actor_helper};

/// Helper to create a test node
async fn create_test_node() -> Node {
    use plexspaces_node::NodeBuilder;
    NodeBuilder::new("test-node")
        .with_listen_address("127.0.0.1:0")
        .with_max_connections(10)
        .with_heartbeat_interval_ms(1000)
        .with_clustering_enabled(false)
        .build().await
}

/// Helper to create a test actor ref
async fn create_test_actor_ref(node: &Node, actor_id: &str) -> plexspaces_actor::ActorRef {
    // Create a simple test behavior that processes messages (including EXIT messages)
    struct TestBehavior;
    
    #[async_trait::async_trait]
    impl plexspaces_core::Actor for TestBehavior {
        async fn handle_message(
            &mut self,
            _ctx: &plexspaces_core::ActorContext,
            _msg: plexspaces_mailbox::Message,
        ) -> Result<(), plexspaces_core::BehaviorError> {
            // Just consume the message - no processing needed for link tests
            Ok(())
        }

        fn behavior_type(&self) -> plexspaces_core::BehaviorType {
            plexspaces_core::BehaviorType::GenServer
        }
    }

    // Spawn a real actor that processes messages (needed for EXIT message handling)
    use plexspaces_actor::ActorBuilder;
    use plexspaces_core::ActorId;
    use test_helpers::spawn_actor_helper;
    
    let behavior = Box::new(TestBehavior);
    let actor_id_full = format!("{}@test-node", actor_id);
    let mut actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from(actor_id_full.clone()))
        .build()
        .await
        .unwrap();
    
    // Spawn the actor (this creates a real actor that processes messages, including EXIT messages)
    spawn_actor_helper(node, actor).await.unwrap()
}

/// Test basic linking functionality: linking two actors and bidirectional link verification
#[tokio::test]
async fn test_link_basic() {
    let node = create_test_node().await;
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    let actor2 = create_test_actor_ref(&node, "actor-2").await;
    
    // Link actor1 to actor2
    node.link(actor1.id(), actor2.id()).await.unwrap();
    
    // Verify bidirectional linking: unlink should work from either direction
    // This confirms that linking is bidirectional (actor1->actor2 and actor2->actor1)
    node.unlink(actor2.id(), actor1.id()).await.unwrap();
    
    // Re-link to test unlinking from original direction
    node.link(actor1.id(), actor2.id()).await.unwrap();
    node.unlink(actor1.id(), actor2.id()).await.unwrap();
}

#[tokio::test]
async fn test_link_self_fails() {
    let node = create_test_node().await;
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    
    // Linking actor to itself should fail
    let result = node.link(actor1.id(), actor1.id()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_unlink_nonexistent() {
    let node = create_test_node().await;
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    let actor2 = create_test_actor_ref(&node, "actor-2").await;
    
    // Unlinking actors that aren't linked should succeed (idempotent)
    node.unlink(actor1.id(), actor2.id()).await.unwrap();
}

/// Helper to wait for an actor to die (or verify it's alive)
/// Optimized with bounded iterations and yield_now for responsiveness
async fn wait_for_actor_state(
    node: &Node,
    actor_id: &str,
    should_be_dead: bool,
    timeout_secs: u64,
) -> Result<(), String> {
    let actor_id_full = format!("{}@test-node", actor_id);
    let actor_id_parsed = plexspaces_core::ActorId::from(actor_id_full);
    
    let max_iterations = (timeout_secs * 1000) / 10; // 10ms per iteration
    let check_complete = async {
        let mut iterations = 0;
        loop {
            let result = find_actor_helper(node, &actor_id_parsed).await;
            let is_dead = result.is_err() || matches!(result, Ok(plexspaces_node::ActorLocation::Remote(_)));
            
            if should_be_dead && is_dead {
                return Ok(());
            } else if !should_be_dead && !is_dead {
                return Ok(());
            }
            
            iterations += 1;
            if iterations >= max_iterations {
                return Err(format!("Max iterations reached waiting for actor {} to be {}", actor_id, if should_be_dead { "dead" } else { "alive" }));
            }
            
            tokio::task::yield_now().await;
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    };
    
    match tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), check_complete).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(format!("Timeout waiting for actor {} to be {}", actor_id, if should_be_dead { "dead" } else { "alive" })),
    }
}

/// Helper to terminate an actor with a specific exit reason
async fn terminate_actor(
    node: &Node,
    actor_id: &str,
    reason: ExitReason,
) {
    let actor_id_full = format!("{}@test-node", actor_id);
    let actor_id_parsed = plexspaces_core::ActorId::from(actor_id_full);
    
    unregister_actor_helper(node, &actor_id_parsed).await.unwrap();
    use plexspaces_core::ActorRegistry;
    use plexspaces_core::service_locator::service_names;
    let actor_registry: Arc<ActorRegistry> = node.service_locator().get_service_by_name(service_names::ACTOR_REGISTRY).await.unwrap();
    actor_registry.handle_actor_termination(&actor_id_parsed, reason).await;
}

/// Unified test for exit condition cascading behavior
/// Tests all exit conditions: error cascades, normal doesn't cascade, chain cascading, multiple links
#[tokio::test]
async fn test_exit_condition_cascading() {
    let node = create_test_node().await;
    
    // Test 1: Error exit cascades to linked actor
    {
        let actor1 = create_test_actor_ref(&node, "exit-test-1").await;
        let actor2 = create_test_actor_ref(&node, "exit-test-2").await;
        
        node.link(actor1.id(), actor2.id()).await.unwrap();
        tokio::task::yield_now().await; // Give link time to register
        
        // Verify both actors are alive before termination
        let result1_before = find_actor_helper(&node, actor1.id()).await;
        let result2_before = find_actor_helper(&node, actor2.id()).await;
        assert!(result1_before.is_ok(), "actor1 should be alive before termination");
        assert!(result2_before.is_ok(), "actor2 should be alive before termination");
        
        terminate_actor(&node, "exit-test-1", ExitReason::Error("panic: test".to_string())).await;
        
        wait_for_actor_state(&node, "exit-test-2", true, 10).await
            .expect("Error exit should cascade to linked actor");
    }
    
    // Test 2: Normal exit does NOT cascade to linked actor
    {
        let actor1 = create_test_actor_ref(&node, "normal-test-1").await;
        let actor2 = create_test_actor_ref(&node, "normal-test-2").await;
        
        node.link(actor1.id(), actor2.id()).await.unwrap();
        tokio::task::yield_now().await; // Give link time to register
        
        // Verify both actors are alive before termination
        let result1_before = find_actor_helper(&node, actor1.id()).await;
        let result2_before = find_actor_helper(&node, actor2.id()).await;
        assert!(result1_before.is_ok(), "actor1 should be alive before termination");
        assert!(result2_before.is_ok(), "actor2 should be alive before termination");
        
        terminate_actor(&node, "normal-test-1", ExitReason::Normal).await;
        
        // Give termination time to process, then verify actor2 is still alive
        tokio::task::yield_now().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        wait_for_actor_state(&node, "normal-test-2", false, 1).await
            .expect("Normal exit should NOT cascade to linked actor");
    }
    
    // Test 3: Chain cascading (actor1 -> actor2 -> actor3)
    {
        let actor1 = create_test_actor_ref(&node, "chain-1").await;
        let actor2 = create_test_actor_ref(&node, "chain-2").await;
        let actor3 = create_test_actor_ref(&node, "chain-3").await;
        
        node.link(actor1.id(), actor2.id()).await.unwrap();
        node.link(actor2.id(), actor3.id()).await.unwrap();
        
        // Give links time to register
        tokio::task::yield_now().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        // Verify all actors are alive before termination
        let result1_before = find_actor_helper(&node, actor1.id()).await;
        let result2_before = find_actor_helper(&node, actor2.id()).await;
        let result3_before = find_actor_helper(&node, actor3.id()).await;
        assert!(result1_before.is_ok(), "actor1 should be alive before termination");
        assert!(result2_before.is_ok(), "actor2 should be alive before termination");
        assert!(result3_before.is_ok(), "actor3 should be alive before termination");
        
        terminate_actor(&node, "chain-1", ExitReason::Error("panic: test".to_string())).await;
        
        // Wait for actor2 to die (cascaded from actor1) - use optimized helper
        wait_for_actor_state(&node, "chain-2", true, 3).await
            .expect("actor2 should be dead (cascaded from actor1)");
        
        // Wait for actor3 to die (cascaded from actor2) - use optimized helper
        wait_for_actor_state(&node, "chain-3", true, 3).await
            .expect("actor3 should be dead (cascaded from actor2)");
    }
    
    // Test 4: Multiple links with partial unlinking
    {
        let actor1 = create_test_actor_ref(&node, "multi-1").await;
        let actor2 = create_test_actor_ref(&node, "multi-2").await;
        let actor3 = create_test_actor_ref(&node, "multi-3").await;
        
        node.link(actor1.id(), actor2.id()).await.unwrap();
        node.link(actor1.id(), actor3.id()).await.unwrap();
        node.unlink(actor1.id(), actor2.id()).await.unwrap(); // Unlink actor2
        tokio::task::yield_now().await; // Give unlink time to process
        
        terminate_actor(&node, "multi-1", ExitReason::Error("panic: test".to_string())).await;
        tokio::task::yield_now().await; // Give termination time to propagate
        
        // actor2 should be alive (unlinked), actor3 should be dead (still linked)
        wait_for_actor_state(&node, "multi-2", false, 1).await
            .expect("actor2 should be alive (unlinked)");
        wait_for_actor_state(&node, "multi-3", true, 2).await
            .expect("actor3 should be dead (still linked)");
    }
}

