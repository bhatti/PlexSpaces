// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
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
//! - `SQLITE`: SQLite embedded (use `:memory:` for in-memory)
//! - `REDIS`: Redis with pub/sub (distributed production)
//! - `POSTGRES`: PostgreSQL with JSONB (ACID transactions)
//! - `DYNAMODB`: DynamoDB (AWS managed)
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
//! use plexspaces_tuplespace::TupleSpaceError;
//!
//! # async fn example() -> Result<(), TupleSpaceError> {
//! // Create in-memory storage with config
//! let config = MemoryStorageConfig {
//!     initial_capacity: 1000,
//!     cleanup_interval_ms: 60000,
//! };
//! let storage = MemoryStorage::new(config);
//!
//! // Write tuple
//! use plexspaces_tuplespace::Tuple;
//! let tuple = Tuple::new(vec![]);
//! storage.write(tuple).await?;
//!
//! // Read tuple
//! use plexspaces_tuplespace::Pattern;
//! let pattern = Pattern::new(vec![]);
//! let _tuples = storage.read(pattern, None).await?;
//! # Ok(())
//! # }
//! ```

#[cfg(feature = "redis-backend")]
pub mod redis;

#[cfg(feature = "sql-backend")]
pub mod sql;

#[cfg(feature = "ddb-backend")]
pub mod ddb;

#[cfg(feature = "ddb-backend")]
pub use ddb::DynamoDBStorage;

use async_trait::async_trait;
use plexspaces_proto::tuplespace::v1::StorageStats;

/// Memory storage configuration (used for SQLite :memory: backend)
#[derive(Debug, Clone)]
pub struct MemoryStorageConfig {
    /// Initial capacity for hash maps (affects pre-allocation)
    pub initial_capacity: usize,
    /// Interval for TTL cleanup in milliseconds
    pub cleanup_interval_ms: u64,
}

use crate::{Pattern, Tuple, TupleSpaceError};
use std::time::Duration;


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

