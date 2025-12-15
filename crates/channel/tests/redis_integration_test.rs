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

//! Redis Channel Integration Tests
//!
//! ## Purpose
//! Comprehensive tests for Redis Streams backend using real Redis instance.
//!
//! ## Running Tests
//! ```bash
//! # Start Redis
//! docker-compose up -d redis
//!
//! # Run tests
//! cargo test --features redis-backend --test redis_integration_test
//! ```
//!
//! ## Test Coverage
//! These tests verify:
//! - Send/receive patterns (queue)
//! - Publish/subscribe patterns (pub/sub)
//! - Consumer groups for load balancing
//! - At-least-once delivery with ack/nack
//! - Message persistence across restarts
//! - Channel statistics
//! - Error handling

#![cfg(feature = "redis-backend")]

use futures::StreamExt;
use plexspaces_channel::*;
use plexspaces_proto::channel::v1::*;

// Helper to check if Redis is available
async fn is_redis_available() -> bool {
    redis::Client::open("redis://localhost:6379")
        .and_then(|client| {
            let mut conn = client.get_connection()?;
            redis::cmd("PING").query::<String>(&mut conn)
        })
        .is_ok()
}

// Helper to create test config
fn create_test_config(name: &str) -> ChannelConfig {
    use prost_types::Duration;

    ChannelConfig {
        name: name.to_string(),
        backend: ChannelBackend::ChannelBackendRedis as i32,
        capacity: 100,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        backend_config: Some(channel_config::BackendConfig::Redis(RedisConfig {
            url: "redis://localhost:6379".to_string(),
            stream_key: format!("test-stream:{}", name),
            max_length: 1000,
            consumer_group: "".to_string(),
            consumer_name: "".to_string(),
            claim_timeout: Some(Duration {
                seconds: 5,
                nanos: 0,
            }),
            pool_size: 10,
        })),
        ..Default::default()
    }
}

// Helper to cleanup Redis streams
async fn cleanup_redis_stream(stream_name: &str) {
    if let Ok(client) = redis::Client::open("redis://localhost:6379") {
        if let Ok(mut conn) = client.get_connection() {
            let _: Result<(), redis::RedisError> =
                redis::cmd("DEL").arg(stream_name).query(&mut conn);
        }
    }
}

#[tokio::test]
async fn test_redis_send_and_receive_single_message() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let stream_name = "test-redis-single-msg";
    cleanup_redis_stream(stream_name).await;

    let config = create_test_config(stream_name);
    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create Redis channel");

    // Send message
    let msg = ChannelMessage {
        id: ulid::Ulid::new().to_string(),
        channel: stream_name.to_string(),
        payload: b"Hello Redis".to_vec(),
        ..Default::default()
    };
    let msg_id = channel.send(msg.clone()).await.expect("Failed to send");
    assert!(!msg_id.is_empty());

    // Receive message
    let messages = channel.receive(1).await.expect("Failed to receive");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].payload, b"Hello Redis");

    // Cleanup
    cleanup_redis_stream(stream_name).await;
}

#[tokio::test]
async fn test_redis_send_and_receive_multiple_messages() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let stream_name = "test-redis-multiple-msg";
    cleanup_redis_stream(stream_name).await;

    let config = create_test_config(stream_name);
    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send 5 messages
    for i in 0..5 {
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: stream_name.to_string(),
            payload: format!("Message {}", i).into_bytes(),
            ..Default::default()
        };
        channel.send(msg).await.expect("Failed to send");
    }

    // Receive all messages
    let messages = channel.receive(10).await.expect("Failed to receive");
    assert_eq!(messages.len(), 5);

    // Verify order (FIFO)
    for (i, msg) in messages.iter().enumerate() {
        assert_eq!(msg.payload, format!("Message {}", i).into_bytes());
    }

    cleanup_redis_stream(stream_name).await;
}

#[tokio::test]
async fn test_redis_try_receive_empty() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let stream_name = "test-redis-try-receive-empty";
    cleanup_redis_stream(stream_name).await;

    let config = create_test_config(stream_name);
    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Try receive from empty stream
    let messages = channel.try_receive(1).await.expect("Failed to try_receive");
    assert_eq!(messages.len(), 0);

    cleanup_redis_stream(stream_name).await;
}

#[tokio::test]
async fn test_redis_try_receive_with_messages() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let stream_name = "test-redis-try-receive";
    cleanup_redis_stream(stream_name).await;

    let config = create_test_config(stream_name);
    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send message
    let msg = ChannelMessage {
        id: ulid::Ulid::new().to_string(),
        channel: stream_name.to_string(),
        payload: b"Quick message".to_vec(),
        ..Default::default()
    };
    channel.send(msg).await.expect("Failed to send");

    // Try receive
    let messages = channel.try_receive(1).await.expect("Failed to try_receive");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].payload, b"Quick message");

    cleanup_redis_stream(stream_name).await;
}

#[tokio::test]
async fn test_redis_publish_subscribe() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let channel_name = "test-redis-pubsub";
    cleanup_redis_stream(channel_name).await;

    let config = create_test_config(channel_name);
    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Subscribe first
    let mut stream = channel.subscribe(None).await.expect("Failed to subscribe");

    // Publish message (in background task)
    let channel_clone = channel.clone();
    let publish_handle = tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: channel_name.to_string(),
            payload: b"Pub/Sub Event".to_vec(),
            ..Default::default()
        };
        channel_clone.publish(msg).await.expect("Failed to publish");
    });

    // Receive from subscription with timeout
    let received = tokio::time::timeout(tokio::time::Duration::from_secs(2), stream.next())
        .await
        .expect("Timeout waiting for message");

    assert!(received.is_some());
    let msg = received.unwrap();
    assert_eq!(msg.payload, b"Pub/Sub Event");

    publish_handle.await.expect("Publish task failed");
    cleanup_redis_stream(channel_name).await;
}

