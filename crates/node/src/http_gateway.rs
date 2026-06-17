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

//! HTTP Gateway Module
//!
//! ## Purpose
//! Provides HTTP gateway functionality for PlexSpaces node, including:
//! - Actor ask/tell handling via HTTP (REST-like API)
//! - WASM application deployment via HTTP multipart
//! - Dashboard routes (if enabled)
//! - JWT authentication middleware
//!
//! ## Design
//! This module extracts HTTP gateway setup from `Node::start()` into reusable
//! components. It defines shared types, helpers, and middleware that can be
//! used to build the HTTP gateway router.
//!
//! ## Usage
//! ```rust,ignore
//! use plexspaces_node::http_gateway::{HttpGatewayState, create_gateway_state};
//!
//! // In Node::start():
//! let state = create_gateway_state(actor_service, service_locator, dashboard_opt).await;
//! let router = create_http_gateway_router(state);
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::Query,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{Json, Response},
};
use serde_json::Value;

use plexspaces_actor::RequestContextExt;
use plexspaces_services::actor_service::ActorServiceImpl;
use plexspaces_services::dashboard_service::DashboardServiceImpl;

use crate::http_jwt::JwtClaims;

/// Maximum body size for HTTP requests (100MB for WASM uploads)
pub const MAX_BODY_SIZE: usize = 100 * 1024 * 1024;

/// Maximum WASM file size (100MB)
pub const MAX_WASM_FILE_SIZE: usize = 100 * 1024 * 1024;

/// HTTP Gateway state type
///
/// ## Components
/// - `ActorServiceImpl`: For actor ask/tell handling
/// - `bool`: auth_disabled flag
/// - `Option<Arc<JwtKeyPair>>`: JWT key pair for signing/verification (None if not configured)
/// - `Arc<dyn ServiceLocator>`: Service locator for config access
/// - `Option<DashboardServiceImpl>`: Dashboard service (if enabled)
pub type HttpGatewayState = (
    Arc<ActorServiceImpl>,
    bool,                              // auth_disabled
    Option<Arc<plexspaces_grpc_middleware::JwtKeyPair>>, // jwt_key_pair
    Arc<dyn plexspaces_actor::ServiceLocator>,
    Option<Arc<DashboardServiceImpl>>,
    Option<Arc<dyn plexspaces_services::user_service::ApiTokenRepository>>, // token revocation
);

/// Create gateway state from service locator and services
///
/// ## Arguments
/// * `actor_service` - Actor service implementation
/// * `service_locator` - Service locator for config access
/// * `dashboard_service` - Optional dashboard service
///
/// ## Returns
/// Tuple containing all state needed by HTTP gateway handlers
pub async fn create_gateway_state(
    actor_service: Arc<ActorServiceImpl>,
    service_locator: Arc<dyn plexspaces_actor::ServiceLocator>,
    dashboard_service: Option<Arc<DashboardServiceImpl>>,
) -> HttpGatewayState {
    let auth_disabled = service_locator.is_auth_disabled().await;
    let jwt_config = service_locator
        .get_security_config()
        .await
        .and_then(|c| c.jwt);

    let jwt_key_pair = match jwt_config {
        Some(ref cfg) => {
            tracing::info!(
                private_key_file = %cfg.private_key_file,
                has_private_key_pem = !cfg.private_key_pem.is_empty(),
                has_secret = !cfg.secret.is_empty(),
                auto_generate_key = cfg.auto_generate_key,
                "Resolving JWT key pair from config"
            );
            match plexspaces_grpc_middleware::JwtKeyPair::from_config(
                &cfg.private_key_pem,
                &cfg.private_key_file,
                &cfg.secret,
                cfg.auto_generate_key,
            ) {
                Ok(kp) => {
                    tracing::info!(algorithm = ?kp.algorithm(), kid = %kp.kid(), "JWT key pair loaded");
                    Some(Arc::new(kp))
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to load JWT key pair from config");
                    None
                }
            }
        }
        None => {
            tracing::warn!("No JWT config in SecurityConfig, trying env vars");
            match plexspaces_grpc_middleware::JwtKeyPair::from_env(None) {
                Ok(kp) => Some(Arc::new(kp)),
                Err(e) => {
                    tracing::error!(error = %e, "Failed to load JWT key pair from env");
                    None
                }
            }
        }
    };

    (
        actor_service,
        auth_disabled,
        jwt_key_pair,
        service_locator,
        dashboard_service,
        None,
    )
}

