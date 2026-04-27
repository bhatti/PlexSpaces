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
//! - [`SqliteJournalStorage`]: SQLite with WAL mode (use `:memory:` for in-memory)
//! - [`PostgresJournalStorage`]: PostgreSQL with auto-migrations (production)
//! - [`RedisJournalStorage`]: Redis (distributed, eventually consistent)
//! - [`DynamoDBJournalStorage`]: DynamoDB (AWS managed)

use plexspaces_common::{resolve_shared_db_backend, SharedDbBackend};
use plexspaces_core::{JournalError, JournalResult};
use plexspaces_proto::storage::v1::SharedDbConfig;
use std::sync::Arc;

// Re-export reminder types from proto
pub use plexspaces_proto::timer::v1::{ReminderRegistration, ReminderState};

// Re-export JournalStorage trait from core to avoid duplication
pub use plexspaces_core::JournalStorage;

// The JournalStorage trait is now defined in plexspaces-core to avoid circular dependencies
// All implementations in this crate implement the trait from core

// Backend implementations
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

/// Create journal storage from shared database config.
///
/// This is the canonical construction path for journal storage in runtime
/// initialization and application setup.
pub async fn create_journal_storage_from_shared_db(
    config: &SharedDbConfig,
) -> JournalResult<Arc<dyn JournalStorage>> {
    match resolve_shared_db_backend(config).map_err(JournalError::InvalidConfiguration)? {
        SharedDbBackend::Postgres { connection_string } => {
            create_journal_storage_from_backend_url(&connection_string).await
        }
        SharedDbBackend::Sqlite { database_path, .. } => {
            create_journal_storage_from_backend_url(&database_path).await
        }
    }
}

async fn create_journal_storage_from_backend_url(
    db_url: &str,
) -> JournalResult<Arc<dyn JournalStorage>> {
    // Determine backend type from connection string or plain path
    // Support both connection strings (sqlite:///path) and plain paths (/path/to/db or ~/path/to/db)
    if db_url.starts_with("postgres://") || db_url.starts_with("postgresql://") {
        #[cfg(feature = "postgres-backend")]
        {
            let storage = PostgresJournalStorage::new(db_url).await?;
            Ok(Arc::new(storage))
        }
        #[cfg(not(feature = "postgres-backend"))]
        {
            Err(JournalError::InvalidConfiguration(
                "PostgreSQL backend requires 'postgres-backend' feature".to_string(),
            ))
        }
    } else if !db_url.is_empty() {
        // SQLite (connection string or plain path)
        #[cfg(feature = "sqlite-backend")]
        {
            // Extract path from SQLite connection string or use plain path
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
                // Plain file path (e.g., /path/to/db or ~/path/to/db)
                // Expand ~ to home directory if needed
                if db_url.starts_with("~/") {
                    let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
                    db_url.replacen("~/", &format!("{}/", home), 1)
                } else {
                    db_url.to_string()
                }
            };

            // Ensure directory exists for file-based SQLite databases
            if path != ":memory:" && !path.is_empty() {
                if let Some(parent) = std::path::Path::new(&path).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        JournalError::InvalidConfiguration(format!(
                            "Failed to create database directory '{}': {}",
                            parent.display(),
                            e
                        ))
                    })?;
                }
            }

            let storage = SqliteJournalStorage::new(&path).await?;
            Ok(Arc::new(storage))
        }
        #[cfg(not(feature = "sqlite-backend"))]
        {
            Err(JournalError::InvalidConfiguration(
                "SQLite backend requires 'sqlite-backend' feature".to_string(),
            ))
        }
    } else {
        // Fallback to in-memory SQLite for empty/unsupported databases
        tracing::warn!(
            db_url = %db_url,
            "Unsupported database type for journaling, using in-memory SQLite fallback"
        );
        #[cfg(feature = "sqlite-backend")]
        {
            let storage = SqliteJournalStorage::new(":memory:").await?;
            Ok(Arc::new(storage))
        }
        #[cfg(not(feature = "sqlite-backend"))]
        {
            Err(JournalError::InvalidConfiguration(format!(
                "Unsupported database URL: {} (and sqlite-backend not enabled)",
                db_url
            )))
        }
    }
}
