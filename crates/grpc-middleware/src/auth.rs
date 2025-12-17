// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! # AuthInterceptor - JWT Authentication & RBAC
//!
//! ## Purpose
//! Validates JWT tokens and enforces RBAC policies for gRPC requests.
//!
//! ## Features
//! - JWT token validation (HS256, RS256)
//! - Role-Based Access Control (RBAC)
//! - Service/method-level permissions
//! - Optional permissive mode (allow unauthenticated)
//!
//! ## Configuration
//! Uses proto `AuthMiddlewareConfig`:
//! - `jwt_key`: Secret (HS256) or public key path (RS256)
//! - `rbac`: Role definitions and user assignments
//! - `allow_unauthenticated`: Permissive mode flag

use crate::chain::{Interceptor, InterceptorError};
use async_trait::async_trait;
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use plexspaces_proto::grpc::v1::{
    AuthMethod, AuthMiddlewareConfig, InterceptorDecision, InterceptorRequest, InterceptorResponse,
    InterceptorResult,
};
use serde::{Deserialize, Serialize};
use std::fs;

/// JWT Authentication Interceptor
///
/// ## Purpose
/// Validates JWT tokens from `authorization` header and enforces RBAC policies.
///
/// ## Design
/// - Extracts JWT from "Bearer <token>" header
/// - Validates signature using configured key
/// - Checks expiration (exp claim)
/// - Enforces RBAC if configured
///
/// ## Proto Integration
/// - Config: `AuthMiddlewareConfig` from proto
/// - Claims: Maps JWT to proto `JwtClaims`
/// - RBAC: Uses proto `RbacConfig`
pub struct AuthInterceptor {
    config: AuthMiddlewareConfig,
    decoding_key: Option<DecodingKey>,
    mtls_trusted_services: std::collections::HashSet<String>,
}

/// Internal JWT claims for jsonwebtoken crate
///
/// ## Design Note
/// This is a Rust-side mirror of proto `JwtClaims` for deserialization.
/// We extract from JWT, then can convert to proto type if needed.
#[derive(Debug, Serialize, Deserialize)]
struct InternalJwtClaims {
    sub: String,
    exp: i64,
    iat: i64,
    #[serde(default)]
    iss: String,
    #[serde(default)]
    aud: Vec<String>,
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    tenant_id: String,
}

impl AuthInterceptor {
    /// Create AuthInterceptor from proto config
    ///
    /// ## Arguments
    /// * `config` - Proto `AuthMiddlewareConfig` with JWT key and RBAC rules
    ///
    /// ## Returns
    /// Configured interceptor ready to validate requests
    ///
    /// ## Errors
    /// - Invalid JWT key format
    /// - Unreadable public key file
    pub fn new(config: AuthMiddlewareConfig) -> Result<Self, InterceptorError> {
        let decoding_key = if config.method == AuthMethod::AuthMethodJwt as i32 {
            Some(Self::load_decoding_key(&config.jwt_key)?)
        } else {
            None
        };

        // Build trusted services set for mTLS
        let mtls_trusted_services: std::collections::HashSet<String> = config
            .mtls_trusted_services
            .iter()
            .map(|s| s.clone())
            .collect();

        Ok(Self {
            config,
            decoding_key,
            mtls_trusted_services,
        })
    }

    /// Load JWT decoding key from config
    ///
    /// ## Arguments
    /// * `key_str` - Either secret string or path to public key file
    ///
    /// ## Returns
    /// DecodingKey for JWT validation
    ///
    /// ## Supports
    /// - HS256: Direct secret string
    /// - RS256: Path to PEM public key file
    fn load_decoding_key(key_str: &str) -> Result<DecodingKey, InterceptorError> {
        // If key_str looks like a file path, load it
        if key_str.ends_with(".pem") || key_str.starts_with("/") || key_str.starts_with("./") {
            let pem_data = fs::read(key_str).map_err(|e| {
                InterceptorError::AuthenticationFailed(format!(
                    "Failed to read public key file {}: {}",
                    key_str, e
                ))
            })?;
            DecodingKey::from_rsa_pem(&pem_data).map_err(|e| {
                InterceptorError::AuthenticationFailed(format!("Invalid RSA public key: {}", e))
            })
        } else {
            // Treat as HMAC secret
            Ok(DecodingKey::from_secret(key_str.as_bytes()))
        }
    }

