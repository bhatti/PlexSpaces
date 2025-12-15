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

//! TupleSpace Storage Backend Abstraction
//!
//! ## Purpose
//! Defines the storage backend trait that enables pluggable persistence
//! for TupleSpace operations. This follows Proto-First Design - all behavior
//! defined in tuplespace_storage.proto.
//!
//! ## Architecture Context
//! Part of **Pillar 1 (TupleSpace - Linda Model)**. Storage abstraction enables:
//! - **Development**: In-memory (fast, no setup)
//! - **Production**: Redis/PostgreSQL (durable, distributed)
//! - **Edge**: SQLite (embedded, low-resource)
//!
//! ## Storage Providers
//! All providers defined in proto/plexspaces/v1/tuplespace_storage.proto:
//! - `MEMORY`: In-memory HashMap (testing/development)
//! - `REDIS`: Redis with pub/sub (distributed production)
//! - `POSTGRES`: PostgreSQL with JSONB (ACID transactions)
//! - `SQLITE`: SQLite embedded (edge deployments)
//!
//! ## Design Decisions
//! - **Proto-First**: StorageProvider enum and configs defined in proto
//! - **Async**: All operations async for I/O efficiency
//! - **Pattern Matching**: Exact, wildcard, type-based (from proto)
//! - **Leases**: TTL-based automatic expiry with renewal (from proto)
//! - **Transactions**: Optional transaction support for consistency
//!
//! ## Usage
//! ```rust
//! use plexspaces_tuplespace::storage::*;
//! use plexspaces_proto::v1::*;
//!
//! # async fn example() -> Result<(), TupleSpaceError> {
//! // Create storage from proto config
//! let config = TupleSpaceStorageConfig {
//!     provider: StorageProvider::StorageProviderMemory as i32,
//!     config: Some(tuplespace_storage_config::Config::Memory(
//!         MemoryStorageConfig {
//!             initial_capacity: 1000,
//!             cleanup_interval_ms: 60000,
//!         }
//!     )),
//!     enable_metrics: true,
//!     cleanup_interval: None,
//! };
//!
//! let storage = create_storage(config).await?;
//!
//! // Write tuple
//! let tuple = Tuple { /* ... */ };
//! let tuple_id = storage.write(tuple).await?;
//!
//! // Read tuple
//! let pattern = Pattern { /* ... */ };
//! let tuples = storage.read(pattern, None).await?;
//! # Ok(())
//! # }
//! ```

pub mod memory;

#[cfg(feature = "redis-backend")]
pub mod redis;

#[cfg(feature = "sql-backend")]
pub mod sql;

use async_trait::async_trait;
use plexspaces_proto::tuplespace::v1::{
    MemoryStorageConfig, StorageProvider, StorageStats, TupleSpaceStorageConfig,
};

#[allow(unused_imports)]
use plexspaces_proto::tuplespace::v1::{PostgresStorageConfig, SqliteStorageConfig};

use crate::{Pattern, Tuple, TupleSpaceError};
use std::time::Duration;

// Re-export for serialization
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

/// Watch event message for distributed notifications
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WatchEventMessage {
    /// Event type ("Added" or "Removed")
    pub event_type: String,
    /// The tuple that triggered the event
    pub tuple: Tuple,
    /// Pattern that matched (for filtering) - serialized as JSON string
    /// Note: Pattern with Predicate variants cannot be serialized, so we use JSON string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern_json: Option<String>,
}

/// TupleSpace storage backend trait
///
/// ## Purpose
/// Defines the contract that all storage backends must implement.
/// Follows Linda model operations: write, read, take, count.
///
/// ## Implementation Notes
/// - All methods are async for I/O efficiency
/// - Pattern matching logic handled by storage backend
/// - Lease management (TTL, renewal) handled by storage backend
/// - Transactions are optional (Some backends don't support)
///
/// ## Design from Proto
/// This trait maps directly to TupleSpace operations defined in
/// proto/plexspaces/v1/tuplespace.proto service methods.
#[async_trait]
pub trait TupleSpaceStorage: Send + Sync {
    /// Write tuple to storage
    ///
    /// ## Returns
    /// Tuple ID (UUID) assigned by storage
    ///
    /// ## Errors
    /// - StorageError if write fails
    /// - SerializationError if tuple can't be serialized
    async fn write(&self, tuple: Tuple) -> Result<String, TupleSpaceError>;

