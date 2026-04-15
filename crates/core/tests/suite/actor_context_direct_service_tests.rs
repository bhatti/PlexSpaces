// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext using services directly (convenience methods removed)

use plexspaces_core::Message;
use plexspaces_core::{
    ActorContext, ActorService, ChannelService, FacetService, ObjectRegistry, ProcessGroupService,
    RequestContext, TupleSpaceProvider,
};
use plexspaces_tuplespace::{Pattern, Tuple, TupleSpaceError};
use std::sync::Arc;
use ulid::Ulid;

/// Helper to create a test message
fn create_test_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

fn create_test_message_with_correlation(payload: Vec<u8>, correlation_id: &str) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        correlation_id: correlation_id.to_string(),
        ..Default::default()
    }
}

// Mock implementations
struct MockChannelService;
#[async_trait::async_trait]
impl ChannelService for MockChannelService {
    async fn send_to_queue(
        &self,
        _queue_name: &str,
        _message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("msg-id".to_string())
    }
    async fn publish_to_topic(
        &self,
        _topic_name: &str,
        _message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("msg-id".to_string())
    }
    async fn subscribe_to_topic(
        &self,
        _topic_name: &str,
    ) -> Result<
        futures::stream::BoxStream<'static, Message>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        use futures::stream;
        Ok(Box::pin(stream::empty()))
    }
    async fn receive_from_queue(
        &self,
        _queue_name: &str,
        _timeout: Option<std::time::Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(None)
    }
}

struct MockActorService {
    sent_messages: Arc<std::sync::Mutex<Vec<(String, Message)>>>,
}
#[async_trait::async_trait]
impl ActorService for MockActorService {
    async fn spawn_actor(
        &self,
        _ctx: &RequestContext,
        _actor_id: &str,
        _actor_type: &str,
        _initial_state: Vec<u8>,
    ) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        Err("Not implemented".into())
    }
    async fn send(
        &self,
        _ctx: &RequestContext,
        actor_id: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.sent_messages
            .lock()
            .unwrap()
            .push((actor_id.to_string(), message));
        Ok("msg-id".to_string())
    }
}

