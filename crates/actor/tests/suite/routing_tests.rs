// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Tests for Unified Routing Module (`crates/actor/src/routing.rs`)
//!
//! Tests routing helpers that can be used by both ActorRef and ActorService:
//! - extract_node_id(): Extract node_id from actor ID
//! - is_actor_local(): Determine if actor is local
//! - ask_helper(): Generic ask pattern implementation

use plexspaces_actor::{
    routing::{extract_node_id, is_actor_local, route_message},
    ActorRefError,
};
use plexspaces_core::{ActorRegistry, RequestContext, ServiceLocator};
use plexspaces_node::NodeBuilder;
use std::sync::Arc;

// ========================================================================
// extract_node_id() Tests
// ========================================================================

#[test]
fn test_extract_node_id_with_node_suffix() {
    let (name, node_id) = extract_node_id("actor-name@node-123");
    assert_eq!(name, "actor-name");
    assert_eq!(node_id, Some("node-123".to_string()));
}

#[test]
fn test_extract_node_id_without_node_suffix() {
    let (name, node_id) = extract_node_id("actor-name");
    assert_eq!(name, "actor-name");
    assert_eq!(node_id, None);
}

#[test]
fn test_extract_node_id_empty_string() {
    let (name, node_id) = extract_node_id("");
    assert_eq!(name, "");
    assert_eq!(node_id, None);
}

#[test]
fn test_extract_node_id_multiple_at_signs() {
    // Should only split on first @
    let (name, node_id) = extract_node_id("actor@name@node-123");
    assert_eq!(name, "actor");
    assert_eq!(node_id, Some("name@node-123".to_string()));
}

#[test]
fn test_extract_node_id_only_at_sign() {
    let (name, node_id) = extract_node_id("@node-123");
    assert_eq!(name, "");
    assert_eq!(node_id, Some("node-123".to_string()));
}

// ========================================================================
// is_actor_local() Tests
// ========================================================================

#[tokio::test]
async fn test_is_actor_local_with_matching_node_id() {
    let node = NodeBuilder::new("test-node-1").build().await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();

    // Actor ID with matching node_id
    let is_local = is_actor_local("actor@test-node-1", &service_locator).await;
    assert!(is_local, "Actor with matching node_id should be local");
}

#[tokio::test]
async fn test_is_actor_local_with_different_node_id() {
    let node = NodeBuilder::new("test-node-1").build().await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();

    // Actor ID with different node_id
    let is_local = is_actor_local("actor@test-node-2", &service_locator).await;
    assert!(!is_local, "Actor with different node_id should be remote");
}

#[tokio::test]
async fn test_is_actor_local_without_node_suffix() {
    let node = NodeBuilder::new("test-node-1").build().await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();

    // Actor ID without @node suffix
    let is_local = is_actor_local("actor", &service_locator).await;
    // Should check if actor exists locally
    // Since actor doesn't exist, should return false
    assert!(!is_local, "Actor without node_id should check registry");
}

