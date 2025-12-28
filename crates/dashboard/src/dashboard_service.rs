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

//! Dashboard Service Implementation
//!
//! ## Purpose
//! Provides aggregated metrics and metadata for dashboard visualization.
//! Aggregates data from all nodes in the cluster with filtering and pagination support.
//!
//! ## Architecture Context
//! - Aggregates metrics from local node and remote nodes (via ServiceLocator)
//! - Supports tenant filtering (admin vs non-admin)
//! - Provides pagination for large datasets
//! - Production-ready error handling and validation

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use chrono::{DateTime, Utc};
use tonic::{Request, Response, Status};
use prost_types::Timestamp;

use plexspaces_core::{ServiceLocator, ActorRegistry, RequestContext, ActorId, FacetManagerServiceWrapper, ApplicationManager};
use plexspaces_facet::FacetManager;
use plexspaces_object_registry::ObjectRegistry;
use plexspaces_proto::dashboard::v1::{
    dashboard_service_server::DashboardService,
    GetSummaryRequest, GetSummaryResponse,
    GetNodesRequest, GetNodesResponse,
    GetNodeDashboardRequest, GetNodeDashboardResponse,
    GetApplicationsRequest, GetApplicationsResponse,
    GetActorsRequest, GetActorsResponse,
    GetWorkflowsRequest, GetWorkflowsResponse,
    GetDependencyHealthRequest, GetDependencyHealthResponse,
    ActorInfo, NodeSummaryMetrics, WorkflowInfo,
};
use plexspaces_proto::system::v1::DetailedHealthCheck;
use plexspaces_proto::node::v1::{
    Node as ProtoNode, NodeType, NodeStatus, NodeMetrics as ProtoNodeMetrics,
};
use plexspaces_proto::application::v1::{
    ApplicationInfo,
    application_service_client::ApplicationServiceClient,
};
use plexspaces_proto::workflow::v1::{
    WorkflowExecution, WorkflowDefinition, ExecutionStatus,
};
use plexspaces_proto::common::v1::{PageRequest, PageResponse};
use plexspaces_proto::metrics::v1::{SystemMetrics, ActorMetrics};

/// Trait for accessing detailed health information
/// 
/// ## Purpose
/// Abstracts health reporter access to avoid circular dependency between dashboard and node crates.
/// Dashboard can use this trait to get dependency health without importing node crate.
#[async_trait::async_trait]
pub trait HealthReporterAccess: Send + Sync {
    /// Get detailed health check with dependency information
    async fn get_detailed_health(&self, include_non_critical: bool) -> DetailedHealthCheck;
}

/// Dashboard Service implementation
pub struct DashboardServiceImpl {
    /// Service locator for accessing all services
    service_locator: Arc<ServiceLocator>,
    
    /// Optional health reporter access (to avoid circular dependency)
    health_reporter_access: Option<Arc<dyn HealthReporterAccess>>,
}

impl DashboardServiceImpl {
    /// Create new dashboard service
    pub fn new(service_locator: Arc<ServiceLocator>) -> Self {
        Self {
            service_locator,
            health_reporter_access: None,
        }
    }
    
    /// Create new dashboard service with health reporter access
    pub fn with_health_reporter(
        service_locator: Arc<ServiceLocator>,
        health_reporter_access: Arc<dyn HealthReporterAccess>,
    ) -> Self {
        Self {
            service_locator,
            health_reporter_access: Some(health_reporter_access),
        }
    }

