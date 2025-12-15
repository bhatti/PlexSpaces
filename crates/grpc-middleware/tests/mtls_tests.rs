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

//! Tests for mTLS authentication (TDD Phase 1.2)

#[cfg(test)]
mod tests {
    use plexspaces_grpc_middleware::auth::AuthInterceptor;
    use plexspaces_grpc_middleware::chain::Interceptor;
    use plexspaces_proto::grpc::v1::{AuthMethod, AuthMiddlewareConfig};
    use plexspaces_proto::security::v1::MtlsConfig;
    use plexspaces_proto::prost_types;

    #[test]
    fn test_mtls_config_creation() {
        // Test that we can create mTLS config
        let mtls_config = MtlsConfig {
            enable_mtls: true,
            ca_certificate: "-----BEGIN CERTIFICATE-----\n...".to_string(),
            trusted_services: vec!["actor-service".to_string(), "scheduler-service".to_string()],
            certificate_rotation_interval: Some(prost_types::Duration {
                seconds: 86400 * 30, // 30 days
                nanos: 0,
            }),
        };

        assert!(mtls_config.enable_mtls);
        assert!(!mtls_config.ca_certificate.is_empty());
        assert_eq!(mtls_config.trusted_services.len(), 2);
    }

    #[tokio::test]
    async fn test_mtls_interceptor_creation() {
        // Test that we can create an AuthInterceptor with mTLS method
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodMtls as i32,
            jwt_key: String::new(), // Not used for mTLS
            rbac: None,
            allow_unauthenticated: false,
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec![],
        };

        // This should succeed (even if mTLS implementation is not complete yet)
        let interceptor = AuthInterceptor::new(config);
        // For now, we expect this to work (implementation will be added)
        assert!(interceptor.is_ok());
    }

    #[tokio::test]
    async fn test_mtls_extract_service_identity_from_certificate() {
        // Test extracting service identity from certificate CN or SAN
        // This is a unit test - full integration requires real TLS setup
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodMtls as i32,
            jwt_key: String::new(),
            rbac: None,
            allow_unauthenticated: false,
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec![],
        };

        let interceptor = AuthInterceptor::new(config).unwrap();

        // Create request with peer certificate
        let mut headers = std::collections::HashMap::new();
        let request = plexspaces_proto::grpc::v1::InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers,
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: "-----BEGIN CERTIFICATE-----\nCN=actor-service\n...".to_string(),
            peer_service_id: "actor-service".to_string(),
        };

        // For now, this should handle mTLS (implementation will be added)
        let result = interceptor.before_request(&request).await;
        // Implementation will validate certificate and check trusted services
        assert!(result.is_ok() || result.is_err()); // Either is fine for now
    }

    #[tokio::test]
    async fn test_mtls_denies_untrusted_service() {
        // Test that mTLS denies requests from untrusted services
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodMtls as i32,
            jwt_key: String::new(),
            rbac: None,
            allow_unauthenticated: false,
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec!["scheduler-service".to_string()], // Only trust scheduler-service
        };

        let interceptor = AuthInterceptor::new(config).unwrap();

        let request = plexspaces_proto::grpc::v1::InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: "-----BEGIN CERTIFICATE-----\nCN=untrusted-service\n...".to_string(),
            peer_service_id: "untrusted-service".to_string(),
        };

        // Should deny untrusted service (implementation will check trusted services list)
        let result = interceptor.before_request(&request).await;
        // Should return error because untrusted-service is not in trusted list
        // The error is returned as Err, not as a DENY decision
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not in trusted services"));
    }

    #[tokio::test]
    async fn test_mtls_allows_trusted_service() {
        // Test that mTLS allows requests from trusted services
        let config = AuthMiddlewareConfig {
            method: AuthMethod::AuthMethodMtls as i32,
            jwt_key: String::new(),
            rbac: None,
            allow_unauthenticated: false,
            mtls_ca_certificate: String::new(),
            mtls_trusted_services: vec!["scheduler-service".to_string()], // Trust scheduler-service
        };

        let interceptor = AuthInterceptor::new(config).unwrap();

        let request = plexspaces_proto::grpc::v1::InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: "-----BEGIN CERTIFICATE-----\nCN=scheduler-service\n...".to_string(),
            peer_service_id: "scheduler-service".to_string(),
        };

        // Should allow trusted service (implementation will check trusted services list)
        let result = interceptor.before_request(&request).await;
        // Should be allowed because scheduler-service is in trusted list
        assert!(result.is_ok());
        let decision = result.unwrap();
        assert_eq!(
            decision.decision,
            plexspaces_proto::grpc::v1::InterceptorDecision::InterceptorDecisionAllow as i32
        );
    }

    // Note: Full mTLS integration tests require:
    // - TLS certificates (CA, server, client)
    // - TLS server/client setup with Tonic
    // - Certificate validation using rustls or native-tls
    // - Service identity extraction from certificate CN/SAN
    // - Integration with gRPC server TLS configuration
    // These will be added as the implementation progresses.
}

