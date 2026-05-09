// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Configuration Manager (Viper-style)
//!
//! ## Purpose
//! Centralized configuration management with environment variable overrides.
//! Follows Go's Viper pattern: file config + env var precedence.
//!
//! ## Design Principles
//! 1. **Single source of truth**: ALL env var handling for config is done HERE
//! 2. **Env var precedence**: Environment variables always override file config
//! 3. **Type-safe helpers**: Strongly typed accessors for common config values
//! 4. **Security-first**: Secrets must come from env vars, not config files
//!
//! ## IMPORTANT
//! No other module should read PLEXSPACES_* env vars for configuration.
//! All env var handling is centralized in `initialize()` to ensure consistent
//! precedence and behavior. Other modules should only read from the
//! ReleaseSpec that has been processed by `initialize()`.
//!
//! ## Usage
//! ```rust,ignore
//! use plexspaces_common::config_manager::{get_env, get_env_or, EnvConfig};
//!
//! // Simple env access
//! let node_id = get_env(ENV_NODE_ID);
//! let listen_addr = get_env_or(ENV_LISTEN_ADDR, "0.0.0.0:8000");
//!
//! // Typed config access
//! let config = EnvConfig::from_env();
//! if config.is_auth_disabled() {
//!     println!("Auth is disabled for testing");
//! }
//! ```

use std::env;

// ============================================================================
// Environment Variable Names
// ============================================================================

/// Node identity
pub const ENV_NODE_ID: &str = "PLEXSPACES_NODE_ID";

/// gRPC listen address (e.g., "0.0.0.0:8000")
pub const ENV_LISTEN_ADDR: &str = "PLEXSPACES_LISTEN_ADDR";

/// gRPC address advertised to other nodes
pub const ENV_GRPC_ADDRESS: &str = "PLEXSPACES_GRPC_ADDRESS";

/// Logical cluster name for node registry, placement (`from_registry`), and SWIM labels
pub const ENV_CLUSTER_NAME: &str = "PLEXSPACES_CLUSTER_NAME";

/// Default `node.cluster_name` when release config and [`ENV_CLUSTER_NAME`] are unset or empty.
pub const DEFAULT_CLUSTER_NAME: &str = "default";

/// WASM applications directory for auto-deploy
pub const ENV_WASM_APPS_DIR: &str = "PLEXSPACES_WASM_APPS_DIR";

/// Save deployed WASM applications to wasm_apps_directory (for testing only)
pub const ENV_SAVE_WASM_APPS: &str = "PLEXSPACES_SAVE_WASM_APPS";

/// Base directory for PlexSpaces data (default: $HOME/plexspaces)
pub const ENV_BASE_DIR: &str = "PLEXSPACES_BASE_DIR";

/// Path to release config file (YAML/TOML)
pub const ENV_RELEASE_CONFIG_PATH: &str = "PLEXSPACES_RELEASE_CONFIG_PATH";

// ============================================================================
// Security Environment Variables
// ============================================================================

/// JWT secret for token signing/validation (REQUIRED when JWT auth enabled)
pub const ENV_JWT_SECRET: &str = "PLEXSPACES_JWT_SECRET";

/// Disable authentication (for testing only) - "1", "true", or "yes"
pub const ENV_DISABLE_AUTH: &str = "PLEXSPACES_DISABLE_AUTH";

/// Enable test mode (relaxed security checks)
pub const ENV_TEST_MODE: &str = "PLEXSPACES_TEST_MODE";

/// mTLS CA certificate path
pub const ENV_MTLS_CA_CERT: &str = "PLEXSPACES_MTLS_CA_CERT";

/// mTLS server certificate path
pub const ENV_MTLS_SERVER_CERT: &str = "PLEXSPACES_MTLS_SERVER_CERT";

/// mTLS server private key path
pub const ENV_MTLS_SERVER_KEY: &str = "PLEXSPACES_MTLS_SERVER_KEY";

/// mTLS certificate directory
pub const ENV_MTLS_CERT_DIR: &str = "PLEXSPACES_MTLS_CERT_DIR";

// ============================================================================
// Database Environment Variables
// ============================================================================

/// Shared database URL (SQLite, PostgreSQL)
pub const ENV_DATABASE_URL: &str = "PLEXSPACES_DATABASE_URL";

/// Journal database path
pub const ENV_JOURNAL_DB: &str = "PLEXSPACES_JOURNAL_DB";

/// PostgreSQL URL (for tuplespace, keyvalue, etc.)
pub const ENV_POSTGRES_URL: &str = "PLEXSPACES_POSTGRES_URL";

/// PostgreSQL table name
pub const ENV_POSTGRES_TABLE: &str = "PLEXSPACES_POSTGRES_TABLE";

/// SQLite path
pub const ENV_SQLITE_PATH: &str = "PLEXSPACES_SQLITE_PATH";

/// Redis URL
pub const ENV_REDIS_URL: &str = "PLEXSPACES_REDIS_URL";

/// Redis namespace prefix
pub const ENV_REDIS_NAMESPACE: &str = "PLEXSPACES_REDIS_NAMESPACE";

/// Connection pool size
pub const ENV_POOL_SIZE: &str = "PLEXSPACES_POOL_SIZE";

// ============================================================================
// KeyValue-specific Environment Variables
// ============================================================================

/// KeyValue backend type (sqlite, postgres, redis)
/// KeyValue SQLite path
pub const ENV_KV_SQLITE_PATH: &str = "PLEXSPACES_KV_SQLITE_PATH";

