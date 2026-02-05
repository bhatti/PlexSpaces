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

//! Configuration support for Object Registry Repository backends.
//!
//! ## Purpose
//! Provides environment-based configuration for selecting and configuring
//! different Object Registry Repository backends (InMemory, SQLite, PostgreSQL, DynamoDB).
//!
//! ## Environment Variables
//!
//! ### Backend Selection
//! - `PLEXSPACES_OBJECT_REGISTRY_BACKEND`: Backend type (default: "in-memory")
//!   - "in-memory" | "memory" → SQLite :memory: (in-memory)
//!   - "sqlite" → SqliteObjectRegistryRepository
//!   - "postgres" | "postgresql" → PostgresObjectRegistryRepository
//!   - "dynamodb" | "ddb" → DynamoDBObjectRegistryRepository
//!
//! ### SQLite Configuration
//! - `PLEXSPACES_OBJECT_REGISTRY_SQLITE_PATH`: Database file path (default: ":memory:")
//!
//! ### PostgreSQL Configuration
//! - `PLEXSPACES_OBJECT_REGISTRY_POSTGRES_URL`: Connection string
//!   - Format: `postgres://user:password@host:port/database`
//!
//! ### DynamoDB Configuration
//! - `PLEXSPACES_OBJECT_REGISTRY_DDB_TABLE`: Table name (default: "plexspaces-object-registry")
//! - `PLEXSPACES_OBJECT_REGISTRY_DDB_REGION`: AWS region (default: "us-east-1")
//! - `PLEXSPACES_OBJECT_REGISTRY_DDB_ENDPOINT`: Optional endpoint URL (for DynamoDB Local)
//!
//! ## Examples
//!
//! ### In-Memory (Default)
//! ```bash
//! # No environment variables needed
//! cargo run
//! ```
//!
//! ### SQLite
//! ```bash
//! export PLEXSPACES_OBJECT_REGISTRY_BACKEND=sqlite
//! export PLEXSPACES_OBJECT_REGISTRY_SQLITE_PATH=/tmp/plexspaces-registry.db
//! cargo run
//! ```
//!
//! ### PostgreSQL
//! ```bash
//! export PLEXSPACES_OBJECT_REGISTRY_BACKEND=postgres
//! export PLEXSPACES_OBJECT_REGISTRY_POSTGRES_URL=postgres://user:pass@localhost/plexspaces
//! cargo run
//! ```
//!
//! ### DynamoDB
//! ```bash
//! export PLEXSPACES_OBJECT_REGISTRY_BACKEND=dynamodb
//! export PLEXSPACES_OBJECT_REGISTRY_DDB_TABLE=my-registry
//! export PLEXSPACES_OBJECT_REGISTRY_DDB_REGION=us-west-2
//! cargo run
//! ```

use crate::repository::{ObjectRegistryRepository, RepositoryError, RepositoryResult};
use std::sync::Arc;

/// Backend type configuration for Object Registry Repository
#[derive(Debug, Clone)]
pub enum ObjectRegistryBackendType {
    /// In-memory backend using SQLite :memory: (requires sql-backend feature)
    InMemory,
    /// SQLite backend (requires sql-backend feature, use ":memory:" for in-memory)
    Sqlite {
        /// Path to SQLite database file
        path: String,
    },
    /// PostgreSQL backend (requires sql-backend feature)
    PostgreSQL {
        /// PostgreSQL connection string
        connection_string: String,
    },
    /// DynamoDB backend (requires ddb-backend feature)
    DynamoDB {
        /// DynamoDB table name
        table_name: String,
        /// AWS region
        region: String,
        /// Optional endpoint URL (for DynamoDB Local)
        endpoint_url: Option<String>,
    },
}

impl Default for ObjectRegistryBackendType {
    fn default() -> Self {
        Self::InMemory
    }
}

/// Configuration for Object Registry Repository
#[derive(Debug, Clone)]
pub struct ObjectRegistryConfig {
    /// Backend type
    pub backend: ObjectRegistryBackendType,
}

impl Default for ObjectRegistryConfig {
    fn default() -> Self {
        Self {
            backend: ObjectRegistryBackendType::default(),
        }
    }
}

