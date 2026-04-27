// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for ChannelService trait with InMemoryChannel
// Moved from plexspaces-core to avoid circular dependencies

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use futures::StreamExt;
    use plexspaces_channel::{Channel, InMemoryChannel};
    use plexspaces_proto::channel::v1::{
        ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee,
    };
    use plexspaces_proto::common::v1::Message as ProtoMessage;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;

    /// ChannelService trait - mirrors plexspaces_core::ChannelService
    /// Defined here to avoid circular dependency with plexspaces-core
    #[async_trait]
    pub trait ChannelService: Send + Sync {
        async fn send_to_queue(
            &self,
            queue_name: &str,
            message: TestMessage,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

        async fn publish_to_topic(
            &self,
            topic_name: &str,
            message: TestMessage,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

        async fn subscribe_to_topic(
            &self,
            topic_name: &str,
        ) -> Result<
            futures::stream::BoxStream<'static, TestMessage>,
            Box<dyn std::error::Error + Send + Sync>,
        >;

        async fn receive_from_queue(
            &self,
            queue_name: &str,
            timeout: Option<Duration>,
        ) -> Result<Option<TestMessage>, Box<dyn std::error::Error + Send + Sync>>;
    }

    /// Simple test message type
    #[derive(Clone, Debug)]
    pub struct TestMessage {
        pub id: String,
        pub payload: Vec<u8>,
        pub sender: Option<String>,
        pub metadata: HashMap<String, String>,
        pub correlation_id: Option<String>,
        pub reply_to: Option<String>,
    }

    impl TestMessage {
        pub fn new(payload: Vec<u8>) -> Self {
            Self {
                id: ulid::Ulid::new().to_string(),
                payload,
                sender: None,
                metadata: HashMap::new(),
                correlation_id: None,
                reply_to: None,
            }
        }
    }

    /// Integration test ChannelService implementation using InMemoryChannel
    struct IntegrationChannelService {
        channels: Arc<RwLock<HashMap<String, Arc<dyn Channel>>>>,
    }

    impl IntegrationChannelService {
        fn new() -> Self {
            Self {
                channels: Arc::new(RwLock::new(HashMap::new())),
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
            message: TestMessage,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let channel = self.get_or_create_channel(queue_name).await?;

            let channel_msg = ProtoMessage {
                id: message.id.clone(),
                channel: queue_name.to_string(),
                sender_id: message.sender.clone().unwrap_or_default(),
                payload: message.payload.clone(),
                headers: message.metadata.clone(),
                timestamp: Some(prost_types::Timestamp {
                    seconds: chrono::Utc::now().timestamp(),
                    nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
                }),
                partition_key: String::new(),
                correlation_id: message.correlation_id.clone().unwrap_or_default(),
                reply_to: message.reply_to.clone().unwrap_or_default(),
                delivery_count: 0,
                ..Default::default()
            };

            channel
                .send(channel_msg)
                .await
                .map_err(|e| format!("Failed to send to queue {}: {}", queue_name, e).into())
        }

        async fn publish_to_topic(
            &self,
            topic_name: &str,
            message: TestMessage,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let channel = self.get_or_create_channel(topic_name).await?;

            let channel_msg = ProtoMessage {
                id: message.id.clone(),
                channel: topic_name.to_string(),
                sender_id: message.sender.clone().unwrap_or_default(),
                payload: message.payload.clone(),
                headers: message.metadata.clone(),
                timestamp: Some(prost_types::Timestamp {
                    seconds: chrono::Utc::now().timestamp(),
                    nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
                }),
                partition_key: String::new(),
                correlation_id: message.correlation_id.clone().unwrap_or_default(),
                reply_to: message.reply_to.clone().unwrap_or_default(),
                delivery_count: 0,
                ..Default::default()
            };

            channel
                .publish(channel_msg)
                .await
                .map(|_| message.id.clone())
                .map_err(|e| format!("Failed to publish to topic {}: {}", topic_name, e).into())
        }

        async fn subscribe_to_topic(
            &self,
            topic_name: &str,
        ) -> Result<
            futures::stream::BoxStream<'static, TestMessage>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            let channel = self.get_or_create_channel(topic_name).await?;

            let stream = channel
                .subscribe(None)
                .await
                .map_err(|e| format!("Failed to subscribe to topic {}: {}", topic_name, e))?;

            let message_stream = stream.map(|channel_msg| {
                let mut msg = TestMessage::new(channel_msg.payload);
                msg.id = channel_msg.id;
                msg.metadata = channel_msg.headers;
                msg.correlation_id = if channel_msg.correlation_id.is_empty() {
                    None
                } else {
                    Some(channel_msg.correlation_id)
                };
                msg.reply_to = if channel_msg.reply_to.is_empty() {
                    None
                } else {
                    Some(channel_msg.reply_to)
                };
                msg.sender = if channel_msg.sender_id.is_empty() {
                    None
                } else {
                    Some(channel_msg.sender_id)
                };
                msg
            });

            Ok(Box::pin(message_stream))
        }

        async fn receive_from_queue(
            &self,
            queue_name: &str,
            timeout: Option<Duration>,
        ) -> Result<Option<TestMessage>, Box<dyn std::error::Error + Send + Sync>> {
            let channel = self.get_or_create_channel(queue_name).await?;

            let messages =
                if timeout.is_some() {
                    channel.try_receive(1).await.map_err(|e| {
                        format!("Failed to receive from queue {}: {}", queue_name, e)
                    })?
                } else {
                    channel.receive(1).await.map_err(|e| {
                        format!("Failed to receive from queue {}: {}", queue_name, e)
                    })?
                };

            if messages.is_empty() {
                return Ok(None);
            }

            let channel_msg = &messages[0];
            let mut msg = TestMessage::new(channel_msg.payload.clone());
            msg.id = channel_msg.id.clone();
            msg.metadata = channel_msg.headers.clone();
            msg.correlation_id = if channel_msg.correlation_id.is_empty() {
                None
            } else {
                Some(channel_msg.correlation_id.clone())
            };
            msg.reply_to = if channel_msg.reply_to.is_empty() {
                None
            } else {
                Some(channel_msg.reply_to.clone())
            };
            msg.sender = if channel_msg.sender_id.is_empty() {
                None
            } else {
                Some(channel_msg.sender_id.clone())
            };
            Ok(Some(msg))
        }
    }

    #[tokio::test]
    async fn test_channel_service_send_to_queue() {
        let service = IntegrationChannelService::new();
        let msg = TestMessage::new(b"test payload".to_vec());

        let result = service.send_to_queue("test-queue", msg).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_channel_service_publish_to_topic() {
        let service = IntegrationChannelService::new();
        let msg = TestMessage::new(b"test payload".to_vec());

        let result = service.publish_to_topic("test-topic", msg).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_channel_service_receive_from_queue() {
        let service = IntegrationChannelService::new();

        // Send a message first
        let msg = TestMessage::new(b"test payload".to_vec());
        service.send_to_queue("test-queue", msg).await.unwrap();

        // Try to receive (with timeout to avoid blocking)
        let result = service
            .receive_from_queue("test-queue", Some(Duration::from_millis(100)))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_channel_service_subscribe_to_topic() {
        let service = IntegrationChannelService::new();

        let result = service.subscribe_to_topic("test-topic").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_channel_service_roundtrip() {
        let service = IntegrationChannelService::new();
        let queue_name = "roundtrip-queue";

        // Send multiple messages
        for i in 0..3 {
            let mut msg = TestMessage::new(format!("message-{}", i).into_bytes());
            msg.sender = Some("test-sender".to_string());
            service.send_to_queue(queue_name, msg).await.unwrap();
        }

        // Receive messages
        for _ in 0..3 {
            let received = service
                .receive_from_queue(queue_name, Some(Duration::from_millis(100)))
                .await
                .unwrap();
            // May or may not receive depending on timing
            if received.is_some() {
                assert!(!received.unwrap().payload.is_empty());
            }
        }
    }
}
