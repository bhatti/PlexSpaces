// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
//! Tests for get_or_activate_actor (automatic activation pattern)

use plexspaces_actor::Actor;
use plexspaces_behavior::MockBehavior;
use plexspaces_core::{ActorId, ActorRef};
use plexspaces_mailbox::{mailbox_config_default, Mailbox};
use plexspaces_node::{Node, NodeId, default_node_config};
use std::sync::Arc;
#[path = "test_helpers.rs"]
#[path = "test_helpers.rs"]
mod test_helpers;
use test_helpers::{lookup_actor_ref, activate_virtual_actor, get_or_activate_actor_helper, spawn_actor_builder_helper, find_actor_helper, spawn_actor_helper};


#[tokio::test]
async fn test_get_or_activate_actor_new_actor() {
    // Test: Creating a new actor when it doesn't exist
    let node = NodeBuilder::new("test-node").build();
    let node_id = node.id().clone();
    
    let actor_id: ActorId = format!("test-actor@{}", node_id.as_str()).into();
    
    // Get or activate actor (should create new one)
    let actor_ref = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            // Create actor
            let behavior = Box::new(MockBehavior::new());
            let mailbox = Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()))
                .await
                .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id.clone().into(), e.to_string()))?;
            
            let actor = Actor::new(
                actor_id.clone().into(),
                behavior,
                mailbox,
                "default".to_string(),
                None,
            );
            
            Ok(actor)
        },
    ).await.unwrap();
    
    // Verify actor was created
    assert_eq!(actor_ref.id(), &actor_id);
    
    // Verify actor exists in registry
    let location = find_actor_helper(&node, &actor_id).await.unwrap();
    match location {
        plexspaces_node::ActorLocation::Local(_) => {
            // Expected
        }
        _ => panic!("Expected local actor"),
    }
    
    // Additional verification: Check ActorRegistry registration
    use plexspaces_core::ActorRegistry;
    let actor_registry = node.actor_registry();
    let routing = actor_registry.lookup_routing(&actor_id.to_string())
        .await
        .ok()
        .flatten();
    assert!(routing.is_some(), "Actor should be registered in ActorRegistry");
    assert!(routing.as_ref().unwrap().is_local, "Actor should be local");
}

#[tokio::test]
async fn test_get_or_activate_actor_existing_actor() {
    // Test: Returning existing actor when it already exists
    let node = NodeBuilder::new("test-node").build();
    let node_id = node.id().clone();
    
    let actor_id: ActorId = format!("test-actor@{}", node_id.as_str()).into();
    
    // First, spawn an actor
    let behavior1 = Box::new(MockBehavior::new());
    let mailbox1 = Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new()))
        .await.unwrap();
    let actor1 = Actor::new(
        actor_id.clone().into(),
        behavior1,
        mailbox1,
        "default".to_string(),
        None,
    );
    let actor_ref1 = spawn_actor_helper(&node, actor1).await.unwrap();
    
    // Now get or activate (should return existing)
    let mut factory_called = false;
    let actor_ref2 = get_or_activate_actor_helper(&node, 
        actor_id.clone(),
        || async {
            factory_called = true; // Should not be called
            Err(plexspaces_node::NodeError::ActorRegistrationFailed(
                actor_id.clone().into(),
                "Should not be called".to_string(),
            ))
        },
    ).await.unwrap();
    
    // Verify factory was not called
    assert!(!factory_called, "Factory should not be called for existing actor");
    
    // Verify both refs point to same actor
    assert_eq!(actor_ref1.id(), actor_ref2.id());
}

#[tokio::test]
async fn test_get_or_activate_actor_concurrent_activation() {
    // Test: Concurrent get_or_activate calls should handle race conditions
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let node_id = node.id().clone();
    
    let actor_id: ActorId = format!("test-actor@{}", node_id.as_str()).into();
    
    // Spawn multiple concurrent get_or_activate calls
    let mut handles = Vec::new();
    for i in 0..5 {
        let node_clone = node.clone();
        let actor_id_clone = actor_id.clone();
        let handle = tokio::spawn(async move {
            get_or_activate_actor_helper(
                &node_clone,
                actor_id_clone.clone(),
                || async {
                    // Create actor
                    let behavior = Box::new(MockBehavior::new());
                    let mailbox = Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}-{}", i, ulid::Ulid::new()))
                        .await
                        .map_err(|e| plexspaces_node::NodeError::ActorRegistrationFailed(actor_id_clone.clone().into(), e.to_string()))?;
                    
                    let actor = Actor::new(
                        actor_id_clone.clone().into(),
                        behavior,
                        mailbox,
                        "default".to_string(),
                        None,
                    );
                    
                    Ok(actor)
                },
            )
            .await
        });
        handles.push(handle);
    }
    
    // Wait for all to complete
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }
    
    // All should succeed and return same actor
    let first_ref = results[0].as_ref().unwrap();
    for result in results.iter().skip(1) {
        assert!(result.is_ok());
        assert_eq!(result.as_ref().unwrap().id(), first_ref.id());
    }
    
    // Verify only one actor was created
    let location = find_actor_helper(&node, &actor_id).await.unwrap();
    match location {
        plexspaces_node::ActorLocation::Local(_) => {
            // Expected - only one actor should exist
        }
        _ => panic!("Expected local actor"),
    }
}
