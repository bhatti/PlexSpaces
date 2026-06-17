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

use chrono::Utc;
use prost_types::Timestamp;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tonic::{Request, Response, Status};

use plexspaces_actor::{
    actor_metrics_from_exposition_for_namespace, max_histogram_bucket_upper_bound_for_labels,
    sum_counter_for_labels, sum_sample_values_for_labels, ActorId, ActorRegistry,
    DiscoverOptions, ProcessResourceSampler, RequestContext, RequestContextExt,
    ServiceLocator as ServiceLocatorTrait, ServiceLocator,
};
use plexspaces_common::{resolve_shared_db_backend, SharedDbBackend};
use plexspaces_proto::application::v1::application_service_client::ApplicationServiceClient;
use plexspaces_proto::application::v1::ApplicationInfo;
use plexspaces_proto::common::v1::{PageRequest, PageResponse};
use plexspaces_proto::dashboard::v1::{
    dashboard_service_server::DashboardService, ActorInfo, GetActorsRequest, GetActorsResponse,
    GetApplicationsRequest, GetApplicationsResponse, GetBlobPresignedUrlRequest,
    GetBlobPresignedUrlResponse, GetBlobsRequest, GetBlobsResponse, GetDashboardMetricsRequest,
    GetDashboardMetricsResponse, GetDependencyHealthRequest, GetDependencyHealthResponse,
    GetKeyValuesRequest, GetKeyValuesResponse, GetMetricsTableRequest, GetMetricsTableResponse,
    GetNodeDashboardRequest, GetNodeDashboardResponse, GetNodesRequest, GetNodesResponse,
    GetObjectsRequest, GetObjectsResponse, GetServiceLinksRequest, GetServiceLinksResponse,
    GetSummaryRequest, GetSummaryResponse, GetTupleSpacesRequest, GetTupleSpacesResponse,
    GetWorkflowsRequest, GetWorkflowsResponse, KeyValueDashboardEntry, NodeSummaryMetrics,
    TupleSpaceSummary,
};
use plexspaces_proto::metrics::v1::{ActorMetrics, SystemMetrics};
use plexspaces_proto::node::v1::{
    Node as ProtoNode, NodeMetrics as ProtoNodeMetrics, NodeStatus, NodeType,
};
use plexspaces_proto::system::v1::DetailedHealthCheck;
use plexspaces_proto::v1::actor::ActorState as ProtoActorState;
use plexspaces_workflow::storage::WorkflowStorage;
use plexspaces_workflow::types::ExecutionStatus;

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
    service_locator: Arc<dyn ServiceLocator>,

    /// Optional health reporter access (to avoid circular dependency)
    health_reporter_access: Option<Arc<dyn HealthReporterAccess>>,

    /// Optional tenant repository for accurate tenant count in summary.
    tenant_repo: tokio::sync::RwLock<Option<Arc<dyn crate::user_service::TenantRepository>>>,

    /// Reused process sampler for local node dashboard metrics.
    process_sampler: Arc<std::sync::Mutex<ProcessResourceSampler>>,
}

impl DashboardServiceImpl {
    /// Create new dashboard service
    pub fn new(service_locator: Arc<dyn ServiceLocatorTrait>) -> Self {
        Self {
            service_locator,
            health_reporter_access: None,
            tenant_repo: tokio::sync::RwLock::new(None),
            process_sampler: Arc::new(std::sync::Mutex::new(
                ProcessResourceSampler::new()
                    .expect("process metrics sampler must initialize for current process"),
            )),
        }
    }

    /// Create new dashboard service with health reporter access
    pub fn with_health_reporter(
        service_locator: Arc<dyn ServiceLocatorTrait>,
        health_reporter_access: Arc<dyn HealthReporterAccess>,
    ) -> Self {
        Self {
            service_locator,
            health_reporter_access: Some(health_reporter_access),
            tenant_repo: tokio::sync::RwLock::new(None),
            process_sampler: Arc::new(std::sync::Mutex::new(
                ProcessResourceSampler::new()
                    .expect("process metrics sampler must initialize for current process"),
            )),
        }
    }

    /// Set tenant repository for accurate tenant count in dashboard summary.
    pub async fn set_tenant_repo(&self, repo: Arc<dyn crate::user_service::TenantRepository>) {
        *self.tenant_repo.write().await = Some(repo);
    }

    /// Get service locator reference
    pub fn service_locator(&self) -> &Arc<dyn ServiceLocatorTrait> {
        &self.service_locator
    }