impl ObjectRegistryConfig {
    /// Create configuration from environment variables
    ///
    /// ## Environment Variables
    /// - `PLEXSPACES_OBJECT_REGISTRY_BACKEND`: Backend type
    /// - `PLEXSPACES_OBJECT_REGISTRY_SQLITE_PATH`: SQLite database path
    /// - `PLEXSPACES_OBJECT_REGISTRY_POSTGRES_URL`: PostgreSQL connection string
    /// - `PLEXSPACES_OBJECT_REGISTRY_DDB_TABLE`: DynamoDB table name
    /// - `PLEXSPACES_OBJECT_REGISTRY_DDB_REGION`: DynamoDB region
    /// - `PLEXSPACES_OBJECT_REGISTRY_DDB_ENDPOINT`: DynamoDB endpoint URL
    pub fn from_env() -> Self {
        let backend_str = std::env::var("PLEXSPACES_OBJECT_REGISTRY_BACKEND")
            .unwrap_or_else(|_| "in-memory".to_string())
            .to_lowercase();

        let backend = match backend_str.as_str() {
            "in-memory" | "memory" | "inmemory" => ObjectRegistryBackendType::InMemory,
            "sqlite" => {
                let path = std::env::var("PLEXSPACES_OBJECT_REGISTRY_SQLITE_PATH")
                    .unwrap_or_else(|_| ":memory:".to_string());
                ObjectRegistryBackendType::Sqlite { path }
            }
            "postgres" | "postgresql" => {
                let connection_string = std::env::var("PLEXSPACES_OBJECT_REGISTRY_POSTGRES_URL")
                    .expect("PLEXSPACES_OBJECT_REGISTRY_POSTGRES_URL must be set for PostgreSQL backend");
                ObjectRegistryBackendType::PostgreSQL { connection_string }
            }
            "dynamodb" | "ddb" => {
                let table_name = std::env::var("PLEXSPACES_OBJECT_REGISTRY_DDB_TABLE")
                    .unwrap_or_else(|_| "plexspaces-object-registry".to_string());
                let region = std::env::var("PLEXSPACES_OBJECT_REGISTRY_DDB_REGION")
                    .unwrap_or_else(|_| "us-east-1".to_string());
                let endpoint_url = std::env::var("PLEXSPACES_OBJECT_REGISTRY_DDB_ENDPOINT").ok();
                ObjectRegistryBackendType::DynamoDB {
                    table_name,
                    region,
                    endpoint_url,
                }
            }
            _ => {
                tracing::warn!(
                    backend = %backend_str,
                    "Unknown object registry backend, defaulting to in-memory"
                );
                ObjectRegistryBackendType::InMemory
            }
        };

        Self { backend }
    }

    /// Create configuration for in-memory backend
    pub fn in_memory() -> Self {
        Self {
            backend: ObjectRegistryBackendType::InMemory,
        }
    }

    /// Create configuration for SQLite backend
    pub fn sqlite(path: &str) -> Self {
        Self {
            backend: ObjectRegistryBackendType::Sqlite {
                path: path.to_string(),
            },
        }
    }

    /// Create configuration for PostgreSQL backend
    pub fn postgres(connection_string: &str) -> Self {
        Self {
            backend: ObjectRegistryBackendType::PostgreSQL {
                connection_string: connection_string.to_string(),
            },
        }
    }

    /// Create configuration for DynamoDB backend
    pub fn dynamodb(table_name: &str, region: &str, endpoint_url: Option<&str>) -> Self {
        Self {
            backend: ObjectRegistryBackendType::DynamoDB {
                table_name: table_name.to_string(),
                region: region.to_string(),
                endpoint_url: endpoint_url.map(|s| s.to_string()),
            },
        }
    }
}

