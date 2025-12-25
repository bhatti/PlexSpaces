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

//! Dashboard HTTP Handlers
//!
//! ## Purpose
//! Serves dashboard HTML pages and handles dashboard API requests.
//! Routes:
//! - GET / → Home page
//! - GET /node/{node_id} → Node detail page
//! - GET /static/* → Static assets (JS, CSS)
//! - GET /api/v1/dashboard/* → Dashboard API endpoints

use axum::{
    extract::{Path, Query},
    http::{StatusCode, header},
    response::{Html, Json, Response},
    routing::get,
    Router,
};
use std::sync::Arc;
use std::collections::HashMap;

use plexspaces_core::ServiceLocator;
use crate::dashboard_service::DashboardServiceImpl;
use plexspaces_proto::dashboard::v1::{
    dashboard_service_server::DashboardService,
    GetSummaryRequest, GetNodesRequest, GetNodeDashboardRequest,
    GetApplicationsRequest, GetActorsRequest, GetWorkflowsRequest,
    GetDependencyHealthRequest,
};
use plexspaces_proto::common::v1::{PageRequest, PageResponse};
use tonic::Request;

/// Create dashboard router
pub fn create_dashboard_router(
    service_locator: Arc<ServiceLocator>,
    dashboard_service: Option<Arc<DashboardServiceImpl>>,
) -> Router {
    Router::new()
        .route("/", get(home_page))
        .route("/dashboard", get(home_page))  // Alias for home
        .route("/dashboard/node/:node_id", get(node_page))
        .route("/node/:node_id", get(node_page))  // Also support without /dashboard prefix
        .route("/dashboard/application/:name", get(application_page))
        .route("/static/dashboard.css", get(serve_css))
        .route("/static/dashboard.js", get(serve_js))
        .route("/api/v1/dashboard/summary", get(api_summary))
        .route("/api/v1/dashboard/nodes", get(api_nodes))
        .route("/api/v1/dashboard/node/:node_id", get(api_node_dashboard))
        .route("/api/v1/dashboard/applications", get(api_applications))
        .route("/api/v1/dashboard/application/:name", get(api_application_detail))
        .route("/api/v1/dashboard/actors", get(api_actors))
        .route("/api/v1/dashboard/actor/:actor_id", get(api_actor_detail))
        .route("/api/v1/dashboard/workflows", get(api_workflows))
        .route("/api/v1/dashboard/workflow/:definition_id", get(api_workflow_detail))
        .route("/api/v1/dashboard/dependencies", get(api_dependencies))
        .route("/api/v1/dashboard/system-info", get(api_system_info))
        .with_state((service_locator, dashboard_service))
}

/// Home page handler
async fn home_page() -> Html<&'static str> {
    Html(include_str!("../static/dashboard/home.html"))
}

/// Node detail page handler
async fn node_page(
    Path(node_id): Path<String>,
) -> Result<Html<String>, StatusCode> {
    // Replace all :node_id placeholders in HTML with actual node_id
    let html = include_str!("../static/dashboard/node.html")
        .replace(":node_id", &node_id);
    Ok(Html(html))
}

