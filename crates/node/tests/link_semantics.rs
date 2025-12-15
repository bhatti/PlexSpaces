// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for Link Semantics (Erlang link/1 pattern)

use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::{Node, NodeConfig, NodeId};
use std::sync::Arc;

#[path = "test_helpers.rs"]
mod test_helpers;
use test_helpers::{find_actor_helper, unregister_actor_helper};

/// Helper to create a test node
fn create_test_node() -> Node {
    Node::new(
        NodeId::new("test-node"),
        NodeConfig {
            listen_addr: "127.0.0.1:0".to_string(),
            max_connections: 10,
            heartbeat_interval_ms: 1000,
            clustering_enabled: false,
            metadata: std::collections::HashMap::new(),
        },
    )
}

/// Helper to create a test actor ref
async fn create_test_actor_ref(node: &Node, actor_id: &str) -> plexspaces_actor::ActorRef {
    use plexspaces_actor::ActorRef;
    use plexspaces_actor::RegularActorWrapper;
    use plexspaces_core::MessageSender;
    
    let actor_id_full = format!("{}@test-node", actor_id);
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), actor_id_full.clone()).await.unwrap());
    let service_locator = node.service_locator();
    let actor_ref = ActorRef::local(actor_id_full.clone(), mailbox.clone(), service_locator.clone());
    
    // Register actor with MessageSender (mailbox is internal)
    let wrapper = Arc::new(RegularActorWrapper::new(
        actor_id_full.clone(),
        mailbox,
        service_locator,
    ));
    node.actor_registry().register_actor(actor_id_full, wrapper).await;
    
    // Also register with node for config tracking
    node.actor_registry().register_actor_with_config(actor_ref.id().as_str().to_string(), None).await.unwrap();
    
    actor_ref
}

#[tokio::test]
async fn test_link_two_actors() {
    let node = create_test_node();
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    let actor2 = create_test_actor_ref(&node, "actor-2").await;
    
    // Link the two actors
    node.link(actor1.id(), actor2.id()).await.unwrap();
    
    // Verify link exists (by checking that unlink succeeds)
    node.unlink(actor1.id(), actor2.id()).await.unwrap();
}

#[tokio::test]
async fn test_link_bidirectional() {
    let node = create_test_node();
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    let actor2 = create_test_actor_ref(&node, "actor-2").await;
    
    // Link actor1 to actor2
    node.link(actor1.id(), actor2.id()).await.unwrap();
    
    // Verify bidirectional: actor2 should be linked to actor1
    // (We can't directly check, but unlink should work from either direction)
    node.unlink(actor2.id(), actor1.id()).await.unwrap();
}

