// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! HTTP REST bridge routes for actor ask/tell operations.
//!
//! Each handler is a thin delegate to `ActorServiceImpl` via
//! `crate::http_gateway::actor_http_request`. No logic lives here.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde_json::Value;

use plexspaces_services::actor_service::ActorServiceImpl;

use crate::http_jwt::JwtClaims;

/// State shared across actor HTTP bridge handlers.
#[derive(Clone)]
pub struct ActorRouteState {
    /// Actor service that handles ask/tell dispatch.
    pub actor_service: Arc<ActorServiceImpl>,
    /// True when auth is disabled (e.g. local testing).
    pub auth_disabled: bool,
    /// JWT secret for validating bearer tokens when auth is enabled.
    pub jwt_secret: Option<String>,
}

/// Resolve the effective tenant_id from JWT claims or request headers.
///
/// When auth is enabled, the tenant_id MUST come from a valid JWT. A missing or
/// invalid JWT returns `Err(401)` so the caller can propagate the rejection.
/// When auth is disabled (testing / dev mode), the `x-tenant-id` header is used
/// as a fallback; an absent header returns an empty string (anonymous).
fn effective_tenant_id(
    jwt: &Option<axum::extract::Extension<JwtClaims>>,
    auth_disabled: bool,
    jwt_secret: Option<&str>,
    headers: &axum::http::HeaderMap,
) -> Result<String, (StatusCode, Json<Value>)> {
    // JWT middleware already validated the token — use the extracted claims.
    if let Some(ext) = jwt {
        return Ok(ext.tenant_id.clone());
    }
    if auth_disabled {
        // Auth is off: accept x-tenant-id header as-is for dev / test requests.
        let tenant = headers
            .get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        return Ok(tenant);
    }
    // Auth is enabled but the middleware did not inject claims — try the raw header.
    let secret = jwt_secret.ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Auth enabled but JWT secret not configured"})),
        )
    })?;
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    crate::http_jwt::validate_bearer_token(secret, auth.as_deref())
        .map(|claims| claims.tenant_id)
        .map_err(|e| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": format!("Invalid or missing JWT: {}", e)})),
            )
        })
}

#[allow(clippy::too_many_arguments)]
async fn handle_ask(
    State(s): State<ActorRouteState>,
    jwt: Option<axum::extract::Extension<JwtClaims>>,
    Path((namespace, actor_type)): Path<(String, String)>,
    query: Option<Query<HashMap<String, String>>>,
    headers: axum::http::HeaderMap,
    method: axum::http::Method,
    subpath: &str,
    body: Option<axum::body::Bytes>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let tenant_id = effective_tenant_id(&jwt, s.auth_disabled, s.jwt_secret.as_deref(), &headers)?;
    let path = if subpath.is_empty() {
        format!("/api/v1/actors/{}/{}", namespace, actor_type)
    } else {
        format!("/api/v1/actors/{}/{}/{}", namespace, actor_type, subpath)
    };
    crate::http_gateway::actor_http_request(
        tenant_id,
        method,
        path,
        query,
        body,
        headers,
        s.actor_service,
    )
    .await
}

