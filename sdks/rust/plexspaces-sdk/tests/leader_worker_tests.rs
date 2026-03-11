// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Tests for leader_worker helpers. Uses TestServiceLocatorStub (no NodeRegistry)
// to verify error handling. Full integration tests with real nodes live in
// crates/services or crates/node.

use plexspaces_actor::TestServiceLocatorStub;
use plexspaces_core::RequestContext;
use plexspaces_sdk::leader_worker;
use std::sync::Arc;

#[tokio::test]
async fn test_list_worker_node_ids_fails_when_node_registry_not_registered() {
    let ctx = RequestContext::new_without_auth("tenant".into(), "ns".into());
    let sl: Arc<dyn plexspaces_core::ServiceLocator> = Arc::new(TestServiceLocatorStub::new());

    let err = leader_worker::list_worker_node_ids(&ctx, sl, None, 100)
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("NodeRegistry not registered"),
        "expected NodeRegistry error, got: {}",
        err
    );
}
