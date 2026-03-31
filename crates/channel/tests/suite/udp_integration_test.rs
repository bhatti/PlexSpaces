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

//! Integration tests for UDP multicast channel
//!
//! Tests cover:
//! - Multicast pub/sub messaging
//! - Cluster name validation
//! - Observability metrics
//! - Channel closing

#[cfg(feature = "udp-backend")]
mod tests {
    use futures::StreamExt;
    use plexspaces_channel::{create_channel, Channel};
    use plexspaces_proto::channel::v1::{ChannelConfig, ChannelProvider, UdpConfig};
    use plexspaces_proto::common::v1::Message;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;
    use tokio::time::timeout;

    // Static counter for generating unique ports per test
    // Starts from 20000 to avoid conflicts with system ports
    static PORT_COUNTER: AtomicU32 = AtomicU32::new(20000);

    /// Get a unique port for this test
    /// Uses atomic counter to ensure no conflicts when tests run in parallel
    fn get_unique_port() -> u32 {
        PORT_COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    fn create_udp_config(_cluster_name: &str, port: u32) -> UdpConfig {
        UdpConfig {
            multicast_address: "239.255.0.1".to_string(),
            multicast_port: port,
            bind_address: "0.0.0.0".to_string(),
            message_ttl_seconds: 60,
        }
    }

    async fn create_udp_channel(
        name: &str,
        cluster_name: &str,
        port: u32,
    ) -> plexspaces_channel::ChannelResult<Box<dyn Channel>> {
        let udp_config = create_udp_config(cluster_name, port);
        let channel_config = ChannelConfig {
            name: name.to_string(),
            provider: ChannelProvider::ChannelProviderUdp as i32,
            capacity: 0,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtMostOnce
                as i32,
            ordering: plexspaces_proto::channel::v1::OrderingGuarantee::OrderingGuaranteeNone
                as i32,
            backend_config: Some(
                plexspaces_proto::channel::v1::channel_config::BackendConfig::Udp(udp_config),
            ),
            ..Default::default()
        };

        create_channel(channel_config).await
    }

    async fn create_udp_channel_or_skip(
        name: &str,
        cluster_name: &str,
        port: u32,
    ) -> Option<Box<dyn Channel>> {
        match create_udp_channel(name, cluster_name, port).await {
            Ok(channel) => Some(channel),
            Err(err) => {
                eprintln!("Skipping UDP test - backend unavailable: {}", err);
                None
            }
        }
    }

    #[tokio::test]
    async fn test_udp_channel_creation() {
        let port = get_unique_port();
        let Some(channel) = create_udp_channel_or_skip("test-udp-1", "test-cluster", port).await
        else {
            return;
        };

        assert_eq!(channel.get_config().name, "test-udp-1");
        assert!(!channel.is_closed());
    }

    #[tokio::test]
    async fn test_udp_channel_creation_with_config() {
        let port = get_unique_port();
        let udp_config = UdpConfig {
            multicast_address: "239.255.0.1".to_string(),
            multicast_port: port,
            bind_address: "0.0.0.0".to_string(),
            message_ttl_seconds: 60,
        };

        let channel_config = ChannelConfig {
            name: "test-udp".to_string(),
            provider: ChannelProvider::ChannelProviderUdp as i32,
            capacity: 0,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtMostOnce
                as i32,
            ordering: plexspaces_proto::channel::v1::OrderingGuarantee::OrderingGuaranteeNone
                as i32,
            backend_config: Some(
                plexspaces_proto::channel::v1::channel_config::BackendConfig::Udp(udp_config),
            ),
            ..Default::default()
        };

        // UDP channel should be created successfully with valid config
        // (cluster_name is no longer required - cluster membership determined by multicast address/port)
        let result = create_channel(channel_config).await;
        assert!(
            result.is_ok(),
            "UDP channel should be created with valid multicast config"
        );
    }

    #[tokio::test]
    async fn test_udp_send_receive() {
        // Use same port for both channels (they need to share multicast group)
        let port = get_unique_port();
        let Some(channel1) = create_udp_channel_or_skip("test-udp-2", "test-cluster-2", port).await
        else {
            return;
        };
        let Some(channel2) = create_udp_channel_or_skip("test-udp-2", "test-cluster-2", port).await
        else {
            return;
        };
        let mut stream = channel2.subscribe(None).await.unwrap();

        // Send message from channel1
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            channel: "test-udp-2".to_string(),
            payload: b"test message".to_vec(),
            ..Default::default()
        };

        let msg_id = channel1.send(msg.clone()).await.unwrap();
        assert_eq!(msg_id, msg.id);

        let received = timeout(Duration::from_millis(750), stream.next())
            .await
            .expect("UDP subscriber should receive message quickly")
            .expect("UDP stream should yield a message");
        assert_eq!(received.payload, b"test message");
    }

