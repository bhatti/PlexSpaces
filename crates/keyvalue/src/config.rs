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

//! Configuration support for KeyValue store backends.
//!
//! ## Purpose
//! Provides environment-based configuration for selecting and configuring
//! different KeyValue store backends (InMemory, SQLite, PostgreSQL, Redis).
//!
//! ## Environment Variables
//!
//! ### Backend Selection
//! - `PLEXSPACES_KV_BACKEND`: Backend type (default: "in-memory")
//!   - "in-memory" | "memory" → InMemoryKVStore
//!   - "sqlite" → SqliteKVStore
//!   - "postgres" | "postgresql" → PostgreSQLKVStore
//!   - "redis" → RedisKVStore
//!
//! ### SQLite Configuration
//! - `PLEXSPACES_KV_SQLITE_PATH`: Database file path (default: ":memory:")
//!
//! ### PostgreSQL Configuration
//! - `PLEXSPACES_KV_POSTGRES_URL`: Connection string
//!   - Format: `postgres://user:password@host:port/database`
//! - `PLEXSPACES_KV_POSTGRES_POOL_SIZE`: Connection pool size (default: 10)
//!
//! ### Redis Configuration
//! - `PLEXSPACES_KV_REDIS_URL`: Redis server URL (default: "redis://localhost:6379")
//! - `PLEXSPACES_KV_REDIS_NAMESPACE`: Key prefix for isolation (default: "plexspaces:")
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
//! export PLEXSPACES_KV_BACKEND=sqlite
//! export PLEXSPACES_KV_SQLITE_PATH=/tmp/plexspaces.db
//! cargo run
//! ```
//!
//! ### PostgreSQL
//! ```bash
//! export PLEXSPACES_KV_BACKEND=postgres
//! export PLEXSPACES_KV_POSTGRES_URL=postgres://user:pass@localhost/plexspaces
//! cargo run
//! ```
//!
//! ### Redis
//! ```bash
//! export PLEXSPACES_KV_BACKEND=redis
//! export PLEXSPACES_KV_REDIS_URL=redis://localhost:6379
//! export PLEXSPACES_KV_REDIS_NAMESPACE=myapp:
//! cargo run
//! ```

use crate::{InMemoryKVStore, KVError, KVResult, KeyValueStore};
use std::sync::Arc;

/// Backend type configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendType {
    /// In-memory HashMap backend (default, always available)
    InMemory,
    /// SQLite backend (requires sql-backend feature)
    Sqlite {
        /// Path to SQLite database file
        path: String,
    },
    /// PostgreSQL backend (requires sql-backend feature)
    PostgreSQL {
        /// PostgreSQL connection string
        connection_string: String,
        /// Connection pool size
        pool_size: u32,
    },
    /// Redis backend (requires redis-backend feature)
    Redis {
        /// Redis server URL
        url: String,
        /// Redis key namespace prefix
        namespace: String,
    },
}

#[allow(clippy::derivable_impls)]
impl Default for BackendType {
    fn default() -> Self {
        Self::InMemory
    }
}

/// KeyValue store configuration.
#[derive(Debug, Clone)]
pub struct KVConfig {
    /// Backend type
    pub backend: BackendType,
}

impl Default for KVConfig {
    fn default() -> Self {
        Self {
            backend: BackendType::InMemory,
        }
    }
}

impl KVConfig {
    /// Create configuration from environment variables.
    ///
    /// ## Environment Variables
    /// See module documentation for complete list.
    ///
    /// ## Examples
    /// ```rust
    /// use plexspaces_keyvalue::KVConfig;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = KVConfig::from_env()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_env() -> KVResult<Self> {
        let backend_str = std::env::var("PLEXSPACES_KV_BACKEND")
            .unwrap_or_else(|_| "in-memory".to_string())
            .to_lowercase();