    /// Get tenant ID from request context (for filtering)
    fn get_tenant_id_from_context(&self, request: &Request<()>) -> Option<String> {
        // Extract tenant_id from request metadata (set by auth middleware)
        request.metadata()
            .get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    /// Check if user is admin (from request context)
    fn is_admin(&self, request: &Request<()>) -> bool {
        // Check for admin role in metadata (set by auth middleware)
        request.metadata()
            .get("x-user-role")
            .and_then(|v| v.to_str().ok())
            .map(|s| s == "admin")
            .unwrap_or(true) // Default to admin if no auth (development mode)
    }

    /// Get default "since" timestamp (now - 24 hours)
    fn default_since() -> Timestamp {
        let now = Utc::now();
        let since = now - chrono::Duration::hours(24);
        Timestamp {
            seconds: since.timestamp(),
            nanos: since.timestamp_subsec_nanos() as i32,
        }
    }

    /// Convert Node to ProtoNode
    async fn node_to_proto(&self) -> Result<ProtoNode, Status> {
        // Get NodeMetricsAccessor from ServiceLocator
        let metrics_accessor = self.service_locator.get_node_metrics_accessor().await
            .ok_or_else(|| Status::internal("NodeMetricsAccessor not registered in ServiceLocator"))?;
        
        let metrics = metrics_accessor.get_metrics().await;
        
        Ok(ProtoNode {
            id: metrics.node_id.clone(),
            node_type: NodeType::NodeTypeProcess as i32,
            status: NodeStatus::NodeStatusReady as i32,
            capabilities: None,
            metadata: None,
            created_at: None,
            last_heartbeat: Some(Timestamp {
                seconds: Utc::now().timestamp(),
                nanos: Utc::now().timestamp_subsec_nanos() as i32,
            }),
            config: None,
            actor_ids: vec![],
            metrics: Some(ProtoNodeMetrics {
                node_id: metrics.node_id.clone(),
                cluster_name: metrics.cluster_name.clone(),
                memory_used_bytes: metrics.memory_used_bytes,
                memory_available_bytes: metrics.memory_available_bytes,
                cpu_usage_percent: metrics.cpu_usage_percent,
                uptime_seconds: metrics.uptime_seconds,
                messages_routed: metrics.messages_routed,
                local_deliveries: metrics.local_deliveries,
                remote_deliveries: metrics.remote_deliveries,
                failed_deliveries: metrics.failed_deliveries,
                messages_processed: metrics.messages_processed,
                active_actors: metrics.active_actors,
                actor_count: metrics.actor_count,
                connected_nodes: metrics.connected_nodes,
            }),
            mtls_identity: None,
            public_certificate: vec![],
            auto_generate_certs: false,
            cluster_name: metrics.cluster_name.clone(),
        })
    }

    /// Get dependency health status
    async fn get_dependency_health_internal(&self, include_non_critical: bool) -> DetailedHealthCheck {
        // Use health reporter access if available
        if let Some(health_access) = &self.health_reporter_access {
            health_access.get_detailed_health(include_non_critical).await
        } else {
            // Fallback: return empty health check if health reporter not available
            DetailedHealthCheck {
                overall_status: plexspaces_proto::system::v1::HealthStatus::HealthStatusUnhealthy as i32,
                component_checks: vec![],
                dependency_checks: vec![],
                critical_dependencies_healthy: false,
                non_critical_dependencies_healthy: false,
            }
        }
    }

    /// Get system metrics from node
    async fn get_system_metrics(&self) -> Option<SystemMetrics> {
        use sysinfo::System;
        use plexspaces_proto::metrics::v1::{CpuMetrics, MemoryMetrics, DiskMetrics, NetworkMetrics, ComponentMetrics};

        let mut system = System::new();
        system.refresh_all();

        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let available_memory = system.available_memory();
        let cpu_count = system.cpus().len() as u32;
        let cpu_usage = if cpu_count > 0 {
            system.cpus().iter().map(|cpu| cpu.cpu_usage() as f64).sum::<f64>() / cpu_count as f64
        } else {
            0.0
        };

        // Get actors by type using actor_type_index
        let mut active_actors_by_type = HashMap::new();
        if let Some(actor_registry) = self.service_locator.get_service::<ActorRegistry>().await {
            let index = actor_registry.actor_type_index().read().await;
            for ((_tenant, _namespace, actor_type), actor_ids) in index.iter() {
                *active_actors_by_type.entry(actor_type.clone()).or_insert(0) += actor_ids.len() as u32;
            }
            
            // Also count actors without type (registered but not in index)
            let registered_ids = actor_registry.registered_actor_ids().read().await;
            let mut typed_actors = HashSet::new();
            for actor_ids in index.values() {
                for actor_id in actor_ids {
                    typed_actors.insert(actor_id.clone());
                }
            }
            let untyped_count = registered_ids.len().saturating_sub(typed_actors.len());
            if untyped_count > 0 {
                *active_actors_by_type.entry("unknown".to_string()).or_insert(0) += untyped_count as u32;
            }
        }

        Some(SystemMetrics {
            timestamp: Some(Timestamp {
                seconds: Utc::now().timestamp(),
                nanos: Utc::now().timestamp_subsec_nanos() as i32,
            }),
            cpu: Some(CpuMetrics {
                usage_percent: cpu_usage,
                load_average_1m: 0.0, // sysinfo doesn't provide this
                load_average_5m: 0.0,
                load_average_15m: 0.0,
                active_processes: system.processes().len() as i32,
            }),
            memory: Some(MemoryMetrics {
                total_mb: total_memory / (1024 * 1024),
                used_mb: used_memory / (1024 * 1024),
                free_mb: available_memory / (1024 * 1024),
                cached_mb: 0, // sysinfo doesn't provide this separately
                usage_percent: if total_memory > 0 {
                    (used_memory as f64 / total_memory as f64) * 100.0
                } else {
                    0.0
                },
            }),
            disk: {
                use sysinfo::Disks;
                let disks = Disks::new_with_refreshed_list();
                let total_disk_bytes: u64 = disks.iter().map(|d| d.total_space()).sum();
                let available_disk_bytes: u64 = disks.iter().map(|d| d.available_space()).sum();
                let used_disk_bytes = total_disk_bytes.saturating_sub(available_disk_bytes);
                
                if total_disk_bytes > 0 {
                    Some(DiskMetrics {
                        total_gb: total_disk_bytes / (1024 * 1024 * 1024),
                        used_gb: used_disk_bytes / (1024 * 1024 * 1024),
                        free_gb: available_disk_bytes / (1024 * 1024 * 1024),
                        usage_percent: if total_disk_bytes > 0 {
                            (used_disk_bytes as f64 / total_disk_bytes as f64) * 100.0
                        } else {
                            0.0
                        },
                        read_ops_per_sec: 0, // sysinfo doesn't provide this
                        write_ops_per_sec: 0,
                    })
                } else {
                    None
                }
            },
            network: Some(NetworkMetrics {
                bytes_received_per_sec: 0, // sysinfo doesn't provide this
                bytes_sent_per_sec: 0,
                packets_received_per_sec: 0,
                packets_sent_per_sec: 0,
                active_connections: 0,
            }),
            components: Some(ComponentMetrics {
                active_actors_by_type,
                active_vms: 0, // VM registry not yet integrated
                tuplespace_size: 0, // TupleSpace size tracking not yet integrated
                active_subscriptions: 0, // Subscription tracking not yet implemented
                active_transactions: 0, // Transaction tracking not yet implemented
            }),
        })
    }

    /// Get journal size and checkpoint for durable actor
    async fn get_durable_actor_metrics(
        &self,
        actor_id: &ActorId,
    ) -> (u64, Option<Timestamp>) {
        // Check if actor has durability facet
        if let Some(facet_manager_wrapper) = self.service_locator.get_service_by_name::<plexspaces_core::FacetManagerServiceWrapper>(plexspaces_core::service_locator::service_names::FACET_MANAGER).await {
            let facet_manager = facet_manager_wrapper.inner_clone();
            if let Some(facet_container_arc) = facet_manager.get_facets(&actor_id.to_string()).await {
                let facet_container = facet_container_arc.read().await;
                // Check if durability facet is attached using list_facets()
                let facet_types = facet_container.list_facets();
                let has_durability = facet_types.iter().any(|t| t == "durability");
                
                if has_durability {
                    // Journal metrics would require accessing DurabilityFacet's storage backend
                    // This would need a method on DurabilityFacet to expose checkpoint info
                    // For now, return zero values - can be enhanced when DurabilityFacet exposes metrics API
                    return (0, None);
                }
            }
        }
        
        (0, None)
    }

    /// Get actor metrics from ActorRegistry
    async fn get_actor_metrics(&self, actor_id: &ActorId) -> Option<ActorMetrics> {
        if let Some(actor_registry) = self.service_locator.get_service::<ActorRegistry>().await {
            let metrics_handle = actor_registry.actor_metrics();
            let metrics = metrics_handle.read().await;
            
            // Get metrics for this specific actor
            // Note: ActorMetrics in registry is aggregate, not per-actor
            // For per-actor metrics, we'd need to track them separately
            // For now, return aggregate metrics as approximation
            // ActorMetrics is a proto struct with fields, not methods
            Some(ActorMetrics {
                spawn_total: metrics.spawn_total,
                active: if actor_registry.registered_actor_ids().read().await.contains(actor_id) {
                    1
                } else {
                    0
                },
                messages_routed: metrics.messages_routed,
                local_deliveries: metrics.local_deliveries,
                remote_deliveries: metrics.remote_deliveries,
                failed_deliveries: metrics.failed_deliveries,
                error_total: metrics.error_total,
                // Lifecycle metrics (Phase 1-3) - aggregate across all actors
                init_total: metrics.init_total,
                init_errors_total: metrics.init_errors_total,
                terminate_total: metrics.terminate_total,
                terminate_errors_total: metrics.terminate_errors_total,
                exit_handled_total: metrics.exit_handled_total,
                exit_propagated_total: metrics.exit_propagated_total,
                exit_handle_errors_total: metrics.exit_handle_errors_total,
                // Parent-child metrics (Phase 3)
                parent_child_registered_total: metrics.parent_child_registered_total,
                parent_child_unregistered_total: metrics.parent_child_unregistered_total,
            })
        } else {
            None
        }
    }

    /// Parse actor ID to extract namespace and tenant
    fn parse_actor_id(&self, actor_id: &ActorId) -> (String, String) {
        // Actor ID format: "{namespace}/{name}@{node}" or "{name}@{node}"
        let parts: Vec<&str> = actor_id.split('@').collect();
        let name_part = parts[0];
        let name_parts: Vec<&str> = name_part.split('/').collect();
        
        if name_parts.len() == 2 {
            (name_parts[0].to_string(), "default".to_string())
        } else {
            ("default".to_string(), "default".to_string())
        }
    }

    /// Apply pagination to a vector using offset and limit
    fn apply_pagination<T: Clone>(
        items: Vec<T>,
        page_request: Option<PageRequest>,
    ) -> (Vec<T>, PageResponse) {
        // Get offset and limit from page_request, with defaults
        let offset = page_request
            .as_ref()
            .map(|p| p.offset.max(0) as usize)
            .unwrap_or(0);
        let limit = page_request
            .as_ref()
            .map(|p| p.limit.max(1).min(1000) as usize)
            .unwrap_or(50);

        let total_size = items.len();
        let start = offset.min(total_size);
        let end = (start + limit).min(total_size);
        
        let paginated_items = items[start..end].to_vec();
        let has_next = end < total_size;

        let page_response = PageResponse {
            total_size: total_size as i32,
            offset: start as i32,
            limit: limit as i32,
            has_next,
        };

        (paginated_items, page_response)
    }

    /// Query remote nodes via ObjectRegistry
    async fn query_remote_nodes(
        &self,
        tenant_id: Option<String>,
        cluster_id: Option<String>,
    ) -> Result<Vec<ProtoNode>, Status> {
        // Get ObjectRegistry to discover remote nodes
        // Try get_service_by_name first (as it's registered by name)
        use plexspaces_core::service_locator::service_names;
        let object_registry_opt = self.service_locator.get_service_by_name::<ObjectRegistry>(service_names::OBJECT_REGISTRY).await;
        let object_registry = if let Some(reg) = object_registry_opt {
            reg
        } else {
            self.service_locator.get_service::<ObjectRegistry>().await
                .ok_or_else(|| Status::internal("ObjectRegistry not found in ServiceLocator"))?
        };
        
        let ctx = RequestContext::internal();
        
        // Query for node registrations
        // ObjectRegistry stores nodes with object_id = node_id (using ObjectTypeNode)
        let mut remote_nodes = Vec::new();
        
        // Remote node discovery:
        // Would require querying ObjectRegistry for node registrations and calling NodeService.GetNode
        // This is a future enhancement for multi-node aggregation
        // For now, only local node is returned
        
        Ok(remote_nodes)
    }
}

#[tonic::async_trait]
impl DashboardService for DashboardServiceImpl {
    async fn get_summary(
        &self,
        request: Request<GetSummaryRequest>,
    ) -> Result<Response<GetSummaryResponse>, Status> {
        // Extract metadata before consuming request
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        // Create a new Request with the metadata for context methods
        let mut request_for_context = Request::new(());
        *request_for_context.metadata_mut() = metadata;
        let tenant_id = if req.tenant_id.is_empty() {
            self.get_tenant_id_from_context(&request_for_context)
        } else {
            Some(req.tenant_id.clone())
        };
        let is_admin = self.is_admin(&request_for_context);

        // Get since timestamp (default: now - 24 hours)
        let since = req.since.unwrap_or_else(Self::default_since);
        let until = Timestamp {
            seconds: Utc::now().timestamp(),
            nanos: Utc::now().timestamp_subsec_nanos() as i32,
        };

        // Get nodes list (local + remote)
        let cluster_id = if req.cluster_id.is_empty() { None } else { Some(req.cluster_id.clone()) };
        let local_nodes = self.get_nodes_internal(tenant_id.clone(), cluster_id.clone(), None).await?;
        let remote_nodes = self.query_remote_nodes(tenant_id.clone(), cluster_id.clone()).await?;
        let all_nodes = [local_nodes, remote_nodes].concat();
        let total_nodes = all_nodes.len() as u32;

        // Count unique clusters
        // If nodes exist but have no cluster_name, count as "default" cluster
        // If no nodes exist, show 0 clusters
        let clusters: HashSet<String> = if total_nodes > 0 {
            let mut cluster_set: HashSet<String> = all_nodes
                .iter()
                .filter_map(|n| {
                    if n.cluster_name.is_empty() {
                        Some("default".to_string()) // Use "default" for nodes without cluster_name
                    } else {
                        Some(n.cluster_name.clone())
                    }
                })
                .collect();
            // If all nodes had empty cluster_name, we'll have {"default"}
            // If some had names, we'll have those names
            cluster_set
        } else {
            HashSet::new() // No nodes = no clusters
        };
        let total_clusters = clusters.len() as u32;

        // Count tenants
        // For admin: count unique tenant IDs from all applications/actors
        // For non-admin: return 1 if tenant_id is set, 0 otherwise
        // Always show at least 1 tenant if there are any nodes/applications/actors
        let total_tenants = if is_admin {
            // Collect unique tenant IDs from applications and actors
            let mut tenant_ids = HashSet::new();
            
            // Always include "internal" and "default" tenants if node is running
            if total_nodes > 0 {
                tenant_ids.insert("internal".to_string());
                tenant_ids.insert("default".to_string());
            }
            
            // Get from applications (if they have tenant metadata)
            let app_manager: Arc<ApplicationManager> = self.service_locator.get_service_by_name(plexspaces_core::service_locator::service_names::APPLICATION_MANAGER).await
                .ok_or_else(|| Status::internal("ApplicationManager not found in ServiceLocator"))?;
            let app_names = app_manager.list_applications().await;
            for name in app_names {
                if let Some(info) = app_manager.get_application_info(&name).await {
                    // Applications don't currently store tenant_id in ApplicationInfo
                    // For now, use default tenant
                    tenant_ids.insert("default".to_string());
                }
            }
            
            // Get from actors (parse from actor IDs or get from isolation context)
            if let Some(actor_registry) = self.service_locator.get_service::<ActorRegistry>().await {
                let registered_ids = actor_registry.registered_actor_ids().read().await;
                for actor_id in registered_ids.iter() {
                    let (_, tenant) = self.parse_actor_id(actor_id);
                    tenant_ids.insert(tenant);
                }
            }
            
            tenant_ids.len() as u32
        } else if let Some(ref tid) = tenant_id {
            if tid.is_empty() {
                // If no tenant_id but node is running, show 1 (current tenant)
                if total_nodes > 0 { 1 } else { 0 }
            } else {
                1
            }
        } else {
            // No tenant_id specified, but if node is running, show 1
            if total_nodes > 0 { 1 } else { 0 }
        };

        // Count applications (filtered by tenant if auth)
        // Try get_service_by_name first (as it's registered by name)
        use plexspaces_core::service_locator::service_names;
        let app_manager_opt = self.service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await;
        let app_manager: Arc<ApplicationManager> = if let Some(mgr) = app_manager_opt {
            mgr
        } else {
            self.service_locator.get_service().await
                .ok_or_else(|| Status::internal("ApplicationManager not found in ServiceLocator"))?
        };
        let app_names = app_manager.list_applications().await;
        let total_applications = app_names.len() as u32;

        // Aggregate actors by type from ActorRegistry using actor_type_index
        let mut actors_by_type: HashMap<String, u32> = HashMap::new();
        
        // Try get_service first, then fallback to get_service_by_name
        let actor_registry = self.service_locator.get_service::<ActorRegistry>().await;
        
        // If get_service failed, try get_service_by_name
        let actor_registry = if actor_registry.is_none() {
            use plexspaces_core::service_locator::service_names;
            self.service_locator.get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY).await
        } else {
            actor_registry
        };
        