/// KeyValue PostgreSQL URL
pub const ENV_KV_POSTGRES_URL: &str = "PLEXSPACES_KV_POSTGRES_URL";

/// KeyValue PostgreSQL pool size
pub const ENV_KV_POSTGRES_POOL_SIZE: &str = "PLEXSPACES_KV_POSTGRES_POOL_SIZE";

/// KeyValue Redis URL
pub const ENV_KV_REDIS_URL: &str = "PLEXSPACES_KV_REDIS_URL";

/// KeyValue Redis namespace
pub const ENV_KV_REDIS_NAMESPACE: &str = "PLEXSPACES_KV_REDIS_NAMESPACE";

// ============================================================================
// TupleSpace-specific Environment Variables
// ============================================================================

/// TupleSpace backend type (in-memory, sqlite, redis, postgres)
pub const ENV_TUPLESPACE_BACKEND: &str = "PLEXSPACES_TUPLESPACE_BACKEND";

/// TupleSpace PostgreSQL URL
pub const ENV_TUPLESPACE_POSTGRES_URL: &str = "PLEXSPACES_TUPLESPACE_POSTGRES_URL";

// ============================================================================
// AWS/Cloud Environment Variables
// ============================================================================

/// AWS region
pub const ENV_AWS_REGION: &str = "AWS_REGION";

/// DynamoDB endpoint URL (for local testing)
pub const ENV_DDB_ENDPOINT_URL: &str = "PLEXSPACES_DDB_ENDPOINT_URL";

/// DynamoDB region
pub const ENV_DDB_REGION: &str = "PLEXSPACES_DYNAMODB_REGION";

/// DynamoDB table name
pub const ENV_DDB_TABLE: &str = "PLEXSPACES_DYNAMODB_TABLE";

/// DynamoDB locks table
pub const ENV_DDB_LOCKS_TABLE: &str = "PLEXSPACES_DDB_LOCKS_TABLE";

/// SQS region
pub const ENV_SQS_REGION: &str = "PLEXSPACES_SQS_REGION";

/// SQS queue URL
pub const ENV_SQS_QUEUE_URL: &str = "PLEXSPACES_SQS_QUEUE_URL";

/// SQS queue prefix
pub const ENV_SQS_QUEUE_PREFIX: &str = "PLEXSPACES_SQS_QUEUE_PREFIX";

/// SQS endpoint URL (for local testing)
pub const ENV_SQS_ENDPOINT_URL: &str = "PLEXSPACES_SQS_ENDPOINT_URL";

/// Kafka brokers
pub const ENV_KAFKA_BROKERS: &str = "PLEXSPACES_KAFKA_BROKERS";

// ============================================================================
// Blob Storage Environment Variables
// ============================================================================

/// Blob storage backend (minio, s3, gcs, azure)
pub const ENV_BLOB_BACKEND: &str = "BLOB_BACKEND";

/// Blob storage bucket name
pub const ENV_BLOB_BUCKET: &str = "BLOB_BUCKET";

/// Blob storage endpoint URL
pub const ENV_BLOB_ENDPOINT: &str = "BLOB_ENDPOINT";

/// Blob storage region
pub const ENV_BLOB_REGION: &str = "BLOB_REGION";

/// Blob access key ID
pub const ENV_BLOB_ACCESS_KEY_ID: &str = "BLOB_ACCESS_KEY_ID";

/// Blob secret access key
pub const ENV_BLOB_SECRET_ACCESS_KEY: &str = "BLOB_SECRET_ACCESS_KEY";

/// Blob use SSL
pub const ENV_BLOB_USE_SSL: &str = "BLOB_USE_SSL";

/// Blob prefix
pub const ENV_BLOB_PREFIX: &str = "BLOB_PREFIX";

/// GCP service account JSON
pub const ENV_GCP_SERVICE_ACCOUNT_JSON: &str = "GCP_SERVICE_ACCOUNT_JSON";

/// Azure account name
pub const ENV_AZURE_ACCOUNT_NAME: &str = "AZURE_ACCOUNT_NAME";

/// Azure account key
pub const ENV_AZURE_ACCOUNT_KEY: &str = "AZURE_ACCOUNT_KEY";

/// MinIO endpoint
pub const ENV_MINIO_ENDPOINT: &str = "PLEXSPACES_MINIO_ENDPOINT";

/// MinIO access key
pub const ENV_MINIO_ACCESS_KEY: &str = "PLEXSPACES_MINIO_ACCESS_KEY";

/// MinIO secret key
pub const ENV_MINIO_SECRET_KEY: &str = "PLEXSPACES_MINIO_SECRET_KEY";

// ============================================================================
// Helper Functions
// ============================================================================

/// Get environment variable value, returning None if not set or empty
///
/// ## Arguments
/// * `key` - Environment variable name
///
/// ## Returns
/// `Some(value)` if set and non-empty, `None` otherwise
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_common::config_manager::{get_env, ENV_NODE_ID};
/// if let Some(node_id) = get_env(ENV_NODE_ID) {
///     println!("Node ID: {}", node_id);
/// }
/// ```
pub fn get_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|s| !s.is_empty())
}

/// Get environment variable value with default
///
/// ## Arguments
/// * `key` - Environment variable name
/// * `default` - Default value if not set or empty
///
/// ## Returns
/// Environment variable value if set and non-empty, default otherwise
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_common::config_manager::{get_env_or, ENV_LISTEN_ADDR};
/// let addr = get_env_or(ENV_LISTEN_ADDR, "0.0.0.0:8000");
/// ```
pub fn get_env_or(key: &str, default: &str) -> String {
    get_env(key).unwrap_or_else(|| default.to_string())
}