    /// Get tenant ID from request context (for filtering)
    fn get_tenant_id_from_context(&self, request: &Request<()>) -> Option<String> {
        // Extract tenant_id from request metadata (set by auth middleware)
        request
            .metadata()
            .get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    /// Check if user is admin (from request context)
    async fn is_admin(&self, request: &Request<()>) -> bool {
        if self.service_locator.is_auth_disabled().await {
            return true;
        }

        if request
            .metadata()
            .get("x-admin")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
        {
            return true;
        }

        if request
            .metadata()
            .get("x-user-role")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("admin"))
        {
            return true;
        }

        request
            .metadata()
            .get("x-user-roles")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|roles| {
                roles
                    .split(',')
                    .map(str::trim)
                    .any(|role| role.eq_ignore_ascii_case("admin"))
            })
    }

    /// Build request context for dashboard API requests.
    ///
    /// Tenant and namespace both come from the incoming request scope. Dashboard reads must not
    /// synthesize tenant identity from node-local defaults.
    async fn request_context_for_dashboard(
        &self,
        tenant_id: Option<String>,
        namespace: Option<String>,
    ) -> RequestContext {
        let effective_tenant = tenant_id.unwrap_or_default();
        let effective_namespace = namespace.unwrap_or_default();
        RequestContext::new_without_auth(effective_tenant, effective_namespace)
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

    /// Convert local node to proto (sysinfo + unified Prometheus counters).
    async fn node_to_proto(&self) -> Result<ProtoNode, Status> {
        let cfg = self
            .service_locator
            .get_node_config()
            .await
            .filter(|c| !c.id.is_empty())
            .ok_or_else(|| Status::internal("NodeConfig not registered in ServiceLocator"))?;

        let metrics = crate::node_service::snapshot_local_node_metrics(
            self.service_locator.clone(),
            cfg.id.clone(),
            0,
            self.process_sampler.clone(),
        )
        .await;

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
            actor_ids: vec![],
            metrics: Some(metrics.clone()),
            mtls_identity: None,
            public_certificate: vec![],
            auto_generate_certs: false,
            cluster_name: metrics.cluster_name.clone(),
        })
    }

    /// Get dependency health status
    async fn get_dependency_health_internal(
        &self,
        include_non_critical: bool,
    ) -> DetailedHealthCheck {
        // Use health reporter access if available
        if let Some(health_access) = &self.health_reporter_access {
            health_access
                .get_detailed_health(include_non_critical)
                .await
        } else {
            // Fallback: return empty health check if health reporter not available
            DetailedHealthCheck {
                overall_status: plexspaces_proto::system::v1::HealthStatus::HealthStatusUnhealthy
                    as i32,
                component_checks: vec![],
                dependency_checks: vec![],
                critical_dependencies_healthy: false,
                non_critical_dependencies_healthy: false,
            }
        }
    }

    /// Get system metrics from node
    async fn get_system_metrics(&self) -> Option<SystemMetrics> {
        use plexspaces_proto::metrics::v1::{
            ComponentMetrics, CpuMetrics, DiskMetrics, MemoryMetrics, NetworkMetrics,
        };
        use sysinfo::System;

        let mut system = System::new();
        system.refresh_all();

        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let available_memory = system.available_memory();
        let cpu_count = system.cpus().len() as u32;
        let cpu_usage = if cpu_count > 0 {
            system
                .cpus()
                .iter()
                .map(|cpu| cpu.cpu_usage() as f64)
                .sum::<f64>()
                / cpu_count as f64
        } else {
            0.0
        };

        // Get actors by type using actor_type_index
        let mut active_actors_by_type: HashMap<String, u32> = HashMap::new();
        if let Some(actor_registry) = self.service_locator.actor_registry().await {
            let index: tokio::sync::RwLockReadGuard<
                '_,
                HashMap<(String, String, String), Vec<ActorId>>,
            > = actor_registry.actor_type_index().read().await;
            for ((_tenant, _namespace, actor_type), actor_ids) in index.iter() {
                *active_actors_by_type.entry(actor_type.clone()).or_insert(0) +=
                    actor_ids.len() as u32;
            }

            // Also count actors without type (registered but not in index)
            let registered_actor_entries = actor_registry.registered_actor_entries().await;
            let mut typed_actors: HashSet<(String, String, ActorId)> = HashSet::new();
            for ((tenant_id, namespace, _actor_type), actor_ids) in index.iter() {
                for actor_id in actor_ids {
                    typed_actors.insert((tenant_id.clone(), namespace.clone(), actor_id.clone()));
                }
            }
            let untyped_count = registered_actor_entries
                .iter()
                .filter(|entry| !typed_actors.contains(*entry))
                .count();
            if untyped_count > 0 {
                *active_actors_by_type
                    .entry("unknown".to_string())
                    .or_insert(0) += untyped_count as u32;
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
                active_vms: 0,           // VM registry not yet integrated
                tuplespace_size: 0,      // TupleSpace size tracking not yet integrated
                active_subscriptions: 0, // Subscription tracking not yet implemented
                active_transactions: 0,  // Transaction tracking not yet implemented
            }),
        })
    }

    /// Get journal size and checkpoint for durable actor
    async fn get_durable_actor_metrics(&self, actor_id: &ActorId) -> (u64, Option<Timestamp>) {
        // Check if actor has durability facet
        if let Some(facet_manager_wrapper) = self.service_locator.get_facet_manager().await {
            let facet_manager = facet_manager_wrapper.inner_clone();
            if let Some(facet_container_arc) = facet_manager.get_facets(&actor_id.to_string()).await
            {
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

    /// Actor row metrics from the unified Prometheus recorder (namespace + local node_id).
    async fn get_actor_metrics(&self, actor_id: &ActorId) -> Option<ActorMetrics> {
        let node_id = self.service_locator.get_node_config().await?.id.clone();
        if node_id.is_empty() {
            return None;
        }
        let exposition = if let Some(access) =
            self.service_locator.get_metrics_service_access().await
        {
            access.export_prometheus_text().await
        } else if let Some(renderer) = self.service_locator.get_metrics_prometheus_renderer().await
        {
            renderer.render_prometheus_text()
        } else {
            return None;
        };
        let is_live = if let Some(reg) = self.service_locator.actor_registry().await {
            reg.live_actor_entries()
                .await
                .iter()
                .any(|(_, _, id)| id == actor_id)
        } else {
            false
        };
        Some(actor_metrics_from_exposition_for_namespace(
            &exposition,
            actor_id.namespace(),
            node_id.as_str(),
            is_live,
        ))
    }

    /// Apply pagination to a vector using offset and limit
    fn apply_pagination<T: Clone>(
        items: Vec<T>,
        page_request: Option<PageRequest>,
    ) -> (Vec<T>, PageResponse) {
        let (offset, limit) = Self::page_window(page_request.as_ref());

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

    fn page_window(page_request: Option<&PageRequest>) -> (usize, usize) {
        let offset = page_request.map(|p| p.offset.max(0) as usize).unwrap_or(0);
        let limit = page_request
            .map(|p| p.limit.clamp(1, 1000) as usize)
            .unwrap_or(50);
        (offset, limit)
    }

    fn application_matches_filters(
        app: &ApplicationInfo,
        req: &GetApplicationsRequest,
        tenant_id: Option<&str>,
        is_admin: bool,
    ) -> bool {
        if !req.name_pattern.is_empty()
            && !app.name.contains(&req.name_pattern)
            && !app.application_id.contains(&req.name_pattern)
        {
            return false;
        }

        if !req.namespace.is_empty() && app.name != req.namespace {
            return false;
        }

        if is_admin {
            if let Some(filter_tenant) = tenant_id.filter(|value| !value.is_empty()) {
                return app.tenant_id == filter_tenant;
            }
            return true;
        }

        let Some(filter_tenant) = tenant_id.filter(|value| !value.is_empty()) else {
            return true;
        };
        app.tenant_id == filter_tenant
    }

    async fn dashboard_request_context(
        &self,
        tenant_id: Option<&str>,
        is_admin: bool,
    ) -> RequestContext {
        if is_admin {
            self.service_locator
                .request_context_for_system_operations()
                .await
        } else {
            self.request_context_for_dashboard(tenant_id.map(str::to_string), None)
                .await
        }
    }

    fn tonic_request_with_dashboard_metadata<T>(
        payload: T,
        tenant_id: Option<&str>,
        namespace: Option<&str>,
        is_admin: bool,
    ) -> tonic::Request<T> {
        let mut request = tonic::Request::new(payload);
        if let Some(tenant_id) = tenant_id.filter(|value| !value.is_empty()) {
            if let Ok(value) = tonic::metadata::MetadataValue::try_from(tenant_id) {
                request.metadata_mut().insert("x-tenant-id", value);
            }
        }
        if let Some(namespace) = namespace.filter(|value| !value.is_empty()) {
            if let Ok(value) = tonic::metadata::MetadataValue::try_from(namespace) {
                request.metadata_mut().insert("x-namespace", value);
            }
        }
        if let Ok(value) =
            tonic::metadata::MetadataValue::try_from(if is_admin { "true" } else { "false" })
        {
            request.metadata_mut().insert("x-admin", value);
        }
        request
    }


    async fn application_rows_from_registry(
        &self,
        req: &GetApplicationsRequest,
        tenant_id: Option<&str>,
        is_admin: bool,
    ) -> Result<Vec<ApplicationInfo>, Status> {
        let (applications, _) = self
            .application_page_from_registry(req, tenant_id, is_admin)
            .await?;
        Ok(applications)
    }

    async fn application_page_from_registry(
        &self,
        req: &GetApplicationsRequest,
        tenant_id: Option<&str>,
        is_admin: bool,
    ) -> Result<(Vec<ApplicationInfo>, PageResponse), Status> {
        use plexspaces_proto::application::v1::ApplicationStatus;
        use plexspaces_proto::object_registry::v1::ObjectType;

        let ctx = self.dashboard_request_context(tenant_id, is_admin).await;
        let object_registry = self
            .service_locator
            .get_object_registry()
            .await
            .ok_or_else(|| Status::internal("ObjectRegistry not available in ServiceLocator"))?;

        let local_node_id = self
            .service_locator
            .get_node_config()
            .await
            .map(|cfg| cfg.id)
            .unwrap_or_default();
        let page_request = req.page.as_ref();
        let (page_offset, page_limit) = if page_request.is_some() {
            Self::page_window(page_request)
        } else {
            (0, usize::MAX)
        };
        let batch_size = if page_limit == usize::MAX {
            500
        } else {
            page_limit.max(100)
        };

        let mut registry_offset = 0;
        let mut total_size = 0usize;
        let mut applications = Vec::new();

        loop {
            let registrations = object_registry
                .discover(
                    &ctx,
                    DiscoverOptions {
                        object_type: Some(ObjectType::ObjectTypeApplication),
                        offset: registry_offset,
                        limit: batch_size,
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| Status::internal(format!("Failed to list applications: {e}")))?;
            if registrations.is_empty() {
                break;
            }
            let fetched_count = registrations.len();
            registry_offset += registrations.len();

            for registration in registrations {
                if !req.node_id.is_empty() && registration.node_id != req.node_id {
                    continue;
                }

                let mut info = ApplicationInfo {
                    application_id: registration.object_id.clone(),
                    name: registration.object_name.clone(),
                    tenant_id: registration.tenant_id.clone(),
                    version: registration.version.clone(),
                    status: ApplicationStatus::ApplicationStatusRunning as i32,
                    deployed_at: registration.created_at,
                    metrics: None,
                };

                if !Self::application_matches_filters(&info, req, tenant_id, is_admin) {
                    continue;
                }

                let include_in_page = total_size >= page_offset
                    && (page_limit == usize::MAX || applications.len() < page_limit);
                total_size += 1;

                if !include_in_page {
                    continue;
                }

                if registration.node_id == local_node_id {
                    if let Some(app_manager) = self.service_locator.application_manager().await {
                        if let Some(local_info) = app_manager
                            .get_application_info(&registration.object_name)
                            .await
                        {
                            info.version = local_info.version;
                            info.status = local_info.status;
                            info.deployed_at = local_info.deployed_at;
                            info.metrics = local_info.metrics;
                            self.merge_application_prometheus_metrics(&mut info).await;
                        }
                    }
                } else if let Ok(mut remote_info) = self
                    .query_remote_application_status(
                        &registration.node_id,
                        &plexspaces_common::dialable_node_address(&registration.grpc_address),
                        &registration.object_name,
                        tenant_id,
                        Some(registration.namespace.as_str()),
                        is_admin,
                    )
                    .await
                {
                    remote_info.application_id = registration.object_id.clone();
                    remote_info.name = registration.object_name.clone();
                    remote_info.tenant_id = registration.tenant_id.clone();
                    info = remote_info;
                }

                applications.push(info);
            }

            if fetched_count < batch_size {
                break;
            }
        }
        applications.sort_by(|left, right| left.application_id.cmp(&right.application_id));
        let page_response = PageResponse {
            total_size: total_size as i32,
            offset: page_offset.min(total_size) as i32,
            limit: if page_limit == usize::MAX {
                total_size as i32
            } else {
                page_limit as i32
            },
            has_next: page_limit != usize::MAX && page_offset + page_limit < total_size,
        };
        Ok((applications, page_response))
    }

    /// Query a remote node's ApplicationService for a single application status.
    ///
    /// Uses [`GrpcConnectionManager`] for pooled, lazy connections — no new TCP
    /// sockets are opened if a channel to this node already exists.
    async fn query_remote_application_status(
        &self,
        node_id: &str,
        grpc_address: &str,
        application_name: &str,
        tenant_id: Option<&str>,
        namespace: Option<&str>,
        is_admin: bool,
    ) -> Result<ApplicationInfo, Status> {
        let conn_mgr = self
            .service_locator
            .get_grpc_connection_manager()
            .await
            .ok_or_else(|| Status::internal("GrpcConnectionManager not available"))?;
        let channel = conn_mgr
            .get_application_service_connection(node_id, grpc_address)
            .await
            .map_err(|e| Status::unavailable(format!("Cannot reach node {node_id}: {e}")))?;
        let mut client = ApplicationServiceClient::new(channel);
        let response = client
            .get_application_status(Self::tonic_request_with_dashboard_metadata(
                plexspaces_proto::application::v1::GetApplicationStatusRequest {
                    application_id: application_name.to_string(),
                },
                tenant_id,
                namespace,
                is_admin,
            ))
            .await
            .map_err(|e| {
                Status::internal(format!("Failed to get remote application status: {e}"))
            })?;
        response
            .into_inner()
            .application
            .ok_or_else(|| Status::not_found(format!("Application not found: {application_name}")))
    }

    /// Query remote nodes via ObjectRegistry. Uses request-scoped context (no admin).
    async fn query_remote_nodes(
        &self,
        tenant_id: Option<String>,
        cluster_id: Option<String>,
    ) -> Result<Vec<ProtoNode>, Status> {
        let ctx = self.request_context_for_dashboard(tenant_id, None).await;

        // Use NodeRegistry (which internally uses ObjectRegistry with caching)
        let node_registry = self
            .service_locator
            .get_node_registry()
            .await
            .ok_or_else(|| Status::internal("NodeRegistry not found in ServiceLocator"))?;

        let cluster = cluster_id.as_deref();
        let (registrations, _next_token) = node_registry
            .list_nodes(&ctx, cluster, 1000, "")
            .await
            .map_err(|e| Status::internal(format!("Failed to list nodes: {}", e)))?;

        // Convert NodeRegistrations to ProtoNodes
        let nodes = registrations
            .into_iter()
            .map(|reg| ProtoNode {
                id: reg.node_id.clone(),
                node_type: NodeType::NodeTypeProcess as i32,
                status: reg.status,
                capabilities: None,
                metadata: None,
                created_at: reg.registered_at,
                last_heartbeat: reg.last_heartbeat,
                metrics: None,
                mtls_identity: None,
                public_certificate: vec![],
                auto_generate_certs: false,
                cluster_name: reg.capabilities.get("cluster").cloned().unwrap_or_default(),
                actor_ids: vec![],
            })
            .collect();

        Ok(nodes)
    }

    /// Dashboard node table: rows come only from [`NodeRegistry::list_nodes`].
    ///
    /// If this process has a node id, live metrics and heartbeat are merged into that row when it
    /// appears in the registry. If the local node is not registered yet but matches the cluster
    /// filter, a single row is appended so the UI is not empty during bootstrap.
    async fn nodes_from_registry_for_dashboard(
        &self,
        tenant_id: Option<String>,
        cluster_id: Option<String>,
        include_metrics: bool,
    ) -> Result<Vec<ProtoNode>, Status> {
        let mut nodes = self
            .query_remote_nodes(tenant_id.clone(), cluster_id.clone())
            .await?;

        let Some(cfg) = self
            .service_locator
            .get_node_config()
            .await
            .filter(|c| !c.id.is_empty())
        else {
            nodes.sort_by(|a, b| a.id.cmp(&b.id));
            return Ok(nodes);
        };

        let local_id = cfg.id.clone();
        let matches_cluster = cluster_id.as_ref().is_none_or(|cid| {
            !cfg.cluster_name.is_empty() && cfg.cluster_name == *cid
        });

        if let Ok(mut local) = self.node_to_proto().await {
            if let Some(n) = nodes.iter_mut().find(|n| n.id == local_id) {
                n.metrics = local.metrics.take();
                if local.last_heartbeat.is_some() {
                    n.last_heartbeat = local.last_heartbeat.take();
                }
            } else if matches_cluster {
                nodes.push(local);
            }
        }

        let dashboard_ctx = if include_metrics {
            Some(
                self.request_context_for_dashboard(tenant_id.clone(), None)
                    .await,
            )
        } else {
            None
        };

        let mut deduped: HashMap<String, ProtoNode> = HashMap::new();
        for mut node in nodes {
            if include_metrics && node.metrics.is_none() && node.id != local_id {
                if let Some(ctx) = dashboard_ctx.as_ref() {
                    node.metrics = self.query_remote_node_metrics(ctx, &node.id).await.ok();
                }
            }
            deduped.entry(node.id.clone()).or_insert(node);
        }

        let mut nodes: Vec<_> = deduped.into_values().collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(nodes)
    }

    /// Fills [`plexspaces_proto::application::v1::ApplicationMetrics`] from Prometheus exposition.
    async fn merge_application_prometheus_metrics(
        &self,
        info: &mut plexspaces_proto::application::v1::ApplicationInfo,
    ) {
        let exposition = if let Some(access) =
            self.service_locator.get_metrics_service_access().await
        {
            access.export_prometheus_text().await
        } else if let Some(renderer) = self.service_locator.get_metrics_prometheus_renderer().await
        {
            renderer.render_prometheus_text()
        } else {
            return;
        };
        Self::merge_application_info_from_exposition(
            info,
            &exposition,
            self.service_locator.actor_registry().await.as_ref(),
        )
        .await;
    }

    /// Applies exposition text to one application row (local or fetched from a peer).
    async fn merge_application_info_from_exposition(
        info: &mut plexspaces_proto::application::v1::ApplicationInfo,
        exposition: &str,
        local_registry: Option<&Arc<ActorRegistry>>,
    ) {
        // Namespace is always derived from the application name.
        let namespace = if !info.name.is_empty() {
            info.name.as_str()
        } else {
            info.application_id.as_str()
        };
        if namespace.is_empty() {
            return;
        }
        let ns = [("namespace", namespace)];
        let mut metrics = info.metrics.take().unwrap_or_default();
        metrics.message_count = metrics.message_count.saturating_add(sum_counter_for_labels(
            exposition,
            "plexspaces_messages_routed_total",
            &ns,
        ));
        metrics.error_count = metrics.error_count.saturating_add(sum_counter_for_labels(
            exposition,
            "plexspaces_messages_failed_total",
            &ns,
        ));

        // Latency: same histogram as `record_message_routing_red` / `record_message_routing_metrics`.
        let route_sum_sec = sum_sample_values_for_labels(
            exposition,
            "plexspaces_message_routing_duration_seconds_sum",
            &ns,
        );
        let route_count = sum_counter_for_labels(
            exposition,
            "plexspaces_message_routing_duration_seconds_count",
            &ns,
        );
        if route_count > 0 {
            let total_ms = (route_sum_sec * 1000.0).ceil() as u64;
            metrics
                .latency_totals_ms
                .insert("message_routing".to_string(), total_ms);
            metrics
                .latency_samples
                .insert("message_routing".to_string(), route_count);
        }
        if let Some(max_secs) = max_histogram_bucket_upper_bound_for_labels(
            exposition,
            "plexspaces_message_routing_duration_seconds",
            &ns,
        ) {
            let max_ms = (max_secs * 1000.0).ceil() as u64;
            metrics
                .latency_max_ms
                .insert("message_routing".to_string(), max_ms.max(1));
        }

        if let Some(reg) = local_registry {
            let entries = reg.registered_actor_entries().await;
            let n = entries
                .iter()
                .filter(|(_, ns_entry, _)| *ns_entry == namespace)
                .count() as u64;
            metrics.actor_counts.insert("registered".to_string(), n);
        }
        info.metrics = Some(metrics);
    }

    /// Remote `GetMetrics` via gRPC using a pooled [`GrpcConnectionManager`] channel.
    ///
    /// Same shape as local `node_service::snapshot_local_node_metrics`.
    async fn query_remote_node_metrics(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<ProtoNodeMetrics, Status> {
        use plexspaces_actor::grpc_connection_manager::ServiceType;
        use plexspaces_proto::node::v1::node_service_client::NodeServiceClient;
        use plexspaces_proto::node::v1::GetMetricsRequest;
        use plexspaces_proto::object_registry::v1::ObjectType;

        let object_registry = self
            .service_locator
            .get_object_registry()
            .await
            .ok_or_else(|| Status::internal("ObjectRegistry not found in ServiceLocator"))?;

        let registration = object_registry
            .lookup_full(ctx, ObjectType::ObjectTypeNode, node_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to lookup node: {}", e)))?
            .ok_or_else(|| Status::not_found(format!("Node not found: {}", node_id)))?;

        let addr = plexspaces_common::dialable_node_address(&registration.grpc_address);
        let conn_mgr = self
            .service_locator
            .get_grpc_connection_manager()
            .await
            .ok_or_else(|| Status::internal("GrpcConnectionManager not available"))?;
        let channel = conn_mgr
            .get_connection(ServiceType::ServiceNameNodeService, node_id, &addr)
            .await
            .map_err(|e| Status::unavailable(format!("Cannot reach node {node_id}: {e}")))?;

        let mut client = NodeServiceClient::new(channel);
        client
            .get_metrics(Request::new(GetMetricsRequest {
                node_id: node_id.to_string(),
                include_extended: false,
            }))
            .await
            .map_err(|e| Status::internal(format!("Remote GetMetrics failed: {}", e)))
            .map(|r| r.into_inner())
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
        let is_admin = self.is_admin(&request_for_context).await;
        let tenant_id = if !req.tenant_id.is_empty() {
            Some(req.tenant_id.clone())
        } else if is_admin {
            None
        } else {
            self.get_tenant_id_from_context(&request_for_context)
        };

        // Get since timestamp (default: now - 24 hours)
        let since = req.since.unwrap_or_else(Self::default_since);
        let until = Timestamp {
            seconds: Utc::now().timestamp(),
            nanos: Utc::now().timestamp_subsec_nanos() as i32,
        };

        // Node counts from NodeRegistry only (see `nodes_from_registry_for_dashboard`)
        let cluster_id = if req.cluster_id.is_empty() {
            None
        } else {
            Some(req.cluster_id.clone())
        };
        let all_nodes = self
            .nodes_from_registry_for_dashboard(tenant_id.clone(), cluster_id.clone(), false)
            .await?;
        let total_nodes = all_nodes.len() as u32;

        // Count unique clusters
        // If nodes exist but have no cluster_name, count as "default" cluster
        // If no nodes exist, show 0 clusters
        let clusters: HashSet<String> = if total_nodes > 0 {
            let cluster_set: HashSet<String> = all_nodes
                .iter()
                .map(|n| {
                    if n.cluster_name.is_empty() {
                        "default".to_string()
                    } else {
                        n.cluster_name.clone()
                    }
                })
                .collect();
            cluster_set
        } else {
            HashSet::new() // No nodes = no clusters
        };
        let total_clusters = clusters.len() as u32;

        // Count tenants
        // For admin: count unique tenant IDs from all applications/actors
        // For non-admin: return 1 if tenant_id is set, 0 otherwise
        // Always show at least 1 tenant if there are any nodes/applications/actors
        let summary_applications = self
            .application_rows_from_registry(
                &GetApplicationsRequest {
                    node_id: req.node_id.clone(),
                    tenant_id: tenant_id.clone().unwrap_or_default(),
                    namespace: String::new(),
                    name_pattern: String::new(),
                    page: None,
                },
                tenant_id.as_deref(),
                is_admin,
            )
            .await?;

        let total_tenants = if let Some(ref tenant_repo) = *self.tenant_repo.read().await {
            // DB table is the authoritative source for tenant count.
            match tenant_repo.list_tenants(0, 1).await {
                Ok((_, total)) => total as u32,
                Err(_) => 0,
            }
        } else if is_admin {
            let mut tenant_ids: HashSet<String> = summary_applications
                .iter()
                .filter_map(|app| (!app.tenant_id.is_empty()).then_some(app.tenant_id.clone()))
                .collect();
            if let Some(actor_registry) = self.service_locator.actor_registry().await {
                tenant_ids.extend(actor_registry.registered_tenant_ids().await);
            }
            let count = tenant_ids.len() as u32;
            if count == 0 && total_nodes > 0 { 1 } else { count }
        } else {
            tenant_id
                .as_ref()
                .filter(|value| !value.is_empty())
                .map(|_| 1)
                .unwrap_or(0)
        };

        let total_applications = summary_applications.len() as u32;

        // Aggregate actors by type from ActorRegistry using actor_type_index
        let mut actors_by_type: HashMap<String, u32> = HashMap::new();

        // Get ActorRegistry using helper method
        let actor_registry = self.service_locator.actor_registry().await;

        if let Some(actor_registry) = actor_registry {
            // Use actor_type_index to get counts by type
            let index = actor_registry.actor_type_index().read().await;
            for ((entry_tenant, _namespace, actor_type), actor_ids) in index.iter() {
                if !is_admin
                    && tenant_id
                        .as_deref()
                        .filter(|value| !value.is_empty())
                        .is_none_or(|filter_tenant| filter_tenant != entry_tenant)
                {
                    continue;
                }
                *actors_by_type.entry(actor_type.clone()).or_insert(0) += actor_ids.len() as u32;
            }

            // Also count actors without type (registered but not in index)
            let registered_actor_entries = actor_registry.registered_actor_entries().await;
            let mut typed_actors: HashSet<(String, String, ActorId)> = HashSet::new();
            for ((tenant_id, namespace, _actor_type), actor_ids) in index.iter() {
                for actor_id in actor_ids {
                    typed_actors.insert((tenant_id.clone(), namespace.clone(), actor_id.clone()));
                }
            }
            let untyped_count = registered_actor_entries
                .iter()
                .filter(|entry| !typed_actors.contains(*entry))
                .count();
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
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        let mut request_for_context = Request::new(());
        *request_for_context.metadata_mut() = metadata;
        let is_admin = self.is_admin(&request_for_context).await;
        let tenant_id = if !req.tenant_id.is_empty() {
            Some(req.tenant_id.clone())
        } else if is_admin {
            None
        } else {
            self.get_tenant_id_from_context(&request_for_context)
        };
        let cluster_id = if req.cluster_id.is_empty() {
            None
        } else {
            Some(req.cluster_id.clone())
        };
        let page_request = req.page;

        let all_nodes = self
            .nodes_from_registry_for_dashboard(tenant_id, cluster_id, true)
            .await?;

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
        let metadata = request.metadata().clone();
        let req = request.into_inner();

        if req.node_id.is_empty() {
            return Err(Status::invalid_argument("node_id is required"));
        }

        let mut request_for_context = Request::new(());
        *request_for_context.metadata_mut() = metadata;
        let is_admin = self.is_admin(&request_for_context).await;
        let tenant_id = if is_admin {
            None
        } else {
            self.get_tenant_id_from_context(&request_for_context)
        };
        let ctx = self
            .request_context_for_dashboard(tenant_id.clone(), None)
            .await;

        let local_node_id = self
            .service_locator
            .get_node_config()
            .await
            .filter(|c| !c.id.is_empty())
            .map(|c| c.id)
            .ok_or_else(|| Status::internal("NodeConfig not registered in ServiceLocator"))?;

        // Get node information
        let node = if req.node_id == local_node_id {
            self.node_to_proto().await?
        } else {
            self.query_remote_node(&ctx, &req.node_id).await?
        };

        let node_metrics = if req.node_id == local_node_id {
            Some(
                crate::node_service::snapshot_local_node_metrics(
                    self.service_locator.clone(),
                    local_node_id.clone(),
                    0,
                    self.process_sampler.clone(),
                )
                .await,
            )
        } else {
            self.query_remote_node_metrics(&ctx, &req.node_id)
                .await
                .ok()
        };

        let total_applications = self
            .application_rows_from_registry(
                &GetApplicationsRequest {
                    node_id: req.node_id.clone(),
                    tenant_id: tenant_id.clone().unwrap_or_default(),
                    namespace: String::new(),
                    name_pattern: String::new(),
                    page: None,
                },
                tenant_id.as_deref(),
                is_admin,
            )
            .await?
            .len() as u32;

        // Get actors by type with proper type detection using actor_type_index
        let mut actors_by_type: HashMap<String, u32> = HashMap::new();

        // Get ActorRegistry using helper method
        let actor_registry = self.service_locator.actor_registry().await;

        if let Some(actor_registry) = actor_registry {
            // Use actor_type_index to get counts by type
            let index = actor_registry.actor_type_index().read().await;
            for ((entry_tenant, _namespace, actor_type), actor_ids) in index.iter() {
                if !is_admin
                    && tenant_id
                        .as_deref()
                        .filter(|value| !value.is_empty())
                        .is_none_or(|filter_tenant| filter_tenant != entry_tenant)
                {
                    continue;
                }
                *actors_by_type.entry(actor_type.clone()).or_insert(0) += actor_ids.len() as u32;
            }

            // Also count actors without type (registered but not in index)
            let registered_actor_entries = actor_registry.registered_actor_entries().await;
            let mut typed_actors: HashSet<(String, String, ActorId)> = HashSet::new();
            for ((tenant_id, namespace, _actor_type), actor_ids) in index.iter() {
                for actor_id in actor_ids {
                    typed_actors.insert((tenant_id.clone(), namespace.clone(), actor_id.clone()));
                }
            }
            let untyped_count = registered_actor_entries
                .iter()
                .filter(|entry| !typed_actors.contains(*entry))
                .count();
            if untyped_count > 0 {
                *actors_by_type.entry("unknown".to_string()).or_insert(0) += untyped_count as u32;
            }
        }

        // Count tenants from DB (authoritative source)
        let total_tenants = if let Some(ref tenant_repo) = *self.tenant_repo.read().await {
            match tenant_repo.list_tenants(0, 1).await {
                Ok((_, total)) => total as u32,
                Err(_) => 0,
            }
        } else if is_admin {
            let mut tenant_ids = HashSet::new();
            if let Some(actor_registry) = self.service_locator.actor_registry().await {
                tenant_ids.extend(actor_registry.registered_tenant_ids().await);
            }
            tenant_ids.len() as u32
        } else {
            tenant_id
                .as_ref()
                .filter(|value| !value.is_empty())
                .map(|_| 1)
                .unwrap_or(0)
        };

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

        let is_admin = self.is_admin(&request_for_context).await;

        // Admin sees all apps unless explicitly filtering by tenant_id query param.
        // Non-admin is always scoped to their own tenant from JWT context.
        let tenant_id = if !req.tenant_id.is_empty() {
            Some(req.tenant_id.clone())
        } else if is_admin {
            None
        } else {
            self.get_tenant_id_from_context(&request_for_context)
        };

        let (paginated_apps, page_response) = self
            .application_page_from_registry(&req, tenant_id.as_deref(), is_admin)
            .await?;

        Ok(Response::new(GetApplicationsResponse {
            applications: paginated_apps,
            page: Some(page_response),
        }))
    }

    async fn get_actors(
        &self,
        request: Request<GetActorsRequest>,
    ) -> Result<Response<GetActorsResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        let mut request_for_context = Request::new(());
        *request_for_context.metadata_mut() = metadata;
        let is_admin = self.is_admin(&request_for_context).await;
        let tenant_filter = if !req.tenant_id.is_empty() {
            Some(req.tenant_id.clone())
        } else if is_admin {
            None
        } else {
            self.get_tenant_id_from_context(&request_for_context)
        };

        // Get ActorRegistry
        let actor_registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| Status::internal("ActorRegistry not found in ServiceLocator"))?;

        // Get registered actor IDs from the registry inventory. Behavior filtering is evaluated at
        // query time so it can compose with status, tenant, namespace, and other runtime state.
        let registered_entries = actor_registry.registered_actor_entries().await;
        let actor_configs = actor_registry.actor_configs().read().await.clone();


        // Phase 1: filter cheaply, collect candidates without expensive per-actor lookups.
        struct ActorCandidate {
            actor_id: ActorId,
            actor_type: String,
            actor_group: String,
            namespace: String,
            tenant_id: String,
            node_id: String,
            status: String,
            behavior_kind: String,
        }
        let mut candidates: Vec<ActorCandidate> = Vec::new();
        for (entry_tenant_id, entry_namespace, actor_id) in registered_entries {
            // Apply filters (proto fields are String, not Option<String>, so check if empty)
            if !req.actor_id_pattern.is_empty()
                && !actor_id.contains(&req.actor_id_pattern) {
                    continue;
            }

            if !req.node_id.is_empty() {
                // Parse actor_id to extract node_id from the canonical actor ID
                let parts: Vec<&str> = actor_id.split('@').collect();
                // Get local node ID from metrics
                let local_node_id = self
                    .service_locator
                    .get_node_config()
                    .await
                    .map(|c| c.id)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "unknown".to_string());
                let node_matches = if parts.len() == 2 {
                    parts[1] == req.node_id
                } else {
                    req.node_id == local_node_id
                };
                if !node_matches {
                    continue;
                }
            }

            // Get actor type from actor_type_index
            let actor_type = {
                let index = actor_registry.actor_type_index().read().await;
                let mut found_type = None;
                for ((_tenant, _namespace, atype), actor_ids) in index.iter() {
                    if actor_ids.contains(&actor_id) {
                        found_type = Some(atype.clone());
                        break;
                    }
                }
                found_type.unwrap_or_else(|| "unknown".to_string())
            };

            if !req.actor_type.is_empty()
                && actor_type != req.actor_type {
                    continue;
            }

            let behavior_kind = actor_registry
                .get_behavior_kind(&actor_id)
                .await
                .unwrap_or_else(|| {
                    let behavior_type = match actor_type.as_str() {
                        "gen_server" => plexspaces_actor::BehaviorType::GenServer,
                        "gen_event" => plexspaces_actor::BehaviorType::GenEvent,
                        "gen_state_machine" => plexspaces_actor::BehaviorType::GenStateMachine,
                        "workflow" => plexspaces_actor::BehaviorType::Workflow,
                        other => plexspaces_actor::BehaviorType::Custom(other.to_string()),
                    };
                    ActorRegistry::behavior_kind_key(&behavior_type)
                });
            if !Self::actor_behavior_matches(&req.behavior_kind, &behavior_kind) {
                continue;
            }

            if !req.namespace.is_empty()
                && entry_namespace != req.namespace {
                    continue;
            }

            if !is_admin
                && tenant_filter
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .is_none_or(|filter_tenant| filter_tenant != entry_tenant_id)
            {
                continue;
            }
            if is_admin
                && tenant_filter
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .is_some_and(|filter_tenant| filter_tenant != entry_tenant_id)
            {
                continue;
            }

            let is_activated = actor_registry.is_actor_activated(&actor_id).await;
            let actor_state = actor_registry.get_actor_state(&actor_id).await;
            let current_status = Self::actor_state_label(actor_state, is_activated);

            // Apply status filter
            if !Self::actor_status_matches(&req.status, &current_status) {
                continue;
            }

            let actor_group = actor_configs
                .get(&actor_id)
                .and_then(|config| config.actor_groups.first())
                .cloned()
                .unwrap_or_default();

            if !req.actor_group.is_empty() && actor_group != req.actor_group {
                continue;
            }

            candidates.push(ActorCandidate {
                node_id: actor_id.node_id().to_string(),
                actor_id,
                actor_type,
                actor_group,
                namespace: entry_namespace,
                tenant_id: entry_tenant_id,
                status: current_status,
                behavior_kind,
            });
        }

        // Phase 2: paginate on the cheap candidate list, then enrich only the page subset.
        let total_size = candidates.len();
        let (offset, limit) = Self::page_window(req.page.as_ref());
        let start = offset.min(total_size);
        let end = (start + limit).min(total_size);
        let has_next = end < total_size;
        let page_response = PageResponse {
            total_size: total_size as i32,
            offset: start as i32,
            limit: limit as i32,
            has_next,
        };
        let page_candidates = &candidates[start..end];

        let mut actors = Vec::with_capacity(page_candidates.len());
        for c in page_candidates {
            let actor_id_str = c.actor_id.to_string();

            // Get journal metrics for durable actors
            let (journal_size_bytes, last_checkpoint) =
                self.get_durable_actor_metrics(&c.actor_id).await;

            // Get actor metrics
            let metrics = self.get_actor_metrics(&c.actor_id).await;

            // Get created_at from the ActorRef (set at spawn time).
            // ActorRef::local() records SystemTime::now() at construction — always present for live actors.
            let created_at = actor_registry
                .lookup_actor(&c.actor_id)
                .await
                .and_then(|sender| sender.created_at());

            actors.push(ActorInfo {
                actor_id: actor_id_str,
                actor_type: c.actor_type.clone(),
                actor_group: c.actor_group.clone(),
                namespace: c.namespace.clone(),
                tenant_id: c.tenant_id.clone(),
                node_id: c.node_id.clone(),
                status: c.status.clone(),
                metrics,
                journal_size_bytes,
                last_checkpoint,
                created_at,
                behavior_kind: c.behavior_kind.clone(),
            });
        }

        let paginated_actors = actors;

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

        let health_check = self
            .get_dependency_health_internal(include_non_critical)
            .await;

        Ok(Response::new(GetDependencyHealthResponse {
            health_check: Some(health_check),
            node_id: req.node_id,
        }))
    }

    async fn get_dashboard_metrics(
        &self,
        request: Request<GetDashboardMetricsRequest>,
    ) -> Result<Response<GetDashboardMetricsResponse>, Status> {
        let req = request.into_inner();
        let name_pattern = if req.name_pattern.trim().is_empty() {
            "*".to_string()
        } else {
            req.name_pattern
        };
        let mut label_filter = req.label_filter;
        if !req.namespace.is_empty() {
            label_filter
                .entry("namespace".to_string())
                .or_insert(req.namespace);
        }
        let include_defs = req.include_definitions;
        let include_text = req.include_prometheus_text;

        if let Some(access) = self.service_locator.get_metrics_service_access().await {
            let metrics = access
                .get_metrics_filtered(name_pattern.clone(), label_filter.clone())
                .await;
            let definitions = if include_defs {
                access
                    .list_metric_definitions_filtered(name_pattern.clone())
                    .await
            } else {
                vec![]
            };
            let prometheus_text = if include_text {
                access.export_prometheus_text().await
            } else {
                String::new()
            };
            return Ok(Response::new(GetDashboardMetricsResponse {
                metrics,
                definitions,
                prometheus_text,
            }));
        }

        if let Some(renderer) = self.service_locator.get_metrics_prometheus_renderer().await {
            let prometheus_text_full = renderer.render_prometheus_text();
            let metrics = crate::metrics_service::parse_prometheus_text(
                &prometheus_text_full,
                &name_pattern,
                &label_filter,
            );
            let definitions = if include_defs {
                crate::metrics_service::unified_metric_definitions()
                    .into_iter()
                    .filter(|d| crate::metrics_service::metric_name_matches(&name_pattern, &d.name))
                    .collect()
            } else {
                vec![]
            };
            let prometheus_text = if include_text {
                prometheus_text_full
            } else {
                String::new()
            };
            return Ok(Response::new(GetDashboardMetricsResponse {
                metrics,
                definitions,
                prometheus_text,
            }));
        }

        Err(Status::failed_precondition(
            "metrics service not registered (no Prometheus recorder on this node)",
        ))
    }

    async fn get_workflows(
        &self,
        request: Request<GetWorkflowsRequest>,
    ) -> Result<Response<GetWorkflowsResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        let mut request_for_context = Request::new(());
        *request_for_context.metadata_mut() = metadata;
        let is_admin = self.is_admin(&request_for_context).await;
        let tenant_filter = if !req.tenant_id.is_empty() {
            Some(req.tenant_id.clone())
        } else if is_admin {
            None
        } else {
            self.get_tenant_id_from_context(&request_for_context)
        };

        let ctx = self.dashboard_ctx_from_metadata(request_for_context.metadata()).await;
        let storage = self.workflow_storage().await?;
        let statuses = Self::workflow_statuses(req.status)?;
        let node_filter = (!req.node_id.is_empty()).then_some(req.node_id.as_str());
        let mut executions = storage
            .list_executions_by_status(&ctx, statuses, node_filter)
            .await
            .map_err(|error| {
                Status::internal(format!("Failed to list workflow executions: {error}"))
            })?;

        if !req.definition_id.is_empty() {
            executions.retain(|execution| execution.definition_id == req.definition_id);
        }

        // Workflow execution metadata is shared storage state and does not yet persist tenant_id.
        // Until workflow metadata carries tenant scope, only explicit tenant-scoped executions can
        // be filtered upstream by the workflow service itself.
        if tenant_filter
            .as_deref()
            .is_some_and(|tenant| tenant.is_empty())
        {
            executions.clear();
        }

        let mut workflows = Vec::with_capacity(executions.len());
        for execution in executions {
            let definition = storage
                .get_definition(&ctx, &execution.definition_id, &execution.definition_version)
                .await
                .ok();
            workflows.push(plexspaces_proto::dashboard::v1::WorkflowInfo {
                execution: Some(execution),
                definition,
            });
        }

        let (workflows, page_response) = Self::apply_pagination(workflows, req.page);

        Ok(Response::new(GetWorkflowsResponse {
            workflows,
            page: Some(page_response),
        }))
    }

    async fn get_objects(
        &self,
        request: Request<GetObjectsRequest>,
    ) -> Result<Response<GetObjectsResponse>, Status> {
        self.get_objects_impl(request).await
    }

    async fn get_key_values(
        &self,
        request: Request<GetKeyValuesRequest>,
    ) -> Result<Response<GetKeyValuesResponse>, Status> {
        self.get_key_values_impl(request).await
    }

    async fn get_tuple_spaces(
        &self,
        request: Request<GetTupleSpacesRequest>,
    ) -> Result<Response<GetTupleSpacesResponse>, Status> {
        self.get_tuple_spaces_impl(request).await
    }

    async fn get_blobs(
        &self,
        request: Request<GetBlobsRequest>,
    ) -> Result<Response<GetBlobsResponse>, Status> {
        self.get_blobs_impl(request).await
    }

    async fn get_blob_presigned_url(
        &self,
        request: Request<GetBlobPresignedUrlRequest>,
    ) -> Result<Response<GetBlobPresignedUrlResponse>, Status> {
        self.get_blob_presigned_url_impl(request).await
    }

    async fn get_service_links(
        &self,
        request: Request<GetServiceLinksRequest>,
    ) -> Result<Response<GetServiceLinksResponse>, Status> {
        self.get_service_links_impl(request).await
    }

    async fn get_metrics_table(
        &self,
        request: Request<GetMetricsTableRequest>,
    ) -> Result<Response<GetMetricsTableResponse>, Status> {
        self.get_metrics_table_impl(request).await
    }
}

impl DashboardServiceImpl {
    /// Query remote node via NodeRegistry (which internally uses ObjectRegistry with caching).
    /// Uses request-scoped context (no admin).
    async fn query_remote_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<ProtoNode, Status> {
        let node_registry = self
            .service_locator
            .get_node_registry()
            .await
            .ok_or_else(|| Status::internal("NodeRegistry not found in ServiceLocator"))?;

        let reg = node_registry
            .lookup_node(ctx, node_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to lookup node: {}", e)))?
            .ok_or_else(|| Status::not_found(format!("Node not found: {}", node_id)))?;

        Ok(ProtoNode {
            id: reg.node_id.clone(),
            node_type: NodeType::NodeTypeProcess as i32,
            status: reg.status,
            capabilities: None,
            metadata: None,
            created_at: reg.registered_at,
            last_heartbeat: reg.last_heartbeat,
            metrics: None,
            mtls_identity: None,
            public_certificate: vec![],
            auto_generate_certs: false,
            cluster_name: reg.capabilities.get("cluster").cloned().unwrap_or_default(),
            actor_ids: vec![],
        })
    }

    async fn workflow_storage(&self) -> Result<WorkflowStorage, Status> {
        let runtime_config = self
            .service_locator
            .get_runtime_config()
            .await
            .ok_or_else(|| {
                Status::failed_precondition("RuntimeConfig not registered in ServiceLocator")
            })?;
        let shared_db = runtime_config.db.ok_or_else(|| {
            Status::failed_precondition(
                "RuntimeConfig.db is required for workflow dashboard queries",
            )
        })?;

        match resolve_shared_db_backend(&shared_db).map_err(|error| {
            Status::failed_precondition(format!("Invalid shared database configuration: {error}"))
        })? {
            SharedDbBackend::Sqlite {
                connection_string, ..
            } => WorkflowStorage::new_sqlite(&connection_string)
                .await
                .map_err(|error| {
                    Status::internal(format!("Failed to open workflow SQLite storage: {error}"))
                }),
            SharedDbBackend::Postgres { connection_string } => {
                WorkflowStorage::new_postgres(&connection_string)
                    .await
                    .map_err(|error| {
                        Status::internal(format!(
                            "Failed to open workflow PostgreSQL storage: {error}"
                        ))
                    })
            }
        }
    }

    fn workflow_statuses(status: i32) -> Result<Vec<ExecutionStatus>, Status> {
        if status == 0 {
            return Ok(vec![
                ExecutionStatus::ExecutionStatusPending,
                ExecutionStatus::ExecutionStatusRunning,
                ExecutionStatus::ExecutionStatusCompleted,
                ExecutionStatus::ExecutionStatusFailed,
                ExecutionStatus::ExecutionStatusCancelled,
                ExecutionStatus::ExecutionStatusTimedOut,
            ]);
        }

        let status = ExecutionStatus::try_from(status)
            .map_err(|_| Status::invalid_argument("Invalid workflow status filter"))?;
        Ok(vec![status])
    }

    fn actor_state_label(state: Option<ProtoActorState>, activated: bool) -> String {
        match state {
            Some(ProtoActorState::ActorStateCreating) => "creating",
            Some(ProtoActorState::ActorStateActive) => "active",
            Some(ProtoActorState::ActorStateInactive) => "inactive",
            Some(ProtoActorState::ActorStateActivating) => "activating",
            Some(ProtoActorState::ActorStateDeactivating) => "deactivating",
            Some(ProtoActorState::ActorStateStopping) => "stopping",
            Some(ProtoActorState::ActorStateMigrating) => "migrating",
            Some(ProtoActorState::ActorStateFailed) => "failed",
            Some(ProtoActorState::ActorStateTerminated) => "terminated",
            Some(ProtoActorState::ActorStateUnspecified) | None => {
                if activated {
                    "active"
                } else {
                    "terminated"
                }
            }
        }
        .to_string()
    }

    fn actor_status_matches(filter: &str, current_status: &str) -> bool {
        match filter {
            "" => true,
            "running" => matches!(
                current_status,
                "active" | "activating" | "inactive" | "deactivating"
            ),
            "terminated" => matches!(current_status, "terminated" | "failed" | "stopping"),
            other => current_status == other,
        }
    }

    fn normalize_behavior_filter(value: &str) -> String {
        value.trim().to_ascii_lowercase().replace('-', "_")
    }

    fn is_builtin_behavior_kind(value: &str) -> bool {
        matches!(
            value,
            "gen_server" | "gen_event" | "gen_state_machine" | "workflow"
        )
    }

    fn actor_behavior_matches(filter: &str, behavior_kind: &str) -> bool {
        let filter = Self::normalize_behavior_filter(filter);
        if filter.is_empty() {
            return true;
        }
        match filter.as_str() {
            "builtin" => Self::is_builtin_behavior_kind(behavior_kind),
            "custom" => !behavior_kind.is_empty() && !Self::is_builtin_behavior_kind(behavior_kind),
            other => behavior_kind == other,
        }
    }

    async fn local_node_id_string(&self) -> String {
        self.service_locator
            .get_node_config()
            .await
            .map(|c| c.id)
            .unwrap_or_default()
    }

    fn is_local_node(&self, node_id: &str, local_id: &str) -> bool {
        Self::is_local_node_id(local_id, node_id)
    }

    pub(crate) fn is_local_node_id(local_id: &str, node_id: &str) -> bool {
        node_id.is_empty() || node_id == "local" || node_id == local_id
    }

    /// Build a [`RequestContext`] for dashboard RPCs from gRPC request metadata.
    ///
    /// `x-tenant-id` is populated by the auth interceptor from the JWT `tenant_id` claim
    /// when auth is enabled. `x-namespace` is set from request query parameters by the
    /// HTTP→gRPC gateway. Both default to an empty string (effectively system-scoped)
    /// when auth is disabled or the header is absent.
    async fn dashboard_ctx_from_metadata(
        &self,
        metadata: &tonic::metadata::MetadataMap,
    ) -> RequestContext {
        let auth_disabled = self.service_locator.is_auth_disabled().await;
        let tenant_id = metadata
            .get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let namespace = metadata
            .get("x-namespace")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let is_admin = auth_disabled
            || metadata
                .get("x-admin")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.eq_ignore_ascii_case("true"));
        RequestContext::new_without_auth(tenant_id, namespace).with_admin(is_admin)
    }

    /// Resolve the gRPC address for a remote node and return a pooled channel.
    ///
    /// Node registry lookups are infra-level (not tenant-scoped) so an empty tenant context
    /// is appropriate here — the registry stores node topology, not tenant data.
    async fn get_remote_channel(
        &self,
        node_id: &str,
        service_type: plexspaces_actor::grpc_connection_manager::ServiceType,
    ) -> Result<tonic::transport::Channel, Status> {
        let node_registry = self
            .service_locator
            .get_node_registry()
            .await
            .ok_or_else(|| Status::internal("NodeRegistry not available"))?;
        // Node lookup is topology/infra — not tenant-scoped.
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        let reg = node_registry
            .lookup_node(&ctx, node_id)
            .await
            .map_err(|e| Status::internal(format!("Node lookup failed: {e}")))?
            .ok_or_else(|| Status::not_found(format!("Node not found: {node_id}")))?;
        let addr = plexspaces_common::dialable_node_address(&reg.node_address);
        let conn_mgr = self
            .service_locator
            .get_grpc_connection_manager()
            .await
            .ok_or_else(|| Status::internal("GrpcConnectionManager not available"))?;
        conn_mgr
            .get_connection(service_type, node_id, &addr)
            .await
            .map_err(|e| Status::unavailable(format!("Cannot reach node {node_id}: {e}")))
    }
}

impl DashboardServiceImpl {
    async fn get_objects_impl(
        &self,
        request: Request<GetObjectsRequest>,
    ) -> Result<Response<GetObjectsResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        let local_id = self.local_node_id_string().await;
        let ctx = self.dashboard_ctx_from_metadata(&metadata).await;

        if self.is_local_node(&req.node_id, &local_id) {
            let registry = self
                .service_locator
                .get_object_registry()
                .await
                .ok_or_else(|| Status::unavailable("Object registry not available"))?;

            use plexspaces_proto::object_registry::v1::ObjectType;
            let object_type = if req.object_type.is_empty() {
                None
            } else {
                match req.object_type.to_uppercase().as_str() {
                    "ACTOR" => Some(ObjectType::ObjectTypeActor),
                    "TUPLESPACE" => Some(ObjectType::ObjectTypeTuplespace),
                    "SERVICE" => Some(ObjectType::ObjectTypeService),
                    "VM" => Some(ObjectType::ObjectTypeVm),
                    "APPLICATION" => Some(ObjectType::ObjectTypeApplication),
                    "WORKFLOW" => Some(ObjectType::ObjectTypeWorkflow),
                    "NODE" => Some(ObjectType::ObjectTypeNode),
                    _ => None,
                }
            };

            use plexspaces_proto::object_registry::v1::HealthStatus;
            let health_status = if req.health_status.is_empty() {
                None
            } else {
                match req.health_status.to_uppercase().as_str() {
                    "HEALTHY" => Some(HealthStatus::HealthStatusHealthy),
                    "DEGRADED" => Some(HealthStatus::HealthStatusDegraded),
                    "DEAD" => Some(HealthStatus::HealthStatusDead),
                    _ => None,
                }
            };

            let (offset, limit) = Self::page_window(req.page.as_ref());
            // Fetch limit+1 to determine has_next without a separate count query
            let fetch_limit = limit + 1;

            let registrations = registry
                .discover(
                    &ctx,
                    DiscoverOptions {
                        object_type,
                        health_status,
                        offset,
                        limit: fetch_limit,
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| Status::internal(format!("Object registry query failed: {e}")))?;

            let id_pat = req.id_pattern.to_lowercase();
            let has_next = registrations.len() > limit;
            let objects: Vec<plexspaces_proto::object_registry::v1::ObjectRegistration> =
                registrations.into_iter()
                    .filter(|r| id_pat.is_empty() || r.object_id.to_lowercase().contains(&id_pat))
                    .take(limit)
                    .map(|r| {
                    plexspaces_proto::object_registry::v1::ObjectRegistration {
                        object_id: r.object_id,
                        object_name: r.object_name,
                        object_type: r.object_type,
                        version: r.version,
                        tenant_id: r.tenant_id,
                        namespace: r.namespace,
                        node_id: r.node_id,
                        grpc_address: r.grpc_address,
                        object_category: r.object_category,
                        capabilities: r.capabilities,
                        metadata: None,
                        health_status: r.health_status,
                        last_heartbeat: r.last_heartbeat,
                        created_at: r.created_at,
                        ..Default::default()
                    }
                }).collect();

            let page_response = plexspaces_proto::common::v1::PageResponse {
                total_size: -1, // unknown without count query
                offset: offset as i32,
                limit: limit as i32,
                has_next,
            };

            Ok(Response::new(GetObjectsResponse {
                objects,
                page: Some(page_response),
            }))
        } else {
            // Remote node — forward via object registry gRPC client
            let channel = self
                .get_remote_channel(
                    &req.node_id,
                    plexspaces_actor::grpc_connection_manager::ServiceType::ServiceNameObjectRegistry,
                )
                .await?;
            let mut client = plexspaces_proto::object_registry::v1::object_registry_client::ObjectRegistryClient::new(channel);

            use plexspaces_proto::object_registry::v1::ObjectType as RemoteObjType;
            let discover_req = plexspaces_proto::object_registry::v1::DiscoverRequest {
                object_type: if req.object_type.is_empty() {
                    0
                } else {
                    match req.object_type.to_uppercase().as_str() {
                        "ACTOR" => RemoteObjType::ObjectTypeActor as i32,
                        "TUPLESPACE" => RemoteObjType::ObjectTypeTuplespace as i32,
                        "SERVICE" => RemoteObjType::ObjectTypeService as i32,
                        "VM" => RemoteObjType::ObjectTypeVm as i32,
                        "APPLICATION" => RemoteObjType::ObjectTypeApplication as i32,
                        "WORKFLOW" => RemoteObjType::ObjectTypeWorkflow as i32,
                        "NODE" => RemoteObjType::ObjectTypeNode as i32,
                        _ => 0,
                    }
                },
                tenant_id: ctx.tenant_id().to_string(),
                namespace: ctx.namespace().to_string(),
                page_size: req.page.as_ref().map(|p| p.limit).unwrap_or(50),
                ..Default::default()
            };
            let mut remote_req = tonic::Request::new(discover_req);
            *remote_req.metadata_mut() = metadata;
            let resp = client
                .discover(remote_req)
                .await
                .map_err(|e| Status::internal(format!("Remote object registry failed: {e}")))?;
            let inner = resp.into_inner();
            let has_more = inner.has_more;
            let total = inner.total_count;
            let (offset, limit) = Self::page_window(req.page.as_ref());
            Ok(Response::new(GetObjectsResponse {
                objects: inner.registrations,
                page: Some(plexspaces_proto::common::v1::PageResponse {
                    total_size: total as i32,
                    offset: offset as i32,
                    limit: limit as i32,
                    has_next: has_more,
                }),
            }))
        }
    }

    async fn get_key_values_impl(
        &self,
        request: Request<GetKeyValuesRequest>,
    ) -> Result<Response<GetKeyValuesResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        let local_id = self.local_node_id_string().await;
        let ctx = self.dashboard_ctx_from_metadata(&metadata).await;

        if self.is_local_node(&req.node_id, &local_id) {
            let kv = self
                .service_locator
                .get_keyvalue_store()
                .await
                .ok_or_else(|| Status::unavailable("Key/value store not available"))?;

            let all_keys = kv
                .list_keys(&ctx, &req.prefix)
                .await
                .map_err(|e| Status::internal(format!("KV list failed: {e}")))?;

            let (offset, limit) = Self::page_window(req.page.as_ref());
            let total_size = all_keys.len();
            let has_next = (offset + limit) < total_size;
            let page_keys: Vec<String> = all_keys.into_iter().skip(offset).take(limit).collect();

            let mut entries = Vec::with_capacity(page_keys.len());
            for key in &page_keys {
                let value_bytes = kv
                    .get(&ctx, key)
                    .await
                    .unwrap_or_default()
                    .unwrap_or_default();
                let size_bytes = value_bytes.len() as u64;
                let value_preview = String::from_utf8_lossy(&value_bytes[..value_bytes.len().min(100)]).to_string();
                entries.push(KeyValueDashboardEntry {
                    key: key.clone(),
                    value_preview,
                    size_bytes,
                });
            }

            Ok(Response::new(GetKeyValuesResponse {
                entries,
                page: Some(plexspaces_proto::common::v1::PageResponse {
                    total_size: total_size as i32,
                    offset: offset as i32,
                    limit: limit as i32,
                    has_next,
                }),
            }))
        } else {
            // Remote node — forward via KeyValue gRPC client
            let channel = self
                .get_remote_channel(
                    &req.node_id,
                    plexspaces_actor::grpc_connection_manager::ServiceType::ServiceNameKeyValueService,
                )
                .await?;
            let mut client =
                plexspaces_proto::keyvalue::v1::key_value_service_client::KeyValueServiceClient::new(channel);
            let list_req = plexspaces_proto::keyvalue::v1::ListRequest {
                prefix: req.prefix,
                namespace: req.namespace,
            };
            let mut remote_req = tonic::Request::new(list_req);
            *remote_req.metadata_mut() = metadata;
            let resp = client
                .list(remote_req)
                .await
                .map_err(|e| Status::internal(format!("Remote KV list failed: {e}")))?;
            let (offset, limit) = Self::page_window(req.page.as_ref());
            let keys = resp.into_inner().keys;
            let total_size = keys.len();
            let has_next = (offset + limit) < total_size;
            let page_keys: Vec<String> = keys.into_iter().skip(offset).take(limit).collect();
            let entries = page_keys.into_iter().map(|key| KeyValueDashboardEntry {
                key,
                value_preview: String::new(),
                size_bytes: 0,
            }).collect();
            Ok(Response::new(GetKeyValuesResponse {
                entries,
                page: Some(plexspaces_proto::common::v1::PageResponse {
                    total_size: total_size as i32,
                    offset: offset as i32,
                    limit: limit as i32,
                    has_next,
                }),
            }))
        }
    }

    async fn get_tuple_spaces_impl(
        &self,
        request: Request<GetTupleSpacesRequest>,
    ) -> Result<Response<GetTupleSpacesResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        let local_id = self.local_node_id_string().await;

        if self.is_local_node(&req.node_id, &local_id) {
            let provider = self
                .service_locator
                .get_tuplespace_provider()
                .await
                .ok_or_else(|| Status::unavailable("TupleSpace provider not available"))?;

            let namespace = if req.namespace.is_empty() {
                "default".to_string()
            } else {
                req.namespace.clone()
            };

            use plexspaces_tuplespace::{Pattern, PatternField};
            let wildcard = Pattern::new(vec![PatternField::Wildcard]);
            let count = provider.count(&wildcard).await.unwrap_or(0);
            let raw_tuples = provider.read(&wildcard).await.unwrap_or_default();
            // Convert to proto Tuple and take up to 20 as sample
            let sample_tuples: Vec<plexspaces_proto::tuplespace::v1::Tuple> = raw_tuples
                .into_iter()
                .take(20)
                .map(|t| plexspaces_tuplespace::proto_conversion::tuple_to_proto_tuple(&t))
                .collect();

            let summary = TupleSpaceSummary {
                namespace,
                pattern: req.pattern.clone(),
                tuple_count: count as u64,
                sample_tuples,
            };

            Ok(Response::new(GetTupleSpacesResponse {
                spaces: vec![summary],
            }))
        } else {
            // Remote node — forward via TupleSpace gRPC count call
            let channel = self
                .get_remote_channel(
                    &req.node_id,
                    plexspaces_actor::grpc_connection_manager::ServiceType::ServiceNameTuplespaceService,
                )
                .await?;
            let mut client =
                plexspaces_proto::tuplespace::v1::tuple_space_service_client::TupleSpaceServiceClient::new(channel);
            let count_req = plexspaces_proto::tuplespace::v1::CountRequest {
                template: None,
                ..Default::default()
            };
            let mut remote_req = tonic::Request::new(count_req);
            *remote_req.metadata_mut() = metadata;
            let count = client
                .count(remote_req)
                .await
                .map(|r| r.into_inner().count)
                .unwrap_or(0);
            Ok(Response::new(GetTupleSpacesResponse {
                spaces: vec![TupleSpaceSummary {
                    namespace: req.namespace,
                    pattern: req.pattern,
                    tuple_count: count as u64,
                    sample_tuples: vec![],
                }],
            }))
        }
    }

    async fn get_blobs_impl(
        &self,
        request: Request<GetBlobsRequest>,
    ) -> Result<Response<GetBlobsResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        let local_id = self.local_node_id_string().await;
        let ctx = self.dashboard_ctx_from_metadata(&metadata).await;

        if self.is_local_node(&req.node_id, &local_id) {
            let blob_svc = self
                .service_locator
                .get_blob_service()
                .await
                .ok_or_else(|| Status::unavailable("Blob service not available"))?;

            let (offset, limit) = Self::page_window(req.page.as_ref());
            let kind_filter = if req.kind.is_empty() { None } else { Some(req.kind.as_str()) };

            let (blobs, has_next) = blob_svc
                .list_metadata(&ctx, &req.prefix, kind_filter, offset, limit)
                .await
                .map_err(|e| Status::internal(format!("Blob list failed: {e}")))?;

            let page_response = plexspaces_proto::common::v1::PageResponse {
                total_size: -1,
                offset: offset as i32,
                limit: limit as i32,
                has_next,
            };

            Ok(Response::new(GetBlobsResponse {
                blobs,
                page: Some(page_response),
            }))
        } else {
            // Remote node — forward via Blob gRPC client
            let channel = self
                .get_remote_channel(
                    &req.node_id,
                    plexspaces_actor::grpc_connection_manager::ServiceType::ServiceNameBlobService,
                )
                .await?;
            let mut client =
                plexspaces_proto::storage::v1::blob_service_client::BlobServiceClient::new(channel);
            let (offset, limit) = Self::page_window(req.page.as_ref());
            let list_req = plexspaces_proto::storage::v1::ListBlobsRequest {
                name_prefix: req.prefix,
                kind: req.kind,
                namespace: req.namespace,
                page: Some(plexspaces_proto::common::v1::PageRequest {
                    offset: offset as i32,
                    limit: limit as i32,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let mut remote_req = tonic::Request::new(list_req);
            *remote_req.metadata_mut() = metadata;
            let resp = client
                .list_blobs(remote_req)
                .await
                .map_err(|e| Status::internal(format!("Remote blob list failed: {e}")))?;
            let inner = resp.into_inner();
            let page = inner.page;
            Ok(Response::new(GetBlobsResponse {
                blobs: inner.blobs,
                page,
            }))
        }
    }

    async fn get_blob_presigned_url_impl(
        &self,
        request: Request<GetBlobPresignedUrlRequest>,
    ) -> Result<Response<GetBlobPresignedUrlResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        let local_id = self.local_node_id_string().await;
        let ctx = self.dashboard_ctx_from_metadata(&metadata).await;

        if self.is_local_node(&req.node_id, &local_id) {
            let blob_svc = self
                .service_locator
                .get_blob_service()
                .await
                .ok_or_else(|| Status::unavailable("Blob service not available"))?;

            match blob_svc
                .generate_presigned_url(
                    &ctx,
                    &req.blob_id,
                    "GET",
                    std::time::Duration::from_secs(3600),
                )
                .await
            {
                Ok(Some(url)) => Ok(Response::new(GetBlobPresignedUrlResponse {
                    url,
                    error: String::new(),
                    expires_at: Some(prost_types::Timestamp {
                        seconds: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp(),
                        nanos: 0,
                    }),
                })),
                Ok(None) => Ok(Response::new(GetBlobPresignedUrlResponse {
                    url: String::new(),
                    error: "Presigned URLs not supported by this blob backend".to_string(),
                    expires_at: None,
                })),
                Err(e) => {
                    let msg = e.to_string();
                    let user_msg = if msg.contains("Access key") || msg.contains("credentials") || msg.contains("Configuration error") {
                        "Download unavailable: blob storage not configured for presigned URLs".to_string()
                    } else {
                        format!("Download failed: {e}")
                    };
                    Ok(Response::new(GetBlobPresignedUrlResponse {
                        url: String::new(),
                        error: user_msg,
                        expires_at: None,
                    }))
                }
            }
        } else {
            // Remote node — forward via Blob gRPC client
            let channel = self
                .get_remote_channel(
                    &req.node_id,
                    plexspaces_actor::grpc_connection_manager::ServiceType::ServiceNameBlobService,
                )
                .await?;
            let mut client =
                plexspaces_proto::storage::v1::blob_service_client::BlobServiceClient::new(channel);
            let presign_req = plexspaces_proto::storage::v1::GeneratePresignedUrlRequest {
                blob_id: req.blob_id,
                operation: "GET".to_string(),
                tenant_id: req.tenant_id,
                namespace: req.namespace,
                ..Default::default()
            };
            let mut remote_req = tonic::Request::new(presign_req);
            *remote_req.metadata_mut() = metadata;
            match client.generate_presigned_url(remote_req).await {
                Ok(resp) => {
                    let inner = resp.into_inner();
                    Ok(Response::new(GetBlobPresignedUrlResponse {
                        url: inner.url,
                        expires_at: inner.expires_at,
                        error: String::new(),
                    }))
                }
                Err(e) => Ok(Response::new(GetBlobPresignedUrlResponse {
                    url: String::new(),
                    error: format!("Remote presigned URL failed: {e}"),
                    expires_at: None,
                })),
            }
        }
    }

    async fn get_service_links_impl(
        &self,
        request: Request<GetServiceLinksRequest>,
    ) -> Result<Response<GetServiceLinksResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        let local_id = self.local_node_id_string().await;
        let ctx = self.dashboard_ctx_from_metadata(&metadata).await;

        if self.is_local_node(&req.node_id, &local_id) {
            let sls = self.service_locator.get_service_link_service().await;
            let service_links = if let Some(sls) = sls {
                sls.list_links(&ctx)
                    .await
                    .map_err(|e| Status::internal(format!("Service link list failed: {e}")))?
            } else {
                // Fall back to static RuntimeConfig if live service not registered
                self.service_locator
                    .get_runtime_config()
                    .await
                    .map(|rc| rc.service_links)
                    .unwrap_or_default()
            };
            Ok(Response::new(GetServiceLinksResponse { service_links }))
        } else {
            // Remote node — forward via ServiceLink gRPC client
            let channel = self
                .get_remote_channel(
                    &req.node_id,
                    plexspaces_actor::grpc_connection_manager::ServiceType::ServiceNameServiceLinkService,
                )
                .await?;
            let mut client =
                plexspaces_proto::node::v1::service_link_service_client::ServiceLinkServiceClient::new(channel);
            let list_req = plexspaces_proto::node::v1::ListServiceLinksRequest {
                page_size: 1000,
                ..Default::default()
            };
            let mut remote_req = tonic::Request::new(list_req);
            *remote_req.metadata_mut() = metadata;
            let resp = client
                .list_service_links(remote_req)
                .await
                .map_err(|e| Status::internal(format!("Remote service links failed: {e}")))?;
            Ok(Response::new(GetServiceLinksResponse {
                service_links: resp.into_inner().links,
            }))
        }
    }

    async fn get_metrics_table_impl(
        &self,
        request: Request<GetMetricsTableRequest>,
    ) -> Result<Response<GetMetricsTableResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();
        let local_id = self.local_node_id_string().await;

        if self.is_local_node(&req.node_id, &local_id) {
            // Re-use GetDashboardMetrics internally
            let dash_req = GetDashboardMetricsRequest {
                namespace: req.namespace,
                name_pattern: req.name_pattern,
                label_filter: req.label_filter,
                include_definitions: true,
                include_prometheus_text: false,
            };
            let mut inner_request = tonic::Request::new(dash_req);
            *inner_request.metadata_mut() = metadata;
            let resp = self.get_dashboard_metrics(inner_request).await?;
            let inner = resp.into_inner();
            Ok(Response::new(GetMetricsTableResponse {
                metrics: inner.metrics,
                definitions: inner.definitions,
            }))
        } else {
            // Remote node — forward via MetricsService gRPC client
            let channel = self
                .get_remote_channel(
                    &req.node_id,
                    plexspaces_actor::grpc_connection_manager::ServiceType::ServiceNameMetricsService,
                )
                .await?;
            let mut client =
                plexspaces_proto::metrics::v1::metrics_service_client::MetricsServiceClient::new(channel);
            let get_req = plexspaces_proto::metrics::v1::GetMetricsRequest {
                name_pattern: req.name_pattern,
                label_filter: req.label_filter,
            };
            let mut remote_req = tonic::Request::new(get_req);
            *remote_req.metadata_mut() = metadata;
            let resp = client
                .get_metrics(remote_req)
                .await
                .map_err(|e| Status::internal(format!("Remote metrics failed: {e}")))?;
            let inner = resp.into_inner();
            Ok(Response::new(GetMetricsTableResponse {
                metrics: inner.metrics,
                definitions: vec![],
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

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
            node_id: String::new(),
            tenant_id: String::new(),
            namespace: String::new(),
            actor_id_pattern: String::new(),
            actor_group: String::new(),
            actor_type: String::new(),
            status: String::new(),
            since: None,
            page: None,
            behavior_kind: String::new(),
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

        let (paginated, page_response) =
            DashboardServiceImpl::apply_pagination(items, page_request);

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

        let (paginated, page_response) =
            DashboardServiceImpl::apply_pagination(items, page_request);

        assert_eq!(paginated.len(), 5);
        assert_eq!(paginated[0], 10);
        assert_eq!(page_response.total_size, 15);
        assert_eq!(page_response.offset, 10);
        assert_eq!(page_response.limit, 10);
        assert!(!page_response.has_next);
    }

    #[test]
    fn test_actor_namespace_is_read_from_structured_actor_id() {
        let actor_id =
            ActorId::from_canonical("actor//worker::tenant-a@node").expect("canonical actor id");
        assert_eq!(actor_id.namespace(), "tenant-a");

        let actor_id2 =
            ActorId::from_canonical("actor//worker::default@node").expect("canonical actor id");
        assert_eq!(actor_id2.namespace(), "default");
    }

    #[tokio::test]
    async fn test_default_since() {
        let since = DashboardServiceImpl::default_since();
        let now = Utc::now();
        let since_dt = DateTime::<Utc>::from_timestamp(since.seconds, since.nanos as u32).unwrap();

        // Should be approximately 24 hours ago
        let diff = now.signed_duration_since(since_dt);
        let hours = diff.num_hours();
        assert!(hours >= 23 && hours <= 25);
    }

    #[test]
    fn test_is_local_node_empty_node_id_is_local() {
        // Empty node_id always routes locally
        assert!(DashboardServiceImpl::is_local_node_id("local-node", ""));
    }

    #[test]
    fn test_is_local_node_matching_is_local() {
        assert!(DashboardServiceImpl::is_local_node_id("my-node-123", "my-node-123"));
    }

    #[test]
    fn test_is_local_node_different_id_is_remote() {
        assert!(!DashboardServiceImpl::is_local_node_id("node-a", "node-b"));
    }

    #[test]
    fn test_is_local_node_keyword_local() {
        assert!(DashboardServiceImpl::is_local_node_id("any-node-id", "local"));
    }
}