        if let Some(actor_registry) = actor_registry {
            // Use actor_type_index to get counts by type
            let index = actor_registry.actor_type_index().read().await;
            tracing::debug!("get_summary: actor_type_index has {} entries", index.len());
            for ((_tenant, _namespace, actor_type), actor_ids) in index.iter() {
                *actors_by_type.entry(actor_type.clone()).or_insert(0) += actor_ids.len() as u32;
                tracing::debug!("get_summary: aggregated actor_type={}, count={}", actor_type, actor_ids.len());
            }
            
            // Also count actors without type (registered but not in index)
            let registered_ids = actor_registry.registered_actor_ids().read().await;
            let mut typed_actors = HashSet::new();
            for actor_ids in index.values() {
                for actor_id in actor_ids {
                    typed_actors.insert(actor_id.clone());
                }
            }
            let untyped_count = registered_ids.len().saturating_sub(typed_actors.len());
            if untyped_count > 0 {
                *actors_by_type.entry("unknown".to_string()).or_insert(0) += untyped_count as u32;
            }
        }

        Ok(Response::new(GetSummaryResponse {
            total_clusters,
            total_nodes,
            total_tenants,
            total_applications,
            actors_by_type,
            since: Some(since),
            until: Some(until),
        }))
    }

    async fn get_nodes(
        &self,
        request: Request<GetNodesRequest>,
    ) -> Result<Response<GetNodesResponse>, Status> {
        let req = request.into_inner();
        let tenant_id = if req.tenant_id.is_empty() { None } else { Some(req.tenant_id.clone()) };
        let cluster_id = if req.cluster_id.is_empty() { None } else { Some(req.cluster_id.clone()) };
        let page_request = req.page;

        let local_nodes = self.get_nodes_internal(tenant_id.clone(), cluster_id.clone(), page_request.clone()).await?;
        let remote_nodes = self.query_remote_nodes(tenant_id, cluster_id).await?;
        let all_nodes = [local_nodes, remote_nodes].concat();

        // Apply pagination
        let (paginated_nodes, page_response) = Self::apply_pagination(all_nodes, page_request);

        Ok(Response::new(GetNodesResponse {
            nodes: paginated_nodes,
            page: Some(page_response),
        }))
    }

