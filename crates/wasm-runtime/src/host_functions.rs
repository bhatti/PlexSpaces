// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Host functions provided to WASM actors

use async_trait::async_trait;
use plexspaces_core::ChannelService;
use plexspaces_mailbox::Message;
use std::sync::Arc;

/// Trait for sending messages from WASM actors to other actors
///
/// ## Purpose
/// Enables WASM actors to send messages to other actors (local or remote).
/// Implemented by the node/actor system to route messages.
///
/// ## Design (Simplicity + Extensibility)
/// - Simple trait interface
/// - Can be implemented by any message routing system
/// - Supports both local and remote actors
#[async_trait]
pub trait MessageSender: Send + Sync {
    /// Send a message to an actor
    ///
    /// ## Arguments
    /// * `from` - Sender actor ID
    /// * `to` - Recipient actor ID (can be "actor@node" format for remote)
    /// * `message` - Message payload
    ///
    /// ## Returns
    /// Success or error
    async fn send_message(&self, from: &str, to: &str, message: &str) -> Result<(), String>;
}

/// Host functions for WASM actors
pub struct HostFunctions {
    /// Message sender for routing messages (optional)
    message_sender: Option<Box<dyn MessageSender>>,
    /// Channel service for queue/topic patterns (optional)
    channel_service: Option<Arc<dyn ChannelService>>,
}

impl HostFunctions {
    /// Create new host functions context
    pub fn new() -> Self {
        Self {
            message_sender: None,
            channel_service: None,
        }
    }

    /// Create with message sender for multi-node coordination
    pub fn with_message_sender(sender: Box<dyn MessageSender>) -> Self {
        Self {
            message_sender: Some(sender),
            channel_service: None,
        }
    }

    /// Create with channel service for queue/topic patterns
    pub fn with_channel_service(channel_service: Arc<dyn ChannelService>) -> Self {
        Self {
            message_sender: None,
            channel_service: Some(channel_service),
        }
    }

    /// Create with both message sender and channel service
    pub fn with_services(
        sender: Option<Box<dyn MessageSender>>,
        channel_service: Option<Arc<dyn ChannelService>>,
    ) -> Self {
        Self {
            message_sender: sender,
            channel_service,
        }
    }

    /// Send message via message sender if available
    pub async fn send_message(&self, from: &str, to: &str, message: &str) -> Result<(), String> {
        if let Some(sender) = &self.message_sender {
            sender.send_message(from, to, message).await
        } else {
            // Fallback: log message (for development/testing)
            tracing::warn!(
                from = from,
                to = to,
                message = message,
                "Message sender not configured, message not delivered"
            );
            Ok(())
        }
    }

    /// Send message to queue via channel service if available
    pub async fn send_to_queue(
        &self,
        queue_name: &str,
        message_type: &str,
        payload: Vec<u8>,
    ) -> Result<String, String> {
        if let Some(channel_service) = &self.channel_service {
            let message = Message::new(payload).with_message_type(message_type.to_string());
            channel_service
                .send_to_queue(queue_name, message)
                .await
                .map_err(|e| e.to_string())
        } else {
            Err("Channel service not configured".to_string())
        }
    }

    /// Publish message to topic via channel service if available
    pub async fn publish_to_topic(
        &self,
        topic_name: &str,
        message_type: &str,
        payload: Vec<u8>,
    ) -> Result<String, String> {
        if let Some(channel_service) = &self.channel_service {
            let message = Message::new(payload).with_message_type(message_type.to_string());
            channel_service
                .publish_to_topic(topic_name, message)
                .await
                .map_err(|e| e.to_string())
        } else {
            Err("Channel service not configured".to_string())
        }
    }

