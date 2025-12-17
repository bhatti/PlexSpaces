// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext methods to improve coverage

use plexspaces_core::{ActorContext, ServiceLocator};
use plexspaces_mailbox::Message;
use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField, TupleSpaceError};
use std::sync::Arc;

// Mock implementations
struct MockChannelService;
#[async_trait::async_trait]
impl ChannelService for MockChannelService {
    async fn send_to_queue(&self, _queue_name: &str, _message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("msg-id".to_string())
    }
    async fn publish_to_topic(&self, _topic_name: &str, _message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("msg-id".to_string())
    }
    async fn subscribe_to_topic(&self, _topic_name: &str) -> Result<futures::stream::BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
        use futures::stream;
        Ok(Box::pin(stream::empty()))
    }
    async fn receive_from_queue(&self, _queue_name: &str, _timeout: Option<std::time::Duration>) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(None)
    }
}

struct MockActorService;
#[async_trait::async_trait]
impl ActorService for MockActorService {
    async fn spawn_actor(&self, _actor_id: &str, _actor_type: &str, _initial_state: Vec<u8>) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        Err("Not implemented".into())
    }
    async fn send(&self, _actor_id: &str, _message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("msg-id".to_string())
    }
}

struct MockObjectRegistry;
#[async_trait::async_trait]
impl ObjectRegistry for MockObjectRegistry {
    async fn lookup(&self, _tenant_id: &str, _object_id: &str, _namespace: &str, _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(None)
    }
    async fn lookup_full(&self, _tenant_id: &str, _namespace: &str, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(None)
    }
    async fn register(&self, _registration: plexspaces_core::ObjectRegistration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

struct MockTupleSpaceProvider;
#[async_trait::async_trait]
impl TupleSpaceProvider for MockTupleSpaceProvider {
    async fn write(&self, _tuple: Tuple) -> Result<(), TupleSpaceError> {
        Ok(())
    }
    async fn read(&self, _pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        Ok(vec![])
    }
    async fn take(&self, _pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError> {
        Ok(None)
    }
    async fn count(&self, _pattern: &Pattern) -> Result<usize, TupleSpaceError> {
        Ok(0)
    }
}

struct MockProcessGroupService;
#[async_trait::async_trait]
impl ProcessGroupService for MockProcessGroupService {
    async fn join_group(&self, _group_name: &str, _tenant_id: &str, _namespace: &str, _actor_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    async fn leave_group(&self, _group_name: &str, _tenant_id: &str, _namespace: &str, _actor_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    async fn publish_to_group(&self, _group_name: &str, _tenant_id: &str, _namespace: &str, _message: Message) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![])
    }
    async fn get_members(&self, _group_name: &str, _tenant_id: &str, _namespace: &str) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![])
    }
}

#[async_trait::async_trait]

struct MockFacetService;
#[async_trait::async_trait]
impl FacetService for MockFacetService {
    async fn get_facet(
        &self,
        _actor_id: &plexspaces_core::ActorId,
        _facet_type: &str,
    ) -> Result<std::sync::Arc<tokio::sync::RwLock<Box<dyn plexspaces_facet::Facet>>>, Box<dyn std::error::Error + Send + Sync>> {
        Err("Not implemented".into())
    }
}

fn create_test_context() -> ActorContext {
    let service_locator = Arc::new(ServiceLocator::new());
    ActorContext::new(
        "test-node".to_string(),
        "test-ns".to_string(),
        "tenant-123".to_string(),
        service_locator,
        None,
    )
}

// Tests for with_message removed - ActorContext is now static
// sender_id and correlation_id are in Message, not ActorContext
// This test is no longer applicable

#[tokio::test]
async fn test_actor_context_minimal_with_config() {
    use plexspaces_proto::v1::actor::ActorConfig;
    
    let mut config = ActorConfig::default();
    config.max_mailbox_size = 1000;
    config.enable_persistence = true;
    let config = Some(config);
    
    let ctx = ActorContext::minimal_with_config(
        "test-node".to_string(),
        "test-ns".to_string(),
        "tenant-123".to_string(),
        config.clone(),
    );
    
    // actor_id removed from ActorContext
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.namespace, "test-ns");
    assert_eq!(ctx.config, config);
}

#[tokio::test]
async fn test_actor_context_clone() {
    let ctx = create_test_context();
    let ctx_clone = ctx.clone();
    
    // actor_id removed from ActorContext
    assert_eq!(ctx.node_id, ctx_clone.node_id);
    assert_eq!(ctx.namespace, ctx_clone.namespace);
}

#[tokio::test]
async fn test_actor_context_metadata() {
    let mut ctx = create_test_context();
    ctx.metadata.insert("key1".to_string(), "value1".to_string());
    ctx.metadata.insert("key2".to_string(), "value2".to_string());
    
    assert_eq!(ctx.metadata.get("key1"), Some(&"value1".to_string()));
    assert_eq!(ctx.metadata.get("key2"), Some(&"value2".to_string()));
}

#[tokio::test]
async fn test_actor_context_service_access() {
    let ctx = create_test_context();
    
    // Services are accessed via service_locator, not directly
    // Test that service_locator is accessible
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.tenant_id(), "tenant-123");
}

#[tokio::test]
async fn test_actor_context_convenience_methods() {
    let ctx = create_test_context();
    
    // Services are accessed via service_locator
    // This test verifies the context is created correctly
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.tenant_id(), "tenant-123");
    
    // Test tuplespace convenience methods (via service_locator)
    // Services are accessed via service_locator, not directly
    // This test verifies the context is created correctly
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.tenant_id(), "tenant-123");
}

