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

//! TupleSpace Configuration Module
//!
//! ## Purpose
//! Provides configuration infrastructure for TupleSpace backend selection and initialization.
//! Uses protobuf-defined configuration types for type safety and consistency.
//!
//! ## Configuration Hierarchy
//! 1. **CODE**: Explicit `TupleSpaceConfig` in application code (highest priority)
//! 2. **ENV**: Environment variables (PLEXSPACES_TUPLESPACE_BACKEND, etc.)
//! 3. **FILE**: YAML/TOML configuration files
//! 4. **DEFAULT**: In-memory backend (lowest priority)
//!
//! ## Supported Backends
//! - **InMemory**: Fast, single-process, no persistence
//! - **SQLite**: Multi-process, embedded, no external dependencies
//! - **Redis**: Distributed, sub-millisecond latency, production-ready
//! - **PostgreSQL**: Distributed, ACID transactions, strong consistency
//!
//! ## Examples
//!
//! ### From Code (Highest Priority)
//! ```rust
//! use plexspaces_tuplespace::TupleSpace;
//! use plexspaces_proto::tuplespace::v1::{TupleSpaceConfig, SqliteBackend};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = TupleSpaceConfig {
//!     backend: Some(plexspaces_proto::v1::tuplespace::tuple_space_config::Backend::Sqlite(
//!         SqliteBackend {
//!             path: "/tmp/test.db".to_string(),
//!         }
//!     )),
//!     pool_size: 1,
//!     default_ttl_seconds: 0,
//!     enable_indexing: false,
//! };
//! let space = TupleSpace::from_config(config).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### From Environment Variables
//! ```bash
//! export PLEXSPACES_TUPLESPACE_BACKEND=sqlite
//! export PLEXSPACES_SQLITE_PATH=/tmp/tuples.db
//! ```
//!
//! ```rust
//! use plexspaces_tuplespace::TupleSpace;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let space = TupleSpace::from_env().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### From Config File (YAML)
//! ```yaml
//! backend:
//!   sqlite:
//!     path: /tmp/tuples.db
//! pool_size: 1
//! default_ttl_seconds: 3600
//! enable_indexing: true
//! ```
//!
//! ```rust
//! use plexspaces_tuplespace::TupleSpace;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let space = TupleSpace::from_file("config/tuplespace.yaml").await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Smart Default (Multi-Source)
//! ```rust
//! use plexspaces_tuplespace::TupleSpace;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Tries env vars first, falls back to in-memory
//! let space = TupleSpace::from_env_or_default().await?;
//! # Ok(())
//! # }
//! ```

use crate::{TupleSpace, TupleSpaceError};
use plexspaces_proto::tuplespace::v1::{
    tuple_space_config::Backend, InMemoryBackend, PostgresBackend, RedisBackend, SqliteBackend,
    TupleSpaceConfig,
};

