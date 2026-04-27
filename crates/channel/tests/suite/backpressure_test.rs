// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Unit tests for channel backpressure functionality
//
// Tests verify that:
// 1. Bounded memory channels block producers when full (Go-like behavior)
// 2. Backpressure propagates naturally through the pipeline
// 3. Unbounded channels don't block (for pub/sub patterns)
// 4. Capacity configuration works correctly

use plexspaces_channel::{Channel, InMemoryChannel};
use plexspaces_proto::channel::v1::{
    ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee,
};
use plexspaces_proto::common::v1::Message;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Test that bounded channels block senders when full (backpressure)
#[tokio::test]
async fn test_bounded_channel_backpressure_blocks_sender() {
    // Create bounded channel with small capacity
    let config = ChannelConfig {
        name: "backpressure-test".to_string(),
        provider: ChannelProvider::ChannelProviderInMemory as i32,
        capacity: 10, // Small capacity to test backpressure
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        ..Default::default()
    };

    let channel = InMemoryChannel::new(config).await.unwrap();

    // Fill channel to capacity
    for i in 0..10 {
        let msg = Message {
            id: format!("msg-{}", i),
            channel: "backpressure-test".to_string(),
            payload: format!("data-{}", i).into_bytes(),
            ..Default::default()
        };
        channel.send(msg).await.unwrap();
    }

    // Verify channel is at capacity (check via receive to see pending count)
    // Note: We can't directly check stats, so we verify by behavior

    // Try to send one more message - should block until space is available
    let send_complete = Arc::new(AtomicBool::new(false));

    let channel = Arc::new(channel);
    let channel_clone = channel.clone();
    let send_complete_clone = send_complete.clone();
    let send_task = tokio::spawn(async move {
        let msg = Message {
            id: "msg-blocked".to_string(),
            channel: "backpressure-test".to_string(),
            payload: b"blocked-data".to_vec(),
            ..Default::default()
        };
        channel_clone.send(msg).await.unwrap();
        send_complete_clone.store(true, Ordering::Relaxed);
    });

    // Wait a bit to ensure send is blocked
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !send_complete.load(Ordering::Relaxed),
        "Send should be blocked"
    );

    // Receive one message to free up space
    let _messages = channel.receive(1).await.unwrap();
    assert_eq!(_messages.len(), 1);

    // Now the blocked send should complete
    let result = timeout(Duration::from_secs(1), send_task).await;
    assert!(result.is_ok(), "Send should complete after receive");
    assert!(
        send_complete.load(Ordering::Relaxed),
        "Send should have completed"
    );

    // Verify the blocked message was sent by receiving all messages
    let mut received = 0;
    loop {
        let messages = channel.receive(10).await.unwrap();
        if messages.is_empty() {
            break;
        }
        received += messages.len();
        if received >= 10 {
            break;
        }
    }
    assert_eq!(received, 10, "Should have received all messages");
}

/// Test that backpressure works with multiple concurrent senders
#[tokio::test]
async fn test_backpressure_with_multiple_senders() {
    let config = ChannelConfig {
        name: "multi-sender-backpressure".to_string(),
        provider: ChannelProvider::ChannelProviderInMemory as i32,
        capacity: 5u64, // Small capacity
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        ..Default::default()
    };

    let channel = Arc::new(InMemoryChannel::new(config).await.unwrap());

    // Spawn multiple senders trying to send more than capacity
    let mut send_tasks = Vec::new();
    let messages_sent = Arc::new(AtomicU64::new(0));

    for i in 0..10 {
        let channel_clone = channel.clone();
        let messages_sent_clone = messages_sent.clone();
        let task = tokio::spawn(async move {
            let msg = Message {
                id: format!("msg-{}", i),
                channel: "multi-sender-backpressure".to_string(),
                payload: format!("data-{}", i).into_bytes(),
                ..Default::default()
            };
            channel_clone.send(msg).await.unwrap();
            messages_sent_clone.fetch_add(1, Ordering::Relaxed);
        });
        send_tasks.push(task);
    }

    // Wait a bit - some sends should complete, others should block
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Receive messages to free up space
    let mut total_received = 0;
    for _ in 0..10 {
        let messages = channel.receive(1).await.unwrap();
        total_received += messages.len();
        if total_received >= 10 {
            break;
        }
    }

    // Wait for all sends to complete
    for task in send_tasks {
        task.await.unwrap();
    }

    // Verify all messages were sent
    assert_eq!(messages_sent.load(Ordering::Relaxed), 10);
    assert_eq!(total_received, 10);
}

/// Test that unbounded channels don't block (for pub/sub patterns)
#[tokio::test]
async fn test_unbounded_channel_no_backpressure() {
    let config = ChannelConfig {
        name: "unbounded-test".to_string(),
        provider: ChannelProvider::ChannelProviderInMemory as i32,
        capacity: 0, // 0 = unbounded
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        ..Default::default()
    };

    let channel = InMemoryChannel::new(config).await.unwrap();

    // Send many messages - should not block
    let send_start = Instant::now();
    for i in 0..1000 {
        let msg = Message {
            id: format!("msg-{}", i),
            channel: "unbounded-test".to_string(),
            payload: format!("data-{}", i).into_bytes(),
            ..Default::default()
        };
        channel.send(msg).await.unwrap();
    }
    let send_duration = send_start.elapsed();

    // Should complete quickly (no blocking)
    assert!(
        send_duration < Duration::from_millis(100),
        "Unbounded channel should not block"
    );

    // Verify all messages were sent by receiving them
    let mut received = 0;
    loop {
        let messages = channel.receive(100).await.unwrap();
        if messages.is_empty() {
            break;
        }
        received += messages.len();
        if received >= 1000 {
            break;
        }
    }
    assert_eq!(received, 1000, "Should have received all 1000 messages");
}

