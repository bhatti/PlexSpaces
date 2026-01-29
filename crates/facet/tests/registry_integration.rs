// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for RegistryFacet with real actors (Rust and WASM)
// Tests verify that RegistryFacet correctly intercepts registry operations and uses
// the ObjectRegistry from ServiceLocator (based on node config).

use plexspaces_actor::ActorRef;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId};
use plexspaces_facet::capabilities::registry::RegistryFacet;
use plexspaces_mailbox::Message;
use plexspaces_node::{Node, NodeBuilder};
use serde_json::json;
use std::sync::Arc;
use std::sync::OnceLock;
use plexspaces_mailbox::Mailbox;

// Initialize tracing for tests (if not already initialized)
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
/// 
/// ## Purpose
/// Creates a test node with services initialized but without gRPC server.
/// This is sufficient for facet integration tests that don't need network communication.
/// 
/// ## Pattern
/// Follows the same pattern as other integration tests in the codebase:
/// - Build node with in-memory backends
/// - Initialize services (already done in build())
/// - Wait briefly for services to be ready
/// - Reuse node across tests for efficiency
async fn get_shared_node() -> Arc<Node> {
    if let Some(node) = SHARED_NODE.get() {
        return node.clone();
    }

    // Use a lock to ensure only one thread initializes
    // Handle poison errors gracefully (if a test panicked while holding the lock)
    let _guard = INIT_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    // Double-check after acquiring lock
    if let Some(node) = SHARED_NODE.get() {
        return node.clone();
    }

    // Create node with in-memory backends (for testing)
    // build() already initializes services, so we don't need to call initialize_services()
    let node = Arc::new(
        NodeBuilder::new("test-node-registry")
            .with_in_memory_backends()
            .build()
            .await
    );

    // Wait for services to be ready with polling (no gRPC server startup needed)
    use tokio::task::yield_now;
    use std::time::Duration;
    use tokio::time::sleep;
    for _ in 0..5 {
        yield_now().await;
        sleep(Duration::from_millis(10)).await;
    }

    SHARED_NODE.get_or_init(|| node.clone()).clone()
}

