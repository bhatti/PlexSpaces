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

//! Service wrappers for ActorContext
//!
//! ## Purpose
//! Provides adapter implementations that wrap Node's services to implement
//! the traits defined in `plexspaces_core::actor_context`.
//!
//! ## Design Decision
//! These wrappers are in the `node` crate (not `core`) to avoid circular dependencies:
//! - `core` defines the traits (no dependencies on node/actor-service)
//! - `node` implements the wrappers (depends on core, which is fine)
//! - Node creates wrappers and passes them to ActorContext

use async_trait::async_trait;
use std::sync::Arc;

use futures::stream::BoxStream;
use plexspaces_core::actor_context::{
    ActorService, ChannelService, ProcessGroupService, TupleSpaceProvider,
};
use plexspaces_core::Service;
use plexspaces_proto::common::v1::Message;
use plexspaces_tuplespace::{Pattern, Tuple, TupleSpaceError};
use std::time::Duration;

use futures::StreamExt;

// NodeOperationsWrapper removed - ActorFactory uses ActorRegistry and VirtualActorManager directly

/// Wrapper that adapts TupleSpace to TupleSpaceProvider trait
///
/// ## Purpose
/// Allows TupleSpace to be used as Arc<dyn TupleSpaceProvider> in ActorContext.
///
/// ## Note
/// This delegates to the TupleSpaceProviderWrapper from core since TupleSpace is already available.
pub struct TupleSpaceProviderWrapper {
    inner: Arc<plexspaces_tuplespace::TupleSpace>,
}

impl TupleSpaceProviderWrapper {
    /// Create a new wrapper from TupleSpace
    pub fn new(inner: Arc<plexspaces_tuplespace::TupleSpace>) -> Self {
        Self { inner }
    }
}

impl Service for TupleSpaceProviderWrapper {
    fn service_name(&self) -> String {
        "TupleSpaceProviderWrapper".to_string()
    }
}

#[async_trait]
impl TupleSpaceProvider for TupleSpaceProviderWrapper {
    async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError> {
        self.inner.write(tuple).await
    }

    async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        self.inner.read_all(pattern.clone()).await
    }

    async fn take(&self, pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError> {
        self.inner.take(pattern.clone()).await
    }

    async fn count(&self, pattern: &Pattern) -> Result<usize, TupleSpaceError> {
        self.inner.count(pattern.clone()).await
    }
}

/// Wrapper that adapts Channel to ChannelService trait
///
/// ## Purpose
/// Allows Channel implementations (InMemoryChannel, RedisChannel, etc.) to be used
/// as Arc<dyn ChannelService> in ActorContext.
///
/// ## Design
/// This wrapper manages a registry of channels by name, creating them on-demand
/// if they don't exist. For production use, Node should provide a channel registry.
pub struct ChannelServiceWrapper {
    // For now, we'll use a simple in-memory channel registry
    // In production, Node should provide a proper channel manager
    channels: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<String, Arc<dyn plexspaces_channel::Channel>>,
        >,
    >,
}

impl ChannelServiceWrapper {
    /// Create a new wrapper with empty channel registry
    pub fn new() -> Self {
        Self {
            channels: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Get or create a channel by name (public for use by TaskRouter)
    ///
    /// ## Note
    /// This method creates channels directly. In the future, this should use
    /// ServiceLocator::create_default_channel() to respect channel_provider configuration.
    pub async fn get_or_create_channel(
        &self,
        name: &str,
    ) -> Result<Arc<dyn plexspaces_channel::Channel>, Box<dyn std::error::Error + Send + Sync>>
    {
        let mut channels = self.channels.write().await;

        if let Some(channel) = channels.get(name) {
            return Ok(channel.clone());
        }

        // Create a new in-memory channel (default)
        // TODO: Use ServiceLocator::create_default_channel() when ServiceLocator is available
        use plexspaces_proto::channel::v1::{
            ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee,
        };
        let config = ChannelConfig {
            name: name.to_string(),
            provider: ChannelProvider::ChannelProviderInMemory as i32,
            capacity: 1000, // Default capacity
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };

        let channel_result = plexspaces_channel::InMemoryChannel::new(config).await;
        let channel = Arc::new(
            channel_result.map_err(|e| format!("Failed to create channel {}: {}", name, e))?,
        );

        channels.insert(name.to_string(), channel.clone());
        Ok(channel)
    }
}

impl Default for ChannelServiceWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl Service for ChannelServiceWrapper {
    fn service_name(&self) -> String {
        plexspaces_core::service_names::CHANNEL_SERVICE.to_string()
    }
}

#[async_trait]
impl ChannelService for ChannelServiceWrapper {
    async fn send_to_queue(
        &self,
        queue_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(queue_name).await?;

        // Message is already proto Message (unified type) - set channel name
        let mut channel_msg = message.clone();
        channel_msg.channel = queue_name.to_string();
        if channel_msg.timestamp.is_none() {
            channel_msg.timestamp = Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
            });
        }

        channel
            .send(channel_msg)
            .await
            .map_err(|e| format!("Failed to send to queue {}: {}", queue_name, e).into())
    }

