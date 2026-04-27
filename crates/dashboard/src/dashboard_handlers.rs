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

//! Dashboard HTTP Handlers
//!
//! ## Purpose
//! Serves dashboard HTML pages and handles dashboard API requests.
//! Routes:
//! - GET / → Home page
//! - GET /node/{node_id} → Node detail page
//! - GET /static/* → Static assets (JS, CSS)
//! - GET /api/v1/dashboard/* → Dashboard API endpoints (UI; not a substitute for ApplicationService HTTP)

use axum::{
    extract::{Path, Query},
    http::{header, HeaderMap, StatusCode},
    response::{Html, Json, Response},
    routing::{get, post},
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;

use plexspaces_core::{
    local_prometheus_recorder_chart_summary, request_context_from_grpc_request, ActorId,
    ServiceLocator,
};
use plexspaces_proto::application::v1::ApplicationMetrics;
use plexspaces_proto::common::v1::PageRequest;
use plexspaces_proto::dashboard::v1::{
    dashboard_service_server::DashboardService, GetActorsRequest, GetApplicationsRequest,
    GetDashboardMetricsRequest, GetDependencyHealthRequest, GetNodeDashboardRequest,
    GetNodesRequest, GetSummaryRequest,
};
use plexspaces_proto::object_registry::v1::ObjectType;
use plexspaces_services::actor_service::ActorServiceImpl;
use plexspaces_services::dashboard_service::DashboardServiceImpl;
use tonic::Request;

/// Shared HTTP gateway state type (must match node's HttpGatewayState for router merge).
pub type HttpGatewayState = (
    Arc<ActorServiceImpl>,
    bool,
    Option<String>,
    Arc<dyn ServiceLocator>,
    Option<Arc<DashboardServiceImpl>>,
);

/// Create dashboard router with unified gateway state type so node can merge it.
/// Node calls .with_state(gateway_state) once after merging to get Router<()> for serve.
pub fn create_dashboard_router() -> Router<HttpGatewayState> {
    Router::new()
        .route("/", get(home_page))
        .route("/dashboard", get(home_page)) // Alias for home
        .route("/dashboard/node/:node_id", get(node_page))
        .route("/node/:node_id", get(node_page)) // Also support without /dashboard prefix
        .route("/dashboard/application/:name", get(application_page))
        .route("/dashboard/tenant/:tenant_id", get(tenant_page))
        .route("/static/dashboard.css", get(serve_css))
        .route("/static/dashboard.js", get(serve_js))
        .route("/api/v1/dashboard/summary", get(api_summary))
        .route("/api/v1/dashboard/nodes", get(api_nodes))
        .route("/api/v1/dashboard/node/:node_id", get(api_node_dashboard))
        .route(
            "/api/v1/dashboard/local-recorder-summary",
            get(api_local_recorder_summary),
        )
        .route("/api/v1/dashboard/applications", get(api_applications))
        .route("/api/v1/dashboard/tenants", get(api_tenants))
        .route(
            "/api/v1/dashboard/application/:name",
            get(api_application_detail),
        )
        .route("/api/v1/dashboard/actors", get(api_actors))
        .route("/api/v1/dashboard/actor/:actor_id", get(api_actor_detail))
        .route(
            "/api/v1/dashboard/actor/:actor_id/stop",
            post(api_actor_stop),
        )
        .route("/api/v1/dashboard/dependencies", get(api_dependencies))
        .route("/api/v1/dashboard/system-info", get(api_system_info))
}

/// Home page handler
async fn home_page() -> Html<&'static str> {
    Html(include_str!("../static/dashboard/home.html"))
}

/// Node detail page handler
async fn node_page(Path(node_id): Path<String>) -> Result<Html<String>, StatusCode> {
    // Replace all :node_id placeholders in HTML with actual node_id
    let html = include_str!("../static/dashboard/node.html").replace(":node_id", &node_id);
    Ok(Html(html))
}

/// Application detail page handler.
async fn application_page(Path(name): Path<String>) -> Result<Html<String>, StatusCode> {
    let html =
        include_str!("../static/dashboard/application.html").replace(":application_id", &name);
    Ok(Html(html))
}

/// Tenant detail page handler.
async fn tenant_page(Path(tenant_id): Path<String>) -> Result<Html<String>, StatusCode> {
    let html = include_str!("../static/dashboard/tenant.html").replace(":tenant_id", &tenant_id);
    Ok(Html(html))
}

/// Serve CSS file
async fn serve_css() -> Response<String> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/css")
        .body(include_str!("../static/dashboard.css").to_string())
        .unwrap()
}

