// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for ActorContext with real service implementations
// These tests verify end-to-end behavior with actual ChannelServiceWrapper

#[cfg(test)]
mod tests {
    use plexspaces_mailbox::Message;
    use std::sync::Arc;
    use futures::StreamExt;
    use plexspaces_channel::{Channel, InMemoryChannel};
    use async_trait::async_trait;
    use plexspaces_core::{ChannelService, ActorService, ObjectRegistry, TupleSpaceProvider};

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
            
            use plexspaces_proto::channel::v1::ChannelMessage;
            let channel_msg = ChannelMessage {
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
            };
            
            channel.send(channel_msg).await
                .map_err(|e| format!("Failed to send to queue {}: {}", queue_name, e).into())
        }

        async fn publish_to_topic(&self, topic_name: &str, message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let channel = self.get_or_create_channel(topic_name).await?;
            
            use plexspaces_proto::channel::v1::ChannelMessage;
            let channel_msg = ChannelMessage {
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
            };
            
            channel.publish(channel_msg).await
                .map(|_| message.id.clone())
                .map_err(|e| format!("Failed to publish to topic {}: {}", topic_name, e).into())
        }

        async fn subscribe_to_topic(&self, topic_name: &str) -> Result<futures::stream::BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
            let channel = self.get_or_create_channel(topic_name).await?;
            
            let stream = channel.subscribe(None).await
                .map_err(|e| format!("Failed to subscribe to topic {}: {}", topic_name, e))?;
            
            let message_stream = stream.map(|channel_msg| {
                let mut msg = Message::new(channel_msg.payload);
                msg.metadata = channel_msg.headers;
                msg.priority = plexspaces_mailbox::MessagePriority::Normal;
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
                msg.receiver = channel_msg.channel.clone();
                msg.message_type = String::new();
                msg
            });
            
            Ok(Box::pin(message_stream))
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
            
            if messages.is_empty() {
                return Ok(None);
            }
            
            let channel_msg = &messages[0];
            let mut msg = Message::new(channel_msg.payload.clone());
            msg.metadata = channel_msg.headers.clone();
            msg.priority = plexspaces_mailbox::MessagePriority::Normal;
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
            msg.receiver = channel_msg.channel.clone();
            msg.message_type = String::new();
            Ok(Some(msg))
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
}