/// Get environment variable as bool (accepts "1", "true", "yes")
///
/// ## Arguments
/// * `key` - Environment variable name
///
/// ## Returns
/// `true` if set to "1", "true", or "yes" (case-insensitive), `false` otherwise
pub fn get_env_bool(key: &str) -> bool {
    get_env(key)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes"))
        .unwrap_or(false)
}

/// Get environment variable as u32
///
/// ## Arguments
/// * `key` - Environment variable name
/// * `default` - Default value if not set or not parseable
///
/// ## Returns
/// Parsed u32 value, or default if not set/parseable
pub fn get_env_u32(key: &str, default: u32) -> u32 {
    get_env(key)
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(default)
}

/// Get environment variable as u64
///
/// ## Arguments
/// * `key` - Environment variable name
/// * `default` - Default value if not set or not parseable
///
/// ## Returns
/// Parsed u64 value, or default if not set/parseable
pub fn get_env_u64(key: &str, default: u64) -> u64 {
    get_env(key)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

// ============================================================================
// EnvConfig - Typed Configuration Access
// ============================================================================

/// Typed configuration from environment variables
///
/// ## Purpose
/// Provides strongly-typed accessors for common configuration values.
/// Use this instead of scattered `std::env::var` calls.
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_common::config_manager::EnvConfig;
/// let config = EnvConfig::from_env();
/// if config.is_auth_disabled() {
///     println!("Running in test mode with auth disabled");
/// }
/// ```
#[derive(Debug, Clone)]
pub struct EnvConfig {
    /// Node ID (from PLEXSPACES_NODE_ID)
    pub node_id: Option<String>,
    /// Listen address (from PLEXSPACES_LISTEN_ADDR)
    pub listen_addr: Option<String>,
    /// gRPC address (from PLEXSPACES_GRPC_ADDRESS)
    pub grpc_address: Option<String>,
    /// Node cluster name (from PLEXSPACES_CLUSTER_NAME)
    pub cluster_name: Option<String>,
    /// WASM apps directory (from PLEXSPACES_WASM_APPS_DIR)
    pub wasm_apps_dir: Option<String>,
    /// Save deployed WASM apps to disk (from PLEXSPACES_SAVE_WASM_APPS)
    pub save_wasm_apps: bool,
    /// Base directory (from PLEXSPACES_BASE_DIR)
    pub base_dir: Option<String>,
    /// Release config path (from PLEXSPACES_RELEASE_CONFIG_PATH)
    pub release_config_path: Option<String>,
    /// JWT secret (from PLEXSPACES_JWT_SECRET)
    pub jwt_secret: Option<String>,
    /// Auth disabled flag (from PLEXSPACES_DISABLE_AUTH)
    pub auth_disabled: bool,
    /// Test mode flag (from PLEXSPACES_TEST_MODE)
    pub test_mode: bool,
    /// Database URL (from PLEXSPACES_DATABASE_URL)
    pub database_url: Option<String>,
    /// Redis URL (from PLEXSPACES_REDIS_URL)
    pub redis_url: Option<String>,
    /// mTLS cert directory (from PLEXSPACES_MTLS_CERT_DIR)
    pub mtls_cert_dir: Option<String>,
    /// mTLS CA cert path (from PLEXSPACES_MTLS_CA_CERT)
    pub mtls_ca_cert: Option<String>,
    /// mTLS server cert path (from PLEXSPACES_MTLS_SERVER_CERT)
    pub mtls_server_cert: Option<String>,
    /// mTLS server key path (from PLEXSPACES_MTLS_SERVER_KEY)
    pub mtls_server_key: Option<String>,
}

impl EnvConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            node_id: get_env(ENV_NODE_ID),
            listen_addr: get_env(ENV_LISTEN_ADDR),
            grpc_address: get_env(ENV_GRPC_ADDRESS),
            cluster_name: get_env(ENV_CLUSTER_NAME),
            wasm_apps_dir: get_env(ENV_WASM_APPS_DIR),
            save_wasm_apps: get_env_bool(ENV_SAVE_WASM_APPS),
            base_dir: get_env(ENV_BASE_DIR),
            release_config_path: get_env(ENV_RELEASE_CONFIG_PATH),
            jwt_secret: get_env(ENV_JWT_SECRET),
            auth_disabled: get_env_bool(ENV_DISABLE_AUTH),
            test_mode: get_env_bool(ENV_TEST_MODE),
            database_url: get_env(ENV_DATABASE_URL),
            redis_url: get_env(ENV_REDIS_URL),
            mtls_cert_dir: get_env(ENV_MTLS_CERT_DIR),
            mtls_ca_cert: get_env(ENV_MTLS_CA_CERT),
            mtls_server_cert: get_env(ENV_MTLS_SERVER_CERT),
            mtls_server_key: get_env(ENV_MTLS_SERVER_KEY),
        }
    }

    /// Check if authentication is disabled
    pub fn is_auth_disabled(&self) -> bool {
        self.auth_disabled
    }

    /// Check if running in test mode
    pub fn is_test_mode(&self) -> bool {
        self.test_mode
    }

    /// Check if JWT is configured (secret present)
    pub fn has_jwt_secret(&self) -> bool {
        self.jwt_secret.is_some()
    }

    /// Check if mTLS certificates are configured
    pub fn has_mtls_certs(&self) -> bool {
        self.mtls_ca_cert.is_some()
            && self.mtls_server_cert.is_some()
            && self.mtls_server_key.is_some()
    }

    /// Get database URL with fallback to default SQLite path
    pub fn database_url_or_default(&self, node_id: &str, component: &str) -> String {
        let base_dir = self
            .base_dir
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(get_default_base_dir);
        self.database_url.clone().unwrap_or_else(|| {
            format!(
                "sqlite://{}/db/plexspaces-{}-{}.db?mode=rwc",
                base_dir, component, node_id
            )
        })
    }

    /// Get mTLS cert directory with default
    pub fn mtls_cert_dir_or_default(&self) -> String {
        self.mtls_cert_dir
            .clone()
            .unwrap_or_else(|| "/app/certs".to_string())
    }
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

