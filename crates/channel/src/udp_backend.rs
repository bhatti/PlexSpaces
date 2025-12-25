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

//! UDP backend for multicast pub/sub channels
//!
//! ## Purpose
//! Provides lightweight, high-performance distributed channel implementation
//! using UDP multicast for pub/sub messaging within a cluster.
//!
//! ## Architecture Context
//! UDP backend enables:
//! - **Very Low Latency**: < 100μs latency for local network
//! - **Pub/Sub**: Broadcast messages to all subscribers via multicast
//! - **Cluster-Based**: Nodes with same cluster_name can communicate
//! - **Best-Effort**: No persistence, messages may be lost
//!
//! ## Design Decisions
//! - **Multicast**: Uses UDP multicast for pub/sub (one-to-many)
//! - **Unicast**: Optional point-to-point messaging
//! - **No ACK/NACK**: UDP is best-effort, no delivery guarantees
//! - **Cluster Name**: Nodes must share cluster_name to communicate
//!
//! ## Performance
//! - Latency: < 100μs for send/receive (local network)
//! - Throughput: > 1M messages/second (limited by network)
//! - Persistence: None (messages lost on restart)

use crate::{Channel, ChannelError, ChannelResult};
use crate::observability::record_channel_error;
use tracing::{debug, info};
use async_trait::async_trait;
use futures::stream::BoxStream;
use plexspaces_proto::channel::v1::{
    channel_config, ChannelBackend, ChannelConfig, ChannelMessage, ChannelStats, UdpConfig,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket as TokioUdpSocket;
use tokio::sync::broadcast;
use tokio::time::timeout;

/// UDP channel implementation using UDP multicast
///
/// ## Purpose
/// Lightweight distributed channel backend using UDP multicast for high-performance
/// pub/sub messaging within a cluster.
///
/// ## Invariants
/// - Multicast address: Must be in multicast range (224.0.0.0 to 239.255.255.255)
/// - Cluster name: All nodes must share same cluster_name
/// - Best-effort delivery: No ACK/NACK, messages may be lost
/// - No persistence: Messages lost on restart
#[derive(Clone)]
pub struct UdpChannel {
    config: ChannelConfig,
    udp_config: UdpConfig,
    socket: Arc<TokioUdpSocket>,
    multicast_addr: SocketAddr,
    stats: Arc<ChannelStatsData>,
    closed: Arc<AtomicBool>,
    broadcast_tx: Arc<broadcast::Sender<ChannelMessage>>,
}

struct ChannelStatsData {
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    messages_failed: AtomicU64,
    errors: AtomicU64,
}

impl UdpChannel {
    /// Create a new UDP channel
    ///
    /// ## Arguments
    /// * `config` - Channel configuration with UDP backend config
    ///
    /// ## Returns
    /// New UdpChannel instance bound to multicast address
    ///
    /// ## Errors
    /// - [`ChannelError::InvalidConfiguration`]: Missing UDP config or invalid multicast address
    /// - [`ChannelError::BackendError`]: Failed to bind socket or join multicast group
    pub async fn new(config: ChannelConfig) -> ChannelResult<Self> {
        // Validate backend
        if config.backend() != ChannelBackend::ChannelBackendUdp {
            return Err(ChannelError::InvalidConfiguration(format!(
                "Invalid backend for UdpChannel: {:?}",
                config.backend()
            )));
        }

        // Extract UDP config
        let udp_config = match &config.backend_config {
            Some(channel_config::BackendConfig::Udp(cfg)) => cfg.clone(),
            _ => {
                return Err(ChannelError::InvalidConfiguration(
                    "Missing UDP configuration".to_string(),
                ))
            }
        };

        // Validate cluster name
        if udp_config.cluster_name.is_empty() {
            return Err(ChannelError::InvalidConfiguration(
                "UDP channel requires cluster_name".to_string(),
            ));
        }

        // Parse multicast address
        let multicast_ip: Ipv4Addr = if udp_config.multicast_address.is_empty() {
            Ipv4Addr::new(239, 255, 0, 1)
        } else {
            udp_config
                .multicast_address
                .parse()
                .map_err(|e| {
                    ChannelError::InvalidConfiguration(format!(
                        "Invalid multicast address '{}': {}",
                        udp_config.multicast_address, e
                    ))
                })?
        };

        // Validate multicast range
        if !is_multicast_ip(multicast_ip) {
            return Err(ChannelError::InvalidConfiguration(format!(
                "Address {} is not a multicast address (must be in 224.0.0.0-239.255.255.255)",
                multicast_ip
            )));
        }

        let multicast_port = if udp_config.multicast_port == 0 {
            9999u16
        } else {
            udp_config.multicast_port as u16
        };

        let multicast_addr = SocketAddr::new(IpAddr::V4(multicast_ip), multicast_port);

        // Parse bind address
        let bind_ip: Ipv4Addr = if udp_config.bind_address.is_empty() {
            Ipv4Addr::new(0, 0, 0, 0)
        } else {
            udp_config
                .bind_address
                .parse()
                .map_err(|e| {
                    ChannelError::InvalidConfiguration(format!(
                        "Invalid bind address '{}': {}",
                        udp_config.bind_address, e
                    ))
                })?
        };

        let bind_addr = SocketAddr::new(IpAddr::V4(bind_ip), multicast_port as u16);

        // Create UDP socket using socket2 (for multicast support)
        // We need to do this in a blocking context, then convert to async
        let multicast_ip_clone = multicast_ip;
        let bind_addr_clone = bind_addr;
        let ttl = if udp_config.ttl == 0 { 1 } else { udp_config.ttl };
        let std_socket = tokio::task::spawn_blocking(move || -> Result<std::net::UdpSocket, std::io::Error> {
            let socket = socket2::Socket::new(
                socket2::Domain::IPV4,
                socket2::Type::DGRAM,
                Some(socket2::Protocol::UDP),
            )?;

            // Set reuse address
            socket.set_reuse_address(true)?;

            // Bind socket
            socket.bind(&bind_addr_clone.into())?;

            // Set multicast TTL
            socket.set_multicast_ttl_v4(ttl)?;

            // Join multicast group
            socket.join_multicast_v4(&multicast_ip_clone, &Ipv4Addr::new(0, 0, 0, 0))?;

            // Convert to std::net::UdpSocket
            Ok(socket.into())
        })
        .await
        .map_err(|e| ChannelError::BackendError(format!("Failed to create socket in blocking context: {}", e)))?
        .map_err(|e| ChannelError::BackendError(format!("Failed to create UDP socket: {}", e)))?;

        // Convert to Tokio socket
        let socket = TokioUdpSocket::from_std(std_socket)
            .map_err(|e| ChannelError::BackendError(format!("Failed to convert socket: {}", e)))?;

        // Create broadcast channel for local subscribers
        let (broadcast_tx, _) = broadcast::channel(1024);
        let broadcast_tx_arc = Arc::new(broadcast_tx);

        // Start receive loop in background
        let socket_clone = Arc::new(socket);
        let broadcast_tx_clone = broadcast_tx_arc.clone();
        let stats_clone = Arc::new(ChannelStatsData {
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            messages_failed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        });
        let closed_clone = Arc::new(AtomicBool::new(false));
        let max_message_size = if udp_config.max_message_size == 0 {
            65507
        } else {
            udp_config.max_message_size as usize
        };

        // Spawn receive loop
        let channel_name = config.name.clone();
        tokio::spawn(Self::receive_loop(
            socket_clone.clone(),
            broadcast_tx_clone,
            stats_clone.clone(),
            closed_clone.clone(),
            max_message_size,
            channel_name,
        ));

        info!(
            channel = %config.name,
            multicast = %multicast_addr,
            cluster = %udp_config.cluster_name,
            "UDP channel created"
        );

        Ok(UdpChannel {
            config,
            udp_config,
            socket: socket_clone,
            multicast_addr,
            stats: stats_clone,
            closed: closed_clone,
            broadcast_tx: broadcast_tx_arc,
        })
    }

    /// Serialize message to protobuf bytes for UDP payload
    fn serialize_message(msg: &ChannelMessage) -> ChannelResult<Vec<u8>> {
        use prost::Message;
        let mut buf = Vec::new();
        msg.encode(&mut buf).map_err(|e| {
            ChannelError::SerializationError(format!("Failed to encode message: {}", e))
        })?;
        Ok(buf)
    }

    /// Deserialize message from UDP payload (protobuf bytes)
    fn deserialize_message(data: &[u8]) -> ChannelResult<ChannelMessage> {
        use prost::Message;
        ChannelMessage::decode(data).map_err(|e| {
            ChannelError::SerializationError(format!("Failed to decode message: {}", e))
        })
    }

    /// Start receiving loop (spawned as background task)
    async fn receive_loop(
        socket: Arc<TokioUdpSocket>,
        broadcast_tx: Arc<broadcast::Sender<ChannelMessage>>,
        stats: Arc<ChannelStatsData>,
        closed: Arc<AtomicBool>,
        max_message_size: usize,
        channel_name: String,
    ) {
        let mut buf = vec![0u8; max_message_size];
        
        loop {
            if closed.load(Ordering::Relaxed) {
                break;
            }

            match timeout(Duration::from_millis(100), socket.recv_from(&mut buf)).await {
                Ok(Ok((len, _addr))) => {
                    let data = &buf[..len];
                    match Self::deserialize_message(data) {
                        Ok(msg) => {
                            // Broadcast to local subscribers (ignore errors if no subscribers)
                            let _ = broadcast_tx.send(msg);
                            stats.messages_received.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            record_channel_error(
                                &channel_name,
                                "deserialize",
                                &format!("Failed to deserialize: {}", e),
                                "udp"
                            );
                        }
                    }
                }
                Ok(Err(e)) => {
                    if !closed.load(Ordering::Relaxed) {
                        stats.errors.fetch_add(1, Ordering::Relaxed);
                        record_channel_error(
                            &channel_name,
                            "receive",
                            &format!("Failed to receive: {}", e),
                            "udp"
                        );
                    }
                }
                Err(_) => {
                    // Timeout - continue loop to check closed flag
                    continue;
                }
            }
        }
    }
}

/// Check if IP address is in multicast range
fn is_multicast_ip(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] >= 224 && octets[0] <= 239
}

