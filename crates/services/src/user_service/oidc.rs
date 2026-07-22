// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! OIDC authentication logic for dashboard OAuth login.
//!
//! ## Flow
//! 1. GET /api/v1/auth/oidc/login → redirect to OIDC provider
//! 2. GET /api/v1/auth/oidc/callback?code=...&state=... → exchange code, get user, issue JWT
//!
//! ## Design
//! All OIDC logic lives here as plain async functions that operate on `Arc<OidcState>`.
//! Route mounting is handled by the `auth_routes` module in the node crate — this module
//! has no dependency on axum routing primitives (no `Router`, no `with_state`).
//! This keeps the service layer independent of the HTTP framework's routing DSL.

use axum::response::{IntoResponse, Redirect, Response};
use plexspaces_proto::security::v1::{GetOrCreateByEmailRequest, OidcConfig};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::repository::UserRepository;
use super::tenant_repository::TenantRepository;

// ─── Internal data structures ─────────────────────────────────────────────────

/// OIDC discovery document (subset of fields we need).
#[derive(Debug, Clone, Deserialize)]
struct OidcDiscovery {
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    userinfo_endpoint: String,
    issuer: String,
}

/// Token response from OIDC provider.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    token_type: String,
}

/// UserInfo response (standard OIDC claims).
#[derive(Debug, Deserialize)]
struct UserInfo {
    sub: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    picture: String,
    #[serde(flatten)]
    extra: std::collections::HashMap<String, serde_json::Value>,
}

// ─── Public state ─────────────────────────────────────────────────────────────

/// Shared state for OIDC flows. Constructed once at node startup by `build_oidc_state`.
pub struct OidcState {
    discovery: OidcDiscovery,
    http_client: reqwest::Client,
    pub config: OidcConfig,
    user_repo: Arc<dyn UserRepository>,
    tenant_repo: Arc<dyn TenantRepository>,
    jwt_key_pair: Arc<plexspaces_grpc_middleware::JwtKeyPair>,
    /// CSRF state tokens pending callback validation. Maps state → creation time (for TTL).
    pending_states: Arc<RwLock<std::collections::HashMap<String, u64>>>,
}

/// Construct an `OidcState` from configuration, performing the OIDC discovery fetch.
///
/// Returns `Err(OidcError::NotConfigured)` when OIDC is disabled or incompletely configured.
/// Returns `Err(OidcError::Discovery)` when the discovery URL cannot be fetched.
pub async fn build_oidc_state(
    config: &OidcConfig,
    user_repo: Arc<dyn UserRepository>,
    tenant_repo: Arc<dyn TenantRepository>,
    jwt_key_pair: Arc<plexspaces_grpc_middleware::JwtKeyPair>,
) -> Result<Arc<OidcState>, OidcError> {
    if !config.enabled || config.discovery_url.is_empty() || config.client_id.is_empty() {
        return Err(OidcError::NotConfigured);
    }

    let http_client = reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| OidcError::Configuration(format!("HTTP client build failed: {}", e)))?;

    let discovery: OidcDiscovery = http_client
        .get(&config.discovery_url)
        .send()
        .await
        .map_err(|e| OidcError::Discovery(format!("Discovery fetch failed: {}", e)))?
        .json()
        .await
        .map_err(|e| OidcError::Discovery(format!("Discovery parse failed: {}", e)))?;

    Ok(Arc::new(OidcState {
        discovery,
        http_client,
        config: config.clone(),
        user_repo,
        tenant_repo,
        jwt_key_pair,
        pending_states: Arc::new(RwLock::new(std::collections::HashMap::new())),
    }))
}

// ─── Public request params ────────────────────────────────────────────────────

/// Query parameters for the OIDC callback.
#[derive(Debug, Deserialize)]
pub struct OidcCallbackParams {
    pub code: String,
    pub state: String,
}

// ─── Public request handlers ─────────────────────────────────────────────────