    /// Write multiple tuples atomically (if supported)
    ///
    /// ## Returns
    /// Vector of tuple IDs
    ///
    /// ## Notes
    /// - Some backends (PostgreSQL, Redis) support atomic writes
    /// - Others (Memory, SQLite) may write sequentially
    async fn write_batch(&self, tuples: Vec<Tuple>) -> Result<Vec<String>, TupleSpaceError>;

    /// Read tuples matching pattern (non-destructive)
    ///
    /// ## Arguments
    /// - `pattern`: PatternField-based matching (exact, wildcard, type)
    /// - `timeout`: Optional blocking timeout (None = immediate)
    ///
    /// ## Returns
    /// Vector of matching tuples (may be empty if no match)
    ///
    /// ## Blocking Behavior
    /// - If timeout is Some and no tuples match, block until:
    ///   - A matching tuple appears, OR
    ///   - Timeout expires
    /// - If timeout is None, return immediately
    async fn read(
        &self,
        pattern: Pattern,
        timeout: Option<Duration>,
    ) -> Result<Vec<Tuple>, TupleSpaceError>;

    /// Take tuples matching pattern (destructive read)
    ///
    /// ## Arguments
    /// - `pattern`: PatternField-based matching
    /// - `timeout`: Optional blocking timeout
    ///
    /// ## Returns
    /// Vector of matching tuples (removed from storage)
    ///
    /// ## Notes
    /// - This is atomic: read + remove in single operation
    /// - Blocks same as read() if timeout specified
    async fn take(
        &self,
        pattern: Pattern,
        timeout: Option<Duration>,
    ) -> Result<Vec<Tuple>, TupleSpaceError>;

    /// Count tuples matching pattern
    ///
    /// ## Returns
    /// Number of tuples matching pattern
    async fn count(&self, pattern: Pattern) -> Result<usize, TupleSpaceError>;

    /// Check if any tuples match pattern
    ///
    /// ## Returns
    /// true if at least one tuple matches
    async fn exists(&self, pattern: Pattern) -> Result<bool, TupleSpaceError>;

    /// Renew lease for tuple (if renewable)
    ///
    /// ## Arguments
    /// - `tuple_id`: ID of tuple to renew
    /// - `new_ttl`: Optional new TTL (if None, use original)
    ///
    /// ## Returns
    /// New expiration timestamp
    ///
    /// ## Errors
    /// - TupleNotFound if ID doesn't exist
    /// - LeaseNotRenewable if tuple's lease has renewable=false
    async fn renew_lease(
        &self,
        tuple_id: &str,
        new_ttl: Option<Duration>,
    ) -> Result<chrono::DateTime<chrono::Utc>, TupleSpaceError>;

    /// Clear all tuples from storage
    async fn clear(&self) -> Result<(), TupleSpaceError>;

    /// Get storage statistics (from proto StorageStats)
    async fn stats(&self) -> Result<StorageStats, TupleSpaceError>;

    /// Begin transaction (if supported)
    ///
    /// ## Returns
    /// Transaction ID
    ///
    /// ## Errors
    /// - NotSupported if backend doesn't support transactions
    async fn begin_transaction(&self) -> Result<String, TupleSpaceError> {
        Err(TupleSpaceError::NotSupported(
            "Transactions not supported by this storage backend".to_string(),
        ))
    }