        let backend = match backend_str.as_str() {
            "in-memory" | "memory" => BackendType::InMemory,

            "sqlite" => {
                let path = std::env::var("PLEXSPACES_KV_SQLITE_PATH")
                    .unwrap_or_else(|_| ":memory:".to_string());
                BackendType::Sqlite { path }
            }

            "postgres" | "postgresql" => {
                let connection_string =
                    std::env::var("PLEXSPACES_KV_POSTGRES_URL").map_err(|_| {
                        KVError::ConfigError("PLEXSPACES_KV_POSTGRES_URL not set".to_string())
                    })?;
                let pool_size = std::env::var("PLEXSPACES_KV_POSTGRES_POOL_SIZE")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(10);
                BackendType::PostgreSQL {
                    connection_string,
                    pool_size,
                }
            }

            "redis" => {
                let url = std::env::var("PLEXSPACES_KV_REDIS_URL")
                    .unwrap_or_else(|_| "redis://localhost:6379".to_string());
                let namespace = std::env::var("PLEXSPACES_KV_REDIS_NAMESPACE")
                    .unwrap_or_else(|_| "plexspaces:".to_string());
                BackendType::Redis { url, namespace }
            }

            other => {
                return Err(KVError::ConfigError(format!(
                    "Unknown backend type: {}. Valid options: in-memory, sqlite, postgres, redis",
                    other
                )));
            }
        };

        Ok(Self { backend })
    }

    /// Create configuration with explicit backend.
    ///
    /// ## Examples
    /// ```rust
    /// use plexspaces_keyvalue::{KVConfig, BackendType};
    ///
    /// let config = KVConfig::new(BackendType::Sqlite {
    ///     path: "/tmp/test.db".to_string()
    /// });
    /// ```
    pub fn new(backend: BackendType) -> Self {
        Self { backend }
    }
}

/// Create a KeyValue store from environment configuration.
///
/// ## Examples
/// ```rust
/// use plexspaces_keyvalue::create_keyvalue_from_env;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let kv = create_keyvalue_from_env().await?;
/// kv.put("key", b"value".to_vec()).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_keyvalue_from_env() -> KVResult<Arc<dyn KeyValueStore>> {
    let config = KVConfig::from_env()?;
    create_keyvalue_from_config(config).await
}