/// Serve JavaScript file
async fn serve_js() -> Response<String> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/javascript")
        .body(include_str!("../static/dashboard.js").to_string())
        .unwrap()
}

fn dashboard_request<T>(payload: T, headers: &HeaderMap) -> Request<T> {
    let mut request = Request::new(payload);
    for header_name in [
        "x-tenant-id",
        "x-namespace",
        "x-admin",
        "x-user-role",
        "x-user-roles",
    ] {
        if let Some(value) = headers.get(header_name) {
            if let Ok(value) = value.to_str() {
                if let Ok(metadata) = tonic::metadata::MetadataValue::try_from(value) {
                    request.metadata_mut().insert(header_name, metadata);
                }
            }
        }
    }
    request
}

fn node_metrics_to_json(metrics: &plexspaces_proto::node::v1::NodeMetrics) -> serde_json::Value {
    serde_json::json!({
        "node_id": metrics.node_id,
        "cluster_name": metrics.cluster_name,
        "memory_used_bytes": metrics.memory_used_bytes,
        "memory_available_bytes": metrics.memory_available_bytes,
        "cpu_usage_percent": metrics.cpu_usage_percent,
        "uptime_seconds": metrics.uptime_seconds,
        "messages_routed": metrics.messages_routed,
        "local_deliveries": metrics.local_deliveries,
        "remote_deliveries": metrics.remote_deliveries,
        "failed_deliveries": metrics.failed_deliveries,
        "active_actors": metrics.active_actors,
        "connected_nodes": metrics.connected_nodes,
        "shard_groups_created": metrics.shard_groups_created,
        "shard_messages_sent": metrics.shard_messages_sent,
        "shard_messages_received": metrics.shard_messages_received,
        "shard_operations_total": metrics.shard_operations_total,
        "shard_operations_failed": metrics.shard_operations_failed,
    })
}

fn node_to_json(node: &plexspaces_proto::node::v1::Node) -> serde_json::Value {
    let mut node_json = serde_json::Map::new();
    node_json.insert("id".to_string(), serde_json::Value::String(node.id.clone()));
    node_json.insert(
        "cluster_name".to_string(),
        serde_json::Value::String(node.cluster_name.clone()),
    );
    node_json.insert(
        "status".to_string(),
        serde_json::Value::Number(node.status.into()),
    );
    if let Some(last_heartbeat) = &node.last_heartbeat {
        node_json.insert(
            "last_heartbeat".to_string(),
            serde_json::json!({
                "seconds": last_heartbeat.seconds,
                "nanos": last_heartbeat.nanos,
            }),
        );
    }
    if let Some(created_at) = &node.created_at {
        node_json.insert(
            "created_at".to_string(),
            serde_json::json!({
                "seconds": created_at.seconds,
                "nanos": created_at.nanos,
            }),
        );
    }
    if let Some(metrics) = &node.metrics {
        node_json.insert("metrics".to_string(), node_metrics_to_json(metrics));
    }
    serde_json::Value::Object(node_json)
}

#[cfg(test)]
mod tests {
    use super::{dashboard_request, node_to_json};
    use axum::http::HeaderMap;
    use plexspaces_proto::node::v1::{Node, NodeMetrics};

    #[test]
    fn dashboard_request_forwards_identity_and_scope_metadata() {
        let mut headers = HeaderMap::new();
        headers.insert("x-tenant-id", "tenant-a".parse().unwrap());
        headers.insert("x-namespace", "ns-a".parse().unwrap());
        headers.insert("x-admin", "true".parse().unwrap());
        headers.insert("x-user-role", "admin".parse().unwrap());
        headers.insert("x-user-roles", "admin,developer".parse().unwrap());

        let request = dashboard_request((), &headers);

        assert_eq!(
            request
                .metadata()
                .get("x-tenant-id")
                .unwrap()
                .to_str()
                .unwrap(),
            "tenant-a"
        );
        assert_eq!(
            request
                .metadata()
                .get("x-namespace")
                .unwrap()
                .to_str()
                .unwrap(),
            "ns-a"
        );
        assert_eq!(
            request.metadata().get("x-admin").unwrap().to_str().unwrap(),
            "true"
        );
        assert_eq!(
            request
                .metadata()
                .get("x-user-role")
                .unwrap()
                .to_str()
                .unwrap(),
            "admin"
        );
        assert_eq!(
            request
                .metadata()
                .get("x-user-roles")
                .unwrap()
                .to_str()
                .unwrap(),
            "admin,developer"
        );
    }

