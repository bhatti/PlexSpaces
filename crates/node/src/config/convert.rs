// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Conversion from YAML intermediate representation to proto ReleaseSpec

use super::yaml::{JwtConfigYaml, MtlsConfigYaml, *};
use plexspaces_proto::application::v1::{ApplicationServiceLinkRequirement, ApplicationSpec};
use plexspaces_proto::channel::v1::ChannelProvider;
use plexspaces_proto::node::v1::{
    GrpcConfig, HealthConfig, MiddlewareConfig, NodeConfig, OutboundTransport, ReleaseSpec,
    RuntimeConfig, SecurityConfig, ServiceLinkConfig, ShutdownConfig,
};
use plexspaces_proto::security::v1::{ApiKey, JwtConfig, MtlsConfig, ServiceIdentity};
use plexspaces_proto::storage::v1::{
    BlobConfig, RedisBackendConfig, SharedDbConfig, SqliteBackendConfig, StorageProvider,
    StorageProviderConfig,
};
use std::collections::HashMap;

/// Convert YAML representation to proto ReleaseSpec
pub fn convert_yaml_to_proto(yaml: ReleaseYaml) -> Result<ReleaseSpec, String> {
    Ok(ReleaseSpec {
        name: yaml.name,
        version: yaml.version,
        description: yaml.description,
        node: Some(NodeConfig {
            id: yaml.node.id,
            listen_addr: yaml.node.listen_addr,
            cluster_seed_nodes: yaml.node.cluster_seed_nodes,
            cluster_name: String::new(), // Will be set from config if available
            max_connections: 100,
            heartbeat_interval_ms: 5000,
            clustering_enabled: true,
            grpc_connection_pool_size: 2,
            metadata: HashMap::new(),
            node_registry: None,
            grpc_address: String::new(),
            blob_http_port: yaml.node.blob_http_port,
        }),
        runtime: Some(RuntimeConfig {
            grpc: Some(convert_grpc_config(yaml.runtime.grpc)),
            health: Some(convert_health_config(yaml.runtime.health)),
            security: Some(convert_security_config(yaml.runtime.security)?),
            blob: yaml.runtime.blob.map(convert_blob_config),
            db: yaml.runtime.db.map(convert_shared_db_config),
            locks_provider: yaml.runtime.locks_provider.map(convert_storage_provider),
            channel_provider: parse_channel_provider(&yaml.runtime.channel_provider),
            mailbox_provider: parse_channel_provider(&yaml.runtime.mailbox_provider),
            framework_info: None,            // Set at runtime
            base_dir: yaml.runtime.base_dir, // Set by config_manager::initialize if empty
            wasm_apps_directory: yaml.runtime.wasm_apps_directory, // Set by config_manager::initialize if empty
            save_wasm_apps: false, // Default: disabled (only for testing)
            default_virtual_actor_config: None, // Defaults applied in code when None (5m, pool 100, lazy)
            service_links: yaml
                .runtime
                .service_links
                .into_iter()
                .map(convert_service_link_config)
                .collect::<Result<Vec<_>, _>>()?,
            default_outbound_client_policy: None,
            outbound_policy_templates: std::collections::HashMap::new(),
        }),
        system_applications: yaml.system_applications,
        applications: yaml
            .applications
            .into_iter()
            .map(convert_application_config)
            .collect(),
        env: yaml.env,
        shutdown: Some(convert_shutdown_config(yaml.shutdown)),
    })
}

/// Parse channel provider string to ChannelProvider enum value (as i32)
fn parse_channel_provider(provider: &Option<String>) -> i32 {
    let provider_str = match provider {
        Some(s) if !s.is_empty() => s.as_str(),
        _ => return ChannelProvider::ChannelProviderInMemory as i32,
    };

    match provider_str.to_uppercase().as_str() {
        "IN_MEMORY" | "MEMORY" => ChannelProvider::ChannelProviderInMemory as i32,
        "REDIS" => ChannelProvider::ChannelProviderRedis as i32,
        "KAFKA" => ChannelProvider::ChannelProviderKafka as i32,
        "NATS" => ChannelProvider::ChannelProviderNats as i32,
        "SQLITE" => ChannelProvider::ChannelProviderSqlite as i32,
        "POSTGRES" | "POSTGRESQL" => ChannelProvider::ChannelProviderPostgres as i32,
        "SQS" => ChannelProvider::ChannelProviderSqs as i32,
        "UDP" => ChannelProvider::ChannelProviderUdp as i32,
        "PROCESS_GROUP" => ChannelProvider::ChannelProviderProcessGroup as i32,
        _ => ChannelProvider::ChannelProviderInMemory as i32,
    }
}

