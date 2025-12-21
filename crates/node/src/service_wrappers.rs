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

use plexspaces_core::actor_context::{
    ActorService, ChannelService, FacetService, ObjectRegistry, ObjectRegistration, ProcessGroupService, TupleSpaceProvider,
};
use plexspaces_core::{RequestContext, Service};
use plexspaces_facet::Facet;
use plexspaces_core::ActorRef;
use plexspaces_mailbox::Message;
use plexspaces_tuplespace::{Pattern, Tuple, TupleSpaceError};
use futures::stream::BoxStream;
use std::time::Duration;

use crate::grpc_service::ActorServiceImpl as NodeActorServiceImpl;
use crate::{ActorLocation, Node};
use plexspaces_actor_service::ActorServiceImpl;
use futures::StreamExt;

/// Wrapper that adapts Node's ActorServiceImpl to ActorService trait
///
/// ## Purpose
/// Allows Node's ActorServiceImpl to be used as Arc<dyn ActorService> in ActorContext.
pub struct ActorServiceWrapper {
    inner: Arc<ActorServiceImpl>,
}

impl Service for ActorServiceWrapper {}

impl ActorServiceWrapper {
    /// Create a new wrapper from ActorServiceImpl
    pub fn new(inner: Arc<ActorServiceImpl>) -> Self {
        Self { inner }
    }

    /// Create a new wrapper from Node (creates ActorServiceImpl internally)
    /// Note: This method is deprecated - use Node's actor_registry directly
    pub fn from_node(_node: Arc<Node>, _object_registry: Arc<plexspaces_object_registry::ObjectRegistry>) -> Self {
        // This method signature is kept for backward compatibility but should not be used
        // ActorServiceImpl now requires ActorRegistry, not ObjectRegistry
        // Use Node's create_actor_context or access actor_registry directly
        panic!("from_node is deprecated - ActorServiceImpl requires ActorRegistry, not ObjectRegistry")
    }

    /// Get a reference to the inner ActorServiceImpl
    pub fn inner(&self) -> &Arc<ActorServiceImpl> {
        &self.inner
    }
}

#[async_trait]
impl ActorService for ActorServiceWrapper {
    async fn spawn_actor(
        &self,
        actor_id: &str,
        actor_type: &str,
        _initial_state: Vec<u8>,
    ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        // Delegate to Node's spawn_actor via ActorServiceImpl
        // For now, this is a placeholder - will be implemented when Node has spawn_actor helper
        Err(format!(
            "Actor spawning via ActorService not yet implemented. Use Node::spawn_actor() directly. actor_id={}, actor_type={}",
            actor_id, actor_type
        ).into())
    }

    async fn send(
        &self,
        actor_id: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Delegate to ActorServiceImpl's send_message
        self.inner.send_message(actor_id, message).await
    }

    async fn send_reply(
        &self,
        correlation_id: Option<&str>,
        sender_id: &plexspaces_core::ActorId,
        target_actor_id: plexspaces_core::ActorId,
        reply_message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Delegate to ActorServiceImpl's send_reply
        self.inner.send_reply(
            correlation_id,
            sender_id,
            target_actor_id,
            reply_message,
        ).await
    }

}

/// Wrapper that adapts ObjectRegistry to ObjectRegistry trait
///
/// ## Purpose
/// Allows ObjectRegistry to be used as Arc<dyn ObjectRegistry> in ActorContext.
pub struct ObjectRegistryWrapper {
    inner: Arc<plexspaces_object_registry::ObjectRegistry>,
}

impl ObjectRegistryWrapper {
    /// Create a new wrapper from ObjectRegistry
    pub fn new(registry: Arc<plexspaces_object_registry::ObjectRegistry>) -> Self {
        Self { inner: registry }
    }
}

impl Service for ObjectRegistryWrapper {}

#[async_trait]
impl ObjectRegistry for ObjectRegistryWrapper {
    async fn lookup(
        &self,
        tenant_id: &str,
        object_id: &str,
        namespace: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
        self.inner
            .lookup(&ctx, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
    }

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .register(ctx, registration)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }
}

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

impl Service for TupleSpaceProviderWrapper {}

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
    channels: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Arc<dyn plexspaces_channel::Channel>>>>,
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
    pub async fn get_or_create_channel(&self, name: &str) -> Result<Arc<dyn plexspaces_channel::Channel>, Box<dyn std::error::Error + Send + Sync>> {
        let mut channels = self.channels.write().await;
        
        if let Some(channel) = channels.get(name) {
            return Ok(channel.clone());
        }

        // Create a new in-memory channel (default)
        // TODO: Use ServiceLocator::create_default_channel() when ServiceLocator is available
        use plexspaces_proto::channel::v1::{ChannelBackend, ChannelConfig, DeliveryGuarantee, OrderingGuarantee};
        let config = ChannelConfig {
            name: name.to_string(),
            backend: ChannelBackend::ChannelBackendInMemory as i32,
            capacity: 1000, // Default capacity
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };
        
        let channel_result = plexspaces_channel::InMemoryChannel::new(config).await;
        let channel = Arc::new(channel_result
            .map_err(|e| format!("Failed to create channel {}: {}", name, e))?);
        
        channels.insert(name.to_string(), channel.clone());
        Ok(channel)
    }
}

