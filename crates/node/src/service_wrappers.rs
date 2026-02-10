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
    ActorService, ChannelService, FacetService, ProcessGroupService, TupleSpaceProvider,
};
use plexspaces_core::Service;
use plexspaces_facet::Facet;
use plexspaces_proto::common::v1::Message;
use plexspaces_tuplespace::{Pattern, Tuple, TupleSpaceError};
use futures::stream::BoxStream;
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
        use plexspaces_proto::channel::v1::{ChannelProvider, ChannelConfig, DeliveryGuarantee, OrderingGuarantee};
        let config = ChannelConfig {
            name: name.to_string(),
            provider: ChannelProvider::ChannelProviderInMemory as i32,
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
        let message_id = message.id.clone();
        
        // Message is already proto Message (unified type) - send directly
        channel.send(message).await
            .map_err(|e| format!("Failed to send to queue {}: {}", queue_name, e))?;
        Ok(message_id)
    }

    async fn publish_to_topic(
        &self,
        topic_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(topic_name).await?;
        let message_id = message.id.clone();
        
        // Message is already proto Message (unified type) - publish directly
        channel.publish(message).await
            .map_err(|e| format!("Failed to publish to topic {}: {}", topic_name, e))?;
        Ok(message_id)
    }

    async fn subscribe_to_topic(
        &self,
        topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(topic_name).await?;
        
        // Message is already proto Message (unified type) - stream directly
        let stream = channel.subscribe(None).await
            .map_err(|e| format!("Failed to subscribe to topic {}: {}", topic_name, e))?;
        
        Ok(Box::pin(stream))
    }

    async fn receive_from_queue(
        &self,
        queue_name: &str,
        _timeout: Option<std::time::Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.get_or_create_channel(queue_name).await?;
        
        // Try to receive a message
        let messages = channel.try_receive(1).await
            .map_err(|e| format!("Failed to receive from queue {}: {}", queue_name, e))?;
        
        // Message is already proto Message (unified type) - return directly
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
        
        // Convert actor_id string to ActorId
        use plexspaces_core::ActorId;
        let actor_id = ActorId::from(actor_id.to_string());
        
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
        let actor_id = ActorId::from(actor_id.to_string());
        
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
        let actor_ids = self.registry
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
        let actor_ids = self.registry
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
        // Message is already proto Message (unified type) - use payload directly
        let payload = message.payload.clone();
        
        let actor_ids = self.registry
            .publish_to_group(ctx, group_name, topic, payload)
            .await
            .map_err(|e| format!("Failed to publish to group {}: {}", group_name, e))?;
        
        Ok(actor_ids.len() as u32)
    }
}

/// Wrapper that adapts ServiceLocator to FacetService trait
///
/// ## Purpose
/// Allows FacetManager from ServiceLocator to be used as Arc<dyn FacetService> in ActorContext.
/// This replaces the previous Node-based wrapper with a ServiceLocator-based approach.
pub struct FacetServiceWrapper {
    service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
}