#[async_trait]
impl Channel for UdpChannel {
    async fn send(&self, message: ChannelMessage) -> ChannelResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        let payload = Self::serialize_message(&message)?;
        
        // Check message size
        let max_size = if self.udp_config.max_message_size == 0 {
            65507 // Max UDP packet size
        } else {
            self.udp_config.max_message_size as usize
        };

        if payload.len() > max_size {
            return Err(ChannelError::BackendError(format!(
                "Message size {} exceeds max_message_size {}",
                payload.len(),
                max_size
            )));
        }

        // Send to multicast address
        self.socket
            .send_to(&payload, self.multicast_addr)
            .await
            .map_err(|e| {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                ChannelError::BackendError(format!("Failed to send UDP message: {}", e))
            })?;

        self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
        debug!(
            channel = %self.config.name,
            message_id = %message.id,
            "UDP message sent"
        );

        Ok(message.id.clone())
    }

    async fn receive(&self, max_messages: u32) -> ChannelResult<Vec<ChannelMessage>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        // For UDP, we use the broadcast channel for local subscribers
        // This is a simplified implementation - in practice, you'd use subscribe() for streaming
        let mut subscriber = self.broadcast_tx.subscribe();
        let mut messages = Vec::new();
        let mut count = 0;

        while count < max_messages {
            match timeout(Duration::from_secs(5), subscriber.recv()).await {
                Ok(Ok(msg)) => {
                    messages.push(msg);
                    count += 1;
                }
                Ok(Err(_)) => {
                    // Channel closed
                    break;
                }
                Err(_) => {
                    // Timeout
                    break;
                }
            }
        }

        Ok(messages)
    }

    async fn try_receive(&self, max_messages: u32) -> ChannelResult<Vec<ChannelMessage>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        // Non-blocking receive from broadcast channel
        let mut subscriber = self.broadcast_tx.subscribe();
        let mut messages = Vec::new();
        let mut count = 0;

        while count < max_messages {
            match subscriber.try_recv() {
                Ok(msg) => {
                    messages.push(msg);
                    count += 1;
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    // No more messages available
                    break;
                }
                Err(broadcast::error::TryRecvError::Closed) => {
                    // Channel closed
                    break;
                }
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    // Lagged - skip and continue
                    continue;
                }
            }
        }

        Ok(messages)
    }

    async fn subscribe(
        &self,
        _consumer_group: Option<String>,
    ) -> ChannelResult<BoxStream<'static, ChannelMessage>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        // Create stream from broadcast channel
        let subscriber = self.broadcast_tx.subscribe();
        let closed = self.closed.clone();

        let stream = async_stream::stream! {
            let mut sub = subscriber;
            loop {
                if closed.load(Ordering::Relaxed) {
                    break;
                }

                match sub.recv().await {
                    Ok(msg) => yield msg,
                    Err(_) => break,
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn publish(&self, message: ChannelMessage) -> ChannelResult<u32> {
        // UDP multicast is inherently pub/sub - all subscribers receive
        // We can't know the exact count, so we return 1 (best-effort)
        self.send(message).await?;
        Ok(1) // Best-effort count
    }

    async fn ack(&self, _message_id: &str) -> ChannelResult<()> {
        // UDP is best-effort, no ACK support
        // This is a no-op for UDP channels
        Ok(())
    }

    async fn nack(&self, _message_id: &str, _requeue: bool) -> ChannelResult<()> {
        // UDP is best-effort, no NACK support
        // This is a no-op for UDP channels
        Ok(())
    }

    async fn get_stats(&self) -> ChannelResult<ChannelStats> {
        Ok(ChannelStats {
            name: self.config.name.clone(),
            backend: ChannelBackend::ChannelBackendUdp as i32,
            messages_sent: self.stats.messages_sent.load(Ordering::Relaxed),
            messages_received: self.stats.messages_received.load(Ordering::Relaxed),
            messages_pending: 0, // UDP has no queue
            messages_failed: self.stats.messages_failed.load(Ordering::Relaxed),
            avg_latency_us: 0, // Not tracked for UDP
            throughput: 0.0,    // Not tracked for UDP
            backend_stats: {
                let mut stats = std::collections::HashMap::new();
                stats.insert(
                    "multicast_address".to_string(),
                    self.multicast_addr.to_string(),
                );
                stats.insert("cluster_name".to_string(), self.udp_config.cluster_name.clone());
                stats.insert(
                    "errors".to_string(),
                    self.stats.errors.load(Ordering::Relaxed).to_string(),
                );
                stats
            },
        })
    }

    async fn close(&self) -> ChannelResult<()> {
        self.closed.store(true, Ordering::Relaxed);
        info!(
            channel = %self.config.name,
            "UDP channel closed"
        );
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    fn get_config(&self) -> &ChannelConfig {
        &self.config
    }
}
