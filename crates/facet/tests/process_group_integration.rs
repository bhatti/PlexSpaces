// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for ProcessGroupFacet with real actors (Rust and WASM)
// Tests verify that ProcessGroupFacet correctly intercepts process group operations and uses
// an in-memory ProcessGroupRegistry.

use plexspaces_actor::ActorRef;
use plexspaces_common::RequestContext;
use plexspaces_actor::{service_locator_trait::ServiceLocator, ActorId, InitializableServiceLocator, RequestContextExt};
use plexspaces_facet::capabilities::process_groups::{ProcessGroupFacet, ProcessGroupRegistry};
use plexspaces_mailbox::{new_message, Message};
use plexspaces_node::{Node, NodeBuilder};
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

struct EchoBehavior;

#[async_trait::async_trait]
impl plexspaces_actor::Actor for EchoBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &plexspaces_actor::ActorContext,
        _message: Message,
    ) -> Result<(), plexspaces_actor::BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_actor::BehaviorType {
        plexspaces_actor::BehaviorType::GenServer
    }
}

struct TestProcessGroupRegistry {
    groups: Arc<tokio::sync::RwLock<HashMap<String, Vec<String>>>>,
}

impl TestProcessGroupRegistry {
    fn new() -> Self {
        Self {
            groups: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl ProcessGroupRegistry for TestProcessGroupRegistry {
    async fn create_group(&self, _ctx: &RequestContext, group_name: &str) -> Result<(), String> {
        let mut groups = self.groups.write().await;
        if groups.contains_key(group_name) {
            return Err(format!("Group already exists: {}", group_name));
        }
        groups.insert(group_name.to_string(), vec![]);
        Ok(())
    }

    async fn join_group(
        &self,
        _ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
        _topics: Vec<String>,
    ) -> Result<(), String> {
        let mut groups = self.groups.write().await;
        let members = groups
            .get_mut(group_name)
            .ok_or_else(|| format!("Group not found: {}", group_name))?;
        if !members.contains(&actor_id.to_string()) {
            members.push(actor_id.to_string());
        }
        Ok(())
    }

    async fn leave_group(
        &self,
        _ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
    ) -> Result<(), String> {
        let mut groups = self.groups.write().await;
        let members = groups
            .get_mut(group_name)
            .ok_or_else(|| format!("Group not found: {}", group_name))?;
        members.retain(|id| id != actor_id);
        Ok(())
    }

    async fn get_members(
        &self,
        _ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, String> {
        let groups = self.groups.read().await;
        Ok(groups.get(group_name).cloned().unwrap_or_default())
    }

    async fn get_local_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, String> {
        self.get_members(ctx, group_name).await
    }

    async fn list_groups(&self, _ctx: &RequestContext) -> Result<Vec<String>, String> {
        let groups = self.groups.read().await;
        Ok(groups.keys().cloned().collect())
    }

    async fn publish_to_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        _topic: Option<&str>,
        _message: Vec<u8>,
    ) -> Result<Vec<String>, String> {
        self.get_members(ctx, group_name).await
    }
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
        NodeBuilder::new("test-node-process-group")
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

#[derive(Clone, Copy)]
enum ProcessGroupCase {
    CreateJoinAndMembers,
    Publish,
}

async fn run_process_group_case(node: &Arc<Node>, case: ProcessGroupCase, run_id: &str) {
    let process_group_facet =
        ProcessGroupFacet::new(Arc::new(TestProcessGroupRegistry::new()), json!({}), 50);

    let node_id = node.id();
    let actor_name = format!("pg-tbl-{}", ulid::Ulid::new());
    let actor_id = ActorId::new(
        &actor_name,
        "gen_server",
        "test-namespace",
        node_id.as_str(),
    )
    .expect("test actor id should be valid");
    let actor_id_str = actor_id.to_string();
    let ctx = plexspaces_actor::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "test-namespace".to_string(),
    );

    node.spawn(
        &ctx,
        &actor_id,
        "gen_server",
        vec![],
        None,
        HashMap::new(),
        vec![Box::new(process_group_facet) as Box<dyn plexspaces_facet::Facet>],
    )
    .await
    .expect("Failed to spawn actor");

    let actor_ref = get_actor_ref_after_spawn(node, &actor_id).await;

    match case {
        ProcessGroupCase::CreateJoinAndMembers => {
            let group_name = format!("tbl-{run_id}-g1");
            let reply = actor_ref
                .ask(
                    &ctx,
                    json_message(&json!(group_name), "create_group"),
                    ASK_TIMEOUT,
                )
                .await
                .expect("Failed to create group");
            let response: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("Failed to parse response");
            assert_eq!(response["status"], "ok");

            let join_msg = json_message(
                &json!({
                    "group_name": group_name,
                    "actor_id": actor_id_str.clone(),
                    "topics": ["topic-1", "topic-2"]
                }),
                "join_group",
            );
            let reply = actor_ref
                .ask(&ctx, join_msg, ASK_TIMEOUT)
                .await
                .expect("Failed to join group");
            let response: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("Failed to parse response");
            assert_eq!(response["status"], "ok");

            let get_members_msg = json_message(&json!(group_name), "get_members");
            let reply = actor_ref
                .ask(&ctx, get_members_msg, ASK_TIMEOUT)
                .await
                .expect("Failed to get members");
            let response: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("Failed to parse response");
            let members: Vec<String> =
                serde_json::from_value(response["members"].clone()).expect("parse members");
            assert!(
                members.contains(&actor_id_str),
                "Actor {} should be in members list: {:?}",
                actor_id_str,
                members
            );
        }
        ProcessGroupCase::Publish => {
            let group_name = format!("tbl-{run_id}-g2");
            actor_ref
                .ask(
                    &ctx,
                    json_message(&json!(group_name), "create_group"),
                    ASK_TIMEOUT,
                )
                .await
                .expect("Failed to create group");

            let join_msg = json_message(
                &json!({
                    "group_name": group_name,
                    "actor_id": actor_id_str.clone(),
                    "topics": ["news"]
                }),
                "join_group",
            );
            actor_ref
                .ask(&ctx, join_msg, ASK_TIMEOUT)
                .await
                .expect("Failed to join group");

            let publish_msg = json_message(
                &json!({
                    "group_name": group_name,
                    "topic": "news",
                    "message": "Hello, group!"
                }),
                "publish_to_group",
            );
            let reply = actor_ref
                .ask(&ctx, publish_msg, ASK_TIMEOUT)
                .await
                .expect("Failed to publish");
            let response: serde_json::Value =
                serde_json::from_slice(&reply.payload).expect("Failed to parse response");
            assert_eq!(response["status"], "ok");
        }
    }
}

/// Single shared node; per-case isolated registry, actor, and group names.
#[tokio::test]
async fn process_group_facet_integration_table() {
    init_test_tracing();
    let node = get_shared_node().await;
    for case in [
        ProcessGroupCase::CreateJoinAndMembers,
        ProcessGroupCase::Publish,
    ] {
        let run_id = ulid::Ulid::new().to_string();
        run_process_group_case(&node, case, &run_id).await;
    }
}
