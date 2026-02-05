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

//! HTTP JWT Validation
//!
//! ## Purpose
//! Re-exports shared JWT validation from grpc-middleware for HTTP gateway use.
//! This ensures consistent JWT validation between HTTP and gRPC endpoints.
//!
//! ## Design
//! - Single source of truth: `plexspaces_grpc_middleware::jwt`
//! - `JwtClaims` contains all needed fields (tenant_id, sub, is_admin)
//! - `JwtClaims::to_request_context()` converts to proper `RequestContext`
//!
//! ## Usage
//! ```rust,ignore
//! use plexspaces_node::http_jwt::{validate_bearer_token, JwtClaims};
//!
//! let claims = validate_bearer_token("secret", Some("Bearer eyJ..."))?;
//! let ctx = claims.to_request_context("namespace".to_string(), true);
//! ```

// Re-export everything from grpc-middleware JWT module
pub use plexspaces_grpc_middleware::jwt::{
    validate_bearer_token,
    validate_jwt_token,
    resolve_tenant_id,
    JwtClaims,
    AUTH_REQUIRED_HINT,
};

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_token(secret: &str, tenant_id: &str, exp_offset_secs: i64) -> String {
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
    fn test_validate_bearer_token_valid() {
        let secret = "test-secret";
        let token = make_token(secret, "tenant-1", 3600);
        let auth = format!("Bearer {}", token);
        let out = validate_bearer_token(secret, Some(&auth));
        assert!(out.is_ok(), "valid token should succeed: {:?}", out);
        let c = out.unwrap();
        assert_eq!(c.tenant_id, "tenant-1");
        assert_eq!(c.sub, "test-user");
        assert!(!c.is_admin);
    }

    #[test]
    fn test_validate_bearer_token_missing_header() {
        let out = validate_bearer_token("secret", None);
        assert!(out.is_err());
        let e = out.unwrap_err();
        assert!(e.contains("Missing or invalid Authorization"));
        assert!(e.contains("PLEXSPACES_DISABLE_AUTH"));
    }

    #[test]
    fn test_validate_bearer_token_empty() {
        let out = validate_bearer_token("secret", Some("Bearer "));
        assert!(out.is_err());
    }

    #[test]
    fn test_validate_bearer_token_wrong_secret() {
        let token = make_token("right-secret", "t1", 3600);
        let auth = format!("Bearer {}", token);
        let out = validate_bearer_token("wrong-secret", Some(&auth));
        assert!(out.is_err());
        let e = out.unwrap_err();
        assert!(e.contains("JWT validation failed"));
        assert!(e.contains("PLEXSPACES_DISABLE_AUTH"));
    }

    #[test]
    fn test_validate_bearer_token_expired() {
        let token = make_token("secret", "t1", -3600); // expired 1h ago
        let auth = format!("Bearer {}", token);
        let out = validate_bearer_token("secret", Some(&auth));
        assert!(out.is_err());
        let e = out.unwrap_err();
        assert!(e.contains("JWT validation failed") || e.contains("expired"));
        assert!(e.contains("PLEXSPACES_DISABLE_AUTH"));
    }

    #[test]
    fn test_validate_bearer_token_missing_tenant_id() {
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
        let out = validate_bearer_token("secret", Some(&auth));
        assert!(out.is_err());
        let e = out.unwrap_err();
        assert!(e.contains("tenant_id"));
        assert!(e.contains("PLEXSPACES_DISABLE_AUTH"));
    }

    #[test]
    fn test_validate_bearer_token_not_bearer_prefix() {
        let token = make_token("s", "t", 3600);
        let out = validate_bearer_token("s", Some(&token)); // no "Bearer " prefix
        assert!(out.is_err());
    }

    #[test]
    fn test_jwt_claims_to_request_context() {
        let claims = JwtClaims {
            sub: "user-123".to_string(),
            exp: 0,
            iat: 0,
            iss: String::new(),
            aud: vec![],
            tenant_id: "tenant-456".to_string(),
            roles: vec!["admin".to_string()],
            groups: vec![],
            is_admin: true,
        };

        let ctx = claims.to_request_context("my-namespace".to_string(), true);
        assert_eq!(ctx.tenant_id(), "tenant-456");
        assert_eq!(ctx.namespace(), "my-namespace");
        assert_eq!(ctx.user_id(), Some("user-123"));
        assert!(ctx.is_admin());
    }
}