fn convert_grpc_config(yaml: GrpcConfigYaml) -> GrpcConfig {
    GrpcConfig {
        enabled: yaml.enabled,
        address: yaml.address,
        max_connections: yaml.max_connections,
        keepalive_interval_seconds: yaml.keepalive_interval_seconds,
        middleware: yaml
            .middleware
            .into_iter()
            .map(|m| MiddlewareConfig {
                r#type: m.type_,
                enabled: m.enabled,
                config: m.config,
            })
            .collect(),
    }
}

fn convert_health_config(yaml: HealthConfigYaml) -> HealthConfig {
    HealthConfig {
        heartbeat_interval_seconds: yaml.heartbeat_interval_seconds,
        heartbeat_timeout_seconds: yaml.heartbeat_timeout_seconds,
        registry_url: yaml.registry_url,
    }
}

fn convert_service_link_config(yaml: ServiceLinkConfigYaml) -> Result<ServiceLinkConfig, String> {
    let transport = match yaml.transport.trim().to_uppercase().as_str() {
        "" | "HTTP" | "OUTBOUND_TRANSPORT_HTTP" => OutboundTransport::OutboundTransportHttp as i32,
        "GRPC" | "OUTBOUND_TRANSPORT_GRPC" => OutboundTransport::OutboundTransportGrpc as i32,
        "CHANNEL" | "OUTBOUND_TRANSPORT_CHANNEL" => {
            OutboundTransport::OutboundTransportChannel as i32
        }
        "UNSPECIFIED" | "OUTBOUND_TRANSPORT_UNSPECIFIED" => {
            OutboundTransport::OutboundTransportUnspecified as i32
        }
        other => {
            return Err(format!(
                "runtime.service_links: unknown transport {other:?} (use HTTP, GRPC, or CHANNEL)"
            ));
        }
    };

    Ok(ServiceLinkConfig {
        name: yaml.name,
        transport,
        base_url: yaml.base_url,
        publish_to_registry: yaml.publish_to_registry,
        default_headers: yaml.default_headers,
        api_key_header_name: yaml.api_key_header_name,
        api_key_env_var: yaml.api_key_env_var,
        bearer_token_env_var: yaml.bearer_token_env_var,
        policy_template: yaml.policy_template,
    })
}

