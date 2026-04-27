// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

use plexspaces_actor::{routing::is_actor_local, routing::route_message, ActorRefError};
use plexspaces_core::{ActorId, RequestContext, ServiceLocator};
use plexspaces_node::NodeBuilder;
use std::sync::Arc;

fn actor_id(name: &str, node_id: &str) -> ActorId {
    ActorId::new(name, "worker", "default", node_id).unwrap()
}

#[tokio::test]
async fn is_actor_local_returns_true_for_matching_node() {
    let node = NodeBuilder::new("test-node-1")
        .with_in_memory_backends()
        .build()
        .await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();

    assert!(is_actor_local(&actor_id("actor", "test-node-1"), &service_locator).await);
}

#[tokio::test]
async fn is_actor_local_returns_false_for_different_node() {
    let node = NodeBuilder::new("test-node-1")
        .with_in_memory_backends()
        .build()
        .await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();

    assert!(!is_actor_local(&actor_id("actor", "test-node-2"), &service_locator).await);
}

#[tokio::test]
async fn route_message_uses_canonical_local_id() {
    let node = NodeBuilder::new("test-node-1")
        .with_in_memory_backends()
        .build()
        .await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    let message = plexspaces_mailbox::new_message(br#"{"op":"status"}"#.to_vec());
    let target = actor_id("webhook", "test-node-1").to_string();

    let result = route_message(ctx, service_locator, target.clone(), message, false, None).await;

    match result {
        Err(ActorRefError::ActorNotFound(actor_id)) => assert_eq!(actor_id.to_string(), target),
        other => panic!("expected local ActorNotFound, got {:?}", other),
    }
}

#[tokio::test]
async fn route_message_rejects_non_canonical_ids() {
    let node = NodeBuilder::new("test-node-1")
        .with_in_memory_backends()
        .build()
        .await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    let message = plexspaces_mailbox::new_message(br#"{"op":"status"}"#.to_vec());

    let result = route_message(
        ctx,
        service_locator,
        "webhook@test-node-1".to_string(),
        message,
        false,
        None,
    )
    .await;

    assert!(matches!(result, Err(ActorRefError::InvalidActorId(_))));
}

#[tokio::test]
async fn is_actor_local_preserves_case_sensitive_node_matching() {
    let node = NodeBuilder::new("test-node-1")
        .with_in_memory_backends()
        .build()
        .await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();

    assert!(!is_actor_local(&actor_id("actor", "TEST-NODE-1"), &service_locator).await);
}