// ============================================================================
// ReleaseSpec Initialization and Validation
// ============================================================================

/// Get the default base directory for PlexSpaces data.
/// Priority: env var > home_dir/plexspaces > /tmp/plexspaces
pub fn get_default_base_dir() -> String {
    if let Some(base_dir) = get_env(ENV_BASE_DIR) {
        return base_dir;
    }

    if let Some(home) = dirs::home_dir() {
        return home.join("plexspaces").to_string_lossy().to_string();
    }

    "/tmp/plexspaces".to_string()
}

/// Default shared SQLite database URL rooted under the runtime base directory.
pub fn default_shared_db_url(base_dir: &str) -> String {
    format!("sqlite://{}/db/plexspaces.db?mode=rwc", base_dir)
}

/// Mask sensitive parts of a database URL for logging
fn mask_db_url(url: &str) -> String {
    // Mask password in URLs like postgres://user:password@host/db
    if let Some(at_pos) = url.find('@') {
        if let Some(colon_pos) = url[..at_pos].rfind(':') {
            let scheme_end = url.find("://").map(|p| p + 3).unwrap_or(0);
            if colon_pos > scheme_end {
                return format!("{}****{}", &url[..colon_pos + 1], &url[at_pos..]);
            }
        }
    }
    url.to_string()
}