    /// Extract JWT token from authorization header
    ///
    /// ## Arguments
    /// * `headers` - Request headers map
    ///
    /// ## Returns
    /// JWT token string (without "Bearer " prefix)
    ///
    /// ## Header Format
    /// `authorization: Bearer <token>`
    fn extract_token(headers: &std::collections::HashMap<String, String>) -> Option<String> {
        headers
            .get("authorization")
            .and_then(|auth| auth.strip_prefix("Bearer ").map(|s| s.to_string()))
    }

    /// Validate JWT token
    ///
    /// ## Arguments
    /// * `token` - JWT token string
    ///
    /// ## Returns
    /// Validated JWT claims
    ///
    /// ## Validates
    /// - Signature (using configured key)
    /// - Expiration (exp claim)
    /// - Issuer (if configured)
    fn validate_jwt(&self, token: &str) -> Result<InternalJwtClaims, InterceptorError> {
        let decoding_key = self.decoding_key.as_ref().ok_or_else(|| {
            InterceptorError::AuthenticationFailed("JWT decoding key not configured".to_string())
        })?;

        // Determine algorithm from token header
        let header = decode_header(token).map_err(|e| {
            InterceptorError::AuthenticationFailed(format!("Invalid JWT header: {}", e))
        })?;

        let mut validation = Validation::new(header.alg);

        // Disable audience validation (can enable if needed)
        validation.validate_aud = false;

        // Decode and validate
        let token_data =
            decode::<InternalJwtClaims>(token, decoding_key, &validation).map_err(|e| {
                InterceptorError::AuthenticationFailed(format!("JWT validation failed: {}", e))
            })?;

        Ok(token_data.claims)
    }

    /// Check RBAC permissions
    ///
    /// ## Arguments
    /// * `claims` - Validated JWT claims with roles
    /// * `method` - gRPC method path (e.g., "/ActorService/SpawnActor")
    ///
    /// ## Returns
    /// `Ok(())` if permitted, error otherwise
    ///
    /// ## Logic
    /// 1. Extract service and method from path
    /// 2. For each user role, check if role has permission
    /// 3. Allow if ANY role grants access
    fn check_rbac(&self, claims: &InternalJwtClaims, method: &str) -> Result<(), InterceptorError> {
        let rbac = match &self.config.rbac {
            Some(r) => r,
            None => return Ok(()), // No RBAC configured, allow
        };

        // Parse method: "/package.Service/Method" or "/Service/Method"
        let (service, method_name) = Self::parse_method(method);

        // Check if user has ANY role that grants access
        for role_name in &claims.roles {
            if let Some(role_perms) = rbac.roles.get(role_name) {
                // Check service permission
                if !role_perms.allowed_services.is_empty()
                    && !role_perms.allowed_services.contains(&service.to_string())
                {
                    continue; // This role doesn't grant service access
                }

                // Check method permission
                if !role_perms.allowed_methods.is_empty()
                    && !role_perms
                        .allowed_methods
                        .contains(&method_name.to_string())
                {
                    continue; // This role doesn't grant method access
                }

                // Role grants access
                return Ok(());
            }
        }

        // No role granted access
        Err(InterceptorError::AuthorizationFailed(format!(
            "User {} with roles {:?} not authorized for {}/{}",
            claims.sub, claims.roles, service, method_name
        )))
    }

    /// Parse gRPC method path
    ///
    /// ## Arguments
    /// * `method` - Full gRPC method path
    ///
    /// ## Returns
    /// (service_name, method_name) tuple
    ///
    /// ## Examples
    /// - `/plexspaces.actor.v1.ActorService/SpawnActor` → ("ActorService", "SpawnActor")
    /// - `/ActorService/SpawnActor` → ("ActorService", "SpawnActor")
    fn parse_method(method: &str) -> (&str, &str) {
        if let Some(slash_pos) = method.rfind('/') {
            let service_path = &method[1..slash_pos];
            let method_name = &method[slash_pos + 1..];

            let service_name = if let Some(dot_pos) = service_path.rfind('.') {
                &service_path[dot_pos + 1..]
            } else {
                service_path
            };

            (service_name, method_name)
        } else {
            ("unknown", method)
        }
    }

