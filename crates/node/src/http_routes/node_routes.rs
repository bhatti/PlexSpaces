// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! HTTP REST bridge for NodeService.
//!
//! `GET /api/v1/nodes` delegates to `NodeServiceImpl::list_connected_nodes`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{Router, extract::{Query, State}, http::StatusCode, response::Json, routing::get};
use plexspaces_core::ServiceLocator;
use plexspaces_proto::node::v1::node_service_server::NodeService as NodeServiceTrait;
use plexspaces_proto::node::v1::ListConnectedNodesRequest;
use plexspaces_services::node_service::NodeServiceImpl;
use serde_json::Value;

/// Build the node HTTP bridge router.
pub fn node_router(service_locator: Arc<dyn ServiceLocator>) -> Router {
    Router::new()
        .route("/api/v1/nodes", get(list_connected_nodes))
        .with_state(service_locator)
}

async fn list_connected_nodes(
    State(service_locator): State<Arc<dyn ServiceLocator>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let cluster = params.get("cluster").cloned().unwrap_or_default();
    let page_size = params
        .get("page_size")
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|&s| s > 0)
        .map(|s| s.min(1000))
        .unwrap_or(100);
    let page_token = params.get("page_token").cloned().unwrap_or_default();

    // NodeServiceImpl is lightweight (wraps Arc); local_node_id is resolved once per call
    // via the in-memory service registry — no I/O involved.
    let local_node_id = service_locator
        .get_node_config()
        .await
        .map(|c| c.id)
        .unwrap_or_default();

    let node_service = NodeServiceImpl::new(service_locator, local_node_id);
    let resp = node_service
        .list_connected_nodes(tonic::Request::new(ListConnectedNodesRequest {
            cluster,
            page_size,
            page_token,
            include_health: false,
        }))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.message().to_string()))?
        .into_inner();

    let nodes: Vec<Value> = resp
        .nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "node_id": n.node_id,
                "node_address": n.node_address,
                "capabilities": n.capabilities,
                "status": n.status,
                "actor_count": n.actor_count,
                "message_count": n.message_count,
                "error_count": n.error_count,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "nodes": nodes,
        "next_page_token": resp.next_page_token,
        "total_count": resp.total_count,
    })))
}
