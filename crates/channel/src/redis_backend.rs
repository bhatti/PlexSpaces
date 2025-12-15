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

//! Redis Streams backend for distributed channels
//!
//! ## Purpose
//! Provides distributed, persistent channel implementation using Redis Streams
//! with consumer groups for load-balanced consumption.
//!
//! ## Architecture Context
//! Redis backend enables:
//! - **Cross-Node Communication**: Messages persist in Redis, accessible from any node
//! - **At-Least-Once Delivery**: Consumer groups with ACK/NACK semantics
//! - **Persistence**: Messages survive node restarts
//! - **Load Balancing**: Multiple consumers in same group share work
//!
//! ## Design Decisions
//! - **Redis Streams**: Native support for consumer groups and message acknowledgment
//! - **XADD for send**: Append messages to stream with automatic ID generation
//! - **XREADGROUP for receive**: Consumer group-based consumption
//! - **XACK for ack**: Acknowledge message processing
//! - **XCLAIM for nack**: Reclaim messages for redelivery
//!
//! ## Performance
//! - Latency: < 1ms for send/receive (local Redis)
//! - Throughput: > 100K messages/second
//! - Persistence: AOF/RDB snapshots for durability

use crate::{Channel, ChannelError, ChannelResult};
use async_trait::async_trait;
use futures::stream::BoxStream;
use plexspaces_proto::channel::v1::{
    channel_config, ChannelBackend, ChannelConfig, ChannelMessage, ChannelStats, RedisConfig,
};
use redis::aio::Connection;
use redis::{Client, RedisResult, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Redis channel implementation using Redis Streams
///
/// ## Purpose
/// Distributed channel backend using Redis Streams for persistent messaging
/// across multiple nodes with consumer group support.
///
/// ## Invariants
/// - Stream key format: "channel:{channel_name}"
/// - Consumer group created on first use
/// - Message IDs are Redis-generated (timestamp-sequence)
/// - ACKs required for at-least-once delivery
#[derive(Clone)]
pub struct RedisChannel {
    config: ChannelConfig,
    redis_config: RedisConfig,
    client: Client,
    stream_key: String,
    consumer_group: String,
    consumer_name: String,
    stats: Arc<ChannelStatsData>,
    closed: Arc<AtomicBool>,
}

struct ChannelStatsData {
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    messages_acked: AtomicU64,
    messages_nacked: AtomicU64,
    errors: AtomicU64,
}

impl RedisChannel {
    /// Get blocking timeout in milliseconds from claim_timeout Duration
    fn get_block_ms(&self) -> i64 {
        if let Some(ref timeout) = self.redis_config.claim_timeout {
            // Convert seconds and nanos to milliseconds
            (timeout.seconds * 1000) + (timeout.nanos as i64 / 1_000_000)
        } else {
            5000 // Default 5 seconds
        }
    }

    /// Create a new Redis channel
    ///
    /// ## Arguments
    /// * `config` - Channel configuration with Redis backend config
    ///
    /// ## Returns
    /// New RedisChannel instance connected to Redis
    ///
    /// ## Errors
    /// - [`ChannelError::InvalidConfiguration`]: Missing Redis config
    /// - [`ChannelError::BackendError`]: Failed to connect to Redis
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_channel::*;
    /// # use plexspaces_proto::channel::v1::*;
    /// # async fn example() -> ChannelResult<()> {
    /// let config = ChannelConfig {
    ///     name: "my-stream".to_string(),
    ///     backend: ChannelBackend::ChannelBackendRedis as i32,
    ///     backend_config: Some(channel_config::BackendConfig::RedisConfig(
    ///         RedisConfig {
    ///             url: "redis://localhost:6379".to_string(),
    ///             stream_max_len: 1000,
    ///             consumer_group: "my-group".to_string(),
    ///             consumer_name: "consumer-1".to_string(),
    ///             block_ms: 5000,
    ///         }
    ///     )),
    ///     ..Default::default()
    /// };
    /// let channel = RedisChannel::new(config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(config: ChannelConfig) -> ChannelResult<Self> {
        // Extract Redis config
        let redis_config = match &config.backend_config {
            Some(channel_config::BackendConfig::Redis(cfg)) => cfg.clone(),
            _ => {
                return Err(ChannelError::InvalidConfiguration(
                    "Missing Redis configuration".to_string(),
                ))
            }
        };

        // Connect to Redis
        let client = Client::open(redis_config.url.as_str()).map_err(|e| {
            ChannelError::BackendError(format!("Failed to create Redis client: {}", e))
        })?;

        // Test connection
        let mut conn = client.get_async_connection().await.map_err(|e| {
            ChannelError::BackendError(format!("Failed to connect to Redis: {}", e))
        })?;

        // Create stream key
        let stream_key = format!("channel:{}", config.name);

        // Create consumer group if specified (ignore error if already exists)
        if !redis_config.consumer_group.is_empty() {
            let _: RedisResult<Value> = redis::cmd("XGROUP")
                .arg("CREATE")
                .arg(&stream_key)
                .arg(&redis_config.consumer_group)
                .arg("0")
                .arg("MKSTREAM")
                .query_async(&mut conn)
                .await;
            // Ignore error - group might already exist
        }

        // Generate consumer name if not provided
        let consumer_name = if redis_config.consumer_name.is_empty() {
            format!("consumer-{}", ulid::Ulid::new())
        } else {
            redis_config.consumer_name.clone()
        };

        let consumer_group = redis_config.consumer_group.clone();

        Ok(RedisChannel {
            config,
            redis_config,
            client,
            stream_key,
            consumer_group,
            consumer_name,
            stats: Arc::new(ChannelStatsData {
                messages_sent: AtomicU64::new(0),
                messages_received: AtomicU64::new(0),
                messages_acked: AtomicU64::new(0),
                messages_nacked: AtomicU64::new(0),
                errors: AtomicU64::new(0),
            }),
            closed: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Get a Redis connection
    async fn get_connection(&self) -> ChannelResult<Connection> {
        self.client.get_async_connection().await.map_err(|e| {
            self.stats.errors.fetch_add(1, Ordering::Relaxed);
            ChannelError::BackendError(format!("Failed to get Redis connection: {}", e))
        })
    }

    /// Serialize message to Redis fields
    fn serialize_message(msg: &ChannelMessage) -> Vec<(&str, Vec<u8>)> {
        vec![
            ("id", msg.id.as_bytes().to_vec()),
            ("channel", msg.channel.as_bytes().to_vec()),
            ("payload", msg.payload.clone()),
            ("sender_id", msg.sender_id.as_bytes().to_vec()),
            ("correlation_id", msg.correlation_id.as_bytes().to_vec()),
            ("reply_to", msg.reply_to.as_bytes().to_vec()),
            ("partition_key", msg.partition_key.as_bytes().to_vec()),
            (
                "delivery_count",
                msg.delivery_count.to_string().into_bytes(),
            ),
        ]
    }

    /// Deserialize message from Redis fields
    fn deserialize_message(fields: HashMap<String, String>, redis_id: String) -> ChannelMessage {
        ChannelMessage {
            id: fields.get("id").cloned().unwrap_or(redis_id),
            channel: fields.get("channel").cloned().unwrap_or_default(),
            sender_id: fields.get("sender_id").cloned().unwrap_or_default(),
            payload: fields
                .get("payload")
                .map(|s| s.as_bytes().to_vec())
                .unwrap_or_default(),
            correlation_id: fields.get("correlation_id").cloned().unwrap_or_default(),
            reply_to: fields.get("reply_to").cloned().unwrap_or_default(),
            partition_key: fields.get("partition_key").cloned().unwrap_or_default(),
            delivery_count: fields
                .get("delivery_count")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            timestamp: None, // TODO: Parse from Redis ID timestamp
            headers: HashMap::new(),
        }
    }
}

#[async_trait]
impl Channel for RedisChannel {
    async fn send(&self, message: ChannelMessage) -> ChannelResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        let mut conn = self.get_connection().await?;

        // Serialize message fields
        let fields = Self::serialize_message(&message);

        // XADD stream_key * field1 value1 field2 value2 ...
        let mut cmd = redis::cmd("XADD");
        cmd.arg(&self.stream_key).arg("*"); // * = auto-generate ID

        for (key, value) in fields {
            cmd.arg(key).arg(value);
        }

        // Add MAXLEN to limit stream size
        if self.redis_config.max_length > 0 {
            cmd.arg("MAXLEN").arg("~").arg(self.redis_config.max_length);
        }

        let redis_id: String = cmd.query_async(&mut conn).await.map_err(|e| {
            self.stats.errors.fetch_add(1, Ordering::Relaxed);
            ChannelError::BackendError(format!("Failed to send message: {}", e))
        })?;

        self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
        Ok(redis_id)
    }

    async fn receive(&self, max_messages: u32) -> ChannelResult<Vec<ChannelMessage>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        let mut conn = self.get_connection().await?;

        // Use consumer group if configured, otherwise simple read
        let block_ms = self.get_block_ms();
        let messages = if !self.consumer_group.is_empty() {
            // XREADGROUP GROUP group consumer COUNT count BLOCK ms STREAMS stream >
            let result: RedisResult<Vec<Value>> = redis::cmd("XREADGROUP")
                .arg("GROUP")
                .arg(&self.consumer_group)
                .arg(&self.consumer_name)
                .arg("COUNT")
                .arg(max_messages)
                .arg("BLOCK")
                .arg(block_ms)
                .arg("STREAMS")
                .arg(&self.stream_key)
                .arg(">") // Only new messages
                .query_async(&mut conn)
                .await;

            Self::parse_xread_response(result)?
        } else {
            // XREAD COUNT count BLOCK ms STREAMS stream $
            let result: RedisResult<Vec<Value>> = redis::cmd("XREAD")
                .arg("COUNT")
                .arg(max_messages)
                .arg("BLOCK")
                .arg(block_ms)
                .arg("STREAMS")
                .arg(&self.stream_key)
                .arg("$") // Only new messages
                .query_async(&mut conn)
                .await;

            Self::parse_xread_response(result)?
        };

        self.stats
            .messages_received
            .fetch_add(messages.len() as u64, Ordering::Relaxed);
        Ok(messages)
    }

    async fn try_receive(&self, max_messages: u32) -> ChannelResult<Vec<ChannelMessage>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        let mut conn = self.get_connection().await?;

        // Non-blocking read (BLOCK 0)
        let messages = if !self.consumer_group.is_empty() {
            let result: RedisResult<Vec<Value>> = redis::cmd("XREADGROUP")
                .arg("GROUP")
                .arg(&self.consumer_group)
                .arg(&self.consumer_name)
                .arg("COUNT")
                .arg(max_messages)
                .arg("STREAMS")
                .arg(&self.stream_key)
                .arg(">")
                .query_async(&mut conn)
                .await;

            Self::parse_xread_response(result)?
        } else {
            let result: RedisResult<Vec<Value>> = redis::cmd("XREAD")
                .arg("COUNT")
                .arg(max_messages)
                .arg("STREAMS")
                .arg(&self.stream_key)
                .arg("0-0") // From beginning
                .query_async(&mut conn)
                .await;

            Self::parse_xread_response(result)?
        };

        self.stats
            .messages_received
            .fetch_add(messages.len() as u64, Ordering::Relaxed);
        Ok(messages)
    }

    async fn subscribe(
        &self,
        consumer_group: Option<String>,
    ) -> ChannelResult<BoxStream<'static, ChannelMessage>> {
        let stream_key = self.stream_key.clone();
        let group = consumer_group.unwrap_or_else(|| self.consumer_group.clone());
        let consumer = self.consumer_name.clone();
        let client = self.client.clone();
        let block_ms = self.get_block_ms();

        let stream = async_stream::stream! {
            let mut conn = match client.get_async_connection().await {
                Ok(c) => c,
                Err(_) => return,
            };

            loop {
                let result: RedisResult<Vec<Value>> = if !group.is_empty() {
                    redis::cmd("XREADGROUP")
                        .arg("GROUP")
                        .arg(&group)
                        .arg(&consumer)
                        .arg("COUNT")
                        .arg(1)
                        .arg("BLOCK")
                        .arg(block_ms)
                        .arg("STREAMS")
                        .arg(&stream_key)
                        .arg(">")
                        .query_async(&mut conn)
                        .await
                } else {
                    redis::cmd("XREAD")
                        .arg("COUNT")
                        .arg(1)
                        .arg("BLOCK")
                        .arg(block_ms)
                        .arg("STREAMS")
                        .arg(&stream_key)
                        .arg("$")
                        .query_async(&mut conn)
                        .await
                };

                if let Ok(messages) = Self::parse_xread_response(result) {
                    for msg in messages {
                        yield msg;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn publish(&self, message: ChannelMessage) -> ChannelResult<u32> {
        // For Redis, publish is same as send (all consumers will receive)
        self.send(message).await?;
        Ok(1) // Redis doesn't track subscriber count easily
    }

    async fn ack(&self, message_id: &str) -> ChannelResult<()> {
        if self.consumer_group.is_empty() {
            return Ok(()); // No-op if not using consumer groups
        }

        let mut conn = self.get_connection().await?;

        // XACK stream group id
        let _: i32 = redis::cmd("XACK")
            .arg(&self.stream_key)
            .arg(&self.consumer_group)
            .arg(message_id)
            .query_async(&mut conn)
            .await
            .map_err(|e| {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                ChannelError::BackendError(format!("Failed to ack message: {}", e))
            })?;

        self.stats.messages_acked.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn nack(&self, message_id: &str, requeue: bool) -> ChannelResult<()> {
        if self.consumer_group.is_empty() {
            return Ok(()); // No-op if not using consumer groups
        }

        let mut conn = self.get_connection().await?;

        if requeue {
            // XCLAIM to force redelivery by claiming the message back
            let _: Value = redis::cmd("XCLAIM")
                .arg(&self.stream_key)
                .arg(&self.consumer_group)
                .arg(&self.consumer_name)
                .arg(0) // Min idle time (0 = immediate)
                .arg(message_id)
                .query_async(&mut conn)
                .await
                .map_err(|e| {
                    self.stats.errors.fetch_add(1, Ordering::Relaxed);
                    ChannelError::BackendError(format!("Failed to nack/requeue message: {}", e))
                })?;
        }
        // If not requeue, just don't ACK (message will be redelivered after timeout)

        self.stats.messages_nacked.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn get_stats(&self) -> ChannelResult<ChannelStats> {
        let mut conn = self.get_connection().await?;

        // XINFO STREAM to get stream length
        let info: RedisResult<Vec<Value>> = redis::cmd("XINFO")
            .arg("STREAM")
            .arg(&self.stream_key)
            .query_async(&mut conn)
            .await;

        let pending_count = match info {
            Ok(values) => {
                // Parse stream info (format: [key1, value1, key2, value2, ...])
                let mut length = 0i64;
                for i in (0..values.len()).step_by(2) {
                    if let Value::Data(ref key_bytes) = values[i] {
                        if let Ok(key) = String::from_utf8(key_bytes.clone()) {
                            if key == "length" {
                                if let Value::Int(len) = values[i + 1] {
                                    length = len;
                                }
                            }
                        }
                    }
                }
                length as u64
            }
            Err(_) => 0,
        };

        Ok(ChannelStats {
            name: self.config.name.clone(),
            backend: ChannelBackend::ChannelBackendRedis as i32,
            messages_sent: self.stats.messages_sent.load(Ordering::Relaxed),
            messages_received: self.stats.messages_received.load(Ordering::Relaxed),
            messages_pending: pending_count,
            messages_failed: self.stats.errors.load(Ordering::Relaxed),
            ..Default::default()
        })
    }

    async fn close(&self) -> ChannelResult<()> {
        self.closed.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    fn get_config(&self) -> &ChannelConfig {
        &self.config
    }
}

impl RedisChannel {
    /// Parse XREAD/XREADGROUP response into ChannelMessages
    fn parse_xread_response(result: RedisResult<Vec<Value>>) -> ChannelResult<Vec<ChannelMessage>> {
        match result {
            Ok(values) => {
                let mut messages = Vec::new();

                // Response format: [[stream_name, [[id, [field1, value1, field2, value2, ...]], ...]]]
                for stream_value in values {
                    if let Value::Bulk(stream_parts) = stream_value {
                        if stream_parts.len() >= 2 {
                            if let Value::Bulk(entries) = &stream_parts[1] {
                                for entry in entries {
                                    if let Value::Bulk(entry_parts) = entry {
                                        if entry_parts.len() >= 2 {
                                            // Extract message ID
                                            let msg_id = match &entry_parts[0] {
                                                Value::Data(bytes) => {
                                                    String::from_utf8_lossy(bytes).to_string()
                                                }
                                                _ => continue,
                                            };

                                            // Extract fields
                                            if let Value::Bulk(fields) = &entry_parts[1] {
                                                let mut field_map = HashMap::new();
                                                for i in (0..fields.len()).step_by(2) {
                                                    if i + 1 < fields.len() {
                                                        let key = match &fields[i] {
                                                            Value::Data(bytes) => {
                                                                String::from_utf8_lossy(bytes)
                                                                    .to_string()
                                                            }
                                                            _ => continue,
                                                        };
                                                        let value = match &fields[i + 1] {
                                                            Value::Data(bytes) => {
                                                                String::from_utf8_lossy(bytes)
                                                                    .to_string()
                                                            }
                                                            _ => continue,
                                                        };
                                                        field_map.insert(key, value);
                                                    }
                                                }

                                                messages.push(Self::deserialize_message(
                                                    field_map, msg_id,
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                Ok(messages)
            }
            Err(e) => {
                // Empty result or error - return empty vec
                if format!("{:?}", e).contains("nil") || format!("{:?}", e).contains("NOGROUP") {
                    Ok(Vec::new())
                } else {
                    Err(ChannelError::BackendError(format!(
                        "Failed to read from stream: {}",
                        e
                    )))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize_message() {
        let msg = ChannelMessage {
            id: "test-123".to_string(),
            channel: "test-channel".to_string(),
            sender_id: "sender-1".to_string(),
            payload: b"Hello Redis".to_vec(),
            correlation_id: "corr-456".to_string(),
            reply_to: "reply-channel".to_string(),
            partition_key: "part-1".to_string(),
            delivery_count: 2,
            ..Default::default()
        };

        // Serialize
        let fields = RedisChannel::serialize_message(&msg);
        assert_eq!(fields.len(), 8); // 8 fields

        // Create field map for deserialization
        let mut field_map = HashMap::new();
        for (key, value) in fields {
            field_map.insert(key.to_string(), String::from_utf8_lossy(&value).to_string());
        }

        // Deserialize
        let deserialized = RedisChannel::deserialize_message(field_map, "redis-id-123".to_string());
        assert_eq!(deserialized.id, "test-123");
        assert_eq!(deserialized.channel, "test-channel");
        assert_eq!(deserialized.sender_id, "sender-1");
        assert_eq!(deserialized.payload, b"Hello Redis");
        assert_eq!(deserialized.correlation_id, "corr-456");
        assert_eq!(deserialized.reply_to, "reply-channel");
        assert_eq!(deserialized.partition_key, "part-1");
        assert_eq!(deserialized.delivery_count, 2);
    }
}