/// Create Object Registry Repository from configuration
///
/// ## Arguments
/// * `config` - ObjectRegistryConfig with backend settings
///
/// ## Returns
/// Arc<dyn ObjectRegistryRepository> for the configured backend
///
/// ## Errors
/// - Backend not available (feature not enabled)
/// - Connection failure
/// - Migration failure
pub async fn create_repository_from_config(
    config: &ObjectRegistryConfig,
) -> RepositoryResult<Arc<dyn ObjectRegistryRepository>> {
    match &config.backend {
        // InMemory maps to SQLite :memory:
        ObjectRegistryBackendType::InMemory => {
            #[cfg(feature = "sql-backend")]
            {
                use crate::repository::SqliteObjectRegistryRepository;
                tracing::info!(backend = "SQLite :memory:", "Object Registry using in-memory SQLite backend");
                let repo = SqliteObjectRegistryRepository::new(":memory:").await?;
                Ok(Arc::new(repo))
            }
            #[cfg(not(feature = "sql-backend"))]
            {
                Err(RepositoryError::Storage(
                    "InMemory backend requires 'sql-backend' feature (uses SQLite :memory:).".to_string(),
                ))
            }
        }
        ObjectRegistryBackendType::Sqlite { path } => {
            #[cfg(feature = "sql-backend")]
            {
                use crate::repository::SqliteObjectRegistryRepository;
                tracing::info!(backend = "SQLite", path = %path, "Object Registry using SQLite backend");
                let repo = SqliteObjectRegistryRepository::new(path).await?;
                Ok(Arc::new(repo))
            }
            #[cfg(not(feature = "sql-backend"))]
            {
                Err(RepositoryError::Storage(
                    "SQLite backend not available. Enable 'sql-backend' feature.".to_string(),
                ))
            }
        }
        ObjectRegistryBackendType::PostgreSQL { connection_string } => {
            #[cfg(feature = "sql-backend")]
            {
                use crate::repository::PostgresObjectRegistryRepository;
                tracing::info!(backend = "PostgreSQL", "Object Registry using PostgreSQL backend");
                let repo = PostgresObjectRegistryRepository::new(connection_string).await?;
                Ok(Arc::new(repo))
            }
            #[cfg(not(feature = "sql-backend"))]
            {
                Err(RepositoryError::Storage(
                    "PostgreSQL backend not available. Enable 'sql-backend' feature.".to_string(),
                ))
            }
        }
        ObjectRegistryBackendType::DynamoDB {
            table_name,
            region,
            endpoint_url,
        } => {
            #[cfg(feature = "ddb-backend")]
            {
                use crate::repository::DynamoDBObjectRegistryRepository;
                tracing::info!(
                    backend = "DynamoDB",
                    table = %table_name,
                    region = %region,
                    "Object Registry using DynamoDB backend"
                );
                let repo = DynamoDBObjectRegistryRepository::new(
                    region.clone(),
                    table_name.clone(),
                    endpoint_url.clone(),
                )
                .await?;
                Ok(Arc::new(repo))
            }
            #[cfg(not(feature = "ddb-backend"))]
            {
                Err(RepositoryError::Storage(
                    "DynamoDB backend not available. Enable 'ddb-backend' feature.".to_string(),
                ))
            }
        }
    }
}

/// Create Object Registry Repository from environment variables
///
/// Convenience function that combines from_env() and create_repository_from_config().
pub async fn create_repository_from_env() -> RepositoryResult<Arc<dyn ObjectRegistryRepository>> {
    let config = ObjectRegistryConfig::from_env();
    create_repository_from_config(&config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ObjectRegistryConfig::default();
        assert!(matches!(config.backend, ObjectRegistryBackendType::InMemory));
    }

    #[test]
    fn test_sqlite_config() {
        let config = ObjectRegistryConfig::sqlite("/tmp/test.db");
        assert!(matches!(
            config.backend,
            ObjectRegistryBackendType::Sqlite { path } if path == "/tmp/test.db"
        ));
    }

    #[test]
    fn test_postgres_config() {
        let config = ObjectRegistryConfig::postgres("postgres://localhost/test");
        assert!(matches!(
            config.backend,
            ObjectRegistryBackendType::PostgreSQL { connection_string } if connection_string == "postgres://localhost/test"
        ));
    }

    #[test]
    fn test_dynamodb_config() {
        let config = ObjectRegistryConfig::dynamodb("my-table", "us-west-2", None);
        assert!(matches!(
            config.backend,
            ObjectRegistryBackendType::DynamoDB { table_name, region, endpoint_url }
                if table_name == "my-table" && region == "us-west-2" && endpoint_url.is_none()
        ));
    }

    #[cfg(feature = "sql-backend")]
    #[tokio::test]
    async fn test_create_in_memory() {
        let config = ObjectRegistryConfig::in_memory();
        let repo = create_repository_from_config(&config).await.unwrap();
        // Just verify it was created successfully
        assert!(Arc::strong_count(&repo) >= 1);
    }
}
