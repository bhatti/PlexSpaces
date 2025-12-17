// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Intermediate YAML structures for parsing ReleaseSpec from YAML

use serde::Deserialize;
use std::collections::HashMap;

/// Intermediate YAML representation for ReleaseSpec
#[derive(Debug, Deserialize)]
pub struct ReleaseYaml {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub node: NodeConfigYaml,
    pub runtime: RuntimeConfigYaml,
    #[serde(default)]
    pub system_applications: Vec<String>,
    #[serde(default)]
    pub applications: Vec<ApplicationConfigYaml>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub shutdown: ShutdownConfigYaml,
}

#[derive(Debug, Deserialize)]
pub struct NodeConfigYaml {
    pub id: String,
    pub listen_address: String,
    #[serde(default)]
    pub cluster_seed_nodes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeConfigYaml {
    #[serde(default)]
    pub grpc: GrpcConfigYaml,
    #[serde(default)]
    pub health: HealthConfigYaml,
    #[serde(default)]
    pub security: SecurityConfigYaml,
    #[serde(default)]
    pub blob: Option<BlobConfigYaml>,
    #[serde(default)]
    pub shared_database: Option<SharedRelationalDbConfigYaml>,
    #[serde(default)]
    pub locks_provider: Option<StorageProviderConfigYaml>,
    #[serde(default)]
    pub channel_provider: Option<StorageProviderConfigYaml>,
    #[serde(default)]
    pub tuplespace_provider: Option<StorageProviderConfigYaml>,
    #[serde(default)]
    pub keyvalue_provider: Option<StorageProviderConfigYaml>,
    #[serde(default)]
    pub mailbox_provider: Option<StorageProviderConfigYaml>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GrpcConfigYaml {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_grpc_address")]
    pub address: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_keepalive_interval")]
    pub keepalive_interval_seconds: u64,
    #[serde(default)]
    pub middleware: Vec<MiddlewareConfigYaml>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MiddlewareConfigYaml {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub config: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct HealthConfigYaml {
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_seconds: u64,
    #[serde(default = "default_heartbeat_timeout")]
    pub heartbeat_timeout_seconds: u64,
    #[serde(default)]
    pub registry_url: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct SecurityConfigYaml {
    #[serde(default)]
    pub service_identity: Option<ServiceIdentityYaml>,
    #[serde(default)]
    pub mtls: Option<MtlsConfigYaml>,
    #[serde(default)]
    pub jwt: Option<JwtConfigYaml>,
    #[serde(default)]
    pub api_keys: Vec<ApiKeyYaml>,
    #[serde(default)]
    pub authn_config: Option<AuthnConfigYaml>,
    #[serde(default)]
    pub authz_config: Option<AuthzConfigYaml>,
}

#[derive(Debug, Deserialize)]
pub struct ServiceIdentityYaml {
    pub service_id: String,
    pub certificate: String,
    pub private_key: String,
    pub public_key: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MtlsConfigYaml {
    #[serde(default = "default_true")]
    pub enable_mtls: bool,
    #[serde(default)]
    pub ca_certificate: String,
    #[serde(default)]
    pub client_certificate: String,
    #[serde(default)]
    pub client_private_key: String,
    #[serde(default)]
    pub auto_generate_certs: bool,
    #[serde(default)]
    pub disable_auth_for_testing: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct JwtConfigYaml {
    #[serde(default = "default_true")]
    pub enable_jwt: bool,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub issuer: String,
    #[serde(default)]
    pub jwks_url: String,
    #[serde(default)]
    pub allowed_audiences: Vec<String>,
    #[serde(default)]
    pub disable_auth_for_testing: bool,
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyYaml {
    pub key_id: String,
    pub key: String,
    pub allowed_services: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AuthnConfigYaml {
    #[serde(default)]
    pub jwt_config: Option<JwtConfigYaml>,
    #[serde(default)]
    pub mtls_config: Option<MtlsConfigYaml>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AuthzConfigYaml {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub policy_file_path: String,
}

#[derive(Debug, Deserialize)]
pub struct ApplicationConfigYaml {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub config_path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub auto_start: bool,
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_seconds: u64,
    #[serde(default)]
    pub shutdown_strategy: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ShutdownConfigYaml {
    #[serde(default = "default_global_timeout")]
    pub global_timeout_seconds: u64,
    #[serde(default = "default_grace_period")]
    pub grace_period_seconds: u64,
    #[serde(default = "default_grpc_drain_timeout")]
    pub grpc_drain_timeout_seconds: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct BlobConfigYaml {
    #[serde(default = "default_blob_backend")]
    pub backend: String,
    #[serde(default = "default_blob_bucket")]
    pub bucket: String,
    #[serde(default = "default_blob_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub secret_access_key: String,
    #[serde(default)]
    pub use_ssl: bool,
    #[serde(default)]
    pub prefix: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct SharedRelationalDbConfigYaml {
    pub connection_string: String,
    #[serde(default = "default_pool_size")]
    pub pool_size: u32,
    #[serde(default = "default_true")]
    pub auto_migrate: bool,
    #[serde(default)]
    pub migration_paths: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct StorageProviderConfigYaml {
    #[serde(default)]
    pub provider_type: String,
    #[serde(default)]
    pub default_provider_type: String,
    #[serde(default)]
    pub in_memory: Option<InMemoryBackendConfigYaml>,
    #[serde(default)]
    pub sqlite: Option<SqliteBackendConfigYaml>,
    #[serde(default)]
    pub postgres: Option<SharedRelationalDbConfigYaml>,
    #[serde(default)]
    pub redis: Option<RedisBackendConfigYaml>,
    #[serde(default)]
    pub kafka: Option<KafkaBackendConfigYaml>,
    #[serde(default)]
    pub nats: Option<NatsBackendConfigYaml>,
    #[serde(default)]
    pub dynamodb: Option<DynamoDbBackendConfigYaml>,
}

#[derive(Debug, Deserialize, Default)]
pub struct InMemoryBackendConfigYaml {
    #[serde(default = "default_capacity")]
    pub capacity: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct SqliteBackendConfigYaml {
    pub database_path: String,
    #[serde(default = "default_true")]
    pub wal_mode: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct RedisBackendConfigYaml {
    pub url: String,
    #[serde(default = "default_pool_size")]
    pub pool_size: u32,
    #[serde(default = "default_key_prefix")]
    pub key_prefix: String,
    #[serde(default)]
    pub connect_timeout: Option<String>,
    #[serde(default)]
    pub cluster_mode: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct KafkaBackendConfigYaml {
    #[serde(default)]
    pub brokers: Vec<String>,
    #[serde(default)]
    pub topic_prefix: String,
    #[serde(default)]
    pub consumer_group_prefix: String,
    #[serde(default)]
    pub partitions: u32,
    #[serde(default)]
    pub replication_factor: u32,
}

#[derive(Debug, Deserialize, Default)]
pub struct NatsBackendConfigYaml {
    #[serde(default)]
    pub servers: String,
    #[serde(default)]
    pub subject_prefix: String,
    #[serde(default)]
    pub queue_group_prefix: String,
    #[serde(default)]
    pub jetstream_enabled: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct DynamoDbBackendConfigYaml {
    pub region: String,
    #[serde(default)]
    pub table_prefix: String,
    #[serde(default)]
    pub endpoint_url: String,
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub secret_access_key: String,
}

// Default value functions
fn default_true() -> bool {
    true
}

fn default_grpc_address() -> String {
    "0.0.0.0:9001".to_string()
}

fn default_max_connections() -> u32 {
    100
}

fn default_keepalive_interval() -> u64 {
    30
}

fn default_heartbeat_interval() -> u64 {
    10
}

fn default_heartbeat_timeout() -> u64 {
    5
}

fn default_global_timeout() -> u64 {
    30
}

fn default_grace_period() -> u64 {
    5
}

fn default_grpc_drain_timeout() -> u64 {
    10
}

fn default_shutdown_timeout() -> u64 {
    30
}

fn default_pool_size() -> u32 {
    10
}

fn default_capacity() -> u64 {
    1000
}

fn default_key_prefix() -> String {
    "plexspaces:".to_string()
}

fn default_blob_backend() -> String {
    "minio".to_string()
}

fn default_blob_bucket() -> String {
    "plexspaces".to_string()
}

fn default_blob_endpoint() -> String {
    "http://minio:9000".to_string()
}