struct MockObjectRegistry;
#[async_trait::async_trait]
impl ObjectRegistry for MockObjectRegistry {
    async fn lookup(
        &self,
        _ctx: &RequestContext,
        _object_id: &str,
        _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>>
    {
        Ok(None)
    }
    async fn lookup_full(
        &self,
        _ctx: &RequestContext,
        _object_type: plexspaces_proto::object_registry::v1::ObjectType,
        _object_id: &str,
    ) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>>
    {
        Ok(None)
    }
    async fn register(
        &self,
        _ctx: &RequestContext,
        _registration: plexspaces_core::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    async fn unregister(
        &self,
        _ctx: &RequestContext,
        _object_type: plexspaces_proto::object_registry::v1::ObjectType,
        _object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    async fn heartbeat(
        &self,
        _ctx: &RequestContext,
        _object_type: plexspaces_proto::object_registry::v1::ObjectType,
        _object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    async fn discover(
        &self,
        _ctx: &RequestContext,
        _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        _object_category: Option<String>,
        _capabilities: Option<Vec<String>>,
        _labels: Option<Vec<String>>,
        _health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        _offset: usize,
        _limit: usize,
    ) -> Result<Vec<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>>
    {
        Ok(vec![])
    }
}

struct MockTupleSpaceProvider;
#[async_trait::async_trait]
impl TupleSpaceProvider for MockTupleSpaceProvider {
    async fn write(
        &self,
        _tuple: plexspaces_tuplespace::Tuple,
    ) -> Result<(), plexspaces_tuplespace::TupleSpaceError> {
        Ok(())
    }
    async fn read(
        &self,
        _pattern: &plexspaces_tuplespace::Pattern,
    ) -> Result<Vec<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError> {
        Ok(vec![])
    }
    async fn take(
        &self,
        _pattern: &plexspaces_tuplespace::Pattern,
    ) -> Result<Option<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError> {
        Ok(None)
    }
    async fn count(
        &self,
        _pattern: &plexspaces_tuplespace::Pattern,
    ) -> Result<usize, plexspaces_tuplespace::TupleSpaceError> {
        Ok(0)
    }
}

struct MockProcessGroupService {
    joined_groups: Arc<std::sync::Mutex<Vec<(String, String, String, String)>>>, // (group_name, tenant_id, namespace, actor_id)
    left_groups: Arc<std::sync::Mutex<Vec<(String, String, String, String)>>>,
    published_messages: Arc<std::sync::Mutex<Vec<(String, String, String, Message)>>>, // (group_name, tenant_id, namespace, message)
    members: Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>>, // group_name -> actor_ids
}
#[async_trait::async_trait]
impl ProcessGroupService for MockProcessGroupService {
    async fn create_group(
        &self,
        _ctx: &RequestContext,
        _group_name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    async fn delete_group(
        &self,
        _ctx: &RequestContext,
        _group_name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    async fn join_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
        _topics: Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.joined_groups.lock().unwrap().push((
            group_name.to_string(),
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            actor_id.to_string(),
        ));
        Ok(())
    }
    async fn leave_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.left_groups.lock().unwrap().push((
            group_name.to_string(),
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            actor_id.to_string(),
        ));
        Ok(())
    }
    async fn get_members(
        &self,
        _ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let members = self.members.lock().unwrap();
        Ok(members.get(group_name).cloned().unwrap_or_default())
    }
    async fn get_local_members(
        &self,
        _ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let members = self.members.lock().unwrap();
        Ok(members.get(group_name).cloned().unwrap_or_default())
    }
    async fn list_groups(
        &self,
        _ctx: &RequestContext,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let members = self.members.lock().unwrap();
        Ok(members.keys().cloned().collect())
    }
    async fn publish_to_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        _topic: Option<&str>,
        message: Message,
    ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        self.published_messages.lock().unwrap().push((
            group_name.to_string(),
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            message,
        ));
        let members = self.members.lock().unwrap();
        Ok(members.get(group_name).map(|v| v.len() as u32).unwrap_or(0))
    }
}

struct MockFacetService;
#[async_trait::async_trait]
impl FacetService for MockFacetService {
    async fn get_facet(
        &self,
        _actor_id: &plexspaces_core::ActorId,
        _facet_type: &str,
    ) -> Result<
        std::sync::Arc<tokio::sync::RwLock<Box<dyn plexspaces_facet::Facet>>>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("Not implemented".into())
    }
}

async fn create_test_context_with_services(
    actor_service: Arc<dyn ActorService>,
    process_group_service: Arc<dyn ProcessGroupService>,
) -> ActorContext {
    create_test_context_with_services_custom(
        actor_service,
        process_group_service,
        String::new(),         // default tenant_id (empty if auth disabled)
        "test-ns".to_string(), // default namespace
    )
    .await
}

async fn create_test_context_with_services_custom(
    _actor_service: Arc<dyn ActorService>,
    _process_group_service: Arc<dyn ProcessGroupService>,
    tenant_id: String,
    namespace: String,
) -> ActorContext {
    use plexspaces_node::service_locator_helpers::create_default_service_locator;
    // Create a ServiceLocator with all default services for testing
    let service_locator = create_default_service_locator(None, None).await;

    // Register services in ServiceLocator (if needed for tests)
    // For now, just create context with ServiceLocator
    ActorContext::new(
        "test-node".to_string(),
        tenant_id,
        namespace,
        service_locator,
        None,
    )
}

// Test reply using actor_service directly
#[tokio::test]
async fn test_reply_using_actor_service() {
    let sent_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService {
        sent_messages: sent_messages.clone(),
    });
    let process_group_service: Arc<dyn ProcessGroupService> = Arc::new(MockProcessGroupService {
        joined_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        left_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        published_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
        members: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    });

    let ctx = create_test_context_with_services(actor_service.clone(), process_group_service).await;

    // sender_id and correlation_id are in Message, not ActorContext
    let reply_msg = create_test_message_with_correlation(vec![1, 2, 3], "corr-123");

    // Use actor_service directly
    let ctx = RequestContext::new_without_auth(String::new(), "test-ns".to_string());
    let result = actor_service.send(&ctx, "sender-actor", reply_msg).await;
    assert!(result.is_ok());

    let sent = sent_messages.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "sender-actor");
    assert_eq!(sent[0].1.correlation_id, "corr-123");
}

// Test join_group using process_group_service directly
#[tokio::test]
async fn test_join_group_using_process_group_service() {
    let joined_groups = Arc::new(std::sync::Mutex::new(Vec::new()));
    let process_group_service: Arc<dyn ProcessGroupService> = Arc::new(MockProcessGroupService {
        joined_groups: joined_groups.clone(),
        left_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        published_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
        members: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    });

    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService {
        sent_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
    });

    let ctx = create_test_context_with_services(actor_service, process_group_service.clone()).await;

    // Use process_group_service directly with RequestContext
    let request_ctx =
        RequestContext::new_without_auth("default".to_string(), ctx.namespace.clone());
    let result = process_group_service
        .join_group(&request_ctx, "test-group", "test-actor", vec![])
        .await;

    assert!(result.is_ok());
    let joined = joined_groups.lock().unwrap();
    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].0, "test-group");
    assert_eq!(joined[0].1, "default");
    assert_eq!(joined[0].2, "test-ns");
    assert_eq!(joined[0].3, "test-actor");
}

