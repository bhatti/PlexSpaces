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

//! Kafka Channel Integration Tests
//!
//! ## Purpose
//! Comprehensive tests for Kafka backend using real Kafka broker.
//!
//! ## Running Tests
//! ```bash
//! # Start Kafka and Zookeeper
//! docker-compose up -d kafka zookeeper
//!
//! # Run tests
//! cargo test --features kafka-backend --test kafka_integration_test
//! ```
//!
//! ## Test Coverage
//! These tests verify:
//! - Send/receive patterns (queue)
//! - Publish/subscribe patterns (pub/sub)
//! - Consumer groups for load balancing
//! - Message persistence across restarts
//! - Partition key routing
//! - Channel statistics
//! - Error handling

#![cfg(feature = "kafka-backend")]

use futures::StreamExt;
use plexspaces_channel::*;
use plexspaces_proto::channel::v1::*;
use plexspaces_proto::prost_types::Duration;
use rdkafka::admin::{AdminClient, AdminOptions};
use rdkafka::config::ClientConfig;
use rdkafka::consumer::BaseConsumer;

// Helper to check if Kafka is available
async fn is_kafka_available() -> bool {
    let client_config = ClientConfig::new()
        .set("bootstrap.servers", "localhost:9092")
        .set("socket.timeout.ms", "2000")
        .clone();

    match client_config.create::<BaseConsumer>() {
        Ok(_) => true,
        Err(_) => false,
    }
}

// Helper to create test config
fn create_test_config(name: &str) -> ChannelConfig {
    ChannelConfig {
        name: name.to_string(),
        backend: ChannelBackend::ChannelBackendKafka as i32,
        capacity: 100,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        backend_config: Some(channel_config::BackendConfig::Kafka(KafkaConfig {
            brokers: vec!["localhost:9092".to_string()],
            topic: format!("test-topic-{}", name),
            consumer_group: format!("test-group-{}", name),
            partitions: 1,
            replication_factor: 1,
            compression: kafka_config::CompressionType::CompressionTypeSnappy as i32,
            acks: kafka_config::ProducerAcks::ProducerAcksLeader as i32,
            batch_size: 16384,
            linger_ms: Some(Duration {
                seconds: 0,
                nanos: 10_000_000, // 10ms
            }),
        })),
        ..Default::default()
    }
}

