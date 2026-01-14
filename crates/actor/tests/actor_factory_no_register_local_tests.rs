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
// GNU Lesser General License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Tests for ActorFactoryImpl without register_local
//!
//! These tests verify that:
//! 1. ActorFactoryImpl only uses register_actor (not register_local)
//! 2. Mailbox is not exposed via registry
//! 3. Only MessageSender is registered

use async_trait::async_trait;
use plexspaces_actor::{Actor, ActorBuilder, ActorFactory, actor_factory_impl::ActorFactoryImpl, ActorRef};
use plexspaces_core::{ActorId, ActorRegistry, ServiceLocator, VirtualActorManager, FacetManagerServiceWrapper, Actor as ActorTrait, BehaviorType, BehaviorError, ActorContext, actor_context::ObjectRegistry, RequestContext};
use plexspaces_facet::FacetManager;
use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_mailbox::Message;
use plexspaces_object_registry::ObjectRegistry as ObjectRegistryImpl;
use plexspaces_proto::object_registry::v1::{ObjectType, ObjectRegistration, HealthStatus};
use std::sync::Arc;

// Adapter to convert ObjectRegistryImpl to ObjectRegistry trait
struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistryImpl>,
}

#[async_trait::async_trait]
impl ObjectRegistry for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(ctx, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }
    
    async fn discover(
        &self,
        _ctx: &plexspaces_core::RequestContext,
        _object_type: Option<ObjectType>,
        _object_category: Option<String>,
        _capabilities: Option<Vec<String>>,
        _labels: Option<Vec<String>>,
        _health_status: Option<HealthStatus>,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Vec::new())
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
        ctx: &plexspaces_core::RequestContext,
        registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .register(ctx, registration)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

}

struct TestBehavior;

#[async_trait]
impl ActorTrait for TestBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _msg: plexspaces_mailbox::Message,
    ) -> Result<(), BehaviorError> {
        Ok(())
    }
    
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

async fn create_test_service_locator() -> Arc<ServiceLocator> {
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    
    // Create ObjectRegistry for ActorRegistry
    let kv = Arc::new(InMemoryKVStore::new());
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(kv));
    let object_registry: Arc<dyn ObjectRegistry> = Arc::new(ObjectRegistryAdapter { 
        inner: object_registry_impl 
    });
    
    // Register ActorRegistry
    let registry = Arc::new(ActorRegistry::new(object_registry, "test-node".to_string()));
    service_locator.register_service(registry.clone()).await;
    
    // Register VirtualActorManager
    let manager = Arc::new(VirtualActorManager::new(registry.clone()));
    service_locator.register_service(manager.clone()).await;
    
    // Register FacetManager (wrapped in FacetManagerServiceWrapper to implement Service trait)
    let facet_manager = Arc::new(FacetManager::new());
    let facet_manager_wrapper = Arc::new(FacetManagerServiceWrapper::new(facet_manager.clone()));
    service_locator.register_service(facet_manager_wrapper).await;
    
    service_locator
}

#[tokio::test]
async fn test_spawn_built_actor_registers_message_sender_only() {
    let service_locator = create_test_service_locator().await;
    let factory = ActorFactoryImpl::new(service_locator.clone());
    
    // Get ActorRegistry to verify registration
    let registry: Arc<ActorRegistry> = service_locator.actor_registry().await.unwrap();
    
    // Spawn actor using spawn_actor
    let actor_id: ActorId = "test-actor@test-node".to_string();
    let ctx = plexspaces_core::RequestContext::new_without_auth("internal".to_string(), "system".to_string());
    let message_sender = factory.spawn_actor(
        &ctx,
        &actor_id,
        "GenServer", // actor_type from TestBehavior
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![], // facets
    ).await.unwrap();
    
    // Verify actor is registered (via MessageSender, not mailbox)
    let actor_id: ActorId = "test-actor@test-node".to_string();
    assert!(registry.is_actor_activated(&actor_id).await, "Actor should be activated");
    
    // Verify we can lookup MessageSender
    let found_sender = registry.lookup_actor(&actor_id).await;
    assert!(found_sender.is_some(), "MessageSender should be registered");
    
    // Verify we can send messages
    let message = Message::new(vec![1, 2, 3]);
    let result = message_sender.tell(message).await;
    assert!(result.is_ok(), "Should be able to send message");
}

#[tokio::test]
async fn test_spawn_actor_registers_message_sender_only() {
    let service_locator = create_test_service_locator().await;
    let factory = ActorFactoryImpl::new(service_locator.clone());
    
    // Get ActorRegistry to verify registration
    let registry: Arc<ActorRegistry> = service_locator.actor_registry().await.unwrap();
    
    // Spawn actor
    let actor_id: ActorId = "test-actor@test-node".to_string();
    let ctx = plexspaces_core::RequestContext::new_without_auth("internal".to_string(), "system".to_string());
    let message_sender = factory.spawn_actor(
        &ctx,
        &actor_id,
        "test",
        vec![],
        None,
        std::collections::HashMap::new(),
        vec![], // facets
    ).await.unwrap();
    
    // Verify actor is registered
    assert!(registry.is_actor_activated(&actor_id).await);
    
    // Verify MessageSender works
    let message = Message::new(vec![1, 2, 3]);
    let result = message_sender.tell(message).await;
    assert!(result.is_ok(), "Should be able to send message");
}

#[tokio::test]
async fn test_multiple_actors_spawned_via_factory() {
    let service_locator = create_test_service_locator().await;
    let factory = ActorFactoryImpl::new(service_locator.clone());
    let registry: Arc<ActorRegistry> = service_locator.actor_registry().await.unwrap();
    
    // Spawn multiple actors using spawn_actor
    let ctx = plexspaces_core::RequestContext::new_without_auth("internal".to_string(), "system".to_string());
    for i in 0..5 {
        let actor_id: ActorId = format!("actor-{}@test-node", i);
        factory.spawn_actor(
            &ctx,
            &actor_id,
            "GenServer", // actor_type from TestBehavior
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![], // facets
        ).await.unwrap();
        
        // Verify each is registered
        assert!(registry.is_actor_activated(&actor_id).await, "Actor {} should be activated", i);
    }
}