/// Application detail page handler - redirects to home with modal
async fn application_page(
    Path(_name): Path<String>,
) -> Result<axum::response::Redirect, StatusCode> {
    // Redirect to home page - modals will handle the detail view
    Ok(axum::response::Redirect::to("/"))
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

/// API: Get summary
async fn api_summary(
    axum::extract::State((_service_locator, dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    // Parse query parameters
    let tenant_id = _params.get("tenant_id").cloned().unwrap_or_default();
    let node_id = _params.get("node_id").cloned().unwrap_or_default();
    let cluster_id = _params.get("cluster_id").cloned().unwrap_or_default();
    
    // Create gRPC request
    let request = Request::new(GetSummaryRequest {
        tenant_id,
        node_id,
        cluster_id,
        since: None,
    });
    
    // Call DashboardService
    let response = DashboardService::get_summary(dashboard_service.as_ref(), request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Convert to JSON using prost_types JSON encoding
    let summary = response.into_inner();
    
    // Convert proto to JSON manually since proto types don't implement Serialize
    let mut json = serde_json::Map::new();
    json.insert("total_clusters".to_string(), serde_json::Value::Number(summary.total_clusters.into()));
    json.insert("total_nodes".to_string(), serde_json::Value::Number(summary.total_nodes.into()));
    json.insert("total_tenants".to_string(), serde_json::Value::Number(summary.total_tenants.into()));
    json.insert("total_applications".to_string(), serde_json::Value::Number(summary.total_applications.into()));
    
    // Convert actors_by_type HashMap
    let actors_map: serde_json::Map<String, serde_json::Value> = summary.actors_by_type
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::Number(v.into())))
        .collect();
    json.insert("actors_by_type".to_string(), serde_json::Value::Object(actors_map));
    
    // Convert timestamps
    if let Some(since) = summary.since {
        json.insert("since".to_string(), serde_json::json!({
            "seconds": since.seconds,
            "nanos": since.nanos,
        }));
    }
    if let Some(until) = summary.until {
        json.insert("until".to_string(), serde_json::json!({
            "seconds": until.seconds,
            "nanos": until.nanos,
        }));
    }
    
    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get nodes
async fn api_nodes(
    axum::extract::State((_service_locator, dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    let tenant_id = _params.get("tenant_id").cloned().unwrap_or_default();
    let cluster_id = _params.get("cluster_id").cloned().unwrap_or_default();
    
    // Parse pagination params
    let offset = _params.get("offset").and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    let limit = _params.get("limit").and_then(|s| s.parse::<i32>().ok()).unwrap_or(50);
    
    let page_request = Some(PageRequest {
        offset,
        limit,
        filter: String::new(),
        order_by: String::new(),
    });
    
    let request = Request::new(GetNodesRequest {
        tenant_id,
        cluster_id,
        page: page_request,
    });
    
    let response = DashboardService::get_nodes(dashboard_service.as_ref(), request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let nodes_response = response.into_inner();
    
    // Convert nodes to JSON manually
    let nodes: Vec<serde_json::Value> = nodes_response.nodes.iter().map(|n| {
        let mut node_json = serde_json::Map::new();
        node_json.insert("id".to_string(), serde_json::Value::String(n.id.clone()));
        node_json.insert("cluster_name".to_string(), serde_json::Value::String(n.cluster_name.clone()));
        node_json.insert("status".to_string(), serde_json::Value::Number(n.status.into()));
        serde_json::Value::Object(node_json)
    }).collect();
    
    let mut json = serde_json::Map::new();
    json.insert("nodes".to_string(), serde_json::Value::Array(nodes));
    
    // Include page response
    if let Some(page) = nodes_response.page {
        let mut page_json = serde_json::Map::new();
        page_json.insert("total_size".to_string(), serde_json::Value::Number(page.total_size.into()));
        page_json.insert("offset".to_string(), serde_json::Value::Number(page.offset.into()));
        page_json.insert("limit".to_string(), serde_json::Value::Number(page.limit.into()));
        page_json.insert("has_next".to_string(), serde_json::Value::Bool(page.has_next));
        json.insert("page".to_string(), serde_json::Value::Object(page_json));
    }
    
    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get node dashboard
async fn api_node_dashboard(
    axum::extract::State((_service_locator, dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
    Path(node_id): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    let request = Request::new(GetNodeDashboardRequest {
        node_id,
        since: None,
    });
    
    let response = DashboardService::get_node_dashboard(dashboard_service.as_ref(), request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let dashboard = response.into_inner();
    
    // Convert to JSON manually - proto types don't implement Serialize
    let mut json = serde_json::Map::new();
    
    // Convert node
    if let Some(node) = dashboard.node {
        let mut node_json = serde_json::Map::new();
        node_json.insert("id".to_string(), serde_json::Value::String(node.id));
        node_json.insert("cluster_name".to_string(), serde_json::Value::String(node.cluster_name));
        node_json.insert("status".to_string(), serde_json::Value::Number(node.status.into()));
        json.insert("node".to_string(), serde_json::Value::Object(node_json));
    }
    
    // Convert node metrics
    if let Some(metrics) = dashboard.node_metrics {
        let mut metrics_json = serde_json::Map::new();
        metrics_json.insert("node_id".to_string(), serde_json::Value::String(metrics.node_id));
        metrics_json.insert("cluster_name".to_string(), serde_json::Value::String(metrics.cluster_name));
        metrics_json.insert("memory_used_bytes".to_string(), serde_json::Value::Number(metrics.memory_used_bytes.into()));
        metrics_json.insert("memory_available_bytes".to_string(), serde_json::Value::Number(metrics.memory_available_bytes.into()));
        metrics_json.insert("cpu_usage_percent".to_string(), serde_json::Value::Number((metrics.cpu_usage_percent as u64).into()));
        metrics_json.insert("uptime_seconds".to_string(), serde_json::Value::Number(metrics.uptime_seconds.into()));
        metrics_json.insert("messages_routed".to_string(), serde_json::Value::Number(metrics.messages_routed.into()));
        metrics_json.insert("active_actors".to_string(), serde_json::Value::Number(metrics.active_actors.into()));
        metrics_json.insert("actor_count".to_string(), serde_json::Value::Number(metrics.actor_count.into()));
        json.insert("node_metrics".to_string(), serde_json::Value::Object(metrics_json));
    }
    
    // Convert summary
    if let Some(summary) = dashboard.summary {
        let mut summary_json = serde_json::Map::new();
        summary_json.insert("total_tenants".to_string(), serde_json::Value::Number(summary.total_tenants.into()));
        summary_json.insert("total_applications".to_string(), serde_json::Value::Number(summary.total_applications.into()));
        let actors_map: serde_json::Map<String, serde_json::Value> = summary.actors_by_type
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::Number(v.into())))
            .collect();
        summary_json.insert("actors_by_type".to_string(), serde_json::Value::Object(actors_map));
        json.insert("summary".to_string(), serde_json::Value::Object(summary_json));
    }
    
    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get applications
async fn api_applications(
    axum::extract::State((_service_locator, dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    // Parse pagination params
    let offset = _params.get("offset").and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    let limit = _params.get("limit").and_then(|s| s.parse::<i32>().ok()).unwrap_or(50);
    
    let page_request = Some(PageRequest {
        offset,
        limit,
        filter: String::new(),
        order_by: String::new(),
    });
    
    let request = Request::new(GetApplicationsRequest {
        node_id: _params.get("node_id").cloned().unwrap_or_default(),
        tenant_id: _params.get("tenant_id").cloned().unwrap_or_default(),
        namespace: _params.get("namespace").cloned().unwrap_or_default(),
        name_pattern: _params.get("name_pattern").cloned().unwrap_or_default(),
        page: page_request,
    });
    
    let response = dashboard_service.get_applications(request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let apps_response = response.into_inner();
    
    // Convert applications to JSON manually
    // Get namespace and tenant_id for each application from ApplicationManager
    let mut apps: Vec<serde_json::Value> = Vec::new();
    for app in &apps_response.applications {
        let mut app_json = serde_json::Map::new();
        app_json.insert("application_id".to_string(), serde_json::Value::String(app.application_id.clone()));
        app_json.insert("name".to_string(), serde_json::Value::String(app.name.clone()));
        app_json.insert("version".to_string(), serde_json::Value::String(app.version.clone()));
        app_json.insert("status".to_string(), serde_json::Value::Number(app.status.into()));
        
        // Get namespace and tenant_id from ApplicationManager (stored in ApplicationInstance)
        let (namespace, tenant_id) = if let Some(app_manager) = _service_locator.get_service_by_name::<plexspaces_core::ApplicationManager>(plexspaces_core::service_locator::service_names::APPLICATION_MANAGER).await {
            app_manager.get_application_namespace_tenant(&app.name).await
                .unwrap_or_else(|| ("default".to_string(), "internal".to_string()))
        } else {
            ("default".to_string(), "internal".to_string())
        };
        app_json.insert("namespace".to_string(), serde_json::Value::String(namespace));
        app_json.insert("tenant_id".to_string(), serde_json::Value::String(tenant_id));
        
        if let Some(deployed_at) = &app.deployed_at {
            app_json.insert("created_at".to_string(), serde_json::json!({
                "seconds": deployed_at.seconds,
                "nanos": deployed_at.nanos,
            }));
        }
        apps.push(serde_json::Value::Object(app_json));
    }
    
    let mut json = serde_json::Map::new();
    json.insert("applications".to_string(), serde_json::Value::Array(apps));
    
    // Include page response
    if let Some(page) = apps_response.page {
        let mut page_json = serde_json::Map::new();
        page_json.insert("total_size".to_string(), serde_json::Value::Number(page.total_size.into()));
        page_json.insert("offset".to_string(), serde_json::Value::Number(page.offset.into()));
        page_json.insert("limit".to_string(), serde_json::Value::Number(page.limit.into()));
        page_json.insert("has_next".to_string(), serde_json::Value::Bool(page.has_next));
        json.insert("page".to_string(), serde_json::Value::Object(page_json));
    }
    
    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get actors
async fn api_actors(
    axum::extract::State((_service_locator, dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    // Parse pagination params
    let offset = _params.get("offset").and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    let limit = _params.get("limit").and_then(|s| s.parse::<i32>().ok()).unwrap_or(50);
    
    let page_request = Some(PageRequest {
        offset,
        limit,
        filter: String::new(),
        order_by: String::new(),
    });
    
    let request = Request::new(GetActorsRequest {
        node_id: _params.get("node_id").cloned().unwrap_or_default(),
        tenant_id: _params.get("tenant_id").cloned().unwrap_or_default(),
        namespace: _params.get("namespace").cloned().unwrap_or_default(),
        actor_id_pattern: _params.get("actor_id_pattern").cloned().unwrap_or_default(),
        actor_group: _params.get("actor_group").cloned().unwrap_or_default(),
        actor_type: _params.get("actor_type").cloned().unwrap_or_default(),
        status: _params.get("status").cloned().unwrap_or_default(),
        since: None,
        page: page_request,
    });
    
    let response = dashboard_service.get_actors(request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let actors_response = response.into_inner();
    
    // Convert actors to JSON manually
    let actors: Vec<serde_json::Value> = actors_response.actors.iter().map(|actor| {
        let mut actor_json = serde_json::Map::new();
        actor_json.insert("actor_id".to_string(), serde_json::Value::String(actor.actor_id.clone()));
        actor_json.insert("actor_type".to_string(), serde_json::Value::String(actor.actor_type.clone()));
        serde_json::Value::Object(actor_json)
    }).collect();
    
    let mut json = serde_json::Map::new();
    json.insert("actors".to_string(), serde_json::Value::Array(actors));
    
    // Include page response
    if let Some(page) = actors_response.page {
        let mut page_json = serde_json::Map::new();
        page_json.insert("total_size".to_string(), serde_json::Value::Number(page.total_size.into()));
        page_json.insert("offset".to_string(), serde_json::Value::Number(page.offset.into()));
        page_json.insert("limit".to_string(), serde_json::Value::Number(page.limit.into()));
        page_json.insert("has_next".to_string(), serde_json::Value::Bool(page.has_next));
        json.insert("page".to_string(), serde_json::Value::Object(page_json));
    }
    
    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get workflows
async fn api_workflows(
    axum::extract::State((_service_locator, dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    // Parse pagination params
    let offset = _params.get("offset").and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    let limit = _params.get("limit").and_then(|s| s.parse::<i32>().ok()).unwrap_or(50);
    
    let page_request = Some(PageRequest {
        offset,
        limit,
        filter: String::new(),
        order_by: String::new(),
    });
    
    let request = Request::new(GetWorkflowsRequest {
        node_id: _params.get("node_id").cloned().unwrap_or_default(),
        tenant_id: _params.get("tenant_id").cloned().unwrap_or_default(),
        definition_id: _params.get("definition_id").cloned().unwrap_or_default(),
        status: _params.get("status").and_then(|s| s.parse().ok()).unwrap_or(0),
        page: page_request,
    });
    
    let response = dashboard_service.get_workflows(request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let workflows_response = response.into_inner();
    
    // Convert workflows to JSON manually
    // WorkflowInfo has execution and definition fields
    let workflows: Vec<serde_json::Value> = workflows_response.workflows.iter().map(|wf| {
        let mut wf_json = serde_json::Map::new();
        // Extract execution_id and definition_id from nested fields
        let execution_id = if let Some(execution) = &wf.execution {
            execution.execution_id.clone()
        } else {
            String::new()
        };
        let definition_id = if let Some(execution) = &wf.execution {
            execution.definition_id.clone()
        } else if let Some(definition) = &wf.definition {
            definition.id.clone() // WorkflowDefinition uses 'id' not 'definition_id'
        } else {
            String::new()
        };
        let status: i32 = if let Some(execution) = &wf.execution {
            execution.status as i32
        } else {
            0
        };
        let node_id = if let Some(execution) = &wf.execution {
            execution.node_id.clone()
        } else {
            String::new()
        };
        let created_at = if let Some(execution) = &wf.execution {
            execution.created_at.clone()
        } else {
            None
        };
        wf_json.insert("workflow_id".to_string(), serde_json::Value::String(execution_id.clone()));
        wf_json.insert("execution_id".to_string(), serde_json::Value::String(execution_id));
        wf_json.insert("definition_id".to_string(), serde_json::Value::String(definition_id));
        wf_json.insert("status".to_string(), serde_json::json!(status));
        wf_json.insert("node_id".to_string(), serde_json::Value::String(node_id));
        if let Some(ts) = created_at {
            wf_json.insert("created_at".to_string(), serde_json::json!({
                "seconds": ts.seconds,
                "nanos": ts.nanos,
            }));
        }
        serde_json::Value::Object(wf_json)
    }).collect();
    
    // Note: Node counts for workflows are calculated on-demand in the detail endpoint
    // to avoid async issues in the map closure
    
    let mut json = serde_json::Map::new();
    json.insert("workflows".to_string(), serde_json::Value::Array(workflows));
    
    // Include page response
    if let Some(page) = workflows_response.page {
        let mut page_json = serde_json::Map::new();
        page_json.insert("total_size".to_string(), serde_json::Value::Number(page.total_size.into()));
        page_json.insert("offset".to_string(), serde_json::Value::Number(page.offset.into()));
        page_json.insert("limit".to_string(), serde_json::Value::Number(page.limit.into()));
        page_json.insert("has_next".to_string(), serde_json::Value::Bool(page.has_next));
        json.insert("page".to_string(), serde_json::Value::Object(page_json));
    }
    
    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get workflow detail by definition ID
async fn api_workflow_detail(
    axum::extract::State((_service_locator, dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
    Path(definition_id): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    // Get workflows with this definition ID
    let request = Request::new(GetWorkflowsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        definition_id: definition_id.clone(),
        status: 0,
        page: None,
    });
    
    let response = dashboard_service.get_workflows(request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let workflows_response = response.into_inner();
    
    if workflows_response.workflows.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }
    
    // Get node count from object-registry
    use plexspaces_core::RequestContext;
    use plexspaces_proto::object_registry::v1::ObjectType;
    
    let ctx = RequestContext::internal();
    let node_count = if let Some(object_registry) = _service_locator.get_service_by_name::<plexspaces_object_registry::ObjectRegistry>(plexspaces_core::service_locator::service_names::OBJECT_REGISTRY).await {
        // Query object-registry directly for workflow nodes
        if let Ok(registrations) = object_registry.discover(
            &ctx,
            Some(ObjectType::ObjectTypeWorkflow),
            Some(definition_id.clone()),
            None, // capabilities
            None, // labels
            None, // health_status
            0, // offset
            1000, // limit
        ).await {
            registrations.len() as u32
        } else {
            1
        }
    } else {
        1
    };
    
    // Convert workflows to JSON
    let workflows: Vec<serde_json::Value> = workflows_response.workflows.iter().map(|wf| {
        let mut wf_json = serde_json::Map::new();
        let execution_id = if let Some(execution) = &wf.execution {
            execution.execution_id.clone()
        } else {
            String::new()
        };
        let status: i32 = if let Some(execution) = &wf.execution {
            execution.status as i32
        } else {
            0
        };
        let node_id = if let Some(execution) = &wf.execution {
            execution.node_id.clone()
        } else {
            String::new()
        };
        let created_at = if let Some(execution) = &wf.execution {
            execution.created_at.clone()
        } else {
            None
        };
        wf_json.insert("workflow_id".to_string(), serde_json::Value::String(execution_id));
        wf_json.insert("status".to_string(), serde_json::json!(status));
        wf_json.insert("node_id".to_string(), serde_json::Value::String(node_id));
        if let Some(ts) = created_at {
            wf_json.insert("created_at".to_string(), serde_json::json!({
                "seconds": ts.seconds,
                "nanos": ts.nanos,
            }));
        }
        serde_json::Value::Object(wf_json)
    }).collect();
    
    let mut json = serde_json::Map::new();
    json.insert("definition_id".to_string(), serde_json::Value::String(definition_id));
    json.insert("node_count".to_string(), serde_json::Value::Number(node_count.into()));
    json.insert("workflows".to_string(), serde_json::Value::Array(workflows));
    
    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get dependencies
async fn api_dependencies(
    axum::extract::State((_service_locator, dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    let include_non_critical = _params.get("include_non_critical")
        .and_then(|s| s.parse().ok())
        .unwrap_or(true);
    
    let request = Request::new(GetDependencyHealthRequest {
        node_id: _params.get("node_id").cloned().unwrap_or_default(),
        include_non_critical,
    });
    
    let response = DashboardService::get_dependency_health(dashboard_service.as_ref(), request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let deps_response = response.into_inner();
    
    // Convert health check to JSON manually
    let mut json = serde_json::Map::new();
    if let Some(health_check) = deps_response.health_check {
        let mut hc_json = serde_json::Map::new();
        hc_json.insert("overall_status".to_string(), serde_json::Value::Number(health_check.overall_status.into()));
        // Convert dependency_checks array
        let deps: Vec<serde_json::Value> = health_check.dependency_checks.iter().map(|dep| {
            let mut dep_json = serde_json::Map::new();
            dep_json.insert("name".to_string(), serde_json::Value::String(dep.name.clone()));
            dep_json.insert("status".to_string(), serde_json::Value::Number(dep.status.into()));
            serde_json::Value::Object(dep_json)
        }).collect();
        hc_json.insert("dependency_checks".to_string(), serde_json::Value::Array(deps));
        json.insert("health_check".to_string(), serde_json::Value::Object(hc_json));
    }
    
    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get application detail
async fn api_application_detail(
    axum::extract::State((_service_locator, dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
    Path(name): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    // Get application info
    let request = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: name.clone(),
        page: None,
    });
    
    let response = dashboard_service.get_applications(request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let apps_response = response.into_inner();
    
    // Find the application by name
    let app = apps_response.applications.iter().find(|a| a.name == name);
    
    if app.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    
    let app = app.unwrap();
    
    // Get actors for this application
    let actors_request = Request::new(GetActorsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        actor_id_pattern: String::new(),
        actor_group: String::new(),
        actor_type: String::new(),
        status: String::new(),
        since: None,
        page: None,
    });
    
    let actors_response = dashboard_service.get_actors(actors_request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let actors = actors_response.into_inner().actors;
    
    // Get node count from object-registry
    let node_count = {
        use plexspaces_core::RequestContext;
        use plexspaces_proto::object_registry::v1::ObjectType;
        
        let ctx = RequestContext::internal();
        if let Some(object_registry) = _service_locator.get_service_by_name::<plexspaces_object_registry::ObjectRegistry>(plexspaces_core::service_locator::service_names::OBJECT_REGISTRY).await {
            // Use discover method with individual parameters
            if let Ok(registrations) = object_registry.discover(
                &ctx,
                Some(ObjectType::ObjectTypeApplication),
                Some(name.clone()),
                None, // capabilities
                None, // labels
                None, // health_status
                0, // offset
                1000, // limit
            ).await {
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
    app_json.insert("application_id".to_string(), serde_json::Value::String(app.application_id.clone()));
    app_json.insert("name".to_string(), serde_json::Value::String(app.name.clone()));
    app_json.insert("version".to_string(), serde_json::Value::String(app.version.clone()));
    app_json.insert("status".to_string(), serde_json::Value::Number(app.status.into()));
    app_json.insert("node_count".to_string(), serde_json::Value::Number(node_count.into()));
    if let Some(deployed_at) = &app.deployed_at {
        app_json.insert("deployed_at".to_string(), serde_json::json!({
            "seconds": deployed_at.seconds,
            "nanos": deployed_at.nanos,
        }));
    }
    
    let actors_json: Vec<serde_json::Value> = actors.iter().map(|a| {
        let mut actor_json = serde_json::Map::new();
        actor_json.insert("actor_id".to_string(), serde_json::Value::String(a.actor_id.clone()));
        actor_json.insert("actor_type".to_string(), serde_json::Value::String(a.actor_type.clone()));
        serde_json::Value::Object(actor_json)
    }).collect();
    
    let mut json = serde_json::Map::new();
    json.insert("application".to_string(), serde_json::Value::Object(app_json));
    json.insert("actors".to_string(), serde_json::Value::Array(actors_json));
    
    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get actor detail
async fn api_actor_detail(
    axum::extract::State((_service_locator, dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
    Path(actor_id): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dashboard_service = dashboard_service_opt.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    
    // Get actor info
    let request = Request::new(GetActorsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        actor_id_pattern: actor_id.clone(),
        actor_group: String::new(),
        actor_type: String::new(),
        status: String::new(),
        since: None,
        page: None,
    });
    
    let response = dashboard_service.get_actors(request).await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let actors_response = response.into_inner();
    
    // Find the actor by ID
    let actor = actors_response.actors.iter().find(|a| a.actor_id == actor_id);
    
    if actor.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    
    let actor = actor.unwrap();
    
    // Convert to JSON
    let mut actor_json = serde_json::Map::new();
    actor_json.insert("actor_id".to_string(), serde_json::Value::String(actor.actor_id.clone()));
    actor_json.insert("actor_type".to_string(), serde_json::Value::String(actor.actor_type.clone()));
    actor_json.insert("actor_group".to_string(), serde_json::Value::String(actor.actor_group.clone()));
    actor_json.insert("namespace".to_string(), serde_json::Value::String(actor.namespace.clone()));
    actor_json.insert("tenant_id".to_string(), serde_json::Value::String(actor.tenant_id.clone()));
    actor_json.insert("node_id".to_string(), serde_json::Value::String(actor.node_id.clone()));
    actor_json.insert("status".to_string(), serde_json::Value::String(actor.status.clone()));
    
    // Add metrics if available
    if let Some(metrics) = &actor.metrics {
        let mut metrics_json = serde_json::Map::new();
        metrics_json.insert("spawn_total".to_string(), serde_json::Value::Number(metrics.spawn_total.into()));
        metrics_json.insert("active".to_string(), serde_json::Value::Number(metrics.active.into()));
        metrics_json.insert("messages_routed".to_string(), serde_json::Value::Number(metrics.messages_routed.into()));
        metrics_json.insert("local_deliveries".to_string(), serde_json::Value::Number(metrics.local_deliveries.into()));
        metrics_json.insert("remote_deliveries".to_string(), serde_json::Value::Number(metrics.remote_deliveries.into()));
        metrics_json.insert("failed_deliveries".to_string(), serde_json::Value::Number(metrics.failed_deliveries.into()));
        metrics_json.insert("error_total".to_string(), serde_json::Value::Number(metrics.error_total.into()));
        actor_json.insert("metrics".to_string(), serde_json::Value::Object(metrics_json));
    }
    
    if let Some(ts) = &actor.created_at {
        actor_json.insert("created_at".to_string(), serde_json::json!({
            "seconds": ts.seconds,
            "nanos": ts.nanos,
        }));
    }
    
    let mut json = serde_json::Map::new();
    json.insert("actor".to_string(), serde_json::Value::Object(actor_json));
    
    Ok(Json(serde_json::Value::Object(json)))
}

/// API: Get system info (version, build date, git commit)
/// Uses build-time constants from build.rs
async fn api_system_info(
    axum::extract::State((_service_locator, _dashboard_service_opt)): axum::extract::State<(Arc<ServiceLocator>, Option<Arc<DashboardServiceImpl>>)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut json = serde_json::Map::new();
    json.insert("version".to_string(), serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string()));
    json.insert("build_date".to_string(), serde_json::Value::String(
        option_env!("PLEXSPACES_BUILD_DATE").unwrap_or("unknown").to_string()
    ));
    json.insert("git_commit".to_string(), serde_json::Value::String(
        option_env!("PLEXSPACES_GIT_COMMIT").unwrap_or("unknown").to_string()
    ));
    Ok(Json(serde_json::Value::Object(json)))
}
