// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for behavior registration patterns (embedded and WASM)

use async_trait::async_trait;
use plexspaces_core::{Actor, BehaviorFactory, BehaviorRegistry, Message, ServiceLocator};
use plexspaces_node::NodeBuilder;
use std::sync::Arc;

/// Test that embedded applications can register behaviors explicitly
///
/// ## Purpose
/// Verifies that native Rust applications can register behaviors explicitly
/// using BehaviorRegistry, which is the pattern for embedded apps.
#[tokio::test]
async fn test_embedded_application_explicit_behavior_registration() {
    // Create a test node
    let node = NodeBuilder::new("test-node-embedded-registration")
        .build()
        .await;

    // Create a simple test behavior
    struct TestWorker {
        id: String,
    }

    #[async_trait]
    impl Actor for TestWorker {
        async fn handle_message(
            &mut self,
            _ctx: &plexspaces_core::ActorContext,
            _msg: Message,
        ) -> Result<(), plexspaces_core::BehaviorError> {
            Ok(())
        }

        fn behavior_type(&self) -> plexspaces_core::BehaviorType {
            plexspaces_core::BehaviorType::GenServer
        }
    }

    // Register behavior explicitly (embedded app pattern - async-only)
    let behavior_registry = BehaviorRegistry::new();
    behavior_registry
        .register_simple("worker", || {
            Box::pin(async move {
                Ok(Box::new(TestWorker {
                    id: "worker-1".to_string(),
                }) as Box<dyn plexspaces_core::Actor>)
            })
        })
        .await;

    // Register with ServiceLocator
    let service_locator = node.service_locator();
    service_locator
        .register_behavior_registry(Arc::new(behavior_registry))
        .await;

    // Verify behavior is registered
    let registry_opt = service_locator.get_behavior_registry().await;
    assert!(
        registry_opt.is_some(),
        "BehaviorRegistry should be registered"
    );

    let registry = registry_opt.unwrap();
    assert!(
        registry.is_registered("worker").await,
        "worker behavior should be registered"
    );
    assert!(
        !registry.is_registered("unknown").await,
        "unknown behavior should not be registered"
    );

    // Verify behavior can be created
    let behavior_result = registry.create("worker", &[]).await;
    assert!(
        behavior_result.is_ok(),
        "Should be able to create worker behavior"
    );
}

/// Test that BehaviorRegistry supports Arc (interior mutability)
///
/// ## Purpose
/// Verifies that BehaviorRegistry methods work with Arc, enabling sharing
/// across multiple contexts without requiring mutable access.
#[tokio::test]
async fn test_behavior_registry_arc_interior_mutability() {
    struct TestActor {
        name: String,
    }

    #[async_trait]
    impl Actor for TestActor {
        async fn handle_message(
            &mut self,
            _ctx: &plexspaces_core::ActorContext,
            _msg: Message,
        ) -> Result<(), plexspaces_core::BehaviorError> {
            Ok(())
        }

        fn behavior_type(&self) -> plexspaces_core::BehaviorType {
            plexspaces_core::BehaviorType::GenServer
        }
    }

    // Create registry and wrap in Arc
    let registry = Arc::new(BehaviorRegistry::new());

    // Register behaviors using Arc (no mutable access needed, async-only)
    registry
        .register_simple("actor1", || {
            Box::pin(async move {
                Ok(Box::new(TestActor {
                    name: "actor1".to_string(),
                }) as Box<dyn plexspaces_core::Actor>)
            })
        })
        .await;

    registry
        .register_simple("actor2", || {
            Box::pin(async move {
                Ok(Box::new(TestActor {
                    name: "actor2".to_string(),
                }) as Box<dyn plexspaces_core::Actor>)
            })
        })
        .await;

    // Verify behaviors are registered
    assert!(registry.is_registered("actor1").await);
    assert!(registry.is_registered("actor2").await);

    // Verify behaviors can be created
    let behavior1_result = registry.create("actor1", &[]).await;
    assert!(behavior1_result.is_ok());

    let behavior2_result = registry.create("actor2", &[]).await;
    assert!(behavior2_result.is_ok());

    // Verify registered_modules() works
    let modules = registry.registered_modules().await;
    assert_eq!(modules.len(), 2);
    assert!(modules.contains(&"actor1".to_string()));
    assert!(modules.contains(&"actor2".to_string()));
}
