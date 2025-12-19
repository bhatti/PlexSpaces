// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext tenant_id support

use plexspaces_core::{ActorContext, ServiceLocator};
use std::sync::Arc;

#[tokio::test]
async fn test_actor_context_new_with_tenant_id() {
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("node1".to_string()), None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "production".to_string(),
        "tenant-123".to_string(),
        service_locator,
        None,
    );

    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.namespace, "production");
    assert_eq!(ctx.tenant_id, "tenant-123");
}

#[tokio::test]
async fn test_actor_context_tenant_id_getter() {
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("node1".to_string()), None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "production".to_string(),
        "tenant-789".to_string(),
        service_locator,
        None,
    );

    assert_eq!(ctx.tenant_id, "tenant-789");
}

#[tokio::test]
async fn test_actor_context_clone_preserves_tenant_id() {
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("node1".to_string()), None, None).await;
    let ctx1 = ActorContext::new(
        "node1".to_string(),
        "production".to_string(),
        "tenant-123".to_string(),
        service_locator,
        None,
    );

    let ctx2 = ctx1.clone();

    assert_eq!(ctx1.tenant_id, ctx2.tenant_id);
    assert_eq!(ctx1.tenant_id, "tenant-123");
    assert_eq!(ctx2.tenant_id, "tenant-123");
}