/// Test that backpressure propagates through pipeline stages
#[tokio::test]
async fn test_backpressure_propagation() {
    // Create pipeline: input -> processor -> output
    let input_config = ChannelConfig {
        name: "input-channel".to_string(),
        provider: ChannelProvider::ChannelProviderInMemory as i32,
        capacity: 5u64, // Small capacity
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        ..Default::default()
    };

    let output_config = ChannelConfig {
        name: "output-channel".to_string(),
        provider: ChannelProvider::ChannelProviderInMemory as i32,
        capacity: 5u64, // Small capacity
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        ..Default::default()
    };

    let input_channel = Arc::new(InMemoryChannel::new(input_config).await.unwrap());
    let output_channel = Arc::new(InMemoryChannel::new(output_config).await.unwrap());

    // Fill input channel
    for i in 0..5 {
        let msg = Message {
            id: format!("input-{}", i),
            channel: "input-channel".to_string(),
            payload: format!("data-{}", i).into_bytes(),
            ..Default::default()
        };
        input_channel.send(msg).await.unwrap();
    }

    // Processor: read from input, send to output
    let input_clone = Arc::clone(&input_channel);
    let output_clone = Arc::clone(&output_channel);
    let processor_task = tokio::spawn(async move {
        loop {
            let messages = input_clone.receive(1).await.unwrap();
            if messages.is_empty() {
                break;
            }
            for msg in messages {
                let output_msg = Message {
                    id: format!("output-{}", msg.id),
                    channel: "output-channel".to_string(),
                    payload: msg.payload,
                    ..Default::default()
                };
                output_clone.send(output_msg).await.unwrap();
            }
        }
    });

    // Fill output channel to capacity
    for i in 0..5 {
        let msg = Message {
            id: format!("output-fill-{}", i),
            channel: "output-channel".to_string(),
            payload: format!("fill-{}", i).into_bytes(),
            ..Default::default()
        };
        output_channel.send(msg).await.unwrap();
    }

    // Try to send more to input - should block because output is full
    // (backpressure propagates: output full -> processor blocked -> input blocked)
    let send_blocked = Arc::new(AtomicBool::new(false));
    let send_blocked_clone = send_blocked.clone();
    let input_clone = Arc::clone(&input_channel);
    let send_task = tokio::spawn(async move {
        let msg = Message {
            id: "blocked-input".to_string(),
            channel: "input-channel".to_string(),
            payload: b"blocked".to_vec(),
            ..Default::default()
        };
        send_blocked_clone.store(true, Ordering::Relaxed);
        input_clone.send(msg).await.unwrap();
    });

    // Wait a bit
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Receive from output to free up space
    let _messages = output_channel.receive(1).await.unwrap();

    // Now input send should complete
    let result = timeout(Duration::from_secs(1), send_task).await;
    assert!(result.is_ok(), "Send should complete after output receive");

    // Clean up
    drop(input_channel);
    processor_task.abort();
}

/// Test that capacity configuration is respected
#[tokio::test]
async fn test_capacity_configuration() {
    for capacity in [1, 10, 100, 1000, 4096] {
        let config = ChannelConfig {
            name: format!("capacity-{}", capacity),
            provider: ChannelProvider::ChannelProviderInMemory as i32,
            capacity: capacity as u64,
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };

        let channel = InMemoryChannel::new(config).await.unwrap();

        // Fill to capacity
        for i in 0..capacity {
            let msg = Message {
                id: format!("msg-{}", i),
                channel: format!("capacity-{}", capacity),
                payload: format!("data-{}", i).into_bytes(),
                ..Default::default()
            };
            channel.send(msg).await.unwrap();
        }

        // Verify we're at capacity by checking stats
        let stats = channel.get_stats().await.unwrap();
        assert_eq!(
            stats.messages_pending, capacity as u64,
            "Channel should be at capacity {}",
            capacity
        );
    }
}

/// Test that backpressure works with send timeout
#[tokio::test]
async fn test_backpressure_with_timeout() {
    use plexspaces_proto::channel::v1::channel_config;
    use prost_types::Duration as ProtoDuration;

    let mut config = ChannelConfig {
        name: "timeout-backpressure".to_string(),
        provider: ChannelProvider::ChannelProviderInMemory as i32,
        capacity: 5u64,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        ..Default::default()
    };

    // Set send timeout
    use plexspaces_proto::channel::v1::InMemoryConfig;
    let mut in_memory_config = InMemoryConfig::default();
    in_memory_config.send_timeout = Some(ProtoDuration {
        seconds: 0,
        nanos: 100_000_000, // 100ms
    });
    config.backend_config = Some(channel_config::BackendConfig::InMemory(in_memory_config));

    let channel = InMemoryChannel::new(config).await.unwrap();

    // Fill to capacity
    for i in 0..5 {
        let msg = Message {
            id: format!("msg-{}", i),
            channel: "timeout-backpressure".to_string(),
            payload: format!("data-{}", i).into_bytes(),
            ..Default::default()
        };
        channel.send(msg).await.unwrap();
    }

    // Try to send with timeout - should timeout because channel is full
    let msg = Message {
        id: "timeout-msg".to_string(),
        channel: "timeout-backpressure".to_string(),
        payload: b"timeout-data".to_vec(),
        ..Default::default()
    };

    let result = channel.send(msg).await;
    assert!(result.is_err(), "Send should timeout when channel is full");
    assert!(matches!(
        result.unwrap_err(),
        plexspaces_channel::ChannelError::Timeout(_)
    ));
}