/// Create a KeyValue store from explicit configuration.
///
/// ## Examples
/// ```rust
/// use plexspaces_keyvalue::{create_keyvalue_from_config, KVConfig, BackendType};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = KVConfig::new(BackendType::InMemory);
/// let kv = create_keyvalue_from_config(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_keyvalue_from_config(config: KVConfig) -> KVResult<Arc<dyn KeyValueStore>> {
    match config.backend {
        BackendType::InMemory => Ok(Arc::new(InMemoryKVStore::new())),

        #[cfg(feature = "sql-backend")]
        BackendType::Sqlite { path } => {
            use crate::sql::SqliteKVStore;
            let store = SqliteKVStore::new(&path).await?;
            Ok(Arc::new(store))
        }

        #[cfg(not(feature = "sql-backend"))]
        BackendType::Sqlite { .. } => Err(KVError::ConfigError(
            "SQLite backend requires 'sql-backend' feature".to_string(),
        )),

        #[cfg(feature = "sql-backend")]
        BackendType::PostgreSQL {
            connection_string,
            pool_size,
        } => {
            use crate::sql::PostgreSQLKVStore;
            let store = PostgreSQLKVStore::new(&connection_string, pool_size).await?;
            Ok(Arc::new(store))
        }

        #[cfg(not(feature = "sql-backend"))]
        BackendType::PostgreSQL { .. } => Err(KVError::ConfigError(
            "PostgreSQL backend requires 'sql-backend' feature".to_string(),
        )),

        #[cfg(feature = "redis-backend")]
        BackendType::Redis { url, namespace } => {
            use crate::redis::RedisKVStore;
            let store = RedisKVStore::new(&url, &namespace).await?;
            Ok(Arc::new(store))
        }

        #[cfg(not(feature = "redis-backend"))]
        BackendType::Redis { .. } => Err(KVError::ConfigError(
            "Redis backend requires 'redis-backend' feature".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_default_config() {
        let config = KVConfig::default();
        assert_eq!(config.backend, BackendType::InMemory);
    }

    #[test]
    #[serial]
    fn test_config_from_env_default() {
        // Use unique test to avoid env var conflicts
        std::env::remove_var("PLEXSPACES_KV_BACKEND");

        let config = KVConfig::from_env().unwrap();
        assert_eq!(config.backend, BackendType::InMemory);
    }

    #[test]
    #[serial]
    fn test_config_from_env_sqlite() {
        // Set specific vars for this test
        std::env::set_var("PLEXSPACES_KV_BACKEND", "sqlite");
        std::env::set_var("PLEXSPACES_KV_SQLITE_PATH", "/tmp/test.db");

        let config = KVConfig::from_env().unwrap();
        assert_eq!(
            config.backend,
            BackendType::Sqlite {
                path: "/tmp/test.db".to_string()
            }
        );

        // Cleanup
        std::env::remove_var("PLEXSPACES_KV_BACKEND");
        std::env::remove_var("PLEXSPACES_KV_SQLITE_PATH");
    }

    #[test]
    #[serial]
    fn test_config_from_env_postgres() {
        std::env::set_var("PLEXSPACES_KV_BACKEND", "postgres");
        std::env::set_var("PLEXSPACES_KV_POSTGRES_URL", "postgres://localhost/test");
        std::env::set_var("PLEXSPACES_KV_POSTGRES_POOL_SIZE", "5");

        let config = KVConfig::from_env().unwrap();
        assert_eq!(
            config.backend,
            BackendType::PostgreSQL {
                connection_string: "postgres://localhost/test".to_string(),
                pool_size: 5
            }
        );

        std::env::remove_var("PLEXSPACES_KV_BACKEND");
        std::env::remove_var("PLEXSPACES_KV_POSTGRES_URL");
        std::env::remove_var("PLEXSPACES_KV_POSTGRES_POOL_SIZE");
    }

    #[test]
    #[serial]
    fn test_config_from_env_redis() {
        std::env::set_var("PLEXSPACES_KV_BACKEND", "redis");
        std::env::set_var("PLEXSPACES_KV_REDIS_URL", "redis://localhost:6379");
        std::env::set_var("PLEXSPACES_KV_REDIS_NAMESPACE", "test:");

        let config = KVConfig::from_env().unwrap();
        assert_eq!(
            config.backend,
            BackendType::Redis {
                url: "redis://localhost:6379".to_string(),
                namespace: "test:".to_string()
            }
        );

        std::env::remove_var("PLEXSPACES_KV_BACKEND");
        std::env::remove_var("PLEXSPACES_KV_REDIS_URL");
        std::env::remove_var("PLEXSPACES_KV_REDIS_NAMESPACE");
    }

    #[test]
    #[serial]
    fn test_config_from_env_invalid_backend() {
        std::env::set_var("PLEXSPACES_KV_BACKEND", "invalid");

        let result = KVConfig::from_env();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unknown backend type"));

        std::env::remove_var("PLEXSPACES_KV_BACKEND");
    }

    #[test]
    fn test_config_new_explicit() {
        let config = KVConfig::new(BackendType::Sqlite {
            path: ":memory:".to_string(),
        });
        assert_eq!(
            config.backend,
            BackendType::Sqlite {
                path: ":memory:".to_string()
            }
        );
    }

    #[tokio::test]
    async fn test_create_keyvalue_in_memory() {
        let config = KVConfig::new(BackendType::InMemory);
        let kv = create_keyvalue_from_config(config).await.unwrap();

        let ctx = plexspaces_core::RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());
        kv.put(&ctx, "test", b"value".to_vec()).await.unwrap();
        let value = kv.get(&ctx, "test").await.unwrap();
        assert_eq!(value, Some(b"value".to_vec()));
    }

    #[tokio::test]
    #[serial]
    async fn test_create_keyvalue_from_env_default() {
        std::env::remove_var("PLEXSPACES_KV_BACKEND");

        let kv = create_keyvalue_from_env().await.unwrap();
        let ctx = plexspaces_core::RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());
        kv.put(&ctx, "test", b"value".to_vec()).await.unwrap();
        let value = kv.get(&ctx, "test").await.unwrap();
        assert_eq!(value, Some(b"value".to_vec()));
    }
}
