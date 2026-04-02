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

//! Factory for creating lock manager instances.
//!
//! ## Purpose
//! Provides a unified way to create lock managers based on configuration,
//! automatically choosing the best implementation for the deployment scenario.

use crate::{LockError, LockManager, LockResult};
use plexspaces_common::{resolve_shared_db_backend, SharedDbBackend};
use plexspaces_proto::node::v1::RuntimeConfig;
use plexspaces_proto::storage::v1::{
    storage_provider_config, SharedDbConfig, StorageProvider, StorageProviderConfig,
};
use std::sync::Arc;

/// Lock manager backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockBackend {
    /// In-memory lock manager (tokio primitives) - fastest for single-node
    Memory,
    /// SQLite backend (persistent, single-node)
    Sqlite,
    /// PostgreSQL backend (distributed, multi-node)
    Postgres,
    /// Redis backend (distributed with TTL)
    Redis,
    /// DynamoDB backend (AWS managed)
    DynamoDB,
}

/// Create a lock manager from runtime storage configuration.
pub async fn create_lock_manager_from_runtime(
    runtime: &RuntimeConfig,
) -> LockResult<Arc<dyn LockManager>> {
    create_lock_manager_from_storage_config(runtime.locks_provider.as_ref(), runtime.db.as_ref())
        .await
}

/// Create a lock manager from storage provider config with shared-db fallback.
pub async fn create_lock_manager_from_storage_config(
    provider_config: Option<&StorageProviderConfig>,
    shared_db: Option<&SharedDbConfig>,
) -> LockResult<Arc<dyn LockManager>> {
    match provider_config {
        Some(provider_config) => {
            match StorageProvider::try_from(provider_config.provider)
                .unwrap_or(StorageProvider::StorageProviderUnspecified)
            {
                StorageProvider::StorageProviderRedis => {
                    let redis = match provider_config.config.as_ref() {
                        Some(storage_provider_config::Config::Redis(redis)) => redis,
                        _ => {
                            return Err(LockError::ConfigError(
                                "redis lock provider requires redis config".to_string(),
                            ))
                        }
                    };
                    create_lock_manager(
                        LockBackend::Redis,
                        Some(if redis.url.is_empty() {
                            "redis://localhost:6379".to_string()
                        } else {
                            redis.url.clone()
                        }),
                    )
                    .await
                }
                StorageProvider::StorageProviderDynamodb => {
                    let dynamodb = match provider_config.config.as_ref() {
                        Some(storage_provider_config::Config::Dynamodb(config)) => config,
                        _ => {
                            return Err(LockError::ConfigError(
                                "dynamodb lock provider requires dynamodb config".to_string(),
                            ))
                        }
                    };
                    let table_name = if dynamodb.table_prefix.is_empty() {
                        "plexspaces-locks".to_string()
                    } else {
                        format!("{}locks", dynamodb.table_prefix)
                    };
                    let provider_payload = format!(
                        "{}:{}{}",
                        dynamodb.region,
                        table_name,
                        if dynamodb.endpoint_url.is_empty() {
                            String::new()
                        } else {
                            format!(":{}", dynamodb.endpoint_url)
                        }
                    );
                    create_lock_manager(LockBackend::DynamoDB, Some(provider_payload)).await
                }
                StorageProvider::StorageProviderSqlite
                | StorageProvider::StorageProviderPostgres
                | StorageProvider::StorageProviderUnspecified => {
                    create_lock_manager_from_shared_db(
                        provider_config
                            .config
                            .as_ref()
                            .and_then(|cfg| match cfg {
                                storage_provider_config::Config::Postgres(config) => Some(config),
                                _ => None,
                            })
                            .or(shared_db),
                        provider_config.config.as_ref().and_then(|cfg| match cfg {
                            storage_provider_config::Config::Sqlite(config) => {
                                Some(config.database_path.clone())
                            }
                            _ => None,
                        }),
                    )
                    .await
                }
            }
        }
        None => create_lock_manager_from_shared_db(shared_db, None).await,
    }
}

async fn create_lock_manager_from_shared_db(
    shared_db: Option<&SharedDbConfig>,
    sqlite_override_path: Option<String>,
) -> LockResult<Arc<dyn LockManager>> {
    let shared_db = shared_db.ok_or_else(|| {
        LockError::ConfigError(
            "lock manager requires runtime.db when no explicit locks provider is configured"
                .to_string(),
        )
    })?;

    match resolve_shared_db_backend(shared_db).map_err(LockError::ConfigError)? {
        SharedDbBackend::Sqlite {
            connection_string,
            database_path,
        } => {
            let path = sqlite_override_path.unwrap_or(database_path);
            if path == ":memory:" {
                create_lock_manager(LockBackend::Memory, None).await
            } else {
                create_lock_manager(
                    LockBackend::Sqlite,
                    Some(if connection_string.contains(":memory:") {
                        path
                    } else {
                        connection_string
                    }),
                )
                .await
            }
        }
        SharedDbBackend::Postgres { connection_string } => {
            create_lock_manager(LockBackend::Postgres, Some(connection_string)).await
        }
    }
}

