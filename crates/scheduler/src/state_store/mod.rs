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

//! State store for scheduling requests.
//!
//! ## Purpose
//! Stores scheduling requests and their status for tracking and recovery.
//! Uses SQL database for efficient queries by status (especially PENDING for recovery).
//!
//! ## Design
//! Following the workflow_executions pattern:
//! - SQL table for queryable metadata
//! - Indexed on status for efficient PENDING queries
//! - Atomic updates via SQL transactions
//! - Recovery support via query_pending_requests()

use async_trait::async_trait;
use plexspaces_common::{resolve_shared_db_backend, SharedDbBackend};
use plexspaces_core::RequestContext;
use plexspaces_proto::scheduling::v1::SchedulingRequest;
use plexspaces_proto::storage::v1::SharedDbConfig;
use std::error::Error;
use std::sync::Arc;

/// Trait for scheduling state store.
///
/// ## Purpose
/// Stores scheduling requests and their status for tracking and recovery.
///
/// ## Multi-Tenancy
/// **CRITICAL**: All methods require `RequestContext` for proper tenant/namespace isolation.
/// All operations MUST filter by tenant_id and namespace to prevent data leakage between tenants.
///
/// ## Backend Support
/// - SQL: PostgreSQL/SQLite for persistence (use `:memory:` for in-memory testing)
/// - DynamoDB: AWS DynamoDB for distributed persistence
#[async_trait]
pub trait SchedulingStateStore: Send + Sync {
    /// Store a scheduling request
    ///
    /// ## Arguments
    /// * `ctx` - Request context (REQUIRED for tenant/namespace isolation)
    /// * `request` - Scheduling request to store
    ///
    /// ## Security
    /// The request's tenant_id and namespace MUST match the context's tenant_id and namespace.
    /// The implementation MUST validate this to prevent data leakage.
    async fn store_request(
        &self,
        ctx: &RequestContext,
        request: SchedulingRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;

    /// Get a scheduling request by ID
    ///
    /// ## Arguments
    /// * `ctx` - Request context (REQUIRED for tenant/namespace isolation)
    /// * `request_id` - Request ID to lookup
    ///
    /// ## Security
    /// MUST only return requests that match the context's tenant_id and namespace.
    /// MUST NOT return requests from other tenants/namespaces.
    async fn get_request(
        &self,
        ctx: &RequestContext,
        request_id: &str,
    ) -> Result<Option<SchedulingRequest>, Box<dyn Error + Send + Sync>>;

    /// Update a scheduling request
    ///
    /// ## Arguments
    /// * `ctx` - Request context (REQUIRED for tenant/namespace isolation)
    /// * `request` - Scheduling request to update
    ///
    /// ## Security
    /// The request's tenant_id and namespace MUST match the context's tenant_id and namespace.
    /// The implementation MUST validate this to prevent data leakage.
    async fn update_request(
        &self,
        ctx: &RequestContext,
        request: SchedulingRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;

    /// Query PENDING requests (for recovery on startup)
    ///
    /// ## Arguments
    /// * `ctx` - Request context (REQUIRED for tenant/namespace isolation)
    ///
    /// ## Security
    /// MUST only return PENDING requests that match the context's tenant_id and namespace.
    /// MUST NOT return requests from other tenants/namespaces.
    async fn query_pending_requests(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<SchedulingRequest>, Box<dyn Error + Send + Sync>>;
}

#[cfg(any(feature = "sqlite-backend", feature = "postgres-backend"))]
pub mod sql;

#[cfg(feature = "ddb-backend")]
pub mod ddb;

/// Create scheduling state store from shared database config.
pub async fn create_state_store_from_shared_db(
    config: &SharedDbConfig,
) -> Result<Arc<dyn SchedulingStateStore>, Box<dyn Error + Send + Sync>> {
    match resolve_shared_db_backend(config)? {
        SharedDbBackend::Sqlite { database_path, .. } => {
            #[cfg(feature = "sqlite-backend")]
            {
                use sql::SqliteSchedulingStateStore;
                let store = SqliteSchedulingStateStore::new(&database_path).await?;
                Ok(Arc::new(store))
            }
            #[cfg(not(feature = "sqlite-backend"))]
            {
                Err("SQLite backend not enabled. Enable 'sqlite-backend' feature.".into())
            }
        }
        SharedDbBackend::Postgres { connection_string } => Err(format!(
            "PostgreSQL scheduler state store is not implemented for shared db '{}'",
            connection_string
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::create_state_store_from_shared_db;
    use plexspaces_proto::storage::v1::SharedDbConfig;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_create_state_store_from_shared_db_sqlite_memory() {
        let config = SharedDbConfig {
            connection_string: "sqlite::memory:".to_string(),
            ..Default::default()
        };

        let store = create_state_store_from_shared_db(&config).await.unwrap();
        assert!(Arc::strong_count(&store) >= 1);
    }

    #[tokio::test]
    async fn test_create_state_store_from_shared_db_rejects_postgres_until_supported() {
        let config = SharedDbConfig {
            connection_string: "postgres://localhost/plexspaces".to_string(),
            ..Default::default()
        };

        let result = create_state_store_from_shared_db(&config).await;
        assert!(result.is_err(), "postgres scheduler state store should fail fast");
        assert!(
            result.err().unwrap().to_string().contains("not implemented"),
            "error should explain that postgres support is not implemented"
        );
    }
}
