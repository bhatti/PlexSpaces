// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! HTTP REST bridge for NodeService.
//!
//! `GET /api/v1/nodes` delegates to `NodeServiceImpl::list_connected_nodes`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use plexspaces_actor::ServiceLocator;
use plexspaces_proto::node::v1::node_service_server::NodeService as NodeServiceTrait;
use plexspaces_proto::node::v1::ListConnectedNodesRequest;
use plexspaces_services::node_service::NodeServiceImpl;
use serde_json::Value;
use tonic::metadata::MetadataValue;

/// State shared across node HTTP route handlers.
#[derive(Clone)]
pub struct NodeRouteState {
    /// Service locator for accessing node-wide services.
    pub service_locator: Arc<dyn ServiceLocator>,
    /// When true, authentication checks are skipped.
    pub auth_disabled: bool,
    /// JWT key pair for verifying bearer tokens. None when auth is disabled.
    pub jwt_key_pair: Option<Arc<plexspaces_grpc_middleware::JwtKeyPair>>,
}

/// Build the node HTTP bridge router.
pub fn node_router(
    service_locator: Arc<dyn ServiceLocator>,
    auth_disabled: bool,
    jwt_key_pair: Option<Arc<plexspaces_grpc_middleware::JwtKeyPair>>,
) -> Router {
    let state = NodeRouteState {
        service_locator,
        auth_disabled,
        jwt_key_pair,
    };
    Router::new()
        .route("/api/v1/nodes", get(list_connected_nodes))
        .with_state(state)
}

async fn list_connected_nodes(
    State(s): State<NodeRouteState>,
    headers: axum::http::HeaderMap,
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

    let tenant_id = crate::http_jwt::extract_tenant_id_from_headers(
        &headers,
        s.auth_disabled,
        s.jwt_key_pair.as_deref(),
    )?;

    let local_node_id = s
        .service_locator
        .get_node_config()
        .await
        .map(|c| c.id)
        .unwrap_or_default();

    let node_service = NodeServiceImpl::new(s.service_locator, local_node_id);
    let mut grpc_request = tonic::Request::new(ListConnectedNodesRequest {
        request_id: ulid::Ulid::new().to_string(),
        cluster,
        page_size,
        page_token,
        include_health: false,
    });
    grpc_request.metadata_mut().insert(
        "x-tenant-id",
        MetadataValue::try_from(tenant_id.as_str())
            .unwrap_or_else(|_| MetadataValue::from_static("")),
    );
    let resp = node_service
        .list_connected_nodes(grpc_request)
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
