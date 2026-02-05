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

//! JWT Validation Helpers
//!
//! ## Purpose
//! Provides shared JWT validation logic for both gRPC (AuthInterceptor) and HTTP
//! (Axum gateway) authentication. Centralizes token parsing, validation, and
//! claim extraction.
//!
//! ## Usage
//! ```rust,ignore
//! use plexspaces_grpc_middleware::jwt::{validate_bearer_token, JwtClaims};
//!
//! // From HTTP Authorization header
//! let claims = validate_bearer_token("my-secret", Some("Bearer eyJ..."))?;
//! println!("Tenant: {}", claims.tenant_id);
//!
//! // Or just the token (without Bearer prefix)
//! let claims = validate_jwt_token("my-secret", "eyJ...")?;
//! ```

use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

/// Hint message for auth-related errors
pub const AUTH_REQUIRED_HINT: &str =
    " For local testing, set PLEXSPACES_DISABLE_AUTH=1.";

/// JWT claims extracted from token
///
/// ## Purpose
/// Contains all claims needed for PlexSpaces authentication and authorization.
/// Used by both gRPC interceptors and HTTP middleware.
///
/// ## Standard Claims (RFC 7519)
/// - `sub`: Subject (user ID)
/// - `exp`: Expiration time
/// - `iat`: Issued at time
/// - `iss`: Issuer
/// - `aud`: Audience
///
/// ## Custom Claims (PlexSpaces)
/// - `tenant_id`: Tenant identifier for multi-tenancy
/// - `roles`: User roles for RBAC
/// - `groups`: User groups for group-based access
/// - `is_admin`: Admin flag for elevated privileges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject (user ID)
    pub sub: String,
    /// Expiration time (Unix timestamp)
    pub exp: i64,
    /// Issued at time (Unix timestamp)
    pub iat: i64,
    /// Issuer
    #[serde(default)]
    pub iss: String,
    /// Audience
    #[serde(default)]
    pub aud: Vec<String>,
    /// Tenant ID for multi-tenancy
    #[serde(default)]
    pub tenant_id: String,
    /// User roles for RBAC
    #[serde(default)]
    pub roles: Vec<String>,
    /// User groups for group-based access
    #[serde(default)]
    pub groups: Vec<String>,
    /// Admin flag
    #[serde(default)]
    pub is_admin: bool,
}

impl JwtClaims {
    /// Get namespace (defaults to empty, can be set from request context)
    ///
    /// Note: Namespace is not typically in JWT - it comes from request context.
    /// This is provided for compatibility with HTTP gateway patterns.
    pub fn namespace(&self) -> &str {
        ""
    }

    /// Convert JWT claims to RequestContext
    ///
    /// ## Purpose
    /// Creates a properly-constructed `RequestContext` from validated JWT claims.
    /// This is the canonical way to convert authentication info to request context.
    ///
    /// ## Arguments
    /// * `namespace` - Namespace from request (path, header, or default)
    /// * `auth_enabled` - Whether authentication is enabled (from SecurityConfig)
    ///
    /// ## Returns
    /// `RequestContext` with tenant_id from JWT, user_id from sub, and admin flag.
    ///
    /// ## Example
    /// ```rust,ignore
    /// use plexspaces_grpc_middleware::jwt::{validate_bearer_token, JwtClaims};
    ///
    /// let claims = validate_bearer_token("secret", Some("Bearer eyJ..."))?;
    /// let ctx = claims.to_request_context("my-namespace".to_string(), true);
    /// ```
    pub fn to_request_context(
        &self,
        namespace: String,
        auth_enabled: bool,
    ) -> plexspaces_common::RequestContext {
        // Since we have validated JWT, we know tenant_id is present
        // Use new() which validates tenant_id when auth_enabled
        let ctx = plexspaces_common::RequestContext::new(
            self.tenant_id.clone(),
            namespace,
            auth_enabled,
        )
        .expect("JWT claims have validated tenant_id");

        ctx.with_user_id(self.sub.clone())
           .with_admin(self.is_admin)
    }

    /// Convert JWT claims to RequestContext with default namespace
    ///
    /// ## Purpose
    /// Convenience method when namespace is not available from request.
    /// Uses empty string as namespace.
    pub fn to_request_context_default(&self, auth_enabled: bool) -> plexspaces_common::RequestContext {
        self.to_request_context(String::new(), auth_enabled)
    }
}

