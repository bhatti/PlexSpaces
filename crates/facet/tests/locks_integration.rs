// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for LockFacet with real actors (Rust and WASM)
// Tests verify that LockFacet correctly intercepts lock operations and uses
// the LockManager from ServiceLocator (based on node config).

use plexspaces_actor::{create_facet_from_proto, ActorRef};
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, LockManager as CoreLockManager};
use plexspaces_facet::capabilities::locks::LockFacet;
use plexspaces_mailbox::Mailbox;
use plexspaces_mailbox::Message;
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::locks::prv::Lock;
use serde_json::json;
use std::sync::Arc;
use std::sync::OnceLock;

// Initialize tracing for tests (if not already initialized)
static TRACING_INIT: std::sync::Once = std::sync::Once::new();

fn init_test_tracing() {
    TRACING_INIT.call_once(|| {
        // Only initialize if RUST_LOG is set, otherwise use default
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
    let _guard = INIT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Double-check after acquiring lock
    if let Some(node) = SHARED_NODE.get() {
        return node.clone();
    }

    // Create node with in-memory backends (for testing)
    // build() already initializes services, so we don't need to call initialize_services()
    let node = Arc::new(
        NodeBuilder::new("test-node")
            .with_in_memory_backends()
            .build()
            .await,
    );

    // Wait for services to be ready with polling (no gRPC server startup needed)
    use std::time::Duration;
    use tokio::task::yield_now;
    use tokio::time::sleep;
    for _ in 0..5 {
        yield_now().await;
        sleep(Duration::from_millis(10)).await;
    }

    SHARED_NODE.get_or_init(|| node.clone()).clone()
}

/// Simple test behavior that echoes messages
struct EchoBehavior;

#[async_trait::async_trait]
impl ActorTrait for EchoBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _message: plexspaces_proto::common::v1::Message,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        // Echo behavior - just acknowledge
        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
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
                String::new(), // Test namespace
                mailbox_for_ref,
                node.service_locator().clone(),
            );
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
    panic!("Actor {} should be registered after spawning", actor_id);
}

