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

use std::pin::Pin;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use async_trait::async_trait;
use futures::Stream;
use prost_types::Timestamp;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::{debug, info};

use plexspaces_core::{RequestContext, ServiceLocator, mask_release_spec};
use plexspaces_proto::node::v1::{
    node_service_server::NodeService as NodeServiceTrait,
    CalculateCapacityRequest, GetHealthRequest, GetHealthResponse, GetMetricsRequest,
    GetReleaseSpecRequest, GetReleaseSpecResponse, ListConnectedNodesRequest,
    ListConnectedNodesResponse, ListNodeApplicationsRequest, ListNodeApplicationsResponse,
    NodeApplicationInfo, NodeCapacity, NodeHealthStatus, NodeMetrics, NodeRegistration,
    RegisterNodesRequest, RegisterNodesResponse, ReleaseSpec, SendHeartbeatRequest, 
    SendHeartbeatResponse, StreamConnectedNodesRequest, UnregisterNodeRequest, 
    UnregisterNodeResponse,
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
        
        debug!("Received ping from node: {}", req.source_node_id);
        metrics::counter!("plexspaces_swim_pings_received").increment(1);

        // Get our incarnation from SWIM protocol
        let incarnation = if let Some(_node_registry) = self.service_locator.get_node_registry().await {
            // Get incarnation from local SWIM state - for now return 0
            // In production this would query the SWIM protocol state
            0u64
        } else {
            0u64
        };

        // Get piggyback updates to include in response
        let updates = Vec::new(); // Would get from SWIM protocol in full impl

        Ok(Response::new(plexspaces_proto::node::v1::PingResponse {
            node_id: self.local_node_id.clone(),
            sequence_number: req.sequence_number,
            incarnation,
            updates,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use plexspaces_proto::node::v1::ReleaseSpec;

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
}