    #[test]
    fn node_to_json_includes_metrics_used_by_dashboard_tables() {
        let node = Node {
            id: "node-a".to_string(),
            cluster_name: "cluster-a".to_string(),
            status: 1,
            metrics: Some(NodeMetrics {
                node_id: "node-a".to_string(),
                cluster_name: "cluster-a".to_string(),
                cpu_usage_percent: 37.5,
                memory_used_bytes: 256,
                memory_available_bytes: 768,
                messages_routed: 42,
                failed_deliveries: 3,
                active_actors: 9,
                ..Default::default()
            }),
            ..Default::default()
        };

        let value = node_to_json(&node);

        assert_eq!(value["id"], "node-a");
        assert_eq!(value["cluster_name"], "cluster-a");
        assert_eq!(value["metrics"]["messages_routed"], 42);
        assert_eq!(value["metrics"]["active_actors"], 9);
        assert_eq!(value["metrics"]["failed_deliveries"], 3);
        assert_eq!(value["metrics"]["cpu_usage_percent"], 37.5);
    }
}

/// API: Get summary
async fn api_summary(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        _service_locator,
        dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Query(_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Parse query parameters
    let tenant_id = _params.get("tenant_id").cloned().unwrap_or_default();
    let node_id = _params.get("node_id").cloned().unwrap_or_default();
    let cluster_id = _params.get("cluster_id").cloned().unwrap_or_default();

    // Create gRPC request
    let request = dashboard_request(
        GetSummaryRequest {
            tenant_id,
            node_id,
            cluster_id,
            since: None,
        },
        &headers,
    );

    // Call DashboardService
    let response = DashboardService::get_summary(dashboard_service.as_ref(), request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Convert to JSON using prost_types JSON encoding
    let summary = response.into_inner();

    // Convert proto to JSON manually since proto types don't implement Serialize
    let mut json = serde_json::Map::new();
    json.insert(
        "total_clusters".to_string(),
        serde_json::Value::Number(summary.total_clusters.into()),
    );
    json.insert(
        "total_nodes".to_string(),
        serde_json::Value::Number(summary.total_nodes.into()),
    );
    json.insert(
        "total_tenants".to_string(),
        serde_json::Value::Number(summary.total_tenants.into()),
    );
    json.insert(
        "total_applications".to_string(),
        serde_json::Value::Number(summary.total_applications.into()),
    );

    // Convert actors_by_type HashMap
    let actors_map: serde_json::Map<String, serde_json::Value> = summary
        .actors_by_type
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::Number(v.into())))
        .collect();
    json.insert(
        "actors_by_type".to_string(),
        serde_json::Value::Object(actors_map),
    );

    // Convert timestamps
    if let Some(since) = summary.since {
        json.insert(
            "since".to_string(),
            serde_json::json!({
                "seconds": since.seconds,
                "nanos": since.nanos,
            }),
        );
    }
    if let Some(until) = summary.until {
        json.insert(
            "until".to_string(),
            serde_json::json!({
                "seconds": until.seconds,
                "nanos": until.nanos,
            }),
        );
    }

    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get nodes
async fn api_nodes(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        _service_locator,
        dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Query(_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let tenant_id = _params.get("tenant_id").cloned().unwrap_or_default();
    let cluster_id = _params.get("cluster_id").cloned().unwrap_or_default();

    // Parse pagination params
    let offset = _params
        .get("offset")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    let limit = _params
        .get("limit")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(50);

    let page_request = Some(PageRequest {
        offset,
        limit,
        filter: String::new(),
        order_by: String::new(),
    });

    let request = dashboard_request(
        GetNodesRequest {
            tenant_id,
            cluster_id,
            page: page_request,
        },
        &headers,
    );

    let response = DashboardService::get_nodes(dashboard_service.as_ref(), request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let nodes_response = response.into_inner();

    // Convert nodes to JSON manually
    let nodes: Vec<serde_json::Value> = nodes_response.nodes.iter().map(node_to_json).collect();

    let mut json = serde_json::Map::new();
    json.insert("nodes".to_string(), serde_json::Value::Array(nodes));

    // Include page response
    if let Some(page) = nodes_response.page {
        let mut page_json = serde_json::Map::new();
        page_json.insert(
            "total_size".to_string(),
            serde_json::Value::Number(page.total_size.into()),
        );
        page_json.insert(
            "offset".to_string(),
            serde_json::Value::Number(page.offset.into()),
        );
        page_json.insert(
            "limit".to_string(),
            serde_json::Value::Number(page.limit.into()),
        );
        page_json.insert(
            "has_next".to_string(),
            serde_json::Value::Bool(page.has_next),
        );
        json.insert("page".to_string(), serde_json::Value::Object(page_json));
    }

    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get node dashboard
async fn api_node_dashboard(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        _service_locator,
        dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Path(node_id): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let request = dashboard_request(
        GetNodeDashboardRequest {
            node_id,
            since: None,
        },
        &headers,
    );

    let response = DashboardService::get_node_dashboard(dashboard_service.as_ref(), request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let dashboard = response.into_inner();

    // Convert to JSON manually - proto types don't implement Serialize
    let mut json = serde_json::Map::new();

    // Convert node
    if let Some(node) = dashboard.node {
        json.insert("node".to_string(), node_to_json(&node));
    }

    // Convert node metrics
    if let Some(metrics) = dashboard.node_metrics {
        json.insert("node_metrics".to_string(), node_metrics_to_json(&metrics));
    }

    // Convert summary
    if let Some(summary) = dashboard.summary {
        let mut summary_json = serde_json::Map::new();
        summary_json.insert(
            "total_tenants".to_string(),
            serde_json::Value::Number(summary.total_tenants.into()),
        );
        summary_json.insert(
            "total_applications".to_string(),
            serde_json::Value::Number(summary.total_applications.into()),
        );
        let actors_map: serde_json::Map<String, serde_json::Value> = summary
            .actors_by_type
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::Number(v.into())))
            .collect();
        summary_json.insert(
            "actors_by_type".to_string(),
            serde_json::Value::Object(actors_map),
        );
        json.insert(
            "summary".to_string(),
            serde_json::Value::Object(summary_json),
        );
    }

    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Chart aggregates from the **local** process Prometheus recorder (`GetDashboardMetrics` pipeline).
///
/// If `node_id` is set and does not match this process's configured node id, returns zeros with
/// `scope: "remote"` (only the local recorder is available to this handler).
async fn api_local_recorder_summary(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        service_locator,
        dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let requested = params.get("node_id").cloned().unwrap_or_default();
    let local_id = service_locator
        .get_node_config()
        .await
        .map(|c| c.id)
        .unwrap_or_default();
    if !requested.is_empty() && !local_id.is_empty() && requested != local_id {
        return Ok(Json(serde_json::json!({
            "scope": "remote",
            "message_routing_latency_avg_ms": 0.0,
            "message_routing_latency_max_ms": 0.0,
            "actor_message_processing_latency_avg_ms": 0.0,
            "actor_message_processing_latency_max_ms": 0.0,
            "application_supervisors_total": 0,
        })));
    }

    let req = dashboard_request(
        GetDashboardMetricsRequest {
            namespace: String::new(),
            name_pattern: "*".to_string(),
            label_filter: HashMap::new(),
            include_definitions: false,
            include_prometheus_text: true,
        },
        &headers,
    );
    let text = dashboard
        .get_dashboard_metrics(req)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_inner()
        .prometheus_text;
    let s = local_prometheus_recorder_chart_summary(&text);
    Ok(Json(serde_json::json!({
        "scope": "local",
        "message_routing_latency_avg_ms": s.message_routing_latency_avg_ms,
        "message_routing_latency_max_ms": s.message_routing_latency_max_ms,
        "actor_message_processing_latency_avg_ms": s.actor_message_processing_latency_avg_ms,
        "actor_message_processing_latency_max_ms": s.actor_message_processing_latency_max_ms,
        "application_supervisors_total": s.application_supervisors_total,
    })))
}

/// API: Get applications
async fn api_applications(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        _service_locator,
        dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Query(_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Parse pagination params
    let offset = _params
        .get("offset")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    let limit = _params
        .get("limit")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(50);

    let page_request = Some(PageRequest {
        offset,
        limit,
        filter: String::new(),
        order_by: String::new(),
    });

    let request = dashboard_request(
        GetApplicationsRequest {
            node_id: _params.get("node_id").cloned().unwrap_or_default(),
            tenant_id: _params.get("tenant_id").cloned().unwrap_or_default(),
            namespace: _params.get("namespace").cloned().unwrap_or_default(),
            name_pattern: _params.get("name_pattern").cloned().unwrap_or_default(),
            page: page_request,
        },
        &headers,
    );

    let response = dashboard_service
        .get_applications(request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let apps_response = response.into_inner();

    // Convert applications to JSON manually
    // Get namespace and tenant_id for each application from ApplicationManager
    let mut apps: Vec<serde_json::Value> = Vec::new();
    for app in &apps_response.applications {
        let mut app_json = serde_json::Map::new();
        app_json.insert(
            "application_id".to_string(),
            serde_json::Value::String(app.application_id.clone()),
        );
        app_json.insert(
            "name".to_string(),
            serde_json::Value::String(app.name.clone()),
        );
        app_json.insert(
            "version".to_string(),
            serde_json::Value::String(app.version.clone()),
        );
        app_json.insert(
            "status".to_string(),
            serde_json::Value::Number(app.status.into()),
        );

        app_json.insert(
            "namespace".to_string(),
            serde_json::Value::String(app.namespace.clone()),
        );
        app_json.insert(
            "tenant_id".to_string(),
            serde_json::Value::String(app.tenant_id.clone()),
        );

        if let Some(deployed_at) = &app.deployed_at {
            app_json.insert(
                "created_at".to_string(),
                serde_json::json!({
                    "seconds": deployed_at.seconds,
                    "nanos": deployed_at.nanos,
                }),
            );
        }
        if let Some(ref m) = app.metrics {
            app_json.insert("metrics".to_string(), application_metrics_to_json(m));
        }
        apps.push(serde_json::Value::Object(app_json));
    }

    let mut json = serde_json::Map::new();
    json.insert("applications".to_string(), serde_json::Value::Array(apps));

    // Include page response
    if let Some(page) = apps_response.page {
        let mut page_json = serde_json::Map::new();
        page_json.insert(
            "total_size".to_string(),
            serde_json::Value::Number(page.total_size.into()),
        );
        page_json.insert(
            "offset".to_string(),
            serde_json::Value::Number(page.offset.into()),
        );
        page_json.insert(
            "limit".to_string(),
            serde_json::Value::Number(page.limit.into()),
        );
        page_json.insert(
            "has_next".to_string(),
            serde_json::Value::Bool(page.has_next),
        );
        json.insert("page".to_string(), serde_json::Value::Object(page_json));
    }

    Ok(Json(serde_json::Value::Object(json)))
}

/// API: List tenants visible to the current caller.
async fn api_tenants(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        service_locator,
        _dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Query(_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let object_registry = service_locator
        .get_object_registry()
        .await
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let offset = _params
        .get("offset")
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0)
        .max(0) as usize;
    let limit = _params
        .get("limit")
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(25)
        .clamp(1, 1000) as usize;
    let request = dashboard_request((), &headers);
    let ctx =
        request_context_from_grpc_request(request.metadata(), &HashMap::new(), &service_locator)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let total_size = object_registry
        .count_tenant_ids_by_object_type(&ctx, ObjectType::ObjectTypeApplication)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let tenants = object_registry
        .list_tenant_ids_by_object_type(&ctx, ObjectType::ObjectTypeApplication, offset, limit)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .filter(|tenant_id| !tenant_id.is_empty())
        .map(|tenant_id| serde_json::json!({ "tenant_id": tenant_id }))
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "tenants": tenants,
        "page": {
            "total_size": total_size,
            "offset": offset,
            "limit": limit,
            "has_next": offset + limit < total_size,
        }
    })))
}

/// Serialize [`ApplicationMetrics`] for HTTP JSON (matches WASM host / `application_metrics_add` shape).
fn application_metrics_to_json(metrics: &ApplicationMetrics) -> serde_json::Value {
    serde_json::json!({
        "actor_counts": metrics.actor_counts,
        "supervisor_count": metrics.supervisor_count,
        "uptime_seconds": metrics.uptime_seconds,
        "message_count": metrics.message_count,
        "error_count": metrics.error_count,
        "counter_metrics": metrics.counter_metrics,
        "latency_totals_ms": metrics.latency_totals_ms,
        "latency_max_ms": metrics.latency_max_ms,
        "latency_samples": metrics.latency_samples,
    })
}

fn actor_info_to_json(actor: &plexspaces_proto::dashboard::v1::ActorInfo) -> serde_json::Value {
    let mut actor_json = serde_json::Map::new();
    actor_json.insert(
        "actor_id".to_string(),
        serde_json::Value::String(actor.actor_id.clone()),
    );
    actor_json.insert(
        "actor_type".to_string(),
        serde_json::Value::String(actor.actor_type.clone()),
    );
    actor_json.insert(
        "actor_group".to_string(),
        serde_json::Value::String(actor.actor_group.clone()),
    );
    actor_json.insert(
        "namespace".to_string(),
        serde_json::Value::String(actor.namespace.clone()),
    );
    actor_json.insert(
        "tenant_id".to_string(),
        serde_json::Value::String(actor.tenant_id.clone()),
    );
    actor_json.insert(
        "node_id".to_string(),
        serde_json::Value::String(actor.node_id.clone()),
    );
    actor_json.insert(
        "status".to_string(),
        serde_json::Value::String(actor.status.clone()),
    );
    actor_json.insert(
        "behavior_kind".to_string(),
        serde_json::Value::String(actor.behavior_kind.clone()),
    );
    actor_json.insert(
        "current_status".to_string(),
        serde_json::Value::String(actor.status.clone()),
    );
    let exit_status = match actor.status.as_str() {
        "failed" | "terminated" => actor.status.clone(),
        _ => String::new(),
    };
    actor_json.insert(
        "exit_status".to_string(),
        serde_json::Value::String(exit_status),
    );
    actor_json.insert(
        "journal_size_bytes".to_string(),
        serde_json::Value::Number(actor.journal_size_bytes.into()),
    );
    if let Some(checkpoint) = &actor.last_checkpoint {
        actor_json.insert(
            "last_checkpoint".to_string(),
            serde_json::json!({
                "seconds": checkpoint.seconds,
                "nanos": checkpoint.nanos,
            }),
        );
    }
    if let Some(created_at) = &actor.created_at {
        actor_json.insert(
            "created_at".to_string(),
            serde_json::json!({
                "seconds": created_at.seconds,
                "nanos": created_at.nanos,
            }),
        );
    }
    actor_json.insert(
        "metrics".to_string(),
        serde_json::json!({
            "messages_routed": actor.metrics.as_ref().map(|m| m.messages_routed).unwrap_or_default(),
            "local_deliveries": actor.metrics.as_ref().map(|m| m.local_deliveries).unwrap_or_default(),
            "remote_deliveries": actor.metrics.as_ref().map(|m| m.remote_deliveries).unwrap_or_default(),
            "failed_deliveries": actor.metrics.as_ref().map(|m| m.failed_deliveries).unwrap_or_default(),
            "error_total": actor.metrics.as_ref().map(|m| m.error_total).unwrap_or_default(),
            "spawn_total": actor.metrics.as_ref().map(|m| m.spawn_total).unwrap_or_default(),
        }),
    );
    serde_json::Value::Object(actor_json)
}

/// API: Get actors
async fn api_actors(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        _service_locator,
        dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Query(_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Parse pagination params
    let offset = _params
        .get("offset")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    let limit = _params
        .get("limit")
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(50);

    let page_request = Some(PageRequest {
        offset,
        limit,
        filter: String::new(),
        order_by: String::new(),
    });

    let request = dashboard_request(
        GetActorsRequest {
            node_id: _params.get("node_id").cloned().unwrap_or_default(),
            tenant_id: _params.get("tenant_id").cloned().unwrap_or_default(),
            namespace: _params.get("namespace").cloned().unwrap_or_default(),
            actor_id_pattern: _params.get("actor_id_pattern").cloned().unwrap_or_default(),
            actor_group: _params.get("actor_group").cloned().unwrap_or_default(),
            actor_type: _params.get("actor_type").cloned().unwrap_or_default(),
            status: _params.get("status").cloned().unwrap_or_default(),
            since: None,
            page: page_request,
            behavior_kind: _params.get("behavior_kind").cloned().unwrap_or_default(),
        },
        &headers,
    );

    let response = dashboard_service
        .get_actors(request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let actors_response = response.into_inner();

    let actors: Vec<serde_json::Value> = actors_response
        .actors
        .iter()
        .map(actor_info_to_json)
        .collect();

    let mut json = serde_json::Map::new();
    json.insert("actors".to_string(), serde_json::Value::Array(actors));

    // Include page response
    if let Some(page) = actors_response.page {
        let mut page_json = serde_json::Map::new();
        page_json.insert(
            "total_size".to_string(),
            serde_json::Value::Number(page.total_size.into()),
        );
        page_json.insert(
            "offset".to_string(),
            serde_json::Value::Number(page.offset.into()),
        );
        page_json.insert(
            "limit".to_string(),
            serde_json::Value::Number(page.limit.into()),
        );
        page_json.insert(
            "has_next".to_string(),
            serde_json::Value::Bool(page.has_next),
        );
        json.insert("page".to_string(), serde_json::Value::Object(page_json));
    }

    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get dependencies
async fn api_dependencies(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        _service_locator,
        dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Query(_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let include_non_critical = _params
        .get("include_non_critical")
        .and_then(|s| s.parse().ok())
        .unwrap_or(true);

    let request = dashboard_request(
        GetDependencyHealthRequest {
            node_id: _params.get("node_id").cloned().unwrap_or_default(),
            include_non_critical,
        },
        &headers,
    );

    let response = DashboardService::get_dependency_health(dashboard_service.as_ref(), request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let deps_response = response.into_inner();

    // Convert health check to JSON manually
    let mut json = serde_json::Map::new();
    if let Some(health_check) = deps_response.health_check {
        let mut hc_json = serde_json::Map::new();
        hc_json.insert(
            "overall_status".to_string(),
            serde_json::Value::Number(health_check.overall_status.into()),
        );
        // Convert dependency_checks array
        let deps: Vec<serde_json::Value> = health_check
            .dependency_checks
            .iter()
            .map(|dep| {
                let mut dep_json = serde_json::Map::new();
                dep_json.insert(
                    "name".to_string(),
                    serde_json::Value::String(dep.name.clone()),
                );
                dep_json.insert(
                    "status".to_string(),
                    serde_json::Value::Number(dep.status.into()),
                );
                serde_json::Value::Object(dep_json)
            })
            .collect();
        hc_json.insert(
            "dependency_checks".to_string(),
            serde_json::Value::Array(deps),
        );
        json.insert(
            "health_check".to_string(),
            serde_json::Value::Object(hc_json),
        );
    }

    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get application detail
async fn api_application_detail(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        _service_locator,
        dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Path(name): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Get application info
    let request = dashboard_request(
        GetApplicationsRequest {
            node_id: String::new(),
            tenant_id: String::new(),
            namespace: String::new(),
            name_pattern: name.clone(),
            page: None,
        },
        &headers,
    );

    let response = dashboard_service
        .get_applications(request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let apps_response = response.into_inner();

    // Find the application by canonical id or display name.
    let app = apps_response
        .applications
        .iter()
        .find(|a| a.application_id == name || a.name == name);

    if app.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let app = app.unwrap();

    // Get actors for this application
    let actors_request = dashboard_request(
        GetActorsRequest {
            node_id: String::new(),
            tenant_id: app.tenant_id.clone(),
            namespace: app.namespace.clone(),
            actor_id_pattern: String::new(),
            actor_group: String::new(),
            actor_type: String::new(),
            status: String::new(),
            since: None,
            page: None,
            behavior_kind: String::new(),
        },
        &headers,
    );

    let actors_response = dashboard_service
        .get_actors(actors_request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let actors = actors_response.into_inner().actors;

    // Get node count from object-registry
    let node_count = {
        use plexspaces_core::RequestContext;
        use plexspaces_proto::object_registry::v1::ObjectType;

        // Dashboard API path: use empty tenant/namespace (tenant comes from auth, not config)
        // NOTE: default_tenant_id and default_namespace have been removed from NodeConfig.
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        if let Some(object_registry) = _service_locator.get_object_registry().await {
            // Use discover method with individual parameters
            if let Ok(registrations) = object_registry
                .discover(
                    &ctx,
                    Some(ObjectType::ObjectTypeApplication),
                    Some(name.clone()),
                    None, // capabilities
                    None, // labels
                    None, // health_status
                    0,    // offset
                    1000, // limit
                )
                .await
            {
                registrations.len() as u32
            } else {
                1 // Default to 1 if query fails
            }
        } else {
            1 // Default to 1 if object-registry not available
        }
    };

    // Convert to JSON
    let mut app_json = serde_json::Map::new();
    app_json.insert(
        "application_id".to_string(),
        serde_json::Value::String(app.application_id.clone()),
    );
    app_json.insert(
        "name".to_string(),
        serde_json::Value::String(app.name.clone()),
    );
    app_json.insert(
        "version".to_string(),
        serde_json::Value::String(app.version.clone()),
    );
    app_json.insert(
        "status".to_string(),
        serde_json::Value::Number(app.status.into()),
    );
    app_json.insert(
        "node_count".to_string(),
        serde_json::Value::Number(node_count.into()),
    );
    app_json.insert(
        "namespace".to_string(),
        serde_json::Value::String(app.namespace.clone()),
    );
    app_json.insert(
        "tenant_id".to_string(),
        serde_json::Value::String(app.tenant_id.clone()),
    );
    if let Some(deployed_at) = &app.deployed_at {
        app_json.insert(
            "deployed_at".to_string(),
            serde_json::json!({
                "seconds": deployed_at.seconds,
                "nanos": deployed_at.nanos,
            }),
        );
    }
    if let Some(metrics) = &app.metrics {
        app_json.insert("metrics".to_string(), application_metrics_to_json(metrics));
    }

    let actors_json: Vec<serde_json::Value> = actors.iter().map(actor_info_to_json).collect();

    let mut json = serde_json::Map::new();
    json.insert(
        "application".to_string(),
        serde_json::Value::Object(app_json),
    );
    json.insert("actors".to_string(), serde_json::Value::Array(actors_json));

    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get actor detail
async fn api_actor_detail(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        _service_locator,
        dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Path(actor_id): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Get actor info
    let request = dashboard_request(
        GetActorsRequest {
            node_id: String::new(),
            tenant_id: String::new(),
            namespace: String::new(),
            actor_id_pattern: actor_id.clone(),
            actor_group: String::new(),
            actor_type: String::new(),
            status: String::new(),
            since: None,
            page: None,
            behavior_kind: String::new(),
        },
        &headers,
    );

    let response = dashboard_service
        .get_actors(request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let actors_response = response.into_inner();

    // Find the actor by ID
    let actor = actors_response
        .actors
        .iter()
        .find(|a| a.actor_id == actor_id);

    if actor.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let actor = actor.unwrap();

    // Convert to JSON
    let mut actor_json = serde_json::Map::new();
    actor_json.insert(
        "actor_id".to_string(),
        serde_json::Value::String(actor.actor_id.clone()),
    );
    actor_json.insert(
        "actor_type".to_string(),
        serde_json::Value::String(actor.actor_type.clone()),
    );
    actor_json.insert(
        "actor_group".to_string(),
        serde_json::Value::String(actor.actor_group.clone()),
    );
    actor_json.insert(
        "namespace".to_string(),
        serde_json::Value::String(actor.namespace.clone()),
    );
    actor_json.insert(
        "tenant_id".to_string(),
        serde_json::Value::String(actor.tenant_id.clone()),
    );
    actor_json.insert(
        "node_id".to_string(),
        serde_json::Value::String(actor.node_id.clone()),
    );
    actor_json.insert(
        "status".to_string(),
        serde_json::Value::String(actor.status.clone()),
    );
    actor_json.insert(
        "behavior_kind".to_string(),
        serde_json::Value::String(actor.behavior_kind.clone()),
    );

    // Add metrics if available
    if let Some(metrics) = &actor.metrics {
        let mut metrics_json = serde_json::Map::new();
        metrics_json.insert(
            "spawn_total".to_string(),
            serde_json::Value::Number(metrics.spawn_total.into()),
        );
        metrics_json.insert(
            "active".to_string(),
            serde_json::Value::Number(metrics.active.into()),
        );
        metrics_json.insert(
            "messages_routed".to_string(),
            serde_json::Value::Number(metrics.messages_routed.into()),
        );
        metrics_json.insert(
            "local_deliveries".to_string(),
            serde_json::Value::Number(metrics.local_deliveries.into()),
        );
        metrics_json.insert(
            "remote_deliveries".to_string(),
            serde_json::Value::Number(metrics.remote_deliveries.into()),
        );
        metrics_json.insert(
            "failed_deliveries".to_string(),
            serde_json::Value::Number(metrics.failed_deliveries.into()),
        );
        metrics_json.insert(
            "error_total".to_string(),
            serde_json::Value::Number(metrics.error_total.into()),
        );
        actor_json.insert(
            "metrics".to_string(),
            serde_json::Value::Object(metrics_json),
        );
    }

    if let Some(ts) = &actor.created_at {
        actor_json.insert(
            "created_at".to_string(),
            serde_json::json!({
                "seconds": ts.seconds,
                "nanos": ts.nanos,
            }),
        );
    }

    let mut json = serde_json::Map::new();
    json.insert("actor".to_string(), serde_json::Value::Object(actor_json));

    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Stop actor
async fn api_actor_stop(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        service_locator,
        _dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
    Path(actor_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let parsed_actor_id =
        ActorId::from_canonical(&actor_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let actor_factory = service_locator
        .get_actor_factory()
        .await
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let request = dashboard_request((), &headers);
    let ctx =
        request_context_from_grpc_request(request.metadata(), &HashMap::new(), &service_locator)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?
            .with_namespace(parsed_actor_id.namespace().to_string());

    actor_factory
        .stop_actor(&ctx, &parsed_actor_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "actor_id": actor_id,
        "stopped": true
    })))
}

/// API: Get system info (version, build date, git commit)
/// Uses build-time constants from build.rs
async fn api_system_info(
    axum::extract::State((
        _actor_svc,
        _auth_disabled,
        _jwt_secret,
        _service_locator,
        _dashboard_service_opt,
    )): axum::extract::State<HttpGatewayState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut json = serde_json::Map::new();
    json.insert(
        "version".to_string(),
        serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string()),
    );
    json.insert(
        "build_date".to_string(),
        serde_json::Value::String(
            option_env!("PLEXSPACES_BUILD_DATE")
                .unwrap_or("unknown")
                .to_string(),
        ),
    );
    json.insert(
        "git_commit".to_string(),
        serde_json::Value::String(
            option_env!("PLEXSPACES_GIT_COMMIT")
                .unwrap_or("unknown")
                .to_string(),
        ),
    );
    Ok(Json(serde_json::Value::Object(json)))
}
