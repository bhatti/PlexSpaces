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

//! Factory helpers for creating a `ChannelRepository` from node-level shared DB config.
//!
//! ## Pattern
//! Mirrors `create_keyvalue_stores_from_shared_db` in `plexspaces-keyvalue` and
//! `create_repository_from_shared_db` in `plexspaces-object-registry`.
//!
//! Call this once during node initialization with the same `SharedDbConfig` used
//! for journals and KV stores.  The resulting `Arc<dyn ChannelRepository>` is
//! registered with `ServiceLocatorImpl` and handed to the actor factory.

use crate::{ChannelError, ChannelResult};
use crate::repository::ChannelRepository;
use std::sync::Arc;

/// Create a `ChannelRepository` backed by the node-wide shared SQLite or PostgreSQL pool.
///
/// Dispatches to `SqliteChannelRepository` or `PostgresChannelRepository`
/// based on the `SharedDbConfig` backend type, following the same pattern as
/// `create_keyvalue_stores_from_shared_db` in `plexspaces-keyvalue`.
#[cfg(any(
    all(feature = "sqlite-backend", feature = "repository"),
    all(feature = "postgres-backend", feature = "repository")
))]
pub async fn create_channel_repository_from_shared_db(
    shared_db: &plexspaces_proto::storage::v1::SharedDbConfig,
) -> ChannelResult<Arc<dyn ChannelRepository>> {
    use plexspaces_common::{resolve_shared_db_backend, SharedDbBackend};

    let backend = resolve_shared_db_backend(shared_db)
        .map_err(|e| ChannelError::BackendError(format!("Invalid shared DB config: {}", e)))?;

    match backend {
        #[cfg(all(feature = "sqlite-backend", feature = "repository"))]
        SharedDbBackend::Sqlite { connection_string, .. } => {
            use crate::sql::SqliteChannelRepository;
            use sqlx::sqlite::SqlitePoolOptions;
            let pool = SqlitePoolOptions::new()
                .max_connections(4)
                .connect(&connection_string)
                .await
                .map_err(|e| {
                    ChannelError::BackendError(format!(
                        "Failed to open SQLite pool for ChannelRepository: {}",
                        e
                    ))
                })?;
            Ok(Arc::new(SqliteChannelRepository::new(pool).await?))
        }
        #[cfg(all(feature = "postgres-backend", feature = "repository"))]
        SharedDbBackend::Postgres { connection_string, .. } => {
            use crate::sql::PostgresChannelRepository;
            use sqlx::postgres::PgPoolOptions;
            let pool = PgPoolOptions::new()
                .max_connections(10)
                .connect(&connection_string)
                .await
                .map_err(|e| {
                    ChannelError::BackendError(format!(
                        "Failed to open PostgreSQL pool for ChannelRepository: {}",
                        e
                    ))
                })?;
            Ok(Arc::new(PostgresChannelRepository::from_shared_pool(pool).await?))
        }
        #[allow(unreachable_patterns)]
        _ => Err(ChannelError::BackendError(
            "No matching channel repository backend enabled for this shared DB config. \
             Enable 'sqlite-backend'+'repository' or 'postgres-backend'+'repository'."
                .to_string(),
        )),
    }
}
