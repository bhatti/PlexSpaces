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

use plexspaces_core::{ActorRegistry, ActorId, actor_context::ObjectRegistry, MessageSender};
use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_mailbox::{Mailbox, MailboxConfig, Message};
use plexspaces_object_registry::ObjectRegistry as ObjectRegistryImpl;
use std::sync::Arc;

// Simple MessageSender implementation for testing
struct TestMessageSender {
    actor_id: ActorId,
    mailbox: Arc<Mailbox>,
}

#[async_trait::async_trait]
impl MessageSender for TestMessageSender {
    async fn tell(&self, message: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.mailbox.enqueue(message).await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
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
        tenant_id: &str,
        object_id: &str,
        namespace: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        let ctx = plexspaces_core::RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
        self.inner
            .lookup(&ctx, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn lookup_full(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn register(
        &self,
        registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .register(registration)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

}

fn create_test_registry() -> Arc<ActorRegistry> {
    let kv = Arc::new(InMemoryKVStore::new());
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(kv));
    let object_registry: Arc<dyn ObjectRegistry> = Arc::new(ObjectRegistryAdapter { 
        inner: object_registry_impl 
    });
    Arc::new(ActorRegistry::new(object_registry, "test-node".to_string()))
}

#[tokio::test]
async fn test_register_actor_with_message_sender() {
    let registry = create_test_registry();
    let actor_id: ActorId = "test-actor@test-node".to_string();
    
    // Create mailbox with larger capacity and MessageSender
    let mut mailbox_config = MailboxConfig::default();
    mailbox_config.capacity = 1000;
    let mailbox = Arc::new(Mailbox::new(mailbox_config, actor_id.clone()).await.unwrap());
    let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender {
        actor_id: actor_id.clone(),
        mailbox: mailbox.clone(),
    });
    
    // Register actor with MessageSender
    let ctx = plexspaces_core::RequestContext::internal();
    registry.register_actor(&ctx, actor_id.clone(), sender, None).await;
    
    // Verify actor is registered
    let found = registry.lookup_actor(&actor_id).await;
    assert!(found.is_some(), "Actor should be registered");
    
    // Verify is_actor_activated works
    assert!(registry.is_actor_activated(&actor_id).await, "Actor should be activated");
}

#[tokio::test]
async fn test_register_actor_mailbox_not_exposed() {
    let registry = create_test_registry();
    let actor_id: ActorId = "test-actor@test-node".to_string();
    
    // Create mailbox with larger capacity and MessageSender
    let mut mailbox_config = MailboxConfig::default();
    mailbox_config.capacity = 1000;
    let mailbox = Arc::new(Mailbox::new(mailbox_config, actor_id.clone()).await.unwrap());
    let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender {
        actor_id: actor_id.clone(),
        mailbox: mailbox.clone(),
    });
    
    // Register actor
    let ctx = plexspaces_core::RequestContext::internal();
    registry.register_actor(&ctx, actor_id.clone(), sender, None).await;
    
    // Verify we can send messages via MessageSender
    let sender = registry.lookup_actor(&actor_id).await.unwrap();
    let message = Message::new(vec![1, 2, 3]);
    let result = sender.tell(message).await;
    if let Err(e) = &result {
        eprintln!("Error sending message: {}", e);
    }
    assert!(result.is_ok(), "Should be able to send message via MessageSender, got error: {:?}", result.err());
    
    // Verify message was delivered to mailbox
    // Give it a moment for async processing
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    let received = mailbox.dequeue().await;
    assert!(received.is_some(), "Message should be in mailbox");
}

#[tokio::test]
async fn test_unregister_actor_removes_message_sender() {
    let registry = create_test_registry();
    let actor_id: ActorId = "test-actor@test-node".to_string();
    
    // Register actor
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), actor_id.clone()).await.unwrap());
    let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender {
        actor_id: actor_id.clone(),
        mailbox: mailbox.clone(),
    });
    let ctx = plexspaces_core::RequestContext::internal();
    registry.register_actor(&ctx, actor_id.clone(), sender, None).await;
    
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
    let registry = create_test_registry();
    let actor_id: ActorId = "test-actor@test-node".to_string();
    
    // Initially not activated
    assert!(!registry.is_actor_activated(&actor_id).await);
    
    // Register actor
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), actor_id.clone()).await.unwrap());
    let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender {
        actor_id: actor_id.clone(),
        mailbox: mailbox.clone(),
    });
    let ctx = plexspaces_core::RequestContext::internal();
    registry.register_actor(&ctx, actor_id.clone(), sender, None).await;
    
    // Now activated
    assert!(registry.is_actor_activated(&actor_id).await);
}

#[tokio::test]
async fn test_multiple_actors_registration() {
    let registry = create_test_registry();
    
    // Register multiple actors
    for i in 0..10 {
        let actor_id: ActorId = format!("actor-{}@test-node", i);
        let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), actor_id.clone()).await.unwrap());
        let sender: Arc<dyn MessageSender> = Arc::new(TestMessageSender {
            actor_id: actor_id.clone(),
            mailbox: mailbox.clone(),
        });
        let ctx = plexspaces_core::RequestContext::internal();
    registry.register_actor(&ctx, actor_id.clone(), sender, None).await;
    }
    
    // Verify all are registered
    for i in 0..10 {
        let actor_id: ActorId = format!("actor-{}@test-node", i);
        assert!(registry.is_actor_activated(&actor_id).await, "Actor {} should be activated", i);
    }
}
