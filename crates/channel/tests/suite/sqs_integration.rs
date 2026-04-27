// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! SQS channel backend integration tests.
//!
//! ## Purpose
//! Comprehensive test suite for SQS-based channel with 95%+ coverage.
//! Tests verify message sending, receiving, ACK/NACK, DLQ, and all edge cases.
//!
//! When LocalStack is not running, tests skip quickly using the same TCP endpoint
//! probe and AWS local env setup as DynamoDB integration tests (`plexspaces_common::test_helpers`).
//!
//! ## Test Coverage
//! - Message send/receive
//! - ACK/NACK operations
//! - DLQ handling
//! - Visibility timeout
//! - Concurrent operations
//! - Error handling
//! - Observability (metrics, tracing)

#[cfg(feature = "sqs-backend")]
mod tests {
    use plexspaces_channel::{Channel, SQSChannel};
    use plexspaces_common::test_helpers::{
        get_sqs_endpoint, setup_aws_local_env, sqs_simulator_available,
    };
    use plexspaces_proto::channel::v1::{ChannelConfig, DeliveryGuarantee, OrderingGuarantee};
    use plexspaces_proto::common::v1::Message;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    /// Same runtime pattern as DynamoDB integration tests: short TCP check to
    /// [`get_sqs_endpoint`], cached process-wide in `plexspaces_common::test_helpers`.
    async fn check_sqs_available() -> bool {
        sqs_simulator_available().await
    }

    /// Create SQS channel for testing (LocalStack or custom endpoint from env).
    async fn create_channel(channel_name: &str) -> SQSChannel {
        setup_aws_local_env();
        let endpoint = get_sqs_endpoint();
        std::env::set_var("SQS_ENDPOINT_URL", &endpoint);

        let region = std::env::var("AWS_REGION")
            .or_else(|_| std::env::var("PLEXSPACES_AWS_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());

        std::env::set_var("AWS_REGION", &region);
        std::env::set_var("PLEXSPACES_SQS_QUEUE_PREFIX", "plexspaces-test-");

        let config = ChannelConfig {
            name: channel_name.to_string(),
            provider: 6, // ChannelProvider::ChannelProviderSqs - TODO: use enum once proto regenerated
            capacity: 0, // Unbounded for SQS
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            // backend_config will be None - SQSChannel reads from env vars
            // TODO: Use channel_config::BackendConfig::SqsConfig once proto is regenerated
            backend_config: None,
            ..Default::default()
        };

        SQSChannel::new(config)
            .await
            .expect("Failed to create SQS channel")
    }

    #[tokio::test]
    async fn test_sqs_send_receive() {
        if !check_sqs_available().await {
            eprintln!("⚠️  WARNING: SQS simulator (LocalStack) is not running. Skipping SQS test.");
            eprintln!(
                "To run SQS tests, start LocalStack: docker run -p 4566:4566 localstack/localstack"
            );
            return;
        }
        let channel = create_channel("test-send-receive").await;

        // Send message
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            channel: "test-send-receive".to_string(),
            payload: b"test payload".to_vec(),
            ..Default::default()
        };

        let msg_id = channel.send(msg.clone()).await.unwrap();
        assert!(!msg_id.is_empty());

        // Receive message
        let received = channel.receive(1).await.unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].payload, b"test payload");

