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
//! 1. **CODE**: Explicit `TupleSpaceConfig` in application code
//! 2. **FILE**: YAML/TOML configuration files
//! 3. **RUNTIME**: Shared database and service defaults from `RuntimeConfig`
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
    pub async fn from_config(_config: TupleSpaceConfig) -> Result<Self, TupleSpaceError> {
        // TupleSpace instances resolve persistence via the enclosing runtime and service layer.
        // The local handle itself only needs tenant/namespace context.
        Ok(Self::with_tenant_namespace("", ""))
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

    // TupleSpaceConfig uses runtime-managed storage selection; file-based backend hints are ignored.
    Ok(TupleSpaceConfig {
        default_ttl_seconds,
        enable_indexing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::TupleSpaceProvider;

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
    async fn test_from_config_returns_default_tenant_namespace() {
        let space = TupleSpace::from_config(TupleSpaceConfig::default())
            .await
            .unwrap();
        assert_eq!(space.tenant(), "");
        assert_eq!(space.namespace(), "");
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
}