impl TupleSpace {
    /// Create TupleSpace from explicit configuration (CODE - highest priority)
    ///
    /// ## Purpose
    /// Creates a TupleSpace instance from a protobuf-defined configuration.
    /// This is the highest priority configuration method.
    ///
    /// ## Arguments
    /// * `config` - TupleSpaceConfig protobuf message
    ///
    /// ## Returns
    /// Configured TupleSpace instance
    ///
    /// ## Errors
    /// - `TupleSpaceError::Configuration`: Invalid or missing backend configuration
    /// - `TupleSpaceError::StorageError`: Backend connection failure
    ///
    /// ## Examples
    /// ```rust
    /// use plexspaces_tuplespace::TupleSpace;
    /// use plexspaces_proto::tuplespace::v1::{TupleSpaceConfig, SqliteBackend};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = TupleSpaceConfig {
    ///     backend: Some(plexspaces_proto::v1::tuplespace::tuple_space_config::Backend::Sqlite(
    ///         SqliteBackend { path: "/tmp/test.db".to_string() }
    ///     )),
    ///     pool_size: 1,
    ///     default_ttl_seconds: 0,
    ///     enable_indexing: false,
    /// };
    /// let space = TupleSpace::from_config(config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_config(config: TupleSpaceConfig) -> Result<Self, TupleSpaceError> {
        match config.backend {
            None => {
                // No backend specified, use in-memory default with internal tenant/namespace
                // Note: Can't use RequestContext here due to circular dependency (core -> tuplespace)
                Ok(Self::with_tenant_namespace("internal", "system"))
            }
            Some(Backend::InMemory(_)) => {
                // In-memory backend with internal tenant/namespace
                Ok(Self::with_tenant_namespace("internal", "system"))
            }
            Some(Backend::Sqlite(_sqlite_config)) => {
                // SQLite backend
                #[cfg(feature = "sql-backend")]
                {
                    use crate::storage::sql::{SqlStorage, SqliteConfig as SqliteStorageConfig};
                    let storage_config = SqliteStorageConfig {
                        database_path: _sqlite_config.path.clone(),
                        enable_wal: true, // Enable Write-Ahead Logging for better concurrency
                        cache_size_kb: 2000, // 2MB cache
                    };
                    let storage = SqlStorage::new_sqlite(storage_config).await?;
                    Ok(Self::with_storage_and_tenant(Box::new(storage), "internal", "system"))
                }
                #[cfg(not(feature = "sql-backend"))]
                {
                    Err(TupleSpaceError::InvalidConfiguration(
                        "SQLite backend requires 'sql-backend' feature".to_string(),
                    ))
                }
            }
            Some(Backend::Redis(_redis_config)) => {
                // Redis backend
                #[cfg(feature = "redis-backend")]
                {
                    use crate::storage::redis::RedisStorage;
                    use plexspaces_proto::tuplespace::v1::RedisStorageConfig as RedisStorageProtoConfig;

                    let storage_config = RedisStorageProtoConfig {
                        connection_string: _redis_config.url.clone(),
                        key_prefix: _redis_config.namespace.clone(),
                        enable_pubsub: false, // Disable pub/sub for now
                        pool_size: 5,         // Default pool size
                    };
                    let storage = RedisStorage::new(storage_config).await?;
                    Ok(Self::with_storage_and_tenant(Box::new(storage), "internal", "system"))
                }
                #[cfg(not(feature = "redis-backend"))]
                {
                    Err(TupleSpaceError::InvalidConfiguration(
                        "Redis backend requires 'redis-backend' feature".to_string(),
                    ))
                }
            }
            Some(Backend::Postgres(_postgres_config)) => {
                // PostgreSQL backend
                #[cfg(feature = "sql-backend")]
                {
                    use crate::storage::sql::{
                        PostgresConfig as PostgresStorageConfig, SqlStorage,
                    };
                    let storage_config = PostgresStorageConfig {
                        connection_string: _postgres_config.connection_string.clone(),
                        pool_size: if config.pool_size > 0 {
                            config.pool_size
                        } else {
                            10 // Default for PostgreSQL
                        },
                        table_name: if _postgres_config.table_name.is_empty() {
                            "tuples".to_string()
                        } else {
                            _postgres_config.table_name.clone()
                        },
                    };
                    let storage = SqlStorage::new_postgres(storage_config).await?;
                    Ok(Self::with_storage_and_tenant(Box::new(storage), "internal", "system"))
                }
                #[cfg(not(feature = "sql-backend"))]
                {
                    Err(TupleSpaceError::InvalidConfiguration(
                        "PostgreSQL backend requires 'sql-backend' feature".to_string(),
                    ))
                }
            }
        }
    }

    /// Create TupleSpace from environment variables (ENV - medium priority)
    ///
    /// ## Purpose
    /// Creates a TupleSpace instance from environment variables.
    /// This allows runtime configuration without code changes.
    ///
    /// ## Environment Variables
    /// - `PLEXSPACES_TUPLESPACE_BACKEND`: Backend type ("in-memory", "sqlite", "redis", "postgres")
    /// - `PLEXSPACES_SQLITE_PATH`: SQLite database file path
    /// - `PLEXSPACES_REDIS_URL`: Redis connection URL
    /// - `PLEXSPACES_REDIS_NAMESPACE`: Redis key namespace
    /// - `PLEXSPACES_POSTGRES_URL`: PostgreSQL connection string
    /// - `PLEXSPACES_POSTGRES_TABLE`: PostgreSQL table name (default: "tuples")
    /// - `PLEXSPACES_POOL_SIZE`: Connection pool size
    ///
    /// ## Returns
    /// Configured TupleSpace instance
    ///
    /// ## Errors
    /// - `TupleSpaceError::Configuration`: Missing required environment variables
    /// - `TupleSpaceError::StorageError`: Backend connection failure
    ///
    /// ## Examples
    /// ```bash
    /// export PLEXSPACES_TUPLESPACE_BACKEND=sqlite
    /// export PLEXSPACES_SQLITE_PATH=/tmp/tuples.db
    /// export PLEXSPACES_POOL_SIZE=1
    /// ```
    ///
    /// ```rust
    /// use plexspaces_tuplespace::TupleSpace;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let space = TupleSpace::from_env().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_env() -> Result<Self, TupleSpaceError> {
        let backend_type = std::env::var("PLEXSPACES_TUPLESPACE_BACKEND")
            .unwrap_or_else(|_| "in-memory".to_string())
            .to_lowercase();

        let pool_size = std::env::var("PLEXSPACES_POOL_SIZE")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0); // 0 = use backend default

        match backend_type.as_str() {
            "in-memory" | "in_memory" | "memory" => {
                // In-memory backend (no configuration needed)
                let config = TupleSpaceConfig {
                    backend: Some(Backend::InMemory(InMemoryBackend {})),
                    pool_size: 0,
                    default_ttl_seconds: 0,
                    enable_indexing: false,
                };
                Self::from_config(config).await
            }
            "sqlite" => {
                let path = std::env::var("PLEXSPACES_SQLITE_PATH").map_err(|_| {
                    TupleSpaceError::InvalidConfiguration(
                        "PLEXSPACES_SQLITE_PATH environment variable required for SQLite backend"
                            .to_string(),
                    )
                })?;

                let config = TupleSpaceConfig {
                    backend: Some(Backend::Sqlite(SqliteBackend { path })),
                    pool_size,
                    default_ttl_seconds: 0,
                    enable_indexing: false,
                };
                Self::from_config(config).await
            }
            "redis" => {
                let url = std::env::var("PLEXSPACES_REDIS_URL").map_err(|_| {
                    TupleSpaceError::InvalidConfiguration(
                        "PLEXSPACES_REDIS_URL environment variable required for Redis backend"
                            .to_string(),
                    )
                })?;

                let namespace = std::env::var("PLEXSPACES_REDIS_NAMESPACE")
                    .unwrap_or_else(|_| "plexspaces".to_string());

                let config = TupleSpaceConfig {
                    backend: Some(Backend::Redis(RedisBackend { url, namespace })),
                    pool_size,
                    default_ttl_seconds: 0,
                    enable_indexing: false,
                };
                Self::from_config(config).await
            }
            "postgres" | "postgresql" => {
                let connection_string = std::env::var("PLEXSPACES_POSTGRES_URL").map_err(|_| {
                    TupleSpaceError::InvalidConfiguration(
                        "PLEXSPACES_POSTGRES_URL environment variable required for PostgreSQL backend"
                            .to_string(),
                    )
                })?;

                let table_name = std::env::var("PLEXSPACES_POSTGRES_TABLE")
                    .unwrap_or_else(|_| "tuples".to_string());

                let config = TupleSpaceConfig {
                    backend: Some(Backend::Postgres(PostgresBackend {
                        connection_string,
                        table_name,
                    })),
                    pool_size,
                    default_ttl_seconds: 0,
                    enable_indexing: false,
                };
                Self::from_config(config).await
            }
            _ => {
                // Default: in-memory (handles "in-memory" and all other cases)
                let config = TupleSpaceConfig {
                    backend: Some(Backend::InMemory(InMemoryBackend {})),
                    pool_size: 0,
                    default_ttl_seconds: 0,
                    enable_indexing: false,
                };
                Self::from_config(config).await
            }
        }
    }

    /// Create TupleSpace from configuration file (FILE - low priority)
    ///
    /// ## Purpose
    /// Creates a TupleSpace instance from a YAML or TOML configuration file.
    /// File format is detected by extension (.yaml, .yml, .toml).
    ///
    /// ## Arguments
    /// * `path` - Path to configuration file
    ///
    /// ## Returns
    /// Configured TupleSpace instance
    ///
    /// ## Errors
    /// - `TupleSpaceError::Configuration`: File not found or invalid format
    /// - `TupleSpaceError::StorageError`: Backend connection failure
    ///
    /// ## Examples
    ///
    /// **config/tuplespace.yaml**:
    /// ```yaml
    /// backend:
    ///   sqlite:
    ///     path: /tmp/tuples.db
    /// pool_size: 1
    /// default_ttl_seconds: 3600
    /// enable_indexing: true
    /// ```
    ///
    /// **config/tuplespace.toml**:
    /// ```toml
    /// pool_size = 1
    /// default_ttl_seconds = 3600
    /// enable_indexing = true
    ///
    /// [backend.sqlite]
    /// path = "/tmp/tuples.db"
    /// ```
    ///
    /// ```rust
    /// use plexspaces_tuplespace::TupleSpace;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let space = TupleSpace::from_file("config/tuplespace.yaml").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_file(path: &str) -> Result<Self, TupleSpaceError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            TupleSpaceError::InvalidConfiguration(format!(
                "Failed to read config file '{}': {}",
                path, e
            ))
        })?;

        // Parse file to JSON value first (intermediate format)
        let json_value: serde_json::Value = if path.ends_with(".toml") {
            // TOML -> Value -> JSON
            let toml_value: toml::Value = toml::from_str(&content).map_err(|e| {
                TupleSpaceError::InvalidConfiguration(format!("Failed to parse TOML config: {}", e))
            })?;
            serde_json::to_value(toml_value).map_err(|e| {
                TupleSpaceError::InvalidConfiguration(format!(
                    "Failed to convert TOML to JSON: {}",
                    e
                ))
            })?
        } else if path.ends_with(".yaml") || path.ends_with(".yml") {
            // YAML -> JSON
            serde_yaml::from_str(&content).map_err(|e| {
                TupleSpaceError::InvalidConfiguration(format!("Failed to parse YAML config: {}", e))
            })?
        } else {
            return Err(TupleSpaceError::InvalidConfiguration(format!(
                "Unsupported config file format: {}. Use .yaml, .yml, or .toml",
                path
            )));
        };

        // Parse JSON value into protobuf TupleSpaceConfig
        // Note: We construct manually because proto types don't implement Serde
        let config = parse_config_from_json(&json_value)?;

        Self::from_config(config).await
    }

    /// Create TupleSpace with smart defaults (Multi-source - fallback)
    ///
    /// ## Purpose
    /// Tries environment variables first, falls back to in-memory default.
    /// This is the recommended method for applications that want flexibility.
    ///
    /// ## Configuration Priority
    /// 1. Environment variables (if `PLEXSPACES_TUPLESPACE_BACKEND` is set)
    /// 2. In-memory default (if no env vars)
    ///
    /// ## Returns
    /// Configured TupleSpace instance (never fails, uses in-memory as last resort)
    ///
    /// ## Examples
    /// ```rust
    /// use plexspaces_tuplespace::TupleSpace;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Uses env vars if available, otherwise in-memory
    /// let space = TupleSpace::from_env_or_default().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_env_or_default() -> Result<Self, TupleSpaceError> {
        // Try env first
        if std::env::var("PLEXSPACES_TUPLESPACE_BACKEND").is_ok() {
            Self::from_env().await
        } else {
            // Fall back to in-memory default with internal tenant/namespace
            // Note: Can't use RequestContext here due to circular dependency (core -> tuplespace)
            Ok(Self::with_tenant_namespace("internal", "system"))
        }
    }
}

