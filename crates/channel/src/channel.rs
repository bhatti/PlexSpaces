// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Channel trait and error types

use async_trait::async_trait;
use futures::stream::BoxStream;
use plexspaces_proto::channel::v1::{ChannelBackend, ChannelConfig, ChannelMessage, ChannelStats};
use thiserror::Error;

/// Errors that can occur during channel operations
#[derive(Error, Debug)]
pub enum ChannelError {
    /// Channel is full (bounded channels only)
    #[error("Channel full: {0}")]
    ChannelFull(String),

    /// Channel is closed
    #[error("Channel closed: {0}")]
    ChannelClosed(String),

    /// Timeout waiting for operation
    #[error("Operation timeout: {0}")]
    Timeout(String),

    /// Message not found
    #[error("Message not found: {0}")]
    MessageNotFound(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Backend-specific error
    #[error("Backend error: {0}")]
    BackendError(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Result type for channel operations
pub type ChannelResult<T> = Result<T, ChannelError>;

/// Core channel trait for extensible messaging
///
/// ## Purpose
/// Defines the common interface for all channel implementations, enabling
/// pluggable backends (InMemory, Redis, Kafka) with consistent API.
///
/// ## Design Decisions
/// - **Async**: All operations are async for non-blocking I/O
/// - **Generic Backend**: Implementations can use any transport (memory, network, disk)
/// - **Proto-Based**: All types come from channel.proto for RPC compatibility
///
/// ## Invariants
/// - Channel name must be unique within a node
/// - Messages are delivered according to configured guarantees
/// - Closed channels reject new messages but can drain existing ones
#[async_trait]
pub trait Channel: Send + Sync {
    /// Send a message to the channel
    ///
    /// ## Arguments
    /// * `message` - Message to send with payload and metadata
    ///
    /// ## Returns
    /// `Ok(message_id)` on success
    ///
    /// ## Errors
    /// - [`ChannelError::ChannelFull`]: Bounded channel at capacity
    /// - [`ChannelError::ChannelClosed`]: Channel has been closed
    /// - [`ChannelError::Timeout`]: Send timeout exceeded
    /// - [`ChannelError::BackendError`]: Backend-specific failure
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_channel::*;
    /// # use plexspaces_proto::plexspaces::channel::v1::*;
    /// # async fn example(channel: &dyn Channel) -> ChannelResult<()> {
    /// let msg = ChannelMessage {
    ///     id: ulid::Ulid::new().to_string(),
    ///     channel: "work-queue".to_string(),
    ///     payload: b"task data".to_vec(),
    ///     ..Default::default()
    /// };
    /// let msg_id = channel.send(msg).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn send(&self, message: ChannelMessage) -> ChannelResult<String>;

    /// Receive messages from the channel (blocking until available)
    ///
    /// ## Arguments
    /// * `max_messages` - Maximum number of messages to receive (1 for single message)
    ///
    /// ## Returns
    /// Vector of messages (may be empty if timeout)
    ///
    /// ## Errors
    /// - [`ChannelError::ChannelClosed`]: Channel has been closed
    /// - [`ChannelError::Timeout`]: Receive timeout exceeded
    /// - [`ChannelError::BackendError`]: Backend-specific failure
    ///
    /// ## Performance
    /// Blocks until at least one message available or timeout. Use `try_receive`
    /// for non-blocking operation.
    async fn receive(&self, max_messages: u32) -> ChannelResult<Vec<ChannelMessage>>;

    /// Try to receive messages without blocking
    ///
    /// ## Arguments
    /// * `max_messages` - Maximum number of messages to receive
    ///
    /// ## Returns
    /// Vector of messages (empty if none available)
    ///
    /// ## Performance
    /// Non-blocking, returns immediately with available messages.
    async fn try_receive(&self, max_messages: u32) -> ChannelResult<Vec<ChannelMessage>>;

    /// Subscribe to channel messages (streaming, for pub/sub pattern)
    ///
    /// ## Arguments
    /// * `consumer_group` - Optional consumer group for load-balanced consumption
    ///
    /// ## Returns
    /// Stream of messages that can be consumed asynchronously
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_channel::*;
    /// # use futures::StreamExt;
    /// # async fn example(channel: &dyn Channel) -> ChannelResult<()> {
    /// let mut stream = channel.subscribe(None).await?;
    /// while let Some(msg) = stream.next().await {
    ///     println!("Received: {:?}", msg);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    async fn subscribe(
        &self,
        consumer_group: Option<String>,
    ) -> ChannelResult<BoxStream<'static, ChannelMessage>>;

    /// Publish a message to all subscribers (pub/sub pattern)
    ///
    /// ## Arguments
    /// * `message` - Message to publish
    ///
    /// ## Returns
    /// Number of subscribers that received the message
    ///
    /// ## Design Notes
    /// Unlike `send`, publish delivers to ALL subscribers, not just one consumer.
    async fn publish(&self, message: ChannelMessage) -> ChannelResult<u32>;

    /// Acknowledge message processing (for at-least-once delivery)
    ///
    /// ## Arguments
    /// * `message_id` - ID of message to acknowledge
    ///
    /// ## Returns
    /// `Ok(())` if acknowledged successfully
    ///
    /// ## Design Notes
    /// Required for at-least-once delivery guarantee. Messages not acknowledged
    /// within timeout will be redelivered.
    async fn ack(&self, message_id: &str) -> ChannelResult<()>;

    /// Negative acknowledge (requeue message for retry)
    ///
    /// ## Arguments
    /// * `message_id` - ID of message to nack
    /// * `requeue` - Whether to requeue message (true) or send to DLQ (false)
    async fn nack(&self, message_id: &str, requeue: bool) -> ChannelResult<()>;

    /// Get channel statistics
    ///
    /// ## Returns
    /// Channel metrics (message counts, throughput, latency)
    async fn get_stats(&self) -> ChannelResult<ChannelStats>;

    /// Close the channel (stop accepting new messages)
    ///
    /// ## Returns
    /// `Ok(())` if closed successfully
    ///
    /// ## Design Notes
    /// After closing, `send` will fail but `receive` can still drain existing messages.
    async fn close(&self) -> ChannelResult<()>;

    /// Check if channel is closed
    fn is_closed(&self) -> bool;

    /// Get channel configuration
    fn get_config(&self) -> &ChannelConfig;
}

/// Helper to create appropriate channel based on backend type
///
/// ## Purpose
/// Factory function for creating channel instances based on configuration.
///
/// ## Arguments
/// * `config` - Channel configuration specifying backend and parameters
///
/// ## Returns
/// Boxed channel implementation
///
/// ## Errors
/// - [`ChannelError::InvalidConfiguration`]: Invalid backend or parameters
/// - [`ChannelError::BackendError`]: Backend initialization failed
///
/// ## Examples
/// ```rust
/// # use plexspaces_channel::*;
/// # use plexspaces_proto::plexspaces::channel::v1::*;
/// # async fn example() -> ChannelResult<()> {
/// let config = ChannelConfig {
///     name: "my-channel".to_string(),
///     backend: ChannelBackend::ChannelBackendInMemory as i32,
///     capacity: 100,
///     ..Default::default()
/// };
/// let channel = create_channel(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_channel(config: ChannelConfig) -> ChannelResult<Box<dyn Channel>> {
    use crate::InMemoryChannel;

    match config.backend() {
        ChannelBackend::ChannelBackendInMemory => {
            let channel = InMemoryChannel::new(config).await?;
            Ok(Box::new(channel))
        }
        #[cfg(feature = "redis-backend")]
        ChannelBackend::ChannelBackendRedis => {
            use crate::RedisChannel;
            let channel = RedisChannel::new(config).await?;
            Ok(Box::new(channel))
        }
        #[cfg(not(feature = "redis-backend"))]
        ChannelBackend::ChannelBackendRedis => Err(ChannelError::InvalidConfiguration(
            "Redis backend not enabled. Enable 'redis-backend' feature.".to_string(),
        )),
        #[cfg(feature = "kafka-backend")]
        ChannelBackend::ChannelBackendKafka => {
            use crate::KafkaChannel;
            let channel = KafkaChannel::new(config).await?;
            Ok(Box::new(channel))
        }
        #[cfg(not(feature = "kafka-backend"))]
        ChannelBackend::ChannelBackendKafka => Err(ChannelError::InvalidConfiguration(
            "Kafka backend not enabled. Enable 'kafka-backend' feature.".to_string(),
        )),
        #[cfg(feature = "nats-backend")]
        ChannelBackend::ChannelBackendNats => {
            use crate::NatsChannel;
            let channel = NatsChannel::new(config).await?;
            Ok(Box::new(channel))
        }
        #[cfg(not(feature = "nats-backend"))]
        ChannelBackend::ChannelBackendNats => Err(ChannelError::InvalidConfiguration(
            "NATS backend not enabled. Enable 'nats-backend' feature.".to_string(),
        )),
        #[cfg(feature = "sqlite-backend")]
        ChannelBackend::ChannelBackendSqlite => {
            use crate::SqliteChannel;
            let channel = SqliteChannel::new(config).await?;
            Ok(Box::new(channel))
        }
        #[cfg(not(feature = "sqlite-backend"))]
        ChannelBackend::ChannelBackendSqlite => Err(ChannelError::InvalidConfiguration(
            "SQLite backend not enabled. Enable 'sqlite-backend' feature.".to_string(),
        )),
        #[cfg(feature = "sqs-backend")]
        ChannelBackend::ChannelBackendSqs => {
            use crate::SQSChannel;
            let channel = SQSChannel::new(config).await?;
            Ok(Box::new(channel))
        }
        #[cfg(not(feature = "sqs-backend"))]
        ChannelBackend::ChannelBackendSqs => Err(ChannelError::InvalidConfiguration(
            "SQS backend not enabled. Enable 'sqs-backend' feature.".to_string(),
        )),
        #[cfg(feature = "udp-backend")]
        ChannelBackend::ChannelBackendUdp => {
            use crate::UdpChannel;
            let channel = UdpChannel::new(config).await?;
            Ok(Box::new(channel))
        }
        #[cfg(not(feature = "udp-backend"))]
        ChannelBackend::ChannelBackendUdp => Err(ChannelError::InvalidConfiguration(
            "UDP backend not enabled. Enable 'udp-backend' feature.".to_string(),
        )),
        ChannelBackend::ChannelBackendCustom => Err(ChannelError::InvalidConfiguration(
            "Custom backend requires manual instantiation".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::channel::v1::{DeliveryGuarantee, OrderingGuarantee};

    #[tokio::test]
    async fn test_create_channel_in_memory() {
        let config = ChannelConfig {
            name: "test-channel".to_string(),
            backend: ChannelBackend::ChannelBackendInMemory as i32,
            capacity: 10,
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };

        let result = create_channel(config).await;
        assert!(result.is_ok());

        let channel = result.unwrap();
        assert!(!channel.is_closed());
    }

    #[tokio::test]
    async fn test_create_channel_kafka_not_implemented() {
        let config = ChannelConfig {
            name: "test-channel".to_string(),
            backend: ChannelBackend::ChannelBackendKafka as i32,
            ..Default::default()
        };

        let result = create_channel(config).await;
        assert!(matches!(result, Err(ChannelError::InvalidConfiguration(_))));
    }
}