/// Create a lock manager based on backend type and configuration.
///
/// ## Purpose
/// Factory function that creates the appropriate lock manager implementation
/// based on the backend type and configuration.
///
/// ## Backend Selection
/// - **Memory**: Use for single-node SQLite deployments (more efficient)
/// - **Sqlite**: Use for persistent single-node deployments
/// - **Postgres**: Use for distributed multi-node deployments
/// - **Redis**: Use for distributed deployments with TTL support
/// - **DynamoDB**: Use for AWS deployments
///
/// ## Examples
/// ```rust,no_run
/// use plexspaces_locks::factory::{create_lock_manager, LockBackend};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // In-memory for single-node SQLite (most efficient)
/// let manager = create_lock_manager(LockBackend::Memory, None).await?;
///
/// // SQLite for persistent single-node
/// let manager = create_lock_manager(LockBackend::Sqlite, Some("sqlite://locks.db".to_string())).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_lock_manager(
    backend: LockBackend,
    database_url: Option<String>,
) -> LockResult<Arc<dyn LockManager>> {
    match backend {
        LockBackend::Memory => {
            tracing::info!(backend = "Memory", "Creating in-memory lock manager");
            Ok(Arc::new(crate::MemoryLockManager::new()))
        }
        #[cfg(feature = "sqlite-backend")]
        LockBackend::Sqlite => {
            use crate::sql::SqliteLockManager;
            let url = database_url.unwrap_or_else(|| ":memory:".to_string());
            tracing::info!(backend = "SQLite", db_url = %url, "Creating SQLite lock manager");
            SqliteLockManager::new(&url)
                .await
                .map(|m| Arc::new(m) as Arc<dyn LockManager>)
        }
        #[cfg(not(feature = "sqlite-backend"))]
        LockBackend::Sqlite => Err(crate::LockError::BackendError(
            "SQLite backend not available. Enable 'sqlite-backend' feature.".to_string(),
        )),
        #[cfg(feature = "postgres-backend")]
        LockBackend::Postgres => {
            // PostgreSQL lock manager not yet implemented
            // Use SQLite or Redis instead
            Err(crate::LockError::BackendError(
                "PostgreSQL backend not yet implemented. Use SQLite or Redis.".to_string(),
            ))
        }
        #[cfg(not(feature = "postgres-backend"))]
        LockBackend::Postgres => Err(crate::LockError::BackendError(
            "PostgreSQL backend not available. Enable 'postgres-backend' feature.".to_string(),
        )),
        #[cfg(feature = "redis-backend")]
        LockBackend::Redis => {
            use crate::redis::RedisLockManager;
            let url = database_url
                .ok_or_else(|| crate::LockError::BackendError("Redis URL required".to_string()))?;
            tracing::info!(backend = "Redis", url = %url, "Creating Redis lock manager");
            RedisLockManager::new(&url)
                .await
                .map(|m| Arc::new(m) as Arc<dyn LockManager>)
        }
        #[cfg(not(feature = "redis-backend"))]
        LockBackend::Redis => Err(crate::LockError::BackendError(
            "Redis backend not available. Enable 'redis-backend' feature.".to_string(),
        )),
        #[cfg(feature = "ddb-backend")]
        LockBackend::DynamoDB => {
            use crate::ddb::DynamoDBLockManager;
            // DynamoDB URL format: "region:table_name" or "region:table_name:endpoint_url"
            // Default region: "us-east-1"
            let (region, table_name, endpoint_url) = if let Some(url) = database_url {
                let parts: Vec<&str> = url.split(':').collect();
                match parts.len() {
                    1 => {
                        // Just table name, use default region
                        ("us-east-1".to_string(), parts[0].to_string(), None)
                    }
                    2 => {
                        // region:table_name
                        (parts[0].to_string(), parts[1].to_string(), None)
                    }
                    _ => {
                        // region:table_name:endpoint_url
                        (
                            parts[0].to_string(),
                            parts[1].to_string(),
                            Some(parts[2..].join(":")),
                        )
                    }
                }
            } else {
                return Err(crate::LockError::BackendError(
                    "DynamoDB table name required (format: 'region:table_name' or 'region:table_name:endpoint_url')".to_string()
                ));
            };
            tracing::info!(backend = "DynamoDB", region = %region, table = %table_name, "Creating DynamoDB lock manager");
            DynamoDBLockManager::new(region, table_name, endpoint_url)
                .await
                .map(|m| Arc::new(m) as Arc<dyn LockManager>)
        }
        #[cfg(not(feature = "ddb-backend"))]
        LockBackend::DynamoDB => Err(crate::LockError::BackendError(
            "DynamoDB backend not available. Enable 'ddb-backend' feature.".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::create_lock_manager_from_runtime;
    use plexspaces_proto::node::v1::RuntimeConfig;
    use plexspaces_proto::storage::v1::{
        storage_provider_config, RedisBackendConfig, SharedDbConfig, StorageProvider,
        StorageProviderConfig,
    };

    #[tokio::test]
    async fn test_create_lock_manager_from_runtime_shared_db_memory() {
        let runtime = RuntimeConfig {
            db: Some(SharedDbConfig {
                connection_string: "sqlite::memory:".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = create_lock_manager_from_runtime(&runtime).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_lock_manager_from_runtime_redis_provider_without_feature_errors() {
        let runtime = RuntimeConfig {
            db: Some(SharedDbConfig {
                connection_string: "sqlite::memory:".to_string(),
                ..Default::default()
            }),
            locks_provider: Some(StorageProviderConfig {
                provider: StorageProvider::StorageProviderRedis as i32,
                config: Some(storage_provider_config::Config::Redis(RedisBackendConfig {
                    url: "redis://localhost:6379".to_string(),
                    ..Default::default()
                })),
            }),
            ..Default::default()
        };

        let result = create_lock_manager_from_runtime(&runtime).await;
        #[cfg(feature = "redis-backend")]
        assert!(result.is_ok());
        #[cfg(not(feature = "redis-backend"))]
        assert!(result.is_err());
    }
}
