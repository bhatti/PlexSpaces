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

//! Shared helpers for runtime storage configuration.
//!
//! These helpers keep connection-string parsing and backend classification in one place so
//! storage crates can expose proto-first factories without duplicating URL/path handling.

use plexspaces_proto::storage::v1::SharedDbConfig;

/// Parsed relational backend from [`SharedDbConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedDbBackend {
    /// SQLite backend with both the original connection string and extracted database path.
    Sqlite {
        /// Original configured connection string.
        connection_string: String,
        /// SQLite database path expected by SQLx constructors.
        database_path: String,
    },
    /// PostgreSQL backend with full connection string.
    Postgres {
        /// Original configured connection string.
        connection_string: String,
    },
}

/// Resolve the configured relational backend from a shared DB config.
pub fn resolve_shared_db_backend(config: &SharedDbConfig) -> Result<SharedDbBackend, String> {
    let connection_string = config.connection_string.trim();
    if connection_string.is_empty() {
        return Err("shared database connection_string is required".to_string());
    }

    if connection_string.starts_with("postgres://")
        || connection_string.starts_with("postgresql://")
    {
        return Ok(SharedDbBackend::Postgres {
            connection_string: connection_string.to_string(),
        });
    }

    let database_path = sqlite_database_path(connection_string)?;
    Ok(SharedDbBackend::Sqlite {
        connection_string: connection_string.to_string(),
        database_path,
    })
}

/// Extract a SQLite database path from a SQLite connection string.
pub fn sqlite_database_path(connection_string: &str) -> Result<String, String> {
    let trimmed = connection_string.trim();
    if trimmed.is_empty() {
        return Err("sqlite connection string is empty".to_string());
    }

    if trimmed == ":memory:" || trimmed == "sqlite::memory:" || trimmed.contains(":memory:") {
        return Ok(":memory:".to_string());
    }

    if let Some(rest) = trimmed.strip_prefix("sqlite:///") {
        return Ok(format!("/{}", rest.split('?').next().unwrap_or(rest)));
    }

    if let Some(rest) = trimmed.strip_prefix("sqlite://") {
        return Ok(rest.split('?').next().unwrap_or(rest).to_string());
    }

    if let Some(rest) = trimmed.strip_prefix("sqlite:") {
        return Ok(rest.split('?').next().unwrap_or(rest).to_string());
    }

    Err(format!(
        "unsupported shared database connection string '{}': expected sqlite:// or postgres://",
        connection_string
    ))
}

#[cfg(test)]
mod tests {
    use super::{resolve_shared_db_backend, sqlite_database_path, SharedDbBackend};
    use plexspaces_proto::storage::v1::SharedDbConfig;

    #[test]
    fn test_sqlite_database_path_from_sqlite_url() {
        assert_eq!(
            sqlite_database_path("sqlite:///tmp/plexspaces.db?mode=rwc").unwrap(),
            "/tmp/plexspaces.db"
        );
    }

    #[test]
    fn test_sqlite_database_path_from_memory() {
        assert_eq!(sqlite_database_path("sqlite::memory:").unwrap(), ":memory:");
    }

    #[test]
    fn test_resolve_shared_db_backend_postgres() {
        let config = SharedDbConfig {
            connection_string: "postgres://localhost/test".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            resolve_shared_db_backend(&config).unwrap(),
            SharedDbBackend::Postgres { .. }
        ));
    }

    #[test]
    fn test_resolve_shared_db_backend_sqlite() {
        let config = SharedDbConfig {
            connection_string: "sqlite:///tmp/plexspaces.db".to_string(),
            ..Default::default()
        };
        assert!(matches!(
            resolve_shared_db_backend(&config).unwrap(),
            SharedDbBackend::Sqlite { .. }
        ));
    }
}