// Helper to cleanup Kafka topic
async fn cleanup_kafka_topic(topic_name: &str) {
    let admin_config = ClientConfig::new()
        .set("bootstrap.servers", "localhost:9092")
        .clone();

    if let Ok(admin_client) = admin_config.create::<AdminClient<_>>() {
        let _ = admin_client
            .delete_topics(&[topic_name], &AdminOptions::new())
            .await;
        // Wait a bit for deletion to propagate
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_send_and_receive_single_message() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let topic_name = "test-kafka-single-msg";
    cleanup_kafka_topic(topic_name).await;

    let config = create_test_config(topic_name);
    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create Kafka channel");

    // Send message
    let msg = ChannelMessage {
        id: ulid::Ulid::new().to_string(),
        channel: topic_name.to_string(),
        payload: b"Hello Kafka".to_vec(),
        ..Default::default()
    };
    let msg_id = channel.send(msg.clone()).await.expect("Failed to send");
    assert!(!msg_id.is_empty());

    // Wait for message to be available
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Receive message
    let messages = channel.receive(1).await.expect("Failed to receive");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].payload, b"Hello Kafka");

    cleanup_kafka_topic(topic_name).await;
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_send_and_receive_multiple_messages() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let topic_name = "test-kafka-multiple-msg";
    cleanup_kafka_topic(topic_name).await;

    let config = create_test_config(topic_name);
    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send 5 messages
    for i in 0..5 {
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: topic_name.to_string(),
            payload: format!("Message {}", i).into_bytes(),
            ..Default::default()
        };
        channel.send(msg).await.expect("Failed to send");
    }

    // Wait for messages to be available
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    // Receive all messages
    let messages = channel.receive(10).await.expect("Failed to receive");
    assert_eq!(messages.len(), 5);

    // Verify order (FIFO)
    for (i, msg) in messages.iter().enumerate() {
        assert_eq!(msg.payload, format!("Message {}", i).into_bytes());
    }

    cleanup_kafka_topic(topic_name).await;
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_try_receive_empty() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let topic_name = "test-kafka-try-receive-empty";
    cleanup_kafka_topic(topic_name).await;

    let config = create_test_config(topic_name);
    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Wait for consumer to be ready
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Try receive from empty topic
    let messages = channel.try_receive(1).await.expect("Failed to try_receive");
    assert_eq!(messages.len(), 0);

    cleanup_kafka_topic(topic_name).await;
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_try_receive_with_messages() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let topic_name = "test-kafka-try-receive";
    cleanup_kafka_topic(topic_name).await;

    let config = create_test_config(topic_name);
    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send message
    let msg = ChannelMessage {
        id: ulid::Ulid::new().to_string(),
        channel: topic_name.to_string(),
        payload: b"Quick message".to_vec(),
        ..Default::default()
    };
    channel.send(msg).await.expect("Failed to send");

    // Wait for message to be available
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Try receive
    let messages = channel.try_receive(1).await.expect("Failed to try_receive");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].payload, b"Quick message");

    cleanup_kafka_topic(topic_name).await;
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_publish_subscribe() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let channel_name = "test-kafka-pubsub";
    cleanup_kafka_topic(channel_name).await;

    let config = create_test_config(channel_name);
    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Subscribe first
    let mut stream = channel.subscribe(None).await.expect("Failed to subscribe");

    // Publish message (in background task)
    let channel_clone = channel.clone();
    let publish_handle = tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: channel_name.to_string(),
            payload: b"Pub/Sub Event".to_vec(),
            ..Default::default()
        };
        channel_clone.publish(msg).await.expect("Failed to publish");
    });

    // Receive from subscription with timeout
    let received = tokio::time::timeout(tokio::time::Duration::from_secs(3), stream.next())
        .await
        .expect("Timeout waiting for message");

    assert!(received.is_some());
    let msg = received.unwrap();
    assert_eq!(msg.payload, b"Pub/Sub Event");

    publish_handle.await.expect("Publish task failed");
    cleanup_kafka_topic(channel_name).await;
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_partition_key_routing() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let topic_name = "test-kafka-partition-key";
    cleanup_kafka_topic(topic_name).await;

    let config = create_test_config(topic_name);
    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send messages with partition keys
    for i in 0..3 {
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: topic_name.to_string(),
            payload: format!("Partitioned message {}", i).into_bytes(),
            partition_key: format!("key-{}", i % 2), // Alternate between 2 keys
            ..Default::default()
        };
        channel.send(msg).await.expect("Failed to send");
    }

    // Wait for messages to be available
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    // Receive messages
    let messages = channel.receive(5).await.expect("Failed to receive");
    assert_eq!(messages.len(), 3);

    // Verify partition keys are preserved
    assert!(!messages[0].partition_key.is_empty());
    assert!(!messages[1].partition_key.is_empty());
    assert!(!messages[2].partition_key.is_empty());

    cleanup_kafka_topic(topic_name).await;
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_consumer_group() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let topic_name = "test-kafka-consumer-group";
    cleanup_kafka_topic(topic_name).await;

    // Create channel with consumer group
    let mut config = create_test_config(topic_name);
    if let Some(channel_config::BackendConfig::Kafka(ref mut kafka_cfg)) = config.backend_config {
        kafka_cfg.consumer_group = "test-consumer-group".to_string();
    }

    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send messages
    for i in 0..3 {
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: topic_name.to_string(),
            payload: format!("Group message {}", i).into_bytes(),
            ..Default::default()
        };
        channel.send(msg).await.expect("Failed to send");
    }

    // Wait for messages to be available
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    // Receive with consumer group
    let messages = channel.receive(3).await.expect("Failed to receive");
    assert_eq!(messages.len(), 3);

    cleanup_kafka_topic(topic_name).await;
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_ack() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let topic_name = "test-kafka-ack";
    cleanup_kafka_topic(topic_name).await;

    let config = create_test_config(topic_name);
    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send message
    let msg = ChannelMessage {
        id: ulid::Ulid::new().to_string(),
        channel: topic_name.to_string(),
        payload: b"Ack me".to_vec(),
        ..Default::default()
    };
    let msg_id = channel.send(msg).await.expect("Failed to send");

    // Wait for message to be available
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Receive message
    let messages = channel.receive(1).await.expect("Failed to receive");
    assert_eq!(messages.len(), 1);

    // Ack the message (Kafka auto-commits offsets)
    channel.ack(&msg_id).await.expect("Failed to ack");

    cleanup_kafka_topic(topic_name).await;
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_get_stats() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let topic_name = "test-kafka-stats";
    cleanup_kafka_topic(topic_name).await;

    let config = create_test_config(topic_name);
    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create channel");

    // Send some messages
    for i in 0..5 {
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: topic_name.to_string(),
            payload: format!("Stats message {}", i).into_bytes(),
            ..Default::default()
        };
        channel.send(msg).await.expect("Failed to send");
    }

    // Get stats
    let stats = channel.get_stats().await.expect("Failed to get stats");
    assert_eq!(stats.messages_sent, 5);
    assert_eq!(stats.backend, ChannelBackend::ChannelBackendKafka as i32);

    cleanup_kafka_topic(topic_name).await;
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_close_channel() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let topic_name = "test-kafka-close";
    cleanup_kafka_topic(topic_name).await;

    let config = create_test_config(topic_name);
    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create channel");

    assert!(!channel.is_closed());

    // Close channel
    channel.close().await.expect("Failed to close");
    assert!(channel.is_closed());

    // Sending should fail
    let msg = ChannelMessage {
        id: ulid::Ulid::new().to_string(),
        channel: topic_name.to_string(),
        payload: b"Should fail".to_vec(),
        ..Default::default()
    };
    let result = channel.send(msg).await;
    assert!(matches!(result, Err(ChannelError::ChannelClosed(_))));

    cleanup_kafka_topic(topic_name).await;
}

#[tokio::test]
#[ignore] // Integration test - requires Kafka running
async fn test_kafka_message_persistence() {
    if !is_kafka_available().await {
        eprintln!("Skipping test: Kafka not available");
        return;
    }

    let topic_name = "test-kafka-persistence";
    cleanup_kafka_topic(topic_name).await;

    // Create channel and send message
    {
        let config = create_test_config(topic_name);
        let channel = KafkaChannel::new(config)
            .await
            .expect("Failed to create channel");

        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: topic_name.to_string(),
            payload: b"Persistent message".to_vec(),
            ..Default::default()
        };
        channel.send(msg).await.expect("Failed to send");
    }
    // Channel dropped here

    // Wait for message to be persisted
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Create new channel instance - should still see message
    let config = create_test_config(topic_name);
    let channel = KafkaChannel::new(config)
        .await
        .expect("Failed to create channel");

    let messages = channel.receive(1).await.expect("Failed to receive");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].payload, b"Persistent message");

    cleanup_kafka_topic(topic_name).await;
}
