// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Tests for ActorRegistry without mailbox exposure
//!
//! These tests verify that:
//! 1. Only MessageSender is registered (not mailbox directly)
//! 2. Mailbox is truly internal
//! 3. register_local() has been removed
//! 4. is_actor_activated() checks MessageSender, not mailbox

use plexspaces_core::{
    actor_context::ObjectRegistry, actor_id::build_actor_id, ActorId, ActorRegistry, Message,
    MessageSender, RequestContext, VirtualActorManager,
};
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use ulid::Ulid;

// Atomic counter for generating unique test IDs
static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Helper to create a test message
fn create_test_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

// Simple MessageSender implementation for testing
struct TestMessageSender {
    actor_id: ActorId,
    tenant_id: String,
    namespace: String,
    actor_type: std::sync::RwLock<Option<String>>,
    local_state_handle: std::sync::RwLock<Option<Arc<dyn plexspaces_core::ActorStateHandle>>>,
    messages: Arc<tokio::sync::RwLock<Vec<Message>>>,
}

impl TestMessageSender {
    fn new(actor_id: ActorId) -> Self {
        Self {
            actor_id,
            tenant_id: String::new(),
            namespace: String::new(),
            actor_type: std::sync::RwLock::new(None),
            local_state_handle: std::sync::RwLock::new(None),
            messages: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    fn with_scope(mut self, tenant_id: impl Into<String>, namespace: impl Into<String>) -> Self {
        self.tenant_id = tenant_id.into();
        self.namespace = namespace.into();
        self
    }
}

#[async_trait::async_trait]
impl MessageSender for TestMessageSender {
    async fn tell(&self, message: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.messages.write().await.push(message);
        Ok(())
    }

    fn actor_id(&self) -> Option<String> {
        Some(self.actor_id.clone())
    }

    fn tenant_id(&self) -> Option<&str> {
        Some(&self.tenant_id)
    }

    fn namespace(&self) -> Option<&str> {
        Some(&self.namespace)
    }

    fn actor_type(&self) -> Option<String> {
        self.actor_type.read().ok().and_then(|guard| guard.clone())
    }

    async fn set_actor_type(&self, actor_type: Option<String>) {
        if let Ok(mut guard) = self.actor_type.write() {
            *guard = actor_type;
        }
    }

    fn local_state_handle(&self) -> Option<Arc<dyn plexspaces_core::ActorStateHandle>> {
        self.local_state_handle
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    async fn set_local_state_handle(
        &self,
        handle: Option<Arc<dyn plexspaces_core::ActorStateHandle>>,
    ) {
        if let Ok(mut guard) = self.local_state_handle.write() {
            *guard = handle;
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct TestActorStateHandle;

#[async_trait::async_trait]
impl plexspaces_core::ActorStateHandle for TestActorStateHandle {
    async fn actor_state(&self) -> plexspaces_proto::v1::actor::ActorState {
        plexspaces_proto::v1::actor::ActorState::ActorStateActive
    }

    async fn stop_actor(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

// Adapter to convert ObjectRegistryImpl to ObjectRegistry trait
struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistryImpl>,
}

#[async_trait::async_trait]
impl ObjectRegistry for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<
        Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let obj_type = object_type
            .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(ctx, obj_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<
        Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn discover(
        &self,
        ctx: &RequestContext,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        name_pattern: Option<String>,
        tags: Option<Vec<String>>,
        metadata: Option<Vec<String>>,
        health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        offset: usize,
        limit: usize,
    ) -> Result<
        Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.inner
            .discover(
                ctx,
                object_type,
                name_pattern,
                tags,
                metadata,
                health_status,
                offset,
                limit,
            )
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner.register(ctx, registration).await.map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )) as Box<dyn std::error::Error + Send + Sync>
        })
    }

    async fn unregister(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .unregister(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn heartbeat(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .heartbeat(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }
}

async fn create_test_registry() -> Arc<ActorRegistry> {
    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
    let object_registry: Arc<dyn ObjectRegistry> = Arc::new(ObjectRegistryAdapter {
        inner: object_registry_impl,
    });
    Arc::new(ActorRegistry::new(object_registry, "test-node".to_string()))
}

/// Helper to create test RequestContext with proper tenant/namespace isolation
/// Generates unique tenant/namespace per test to allow concurrent test execution
fn create_test_context() -> RequestContext {
    let test_id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    RequestContext::new_without_auth(
        format!("test-tenant-{}", test_id),
        format!("test-namespace-{}", test_id),
    )
}

#[tokio::test]
async fn test_register_actor_with_message_sender() {
    let registry = create_test_registry().await;
    let actor_id: ActorId = "test-actor@test-node".to_string();

    // Create MessageSender
    let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender::new(actor_id.clone()));

    // Register actor with MessageSender
    let ctx = create_test_context();
    registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            sender,
            "TestActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    // Verify actor is registered
    let found = registry.lookup_actor(&actor_id).await;
    assert!(found.is_some(), "Actor should be registered");

    // Verify is_actor_activated works
    assert!(
        registry.is_actor_activated(&actor_id).await,
        "Actor should be activated"
    );
}

#[tokio::test]
async fn test_register_actor_mailbox_not_exposed() {
    let registry = create_test_registry().await;
    let actor_id: ActorId = "test-actor@test-node".to_string();

    // Create MessageSender with internal message storage
    let test_sender = Arc::new(TestMessageSender::new(actor_id.clone()));
    let messages = test_sender.messages.clone();
    let sender: Arc<dyn MessageSender> = test_sender;

    // Register actor
    let ctx = create_test_context();
    registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            sender,
            "TestActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    // Verify we can send messages via MessageSender
    let sender = registry.lookup_actor(&actor_id).await.unwrap();
    let message = create_test_message(vec![1, 2, 3]);
    let result = sender.tell(message).await;
    if let Err(e) = &result {
        eprintln!("Error sending message: {}", e);
    }
    assert!(
        result.is_ok(),
        "Should be able to send message via MessageSender, got error: {:?}",
        result.err()
    );

    // Verify message was delivered
    let received = messages.read().await;
    assert_eq!(received.len(), 1, "Message should be stored");
}

#[tokio::test]
async fn test_unregister_actor_removes_message_sender() {
    let registry = create_test_registry().await;
    let actor_id: ActorId = "test-actor@test-node".to_string();

    // Register actor
    let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender::new(actor_id.clone()));
    let ctx = create_test_context();
    registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            sender,
            "TestActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    // Verify registered
    assert!(registry.is_actor_activated(&actor_id).await);

    // Unregister
    registry.unregister(&actor_id).await.unwrap();

    // Verify unregistered
    assert!(!registry.is_actor_activated(&actor_id).await);
    assert!(registry.lookup_actor(&actor_id).await.is_none());
}

#[tokio::test]
async fn test_is_actor_activated_checks_message_sender() {
    let registry = create_test_registry().await;
    let actor_id: ActorId = "test-actor@test-node".to_string();

    // Initially not activated
    assert!(!registry.is_actor_activated(&actor_id).await);

    // Register actor
    let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender::new(actor_id.clone()));
    let ctx = create_test_context();
    registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            sender,
            "TestActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    // Now activated
    assert!(registry.is_actor_activated(&actor_id).await);
}

#[tokio::test]
async fn test_registry_reads_scope_from_registered_sender() {
    let registry = create_test_registry().await;
    let actor_id: ActorId = "scoped-actor@test-node".to_string();
    let ctx = RequestContext::new_without_auth("tenant-a".to_string(), "ns-a".to_string());

    let sender: Arc<dyn MessageSender> =
        Arc::new(TestMessageSender::new(actor_id.clone()).with_scope("tenant-a", "ns-a"));
    registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            sender,
            "ScopedActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    let metadata = registry
        .get_actor_metadata(&actor_id)
        .await
        .expect("scope should be readable from registered sender");
    assert_eq!(metadata, ("tenant-a".to_string(), "ns-a".to_string()));
    assert_eq!(
        registry.get_actor_type(&actor_id).await,
        Some("ScopedActor".to_string())
    );

    let sender = registry.lookup_actor(&actor_id).await.unwrap();
    assert_eq!(sender.actor_type(), Some("ScopedActor".to_string()));
}

#[tokio::test]
async fn test_registry_reads_virtual_actor_scope_and_type_from_manager_metadata() {
    let registry = create_test_registry().await;
    let manager = Arc::new(VirtualActorManager::new(registry.clone()));
    registry.set_virtual_actor_manager(manager.clone()).await;

    manager
        .register_virtual_actor_type(
            "shopping-cart".to_string(),
            None,
            "shop-ns".to_string(),
            serde_json::json!({
                "virtual_actor": {
                    "activation_strategy": "lazy"
                },
                "durability": {}
            }),
            Some("tenant-shop".to_string()),
            None,
        )
        .await
        .expect("virtual actor type registration should succeed");

    let actor_id = build_actor_id("cart-1", "shopping-cart", Some("shop-ns"), "test-node");

    let metadata = registry
        .get_actor_metadata(&actor_id)
        .await
        .expect("virtual actor metadata should be readable from virtual actor manager");
    assert_eq!(metadata, ("tenant-shop".to_string(), "shop-ns".to_string()));
    assert_eq!(
        registry.get_actor_type(&actor_id).await,
        Some("shopping-cart".to_string())
    );
    assert!(registry.lookup_actor(&actor_id).await.is_none());
}

#[tokio::test]
async fn test_lookup_actor_in_scope_isolates_same_actor_id_across_scopes() {
    let registry = create_test_registry().await;
    let actor_id: ActorId = "shared-actor@test-node".to_string();

    let ctx_a = RequestContext::new_without_auth("tenant-a".to_string(), "ns-a".to_string());
    let sender_a: Arc<dyn MessageSender> =
        Arc::new(TestMessageSender::new(actor_id.clone()).with_scope("tenant-a", "ns-a"));
    registry
        .register_actor(
            &ctx_a,
            actor_id.clone(),
            sender_a,
            "ScopedActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    let ctx_b = RequestContext::new_without_auth("tenant-b".to_string(), "ns-b".to_string());
    let sender_b: Arc<dyn MessageSender> =
        Arc::new(TestMessageSender::new(actor_id.clone()).with_scope("tenant-b", "ns-b"));
    registry
        .register_actor(
            &ctx_b,
            actor_id.clone(),
            sender_b,
            "ScopedActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    let scoped_a = registry
        .lookup_actor_in_scope("tenant-a", "ns-a", &actor_id)
        .await
        .expect("tenant-a scoped actor should exist");
    assert_eq!(scoped_a.tenant_id(), Some("tenant-a"));
    assert_eq!(scoped_a.namespace(), Some("ns-a"));

    let scoped_b = registry
        .lookup_actor_in_scope("tenant-b", "ns-b", &actor_id)
        .await
        .expect("tenant-b scoped actor should exist");
    assert_eq!(scoped_b.tenant_id(), Some("tenant-b"));
    assert_eq!(scoped_b.namespace(), Some("ns-b"));

    assert!(
        registry.lookup_actor(&actor_id).await.is_none(),
        "flat lookup must not return an arbitrary actor when multiple scopes share the same actor id"
    );
}

#[tokio::test]
async fn test_live_actor_helpers_preserve_scope_isolation() {
    let registry = create_test_registry().await;
    let shared_actor_id: ActorId = "shared-actor@test-node".to_string();

    let ctx_a = RequestContext::new_without_auth("tenant-a".to_string(), "ns-a".to_string());
    let sender_a: Arc<dyn MessageSender> =
        Arc::new(TestMessageSender::new(shared_actor_id.clone()).with_scope("tenant-a", "ns-a"));
    registry
        .register_actor(
            &ctx_a,
            shared_actor_id.clone(),
            sender_a,
            "ScopedActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    let ctx_b = RequestContext::new_without_auth("tenant-b".to_string(), "ns-b".to_string());
    let sender_b: Arc<dyn MessageSender> =
        Arc::new(TestMessageSender::new(shared_actor_id.clone()).with_scope("tenant-b", "ns-b"));
    registry
        .register_actor(
            &ctx_b,
            shared_actor_id.clone(),
            sender_b,
            "ScopedActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    let live_entries = registry.live_actor_entries().await;
    assert_eq!(live_entries.len(), 2);
    assert!(live_entries.contains(&(
        "tenant-a".to_string(),
        "ns-a".to_string(),
        shared_actor_id.clone()
    )));
    assert!(live_entries.contains(&(
        "tenant-b".to_string(),
        "ns-b".to_string(),
        shared_actor_id.clone()
    )));

    assert_eq!(registry.live_actor_count().await, 2);

    let live_tenant_ids = registry.live_tenant_ids().await;
    assert_eq!(live_tenant_ids.len(), 2);
    assert!(live_tenant_ids.contains("tenant-a"));
    assert!(live_tenant_ids.contains("tenant-b"));
}

#[tokio::test]
async fn test_registered_inventory_helpers_include_passivated_virtual_entries() {
    let registry = create_test_registry().await;
    let actor_id = build_actor_id("cart-1", "shopping-cart", Some("shop-ns"), "test-node");
    let ctx = RequestContext::new_without_auth("tenant-shop".to_string(), "shop-ns".to_string());

    registry
        .register_virtual_actor_index(&ctx, actor_id.clone(), "shopping-cart".to_string())
        .await;

    let registered_entries = registry.registered_actor_entries().await;
    assert!(registered_entries.contains(&(
        "tenant-shop".to_string(),
        "shop-ns".to_string(),
        actor_id.clone()
    )));
    assert_eq!(registry.registered_actor_count().await, 1);
    assert_eq!(registry.registered_actor_ids().await.len(), 1);

    let registered_tenant_ids = registry.registered_tenant_ids().await;
    assert_eq!(registered_tenant_ids.len(), 1);
    assert!(registered_tenant_ids.contains("tenant-shop"));
}

#[tokio::test]
async fn test_multiple_actors_registration() {
    let registry = create_test_registry().await;

    // Register multiple actors
    for i in 0..10 {
        let actor_id: ActorId = format!("actor-{}@test-node", i);
        let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender::new(actor_id.clone()));
        let ctx = create_test_context();
        registry
            .register_actor(
                &ctx,
                actor_id.clone(),
                sender,
                "TestActor".to_string(),
                None,
                None,
                None,
            )
            .await;
    }

    // Verify all are registered
    for i in 0..10 {
        let actor_id: ActorId = format!("actor-{}@test-node", i);
        assert!(
            registry.is_actor_activated(&actor_id).await,
            "Actor {} should be activated",
            i
        );
    }
}

/// Leader-election routing: discover_actors_by_type must return different actors
/// for different namespaces (leader-election-term1 vs leader-election-term2).
/// If both namespaces returned the same actor, both try_lead requests would hit
/// one actor and both would see leader:true (bug).
#[tokio::test]
async fn test_leader_election_discover_actors_by_namespace() {
    let registry = create_test_registry().await;
    let tenant = "".to_string();
    let ns1 = "leader-election-term1".to_string();
    let ns2 = "leader-election-term2".to_string();
    let actor_type = "LeaderElection".to_string();

    let actor_id1: ActorId = "LeaderElection:leader-election-term1@test-node".to_string();
    let actor_id2: ActorId = "LeaderElection:leader-election-term2@test-node".to_string();

    let ctx1 = RequestContext::new_without_auth(tenant.clone(), ns1.clone());
    let ctx2 = RequestContext::new_without_auth(tenant.clone(), ns2.clone());

    registry
        .register_actor(
            &ctx1,
            actor_id1.clone(),
            Arc::new(TestMessageSender::new(actor_id1.clone())),
            actor_type.clone(),
            None,
            None,
            None,
        )
        .await;
    registry
        .register_actor(
            &ctx2,
            actor_id2.clone(),
            Arc::new(TestMessageSender::new(actor_id2.clone())),
            actor_type.clone(),
            None,
            None,
            None,
        )
        .await;

    let found1 = registry.discover_actors_by_type(&ctx1, &actor_type).await;
    let found2 = registry.discover_actors_by_type(&ctx2, &actor_type).await;

    assert_eq!(
        found1.len(),
        1,
        "namespace term1 must resolve to exactly one actor"
    );
    assert_eq!(
        found2.len(),
        1,
        "namespace term2 must resolve to exactly one actor"
    );
    assert_ne!(
        found1[0], found2[0],
        "term1 and term2 must resolve to different actors (leader-election routing)"
    );
    assert_eq!(found1[0], actor_id1);
    assert_eq!(found2[0], actor_id2);
}

#[tokio::test]
async fn test_register_actor_deduplicates_actor_type_index_entries() {
    let registry = create_test_registry().await;
    let ctx = create_test_context();
    let actor_type = "leader".to_string();
    let actor_id: ActorId = "01TEST//leader::test@test-node".to_string();
    let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender::new(actor_id.clone()));

    registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            sender.clone(),
            actor_type.clone(),
            None,
            None,
            None,
        )
        .await;
    registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            sender,
            actor_type.clone(),
            None,
            None,
            None,
        )
        .await;

    let discovered = registry.discover_actors_by_type(&ctx, &actor_type).await;
    assert_eq!(discovered, vec![actor_id]);
}

#[tokio::test]
async fn test_register_actor_preserves_existing_local_state_handle() {
    let registry = create_test_registry().await;
    let ctx = create_test_context();
    let actor_id: ActorId = "preserve-handle@test-node".to_string();
    let state_handle: Arc<dyn plexspaces_core::ActorStateHandle> = Arc::new(TestActorStateHandle);

    registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            Arc::new(TestMessageSender::new(actor_id.clone())),
            "TestActor".to_string(),
            None,
            Some(state_handle.clone()),
            None,
        )
        .await;

    registry
        .register_actor(
            &ctx,
            actor_id.clone(),
            Arc::new(TestMessageSender::new(actor_id.clone())),
            "TestActor".to_string(),
            None,
            None,
            None,
        )
        .await;

    let preserved = registry
        .get_actor_instance(&actor_id)
        .await
        .expect("state handle should survive handle-less re-registration");
    assert_eq!(
        preserved.actor_state().await,
        plexspaces_proto::v1::actor::ActorState::ActorStateActive
    );
}