/// Helper to get ActorRef after spawning an actor
/// 
/// ## Purpose
/// Waits for actor to be registered and creates ActorRef for ask() pattern.
/// For local actors, uses ActorRef::local() with mailbox.
/// For remote actors, uses ActorRef::remote().
async fn get_actor_ref_after_spawn(
    node: &Node,
    actor_id: &ActorId,
) -> ActorRef {
    let actor_registry = node.service_locator().actor_registry().await
        .expect("ActorRegistry should be available");
    
    // Wait for actor to be registered (async registration)
    let node_id = node.id().as_str().to_string();
    for _ in 0..20 {
        // Check if actor exists in registry
        if actor_registry.lookup_actor(actor_id).await.is_some() {
            // Actor is registered - create ActorRef::local() with a new mailbox
            // Note: We create a new mailbox for ActorRef even though the actor has its own.
            // This is a test pattern that works because ActorRef::local() uses the mailbox
            // for reply routing, and the actual actor mailbox is used for receiving messages.
            use plexspaces_mailbox::{mailbox_config_default, Mailbox};
            let mailbox_for_ref = Arc::new(
                Mailbox::new(
                    mailbox_config_default(),
                    format!("ref-{}", actor_id),
                )
                .await
                .expect("Failed to create mailbox for ActorRef"),
            );
            return ActorRef::local(
                actor_id.clone(),
                String::new(), // Test namespace
                mailbox_for_ref,
                node.service_locator().clone(),
            );
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
    panic!("Actor {} should be registered after spawning", actor_id);
}

/// Adapter for ObjectRegistry trait
struct ObjectRegistryAdapter {
    inner: Arc<dyn plexspaces_core::ObjectRegistry>,
}

#[async_trait::async_trait]
impl plexspaces_facet::capabilities::registry::ObjectRegistry for ObjectRegistryAdapter {
    async fn register(
        &self,
        ctx: &plexspaces_common::RequestContext,
        registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> Result<(), String> {
        self.inner
            .register(ctx, registration)
            .await
            .map_err(|e| e.to_string())
    }

    async fn unregister(
        &self,
        ctx: &plexspaces_common::RequestContext,
        object_id: &str,
        object_type: Option<String>,
    ) -> Result<(), String> {
        let object_type_enum = object_type
            .as_ref()
            .map(|s| match s.as_str() {
                "Actor" | "actor" => {
                    plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor
                }
                "TupleSpace" | "tuplespace" => {
                    plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace
                }
                "Service" | "service" => {
                    plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService
                }
                _ => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            })
            .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor);

        self.inner
            .unregister(ctx, object_type_enum, object_id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn lookup(
        &self,
        ctx: &plexspaces_common::RequestContext,
        object_id: &str,
        object_type: Option<String>,
    ) -> Result<Option<plexspaces_proto::object_registry::v1::ObjectRegistration>, String> {
        let object_type_enum = object_type.as_ref().map(|s| match s.as_str() {
            "Actor" | "actor" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            "TupleSpace" | "tuplespace" => {
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace
            }
            "Service" | "service" => {
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService
            }
            _ => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
        });

        self.inner
            .lookup(ctx, object_id, object_type_enum)
            .await
            .map_err(|e| e.to_string())
    }

    async fn discover(
        &self,
        ctx: &plexspaces_common::RequestContext,
        object_type: Option<String>,
        name: Option<String>,
        labels: Option<Vec<String>>,
        health_status: Option<String>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>, String> {
        let object_type_enum = object_type.as_ref().map(|s| match s.as_str() {
            "Actor" | "actor" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            "TupleSpace" | "tuplespace" => {
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace
            }
            "Service" | "service" => {
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService
            }
            _ => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
        });

        let health_status_enum = health_status.as_ref().map(|s| match s.as_str() {
            "Healthy" | "healthy" => {
                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy
            }
            "Unhealthy" | "unhealthy" => {
                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnhealthy
            }
            "Unknown" | "unknown" => {
                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnknown
            }
            _ => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnknown,
        });

        self.inner
            .discover(
                ctx,
                object_type_enum,
                name,
                labels,
                None, // exclude_labels
                health_status_enum,
                limit,
                offset,
            )
            .await
            .map_err(|e| e.to_string())
    }
}

/// Test: Rust actor with RegistryFacet - register and lookup
#[tokio::test]
async fn test_rust_actor_registry_facet_register_lookup() {
    init_test_tracing();
    let node = get_shared_node().await;

    let service_locator = node.service_locator();
    let object_registry = service_locator
        .get_object_registry()
        .await
        .expect("ObjectRegistry should be registered");

    let registry_facet = RegistryFacet::new(
        Arc::new(ObjectRegistryAdapter {
            inner: object_registry,
        }),
        json!({}),
        50,
    );

    let node_id = node.id();
    let actor_name = format!("test-actor-{}", ulid::Ulid::new());
    let actor_id = format!("{}@{}", actor_name, node_id);
    let actor_id_typed = ActorId::from(actor_id.clone());
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "test-namespace".to_string(),
    );

    node.spawn(
        &ctx,
        &actor_id_typed,
        "GenServer",
        vec![],
        None,
        std::collections::HashMap::new(),
        vec![Box::new(registry_facet) as Box<dyn plexspaces_facet::Facet>],
    )
    .await
    .expect("Failed to spawn actor");

    let actor_ref = get_actor_ref_after_spawn(&node, &actor_id_typed).await;

    // ACT: Register object
    let register_msg = Message::json(&json!({
        "object_id": "service-1",
        "object_type": "Service",
        "grpc_address": "http://service-1:50051",
        "metadata": {"version": "1.0.0"}
    }))
    .expect("Failed to create message")
    .with_message_type("register_object");

    let reply = actor_ref
        .ask(register_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to register object");

    // ASSERT: Should receive success
    let response: serde_json::Value = serde_json::from_slice(&reply.payload)
        .expect("Failed to parse response");
    assert_eq!(response["status"], "ok");

    // ACT: Lookup object
    let lookup_msg = Message::json(&json!({
        "object_id": "service-1",
        "object_type": "Service"
    }))
    .expect("Failed to create message")
    .with_message_type("lookup_object");

    let reply = actor_ref
        .ask(lookup_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to lookup object");

    // ASSERT: Should return object
    let response: serde_json::Value = serde_json::from_slice(&reply.payload)
        .expect("Failed to parse response");
    assert_eq!(response["object_id"], "service-1");
    assert_eq!(response["grpc_address"], "http://service-1:50051");
}

/// Test: Rust actor with RegistryFacet - unregister
#[tokio::test]
async fn test_rust_actor_registry_facet_unregister() {
    init_test_tracing();
    let node = get_shared_node().await;

    let service_locator = node.service_locator();
    let object_registry = service_locator
        .get_object_registry()
        .await
        .expect("ObjectRegistry should be registered");

    let registry_facet = RegistryFacet::new(
        Arc::new(ObjectRegistryAdapter {
            inner: object_registry,
        }),
        json!({}),
        50,
    );

    let node_id = node.id();
    let actor_name = format!("test-actor-{}", ulid::Ulid::new());
    let actor_id = format!("{}@{}", actor_name, node_id);
    let actor_id_typed = ActorId::from(actor_id.clone());
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "test-namespace".to_string(),
    );

    node.spawn(
        &ctx,
        &actor_id_typed,
        "GenServer",
        vec![],
        None,
        std::collections::HashMap::new(),
        vec![Box::new(registry_facet) as Box<dyn plexspaces_facet::Facet>],
    )
    .await
    .expect("Failed to spawn actor");

    let actor_ref = get_actor_ref_after_spawn(&node, &actor_id_typed).await;

    // ARRANGE: Register object first
    let register_msg = Message::json(&json!({
        "object_id": "service-2",
        "object_type": "Service",
        "grpc_address": "http://service-2:50051"
    }))
    .expect("Failed to create message")
    .with_message_type("register_object");
    actor_ref
        .ask(register_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to register object");

    // ACT: Unregister object
    let unregister_msg = Message::json(&json!({
        "object_id": "service-2",
        "object_type": "Service"
    }))
    .expect("Failed to create message")
    .with_message_type("unregister_object");

    let reply = actor_ref
        .ask(unregister_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to unregister object");

    // ASSERT: Should receive success
    let response: serde_json::Value = serde_json::from_slice(&reply.payload)
        .expect("Failed to parse response");
    assert_eq!(response["status"], "ok");

    // ASSERT: Lookup should return None
    let lookup_msg = Message::json(&json!({
        "object_id": "service-2",
        "object_type": "Service"
    }))
    .expect("Failed to create message")
    .with_message_type("lookup_object");

    let reply = actor_ref
        .ask(lookup_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to lookup object");

    let response: serde_json::Value = serde_json::from_slice(&reply.payload)
        .expect("Failed to parse response");
    assert!(response.get("found").is_none() || response["found"] == false);
}

/// Test: Rust actor with RegistryFacet - discover
#[tokio::test]
async fn test_rust_actor_registry_facet_discover() {
    init_test_tracing();
    let node = get_shared_node().await;

    let service_locator = node.service_locator();
    let object_registry = service_locator
        .get_object_registry()
        .await
        .expect("ObjectRegistry should be registered");

    let registry_facet = RegistryFacet::new(
        Arc::new(ObjectRegistryAdapter {
            inner: object_registry,
        }),
        json!({}),
        50,
    );

    let node_id = node.id();
    let actor_name = format!("test-actor-{}", ulid::Ulid::new());
    let actor_id = format!("{}@{}", actor_name, node_id);
    let actor_id_typed = ActorId::from(actor_id.clone());
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "test-namespace".to_string(),
    );

    node.spawn(
        &ctx,
        &actor_id_typed,
        "GenServer",
        vec![],
        None,
        std::collections::HashMap::new(),
        vec![Box::new(registry_facet) as Box<dyn plexspaces_facet::Facet>],
    )
    .await
    .expect("Failed to spawn actor");

    let actor_ref = get_actor_ref_after_spawn(&node, &actor_id_typed).await;

    // ARRANGE: Register multiple objects
    for i in 1..=3 {
        let register_msg = Message::json(&json!({
            "object_id": format!("service-{}", i),
            "object_type": "Service",
            "grpc_address": format!("http://service-{}:50051", i)
        }))
        .expect("Failed to create message")
        .with_message_type("register_object");
        actor_ref
            .ask(register_msg.to_proto(), std::time::Duration::from_secs(5))
            .await
            .expect(&format!("Failed to register service-{}", i));
    }

    // ACT: Discover objects
    let discover_msg = Message::json(&json!({
        "object_type": "Service",
        "limit": 10,
        "offset": 0
    }))
    .expect("Failed to create message")
    .with_message_type("discover_objects");

    let reply = actor_ref
        .ask(discover_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to discover objects");

    // ASSERT: Should return multiple objects
    let response: serde_json::Value = serde_json::from_slice(&reply.payload)
        .expect("Failed to parse response");
    let objects: Vec<serde_json::Value> =
        serde_json::from_value(response["objects"].clone()).expect("Failed to parse objects");
    assert!(objects.len() >= 3);
}