    async fn get_node_dashboard(
        &self,
        request: Request<GetNodeDashboardRequest>,
    ) -> Result<Response<GetNodeDashboardResponse>, Status> {
        let req = request.into_inner();
        
        if req.node_id.is_empty() {
            return Err(Status::invalid_argument("node_id is required"));
        }

        // Get local node ID from metrics
        let metrics_accessor = self.service_locator.get_node_metrics_accessor().await
            .ok_or_else(|| Status::internal("NodeMetricsAccessor not registered in ServiceLocator"))?;
        let local_metrics = metrics_accessor.get_metrics().await;
        let local_node_id = local_metrics.node_id;
        
        // Get node information
        let node = if req.node_id == local_node_id {
            // Local node
            self.node_to_proto().await?
        } else {
            // Remote node - query via NodeService
            self.query_remote_node(&req.node_id).await?
        };

        // Get node metrics from NodeMetricsAccessor
        let metrics = metrics_accessor.get_metrics().await;
        let node_metrics = Some(ProtoNodeMetrics {
            node_id: metrics.node_id.clone(),
            cluster_name: metrics.cluster_name.clone(),
            memory_used_bytes: metrics.memory_used_bytes,
            memory_available_bytes: metrics.memory_available_bytes,
            cpu_usage_percent: metrics.cpu_usage_percent,
            uptime_seconds: metrics.uptime_seconds,
            messages_routed: metrics.messages_routed,
            local_deliveries: metrics.local_deliveries,
            remote_deliveries: metrics.remote_deliveries,
            failed_deliveries: metrics.failed_deliveries,
            messages_processed: metrics.messages_processed,
            active_actors: metrics.active_actors,
            actor_count: metrics.actor_count,
            connected_nodes: metrics.connected_nodes,
        });

        // Get applications count
        // Try get_service_by_name first (as it's registered by name)
        use plexspaces_core::service_locator::service_names;
        let app_manager_opt = self.service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await;
        let app_manager: Arc<ApplicationManager> = if let Some(mgr) = app_manager_opt {
            mgr
        } else {
            self.service_locator.get_service().await
                .ok_or_else(|| Status::internal("ApplicationManager not found in ServiceLocator"))?
        };
        let app_names = app_manager.list_applications().await;
        let total_applications = app_names.len() as u32;

        // Get actors by type with proper type detection using actor_type_index
        let mut actors_by_type: HashMap<String, u32> = HashMap::new();
        
        // Try get_service first, then fallback to get_service_by_name
        let actor_registry = self.service_locator.get_service::<ActorRegistry>().await;
        
        // If get_service failed, try get_service_by_name
        let actor_registry = if actor_registry.is_none() {
            use plexspaces_core::service_locator::service_names;
            self.service_locator.get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY).await
        } else {
            actor_registry
        };
        
