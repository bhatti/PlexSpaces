// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext::new() to improve coverage

use plexspaces_mailbox::Message;
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
    async fn write(&self, _tuple: plexspaces_tuplespace::Tuple) -> Result<(), plexspaces_tuplespace::TupleSpaceError> {
        Ok(())
    }
    async fn read(&self, _pattern: &plexspaces_tuplespace::Pattern) -> Result<Vec<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError> {
        Ok(vec![])
    }
    async fn take(&self, _pattern: &plexspaces_tuplespace::Pattern) -> Result<Option<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError> {
        Ok(None)
    }
    async fn count(&self, _pattern: &plexspaces_tuplespace::Pattern) -> Result<usize, plexspaces_tuplespace::TupleSpaceError> {
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

#[tokio::test]
async fn test_actor_context_new() {
    let channel_service: Arc<dyn ChannelService> = Arc::new(MockChannelService);
    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService);
    let object_registry: Arc<dyn ObjectRegistry> = Arc::new(MockObjectRegistry);
    let tuplespace: Arc<dyn TupleSpaceProvider> = Arc::new(MockTupleSpaceProvider);
    let process_group_service: Arc<dyn ProcessGroupService> = Arc::new(MockProcessGroupService);
    let facet_service: Arc<dyn FacetService> = Arc::new(MockFacetService);

    let ctx = ActorContext::new(
        "test-actor".to_string(),
        "test-node".to_string(),
        "test-ns".to_string(),
        channel_service.clone(),
        actor_service.clone(),
        object_registry.clone(),
        tuplespace.clone(),
        process_group_service.clone(),
        node.clone(),
        facet_service.clone(),
        None,
    );

    // actor_id removed from ActorContext
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.namespace, "test-ns");
    assert!(ctx.metadata.is_empty());
    assert_eq!(ctx.config, None);
    assert_eq!(ctx.sender_id, None);
    assert_eq!(ctx.correlation_id, None);
    assert_eq!(ctx.node.node_id(), "test-node");
    assert_eq!(ctx.node.node_address(), "127.0.0.1:9001");
}

#[tokio::test]
async fn test_actor_context_new_with_config() {
    use plexspaces_proto::v1::actor::ActorConfig;

    let channel_service: Arc<dyn ChannelService> = Arc::new(MockChannelService);
    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService);
    let object_registry: Arc<dyn ObjectRegistry> = Arc::new(MockObjectRegistry);
    let tuplespace: Arc<dyn TupleSpaceProvider> = Arc::new(MockTupleSpaceProvider);
    let process_group_service: Arc<dyn ProcessGroupService> = Arc::new(MockProcessGroupService);
    let facet_service: Arc<dyn FacetService> = Arc::new(MockFacetService);

    let mut config = ActorConfig::default();
    config.max_mailbox_size = 5000;
    config.enable_persistence = true;
    let config = Some(config.clone());

    let ctx = ActorContext::new(
        "test-actor".to_string(),
        "test-node".to_string(),
        "test-ns".to_string(),
        channel_service,
        actor_service,
        object_registry,
        tuplespace,
        process_group_service,
        node,
        facet_service,
        config.clone(),
    );

    assert_eq!(ctx.config, config);
    assert_eq!(ctx.config.as_ref().unwrap().max_mailbox_size, 5000);
    assert_eq!(ctx.config.as_ref().unwrap().enable_persistence, true);
}

#[tokio::test]
async fn test_actor_context_new_with_metadata() {
    let channel_service: Arc<dyn ChannelService> = Arc::new(MockChannelService);
    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService);
    let object_registry: Arc<dyn ObjectRegistry> = Arc::new(MockObjectRegistry);
    let tuplespace: Arc<dyn TupleSpaceProvider> = Arc::new(MockTupleSpaceProvider);
    let process_group_service: Arc<dyn ProcessGroupService> = Arc::new(MockProcessGroupService);
    let facet_service: Arc<dyn FacetService> = Arc::new(MockFacetService);

    let mut ctx = ActorContext::new(
        "test-actor".to_string(),
        "test-node".to_string(),
        "test-ns".to_string(),
        channel_service,
        actor_service,
        object_registry,
        tuplespace,
        process_group_service,
        node,
        facet_service,
        None,
    );

    ctx.metadata.insert("key1".to_string(), "value1".to_string());
    ctx.metadata.insert("key2".to_string(), "value2".to_string());

    assert_eq!(ctx.metadata.get("key1"), Some(&"value1".to_string()));
    assert_eq!(ctx.metadata.get("key2"), Some(&"value2".to_string()));
    assert_eq!(ctx.metadata.len(), 2);
}

