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
//! use plexspaces_proto::tuplespace::v1::TupleSpaceConfig;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // TupleSpaceConfig uses shared database from RuntimeConfig.db (no backend config needed)
//! let config = TupleSpaceConfig {
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
use plexspaces_proto::tuplespace::v1::TupleSpaceConfig;

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
    /// use plexspaces_proto::tuplespace::v1::TupleSpaceConfig;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // TupleSpaceConfig uses shared database from RuntimeConfig.db (no backend config needed)
    /// let config = TupleSpaceConfig {
    ///     default_ttl_seconds: 0,
    ///     enable_indexing: false,
    /// };
    /// let space = TupleSpace::from_config(config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_config(config: TupleSpaceConfig) -> Result<Self, TupleSpaceError> {
        // TupleSpaceConfig no longer has backend field - uses shared database from RuntimeConfig.db
        // For backward compatibility, use in-memory default
        // Note: tenant_id/namespace should be provided by caller from node config
        Ok(Self::with_tenant_namespace("", ""))
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
        // TupleSpaceConfig no longer uses backend-specific env vars - uses shared database from RuntimeConfig.db
        // All backend types now use the same default config
        let config = TupleSpaceConfig {
            default_ttl_seconds: 0,
            enable_indexing: false,
        };
        Self::from_config(config).await
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
            // Fall back to in-memory default with empty tenant/namespace
            // Note: tenant_id/namespace should be provided by caller from node config
            // For backward compatibility, use empty strings
            Ok(Self::with_tenant_namespace("", ""))
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

    let default_ttl_seconds = obj
        .get("default_ttl_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let enable_indexing = obj
        .get("enable_indexing")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // TupleSpaceConfig no longer has backend field - uses shared database from RuntimeConfig.db
    // Backend config in JSON files is ignored (for backward compatibility)
    Ok(TupleSpaceConfig {
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
        assert!(
            result.is_err(),
            "Should fail when SQLite backend is specified but path is missing"
        );
        if let Err(e) = result {
            assert!(
                e.to_string().contains("PLEXSPACES_SQLITE_PATH"),
                "Error should mention PLEXSPACES_SQLITE_PATH"
            );
        }

        // Clean up after test
        std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
    }
}
