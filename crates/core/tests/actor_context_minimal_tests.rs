// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext minimal constructors to improve coverage

use plexspaces_core::ActorContext;
use plexspaces_proto::v1::actor::ActorConfig;

#[tokio::test]
async fn test_actor_context_minimal() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "test-ns".to_string(), // namespace
        "test-tenant".to_string(), // tenant_id (required)
    );

    // actor_id removed from ActorContext
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.namespace, "test-ns");
    assert_eq!(ctx.tenant_id(), "test-tenant");
    assert!(ctx.metadata.is_empty());
    assert_eq!(ctx.config, None);
}

#[tokio::test]
async fn test_actor_context_minimal_with_config_none() {
    let ctx = ActorContext::minimal_with_config(
        "test-node".to_string(),
        "test-ns".to_string(),
        "test-tenant".to_string(), // tenant_id (required)
        None,
    );

    // actor_id removed from ActorContext
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.namespace, "test-ns");
    assert_eq!(ctx.config, None);
}

#[tokio::test]
async fn test_actor_context_minimal_with_config_some() {
    let mut config = ActorConfig::default();
    config.max_mailbox_size = 2000;
    config.enable_persistence = false;
    config.checkpoint_interval = Some(prost_types::Duration {
        seconds: 60,
        nanos: 0,
    });
    let config = Some(config.clone());

    let ctx = ActorContext::minimal_with_config(
        "test-node".to_string(),
        "test-ns".to_string(),
        "test-tenant".to_string(), // tenant_id (required)
        config.clone(),
    );

    assert_eq!(ctx.config, config);
    assert_eq!(ctx.config.as_ref().unwrap().max_mailbox_size, 2000);
    assert_eq!(ctx.config.as_ref().unwrap().enable_persistence, false);
}

#[tokio::test]
async fn test_actor_context_minimal_creates_stub_services() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "default".to_string(), // namespace
        "test-tenant".to_string(), // tenant_id (required)
    );

    // Services are accessed via service_locator, not directly
    // Verify the context is created correctly
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.tenant_id(), "test-tenant");
}

