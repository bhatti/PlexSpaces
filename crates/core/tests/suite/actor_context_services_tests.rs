// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Consolidated tests for ActorContext service integrations
// Merged from:
// - actor_context_channel_service_tests.rs (3 tests)
// - actor_context_channel_service_timeout_none_tests.rs (2 tests)
// - actor_context_integration_tests.rs (2 tests)
// - actor_context_process_group_convenience_tests.rs (3 tests)
// - actor_context_reply_tests.rs (2 tests)
// Total: 12 tests

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use plexspaces_channel::{Channel, InMemoryChannel};
use plexspaces_core::{
    ActorContext, ActorService, ChannelService, Message, ObjectRegistry, ProcessGroupService,
    RequestContext, TupleSpaceProvider,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use ulid::Ulid;

// =============================================================================
// COMMON HELPERS
// =============================================================================

fn create_test_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

fn create_test_message_with_sender(
    payload: Vec<u8>,
    sender_id: &str,
    correlation_id: &str,
) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        sender_id: sender_id.to_string(),
        correlation_id: correlation_id.to_string(),
        ..Default::default()
    }
}

// =============================================================================
// MOCK IMPLEMENTATIONS
// =============================================================================

// Mock ChannelService for basic tests
struct MockChannelService {
    sent_to_queue: Arc<std::sync::Mutex<Vec<(String, Message)>>>,
    published_to_topic: Arc<std::sync::Mutex<Vec<(String, Message)>>>,
    queue_messages: Arc<std::sync::Mutex<std::collections::VecDeque<Message>>>,
    return_none: bool,
}

impl MockChannelService {
    fn new() -> Self {
        Self {
            sent_to_queue: Arc::new(std::sync::Mutex::new(Vec::new())),
            published_to_topic: Arc::new(std::sync::Mutex::new(Vec::new())),
            queue_messages: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
            return_none: false,
        }
    }

    fn with_return_none(return_none: bool) -> Self {
        Self {
            sent_to_queue: Arc::new(std::sync::Mutex::new(Vec::new())),
            published_to_topic: Arc::new(std::sync::Mutex::new(Vec::new())),
            queue_messages: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
            return_none,
        }
    }
}

impl plexspaces_core::Service for MockChannelService {
    fn service_name(&self) -> String {
        "MockChannelService".to_string()
    }
}

#[async_trait]
impl ChannelService for MockChannelService {
    async fn send_to_queue(
        &self,
        queue_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.sent_to_queue
            .lock()
            .unwrap()
            .push((queue_name.to_string(), message));
        Ok("msg-id".to_string())
    }

    async fn publish_to_topic(
        &self,
        topic_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.published_to_topic
            .lock()
            .unwrap()
            .push((topic_name.to_string(), message));
        Ok("msg-id".to_string())
    }

    async fn subscribe_to_topic(
        &self,
        _topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
        use futures::stream;
        Ok(Box::pin(stream::empty()))
    }

    async fn receive_from_queue(
        &self,
        _queue_name: &str,
        timeout: Option<Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        // Handle None timeout path for coverage
        if timeout.is_none() {
            if self.return_none {
                Ok(None)
            } else {
                Ok(Some(Message {
                    id: Ulid::new().to_string(),
                    payload: vec![1, 2, 3],
                    ..Default::default()
                }))
            }
        } else {
            let mut queue = self.queue_messages.lock().unwrap();
            Ok(queue.pop_front())
        }
    }
}

// Integration ChannelService using real InMemoryChannel
struct IntegrationChannelService {
    channels: Arc<tokio::sync::RwLock<HashMap<String, Arc<dyn Channel>>>>,
}

impl IntegrationChannelService {
    fn new() -> Self {
        Self {
            channels: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    async fn get_or_create_channel(
        &self,
        name: &str,
    ) -> Result<Arc<dyn Channel>, Box<dyn std::error::Error + Send + Sync>> {
        let mut channels = self.channels.write().await;

        if let Some(channel) = channels.get(name) {
            return Ok(channel.clone());
        }

        use plexspaces_proto::channel::v1::{
            ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee,
        };
        let config = ChannelConfig {
            name: name.to_string(),
            provider: ChannelProvider::ChannelProviderInMemory as i32,
            capacity: 1000,
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };

        let channel_result = InMemoryChannel::new(config).await;
        let channel = Arc::new(
            channel_result.map_err(|e| format!("Failed to create channel {}: {}", name, e))?,
        );

        channels.insert(name.to_string(), channel.clone());
        Ok(channel)
    }
}

#[async_trait]
impl ChannelService for IntegrationChannelService {
    async fn send_to_queue(
        &self,
        queue_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(queue_name).await?;

        let mut channel_msg = message.clone();
        channel_msg.channel = queue_name.to_string();
        if channel_msg.timestamp.is_none() {
            channel_msg.timestamp = Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
            });
        }

        channel
            .send(channel_msg)
            .await
            .map_err(|e| format!("Failed to send to queue {}: {}", queue_name, e).into())
    }

