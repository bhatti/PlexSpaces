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

//! Tests for JWT authentication middleware (TDD Phase 1.3)

#[cfg(test)]
mod tests {
    use plexspaces_grpc_middleware::auth::AuthInterceptor;
    use plexspaces_grpc_middleware::chain::Interceptor;
    use plexspaces_proto::grpc::v1::{AuthMethod, AuthMiddlewareConfig};
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    // Test JWT claims structure
    #[derive(serde::Serialize, serde::Deserialize)]
    struct TestClaims {
        sub: String,
        exp: i64,
        iat: i64,
        iss: String,
        aud: Vec<String>,
        roles: Vec<String>,
        tenant_id: String,
    }

    fn create_test_jwt(secret: &str, claims: &TestClaims) -> String {
        let header = Header::new(Algorithm::HS256);
        let key = EncodingKey::from_secret(secret.as_bytes());
        encode(&header, claims, &key).unwrap()
    }

    #[tokio::test]
    async fn test_jwt_interceptor_creation() {
        // Test that we can create an AuthInterceptor with JWT method
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodJwt as i32,
            jwt_key: "test-secret".to_string(),
            rbac: None,
            allow_unauthenticated: false,
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec![],
        };

        let interceptor = AuthInterceptor::new(config);
        assert!(interceptor.is_ok());
    }

    #[tokio::test]
    async fn test_jwt_validates_token() {
        // Test that JWT interceptor validates valid tokens
        let secret = "test-secret-key";
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodJwt as i32,
            jwt_key: secret.to_string(),
            rbac: None,
            allow_unauthenticated: false,
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec![],
        };

        let interceptor = AuthInterceptor::new(config).unwrap();

        // Create valid JWT token
        let claims = TestClaims {
            sub: "user123".to_string(),
            exp: chrono::Utc::now().timestamp() + 3600, // 1 hour from now
            iat: chrono::Utc::now().timestamp(),
            iss: "test-issuer".to_string(),
            aud: vec![],
            roles: vec!["admin".to_string()],
            tenant_id: "tenant1".to_string(),
        };
        let token = create_test_jwt(secret, &claims);

        // Create request with valid JWT token
        let mut headers = std::collections::HashMap::new();
        headers.insert("authorization".to_string(), format!("Bearer {}", token));
        let request = plexspaces_proto::grpc::v1::InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers,
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        // Should allow valid token
        let result = interceptor.before_request(&request).await;
        assert!(result.is_ok());
        let decision = result.unwrap();
        assert_eq!(
            decision.decision,
            plexspaces_proto::grpc::v1::InterceptorDecision::InterceptorDecisionAllow as i32
        );
    }

    #[tokio::test]
    async fn test_jwt_rejects_invalid_token() {
        // Test that JWT interceptor rejects invalid tokens
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodJwt as i32,
            jwt_key: "correct-secret".to_string(),
            rbac: None,
            allow_unauthenticated: false,
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec![],
        };

        let interceptor = AuthInterceptor::new(config).unwrap();

        // Create JWT token with wrong secret
        let claims = TestClaims {
            sub: "user123".to_string(),
            exp: chrono::Utc::now().timestamp() + 3600,
            iat: chrono::Utc::now().timestamp(),
            iss: "test-issuer".to_string(),
            aud: vec![],
            roles: vec![],
            tenant_id: String::new(),
        };
        let token = create_test_jwt("wrong-secret", &claims);

        // Create request with invalid JWT token
        let mut headers = std::collections::HashMap::new();
        headers.insert("authorization".to_string(), format!("Bearer {}", token));
        let request = plexspaces_proto::grpc::v1::InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers,
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        // Should reject invalid token
        let result = interceptor.before_request(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_jwt_rejects_expired_token() {
        // Test that JWT interceptor rejects expired tokens
        let secret = "test-secret";
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodJwt as i32,
            jwt_key: secret.to_string(),
            rbac: None,
            allow_unauthenticated: false,
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec![],
        };

        let interceptor = AuthInterceptor::new(config).unwrap();

        // Create expired JWT token
        let claims = TestClaims {
            sub: "user123".to_string(),
            exp: chrono::Utc::now().timestamp() - 3600, // 1 hour ago (expired)
            iat: chrono::Utc::now().timestamp() - 7200, // 2 hours ago
            iss: "test-issuer".to_string(),
            aud: vec![],
            roles: vec![],
            tenant_id: String::new(),
        };
        let token = create_test_jwt(secret, &claims);

        // Create request with expired JWT token
        let mut headers = std::collections::HashMap::new();
        headers.insert("authorization".to_string(), format!("Bearer {}", token));
        let request = plexspaces_proto::grpc::v1::InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers,
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        // Should reject expired token
        let result = interceptor.before_request(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_jwt_allows_unauthenticated_when_permissive() {
        // Test that JWT interceptor allows unauthenticated requests in permissive mode
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodJwt as i32,
            jwt_key: "test-secret".to_string(),
            rbac: None,
            allow_unauthenticated: true, // Permissive mode
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec![],
        };

        let interceptor = AuthInterceptor::new(config).unwrap();

        // Create request without JWT token
        let request = plexspaces_proto::grpc::v1::InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        // Should allow in permissive mode
        let result = interceptor.before_request(&request).await;
        assert!(result.is_ok());
        let decision = result.unwrap();
        assert_eq!(
            decision.decision,
            plexspaces_proto::grpc::v1::InterceptorDecision::InterceptorDecisionAllow as i32
        );
    }

    #[tokio::test]
    async fn test_jwt_denies_unauthenticated_when_strict() {
        // Test that JWT interceptor denies unauthenticated requests in strict mode
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodJwt as i32,
            jwt_key: "test-secret".to_string(),
            rbac: None,
            allow_unauthenticated: false, // Strict mode
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec![],
        };

        let interceptor = AuthInterceptor::new(config).unwrap();

        // Create request without JWT token
        let request = plexspaces_proto::grpc::v1::InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        // Should deny in strict mode
        let result = interceptor.before_request(&request).await;
        assert!(result.is_ok());
        let decision = result.unwrap();
        assert_eq!(
            decision.decision,
            plexspaces_proto::grpc::v1::InterceptorDecision::InterceptorDecisionDeny as i32
        );
        assert!(decision.error_message.contains("Missing authorization"));
    }
}