/// Create storage backend from shared database URL
///
/// ## Purpose
/// Factory function that creates appropriate storage backend based on
/// the shared database connection string from RuntimeConfig.db.
///
/// ## Arguments
/// - `db_url`: Database connection string (e.g., "sqlite:///path/to/db", "postgres://...")
///
/// ## Returns
/// Boxed storage backend implementing TupleSpaceStorage trait
///
/// ## Errors
/// - InvalidConfiguration if db_url is malformed
/// - ConnectionError if backend can't connect
///
/// ## Example
/// ```rust
/// use plexspaces_tuplespace::storage::*;
/// use plexspaces_tuplespace::TupleSpaceError;
///
/// # async fn example() -> Result<(), TupleSpaceError> {
/// let db_url = "sqlite:///tmp/tuplespace.db";
/// let storage = create_storage(&db_url).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_storage(db_url: &str) -> Result<Box<dyn TupleSpaceStorage>, TupleSpaceError> {
    // Determine backend type from connection string
    if db_url.contains(":memory:")
        || db_url.starts_with("sqlite:")
        || db_url.starts_with("sqlite://")
    {
        #[cfg(feature = "sql-backend")]
        {
            // Extract path from SQLite connection string
            let path = if db_url == ":memory:" || db_url.contains(":memory:") {
                ":memory:".to_string()
            } else if db_url.starts_with("sqlite:///") {
                // Format: "sqlite:///absolute/path" - preserve leading /
                let extracted = db_url
                    .strip_prefix("sqlite:///")
                    .and_then(|s| s.split('?').next())
                    .unwrap_or(db_url);
                format!("/{}", extracted) // Restore leading /
            } else if db_url.starts_with("sqlite://") {
                db_url
                    .strip_prefix("sqlite://")
                    .and_then(|s| s.split('?').next())
                    .unwrap_or(db_url)
                    .to_string()
            } else if db_url.starts_with("sqlite:") {
                db_url
                    .strip_prefix("sqlite:")
                    .and_then(|s| s.split('?').next())
                    .unwrap_or(db_url)
                    .to_string()
            } else {
                return Err(TupleSpaceError::InvalidConfiguration(
                    "Invalid SQLite connection string format".to_string(),
                ));
            };

            // Ensure directory exists for file-based SQLite databases
            if path != ":memory:" && !path.is_empty() {
                if let Some(parent) = std::path::Path::new(&path).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        TupleSpaceError::InvalidConfiguration(format!(
                            "Failed to create database directory '{}': {}",
                            parent.display(),
                            e
                        ))
                    })?;
                }
            }

            use sql::{SqlStorage, SqliteConfig};
            // Create SQLite storage config
            let sqlite_config = SqliteConfig {
                database_path: path,
                enable_wal: true,
                cache_size_kb: 2000,
            };
            let storage = SqlStorage::new_sqlite(sqlite_config).await?;
            Ok(Box::new(storage))
        }
        #[cfg(not(feature = "sql-backend"))]
        {
            Err(TupleSpaceError::NotSupported(
                "SQLite backend requires 'sql-backend' feature".to_string(),
            ))
        }
    } else if db_url.starts_with("postgres://") || db_url.starts_with("postgresql://") {
        #[cfg(feature = "sql-backend")]
        {
            use sql::{PostgresConfig, SqlStorage};
            // Create PostgreSQL storage config
            let postgres_config = PostgresConfig {
                connection_string: db_url.to_string(),
                pool_size: 10, // Default pool size
                table_name: "tuples".to_string(),
            };
            let storage = SqlStorage::new_postgres(postgres_config).await?;
            Ok(Box::new(storage))
        }
        #[cfg(not(feature = "sql-backend"))]
        {
            Err(TupleSpaceError::NotSupported(
                "PostgreSQL backend requires 'sql-backend' feature".to_string(),
            ))
        }
    } else {
        // Fallback to in-memory SQLite for unsupported databases
        tracing::warn!(
            db_url = %db_url,
            "Unsupported database type for tuplespace, using in-memory SQLite fallback"
        );
        #[cfg(feature = "sql-backend")]
        {
            use sql::{SqlStorage, SqliteConfig};
            let sqlite_config = SqliteConfig {
                database_path: ":memory:".to_string(),
                enable_wal: false,
                cache_size_kb: 2000,
            };
            let storage = SqlStorage::new_sqlite(sqlite_config).await?;
            Ok(Box::new(storage))
        }
        #[cfg(not(feature = "sql-backend"))]
        {
            Err(TupleSpaceError::NotSupported(format!(
                "Unsupported database URL: {} (and sql-backend not enabled)",
                db_url
            )))
        }
    }
}

/// Create default in-memory storage using SQLite :memory:
///
/// Convenience function for tests and simple use cases
#[cfg(feature = "sql-backend")]
pub async fn create_memory_storage() -> Result<Box<dyn TupleSpaceStorage>, TupleSpaceError> {
    create_storage(":memory:").await
}

/// Create default in-memory storage - requires sql-backend feature
#[cfg(not(feature = "sql-backend"))]
pub async fn create_memory_storage() -> Result<Box<dyn TupleSpaceStorage>, TupleSpaceError> {
    Err(TupleSpaceError::NotSupported(
        "Memory storage requires 'sql-backend' feature (uses SQLite :memory:)".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "sql-backend")]
    #[tokio::test]
    async fn test_create_storage_memory() {
        let result = create_storage(":memory:").await;
        assert!(
            result.is_ok(),
            "Memory storage (via SQLite :memory:) should work"
        );
    }

    #[cfg(feature = "sql-backend")]
    #[tokio::test]
    async fn test_create_storage_memory_default() {
        // Test default memory storage using helper
        let storage = create_memory_storage().await.unwrap();
        // Verify storage is working by counting with empty pattern
        let pattern = crate::Pattern::new(vec![]);
        assert!(storage.count(pattern).await.is_ok());
    }

    #[cfg(feature = "sql-backend")]
    #[tokio::test]
    async fn test_create_storage_sqlite() {
        let result = create_storage("sqlite::memory:").await;
        assert!(result.is_ok(), "SQLite storage should be created");
    }
}