/// Helper function to parse JSON value into TupleSpaceConfig
///
/// Manually constructs TupleSpaceConfig from serde_json::Value since
/// protobuf-generated types don't implement Serde by default.
fn parse_config_from_json(json: &serde_json::Value) -> Result<TupleSpaceConfig, TupleSpaceError> {
    let obj = json.as_object().ok_or_else(|| {
        TupleSpaceError::InvalidConfiguration("Config must be a JSON object".to_string())
    })?;

    let pool_size = obj.get("pool_size").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    let default_ttl_seconds = obj
        .get("default_ttl_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let enable_indexing = obj
        .get("enable_indexing")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Parse backend (required)
    let backend_obj = obj
        .get("backend")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            TupleSpaceError::InvalidConfiguration("Missing 'backend' field in config".to_string())
        })?;

    let backend = if backend_obj.contains_key("in_memory") || backend_obj.contains_key("in-memory")
    {
        Some(Backend::InMemory(InMemoryBackend {}))
    } else if let Some(sqlite_obj) = backend_obj.get("sqlite").and_then(|v| v.as_object()) {
        let path = sqlite_obj
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                TupleSpaceError::InvalidConfiguration(
                    "SQLite backend requires 'path' field".to_string(),
                )
            })?
            .to_string();
        Some(Backend::Sqlite(SqliteBackend { path }))
    } else if let Some(redis_obj) = backend_obj.get("redis").and_then(|v| v.as_object()) {
        let url = redis_obj
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                TupleSpaceError::InvalidConfiguration(
                    "Redis backend requires 'url' field".to_string(),
                )
            })?
            .to_string();
        let namespace = redis_obj
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("plexspaces")
            .to_string();
        Some(Backend::Redis(RedisBackend { url, namespace }))
    } else if let Some(postgres_obj) = backend_obj.get("postgres").and_then(|v| v.as_object()) {
        let connection_string = postgres_obj
            .get("connection_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                TupleSpaceError::InvalidConfiguration(
                    "PostgreSQL backend requires 'connection_string' field".to_string(),
                )
            })?
            .to_string();
        let table_name = postgres_obj
            .get("table_name")
            .and_then(|v| v.as_str())
            .unwrap_or("tuples")
            .to_string();
        Some(Backend::Postgres(PostgresBackend {
            connection_string,
            table_name,
        }))
    } else {
        return Err(TupleSpaceError::InvalidConfiguration(
            "Unknown backend type in config. Use 'in_memory', 'sqlite', 'redis', or 'postgres'"
                .to_string(),
        ));
    };

    Ok(TupleSpaceConfig {
        backend,
        pool_size,
        default_ttl_seconds,
        enable_indexing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_from_config_in_memory() {
        let config = TupleSpaceConfig {
            backend: Some(Backend::InMemory(InMemoryBackend {})),
            pool_size: 0,
            default_ttl_seconds: 0,
            enable_indexing: false,
        };

        let space = TupleSpace::from_config(config).await.unwrap();
        // Should create in-memory TupleSpace successfully
        drop(space);
    }

    #[tokio::test]
    async fn test_from_env_or_default_without_env() {
        // Clean up any env vars from other tests first (before setting)
        std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
        std::env::remove_var("PLEXSPACES_SQLITE_PATH");
        std::env::remove_var("PLEXSPACES_REDIS_URL");
        std::env::remove_var("PLEXSPACES_POSTGRES_URL");
        std::env::remove_var("PLEXSPACES_POSTGRES_TABLE");
        std::env::remove_var("PLEXSPACES_POOL_SIZE");
        std::env::remove_var("PLEXSPACES_REDIS_NAMESPACE");
        
        // Small delay to ensure env vars are cleared
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let space = TupleSpace::from_env_or_default().await.unwrap();
        drop(space);
        
        // Clean up after test
        std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
    }

    #[tokio::test]
    async fn test_from_env_in_memory() {
        // Clean up any env vars from other tests first (before setting)
        std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
        std::env::remove_var("PLEXSPACES_SQLITE_PATH");
        std::env::remove_var("PLEXSPACES_REDIS_URL");
        std::env::remove_var("PLEXSPACES_POSTGRES_URL");
        std::env::remove_var("PLEXSPACES_POSTGRES_TABLE");
        std::env::remove_var("PLEXSPACES_POOL_SIZE");
        
        // Small delay to ensure env vars are cleared
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        std::env::set_var("PLEXSPACES_TUPLESPACE_BACKEND", "in-memory");

        let space = TupleSpace::from_env().await.unwrap();
        drop(space);

        // Clean up after test
        std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
    }

    #[tokio::test]
    #[cfg(feature = "sql-backend")]
    async fn test_from_config_sqlite() {
        let config = TupleSpaceConfig {
            backend: Some(Backend::Sqlite(SqliteBackend {
                path: ":memory:".to_string(),
            })),
            pool_size: 1,
            default_ttl_seconds: 0,
            enable_indexing: false,
        };

        let space = TupleSpace::from_config(config).await.unwrap();
        drop(space);
    }

    #[tokio::test]
    #[cfg(feature = "sql-backend")]
    async fn test_from_env_sqlite() {
        // Use a mutex to ensure test isolation (prevents race conditions with env vars)
        use std::sync::Mutex;
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();
        
        // Clean up first to avoid interference
        std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
        std::env::remove_var("PLEXSPACES_SQLITE_PATH");
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        std::env::set_var("PLEXSPACES_TUPLESPACE_BACKEND", "sqlite");
        std::env::set_var("PLEXSPACES_SQLITE_PATH", ":memory:");

        let space = TupleSpace::from_env().await.unwrap();
        drop(space);

        // Clean up after test
        std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
        std::env::remove_var("PLEXSPACES_SQLITE_PATH");
    }

    #[tokio::test]
    async fn test_from_env_sqlite_missing_path() {
        // Clean up any env vars from other tests first (before setting)
        std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
        std::env::remove_var("PLEXSPACES_SQLITE_PATH");
        std::env::remove_var("PLEXSPACES_REDIS_URL");
        std::env::remove_var("PLEXSPACES_POSTGRES_URL");
        
        // Small delay to ensure env vars are cleared
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        std::env::set_var("PLEXSPACES_TUPLESPACE_BACKEND", "sqlite");
        // Ensure SQLITE_PATH is not set
        std::env::remove_var("PLEXSPACES_SQLITE_PATH");

        let result = TupleSpace::from_env().await;
        assert!(result.is_err(), "Should fail when SQLite backend is specified but path is missing");
        if let Err(e) = result {
            assert!(e.to_string().contains("PLEXSPACES_SQLITE_PATH"), "Error should mention PLEXSPACES_SQLITE_PATH");
        }

        // Clean up after test
        std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
    }
}