#[tokio::test]
async fn test_link_self_fails() {
    let node = create_test_node();
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    
    // Linking actor to itself should fail
    let result = node.link(actor1.id(), actor1.id()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_unlink_nonexistent() {
    let node = create_test_node();
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    let actor2 = create_test_actor_ref(&node, "actor-2").await;
    
    // Unlinking actors that aren't linked should succeed (idempotent)
    node.unlink(actor1.id(), actor2.id()).await.unwrap();
}

#[tokio::test]
async fn test_cascading_failure_abnormal_death() {
    let node = create_test_node();
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    let actor2 = create_test_actor_ref(&node, "actor-2").await;
    
    // Link actor1 to actor2
    node.link(actor1.id(), actor2.id()).await.unwrap();
    
    // Monitor actor2 to detect when it dies
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let _monitor_ref = node.monitor(actor2.id(), actor1.id(), tx).await.unwrap();
    
    // Simulate abnormal death of actor1
    unregister_actor_helper(&node, actor1.id()).await.unwrap();
    node.notify_actor_down(actor1.id(), "panic: test failure").await.unwrap();
    
    // Wait for cascading failure to propagate - poll until actor2 is killed
    let cascading_complete = async {
        loop {
            let result = find_actor_helper(&node, actor2.id()).await;
            if result.is_err() || matches!(result, Ok(plexspaces_node::ActorLocation::Remote(_))) {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(5), cascading_complete)
        .await
        .expect("Cascading failure should propagate within 5 seconds");
    
    // Verify actor2 was also killed (should not be in registry)
    let result = find_actor_helper(&node, actor2.id()).await;
    assert!(result.is_err() || matches!(result, Ok(plexspaces_node::ActorLocation::Remote(_))));
}

#[tokio::test]
async fn test_cascading_failure_normal_shutdown() {
    let node = create_test_node();
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    let actor2 = create_test_actor_ref(&node, "actor-2").await;
    
    // Link actor1 to actor2
    node.link(actor1.id(), actor2.id()).await.unwrap();
    
    // Simulate normal shutdown of actor1
    unregister_actor_helper(&node, actor1.id()).await.unwrap();
    node.notify_actor_down(actor1.id(), "normal").await.unwrap();
    
    // Wait for notification to be processed - normal shutdown shouldn't cascade
    let notification_processed = async {
        tokio::task::yield_now().await;
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(1), notification_processed)
        .await
        .expect("Notification processing should complete quickly");
    
    // Verify actor2 is still alive (normal shutdown doesn't cascade)
    let result = find_actor_helper(&node, actor2.id()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_multiple_links() {
    let node = create_test_node();
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    let actor2 = create_test_actor_ref(&node, "actor-2").await;
    let actor3 = create_test_actor_ref(&node, "actor-3").await;
    
    // Link actor1 to both actor2 and actor3
    node.link(actor1.id(), actor2.id()).await.unwrap();
    node.link(actor1.id(), actor3.id()).await.unwrap();
    
    // Unlink actor1 from actor2 (actor3 should still be linked)
    node.unlink(actor1.id(), actor2.id()).await.unwrap();
    
    // Verify actor3 is still linked (by checking cascading still works)
    unregister_actor_helper(&node, actor1.id()).await.unwrap();
    node.notify_actor_down(actor1.id(), "panic: test").await.unwrap();
    
    // Wait for cascading failure to propagate - poll until actor3 is killed
    let cascading_complete = async {
        loop {
            let result3 = find_actor_helper(&node, actor3.id()).await;
            if result3.is_err() || matches!(result3, Ok(plexspaces_node::ActorLocation::Remote(_))) {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(5), cascading_complete)
        .await
        .expect("Cascading failure should propagate within 5 seconds");
    
    // Actor3 should be killed, actor2 should be alive
    let result2 = find_actor_helper(&node, actor2.id()).await;
    let result3 = find_actor_helper(&node, actor3.id()).await;
    
    assert!(result2.is_ok()); // actor2 should be alive (unlinked)
    assert!(result3.is_err() || matches!(result3, Ok(plexspaces_node::ActorLocation::Remote(_)))); // actor3 should be dead (still linked)
}

#[tokio::test]
async fn test_link_chain_cascading() {
    let node = create_test_node();
    
    let actor1 = create_test_actor_ref(&node, "actor-1").await;
    let actor2 = create_test_actor_ref(&node, "actor-2").await;
    let actor3 = create_test_actor_ref(&node, "actor-3").await;
    
    // Create chain: actor1 -> actor2 -> actor3
    node.link(actor1.id(), actor2.id()).await.unwrap();
    node.link(actor2.id(), actor3.id()).await.unwrap();
    
    // Kill actor1 abnormally
    unregister_actor_helper(&node, actor1.id()).await.unwrap();
    node.notify_actor_down(actor1.id(), "panic: test").await.unwrap();
    
    // Wait for cascading failure to propagate through the chain
    let cascading_complete = async {
        loop {
            let result1 = find_actor_helper(&node, actor1.id()).await;
            let result2 = find_actor_helper(&node, actor2.id()).await;
            let result3 = find_actor_helper(&node, actor3.id()).await;
            // All should be dead (error or remote)
            if (result1.is_err() || matches!(result1, Ok(plexspaces_node::ActorLocation::Remote(_))))
                && (result2.is_err() || matches!(result2, Ok(plexspaces_node::ActorLocation::Remote(_))))
                && (result3.is_err() || matches!(result3, Ok(plexspaces_node::ActorLocation::Remote(_))))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(5), cascading_complete)
        .await
        .expect("Cascading failure should propagate within 5 seconds");
    
    // All actors in chain should be dead
            let result1 = find_actor_helper(&node, actor1.id()).await;
            let result2 = find_actor_helper(&node, actor2.id()).await;
            let result3 = find_actor_helper(&node, actor3.id()).await;
    
    assert!(result1.is_err() || matches!(result1, Ok(plexspaces_node::ActorLocation::Remote(_))));
    assert!(result2.is_err() || matches!(result2, Ok(plexspaces_node::ActorLocation::Remote(_))));
    assert!(result3.is_err() || matches!(result3, Ok(plexspaces_node::ActorLocation::Remote(_))));
}

