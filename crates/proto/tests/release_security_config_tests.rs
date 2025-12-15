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

//! Tests for SecurityConfig in release.proto (TDD)

#[cfg(test)]
mod tests {
    use plexspaces_proto::node::v1::{ReleaseSpec, RuntimeConfig, SecurityConfig};
    use plexspaces_proto::security::v1::{ServiceIdentity, MtlsConfig, JwtConfig, ApiKey};
    use plexspaces_proto::prost_types;

    #[test]
    fn test_security_config_creation() {
        let service_identity = ServiceIdentity {
            service_id: "actor-service".to_string(),
            certificate: b"-----BEGIN CERTIFICATE-----\n...".to_vec(),
            private_key: b"-----BEGIN PRIVATE KEY-----\n...".to_vec(),
            expires_at: Some(prost_types::Timestamp {
                seconds: 1735689600,
                nanos: 0,
            }),
            allowed_services: vec!["scheduler-service".to_string()],
        };

        let mtls_config = MtlsConfig {
            enable_mtls: true,
            ca_certificate: "-----BEGIN CERTIFICATE-----\n...".to_string(),
            trusted_services: vec!["scheduler-service".to_string()],
            certificate_rotation_interval: Some(prost_types::Duration {
                seconds: 86400 * 30,
                nanos: 0,
            }),
        };

        let jwt_config = JwtConfig {
            enable_jwt: true,
            issuer: "https://auth.example.com".to_string(),
            jwks_url: "https://auth.example.com/.well-known/jwks.json".to_string(),
            allowed_audiences: vec!["plexspaces-api".to_string()],
            token_ttl: Some(prost_types::Duration {
                seconds: 3600,
                nanos: 0,
            }),
        };

        let api_key = ApiKey {
            id: "key-123".to_string(),
            name: "service-api-key".to_string(),
            description: "API key for service authentication".to_string(),
            key_hash: "sha256:...".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
            expires_at: Some(prost_types::Timestamp {
                seconds: 1735689600,
                nanos: 0,
            }),
            last_used: None,
            metadata: None,
        };

        let security_config = SecurityConfig {
            service_identity: Some(service_identity),
            mtls: Some(mtls_config),
            jwt: Some(jwt_config),
            api_keys: vec![api_key],
        };

        assert!(security_config.service_identity.is_some());
        assert!(security_config.mtls.is_some());
        assert!(security_config.jwt.is_some());
        assert_eq!(security_config.api_keys.len(), 1);
    }

    #[test]
    fn test_runtime_config_with_security() {
        let security_config = SecurityConfig {
            service_identity: Some(ServiceIdentity {
                service_id: "node-1".to_string(),
                certificate: b"cert".to_vec(),
                private_key: b"key".to_vec(),
                expires_at: None,
                allowed_services: vec![],
            }),
            mtls: None,
            jwt: None,
            api_keys: vec![],
        };

        let runtime_config = RuntimeConfig {
            grpc: None,
            health: None,
            security: Some(security_config),
        };

        assert!(runtime_config.security.is_some());
        assert!(runtime_config.security.as_ref().unwrap().service_identity.is_some());
    }

    #[test]
    fn test_release_spec_with_security() {
        let security_config = SecurityConfig {
            service_identity: Some(ServiceIdentity {
                service_id: "my-node".to_string(),
                certificate: b"cert".to_vec(),
                private_key: b"key".to_vec(),
                expires_at: None,
                allowed_services: vec![],
            }),
            mtls: None,
            jwt: None,
            api_keys: vec![],
        };

        let runtime_config = RuntimeConfig {
            grpc: None,
            health: None,
            security: Some(security_config),
        };

        let release_spec = ReleaseSpec {
            name: "test-release".to_string(),
            version: "1.0.0".to_string(),
            description: "Test release".to_string(),
            node: None,
            runtime: Some(runtime_config),
            system_applications: vec![],
            applications: vec![],
            env: std::collections::HashMap::new(),
            shutdown: None,
        };

        assert!(release_spec.runtime.is_some());
        assert!(release_spec.runtime.as_ref().unwrap().security.is_some());
    }
}

