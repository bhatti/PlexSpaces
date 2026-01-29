// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for ProcessGroupFacet with real actors (Rust and WASM)
// Tests verify that ProcessGroupFacet correctly intercepts process group operations and uses
// the ProcessGroupRegistry from ServiceLocator (based on node config).

use plexspaces_actor::ActorRef;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId};
use plexspaces_facet::capabilities::process_groups::ProcessGroupFacet;
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
        NodeBuilder::new("test-node-process-group")
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

/// Adapter for ProcessGroupRegistry trait (matches pattern from facet_helpers.rs)
struct ProcessGroupRegistryAdapter {
    inner: Arc<dyn plexspaces_core::ProcessGroupService>,
}

#[async_trait::async_trait]
impl plexspaces_facet::capabilities::process_groups::ProcessGroupRegistry for ProcessGroupRegistryAdapter {
    async fn create_group(&self, ctx: &plexspaces_common::RequestContext, group_name: &str) -> Result<(), String> {
        self.inner.create_group(ctx, group_name).await.map_err(|e| e.to_string())
    }

    async fn join_group(
        &self,
        ctx: &plexspaces_common::RequestContext,
        group_name: &str,
        actor_id: &str,
        topics: Vec<String>,
    ) -> Result<(), String> {
        self.inner
            .join_group(ctx, group_name, actor_id, topics)
            .await
            .map_err(|e| e.to_string())
    }

    async fn leave_group(&self, ctx: &plexspaces_common::RequestContext, group_name: &str, actor_id: &str) -> Result<(), String> {
        self.inner
            .leave_group(ctx, group_name, actor_id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn get_members(&self, ctx: &plexspaces_common::RequestContext, group_name: &str) -> Result<Vec<String>, String> {
        self.inner.get_members(ctx, group_name).await.map_err(|e| e.to_string())
    }

    async fn get_local_members(&self, ctx: &plexspaces_common::RequestContext, group_name: &str) -> Result<Vec<String>, String> {
        self.inner
            .get_local_members(ctx, group_name)
            .await
            .map_err(|e| e.to_string())
    }

    async fn list_groups(&self, ctx: &plexspaces_common::RequestContext) -> Result<Vec<String>, String> {
        self.inner.list_groups(ctx).await.map_err(|e| e.to_string())
    }

    async fn publish_to_group(
        &self,
        ctx: &plexspaces_common::RequestContext,
        group_name: &str,
        topic: Option<&str>,
        message: Vec<u8>,
    ) -> Result<Vec<String>, String> {
        // ProcessGroupService::publish_to_group takes Message, not Vec<u8>
        // Convert Vec<u8> to Message for the service call
        use plexspaces_proto::common::v1::Message as ProtoMessage;
        let msg = ProtoMessage {
            id: ulid::Ulid::new().to_string(),
            payload: message,
            ..Default::default()
        };
        
        let recipient_count = self.inner
            .publish_to_group(ctx, group_name, topic, msg)
            .await
            .map_err(|e| e.to_string())?;
        
        // Get members to return as recipients
        let members = self.get_members(ctx, group_name).await?;
        // Return first N members where N = recipient_count
        Ok(members.into_iter().take(recipient_count as usize).collect())
    }
}

/// Test: Rust actor with ProcessGroupFacet - create group and join
#[tokio::test]
async fn test_rust_actor_process_group_facet_create_join() {
    init_test_tracing();
    let node = get_shared_node().await;

    let service_locator = node.service_locator();
    let process_group_service = service_locator
        .get_process_group_service()
        .await
        .expect("ProcessGroupService should be registered");

    let process_group_facet = ProcessGroupFacet::new(
        Arc::new(ProcessGroupRegistryAdapter {
            inner: process_group_service,
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
        vec![Box::new(process_group_facet) as Box<dyn plexspaces_facet::Facet>],
    )
    .await
    .expect("Failed to spawn actor");

    let actor_ref = get_actor_ref_after_spawn(&node, &actor_id_typed).await;

    // ACT: Create group
    let create_msg = Message::json(&json!("test-group-1"))
        .expect("Failed to create message")
        .with_message_type("create_group");

    let reply = actor_ref
        .ask(create_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to create group");

    // ASSERT: Should receive success
    let response: serde_json::Value = serde_json::from_slice(&reply.payload)
        .expect("Failed to parse response");
    assert_eq!(response["status"], "ok");

    // ACT: Join group
    let join_msg = Message::json(&json!({
        "group_name": "test-group-1",
        "actor_id": actor_id.clone(),
        "topics": ["topic-1", "topic-2"]
    }))
    .expect("Failed to create message")
    .with_message_type("join_group");

    let reply = actor_ref
        .ask(join_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to join group");

    // ASSERT: Should receive success
    let response: serde_json::Value = serde_json::from_slice(&reply.payload)
        .expect("Failed to parse response");
    assert_eq!(response["status"], "ok");

    // ACT: Get members
    let get_members_msg = Message::json(&json!("test-group-1"))
        .expect("Failed to create message")
        .with_message_type("get_members");

    let reply = actor_ref
        .ask(get_members_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to get members");

    // ASSERT: Should contain actor
    let response: serde_json::Value = serde_json::from_slice(&reply.payload)
        .expect("Failed to parse response");
    let members: Vec<String> = serde_json::from_value(response["members"].clone())
        .expect("Failed to parse members");
    assert!(members.contains(&actor_id), "Actor {} should be in members list: {:?}", actor_id, members);
}

/// Test: Rust actor with ProcessGroupFacet - publish to group
#[tokio::test]
async fn test_rust_actor_process_group_facet_publish() {
    init_test_tracing();
    let node = get_shared_node().await;

    let service_locator = node.service_locator();
    let process_group_service = service_locator
        .get_process_group_service()
        .await
        .expect("ProcessGroupService should be registered");

    let process_group_facet = ProcessGroupFacet::new(
        Arc::new(ProcessGroupRegistryAdapter {
            inner: process_group_service,
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
        vec![Box::new(process_group_facet) as Box<dyn plexspaces_facet::Facet>],
    )
    .await
    .expect("Failed to spawn actor");

    let actor_ref = get_actor_ref_after_spawn(&node, &actor_id_typed).await;

    // ARRANGE: Create group and join
    let create_msg = Message::json(&json!("test-group-2"))
        .expect("Failed to create message")
        .with_message_type("create_group");
    actor_ref
        .ask(create_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to create group");

    let join_msg = Message::json(&json!({
        "group_name": "test-group-2",
        "actor_id": actor_id.clone(),
        "topics": ["news"]
    }))
    .expect("Failed to create message")
        .with_message_type("join_group");
    actor_ref
        .ask(join_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to join group");

    // ACT: Publish to group
    let publish_msg = Message::json(&json!({
        "group_name": "test-group-2",
        "topic": "news",
        "message": "Hello, group!"
    }))
    .expect("Failed to create message")
    .with_message_type("publish_to_group");

    let reply = actor_ref
        .ask(publish_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to publish");

    // ASSERT: Should receive success
    let response: serde_json::Value = serde_json::from_slice(&reply.payload)
        .expect("Failed to parse response");
    assert_eq!(response["status"], "ok");
}
