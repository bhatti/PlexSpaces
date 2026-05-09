// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Host functions provided to WASM actors

use async_trait::async_trait;
use plexspaces_actor::actor_context::ObjectRegistry;
use plexspaces_actor::JournalStorage;
use plexspaces_actor::{
    ChannelService, ElasticPoolService, KeyValueStore, LockManager, RequestContext,
};
use plexspaces_process_groups::ProcessGroupRegistry;
use plexspaces_proto::actor::v1::{
    AllReduceShardGroupRequest, AllReduceShardGroupResponse, BarrierShardGroupRequest,
    BarrierShardGroupResponse, BroadcastShardGroupRequest, BroadcastShardGroupResponse,
    BulkUpdateShardGroupRequest, BulkUpdateShardGroupResponse, CreateShardGroupRequest,
    CreateShardGroupResponse, MapShardGroupRequest, MapShardGroupResponse, ReduceShardGroupRequest,
    ReduceShardGroupResponse, ScatterGatherRequest, ScatterGatherResponse, SpawnActorsRequest,
    SpawnActorsResponse,
};
use plexspaces_proto::application::v1::{ApplicationInfo, ApplicationMetrics};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::wasm::v1::HttpFetchRequest;
use std::sync::Arc;
use std::sync::Mutex;

use plexspaces_blob::BlobService;

/// Trait for sending messages from WASM actors to other actors
///
/// ## Purpose
/// Enables WASM actors to send messages to other actors (local or remote).
/// Implemented by the node/actor system to route messages.
///
/// ## Design (Simplicity + Extensibility)
/// - Simple trait interface
/// - Can be implemented by any message routing system
/// - Supports both local and remote actors
#[async_trait]
pub trait MessageSender: Send + Sync {
    /// Send a message to an actor
    ///
    /// ## Arguments
    /// * `from` - Sender actor ID
    /// * `to` - Recipient actor ID in canonical format `name//actor_type::namespace@node_id`
    /// * `message_type` - Message type (handler name, e.g. "ping", "cleanup_expired")
    /// * `message` - Message payload bytes
    ///
    /// ## Returns
    /// Success or error
    async fn send_message(
        &self,
        from: &str,
        to: &str,
        message_type: &str,
        message: &[u8],
    ) -> Result<(), String>;

    /// Send a message and wait for reply (request-reply pattern)
    ///
    /// ## Arguments
    /// * `from` - Sender actor ID
    /// * `to` - Recipient actor ID
    /// * `message_type` - Message type for behavior routing
    /// * `payload` - Request payload bytes
    /// * `timeout_ms` - Maximum time to wait for reply (0 = default timeout)
    ///
    /// ## Returns
    /// Reply payload bytes or error
    async fn ask(
        &self,
        from: &str,
        to: &str,
        message_type: &str,
        payload: Vec<u8>,
        timeout_ms: u64,
    ) -> Result<Vec<u8>, String>;

    /// Spawn a new actor from a WASM module
    ///
    /// ## Arguments
    /// * `from` - Spawning actor ID
    /// * `module_ref` - Module reference (name@version or hash)
    /// * `initial_state` - Initial state bytes
    /// * `actor_id` - Optional actor ID (if None, system generates)
    /// * `labels` - Optional labels for the actor
    /// * `durable` - Whether actor should be durable
    ///
    /// ## Returns
    /// Spawned actor ID or error
    async fn spawn_actor(
        &self,
        from: &str,
        module_ref: &str,
        initial_state: Vec<u8>,
        actor_id: Option<String>,
        labels: Vec<(String, String)>,
        durable: bool,
    ) -> Result<String, String>;

    /// Stop an actor gracefully
    ///
    /// ## Arguments
    /// * `from` - Actor requesting stop
    /// * `actor_id` - Actor to stop
    /// * `timeout_ms` - Maximum time to wait for graceful shutdown
    ///
    /// ## Returns
    /// Success or error
    async fn stop_actor(&self, from: &str, actor_id: &str, timeout_ms: u64) -> Result<(), String>;

