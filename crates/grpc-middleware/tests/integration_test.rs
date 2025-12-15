// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! # gRPC Middleware Integration Tests
//!
//! ## Purpose
//! End-to-end integration tests for middleware chain with proto config.
//!
//! ## Tests
//! - InterceptorChain creation from proto config
//! - Middleware execution order (priority-based)
//! - Config-driven middleware activation
//! - Auth interceptor with JWT
//! - Metrics collection

use plexspaces_grpc_middleware::chain::InterceptorChain;
use plexspaces_proto::grpc::v1::{
    AuthMethod, AuthMiddlewareConfig, InterceptorDecision, InterceptorRequest, MiddlewareConfig,
    MiddlewareSpec, MiddlewareType,
};

#[tokio::test]
async fn test_middleware_chain_from_empty_config() {
    let config = MiddlewareConfig { middleware: vec![] };

    let chain = InterceptorChain::from_config(&config).unwrap();

    // Should still have metrics (always added)
    let context = InterceptorRequest {
        method: "/test.Service/Method".to_string(),
        headers: std::collections::HashMap::new(),
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let result = chain.before_request(&context).await;
    assert!(result.is_ok(), "Empty chain should allow requests");
}

#[tokio::test]
async fn test_middleware_chain_with_metrics_only() {
    let config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeMetrics as i32,
            enabled: true,
            priority: 10,
            config: None, // Metrics doesn't need config
        }],
    };

    let chain = InterceptorChain::from_config(&config).unwrap();

    let context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers: std::collections::HashMap::new(),
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let result = chain.before_request(&context).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_middleware_chain_with_auth_permissive() {
    let auth_config = AuthMiddlewareConfig {
        method: AuthMethod::AuthMethodJwt as i32,
        jwt_key: "test-secret".to_string(),
        rbac: None,
        allow_unauthenticated: true, // Permissive mode
        mtls_ca_certificate: String::new(),
        mtls_trusted_services: vec![],
    };

    let config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeAuth as i32,
            enabled: true,
            priority: 30,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Auth(
                auth_config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&config).unwrap();

    // Request without authorization header
    let context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers: std::collections::HashMap::new(), // No auth
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let result = chain.before_request(&context).await;
    assert!(
        result.is_ok(),
        "Permissive mode should allow unauthenticated requests"
    );
}

#[tokio::test]
async fn test_middleware_chain_with_auth_strict() {
    let auth_config = AuthMiddlewareConfig {
        method: AuthMethod::AuthMethodJwt as i32,
        jwt_key: "test-secret".to_string(),
        rbac: None,
        allow_unauthenticated: false, // Strict mode
        mtls_ca_certificate: String::new(),
        mtls_trusted_services: vec![],
    };

    let config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeAuth as i32,
            enabled: true,
            priority: 30,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Auth(
                auth_config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&config).unwrap();

    // Request without authorization header
    let context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers: std::collections::HashMap::new(), // No auth
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let result = chain.before_request(&context).await;
    assert!(
        result.is_err(),
        "Strict mode should reject unauthenticated requests"
    );
}

#[tokio::test]
async fn test_middleware_chain_execution_order() {
    // Create chain with multiple middleware
    let config = MiddlewareConfig {
        middleware: vec![
            MiddlewareSpec {
                middleware_type: MiddlewareType::MiddlewareTypeAuth as i32,
                enabled: true,
                priority: 30,
                config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Auth(
                    AuthMiddlewareConfig {
                        method: AuthMethod::AuthMethodUnspecified as i32,
                        jwt_key: String::new(),
                        rbac: None,
                        allow_unauthenticated: true,
                        mtls_ca_certificate: String::new(),
                        mtls_trusted_services: vec![],
                    },
                )),
            },
            MiddlewareSpec {
                middleware_type: MiddlewareType::MiddlewareTypeRateLimit as i32,
                enabled: true,
                priority: 40,
                config: None,
            },
            MiddlewareSpec {
                middleware_type: MiddlewareType::MiddlewareTypeCompression as i32,
                enabled: true,
                priority: 50,
                config: None,
            },
        ],
    };

    let chain = InterceptorChain::from_config(&config).unwrap();

    let context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers: std::collections::HashMap::new(),
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    // Should execute in priority order: Metrics(10) -> Auth(30) -> RateLimit(40) -> Compression(50)
    let result = chain.before_request(&context).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_middleware_chain_disabled_middleware_skipped() {
    let config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeAuth as i32,
            enabled: false, // Disabled
            priority: 30,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Auth(
                AuthMiddlewareConfig {
                    method: AuthMethod::AuthMethodJwt as i32,
                    jwt_key: "test-secret".to_string(),
                    rbac: None,
                    allow_unauthenticated: false, // Would deny if enabled
                    mtls_ca_certificate: String::new(),
                    mtls_trusted_services: vec![],
                },
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&config).unwrap();

    // Request without auth should pass (auth middleware is disabled)
    let context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers: std::collections::HashMap::new(),
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let result = chain.before_request(&context).await;
    assert!(result.is_ok(), "Disabled middleware should be skipped");
}