/// Validate a Bearer token from Authorization header
///
/// ## Arguments
/// * `secret` - JWT secret (for HS256)
/// * `auth_header` - Authorization header value (e.g., "Bearer eyJ...")
///
/// ## Returns
/// * `Ok(JwtClaims)` - Validated claims
/// * `Err(String)` - Error message with hint for resolution
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_grpc_middleware::jwt::validate_bearer_token;
///
/// // From HTTP request
/// let auth_header = request.headers().get("authorization").and_then(|v| v.to_str().ok());
/// let claims = validate_bearer_token("my-secret", auth_header)?;
/// ```
pub fn validate_bearer_token(secret: &str, auth_header: Option<&str>) -> Result<JwtClaims, String> {
    // Extract token from Bearer header
    let token = auth_header
        .and_then(|v| v.strip_prefix("Bearer ").map(str::trim))
        .ok_or_else(|| {
            format!(
                "Missing or invalid Authorization header (expected: Bearer <token>).{}",
                AUTH_REQUIRED_HINT
            )
        })?;

    validate_jwt_token(secret, token)
}

/// Validate a JWT token directly (without Bearer prefix)
///
/// ## Arguments
/// * `secret` - JWT secret (for HS256)
/// * `token` - JWT token string
///
/// ## Returns
/// * `Ok(JwtClaims)` - Validated claims
/// * `Err(String)` - Error message with hint
///
/// ## Security
/// - Uses HS256 algorithm (pinned, not from token header to prevent algorithm confusion)
/// - Validates expiration
/// - Does not validate audience (can be enabled if needed)
pub fn validate_jwt_token(secret: &str, token: &str) -> Result<JwtClaims, String> {
    let key = DecodingKey::from_secret(secret.as_bytes());
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.validate_aud = false;

    let token_data = decode::<JwtClaims>(token, &key, &validation).map_err(|e| {
        format!(
            "JWT validation failed: {} (token may be expired or invalid).{}",
            e, AUTH_REQUIRED_HINT
        )
    })?;

    let claims = token_data.claims;

    // Validate tenant_id is present (required for multi-tenancy)
    if claims.tenant_id.is_empty() {
        return Err(format!(
            "JWT missing tenant_id claim.{}",
            AUTH_REQUIRED_HINT
        ));
    }

    Ok(claims)
}

/// Extract tenant_id from JWT or headers with fallback
///
/// ## Purpose
/// Resolves effective tenant_id based on auth mode:
/// - If auth enabled: tenant_id MUST come from JWT (security)
/// - If auth disabled: tenant_id can come from headers or path
///
/// ## Arguments
/// * `jwt_claims` - Optional JWT claims (from validated token)
/// * `auth_disabled` - Whether auth is disabled
/// * `jwt_secret` - Optional JWT secret for re-validation
/// * `auth_header` - Optional Authorization header for re-validation
/// * `header_tenant_id` - Tenant ID from x-tenant-id header (fallback when auth disabled)
///
/// ## Returns
/// Effective tenant_id string
pub fn resolve_tenant_id(
    jwt_claims: Option<&JwtClaims>,
    auth_disabled: bool,
    jwt_secret: Option<&str>,
    auth_header: Option<&str>,
    header_tenant_id: Option<&str>,
) -> String {
    // If we have validated claims, use them
    if let Some(claims) = jwt_claims {
        return claims.tenant_id.clone();
    }

    // If auth is disabled, use header fallback
    if auth_disabled {
        return header_tenant_id.unwrap_or_default().to_string();
    }

    // Try to validate from Authorization header
    if let Some(secret) = jwt_secret {
        if let Ok(claims) = validate_bearer_token(secret, auth_header) {
            return claims.tenant_id;
        }
    }

    // Fallback to header (for backward compatibility, though less secure)
    header_tenant_id.unwrap_or_default().to_string()
}

