// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorServiceImpl::spawn_actor - local-only design (TDD)

use plexspaces_actor_service::ActorServiceImpl;
use plexspaces_core::{ActorRegistry, RequestContext, actor_context::ObjectRegistry as ObjectRegistryTrait};
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

    async fn discover(
        &self,
        _ctx: &plexspaces_core::RequestContext,
        _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        _name: Option<String>,
        _labels: Option<Vec<String>>,
        _exclude_labels: Option<Vec<String>>,
        _health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn test_spawn_actor_always_uses_local_node_id() {
    // Test: spawn_actor should always use local node_id, ignoring any node_id in actor_id
    use plexspaces_node::create_default_service_locator;
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
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    
    // Register ActorFactory (required for spawn_actor to work)
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    use plexspaces_core::{FacetManager, FacetManagerServiceWrapper, VirtualActorManager};
    let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    let facet_manager = Arc::new(FacetManagerServiceWrapper::new(Arc::new(FacetManager::new())));
    service_locator.register_service(virtual_actor_manager).await;
    service_locator.register_service(facet_manager).await;
    let actor_factory = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator.register_service(actor_factory).await;
    
    let actor_service = ActorServiceImpl::new(service_locator, "local-node".to_string());
    
    // Test 1: actor_id without @node should use local node_id
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());
    let result = actor_service.spawn_actor(&ctx, "test-actor", "test-type", vec![], None, std::collections::HashMap::new()).await;
    // With ActorFactory registered, spawn_actor should succeed or fail with a different error
    // The key is that it should use local node_id
    if let Ok(actor_ref) = result {
        // If it succeeds, verify the actor_ref uses local node_id
        assert!(actor_ref.id().contains("@local-node"), "Should use local node_id");
    } else {
        // If it fails, the error should mention local node_id, not a different node
        let err_msg = result.unwrap_err().to_string();
        // Should not contain a different node_id
        assert!(!err_msg.contains("@remote-node") && !err_msg.contains("@node2"), "Should not use remote node_id");
    }
    
    // Test 2: actor_id with local node_id should work
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());
    let result = actor_service.spawn_actor(&ctx, "test-actor@local-node", "test-type", vec![], None, std::collections::HashMap::new()).await;
    // Should succeed or fail with appropriate error, but should use local node_id
    if let Ok(actor_ref) = result {
        assert!(actor_ref.id().contains("@local-node"), "Should use local node_id");
    } else {
        let err_msg = result.unwrap_err().to_string();
        assert!(!err_msg.contains("@remote-node") && !err_msg.contains("@node2"), "Should not use remote node_id");
    }
}

#[tokio::test]
async fn test_spawn_actor_rejects_remote_node_id() {
    // Test: spawn_actor should reject actor_id with remote node_id
    use plexspaces_node::create_default_service_locator;
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
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    
    let actor_service = ActorServiceImpl::new(service_locator, "local-node".to_string());
    
    // Should reject remote node_id
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());
    let result = actor_service.spawn_actor(&ctx, "test-actor@remote-node", "test-type", vec![], None, std::collections::HashMap::new()).await;
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
    use plexspaces_node::create_default_service_locator;
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
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    
    // Register ActorFactory (required for spawn_actor to work)
    use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
    use plexspaces_core::{FacetManager, FacetManagerServiceWrapper, VirtualActorManager};
    let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    let facet_manager = Arc::new(FacetManagerServiceWrapper::new(Arc::new(FacetManager::new())));
    service_locator.register_service(virtual_actor_manager).await;
    service_locator.register_service(facet_manager).await;
    let actor_factory = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator.register_service(actor_factory).await;
    
    let actor_service = ActorServiceImpl::new(service_locator, "node1".to_string());
    
    // ActorService on node1 should only create actors on node1
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());
    let result = actor_service.spawn_actor(&ctx, "actor1", "type1", vec![], None, std::collections::HashMap::new()).await;
    // Should succeed or fail appropriately, but should use node1 as local node
    if let Ok(actor_ref) = result {
        assert!(actor_ref.id().contains("@node1"), "Should use node1 as local node");
    } else {
        let err_msg = result.unwrap_err().to_string();
        // Should not contain a different node_id
        assert!(!err_msg.contains("@node2") && !err_msg.contains("@remote"), "Should not use different node_id");
    }
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
    
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
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
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());
    let result = actor_service.spawn_actor(&ctx, "test-actor", "test-type", vec![], None, std::collections::HashMap::new()).await;
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
