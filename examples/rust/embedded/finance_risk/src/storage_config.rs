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

//! Storage configuration for file-based SQLite durable state
//!
//! ## Purpose
//! Provides centralized configuration for SQLite-backed journal storage
//! with file-based persistence for multi-node Docker deployments.
//!
//! ## Design
//! - Uses shared Docker volume `/data/finance` for SQLite files
//! - One database per node for isolation
//! - Configurable checkpoint intervals and compression
//! - Support for both development (local) and production (Docker) modes

use plexspaces_journaling::{
    CompressionType, DurabilityConfig, DurabilityFacet, JournalBackend, JournalResult,
    SqliteJournalStorage,
};
use std::path::PathBuf;
use tracing::{info, warn};

/// Storage configuration for durable state
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Base directory for SQLite files (e.g., "/data/finance" in Docker)
    pub data_dir: PathBuf,

    /// Node identifier (used for database naming)
    pub node_id: String,

    /// Checkpoint interval (number of messages between checkpoints)
    pub checkpoint_interval: u64,

    /// Enable compression for checkpoints
    pub enable_compression: bool,

    /// Replay journal on actor activation
    pub replay_on_activation: bool,

    /// Cache side effects during replay
    pub cache_side_effects: bool,
}

impl StorageConfig {
    /// Create storage configuration from environment variables
    ///
    /// ## Environment Variables
    /// - `FINANCE_DATA_DIR`: Base directory for SQLite files (default: "/data/finance")
    /// - `NODE_ID`: Node identifier (default: "node-1")
    /// - `CHECKPOINT_INTERVAL`: Messages between checkpoints (default: 100)
    /// - `ENABLE_COMPRESSION`: Enable checkpoint compression (default: true)
    /// - `REPLAY_ON_ACTIVATION`: Replay journal on activation (default: true)
    /// - `CACHE_SIDE_EFFECTS`: Cache side effects during replay (default: true)
    pub fn from_env() -> Self {
        let data_dir =
            std::env::var("FINANCE_DATA_DIR").unwrap_or_else(|_| "/data/finance".to_string());

        let node_id = std::env::var("NODE_ID").unwrap_or_else(|_| "node-1".to_string());

        let checkpoint_interval = std::env::var("CHECKPOINT_INTERVAL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);

        let enable_compression = std::env::var("ENABLE_COMPRESSION")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(true);

        let replay_on_activation = std::env::var("REPLAY_ON_ACTIVATION")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(true);

        let cache_side_effects = std::env::var("CACHE_SIDE_EFFECTS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(true);

        Self {
            data_dir: PathBuf::from(data_dir),
            node_id,
            checkpoint_interval,
            enable_compression,
            replay_on_activation,
            cache_side_effects,
        }
    }

    /// Get database file path for this node
    ///
    /// ## Returns
    /// Path like `/data/finance/node-1.db`
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join(format!("{}.db", self.node_id))
    }

    /// Create DurabilityConfig for journal storage
    pub fn durability_config(&self) -> DurabilityConfig {
        DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: self.checkpoint_interval,
            checkpoint_timeout: None,
            replay_on_activation: self.replay_on_activation,
            cache_side_effects: self.cache_side_effects,
            compression: if self.enable_compression {
                CompressionType::CompressionTypeZstd as i32
            } else {
                CompressionType::CompressionTypeNone as i32
            },
            state_schema_version: 1,
            backend_config: None,
        }
    }

    /// Initialize storage directory (create if needed)
    pub async fn initialize(&self) -> JournalResult<()> {
        // Create data directory if it doesn't exist
        if !self.data_dir.exists() {
            info!("Creating data directory: {:?}", self.data_dir);
            std::fs::create_dir_all(&self.data_dir).map_err(|e| {
                plexspaces_journaling::JournalError::Storage(format!(
                    "Failed to create data directory: {}",
                    e
                ))
            })?;
        }

        let db_path = self.database_path();
        info!("Using SQLite database: {:?}", db_path);

        // Check if database exists
        if db_path.exists() {
            info!("Database already exists, will replay journal on activation");
        } else {
            info!("Creating new database");
        }

        Ok(())
    }

