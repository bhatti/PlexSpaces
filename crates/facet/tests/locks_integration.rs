// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for LockFacet with real actors (Rust and WASM)
// Tests verify that LockFacet correctly intercepts lock operations and uses
// the LockManager from ServiceLocator (based on node config).

use plexspaces_actor::{create_facet_from_proto, ActorRef};
use plexspaces_actor::{
    service_locator_trait::ServiceLocator, Actor as ActorTrait, ActorContext, ActorId,
    InitializableServiceLocator, LockManager as CoreLockManager, RequestContextExt,
    ServiceLocatorBase,
};
use plexspaces_facet::capabilities::locks::LockFacet;
use plexspaces_mailbox::{new_message, Message};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::locks::prv::Lock;
use serde_json::json;
use std::collections::HashMap;
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

fn json_message(value: &serde_json::Value, message_type: &str) -> Message {
    let mut message =
        new_message(serde_json::to_vec(value).expect("Failed to serialize JSON message"));
    message.message_type = message_type.to_string();
    message
}

static SHARED_NODE: OnceLock<Arc<Node>> = OnceLock::new();
static INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        NodeBuilder::new("test-node")
            .with_in_memory_backends()
            .build()
            .await,
    );

    use plexspaces_actor::behavior_factory::BehaviorRegistry;
    let registry = BehaviorRegistry::new();
    registry
        .register_simple("gen_server", || {
            Box::pin(async move { Ok(Box::new(EchoBehavior) as Box<dyn plexspaces_actor::Actor>) })
        })
        .await;
    node.service_locator()
        .register_behavior_registry(Arc::new(registry))
        .await;

    {
        use std::time::Duration;
        use tokio::task::yield_now;
        use tokio::time::sleep;
        for _ in 0..5 {
            yield_now().await;
            sleep(Duration::from_millis(10)).await;
        }
    }

    SHARED_NODE.get_or_init(|| node.clone()).clone()
}

const ASK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

struct EchoBehavior;

#[async_trait::async_trait]
impl ActorTrait for EchoBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _message: plexspaces_proto::common::v1::Message,
    ) -> Result<(), plexspaces_actor::BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_actor::BehaviorType {
        plexspaces_actor::BehaviorType::GenServer
    }
}

struct LockManagerAdapter {
    inner: Arc<dyn CoreLockManager + Send + Sync>,
}

#[async_trait::async_trait]
impl plexspaces_facet::capabilities::locks::LockManager for LockManagerAdapter {
    async fn acquire_lock(
        &self,
        ctx: &plexspaces_common::RequestContext,
        options: plexspaces_proto::locks::prv::AcquireLockOptions,
    ) -> Result<Lock, String> {
        self.inner
            .acquire_lock(ctx, options)
            .await
            .map_err(|e| e.to_string())
    }

    async fn renew_lock(
        &self,
        ctx: &plexspaces_common::RequestContext,
        options: plexspaces_proto::locks::prv::RenewLockOptions,
    ) -> Result<Lock, String> {
        self.inner
            .renew_lock(ctx, options)
            .await
            .map_err(|e| e.to_string())
    }

    async fn release_lock(
        &self,
        ctx: &plexspaces_common::RequestContext,
        options: plexspaces_proto::locks::prv::ReleaseLockOptions,
    ) -> Result<(), String> {
        self.inner
            .release_lock(ctx, options)
            .await
            .map_err(|e| e.to_string())
    }

    async fn get_lock(
        &self,
        ctx: &plexspaces_common::RequestContext,
        lock_key: &str,
    ) -> Result<Option<Lock>, String> {
        self.inner
            .get_lock(ctx, lock_key)
            .await
            .map_err(|e| e.to_string())
    }
}