/// Test: Rust actor with LockFacet - acquire and release lock
#[tokio::test]
async fn test_rust_actor_lock_facet_acquire_release() {
    init_test_tracing();

    // ARRANGE: Get shared node
    let node = get_shared_node().await;

    // Get LockManager from ServiceLocator
    let service_locator = node.service_locator();
    let lock_manager = service_locator
        .get_lock_manager()
        .await
        .expect("LockManager should be registered");

    // Create LockFacet with LockManager from ServiceLocator
    let lock_facet = LockFacet::new(
        Arc::new(LockManagerAdapter {
            inner: lock_manager,
        }),
        json!({}),
        50,
    );

    // Create actor with LockFacet
    // Format actor ID as "actor@node" for proper routing
    let node_id = node.id();
    let actor_name = format!("test-actor-{}", ulid::Ulid::new());
    let actor_id = format!("{}@{}", actor_name, node_id);
    let actor_id_typed = ActorId::from(actor_id.clone());
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "test-namespace".to_string(),
    );

    // Spawn actor and get ActorRef for ask() pattern
    node.spawn(
        &ctx,
        &actor_id_typed,
        "GenServer",
        vec![],
        None,
        std::collections::HashMap::new(),
        vec![Box::new(lock_facet) as Box<dyn plexspaces_facet::Facet>],
    )
    .await
    .expect("Failed to spawn actor");

    // Get ActorRef after spawning (waits for registration)
    let actor_ref = get_actor_ref_after_spawn(&node, &actor_id_typed).await;

    // ACT: Send acquire_lock message
    let acquire_msg = Message::json(&json!({
        "lock_key": "test-resource-1",
        "holder_id": actor_id.clone(),
        "lease_duration_secs": 30
    }))
    .expect("Failed to create message")
    .with_message_type("acquire_lock");

    let reply = actor_ref
        .ask(acquire_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to acquire lock");

    // ASSERT: Should receive lock data
    let lock_json: serde_json::Value =
        serde_json::from_slice(&reply.payload).expect("Failed to parse lock from reply");
    assert_eq!(lock_json["lock_key"], "test-resource-1");
    assert_eq!(lock_json["holder_id"], actor_id);
    assert!(!lock_json["version"].as_str().unwrap().is_empty());

    // ACT: Send release_lock message (need to get version from acquire response)
    let acquire_reply2 = actor_ref
        .ask(
            Message::json(&json!({
                "lock_key": "test-resource-1",
                "holder_id": actor_id.clone(),
                "lease_duration_secs": 30
            }))
            .expect("Failed to create message")
            .with_message_type("acquire_lock")
            .to_proto(),
            std::time::Duration::from_secs(5),
        )
        .await
        .expect("Failed to acquire lock");

    let lock_json2: serde_json::Value =
        serde_json::from_slice(&acquire_reply2.payload).expect("Failed to parse lock from reply");
    let version = lock_json2["version"].as_str().unwrap();

    let release_msg = Message::json(&json!({
        "lock_key": "test-resource-1",
        "holder_id": actor_id.clone(),
        "version": version,
        "delete_lock": false
    }))
    .expect("Failed to create message")
    .with_message_type("release_lock");

    let release_reply = actor_ref
        .ask(release_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to release lock");

    // ASSERT: Should receive success response
    let response: serde_json::Value =
        serde_json::from_slice(&release_reply.payload).expect("Failed to parse release response");
    assert_eq!(response["status"], "ok");
}

/// Test: Rust actor with LockFacet - try_acquire_lock (non-blocking)
#[tokio::test]
async fn test_rust_actor_lock_facet_try_acquire() {
    init_test_tracing();

    // ARRANGE: Get shared node
    let node = get_shared_node().await;

    let lock_manager = node
        .service_locator()
        .get_lock_manager()
        .await
        .expect("LockManager should be registered");

    let lock_facet = LockFacet::new(
        Arc::new(LockManagerAdapter {
            inner: lock_manager.clone(),
        }),
        json!({}),
        50,
    );

    let node_id = node.id();
    let actor_name1 = format!("test-actor-1-{}", ulid::Ulid::new());
    let actor_id1 = format!("{}@{}", actor_name1, node_id);
    let actor_id1_typed = ActorId::from(actor_id1.clone());
    let ctx1 = plexspaces_core::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "test-namespace".to_string(),
    );

    // Spawn actor
    node.clone()
        .spawn(
            &ctx1,
            &actor_id1_typed,
            "GenServer",
            vec![],
            None,
            std::collections::HashMap::new(),
            vec![Box::new(lock_facet) as Box<dyn plexspaces_facet::Facet>],
        )
        .await
        .expect("Failed to spawn actor");

    // Get ActorRef from registry
    let node_clone1 = node.clone();
    let actor_registry1 = node_clone1
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry should be available");
    assert!(actor_registry1
        .lookup_actor(&actor_id1_typed)
        .await
        .is_some());
    let actor_ref1 = ActorRef::remote(
        actor_id1_typed.clone(),
        String::new(),
        String::new(),
        node_clone1.id().as_str().to_string(),
        node_clone1.service_locator().clone(),
    );

    // ACT: First actor acquires lock
    let acquire_msg = Message::json(&json!({
        "lock_key": "test-resource-2",
        "holder_id": actor_id1.clone(),
        "lease_duration_secs": 30
    }))
    .expect("Failed to create message")
    .with_message_type("acquire_lock");

    actor_ref1
        .ask(acquire_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to acquire lock");

    // ACT: Second actor tries to acquire (should fail - already held)
    let lock_facet2 = LockFacet::new(
        Arc::new(LockManagerAdapter {
            inner: lock_manager,
        }),
        json!({}),
        50,
    );

    let actor_name2 = format!("test-actor-2-{}", ulid::Ulid::new());
    let actor_id2 = format!("{}@{}", actor_name2, node_id);
    let actor_id2_typed = ActorId::from(actor_id2.clone());
    let ctx2 = plexspaces_core::RequestContext::new_without_auth(
        "test-tenant".to_string(),
        "test-namespace".to_string(),
    );

    // Spawn actor
    node.clone()
        .spawn(
            &ctx2,
            &actor_id2_typed,
            "GenServer",
            vec![],
            None,
            std::collections::HashMap::new(),
            vec![Box::new(lock_facet2) as Box<dyn plexspaces_facet::Facet>],
        )
        .await
        .expect("Failed to spawn actor");

    // Get ActorRef from registry
    let node_clone2 = node.clone();
    let actor_registry2 = node_clone2
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry should be available");
    assert!(actor_registry2
        .lookup_actor(&actor_id2_typed)
        .await
        .is_some());
    let actor_ref2 = ActorRef::remote(
        actor_id2_typed.clone(),
        String::new(),
        String::new(),
        node_clone2.id().as_str().to_string(),
        node_clone2.service_locator().clone(),
    );

    let try_acquire_msg = Message::json(&json!({
        "lock_key": "test-resource-2",
        "holder_id": actor_id2.clone(),
        "lease_duration_secs": 30
    }))
    .expect("Failed to create message")
    .with_message_type("try_acquire_lock");

    let try_reply = actor_ref2
        .ask(
            try_acquire_msg.to_proto(),
            std::time::Duration::from_secs(5),
        )
        .await
        .expect("Failed to try acquire lock");

    // ASSERT: Should return not acquired
    let response: serde_json::Value =
        serde_json::from_slice(&try_reply.payload).expect("Failed to parse try_acquire response");
    assert_eq!(response["acquired"], false);
}

/// Test: Rust actor with LockFacet - get_lock (query lock state)
#[tokio::test]
async fn test_rust_actor_lock_facet_get_lock() {
    init_test_tracing();

    // ARRANGE: Get shared node
    let node = get_shared_node().await;

    let lock_manager = node
        .service_locator()
        .get_lock_manager()
        .await
        .expect("LockManager should be registered");

    let lock_facet = LockFacet::new(
        Arc::new(LockManagerAdapter {
            inner: lock_manager,
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

    // Spawn actor and get ActorRef for ask() pattern
    node.spawn(
        &ctx,
        &actor_id_typed,
        "GenServer",
        vec![],
        None,
        std::collections::HashMap::new(),
        vec![Box::new(lock_facet) as Box<dyn plexspaces_facet::Facet>],
    )
    .await
    .expect("Failed to spawn actor");

    // Get ActorRef after spawning (waits for registration)
    let actor_ref = get_actor_ref_after_spawn(&node, &actor_id_typed).await;

    // ACT: Get lock that doesn't exist
    let get_msg = Message::json(&json!("test-resource-3"))
        .expect("Failed to create message")
        .with_message_type("get_lock");

    let get_reply = actor_ref
        .ask(get_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to get lock");

    // ASSERT: Should return not found
    let response: serde_json::Value =
        serde_json::from_slice(&get_reply.payload).expect("Failed to parse get_lock response");
    assert_eq!(response["found"], false);

    // ACT: Acquire lock, then get it
    let acquire_msg = Message::json(&json!({
        "lock_key": "test-resource-3",
        "holder_id": actor_id.clone(),
        "lease_duration_secs": 30
    }))
    .expect("Failed to create message")
    .with_message_type("acquire_lock");

    actor_ref
        .ask(acquire_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to acquire lock");

    // ACT: Get lock again
    let get_msg2 = Message::json(&json!("test-resource-3"))
        .expect("Failed to create message")
        .with_message_type("get_lock");

    let get_reply2 = actor_ref
        .ask(get_msg2.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to get lock");

    // ASSERT: Should return lock
    let lock_json: serde_json::Value = serde_json::from_slice(&get_reply2.payload)
        .expect("Failed to parse lock from get_lock response");
    assert_eq!(lock_json["lock_key"], "test-resource-3");
    assert_eq!(lock_json["holder_id"], actor_id);
}

/// Adapter that converts core::LockManager to facet::LockManager trait
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

/// Test: Verify LockFacet is created from proto configuration via FacetRegistry
/// This tests the actual flow used by applications (like task-queue)
#[tokio::test]
async fn test_lock_facet_from_proto_config() {
    init_test_tracing();

    // ARRANGE: Get shared node (this initializes FacetRegistry with factories)
    let node = get_shared_node().await;
    node.initialize_services()
        .await
        .expect("Failed to initialize services");

    // Get FacetRegistry from ServiceLocator
    let service_locator = node.service_locator();
    let facet_registry_wrapper = service_locator
        .get_facet_registry()
        .await
        .expect("FacetRegistry should be registered");

    let facet_registry = facet_registry_wrapper.inner_clone();

    // Verify "locks" facet type is registered
    let registered_types = facet_registry.list_types();
    assert!(
        registered_types.contains(&"locks".to_string()),
        "LockFacetFactory should be registered. Found types: {:?}",
        registered_types
    );

    // ACT: Create LockFacet from proto configuration (simulating app-config.toml)
    use plexspaces_proto::common::v1::Facet as ProtoFacet;
    use std::collections::HashMap;
    let proto_facet = ProtoFacet {
        r#type: "locks".to_string(),
        config: HashMap::new(), // Empty config like in app-config.toml
        priority: 50,
        state: HashMap::new(),
        metadata: None,
    };

    // Use create_facet_from_proto helper (same as application deployment)
    let lock_facet = create_facet_from_proto(&proto_facet, &facet_registry)
        .await
        .expect("Failed to create LockFacet from proto");

    // ASSERT: Facet should be LockFacet
    assert_eq!(lock_facet.facet_type(), "locks");

    // ACT: Spawn actor with facet created from proto
    let node_id = node.id();
    let actor_name = format!("test-actor-proto-{}", ulid::Ulid::new());
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
        vec![lock_facet],
    )
    .await
    .expect("Failed to spawn actor with LockFacet from proto");

    // Get ActorRef
    let actor_ref = get_actor_ref_after_spawn(&node, &actor_id_typed).await;

    // ACT: Send acquire_lock message - facet should intercept
    let acquire_msg = Message::json(&json!({
        "lock_key": "test-resource-proto",
        "holder_id": actor_id.clone(),
        "lease_duration_secs": 30
    }))
    .expect("Failed to create message")
    .with_message_type("acquire_lock");

    let reply = actor_ref
        .ask(acquire_msg.to_proto(), std::time::Duration::from_secs(5))
        .await
        .expect("Failed to acquire lock");

    // ASSERT: Should receive lock data (facet intercepted and handled)
    let lock_json: serde_json::Value =
        serde_json::from_slice(&reply.payload).expect("Failed to parse lock from reply");
    assert_eq!(lock_json["lock_key"], "test-resource-proto");
    assert_eq!(lock_json["holder_id"], actor_id);
    assert!(!lock_json["version"].as_str().unwrap().is_empty());
}