/// Stamp validated JWT claims into request headers for downstream HTTP and tonic handlers.
///
/// Mirrors the gRPC AuthInterceptor pattern: explicitly remove client-provided headers
/// to prevent injection, then set ONLY from validated JWT claims.
pub(crate) fn apply_jwt_claim_headers(headers: &mut axum::http::HeaderMap, claims: &JwtClaims) {
    headers.remove("x-tenant-id");
    headers.remove("x-user-id");
    headers.remove("x-admin");
    headers.remove("x-user-roles");
    headers.remove("x-user-groups");

    if let Ok(value) = axum::http::HeaderValue::from_str(&claims.tenant_id) {
        headers.insert("x-tenant-id", value);
    }
    if let Ok(value) = axum::http::HeaderValue::from_str(&claims.sub) {
        headers.insert("x-user-id", value);
    }
    if let Ok(value) =
        axum::http::HeaderValue::from_str(if claims.is_admin { "true" } else { "false" })
    {
        headers.insert("x-admin", value);
    }
    if let Ok(value) = axum::http::HeaderValue::from_str(&claims.roles.join(",")) {
        headers.insert("x-user-roles", value);
    }
    if !claims.groups.is_empty() {
        if let Ok(value) = axum::http::HeaderValue::from_str(&claims.groups.join(",")) {
            headers.insert("x-user-groups", value);
        }
    }
}

/// Resolve tenant_id from JWT claims, with local-test fallback when auth is disabled.
///
/// ## Purpose
/// Determines effective tenant_id based on auth mode:
/// - If JWT extension is present, use its tenant_id (from validated token)
/// - If auth enabled, try to validate the Authorization header
/// - If auth is disabled, allow x-tenant-id for local testing only
///
/// ## Arguments
/// * `jwt` - Optional JWT claims extension (set by auth middleware)
/// * `auth_disabled` - Whether auth is disabled
/// * `jwt_key_pair` - Optional JWT key pair for validation
/// * `headers` - Request headers
///
/// ## Returns
/// Effective tenant_id string
pub fn effective_tenant_id_from_jwt_or_headers(
    jwt: &Option<axum::extract::Extension<JwtClaims>>,
    auth_disabled: bool,
    jwt_key_pair: Option<&plexspaces_grpc_middleware::JwtKeyPair>,
    headers: &HeaderMap,
) -> String {
    // If we have validated JWT claims, use them
    if let Some(ref ext) = jwt {
        return ext.tenant_id.clone();
    }

    // If auth is not disabled, try to validate from Authorization header
    if !auth_disabled {
        if let Some(kp) = jwt_key_pair {
            let auth_header = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            if let Ok(claims) =
                crate::http_jwt::validate_bearer_token_with_keypair(kp, auth_header.as_deref())
            {
                return claims.tenant_id;
            }
        }
    }

    if auth_disabled {
        return headers
            .get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
    }

    String::new()
}