        if let Some(actor_registry) = actor_registry {
            // Use actor_type_index to get counts by type
            let index = actor_registry.actor_type_index().read().await;
            for ((_tenant, _namespace, actor_type), actor_ids) in index.iter() {
                *actors_by_type.entry(actor_type.clone()).or_insert(0) += actor_ids.len() as u32;
            }
            
            // Also count actors without type (registered but not in index)
            let registered_ids = actor_registry.registered_actor_ids().read().await;
            let mut typed_actors = HashSet::new();
            for actor_ids in index.values() {
                for actor_id in actor_ids {
                    typed_actors.insert(actor_id.clone());
                }
            }
            let untyped_count = registered_ids.len().saturating_sub(typed_actors.len());
            if untyped_count > 0 {
                *actors_by_type.entry("unknown".to_string()).or_insert(0) += untyped_count as u32;
            }
        }

        // Count unique tenants from actors
        let mut tenant_ids = HashSet::new();
        if let Some(actor_registry) = self.service_locator.get_service::<ActorRegistry>().await {
            let registered_ids = actor_registry.registered_actor_ids().read().await;
            for actor_id in registered_ids.iter() {
                let (_, tenant) = self.parse_actor_id(actor_id);
                tenant_ids.insert(tenant);
            }
        }
        let total_tenants = tenant_ids.len() as u32;

        // Get summary metrics
        let summary = NodeSummaryMetrics {
            total_tenants,
            total_applications,
            actors_by_type,
        };

        // Get system metrics
        let system_metrics = self.get_system_metrics().await;

        // Get dependency health
        let dependency_health = self.get_dependency_health_internal(true).await;