impl Default for ChannelServiceWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl Service for ChannelServiceWrapper {}

#[async_trait]
impl ChannelService for ChannelServiceWrapper {
    async fn send_to_queue(
        &self,
        queue_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(queue_name).await?;
        
        // Convert Message to ChannelMessage
        use plexspaces_proto::channel::v1::ChannelMessage;
        let channel_msg = ChannelMessage {
            id: message.id.clone(),
            channel: queue_name.to_string(),
            sender_id: message.sender.clone().unwrap_or_default(),
            payload: message.payload.clone(),
            headers: message.metadata.clone(),
            timestamp: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
            }),
            partition_key: String::new(),
            correlation_id: message.correlation_id.clone().unwrap_or_default(),
            reply_to: message.reply_to.clone().unwrap_or_default(),
            delivery_count: 0,
        };
        
        channel.send(channel_msg).await
            .map_err(|e| format!("Failed to send to queue {}: {}", queue_name, e).into())
    }

    async fn publish_to_topic(
        &self,
        topic_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(topic_name).await?;
        
        // Convert Message to ChannelMessage
        use plexspaces_proto::channel::v1::ChannelMessage;
        let channel_msg = ChannelMessage {
            id: message.id.clone(),
            channel: topic_name.to_string(),
            sender_id: message.sender.clone().unwrap_or_default(),
            payload: message.payload.clone(),
            headers: message.metadata.clone(),
            timestamp: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
            }),
            partition_key: String::new(),
            correlation_id: message.correlation_id.clone().unwrap_or_default(),
            reply_to: message.reply_to.clone().unwrap_or_default(),
            delivery_count: 0,
        };
        
        channel.publish(channel_msg).await
            .map(|_| message.id.clone())
            .map_err(|e| format!("Failed to publish to topic {}: {}", topic_name, e).into())
    }

    async fn subscribe_to_topic(
        &self,
        topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(topic_name).await?;
        
        // Subscribe to channel and convert ChannelMessage stream to Message stream
        use futures::StreamExt;
        let stream = channel.subscribe(None).await
            .map_err(|e| format!("Failed to subscribe to topic {}: {}", topic_name, e))?;
        
        let message_stream = stream.map(|channel_msg| {
            let mut msg = Message::new(channel_msg.payload);
            // Message ID is auto-generated, but we can set metadata
            for (k, v) in channel_msg.headers {
                msg = msg.with_metadata(k, v);
            }
            if !channel_msg.correlation_id.is_empty() {
                msg = msg.with_correlation_id(channel_msg.correlation_id);
            }
            if !channel_msg.reply_to.is_empty() {
                msg = msg.with_reply_to(channel_msg.reply_to);
            }
            if !channel_msg.sender_id.is_empty() {
                msg = msg.with_sender(channel_msg.sender_id);
            }
            msg
        });
        
        Ok(Box::pin(message_stream))
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
            channel.try_receive(1).await
                .map_err(|e| format!("Failed to receive from queue {}: {}", queue_name, e))?
        } else {
            // Blocking receive
            channel.receive(1).await
                .map_err(|e| format!("Failed to receive from queue {}: {}", queue_name, e))?
        };
        
        if messages.is_empty() {
            return Ok(None);
        }
        
        // Convert first ChannelMessage to Message
        let channel_msg = &messages[0];
        let mut msg = Message::new(channel_msg.payload.clone());
        // Message ID is auto-generated, but we can set metadata
        for (k, v) in &channel_msg.headers {
            msg = msg.with_metadata(k.clone(), v.clone());
        }
        if !channel_msg.correlation_id.is_empty() {
            msg = msg.with_correlation_id(channel_msg.correlation_id.clone());
        }
        if !channel_msg.reply_to.is_empty() {
            msg = msg.with_reply_to(channel_msg.reply_to.clone());
        }
        if !channel_msg.sender_id.is_empty() {
            msg = msg.with_sender(channel_msg.sender_id.clone());
        }
        Ok(Some(msg))
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
        Err("StubChannelService: send_to_queue not implemented. Use real ChannelServiceWrapper.".into())
    }

    async fn publish_to_topic(
        &self,
        _topic_name: &str,
        _message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubChannelService: publish_to_topic not implemented. Use real ChannelServiceWrapper.".into())
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

impl Service for ProcessGroupServiceWrapper {}

#[async_trait]
impl ProcessGroupService for ProcessGroupServiceWrapper {
    async fn join_group(
        &self,
        group_name: &str,
        tenant_id: &str,
        namespace: &str,
        actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // ProcessGroupRegistry requires group to exist first, so we create it if needed
        // This is a convenience - in production, groups should be created explicitly
        let _ = self.registry.create_group(group_name, tenant_id, namespace).await;
        
        // Convert actor_id string to ActorId
        use plexspaces_core::ActorId;
        let actor_id = ActorId::from(actor_id.to_string());
        
        self.registry
            .join_group(group_name, tenant_id, namespace, &actor_id, vec![])
            .await
            .map_err(|e| format!("Failed to join group {}: {}", group_name, e).into())
    }

    async fn leave_group(
        &self,
        group_name: &str,
        tenant_id: &str,
        namespace: &str,
        actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
        use plexspaces_core::ActorId;
        let actor_id = ActorId::from(actor_id.to_string());
        
        self.registry
            .leave_group(&ctx, group_name, &actor_id)
            .await
            .map_err(|e| format!("Failed to leave group {}: {}", group_name, e).into())
    }

    async fn publish_to_group(
        &self,
        group_name: &str,
        tenant_id: &str,
        namespace: &str,
        message: Message,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
        // Convert Message to Vec<u8> for ProcessGroupRegistry
        let payload = message.payload().to_vec();
        
        let actor_ids = self.registry
            .publish_to_group(&ctx, group_name, None, payload)
            .await
            .map_err(|e| format!("Failed to publish to group {}: {}", group_name, e))?;
        
        // Convert ActorId to String
        Ok(actor_ids.iter().map(|id| id.to_string()).collect())
    }

    async fn get_members(
        &self,
        group_name: &str,
        tenant_id: &str,
        namespace: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
        
        let actor_ids = self.registry
            .get_members(&ctx, group_name)
            .await
            .map_err(|e| format!("Failed to get members of group {}: {}", group_name, e))?;
        
        // Convert ActorId to String
        Ok(actor_ids.iter().map(|id| id.to_string()).collect())
    }
}

/// Wrapper that adapts Node to FacetService trait
///
/// ## Purpose
/// Allows Node to be used as Arc<dyn FacetService> in ActorContext.
pub struct FacetServiceWrapper {
    node: Arc<Node>,
}

impl FacetServiceWrapper {
    /// Create a new wrapper from Node
    pub fn new(node: Arc<Node>) -> Self {
        Self { node }
    }
}

impl Service for FacetServiceWrapper {}

#[async_trait]
impl FacetService for FacetServiceWrapper {
    async fn get_facet(
        &self,
        actor_id: &plexspaces_core::ActorId,
        facet_type: &str,
    ) -> Result<std::sync::Arc<tokio::sync::RwLock<Box<dyn Facet>>>, Box<dyn std::error::Error + Send + Sync>> {
        // Get facets from FacetManager
        if let Some(facets) = self.node.facet_manager().get_facets(actor_id).await {
            // Get facet from facets container
            let facets_guard = facets.read().await;
            if let Some(facet) = facets_guard.get_facet(facet_type) {
                return Ok(facet);
            }
            drop(facets_guard); // Explicitly drop to avoid holding lock
        }
        
        Err(format!("Facet '{}' not found on actor {}", facet_type, actor_id).into())
    }
}

/// Wrapper for Firecracker VM Service
///
/// ## Purpose
/// Allows FirecrackerVmServiceImpl to be registered in ServiceLocator
/// and accessed by actors via ctx.service_locator.get_service()
#[cfg(feature = "firecracker")]
pub struct FirecrackerVmServiceWrapper {
    inner: Arc<crate::firecracker_service::FirecrackerVmServiceImpl>,
}

#[cfg(feature = "firecracker")]
impl Service for FirecrackerVmServiceWrapper {}

/// NodeMetricsUpdater wrapper - updates NodeMetrics from monitoring helpers
pub struct NodeMetricsUpdaterWrapper {
    node: Arc<crate::Node>,
}

impl NodeMetricsUpdaterWrapper {
    /// Create a new wrapper
    pub fn new(node: Arc<crate::Node>) -> Self {
        Self { node }
    }
}

impl plexspaces_core::Service for NodeMetricsUpdaterWrapper {}

#[async_trait::async_trait]
impl plexspaces_core::NodeMetricsUpdater for NodeMetricsUpdaterWrapper {
    async fn increment_messages_routed(&self) {
        self.node.increment_messages_routed().await;
    }
    
    async fn increment_local_deliveries(&self) {
        self.node.increment_local_deliveries().await;
    }
    
    async fn increment_remote_deliveries(&self) {
        self.node.increment_remote_deliveries().await;
    }
    
    async fn increment_failed_deliveries(&self) {
        self.node.increment_failed_deliveries().await;
    }
}

#[cfg(feature = "firecracker")]
impl FirecrackerVmServiceWrapper {
    /// Create a new wrapper from FirecrackerVmServiceImpl
    pub fn new(inner: Arc<crate::firecracker_service::FirecrackerVmServiceImpl>) -> Self {
        Self { inner }
    }

    /// Get a reference to the inner FirecrackerVmServiceImpl
    pub fn inner(&self) -> &Arc<crate::firecracker_service::FirecrackerVmServiceImpl> {
        &self.inner
    }
}