    async fn publish_to_topic(
        &self,
        topic_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(topic_name).await?;

        // Message is already proto Message (unified type) - set channel name
        let mut channel_msg = message.clone();
        channel_msg.channel = topic_name.to_string();
        if channel_msg.timestamp.is_none() {
            channel_msg.timestamp = Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
            });
        }

        channel
            .publish(channel_msg)
            .await
            .map(|_| message.id.clone())
            .map_err(|e| format!("Failed to publish to topic {}: {}", topic_name, e).into())
    }

    async fn subscribe_to_topic(
        &self,
        topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(topic_name).await?;

        // Channel already returns proto Message stream - return directly
        let stream = channel
            .subscribe(None)
            .await
            .map_err(|e| format!("Failed to subscribe to topic {}: {}", topic_name, e))?;

        Ok(stream)
    }

    async fn receive_from_queue(
        &self,
        queue_name: &str,
        timeout: Option<std::time::Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(queue_name).await?;

        // Use try_receive for non-blocking, or receive with timeout
        let messages = if timeout.is_some() {
            // For timeout, we'd need to implement timeout logic
            // For now, use try_receive
            channel
                .try_receive(1)
                .await
                .map_err(|e| format!("Failed to receive from queue {}: {}", queue_name, e))?
        } else {
            // Blocking receive
            channel
                .receive(1)
                .await
                .map_err(|e| format!("Failed to receive from queue {}: {}", queue_name, e))?
        };

        // Channel already returns proto Message - return first message directly
        Ok(messages.into_iter().next())
    }
}

/// Stub ChannelService implementation (for testing/backward compatibility)
///
/// TODO: Remove once ChannelServiceWrapper is fully integrated
pub struct StubChannelService;

#[async_trait]
impl ChannelService for StubChannelService {
    async fn send_to_queue(
        &self,
        _queue_name: &str,
        _message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err(
            "StubChannelService: send_to_queue not implemented. Use real ChannelServiceWrapper."
                .into(),
        )
    }

    async fn publish_to_topic(
        &self,
        _topic_name: &str,
        _message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err(
            "StubChannelService: publish_to_topic not implemented. Use real ChannelServiceWrapper."
                .into(),
        )
    }

    async fn subscribe_to_topic(
        &self,
        _topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
        use futures::stream;
        Ok(Box::pin(stream::empty()))
    }

    async fn receive_from_queue(
        &self,
        _queue_name: &str,
        _timeout: Option<Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubChannelService: receive_from_queue not implemented. Use real ChannelServiceWrapper.".into())
    }
}

/// Wrapper that adapts ProcessGroupRegistry to ProcessGroupService trait
///
/// ## Purpose
/// Allows ProcessGroupRegistry to be used as Arc<dyn ProcessGroupService> in ActorContext.
///
/// ## Design
/// This wrapper adapts ProcessGroupRegistry's API to match ProcessGroupService trait.
/// It extracts tenant_id from ActorContext's namespace or uses a default tenant.
pub struct ProcessGroupServiceWrapper {
    registry: Arc<plexspaces_process_groups::ProcessGroupRegistry>,
}

impl ProcessGroupServiceWrapper {
    /// Create a new wrapper
    pub fn new(registry: Arc<plexspaces_process_groups::ProcessGroupRegistry>) -> Self {
        Self { registry }
    }
}

impl Service for ProcessGroupServiceWrapper {
    fn service_name(&self) -> String {
        plexspaces_core::service_names::PROCESS_GROUP_REGISTRY.to_string()
    }
}

#[async_trait]
impl ProcessGroupService for ProcessGroupServiceWrapper {
    async fn create_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        group_name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.registry
            .create_group(ctx, group_name)
            .await
            .map(|_| ())
            .map_err(|e| format!("Failed to create group {}: {}", group_name, e).into())
    }

    async fn delete_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        group_name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.registry
            .delete_group(ctx, group_name)
            .await
            .map_err(|e| format!("Failed to delete group {}: {}", group_name, e).into())
    }

    async fn join_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        group_name: &str,
        actor_id: &str,
        topics: Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // ProcessGroupRegistry requires group to exist first, so we create it if needed
        // This is a convenience - in production, groups should be created explicitly
        let _ = self.registry.create_group(ctx, group_name).await;

        use plexspaces_core::ActorId;
        let actor_id = ActorId::from_canonical(actor_id)
            .map_err(|e| format!("Invalid actor ID for group member '{actor_id}': {e}"))?;

        self.registry
            .join_group(ctx, group_name, &actor_id, topics)
            .await
            .map_err(|e| format!("Failed to join group {}: {}", group_name, e).into())
    }

    async fn leave_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        group_name: &str,
        actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_core::ActorId;
        let actor_id = ActorId::from_canonical(actor_id)
            .map_err(|e| format!("Invalid actor ID for group member '{actor_id}': {e}"))?;

        self.registry
            .leave_group(ctx, group_name, &actor_id)
            .await
            .map_err(|e| format!("Failed to leave group {}: {}", group_name, e).into())
    }

    async fn get_members(
        &self,
        ctx: &plexspaces_core::RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let actor_ids = self
            .registry
            .get_members(ctx, group_name)
            .await
            .map_err(|e| format!("Failed to get members of group {}: {}", group_name, e))?;

        // Convert ActorId to String
        Ok(actor_ids.iter().map(|id| id.to_string()).collect())
    }

    async fn get_local_members(
        &self,
        ctx: &plexspaces_core::RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let actor_ids = self
            .registry
            .get_local_members(ctx, group_name)
            .await
            .map_err(|e| format!("Failed to get local members of group {}: {}", group_name, e))?;

        // Convert ActorId to String
        Ok(actor_ids.iter().map(|id| id.to_string()).collect())
    }

    async fn list_groups(
        &self,
        ctx: &plexspaces_core::RequestContext,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.registry
            .list_groups(ctx)
            .await
            .map_err(|e| format!("Failed to list groups: {}", e).into())
    }

    async fn publish_to_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        group_name: &str,
        topic: Option<&str>,
        message: Message,
    ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        // Convert Message to Vec<u8> for ProcessGroupRegistry
        let payload = message.payload.clone();

        let actor_ids = self
            .registry
            .publish_to_group(ctx, group_name, topic, payload)
            .await
            .map_err(|e| format!("Failed to publish to group {}: {}", group_name, e))?;

        Ok(actor_ids.len() as u32)
    }
}