async fn get_actor_ref_after_spawn(node: &Node, actor_id: &ActorId) -> ActorRef {
    let actor_registry = node
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry should be available");

    for _ in 0..20 {
        if actor_registry.lookup_actor(actor_id).await.is_some() {
            use plexspaces_mailbox::{mailbox_config_default, Mailbox};
            let mailbox_for_ref = Arc::new(
                Mailbox::new(mailbox_config_default(), format!("ref-{}", actor_id))
                    .await
                    .expect("Failed to create mailbox for ActorRef"),
            );
            return ActorRef::local(
                actor_id.clone(),
                "test-tenant",
                String::new(),
                mailbox_for_ref,
                node.service_locator().clone(),
                plexspaces_proto::actor::v1::ActorVisibility::ActorVisibilityPublic,
            );
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
    panic!("Actor {} should be registered after spawning", actor_id);
}

static LOCK_FACET_SERVICES: OnceLock<()> = OnceLock::new();

async fn ensure_lock_facet_services(node: &Node) {
    if LOCK_FACET_SERVICES.get().is_none() {
        node.initialize_services()
            .await
            .expect("Failed to initialize services");
        let _ = LOCK_FACET_SERVICES.set(());
    }
}

#[derive(Clone, Copy)]
enum LockFacetCase {
    AcquireRelease,
    TryAcquire,
    GetLock,
    FromProto,
}

async fn run_lock_facet_case(node: &Arc<Node>, case: LockFacetCase, run_id: &str) {
    let lock_manager = node
        .service_locator()
        .get_lock_manager()
        .await
        .expect("LockManager should be registered");

    let node_id = node.id();
    let ctx = plexspaces_actor::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "test-namespace".to_string(),
    );

    match case {
        LockFacetCase::AcquireRelease => {
            let lock_key = format!("tbl-{run_id}-ar");
            let lock_facet = LockFacet::new(
                Arc::new(LockManagerAdapter {
                    inner: lock_manager,
                }),
                json!({}),
                50,
            );
            let actor_name = format!("lock-tbl-ar-{}", ulid::Ulid::new());
            let actor_id = ActorId::new(
                &actor_name,
                "gen_server",
                "test-namespace",
                node_id.as_str(),
            )
            .expect("test actor id should be valid");

            node.spawn(
                &ctx,
                &actor_id,
                "gen_server",
                vec![],
                None,
                HashMap::new(),
                vec![Box::new(lock_facet) as Box<dyn plexspaces_facet::Facet>],
            )
            .await
            .expect("Failed to spawn actor");

            let actor_ref = get_actor_ref_after_spawn(node, &actor_id).await;
            let actor_id_str = actor_id.to_string();

            let acquire_msg = json_message(
                &json!({
                    "lock_key": lock_key,
                    "holder_id": actor_id_str.clone(),
                    "lease_duration_secs": 30
                }),
                "acquire_lock",
            );

            let reply = actor_ref
                .ask(&ctx, acquire_msg, ASK_TIMEOUT)
                .await
                .expect("Failed to acquire lock");

            let lock_json: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("Failed to parse lock from reply");
            assert_eq!(lock_json["lock_key"], lock_key);
            assert_eq!(lock_json["holder_id"], actor_id_str);
            assert!(!lock_json["version"].as_str().unwrap().is_empty());

            let acquire_reply2 = actor_ref
                .ask(
                    &ctx,
                    json_message(
                        &json!({
                            "lock_key": lock_key,
                            "holder_id": actor_id_str.clone(),
                            "lease_duration_secs": 30
                        }),
                        "acquire_lock",
                    ),
                    ASK_TIMEOUT,
                )
                .await
                .expect("Failed to acquire lock");

            let lock_json2: serde_json::Value =
                serde_json::from_slice(&acquire_reply2.payload).expect("Failed to parse lock");
            let version = lock_json2["version"].as_str().unwrap();

            let release_msg = json_message(
                &json!({
                    "lock_key": lock_key,
                    "holder_id": actor_id_str.clone(),
                    "version": version,
                    "delete_lock": false
                }),
                "release_lock",
            );

            let release_reply = actor_ref
                .ask(&ctx, release_msg, ASK_TIMEOUT)
                .await
                .expect("Failed to release lock");

            let response: serde_json::Value =
                serde_json::from_slice(&release_reply.payload).expect("Failed to parse release");
            assert_eq!(response["status"], "ok");
        }
        LockFacetCase::TryAcquire => {
            let lock_key = format!("tbl-{run_id}-try");
            let lock_facet = LockFacet::new(
                Arc::new(LockManagerAdapter {
                    inner: lock_manager.clone(),
                }),
                json!({}),
                50,
            );

            let actor_name1 = format!("lock-tbl-1-{}", ulid::Ulid::new());
            let actor_id1 = ActorId::new(
                &actor_name1,
                "gen_server",
                "test-namespace",
                node_id.as_str(),
            )
            .expect("test actor id should be valid");
            let ctx1 = plexspaces_actor::RequestContext::new_without_auth(
                "test-tenant".to_string(),
                "test-namespace".to_string(),
            );

            node.clone()
                .spawn(
                    &ctx1,
                    &actor_id1,
                    "gen_server",
                    vec![],
                    None,
                    HashMap::new(),
                    vec![Box::new(lock_facet) as Box<dyn plexspaces_facet::Facet>],
                )
                .await
                .expect("Failed to spawn actor");

            let node_clone1 = node.clone();
            let actor_registry1 = node_clone1
                .service_locator()
                .actor_registry()
                .await
                .expect("ActorRegistry should be available");
            assert!(actor_registry1.lookup_actor(&actor_id1).await.is_some());
            let actor_ref1 = ActorRef::remote(
                actor_id1.clone(),
                String::new(),
                String::new(),
                node_clone1.id().as_str().to_string(),
                node_clone1.service_locator().clone(),
                plexspaces_proto::actor::v1::ActorVisibility::ActorVisibilityPublic,
            );

            let actor_id1_str = actor_id1.to_string();
            actor_ref1
                .ask(
                    &ctx1,
                    json_message(
                        &json!({
                            "lock_key": lock_key,
                            "holder_id": actor_id1_str.clone(),
                            "lease_duration_secs": 30
                        }),
                        "acquire_lock",
                    ),
                    ASK_TIMEOUT,
                )
                .await
                .expect("Failed to acquire lock");

            let lock_facet2 = LockFacet::new(
                Arc::new(LockManagerAdapter {
                    inner: lock_manager,
                }),
                json!({}),
                50,
            );

            let actor_name2 = format!("lock-tbl-2-{}", ulid::Ulid::new());
            let actor_id2 = ActorId::new(
                &actor_name2,
                "gen_server",
                "test-namespace",
                node_id.as_str(),
            )
            .expect("test actor id should be valid");
            let ctx2 = plexspaces_actor::RequestContext::new_without_auth(
                "test-tenant".to_string(),
                "test-namespace".to_string(),
            );

            node.clone()
                .spawn(
                    &ctx2,
                    &actor_id2,
                    "gen_server",
                    vec![],
                    None,
                    HashMap::new(),
                    vec![Box::new(lock_facet2) as Box<dyn plexspaces_facet::Facet>],
                )
                .await
                .expect("Failed to spawn actor");

            let node_clone2 = node.clone();
            let actor_registry2 = node_clone2
                .service_locator()
                .actor_registry()
                .await
                .expect("ActorRegistry should be available");
            assert!(actor_registry2.lookup_actor(&actor_id2).await.is_some());
            let actor_ref2 = ActorRef::remote(
                actor_id2.clone(),
                String::new(),
                String::new(),
                node_clone2.id().as_str().to_string(),
                node_clone2.service_locator().clone(),
                plexspaces_proto::actor::v1::ActorVisibility::ActorVisibilityPublic,
            );

            let try_reply = actor_ref2
                .ask(
                    &ctx2,
                    json_message(
                        &json!({
                            "lock_key": lock_key,
                            "holder_id": actor_id2.to_string(),
                            "lease_duration_secs": 30
                        }),
                        "try_acquire_lock",
                    ),
                    ASK_TIMEOUT,
                )
                .await
                .expect("Failed to try acquire lock");

            let response: serde_json::Value =
                serde_json::from_slice(&try_reply.payload).expect("Failed to parse try_acquire");
            assert_eq!(response["acquired"], false);
        }
        LockFacetCase::GetLock => {
            let lock_key = format!("tbl-{run_id}-get");
            let lock_facet = LockFacet::new(
                Arc::new(LockManagerAdapter {
                    inner: lock_manager,
                }),
                json!({}),
                50,
            );
            let actor_name = format!("lock-tbl-get-{}", ulid::Ulid::new());
            let actor_id = ActorId::new(
                &actor_name,
                "gen_server",
                "test-namespace",
                node_id.as_str(),
            )
            .expect("test actor id should be valid");

            node.spawn(
                &ctx,
                &actor_id,
                "gen_server",
                vec![],
                None,
                HashMap::new(),
                vec![Box::new(lock_facet) as Box<dyn plexspaces_facet::Facet>],
            )
            .await
            .expect("Failed to spawn actor");

            let actor_ref = get_actor_ref_after_spawn(node, &actor_id).await;
            let actor_id_str = actor_id.to_string();

            let get_msg = json_message(&json!(lock_key), "get_lock");
            let get_reply = actor_ref
                .ask(&ctx, get_msg, ASK_TIMEOUT)
                .await
                .expect("Failed to get lock");

            let response: serde_json::Value =
                serde_json::from_slice(&get_reply.payload).expect("Failed to parse get_lock");
            assert_eq!(response["found"], false);

            actor_ref
                .ask(
                    &ctx,
                    json_message(
                        &json!({
                            "lock_key": lock_key,
                            "holder_id": actor_id_str.clone(),
                            "lease_duration_secs": 30
                        }),
                        "acquire_lock",
                    ),
                    ASK_TIMEOUT,
                )
                .await
                .expect("Failed to acquire lock");

            let get_reply2 = actor_ref
                .ask(
                    &ctx,
                    json_message(&json!(lock_key), "get_lock"),
                    ASK_TIMEOUT,
                )
                .await
                .expect("Failed to get lock");

            let lock_json: serde_json::Value = serde_json::from_slice(&get_reply2.payload)
                .expect("Failed to parse lock from get_lock response");
            assert_eq!(lock_json["lock_key"], lock_key);
            assert_eq!(lock_json["holder_id"], actor_id_str);
        }
        LockFacetCase::FromProto => {
            ensure_lock_facet_services(node).await;

            let service_locator = node.service_locator();
            let facet_registry_wrapper = service_locator
                .get_facet_registry()
                .await
                .expect("FacetRegistry should be registered");
            let facet_registry = facet_registry_wrapper.inner_clone();

            let registered_types = facet_registry.list_types();
            assert!(
                registered_types.contains(&"locks".to_string()),
                "LockFacetFactory should be registered. Found types: {:?}",
                registered_types
            );

            use plexspaces_proto::common::v1::Facet as ProtoFacet;
            let proto_facet = ProtoFacet {
                r#type: "locks".to_string(),
                config: HashMap::new(),
                priority: 50,
                state: HashMap::new(),
                metadata: None,
            };

            let lock_facet = create_facet_from_proto(&proto_facet, &facet_registry)
                .await
                .expect("Failed to create LockFacet from proto");
            assert_eq!(lock_facet.facet_type(), "locks");

            let lock_key = format!("tbl-{run_id}-proto");
            let actor_name = format!("lock-tbl-proto-{}", ulid::Ulid::new());
            let actor_id = ActorId::new(
                &actor_name,
                "gen_server",
                "test-namespace",
                node_id.as_str(),
            )
            .expect("test actor id should be valid");
            let actor_id_str = actor_id.to_string();

            node.spawn(
                &ctx,
                &actor_id,
                "gen_server",
                vec![],
                None,
                HashMap::new(),
                vec![lock_facet],
            )
            .await
            .expect("Failed to spawn actor with LockFacet from proto");

            let actor_ref = get_actor_ref_after_spawn(node, &actor_id).await;
            let reply = actor_ref
                .ask(
                    &ctx,
                    json_message(
                        &json!({
                            "lock_key": lock_key,
                            "holder_id": actor_id_str.clone(),
                            "lease_duration_secs": 30
                        }),
                        "acquire_lock",
                    ),
                    ASK_TIMEOUT,
                )
                .await
                .expect("Failed to acquire lock");

            let lock_json: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("Failed to parse lock from reply");
            assert_eq!(lock_json["lock_key"], lock_key);
            assert_eq!(lock_json["holder_id"], actor_id_str);
            assert!(!lock_json["version"].as_str().unwrap().is_empty());
        }
    }
}

/// Single shared node; table-driven cases with unique lock keys and actors per iteration.
#[tokio::test]
async fn lock_facet_integration_table() {
    init_test_tracing();
    let node = get_shared_node().await;
    for case in [
        LockFacetCase::AcquireRelease,
        LockFacetCase::TryAcquire,
        LockFacetCase::GetLock,
        LockFacetCase::FromProto,
    ] {
        let run_id = ulid::Ulid::new().to_string();
        run_lock_facet_case(&node, case, &run_id).await;
    }
}
