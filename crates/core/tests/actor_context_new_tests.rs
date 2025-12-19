// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext::new() to improve coverage

use plexspaces_core::ActorContext;
use plexspaces_node::create_default_service_locator;
use std::sync::Arc;

#[tokio::test]
async fn test_actor_context_new() {
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let ctx = ActorContext::new(
        "test-node".to_string(),
        "test-ns".to_string(),
        "tenant-123".to_string(),
        service_locator,
        None,
    );

    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.namespace, "test-ns");
    assert_eq!(ctx.tenant_id, "tenant-123");
    assert!(ctx.metadata.is_empty());
    assert_eq!(ctx.config, None);
}

#[tokio::test]
async fn test_actor_context_new_with_config() {
    use plexspaces_proto::v1::actor::ActorConfig;

    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let mut config = ActorConfig::default();
    config.max_mailbox_size = 5000;
    config.enable_persistence = true;
    let config = Some(config.clone());

    let ctx = ActorContext::new(
        "test-node".to_string(),
        "test-ns".to_string(),
        "tenant-123".to_string(),
        service_locator,
        config.clone(),
    );

    assert_eq!(ctx.config, config);
    assert_eq!(ctx.config.as_ref().unwrap().max_mailbox_size, 5000);
    assert_eq!(ctx.config.as_ref().unwrap().enable_persistence, true);
    assert_eq!(ctx.tenant_id, "tenant-123");
}

#[tokio::test]
async fn test_actor_context_new_with_metadata() {
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let mut ctx = ActorContext::new(
        "test-node".to_string(),
        "test-ns".to_string(),
        "tenant-123".to_string(),
        service_locator,
        None,
    );

    ctx.metadata.insert("key1".to_string(), "value1".to_string());
    ctx.metadata.insert("key2".to_string(), "value2".to_string());

    assert_eq!(ctx.metadata.get("key1"), Some(&"value1".to_string()));
    assert_eq!(ctx.metadata.get("key2"), Some(&"value2".to_string()));
    assert_eq!(ctx.metadata.len(), 2);
    assert_eq!(ctx.tenant_id, "tenant-123");
}

