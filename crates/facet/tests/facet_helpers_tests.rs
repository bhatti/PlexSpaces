// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Tests for facet_helpers::create_facets_from_config
// Verifies multi-facet object, single virtual_actor key, and legacy flat object formats

use plexspaces_actor::service_locator_trait::ServiceLocator;
use plexspaces_facet::facet_helpers::create_facets_from_config;
use plexspaces_node::{Node, NodeBuilder};
use serde_json::{json, Value};
use std::sync::Arc;
use std::sync::OnceLock;

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

static SHARED_NODE: OnceLock<Arc<Node>> = OnceLock::new();
static INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static SERVICES_READY: OnceLock<()> = OnceLock::new();

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

    SHARED_NODE.get_or_init(|| node.clone()).clone()
}

async fn ensure_facet_helpers_services(node: &Node) {
    if SERVICES_READY.get().is_none() {
        node.initialize_services()
            .await
            .expect("Failed to initialize services");
        let _ = SERVICES_READY.set(());
    }
}

#[derive(Clone, Copy)]
enum FacetHelpersCase {
    MultiFacet,
    MultiFacetSkipsUnavailableProcessGroup,
    SingleVirtualActor,
    LegacyFlatObject,
    EmptyObject,
    NonObject,
    UnknownFacetSkipped,
}

fn case_config(case: FacetHelpersCase) -> Value {
    match case {
        FacetHelpersCase::MultiFacet => json!({
            "virtual_actor": {
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            },
            "durability": {
                "journal_storage": "memory"
            }
        }),
        FacetHelpersCase::MultiFacetSkipsUnavailableProcessGroup => json!({
            "virtual_actor": {
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            },
            "process_group": {
                "group": "abstractions-group"
            }
        }),
        FacetHelpersCase::SingleVirtualActor => json!({
            "virtual_actor": {
                "idle_timeout": "10m",
                "activation_strategy": "eager"
            }
        }),
        FacetHelpersCase::LegacyFlatObject => json!({
            "idle_timeout": "5m",
            "activation_strategy": "lazy"
        }),
        FacetHelpersCase::EmptyObject => json!({}),
        FacetHelpersCase::NonObject => json!(null),
        FacetHelpersCase::UnknownFacetSkipped => json!({
            "virtual_actor": {
                "idle_timeout": "5m"
            },
            "unknown_facet_type": {
                "some_config": "value"
            }
        }),
    }
}

fn assert_facets(case: FacetHelpersCase, facets: &[Box<dyn plexspaces_facet::Facet>]) {
    match case {
        FacetHelpersCase::MultiFacet => {
            assert_eq!(facets.len(), 2, "multi_facet");
            let types: Vec<String> = facets.iter().map(|f| f.facet_type().to_string()).collect();
            assert!(types.contains(&"virtual_actor".to_string()), "{types:?}");
            assert!(types.contains(&"durability".to_string()), "{types:?}");
        }
        FacetHelpersCase::MultiFacetSkipsUnavailableProcessGroup => {
            assert_eq!(facets.len(), 1, "multi_facet_skips_unavailable_process_group");
            let types: Vec<String> = facets.iter().map(|f| f.facet_type().to_string()).collect();
            assert!(types.contains(&"virtual_actor".to_string()), "{types:?}");
            assert!(
                !types.contains(&"process_group".to_string()),
                "{types:?}"
            );
        }
        FacetHelpersCase::SingleVirtualActor => {
            assert_eq!(facets.len(), 1, "single virtual_actor");
            assert_eq!(facets[0].facet_type(), "virtual_actor");
        }
        FacetHelpersCase::LegacyFlatObject => {
            assert_eq!(facets.len(), 1, "legacy flat");
            assert_eq!(facets[0].facet_type(), "virtual_actor");
        }
        FacetHelpersCase::EmptyObject => {
            assert_eq!(facets.len(), 0, "empty");
        }
        FacetHelpersCase::NonObject => {
            assert_eq!(facets.len(), 0, "non-object");
        }
        FacetHelpersCase::UnknownFacetSkipped => {
            assert_eq!(facets.len(), 1, "unknown skipped");
            assert_eq!(facets[0].facet_type(), "virtual_actor");
        }
    }
}

/// One node build and one `initialize_services` for all cases.
#[tokio::test]
async fn create_facets_from_config_table() {
    init_test_tracing();
    let node = get_shared_node().await;
    ensure_facet_helpers_services(&node).await;

    let service_locator = node.service_locator();
    let facet_registry = service_locator
        .facet_registry()
        .await
        .expect("FacetRegistry should be registered");

    for case in [
        FacetHelpersCase::MultiFacet,
        FacetHelpersCase::MultiFacetSkipsUnavailableProcessGroup,
        FacetHelpersCase::SingleVirtualActor,
        FacetHelpersCase::LegacyFlatObject,
        FacetHelpersCase::EmptyObject,
        FacetHelpersCase::NonObject,
        FacetHelpersCase::UnknownFacetSkipped,
    ] {
        let config = case_config(case);
        let facets = create_facets_from_config(&config, &facet_registry).await;
        assert_facets(case, &facets);
    }
}