    /// Commit transaction
    async fn commit_transaction(&self, _tx_id: &str) -> Result<(), TupleSpaceError> {
        Err(TupleSpaceError::NotSupported(
            "Transactions not supported by this storage backend".to_string(),
        ))
    }

    /// Abort transaction
    async fn abort_transaction(&self, _tx_id: &str) -> Result<(), TupleSpaceError> {
        Err(TupleSpaceError::NotSupported(
            "Transactions not supported by this storage backend".to_string(),
        ))
    }

    /// Publish watch event for distributed watch notifications (optional)
    ///
    /// ## Purpose
    /// Allows storage backends to publish watch events for cross-node watch support.
    /// If not implemented, watch events are local-only.
    ///
    /// ## Arguments
    /// * `event_type` - Type of event ("Added" or "Removed")
    /// * `tuple` - The tuple that triggered the event
    /// * `namespace` - Namespace for channel scoping
    ///
    /// ## Returns
    /// Success or error (NotSupported if backend doesn't support pub/sub)
    async fn publish_watch_event(
        &self,
        _event_type: &str,
        _tuple: &Tuple,
        _namespace: &str,
    ) -> Result<(), TupleSpaceError> {
        // Default: no-op (local watchers only)
        Ok(())
    }

    /// Subscribe to watch events for distributed watch notifications (optional)
    ///
    /// ## Purpose
    /// Allows storage backends to subscribe to watch events from other nodes.
    /// Returns a receiver that yields watch events.
    ///
    /// ## Arguments
    /// * `namespace` - Namespace for channel scoping
    ///
    /// ## Returns
    /// Receiver for watch events, or NotSupported if backend doesn't support pub/sub
    async fn subscribe_watch_events(
        &self,
        _namespace: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<WatchEventMessage>, TupleSpaceError> {
        Err(TupleSpaceError::NotSupported(
            "Watch event subscription not supported by this storage backend".to_string(),
        ))
    }
}

/// Create storage backend from proto configuration
///
/// ## Purpose
/// Factory function that creates appropriate storage backend based on
/// StorageProvider enum from proto.
///
/// ## Arguments
/// - `config`: TupleSpaceStorageConfig from proto
///
/// ## Returns
/// Boxed storage backend implementing TupleSpaceStorage trait
///
/// ## Errors
/// - InvalidConfiguration if config is malformed
/// - ConnectionError if backend can't connect (Redis, PostgreSQL)
///
/// ## Example
/// ```rust
/// use plexspaces_tuplespace::storage::*;
/// use plexspaces_proto::v1::*;
///
/// # async fn example() -> Result<(), TupleSpaceError> {
/// let config = TupleSpaceStorageConfig {
///     provider: StorageProvider::StorageProviderMemory as i32,
///     config: Some(tuplespace_storage_config::Config::Memory(
///         MemoryStorageConfig {
///             initial_capacity: 1000,
///             cleanup_interval_ms: 60000,
///         }
///     )),
///     enable_metrics: true,
///     cleanup_interval: None,
/// };
///
/// let storage = create_storage(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_storage(
    config: TupleSpaceStorageConfig,
) -> Result<Box<dyn TupleSpaceStorage>, TupleSpaceError> {
    let provider = StorageProvider::try_from(config.provider).map_err(|_| {
        TupleSpaceError::InvalidConfiguration(format!(
            "Invalid storage provider: {}",
            config.provider
        ))
    })?;

    match provider {
        StorageProvider::StorageProviderUnspecified => Err(TupleSpaceError::InvalidConfiguration(
            "Storage provider not specified".to_string(),
        )),
        StorageProvider::StorageProviderMemory => {
            // Extract memory config
            let memory_config = match config.config {
                Some(plexspaces_proto::tuplespace::v1::tuple_space_storage_config::Config::Memory(cfg)) => cfg,
                _ => MemoryStorageConfig {
                    initial_capacity: 1000,
                    cleanup_interval_ms: 60000,
                },
            };

            // Create memory storage
            Ok(Box::new(memory::MemoryStorage::new(memory_config)))
        }
        StorageProvider::StorageProviderRedis => {
            #[cfg(feature = "redis-backend")]
            {
                // Extract redis config
                let redis_config = match config.config {
                    Some(plexspaces_proto::tuplespace::v1::tuple_space_storage_config::Config::Redis(cfg)) => {
                        cfg
                    }
                    _ => {
                        return Err(TupleSpaceError::InvalidConfiguration(
                            "Redis config required for Redis storage".to_string(),
                        ));
                    }
                };

                // Create Redis storage
                let storage = redis::RedisStorage::new(redis_config).await?;
                Ok(Box::new(storage))
            }

            #[cfg(not(feature = "redis-backend"))]
            {
                Err(TupleSpaceError::NotSupported(
                    "Redis storage requires 'redis-backend' feature to be enabled".to_string(),
                ))
            }
        }
        StorageProvider::StorageProviderPostgres => {
            #[cfg(feature = "sql-backend")]
            {
                // Extract postgres config
                let postgres_config = match config.config {
                    Some(plexspaces_proto::tuplespace::v1::tuple_space_storage_config::Config::Postgres(
                        cfg,
                    )) => cfg,
                    _ => {
                        return Err(TupleSpaceError::InvalidConfiguration(
                            "PostgreSQL config required for Postgres storage".to_string(),
                        ));
                    }
                };

                // Create PostgreSQL storage
                let storage = sql::SqlStorage::new_postgres(postgres_config).await?;
                Ok(Box::new(storage))
            }

            #[cfg(not(feature = "sql-backend"))]
            {
                Err(TupleSpaceError::NotSupported(
                    "PostgreSQL storage requires 'sql-backend' feature to be enabled".to_string(),
                ))
            }
        }
        StorageProvider::StorageProviderSqlite => {
            #[cfg(feature = "sql-backend")]
            {
                // Extract sqlite config
                let sqlite_config = match config.config {
                    Some(plexspaces_proto::tuplespace::v1::tuple_space_storage_config::Config::Sqlite(cfg)) => {
                        cfg
                    }
                    _ => {
                        return Err(TupleSpaceError::InvalidConfiguration(
                            "SQLite config required for SQLite storage".to_string(),
                        ));
                    }
                };

                // Create SQLite storage
                let storage = sql::SqlStorage::new_sqlite(sqlite_config).await?;
                Ok(Box::new(storage))
            }

            #[cfg(not(feature = "sql-backend"))]
            {
                Err(TupleSpaceError::NotSupported(
                    "SQLite storage requires 'sql-backend' feature to be enabled".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_storage_unspecified() {
        let config = TupleSpaceStorageConfig {
            provider: StorageProvider::StorageProviderUnspecified as i32,
            config: None,
            enable_metrics: false,
            cleanup_interval: None,
        };

        let result = create_storage(config).await;
        assert!(result.is_err());
        match result {
            Err(TupleSpaceError::InvalidConfiguration(_)) => {} // Expected
            _ => panic!("Expected InvalidConfiguration error"),
        }
    }

    #[tokio::test]
    async fn test_create_storage_memory() {
        let config = TupleSpaceStorageConfig {
            provider: StorageProvider::StorageProviderMemory as i32,
            config: Some(
                plexspaces_proto::tuplespace::v1::tuple_space_storage_config::Config::Memory(
                    MemoryStorageConfig {
                        initial_capacity: 1000,
                        cleanup_interval_ms: 60000,
                    },
                ),
            ),
            enable_metrics: false,
            cleanup_interval: None,
        };

        let result = create_storage(config).await;
        assert!(result.is_ok(), "Memory storage should be implemented");
    }

    #[tokio::test]
    async fn test_create_storage_memory_with_defaults() {
        // Test memory storage with missing config (should use defaults)
        let config = TupleSpaceStorageConfig {
            provider: StorageProvider::StorageProviderMemory as i32,
            config: None, // Will use default memory config
            enable_metrics: false,
            cleanup_interval: None,
        };

        let result = create_storage(config).await;
        assert!(
            result.is_ok(),
            "Memory storage should use defaults when config missing"
        );
    }

    #[cfg(feature = "sql-backend")]
    #[tokio::test]
    async fn test_create_storage_sqlite() {
        use plexspaces_proto::tuplespace::v1::{tuple_space_storage_config, SqliteStorageConfig};

        let config = TupleSpaceStorageConfig {
            provider: StorageProvider::StorageProviderSqlite as i32,
            config: Some(tuple_space_storage_config::Config::Sqlite(
                SqliteStorageConfig {
                    database_path: ":memory:".to_string(),
                    enable_wal: false,
                    cache_size_kb: 2000,
                },
            )),
            enable_metrics: false,
            cleanup_interval: None,
        };

        let result = create_storage(config).await;
        assert!(result.is_ok(), "SQLite storage should be created");
    }

    #[cfg(feature = "sql-backend")]
    #[tokio::test]
    async fn test_create_storage_sqlite_missing_config() {
        let config = TupleSpaceStorageConfig {
            provider: StorageProvider::StorageProviderSqlite as i32,
            config: None, // Missing required config
            enable_metrics: false,
            cleanup_interval: None,
        };

        let result = create_storage(config).await;
        assert!(result.is_err());
        match result {
            Err(TupleSpaceError::InvalidConfiguration(_)) => {} // Expected
            _ => panic!("Expected InvalidConfiguration error"),
        }
    }

    #[tokio::test]
    async fn test_create_storage_invalid_provider() {
        let config = TupleSpaceStorageConfig {
            provider: 9999, // Invalid provider value
            config: None,
            enable_metrics: false,
            cleanup_interval: None,
        };

        let result = create_storage(config).await;
        assert!(result.is_err());
        match result {
            Err(TupleSpaceError::InvalidConfiguration(_)) => {} // Expected
            _ => panic!("Expected InvalidConfiguration error"),
        }
    }

    #[cfg(not(feature = "redis-backend"))]
    #[tokio::test]
    async fn test_create_storage_redis_not_enabled() {
        use plexspaces_proto::tuplespace::v1::RedisStorageConfig;

        let config = TupleSpaceStorageConfig {
            provider: StorageProvider::StorageProviderRedis as i32,
            config: Some(
                plexspaces_proto::tuplespace::v1::tuple_space_storage_config::Config::Redis(
                    RedisStorageConfig {
                        connection_string: "redis://localhost".to_string(),
                        pool_size: 10,
                        key_prefix: "test".to_string(),
                        enable_pubsub: false,
                    },
                ),
            ),
            enable_metrics: false,
            cleanup_interval: None,
        };

        let result = create_storage(config).await;
        assert!(result.is_err());
        match result {
            Err(TupleSpaceError::NotSupported(_)) => {} // Expected
            _ => panic!("Expected NotSupported error"),
        }
    }

    #[cfg(not(feature = "sql-backend"))]
    #[tokio::test]
    async fn test_create_storage_postgres_not_enabled() {
        use plexspaces_proto::tuplespace::v1::PostgresStorageConfig;

        let config = TupleSpaceStorageConfig {
            provider: StorageProvider::StorageProviderPostgres as i32,
            config: Some(
                plexspaces_proto::tuplespace::v1::tuple_space_storage_config::Config::Postgres(
                    PostgresStorageConfig {
                        connection_string: "postgres://localhost".to_string(),
                        pool_size: 10,
                        table_name: "tuples".to_string(),
                    },
                ),
            ),
            enable_metrics: false,
            cleanup_interval: None,
        };

        let result = create_storage(config).await;
        assert!(result.is_err());
        match result {
            Err(TupleSpaceError::NotSupported(_)) => {} // Expected
            _ => panic!("Expected NotSupported error"),
        }
    }
}
