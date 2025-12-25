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
use plexspaces_channel::{create_channel, Channel};
use plexspaces_proto::channel::v1::{
    ChannelBackend, ChannelConfig, ChannelMessage, UdpConfig,
};
use std::time::Duration;
use tokio::time::timeout;
use futures::StreamExt;

    fn create_udp_config(cluster_name: &str, port: u32) -> UdpConfig {
        UdpConfig {
            multicast_address: "239.255.0.1".to_string(),
            multicast_port: port,
            bind_address: "0.0.0.0".to_string(),
            ttl: 1,
            max_message_size: 1400,
            unicast_mode: false,
            cluster_name: cluster_name.to_string(),
            interface_name: String::new(),
        }
    }

    async fn create_udp_channel(name: &str, cluster_name: &str, port: u32) -> Box<dyn Channel> {
        let udp_config = create_udp_config(cluster_name, port);
        let channel_config = ChannelConfig {
            name: name.to_string(),
            backend: ChannelBackend::ChannelBackendUdp as i32,
            capacity: 0,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
            ordering: plexspaces_proto::channel::v1::OrderingGuarantee::OrderingGuaranteeNone as i32,
            backend_config: Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Udp(udp_config)),
            ..Default::default()
        };

        create_channel(channel_config).await.unwrap()
    }

    #[tokio::test]
    async fn test_udp_channel_creation() {
        let channel = create_udp_channel("test-udp-1", "test-cluster", 10001).await;
        
        assert_eq!(channel.get_config().name, "test-udp-1");
        assert!(!channel.is_closed());
    }

    #[tokio::test]
    async fn test_udp_channel_requires_cluster_name() {
        let udp_config = UdpConfig {
            multicast_address: "239.255.0.1".to_string(),
            multicast_port: 9999,
            bind_address: "0.0.0.0".to_string(),
            ttl: 1,
            max_message_size: 1400,
            unicast_mode: false,
            cluster_name: String::new(), // Empty cluster name
            interface_name: String::new(),
        };

        let channel_config = ChannelConfig {
            name: "test-udp".to_string(),
            backend: ChannelBackend::ChannelBackendUdp as i32,
            capacity: 0,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
            ordering: plexspaces_proto::channel::v1::OrderingGuarantee::OrderingGuaranteeNone as i32,
            backend_config: Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Udp(udp_config)),
            ..Default::default()
        };

        let result = create_channel(channel_config).await;
        assert!(result.is_err());
        // Check error message without requiring Debug on Channel
        let error_str = match result {
            Err(e) => e.to_string(),
            Ok(_) => String::new(),
        };
        assert!(error_str.contains("cluster_name") || error_str.contains("cluster"));
    }

    #[tokio::test]
    async fn test_udp_send_receive() {
        let channel1 = create_udp_channel("test-udp-2", "test-cluster-2", 10002).await;
        let channel2 = create_udp_channel("test-udp-2", "test-cluster-2", 10002).await;

        // Send message from channel1
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: "test-udp-2".to_string(),
            payload: b"test message".to_vec(),
            ..Default::default()
        };

        let msg_id = channel1.send(msg.clone()).await.unwrap();
        assert_eq!(msg_id, msg.id);

        // Receive from channel2 (multicast pub/sub)
        let received = timeout(Duration::from_secs(2), channel2.receive(1)).await;
        if let Ok(Ok(messages)) = received {
            if let Some(received_msg) = messages.first() {
                assert_eq!(received_msg.payload, b"test message");
            }
        }
    }

    #[tokio::test]
    async fn test_udp_publish_subscribe() {
        let publisher = create_udp_channel("test-udp-3", "test-cluster-3", 10003).await;
        let subscriber = create_udp_channel("test-udp-3", "test-cluster-3", 10003).await;

        // Subscribe
        let mut stream = subscriber.subscribe(None).await.unwrap();

        // Publish message
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: "test-udp-3".to_string(),
            payload: b"pub/sub test".to_vec(),
            ..Default::default()
        };

        let subscriber_count = publisher.publish(msg).await.unwrap();
        // UDP returns best-effort count (1)
        assert!(subscriber_count >= 0);

        // Receive from subscription
        let received = timeout(Duration::from_secs(2), stream.next()).await;
        if let Ok(Some(received_msg)) = received {
            assert_eq!(received_msg.payload, b"pub/sub test");
        }
    }

    #[tokio::test]
    async fn test_udp_ack_nack_noop() {
        // UDP channels don't support ACK/NACK (best-effort delivery)
        let channel = create_udp_channel("test-udp-4", "test-cluster-4", 10004).await;

        // ACK should be a no-op
        let result = channel.ack("test-message-id").await;
        assert!(result.is_ok());

        // NACK should be a no-op
        let result = channel.nack("test-message-id", true).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_udp_channel_close() {
        let channel = create_udp_channel("test-udp-5", "test-cluster-5", 10005).await;

        assert!(!channel.is_closed());
        
        channel.close().await.unwrap();
        
        assert!(channel.is_closed());
    }

    #[tokio::test]
    async fn test_udp_channel_stats() {
        let channel = create_udp_channel("test-udp-6", "test-cluster-6", 10006).await;

        // Send some messages
        for i in 0..5 {
            let msg = ChannelMessage {
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
        assert_eq!(stats.backend, ChannelBackend::ChannelBackendUdp as i32);
        assert!(stats.messages_sent >= 5);
        
        // Check backend stats include cluster_name
        assert!(stats.backend_stats.contains_key("cluster_name"));
        assert_eq!(stats.backend_stats.get("cluster_name").unwrap(), "test-cluster-6");
    }

    #[tokio::test]
    async fn test_udp_invalid_multicast_address() {
        let udp_config = UdpConfig {
            multicast_address: "192.168.1.1".to_string(), // Not a multicast address
            multicast_port: 9999,
            bind_address: "0.0.0.0".to_string(),
            ttl: 1,
            max_message_size: 1400,
            unicast_mode: false,
            cluster_name: "test-cluster".to_string(),
            interface_name: String::new(),
        };

        let channel_config = ChannelConfig {
            name: "test-udp".to_string(),
            backend: ChannelBackend::ChannelBackendUdp as i32,
            capacity: 0,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
            ordering: plexspaces_proto::channel::v1::OrderingGuarantee::OrderingGuaranteeNone as i32,
            backend_config: Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Udp(udp_config)),
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