    async fn publish_to_topic(
        &self,
        topic_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(topic_name).await?;

        let mut channel_msg = message.clone();
        channel_msg.channel = topic_name.to_string();
        if channel_msg.timestamp.is_none() {
            channel_msg.timestamp = Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
            });
        }

        channel
            .publish(channel_msg)
            .await
            .map(|_| message.id.clone())
            .map_err(|e| format!("Failed to publish to topic {}: {}", topic_name, e).into())
    }

    async fn subscribe_to_topic(
        &self,
        topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(topic_name).await?;

        let stream = channel
            .subscribe(None)
            .await
            .map_err(|e| format!("Failed to subscribe to topic {}: {}", topic_name, e))?;

        Ok(Box::pin(stream))
    }

    async fn receive_from_queue(
        &self,
        queue_name: &str,
        timeout: Option<Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(queue_name).await?;

        let messages = if timeout.is_some() {
            channel
                .try_receive(1)
                .await
                .map_err(|e| format!("Failed to receive from queue {}: {}", queue_name, e))?
        } else {
            channel
                .receive(1)
                .await
                .map_err(|e| format!("Failed to receive from queue {}: {}", queue_name, e))?
        };

        Ok(messages.into_iter().next())
    }
}

// Mock ActorService
struct MockActorService {
    sent_messages: Arc<std::sync::Mutex<Vec<(String, Message)>>>,
}

impl MockActorService {
    fn new() -> Self {
        Self {
            sent_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl ActorService for MockActorService {
    async fn spawn_actor(
        &self,
        _ctx: &RequestContext,
        _spec: &plexspaces_proto::actor::v1::ActorSpawnSpec,
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

// Mock ObjectRegistry
struct MockObjectRegistry;

#[async_trait]
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

// Mock TupleSpaceProvider
struct MockTupleSpaceProvider;

#[async_trait]
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

// Mock ProcessGroupService
struct MockProcessGroupService {
    joined_groups: Arc<std::sync::Mutex<Vec<(String, String, String, String)>>>,
    left_groups: Arc<std::sync::Mutex<Vec<(String, String, String, String)>>>,
    published_messages: Arc<std::sync::Mutex<Vec<(String, String, String, Message)>>>,
    members: Arc<std::sync::Mutex<HashMap<String, Vec<String>>>>,
}

impl MockProcessGroupService {
    fn new() -> Self {
        Self {
            joined_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
            left_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
            published_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
            members: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
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
        Ok(2)
    }
}

// =============================================================================
// CHANNEL SERVICE TESTS (from actor_context_channel_service_tests.rs - 3 tests)
// =============================================================================

#[tokio::test]
async fn test_channel_service_send_to_queue() {
    let service = MockChannelService::new();

    let msg = create_test_message(b"test".to_vec());
    let result = service.send_to_queue("test-queue", msg).await;
    assert!(result.is_ok());

    let sent = service.sent_to_queue.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "test-queue");
}

#[tokio::test]
async fn test_channel_service_publish_to_topic() {
    let service = MockChannelService::new();

    let msg = create_test_message(b"test".to_vec());
    let result = service.publish_to_topic("test-topic", msg).await;
    assert!(result.is_ok());

    let published = service.published_to_topic.lock().unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].0, "test-topic");
}

#[tokio::test]
async fn test_channel_service_receive_from_queue() {
    let msg = create_test_message(b"test".to_vec());
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(msg.clone());

    let service = MockChannelService {
        sent_to_queue: Arc::new(std::sync::Mutex::new(Vec::new())),
        published_to_topic: Arc::new(std::sync::Mutex::new(Vec::new())),
        queue_messages: Arc::new(std::sync::Mutex::new(queue)),
        return_none: false,
    };

    let result = service
        .receive_from_queue("test-queue", Some(Duration::from_secs(1)))
        .await;
    assert!(result.is_ok());

    let received = result.unwrap();
    assert!(received.is_some());
    assert_eq!(received.unwrap().id, msg.id);
}

// =============================================================================
// CHANNEL SERVICE TIMEOUT NONE TESTS (from actor_context_channel_service_timeout_none_tests.rs - 2 tests)
// =============================================================================

#[tokio::test]
async fn test_channel_service_receive_from_queue_no_timeout() {
    let channel_service = Arc::new(MockChannelService::with_return_none(false));

    use plexspaces_node::service_locator_helpers::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None).await;
    service_locator
        .register_channel_service(channel_service.clone())
        .await;
    let context = ActorContext::new(
        "node1".to_string(),
        "test-tenant".to_string(),
        "default".to_string(),
        service_locator,
        None,
    );

    let channel_svc = context.get_channel_service().await.unwrap();
    let result = channel_svc.receive_from_queue("test-queue", None).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[tokio::test]
async fn test_channel_service_receive_from_queue_no_timeout_returns_none() {
    let channel_service = Arc::new(MockChannelService::with_return_none(true));

    use plexspaces_node::service_locator_helpers::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None).await;
    service_locator
        .register_channel_service(channel_service.clone())
        .await;
    let context = ActorContext::new(
        "node1".to_string(),
        "test-tenant".to_string(),
        "default".to_string(),
        service_locator,
        None,
    );

    let channel_svc = context.get_channel_service().await.unwrap();
    let result = channel_svc.receive_from_queue("test-queue", None).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

// =============================================================================
// INTEGRATION TESTS (from actor_context_integration_tests.rs - 2 tests)
// =============================================================================

#[tokio::test]
async fn test_integration_channel_send_receive() {
    let service = IntegrationChannelService::new();
    let queue_name = "test-queue";

    let message = create_test_message(b"hello world".to_vec());
    let msg_id = message.id.clone();

    let result = service.send_to_queue(queue_name, message).await;
    assert!(result.is_ok(), "Failed to send message: {:?}", result);

    let received = service
        .receive_from_queue(queue_name, Some(Duration::from_secs(1)))
        .await;
    assert!(
        received.is_ok(),
        "Failed to receive message: {:?}",
        received
    );

    let msg = received.unwrap();
    assert!(msg.is_some(), "No message received");
    assert_eq!(msg.unwrap().id, msg_id);
}

#[tokio::test]
async fn test_integration_channel_pubsub() {
    let service = IntegrationChannelService::new();
    let topic_name = "test-topic";

    let mut stream = service.subscribe_to_topic(topic_name).await.unwrap();

    let message = create_test_message(b"broadcast".to_vec());
    let msg_id = message.id.clone();

    let result = service.publish_to_topic(topic_name, message).await;
    assert!(result.is_ok());

    tokio::select! {
        msg = stream.next() => {
            assert!(msg.is_some(), "No message received from subscription");
            assert_eq!(msg.unwrap().id, msg_id);
        }
        _ = tokio::time::sleep(Duration::from_secs(2)) => {
            panic!("Timeout waiting for message");
        }
    }
}

// =============================================================================
// PROCESS GROUP CONVENIENCE TESTS (from actor_context_process_group_convenience_tests.rs - 3 tests)
// =============================================================================

#[tokio::test]
async fn test_join_group_records_tenant_info() {
    let service = MockProcessGroupService::new();
    let ctx =
        RequestContext::new_without_auth("tenant-123".to_string(), "namespace-abc".to_string());

    service
        .join_group(&ctx, "test-group", "actor-1", vec![])
        .await
        .unwrap();

    let joined = service.joined_groups.lock().unwrap();
    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].0, "test-group");
    assert_eq!(joined[0].1, "tenant-123");
    assert_eq!(joined[0].2, "namespace-abc");
    assert_eq!(joined[0].3, "actor-1");
}

#[tokio::test]
async fn test_leave_group_records_tenant_info() {
    let service = MockProcessGroupService::new();
    let ctx =
        RequestContext::new_without_auth("tenant-123".to_string(), "namespace-abc".to_string());

    service
        .leave_group(&ctx, "test-group", "actor-1")
        .await
        .unwrap();

    let left = service.left_groups.lock().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].0, "test-group");
    assert_eq!(left[0].1, "tenant-123");
    assert_eq!(left[0].2, "namespace-abc");
    assert_eq!(left[0].3, "actor-1");
}