    /// Link two actors (bidirectional death propagation)
    ///
    /// ## Arguments
    /// * `from` - Actor requesting link
    /// * `actor_id` - First actor ID
    /// * `linked_actor_id` - Second actor ID
    ///
    /// ## Returns
    /// Success or error
    async fn link_actor(
        &self,
        from: &str,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String>;

    /// Unlink two actors
    ///
    /// ## Arguments
    /// * `from` - Actor requesting unlink
    /// * `actor_id` - First actor ID
    /// * `linked_actor_id` - Second actor ID
    ///
    /// ## Returns
    /// Success or error
    async fn unlink_actor(
        &self,
        from: &str,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String>;

    /// Monitor an actor (one-way notification on termination)
    ///
    /// ## Arguments
    /// * `from` - Actor doing the monitoring
    /// * `actor_id` - Actor to monitor
    ///
    /// ## Returns
    /// Monitor reference (u64) or error
    async fn monitor_actor(&self, from: &str, actor_id: &str) -> Result<u64, String>;

    /// Remove monitoring for an actor
    ///
    /// ## Arguments
    /// * `from` - Actor doing the demonitoring
    /// * `actor_id` - Actor being monitored
    /// * `monitor_ref` - Monitor reference returned from monitor_actor
    ///
    /// ## Returns
    /// Success or error
    async fn demonitor_actor(
        &self,
        from: &str,
        actor_id: &str,
        monitor_ref: u64,
    ) -> Result<(), String>;

    /// Create a shard group via the framework actor service.
    async fn create_shard_group(
        &self,
        ctx: &RequestContext,
        req: CreateShardGroupRequest,
    ) -> Result<CreateShardGroupResponse, String> {
        let _ = (ctx, req);
        Err("create_shard_group not implemented".to_string())
    }

    /// Bulk update a shard group via the framework actor service.
    async fn bulk_update_shard_group(
        &self,
        ctx: &RequestContext,
        req: BulkUpdateShardGroupRequest,
    ) -> Result<BulkUpdateShardGroupResponse, String> {
        let _ = (ctx, req);
        Err("bulk_update_shard_group not implemented".to_string())
    }

    /// Map across a shard group via the framework actor service.
    async fn map_shard_group(
        &self,
        ctx: &RequestContext,
        req: MapShardGroupRequest,
    ) -> Result<MapShardGroupResponse, String> {
        let _ = (ctx, req);
        Err("map_shard_group not implemented".to_string())
    }

    /// Scatter-gather via the framework actor service.
    async fn scatter_gather(
        &self,
        ctx: &RequestContext,
        req: ScatterGatherRequest,
    ) -> Result<ScatterGatherResponse, String> {
        let _ = (ctx, req);
        Err("scatter_gather not implemented".to_string())
    }

    /// Broadcast via the framework actor service.
    async fn broadcast_shard_group(
        &self,
        ctx: &RequestContext,
        req: BroadcastShardGroupRequest,
    ) -> Result<BroadcastShardGroupResponse, String> {
        let _ = (ctx, req);
        Err("broadcast_shard_group not implemented".to_string())
    }

    /// Reduce via the framework actor service.
    async fn reduce_shard_group(
        &self,
        ctx: &RequestContext,
        req: ReduceShardGroupRequest,
    ) -> Result<ReduceShardGroupResponse, String> {
        let _ = (ctx, req);
        Err("reduce_shard_group not implemented".to_string())
    }

    /// All-reduce via the framework actor service.
    async fn all_reduce_shard_group(
        &self,
        ctx: &RequestContext,
        req: AllReduceShardGroupRequest,
    ) -> Result<AllReduceShardGroupResponse, String> {
        let _ = (ctx, req);
        Err("all_reduce_shard_group not implemented".to_string())
    }

    /// Barrier via the framework actor service.
    async fn barrier_shard_group(
        &self,
        ctx: &RequestContext,
        req: BarrierShardGroupRequest,
    ) -> Result<BarrierShardGroupResponse, String> {
        let _ = (ctx, req);
        Err("barrier_shard_group not implemented".to_string())
    }

    /// Batch spawn actors via the framework actor service.
    async fn spawn_actors(
        &self,
        ctx: &RequestContext,
        req: SpawnActorsRequest,
    ) -> Result<SpawnActorsResponse, String> {
        let _ = (ctx, req);
        Err("spawn_actors not implemented".to_string())
    }

    /// Merge a node-local metrics delta into an application.
    async fn merge_application_metrics(
        &self,
        ctx: &RequestContext,
        application_id: &str,
        metrics: ApplicationMetrics,
    ) -> Result<ApplicationMetrics, String> {
        let _ = (ctx, application_id, metrics);
        Err("merge_application_metrics not implemented".to_string())
    }

    /// Get unified application metrics for a local or remote node.
    async fn get_application_metrics(
        &self,
        ctx: &RequestContext,
        application_id: &str,
        node_id: &str,
    ) -> Result<ApplicationMetrics, String> {
        let _ = (ctx, application_id, node_id);
        Err("get_application_metrics not implemented".to_string())
    }

    /// Get application status for a local or remote node.
    async fn get_application_status(
        &self,
        ctx: &RequestContext,
        application_id: &str,
        node_id: &str,
    ) -> Result<(ApplicationInfo, String), String> {
        let _ = (ctx, application_id, node_id);
        Err("get_application_status not implemented".to_string())
    }
}

/// Host functions for WASM actors
pub struct HostFunctions {
    /// Message sender for routing messages (optional)
    message_sender: Option<Arc<dyn MessageSender>>,
    /// Channel service for queue/topic patterns (optional)
    channel_service: Option<Arc<dyn ChannelService>>,
    /// Key-value store for state/config/registry (optional)
    keyvalue_store: Option<Arc<dyn KeyValueStore>>,
    /// Process group registry for pub/sub (optional)
    process_group_registry: Option<Arc<ProcessGroupRegistry>>,
    /// Lock manager for distributed locks (optional)
    lock_manager: Option<Arc<dyn LockManager + Send + Sync>>,
    /// Object registry for service discovery (optional)
    object_registry: Option<Arc<dyn ObjectRegistry>>,
    /// Journal storage for durability (optional)
    journal_storage: Option<Arc<dyn JournalStorage>>,
    /// Blob service for object storage (optional)
    blob_service: Option<Arc<BlobService>>,
    /// Elastic pool service for checkout/checkin (optional)
    elastic_pool_service: Option<Arc<dyn ElasticPoolService>>,
    /// Resilient outbound HTTP client for named service links (optional).
    /// Populated from RuntimeConfig.service_links via ServiceLocator.
    outbound_http_client: Option<Arc<dyn plexspaces_actor::OutboundHttpClient>>,
    /// Shared send_after timer handles for this actor across all re-instantiations.
    /// Aborted en-masse on application undeploy so stale timers don't fire after cleanup.
    pub timer_handles: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl HostFunctions {
    /// Create new host functions context
    pub fn new() -> Self {
        Self {
            message_sender: None,
            channel_service: None,
            keyvalue_store: None,
            process_group_registry: None,
            lock_manager: None,
            object_registry: None,
            journal_storage: None,
            blob_service: None,
            elastic_pool_service: None,
            outbound_http_client: None,
            timer_handles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Abort all pending send_after timers registered by this actor.
    /// Called during application undeploy so timers don't fire after cleanup.
    pub fn cancel_all_timers(&self) {
        if let Ok(mut handles) = self.timer_handles.lock() {
            for handle in handles.drain(..) {
                handle.abort();
            }
        }
    }

    /// Create with message sender for multi-node coordination
    pub fn with_message_sender(sender: Arc<dyn MessageSender>) -> Self {
        Self {
            message_sender: Some(sender),
            channel_service: None,
            keyvalue_store: None,
            process_group_registry: None,
            lock_manager: None,
            object_registry: None,
            journal_storage: None,
            blob_service: None,
            elastic_pool_service: None,
            outbound_http_client: None,
            timer_handles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create with channel service for queue/topic patterns
    pub fn with_channel_service(channel_service: Arc<dyn ChannelService>) -> Self {
        Self {
            message_sender: None,
            channel_service: Some(channel_service),
            keyvalue_store: None,
            process_group_registry: None,
            lock_manager: None,
            object_registry: None,
            journal_storage: None,
            blob_service: None,
            elastic_pool_service: None,
            outbound_http_client: None,
            timer_handles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create with both message sender and channel service
    pub fn with_services(
        sender: Option<Arc<dyn MessageSender>>,
        channel_service: Option<Arc<dyn ChannelService>>,
    ) -> Self {
        Self {
            message_sender: sender,
            channel_service,
            keyvalue_store: None,
            process_group_registry: None,
            lock_manager: None,
            object_registry: None,
            journal_storage: None,
            blob_service: None,
            elastic_pool_service: None,
            outbound_http_client: None,
            timer_handles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create with all services
    pub fn with_all_services(
        sender: Option<Arc<dyn MessageSender>>,
        channel_service: Option<Arc<dyn ChannelService>>,
        keyvalue_store: Option<Arc<dyn KeyValueStore>>,
        process_group_registry: Option<Arc<ProcessGroupRegistry>>,
        lock_manager: Option<Arc<dyn LockManager + Send + Sync>>,
        object_registry: Option<Arc<dyn ObjectRegistry>>,
        journal_storage: Option<Arc<dyn JournalStorage>>,
        blob_service: Option<Arc<BlobService>>,
        elastic_pool_service: Option<Arc<dyn ElasticPoolService>>,
        outbound_http_client: Option<Arc<dyn plexspaces_actor::OutboundHttpClient>>,
        shared_timer_pool: Option<Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>>,
    ) -> Self {
        let timer_handles = shared_timer_pool
            .unwrap_or_else(|| Arc::new(Mutex::new(Vec::new())));
        Self {
            message_sender: sender,
            channel_service,
            keyvalue_store,
            process_group_registry,
            lock_manager,
            object_registry,
            journal_storage,
            blob_service,
            elastic_pool_service,
            outbound_http_client,
            timer_handles,
        }
    }

    /// Get blob service if available
    pub fn blob_service(&self) -> Option<&Arc<BlobService>> {
        self.blob_service.as_ref()
    }

    /// Get message sender if available
    pub fn message_sender(&self) -> Option<&Arc<dyn MessageSender>> {
        self.message_sender.as_ref()
    }

    /// Get channel service if available
    pub fn channel_service(&self) -> Option<&Arc<dyn ChannelService>> {
        self.channel_service.as_ref()
    }

    /// Get key-value store if available
    pub fn keyvalue_store(&self) -> Option<&Arc<dyn KeyValueStore>> {
        self.keyvalue_store.as_ref()
    }

    /// Get process group registry if available
    pub fn process_group_registry(&self) -> Option<&Arc<ProcessGroupRegistry>> {
        self.process_group_registry.as_ref()
    }

    /// Get elastic pool service if available
    pub fn elastic_pool_service(&self) -> Option<&Arc<dyn ElasticPoolService>> {
        self.elastic_pool_service.as_ref()
    }

    /// Get lock manager if available
    pub fn lock_manager(&self) -> Option<&Arc<dyn LockManager + Send + Sync>> {
        self.lock_manager.as_ref()
    }

    /// Get object registry if available
    pub fn object_registry(&self) -> Option<&Arc<dyn ObjectRegistry>> {
        self.object_registry.as_ref()
    }

    /// Get journal storage if available
    pub fn journal_storage(&self) -> Option<&Arc<dyn JournalStorage>> {
        self.journal_storage.as_ref()
    }

    /// Get outbound HTTP client if available
    pub fn outbound_http_client(&self) -> Option<&Arc<dyn plexspaces_actor::OutboundHttpClient>> {
        self.outbound_http_client.as_ref()
    }

    /// Execute an outbound HTTP request via a named service link and return the core response.
    pub async fn http_fetch(
        &self,
        link_name: &str,
        method: &str,
        path_and_query: &str,
        request: HttpFetchRequest,
    ) -> Result<plexspaces_actor::OutboundHttpResponse, String> {
        use plexspaces_actor::OutboundHttpRequest;
        let client = match &self.outbound_http_client {
            Some(client) => client,
            None => return Err("outbound HTTP client not configured".to_string()),
        };

        use plexspaces_actor::HttpHeader;
        let outbound_request = OutboundHttpRequest {
            method: method.to_string(),
            path_and_query: path_and_query.to_string(),
            headers: request.headers.into_iter().map(|(k, v)| HttpHeader { key: k, value: v }).collect(),
            body: request.body,
        };

        client
            .execute(link_name, outbound_request)
            .await
            .map_err(|err| err.to_string())
    }

    /// Send message via message sender if available
    pub async fn send_message(
        &self,
        from: &str,
        to: &str,
        message_type: &str,
        message: &[u8],
    ) -> Result<(), String> {
        if let Some(sender) = &self.message_sender {
            sender.send_message(from, to, message_type, message).await
        } else {
            // Fallback: log message (for development/testing)
            tracing::warn!(
                from = from,
                to = to,
                message_type = message_type,
                message_len = message.len(),
                "Message sender not configured, message not delivered"
            );
            Ok(())
        }
    }

    /// Get key-value store operation helper
    ///
    /// actor-world and native `plexspaces-actor` both resolve through this helper with
    /// request-scoped key-value semantics from the core store implementation.
    pub async fn get_keyvalue(
        &self,
        ctx: &RequestContext,
        key: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        if let Some(kv) = &self.keyvalue_store {
            kv.get(ctx, key)
                .await
                .map_err(|e| format!("KeyValue get failed: {}", e))
        } else {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    "KeyValue store not configured; kv_get will return error (check node passes keyvalue_store when instantiating WASM)"
                );
            }
            Err("KeyValue store not configured".to_string())
        }
    }

    /// Put key-value store operation helper
    pub async fn put_keyvalue(
        &self,
        ctx: &RequestContext,
        key: &str,
        value: Vec<u8>,
    ) -> Result<(), String> {
        if let Some(kv) = &self.keyvalue_store {
            kv.put(ctx, key, value)
                .await
                .map_err(|e| format!("KeyValue put failed: {}", e))
        } else {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    key = %key,
                    value_len = value.len(),
                    "KeyValue store not configured; kv_put will return error"
                );
            }
            Err("KeyValue store not configured".to_string())
        }
    }

    /// Delete key-value store operation helper
    pub async fn delete_keyvalue(&self, ctx: &RequestContext, key: &str) -> Result<(), String> {
        if let Some(kv) = &self.keyvalue_store {
            kv.delete(ctx, key)
                .await
                .map_err(|e| format!("KeyValue delete failed: {}", e))
        } else {
            Err("KeyValue store not configured".to_string())
        }
    }

    /// List keys by prefix operation helper
    pub async fn list_keyvalue(
        &self,
        ctx: &RequestContext,
        prefix: &str,
    ) -> Result<Vec<String>, String> {
        if let Some(kv) = &self.keyvalue_store {
            kv.list_keys(ctx, prefix)
                .await
                .map_err(|e| format!("KeyValue list failed: {}", e))
        } else {
            Err("KeyValue store not configured".to_string())
        }
    }

    /// Send message and wait for reply via message sender if available
    pub async fn ask(
        &self,
        from: &str,
        to: &str,
        message_type: &str,
        payload: Vec<u8>,
        timeout_ms: u64,
    ) -> Result<Vec<u8>, String> {
        if let Some(sender) = &self.message_sender {
            sender
                .ask(from, to, message_type, payload, timeout_ms)
                .await
        } else {
            Err("Message sender not configured for ask".to_string())
        }
    }

    /// Spawn a new actor via message sender if available
    pub async fn spawn_actor(
        &self,
        from: &str,
        module_ref: &str,
        initial_state: Vec<u8>,
        actor_id: Option<String>,
        labels: Vec<(String, String)>,
        durable: bool,
    ) -> Result<String, String> {
        if let Some(sender) = &self.message_sender {
            sender
                .spawn_actor(from, module_ref, initial_state, actor_id, labels, durable)
                .await
        } else {
            Err("Message sender not configured for spawn_actor".to_string())
        }
    }

    /// Create a shard group through the framework actor service.
    pub async fn create_shard_group(
        &self,
        ctx: &RequestContext,
        req: CreateShardGroupRequest,
    ) -> Result<CreateShardGroupResponse, String> {
        if let Some(sender) = &self.message_sender {
            sender.create_shard_group(ctx, req).await
        } else {
            Err("Actor service not configured for create_shard_group".to_string())
        }
    }

    /// Bulk update a shard group through the framework actor service.
    pub async fn bulk_update_shard_group(
        &self,
        ctx: &RequestContext,
        req: BulkUpdateShardGroupRequest,
    ) -> Result<BulkUpdateShardGroupResponse, String> {
        if let Some(sender) = &self.message_sender {
            sender.bulk_update_shard_group(ctx, req).await
        } else {
            Err("Actor service not configured for bulk_update_shard_group".to_string())
        }
    }

    /// Map across a shard group through the framework actor service.
    pub async fn map_shard_group(
        &self,
        ctx: &RequestContext,
        req: MapShardGroupRequest,
    ) -> Result<MapShardGroupResponse, String> {
        if let Some(sender) = &self.message_sender {
            sender.map_shard_group(ctx, req).await
        } else {
            Err("Actor service not configured for map_shard_group".to_string())
        }
    }

    /// Scatter-gather through the framework actor service.
    pub async fn scatter_gather(
        &self,
        ctx: &RequestContext,
        req: ScatterGatherRequest,
    ) -> Result<ScatterGatherResponse, String> {
        if let Some(sender) = &self.message_sender {
            sender.scatter_gather(ctx, req).await
        } else {
            Err("Actor service not configured for scatter_gather".to_string())
        }
    }

    /// Broadcast a message to all shards in a shard group.
    pub async fn broadcast_shard_group(
        &self,
        ctx: &RequestContext,
        req: BroadcastShardGroupRequest,
    ) -> Result<BroadcastShardGroupResponse, String> {
        if let Some(sender) = &self.message_sender {
            sender.broadcast_shard_group(ctx, req).await
        } else {
            Err("Actor service not configured for broadcast_shard_group".to_string())
        }
    }

    /// Reduce shard responses using a built-in framework reduction.
    pub async fn reduce_shard_group(
        &self,
        ctx: &RequestContext,
        req: ReduceShardGroupRequest,
    ) -> Result<ReduceShardGroupResponse, String> {
        if let Some(sender) = &self.message_sender {
            sender.reduce_shard_group(ctx, req).await
        } else {
            Err("Actor service not configured for reduce_shard_group".to_string())
        }
    }

    /// Reduce shard responses and broadcast the reduced result back to the group.
    pub async fn all_reduce_shard_group(
        &self,
        ctx: &RequestContext,
        req: AllReduceShardGroupRequest,
    ) -> Result<AllReduceShardGroupResponse, String> {
        if let Some(sender) = &self.message_sender {
            sender.all_reduce_shard_group(ctx, req).await
        } else {
            Err("Actor service not configured for all_reduce_shard_group".to_string())
        }
    }

    /// Synchronize all shards in a group at a barrier round.
    pub async fn barrier_shard_group(
        &self,
        ctx: &RequestContext,
        req: BarrierShardGroupRequest,
    ) -> Result<BarrierShardGroupResponse, String> {
        if let Some(sender) = &self.message_sender {
            sender.barrier_shard_group(ctx, req).await
        } else {
            Err("Actor service not configured for barrier_shard_group".to_string())
        }
    }

    /// Spawn multiple actors through the framework actor service.
    pub async fn spawn_actors(
        &self,
        ctx: &RequestContext,
        req: SpawnActorsRequest,
    ) -> Result<SpawnActorsResponse, String> {
        if let Some(sender) = &self.message_sender {
            sender.spawn_actors(ctx, req).await
        } else {
            Err("Actor service not configured for spawn_actors".to_string())
        }
    }

    /// Merge a node-local application metrics delta.
    pub async fn merge_application_metrics(
        &self,
        ctx: &RequestContext,
        application_id: &str,
        metrics: ApplicationMetrics,
    ) -> Result<ApplicationMetrics, String> {
        if let Some(sender) = &self.message_sender {
            sender
                .merge_application_metrics(ctx, application_id, metrics)
                .await
        } else {
            Err("Application metrics service not configured".to_string())
        }
    }

    /// Get unified application metrics for a local or remote node.
    pub async fn get_application_metrics(
        &self,
        ctx: &RequestContext,
        application_id: &str,
        node_id: &str,
    ) -> Result<ApplicationMetrics, String> {
        if let Some(sender) = &self.message_sender {
            sender
                .get_application_metrics(ctx, application_id, node_id)
                .await
        } else {
            Err("Application service not configured".to_string())
        }
    }

    /// Get application status from a local or remote node.
    pub async fn get_application_status(
        &self,
        ctx: &RequestContext,
        application_id: &str,
        node_id: &str,
    ) -> Result<(ApplicationInfo, String), String> {
        if let Some(sender) = &self.message_sender {
            sender
                .get_application_status(ctx, application_id, node_id)
                .await
        } else {
            Err("Application service not configured".to_string())
        }
    }

    /// Stop an actor via message sender if available
    pub async fn stop_actor(
        &self,
        from: &str,
        actor_id: &str,
        timeout_ms: u64,
    ) -> Result<(), String> {
        if let Some(sender) = &self.message_sender {
            sender.stop_actor(from, actor_id, timeout_ms).await
        } else {
            Err("Message sender not configured for stop_actor".to_string())
        }
    }

    /// Link two actors via message sender if available
    pub async fn link_actor(
        &self,
        from: &str,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String> {
        if let Some(sender) = &self.message_sender {
            sender.link_actor(from, actor_id, linked_actor_id).await
        } else {
            Err("Message sender not configured for link_actor".to_string())
        }
    }

    /// Unlink two actors via message sender if available
    pub async fn unlink_actor(
        &self,
        from: &str,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String> {
        if let Some(sender) = &self.message_sender {
            sender.unlink_actor(from, actor_id, linked_actor_id).await
        } else {
            Err("Message sender not configured for unlink_actor".to_string())
        }
    }

    /// Monitor an actor via message sender if available
    pub async fn monitor_actor(&self, from: &str, actor_id: &str) -> Result<u64, String> {
        if let Some(sender) = &self.message_sender {
            sender.monitor_actor(from, actor_id).await
        } else {
            Err("Message sender not configured for monitor_actor".to_string())
        }
    }

    /// Demonitor an actor via message sender if available
    pub async fn demonitor_actor(
        &self,
        from: &str,
        actor_id: &str,
        monitor_ref: u64,
    ) -> Result<(), String> {
        if let Some(sender) = &self.message_sender {
            sender.demonitor_actor(from, actor_id, monitor_ref).await
        } else {
            Err("Message sender not configured for demonitor_actor".to_string())
        }
    }

    /// Send message to queue via channel service if available
    pub async fn send_to_queue(
        &self,
        queue_name: &str,
        message_type: &str,
        payload: Vec<u8>,
    ) -> Result<String, String> {
        if let Some(channel_service) = &self.channel_service {
            let message = Message {
                id: ulid::Ulid::new().to_string(),
                payload,
                message_type: message_type.to_string(),
                ..Default::default()
            };
            channel_service
                .send_to_queue(queue_name, message)
                .await
                .map_err(|e| e.to_string())
        } else {
            Err("Channel service not configured".to_string())
        }
    }

    /// Publish message to topic via channel service if available
    pub async fn publish_to_topic(
        &self,
        topic_name: &str,
        message_type: &str,
        payload: Vec<u8>,
    ) -> Result<String, String> {
        if let Some(channel_service) = &self.channel_service {
            let message = Message {
                id: ulid::Ulid::new().to_string(),
                payload,
                message_type: message_type.to_string(),
                ..Default::default()
            };
            channel_service
                .publish_to_topic(topic_name, message)
                .await
                .map_err(|e| e.to_string())
        } else {
            Err("Channel service not configured".to_string())
        }
    }

    /// Receive message from queue via channel service if available
    pub async fn receive_from_queue(
        &self,
        queue_name: &str,
        timeout_ms: u64,
    ) -> Result<Option<(String, Vec<u8>)>, String> {
        if let Some(channel_service) = &self.channel_service {
            let timeout = if timeout_ms > 0 {
                Some(std::time::Duration::from_millis(timeout_ms))
            } else {
                None
            };
            match channel_service
                .receive_from_queue(queue_name, timeout)
                .await
            {
                Ok(Some(message)) => {
                    let message_type = message.message_type.clone();
                    Ok(Some((message_type, message.payload.clone())))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        } else {
            Err("Channel service not configured".to_string())
        }
    }
}

impl Default for HostFunctions {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use plexspaces_actor::ChannelService;
    use plexspaces_proto::common::v1::Message;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Mock ChannelService for testing
    struct MockChannelService {
        queues: Arc<RwLock<HashMap<String, Vec<Message>>>>,
        topics: Arc<RwLock<HashMap<String, Vec<Message>>>>,
    }

    impl MockChannelService {
        fn new() -> Self {
            Self {
                queues: Arc::new(RwLock::new(HashMap::new())),
                topics: Arc::new(RwLock::new(HashMap::new())),
            }
        }

        async fn get_queue_message(&self, queue_name: &str) -> Option<Message> {
            let mut queues = self.queues.write().await;
            queues.get_mut(queue_name)?.pop()
        }

        async fn get_topic_message(&self, topic_name: &str) -> Option<Message> {
            let mut topics = self.topics.write().await;
            topics.get_mut(topic_name)?.pop()
        }
    }

    #[async_trait::async_trait]
    impl ChannelService for MockChannelService {
        async fn send_to_queue(
            &self,
            queue_name: &str,
            message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let mut queues = self.queues.write().await;
            queues
                .entry(queue_name.to_string())
                .or_insert_with(Vec::new)
                .push(message);
            Ok("msg-001".to_string())
        }

        async fn publish_to_topic(
            &self,
            topic_name: &str,
            message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let mut topics = self.topics.write().await;
            topics
                .entry(topic_name.to_string())
                .or_insert_with(Vec::new)
                .push(message);
            Ok("msg-001".to_string())
        }

        async fn subscribe_to_topic(
            &self,
            _topic_name: &str,
        ) -> Result<
            futures::stream::BoxStream<'static, Message>,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            use futures::stream;
            Ok(Box::pin(stream::empty()))
        }

        async fn receive_from_queue(
            &self,
            queue_name: &str,
            _timeout: Option<std::time::Duration>,
        ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
            let mut queues = self.queues.write().await;
            if let Some(queue) = queues.get_mut(queue_name) {
                if !queue.is_empty() {
                    return Ok(Some(queue.remove(0)));
                }
            }
            Ok(None)
        }
    }

    #[test]
    fn test_new() {
        let _host_functions = HostFunctions::new();
        // Successfully creates instance
    }

    #[test]
    fn test_default() {
        let _host_functions = HostFunctions::default();
        // Successfully creates instance via Default
    }

    #[test]
    fn test_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<HostFunctions>();
        assert_sync::<HostFunctions>();
    }

    #[tokio::test]
    async fn test_send_to_queue_with_channel_service() {
        let channel_service = Arc::new(MockChannelService::new());
        let host_functions = HostFunctions::with_channel_service(channel_service.clone());

        let result = host_functions
            .send_to_queue("test-queue", "test-type", b"test payload".to_vec())
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "msg-001");

        // Verify message was added to queue
        let message = channel_service.get_queue_message("test-queue").await;
        assert!(message.is_some());
        let msg = message.unwrap();
        assert_eq!(msg.message_type.as_str(), "test-type");
        assert_eq!(msg.payload.as_slice(), b"test payload");
    }

    #[tokio::test]
    async fn test_send_to_queue_without_channel_service() {
        let host_functions = HostFunctions::new();

        let result = host_functions
            .send_to_queue("test-queue", "test-type", b"test payload".to_vec())
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Channel service not configured");
    }

    #[tokio::test]
    async fn test_publish_to_topic_with_channel_service() {
        let channel_service = Arc::new(MockChannelService::new());
        let host_functions = HostFunctions::with_channel_service(channel_service.clone());

        let result = host_functions
            .publish_to_topic("test-topic", "event-type", b"event data".to_vec())
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "msg-001");

        // Verify message was published to topic
        let message = channel_service.get_topic_message("test-topic").await;
        assert!(message.is_some());
        let msg = message.unwrap();
        assert_eq!(msg.message_type.as_str(), "event-type");
        assert_eq!(msg.payload.as_slice(), b"event data");
    }

    #[tokio::test]
    async fn test_publish_to_topic_without_channel_service() {
        let host_functions = HostFunctions::new();

        let result = host_functions
            .publish_to_topic("test-topic", "event-type", b"event data".to_vec())
            .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Channel service not configured");
    }

    #[tokio::test]
    async fn test_receive_from_queue_with_message() {
        let channel_service = Arc::new(MockChannelService::new());
        let host_functions = HostFunctions::with_channel_service(channel_service.clone());

        // First, send a message to the queue
        let _ = host_functions
            .send_to_queue("test-queue", "test-type", b"test payload".to_vec())
            .await;

        // Then receive it
        let result = host_functions.receive_from_queue("test-queue", 1000).await;

        assert!(result.is_ok());
        let received = result.unwrap();
        assert!(received.is_some());
        let (msg_type, payload) = received.unwrap();
        assert_eq!(msg_type, "test-type");
        assert_eq!(payload, b"test payload");
    }

    #[tokio::test]
    async fn test_receive_from_queue_empty() {
        let channel_service = Arc::new(MockChannelService::new());
        let host_functions = HostFunctions::with_channel_service(channel_service);

        // Try to receive from empty queue
        let result = host_functions.receive_from_queue("empty-queue", 100).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_receive_from_queue_without_channel_service() {
        let host_functions = HostFunctions::new();

        let result = host_functions.receive_from_queue("test-queue", 1000).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Channel service not configured");
    }

    #[tokio::test]
    async fn test_with_services() {
        let channel_service = Arc::new(MockChannelService::new());
        let host_functions = HostFunctions::with_services(None, Some(channel_service.clone()));

        // Should work with channel service
        let result = host_functions
            .send_to_queue("test-queue", "test-type", b"test".to_vec())
            .await;

        assert!(result.is_ok());
    }
}
