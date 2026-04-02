// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for RegistryFacet with real actors (Rust and WASM)
// Tests verify that RegistryFacet correctly intercepts registry operations and uses
// the ObjectRegistry from ServiceLocator (based on node config).

use plexspaces_actor::ActorRef;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, ServiceLocator};
use plexspaces_facet::capabilities::registry::RegistryFacet;
use plexspaces_mailbox::{new_message, Mailbox, Message};
use plexspaces_node::{Node, NodeBuilder};
use serde_json::json;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

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

/// Minimal GenServer behavior; facet pipeline handles registry JSON request/reply.
struct EchoBehavior;

#[async_trait::async_trait]
impl ActorTrait for EchoBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _message: plexspaces_proto::common::v1::Message,
    ) -> Result<(), BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

fn json_message(value: &serde_json::Value, message_type: &str) -> Message {
    let mut message =
        new_message(serde_json::to_vec(value).expect("Failed to serialize JSON message"));
    message.message_type = message_type.to_string();
    message
}

static SHARED_REGISTRY_NODE: OnceLock<Arc<Node>> = OnceLock::new();
static SHARED_REGISTRY_NODE_INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One shared node for all registry facet table cases (`NodeBuilder` + in-memory backends are costly).
async fn shared_registry_test_node() -> Arc<Node> {
    if let Some(n) = SHARED_REGISTRY_NODE.get() {
        return n.clone();
    }
    let _g = SHARED_REGISTRY_NODE_INIT
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some(n) = SHARED_REGISTRY_NODE.get() {
        return n.clone();
    }

    let node = Arc::new(
        NodeBuilder::new("test-node-registry")
            .with_in_memory_backends()
            .build()
            .await,
    );

    use plexspaces_core::behavior_factory::BehaviorRegistry;
    let registry = BehaviorRegistry::new();
    registry
        .register_simple("GenServer", || {
            Box::pin(async move { Ok(Box::new(EchoBehavior) as Box<dyn plexspaces_core::Actor>) })
        })
        .await;
    node.service_locator()
        .register_behavior_registry(Arc::new(registry))
        .await;

    use std::time::Duration;
    use tokio::task::yield_now;
    use tokio::time::sleep;
    for _ in 0..5 {
        yield_now().await;
        sleep(Duration::from_millis(10)).await;
    }

    SHARED_REGISTRY_NODE.get_or_init(|| node.clone()).clone()
}

const ASK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Spawn a GenServer with [`RegistryFacet`] backed by this node's object registry.
async fn spawn_registry_facet_actor(node: &Arc<Node>) -> (ActorRef, ActorId) {
    let object_registry = node
        .service_locator()
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
    let actor_name = format!("reg-tbl-{}", ulid::Ulid::new());
    let actor_id = ActorId::from(format!("{actor_name}@{node_id}"));
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "test-namespace".to_string(),
    );

    node.spawn(
        &ctx,
        &actor_id,
        "GenServer",
        vec![],
        None,
        std::collections::HashMap::new(),
        vec![Box::new(registry_facet) as Box<dyn plexspaces_facet::Facet>],
    )
    .await
    .expect("Failed to spawn registry facet actor");

    let actor_ref = get_actor_ref_after_spawn(node, &actor_id).await;
    (actor_ref, actor_id)
}

#[derive(Clone, Copy, Debug)]
enum RegistryFacetCase {
    RegisterLookup,
    Unregister,
    DiscoverThree,
}

const REGISTRY_FACET_CASES: &[RegistryFacetCase] = &[
    RegistryFacetCase::RegisterLookup,
    RegistryFacetCase::Unregister,
    RegistryFacetCase::DiscoverThree,
];

