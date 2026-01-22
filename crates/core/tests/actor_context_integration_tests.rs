// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for ActorContext with real service implementations
// These tests verify end-to-end behavior with actual ChannelServiceWrapper

#[cfg(test)]
mod tests {
    use plexspaces_core::Message;
    use std::sync::Arc;
    use futures::StreamExt;
    use plexspaces_channel::{Channel, InMemoryChannel};
    use async_trait::async_trait;
    use plexspaces_core::{ChannelService, ActorService, ObjectRegistry, TupleSpaceProvider, RequestContext};
    use ulid::Ulid;

    /// Helper to create a test message
    fn create_test_message(payload: Vec<u8>) -> Message {
        Message {
            id: Ulid::new().to_string(),
            payload,
            ..Default::default()
        }
    }

    // Integration test: Test ActorContext with real ChannelServiceWrapper from node crate
    // Note: We can't directly import from node crate due to circular dependencies,
    // so we create a test implementation that mimics ChannelServiceWrapper behavior
    // This uses the actual plexspaces-channel crate to test real integration
    struct IntegrationChannelService {
        channels: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Arc<dyn Channel>>>>,
    }

    impl IntegrationChannelService {
        fn new() -> Self {
            Self {
                channels: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            }
        }

        async fn get_or_create_channel(&self, name: &str) -> Result<Arc<dyn Channel>, Box<dyn std::error::Error + Send + Sync>> {
            let mut channels = self.channels.write().await;
            
            if let Some(channel) = channels.get(name) {
                return Ok(channel.clone());
            }

            use plexspaces_proto::channel::v1::{ChannelBackend, ChannelConfig, DeliveryGuarantee, OrderingGuarantee};
            let config = ChannelConfig {
                name: name.to_string(),
                backend: ChannelBackend::ChannelBackendInMemory as i32,
                capacity: 1000,
                delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
                ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
                ..Default::default()
            };
            
            let channel_result = InMemoryChannel::new(config).await;
            let channel = Arc::new(channel_result
                .map_err(|e| format!("Failed to create channel {}: {}", name, e))?);
            
            channels.insert(name.to_string(), channel.clone());
            Ok(channel)
        }
    }

    #[async_trait::async_trait]
    impl ChannelService for IntegrationChannelService {
        async fn send_to_queue(&self, queue_name: &str, message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let channel = self.get_or_create_channel(queue_name).await?;
            
            // Message is already the unified proto Message type
            let mut channel_msg = message.clone();
            channel_msg.channel = queue_name.to_string();
            if channel_msg.timestamp.is_none() {
                channel_msg.timestamp = Some(prost_types::Timestamp {
                    seconds: chrono::Utc::now().timestamp(),
                    nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
                });
            }
            
            channel.send(channel_msg).await
                .map_err(|e| format!("Failed to send to queue {}: {}", queue_name, e).into())
        }

        async fn publish_to_topic(&self, topic_name: &str, message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let channel = self.get_or_create_channel(topic_name).await?;
            
            // Message is already the unified proto Message type
            let mut channel_msg = message.clone();
            channel_msg.channel = topic_name.to_string();
            if channel_msg.timestamp.is_none() {
                channel_msg.timestamp = Some(prost_types::Timestamp {
                    seconds: chrono::Utc::now().timestamp(),
                    nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
                });
            }
            
            channel.publish(channel_msg).await
                .map(|_| message.id.clone())
                .map_err(|e| format!("Failed to publish to topic {}: {}", topic_name, e).into())
        }

        async fn subscribe_to_topic(&self, topic_name: &str) -> Result<futures::stream::BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
            let channel = self.get_or_create_channel(topic_name).await?;
            
            let stream = channel.subscribe(None).await
                .map_err(|e| format!("Failed to subscribe to topic {}: {}", topic_name, e))?;
            
            // Message from channel is already the unified proto Message type
            Ok(Box::pin(stream))
        }

        async fn receive_from_queue(&self, queue_name: &str, timeout: Option<std::time::Duration>) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
            let channel = self.get_or_create_channel(queue_name).await?;
            
            let messages = if timeout.is_some() {
                channel.try_receive(1).await
                    .map_err(|e| format!("Failed to receive from queue {}: {}", queue_name, e))?
            } else {
                channel.receive(1).await
                    .map_err(|e| format!("Failed to receive from queue {}: {}", queue_name, e))?
            };
            
            // Message from channel is already the unified proto Message type
            Ok(messages.into_iter().next())
        }
    }

    // Mock services for integration testing
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
        async fn lookup(&self, _ctx: &RequestContext, _object_id: &str, _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
        async fn lookup_full(&self, _ctx: &RequestContext, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
        async fn register(&self, _ctx: &RequestContext, _registration: plexspaces_core::ObjectRegistration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        async fn unregister(&self, _ctx: &RequestContext, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        async fn heartbeat(&self, _ctx: &RequestContext, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        async fn discover(&self, _ctx: &RequestContext, _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>, _object_category: Option<String>, _capabilities: Option<Vec<String>>, _labels: Option<Vec<String>>, _health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>, _offset: usize, _limit: usize) -> Result<Vec<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(vec![])
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

    #[tokio::test]
    async fn test_integration_channel_send_receive() {
        let service = IntegrationChannelService::new();
        let queue_name = "test-queue";
        
        let message = create_test_message(b"hello world".to_vec());
        let msg_id = message.id.clone();
        
        // Send message
        let result = service.send_to_queue(queue_name, message).await;
        assert!(result.is_ok(), "Failed to send message: {:?}", result);
        
        // Receive message
        let received = service.receive_from_queue(queue_name, Some(std::time::Duration::from_secs(1))).await;
        assert!(received.is_ok(), "Failed to receive message: {:?}", received);
        
        let msg = received.unwrap();
        assert!(msg.is_some(), "No message received");
        assert_eq!(msg.unwrap().id, msg_id);
    }

    #[tokio::test]
    async fn test_integration_channel_pubsub() {
        let service = IntegrationChannelService::new();
        let topic_name = "test-topic";
        
        // Subscribe first
        let mut stream = service.subscribe_to_topic(topic_name).await.unwrap();
        
        // Publish message
        let message = create_test_message(b"broadcast".to_vec());
        let msg_id = message.id.clone();
        
        let result = service.publish_to_topic(topic_name, message).await;
        assert!(result.is_ok());
        
        // Receive from subscription
        tokio::select! {
            msg = stream.next() => {
                assert!(msg.is_some(), "No message received from subscription");
                assert_eq!(msg.unwrap().id, msg_id);
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                panic!("Timeout waiting for message");
            }
        }
    }
}
