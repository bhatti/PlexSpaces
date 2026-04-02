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
//! Provides proto-first configuration helpers for selecting and configuring
//! different KeyValue store backends from `plexspaces.storage.v1` config types.
//!
//! Runtime env var binding happens in `plexspaces_common::config_manager`; this module
//! only consumes resolved `SharedDbConfig` / `StorageProviderConfig` values.
//!
//! ## Examples
//!
//! ### Shared SQLite
//! ```bash
//! runtime:
//!   db:
//!     connection_string: sqlite:///tmp/plexspaces.db
//! ```
//!
//! ### PostgreSQL
//! ```bash
//! runtime:
//!   db:
//!     connection_string: postgres://user:pass@localhost/plexspaces
//! ```
//!
//! ### Redis
//! ```bash
//! provider: REDIS
//! redis:
//!   url: redis://localhost:6379
//!   key_prefix: myapp:
//! ```

use crate::{KVError, KVResult, KeyValueStore};
use plexspaces_common::{resolve_shared_db_backend, SharedDbBackend};
use plexspaces_proto::storage::v1::{
    storage_provider_config, SharedDbConfig, StorageProvider, StorageProviderConfig,
};
use std::sync::Arc;

/// Backend type configuration.
#[derive(Clone)]
pub enum BackendType {
    /// In-memory storage using SQLite :memory: (always available via sql-backend)
    InMemory,
    /// SQLite backend (requires sql-backend feature)
    Sqlite {
        /// Path to SQLite database file (use ":memory:" for in-memory)
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
    /// DynamoDB backend (requires ddb-backend feature)
    #[cfg(feature = "ddb-backend")]
    DynamoDB {
        /// AWS region
        region: String,
        /// Table name prefix
        table_prefix: String,
        /// Endpoint URL (for local testing)
        endpoint_url: Option<String>,
    },
    /// Blob backend from environment variables (requires blob-backend feature)
    /// Created asynchronously in create_keyvalue_from_config
    /// Uses object_store directly - no SQL database or blob service needed
    #[cfg(feature = "blob-backend")]
    BlobFromEnv {
        /// Blob keyvalue configuration
        config: crate::blob::BlobKVConfig,
    },
}

#[allow(clippy::derivable_impls)]
impl Default for BackendType {
    fn default() -> Self {
        Self::InMemory
    }
}

/// KeyValue store configuration.
#[derive(Clone)]
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

fn kv_config_from_shared_db(config: &SharedDbConfig) -> KVResult<KVConfig> {
    match resolve_shared_db_backend(config).map_err(KVError::ConfigError)? {
        SharedDbBackend::Sqlite { database_path, .. } => Ok(KVConfig::new(BackendType::Sqlite {
            path: database_path,
        })),
        SharedDbBackend::Postgres { connection_string } => {
            let pool_size = if config.pool_size == 0 {
                10
            } else {
                config.pool_size
            };
            Ok(KVConfig::new(BackendType::PostgreSQL {
                connection_string,
                pool_size,
            }))
        }
    }
}

fn kv_config_from_storage_provider(
    provider_config: &StorageProviderConfig,
    shared_db: Option<&SharedDbConfig>,
) -> KVResult<KVConfig> {
    match StorageProvider::try_from(provider_config.provider)
        .unwrap_or(StorageProvider::StorageProviderUnspecified)
    {
        StorageProvider::StorageProviderSqlite => {
            let sqlite = match provider_config.config.as_ref() {
                Some(storage_provider_config::Config::Sqlite(sqlite)) => sqlite,
                Some(storage_provider_config::Config::Postgres(postgres)) => {
                    return kv_config_from_shared_db(postgres);
                }
                _ => {
                    return Err(KVError::ConfigError(
                        "sqlite keyvalue provider requires sqlite config".to_string(),
                    ))
                }
            };
            Ok(KVConfig::new(BackendType::Sqlite {
                path: sqlite.database_path.clone(),
            }))
        }
        StorageProvider::StorageProviderPostgres => {
            let postgres = match provider_config.config.as_ref() {
                Some(storage_provider_config::Config::Postgres(postgres)) => postgres,
                _ => {
                    return Err(KVError::ConfigError(
                        "postgres keyvalue provider requires postgres config".to_string(),
                    ))
                }
            };
            kv_config_from_shared_db(postgres)
        }
        StorageProvider::StorageProviderRedis => {
            let redis = match provider_config.config.as_ref() {
                Some(storage_provider_config::Config::Redis(redis)) => redis,
                _ => {
                    return Err(KVError::ConfigError(
                        "redis keyvalue provider requires redis config".to_string(),
                    ))
                }
            };
            Ok(KVConfig::new(BackendType::Redis {
                url: redis.url.clone(),
                namespace: if redis.key_prefix.is_empty() {
                    "plexspaces:".to_string()
                } else {
                    redis.key_prefix.clone()
                },
            }))
        }
        #[cfg(feature = "ddb-backend")]
        StorageProvider::StorageProviderDynamodb => {
            let dynamodb = match provider_config.config.as_ref() {
                Some(storage_provider_config::Config::Dynamodb(config)) => config,
                _ => {
                    return Err(KVError::ConfigError(
                        "dynamodb keyvalue provider requires dynamodb config".to_string(),
                    ))
                }
            };
            Ok(KVConfig::new(BackendType::DynamoDB {
                region: dynamodb.region.clone(),
                table_prefix: dynamodb.table_prefix.clone(),
                endpoint_url: (!dynamodb.endpoint_url.is_empty())
                    .then(|| dynamodb.endpoint_url.clone()),
            }))
        }
        StorageProvider::StorageProviderDynamodb => Err(KVError::ConfigError(
            "dynamodb keyvalue provider requires 'ddb-backend' feature".to_string(),
        )),
        StorageProvider::StorageProviderUnspecified => shared_db
            .ok_or_else(|| {
                KVError::ConfigError(
                    "keyvalue storage requires shared db when provider is unspecified".to_string(),
                )
            })
            .and_then(kv_config_from_shared_db),
    }
}

/// Create both rich and common KeyValue store trait objects from shared relational DB config.
pub async fn create_keyvalue_stores_from_shared_db(
    config: &SharedDbConfig,
) -> KVResult<(
    Arc<dyn KeyValueStore>,
    Arc<dyn plexspaces_common::KeyValueStore>,
)> {
    create_keyvalue_stores_from_config(kv_config_from_shared_db(config)?).await
}

/// Create both rich and common KeyValue store trait objects from proto storage config.
pub async fn create_keyvalue_stores_from_storage_config(
    provider_config: &StorageProviderConfig,
    shared_db: Option<&SharedDbConfig>,
) -> KVResult<(
    Arc<dyn KeyValueStore>,
    Arc<dyn plexspaces_common::KeyValueStore>,
)> {
    create_keyvalue_stores_from_config(kv_config_from_storage_provider(provider_config, shared_db)?)
        .await
}

/// Create both rich and common KeyValue store trait objects from explicit configuration.
///
/// Returns a tuple of:
/// - `Arc<dyn KeyValueStore>` (rich trait for ProcessGroupRegistry and internal services)
/// - `Arc<dyn plexspaces_common::KeyValueStore>` (common trait for WASM actors via ServiceLocator)
pub async fn create_keyvalue_stores_from_config(
    config: KVConfig,
) -> KVResult<(
    Arc<dyn KeyValueStore>,
    Arc<dyn plexspaces_common::KeyValueStore>,
)> {
    match config.backend {
        BackendType::InMemory => {
            #[cfg(feature = "sql-backend")]
            {
                use crate::sql::SqliteKVStore;
                let store = Arc::new(SqliteKVStore::new(":memory:").await?);
                let rich: Arc<dyn KeyValueStore> = store.clone();
                let common: Arc<dyn plexspaces_common::KeyValueStore> = store;
                Ok((rich, common))
            }
            #[cfg(not(feature = "sql-backend"))]
            {
                Err(KVError::ConfigError(
                    "InMemory backend requires 'sql-backend' feature (uses SQLite :memory:)"
                        .to_string(),
                ))
            }
        }

        #[cfg(feature = "sql-backend")]
        BackendType::Sqlite { path } => {
            use crate::sql::SqliteKVStore;
            let store = Arc::new(SqliteKVStore::new(&path).await?);
            let rich: Arc<dyn KeyValueStore> = store.clone();
            let common: Arc<dyn plexspaces_common::KeyValueStore> = store;
            Ok((rich, common))
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
            let store = Arc::new(PostgreSQLKVStore::new(&connection_string, pool_size).await?);
            let rich: Arc<dyn KeyValueStore> = store.clone();
            let common: Arc<dyn plexspaces_common::KeyValueStore> = store;
            Ok((rich, common))
        }

        #[cfg(not(feature = "sql-backend"))]
        BackendType::PostgreSQL { .. } => Err(KVError::ConfigError(
            "PostgreSQL backend requires 'sql-backend' feature".to_string(),
        )),

        #[cfg(feature = "redis-backend")]
        BackendType::Redis { url, namespace } => {
            use crate::redis::RedisKVStore;
            let store = Arc::new(RedisKVStore::new(&url, &namespace).await?);
            let rich: Arc<dyn KeyValueStore> = store.clone();
            let common: Arc<dyn plexspaces_common::KeyValueStore> = store;
            Ok((rich, common))
        }

        #[cfg(not(feature = "redis-backend"))]
        BackendType::Redis { .. } => Err(KVError::ConfigError(
            "Redis backend requires 'redis-backend' feature".to_string(),
        )),

        #[cfg(feature = "ddb-backend")]
        BackendType::DynamoDB {
            region,
            table_prefix,
            endpoint_url,
        } => {
            use crate::ddb::DynamoDBKVStore;
            let table_name = format!("{}{}", table_prefix, "keyvalue");
            let store = Arc::new(
                DynamoDBKVStore::new(region, table_name, endpoint_url)
                    .await
                    .map_err(|e| {
                        KVError::ConfigError(format!(
                            "Failed to create DynamoDB keyvalue store: {}",
                            e
                        ))
                    })?,
            );
            let rich: Arc<dyn KeyValueStore> = store.clone();
            let common: Arc<dyn plexspaces_common::KeyValueStore> = store;
            Ok((rich, common))
        }

        #[cfg(feature = "blob-backend")]
        BackendType::BlobFromEnv { config } => {
            use crate::blob::BlobKVStore;
            let store = Arc::new(BlobKVStore::new(config).await.map_err(|e| {
                KVError::ConfigError(format!("Failed to create blob keyvalue store: {}", e))
            })?);
            let rich: Arc<dyn KeyValueStore> = store.clone();
            let common: Arc<dyn plexspaces_common::KeyValueStore> = store;
            Ok((rich, common))
        }
    }
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
        // InMemory maps to SQLite :memory:
        BackendType::InMemory => {
            #[cfg(feature = "sql-backend")]
            {
                use crate::sql::SqliteKVStore;
                let store = SqliteKVStore::new(":memory:").await?;
                Ok(Arc::new(store))
            }
            #[cfg(not(feature = "sql-backend"))]
            {
                Err(KVError::ConfigError(
                    "InMemory backend requires 'sql-backend' feature (uses SQLite :memory:)"
                        .to_string(),
                ))
            }
        }

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

        #[cfg(feature = "ddb-backend")]
        BackendType::DynamoDB {
            region,
            table_prefix,
            endpoint_url,
        } => {
            use crate::ddb::DynamoDBKVStore;
            // Construct full table name using prefix
            let table_name = format!("{}{}", table_prefix, "keyvalue");
            let store = DynamoDBKVStore::new(region, table_name, endpoint_url)
                .await
                .map_err(|e| {
                    KVError::ConfigError(format!("Failed to create DynamoDB keyvalue store: {}", e))
                })?;
            Ok(Arc::new(store))
        }

        #[cfg(feature = "blob-backend")]
        BackendType::BlobFromEnv { config } => {
            use crate::blob::BlobKVStore;
            // Create blob keyvalue store directly from config
            // Uses object_store directly - no SQL database needed
            // Simple, reliable design: just uses MinIO/S3 directly
            let kv = BlobKVStore::new(config).await.map_err(|e| {
                KVError::ConfigError(format!("Failed to create blob keyvalue store: {}", e))
            })?;
            Ok(Arc::new(kv))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::storage::v1::SharedDbConfig;

    #[test]
    fn test_default_config() {
        let config = KVConfig::default();
        // BackendType doesn't implement PartialEq, so we can't use assert_eq!
        // Just verify it's created successfully
        match config.backend {
            BackendType::InMemory => {}
            _ => panic!("Default should be InMemory"),
        }
    }

    #[test]
    fn test_config_new_explicit() {
        let config = KVConfig::new(BackendType::Sqlite {
            path: ":memory:".to_string(),
        });
        // BackendType doesn't implement PartialEq, verify with pattern matching
        match config.backend {
            BackendType::Sqlite { path } => {
                assert_eq!(path, ":memory:".to_string());
            }
            _ => panic!("Expected Sqlite backend"),
        }
    }

    #[tokio::test]
    async fn test_create_keyvalue_in_memory() {
        let config = KVConfig::new(BackendType::InMemory);
        let kv = create_keyvalue_from_config(config).await.unwrap();

        let ctx = plexspaces_common::RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "default".to_string(),
        );
        kv.put(&ctx, "test", b"value".to_vec()).await.unwrap();
        let value = kv.get(&ctx, "test").await.unwrap();
        assert_eq!(value, Some(b"value".to_vec()));
    }

    #[tokio::test]
    async fn test_create_keyvalue_from_shared_db_sqlite() {
        let shared_db = SharedDbConfig {
            connection_string: "sqlite::memory:".to_string(),
            ..Default::default()
        };

        let (kv, _) = create_keyvalue_stores_from_shared_db(&shared_db)
            .await
            .unwrap();
        let ctx = plexspaces_common::RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "default".to_string(),
        );
        kv.put(&ctx, "test", b"value".to_vec()).await.unwrap();
        let value = kv.get(&ctx, "test").await.unwrap();
        assert_eq!(value, Some(b"value".to_vec()));
    }
}
