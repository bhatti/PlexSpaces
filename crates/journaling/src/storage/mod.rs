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

//! Journal storage backends
//!
//! ## Purpose
//! Defines the `JournalStorage` trait and provides multiple backend implementations
//! for persisting journal entries and checkpoints.
//!
//! ## Design Pattern
//! Following the same pattern as TupleSpaceStorage, this module provides:
//! - Trait abstraction for backend-agnostic journal operations
//! - Multiple implementations (Memory, PostgreSQL, Redis, SQLite)
//! - Feature-gated backends for minimal dependencies
//!
//! ## Backends
//! - [`MemoryJournalStorage`]: In-memory HashMap (testing only)
//! - [`PostgresJournalStorage`]: PostgreSQL with auto-migrations (production)
//! - [`RedisJournalStorage`]: Redis (distributed, eventually consistent)
//! - [`SqliteJournalStorage`]: SQLite with WAL mode (edge deployments)

use plexspaces_core::{JournalError, JournalResult};
use std::sync::Arc;

// Re-export reminder types from proto
pub use plexspaces_proto::timer::v1::{ReminderRegistration, ReminderState};

// Re-export JournalStorage trait from core to avoid duplication
pub use plexspaces_core::JournalStorage;

// The JournalStorage trait is now defined in plexspaces-core to avoid circular dependencies
// All implementations in this crate implement the trait from core

// Backend implementations
mod memory;
pub use memory::MemoryJournalStorage;

#[cfg(any(feature = "postgres-backend", feature = "sqlite-backend"))]
pub mod sql;

#[cfg(feature = "sqlite-backend")]
pub use sql::SqliteJournalStorage;

#[cfg(feature = "postgres-backend")]
pub use sql::PostgresJournalStorage;

#[cfg(feature = "redis-backend")]
mod redis;
#[cfg(feature = "redis-backend")]
pub use redis::RedisJournalStorage;

#[cfg(feature = "ddb-backend")]
mod ddb;
#[cfg(feature = "ddb-backend")]
pub use ddb::DynamoDBJournalStorage;

/// Create journal storage backend from DurabilityConfig
///
/// ## Purpose
/// Factory function that creates appropriate journal storage backend based on
/// JournalBackend enum from DurabilityConfig, following the same pattern as
/// `create_channel` and `create_storage`.
///
/// ## Arguments
/// - `config`: DurabilityConfig from proto
///
/// ## Returns
/// Arc<dyn JournalStorage> for use with DurabilityFacet
///
/// ## Errors
/// - `JournalError::Storage` if backend creation fails
/// - `JournalError::InvalidConfiguration` if config is malformed
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_journaling::*;
/// use plexspaces_proto::v1::journaling::{DurabilityConfig, JournalBackend};
///
/// # async fn example() -> JournalResult<()> {
/// let config = DurabilityConfig {
///     backend: JournalBackend::JournalBackendSqlite as i32,
///     checkpoint_interval: 100,
///     ..Default::default()
/// };
/// let storage = create_journal_storage(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_journal_storage(
    config: plexspaces_proto::v1::journaling::DurabilityConfig,
) -> JournalResult<Arc<dyn JournalStorage>> {
    use plexspaces_proto::v1::journaling::JournalBackend;
    
    let backend = JournalBackend::try_from(config.backend).map_err(|_| {
        JournalError::InvalidConfiguration(format!(
            "Invalid journal backend: {}",
            config.backend
        ))
    })?;
    
    match backend {
        JournalBackend::JournalBackendMemory => {
            let storage = MemoryJournalStorage::new();
            Ok(Arc::new(storage))
        }
        #[cfg(feature = "sqlite-backend")]
        JournalBackend::JournalBackendSqlite => {
            // Extract SQLite config from backend_config
            let path = if let Some(backend_config) = &config.backend_config {
                match backend_config {
                    plexspaces_proto::v1::journaling::durability_config::BackendConfig::Sqlite(sqlite_config) => {
                        sqlite_config.db_path.clone()
                    }
                    _ => ":memory:".to_string(),
                }
            } else {
                ":memory:".to_string()
            };
            let storage = SqliteJournalStorage::new(&path).await?;
            Ok(Arc::new(storage))
        }
        #[cfg(not(feature = "sqlite-backend"))]
        JournalBackend::JournalBackendSqlite => Err(JournalError::InvalidConfiguration(
            "SQLite backend not enabled. Enable 'sqlite-backend' feature.".to_string(),
        )),
        #[cfg(feature = "postgres-backend")]
        JournalBackend::JournalBackendPostgres => {
            // Extract PostgreSQL config from backend_config
            let connection_string = if let Some(backend_config) = &config.backend_config {
                match backend_config {
                    plexspaces_proto::v1::journaling::durability_config::BackendConfig::Postgres(postgres_config) => {
                        postgres_config.connection_string.clone()
                    }
                    _ => {
                        return Err(JournalError::InvalidConfiguration(
                            "Missing PostgreSQL connection string".to_string(),
                        ));
                    }
                }
            } else {
                return Err(JournalError::InvalidConfiguration(
                    "Missing PostgreSQL connection string".to_string(),
                ));
            };
            let storage = PostgresJournalStorage::new(&connection_string).await?;
            Ok(Arc::new(storage))
        }
        #[cfg(not(feature = "postgres-backend"))]
        JournalBackend::JournalBackendPostgres => Err(JournalError::InvalidConfiguration(
            "PostgreSQL backend not enabled. Enable 'postgres-backend' feature.".to_string(),
        )),
        #[cfg(feature = "redis-backend")]
        JournalBackend::JournalBackendRedis => Err(JournalError::InvalidConfiguration(
            "Redis backend is not yet implemented".to_string(),
        )),
        #[cfg(not(feature = "redis-backend"))]
        JournalBackend::JournalBackendRedis => Err(JournalError::InvalidConfiguration(
            "Redis backend not enabled. Enable 'redis-backend' feature.".to_string(),
        )),
        JournalBackend::JournalBackendUnspecified => Err(JournalError::InvalidConfiguration(
            "Journal backend not specified".to_string(),
        )),
    }
}
