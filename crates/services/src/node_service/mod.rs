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

//! NodeService - gRPC service for node management
//!
//! ## Purpose
//! Provides centralized node management operations including:
//! - Node registration and discovery
//! - Health and metrics reporting
//! - Capacity calculation
//! - Application listing
//!
//! ## Design
//! - Uses ServiceLocator for accessing required services
//! - Replaces direct Node methods with gRPC service
//! - Supports pagination and streaming for scalability
//!
//! ## Security
//! - Masks secrets in ReleaseSpec responses (passwords, API keys, tokens)
//! - Uses `plexspaces_core::mask_release_spec` for consistent masking

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use async_trait::async_trait;
use futures::Stream;
use prost_types::Timestamp;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tonic::transport::Channel;
use tracing::{debug, info, warn};

use plexspaces_core::{RequestContext, ServiceLocator, mask_release_spec};
use plexspaces_proto::node::v1::{
    node_service_server::NodeService as NodeServiceTrait,
    node_service_client::NodeServiceClient,
    CalculateCapacityRequest, ConnectNodesRequest, ConnectNodesResponse,
    DisconnectNodesRequest, DisconnectNodesResponse, GetHealthRequest, GetHealthResponse,
    GetMetricsRequest, GetReleaseSpecRequest, GetReleaseSpecResponse, ListConnectedNodesRequest,
    ListConnectedNodesResponse, ListNodeApplicationsRequest, ListNodeApplicationsResponse,
    NodeApplicationInfo, NodeCapacity, NodeHealthStatus, NodeMetrics, NodeRegistration,
    NodeStatus, PingRequest, RegisterNodesRequest, RegisterNodesResponse, ReleaseSpec,
    SendHeartbeatRequest, SendHeartbeatResponse, StreamConnectedNodesRequest,
    UnregisterNodeRequest, UnregisterNodeResponse,
};

use crate::request_context_from_grpc_request;

/// Metrics tracking for NodeService
struct NodeServiceMetrics {
    /// Total messages routed
    messages_routed: std::sync::atomic::AtomicU64,
    /// Local message deliveries
    local_deliveries: std::sync::atomic::AtomicU64,
    /// Remote message deliveries
    remote_deliveries: std::sync::atomic::AtomicU64,
    /// Failed message deliveries
    failed_deliveries: std::sync::atomic::AtomicU64,
    /// Node start time (for uptime calculation)
    start_time: Instant,
}

impl Default for NodeServiceMetrics {
    fn default() -> Self {
        Self {
            messages_routed: std::sync::atomic::AtomicU64::new(0),
            local_deliveries: std::sync::atomic::AtomicU64::new(0),
            remote_deliveries: std::sync::atomic::AtomicU64::new(0),
            failed_deliveries: std::sync::atomic::AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }
}

/// NodeService implementation
///
/// ## Thread Safety
/// NodeServiceImpl is `Send + Sync` and can be safely shared across gRPC handlers.
///
/// ## Performance
/// - Uses atomic counters for metrics (lock-free)
/// - Caches system info to avoid repeated sysinfo calls
/// - Streams large result sets to minimize memory usage
pub struct NodeServiceImpl {
    /// ServiceLocator for accessing required services
    service_locator: Arc<dyn ServiceLocator>,
    /// Local node ID
    local_node_id: String,
    /// Release spec (stored for get_release_spec)
    release_spec: Arc<RwLock<Option<ReleaseSpec>>>,
    /// Internal metrics tracking
    metrics: NodeServiceMetrics,
}

impl NodeServiceImpl {
    /// Create a new NodeServiceImpl
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator for accessing required services
    /// * `local_node_id` - ID of the local node
    pub fn new(service_locator: Arc<dyn ServiceLocator>, local_node_id: String) -> Self {
        Self {
            service_locator,
            local_node_id,
            release_spec: Arc::new(RwLock::new(None)),
            metrics: NodeServiceMetrics::default(),
        }
    }

    /// Create a new NodeServiceImpl with ReleaseSpec
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator for accessing required services
    /// * `local_node_id` - ID of the local node
    /// * `release_spec` - ReleaseSpec for this node
    pub fn with_release_spec(
        service_locator: Arc<dyn ServiceLocator>,
        local_node_id: String,
        release_spec: ReleaseSpec,
    ) -> Self {
        Self {
            service_locator,
            local_node_id,
            release_spec: Arc::new(RwLock::new(Some(release_spec))),
            metrics: NodeServiceMetrics::default(),
        }
    }

    /// Set or update the ReleaseSpec
    pub async fn set_release_spec(&self, spec: ReleaseSpec) {
        let mut guard = self.release_spec.write().await;
        *guard = Some(spec);
    }
    
    /// Get the release spec (for internal/testing use)
    #[cfg(test)]
    pub async fn get_release_spec_internal(&self) -> Option<ReleaseSpec> {
        let guard = self.release_spec.read().await;
        guard.clone()
    }

    /// Get node uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        self.metrics.start_time.elapsed().as_secs()
    }

    /// Extract RequestContext from gRPC request
    async fn extract_context<T>(&self, request: &Request<T>) -> Result<RequestContext, Status> {
        let labels = std::collections::HashMap::new();
        request_context_from_grpc_request(request.metadata(), &labels, &self.service_locator)
            .await
            .map_err(|e| Status::unauthenticated(format!("Failed to extract context: {}", e)))
    }