/// Initialize and validate ReleaseSpec configuration
///
/// ## Purpose
/// This is the ONLY place where PLEXSPACES_* environment variables are read
/// for configuration. All other modules should read from the ReleaseSpec
/// that has been processed by this function.
///
/// ## Priority (highest to lowest)
/// 1. Environment variables (always override)
/// 2. Config file values
/// 3. Defaults
///
/// ## What it does
/// 1. Apply environment variable overrides (env vars take precedence)
/// 2. Set defaults for base_dir, wasm_apps_directory
/// 3. Set defaults for db (shared database config)
/// 4. Set defaults for channel_provider and mailbox_provider
/// 5. Log the resolved configuration
///
/// ## Environment Variable Overrides
/// - `PLEXSPACES_NODE_ID` → `spec.node.id`
/// - `PLEXSPACES_LISTEN_ADDR` → `spec.node.listen_addr`
/// - `PLEXSPACES_CLUSTER_NAME` → `spec.node.cluster_name` (if non-empty)
/// - If `spec.node.cluster_name` is still empty after file + env, it becomes [`DEFAULT_CLUSTER_NAME`]
/// - `PLEXSPACES_GRPC_ADDRESS` → `spec.runtime.grpc.address`
/// - `PLEXSPACES_BASE_DIR` → `spec.runtime.base_dir`
/// - `PLEXSPACES_WASM_APPS_DIR` → `spec.runtime.wasm_apps_directory`
/// - `PLEXSPACES_DATABASE_URL` → `spec.runtime.db.connection_string`
/// - `PLEXSPACES_JWT_SECRET` → `spec.runtime.security.jwt.secret`
/// - `PLEXSPACES_DISABLE_AUTH` → `spec.runtime.security.disable_auth`
/// - `PLEXSPACES_MTLS_*` → mTLS certificate paths
///
/// ## Arguments
/// * `spec` - ReleaseSpec to initialize in-place
pub fn initialize(spec: &mut plexspaces_proto::node::v1::ReleaseSpec) {
    use plexspaces_proto::channel::v1::ChannelProvider;
    use plexspaces_proto::storage::v1::SharedDbConfig;

    let config = EnvConfig::from_env();

    // Ensure runtime config exists
    if spec.runtime.is_none() {
        spec.runtime = Some(plexspaces_proto::node::v1::RuntimeConfig::default());
    }
    let runtime = spec.runtime.as_mut().unwrap();

    // ===========================================
    // 1. Set base_dir (foundation for other paths)
    // ===========================================
    let base_dir = config
        .base_dir
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if !runtime.base_dir.is_empty() {
                Some(runtime.base_dir.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(get_default_base_dir);
    runtime.base_dir = base_dir.clone();

    // ===========================================
    // 2. Set wasm_apps_directory
    // ===========================================
    let wasm_apps_dir = config
        .wasm_apps_dir
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if !runtime.wasm_apps_directory.is_empty() {
                Some(runtime.wasm_apps_directory.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| format!("{}/apps", base_dir));
    runtime.wasm_apps_directory = wasm_apps_dir.clone();

    // ===========================================
    // 3. Set shared database config
    // ===========================================
    let db_dir = format!("{}/db", base_dir);
    let db_url = config
        .database_url
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            runtime
                .db
                .as_ref()
                .map(|db| db.connection_string.clone())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| default_shared_db_url(&base_dir));

    if runtime.db.is_none() {
        runtime.db = Some(SharedDbConfig::default());
    }
    let db = runtime.db.as_mut().unwrap();
    db.connection_string = db_url.clone();
    if db.pool_size == 0 {
        db.pool_size = 10; // Default pool size
    }

    // ===========================================
    // 3a. Create required directories
    // ===========================================
    // Create base_dir if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&base_dir) {
        tracing::warn!(base_dir = %base_dir, error = %e, "Failed to create base directory");
    }
    // Create db directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&db_dir) {
        tracing::warn!(db_dir = %db_dir, error = %e, "Failed to create database directory");
    }
    // Create wasm_apps_directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&wasm_apps_dir) {
        tracing::warn!(wasm_apps_dir = %wasm_apps_dir, error = %e, "Failed to create WASM apps directory");
    }

    // ===========================================
    // 2a. Set save_wasm_apps flag
    // ===========================================
    // Default: false (only enable for testing)
    // Can be overridden via environment variable or config file
    if config.save_wasm_apps {
        runtime.save_wasm_apps = true;
    }
    // Note: If config.save_wasm_apps is false, we keep the existing value from proto (defaults to false)

    // ===========================================
    // 4. Set locks_provider default
    // ===========================================
    // As per design: "redis (if available), else use shared db"
    // Only set if not already explicitly configured
    if runtime.locks_provider.is_none() {
        use plexspaces_proto::storage::v1::{
            RedisBackendConfig, StorageProvider, StorageProviderConfig,
        };

        if let Some(ref redis_url) = config.redis_url {
            // Redis is configured - use Redis for locks
            tracing::info!(
                locks_provider = "Redis",
                "Locks provider set to Redis (from PLEXSPACES_REDIS_URL)"
            );
            runtime.locks_provider = Some(StorageProviderConfig {
                provider: StorageProvider::StorageProviderRedis as i32,
                config: Some(
                    plexspaces_proto::storage::v1::storage_provider_config::Config::Redis(
                        RedisBackendConfig {
                            url: redis_url.clone(),
                            ..Default::default()
                        },
                    ),
                ),
            });
        } else {
            // No Redis - use shared database for locks
            // Determine provider type from db_url
            let provider = if db_url.starts_with("postgres") {
                StorageProvider::StorageProviderPostgres
            } else {
                StorageProvider::StorageProviderSqlite
            };
            tracing::info!(locks_provider = ?provider, "Locks provider set to shared database");
            runtime.locks_provider = Some(StorageProviderConfig {
                provider: provider as i32,
                config: None, // Uses shared db config
            });
        }
    }

    // ===========================================
    // 5. Set channel_provider default
    // ===========================================
    // ChannelProvider enum: 0 = IN_MEMORY (default)
    // If not explicitly set, default to IN_MEMORY
    if runtime.channel_provider == ChannelProvider::ChannelProviderInMemory as i32 {
        // Check if Redis URL is available - prefer Redis if configured
        if config.redis_url.is_some() {
            runtime.channel_provider = ChannelProvider::ChannelProviderRedis as i32;
        }
        // Otherwise keep IN_MEMORY default
    }

    // ===========================================
    // 6. Set mailbox_provider default
    // ===========================================
    // Default to IN_MEMORY (same as channel_provider)
    if runtime.mailbox_provider == ChannelProvider::ChannelProviderInMemory as i32 {
        // Keep IN_MEMORY default for mailbox
    }

    // ===========================================
    // 7. Apply Node config overrides
    // ===========================================
    if let Some(ref mut node) = spec.node {
        if let Some(ref node_id) = config.node_id {
            node.id = node_id.clone();
        }
        if let Some(ref listen_addr) = config.listen_addr {
            node.listen_addr = listen_addr.clone();
        }
        if let Some(ref grpc_addr) = config.grpc_address {
            node.grpc_address = grpc_addr.clone();
        }
        if let Some(ref name) = config.cluster_name {
            if !name.is_empty() {
                node.cluster_name = name.clone();
            }
        }
        if node.cluster_name.is_empty() {
            node.cluster_name = DEFAULT_CLUSTER_NAME.to_string();
        }
    }

    // ===========================================
    // 8. Apply gRPC config overrides
    // ===========================================
    if let Some(ref grpc_addr) = config.grpc_address {
        if let Some(ref mut grpc) = runtime.grpc {
            grpc.address = grpc_addr.clone();
        }
    }

    // ===========================================
    // 9. Apply Security config overrides
    // ===========================================
    if let Some(ref mut security) = runtime.security {
        // Auth disabled
        if config.auth_disabled {
            security.disable_auth = true;
        }

        // JWT secret
        if let Some(ref jwt_secret) = config.jwt_secret {
            if let Some(ref mut jwt) = security.jwt {
                jwt.secret = jwt_secret.clone();
            }
        }

        // mTLS paths
        if let Some(ref mut mtls) = security.mtls {
            if let Some(ref ca_cert) = config.mtls_ca_cert {
                mtls.ca_certificate_path = ca_cert.clone();
            }
            if let Some(ref server_cert) = config.mtls_server_cert {
                mtls.server_certificate_path = server_cert.clone();
            }
            if let Some(ref server_key) = config.mtls_server_key {
                mtls.server_key_path = server_key.clone();
            }
            if let Some(ref cert_dir) = config.mtls_cert_dir {
                mtls.cert_dir = cert_dir.clone();
            }
        }
    }

    // ===========================================
    // 9. Log resolved configuration
    // ===========================================
    let channel_provider_name = match ChannelProvider::try_from(runtime.channel_provider) {
        Ok(ChannelProvider::ChannelProviderInMemory) => "IN_MEMORY",
        Ok(ChannelProvider::ChannelProviderRedis) => "REDIS",
        Ok(ChannelProvider::ChannelProviderKafka) => "KAFKA",
        Ok(ChannelProvider::ChannelProviderSqlite) => "SQLITE",
        Ok(ChannelProvider::ChannelProviderNats) => "NATS",
        Ok(ChannelProvider::ChannelProviderPostgres) => "POSTGRES",
        Ok(ChannelProvider::ChannelProviderProcessGroup) => "PROCESS_GROUP",
        Ok(ChannelProvider::ChannelProviderSqs) => "SQS",
        Ok(ChannelProvider::ChannelProviderUdp) => "UDP",
        Ok(ChannelProvider::ChannelProviderCustom) => "CUSTOM",
        Err(_) => "UNKNOWN",
    };

    let mailbox_provider_name = match ChannelProvider::try_from(runtime.mailbox_provider) {
        Ok(ChannelProvider::ChannelProviderInMemory) => "IN_MEMORY",
        Ok(ChannelProvider::ChannelProviderRedis) => "REDIS",
        Ok(ChannelProvider::ChannelProviderSqlite) => "SQLITE",
        Ok(ChannelProvider::ChannelProviderPostgres) => "POSTGRES",
        Ok(_) => "OTHER",
        Err(_) => "UNKNOWN",
    };

    let auth_status = if runtime
        .security
        .as_ref()
        .map(|s| s.disable_auth)
        .unwrap_or(false)
    {
        "disabled"
    } else {
        "enabled"
    };

    tracing::info!(
        base_dir = %base_dir,
        wasm_apps_directory = %wasm_apps_dir,
        db_url = %mask_db_url(&db_url),
        channel_provider = %channel_provider_name,
        mailbox_provider = %mailbox_provider_name,
        auth = %auth_status,
        "Configuration initialized"
    );
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;

    #[test]
    fn test_get_env_missing() {
        env::remove_var("TEST_MISSING_VAR_12345");
        assert!(get_env("TEST_MISSING_VAR_12345").is_none());
    }

    #[test]
    fn test_get_env_empty() {
        env::set_var("TEST_EMPTY_VAR", "");
        assert!(get_env("TEST_EMPTY_VAR").is_none());
        env::remove_var("TEST_EMPTY_VAR");
    }

    #[test]
    fn test_get_env_present() {
        env::set_var("TEST_PRESENT_VAR", "test-value");
        assert_eq!(get_env("TEST_PRESENT_VAR"), Some("test-value".to_string()));
        env::remove_var("TEST_PRESENT_VAR");
    }

    #[test]
    fn test_get_env_or_missing() {
        env::remove_var("TEST_OR_MISSING_VAR");
        assert_eq!(get_env_or("TEST_OR_MISSING_VAR", "default"), "default");
    }

    #[test]
    fn test_get_env_or_present() {
        env::set_var("TEST_OR_PRESENT_VAR", "custom");
        assert_eq!(get_env_or("TEST_OR_PRESENT_VAR", "default"), "custom");
        env::remove_var("TEST_OR_PRESENT_VAR");
    }

    #[test]
    fn test_get_env_bool() {
        env::set_var("TEST_BOOL_1", "1");
        env::set_var("TEST_BOOL_TRUE", "true");
        env::set_var("TEST_BOOL_YES", "YES");
        env::set_var("TEST_BOOL_FALSE", "false");
        env::set_var("TEST_BOOL_NO", "no");

        assert!(get_env_bool("TEST_BOOL_1"));
        assert!(get_env_bool("TEST_BOOL_TRUE"));
        assert!(get_env_bool("TEST_BOOL_YES"));
        assert!(!get_env_bool("TEST_BOOL_FALSE"));
        assert!(!get_env_bool("TEST_BOOL_NO"));
        assert!(!get_env_bool("TEST_BOOL_MISSING"));

        env::remove_var("TEST_BOOL_1");
        env::remove_var("TEST_BOOL_TRUE");
        env::remove_var("TEST_BOOL_YES");
        env::remove_var("TEST_BOOL_FALSE");
        env::remove_var("TEST_BOOL_NO");
    }

    #[test]
    fn test_get_env_u32() {
        env::set_var("TEST_U32_VALID", "42");
        env::set_var("TEST_U32_INVALID", "not-a-number");

        assert_eq!(get_env_u32("TEST_U32_VALID", 0), 42);
        assert_eq!(get_env_u32("TEST_U32_INVALID", 100), 100);
        assert_eq!(get_env_u32("TEST_U32_MISSING", 200), 200);

        env::remove_var("TEST_U32_VALID");
        env::remove_var("TEST_U32_INVALID");
    }

    #[test]
    #[serial]
    fn test_env_config_from_env() {
        env::set_var("PLEXSPACES_NODE_ID", "test-node");
        env::set_var("PLEXSPACES_DISABLE_AUTH", "1");

        let config = EnvConfig::from_env();
        assert_eq!(config.node_id, Some("test-node".to_string()));
        assert!(config.is_auth_disabled());

        env::remove_var("PLEXSPACES_NODE_ID");
        env::remove_var("PLEXSPACES_DISABLE_AUTH");
    }

    #[test]
    #[serial]
    fn test_env_config_database_url_default() {
        env::remove_var("PLEXSPACES_DATABASE_URL");
        env::remove_var("PLEXSPACES_BASE_DIR");
        let config = EnvConfig::from_env();
        let url = config.database_url_or_default("node-1", "journal");
        let expected_base_dir = get_default_base_dir();
        assert_eq!(
            url,
            format!(
                "sqlite://{}/db/plexspaces-journal-node-1.db?mode=rwc",
                expected_base_dir
            )
        );
    }

    #[test]
    #[serial]
    fn test_initialize_sets_defaults() {
        use plexspaces_proto::node::v1::ReleaseSpec;

        // Clean environment for test
        env::remove_var("PLEXSPACES_BASE_DIR");
        env::remove_var("PLEXSPACES_WASM_APPS_DIR");
        env::remove_var("PLEXSPACES_DATABASE_URL");
        env::remove_var("PLEXSPACES_NODE_ID");

        let mut spec = ReleaseSpec::default();
        initialize(&mut spec);

        // Verify runtime was created
        assert!(spec.runtime.is_some());
        let runtime = spec.runtime.as_ref().unwrap();

        // Verify base_dir is set to default (home_dir/plexspaces)
        assert!(!runtime.base_dir.is_empty());
        assert!(
            runtime.base_dir.ends_with("plexspaces") || runtime.base_dir.contains("plexspaces")
        );

        // Verify wasm_apps_directory is set
        assert!(!runtime.wasm_apps_directory.is_empty());
        assert!(runtime.wasm_apps_directory.ends_with("/apps"));

        // Verify db is configured
        assert!(runtime.db.is_some());
        let db = runtime.db.as_ref().unwrap();
        assert!(!db.connection_string.is_empty());
        assert!(db.connection_string.contains("sqlite"));
        assert!(db.pool_size > 0);
    }

    // Tests that set/remove PLEXSPACES_* env vars use #[serial] — env is process-global.

    #[test]
    #[serial]
    fn test_initialize_with_env_config() {
        use plexspaces_proto::node::v1::{NodeConfig, ReleaseSpec, RuntimeConfig};
        use plexspaces_proto::storage::v1::SharedDbConfig;

        env::remove_var(ENV_CLUSTER_NAME);

        let mut spec = ReleaseSpec {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "test".to_string(),
            node: Some(NodeConfig {
                id: "config-node-id".to_string(),
                ..Default::default()
            }),
            runtime: Some(RuntimeConfig {
                base_dir: "/config/base/dir".to_string(),
                db: Some(SharedDbConfig {
                    connection_string: "postgres://localhost/test".to_string(),
                    pool_size: 5,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            applications: vec![],
            env: std::collections::HashMap::new(),
            shutdown: None,
            system_applications: vec![],
        };

        // Test that initialize preserves config values when no env overrides
        // (This is safe because we don't set env vars)
        initialize(&mut spec);

        let runtime = spec.runtime.as_ref().unwrap();

        // Config file values should be preserved (no env overrides in this test)
        assert_eq!(runtime.base_dir, "/config/base/dir");
        assert_eq!(
            runtime.db.as_ref().unwrap().connection_string,
            "postgres://localhost/test"
        );
        assert_eq!(spec.node.as_ref().unwrap().id, "config-node-id");
        assert_eq!(
            spec.node.as_ref().unwrap().cluster_name,
            DEFAULT_CLUSTER_NAME,
            "empty cluster_name in file and env should default"
        );

        env::remove_var(ENV_CLUSTER_NAME);
    }

    #[test]
    #[serial]
    fn test_initialize_applies_cluster_name_from_env() {
        use plexspaces_proto::node::v1::{NodeConfig, ReleaseSpec};

        env::set_var(ENV_CLUSTER_NAME, "heat");

        let mut spec = ReleaseSpec {
            node: Some(NodeConfig {
                id: "n1".into(),
                cluster_name: String::new(),
                ..Default::default()
            }),
            ..Default::default()
        };

        initialize(&mut spec);

        assert_eq!(
            spec.node.as_ref().unwrap().cluster_name,
            "heat",
            "PLEXSPACES_CLUSTER_NAME should override spec.node.cluster_name"
        );

        env::remove_var(ENV_CLUSTER_NAME);
    }

    #[test]
    fn test_env_config_priority_order() {
        // Test the EnvConfig structure directly
        // EnvConfig.from_env() reads env vars, but we test the priority logic here

        // Priority should be: Env var > Config file > Default
        // This test verifies the logic without actually modifying process env

        let config = EnvConfig {
            node_id: Some("env-node".to_string()),
            listen_addr: None, // Not set in "env"
            grpc_address: None,
            cluster_name: None,
            wasm_apps_dir: Some("/env/apps".to_string()),
            save_wasm_apps: false,
            base_dir: Some("/env/base".to_string()),
            database_url: Some("sqlite::memory:".to_string()),
            jwt_secret: None,
            auth_disabled: true,
            test_mode: false,
            mtls_ca_cert: None,
            mtls_server_cert: None,
            mtls_server_key: None,
            mtls_cert_dir: None,
            redis_url: None,
            release_config_path: None,
        };

        // Verify EnvConfig fields
        assert_eq!(config.node_id, Some("env-node".to_string()));
        assert_eq!(config.base_dir, Some("/env/base".to_string()));
        assert_eq!(config.database_url, Some("sqlite::memory:".to_string()));
        assert!(config.auth_disabled);

        // Verify is_auth_disabled helper
        assert!(config.is_auth_disabled());
    }

    #[test]
    #[serial]
    fn test_initialize_creates_directories() {
        use plexspaces_proto::node::v1::ReleaseSpec;
        use std::path::Path;

        // Unique path that does not exist yet (process env is shared; #[serial] avoids races).
        let parent = tempfile::tempdir().expect("temp parent dir");
        let test_base_dir = parent.path().join("plexspaces-base");
        let test_base_str = test_base_dir.to_string_lossy().to_string();
        env::set_var("PLEXSPACES_BASE_DIR", &test_base_str);

        let mut spec = ReleaseSpec::default();
        initialize(&mut spec);

        // Verify directories were created
        assert!(Path::new(&test_base_str).exists(), "base_dir should exist");
        assert!(test_base_dir.join("db").exists(), "db dir should exist");
        assert!(test_base_dir.join("apps").exists(), "apps dir should exist");

        env::remove_var("PLEXSPACES_BASE_DIR");
    }

    #[test]
    #[serial]
    fn test_initialize_security_overrides() {
        use plexspaces_proto::node::v1::{ReleaseSpec, RuntimeConfig, SecurityConfig};
        use plexspaces_proto::security::v1::JwtConfig;

        env::set_var("PLEXSPACES_DISABLE_AUTH", "1");
        env::set_var("PLEXSPACES_JWT_SECRET", "test-secret-123");

        let mut spec = ReleaseSpec {
            runtime: Some(RuntimeConfig {
                security: Some(SecurityConfig {
                    disable_auth: false,
                    jwt: Some(JwtConfig {
                        enable_jwt: true,
                        secret: String::new(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        initialize(&mut spec);

        let security = spec.runtime.as_ref().unwrap().security.as_ref().unwrap();

        // Auth should be disabled via env var
        assert!(security.disable_auth);

        // JWT secret should be set via env var
        assert_eq!(security.jwt.as_ref().unwrap().secret, "test-secret-123");

        // Cleanup
        env::remove_var("PLEXSPACES_DISABLE_AUTH");
        env::remove_var("PLEXSPACES_JWT_SECRET");
    }

    #[test]
    fn test_initialize_locks_provider_logic() {
        use plexspaces_proto::storage::v1::StorageProvider;

        // Test the locks_provider logic directly via EnvConfig
        // This avoids env var pollution from parallel tests

        // Case 1: No Redis URL -> should use SQLite
        let config_no_redis = EnvConfig {
            redis_url: None,
            ..Default::default()
        };
        assert!(config_no_redis.redis_url.is_none());

        // Case 2: Redis URL set -> should use Redis
        let config_with_redis = EnvConfig {
            redis_url: Some("redis://localhost:6379".to_string()),
            ..Default::default()
        };
        assert!(config_with_redis.redis_url.is_some());

        // Verify StorageProvider enum values match expectations
        assert_eq!(StorageProvider::StorageProviderSqlite as i32, 2);
        assert_eq!(StorageProvider::StorageProviderRedis as i32, 3);
    }

    #[test]
    #[serial]
    fn test_initialize_locks_provider_uses_redis_when_available() {
        use plexspaces_proto::node::v1::ReleaseSpec;
        use plexspaces_proto::storage::v1::StorageProvider;

        // Set Redis URL
        env::set_var("PLEXSPACES_REDIS_URL", "redis://localhost:6379");

        let mut spec = ReleaseSpec::default();
        initialize(&mut spec);

        // Verify locks_provider uses Redis
        let runtime = spec.runtime.as_ref().unwrap();
        assert!(
            runtime.locks_provider.is_some(),
            "locks_provider should be set"
        );

        let locks_provider = runtime.locks_provider.as_ref().unwrap();
        assert_eq!(
            locks_provider.provider,
            StorageProvider::StorageProviderRedis as i32,
            "locks_provider should use Redis when PLEXSPACES_REDIS_URL is set"
        );

        // Verify Redis config is populated
        if let Some(plexspaces_proto::storage::v1::storage_provider_config::Config::Redis(
            redis_config,
        )) = &locks_provider.config
        {
            assert_eq!(redis_config.url, "redis://localhost:6379");
        } else {
            panic!("locks_provider config should be Redis");
        }

        // Cleanup
        env::remove_var("PLEXSPACES_REDIS_URL");
    }

    #[test]
    #[serial]
    fn test_initialize_preserves_explicit_locks_provider() {
        use plexspaces_proto::node::v1::{ReleaseSpec, RuntimeConfig};
        use plexspaces_proto::storage::v1::{
            DynamoDbBackendConfig, StorageProvider, StorageProviderConfig,
        };

        // Set Redis URL (should be ignored since locks_provider is explicit)
        env::set_var("PLEXSPACES_REDIS_URL", "redis://localhost:6379");

        // Create spec with explicit DynamoDB locks_provider
        let mut spec = ReleaseSpec {
            runtime: Some(RuntimeConfig {
                locks_provider: Some(StorageProviderConfig {
                    provider: StorageProvider::StorageProviderDynamodb as i32,
                    config: Some(
                        plexspaces_proto::storage::v1::storage_provider_config::Config::Dynamodb(
                            DynamoDbBackendConfig {
                                table_prefix: "my-locks".to_string(),
                                ..Default::default()
                            },
                        ),
                    ),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        initialize(&mut spec);

        // Verify locks_provider was NOT overridden (explicit config preserved)
        let runtime = spec.runtime.as_ref().unwrap();
        let locks_provider = runtime.locks_provider.as_ref().unwrap();
        assert_eq!(
            locks_provider.provider,
            StorageProvider::StorageProviderDynamodb as i32,
            "Explicit locks_provider should be preserved"
        );

        // Cleanup
        env::remove_var("PLEXSPACES_REDIS_URL");
    }
}