/// JWT auth middleware for HTTP gateway
///
/// ## Purpose
/// When auth is enabled, validates Bearer token from Authorization header
/// and sets `Extension(JwtClaims)` for downstream handlers.
///
/// ## Behavior
/// - Skips validation for non-actor routes
/// - Skips validation when auth_disabled is true
/// - Returns 503 if JWT secret not configured
/// - Returns 401 if token validation fails
/// - Sets JwtClaims extension on success
pub async fn http_auth_middleware(
    axum::extract::State((_svc, auth_disabled, jwt_key_pair, _sl, _ds, token_repo)): axum::extract::State<
        HttpGatewayState,
    >,
    mut req: axum::extract::Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Skip auth for public routes (OIDC login/callback, health, static assets, dashboard HTML pages).
    // Dashboard pages serve static HTML; actual data is fetched via /api/ calls that carry the session cookie.
    if path.starts_with("/api/v1/auth/oidc/")
        || path == "/api/v1/auth/logout"
        || path.starts_with("/health")
        || path.starts_with("/ready")
        || path.starts_with("/static/")
        || path.starts_with("/dashboard")
        || path == "/"
        || path == "/favicon.ico"
        || path == "/.well-known/jwks.json"
    {
        return next.run(req).await;
    }

    // All other /api/ routes require auth when enabled.
    if !path.starts_with("/api/") {
        return next.run(req).await;
    }

    // Skip auth when disabled
    if auth_disabled {
        return next.run(req).await;
    }

    // Require JWT key pair to be configured.
    let kp = match &jwt_key_pair {
        Some(kp) => kp,
        None => {
            tracing::warn!(path = %path, "HTTP auth: JWT key not configured (returning 503)");
            let body = serde_json::json!({
                "code": 503,
                "message": "Auth enabled but JWT key not configured. Set PLEXSPACES_JWT_PRIVATE_KEY_FILE or PLEXSPACES_JWT_SECRET. For local testing, set PLEXSPACES_DISABLE_AUTH=1."
            });
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap();
        }
    };

    // Extract token from Authorization header or plexspaces_token cookie.
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let has_bearer = auth_header
        .as_ref()
        .map(|h| h.starts_with("Bearer "))
        .unwrap_or(false);

    // Fall back to session cookie when no Authorization header is present.
    let auth_header = if auth_header.is_none() || !has_bearer {
        req.headers()
            .get_all("cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|s| s.split(';'))
            .find_map(|c| {
                let c = c.trim();
                c.strip_prefix("plexspaces_token=")
                    .map(|token| format!("Bearer {}", token))
            })
            .or(auth_header)
    } else {
        auth_header
    };

    match crate::http_jwt::validate_bearer_token_with_keypair(kp, auth_header.as_deref()) {
        Ok(claims) => {
            if let Some(ref repo) = token_repo {
                if let Some(ref jti) = claims.jti {
                    if let Ok(true) = repo.is_revoked(jti).await {
                        let body = serde_json::json!({ "code": 401, "message": "Token has been revoked" });
                        return Response::builder()
                            .status(StatusCode::UNAUTHORIZED)
                            .header("content-type", "application/json")
                            .body(Body::from(serde_json::to_string(&body).unwrap()))
                            .unwrap();
                    }
                }
            }
            apply_jwt_claim_headers(req.headers_mut(), &claims);
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(e) => {
            let has_cookie = auth_header.as_ref().map_or(false, |h| h.len() > 7);
            tracing::warn!(
                path = %path,
                has_bearer = %has_bearer,
                has_cookie = %has_cookie,
                error = %e,
                "HTTP auth: JWT validation failed (401)"
            );
            let body = serde_json::json!({ "code": 401, "message": e });
            Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap()
        }
    }
}

/// HTTP handler for AskReply and SendMessage actor routes.
///
/// ## Purpose
/// Handles actor ask/tell requests via HTTP, translating to ActorService calls.
///
/// ## Arguments
/// * `effective_tenant_id` - Tenant ID from JWT or headers
/// * `method` - HTTP method (GET, POST, PUT, DELETE)
/// * `path` - Request path
/// * `query` - Query parameters
/// * `body` - Request body
/// * `headers` - Request headers
/// * `actor_service` - Actor service for ask/tell handling
///
/// ## Returns
/// JSON response with actor ask/tell result
pub async fn actor_http_request(
    effective_tenant_id: String,
    method: axum::http::Method,
    path: String,
    query: Option<Query<HashMap<String, String>>>,
    body: Option<axum::body::Bytes>,
    headers: HeaderMap,
    actor_service: Arc<ActorServiceImpl>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let path_parts: Vec<&str> = path
        .strip_prefix("/api/v1/actors/")
        .unwrap_or("")
        .split('/')
        .collect();

    if path_parts.len() < 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "code": 400,
                "message": "Invalid path format. Expected /api/v1/actors/{namespace}/{actor_type}[/ask]"
            })),
        ));
    }

    let ask_suffix = path_parts.last().copied() == Some("ask");
    let core_len = if ask_suffix {
        path_parts.len().saturating_sub(1)
    } else {
        path_parts.len()
    };
    let (_path_tenant_id, namespace, actor_type) = match core_len {
        2 => (
            String::new(),
            path_parts[0].to_string(),
            path_parts[1].to_string(),
        ),
        3 => (
            path_parts[0].to_string(),
            path_parts[1].to_string(),
            path_parts[2].to_string(),
        ),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "code": 400,
                    "message": "Invalid actor path"
                })),
            ))
        }
    };
    let namespace_for_metadata = namespace.clone();
    let query_params: HashMap<String, String> = query.map(|q| q.0).unwrap_or_default();
    let method_upper = method.as_str().to_uppercase();
    let is_ask = ask_suffix || method_upper == "GET";

    if method_upper == "DELETE" {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(serde_json::json!({
                "code": 405,
                "message": "DELETE is not supported for actor ask/tell endpoints"
            })),
        ));
    }

    let timeout_duration = query_params
        .get("timeout")
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|&secs| secs > 0 && secs <= 3600)
        .map(|secs| prost_types::Duration {
            seconds: secs,
            nanos: 0,
        });

    let mut request_headers = HashMap::new();
    for (key, value) in headers.iter() {
        if let Ok(value_str) = value.to_str() {
            request_headers.insert(key.as_str().to_string(), value_str.to_string());
        }
    }

    use plexspaces_proto::actor::v1::actor_service_server::ActorService as ActorServiceTrait;
    use tonic::metadata::MetadataValue;
    use tonic::Request as TonicRequest;

    if is_ask {
        use plexspaces_proto::actor::v1::AskReplyRequest;

        let mut grpc_req = TonicRequest::new(AskReplyRequest {
            namespace,
            actor_type,
            actor_name: String::new(),
            http_method: method.as_str().to_string(),
            payload: body.map(|b| b.to_vec()).unwrap_or_default(),
            headers: request_headers,
            query_params,
            path: path.clone(),
            subpath: String::new(),
            sender_id: String::new(),
            message_type: "call".to_string(),
            correlation_id: String::new(),
            reply_to: String::new(),
            message_id: String::new(),
            timeout: timeout_duration,
        });
        grpc_req.metadata_mut().insert(
            "x-tenant-id",
            MetadataValue::try_from(effective_tenant_id.as_str())
                .unwrap_or_else(|_| MetadataValue::from_static("")),
        );
        grpc_req.metadata_mut().insert(
            "x-namespace",
            MetadataValue::try_from(namespace_for_metadata.as_str())
                .unwrap_or_else(|_| MetadataValue::from_static("")),
        );

        match ActorServiceTrait::ask_reply(&*actor_service, grpc_req).await {
            Ok(grpc_resp) => {
                let resp_inner = grpc_resp.into_inner();
                use base64::{engine::general_purpose, Engine as _};
                let payload_json = if resp_inner.payload.is_empty() {
                    serde_json::Value::Null
                } else {
                    match String::from_utf8(resp_inner.payload.clone()) {
                        Ok(s) => serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s)),
                        Err(_) => serde_json::Value::String(
                            general_purpose::STANDARD.encode(&resp_inner.payload),
                        ),
                    }
                };

                Ok(Json(serde_json::json!({
                    "success": resp_inner.success,
                    "payload": payload_json,
                    "headers": resp_inner.headers,
                    "actor_id": resp_inner.actor_id,
                    "error_message": resp_inner.error_message,
                })))
            }
            Err(status) => {
                let err_json = serde_json::json!({
                    "code": status.code() as u16,
                    "message": status.message()
                });
                let http_status = match status.code() {
                    tonic::Code::NotFound => StatusCode::NOT_FOUND,
                    tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
                    tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
                    tonic::Code::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                Err((http_status, Json(err_json)))
            }
        }
    } else {
        use plexspaces_proto::actor::v1::SendMessageRequest;

        let mut grpc_req = TonicRequest::new(SendMessageRequest {
            namespace,
            actor_type,
            actor_name: String::new(),
            http_method: method.as_str().to_string(),
            payload: body.map(|b| b.to_vec()).unwrap_or_default(),
            headers: request_headers,
            query_params,
            path: path.clone(),
            subpath: String::new(),
            sender_id: String::new(),
            message_type: "cast".to_string(),
            correlation_id: String::new(),
            reply_to: String::new(),
            message_id: String::new(),
        });
        grpc_req.metadata_mut().insert(
            "x-tenant-id",
            MetadataValue::try_from(effective_tenant_id.as_str())
                .unwrap_or_else(|_| MetadataValue::from_static("")),
        );
        grpc_req.metadata_mut().insert(
            "x-namespace",
            MetadataValue::try_from(namespace_for_metadata.as_str())
                .unwrap_or_else(|_| MetadataValue::from_static("")),
        );

        match ActorServiceTrait::send_message(&*actor_service, grpc_req).await {
            Ok(grpc_resp) => {
                let resp_inner = grpc_resp.into_inner();
                Ok(Json(serde_json::json!({
                    "success": resp_inner.success,
                    "message_id": resp_inner.message_id,
                    "actor_id": resp_inner.actor_id,
                    "error_message": resp_inner.error_message,
                })))
            }
            Err(status) => {
                let err_json = serde_json::json!({
                    "code": status.code() as u16,
                    "message": status.message()
                });
                let http_status = match status.code() {
                    tonic::Code::NotFound => StatusCode::NOT_FOUND,
                    tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
                    tonic::Code::PermissionDenied => StatusCode::FORBIDDEN,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                Err((http_status, Json(err_json)))
            }
        }
    }
}

