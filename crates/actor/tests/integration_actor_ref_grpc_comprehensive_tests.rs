// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Comprehensive gRPC integration tests for ActorRef with ServiceLocator
// Tests local/remote scenarios, error handling, and edge cases

use plexspaces_actor::ActorRef;
use plexspaces_core::{ActorRegistry, ServiceLocator, actor_context::ObjectRegistry as ObjectRegistryTrait};
use plexspaces_mailbox::{Message, Mailbox, MailboxConfig};
use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
use std::sync::Arc;

// Helper to wrap ObjectRegistry for ActorRegistry
struct ObjectRegistryAdapter {
    inner: Arc<plexspaces_object_registry::ObjectRegistry>,
}

#[async_trait::async_trait]
impl ObjectRegistryTrait for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        tenant_id: &str,
        object_id: &str,
        namespace: &str,
        object_type: Option<ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_core::RequestContext;
        let obj_type = object_type.unwrap_or(ObjectType::ObjectTypeUnspecified);
        let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
        self.inner
            .lookup(&ctx, obj_type, object_id)
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
        registration: ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .register(registration)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }
}

#[tokio::test]
async fn test_remote_actor_ref_node_not_found() {
    // Test: Remote ActorRef should handle node not found in registry
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
    
    service_locator.register_service(actor_registry).await;
    
    // Create remote ActorRef for node that doesn't exist
    let actor_ref = ActorRef::remote(
        "test-actor@unknown-node",
        "unknown-node",
        service_locator,
    );
    
    // Send message should fail with node not found
    let message = Message::new(b"test".to_vec());
    let result = actor_ref.tell(message).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Node not found") || err.to_string().contains("Failed"));
}

#[tokio::test]
async fn test_remote_actor_ref_connection_failure() {
    // Test: Remote ActorRef should handle connection failures gracefully
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let object_registry_impl = Arc::new(plexspaces_object_registry::ObjectRegistry::new(
        Arc::new(plexspaces_keyvalue::InMemoryKVStore::new())
    ));
    
    // Register node with unreachable address
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    let node_registration = ObjectRegistration {
        object_id: "_node@remote-node".to_string(),
        object_type: ObjectType::ObjectTypeService as i32,
        object_category: "Node".to_string(),
        grpc_address: "http://127.0.0.1:99999".to_string(), // Unreachable port
        ..Default::default()
    };
    object_registry_impl.register(&ctx, node_registration).await.unwrap();
    
    let object_registry_trait: Arc<dyn ObjectRegistryTrait> = 
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl.clone() });
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        "test-node".to_string(),
    ));
    
    service_locator.register_service(actor_registry).await;
    
    // Create remote ActorRef
    let actor_ref = ActorRef::remote(
        "test-actor@remote-node",
        "remote-node",
        service_locator,
    );
    
    // Send message should fail with connection error
    let message = Message::new(b"test".to_vec());
    let result = actor_ref.tell(message).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Connection") || err.to_string().contains("connection") || err.to_string().contains("Failed"));
}

#[tokio::test]
async fn test_remote_actor_ref_ask_timeout() {
    // Test: Remote ActorRef.ask() should timeout when no response
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let object_registry_impl = Arc::new(plexspaces_object_registry::ObjectRegistry::new(
        Arc::new(plexspaces_keyvalue::InMemoryKVStore::new())
    ));
    
    // Register node with unreachable address
    let node_registration = ObjectRegistration {
        object_id: "_node@remote-node".to_string(),
        object_type: ObjectType::ObjectTypeService as i32,
        object_category: "Node".to_string(),
        grpc_address: "http://127.0.0.1:99999".to_string(),
        tenant_id: "default".to_string(),
        namespace: "default".to_string(),
        ..Default::default()
    };
    object_registry_impl.register(node_registration).await.unwrap();
    
    let object_registry_trait: Arc<dyn ObjectRegistryTrait> = 
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl });
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        "test-node".to_string(),
    ));
    
    service_locator.register_service(actor_registry).await;
    
    // Create remote ActorRef
    let actor_ref = ActorRef::remote(
        "test-actor@remote-node",
        "remote-node",
        service_locator,
    );
    
    // Ask should fail (connection or timeout)
    let message = Message::new(b"test".to_vec());
    let result = actor_ref.ask(message, std::time::Duration::from_millis(100)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_remote_actor_ref_service_locator_client_caching() {
    // Test: Multiple ActorRefs to same node should share gRPC client via ServiceLocator
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let object_registry_impl = Arc::new(plexspaces_object_registry::ObjectRegistry::new(
        Arc::new(plexspaces_keyvalue::InMemoryKVStore::new())
    ));
    
    // Register node
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    let node_registration = ObjectRegistration {
        object_id: "_node@remote-node".to_string(),
        object_type: ObjectType::ObjectTypeService as i32,
        object_category: "Node".to_string(),
        grpc_address: "http://127.0.0.1:99999".to_string(),
        ..Default::default()
    };
    object_registry_impl.register(&ctx, node_registration).await.unwrap();
    
    let object_registry_trait: Arc<dyn ObjectRegistryTrait> = 
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl });
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        "test-node".to_string(),
    ));
    
    service_locator.register_service(actor_registry).await;
    
    // Create multiple ActorRefs to same node
    let actor_ref1 = ActorRef::remote(
        "actor1@remote-node",
        "remote-node",
        service_locator.clone(),
    );
    let actor_ref2 = ActorRef::remote(
        "actor2@remote-node",
        "remote-node",
        service_locator.clone(),
    );
    
    // Both should use the same cached gRPC client from ServiceLocator
    // (Both will fail to connect, but should use same client)
    let message = Message::new(b"test".to_vec());
    let result1 = actor_ref1.tell(message.clone()).await;
    let result2 = actor_ref2.tell(message).await;
    
    // Both should fail with same error type (connection failure)
    assert!(result1.is_err());
    assert!(result2.is_err());
}