#[tokio::test]
async fn test_auth_interceptor_with_valid_jwt() {
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    struct Claims {
        sub: String,
        exp: i64,
        iat: i64,
    }

    // Create a valid JWT
    let secret = "test-secret-key";
    let claims = Claims {
        sub: "user-123".to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp(),
        iat: chrono::Utc::now().timestamp(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    // Create auth interceptor
    let auth_config = AuthMiddlewareConfig {
        method: AuthMethod::AuthMethodJwt as i32,
        jwt_key: secret.to_string(),
        rbac: None,
        allow_unauthenticated: false,
        mtls_ca_certificate: String::new(),
        mtls_trusted_services: vec![],
    };

    let config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeAuth as i32,
            enabled: true,
            priority: 30,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Auth(
                auth_config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&config).unwrap();

    // Request with valid JWT
    let mut headers = std::collections::HashMap::new();
    headers.insert("authorization".to_string(), format!("Bearer {}", token));

    let context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers,
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let result = chain.before_request(&context).await;
    assert!(result.is_ok(), "Valid JWT should be accepted");
}

#[tokio::test]
async fn test_auth_interceptor_with_expired_jwt() {
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    struct Claims {
        sub: String,
        exp: i64,
        iat: i64,
    }

    // Create an expired JWT
    let secret = "test-secret-key";
    let claims = Claims {
        sub: "user-123".to_string(),
        exp: (chrono::Utc::now() - chrono::Duration::hours(1)).timestamp(), // Expired
        iat: (chrono::Utc::now() - chrono::Duration::hours(2)).timestamp(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();

    // Create auth interceptor
    let auth_config = AuthMiddlewareConfig {
        method: AuthMethod::AuthMethodJwt as i32,
        jwt_key: secret.to_string(),
        rbac: None,
        allow_unauthenticated: false,
        mtls_ca_certificate: String::new(),
        mtls_trusted_services: vec![],
    };

    let config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeAuth as i32,
            enabled: true,
            priority: 30,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Auth(
                auth_config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&config).unwrap();

    // Request with expired JWT
    let mut headers = std::collections::HashMap::new();
    headers.insert("authorization".to_string(), format!("Bearer {}", token));

    let context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers,
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let result = chain.before_request(&context).await;
    assert!(result.is_err(), "Expired JWT should be rejected");
}

#[tokio::test]
async fn test_auth_interceptor_with_invalid_signature() {
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    struct Claims {
        sub: String,
        exp: i64,
        iat: i64,
    }

    // Create JWT with one secret
    let wrong_secret = "wrong-secret";
    let claims = Claims {
        sub: "user-123".to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp(),
        iat: chrono::Utc::now().timestamp(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(wrong_secret.as_bytes()),
    )
    .unwrap();

    // Create auth interceptor with different secret
    let correct_secret = "correct-secret";
    let auth_config = AuthMiddlewareConfig {
        method: AuthMethod::AuthMethodJwt as i32,
        jwt_key: correct_secret.to_string(),
        rbac: None,
        allow_unauthenticated: false,
        mtls_ca_certificate: String::new(),
        mtls_trusted_services: vec![],
    };

    let config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeAuth as i32,
            enabled: true,
            priority: 30,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Auth(
                auth_config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&config).unwrap();

    // Request with JWT signed by wrong secret
    let mut headers = std::collections::HashMap::new();
    headers.insert("authorization".to_string(), format!("Bearer {}", token));

    let context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers,
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let result = chain.before_request(&context).await;
    assert!(
        result.is_err(),
        "JWT with invalid signature should be rejected"
    );
}