/// HTTP DELETE handler: stop a virtual actor by canonical or `name:id` address.
///
/// Resolves the client-facing target to a canonical actor ID (running
/// `prime_instance_from_definition` as a side-effect so any subsequent
/// reactivation re-derives `wasm_init_payload` from definition args), then
/// calls `ActorFactory::stop_actor`.
pub async fn stop_actor_http_request(
    effective_tenant_id: String,
    namespace: String,
    actor_target: String,
    actor_service: Arc<ActorServiceImpl>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let ctx = plexspaces_actor::RequestContext::new_without_auth(effective_tenant_id, namespace);
    let canonical_id = actor_service
        .canonical_actor_id_from_client_target(&ctx, &actor_target)
        .await
        .unwrap_or_else(|| actor_target.clone());
    let actor_id = plexspaces_actor::ActorId::from_canonical(&canonical_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error_message": format!("Invalid actor id '{}': {}", actor_target, e)
            })),
        )
    })?;
    let factory = actor_service
        .service_locator()
        .get_actor_factory()
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error_message": "ActorFactory not available"
                })),
            )
        })?;
    factory.stop_actor(&ctx, &actor_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error_message": format!("Failed to stop actor: {}", e)
            })),
        )
    })?;
    Ok(Json(serde_json::json!({
        "success": true,
        "actor_id": canonical_id
    })))
}