    /// Receive message from queue via channel service if available
    pub async fn receive_from_queue(
        &self,
        queue_name: &str,
        timeout_ms: u64,
    ) -> Result<Option<(String, Vec<u8>)>, String> {
        if let Some(channel_service) = &self.channel_service {
            let timeout = if timeout_ms > 0 {
                Some(std::time::Duration::from_millis(timeout_ms))
            } else {
                None
            };
            match channel_service.receive_from_queue(queue_name, timeout).await {
                Ok(Some(message)) => {
                    let message_type = message.message_type_str().to_string();
                    Ok(Some((message_type, message.payload().to_vec())))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        } else {
            Err("Channel service not configured".to_string())
        }
    }
}

impl Default for HostFunctions {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_core::ChannelService;
    use plexspaces_mailbox::Message;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use std::collections::HashMap;
    use futures::StreamExt;

    /// Mock ChannelService for testing
    struct MockChannelService {
        queues: Arc<RwLock<HashMap<String, Vec<Message>>>>,
        topics: Arc<RwLock<HashMap<String, Vec<Message>>>>,
    }

    impl MockChannelService {
        fn new() -> Self {
            Self {
                queues: Arc::new(RwLock::new(HashMap::new())),
                topics: Arc::new(RwLock::new(HashMap::new())),
            }
        }

        async fn get_queue_message(&self, queue_name: &str) -> Option<Message> {
            let mut queues = self.queues.write().await;
            queues.get_mut(queue_name)?.pop()
        }

        async fn get_topic_message(&self, topic_name: &str) -> Option<Message> {
            let mut topics = self.topics.write().await;
            topics.get_mut(topic_name)?.pop()
        }
    }

    #[async_trait::async_trait]
    impl ChannelService for MockChannelService {
        async fn send_to_queue(
            &self,
            queue_name: &str,
            message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let mut queues = self.queues.write().await;
            queues.entry(queue_name.to_string())
                .or_insert_with(Vec::new)
                .push(message);
            Ok("msg-001".to_string())
        }

        async fn publish_to_topic(
            &self,
            topic_name: &str,
            message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let mut topics = self.topics.write().await;
            topics.entry(topic_name.to_string())
                .or_insert_with(Vec::new)
                .push(message);
            Ok("msg-001".to_string())
        }

        async fn subscribe_to_topic(
            &self,
            _topic_name: &str,
        ) -> Result<futures::stream::BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
            use futures::stream;
            Ok(Box::pin(stream::empty()))
        }

        async fn receive_from_queue(
            &self,
            queue_name: &str,
            _timeout: Option<std::time::Duration>,
        ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
            let mut queues = self.queues.write().await;
            if let Some(queue) = queues.get_mut(queue_name) {
                if !queue.is_empty() {
                    return Ok(Some(queue.remove(0)));
                }
            }
            Ok(None)
        }
    }

    #[test]
    fn test_new() {
        let _host_functions = HostFunctions::new();
        // Successfully creates instance
    }

    #[test]
    fn test_default() {
        let _host_functions = HostFunctions::default();
        // Successfully creates instance via Default
    }

    #[test]
    fn test_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<HostFunctions>();
        assert_sync::<HostFunctions>();
    }

    #[tokio::test]
    async fn test_send_to_queue_with_channel_service() {
        let channel_service = Arc::new(MockChannelService::new());
        let host_functions = HostFunctions::with_channel_service(channel_service.clone());

        let result = host_functions
            .send_to_queue("test-queue", "test-type", b"test payload".to_vec())
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "msg-001");

        // Verify message was added to queue
        let message = channel_service.get_queue_message("test-queue").await;
        assert!(message.is_some());
        let msg = message.unwrap();
        assert_eq!(msg.message_type_str(), "test-type");
        assert_eq!(msg.payload(), b"test payload");
    }

    #[tokio::test]
    async fn test_send_to_queue_without_channel_service() {
        let host_functions = HostFunctions::new();

        let result = host_functions
            .send_to_queue("test-queue", "test-type", b"test payload".to_vec())
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Channel service not configured");
    }

    #[tokio::test]
    async fn test_publish_to_topic_with_channel_service() {
        let channel_service = Arc::new(MockChannelService::new());
        let host_functions = HostFunctions::with_channel_service(channel_service.clone());

        let result = host_functions
            .publish_to_topic("test-topic", "event-type", b"event data".to_vec())
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "msg-001");

        // Verify message was published to topic
        let message = channel_service.get_topic_message("test-topic").await;
        assert!(message.is_some());
        let msg = message.unwrap();
        assert_eq!(msg.message_type_str(), "event-type");
        assert_eq!(msg.payload(), b"event data");
    }

    #[tokio::test]
    async fn test_publish_to_topic_without_channel_service() {
        let host_functions = HostFunctions::new();

        let result = host_functions
            .publish_to_topic("test-topic", "event-type", b"event data".to_vec())
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Channel service not configured");
    }

    #[tokio::test]
    async fn test_receive_from_queue_with_message() {
        let channel_service = Arc::new(MockChannelService::new());
        let host_functions = HostFunctions::with_channel_service(channel_service.clone());

        // First, send a message to the queue
        let _ = host_functions
            .send_to_queue("test-queue", "test-type", b"test payload".to_vec())
            .await;

        // Then receive it
        let result = host_functions
            .receive_from_queue("test-queue", 1000)
            .await;

        assert!(result.is_ok());
        let received = result.unwrap();
        assert!(received.is_some());
        let (msg_type, payload) = received.unwrap();
        assert_eq!(msg_type, "test-type");
        assert_eq!(payload, b"test payload");
    }

    #[tokio::test]
    async fn test_receive_from_queue_empty() {
        let channel_service = Arc::new(MockChannelService::new());
        let host_functions = HostFunctions::with_channel_service(channel_service);

        // Try to receive from empty queue
        let result = host_functions
            .receive_from_queue("empty-queue", 100)
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_receive_from_queue_without_channel_service() {
        let host_functions = HostFunctions::new();

        let result = host_functions
            .receive_from_queue("test-queue", 1000)
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Channel service not configured");
    }

    #[tokio::test]
    async fn test_with_services() {
        let channel_service = Arc::new(MockChannelService::new());
        let host_functions = HostFunctions::with_services(None, Some(channel_service.clone()));

        // Should work with channel service
        let result = host_functions
            .send_to_queue("test-queue", "test-type", b"test".to_vec())
            .await;

        assert!(result.is_ok());
    }
}