impl FacetServiceWrapper {
    /// Create a new wrapper from ServiceLocator
    pub fn new(service_locator: Arc<dyn plexspaces_core::ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

impl Service for FacetServiceWrapper {
    fn service_name(&self) -> String {
        plexspaces_core::service_names::FACET_SERVICE.to_string()
    }
}

#[async_trait]
impl FacetService for FacetServiceWrapper {
    async fn get_facet(
        &self,
        actor_id: &plexspaces_core::ActorId,
        facet_type: &str,
    ) -> Result<std::sync::Arc<tokio::sync::RwLock<Box<dyn Facet>>>, Box<dyn std::error::Error + Send + Sync>> {
        // Get FacetManager from ServiceLocator
        let facet_manager_wrapper = self.service_locator.get_facet_manager().await
            .ok_or_else(|| format!("FacetManager not found in ServiceLocator"))?;
        let facet_manager = facet_manager_wrapper.inner_clone();
        
        if let Some(facets) = facet_manager.get_facets(actor_id).await {
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
/// and accessed by actors via ctx.service_locator.actor_registry() or other helper methods
#[cfg(feature = "firecracker")]
pub struct FirecrackerVmServiceWrapper {
    #[cfg(feature = "firecracker")]
    inner: Arc<plexspaces_services::firecracker_service::FirecrackerVmServiceImpl>,
}

#[cfg(feature = "firecracker")]
impl Service for FirecrackerVmServiceWrapper {
    fn service_name(&self) -> String {
        plexspaces_core::service_names::FIRECRACKER_VM_SERVICE.to_string()
    }
}

/// NodeMetricsAccessor wrapper - provides read and write access to NodeMetrics
///
/// ## Purpose
/// Allows components to read and update NodeMetrics without depending on Node type.
/// Combines reading and updating capabilities into a single wrapper.
pub struct NodeMetricsAccessorWrapper {
    node: Arc<crate::Node>,
}

impl NodeMetricsAccessorWrapper {
    /// Create a new wrapper
    pub fn new(node: Arc<crate::Node>) -> Self {
        Self { node }
    }
}

impl plexspaces_core::Service for NodeMetricsAccessorWrapper {
    fn service_name(&self) -> String {
        plexspaces_core::service_names::NODE_METRICS_ACCESSOR.to_string()
    }
}

#[async_trait::async_trait]
impl plexspaces_core::NodeMetricsAccessor for NodeMetricsAccessorWrapper {
    async fn get_metrics(&self) -> plexspaces_proto::node::v1::NodeMetrics {
        // Update metrics with current system info before returning
        self.node.update_metrics_with_system_info().await;
        
        let metrics = self.node.metrics().await;
        // Ensure node_id is set (it should be set in Node::start(), but ensure here)
        let mut metrics_clone = metrics.clone();
        if metrics_clone.node_id.is_empty() {
            metrics_clone.node_id = self.node.id().as_str().to_string();
        }
        // cluster_name is set in Node::start() from NodeConfig
        metrics_clone
    }
    
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
    
    async fn increment_shard_groups_created(&self) {
        self.node.increment_shard_groups_created().await;
    }
    
    async fn increment_shard_messages_sent(&self) {
        self.node.increment_shard_messages_sent().await;
    }
    
    async fn increment_shard_messages_received(&self) {
        self.node.increment_shard_messages_received().await;
    }
    
    async fn increment_shard_operations_total(&self) {
        self.node.increment_shard_operations_total().await;
    }
    
    async fn increment_shard_operations_failed(&self) {
        self.node.increment_shard_operations_failed().await;
    }
}

/// NodeConnectionInfo wrapper - provides access to node connection information
///
/// ## Purpose
/// Allows components to access node connection information (connected nodes list)
/// without depending on Node type.
pub struct NodeConnectionInfoWrapper {
    node: Arc<crate::Node>,
}

impl NodeConnectionInfoWrapper {
    /// Create a new wrapper
    pub fn new(node: Arc<crate::Node>) -> Self {
        Self { node }
    }
}

impl plexspaces_core::Service for NodeConnectionInfoWrapper {
    fn service_name(&self) -> String {
        "NodeConnectionInfo".to_string()
    }
}

#[async_trait::async_trait]
impl plexspaces_core::NodeConnectionInfo for NodeConnectionInfoWrapper {
    async fn connected_nodes(&self) -> Vec<String> {
        // Get connected nodes from NodeRegistry
        let service_locator = self.node.service_locator();
        if let Some(node_registry) = service_locator.get_node_registry().await {
            let ctx = service_locator.request_context_for_system_operations().await;
            match node_registry.list_nodes(&ctx, None, 1000, "").await {
                Ok((nodes, _)) => {
                    nodes.into_iter()
                        .map(|n| n.node_id)
                        .collect()
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        }
    }
}

#[cfg(feature = "firecracker")]
impl FirecrackerVmServiceWrapper {
    /// Create a new wrapper from FirecrackerVmServiceImpl
    pub fn new(inner: Arc<plexspaces_services::firecracker_service::FirecrackerVmServiceImpl>) -> Self {
        Self { inner }
    }

    /// Get a reference to the inner FirecrackerVmServiceImpl
    pub fn inner(&self) -> &Arc<plexspaces_services::firecracker_service::FirecrackerVmServiceImpl> {
        &self.inner
    }
}

