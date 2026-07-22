// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

use plexspaces_actor::{ActorId, RequestContext, RequestContextExt, ServiceLocator};
use plexspaces_node::NodeBuilder;
use std::sync::Arc;

fn actor_id(name: &str, node_id: &str) -> ActorId {
    ActorId::new(name, "worker", "default", node_id).unwrap()
}

#[tokio::test]
async fn actor_registry_tell_local_actor_not_found_returns_error() {
    let node = NodeBuilder::new("test-node-1")
        .with_in_memory_backends()
        .build()
        .await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();
    let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
    let message = plexspaces_mailbox::new_message(br#"{"op":"status"}"#.to_vec());
    let target = actor_id("webhook", "test-node-1");

    let registry = service_locator.actor_registry().await.unwrap();
    let result = registry.tell(&ctx, &target, message).await;
    assert!(result.is_err(), "expected error for unknown local actor");
}

#[tokio::test]
async fn actor_id_node_id_comparison_is_case_sensitive() {
    let node = NodeBuilder::new("test-node-1")
        .with_in_memory_backends()
        .build()
        .await;
    let service_locator: Arc<dyn ServiceLocator> = node.service_locator();
    let registry = service_locator.actor_registry().await.unwrap();

    // Matching node_id: is_on_node should be true
    assert!(actor_id("actor", "test-node-1").is_on_node(registry.local_node_id()));
    // Different node_id: is_on_node should be false
    assert!(!actor_id("actor", "test-node-2").is_on_node(registry.local_node_id()));
    // Case difference: is_on_node should be false
    assert!(!actor_id("actor", "TEST-NODE-1").is_on_node(registry.local_node_id()));
}