async fn run_registry_facet_case(node: &Arc<Node>, case: RegistryFacetCase, case_ulid: &str) {
    let (actor_ref, _) = spawn_registry_facet_actor(node).await;

    match case {
        RegistryFacetCase::RegisterLookup => {
            let object_id = format!("tbl-{case_ulid}-lookup");
            let grpc_address = format!("http://tbl-{case_ulid}-lookup:50051");

            let register_msg = json_message(
                &json!({
                    "object_id": object_id,
                    "object_type": "Service",
                    "grpc_address": grpc_address,
                    "metadata": {"version": "1.0.0"}
                }),
                "register_object",
            );
            let reply = actor_ref
                .ask(register_msg, ASK_TIMEOUT)
                .await
                .unwrap_or_else(|e| panic!("case={case:?} register: {e}"));
            let response: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("parse register reply");
            assert_eq!(response["status"], "ok", "case={case:?}");

            let lookup_msg = json_message(
                &json!({
                    "object_id": object_id,
                    "object_type": "Service"
                }),
                "lookup_object",
            );
            let reply = actor_ref
                .ask(lookup_msg, ASK_TIMEOUT)
                .await
                .unwrap_or_else(|e| panic!("case={case:?} lookup: {e}"));
            let response: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("parse lookup reply");
            assert_eq!(response["object_id"], object_id, "case={case:?}");
            assert_eq!(response["grpc_address"], grpc_address, "case={case:?}");
        }
        RegistryFacetCase::Unregister => {
            let object_id = format!("tbl-{case_ulid}-unreg");
            let grpc_address = format!("http://tbl-{case_ulid}-unreg:50051");

            let register_msg = json_message(
                &json!({
                    "object_id": object_id,
                    "object_type": "Service",
                    "grpc_address": grpc_address
                }),
                "register_object",
            );
            actor_ref
                .ask(register_msg, ASK_TIMEOUT)
                .await
                .unwrap_or_else(|e| panic!("case={case:?} register: {e}"));

            let unregister_msg = json_message(
                &json!({
                    "object_id": object_id,
                    "object_type": "Service"
                }),
                "unregister_object",
            );
            let reply = actor_ref
                .ask(unregister_msg, ASK_TIMEOUT)
                .await
                .unwrap_or_else(|e| panic!("case={case:?} unregister: {e}"));
            let response: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("parse unregister reply");
            assert_eq!(response["status"], "ok", "case={case:?}");

            let lookup_msg = json_message(
                &json!({
                    "object_id": object_id,
                    "object_type": "Service"
                }),
                "lookup_object",
            );
            let reply = actor_ref
                .ask(lookup_msg, ASK_TIMEOUT)
                .await
                .unwrap_or_else(|e| panic!("case={case:?} lookup after unregister: {e}"));
            let response: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("parse lookup reply");
            assert!(
                response.get("found").is_none() || response["found"] == false,
                "case={case:?} expected not found, got {response}"
            );
        }
        RegistryFacetCase::DiscoverThree => {
            let prefix = format!("tbl-{case_ulid}-disc-");
            let mut expected: Vec<String> = Vec::new();
            for i in 1..=3 {
                let object_id = format!("{prefix}{i}");
                expected.push(object_id.clone());
                let register_msg = json_message(
                    &json!({
                        "object_id": object_id,
                        "object_type": "Service",
                        "grpc_address": format!("http://tbl-{case_ulid}-disc-{i}:50051")
                    }),
                    "register_object",
                );
                actor_ref
                    .ask(register_msg, ASK_TIMEOUT)
                    .await
                    .unwrap_or_else(|e| panic!("case={case:?} register {object_id}: {e}"));
            }

            let discover_msg = json_message(
                &json!({
                    "object_type": "Service",
                    "offset": 0,
                    "limit": 50
                }),
                "discover_objects",
            );
            let reply = actor_ref
                .ask(discover_msg, ASK_TIMEOUT)
                .await
                .unwrap_or_else(|e| panic!("case={case:?} discover: {e}"));
            let response: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("parse discover reply");
            let objects: Vec<serde_json::Value> =
                serde_json::from_value(response["objects"].clone()).expect("parse objects array");

            let matching: Vec<&serde_json::Value> = objects
                .iter()
                .filter(|o| {
                    o["object_id"]
                        .as_str()
                        .is_some_and(|id| id.starts_with(&prefix))
                })
                .collect();
            assert_eq!(
                matching.len(),
                3,
                "case={case:?} want 3 objects under prefix {prefix}, got {} total services",
                objects.len()
            );
            let found: HashSet<String> = matching
                .iter()
                .filter_map(|o| o["object_id"].as_str().map(String::from))
                .collect();
            for id in &expected {
                assert!(
                    found.contains(id),
                    "case={case:?} missing {id}, found={found:?}"
                );
            }
        }
    }
}

/// Helper to get ActorRef after spawning an actor
///
/// ## Purpose
/// Waits for actor to be registered and creates ActorRef for ask() pattern.
/// For local actors, uses ActorRef::local() with mailbox.
/// For remote actors, uses ActorRef::remote().
async fn get_actor_ref_after_spawn(node: &Node, actor_id: &ActorId) -> ActorRef {
    let actor_registry = node
        .service_locator()
        .actor_registry()
        .await
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
                Mailbox::new(mailbox_config_default(), format!("ref-{}", actor_id))
                    .await
                    .expect("Failed to create mailbox for ActorRef"),
            );
            return ActorRef::local(
                actor_id.clone(),
                "test-tenant",
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
        offset: usize,
        limit: usize,
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
                None,
                labels,
                health_status_enum,
                offset,
                limit,
            )
            .await
            .map_err(|e| e.to_string())
    }
}

/// Rust [`RegistryFacet`] scenarios on **one** shared node (table-driven; cheap to add rows).
///
/// Each row gets a fresh ULID prefix for `object_id`s and a dedicated actor so cases stay
/// independent without `#[serial]` or cross-test races.
#[tokio::test]
async fn registry_facet_rust_object_registry_table() {
    init_test_tracing();
    let node = shared_registry_test_node().await;
    for case in REGISTRY_FACET_CASES {
        let case_ulid = ulid::Ulid::new().to_string();
        run_registry_facet_case(&node, *case, &case_ulid).await;
    }
}