/// Handle `GET /api/v1/auth/oidc/login` — generate CSRF state and redirect to OIDC provider.
pub async fn handle_login(oidc: Arc<OidcState>) -> Response {
    let csrf_state = ulid::Ulid::new().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    {
        let mut states = oidc.pending_states.write().await;
        // Evict states older than 5 minutes + cap at 1000 to prevent memory DoS.
        states.retain(|_, created_at| now.saturating_sub(*created_at) < 300);
        if states.len() > 1000 {
            states.clear();
        }
        states.insert(csrf_state.clone(), now);
    }

    let scopes = if oidc.config.scopes.is_empty() {
        "openid email profile".to_string()
    } else {
        oidc.config.scopes.join(" ")
    };

    let redirect_uri = if oidc.config.redirect_uri.is_empty() {
        "/api/v1/auth/oidc/callback".to_string()
    } else {
        oidc.config.redirect_uri.clone()
    };

    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
        oidc.discovery.authorization_endpoint,
        urlencoding::encode(&oidc.config.client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&scopes),
        urlencoding::encode(&csrf_state),
    );

    Redirect::temporary(&auth_url).into_response()
}

/// Handle `GET /api/v1/auth/oidc/callback` — exchange code, persist user, issue session JWT.
pub async fn handle_callback(oidc: Arc<OidcState>, params: OidcCallbackParams) -> Response {
    match do_callback(oidc, params).await {
        Ok(response) => response,
        Err(e) => e.into_response(),
    }
}

// ─── Callback implementation ─────────────────────────────────────────────────

async fn do_callback(
    oidc: Arc<OidcState>,
    params: OidcCallbackParams,
) -> Result<Response, OidcCallbackError> {
    // Validate CSRF state — prevents cross-site request forgery on callback.
    oidc.pending_states
        .write()
        .await
        .remove(&params.state)
        .ok_or(OidcCallbackError::InvalidState)?;

    let client_secret = if oidc.config.client_secret.is_empty() {
        std::env::var("PLEXSPACES_OIDC_CLIENT_SECRET").map_err(|_| {
            OidcCallbackError::TokenExchange("OIDC client secret not configured".into())
        })?
    } else {
        oidc.config.client_secret.clone()
    };

    let redirect_uri = if oidc.config.redirect_uri.is_empty() {
        "/api/v1/auth/oidc/callback".to_string()
    } else {
        oidc.config.redirect_uri.clone()
    };

    let token_http_response = oidc
        .http_client
        .post(&oidc.discovery.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &params.code),
            ("client_id", &oidc.config.client_id),
            ("client_secret", &client_secret),
            ("redirect_uri", &redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| OidcCallbackError::TokenExchange(e.to_string()))?;

    let status = token_http_response.status();
    let body_text = token_http_response
        .text()
        .await
        .map_err(|e| OidcCallbackError::TokenExchange(e.to_string()))?;

    if !status.is_success() {
        return Err(OidcCallbackError::TokenExchange(format!(
            "provider returned HTTP {}: {}",
            status.as_u16(),
            body_text
        )));
    }

    let token_response: TokenResponse = serde_json::from_str(&body_text)
        .map_err(|e| OidcCallbackError::TokenExchange(format!("{}: {}", e, body_text)))?;

    let user_info: UserInfo = oidc
        .http_client
        .get(&oidc.discovery.userinfo_endpoint)
        .bearer_auth(&token_response.access_token)
        .send()
        .await
        .map_err(|e| OidcCallbackError::ClaimVerification(e.to_string()))?
        .json()
        .await
        .map_err(|e| OidcCallbackError::ClaimVerification(e.to_string()))?;

    if user_info.email.is_empty() {
        return Err(OidcCallbackError::MissingEmail);
    }

    // Resolve the tenant slug from OIDC claims (or fall back to config default).
    let tenant_slug = if !oidc.config.tenant_claim.is_empty() {
        user_info
            .extra
            .get(&oidc.config.tenant_claim)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&oidc.config.default_tenant_id)
            .to_string()
    } else {
        oidc.config.default_tenant_id.clone()
    };

    // Ensure the tenant record exists before creating the user (tenant is the parent).
    let (tenant, _) = oidc
        .tenant_repo
        .get_or_create_by_slug(&tenant_slug, &tenant_slug)
        .await
        .map_err(|e| OidcCallbackError::UserCreation(format!("tenant create failed: {e}")))?;

    let tenant_id = tenant.tenant_id;

    let (user, _created) = oidc
        .user_repo
        .get_or_create_by_email(&GetOrCreateByEmailRequest {
            request_id: ulid::Ulid::new().to_string(),
            email: user_info.email.clone(),
            tenant_id: tenant_id.clone(),
            display_name: user_info.name.clone(),
            avatar_url: user_info.picture.clone(),
            provider: oidc.discovery.issuer.clone(),
            provider_sub: user_info.sub.clone(),
            roles: vec![],
            groups: vec![],
        })
        .await
        .map_err(|e| OidcCallbackError::UserCreation(e.to_string()))?;

    let is_admin = user.admin
        || (!oidc.config.admin_groups.is_empty()
            && user.groups.iter().any(|g| oidc.config.admin_groups.contains(g)));

    let jwt_claims = plexspaces_grpc_middleware::JwtClaims {
        sub: user.user_id.clone(),
        exp: chrono::Utc::now().timestamp() + 3600,
        iat: chrono::Utc::now().timestamp(),
        iss: "plexspaces".to_string(),
        aud: vec![],
        tenant_id: tenant_id.to_string(),
        roles: user.roles.clone(),
        groups: user.groups.clone(),
        is_admin,
        jti: None,
    };

    let token = plexspaces_grpc_middleware::sign_jwt_with_keypair(&oidc.jwt_key_pair, &jwt_claims)
        .map_err(|e| OidcCallbackError::JwtCreation(e))?;

    metrics::counter!("plexspaces_auth_logins_total", "status" => "oidc_success").increment(1);
    tracing::info!(
        email = %user_info.email,
        tenant_id = %tenant_id,
        admin = %is_admin,
        "auth.oidc.login.success"
    );

    // Set JWT as HttpOnly+SameSite cookie to prevent token leakage.
    // Include Secure flag only when redirect_uri uses HTTPS (not localhost dev).
    let is_secure = oidc.config.redirect_uri.starts_with("https://");
    let secure_flag = if is_secure { "; Secure" } else { "" };
    let samesite = if is_secure { "Strict" } else { "Lax" };
    let cookie = format!(
        "plexspaces_token={}; Path=/; HttpOnly; SameSite={}; Max-Age=3600{}",
        token, samesite, secure_flag
    );
    Ok(Response::builder()
        .status(axum::http::StatusCode::FOUND)
        .header(axum::http::header::LOCATION, "/dashboard")
        .header(axum::http::header::SET_COOKIE, cookie)
        .body(axum::body::Body::empty())
        .unwrap())
}