/// Build the actor HTTP bridge router.
pub fn actor_router(
    actor_service: Arc<ActorServiceImpl>,
    auth_disabled: bool,
    jwt_secret: Option<String>,
) -> Router {
    let state = ActorRouteState {
        actor_service,
        auth_disabled,
        jwt_secret,
    };

    Router::new()
        // Base actor routes: GET=ask, POST/PUT=tell, DELETE=stop
        .route(
            "/api/v1/actors/:namespace/:actor_type",
            get({
                let s = state.clone();
                move |State(_): State<ActorRouteState>,
                      jwt: Option<axum::extract::Extension<JwtClaims>>,
                      Path((ns, at)): Path<(String, String)>,
                      query: Option<Query<HashMap<String, String>>>,
                      headers: axum::http::HeaderMap| {
                    let s = s.clone();
                    async move {
                        handle_ask(
                            State(s),
                            jwt,
                            Path((ns, at)),
                            query,
                            headers,
                            axum::http::Method::GET,
                            "",
                            None,
                        )
                        .await
                    }
                }
            })
            .post({
                let s = state.clone();
                move |State(_): State<ActorRouteState>,
                      jwt: Option<axum::extract::Extension<JwtClaims>>,
                      Path((ns, at)): Path<(String, String)>,
                      query: Option<Query<HashMap<String, String>>>,
                      headers: axum::http::HeaderMap,
                      body: Option<axum::body::Bytes>| {
                    let s = s.clone();
                    async move {
                        handle_ask(
                            State(s),
                            jwt,
                            Path((ns, at)),
                            query,
                            headers,
                            axum::http::Method::POST,
                            "",
                            body,
                        )
                        .await
                    }
                }
            })
            .put({
                let s = state.clone();
                move |State(_): State<ActorRouteState>,
                      jwt: Option<axum::extract::Extension<JwtClaims>>,
                      Path((ns, at)): Path<(String, String)>,
                      query: Option<Query<HashMap<String, String>>>,
                      headers: axum::http::HeaderMap,
                      body: Option<axum::body::Bytes>| {
                    let s = s.clone();
                    async move {
                        handle_ask(
                            State(s),
                            jwt,
                            Path((ns, at)),
                            query,
                            headers,
                            axum::http::Method::PUT,
                            "",
                            body,
                        )
                        .await
                    }
                }
            })
            .delete({
                let s = state.clone();
                move |State(_): State<ActorRouteState>,
                      jwt: Option<axum::extract::Extension<JwtClaims>>,
                      Path((ns, at)): Path<(String, String)>,
                      headers: axum::http::HeaderMap| {
                    let s = s.clone();
                    async move {
                        let tenant_id = effective_tenant_id(
                            &jwt,
                            s.auth_disabled,
                            s.jwt_secret.as_deref(),
                            &headers,
                        )?;
                        crate::http_gateway::stop_actor_http_request(
                            tenant_id,
                            ns,
                            at,
                            s.actor_service,
                        )
                        .await
                    }
                }
            }),
        )
        // Explicit /ask sub-routes: GET/POST/PUT
        .route(
            "/api/v1/actors/:namespace/:actor_type/ask",
            get({
                let s = state.clone();
                move |State(_): State<ActorRouteState>,
                      jwt: Option<axum::extract::Extension<JwtClaims>>,
                      Path((ns, at)): Path<(String, String)>,
                      query: Option<Query<HashMap<String, String>>>,
                      headers: axum::http::HeaderMap| {
                    let s = s.clone();
                    async move {
                        handle_ask(
                            State(s),
                            jwt,
                            Path((ns, at)),
                            query,
                            headers,
                            axum::http::Method::GET,
                            "ask",
                            None,
                        )
                        .await
                    }
                }
            })
            .post({
                let s = state.clone();
                move |State(_): State<ActorRouteState>,
                      jwt: Option<axum::extract::Extension<JwtClaims>>,
                      Path((ns, at)): Path<(String, String)>,
                      query: Option<Query<HashMap<String, String>>>,
                      headers: axum::http::HeaderMap,
                      body: Option<axum::body::Bytes>| {
                    let s = s.clone();
                    async move {
                        handle_ask(
                            State(s),
                            jwt,
                            Path((ns, at)),
                            query,
                            headers,
                            axum::http::Method::POST,
                            "ask",
                            body,
                        )
                        .await
                    }
                }
            })
            .put({
                let s = state.clone();
                move |State(_): State<ActorRouteState>,
                      jwt: Option<axum::extract::Extension<JwtClaims>>,
                      Path((ns, at)): Path<(String, String)>,
                      query: Option<Query<HashMap<String, String>>>,
                      headers: axum::http::HeaderMap,
                      body: Option<axum::body::Bytes>| {
                    let s = s.clone();
                    async move {
                        handle_ask(
                            State(s),
                            jwt,
                            Path((ns, at)),
                            query,
                            headers,
                            axum::http::Method::PUT,
                            "ask",
                            body,
                        )
                        .await
                    }
                }
            }),
        )
        .with_state(state)
}