    /// Get node metrics with system info
    ///
    /// ## Performance
    /// - System metrics are refreshed on each call (sysinfo is efficient)
    /// - Actor/connection counts are cached via ServiceLocator
    /// - Message counters use atomic operations (no locks)
    async fn get_metrics_internal(&self) -> NodeMetrics {
        use sysinfo::System;
        use std::sync::atomic::Ordering;
        
        let mut system = System::new();
        system.refresh_all();

        // Get system info
        let used_memory = system.used_memory();
        let available_memory = system.available_memory();
        let cpu_count = system.cpus().len() as u32;
        let cpu_usage = if cpu_count > 0 {
            system.cpus().iter().map(|cpu| cpu.cpu_usage() as f64).sum::<f64>() / cpu_count as f64
        } else {
            0.0
        };

        // Get actor counts from ActorRegistry
        let active_actors = if let Some(actor_registry) = self.service_locator.actor_registry().await {
            let registered_ids = actor_registry.registered_actor_ids().read().await;
            registered_ids.len() as u32
        } else {
            0
        };

        // Get connected nodes count from NodeRegistry
        let connected_nodes = if let Some(node_registry) = self.service_locator.get_node_registry().await {
            let ctx = self.service_locator.request_context_for_system_operations().await;
            match node_registry.list_nodes(&ctx, None, 1000, "").await {
                Ok((nodes, _)) => nodes.len() as u32,
                Err(_) => 0,
            }
        } else {
            0
        };

        // Get cluster name from NodeConfig
        let cluster_name = if let Some(config) = self.service_locator.get_node_config().await {
            config.cluster_name.clone()
        } else {
            String::new()
        };

        NodeMetrics {
            memory_used_bytes: used_memory,
            memory_available_bytes: available_memory,
            cpu_usage_percent: cpu_usage,
            uptime_seconds: self.uptime_seconds(),
            messages_routed: self.metrics.messages_routed.load(Ordering::Relaxed),
            local_deliveries: self.metrics.local_deliveries.load(Ordering::Relaxed),
            remote_deliveries: self.metrics.remote_deliveries.load(Ordering::Relaxed),
            failed_deliveries: self.metrics.failed_deliveries.load(Ordering::Relaxed),
            active_actors,
            connected_nodes,
            shard_groups_created: 0, // TODO: Track shard groups created in NodeService
            shard_messages_sent: 0, // TODO: Track shard messages sent in NodeService
            shard_messages_received: 0, // TODO: Track shard messages received in NodeService
            shard_operations_total: 0, // TODO: Track shard operations total in NodeService
            shard_operations_failed: 0, // TODO: Track shard operations failed in NodeService
            node_id: self.local_node_id.clone(),
            cluster_name,
        }
    }

