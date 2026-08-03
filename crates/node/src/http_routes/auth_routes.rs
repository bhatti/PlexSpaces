// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! HTTP REST routes for authentication: OIDC login/callback, user/tenant listing.
//!
//! ## Design
//! Follows the same pattern as `actor_routes.rs` and `node_routes.rs`:
//! - `AuthRouteState` carries all service dependencies.
//! - `auth_router(state)` composes the routes; the caller (`all_http_routes`)
//!   merges it into the overall router.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Json, Response},
    routing::{delete, get},
    Router,
};
use serde::Deserialize;
use serde_json::Value;

use plexspaces_actor::ServiceLocator;
use plexspaces_services::user_service::{
    ApiTokenRepository, ApiTokenRepositoryError, TenantRepository, UserRepository,
};

use crate::http_jwt::JwtClaims;

// ─── State ───────────────────────────────────────────────────────────────────

/// All service dependencies needed by auth HTTP handlers.
///
/// Built once at node startup and shared across all auth route handlers.
/// Uses the same Arc-based DI pattern as `ActorRouteState`.
#[derive(Clone)]
pub struct AuthRouteState {
    /// User repository for credential and profile lookups.
    pub user_repo: Arc<dyn UserRepository>,
    /// Tenant repository for tenant resolution and validation.
    pub tenant_repo: Arc<dyn TenantRepository>,
    /// API token repository for token issuance and validation.
    pub token_repo: Arc<dyn ApiTokenRepository>,
    /// Service locator for accessing node-wide services.
    pub service_locator: Arc<dyn ServiceLocator>,
    /// OIDC state. None when OIDC is not configured (auth_disabled or no oidc config).
    pub oidc: Option<Arc<plexspaces_services::user_service::oidc::OidcState>>,
    /// JWT key pair for signing JWTs and serving JWKS.
    pub jwt_key_pair: Option<Arc<plexspaces_grpc_middleware::JwtKeyPair>>,
}

impl AuthRouteState {
    /// Returns true when auth is disabled, reading from the single source of truth
    /// (ServiceLocator → release config). Sync call — the value is cached in-process.
    pub async fn auth_disabled(&self) -> bool {
        self.service_locator.is_auth_disabled().await
    }
}

// ─── Router ──────────────────────────────────────────────────────────────────

/// Build the auth route tree.
///
/// Mounts:
/// - `/api/v1/auth/users`              GET    – list users (admin = all, user = own tenant)
/// - `/api/v1/auth/tenants`            GET    – list tenants (admin = all, user = own)
/// - `/api/v1/auth/tokens`             GET    – list API tokens for current user
/// - `/api/v1/auth/tokens`             POST   – create API token
/// - `/api/v1/auth/tokens/:token_id`   DELETE – revoke API token
/// - `/api/v1/auth/oidc/login`         GET    – OIDC login redirect (only when OIDC configured)
/// - `/api/v1/auth/oidc/callback`      GET    – OIDC callback (only when OIDC configured)
pub fn auth_router(state: AuthRouteState) -> Router {
    let mut router = Router::new()
        .route("/api/v1/auth/me", get(get_current_user))
        .route("/api/v1/auth/logout", get(logout))
        .route("/api/v1/auth/users", get(list_users))
        .route("/api/v1/auth/tenants", get(list_tenants))
        .route(
            "/api/v1/auth/tokens",
            get(list_api_tokens).post(create_api_token),
        )
        .route("/api/v1/auth/tokens/:token_id", delete(delete_api_token))
        .route("/.well-known/jwks.json", get(jwks_endpoint));

    // Mount OIDC routes only when OIDC is configured.
    if state.oidc.is_some() {
        router = router
            .route("/api/v1/auth/oidc/login", get(oidc_login))
            .route("/api/v1/auth/oidc/callback", get(oidc_callback));
    }

    router.with_state(state)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Synthetic superadmin claims used when auth is disabled.
///
/// When `auth_disabled = true` the JWT middleware never runs, so no `JwtClaims`
/// extension is set. These synthetic claims give handlers a consistent interface
/// without needing an `if auth_disabled` branch in every handler.
fn superadmin_claims() -> JwtClaims {
    JwtClaims {
        sub: "system".into(),
        exp: i64::MAX,
        iat: 0,
        iss: String::new(),
        aud: vec![],
        tenant_id: String::new(), // empty = all tenants
        roles: vec!["admin".into()],
        groups: vec![],
        is_admin: true,
        jti: None,
    }
}

/// Extract `JwtClaims` from an `Option<Extension<JwtClaims>>` axum extractor.
///
/// - When `auth_disabled` is true, returns synthetic superadmin claims so all
///   downstream handlers work without a JWT.
/// - When auth is enabled and no claims are present, returns 401.
fn extract_claims_from_opt(
    opt: Option<axum::extract::Extension<JwtClaims>>,
    auth_disabled: bool,
) -> Result<JwtClaims, (StatusCode, Json<Value>)> {
    if auth_disabled {
        return Ok(superadmin_claims());
    }
    opt.map(|ext| ext.0).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "code": 401, "message": "Authentication required" })),
        )
    })
}