        Ok(Response::new(GetNodeDashboardResponse {
            node: Some(node),
            node_metrics,
            system_metrics,
            summary: Some(summary),
            dependency_health: Some(dependency_health),
        }))
    }

    async fn get_applications(
        &self,
        request: Request<GetApplicationsRequest>,
    ) -> Result<Response<GetApplicationsResponse>, Status> {
        // Extract metadata before consuming request
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        // Create a new Request with the metadata for context methods
        let mut request_for_context = Request::new(());
        *request_for_context.metadata_mut() = metadata;
        
        // Get tenant_id from request context if not provided
        let tenant_id = if req.tenant_id.is_empty() {
            self.get_tenant_id_from_context(&request_for_context)
        } else {
            Some(req.tenant_id.clone())
        };
        let is_admin = self.is_admin(&request_for_context);

        // Get local node ID from metrics
        let metrics_accessor = self.service_locator.get_node_metrics_accessor().await
            .ok_or_else(|| Status::internal("NodeMetricsAccessor not registered in ServiceLocator"))?;
        let local_metrics = metrics_accessor.get_metrics().await;
        let local_node_id = local_metrics.node_id;
        
        // Filter by node_id if provided
        if !req.node_id.is_empty() {
            if &req.node_id != &local_node_id {
                // Remote node - query via ApplicationService
                return self.query_remote_applications(&req.node_id, &req, tenant_id, is_admin).await;
            }
        }

        // Get applications from ApplicationManager
        // Try get_service_by_name first (as it's registered by name)
        use plexspaces_core::service_locator::service_names;
        let app_manager_opt = self.service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await;
        let app_manager: Arc<ApplicationManager> = if let Some(mgr) = app_manager_opt {
            mgr
        } else {
            self.service_locator.get_service().await
                .ok_or_else(|| Status::internal("ApplicationManager not found in ServiceLocator"))?
        };
        let app_names = app_manager.list_applications().await;
        
        let mut applications = Vec::new();
        for name in app_names {
            if let Some(info) = app_manager.get_application_info(&name).await {
                // Apply filters
                if !req.name_pattern.is_empty() {
                    if !info.name.contains(&req.name_pattern) && !info.application_id.contains(&req.name_pattern) {
                        continue;
                    }
                }
                
                // Note: ApplicationInfo doesn't currently include namespace/tenant_id metadata
                // Filtering by these would require extending ApplicationInfo proto
                // For now, all applications are returned (filtering by name_pattern works)
                
                applications.push(info);
            }
        }

        // Apply pagination
        let (paginated_apps, page_response) = Self::apply_pagination(applications, req.page);

        Ok(Response::new(GetApplicationsResponse {
            applications: paginated_apps,
            page: Some(page_response),
        }))
    }

    async fn get_actors(
        &self,
        request: Request<GetActorsRequest>,
    ) -> Result<Response<GetActorsResponse>, Status> {
        let req = request.into_inner();
        
        // Get ActorRegistry
        let actor_registry: Arc<ActorRegistry> = self.service_locator.get_service_by_name(plexspaces_core::service_locator::service_names::ACTOR_REGISTRY).await
            .ok_or_else(|| Status::internal("ActorRegistry not found in ServiceLocator"))?;

        // Get registered actor IDs
        let registered_ids = actor_registry.registered_actor_ids().read().await;
        let actor_configs = actor_registry.actor_configs().read().await;

        let mut actors = Vec::new();
        for actor_id in registered_ids.iter() {
            // Apply filters (proto fields are String, not Option<String>, so check if empty)
            if !req.actor_id_pattern.is_empty() {
                if !actor_id.contains(&req.actor_id_pattern) {
                    continue;
                }
            }
            
            if !req.node_id.is_empty() {
                // Parse actor_id to extract node_id (format: "actor@node")
                let parts: Vec<&str> = actor_id.split('@').collect();
                // Get local node ID from metrics
                let metrics_accessor = self.service_locator.get_node_metrics_accessor().await;
                let local_node_id = if let Some(getter) = metrics_accessor {
                    let metrics = getter.get_metrics().await;
                    metrics.node_id
                } else {
                    "unknown".to_string()
                };
                if parts.len() == 2 && parts[1] != &req.node_id {
                    continue;
                } else if parts.len() != 2 && &req.node_id != &local_node_id {
                    continue;
                }
            }

            // Get actor type from actor_type_index
            let actor_type = {
                let index = actor_registry.actor_type_index().read().await;
                let mut found_type = None;
                for ((_tenant, _namespace, atype), actor_ids) in index.iter() {
                    if actor_ids.contains(actor_id) {
                        found_type = Some(atype.clone());
                        break;
                    }
                }
                found_type.unwrap_or_else(|| "unknown".to_string())
            };
            
            if !req.actor_type.is_empty() {
                if &actor_type != &req.actor_type {
                    continue;
                }
            }

            // Parse actor_id for namespace/tenant
            let (namespace, tenant_id) = self.parse_actor_id(actor_id);

            if !req.namespace.is_empty() {
                if &namespace != &req.namespace {
                    continue;
                }
            }

            if !req.tenant_id.is_empty() {
                if &tenant_id != &req.tenant_id {
                    continue;
                }
            }

            // Check actor status (running if registered, terminated otherwise)
            let status = if actor_registry.is_actor_activated(actor_id).await {
                "running"
            } else {
                "terminated"
            };

            // Apply status filter
            if !req.status.is_empty() {
                if status != &req.status {
                    continue;
                }
            }

            // Get journal metrics for durable actors
            let (journal_size_bytes, last_checkpoint) = self.get_durable_actor_metrics(actor_id).await;

            // Get actor metrics
            let metrics = self.get_actor_metrics(actor_id).await;

            // Get created_at from metadata if available
            // Note: Creation time is not currently stored in ActorMetadata
            // This can be enhanced when ActorMetadata includes creation timestamp
            let created_at = None;

            // Create ActorInfo
            let actor_info = ActorInfo {
                actor_id: actor_id.clone(),
                actor_type,
                actor_group: req.actor_group.clone(),
                namespace,
                tenant_id,
                node_id: {
                    let parts: Vec<&str> = actor_id.split('@').collect();
                    if parts.len() == 2 {
                        parts[1].to_string()
                    } else {
                        // Get local node ID from metrics
                        let metrics_accessor = self.service_locator.get_node_metrics_accessor().await;
                        if let Some(getter) = metrics_accessor {
                            let metrics = getter.get_metrics().await;
                            metrics.node_id
                        } else {
                            "unknown".to_string()
                        }
                    }
                },
                status: status.to_string(),
                metrics,
                journal_size_bytes,
                last_checkpoint,
                created_at,
            };

            actors.push(actor_info);
        }

        // Apply pagination
        let (paginated_actors, page_response) = Self::apply_pagination(actors, req.page);

        Ok(Response::new(GetActorsResponse {
            actors: paginated_actors,
            page: Some(page_response),
        }))
    }

    async fn get_dependency_health(
        &self,
        request: Request<GetDependencyHealthRequest>,
    ) -> Result<Response<GetDependencyHealthResponse>, Status> {
        let req = request.into_inner();
        let include_non_critical = req.include_non_critical;
        
        let health_check = self.get_dependency_health_internal(include_non_critical).await;
        
        Ok(Response::new(GetDependencyHealthResponse {
            health_check: Some(health_check),
            node_id: req.node_id,
        }))
    }

    async fn get_workflows(
        &self,
        _request: Request<GetWorkflowsRequest>,
    ) -> Result<Response<GetWorkflowsResponse>, Status> {
        let _req = _request.into_inner();
        
        // Try to get WorkflowService from ServiceLocator
        // WorkflowService might not be registered, so handle gracefully
        // Note: WorkflowServiceImpl might not implement Service trait, so we can't query it this way
        // For now, return empty list - can be enhanced when WorkflowService is properly registered
        // or accessed via gRPC client
        // WorkflowService integration:
        // Workflows are managed by WorkflowService which may not be registered in ServiceLocator.
        // Workflows can be accessed directly via WorkflowService gRPC API.
        // For dashboard aggregation, we'd need WorkflowService to be registered or queried via gRPC client.
        // This is a design decision: workflows are optional and may not be available on all nodes.
        Ok(Response::new(GetWorkflowsResponse {
            workflows: vec![],
            page: None,
        }))
    }
}

