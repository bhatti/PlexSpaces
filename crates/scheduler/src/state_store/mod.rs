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
use plexspaces_core::RequestContext;
use plexspaces_proto::scheduling::v1::SchedulingRequest;
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

/// Create scheduling state store from shared database URL
///
/// ## Purpose
/// Factory function that creates appropriate state store backend based on
/// the shared database connection string from RuntimeConfig.db.
///
/// ## Arguments
/// - `db_url`: Database connection string (e.g., "sqlite:///path/to/db", "postgres://...")
///
/// ## Returns
/// Arc'd state store implementing SchedulingStateStore trait
///
/// ## Errors
/// - InvalidConfiguration if db_url is malformed
/// - ConnectionError if backend can't connect
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_scheduler::state_store::create_state_store;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let db_url = "sqlite:///tmp/scheduler.db";
/// let store = create_state_store(&db_url).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_state_store(
    db_url: &str,
) -> Result<Arc<dyn SchedulingStateStore>, Box<dyn Error + Send + Sync>> {
    // Determine backend type from connection string
    if db_url.contains(":memory:") || db_url.starts_with("sqlite:") || db_url.starts_with("sqlite://") {
        #[cfg(feature = "sqlite-backend")]
        {
            // Extract path from SQLite connection string
            let path = if db_url == ":memory:" || db_url.contains(":memory:") {
                ":memory:".to_string()
            } else if db_url.starts_with("sqlite:///") {
                // Format: "sqlite:///absolute/path" - preserve leading /
                // Example: "sqlite:///Users/sbhatti/plexspaces/db/plexspaces.db?mode=rwc"
                // After strip_prefix("sqlite:///"): "Users/sbhatti/plexspaces/db/plexspaces.db?mode=rwc"
                // After split('?'): "Users/sbhatti/plexspaces/db/plexspaces.db" (no leading /)
                // Result: "/Users/sbhatti/plexspaces/db/plexspaces.db"
                let extracted = db_url.strip_prefix("sqlite:///")
                    .and_then(|s| s.split('?').next()) // Remove query parameters like ?mode=rwc
                    .unwrap_or(db_url);
                // extracted does NOT have leading / after strip_prefix, so add it back
                format!("/{}", extracted)
            } else if db_url.starts_with("sqlite://") {
                // Format: "sqlite://relative/path" - no leading /
                db_url.strip_prefix("sqlite://")
                    .and_then(|s| s.split('?').next())
                    .unwrap_or(db_url)
                    .to_string()
            } else if db_url.starts_with("sqlite:") {
                // Format: "sqlite:path" - may or may not have leading /
                db_url.strip_prefix("sqlite:")
                    .and_then(|s| s.split('?').next())
                    .unwrap_or(db_url)
                    .to_string()
            } else {
                return Err("Invalid SQLite connection string format".into());
            };
            
            // Ensure directory exists for file-based SQLite databases
            if path != ":memory:" && !path.is_empty() {
                if let Some(parent) = std::path::Path::new(&path).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        format!("Failed to create database directory '{}': {}", parent.display(), e)
                    })?;
                }
            }
            
            tracing::debug!(
                db_url = %db_url,
                extracted_path = %path,
                "Creating scheduler state store with extracted path"
            );
            
            use sql::SqliteSchedulingStateStore;
            let store = SqliteSchedulingStateStore::new(&path).await?;
            Ok(Arc::new(store))
        }
        #[cfg(not(feature = "sqlite-backend"))]
        {
            Err("SQLite backend not enabled. Enable 'sqlite-backend' feature.".into())
        }
    } else if db_url.starts_with("postgres://") || db_url.starts_with("postgresql://") {
        // PostgreSQL support not yet implemented - fallback to in-memory SQLite
        tracing::warn!(
            db_url = %db_url,
            "PostgreSQL backend for scheduler not yet implemented, using in-memory SQLite fallback"
        );
        #[cfg(feature = "sqlite-backend")]
        {
            use sql::SqliteSchedulingStateStore;
            let store = SqliteSchedulingStateStore::new(":memory:").await?;
            Ok(Arc::new(store))
        }
        #[cfg(not(feature = "sqlite-backend"))]
        {
            Err("PostgreSQL backend not yet implemented and sqlite-backend not enabled".into())
        }
    } else {
        // Fallback to in-memory SQLite for unsupported databases
        tracing::warn!(
            db_url = %db_url,
            "Unsupported database type for scheduler, using in-memory SQLite fallback"
        );
        #[cfg(feature = "sqlite-backend")]
        {
            use sql::SqliteSchedulingStateStore;
            let store = SqliteSchedulingStateStore::new(":memory:").await?;
            Ok(Arc::new(store))
        }
        #[cfg(not(feature = "sqlite-backend"))]
        {
            Err(format!("Unsupported database URL: {} (and sqlite-backend not enabled)", db_url).into())
        }
    }
}
