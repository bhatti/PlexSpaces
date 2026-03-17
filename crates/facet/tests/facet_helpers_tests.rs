// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Tests for facet_helpers::create_facets_from_config
// Verifies multi-facet object, single virtual_actor key, and legacy flat object formats

use plexspaces_core::service_locator_trait::ServiceLocator;
use plexspaces_facet::facet_helpers::create_facets_from_config;
use plexspaces_node::{Node, NodeBuilder};
use serde_json::json;
use std::sync::Arc;
use std::sync::OnceLock;

// Initialize tracing for tests
static TRACING_INIT: std::sync::Once = std::sync::Once::new();

fn init_test_tracing() {
    TRACING_INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_test_writer()
            .try_init();
    });
}

/// Shared test node (created once, reused for all tests)
static SHARED_NODE: OnceLock<Arc<Node>> = OnceLock::new();
static INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Get or create shared test node
async fn get_shared_node() -> Arc<Node> {
    if let Some(node) = SHARED_NODE.get() {
        return node.clone();
    }

    let _guard = INIT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(node) = SHARED_NODE.get() {
        return node.clone();
    }

    let node = Arc::new(
        NodeBuilder::new("test-node-facet-helpers")
            .with_in_memory_backends()
            .build()
            .await,
    );

    use std::time::Duration;
    use tokio::task::yield_now;
    use tokio::time::sleep;
    for _ in 0..5 {
        yield_now().await;
        sleep(Duration::from_millis(10)).await;
    }

    SHARED_NODE.get_or_init(|| node.clone()).clone()
}

#[tokio::test]
async fn test_create_facets_from_config_multi_facet() {
    init_test_tracing();

    let node = get_shared_node().await;
    node.initialize_services()
        .await
        .expect("Failed to initialize services");

    let service_locator = node.service_locator();
    let facet_registry = service_locator
        .facet_registry()
        .await
        .expect("FacetRegistry should be registered");

    // Multi-facet config: virtual_actor + durability
    let facet_config = json!({
        "virtual_actor": {
            "idle_timeout": "5m",
            "activation_strategy": "lazy"
        },
        "durability": {
            "journal_storage": "memory"
        }
    });

    let facets = create_facets_from_config(&facet_config, &facet_registry).await;

    // Should create both facets
    assert_eq!(
        facets.len(),
        2,
        "Should create virtual_actor and durability facets"
    );

    let facet_types: Vec<String> = facets.iter().map(|f| f.facet_type().to_string()).collect();

    assert!(
        facet_types.contains(&"virtual_actor".to_string()),
        "Should contain virtual_actor facet. Found: {:?}",
        facet_types
    );
    assert!(
        facet_types.contains(&"durability".to_string()),
        "Should contain durability facet. Found: {:?}",
        facet_types
    );
}

#[tokio::test]
async fn test_create_facets_from_config_single_virtual_actor() {
    init_test_tracing();

    let node = get_shared_node().await;
    node.initialize_services()
        .await
        .expect("Failed to initialize services");

    let service_locator = node.service_locator();
    let facet_registry = service_locator
        .facet_registry()
        .await
        .expect("FacetRegistry should be registered");

    // Single virtual_actor key (keyed format)
    let facet_config = json!({
        "virtual_actor": {
            "idle_timeout": "10m",
            "activation_strategy": "eager"
        }
    });

    let facets = create_facets_from_config(&facet_config, &facet_registry).await;

    // Should create one facet
    assert_eq!(facets.len(), 1, "Should create one virtual_actor facet");
    assert_eq!(facets[0].facet_type(), "virtual_actor");
}

#[tokio::test]
async fn test_create_facets_from_config_legacy_flat_object() {
    init_test_tracing();

    let node = get_shared_node().await;
    node.initialize_services()
        .await
        .expect("Failed to initialize services");

    let service_locator = node.service_locator();
    let facet_registry = service_locator
        .facet_registry()
        .await
        .expect("FacetRegistry should be registered");

    // Legacy format: flat object (treated as virtual_actor config)
    // This is a JSON object but not keyed by facet type
    let facet_config = json!({
        "idle_timeout": "5m",
        "activation_strategy": "lazy"
    });

    let facets = create_facets_from_config(&facet_config, &facet_registry).await;

    // Should create one virtual_actor facet (legacy backward compat)
    assert_eq!(
        facets.len(),
        1,
        "Should create one virtual_actor facet from legacy format"
    );
    assert_eq!(facets[0].facet_type(), "virtual_actor");
}

#[tokio::test]
async fn test_create_facets_from_config_empty_object() {
    init_test_tracing();

    let node = get_shared_node().await;
    node.initialize_services()
        .await
        .expect("Failed to initialize services");

    let service_locator = node.service_locator();
    let facet_registry = service_locator
        .facet_registry()
        .await
        .expect("FacetRegistry should be registered");

    // Empty object
    let facet_config = json!({});

    let facets = create_facets_from_config(&facet_config, &facet_registry).await;

    // Should create no facets
    assert_eq!(facets.len(), 0, "Empty config should create no facets");
}

#[tokio::test]
async fn test_create_facets_from_config_non_object() {
    init_test_tracing();

    let node = get_shared_node().await;
    node.initialize_services()
        .await
        .expect("Failed to initialize services");

    let service_locator = node.service_locator();
    let facet_registry = service_locator
        .facet_registry()
        .await
        .expect("FacetRegistry should be registered");

    // Non-object value (should be ignored)
    let facet_config = json!(null);

    let facets = create_facets_from_config(&facet_config, &facet_registry).await;

    // Should create no facets
    assert_eq!(facets.len(), 0, "Non-object config should create no facets");
}

#[tokio::test]
async fn test_create_facets_from_config_unknown_facet_type() {
    init_test_tracing();

    let node = get_shared_node().await;
    node.initialize_services()
        .await
        .expect("Failed to initialize services");

    let service_locator = node.service_locator();
    let facet_registry = service_locator
        .facet_registry()
        .await
        .expect("FacetRegistry should be registered");

    // Unknown facet type (should be skipped, not fail)
    let facet_config = json!({
        "virtual_actor": {
            "idle_timeout": "5m"
        },
        "unknown_facet_type": {
            "some_config": "value"
        }
    });

    let facets = create_facets_from_config(&facet_config, &facet_registry).await;

    // Should create only virtual_actor facet (unknown type skipped)
    assert_eq!(facets.len(), 1, "Should create only known facet types");
    assert_eq!(facets[0].facet_type(), "virtual_actor");
}