// Note: Full router creation is kept in mod.rs for now due to complexity
// of the deploy/undeploy handlers. This module provides the foundation
// for incremental extraction. Future work:
// - Move wasm_deploy_handler here
// - Move undeploy_handler here
// - Create create_http_gateway_router() function
// - Create run_http_gateway() function

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_body_size() {
        assert_eq!(MAX_BODY_SIZE, 100 * 1024 * 1024);
    }

    #[test]
    fn test_max_wasm_file_size() {
        assert_eq!(MAX_WASM_FILE_SIZE, 100 * 1024 * 1024);
    }

    #[test]
    fn test_effective_tenant_id_from_header_when_auth_disabled() {
        let mut headers = HeaderMap::new();
        headers.insert("x-tenant-id", "test-tenant".parse().unwrap());

        let result = effective_tenant_id_from_jwt_or_headers(&None, true, None, &headers);
        assert_eq!(result, "test-tenant");
    }

    #[test]
    fn test_effective_tenant_id_empty_when_auth_disabled() {
        let headers = HeaderMap::new();
        let result = effective_tenant_id_from_jwt_or_headers(&None, true, None, &headers);
        assert_eq!(result, "");
    }

    #[test]
    fn test_effective_tenant_id_ignores_header_when_auth_enabled() {
        let mut headers = HeaderMap::new();
        headers.insert("x-tenant-id", "test-tenant".parse().unwrap());

        let kp = plexspaces_grpc_middleware::JwtKeyPair::from_secret("secret");
        let result =
            effective_tenant_id_from_jwt_or_headers(&None, false, Some(&kp), &headers);
        assert_eq!(result, "");
    }
}