#[tokio::test]
async fn test_route_message_without_node_suffix_stays_local() {
    let node = NodeBuilder::new("test-node-1").build().await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    let message = plexspaces_mailbox::new_message(br#"{"op":"status"}"#.to_vec());

    let result = route_message(
        ctx,
        service_locator,
        "webhook:default".to_string(),
        message,
        false,
        None,
    )
    .await;

    match result {
        Err(ActorRefError::ActorNotFound(actor_id)) => {
            assert!(
                actor_id.starts_with("webhook:default@"),
                "local routing should normalize the actor id with the local node suffix"
            );
        }
        other => panic!("expected local ActorNotFound, got {:?}", other),
    }
}

#[tokio::test]
async fn test_route_message_with_explicit_local_node_suffix_stays_local() {
    let node = NodeBuilder::new("test-node-1").build().await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    let message = plexspaces_mailbox::new_message(br#"{"op":"status"}"#.to_vec());

    let result = route_message(
        ctx,
        service_locator,
        "webhook:default@test-node-1".to_string(),
        message,
        false,
        None,
    )
    .await;

    match result {
        Err(ActorRefError::ActorNotFound(actor_id)) => {
            assert_eq!(actor_id, "webhook:default@test-node-1");
        }
        other => panic!("expected local ActorNotFound, got {:?}", other),
    }
}

#[tokio::test]
async fn test_is_actor_local_registered_locally() {
    let node = NodeBuilder::new("test-node-1").build().await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();
    let registry = service_locator.actor_registry().await.unwrap();

    // Register actor locally (without @node suffix)
    use async_trait::async_trait;
    use plexspaces_actor::{ActorBuilder, ActorRef};
    use plexspaces_behavior::GenServer;
    use plexspaces_core::Actor as ActorTrait;
    use plexspaces_mailbox::{Mailbox, MailboxConfig};

    struct TestActor;

    #[async_trait]
    impl ActorTrait for TestActor {
        async fn handle_message(
            &mut self,
            _ctx: &plexspaces_core::ActorContext,
            _msg: plexspaces_core::Message,
        ) -> Result<(), plexspaces_core::BehaviorError> {
            Ok(())
        }

        fn behavior_type(&self) -> plexspaces_core::BehaviorType {
            plexspaces_core::BehaviorType::GenServer
        }
    }

    #[async_trait]
    impl GenServer for TestActor {
        async fn handle_request(
            &mut self,
            _ctx: &plexspaces_core::ActorContext,
            _msg: plexspaces_core::Message,
        ) -> Result<(), plexspaces_core::BehaviorError> {
            Ok(())
        }
    }

    let actor_id = "local-actor".to_string();
    let mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), actor_id.clone())
            .await
            .unwrap(),
    );
    let actor_ref = ActorRef::local(
        actor_id.clone(),
        "test".to_string(),
        "default".to_string(),
        mailbox,
        service_locator.clone(),
    );

    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            Arc::new(actor_ref),
            "TestActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    // Now check if actor is local (should be true since it's registered)
    let is_local = is_actor_local("local-actor", &service_locator).await;
    assert!(
        is_local,
        "Actor registered locally should be detected as local"
    );
}

#[tokio::test]
async fn test_is_actor_local_no_node_config() {
    // Create service locator without NodeConfig (testing fallback)
    let node = NodeBuilder::new("test-node-1").build().await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();

    // Actor ID with matching node_id (should use ActorRegistry fallback)
    let is_local = is_actor_local("actor@test-node-1", &service_locator).await;
    // Should use ActorRegistry.local_node_id() as fallback
    assert!(
        is_local,
        "Should use ActorRegistry fallback when NodeConfig not available"
    );
}

// ========================================================================
// Edge Case Tests
// ========================================================================

#[tokio::test]
async fn test_is_actor_local_empty_actor_id() {
    let node = NodeBuilder::new("test-node-1").build().await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();

    let is_local = is_actor_local("", &service_locator).await;
    assert!(!is_local, "Empty actor ID should not be local");
}

#[tokio::test]
async fn test_is_actor_local_special_characters() {
    let node = NodeBuilder::new("test-node-1").build().await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();

    // Test with special characters
    let is_local = is_actor_local("actor-name@test-node-1", &service_locator).await;
    assert!(is_local, "Actor ID with hyphens should work");

    let is_local = is_actor_local("actor_name@test-node-1", &service_locator).await;
    assert!(is_local, "Actor ID with underscores should work");
}

#[tokio::test]
async fn test_is_actor_local_case_sensitivity() {
    let node = NodeBuilder::new("test-node-1").build().await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();

    // Node IDs are case-sensitive
    let is_local = is_actor_local("actor@TEST-NODE-1", &service_locator).await;
    assert!(!is_local, "Node ID comparison should be case-sensitive");
}