    /// Validate mTLS service identity
    ///
    /// ## Arguments
    /// * `peer_service_id` - Service ID extracted from peer certificate
    ///
    /// ## Returns
    /// `Ok(())` if service is trusted, error otherwise
    ///
    /// ## Logic
    /// 1. If trusted services list is empty, allow all (permissive)
    /// 2. Otherwise, check if service_id is in trusted services list
    fn validate_mtls_service(&self, peer_service_id: &str) -> Result<(), InterceptorError> {
        // If no trusted services specified, allow all (permissive mode)
        if self.mtls_trusted_services.is_empty() {
            return Ok(());
        }

        // Check if service is in trusted list
        if self.mtls_trusted_services.contains(peer_service_id) {
            Ok(())
        } else {
            Err(InterceptorError::AuthenticationFailed(format!(
                "Service '{}' is not in trusted services list",
                peer_service_id
            )))
        }
    }

    /// Extract service identity from peer certificate
    ///
    /// ## Arguments
    /// * `peer_certificate` - PEM-encoded certificate
    ///
    /// ## Returns
    /// Service ID (CN or first SAN DNS name)
    ///
    /// ## Note
    /// This is a simplified implementation. Full implementation would:
    /// - Parse X.509 certificate
    /// - Extract CN from subject
    /// - Extract DNS names from SAN extension
    /// - Validate certificate chain
    fn extract_service_id_from_certificate(
        _peer_certificate: &str,
    ) -> Result<String, InterceptorError> {
        // TODO: Implement full certificate parsing
        // For now, this is a placeholder that will be implemented with rustls/x509-parser
        // The actual implementation should:
        // 1. Parse PEM certificate
        // 2. Extract CN from subject
        // 3. Extract DNS names from SAN
        // 4. Return first available service identifier
        Err(InterceptorError::AuthenticationFailed(
            "Certificate parsing not yet implemented".to_string(),
        ))
    }

    /// Handle mTLS authentication
    async fn handle_mtls_auth(
        &self,
        context: &InterceptorRequest,
    ) -> Result<InterceptorResult, InterceptorError> {
        // Check if peer certificate is present
        if context.peer_certificate.is_empty() {
            if self.config.allow_unauthenticated {
                // Permissive mode: allow
                return Ok(InterceptorResult {
                    decision: InterceptorDecision::InterceptorDecisionAllow as i32,
                    error_message: String::new(),
                    modified_headers: std::collections::HashMap::new(),
                    metrics: vec![],
                });
            } else {
                // Strict mode: deny
                return Ok(InterceptorResult {
                    decision: InterceptorDecision::InterceptorDecisionDeny as i32,
                    error_message: "Missing peer certificate for mTLS authentication".to_string(),
                    modified_headers: std::collections::HashMap::new(),
                    metrics: vec![],
                });
            }
        }

        // Extract service identity from certificate or use provided peer_service_id
        let service_id = if !context.peer_service_id.is_empty() {
            // Service ID already extracted by TLS layer
            context.peer_service_id.clone()
        } else {
            // Extract from certificate (fallback)
            Self::extract_service_id_from_certificate(&context.peer_certificate)?
        };

        // Validate service is trusted
        self.validate_mtls_service(&service_id)?;

        // mTLS authentication successful
        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers: std::collections::HashMap::new(),
            metrics: vec![],
        })
    }

    /// Handle JWT authentication
    async fn handle_jwt_auth(
        &self,
        context: &InterceptorRequest,
    ) -> Result<InterceptorResult, InterceptorError> {
        // Extract token from authorization header
        let token = match Self::extract_token(&context.headers) {
            Some(t) => t,
            None => {
                // No token provided
                if self.config.allow_unauthenticated {
                    // Permissive mode: allow
                    return Ok(InterceptorResult {
                        decision: InterceptorDecision::InterceptorDecisionAllow as i32,
                        error_message: String::new(),
                        modified_headers: std::collections::HashMap::new(),
                        metrics: vec![],
                    });
                } else {
                    // Strict mode: deny
                    return Ok(InterceptorResult {
                        decision: InterceptorDecision::InterceptorDecisionDeny as i32,
                        error_message: "Missing authorization header".to_string(),
                        modified_headers: std::collections::HashMap::new(),
                        metrics: vec![],
                    });
                }
            }
        };

        // Validate JWT
        let claims = self.validate_jwt(&token)?;

        // Check RBAC permissions
        self.check_rbac(&claims, &context.method)?;

        // Extract tenant_id and user_id from claims and add to headers for downstream use
        let mut modified_headers = std::collections::HashMap::new();
        if !claims.tenant_id.is_empty() {
            modified_headers.insert("x-tenant-id".to_string(), claims.tenant_id.clone());
        }
        modified_headers.insert("x-user-id".to_string(), claims.sub.clone());
        if !claims.roles.is_empty() {
            modified_headers.insert("x-user-roles".to_string(), claims.roles.join(","));
        }

        // Authentication and authorization successful
        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers,
            metrics: vec![],
        })
    }
}