// ─── Current user (session info) ─────────────────────────────────────────────

async fn get_current_user(
    State(state): State<AuthRouteState>,
    claims_opt: Option<axum::extract::Extension<JwtClaims>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_opt(claims_opt, state.auth_disabled().await)?;

    // Look up the full user record for display_name/email.
    let user = state.user_repo.find_by_id(&claims.sub).await.ok().flatten();

    Ok(Json(serde_json::json!({
        "user_id": claims.sub,
        "tenant_id": claims.tenant_id,
        "email": user.as_ref().map(|u| u.email.as_str()).unwrap_or(&claims.sub),
        "display_name": user.as_ref().map(|u| u.display_name.as_str()).unwrap_or(""),
        "is_admin": claims.is_admin,
        "roles": claims.roles,
        "groups": claims.groups,
    })))
}

// ─── Logout ─────────────────────────────────────────────────────────────────

async fn logout() -> Response {
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("location", "/dashboard")
        .header(
            "set-cookie",
            "plexspaces_token=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
        )
        .body(Body::empty())
        .unwrap()
}

// ─── JWKS endpoint ──────────────────────────────────────────────────────────

async fn jwks_endpoint(State(state): State<AuthRouteState>) -> Json<Value> {
    match &state.jwt_key_pair {
        Some(kp) => Json(kp.jwks_json()),
        None => Json(serde_json::json!({ "keys": [] })),
    }
}

// ─── User handlers ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListUsersParams {
    offset: Option<i32>,
    limit: Option<i32>,
    tenant_id: Option<String>,
}

async fn list_users(
    State(state): State<AuthRouteState>,
    claims_opt: Option<axum::extract::Extension<JwtClaims>>,
    Query(params): Query<ListUsersParams>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_opt(claims_opt, state.auth_disabled().await)?;
    let offset = params.offset.unwrap_or(0).max(0);
    let limit = params.limit.unwrap_or(50).clamp(1, 1000);

    // Admins may pass ?tenant_id= to filter; non-admins are always restricted to their own.
    let tenant_filter: Option<String> = if claims.is_admin {
        params.tenant_id
    } else {
        Some(claims.tenant_id.clone())
    };

    let (users, total) = state
        .user_repo
        .list_users(tenant_filter.as_deref(), offset, limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "code": 500, "message": e.to_string() })),
            )
        })?;

    let users_json: Vec<Value> = users
        .iter()
        .map(|u| {
            serde_json::json!({
                "user_id": u.user_id,
                "email": u.email,
                "tenant_id": u.tenant_id,
                "display_name": u.display_name,
                "admin": u.admin,
                "roles": u.roles,
                "groups": u.groups,
                "avatar_url": u.avatar_url,
                "provider": u.provider,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "users": users_json,
        "page": { "total_size": total, "offset": offset, "limit": limit, "has_next": offset + limit < total }
    })))
}

// ─── Tenant handlers ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListTenantsParams {
    offset: Option<i32>,
    limit: Option<i32>,
}

async fn list_tenants(
    State(state): State<AuthRouteState>,
    claims_opt: Option<axum::extract::Extension<JwtClaims>>,
    Query(params): Query<ListTenantsParams>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_opt(claims_opt, state.auth_disabled().await)?;
    let offset = params.offset.unwrap_or(0).max(0);
    let limit = params.limit.unwrap_or(50).clamp(1, 1000);

    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!(
            is_admin = %claims.is_admin,
            tenant_id = %claims.tenant_id,
            sub = %claims.sub,
            "list_tenants called"
        );
    }

    let (tenants, total) = if claims.is_admin {
        state
            .tenant_repo
            .list_tenants(offset, limit)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "code": 500, "message": e.to_string() })),
                )
            })?
    } else {
        // Non-admins: single-item list of their own tenant.
        let tenant = state
            .tenant_repo
            .get_tenant(&claims.tenant_id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "code": 500, "message": e.to_string() })),
                )
            })?;
        let list: Vec<_> = tenant.into_iter().collect();
        let count = list.len() as i32;
        (list, count)
    };

    let tenants_json: Vec<Value> = tenants
        .iter()
        .map(|t| {
            serde_json::json!({
                "tenant_id": t.tenant_id,
                "slug": t.slug,
                "display_name": t.display_name,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "tenants": tenants_json,
        "page": { "total_size": total, "offset": offset, "limit": limit, "has_next": offset + limit < total }
    })))
}

// ─── OIDC handlers ────────────────────────────────────────────────────────────

async fn oidc_login(State(state): State<AuthRouteState>) -> Response {
    let Some(oidc) = state.oidc else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };
    plexspaces_services::user_service::oidc::handle_login(oidc).await
}

