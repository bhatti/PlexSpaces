// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorRef integration with ServiceLocator (TDD)

use plexspaces_actor::ActorRef;
use plexspaces_core::{ActorRegistry, ServiceLocator, actor_context::ObjectRegistry as ObjectRegistryTrait};
use plexspaces_mailbox::{Message, Mailbox, MailboxConfig};
use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
use std::sync::Arc;
use tokio::time;

// Helper to wrap ObjectRegistry for ActorRegistry
struct ObjectRegistryAdapter {
    inner: Arc<plexspaces_object_registry::ObjectRegistry>,
}

#[async_trait::async_trait]
impl ObjectRegistryTrait for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_id: &str,
        object_type: Option<ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(ctx, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn lookup_full(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: ObjectType,
        object_id: &str,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn register(
        &self,
        ctx: &plexspaces_core::RequestContext,
        registration: ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .register(ctx, registration)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn discover(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: Option<ObjectType>,
        object_category: Option<String>,
        capabilities: Option<Vec<String>>,
        labels: Option<Vec<String>>,
        health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .discover(ctx, object_type, object_category, capabilities, labels, health_status, limit, offset)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }
}

#[tokio::test]
async fn test_actor_ref_remote_uses_service_locator() {
    // Test: Remote ActorRef should use ServiceLocator for gRPC client caching
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let object_registry_impl = Arc::new(plexspaces_object_registry::ObjectRegistry::new(
        Arc::new(plexspaces_keyvalue::InMemoryKVStore::new())
    ));
    let object_registry_trait: Arc<dyn ObjectRegistryTrait> = 
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl });
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        "test-node".to_string(),
    ));
    
    // Register ActorRegistry in ServiceLocator
    service_locator.register_service(actor_registry.clone()).await;
    
    // Create remote ActorRef with ServiceLocator
    let actor_ref = ActorRef::remote(
        "test-actor@remote-node",
        "remote-node",
        service_locator.clone(),
    );
    
    assert!(actor_ref.is_remote());
    assert_eq!(actor_ref.id(), "test-actor@remote-node");
}

#[tokio::test]
async fn test_actor_ref_remote_tell_uses_service_locator() {
    // Test: Remote ActorRef.tell() should use ServiceLocator to get gRPC client
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let object_registry_impl = Arc::new(plexspaces_object_registry::ObjectRegistry::new(
        Arc::new(plexspaces_keyvalue::InMemoryKVStore::new())
    ));
    
    // Register node address in ObjectRegistry
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    let node_registration = ObjectRegistration {
        object_id: "remote-node".to_string(),
        object_type: ObjectType::ObjectTypeNode as i32,
        object_category: "Node".to_string(),
        grpc_address: "http://127.0.0.1:9999".to_string(),
        ..Default::default()
    };
    object_registry_impl.register(&ctx, node_registration).await.unwrap();
    
    // Create ActorRegistry with the ObjectRegistry that has the node registered
    let object_registry_trait: Arc<dyn ObjectRegistryTrait> = 
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl.clone() });
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        "test-node".to_string(),
    ));
    
    // Register ActorRegistry in ServiceLocator
    service_locator.register_service(actor_registry.clone()).await;
    
    // Create remote ActorRef with ServiceLocator
    let actor_ref = ActorRef::remote(
        "test-actor@remote-node",
        "remote-node",
        service_locator.clone(),
    );
    
    // Send message (will fail to connect, but should use ServiceLocator)
    // Use timeout to prevent hanging
    let message = Message::new(b"test".to_vec());
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        actor_ref.tell(message)
    ).await;
    
    // Should fail with timeout or connection error (no server), but should have used ServiceLocator
    match result {
        Ok(Err(e)) => {
            // Connection error is expected
            assert!(e.to_string().contains("Connection") || e.to_string().contains("connection") || e.to_string().contains("Failed") || e.to_string().contains("gRPC"));
        }
        Err(_) => {
            // Timeout is also acceptable - connection attempt was made
        }
        Ok(Ok(_)) => {
            panic!("Expected connection failure, but tell() succeeded");
        }
    }
}

#[tokio::test]
async fn test_actor_ref_remote_ask_uses_service_locator() {
    // Test: Remote ActorRef.ask() should use ServiceLocator to get gRPC client
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let object_registry_impl = Arc::new(plexspaces_object_registry::ObjectRegistry::new(
        Arc::new(plexspaces_keyvalue::InMemoryKVStore::new())
    ));
    
    // Register node address in ObjectRegistry
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    let node_registration = ObjectRegistration {
        object_id: "remote-node".to_string(),
        object_type: ObjectType::ObjectTypeNode as i32,
        object_category: "Node".to_string(),
        grpc_address: "http://127.0.0.1:9999".to_string(),
        ..Default::default()
    };
    object_registry_impl.register(&ctx, node_registration).await.unwrap();
    
    // Create ActorRegistry with the ObjectRegistry that has the node registered
    let object_registry_trait: Arc<dyn ObjectRegistryTrait> = 
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl.clone() });
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        "test-node".to_string(),
    ));
    
    // Register ActorRegistry in ServiceLocator
    service_locator.register_service(actor_registry.clone()).await;
    
    // Create remote ActorRef with ServiceLocator
    let actor_ref = ActorRef::remote(
        "test-actor@remote-node",
        "remote-node",
        service_locator.clone(),
    );
    
    // Send ask request (will fail to connect, but should use ServiceLocator)
    // Use timeout to prevent hanging (ask already has timeout, but wrap in additional timeout for safety)
    let message = Message::new(b"test".to_vec());
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        actor_ref.ask(message, std::time::Duration::from_secs(1))
    ).await;
    
    // Should fail with connection error or timeout (no server), but should have used ServiceLocator
    match result {
        Ok(Err(e)) => {
            // Connection error or timeout is expected
            assert!(e.to_string().contains("Connection") || e.to_string().contains("connection") || e.to_string().contains("Failed") || e.to_string().contains("Timeout") || e.to_string().contains("gRPC"));
        }
        Err(_) => {
            // Outer timeout is also acceptable - connection attempt was made
        }
        Ok(Ok(_)) => {
            panic!("Expected connection failure, but ask() succeeded");
        }
    }
}

#[tokio::test]
async fn test_actor_ref_local_unchanged() {
    // Test: Local ActorRef should work the same (no ServiceLocator needed)
    use plexspaces_mailbox::mailbox_config_default;
    let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap());
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let actor_ref = ActorRef::local("test-actor", mailbox.clone(), service_locator.clone());
    
    assert!(actor_ref.is_local());
    assert_eq!(actor_ref.id(), "test-actor");
    
    // Register actor before calling tell()
    use plexspaces_core::{ActorRegistry, RequestContext};
    if let Some(registry) = service_locator.actor_registry().await {
        let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_core::MessageSender> = Arc::new(actor_ref.clone());
        registry.register_actor(&ctx, actor_id, sender, None, None, None).await;
    }
    
    // Send message should work
    let message = Message::new(b"test".to_vec());
    actor_ref.tell(message).await.unwrap();
    
    // Verify message was delivered
    let received = mailbox.dequeue().await;
    assert!(received.is_some());
    assert_eq!(received.unwrap().payload(), b"test");
}