    /// Create SQLite journal storage instance
    pub async fn create_storage(&self) -> JournalResult<SqliteJournalStorage> {
        let db_path = self.database_path();
        let db_path_str = db_path.to_string_lossy();

        info!("Initializing SQLite journal storage: {}", db_path_str);
        SqliteJournalStorage::new(&db_path_str).await
    }

    /// Create DurabilityFacet for an actor
    ///
    /// ## Arguments
    /// - `actor_type`: Type of actor (for logging)
    ///
    /// ## Returns
    /// DurabilityFacet configured with SQLite backend
    pub async fn create_durability_facet(
        &self,
        actor_type: &str,
    ) -> JournalResult<DurabilityFacet<SqliteJournalStorage>> {
        let storage = self.create_storage().await?;
        let config = self.durability_config();

        info!(
            "Creating DurabilityFacet for {} (checkpoint_interval={}, compression={}, replay={})",
            actor_type,
            config.checkpoint_interval,
            self.enable_compression,
            config.replay_on_activation
        );

        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        // Handle checkpoint_timeout separately (Option<Duration> doesn't serialize directly)
        if let Some(ref timeout) = config.checkpoint_timeout {
            config_value["checkpoint_timeout_seconds"] = serde_json::json!(timeout.seconds);
            config_value["checkpoint_timeout_nanos"] = serde_json::json!(timeout.nanos);
        }
        Ok(DurabilityFacet::new(storage, config_value, 50))
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("/data/finance"),
            node_id: "node-1".to_string(),
            checkpoint_interval: 100,
            enable_compression: true,
            replay_on_activation: true,
            cache_side_effects: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_default_config() {
        let config = StorageConfig::default();
        assert_eq!(config.data_dir, PathBuf::from("/data/finance"));
        assert_eq!(config.node_id, "node-1");
        assert_eq!(config.checkpoint_interval, 100);
        assert!(config.enable_compression);
        assert!(config.replay_on_activation);
        assert!(config.cache_side_effects);
    }

    #[test]
    fn test_database_path() {
        let config = StorageConfig {
            data_dir: PathBuf::from("/tmp/test"),
            node_id: "test-node".to_string(),
            ..Default::default()
        };

        assert_eq!(
            config.database_path(),
            PathBuf::from("/tmp/test/test-node.db")
        );
    }

    #[test]
    fn test_from_env() {
        env::set_var("FINANCE_DATA_DIR", "/custom/data");
        env::set_var("NODE_ID", "custom-node");
        env::set_var("CHECKPOINT_INTERVAL", "50");
        env::set_var("ENABLE_COMPRESSION", "false");

        let config = StorageConfig::from_env();

        assert_eq!(config.data_dir, PathBuf::from("/custom/data"));
        assert_eq!(config.node_id, "custom-node");
        assert_eq!(config.checkpoint_interval, 50);
        assert!(!config.enable_compression);

        // Clean up
        env::remove_var("FINANCE_DATA_DIR");
        env::remove_var("NODE_ID");
        env::remove_var("CHECKPOINT_INTERVAL");
        env::remove_var("ENABLE_COMPRESSION");
    }

    #[test]
    fn test_durability_config() {
        let config = StorageConfig::default();
        let durability = config.durability_config();

        assert_eq!(
            durability.backend,
            JournalBackend::JournalBackendSqlite as i32
        );
        assert_eq!(durability.checkpoint_interval, 100);
        assert!(durability.replay_on_activation);
        assert!(durability.cache_side_effects);
        assert_eq!(
            durability.compression,
            CompressionType::CompressionTypeZstd as i32
        );
    }
}