#[derive(Deserialize)]
struct OidcCallbackParams {
    code: String,
    state: String,
}

async fn oidc_callback(
    State(route_state): State<AuthRouteState>,
    Query(params): Query<OidcCallbackParams>,
) -> Response {
    let Some(oidc) = route_state.oidc else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };
    plexspaces_services::user_service::oidc::handle_callback(
        oidc,
        plexspaces_services::user_service::oidc::OidcCallbackParams {
            code: params.code,
            state: params.state,
        },
    )
    .await
}

// ─── API Token handlers ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListTokensParams {
    offset: Option<i32>,
    limit: Option<i32>,
}

async fn list_api_tokens(
    State(state): State<AuthRouteState>,
    claims_opt: Option<axum::extract::Extension<JwtClaims>>,
    Query(params): Query<ListTokensParams>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_opt(claims_opt, state.auth_disabled().await)?;
    let offset = params.offset.unwrap_or(0).max(0);
    let limit = params.limit.unwrap_or(50).clamp(1, 1000);

    let (tokens, total) = state
        .token_repo
        .list_for_user(&claims.sub, offset, limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "code": 500, "message": e.to_string() })),
            )
        })?;

    let tokens_json: Vec<Value> = tokens
        .iter()
        .map(|t| {
            serde_json::json!({
                "token_id": t.token_id,
                "name": t.name,
                "prefix": t.prefix,
                "scopes": t.scopes,
                "created_at": t.created_at.as_ref().map(|ts| ts.seconds),
                "expires_at": t.expires_at.as_ref().map(|ts| ts.seconds),
                "last_used_at": t.last_used_at.as_ref().map(|ts| ts.seconds),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "tokens": tokens_json,
        "page": { "total_size": total, "offset": offset, "limit": limit, "has_next": offset + limit < total }
    })))
}

#[derive(Deserialize)]
struct CreateTokenRequest {
    name: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    ttl_seconds: Option<i64>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    tenant_id: Option<String>,
}

async fn create_api_token(
    State(state): State<AuthRouteState>,
    claims_opt: Option<axum::extract::Extension<JwtClaims>>,
    axum::extract::Json(body): axum::extract::Json<CreateTokenRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_opt(claims_opt, state.auth_disabled().await)?;

    if body.name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "code": 400, "message": "name is required" })),
        ));
    }

    let jwt_key_pair = state.jwt_key_pair.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "code": 503, "message": "JWT signing not configured" })),
        )
    })?;

    let scopes = if body.scopes.is_empty() {
        vec!["read".into(), "write".into()]
    } else {
        body.scopes
    };

    let ttl_secs = body.ttl_seconds.unwrap_or(90 * 24 * 3600);
    let expires_at = Some(chrono::Utc::now().timestamp() + ttl_secs);
    let token_id = ulid::Ulid::new().to_string();

    let effective_user_id = if claims.is_admin {
        body.user_id.as_deref().unwrap_or(&claims.sub)
    } else {
        &claims.sub
    };
    let effective_tenant_id = if claims.is_admin {
        body.tenant_id.as_deref().unwrap_or(&claims.tenant_id)
    } else {
        &claims.tenant_id
    };

    let token = state
        .token_repo
        .create(
            &token_id,
            effective_user_id,
            effective_tenant_id,
            &body.name,
            &scopes,
            expires_at,
            claims.is_admin,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "code": 500, "message": e.to_string() })),
            )
        })?;

    let jwt = jwt_key_pair.sign_api_token(
        effective_user_id,
        effective_tenant_id,
        claims.is_admin,
        &scopes,
        &token_id,
        ttl_secs as u64,
    ).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "code": 500, "message": format!("JWT signing failed: {}", e) })),
        )
    })?;

    Ok(Json(serde_json::json!({
        "token_id": token.token_id,
        "name": token.name,
        "scopes": token.scopes,
        "token": jwt,
        "expires_at": token.expires_at.as_ref().map(|ts| ts.seconds),
    })))
}

async fn delete_api_token(
    State(state): State<AuthRouteState>,
    claims_opt: Option<axum::extract::Extension<JwtClaims>>,
    Path(token_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_opt(claims_opt, state.auth_disabled().await)?;

    state
        .token_repo
        .revoke(&token_id, &claims.sub, claims.is_admin)
        .await
        .map_err(|e| {
            let status = match &e {
                ApiTokenRepositoryError::NotFound(_) => StatusCode::NOT_FOUND,
                ApiTokenRepositoryError::PermissionDenied(_) => StatusCode::FORBIDDEN,
                ApiTokenRepositoryError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status,
                Json(serde_json::json!({ "code": status.as_u16(), "message": e.to_string() })),
            )
        })?;

    Ok(Json(serde_json::json!({ "ok": true })))
}
