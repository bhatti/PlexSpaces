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

//! Tests for simplified security.proto (TDD Phase 1.1)

#[cfg(test)]
mod tests {
    use plexspaces_proto::security::v1::{
        ServiceIdentity, MtlsConfig, JwtConfig, ApiKey,
        AuthenticateServiceRequest, AuthenticateServiceResponse,
        AuthenticateApiRequest, AuthenticateApiResponse,
    };
    use prost_types::Timestamp;

    #[test]
    fn test_service_identity_creation() {
        let identity = ServiceIdentity {
            service_id: "actor-service".to_string(),
            certificate: b"-----BEGIN CERTIFICATE-----\n...".to_vec(),
            private_key: b"-----BEGIN PRIVATE KEY-----\n...".to_vec(),
            expires_at: Some(Timestamp {
                seconds: 1735689600, // Future timestamp
                nanos: 0,
            }),
            allowed_services: vec!["scheduler-service".to_string(), "node-service".to_string()],
        };

        assert_eq!(identity.service_id, "actor-service");
        assert!(!identity.certificate.is_empty());
        assert!(!identity.private_key.is_empty());
        assert_eq!(identity.allowed_services.len(), 2);
    }

    #[test]
    fn test_mtls_config_creation() {
        let config = MtlsConfig {
            enable_mtls: true,
            ca_certificate: "-----BEGIN CERTIFICATE-----\n...".to_string(),
            trusted_services: vec!["actor-service".to_string(), "scheduler-service".to_string()],
            certificate_rotation_interval: Some(prost_types::Duration {
                seconds: 86400 * 30, // 30 days
                nanos: 0,
            }),
        };

        assert!(config.enable_mtls);
        assert!(!config.ca_certificate.is_empty());
        assert_eq!(config.trusted_services.len(), 2);
    }

    #[test]
    fn test_jwt_config_creation() {
        let config = JwtConfig {
            enable_jwt: true,
            issuer: "https://auth.example.com".to_string(),
            jwks_url: "https://auth.example.com/.well-known/jwks.json".to_string(),
            allowed_audiences: vec!["plexspaces-api".to_string(), "plexspaces-admin".to_string()],
            token_ttl: Some(prost_types::Duration {
                seconds: 3600, // 1 hour
                nanos: 0,
            }),
        };

        assert!(config.enable_jwt);
        assert_eq!(config.issuer, "https://auth.example.com");
        assert_eq!(config.allowed_audiences.len(), 2);
    }

    #[test]
    fn test_api_key_simplified() {
        let api_key = ApiKey {
            id: "key-123".to_string(),
            name: "service-api-key".to_string(),
            description: "API key for service authentication".to_string(),
            key_hash: "sha256:...".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
            expires_at: Some(Timestamp {
                seconds: 1735689600,
                nanos: 0,
            }),
            last_used: None,
            metadata: None,
            // Note: user_id field removed (service-scoped, not user-scoped)
        };

        assert_eq!(api_key.id, "key-123");
        assert_eq!(api_key.name, "service-api-key");
        assert_eq!(api_key.scopes.len(), 2);
    }

    #[test]
    fn test_authenticate_service_request() {
        let request = AuthenticateServiceRequest {
            service_id: "actor-service".to_string(),
            certificate: b"-----BEGIN CERTIFICATE-----\n...".to_vec(),
            signature: b"signature_bytes".to_vec(),
        };

        assert_eq!(request.service_id, "actor-service");
        assert!(!request.certificate.is_empty());
        assert!(!request.signature.is_empty());
    }

    #[test]
    fn test_authenticate_service_response() {
        let identity = ServiceIdentity {
            service_id: "actor-service".to_string(),
            certificate: b"cert".to_vec(),
            private_key: b"key".to_vec(),
            expires_at: Some(Timestamp { seconds: 1735689600, nanos: 0 }),
            allowed_services: vec![],
        };

        let response = AuthenticateServiceResponse {
            identity: Some(identity),
            token: "short-lived-token".to_string(),
            expires_at: Some(Timestamp {
                seconds: 1735689600 + 3600, // 1 hour from now
                nanos: 0,
            }),
        };

        assert!(response.identity.is_some());
        assert_eq!(response.token, "short-lived-token");
        assert!(response.expires_at.is_some());
    }

    #[test]
    fn test_authenticate_api_request() {
        let request = AuthenticateApiRequest {
            jwt_token: "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...".to_string(),
        };

        assert!(!request.jwt_token.is_empty());
    }

    #[test]
    fn test_authenticate_api_response() {
        let response = AuthenticateApiResponse {
            service_id: "external-service".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
            expires_at: Some(Timestamp {
                seconds: 1735689600 + 3600,
                nanos: 0,
            }),
        };

        assert_eq!(response.service_id, "external-service");
        assert_eq!(response.scopes.len(), 2);
    }
}