/// Generate a test JWT token (for testing only)
///
/// ## Arguments
/// * `secret` - JWT secret
/// * `tenant_id` - Tenant ID to include in claims
/// * `sub` - Subject (user ID)
/// * `expires_in_secs` - Token validity in seconds
///
/// ## Returns
/// Encoded JWT token string
///
/// ## Warning
/// This is for testing only. Production tokens should be generated by
/// a proper identity provider.
#[cfg(any(test, feature = "test-helpers"))]
pub fn generate_test_token(
    secret: &str,
    tenant_id: &str,
    sub: &str,
    expires_in_secs: i64,
) -> Result<String, String> {
    use jsonwebtoken::{encode, EncodingKey, Header};

    let now = chrono::Utc::now().timestamp();
    let claims = JwtClaims {
        sub: sub.to_string(),
        exp: now + expires_in_secs,
        iat: now,
        iss: "plexspaces-test".to_string(),
        aud: vec!["plexspaces-api".to_string()],
        tenant_id: tenant_id.to_string(),
        roles: vec!["user".to_string()],
        groups: vec![],
        is_admin: false,
    };

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| format!("Failed to generate token: {}", e))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-at-least-32-chars";

    fn create_test_token(tenant_id: &str, sub: &str, exp_offset: i64) -> String {
        use jsonwebtoken::{encode, EncodingKey, Header};

        let now = chrono::Utc::now().timestamp();
        let claims = JwtClaims {
            sub: sub.to_string(),
            exp: now + exp_offset,
            iat: now,
            iss: "test".to_string(),
            aud: vec![],
            tenant_id: tenant_id.to_string(),
            roles: vec!["user".to_string()],
            groups: vec![],
            is_admin: false,
        };

        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_bytes()),
        )
        .unwrap()
    }

    #[test]
    fn test_validate_bearer_token_valid() {
        let token = create_test_token("tenant-123", "user-456", 3600);
        let auth_header = format!("Bearer {}", token);

        let result = validate_bearer_token(TEST_SECRET, Some(&auth_header));
        assert!(result.is_ok());

        let claims = result.unwrap();
        assert_eq!(claims.tenant_id, "tenant-123");
        assert_eq!(claims.sub, "user-456");
    }

    #[test]
    fn test_validate_bearer_token_missing_header() {
        let result = validate_bearer_token(TEST_SECRET, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing or invalid Authorization header"));
    }

    #[test]
    fn test_validate_bearer_token_missing_bearer_prefix() {
        let token = create_test_token("tenant-123", "user-456", 3600);
        let result = validate_bearer_token(TEST_SECRET, Some(&token));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing or invalid Authorization header"));
    }

    #[test]
    fn test_validate_bearer_token_invalid_token() {
        let result = validate_bearer_token(TEST_SECRET, Some("Bearer invalid-token"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("JWT validation failed"));
    }

    #[test]
    fn test_validate_bearer_token_expired() {
        let token = create_test_token("tenant-123", "user-456", -3600); // Already expired
        let auth_header = format!("Bearer {}", token);

        let result = validate_bearer_token(TEST_SECRET, Some(&auth_header));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("JWT validation failed"));
    }

    #[test]
    fn test_validate_bearer_token_wrong_secret() {
        let token = create_test_token("tenant-123", "user-456", 3600);
        let auth_header = format!("Bearer {}", token);

        let result = validate_bearer_token("wrong-secret-key-that-wont-work!", Some(&auth_header));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_jwt_token_missing_tenant_id() {
        use jsonwebtoken::{encode, EncodingKey, Header};

        let now = chrono::Utc::now().timestamp();
        let claims = JwtClaims {
            sub: "user-456".to_string(),
            exp: now + 3600,
            iat: now,
            iss: "test".to_string(),
            aud: vec![],
            tenant_id: String::new(), // Empty tenant_id
            roles: vec![],
            groups: vec![],
            is_admin: false,
        };

        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_bytes()),
        )
        .unwrap();

        let result = validate_jwt_token(TEST_SECRET, &token);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing tenant_id"));
    }

    #[test]
    fn test_resolve_tenant_id_from_claims() {
        let claims = JwtClaims {
            sub: "user".to_string(),
            exp: 0,
            iat: 0,
            iss: String::new(),
            aud: vec![],
            tenant_id: "jwt-tenant".to_string(),
            roles: vec![],
            groups: vec![],
            is_admin: false,
        };

        let result = resolve_tenant_id(Some(&claims), false, None, None, Some("header-tenant"));
        assert_eq!(result, "jwt-tenant");
    }

    #[test]
    fn test_resolve_tenant_id_auth_disabled() {
        let result = resolve_tenant_id(None, true, None, None, Some("header-tenant"));
        assert_eq!(result, "header-tenant");
    }

    #[test]
    fn test_resolve_tenant_id_no_fallback() {
        let result = resolve_tenant_id(None, true, None, None, None);
        assert_eq!(result, "");
    }

    #[test]
    fn test_generate_test_token() {
        let token = generate_test_token(TEST_SECRET, "test-tenant", "test-user", 3600).unwrap();
        let auth_header = format!("Bearer {}", token);

        let claims = validate_bearer_token(TEST_SECRET, Some(&auth_header)).unwrap();
        assert_eq!(claims.tenant_id, "test-tenant");
        assert_eq!(claims.sub, "test-user");
    }

    // ========================================================================
    // JwtClaims::to_request_context() tests
    // ========================================================================

    #[test]
    fn test_to_request_context_basic() {
        let claims = JwtClaims {
            sub: "user-123".to_string(),
            exp: 0,
            iat: 0,
            iss: String::new(),
            aud: vec![],
            tenant_id: "tenant-456".to_string(),
            roles: vec!["admin".to_string()],
            groups: vec![],
            is_admin: false,
        };

        let ctx = claims.to_request_context("my-namespace".to_string(), true);

        assert_eq!(ctx.tenant_id(), "tenant-456");
        assert_eq!(ctx.namespace(), "my-namespace");
        assert_eq!(ctx.user_id(), Some("user-123"));
        assert!(!ctx.is_admin());
    }

    #[test]
    fn test_to_request_context_with_admin() {
        let claims = JwtClaims {
            sub: "admin-user".to_string(),
            exp: 0,
            iat: 0,
            iss: String::new(),
            aud: vec![],
            tenant_id: "tenant-789".to_string(),
            roles: vec!["superadmin".to_string()],
            groups: vec![],
            is_admin: true,
        };

        let ctx = claims.to_request_context("prod".to_string(), true);

        assert_eq!(ctx.tenant_id(), "tenant-789");
        assert_eq!(ctx.namespace(), "prod");
        assert_eq!(ctx.user_id(), Some("admin-user"));
        assert!(ctx.is_admin());
    }

    #[test]
    fn test_to_request_context_default_namespace() {
        let claims = JwtClaims {
            sub: "user".to_string(),
            exp: 0,
            iat: 0,
            iss: String::new(),
            aud: vec![],
            tenant_id: "tenant".to_string(),
            roles: vec![],
            groups: vec![],
            is_admin: false,
        };

        let ctx = claims.to_request_context_default(false);

        assert_eq!(ctx.tenant_id(), "tenant");
        assert_eq!(ctx.namespace(), "");
        assert_eq!(ctx.user_id(), Some("user"));
    }

    #[test]
    fn test_to_request_context_from_validated_token() {
        // End-to-end: validate token, then convert to RequestContext
        let token = generate_test_token(TEST_SECRET, "e2e-tenant", "e2e-user", 3600).unwrap();
        let auth_header = format!("Bearer {}", token);

        let claims = validate_bearer_token(TEST_SECRET, Some(&auth_header)).unwrap();
        let ctx = claims.to_request_context("app-ns".to_string(), true);

        assert_eq!(ctx.tenant_id(), "e2e-tenant");
        assert_eq!(ctx.namespace(), "app-ns");
        assert_eq!(ctx.user_id(), Some("e2e-user"));
        assert!(!ctx.is_admin());
        // Verify request_id is generated (ULID format)
        assert!(!ctx.request_id().is_empty());
        assert!(ctx.request_id().len() == 26, "ULID should be 26 chars");
    }

    #[test]
    fn test_to_request_context_auth_disabled() {
        let claims = JwtClaims {
            sub: "user".to_string(),
            exp: 0,
            iat: 0,
            iss: String::new(),
            aud: vec![],
            tenant_id: "tenant".to_string(),
            roles: vec![],
            groups: vec![],
            is_admin: false,
        };

        // auth_enabled = false should still work
        let ctx = claims.to_request_context("ns".to_string(), false);
        assert_eq!(ctx.tenant_id(), "tenant");
        assert!(!ctx.auth_enabled);
    }
}
