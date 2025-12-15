// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorServiceImpl::spawn_actor - local-only design (TDD)

use plexspaces_actor_service::ActorServiceImpl;
use plexspaces_core::{ActorRegistry, ServiceLocator, actor_context::ObjectRegistry as ObjectRegistryTrait};
use plexspaces_object_registry::ObjectRegistry;
use std::sync::Arc;

// Helper to wrap ObjectRegistry for ActorRegistry
struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistry>,
}

#[async_trait::async_trait]
impl ObjectRegistryTrait for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        tenant_id: &str,
        object_id: &str,
        namespace: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(tenant_id, namespace, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn lookup_full(
        &self,
        tenant_id: &str,
        namespace: &str,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .lookup(tenant_id, namespace, object_type, object_id)
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

#[tokio::test]
async fn test_spawn_actor_always_uses_local_node_id() {
    // Test: spawn_actor should always use local node_id, ignoring any node_id in actor_id
    let _service_locator = Arc::new(ServiceLocator::new());
    let object_registry_impl = Arc::new(ObjectRegistry::new(
        Arc::new(plexspaces_keyvalue::InMemoryKVStore::new())
    ));
    let object_registry_trait: Arc<dyn ObjectRegistryTrait> = 
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl });
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        "local-node".to_string(),
    ));
    
    // Create ServiceLocator and register services
    let service_locator = Arc::new(ServiceLocator::new());
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    let actor_service = ActorServiceImpl::new(service_locator, "local-node".to_string());
    
    // Test 1: actor_id without @node should use local node_id
    let result = actor_service.spawn_actor("test-actor", "test-type", vec![], None, std::collections::HashMap::new()).await;
    assert!(result.is_err()); // Callback not set, but should not fail due to node_id
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("test-actor@local-node"), "Should use local node_id");
    
    // Test 2: actor_id with local node_id should work
    let result = actor_service.spawn_actor("test-actor@local-node", "test-type", vec![], None, std::collections::HashMap::new()).await;
    assert!(result.is_err()); // Callback not set
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("test-actor@local-node"), "Should use local node_id");
}

#[tokio::test]
async fn test_spawn_actor_rejects_remote_node_id() {
    // Test: spawn_actor should reject actor_id with remote node_id
    let _service_locator = Arc::new(ServiceLocator::new());
    let object_registry_impl = Arc::new(ObjectRegistry::new(
        Arc::new(plexspaces_keyvalue::InMemoryKVStore::new())
    ));
    let object_registry_trait: Arc<dyn ObjectRegistryTrait> = 
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl });
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        "local-node".to_string(),
    ));
    
    // Create ServiceLocator and register services
    let service_locator = Arc::new(ServiceLocator::new());
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    let actor_service = ActorServiceImpl::new(service_locator, "local-node".to_string());
    
    // Should reject remote node_id
    let result = actor_service.spawn_actor("test-actor@remote-node", "test-type", vec![], None, std::collections::HashMap::new()).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Cannot spawn actor on remote node"), "Should reject remote node");
    assert!(err_msg.contains("remote-node"), "Should mention the remote node");
    assert!(err_msg.contains("call that node's ActorService directly"), "Should suggest correct approach");
}

#[tokio::test]
async fn test_spawn_actor_design_principle() {
    // Test: Verify design principle - ActorService always creates locally
    // This test documents the design: to spawn on remote node, call that node's ActorService
    let _service_locator = Arc::new(ServiceLocator::new());
    let object_registry_impl = Arc::new(ObjectRegistry::new(
        Arc::new(plexspaces_keyvalue::InMemoryKVStore::new())
    ));
    let object_registry_trait: Arc<dyn ObjectRegistryTrait> = 
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl });
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        "node1".to_string(),
    ));
    
    // Create ServiceLocator and register services
    let service_locator = Arc::new(ServiceLocator::new());
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    let actor_service = ActorServiceImpl::new(service_locator, "node1".to_string());
    
    // ActorService on node1 should only create actors on node1
    let result = actor_service.spawn_actor("actor1", "type1", vec![], None, std::collections::HashMap::new()).await;
    assert!(result.is_err()); // Callback not set
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("node1"), "Should use node1 as local node");
}

#[tokio::test]
async fn test_spawn_actor_with_callback() {
    // Test: spawn_actor should work when callback is set
    use plexspaces_core::ActorRef;
    use std::sync::Arc;
    
    let object_registry_impl = Arc::new(ObjectRegistry::new(
        Arc::new(plexspaces_keyvalue::InMemoryKVStore::new())
    ));
    let object_registry_trait: Arc<dyn ObjectRegistryTrait> = 
        Arc::new(ObjectRegistryAdapter { inner: object_registry_impl });
    let actor_registry = Arc::new(ActorRegistry::new(
        object_registry_trait,
        "test-node".to_string(),
    ));
    
    let service_locator = Arc::new(ServiceLocator::new());
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    
    // Register ActorFactory (required for spawn_actor)
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    let actor_factory = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator.register_service(actor_factory).await;
    
    let actor_service = ActorServiceImpl::new(service_locator, "test-node".to_string());
    
    // Test: spawn_actor should now work with ActorFactory registered
    // Note: This will fail because we don't have a behavior factory registered,
    // but it should at least get past the "ActorFactory not found" error
    let result = actor_service.spawn_actor("test-actor", "test-type", vec![], None, std::collections::HashMap::new()).await;
    // The test verifies that spawn_actor doesn't fail with "ActorFactory not found"
    // It may fail for other reasons (like behavior not found), which is expected
    if let Err(e) = &result {
        let err_msg = e.to_string();
        assert!(!err_msg.contains("ActorFactory not found"), "ActorFactory should be found");
    }
}

// Note: Integration test with real Node requires plexspaces-node dependency
// This test is commented out to avoid circular dependency in test-only code
// The callback mechanism is tested in test_spawn_actor_with_callback above
// Full integration testing should be done in node crate tests