// Test leave_group using process_group_service directly
#[tokio::test]
async fn test_leave_group_using_process_group_service() {
    let left_groups = Arc::new(std::sync::Mutex::new(Vec::new()));
    let process_group_service: Arc<dyn ProcessGroupService> = Arc::new(MockProcessGroupService {
        joined_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        left_groups: left_groups.clone(),
        published_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
        members: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    });

    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService {
        sent_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
    });

    let ctx = create_test_context_with_services(actor_service, process_group_service.clone()).await;

    // Use process_group_service directly with RequestContext
    let request_ctx =
        RequestContext::new_without_auth("default".to_string(), ctx.namespace.clone());
    let result = process_group_service
        .leave_group(&request_ctx, "test-group", "test-actor")
        .await;

    assert!(result.is_ok());
    let left = left_groups.lock().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].0, "test-group");
    assert_eq!(left[0].3, "test-actor");
}

// Test publish_to_group using process_group_service directly
#[tokio::test]
async fn test_publish_to_group_using_process_group_service() {
    let published_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let members = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    members.lock().unwrap().insert(
        "test-group".to_string(),
        vec!["actor-1".to_string(), "actor-2".to_string()],
    );

    let process_group_service: Arc<dyn ProcessGroupService> = Arc::new(MockProcessGroupService {
        joined_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        left_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        published_messages: published_messages.clone(),
        members: members.clone(),
    });

    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService {
        sent_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
    });

    let ctx = create_test_context_with_services(actor_service, process_group_service.clone()).await;
    let message = create_test_message(vec![1, 2, 3]);

    // Use process_group_service directly with RequestContext
    let request_ctx =
        RequestContext::new_without_auth("default".to_string(), ctx.namespace.clone());
    let result = process_group_service
        .publish_to_group(&request_ctx, "test-group", None, message.clone())
        .await;

    assert!(result.is_ok());
    let recipients_count = result.unwrap();
    assert_eq!(recipients_count, 2);

    let published = published_messages.lock().unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].0, "test-group");
    assert_eq!(published[0].1, "default");
}

// Test get_group_members using process_group_service directly
#[tokio::test]
async fn test_get_group_members_using_process_group_service() {
    let members = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    members.lock().unwrap().insert(
        "test-group".to_string(),
        vec![
            "actor-1".to_string(),
            "actor-2".to_string(),
            "actor-3".to_string(),
        ],
    );

    let process_group_service: Arc<dyn ProcessGroupService> = Arc::new(MockProcessGroupService {
        joined_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        left_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        published_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
        members: members.clone(),
    });

    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService {
        sent_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
    });

    let ctx = create_test_context_with_services(actor_service, process_group_service.clone()).await;

    // Use process_group_service directly with RequestContext
    let request_ctx =
        RequestContext::new_without_auth("default".to_string(), ctx.namespace.clone());
    let result = process_group_service
        .get_members(&request_ctx, "test-group")
        .await;

    assert!(result.is_ok());
    let members_list = result.unwrap();
    assert_eq!(members_list.len(), 3);
    assert!(members_list.contains(&"actor-1".to_string()));
    assert!(members_list.contains(&"actor-2".to_string()));
    assert!(members_list.contains(&"actor-3".to_string()));
}

// Test reply without sender_id (error case)
#[tokio::test]
async fn test_reply_without_sender_id() {
    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService {
        sent_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
    });
    let process_group_service: Arc<dyn ProcessGroupService> = Arc::new(MockProcessGroupService {
        joined_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        left_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        published_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
        members: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    });

    // Create context with specific tenant_id for this test
    let ctx = create_test_context_with_services_custom(
        actor_service.clone(),
        process_group_service,
        "tenant-123".to_string(),
        "test-ns".to_string(),
    )
    .await;

    // sender_id is in Message, not ActorContext
    // This test verifies the context is created correctly
    let _reply_msg = create_test_message(vec![1, 2, 3]);

    // In real code, sender_id would come from the message that triggered the reply
    // This test just verifies the context structure
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.tenant_id, "tenant-123");
    assert_eq!(ctx.namespace, "test-ns");

    // Verify no message was sent
    // (This test verifies the pattern, actual error handling depends on implementation)
}