fn convert_security_config(yaml: SecurityConfigYaml) -> Result<SecurityConfig, String> {
    // JWT config can be in either jwt or authn_config.jwt_config
    // Prefer jwt if present, otherwise use authn_config.jwt_config
    let jwt = if yaml.jwt.is_some() {
        yaml.jwt.map(convert_jwt_config)
    } else if let Some(ref authn) = yaml.authn_config {
        authn.jwt_config.as_ref().map(|jwt_yaml| {
            // Convert JwtConfigYaml to JwtConfig
            convert_jwt_config(JwtConfigYaml {
                enable_jwt: jwt_yaml.enable_jwt,
                secret: jwt_yaml.secret.clone(),
                issuer: jwt_yaml.issuer.clone(),
                jwks_url: jwt_yaml.jwks_url.clone(),
                allowed_audiences: jwt_yaml.allowed_audiences.clone(),
                disable_auth_for_testing: jwt_yaml.disable_auth_for_testing,
                algorithm: String::new(),
                private_key_pem: String::new(),
                private_key_file: String::new(),
                auto_generate_key: true,
            })
        })
    } else {
        None
    };

    // mTLS config can be in either mtls or authn_config.mtls_config
    // Prefer mtls if present, otherwise use authn_config.mtls_config
    let mtls = if yaml.mtls.is_some() {
        yaml.mtls.map(convert_mtls_config)
    } else if let Some(ref authn) = yaml.authn_config {
        authn.mtls_config.as_ref().map(|mtls_yaml| {
            // Convert MtlsConfigYaml to MtlsConfig
            convert_mtls_config(MtlsConfigYaml {
                enable_mtls: mtls_yaml.enable_mtls,
                ca_certificate: mtls_yaml.ca_certificate.clone(),
                client_certificate: mtls_yaml.client_certificate.clone(),
                client_private_key: mtls_yaml.client_private_key.clone(),
                auto_generate_certs: mtls_yaml.auto_generate_certs,
                cert_dir: mtls_yaml.cert_dir.clone(),
                disable_auth_for_testing: mtls_yaml.disable_auth_for_testing,
            })
        })
    } else {
        None
    };

    let oidc = yaml.oidc.map(|o| {
        use plexspaces_proto::security::v1::OidcConfig;
        let client_id = if o.client_id.is_empty() {
            std::env::var("PLEXSPACES_OIDC_CLIENT_ID").unwrap_or_default()
        } else {
            o.client_id
        };
        let client_secret = if o.client_secret.is_empty() {
            std::env::var("PLEXSPACES_OIDC_CLIENT_SECRET").unwrap_or_default()
        } else {
            o.client_secret
        };
        let discovery_url = if o.discovery_url.is_empty() {
            std::env::var("PLEXSPACES_OIDC_DISCOVERY_URL").unwrap_or_default()
        } else {
            o.discovery_url
        };
        let redirect_uri = if o.redirect_uri.is_empty() {
            std::env::var("PLEXSPACES_OIDC_REDIRECT_URI")
                .unwrap_or_else(|_| "/api/v1/auth/oidc/callback".to_string())
        } else {
            o.redirect_uri
        };
        OidcConfig {
            enabled: o.enabled,
            discovery_url,
            client_id,
            client_secret,
            redirect_uri,
            scopes: o.scopes,
            tenant_claim: o.tenant_claim,
            admin_groups: o.admin_groups,
            default_tenant_id: o.default_tenant_id,
        }
    });

    Ok(SecurityConfig {
        service_identity: yaml.service_identity.map(convert_service_identity),
        mtls,
        jwt,
        api_keys: yaml.api_keys.into_iter().map(convert_api_key).collect(),
        disable_auth: yaml.disable_auth,
        oidc,
    })
}

fn convert_service_identity(yaml: ServiceIdentityYaml) -> ServiceIdentity {
    ServiceIdentity {
        service_id: yaml.service_id,
        certificate: yaml.certificate.into_bytes(),
        private_key: yaml.private_key.into_bytes(),
        expires_at: None,
        allowed_services: vec![],
    }
}

fn convert_mtls_config(yaml: MtlsConfigYaml) -> MtlsConfig {
    // mTLS certificate paths can come from environment variables
    // Priority: 1. Env var, 2. Config value
    let ca_certificate_path = std::env::var("PLEXSPACES_MTLS_CA_CERT")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| yaml.ca_certificate.clone());

    let server_certificate_path = std::env::var("PLEXSPACES_MTLS_SERVER_CERT")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| yaml.client_certificate.clone());

    let server_key_path = std::env::var("PLEXSPACES_MTLS_SERVER_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| yaml.client_private_key.clone());

    MtlsConfig {
        enable_mtls: yaml.enable_mtls,
        ca_certificate_path,
        server_certificate_path,
        server_key_path,
        auto_generate: yaml.auto_generate_certs,
        // Resolve cert_dir: Priority: 1. Env var, 2. YAML config, 3. Default
        cert_dir: std::env::var("PLEXSPACES_MTLS_CERT_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if !yaml.cert_dir.is_empty() {
                    yaml.cert_dir.clone()
                } else {
                    "/app/certs".to_string()
                }
            }),
        certificate_rotation_interval: None,
        trusted_services: vec![],
    }
}