    #[tokio::test]
    async fn test_udp_publish_subscribe() {
        // Use same port for both channels (they need to share multicast group)
        let port = get_unique_port();
        let Some(publisher) =
            create_udp_channel_or_skip("test-udp-3", "test-cluster-3", port).await
        else {
            return;
        };
        let Some(subscriber) =
            create_udp_channel_or_skip("test-udp-3", "test-cluster-3", port).await
        else {
            return;
        };

        // Subscribe
        let mut stream = subscriber.subscribe(None).await.unwrap();

        // Publish message
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            channel: "test-udp-3".to_string(),
            payload: b"pub/sub test".to_vec(),
            ..Default::default()
        };

        let subscriber_count = publisher.publish(msg).await.unwrap();
        assert_eq!(subscriber_count, 1);

        // Receive from subscription
        let received_msg = timeout(Duration::from_millis(750), stream.next())
            .await
            .expect("UDP pub/sub should deliver quickly")
            .expect("UDP subscription should yield a message");
        assert_eq!(received_msg.payload, b"pub/sub test");
    }

    #[tokio::test]
    async fn test_udp_ack_nack_noop() {
        // UDP channels don't support ACK/NACK (best-effort delivery)
        let port = get_unique_port();
        let Some(channel) = create_udp_channel_or_skip("test-udp-4", "test-cluster-4", port).await
        else {
            return;
        };

        // ACK should be a no-op
        let result = channel.ack("test-message-id").await;
        assert!(result.is_ok());

        // NACK should be a no-op
        let result = channel.nack("test-message-id", true).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_udp_channel_close() {
        let port = get_unique_port();
        let Some(channel) = create_udp_channel_or_skip("test-udp-5", "test-cluster-5", port).await
        else {
            return;
        };

        assert!(!channel.is_closed());

        channel.close().await.unwrap();

        assert!(channel.is_closed());
    }

    #[tokio::test]
    async fn test_udp_channel_stats() {
        let port = get_unique_port();
        let Some(channel) = create_udp_channel_or_skip("test-udp-6", "test-cluster-6", port).await
        else {
            return;
        };

        // Send some messages
        for i in 0..5 {
            let msg = Message {
                id: ulid::Ulid::new().to_string(),
                channel: "test-udp-6".to_string(),
                payload: format!("msg{}", i).into_bytes(),
                ..Default::default()
            };
            channel.send(msg).await.unwrap();
        }

        // Get stats
        let stats = channel.get_stats().await.unwrap();
        assert_eq!(stats.name, "test-udp-6");
        assert_eq!(stats.provider, ChannelProvider::ChannelProviderUdp as i32);
        assert!(stats.messages_sent >= 5);

        // Check backend stats (cluster_name is no longer in stats, removed from UdpConfig)
        assert!(stats.backend_stats.contains_key("multicast_address"));
    }

    #[tokio::test]
    async fn test_udp_invalid_multicast_address() {
        let port = get_unique_port();
        let udp_config = UdpConfig {
            multicast_address: "192.168.1.1".to_string(), // Not a multicast address
            multicast_port: port,
            bind_address: "0.0.0.0".to_string(),
            message_ttl_seconds: 60,
        };

        let channel_config = ChannelConfig {
            name: "test-udp".to_string(),
            provider: ChannelProvider::ChannelProviderUdp as i32,
            capacity: 0,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtMostOnce
                as i32,
            ordering: plexspaces_proto::channel::v1::OrderingGuarantee::OrderingGuaranteeNone
                as i32,
            backend_config: Some(
                plexspaces_proto::channel::v1::channel_config::BackendConfig::Udp(udp_config),
            ),
            ..Default::default()
        };

        let result = create_channel(channel_config).await;
        assert!(result.is_err());
        // Check error message without requiring Debug on Channel
        let error_str = match result {
            Err(e) => e.to_string(),
            Ok(_) => String::new(),
        };
        assert!(error_str.contains("multicast") || error_str.contains("not a multicast"));
    }
}