impl Default for AuthInterceptor {
    fn default() -> Self {
        // Default: permissive mode, no authentication
        Self {
            config: AuthMiddlewareConfig {
                method: AuthMethod::AuthMethodUnspecified as i32,
                jwt_key: String::new(),
                rbac: None,
                allow_unauthenticated: true,
                mtls_ca_certificate: String::new(),
                mtls_trusted_services: vec![],
            },
            decoding_key: None,
            mtls_trusted_services: std::collections::HashSet::new(),
        }
    }
}

#[async_trait]
impl Interceptor for AuthInterceptor {
    async fn before_request(
        &self,
        context: &InterceptorRequest,
    ) -> Result<InterceptorResult, InterceptorError> {
        // Route to appropriate authentication method
        match self.config.method {
            method if method == AuthMethod::AuthMethodMtls as i32 => {
                // mTLS authentication
                self.handle_mtls_auth(context).await
            }
            method if method == AuthMethod::AuthMethodJwt as i32 => {
                // JWT authentication
                self.handle_jwt_auth(context).await
            }
            _ => {
                // Unspecified or unsupported method
                if self.config.allow_unauthenticated {
                    // Permissive mode: allow
                    Ok(InterceptorResult {
                        decision: InterceptorDecision::InterceptorDecisionAllow as i32,
                        error_message: String::new(),
                        modified_headers: std::collections::HashMap::new(),
                        metrics: vec![],
                    })
                } else {
                    // Strict mode: deny
                    Ok(InterceptorResult {
                        decision: InterceptorDecision::InterceptorDecisionDeny as i32,
                        error_message: "Authentication method not configured".to_string(),
                        modified_headers: std::collections::HashMap::new(),
                        metrics: vec![],
                    })
                }
            }
        }
    }

    async fn after_response(
        &self,
        _context: &InterceptorResponse,
    ) -> Result<InterceptorResult, InterceptorError> {
        // No post-processing needed for auth
        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers: std::collections::HashMap::new(),
            metrics: vec![],
        })
    }

    fn name(&self) -> &str {
        "auth"
    }

    fn priority(&self) -> i32 {
        30 // After tracing/metrics, before business logic
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_method() {
        let (service, method) =
            AuthInterceptor::parse_method("/plexspaces.actor.v1.ActorService/SpawnActor");
        assert_eq!(service, "ActorService");
        assert_eq!(method, "SpawnActor");

        let (service, method) = AuthInterceptor::parse_method("/ActorService/SpawnActor");
        assert_eq!(service, "ActorService");
        assert_eq!(method, "SpawnActor");
    }

    #[test]
    fn test_extract_token() {
        let mut headers = std::collections::HashMap::new();
        headers.insert(
            "authorization".to_string(),
            "Bearer my-secret-token".to_string(),
        );

        let token = AuthInterceptor::extract_token(&headers);
        assert_eq!(token, Some("my-secret-token".to_string()));

        let empty = AuthInterceptor::extract_token(&std::collections::HashMap::new());
        assert_eq!(empty, None);
    }

    #[tokio::test]
    async fn test_permissive_mode_allows_unauthenticated() {
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodJwt as i32,
            jwt_key: "test-secret".to_string(),
            rbac: None,
            allow_unauthenticated: true,
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec![],
        };

        let interceptor = AuthInterceptor::new(config).unwrap();

        let context = InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(), // No authorization header
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        let result = interceptor.before_request(&context).await.unwrap();
        assert_eq!(result.decision, InterceptorDecision::InterceptorDecisionAllow as i32);
    }

    #[tokio::test]
    async fn test_strict_mode_denies_unauthenticated() {
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodJwt as i32,
            jwt_key: "test-secret".to_string(),
            rbac: None,
            allow_unauthenticated: false,
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec![],
        };

        let interceptor = AuthInterceptor::new(config).unwrap();

        let context = InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(), // No authorization header
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        let result = interceptor.before_request(&context).await.unwrap();
        assert_eq!(result.decision, InterceptorDecision::InterceptorDecisionDeny as i32);
        assert!(result
            .error_message
            .contains("Missing authorization header"));
    }
}
