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

//! `ChannelServiceImpl` — implements `plexspaces_actor::ChannelService` on top of the
//! `Channel` trait so any provider (InMemory, SQLite, Redis, …) can be used as a
//! drop-in backend for actor messaging.
//!
//! ## Purpose
//! Bridges the high-level `ChannelService` trait (used by ActorContext, WASM host, etc.)
//! with the low-level `Channel` trait (pluggable transports defined in this crate).
//!
//! ## Architecture Context
//! ```text
//! ActorContext / WASM HostFunctions
//!         │
//!         ▼
//! ChannelServiceImpl  ◄── plexspaces_actor::ChannelService
//!         │
//!         ▼
//! Arc<dyn Channel>    ◄── InMemoryChannel | SqliteChannel | RedisChannel | …
//! ```
//!
//! ## Channel Registry
//! Named channels are created lazily on first use.  The default provider is
//! `InMemory` (unbounded); callers that need durability should pre-register
//! a channel via `ChannelServiceImpl::register_channel`.
//!
//! ## Provider Selection
//! `ChannelServiceImpl::new_with_provider` accepts a factory closure so callers
//! can inject SQLite, Redis, or any other `Channel` implementation without
//! coupling this module to optional features.

use crate::{Channel, ChannelError, InMemoryChannel};
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use plexspaces_proto::channel::v1::{ChannelConfig, ChannelProvider, DeliveryGuarantee};
use plexspaces_proto::common::v1::Message;
use plexspaces_service_traits::ChannelService;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Factory function type for creating Channel instances by name.
pub type ChannelFactory = Box<
    dyn Fn(&str) -> futures::future::BoxFuture<'static, Result<Arc<dyn Channel>, ChannelError>>
        + Send
        + Sync,
>;

/// `ChannelServiceImpl` — multi-channel registry backed by pluggable transports.
///
/// # Examples
/// ```rust,ignore
/// // InMemory (default — ideal for single-node / testing)
/// let svc = ChannelServiceImpl::new();
///
/// // Custom provider
/// let svc = ChannelServiceImpl::new_with_provider(Box::new(|name| {
///     let name = name.to_string();
///     Box::pin(async move {
///         let ch = SqliteChannel::new(&name, ":memory:").await?;
///         Ok(Arc::new(ch) as Arc<dyn Channel>)
///     })
/// }));
///
/// // Pre-register a channel
/// svc.register_channel("orders", InMemoryChannel::new(config).await?).await;
/// ```
pub struct ChannelServiceImpl {
    /// Registry of named channels.
    channels: Arc<RwLock<HashMap<String, Arc<dyn Channel>>>>,
    /// Factory to create new channels on demand (default: InMemory unbounded).
    factory: Arc<ChannelFactory>,
}

impl ChannelServiceImpl {
    /// Create with default InMemory provider (unbounded, at-most-once).
    pub fn new() -> Self {
        Self::new_with_provider(Box::new(|name| {
            let name = name.to_string();
            Box::pin(async move {
                let config = ChannelConfig {
                    name: name.clone(),
                    provider: ChannelProvider::ChannelProviderInMemory as i32,
                    capacity: 0, // unbounded
                    delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
                    ..Default::default()
                };
                let ch = InMemoryChannel::new(config)
                    .await
                    .map_err(|e| ChannelError::BackendError(e.to_string()))?;
                Ok(Arc::new(ch) as Arc<dyn Channel>)
            })
        }))
    }

    /// Create with a custom factory that builds the backend Channel for a given name.
    pub fn new_with_provider(factory: ChannelFactory) -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            factory: Arc::new(factory),
        }
    }

    /// Pre-register an already-created channel under `name`.
    pub async fn register_channel(&self, name: impl Into<String>, channel: impl Channel + 'static) {
        let mut map = self.channels.write().await;
        map.insert(name.into(), Arc::new(channel));
    }

    /// Pre-register an `Arc<dyn Channel>` directly.
    pub async fn register_channel_arc(&self, name: impl Into<String>, channel: Arc<dyn Channel>) {
        let mut map = self.channels.write().await;
        map.insert(name.into(), channel);
    }

    /// Get or lazily create the channel for `name`.
    async fn get_or_create(&self, name: &str) -> Result<Arc<dyn Channel>, ChannelError> {
        // Fast path — channel already exists
        {
            let map = self.channels.read().await;
            if let Some(ch) = map.get(name) {
                return Ok(ch.clone());
            }
        }

        // Slow path — create and register
        let new_ch = (self.factory)(name).await?;
        let mut map = self.channels.write().await;
        // Double-check (another task may have created it)
        map.entry(name.to_string())
            .or_insert_with(|| new_ch.clone());
        Ok(map[name].clone())
    }
}

impl Default for ChannelServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChannelService for ChannelServiceImpl {
    async fn send_to_queue(&self, queue_name: &str, message: Message) -> Result<String, BoxError> {
        let ch = self
            .get_or_create(queue_name)
            .await
            .map_err(|e| Box::new(e) as BoxError)?;
        ch.send(message).await.map_err(|e| Box::new(e) as BoxError)
    }

