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

//! Proto-first repository factory helpers for Object Registry storage.

use crate::repository::{ObjectRegistryRepository, RepositoryError, RepositoryResult};
use plexspaces_common::{resolve_shared_db_backend, SharedDbBackend};
use plexspaces_proto::storage::v1::{
    storage_provider_config, SharedDbConfig, StorageProvider, StorageProviderConfig,
};
use std::sync::Arc;

/// Create an object-registry repository from the shared database configuration.
pub async fn create_repository_from_shared_db(
    config: &SharedDbConfig,
) -> RepositoryResult<Arc<dyn ObjectRegistryRepository>> {
    match resolve_shared_db_backend(config).map_err(RepositoryError::InvalidInput)? {
        SharedDbBackend::Sqlite { database_path, .. } => {
            #[cfg(feature = "sql-backend")]
            {
                use crate::repository::SqliteObjectRegistryRepository;
                let repo = SqliteObjectRegistryRepository::new(&database_path).await?;
                Ok(Arc::new(repo))
            }
            #[cfg(not(feature = "sql-backend"))]
            {
                Err(RepositoryError::Storage(
                    "sqlite object-registry backend requires 'sql-backend' feature".to_string(),
                ))
            }
        }
        SharedDbBackend::Postgres { connection_string } => {
            #[cfg(feature = "sql-backend")]
            {
                use crate::repository::PostgresObjectRegistryRepository;
                let repo = PostgresObjectRegistryRepository::new(&connection_string).await?;
                Ok(Arc::new(repo))
            }
            #[cfg(not(feature = "sql-backend"))]
            {
                Err(RepositoryError::Storage(
                    "postgres object-registry backend requires 'sql-backend' feature".to_string(),
                ))
            }
        }
    }
}

/// Create an object-registry repository from provider config, with shared-db fallback.
pub async fn create_repository_from_storage_config(
    provider_config: &StorageProviderConfig,
    shared_db: Option<&SharedDbConfig>,
) -> RepositoryResult<Arc<dyn ObjectRegistryRepository>> {
    match StorageProvider::try_from(provider_config.provider)
        .unwrap_or(StorageProvider::StorageProviderUnspecified)
    {
        StorageProvider::StorageProviderUnspecified | StorageProvider::StorageProviderRedis => {
            let shared_db = shared_db.ok_or_else(|| {
                RepositoryError::InvalidInput(
                    "object registry requires shared db when provider is unspecified".to_string(),
                )
            })?;
            create_repository_from_shared_db(shared_db).await
        }
        StorageProvider::StorageProviderSqlite => {
            let sqlite = match provider_config.config.as_ref() {
                Some(storage_provider_config::Config::Sqlite(sqlite)) => sqlite,
                _ => {
                    return Err(RepositoryError::InvalidInput(
                        "sqlite object-registry provider requires sqlite config".to_string(),
                    ))
                }
            };
            #[cfg(feature = "sql-backend")]
            {
                use crate::repository::SqliteObjectRegistryRepository;
                let repo = SqliteObjectRegistryRepository::new(&sqlite.database_path).await?;
                Ok(Arc::new(repo))
            }
            #[cfg(not(feature = "sql-backend"))]
            {
                Err(RepositoryError::Storage(
                    "sqlite object-registry backend requires 'sql-backend' feature".to_string(),
                ))
            }
        }
        StorageProvider::StorageProviderPostgres => {
            let postgres = match provider_config.config.as_ref() {
                Some(storage_provider_config::Config::Postgres(postgres)) => postgres,
                _ => {
                    return Err(RepositoryError::InvalidInput(
                        "postgres object-registry provider requires postgres config".to_string(),
                    ))
                }
            };
            create_repository_from_shared_db(postgres).await
        }
        StorageProvider::StorageProviderDynamodb => {
            #[cfg(feature = "ddb-backend")]
            {
                let dynamodb = match provider_config.config.as_ref() {
                    Some(storage_provider_config::Config::Dynamodb(dynamodb)) => dynamodb,
                    _ => {
                        return Err(RepositoryError::InvalidInput(
                            "dynamodb object-registry provider requires dynamodb config"
                                .to_string(),
                        ))
                    }
                };
                use crate::repository::DynamoDBObjectRegistryRepository;
                let table_name = if dynamodb.table_prefix.is_empty() {
                    "plexspaces-object-registry".to_string()
                } else {
                    format!("{}object-registry", dynamodb.table_prefix)
                };
                let repo = DynamoDBObjectRegistryRepository::new(
                    dynamodb.region.clone(),
                    table_name,
                    (!dynamodb.endpoint_url.is_empty()).then(|| dynamodb.endpoint_url.clone()),
                )
                .await?;
                Ok(Arc::new(repo))
            }
            #[cfg(not(feature = "ddb-backend"))]
            {
                Err(RepositoryError::Storage(
                    "dynamodb object-registry backend requires 'ddb-backend' feature".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{create_repository_from_shared_db, create_repository_from_storage_config};
    use plexspaces_proto::storage::v1::{
        storage_provider_config, SharedDbConfig, SqliteBackendConfig, StorageProvider,
        StorageProviderConfig,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn test_create_repository_from_shared_db_sqlite_memory() {
        let config = SharedDbConfig {
            connection_string: "sqlite::memory:".to_string(),
            ..Default::default()
        };

        let repo = create_repository_from_shared_db(&config).await.unwrap();
        assert!(Arc::strong_count(&repo) >= 1);
    }

    #[tokio::test]
    async fn test_create_repository_from_storage_config_sqlite() {
        let provider = StorageProviderConfig {
            provider: StorageProvider::StorageProviderSqlite as i32,
            config: Some(storage_provider_config::Config::Sqlite(
                SqliteBackendConfig {
                    database_path: ":memory:".to_string(),
                    ..Default::default()
                },
            )),
        };

        let repo = create_repository_from_storage_config(&provider, None)
            .await
            .unwrap();
        assert!(Arc::strong_count(&repo) >= 1);
    }

    #[tokio::test]
    async fn test_create_repository_from_storage_config_requires_shared_db_for_unspecified() {
        let provider = StorageProviderConfig::default();
        let result = create_repository_from_storage_config(&provider, None).await;
        assert!(result.is_err());
    }
}
