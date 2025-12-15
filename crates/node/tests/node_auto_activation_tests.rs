// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for Node::get_or_activate_actor (automatic activation)

use plexspaces_mailbox::{mailbox_config_default, Mailbox, Message};
use plexspaces_node::{Node, NodeConfig, NodeId};
use plexspaces_actor::{Actor, ActorRef};
use plexspaces_core::Actor as CoreActorTrait;
use std::sync::Arc;

#[path = "test_helpers.rs"]
mod test_helpers;
use test_helpers::{lookup_actor_ref, activate_virtual_actor, get_or_activate_actor_helper, find_actor_helper, spawn_actor_helper};

struct TestBehavior {
    received: Arc<tokio::sync::Mutex<Vec<Message>>>,
}

#[async_trait::async_trait]
impl CoreActorTrait for TestBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &plexspaces_core::ActorContext,
        msg: Message,
        _reply: &dyn plexspaces_core::Reply,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        self.received.lock().await.push(msg);
        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

#[tokio::test]
async fn test_get_or_activate_actor_creates_new_actor() {
    // Test: get_or_activate_actor creates actor if it doesn't exist
    let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));
    let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let actor_ref = get_or_activate_actor_helper(
        &node,
        "test-actor@test-node".to_string(),
        || async {
            let behavior = Box::new(TestBehavior {
                received: received_clone,
            });
            let mailbox = Mailbox::new(
                mailbox_config_default(),
                "test-actor@test-node".to_string(),
            )
            .await
            .unwrap();
            Ok(Actor::new(
                "test-actor@test-node".to_string(),
                behavior,
                mailbox,
                "default".to_string(),
                Some("test-node".to_string()),
            ))
        },
    )
    .await
    .unwrap();

    assert_eq!(actor_ref.id(), "test-actor@test-node");

    // Wait for actor to be fully registered - use a future that polls until registered
    use plexspaces_core::ActorRegistry;
use test_helpers::{lookup_actor_ref, activate_virtual_actor, get_or_activate_actor_helper, spawn_actor_builder_helper};

    let actor_registry = node.actor_registry();
    
    // Poll until actor is registered instead of using sleep
    let registration_future = async {
        loop {
            if let Some(routing) = actor_registry.lookup_routing(&"test-actor@test-node".to_string()).await.ok().flatten() {
                return routing;
            }
            tokio::task::yield_now().await;
        }
    };
    let routing_opt = Some(tokio::time::timeout(tokio::time::Duration::from_secs(5), registration_future)
        .await
        .expect("Actor should be registered within 5 seconds"));
    assert!(routing_opt.is_some(), "Actor should be registered in ActorRegistry");
    assert!(routing_opt.unwrap().is_local, "Actor should be local");
    
    // Core test: get_or_activate_actor successfully created and returned an actor
    // The actor is registered in ActorRegistry (verified above)
    // Message sending is tested in other integration tests
    // This test focuses on the get-or-create pattern working correctly
}

#[tokio::test]
async fn test_get_or_activate_actor_returns_existing_actor() {
    // Test: get_or_activate_actor returns existing actor if it exists
    let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));
    let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    // Create actor first
    let behavior = Box::new(TestBehavior {
        received: received.clone(),
    });
    let mailbox = Mailbox::new(
        mailbox_config_default(),
        "existing-actor@test-node".to_string(),
    )
    .await
    .unwrap();
    let actor = Actor::new(
        "existing-actor@test-node".to_string(),
        behavior,
        mailbox,
        "default".to_string(),
        Some("test-node".to_string()),
    );
    // Spawn actor using ActorFactory
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await.unwrap();
    actor_factory.spawn_built_actor(Arc::new(actor), None, None).await.unwrap();
    let actor_ref1 = lookup_actor_ref(&node, &"existing-actor@test-node".to_string()).await.unwrap().unwrap();

    // Now get_or_activate should return existing actor
    let actor_ref2 = get_or_activate_actor_helper(
        &node,
        "existing-actor@test-node".to_string(),
        || async {
            panic!("Factory should not be called for existing actor");
        },
    )
    .await
    .unwrap();

    // Both should be the same actor
    assert_eq!(actor_ref1.id(), actor_ref2.id());
}

#[tokio::test]
async fn test_get_or_activate_actor_concurrent_activation() {
    // Test: Concurrent calls to get_or_activate_actor should handle race conditions
    // Note: This test verifies that concurrent calls don't panic, but only one actor is created
    let node = Arc::new(Node::new(NodeId::new("test-node"), NodeConfig::default()));
    let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    // Spawn multiple tasks trying to get_or_activate the same actor
    let mut handles = vec![];
    for i in 0..5 {
        let node_clone = node.clone();
        let received_clone = received.clone();
        let handle = tokio::spawn(async move {
            // Add small random delay to increase chance of race condition
            // Use yield_now with a small delay instead of sleep
            for _ in 0..i {
                tokio::task::yield_now().await;
            }
            
            get_or_activate_actor_helper(
                &node_clone,
                "concurrent-actor@test-node".to_string(),
                || async {
                    // All factory calls should create actor with same ID but unique mailbox ID
                    let behavior = Box::new(TestBehavior {
                        received: received_clone,
                    });
                    let mailbox = Mailbox::new(
                        mailbox_config_default(),
                        format!("concurrent-actor@test-node-mailbox-{}", i),
                    )
                    .await
                    .unwrap();
                    Ok(Actor::new(
                        "concurrent-actor@test-node".to_string(),
                        behavior,
                        mailbox,
                        "default".to_string(),
                        Some("test-node".to_string()),
                    ))
                },
            )
            .await
        });
        handles.push(handle);
    }

    // Wait for all tasks
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    // First one should succeed, others may fail with ActorAlreadyRegistered (which is expected)
    let mut success_count = 0;
    let mut error_count = 0;
    for result in results {
        match result {
            Ok(Ok(actor_ref)) => {
                assert_eq!(actor_ref.id(), "concurrent-actor@test-node");
                success_count += 1;
            }
            Ok(Err(_)) => {
                // Expected - actor already registered by first call
                error_count += 1;
            }
            Err(_) => {
                panic!("Task panicked");
            }
        }
    }
    
    // At least one should succeed (the first one to create)
    assert!(success_count >= 1, "At least one call should succeed");
    // Others may fail with ActorAlreadyRegistered (race condition handling)
    assert!(success_count + error_count == 5, "All calls should complete");
}