#[tokio::test]
async fn test_publish_to_group_records_tenant_info() {
    let service = MockProcessGroupService::new();
    let ctx =
        RequestContext::new_without_auth("tenant-123".to_string(), "namespace-abc".to_string());

    let message = create_test_message(b"test payload".to_vec());
    let count = service
        .publish_to_group(&ctx, "test-group", None, message)
        .await
        .unwrap();

    assert_eq!(count, 2);

    let published = service.published_messages.lock().unwrap();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].0, "test-group");
    assert_eq!(published[0].1, "tenant-123");
    assert_eq!(published[0].2, "namespace-abc");
}

// =============================================================================
// REPLY TESTS (from actor_context_reply_tests.rs - 2 tests)
// =============================================================================

#[tokio::test]
async fn test_reply_with_sender_id() {
    let sent_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService {
        sent_messages: sent_messages.clone(),
    });

    let original_msg =
        create_test_message_with_sender(b"request".to_vec(), "sender-actor", "corr-123");

    let reply_msg = Message {
        id: Ulid::new().to_string(),
        payload: b"response".to_vec(),
        receiver_id: original_msg.sender_id.clone(),
        correlation_id: original_msg.correlation_id.clone(),
        ..Default::default()
    };

    let ctx = RequestContext::new_without_auth(String::new(), "test-ns".to_string());
    let result = actor_service
        .send(&ctx, &original_msg.sender_id, reply_msg)
        .await;
    assert!(result.is_ok());

    let sent = sent_messages.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "sender-actor");
    assert_eq!(sent[0].1.correlation_id, "corr-123");
}

#[tokio::test]
async fn test_reply_without_sender_id() {
    let msg = create_test_message(b"no-sender".to_vec());
    assert!(msg.sender_id.is_empty());
}