        // ACK message
        channel.ack(&received[0].id).await.unwrap();
    }

    #[tokio::test]
    async fn test_sqs_send_multiple_receive() {
        if !check_sqs_available().await {
            eprintln!("⚠️  WARNING: SQS simulator (LocalStack) is not running. Skipping SQS test.");
            eprintln!(
                "To run SQS tests, start LocalStack: docker run -p 4566:4566 localstack/localstack"
            );
            return;
        }
        let channel = create_channel("test-multiple").await;

        // Send multiple messages
        for i in 0..5 {
            let msg = Message {
                id: ulid::Ulid::new().to_string(),
                channel: "test-multiple".to_string(),
                payload: format!("message-{}", i).into_bytes(),
                ..Default::default()
            };
            channel.send(msg).await.unwrap();
        }

        // Receive all messages
        let received = channel.receive(10).await.unwrap();
        assert_eq!(received.len(), 5);

        // ACK all
        for msg in &received {
            channel.ack(&msg.id).await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_sqs_ack() {
        if !check_sqs_available().await {
            eprintln!("⚠️  WARNING: SQS simulator (LocalStack) is not running. Skipping SQS test.");
            eprintln!(
                "To run SQS tests, start LocalStack: docker run -p 4566:4566 localstack/localstack"
            );
            return;
        }
        let channel = create_channel("test-ack").await;

        // Send and receive
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            channel: "test-ack".to_string(),
            payload: b"test".to_vec(),
            ..Default::default()
        };
        channel.send(msg).await.unwrap();

        let received = channel.receive(1).await.unwrap();
        assert_eq!(received.len(), 1);

        // ACK message
        channel.ack(&received[0].id).await.unwrap();

        // Message should not be redelivered
        sleep(Duration::from_secs(2)).await;
        let received_again = channel.try_receive(1).await.unwrap();
        assert_eq!(received_again.len(), 0);
    }

    #[tokio::test]
    async fn test_sqs_nack_requeue() {
        if !check_sqs_available().await {
            eprintln!("⚠️  WARNING: SQS simulator (LocalStack) is not running. Skipping SQS test.");
            eprintln!(
                "To run SQS tests, start LocalStack: docker run -p 4566:4566 localstack/localstack"
            );
            return;
        }
        // Use short visibility timeout so redelivery check doesn't take 35s
        std::env::set_var("PLEXSPACES_SQS_VISIBILITY_TIMEOUT", "5");
        let channel = create_channel("test-nack-requeue").await;

        // Send message
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            channel: "test-nack-requeue".to_string(),
            payload: b"test".to_vec(),
            ..Default::default()
        };
        channel.send(msg).await.unwrap();

        // Receive
        let received = channel.receive(1).await.unwrap();
        assert_eq!(received.len(), 1);

        // NACK with requeue
        channel.nack(&received[0].id, true).await.unwrap();

        // Message should be redelivered after visibility timeout (5s + 2s buffer)
        sleep(Duration::from_secs(7)).await;
        let redelivered = channel.receive(1).await.unwrap();
        assert_eq!(redelivered.len(), 1);
        assert_eq!(redelivered[0].payload, b"test");

        // ACK to clean up
        channel.ack(&redelivered[0].id).await.unwrap();
        std::env::remove_var("PLEXSPACES_SQS_VISIBILITY_TIMEOUT");
    }

    #[tokio::test]
    async fn test_sqs_nack_dlq() {
        if !check_sqs_available().await {
            eprintln!("⚠️  WARNING: SQS simulator (LocalStack) is not running. Skipping SQS test.");
            eprintln!(
                "To run SQS tests, start LocalStack: docker run -p 4566:4566 localstack/localstack"
            );
            return;
        }
        // Use short visibility timeout so each NACK cycle completes in seconds not 35s
        std::env::set_var("PLEXSPACES_SQS_VISIBILITY_TIMEOUT", "5");
        let channel = create_channel("test-nack-dlq").await;

        // Send message
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            channel: "test-nack-dlq".to_string(),
            payload: b"test".to_vec(),
            ..Default::default()
        };
        channel.send(msg).await.unwrap();

        // Receive and NACK multiple times to trigger DLQ
        for _ in 0..3 {
            let received = channel.receive(1).await.unwrap();
            assert_eq!(received.len(), 1);
            channel.nack(&received[0].id, false).await.unwrap();
            sleep(Duration::from_secs(7)).await; // Wait for visibility timeout (5s + 2s buffer)
        }

        // After max receive count, message should be in DLQ
        // (This test may need adjustment based on DLQ implementation)
        sleep(Duration::from_secs(2)).await;
        let received = channel.try_receive(1).await.unwrap();
        // Message should not be in main queue (in DLQ)
        assert_eq!(received.len(), 0);
        std::env::remove_var("PLEXSPACES_SQS_VISIBILITY_TIMEOUT");
    }

    #[tokio::test]
    async fn test_sqs_visibility_timeout() {
        if !check_sqs_available().await {
            eprintln!("⚠️  WARNING: SQS simulator (LocalStack) is not running. Skipping SQS test.");
            eprintln!(
                "To run SQS tests, start LocalStack: docker run -p 4566:4566 localstack/localstack"
            );
            return;
        }
        // Use short visibility timeout so the test completes in seconds not 35s
        std::env::set_var("PLEXSPACES_SQS_VISIBILITY_TIMEOUT", "5");
        let channel = create_channel("test-visibility").await;

        // Send message
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            channel: "test-visibility".to_string(),
            payload: b"test".to_vec(),
            ..Default::default()
        };
        channel.send(msg).await.unwrap();

        // Receive message (becomes invisible)
        let received = channel.receive(1).await.unwrap();
        assert_eq!(received.len(), 1);

        // Don't ACK - message should become visible again after timeout
        // Try to receive again immediately (should not get same message)
        let received_immediate = channel.try_receive(1).await.unwrap();
        assert_eq!(received_immediate.len(), 0);

        // After visibility timeout (5s + 2s buffer), message should be redelivered
        sleep(Duration::from_secs(7)).await;
        let redelivered = channel.receive(1).await.unwrap();
        assert_eq!(redelivered.len(), 1);

        // ACK to clean up
        channel.ack(&redelivered[0].id).await.unwrap();
        std::env::remove_var("PLEXSPACES_SQS_VISIBILITY_TIMEOUT");
    }

    #[tokio::test]
    async fn test_sqs_try_receive() {
        if !check_sqs_available().await {
            eprintln!("⚠️  WARNING: SQS simulator (LocalStack) is not running. Skipping SQS test.");
            eprintln!(
                "To run SQS tests, start LocalStack: docker run -p 4566:4566 localstack/localstack"
            );
            return;
        }
        let channel = create_channel("test-try-receive").await;

        // Try receive when empty
        let received = channel.try_receive(1).await.unwrap();
        assert_eq!(received.len(), 0);

        // Send message
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            channel: "test-try-receive".to_string(),
            payload: b"test".to_vec(),
            ..Default::default()
        };
        channel.send(msg).await.unwrap();

        // Try receive should get message
        let received = channel.try_receive(1).await.unwrap();
        assert_eq!(received.len(), 1);

        // ACK
        channel.ack(&received[0].id).await.unwrap();
    }

    #[tokio::test]
    async fn test_sqs_concurrent_send_receive() {
        if !check_sqs_available().await {
            eprintln!("⚠️  WARNING: SQS simulator (LocalStack) is not running. Skipping SQS test.");
            eprintln!(
                "To run SQS tests, start LocalStack: docker run -p 4566:4566 localstack/localstack"
            );
            return;
        }
        let channel = Arc::new(create_channel("test-concurrent").await);
        let mut handles = vec![];

        // Spawn multiple senders
        for i in 0..10 {
            let channel_clone = channel.clone();
            let handle = tokio::spawn(async move {
                for j in 0..5 {
                    let msg = Message {
                        id: ulid::Ulid::new().to_string(),
                        channel: "test-concurrent".to_string(),
                        payload: format!("sender-{}-msg-{}", i, j).into_bytes(),
                        ..Default::default()
                    };
                    channel_clone.send(msg).await.unwrap();
                }
            });
            handles.push(handle);
        }

        // Wait for all sends
        for handle in handles {
            handle.await.unwrap();
        }

        // Receive all messages
        let mut received_count = 0;
        while received_count < 50 {
            let received = channel.receive(10).await.unwrap();
            received_count += received.len() as u32;
            for msg in &received {
                channel.ack(&msg.id).await.unwrap();
            }
        }

        assert_eq!(received_count, 50);
    }

    #[tokio::test]
    async fn test_sqs_get_stats() {
        if !check_sqs_available().await {
            eprintln!("⚠️  WARNING: SQS simulator (LocalStack) is not running. Skipping SQS test.");
            eprintln!(
                "To run SQS tests, start LocalStack: docker run -p 4566:4566 localstack/localstack"
            );
            return;
        }
        let channel = create_channel("test-stats").await;

        // Send some messages
        for i in 0..5 {
            let msg = Message {
                id: ulid::Ulid::new().to_string(),
                channel: "test-stats".to_string(),
                payload: format!("msg-{}", i).into_bytes(),
                ..Default::default()
            };
            channel.send(msg).await.unwrap();
        }

        // Get stats
        let stats = channel.get_stats().await.unwrap();
        assert!(stats.messages_sent >= 5);
    }

    #[tokio::test]
    async fn test_sqs_close() {
        if !check_sqs_available().await {
            eprintln!("⚠️  WARNING: SQS simulator (LocalStack) is not running. Skipping SQS test.");
            eprintln!(
                "To run SQS tests, start LocalStack: docker run -p 4566:4566 localstack/localstack"
            );
            return;
        }
        let channel = create_channel("test-close").await;

        // Send message before closing
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            channel: "test-close".to_string(),
            payload: b"test".to_vec(),
            ..Default::default()
        };
        channel.send(msg).await.unwrap();

        // Close channel
        channel.close().await.unwrap();
        assert!(channel.is_closed());

        // Send should fail
        let msg2 = Message {
            id: ulid::Ulid::new().to_string(),
            channel: "test-close".to_string(),
            payload: b"test2".to_vec(),
            ..Default::default()
        };
        let result = channel.send(msg2).await;
        assert!(result.is_err());

        // But receive should still work to drain
        let received = channel.receive(1).await.unwrap();
        assert_eq!(received.len(), 1);
        channel.ack(&received[0].id).await.unwrap();
    }
}