    async fn publish_to_topic(
        &self,
        topic_name: &str,
        message: Message,
    ) -> Result<String, BoxError> {
        let ch = self
            .get_or_create(topic_name)
            .await
            .map_err(|e| Box::new(e) as BoxError)?;
        let count = ch
            .publish(message)
            .await
            .map_err(|e| Box::new(e) as BoxError)?;
        // Return subscriber count as the "message id" — callers that need an ID
        // should use message.id instead; we return a stable string here.
        Ok(count.to_string())
    }

    async fn subscribe_to_topic(
        &self,
        topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, BoxError> {
        let ch = self
            .get_or_create(topic_name)
            .await
            .map_err(|e| Box::new(e) as BoxError)?;
        ch.subscribe(None)
            .await
            .map_err(|e| Box::new(e) as BoxError)
    }

    async fn receive_from_queue(
        &self,
        queue_name: &str,
        timeout: Option<Duration>,
    ) -> Result<Option<Message>, BoxError> {
        let ch = self
            .get_or_create(queue_name)
            .await
            .map_err(|e| Box::new(e) as BoxError)?;

        if let Some(dur) = timeout {
            match tokio::time::timeout(dur, ch.receive(1)).await {
                Ok(Ok(mut msgs)) => Ok(msgs.pop()),
                Ok(Err(e)) => Err(Box::new(e) as BoxError),
                Err(_) => Ok(None), // timeout — return None, not an error
            }
        } else {
            let mut msgs = ch.receive(1).await.map_err(|e| Box::new(e) as BoxError)?;
            Ok(msgs.pop())
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::common::v1::Message;
    use plexspaces_service_traits::ChannelService;

    fn make_msg(payload: &[u8]) -> Message {
        Message {
            id: ulid::Ulid::new().to_string(),
            payload: payload.to_vec(),
            message_type: "test".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_send_and_receive_single_message() {
        let svc = ChannelServiceImpl::new();
        let msg = make_msg(b"hello");
        let msg_id = msg.id.clone();

        svc.send_to_queue("q1", msg).await.unwrap();
        let received = svc
            .receive_from_queue("q1", Some(Duration::from_millis(200)))
            .await
            .unwrap();

        let received = received.expect("should have a message");
        assert_eq!(received.id, msg_id);
        assert_eq!(received.payload, b"hello");
    }

    #[tokio::test]
    async fn test_receive_timeout_returns_none() {
        let svc = ChannelServiceImpl::new();
        let result = svc
            .receive_from_queue("empty-queue", Some(Duration::from_millis(50)))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_multiple_queues_isolated() {
        let svc = ChannelServiceImpl::new();

        svc.send_to_queue("q-a", make_msg(b"msg-a")).await.unwrap();
        svc.send_to_queue("q-b", make_msg(b"msg-b")).await.unwrap();

        let a = svc
            .receive_from_queue("q-a", Some(Duration::from_millis(200)))
            .await
            .unwrap()
            .expect("msg-a");
        let b = svc
            .receive_from_queue("q-b", Some(Duration::from_millis(200)))
            .await
            .unwrap()
            .expect("msg-b");

        assert_eq!(a.payload, b"msg-a");
        assert_eq!(b.payload, b"msg-b");
    }

    #[tokio::test]
    async fn test_publish_to_topic_and_subscribe() {
        let svc = ChannelServiceImpl::new();

        // Subscribe first
        let mut stream = svc.subscribe_to_topic("events").await.unwrap();

        // Publish
        let msg = make_msg(b"event-payload");
        svc.publish_to_topic("events", msg.clone()).await.unwrap();

        // Receive from stream
        let received = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("timed out waiting for event")
            .expect("stream ended");

        assert_eq!(received.payload, b"event-payload");
    }

    #[tokio::test]
    async fn test_pre_registered_channel_is_used() {
        let svc = ChannelServiceImpl::new();

        // Pre-register with a specific config
        let config = plexspaces_proto::channel::v1::ChannelConfig {
            name: "pre-reg".to_string(),
            provider: ChannelProvider::ChannelProviderInMemory as i32,
            capacity: 10,
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ..Default::default()
        };
        let ch = InMemoryChannel::new(config).await.unwrap();
        svc.register_channel("pre-reg", ch).await;

        // Should use the pre-registered channel
        svc.send_to_queue("pre-reg", make_msg(b"data"))
            .await
            .unwrap();
        let received = svc
            .receive_from_queue("pre-reg", Some(Duration::from_millis(200)))
            .await
            .unwrap();
        assert!(received.is_some());
    }

    #[tokio::test]
    async fn test_fifo_ordering_multiple_messages() {
        let svc = ChannelServiceImpl::new();

        for i in 0u8..5 {
            svc.send_to_queue("ordered", make_msg(&[i])).await.unwrap();
        }

        for i in 0u8..5 {
            let msg = svc
                .receive_from_queue("ordered", Some(Duration::from_millis(200)))
                .await
                .unwrap()
                .expect("should receive");
            assert_eq!(msg.payload[0], i, "FIFO order violated at index {}", i);
        }
    }
}
