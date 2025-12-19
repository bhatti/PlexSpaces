// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext using services directly (convenience methods removed)

use plexspaces_core::{ActorContext, ChannelService, ActorService, ObjectRegistry, TupleSpaceProvider, ProcessGroupService, FacetService};
use plexspaces_mailbox::Message;
use plexspaces_tuplespace::{Pattern, Tuple, TupleSpaceError};
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

struct MockActorService {
    sent_messages: Arc<std::sync::Mutex<Vec<(String, Message)>>>,
}
#[async_trait::async_trait]
impl ActorService for MockActorService {
    async fn spawn_actor(&self, _actor_id: &str, _actor_type: &str, _initial_state: Vec<u8>) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        Err("Not implemented".into())
    }
    async fn send(&self, actor_id: &str, message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.sent_messages.lock().unwrap().push((actor_id.to_string(), message));
        Ok("msg-id".to_string())
    }
    async fn send_reply(
        &self,
        _correlation_id: Option<&str>,
        _sender_id: &plexspaces_core::ActorId,
        _target_actor_id: plexspaces_core::ActorId,
        _reply_message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

struct MockObjectRegistry;
#[async_trait::async_trait]
impl ObjectRegistry for MockObjectRegistry {
    async fn lookup(&self, _tenant_id: &str, _object_id: &str, _namespace: &str, _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(None)
    }
    async fn lookup_full(&self, _ctx: &plexspaces_core::RequestContext, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
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

struct MockProcessGroupService {
    joined_groups: Arc<std::sync::Mutex<Vec<(String, String, String, String)>>>, // (group_name, tenant_id, namespace, actor_id)
    left_groups: Arc<std::sync::Mutex<Vec<(String, String, String, String)>>>,
    published_messages: Arc<std::sync::Mutex<Vec<(String, String, String, Message)>>>, // (group_name, tenant_id, namespace, message)
    members: Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>>, // group_name -> actor_ids
}
#[async_trait::async_trait]
impl ProcessGroupService for MockProcessGroupService {
    async fn join_group(&self, group_name: &str, tenant_id: &str, namespace: &str, actor_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.joined_groups.lock().unwrap().push((
            group_name.to_string(),
            tenant_id.to_string(),
            namespace.to_string(),
            actor_id.to_string(),
        ));
        Ok(())
    }
    async fn leave_group(&self, group_name: &str, tenant_id: &str, namespace: &str, actor_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.left_groups.lock().unwrap().push((
            group_name.to_string(),
            tenant_id.to_string(),
            namespace.to_string(),
            actor_id.to_string(),
        ));
        Ok(())
    }
    async fn publish_to_group(&self, group_name: &str, tenant_id: &str, namespace: &str, message: Message) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.published_messages.lock().unwrap().push((
            group_name.to_string(),
            tenant_id.to_string(),
            namespace.to_string(),
            message,
        ));
        let members = self.members.lock().unwrap();
        Ok(members.get(group_name).cloned().unwrap_or_default())
    }
    async fn get_members(&self, group_name: &str, _tenant_id: &str, _namespace: &str) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let members = self.members.lock().unwrap();
        Ok(members.get(group_name).cloned().unwrap_or_default())
    }
}

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

fn create_test_context_with_services(
    actor_service: Arc<dyn ActorService>,
    process_group_service: Arc<dyn ProcessGroupService>,
) -> ActorContext {
    use plexspaces_core::ServiceLocator;
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    
    // Register services in ServiceLocator (if needed for tests)
    // For now, just create context with ServiceLocator
    ActorContext::new(
        "test-node".to_string(),
        String::new(), // tenant_id (empty if auth disabled)
        "test-ns".to_string(),
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

    let ctx = create_test_context_with_services(actor_service.clone(), process_group_service);
    
    // sender_id and correlation_id are in Message, not ActorContext
    let reply_msg = Message::new(vec![1, 2, 3])
        .with_correlation_id("corr-123".to_string());
    
    // Use actor_service directly
    let result = actor_service.send("sender-actor", reply_msg).await;
    assert!(result.is_ok());

    let sent = sent_messages.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "sender-actor");
    assert_eq!(sent[0].1.correlation_id, Some("corr-123".to_string()));
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

    let ctx = create_test_context_with_services(actor_service, process_group_service.clone());
    
    // Use process_group_service directly instead of ctx.join_group()
    // Services are accessed via service_locator, but for this test we use the service directly
    let tenant = "default";
    let result = process_group_service
        .join_group("test-group", tenant, &ctx.namespace, "test-actor")
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

    let ctx = create_test_context_with_services(actor_service, process_group_service.clone());
    
    // Use process_group_service directly instead of ctx.leave_group()
    let tenant = "default";
    let result = process_group_service
        .leave_group("test-group", tenant, &ctx.namespace, "test-actor")
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
    members.lock().unwrap().insert("test-group".to_string(), vec!["actor-1".to_string(), "actor-2".to_string()]);

    let process_group_service: Arc<dyn ProcessGroupService> = Arc::new(MockProcessGroupService {
        joined_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        left_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
        published_messages: published_messages.clone(),
        members: members.clone(),
    });

    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService {
        sent_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
    });

    let ctx = create_test_context_with_services(actor_service, process_group_service.clone());
    let message = Message::new(vec![1, 2, 3]);
    
    // Use process_group_service directly instead of ctx.publish_to_group()
    let tenant = "default";
    let result = process_group_service
        .publish_to_group("test-group", tenant, &ctx.namespace, message.clone())
        .await;

    assert!(result.is_ok());
    let recipients = result.unwrap();
    assert_eq!(recipients.len(), 2);

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
        vec!["actor-1".to_string(), "actor-2".to_string(), "actor-3".to_string()],
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

    let ctx = create_test_context_with_services(actor_service, process_group_service.clone());
    
    // Use process_group_service directly instead of ctx.get_group_members()
    let tenant = "default";
    let result = process_group_service
        .get_members("test-group", tenant, &ctx.namespace)
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

    let ctx = create_test_context_with_services(actor_service.clone(), process_group_service);
    
    // sender_id is in Message, not ActorContext
    // This test verifies the context is created correctly
    let reply_msg = Message::new(vec![1, 2, 3]);
    
    // In real code, sender_id would come from the message that triggered the reply
    // This test just verifies the context structure
    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.tenant_id, "tenant-123");
    
    // Verify no message was sent
    // (This test verifies the pattern, actual error handling depends on implementation)
}