    /// Calculate node capacity
    async fn calculate_capacity_internal(&self) -> NodeCapacity {
        use sysinfo::{Disks, System};
        use plexspaces_proto::common::v1::ResourceSpec;

        let mut sys = System::new_all();
        sys.refresh_all();

        // Total resources
        let total_memory_bytes = sys.total_memory();
        let total_cpu_cores = sys.cpus().len() as f64;
        
        let disks = Disks::new_with_refreshed_list();
        let total_disk_bytes: u64 = disks.iter().map(|d| d.total_space()).sum();

        let total_resources = ResourceSpec {
            cpu_cores: total_cpu_cores,
            memory_bytes: total_memory_bytes,
            disk_bytes: total_disk_bytes,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        // Allocated resources - would need to track actor resource usage
        let allocated_resources = ResourceSpec {
            cpu_cores: 0.0,
            memory_bytes: 0,
            disk_bytes: 0,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        // Available resources
        let available_resources = ResourceSpec {
            cpu_cores: total_cpu_cores,
            memory_bytes: total_memory_bytes,
            disk_bytes: total_disk_bytes,
            gpu_count: 0,
            gpu_type: String::new(),
        };

        NodeCapacity {
            total: Some(total_resources),
            allocated: Some(allocated_resources),
            available: Some(available_resources),
            labels: std::collections::HashMap::new(),
        }
    }

    /// Increment messages_routed counter
    ///
    /// Thread-safe: uses atomic increment
    pub fn increment_messages_routed(&self) {
        use std::sync::atomic::Ordering;
        self.metrics.messages_routed.fetch_add(1, Ordering::Relaxed);
        metrics::counter!("plexspaces_node_messages_routed_total",
            "node_id" => self.local_node_id.clone()
        ).increment(1);
    }

    /// Increment local_deliveries counter
    ///
    /// Thread-safe: uses atomic increment
    pub fn increment_local_deliveries(&self) {
        use std::sync::atomic::Ordering;
        self.metrics.local_deliveries.fetch_add(1, Ordering::Relaxed);
        metrics::counter!("plexspaces_node_local_deliveries_total",
            "node_id" => self.local_node_id.clone()
        ).increment(1);
    }

    /// Increment remote_deliveries counter
    ///
    /// Thread-safe: uses atomic increment
    pub fn increment_remote_deliveries(&self) {
        use std::sync::atomic::Ordering;
        self.metrics.remote_deliveries.fetch_add(1, Ordering::Relaxed);
        metrics::counter!("plexspaces_node_remote_deliveries_total",
            "node_id" => self.local_node_id.clone()
        ).increment(1);
    }

    /// Increment failed_deliveries counter
    ///
    /// Thread-safe: uses atomic increment
    pub fn increment_failed_deliveries(&self) {
        use std::sync::atomic::Ordering;
        self.metrics.failed_deliveries.fetch_add(1, Ordering::Relaxed);
        metrics::counter!("plexspaces_node_failed_deliveries_total",
            "node_id" => self.local_node_id.clone()
        ).increment(1);
    }

    /// Get current message routing statistics
    pub fn routing_stats(&self) -> (u64, u64, u64, u64) {
        use std::sync::atomic::Ordering;
        (
            self.metrics.messages_routed.load(Ordering::Relaxed),
            self.metrics.local_deliveries.load(Ordering::Relaxed),
            self.metrics.remote_deliveries.load(Ordering::Relaxed),
            self.metrics.failed_deliveries.load(Ordering::Relaxed),
        )
    }
}

#[async_trait]
impl NodeServiceTrait for NodeServiceImpl {
    async fn get_release_spec(
        &self,
        request: Request<GetReleaseSpecRequest>,
    ) -> Result<Response<GetReleaseSpecResponse>, Status> {
        let _ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        // Only return release spec for local node
        if req.node_id != self.local_node_id {
            return Err(Status::not_found(format!("Node not found: {}", req.node_id)));
        }

        // Get release spec and mask secrets before returning
        let spec_guard = self.release_spec.read().await;
        let masked_spec = spec_guard.as_ref().map(|spec| {
            // SECURITY: Mask all secrets before returning via API
            // This prevents credential leakage in logs, responses, etc.
            mask_release_spec(spec.clone())
        });

        info!(
            node_id = %req.node_id,
            has_spec = %masked_spec.is_some(),
            "GetReleaseSpec request"
        );

        Ok(Response::new(GetReleaseSpecResponse {
            release_spec: masked_spec,
        }))
    }

    async fn register_nodes(
        &self,
        request: Request<RegisterNodesRequest>,
    ) -> Result<Response<RegisterNodesResponse>, Status> {
        let ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        let node_registry = self.service_locator.get_node_registry().await
            .ok_or_else(|| Status::internal("NodeRegistry not available"))?;

        let mut registered_ids = Vec::new();
        let mut errors = std::collections::HashMap::new();

        for node_reg in req.nodes {
            let node_id = node_reg.node_id.clone();
            match node_registry.register_node(&ctx, node_reg).await {
                Ok(()) => {
                    registered_ids.push(node_id);
                }
                Err(e) => {
                    errors.insert(node_id, e.to_string());
                }
            }
        }

        info!("Registered {} nodes, {} errors", registered_ids.len(), errors.len());

        Ok(Response::new(RegisterNodesResponse {
            registered_node_ids: registered_ids,
            errors,
        }))
    }

    async fn unregister_node(
        &self,
        request: Request<UnregisterNodeRequest>,
    ) -> Result<Response<UnregisterNodeResponse>, Status> {
        let ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        let node_registry = self.service_locator.get_node_registry().await
            .ok_or_else(|| Status::internal("NodeRegistry not available"))?;

        node_registry.unregister_node(&ctx, &req.node_id).await
            .map_err(|e| Status::internal(format!("Failed to unregister: {}", e)))?;

        info!("Unregistered node: {}", req.node_id);

        Ok(Response::new(UnregisterNodeResponse { success: true }))
    }

    async fn list_connected_nodes(
        &self,
        request: Request<ListConnectedNodesRequest>,
    ) -> Result<Response<ListConnectedNodesResponse>, Status> {
        let ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        let node_registry = self.service_locator.get_node_registry().await
            .ok_or_else(|| Status::internal("NodeRegistry not available"))?;

        let cluster = if req.cluster.is_empty() { None } else { Some(req.cluster.as_str()) };
        let page_size = if req.page_size > 0 { req.page_size as u32 } else { 100 };

        let (nodes, next_token) = node_registry.list_nodes(&ctx, cluster, page_size, &req.page_token).await
            .map_err(|e| Status::internal(format!("Failed to list nodes: {}", e)))?;

        let total_count = nodes.len() as i32;
        Ok(Response::new(ListConnectedNodesResponse {
            nodes,
            next_page_token: next_token,
            total_count,
        }))
    }

    type StreamConnectedNodesStream = Pin<Box<dyn Stream<Item = Result<NodeRegistration, Status>> + Send + 'static>>;

    async fn stream_connected_nodes(
        &self,
        request: Request<StreamConnectedNodesRequest>,
    ) -> Result<Response<Self::StreamConnectedNodesStream>, Status> {
        let ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        let node_registry = self.service_locator.get_node_registry().await
            .ok_or_else(|| Status::internal("NodeRegistry not available"))?;

        let cluster = if req.cluster.is_empty() { None } else { Some(req.cluster.clone()) };

        // Stream nodes from registry
        let ctx_clone = ctx.clone();
        let cluster_clone = cluster.clone();
        
        let stream = async_stream::try_stream! {
            let mut page_token = String::new();
            loop {
                let cluster_ref = cluster_clone.as_deref();
                let (nodes, next_token) = node_registry.list_nodes(&ctx_clone, cluster_ref, 100, &page_token).await
                    .map_err(|e| Status::internal(format!("Failed to list nodes: {}", e)))?;

                for node in nodes {
                    yield node;
                }

                if next_token.is_empty() {
                    break;
                }
                page_token = next_token;
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_metrics(
        &self,
        request: Request<GetMetricsRequest>,
    ) -> Result<Response<NodeMetrics>, Status> {
        let _ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        // Only return metrics for local node
        if req.node_id != self.local_node_id {
            return Err(Status::not_found(format!("Node not found: {}", req.node_id)));
        }

        let metrics = self.get_metrics_internal().await;
        Ok(Response::new(metrics))
    }

    async fn calculate_capacity(
        &self,
        request: Request<CalculateCapacityRequest>,
    ) -> Result<Response<NodeCapacity>, Status> {
        let _ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        // Only return capacity for local node
        if req.node_id != self.local_node_id {
            return Err(Status::not_found(format!("Node not found: {}", req.node_id)));
        }

        let capacity = self.calculate_capacity_internal().await;
        Ok(Response::new(capacity))
    }

    async fn list_node_applications(
        &self,
        request: Request<ListNodeApplicationsRequest>,
    ) -> Result<Response<ListNodeApplicationsResponse>, Status> {
        let _ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        // Only return applications for local node
        if req.node_id != self.local_node_id {
            return Err(Status::not_found(format!("Node not found: {}", req.node_id)));
        }

        // Get application list from ApplicationManager
        let app_manager = self.service_locator.application_manager().await
            .ok_or_else(|| Status::internal("ApplicationManager not available"))?;

        let app_names = app_manager.list_applications().await;
        let mut applications = Vec::new();

        for name in app_names {
            if let Some(info) = app_manager.get_application_info(&name).await {
                // Convert status enum value to string
                let status_str = match plexspaces_proto::application::v1::ApplicationStatus::try_from(info.status) {
                    Ok(s) => format!("{:?}", s),
                    Err(_) => "Unknown".to_string(),
                };
                applications.push(NodeApplicationInfo {
                    name: info.name,
                    version: info.version,
                    status: status_str,
                    started_at: info.deployed_at.clone(), // Use deployed_at as started_at
                    actor_count: info.metrics.as_ref().map(|m| m.actor_count as i32).unwrap_or(0),
                    metadata: std::collections::HashMap::new(), // ApplicationInfo doesn't have metadata field
                });
            }
        }

        let total_count = applications.len() as i32;
        Ok(Response::new(ListNodeApplicationsResponse {
            applications,
            next_page_token: String::new(),
            total_count,
        }))
    }

    async fn get_health(
        &self,
        request: Request<GetHealthRequest>,
    ) -> Result<Response<GetHealthResponse>, Status> {
        let _ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        // Only return health for local node
        if req.node_id != self.local_node_id {
            return Err(Status::not_found(format!("Node not found: {}", req.node_id)));
        }

        // Check if shutdown is requested
        let is_shutting_down = self.service_locator.is_shutdown_requested();
        
        let (status, message) = if is_shutting_down {
            (NodeHealthStatus::NodeHealthStatusDegraded as i32, "Node is shutting down".to_string())
        } else {
            (NodeHealthStatus::NodeHealthStatusHealthy as i32, "Healthy".to_string())
        };

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();

        Ok(Response::new(GetHealthResponse {
            status,
            message,
            last_checked: Some(Timestamp {
                seconds: now.as_secs() as i64,
                nanos: now.subsec_nanos() as i32,
            }),
            details: std::collections::HashMap::new(),
        }))
    }

    async fn send_heartbeat(
        &self,
        request: Request<SendHeartbeatRequest>,
    ) -> Result<Response<SendHeartbeatResponse>, Status> {
        let ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        let node_registry = self.service_locator.get_node_registry().await
            .ok_or_else(|| Status::internal("NodeRegistry not available"))?;

        node_registry.send_heartbeat(&ctx, &req.node_id, req.capacity).await
            .map_err(|e| Status::internal(format!("Failed to send heartbeat: {}", e)))?;

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();

        Ok(Response::new(SendHeartbeatResponse {
            acknowledged: true,
            server_time: Some(Timestamp {
                seconds: now.as_secs() as i64,
                nanos: now.subsec_nanos() as i32,
            }),
        }))
    }

    // ============================================================================
    // SWIM Protocol RPCs
    // ============================================================================

    async fn ping(
        &self,
        request: Request<plexspaces_proto::node::v1::PingRequest>,
    ) -> Result<Response<plexspaces_proto::node::v1::PingResponse>, Status> {
        let req = request.into_inner();
        
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("Received ping from node: {}", req.source_node_id);
        }
        metrics::counter!("plexspaces_swim_pings_received").increment(1);

        // Get our incarnation from SWIM protocol
        let incarnation = if let Some(_node_registry) = self.service_locator.get_node_registry().await {
            // Get incarnation from local SWIM state - for now return 0
            // In production this would query the SWIM protocol state
            0u64
        } else {
            0u64
        };

        // Get cluster name for same-cluster check (ConnectNodes)
        let cluster_name = self
            .service_locator
            .get_node_config()
            .await
            .map(|c| c.cluster_name)
            .unwrap_or_default();

        // Get piggyback updates to include in response
        let updates = Vec::new(); // Would get from SWIM protocol in full impl

        Ok(Response::new(plexspaces_proto::node::v1::PingResponse {
            node_id: self.local_node_id.clone(),
            sequence_number: req.sequence_number,
            incarnation,
            updates,
            cluster_name,
        }))
    }

    async fn ping_req(
        &self,
        request: Request<plexspaces_proto::node::v1::PingReqRequest>,
    ) -> Result<Response<plexspaces_proto::node::v1::PingReqResponse>, Status> {
        let req = request.into_inner();
        
        debug!(
            "Received ping_req from {} to ping {}",
            req.source_node_id, req.target_node_id
        );
        metrics::counter!("plexspaces_swim_ping_reqs_received").increment(1);

        // Try to ping the target node on behalf of the source
        let target_alive = if !req.target_address.is_empty() {
            // Attempt to ping target - simplified implementation
            // In full impl would use gRPC to ping target_address
            true // Assume alive for now
        } else {
            false
        };

        Ok(Response::new(plexspaces_proto::node::v1::PingReqResponse {
            target_alive,
            target_incarnation: 0,
            updates: Vec::new(),
        }))
    }

    async fn sync_membership(
        &self,
        request: Request<plexspaces_proto::node::v1::SyncMembershipRequest>,
    ) -> Result<Response<plexspaces_proto::node::v1::SyncMembershipResponse>, Status> {
        let req = request.into_inner();
        
        debug!(
            "Received sync_membership from {}, {} members, is_push={}",
            req.source_node_id,
            req.members.len(),
            req.is_push
        );
        metrics::counter!("plexspaces_swim_syncs_received").increment(1);

        // Process incoming membership updates
        let mut updates_applied = 0i32;
        
        // In full implementation, would merge req.members with local SWIM state
        // For now, just acknowledge receipt
        updates_applied = req.members.len() as i32;

        // Get our full membership state to return
        let members = Vec::new(); // Would get from SWIM protocol in full impl

        Ok(Response::new(plexspaces_proto::node::v1::SyncMembershipResponse {
            members,
            updates_applied,
        }))
    }

    // ============================================================================
    // Node Connectivity RPCs - Erlang-style Connect/Disconnect
    // ============================================================================

    /// Connect to remote nodes (Erlang-style net_adm:ping)
    ///
    /// ## Purpose
    /// Establishes connections to remote nodes by address. For each address:
    /// 1. Attempts gRPC Ping to verify node is alive and get node_id
    /// 2. Registers node in NodeRegistry for SWIM protocol membership
    /// 3. SWIM protocol handles ongoing failure detection
    ///
    /// ## Semantics
    /// - Idempotent: connecting to already-connected node succeeds
    /// - Partial success: some connections may succeed while others fail
    /// - Returns map of successful connections (node_id -> address)
    async fn connect_nodes(
        &self,
        request: Request<ConnectNodesRequest>,
    ) -> Result<Response<ConnectNodesResponse>, Status> {
        let ctx = self.extract_context(&request).await?;
        let req = request.into_inner();
        let start_time = Instant::now();

        info!(
            addresses = ?req.node_addresses,
            cluster = %req.cluster,
            "ConnectNodes request received"
        );

        // Record metrics
        metrics::counter!(
            "plexspaces_node_connect_attempts_total",
            "node_id" => self.local_node_id.clone()
        ).increment(req.node_addresses.len() as u64);

        // Validate request
        if req.node_addresses.is_empty() {
            return Err(Status::invalid_argument("At least one node address is required"));
        }

        // Get timeout (default 5 seconds)
        let timeout = req.timeout
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
            .unwrap_or(Duration::from_secs(5));

        let node_registry = self.service_locator.get_node_registry().await
            .ok_or_else(|| Status::internal("NodeRegistry not available"))?;

        // Local cluster name for same-cluster check (empty means no cluster)
        let local_cluster = self
            .service_locator
            .get_node_config()
            .await
            .map(|c| c.cluster_name)
            .unwrap_or_default();

        let mut connected: HashMap<String, String> = HashMap::new();
        let mut failed: HashMap<String, String> = HashMap::new();

        // Connect to each node in parallel
        let mut handles = Vec::new();
        for address in req.node_addresses.iter() {
            let address = address.clone();
            let address_for_task = address.clone();
            let timeout = timeout;
            let local_node_id = self.local_node_id.clone();

            let handle = tokio::spawn(async move {
                Self::try_connect_to_node(&address_for_task, &local_node_id, timeout).await
            });
            handles.push((address, handle));
        }

        // Collect results
        for (address, handle) in handles {
            match handle.await {
                Ok(Ok((node_id, node_address, remote_cluster))) => {
                    // Same-cluster check: only connect if both have same cluster (empty matches empty)
                    if !Self::same_cluster_ok(&local_cluster, &remote_cluster) {
                        warn!(
                            address = %address,
                            local_cluster = %local_cluster,
                            remote_cluster = %remote_cluster,
                            "Rejecting connection: cluster mismatch"
                        );
                        failed.insert(address, "cluster mismatch".to_string());
                        metrics::counter!(
                            "plexspaces_node_connect_failures_total",
                            "node_id" => self.local_node_id.clone(),
                            "reason" => "cluster_mismatch"
                        ).increment(1);
                        continue;
                    }
                    // Register the node in NodeRegistry (set cluster in capabilities so list_nodes and SWIM filter align)
                    let mut capabilities = HashMap::new();
                    if !remote_cluster.is_empty() {
                        capabilities.insert("cluster".to_string(), remote_cluster.clone());
                    }
                    let registration = NodeRegistration {
                        node_id: node_id.clone(),
                        node_address: node_address.clone(),
                        capabilities,
                        status: NodeStatus::NodeStatusReady as i32,
                        last_heartbeat: Some(Timestamp {
                            seconds: SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64,
                            nanos: 0,
                        }),
                        actor_count: 0,
                        message_count: 0,
                        error_count: 0,
                        registered_at: Some(Timestamp {
                            seconds: SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64,
                            nanos: 0,
                        }),
                    };

                    match node_registry.register_node(&ctx, registration).await {
                        Ok(()) => {
                            info!(node_id = %node_id, address = %node_address, "Successfully connected to node");
                            connected.insert(node_id, node_address);
                            metrics::counter!(
                                "plexspaces_node_connect_success_total",
                                "node_id" => self.local_node_id.clone()
                            ).increment(1);
                        }
                        Err(e) => {
                            warn!(address = %address, error = %e, "Failed to register node after successful ping");
                            failed.insert(address, format!("Registration failed: {}", e));
                            metrics::counter!(
                                "plexspaces_node_connect_failures_total",
                                "node_id" => self.local_node_id.clone(),
                                "reason" => "registration_failed"
                            ).increment(1);
                        }
                    }
                }
                Ok(Err(e)) => {
                    warn!(address = %address, error = %e, "Failed to connect to node");
                    failed.insert(address, e);
                    metrics::counter!(
                        "plexspaces_node_connect_failures_total",
                        "node_id" => self.local_node_id.clone(),
                        "reason" => "connection_failed"
                    ).increment(1);
                }
                Err(e) => {
                    warn!(address = %address, error = %e, "Task panicked while connecting");
                    failed.insert(address, format!("Task panicked: {}", e));
                    metrics::counter!(
                        "plexspaces_node_connect_failures_total",
                        "node_id" => self.local_node_id.clone(),
                        "reason" => "task_panic"
                    ).increment(1);
                }
            }
        }

        let elapsed = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_node_connect_duration_seconds",
            "node_id" => self.local_node_id.clone()
        ).record(elapsed.as_secs_f64());

        info!(
            connected_count = connected.len(),
            failed_count = failed.len(),
            elapsed_ms = elapsed.as_millis(),
            "ConnectNodes completed"
        );

        Ok(Response::new(ConnectNodesResponse {
            connected,
            failed,
            total_time: Some(prost_types::Duration {
                seconds: elapsed.as_secs() as i64,
                nanos: elapsed.subsec_nanos() as i32,
            }),
        }))
    }

    /// Disconnect from nodes (Erlang-style erlang:disconnect_node)
    ///
    /// ## Purpose
    /// Removes nodes from the local node's membership. The disconnected nodes:
    /// 1. Are marked as 'Left' in SWIM protocol
    /// 2. Are removed from NodeRegistry
    /// 3. Will no longer receive gossip or be probed
    ///
    /// ## Semantics
    /// - Idempotent: disconnecting from unknown node succeeds
    /// - Does NOT notify remote node (they will detect via SWIM timeout)
    /// - Partial success: some disconnections may succeed while others fail
    async fn disconnect_nodes(
        &self,
        request: Request<DisconnectNodesRequest>,
    ) -> Result<Response<DisconnectNodesResponse>, Status> {
        let ctx = self.extract_context(&request).await?;
        let req = request.into_inner();

        info!(
            node_ids = ?req.node_ids,
            notify_remote = req.notify_remote,
            "DisconnectNodes request received"
        );

        // Record metrics
        metrics::counter!(
            "plexspaces_node_disconnect_attempts_total",
            "node_id" => self.local_node_id.clone()
        ).increment(req.node_ids.len() as u64);

        // Validate request
        if req.node_ids.is_empty() {
            return Err(Status::invalid_argument("At least one node_id is required"));
        }

        let node_registry = self.service_locator.get_node_registry().await
            .ok_or_else(|| Status::internal("NodeRegistry not available"))?;

        let mut disconnected: Vec<String> = Vec::new();
        let mut failed: HashMap<String, String> = HashMap::new();

        for node_id in req.node_ids {
            // Optionally notify remote node before disconnecting
            if req.notify_remote {
                // Look up node address first
                if let Ok(Some(node_info)) = node_registry.lookup_node(&ctx, &node_id).await {
                    // Best-effort notification - don't fail if this doesn't work
                    if let Err(e) = Self::notify_disconnect(&node_info.node_address, &self.local_node_id).await {
                        debug!(
                            node_id = %node_id,
                            error = %e,
                            "Failed to notify remote node of disconnect (continuing anyway)"
                        );
                    }
                }
            }

            // Unregister from NodeRegistry
            match node_registry.unregister_node(&ctx, &node_id).await {
                Ok(()) => {
                    info!(node_id = %node_id, "Successfully disconnected from node");
                    disconnected.push(node_id);
                    metrics::counter!(
                        "plexspaces_node_disconnect_success_total",
                        "node_id" => self.local_node_id.clone()
                    ).increment(1);
                }
                Err(e) => {
                    // Check if it's a "not found" error - treat as success (idempotent)
                    let error_str = e.to_string();
                    if error_str.contains("not found") || error_str.contains("NotFound") {
                        info!(node_id = %node_id, "Node already disconnected (idempotent success)");
                        disconnected.push(node_id);
                        metrics::counter!(
                            "plexspaces_node_disconnect_success_total",
                            "node_id" => self.local_node_id.clone()
                        ).increment(1);
                    } else {
                        warn!(node_id = %node_id, error = %e, "Failed to disconnect from node");
                        failed.insert(node_id, e.to_string());
                        metrics::counter!(
                            "plexspaces_node_disconnect_failures_total",
                            "node_id" => self.local_node_id.clone()
                        ).increment(1);
                    }
                }
            }
        }

        info!(
            disconnected_count = disconnected.len(),
            failed_count = failed.len(),
            "DisconnectNodes completed"
        );

        Ok(Response::new(DisconnectNodesResponse {
            disconnected,
            failed,
        }))
    }
}

impl NodeServiceImpl {
    /// Returns true if local and remote cluster match (same-cluster rule for ConnectNodes).
    /// Empty cluster matches empty; non-empty must match exactly.
    fn same_cluster_ok(local: &str, remote: &str) -> bool {
        local == remote
    }

    /// Try to connect to a remote node by pinging it
    ///
    /// ## Returns
    /// - Ok((node_id, address, remote_cluster_name)) on success
    /// - Err(error_message) on failure
    async fn try_connect_to_node(
        address: &str,
        local_node_id: &str,
        timeout: Duration,
    ) -> Result<(String, String, String), String> {
        // Normalize address to include scheme if not present
        let endpoint = if address.starts_with("http://") || address.starts_with("https://") {
            address.to_string()
        } else {
            format!("http://{}", address)
        };

        // Create gRPC client with timeout
        let channel = match tokio::time::timeout(
            timeout,
            Channel::from_shared(endpoint.clone())
                .map_err(|e| format!("Invalid endpoint: {}", e))?
                .connect(),
        ).await {
            Ok(Ok(channel)) => channel,
            Ok(Err(e)) => return Err(format!("Connection failed: {}", e)),
            Err(_) => return Err(format!("Connection timed out after {:?}", timeout)),
        };

        let mut client = NodeServiceClient::new(channel);

        // Send Ping request
        let ping_request = PingRequest {
            source_node_id: local_node_id.to_string(),
            sequence_number: 1,
            updates: Vec::new(),
        };

        match tokio::time::timeout(timeout, client.ping(ping_request)).await {
            Ok(Ok(response)) => {
                let ping_response = response.into_inner();
                let remote_cluster = ping_response.cluster_name.clone();
                Ok((ping_response.node_id, endpoint, remote_cluster))
            }
            Ok(Err(e)) => Err(format!("Ping failed: {}", e)),
            Err(_) => Err(format!("Ping timed out after {:?}", timeout)),
        }
    }

    /// Notify a remote node that we are disconnecting (best-effort)
    async fn notify_disconnect(remote_address: &str, local_node_id: &str) -> Result<(), String> {
        // Normalize address
        let endpoint = if remote_address.starts_with("http://") || remote_address.starts_with("https://") {
            remote_address.to_string()
        } else {
            format!("http://{}", remote_address)
        };

        let channel = Channel::from_shared(endpoint)
            .map_err(|e| format!("Invalid endpoint: {}", e))?
            .connect()
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;

        let mut client = NodeServiceClient::new(channel);

        // Send disconnect notification via a ping with special marker
        // In a full implementation, we might have a dedicated "leave" RPC
        let ping_request = PingRequest {
            source_node_id: local_node_id.to_string(),
            sequence_number: 0, // Special sequence number indicating disconnect
            updates: Vec::new(),
        };

        client.ping(ping_request).await
            .map_err(|e| format!("Notification failed: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use plexspaces_proto::node::v1::ReleaseSpec;
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};

    #[test]
    fn test_node_service_metrics_default() {
        let metrics = NodeServiceMetrics::default();
        assert_eq!(metrics.messages_routed.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(metrics.local_deliveries.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(metrics.remote_deliveries.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(metrics.failed_deliveries.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn test_node_service_metrics_increment() {
        let metrics = NodeServiceMetrics::default();
        metrics.messages_routed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        metrics.local_deliveries.fetch_add(2, std::sync::atomic::Ordering::Relaxed);
        metrics.remote_deliveries.fetch_add(3, std::sync::atomic::Ordering::Relaxed);
        metrics.failed_deliveries.fetch_add(4, std::sync::atomic::Ordering::Relaxed);
        
        assert_eq!(metrics.messages_routed.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(metrics.local_deliveries.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert_eq!(metrics.remote_deliveries.load(std::sync::atomic::Ordering::Relaxed), 3);
        assert_eq!(metrics.failed_deliveries.load(std::sync::atomic::Ordering::Relaxed), 4);
    }

    #[test]
    fn test_node_service_metrics_uptime() {
        let metrics = NodeServiceMetrics::default();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let uptime_ms = metrics.start_time.elapsed().as_millis();
        assert!(uptime_ms >= 10);
    }

    #[tokio::test]
    async fn test_node_service_impl_new() {
        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        let node_id = "test-node-1".to_string();
        
        let service = NodeServiceImpl::new(service_locator.clone(), node_id.clone());
        
        assert_eq!(service.local_node_id, node_id);
        assert_eq!(service.uptime_seconds(), 0); // Just created, uptime is 0
    }

    #[tokio::test]
    async fn test_node_service_with_release_spec() {
        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        let node_id = "test-node-2".to_string();
        
        let release_spec = ReleaseSpec {
            version: "1.0.0".to_string(),
            name: "test-release".to_string(),
            ..Default::default()
        };
        
        let service = NodeServiceImpl::with_release_spec(
            service_locator.clone(), 
            node_id.clone(),
            release_spec.clone(),
        );
        
        assert_eq!(service.local_node_id, node_id);
        
        // Test get_release_spec_internal returns the spec
        let spec = service.get_release_spec_internal().await;
        assert!(spec.is_some());
        let retrieved_spec = spec.unwrap();
        assert_eq!(retrieved_spec.version, "1.0.0");
        assert_eq!(retrieved_spec.name, "test-release");
    }

    #[tokio::test]
    async fn test_node_service_set_release_spec() {
        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        // Initially no spec
        assert!(service.get_release_spec_internal().await.is_none());
        
        // Set a spec
        let spec = ReleaseSpec {
            version: "2.0.0".to_string(),
            name: "updated-release".to_string(),
            ..Default::default()
        };
        service.set_release_spec(spec).await;
        
        // Now spec exists
        let retrieved = service.get_release_spec_internal().await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().version, "2.0.0");
    }

    #[tokio::test]
    async fn test_node_service_increment_metrics() {
        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        // Increment each metric type
        service.increment_messages_routed();
        service.increment_messages_routed();
        service.increment_local_deliveries();
        service.increment_remote_deliveries();
        service.increment_failed_deliveries();
        
        // Get stats
        let (routed, local, remote, failed) = service.routing_stats();
        
        assert_eq!(routed, 2);
        assert_eq!(local, 1);
        assert_eq!(remote, 1);
        assert_eq!(failed, 1);
    }

    #[tokio::test]
    async fn test_node_service_uptime() {
        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        // Initial uptime should be 0
        assert_eq!(service.uptime_seconds(), 0);
        
        // Wait a bit
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        
        // Now uptime should be at least 1
        assert!(service.uptime_seconds() >= 1);
    }

    #[tokio::test]
    async fn test_calculate_capacity_internal() {
        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        let capacity = service.calculate_capacity_internal().await;
        
        // Should have reasonable values
        let total = capacity.total.expect("Should have total resources");
        let available = capacity.available.expect("Should have available resources");
        assert!(total.cpu_cores > 0.0);
        assert!(total.memory_bytes > 0);
        assert!(available.cpu_cores >= 0.0);
        assert!(available.memory_bytes >= 0);
    }

    #[tokio::test]
    async fn test_get_metrics_internal() {
        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        // Increment some metrics first
        service.increment_messages_routed();
        service.increment_local_deliveries();
        
        let metrics = service.get_metrics_internal().await;
        
        assert_eq!(metrics.node_id, "test-node");
        assert_eq!(metrics.messages_routed, 1);
        assert_eq!(metrics.local_deliveries, 1);
        assert!(metrics.memory_available_bytes > 0 || metrics.memory_used_bytes > 0);
    }

    // ============================================================================
    // ConnectNodes / DisconnectNodes Tests
    // ============================================================================

    #[tokio::test]
    async fn test_try_connect_to_node_invalid_address() {
        // Test with invalid address format
        let result = NodeServiceImpl::try_connect_to_node(
            "invalid-no-port",
            "local-node",
            Duration::from_millis(100),
        ).await;
        
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Invalid endpoint") || err.contains("Connection failed") || err.contains("timed out"));
    }

    #[tokio::test]
    async fn test_try_connect_to_node_connection_timeout() {
        // Test with unreachable address - should timeout
        let result = NodeServiceImpl::try_connect_to_node(
            "192.0.2.1:9999", // TEST-NET-1, guaranteed unreachable
            "local-node",
            Duration::from_millis(100),
        ).await;
        
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Should be either timeout or connection error
        assert!(
            err.contains("timed out") || err.contains("Connection failed"),
            "Expected timeout or connection error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_try_connect_to_node_with_scheme() {
        // Test that addresses with http:// scheme work
        let result = NodeServiceImpl::try_connect_to_node(
            "http://192.0.2.1:9999",
            "local-node",
            Duration::from_millis(100),
        ).await;
        
        // Should fail but not due to invalid endpoint
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            !err.contains("Invalid endpoint"),
            "Should accept http:// scheme, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_connect_nodes_empty_addresses() {
        use tonic::Request;

        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        register_test_security_config_disable_auth(&service_locator).await;
        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        let request = Request::new(ConnectNodesRequest {
            node_addresses: vec![],
            cluster: String::new(),
            timeout: None,
        });
        
        let result = service.connect_nodes(request).await;
        
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("At least one node address"));
    }

    #[tokio::test]
    async fn test_connect_nodes_no_registry() {
        use tonic::Request;

        // Create service without registering NodeRegistry
        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        register_test_security_config_disable_auth(&service_locator).await;
        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        let request = Request::new(ConnectNodesRequest {
            node_addresses: vec!["192.0.2.1:8000".to_string()],
            cluster: String::new(),
            timeout: Some(prost_types::Duration { seconds: 0, nanos: 100_000_000 }), // 100ms
        });
        
        let result = service.connect_nodes(request).await;
        
        // Should fail because NodeRegistry is not available
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::Internal);
        assert!(status.message().contains("NodeRegistry not available"));
    }

    /// Registers a SecurityConfig with auth disabled so extract_context succeeds in unit tests.
    async fn register_test_security_config_disable_auth(
        service_locator: &Arc<crate::service_locator::ServiceLocatorImpl>,
    ) {
        let config = plexspaces_proto::node::v1::SecurityConfig {
            disable_auth: true,
            ..Default::default()
        };
        service_locator.register_security_config(config).await;
    }

    /// Helper to create a test NodeRegistry with SQLite backend
    async fn create_test_node_registry(node_id: &str) -> Arc<crate::node_registry::NodeRegistry> {
        use crate::node_registry::NodeRegistry;
        use plexspaces_core::ObjectRegistry as ObjectRegistryTrait;
        
        let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
        let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryImpl::new(object_repo));
        
        Arc::new(NodeRegistry::new_simple(
            object_registry,
            node_id.to_string(),
            Some(60), // cache_ttl_seconds
            Some(false), // gossip_enabled (disabled for tests)
            None,
            None,
        ))
    }

    #[tokio::test]
    async fn test_connect_nodes_unreachable_addresses() {
        use tonic::Request;

        // Create service with NodeRegistry
        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        register_test_security_config_disable_auth(&service_locator).await;
        let node_registry = create_test_node_registry("test-node").await;
        service_locator.register_node_registry(node_registry).await;

        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        let request = Request::new(ConnectNodesRequest {
            node_addresses: vec![
                "192.0.2.1:8000".to_string(), // TEST-NET-1, unreachable
                "192.0.2.2:8000".to_string(),
            ],
            cluster: "test-cluster".to_string(),
            timeout: Some(prost_types::Duration { seconds: 0, nanos: 200_000_000 }), // 200ms
        });
        
        let result = service.connect_nodes(request).await;
        
        // Should succeed but with all addresses in failed map
        assert!(result.is_ok());
        let response = result.unwrap().into_inner();
        
        assert!(response.connected.is_empty(), "No nodes should connect to unreachable addresses");
        assert_eq!(response.failed.len(), 2, "Both addresses should fail");
        assert!(response.total_time.is_some());
    }

    #[tokio::test]
    async fn test_disconnect_nodes_empty_node_ids() {
        use tonic::Request;

        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        register_test_security_config_disable_auth(&service_locator).await;
        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        let request = Request::new(DisconnectNodesRequest {
            node_ids: vec![],
            notify_remote: false,
        });
        
        let result = service.disconnect_nodes(request).await;
        
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("At least one node_id"));
    }

    #[tokio::test]
    async fn test_disconnect_nodes_no_registry() {
        use tonic::Request;

        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        register_test_security_config_disable_auth(&service_locator).await;
        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        let request = Request::new(DisconnectNodesRequest {
            node_ids: vec!["node-1".to_string()],
            notify_remote: false,
        });
        
        let result = service.disconnect_nodes(request).await;
        
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::Internal);
        assert!(status.message().contains("NodeRegistry not available"));
    }

    #[tokio::test]
    async fn test_disconnect_nodes_unknown_node_idempotent() {
        use tonic::Request;

        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        register_test_security_config_disable_auth(&service_locator).await;
        let node_registry = create_test_node_registry("test-node").await;
        service_locator.register_node_registry(node_registry).await;

        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        // Try to disconnect a node that doesn't exist
        let request = Request::new(DisconnectNodesRequest {
            node_ids: vec!["nonexistent-node".to_string()],
            notify_remote: false,
        });
        
        let result = service.disconnect_nodes(request).await;
        
        // Should succeed (idempotent behavior)
        assert!(result.is_ok());
        let response = result.unwrap().into_inner();
        
        // Either in disconnected (idempotent success) or failed
        // The exact behavior depends on NodeRegistry implementation
        assert!(
            response.disconnected.contains(&"nonexistent-node".to_string()) ||
            response.failed.contains_key("nonexistent-node"),
            "Node should be in either disconnected or failed list"
        );
    }

    #[tokio::test]
    async fn test_disconnect_nodes_with_registered_node() {
        use tonic::Request;
        use plexspaces_core::{RequestContext, NodeRegistryTrait};

        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        register_test_security_config_disable_auth(&service_locator).await;
        let node_registry = create_test_node_registry("test-node").await;
        service_locator.register_node_registry(node_registry.clone()).await;
        
        // First register a node
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());
        let registration = NodeRegistration {
            node_id: "remote-node-1".to_string(),
            node_address: "http://remote:8000".to_string(),
            capabilities: HashMap::new(),
            status: NodeStatus::NodeStatusReady as i32,
            last_heartbeat: None,
            actor_count: 0,
            message_count: 0,
            error_count: 0,
            registered_at: None,
        };
        let _ = node_registry.register_node(&ctx, registration).await;
        
        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        // Now disconnect it
        let request = Request::new(DisconnectNodesRequest {
            node_ids: vec!["remote-node-1".to_string()],
            notify_remote: false,
        });
        
        let result = service.disconnect_nodes(request).await;
        
        assert!(result.is_ok());
        let response = result.unwrap().into_inner();
        
        // Should be in disconnected list
        assert!(
            response.disconnected.contains(&"remote-node-1".to_string()),
            "Node should be disconnected, got: {:?}",
            response
        );
        assert!(response.failed.is_empty());
    }

    #[tokio::test]
    async fn test_connect_nodes_timeout_configuration() {
        use tonic::Request;

        let service_locator = Arc::new(crate::service_locator::ServiceLocatorImpl::new());
        register_test_security_config_disable_auth(&service_locator).await;
        let node_registry = create_test_node_registry("test-node").await;
        service_locator.register_node_registry(node_registry).await;

        let service = NodeServiceImpl::new(service_locator, "test-node".to_string());
        
        // Test with very short timeout
        let start = Instant::now();
        let request = Request::new(ConnectNodesRequest {
            node_addresses: vec!["192.0.2.1:8000".to_string()],
            cluster: String::new(),
            timeout: Some(prost_types::Duration { seconds: 0, nanos: 50_000_000 }), // 50ms
        });
        
        let result = service.connect_nodes(request).await;
        let elapsed = start.elapsed();
        
        assert!(result.is_ok());
        // Should complete relatively quickly (within 500ms including overhead)
        assert!(
            elapsed < Duration::from_millis(500),
            "Should timeout quickly, took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_connect_nodes_request_validation() {
        // Test request structure
        let request = ConnectNodesRequest {
            node_addresses: vec!["node1:8000".to_string(), "node2:8000".to_string()],
            cluster: "test-cluster".to_string(),
            timeout: Some(prost_types::Duration { seconds: 10, nanos: 0 }),
        };
        
        assert_eq!(request.node_addresses.len(), 2);
        assert_eq!(request.cluster, "test-cluster");
        assert!(request.timeout.is_some());
    }

    #[test]
    fn test_disconnect_nodes_request_validation() {
        // Test request structure
        let request = DisconnectNodesRequest {
            node_ids: vec!["node-1".to_string(), "node-2".to_string()],
            notify_remote: true,
        };
        
        assert_eq!(request.node_ids.len(), 2);
        assert!(request.notify_remote);
    }

    #[test]
    fn test_connect_nodes_response_structure() {
        // Test response structure
        let mut connected = HashMap::new();
        connected.insert("node-1".to_string(), "http://node1:8000".to_string());
        
        let mut failed = HashMap::new();
        failed.insert("node2:8000".to_string(), "Connection refused".to_string());
        
        let response = ConnectNodesResponse {
            connected,
            failed,
            total_time: Some(prost_types::Duration { seconds: 1, nanos: 500_000_000 }),
        };
        
        assert_eq!(response.connected.len(), 1);
        assert_eq!(response.failed.len(), 1);
        assert!(response.total_time.is_some());
    }

    #[test]
    fn test_disconnect_nodes_response_structure() {
        // Test response structure
        let mut failed = HashMap::new();
        failed.insert("node-3".to_string(), "Internal error".to_string());
        
        let response = DisconnectNodesResponse {
            disconnected: vec!["node-1".to_string(), "node-2".to_string()],
            failed,
        };
        
        assert_eq!(response.disconnected.len(), 2);
        assert_eq!(response.failed.len(), 1);
    }

    #[tokio::test]
    async fn test_notify_disconnect_unreachable() {
        // Test notify_disconnect with unreachable address
        let result = NodeServiceImpl::notify_disconnect(
            "192.0.2.1:9999",
            "local-node",
        ).await;
        
        // Should fail but not panic
        assert!(result.is_err());
    }

    // ============================================================================
    // Same-cluster rule (ConnectNodes only connects nodes with same cluster)
    // ============================================================================

    #[test]
    fn test_same_cluster_ok_both_empty() {
        assert!(NodeServiceImpl::same_cluster_ok("", ""));
    }

    #[test]
    fn test_same_cluster_ok_same_non_empty() {
        assert!(NodeServiceImpl::same_cluster_ok("production", "production"));
        assert!(NodeServiceImpl::same_cluster_ok("test-cluster", "test-cluster"));
    }

    #[test]
    fn test_same_cluster_ok_mismatch_empty_vs_non_empty() {
        assert!(!NodeServiceImpl::same_cluster_ok("", "production"));
        assert!(!NodeServiceImpl::same_cluster_ok("production", ""));
    }

    #[test]
    fn test_same_cluster_ok_mismatch_different_clusters() {
        assert!(!NodeServiceImpl::same_cluster_ok("production", "staging"));
        assert!(!NodeServiceImpl::same_cluster_ok("cluster-a", "cluster-b"));
    }
}
