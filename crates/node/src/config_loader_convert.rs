// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Conversion from YAML intermediate representation to proto ReleaseSpec

use super::config_loader_yaml::{*, JwtConfigYaml, MtlsConfigYaml};
use plexspaces_proto::node::v1::{
    ApplicationConfig, GrpcConfig, HealthConfig, MiddlewareConfig, NodeConfig, ReleaseSpec,
    RuntimeConfig, SecurityConfig, ShutdownConfig,
};
use plexspaces_proto::security::v1::{ApiKey, JwtConfig, MtlsConfig, ServiceIdentity};
use plexspaces_proto::storage::v1::{
    BlobConfig, KafkaBackendConfig, MemoryBackendConfig, NatsBackendConfig,
    RedisBackendConfig, SharedRelationalDbConfig, SqliteBackendConfig, StorageProvider, StorageProviderConfig,
};

/// Convert YAML representation to proto ReleaseSpec
pub fn convert_yaml_to_proto(yaml: ReleaseYaml) -> Result<ReleaseSpec, String> {
    Ok(ReleaseSpec {
        name: yaml.name,
        version: yaml.version,
        description: yaml.description,
        node: Some(NodeConfig {
            id: yaml.node.id,
            listen_address: yaml.node.listen_address,
            cluster_seed_nodes: yaml.node.cluster_seed_nodes,
            default_tenant_id: "internal".to_string(), // Default for local development
            default_namespace: "system".to_string(), // Default for local development
            cluster_name: String::new(), // Will be set from config if available
        }),
        runtime: Some({
            let runtime_config = RuntimeConfig {
                grpc: Some(convert_grpc_config(yaml.runtime.grpc)),
                health: Some(convert_health_config(yaml.runtime.health)),
                security: Some(convert_security_config(yaml.runtime.security)?),
                blob: yaml.runtime.blob.map(convert_blob_config),
                shared_database: yaml.runtime.shared_database.map(convert_shared_db_config),
                locks_provider: yaml.runtime.locks_provider.map(convert_storage_provider),
                channel_provider: yaml.runtime.channel_provider.map(convert_storage_provider),
                tuplespace_provider: yaml.runtime.tuplespace_provider.map(convert_storage_provider),
                framework_info: None, // Set at runtime
                ..Default::default() // Include mailbox_provider default until proto is regenerated
            };
            // Note: mailbox_provider field will be available after running `buf generate`
            // For now, Default::default() will set it to None
            runtime_config
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
                disable_auth_for_testing: mtls_yaml.disable_auth_for_testing,
            })
        })
    } else {
        None
    };
    
    Ok(SecurityConfig {
        service_identity: yaml.service_identity.map(convert_service_identity),
        mtls,
        jwt,
        api_keys: yaml
            .api_keys
            .into_iter()
            .map(convert_api_key)
            .collect(),
        allow_disable_auth: false,
        disable_auth: false,
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
    MtlsConfig {
        enable_mtls: yaml.enable_mtls,
        ca_certificate_path: yaml.ca_certificate,
        server_certificate_path: yaml.client_certificate,
        server_key_path: yaml.client_private_key,
        auto_generate: yaml.auto_generate_certs,
        cert_dir: "/app/certs".to_string(),
        certificate_rotation_interval: None,
        trusted_services: vec![],
    }
}

fn convert_jwt_config(yaml: JwtConfigYaml) -> JwtConfig {
    JwtConfig {
        enable_jwt: yaml.enable_jwt,
        secret: yaml.secret,
        issuer: yaml.issuer,
        jwks_url: yaml.jwks_url,
        allowed_audiences: yaml.allowed_audiences,
        token_ttl: None,
        refresh_token_ttl: None,
        tenant_id_claim: "tenant_id".to_string(),  // Default claim name
        user_id_claim: "sub".to_string(),  // Default claim name
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

fn convert_application_config(yaml: ApplicationConfigYaml) -> ApplicationConfig {
    use plexspaces_proto::node::v1::ShutdownStrategy;
    // Parse shutdown_strategy string to enum
    let shutdown_strategy = match yaml.shutdown_strategy.to_uppercase().as_str() {
        "GRACEFUL" => ShutdownStrategy::ShutdownStrategyGraceful as i32,
        "BRUTAL_KILL" | "IMMEDIATE" => ShutdownStrategy::ShutdownStrategyBrutalKill as i32,
        "INFINITY" => ShutdownStrategy::ShutdownStrategyInfinity as i32,
        _ => ShutdownStrategy::ShutdownStrategyGraceful as i32,  // Default
    };
    
    ApplicationConfig {
        name: yaml.name,
        version: yaml.version,
        config_path: yaml.config_path,
        enabled: yaml.enabled,
        auto_start: yaml.auto_start,
        shutdown_timeout_seconds: yaml.shutdown_timeout_seconds as i64,
        shutdown_strategy,
        dependencies: yaml.dependencies,
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
        gcp_service_account_json: String::new(),  // Not in YAML, set via env var
        azure_account_name: String::new(),  // Not in YAML, set via env var
        azure_account_key: String::new(),  // Not in YAML, set via env var
    }
}

fn convert_shared_db_config(yaml: SharedRelationalDbConfigYaml) -> SharedRelationalDbConfig {
    SharedRelationalDbConfig {
        connection_string: yaml.connection_string,
        pool_size: yaml.pool_size,
        auto_migrate: yaml.auto_migrate,
        migration_paths: yaml.migration_paths,
    }
}

fn convert_storage_provider(yaml: StorageProviderConfigYaml) -> StorageProviderConfig {
    let provider = parse_storage_provider_type(&yaml.provider_type);

    StorageProviderConfig {
        provider: provider as i32,
        config: if let Some(redis) = yaml.redis {
            Some(plexspaces_proto::storage::v1::storage_provider_config::Config::Redis(
                convert_redis_config(redis),
            ))
        } else if let Some(postgres) = yaml.postgres {
            Some(plexspaces_proto::storage::v1::storage_provider_config::Config::Postgres(
                convert_shared_db_config(postgres),
            ))
        } else if let Some(sqlite) = yaml.sqlite {
            Some(plexspaces_proto::storage::v1::storage_provider_config::Config::Sqlite(
                convert_sqlite_config(sqlite),
            ))
        } else if let Some(kafka) = yaml.kafka {
            Some(plexspaces_proto::storage::v1::storage_provider_config::Config::Kafka(
                convert_kafka_config(kafka),
            ))
        } else if let Some(nats) = yaml.nats {
            Some(plexspaces_proto::storage::v1::storage_provider_config::Config::Nats(
                convert_nats_config(nats),
            ))
        } else if let Some(dynamodb) = yaml.dynamodb {
            Some(plexspaces_proto::storage::v1::storage_provider_config::Config::Dynamodb(
                convert_dynamodb_config(dynamodb),
            ))
        } else if let Some(in_memory) = yaml.in_memory {
            Some(plexspaces_proto::storage::v1::storage_provider_config::Config::Memory(
                convert_in_memory_config(in_memory),
            ))
        } else {
            None
        },
    }
}

fn parse_storage_provider_type(s: &str) -> StorageProvider {
    match s.to_uppercase().as_str() {
        "STORAGE_PROVIDER_MEMORY" | "MEMORY" => StorageProvider::StorageProviderMemory,
        "STORAGE_PROVIDER_SQLITE" | "SQLITE" => StorageProvider::StorageProviderSqlite,
        "STORAGE_PROVIDER_POSTGRES" | "POSTGRES" => StorageProvider::StorageProviderPostgres,
        "STORAGE_PROVIDER_REDIS" | "REDIS" => StorageProvider::StorageProviderRedis,
        "STORAGE_PROVIDER_KAFKA" | "KAFKA" => StorageProvider::StorageProviderKafka,
        "STORAGE_PROVIDER_NATS" | "NATS" => StorageProvider::StorageProviderNats,
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
        synchronous: "NORMAL".to_string(),  // Default synchronous mode
    }
}

fn convert_kafka_config(yaml: KafkaBackendConfigYaml) -> KafkaBackendConfig {
    use plexspaces_proto::storage::v1::kafka_backend_config::{CompressionType, ProducerAcks};
    KafkaBackendConfig {
        brokers: yaml.brokers,
        topic: yaml.topic_prefix,  // Use topic_prefix as topic
        consumer_group: yaml.consumer_group_prefix,  // Use consumer_group_prefix as consumer_group
        partitions: yaml.partitions,
        replication_factor: yaml.replication_factor,
        compression: CompressionType::CompressionTypeNone as i32,  // Default: no compression
        acks: ProducerAcks::ProducerAcksAll as i32,  // Default: wait for all replicas
        batch_size: 16384,  // Default: 16KB batch size
        linger_ms: None,  // Default: no batching delay
    }
}

fn convert_nats_config(yaml: NatsBackendConfigYaml) -> NatsBackendConfig {
    let subject = yaml.subject_prefix.clone();
    let queue_group = yaml.queue_group_prefix.clone();
    NatsBackendConfig {
        servers: yaml.servers,
        subject: subject.clone(),  // Use subject_prefix as subject
        queue_group: queue_group.clone(),  // Use queue_group_prefix as queue_group
        jetstream_enabled: yaml.jetstream_enabled,
        jetstream_stream: format!("plexspaces-{}", subject),  // Default stream name
        jetstream_consumer: format!("plexspaces-consumer-{}", queue_group),  // Default consumer name
        connect_timeout: None,  // Default: no timeout
        reconnect_attempts: 10,  // Default: 10 attempts
        tls_enabled: false,  // Default: TLS disabled
        tls_cert_path: String::new(),  // Default: no cert path
        tls_key_path: String::new(),  // Default: no key path
        tls_ca_path: String::new(),  // Default: no CA path
    }
}

fn convert_dynamodb_config(yaml: DynamoDbBackendConfigYaml) -> plexspaces_proto::storage::v1::DynamoDbBackendConfig {
    plexspaces_proto::storage::v1::DynamoDbBackendConfig {
        region: yaml.region,
        table_prefix: yaml.table_prefix,
        endpoint_url: yaml.endpoint_url,
        access_key_id: yaml.access_key_id,
        secret_access_key: yaml.secret_access_key,
    }
}

fn convert_in_memory_config(yaml: InMemoryBackendConfigYaml) -> MemoryBackendConfig {
    MemoryBackendConfig {
        initial_capacity: yaml.capacity as u32,
    }
}

