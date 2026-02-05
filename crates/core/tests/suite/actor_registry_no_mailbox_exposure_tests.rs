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

use plexspaces_core::{ActorRegistry, ActorId, actor_context::ObjectRegistry, MessageSender, Message, RequestContext};
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
    messages: Arc<tokio::sync::RwLock<Vec<Message>>>,
}

impl TestMessageSender {
    fn new(actor_id: ActorId) -> Self {
        Self {
            actor_id,
            messages: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl MessageSender for TestMessageSender {
    async fn tell(&self, message: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.messages.write().await.push(message);
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
    ) -> Result<Option<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(ctx, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
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
    ) -> Result<Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .discover(ctx, object_type, name_pattern, tags, metadata, health_status, offset, limit)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .register(ctx, registration)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
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
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
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
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }
}

async fn create_test_registry() -> Arc<ActorRegistry> {
    let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
    let object_registry: Arc<dyn ObjectRegistry> = Arc::new(ObjectRegistryAdapter { 
        inner: object_registry_impl 
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
    registry.register_actor(&ctx, actor_id.clone(), sender, None, None, None, None).await;
    
    // Verify actor is registered
    let found = registry.lookup_actor(&actor_id).await;
    assert!(found.is_some(), "Actor should be registered");
    
    // Verify is_actor_activated works
    assert!(registry.is_actor_activated(&actor_id).await, "Actor should be activated");
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
    registry.register_actor(&ctx, actor_id.clone(), sender, None, None, None, None).await;
    
    // Verify we can send messages via MessageSender
    let sender = registry.lookup_actor(&actor_id).await.unwrap();
    let message = create_test_message(vec![1, 2, 3]);
    let result = sender.tell(message).await;
    if let Err(e) = &result {
        eprintln!("Error sending message: {}", e);
    }
    assert!(result.is_ok(), "Should be able to send message via MessageSender, got error: {:?}", result.err());
    
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
    registry.register_actor(&ctx, actor_id.clone(), sender, None, None, None, None).await;
    
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
    registry.register_actor(&ctx, actor_id.clone(), sender, None, None, None, None).await;
    
    // Now activated
    assert!(registry.is_actor_activated(&actor_id).await);
}

#[tokio::test]
async fn test_multiple_actors_registration() {
    let registry = create_test_registry().await;
    
    // Register multiple actors
    for i in 0..10 {
        let actor_id: ActorId = format!("actor-{}@test-node", i);
        let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender::new(actor_id.clone()));
        let ctx = create_test_context();
        registry.register_actor(&ctx, actor_id.clone(), sender, None, None, None, None).await;
    }
    
    // Verify all are registered
    for i in 0..10 {
        let actor_id: ActorId = format!("actor-{}@test-node", i);
        assert!(registry.is_actor_activated(&actor_id).await, "Actor {} should be activated", i);
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
            Some(actor_type.clone()),
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
            Some(actor_type.clone()),
            None,
            None,
            None,
        )
        .await;

    let found1 = registry.discover_actors_by_type(&ctx1, &actor_type).await;
    let found2 = registry.discover_actors_by_type(&ctx2, &actor_type).await;

    assert_eq!(found1.len(), 1, "namespace term1 must resolve to exactly one actor");
    assert_eq!(found2.len(), 1, "namespace term2 must resolve to exactly one actor");
    assert_ne!(
        found1[0], found2[0],
        "term1 and term2 must resolve to different actors (leader-election routing)"
    );
    assert_eq!(found1[0], actor_id1);
    assert_eq!(found2[0], actor_id2);
}