// ─── Error types ──────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    #[error("OIDC not configured")]
    NotConfigured,
    #[error("Configuration error: {0}")]
    Configuration(String),
    #[error("Discovery error: {0}")]
    Discovery(String),
}

impl std::fmt::Display for OidcCallbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidState => write!(f, "Invalid CSRF state"),
            Self::TokenExchange(e) => write!(f, "Token exchange failed: {e}"),
            Self::ClaimVerification(e) => write!(f, "Claim verification failed: {e}"),
            Self::MissingEmail => write!(f, "Email claim missing from OIDC token"),
            Self::UserCreation(e) => write!(f, "User creation failed: {e}"),
            Self::JwtCreation(e) => write!(f, "JWT creation failed: {e}"),
        }
    }
}

#[derive(Debug)]
pub enum OidcCallbackError {
    InvalidState,
    TokenExchange(String),
    ClaimVerification(String),
    MissingEmail,
    UserCreation(String),
    JwtCreation(String),
}

impl IntoResponse for OidcCallbackError {
    fn into_response(self) -> axum::response::Response {
        metrics::counter!("plexspaces_auth_logins_total", "status" => "oidc_failed").increment(1);
        tracing::warn!(error = %self, "auth.oidc.login.failed");

        match &self {
            Self::InvalidState | Self::TokenExchange(_) => {
                // Stale/replayed authorization codes or CSRF mismatch — redirect back to
                // login so the user gets a fresh flow instead of seeing a bare error page.
                Redirect::temporary("/api/v1/auth/oidc/login").into_response()
            }
            Self::ClaimVerification(_) => {
                (axum::http::StatusCode::UNAUTHORIZED, "Token verification failed").into_response()
            }
            Self::MissingEmail => {
                (axum::http::StatusCode::BAD_REQUEST, "Email claim missing from token")
                    .into_response()
            }
            Self::UserCreation(_) => {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "User creation failed")
                    .into_response()
            }
            Self::JwtCreation(_) => {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "JWT creation failed")
                    .into_response()
            }
        }
    }
}