#[tokio::test]
async fn test_redis_ack() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let stream_name = "test-redis-ack";
    cleanup_redis_stream(stream_name).await;

    let config = create_test_config(stream_name);
    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send message
    let msg = ChannelMessage {
        id: ulid::Ulid::new().to_string(),
        channel: stream_name.to_string(),
        payload: b"Ack me".to_vec(),
        ..Default::default()
    };
    let msg_id = channel.send(msg).await.expect("Failed to send");

    // Receive message
    let messages = channel.receive(1).await.expect("Failed to receive");
    assert_eq!(messages.len(), 1);

    // Ack the message
    channel.ack(&msg_id).await.expect("Failed to ack");

    // Message should not be redelivered
    let redelivered = channel.try_receive(1).await.expect("Failed to try_receive");
    assert_eq!(redelivered.len(), 0);

    cleanup_redis_stream(stream_name).await;
}

#[tokio::test]
async fn test_redis_nack_requeue() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let stream_name = "test-redis-nack";
    cleanup_redis_stream(stream_name).await;

    let config = create_test_config(stream_name);
    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send message
    let msg = ChannelMessage {
        id: ulid::Ulid::new().to_string(),
        channel: stream_name.to_string(),
        payload: b"Nack me".to_vec(),
        ..Default::default()
    };
    let msg_id = channel.send(msg).await.expect("Failed to send");

    // Receive message
    let messages = channel.receive(1).await.expect("Failed to receive");
    assert_eq!(messages.len(), 1);

    // Nack with requeue
    channel.nack(&msg_id, true).await.expect("Failed to nack");

    // Message should be redelivered
    let redelivered = channel
        .receive(1)
        .await
        .expect("Failed to receive requeued");
    assert_eq!(redelivered.len(), 1);
    assert_eq!(redelivered[0].payload, b"Nack me");

    cleanup_redis_stream(stream_name).await;
}

#[tokio::test]
async fn test_redis_consumer_group() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let stream_name = "test-redis-consumer-group";
    cleanup_redis_stream(stream_name).await;

    // Create channel with consumer group
    let mut config = create_test_config(stream_name);
    if let Some(channel_config::BackendConfig::Redis(ref mut redis_cfg)) = config.backend_config {
        redis_cfg.consumer_group = "test-group".to_string();
        redis_cfg.consumer_name = "consumer-1".to_string();
    }

    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send messages
    for i in 0..3 {
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: stream_name.to_string(),
            payload: format!("Group message {}", i).into_bytes(),
            ..Default::default()
        };
        channel.send(msg).await.expect("Failed to send");
    }

    // Receive with consumer group
    let messages = channel.receive(3).await.expect("Failed to receive");
    assert_eq!(messages.len(), 3);

    cleanup_redis_stream(stream_name).await;
}

#[tokio::test]
async fn test_redis_get_stats() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let stream_name = "test-redis-stats";
    cleanup_redis_stream(stream_name).await;

    let config = create_test_config(stream_name);
    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send some messages
    for i in 0..5 {
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: stream_name.to_string(),
            payload: format!("Stats message {}", i).into_bytes(),
            ..Default::default()
        };
        channel.send(msg).await.expect("Failed to send");
    }

    // Get stats
    let stats = channel.get_stats().await.expect("Failed to get stats");
    assert_eq!(stats.messages_sent, 5);
    assert!(stats.messages_pending > 0);

    cleanup_redis_stream(stream_name).await;
}

#[tokio::test]
async fn test_redis_close_channel() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let stream_name = "test-redis-close";
    cleanup_redis_stream(stream_name).await;

    let config = create_test_config(stream_name);
    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create channel");

    assert!(!channel.is_closed());

    // Close channel
    channel.close().await.expect("Failed to close");
    assert!(channel.is_closed());

    // Sending should fail
    let msg = ChannelMessage {
        id: ulid::Ulid::new().to_string(),
        channel: stream_name.to_string(),
        payload: b"Should fail".to_vec(),
        ..Default::default()
    };
    let result = channel.send(msg).await;
    assert!(matches!(result, Err(ChannelError::ChannelClosed(_))));

    cleanup_redis_stream(stream_name).await;
}

#[tokio::test]
async fn test_redis_message_persistence() {
    if !is_redis_available().await {
        eprintln!("Skipping test: Redis not available");
        return;
    }

    let stream_name = "test-redis-persistence";
    cleanup_redis_stream(stream_name).await;

    // Create channel and send message
    {
        let config = create_test_config(stream_name);
        let channel = RedisChannel::new(config)
            .await
            .expect("Failed to create channel");

        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: stream_name.to_string(),
            payload: b"Persistent message".to_vec(),
            ..Default::default()
        };
        channel.send(msg).await.expect("Failed to send");
    }
    // Channel dropped here

    // Create new channel instance - should still see message
    let config = create_test_config(stream_name);
    let channel = RedisChannel::new(config)
        .await
        .expect("Failed to create channel");

    let messages = channel.receive(1).await.expect("Failed to receive");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].payload, b"Persistent message");

    cleanup_redis_stream(stream_name).await;
}