fn convert_jwt_config(yaml: JwtConfigYaml) -> JwtConfig {
    // JWT secret should come from environment variable (PLEXSPACES_JWT_SECRET)
    // Priority: 1. Env var, 2. Config value (for backward compatibility, but not recommended)
    let secret = std::env::var("PLEXSPACES_JWT_SECRET")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if !yaml.secret.is_empty() {
                tracing::warn!(
                    "JWT secret found in config file. For production, use PLEXSPACES_JWT_SECRET env var instead."
                );
                yaml.secret.clone()
            } else {
                String::new()
            }
        });

    // ES256 private key: env var takes priority over config
    let private_key_pem = std::env::var("PLEXSPACES_JWT_PRIVATE_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| yaml.private_key_pem.clone());

    let private_key_file = std::env::var("PLEXSPACES_JWT_PRIVATE_KEY_FILE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| yaml.private_key_file.clone());

    let algorithm = if !yaml.algorithm.is_empty() {
        yaml.algorithm.clone()
    } else if !private_key_pem.is_empty() || !private_key_file.is_empty() {
        "ES256".to_string()
    } else if !secret.is_empty() {
        "HS256".to_string()
    } else {
        "ES256".to_string() // Default to ES256 (auto-generate)
    };

    JwtConfig {
        enable_jwt: yaml.enable_jwt,
        secret,
        issuer: yaml.issuer,
        jwks_url: yaml.jwks_url,
        allowed_audiences: yaml.allowed_audiences,
        token_ttl: None,
        refresh_token_ttl: None,
        tenant_id_claim: "tenant_id".to_string(),
        user_id_claim: "sub".to_string(),
        algorithm,
        private_key_pem,
        private_key_file,
        auto_generate_key: yaml.auto_generate_key,
    }
}

fn convert_api_key(yaml: ApiKeyYaml) -> ApiKey {
    let key_id = yaml.key_id.clone();
    ApiKey {
        id: yaml.key_id,
        name: key_id, // Use key_id as name if not provided
        description: String::new(),
        key_hash: yaml.key, // Store key as hash (should be hashed in production)
        scopes: yaml.allowed_services,
        expires_at: None,
        last_used: None,
        metadata: None,
    }
}

// AuthnConfig and AuthzConfig are not in the proto yet - skip for now

fn convert_application_config(yaml: ApplicationSpecYaml) -> ApplicationSpec {
    use plexspaces_proto::application::v1::ShutdownStrategy;
    // Parse shutdown_strategy string to enum
    let shutdown_strategy = match yaml.shutdown_strategy.to_uppercase().as_str() {
        "GRACEFUL" => ShutdownStrategy::ShutdownStrategyGraceful as i32,
        "BRUTAL_KILL" | "IMMEDIATE" => ShutdownStrategy::ShutdownStrategyImmediate as i32,
        _ => ShutdownStrategy::ShutdownStrategyGraceful as i32, // Default
    };

    // Convert shutdown timeout from seconds to Duration
    let shutdown_timeout = Some(prost_types::Duration {
        seconds: yaml.shutdown_timeout_seconds as i64,
        nanos: 0,
    });

    ApplicationSpec {
        name: yaml.name.clone(),
        tenant_id: String::new(),
        version: yaml.version,
        description: yaml.description,
        r#type: 0, // Default type
        dependencies: yaml.dependencies,
        env: std::collections::HashMap::new(),
        supervisor: None,
        enabled: yaml.enabled,
        auto_start: yaml.auto_start,
        shutdown_timeout,
        shutdown_strategy,
        metadata: None,
        seed_nodes: vec![],
        required_service_links: yaml
            .required_service_links
            .into_iter()
            .map(|link| ApplicationServiceLinkRequirement {
                link_name: link.link_name,
                policy_template: link.policy_template,
            })
            .collect(),
    }
}

fn convert_shutdown_config(yaml: ShutdownConfigYaml) -> ShutdownConfig {
    ShutdownConfig {
        global_timeout_seconds: yaml.global_timeout_seconds,
        grace_period_seconds: yaml.grace_period_seconds,
        grpc_drain_timeout_seconds: yaml.grpc_drain_timeout_seconds,
    }
}

fn convert_blob_config(yaml: BlobConfigYaml) -> BlobConfig {
    BlobConfig {
        backend: yaml.backend,
        bucket: yaml.bucket,
        endpoint: yaml.endpoint,
        region: yaml.region,
        access_key_id: yaml.access_key_id,
        secret_access_key: yaml.secret_access_key,
        use_ssl: yaml.use_ssl,
        prefix: yaml.prefix,
        gcp_service_account_json: String::new(), // Not in YAML, set via env var
        azure_account_name: String::new(),       // Not in YAML, set via env var
        azure_account_key: String::new(),        // Not in YAML, set via env var
    }
}

fn convert_shared_db_config(yaml: SharedRelationalDbConfigYaml) -> SharedDbConfig {
    SharedDbConfig {
        connection_string: yaml.connection_string,
        pool_size: yaml.pool_size,
        auto_migrate: yaml.auto_migrate,
        migration_paths: yaml.migration_paths,
    }
}

#[allow(clippy::manual_map)]
fn convert_storage_provider(yaml: StorageProviderConfigYaml) -> StorageProviderConfig {
    let provider = parse_storage_provider_type(&yaml.provider_type);

    StorageProviderConfig {
        provider: provider as i32,
        config: if let Some(redis) = yaml.redis {
            Some(
                plexspaces_proto::storage::v1::storage_provider_config::Config::Redis(
                    convert_redis_config(redis),
                ),
            )
        } else if let Some(postgres) = yaml.postgres {
            Some(
                plexspaces_proto::storage::v1::storage_provider_config::Config::Postgres(
                    convert_shared_db_config(postgres),
                ),
            )
        } else if let Some(sqlite) = yaml.sqlite {
            Some(
                plexspaces_proto::storage::v1::storage_provider_config::Config::Sqlite(
                    convert_sqlite_config(sqlite),
                ),
            )
        } else if let Some(dynamodb) = yaml.dynamodb {
            Some(
                plexspaces_proto::storage::v1::storage_provider_config::Config::Dynamodb(
                    convert_dynamodb_config(dynamodb),
                ),
            )
        } else {
            None
        },
    }
}

fn parse_storage_provider_type(s: &str) -> StorageProvider {
    match s.to_uppercase().as_str() {
        "STORAGE_PROVIDER_SQLITE" | "SQLITE" => StorageProvider::StorageProviderSqlite,
        "STORAGE_PROVIDER_POSTGRES" | "POSTGRES" => StorageProvider::StorageProviderPostgres,
        "STORAGE_PROVIDER_REDIS" | "REDIS" => StorageProvider::StorageProviderRedis,
        "STORAGE_PROVIDER_DYNAMODB" | "DYNAMODB" => StorageProvider::StorageProviderDynamodb,
        _ => StorageProvider::StorageProviderUnspecified,
    }
}

fn convert_redis_config(yaml: RedisBackendConfigYaml) -> RedisBackendConfig {
    RedisBackendConfig {
        url: yaml.url,
        pool_size: yaml.pool_size,
        key_prefix: yaml.key_prefix,
        connect_timeout: None, // TODO: Parse duration
        cluster_mode: yaml.cluster_mode,
    }
}

fn convert_sqlite_config(yaml: SqliteBackendConfigYaml) -> SqliteBackendConfig {
    SqliteBackendConfig {
        database_path: yaml.database_path,
        wal_mode: yaml.wal_mode,
        synchronous: "NORMAL".to_string(), // Default synchronous mode
    }
}

fn convert_dynamodb_config(
    yaml: DynamoDbBackendConfigYaml,
) -> plexspaces_proto::storage::v1::DynamoDbBackendConfig {
    plexspaces_proto::storage::v1::DynamoDbBackendConfig {
        region: yaml.region,
        table_prefix: yaml.table_prefix,
        endpoint_url: yaml.endpoint_url,
        access_key_id: yaml.access_key_id,
        secret_access_key: yaml.secret_access_key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_yaml_to_proto_preserves_service_links_and_requirements() {
        let release_yaml = ReleaseYaml {
            name: "my-cluster".to_string(),
            version: "1.0.0".to_string(),
            description: "test".to_string(),
            node: NodeConfigYaml {
                id: "node-1".to_string(),
                listen_addr: "0.0.0.0:8091".to_string(),
                cluster_seed_nodes: vec![],
                blob_http_port: 0,
            },
            runtime: RuntimeConfigYaml {
                grpc: GrpcConfigYaml::default(),
                health: HealthConfigYaml::default(),
                security: SecurityConfigYaml::default(),
                blob: None,
                db: None,
                locks_provider: None,
                channel_provider: None,
                mailbox_provider: None,
                base_dir: String::new(),
                wasm_apps_directory: String::new(),
                service_links: vec![ServiceLinkConfigYaml {
                    name: "weather-api".to_string(),
                    transport: "HTTP".to_string(),
                    base_url: "https://api.open-meteo.com".to_string(),
                    publish_to_registry: true,
                    default_headers: HashMap::new(),
                    api_key_header_name: None,
                    api_key_env_var: None,
                    bearer_token_env_var: None,
                    policy_template: None,
                }],
            },
            system_applications: vec![],
            applications: vec![ApplicationSpecYaml {
                name: "weather".to_string(),
                version: "1.0.0".to_string(),
                description: "weather app".to_string(),
                config_path: "app-config.toml".to_string(),
                enabled: true,
                auto_start: true,
                shutdown_timeout_seconds: 30,
                shutdown_strategy: "graceful".to_string(),
                dependencies: vec![],
                required_service_links: vec![ApplicationServiceLinkRequirementYaml {
                    link_name: "weather-api".to_string(),
                    policy_template: None,
                }],
            }],
            env: HashMap::new(),
            shutdown: ShutdownConfigYaml::default(),
        };

        let release = convert_yaml_to_proto(release_yaml).expect("convert release");
        let runtime = release.runtime.expect("runtime");

        assert_eq!(runtime.service_links.len(), 1);
        assert_eq!(runtime.service_links[0].name, "weather-api");
        assert_eq!(
            runtime.service_links[0].transport,
            OutboundTransport::OutboundTransportHttp as i32
        );
        assert_eq!(
            runtime.service_links[0].base_url,
            "https://api.open-meteo.com"
        );
        assert!(runtime.service_links[0].publish_to_registry);

        assert_eq!(release.applications.len(), 1);
        assert_eq!(release.applications[0].required_service_links.len(), 1);
        assert_eq!(
            release.applications[0].required_service_links[0].link_name,
            "weather-api"
        );
    }

    #[test]
    fn convert_yaml_to_proto_rejects_unknown_service_link_transport() {
        let release_yaml = ReleaseYaml {
            name: "my-cluster".to_string(),
            version: "1.0.0".to_string(),
            description: "test".to_string(),
            node: NodeConfigYaml {
                id: "node-1".to_string(),
                listen_addr: "0.0.0.0:8091".to_string(),
                cluster_seed_nodes: vec![],
                blob_http_port: 0,
            },
            runtime: RuntimeConfigYaml {
                grpc: GrpcConfigYaml::default(),
                health: HealthConfigYaml::default(),
                security: SecurityConfigYaml::default(),
                blob: None,
                db: None,
                locks_provider: None,
                channel_provider: None,
                mailbox_provider: None,
                base_dir: String::new(),
                wasm_apps_directory: String::new(),
                service_links: vec![ServiceLinkConfigYaml {
                    name: "weather-api".to_string(),
                    transport: "SMTP".to_string(),
                    base_url: "https://api.open-meteo.com".to_string(),
                    publish_to_registry: false,
                    default_headers: HashMap::new(),
                    api_key_header_name: None,
                    api_key_env_var: None,
                    bearer_token_env_var: None,
                    policy_template: None,
                }],
            },
            system_applications: vec![],
            applications: vec![],
            env: HashMap::new(),
            shutdown: ShutdownConfigYaml::default(),
        };

        let error = convert_yaml_to_proto(release_yaml).expect_err("invalid transport");
        assert!(error.contains("unknown transport"));
    }
}
