// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ChannelService trait and integration with ActorContext

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use futures::StreamExt;
    use plexspaces_core::Message;
    use plexspaces_core::{
        ActorService, ChannelService, ObjectRegistry, RequestContext, TupleSpaceProvider,
    };
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

    // Test ChannelService implementation
    struct TestChannelService {
        messages: Arc<tokio::sync::RwLock<Vec<Message>>>,
    }

    impl TestChannelService {
        fn new() -> Self {
            Self {
                messages: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl ChannelService for TestChannelService {
        async fn send_to_queue(
            &self,
            _queue_name: &str,
            message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let id = message.id.clone();
            self.messages.write().await.push(message);
            Ok(id)
        }
        async fn publish_to_topic(
            &self,
            _topic_name: &str,
            message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let id = message.id.clone();
            self.messages.write().await.push(message);
            Ok(id)
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
            let messages = self.messages.read().await;
            Ok(messages.first().cloned())
        }
    }

    // Mock services for testing
    struct MockActorService;
    #[async_trait::async_trait]
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
            _actor_id: &str,
            _message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented".into())
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
        ) -> Result<
            Option<plexspaces_core::ObjectRegistration>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            Ok(None)
        }
        async fn lookup_full(
            &self,
            _ctx: &RequestContext,
            _object_type: plexspaces_proto::object_registry::v1::ObjectType,
            _object_id: &str,
        ) -> Result<
            Option<plexspaces_core::ObjectRegistration>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
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
        ) -> Result<
            Vec<plexspaces_core::ObjectRegistration>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
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
        ) -> Result<Vec<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError>
        {
            Ok(vec![])
        }
        async fn take(
            &self,
            _pattern: &plexspaces_tuplespace::Pattern,
        ) -> Result<Option<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError>
        {
            Ok(None)
        }
        async fn count(
            &self,
            _pattern: &plexspaces_tuplespace::Pattern,
        ) -> Result<usize, plexspaces_tuplespace::TupleSpaceError> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn test_channel_service_send_to_queue() {
        let service = TestChannelService::new();
        let message = create_test_message(b"test payload".to_vec());
        let msg_id = message.id.clone();

        let result = service.send_to_queue("test-queue", message).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), msg_id);

        let messages = service.messages.read().await;
        assert_eq!(messages.len(), 1);
    }

    #[tokio::test]
    async fn test_channel_service_publish_to_topic() {
        let service = TestChannelService::new();
        let message = create_test_message(b"test payload".to_vec());
        let msg_id = message.id.clone();

        let result = service.publish_to_topic("test-topic", message).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), msg_id);

        let messages = service.messages.read().await;
        assert_eq!(messages.len(), 1);
    }

    #[tokio::test]
    async fn test_channel_service_receive_from_queue() {
        let service = TestChannelService::new();
        let message = create_test_message(b"test payload".to_vec());
        let msg_id = message.id.clone();

        // First send a message
        service.send_to_queue("test-queue", message).await.unwrap();

        // Then receive it
        let result = service.receive_from_queue("test-queue", None).await;
        assert!(result.is_ok());
        let received = result.unwrap();
        assert!(received.is_some());
        assert_eq!(received.unwrap().id, msg_id);
    }

    #[tokio::test]
    async fn test_channel_service_subscribe_to_topic() {
        let service = TestChannelService::new();

        let result = service.subscribe_to_topic("test-topic").await;
        assert!(result.is_ok());
        // Empty stream should return no messages
        let mut stream = result.unwrap();
        let next = stream.next().await;
        assert!(next.is_none());
    }
}