impl DashboardServiceImpl {
    /// Internal helper to get nodes list
    async fn get_nodes_internal(
        &self,
        _tenant_id: Option<String>,
        cluster_id: Option<String>,
        _page: Option<PageRequest>,
    ) -> Result<Vec<ProtoNode>, Status> {
        let local_node = self.node_to_proto().await?;

        // Filter by cluster_id if provided
        if let Some(ref cid) = cluster_id {
            if local_node.cluster_name == *cid {
                Ok(vec![local_node])
            } else {
                Ok(vec![])
            }
        } else {
            Ok(vec![local_node])
        }
    }

    /// Query remote node via NodeService
    async fn query_remote_node(&self, node_id: &str) -> Result<ProtoNode, Status> {
        // Get node address from ObjectRegistry
        let object_registry = self.service_locator.get_service::<ObjectRegistry>().await
            .ok_or_else(|| Status::internal("ObjectRegistry not found in ServiceLocator"))?;
        
        let ctx = RequestContext::internal();
        // Lookup node registration (nodes are registered with object_id = node_id using ObjectTypeNode)
        use plexspaces_proto::object_registry::v1::ObjectType;
        let registration = object_registry
            .lookup_full(&ctx, ObjectType::ObjectTypeNode, node_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to lookup node: {}", e)))?
            .ok_or_else(|| Status::not_found(format!("Node not found: {}", node_id)))?;
        
        let node_address = registration.grpc_address.clone();

        // Create gRPC client for remote node
        use tonic::transport::Channel;
        use plexspaces_proto::node::v1::node_service_client::NodeServiceClient;
        
        let endpoint = Channel::from_shared(format!("http://{}", node_address))
            .map_err(|e| Status::internal(format!("Invalid endpoint: {}", e)))?;
        let channel = endpoint
            .connect()
            .await
            .map_err(|e| Status::internal(format!("Connection failed: {}", e)))?;
        let mut client = NodeServiceClient::new(channel);

        // Call GetNode
        use tonic::Request as TonicRequest;
        let get_node_req = plexspaces_proto::node::v1::GetNodeRequest {
            node_id: node_id.to_string(),
        };

        match client.get_node(TonicRequest::new(get_node_req)).await {
            Ok(response) => Ok(response.into_inner()),
            Err(e) => Err(Status::internal(format!("Failed to get remote node: {}", e))),
        }
    }

    /// Query remote applications via ApplicationService
    async fn query_remote_applications(
        &self,
        node_id: &str,
        req: &GetApplicationsRequest,
        tenant_id: Option<String>,
        _is_admin: bool,
    ) -> Result<Response<GetApplicationsResponse>, Status> {
        // Get node address from ObjectRegistry
        let object_registry = self.service_locator.get_service::<ObjectRegistry>().await
            .ok_or_else(|| Status::internal("ObjectRegistry not found in ServiceLocator"))?;
        
        let ctx = RequestContext::internal();
        // Lookup node registration (nodes are registered with object_id = node_id using ObjectTypeNode)
        use plexspaces_proto::object_registry::v1::ObjectType;
        let registration = object_registry
            .lookup_full(&ctx, ObjectType::ObjectTypeNode, node_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to lookup node: {}", e)))?
            .ok_or_else(|| Status::not_found(format!("Node not found: {}", node_id)))?;
        
        let node_address = registration.grpc_address.clone();

        // Create gRPC client for remote node
        use tonic::transport::Channel;
        
        let endpoint = Channel::from_shared(format!("http://{}", node_address))
            .map_err(|e| Status::internal(format!("Invalid endpoint: {}", e)))?;
        let channel = endpoint
            .connect()
            .await
            .map_err(|e| Status::internal(format!("Connection failed: {}", e)))?;
        let mut client = ApplicationServiceClient::new(channel);

        // Call ListApplications
        use tonic::Request as TonicRequest;
        let list_req = plexspaces_proto::application::v1::ListApplicationsRequest {
            status_filter: None,
        };

        match client.list_applications(TonicRequest::new(list_req)).await {
            Ok(response) => {
                let mut applications = response.into_inner().applications;
                
                // Apply filters
                if !req.name_pattern.is_empty() {
                    applications.retain(|app| {
                        app.name.contains(&req.name_pattern) || app.application_id.contains(&req.name_pattern)
                    });
                }
                
                // Apply pagination
                let (paginated_apps, page_response) = Self::apply_pagination(applications, req.page.clone());

                Ok(Response::new(GetApplicationsResponse {
                    applications: paginated_apps,
                    page: Some(page_response),
                }))
            }
            Err(e) => Err(Status::internal(format!("Failed to get remote applications: {}", e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Node types only needed in tests - can be conditionally compiled if needed
    // For now, tests are disabled since dashboard doesn't depend on node
    // use plexspaces_node::{Node, NodeBuilder};
    use plexspaces_core::ServiceLocator;
    use std::sync::Arc;
    use tonic::Request;

    // Tests disabled - dashboard no longer depends on node to break cyclic dependency
    // Tests can be re-enabled by making node a dev-dependency if needed
    /*
    async fn create_test_node() -> Arc<Node> {
        let node = NodeBuilder::new("test-node").build();
        Arc::new(node)
    }

    async fn create_test_service(node: Arc<Node>) -> DashboardServiceImpl {
        let service_locator = node.service_locator();
        DashboardServiceImpl::new(service_locator)
    }
    */

    // Tests that require Node are disabled since dashboard no longer depends on node
    // These can be re-enabled by making node a dev-dependency if needed
    /*
    #[tokio::test]
    async fn test_get_summary() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(GetSummaryRequest {
            tenant_id: None,
            node_id: None,
            cluster_id: None,
            since: None,
        });

        let response = service.get_summary(request).await;
        assert!(response.is_ok());
        
        let summary = response.unwrap().into_inner();
        assert_eq!(summary.total_nodes, 1);
        assert!(summary.since.is_some());
        assert!(summary.until.is_some());
    }

    #[tokio::test]
    async fn test_get_nodes() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(GetNodesRequest {
            tenant_id: None,
            cluster_id: None,
            page: None,
        });

        let response = service.get_nodes(request).await;
        assert!(response.is_ok());
        
        let nodes_response = response.unwrap().into_inner();
        assert_eq!(nodes_response.nodes.len(), 1);
        assert_eq!(nodes_response.nodes[0].id, "test-node");
    }

    #[tokio::test]
    async fn test_get_node_dashboard() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(GetNodeDashboardRequest {
            node_id: "test-node".to_string(),
            since: None,
        });

        let response = service.get_node_dashboard(request).await;
        assert!(response.is_ok());
        
        let dashboard = response.unwrap().into_inner();
        assert!(dashboard.node.is_some());
        assert_eq!(dashboard.node.as_ref().unwrap().id, "test-node");
        assert!(dashboard.node_metrics.is_some());
        assert!(dashboard.summary.is_some());
    }

    #[tokio::test]
    async fn test_get_node_dashboard_invalid_node_id() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(GetNodeDashboardRequest {
            node_id: String::new(),
            since: None,
        });

        let response = service.get_node_dashboard(request).await;
        assert!(response.is_err());
        assert_eq!(response.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_get_applications() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(GetApplicationsRequest {
            node_id: None,
            tenant_id: None,
            namespace: None,
            name_pattern: None,
            page: None,
        });

        let response = service.get_applications(request).await;
        assert!(response.is_ok());
        
        let apps_response = response.unwrap().into_inner();
        assert!(apps_response.page.is_some());
    }

    #[tokio::test]
    async fn test_get_actors() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(GetActorsRequest {
            node_id: None,
            tenant_id: None,
            namespace: None,
            actor_id_pattern: None,
            actor_group: None,
            actor_type: None,
            status: None,
            since: None,
            page: None,
        });

        let response = service.get_actors(request).await;
        assert!(response.is_ok());
        
        let actors_response = response.unwrap().into_inner();
        assert!(actors_response.page.is_some());
    }

    #[tokio::test]
    async fn test_get_workflows() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(GetWorkflowsRequest {
            node_id: None,
            tenant_id: None,
            definition_id: None,
            status: None,
            page: None,
        });

        let response = service.get_workflows(request).await;
        assert!(response.is_ok());
        
        let workflows_response = response.unwrap().into_inner();
        // WorkflowService might not be available, so empty list is valid
        assert!(workflows_response.page.is_some() || workflows_response.page.is_none());
    }
    */

    #[tokio::test]
    async fn test_pagination() {
        let items: Vec<i32> = (0..100).collect();
        let page_request = Some(PageRequest {
            offset: 0,
            limit: 10,
            filter: String::new(),
            order_by: String::new(),
        });

        let (paginated, page_response) = DashboardServiceImpl::apply_pagination(items, page_request);
        
        assert_eq!(paginated.len(), 10);
        assert_eq!(paginated[0], 0);
        assert_eq!(paginated[9], 9);
        assert_eq!(page_response.total_size, 100);
        assert_eq!(page_response.offset, 0);
        assert_eq!(page_response.limit, 10);
        assert!(page_response.has_next);
    }

    #[tokio::test]
    async fn test_pagination_last_page() {
        let items: Vec<i32> = (0..15).collect();
        let page_request = Some(PageRequest {
            offset: 10,
            limit: 10,
            filter: String::new(),
            order_by: String::new(),
        });

        let (paginated, page_response) = DashboardServiceImpl::apply_pagination(items, page_request);
        
        assert_eq!(paginated.len(), 5);
        assert_eq!(paginated[0], 10);
        assert_eq!(page_response.total_size, 15);
        assert_eq!(page_response.offset, 10);
        assert_eq!(page_response.limit, 10);
        assert!(!page_response.has_next);
    }

    // test_parse_actor_id disabled - requires Node
    /*
    #[tokio::test]
    async fn test_parse_actor_id() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let actor_id = ActorId::from("namespace/actor@node");
        let (namespace, tenant) = service.parse_actor_id(&actor_id);
        assert_eq!(namespace, "namespace");
        assert_eq!(tenant, "default");
        
        let actor_id2 = ActorId::from("actor@node");
        let (namespace2, tenant2) = service.parse_actor_id(&actor_id2);
        assert_eq!(namespace2, "default");
        assert_eq!(tenant2, "default");
    }
    */

    #[tokio::test]
    async fn test_default_since() {
        let since = DashboardServiceImpl::default_since();
        let now = Utc::now();
        let since_dt = DateTime::<Utc>::from_timestamp(since.seconds, since.nanos as u32)
            .unwrap();
        
        // Should be approximately 24 hours ago
        let diff = now - since_dt;
        assert!(diff.num_hours() >= 23 && diff.num_hours() <= 25);
    }
}
