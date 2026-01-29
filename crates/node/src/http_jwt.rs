// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Validates JWT for HTTP gateway. Uses same claim structure as grpc-middleware
// (tenant_id, roles, groups, is_admin) so tokens work for both HTTP and gRPC.

use serde::{Deserialize, Serialize};

/// Claims extracted from JWT for HTTP request context (must match grpc-middleware InternalJwtClaims).
#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    sub: String,
    exp: i64,
    iat: i64,
    #[serde(default)]
    tenant_id: String,
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    groups: Vec<String>,
    #[serde(default)]
    is_admin: bool,
}

/// Result of successful JWT validation for HTTP gateway.
#[derive(Debug, Clone)]
pub struct HttpJwtClaims {
    pub tenant_id: String,
    pub namespace: String,
    pub sub: String,
    pub is_admin: bool,
}

/// Validate Authorization Bearer token and return claims for HTTP gateway.
/// Returns error message suitable for 401 response (includes hint when applicable).
pub fn validate_http_jwt(
    secret: &str,
    auth_header: Option<&str>,
) -> Result<HttpJwtClaims, String> {
    let token = auth_header
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    let token = token.ok_or_else(|| {
        format!(
            "Missing or invalid Authorization header (expected: Bearer <token>).{}",
            plexspaces_common::AUTH_REQUIRED_HINT
        )
    })?;

    let key = jsonwebtoken::DecodingKey::from_secret(secret.as_bytes());
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.validate_exp = true;
    validation.validate_aud = false;

    let token_data = jsonwebtoken::decode::<JwtClaims>(token, &key, &validation).map_err(|e| {
        format!(
            "JWT validation failed: {} (token may be expired or invalid).{}",
            e,
            plexspaces_common::AUTH_REQUIRED_HINT
        )
    })?;

    let c = &token_data.claims;
    if c.tenant_id.is_empty() {
        return Err(format!(
            "JWT missing tenant_id claim.{}",
            plexspaces_common::AUTH_REQUIRED_HINT
        ));
    }

    Ok(HttpJwtClaims {
        tenant_id: c.tenant_id.clone(),
        namespace: String::new(), // namespace not in standard claims; can add to JWT later
        sub: c.sub.clone(),
        is_admin: c.is_admin,
    })
}

// ============================================================================
// TESTS (TDD: high coverage for auth path)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_token(
        secret: &str,
        tenant_id: &str,
        exp_offset_secs: i64,
    ) -> String {
        #[derive(serde::Serialize)]
        struct C {
            sub: String,
            exp: i64,
            iat: i64,
            tenant_id: String,
            roles: Vec<String>,
            groups: Vec<String>,
            is_admin: bool,
        }
        let now = chrono::Utc::now().timestamp();
        let claims = C {
            sub: "test-user".to_string(),
            exp: now + exp_offset_secs,
            iat: now,
            tenant_id: tenant_id.to_string(),
            roles: vec!["admin".to_string()],
            groups: vec![],
            is_admin: false,
        };
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        let key = jsonwebtoken::EncodingKey::from_secret(secret.as_bytes());
        jsonwebtoken::encode(&header, &claims, &key).unwrap()
    }

    #[test]
    fn test_validate_http_jwt_valid_token() {
        let secret = "test-secret";
        let token = make_token(secret, "tenant-1", 3600);
        let auth = format!("Bearer {}", token);
        let out = validate_http_jwt(secret, Some(&auth));
        assert!(out.is_ok(), "valid token should succeed: {:?}", out);
        let c = out.unwrap();
        assert_eq!(c.tenant_id, "tenant-1");
        assert_eq!(c.sub, "test-user");
        assert!(!c.is_admin);
    }

    #[test]
    fn test_validate_http_jwt_missing_header() {
        let out = validate_http_jwt("secret", None);
        assert!(out.is_err());
        let e = out.unwrap_err();
        assert!(e.contains("Missing or invalid Authorization"));
        assert!(e.contains("PLEXSPACES_DISABLE_AUTH"));
    }

    #[test]
    fn test_validate_http_jwt_empty_bearer() {
        let out = validate_http_jwt("secret", Some("Bearer "));
        assert!(out.is_err());
    }

    #[test]
    fn test_validate_http_jwt_wrong_secret() {
        let token = make_token("right-secret", "t1", 3600);
        let auth = format!("Bearer {}", token);
        let out = validate_http_jwt("wrong-secret", Some(&auth));
        assert!(out.is_err());
        let e = out.unwrap_err();
        assert!(e.contains("JWT validation failed"));
        assert!(e.contains("PLEXSPACES_DISABLE_AUTH"));
    }

    #[test]
    fn test_validate_http_jwt_expired() {
        let token = make_token("secret", "t1", -3600); // expired 1h ago
        let auth = format!("Bearer {}", token);
        let out = validate_http_jwt("secret", Some(&auth));
        assert!(out.is_err());
        let e = out.unwrap_err();
        assert!(e.contains("JWT validation failed") || e.contains("expired"));
        assert!(e.contains("PLEXSPACES_DISABLE_AUTH"));
    }

    #[test]
    fn test_validate_http_jwt_missing_tenant_id() {
        #[derive(serde::Serialize)]
        struct C {
            sub: String,
            exp: i64,
            iat: i64,
            tenant_id: String,
        }
        let now = chrono::Utc::now().timestamp();
        let claims = C {
            sub: "u".to_string(),
            exp: now + 3600,
            iat: now,
            tenant_id: String::new(),
        };
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        let key = jsonwebtoken::EncodingKey::from_secret(b"secret");
        let token = jsonwebtoken::encode(&header, &claims, &key).unwrap();
        let auth = format!("Bearer {}", token);
        let out = validate_http_jwt("secret", Some(&auth));
        assert!(out.is_err());
        let e = out.unwrap_err();
        assert!(e.contains("tenant_id"));
        assert!(e.contains("PLEXSPACES_DISABLE_AUTH"));
    }

    #[test]
    fn test_validate_http_jwt_not_bearer_prefix() {
        let token = make_token("s", "t", 3600);
        let out = validate_http_jwt("s", Some(&token)); // no "Bearer " prefix
        assert!(out.is_err());
    }
}
