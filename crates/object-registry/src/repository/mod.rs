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

//! # Object Registry Repository
//!
//! ## Purpose
//! Provides the storage abstraction for object registrations with indexed columns
//! for fast queries while preserving full registration data as a blob.
//!
//! ## Design
//! - **Repository trait**: Defines CRUD operations with filter support
//! - **Indexed columns**: object_type, node_id, health_status, last_heartbeat, etc.
//! - **Registration blob**: Full ObjectRegistration protobuf preserved
//! - **Multi-backend**: SQLite (use `:memory:` for tests), PostgreSQL (production), DynamoDB (AWS)
//!
//! ## Architecture Context
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │              ObjectRegistryImpl                          │
//! │  register() / unregister() / lookup() / discover()      │
//! └────────────────────┬────────────────────────────────────┘
//!                      │
//!                      ▼
//! ┌─────────────────────────────────────────────────────────┐
//! │         ObjectRegistryRepository (trait)                │
//! │  put() / get() / delete() / discover() / heartbeat()    │
//! └────────────────────┬────────────────────────────────────┘
//!                      │
//!         ┌───────────┼───────────┐
//!         ▼           ▼           ▼
//!    ┌────────┐  ┌────────┐  ┌────────┐
//!    │ SQLite │  │Postgres│  │DynamoDB│
//!    └────────┘  └────────┘  └────────┘
//! ```

#[cfg(feature = "sql-backend")]
pub mod sql;

#[cfg(feature = "ddb-backend")]
pub mod ddb;

#[cfg(feature = "sql-backend")]
pub use sql::{PostgresObjectRegistryRepository, SqliteObjectRegistryRepository};

#[cfg(feature = "ddb-backend")]
pub use ddb::DynamoDBObjectRegistryRepository;

use async_trait::async_trait;
use plexspaces_common::RequestContext;
use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};
use std::fmt::Debug;

/// Error type for repository operations
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    /// Object not found
    #[error("Object not found: {0}")]
    NotFound(String),

    /// Object already exists
    #[error("Object already exists: {0}")]
    AlreadyExists(String),

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),
}

/// Result type for repository operations
pub type RepositoryResult<T> = Result<T, RepositoryError>;

/// Filter criteria for discover queries
#[derive(Debug, Clone, Default)]
pub struct DiscoverFilter {
    /// Filter by object type
    pub object_type: Option<ObjectType>,
    /// Filter by object category
    pub object_category: Option<String>,
    /// Filter by node ID
    pub node_id: Option<String>,
    /// Filter by health status
    pub health_status: Option<HealthStatus>,
    /// Filter objects with last_heartbeat before this timestamp (Unix seconds)
    /// Used to find stale registrations
    pub last_heartbeat_before: Option<i64>,
    /// Filter objects with last_heartbeat after this timestamp (Unix seconds)
    /// Used to find recently active registrations
    pub last_heartbeat_after: Option<i64>,
    /// Filter by labels (all must match)
    pub labels: Option<Vec<String>>,
    /// Filter by capabilities (all must match)
    pub capabilities: Option<Vec<String>>,
}

/// Object Registry Repository trait
///
/// ## Purpose
/// Provides storage abstraction for object registrations with indexed columns
/// for efficient queries.
///
/// ## Design
/// - **Primary key**: (tenant_id, namespace, object_id)
/// - **Indexed columns**: object_type, node_id, health_status, last_heartbeat, object_category
/// - **Blob storage**: Full ObjectRegistration preserved for complete data
///
/// ## Multi-tenancy
/// All operations are scoped to tenant_id and namespace from RequestContext.
#[async_trait]
pub trait ObjectRegistryRepository: Send + Sync + Debug {
    /// Store or update an object registration
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace
    /// * `registration` - ObjectRegistration to store
    ///
    /// ## Behavior
    /// - Upsert: creates if not exists, updates if exists
    /// - Extracts indexed fields from registration for fast queries
    /// - Stores full registration as blob
    async fn put(
        &self,
        ctx: &RequestContext,
        registration: &ObjectRegistration,
    ) -> RepositoryResult<()>;

    /// Get an object registration by ID
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace
    /// * `object_id` - Object identifier
    ///
    /// ## Returns
    /// - `Ok(Some(registration))` if found
    /// - `Ok(None)` if not found
    async fn get(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> RepositoryResult<Option<ObjectRegistration>>;

    /// Delete an object registration
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace
    /// * `object_id` - Object identifier
    ///
    /// ## Returns
    /// - `Ok(())` on success (idempotent - succeeds even if not found)
    async fn delete(&self, ctx: &RequestContext, object_id: &str) -> RepositoryResult<()>;

    /// Discover objects matching filter criteria
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace
    /// * `filter` - Filter criteria (object_type, health_status, etc.)
    /// * `offset` - Number of results to skip
    /// * `limit` - Maximum number of results to return
    ///
    /// ## Returns
    /// Vector of matching ObjectRegistrations
    ///
    /// ## Performance
    /// Uses indexed columns for efficient filtering. O(log n + k) where k = results.
    async fn discover(
        &self,
        ctx: &RequestContext,
        filter: &DiscoverFilter,
        offset: usize,
        limit: usize,
    ) -> RepositoryResult<Vec<ObjectRegistration>>;

    /// Update heartbeat timestamp for an object
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace
    /// * `object_id` - Object identifier
    /// * `timestamp` - New heartbeat timestamp (Unix seconds)
    ///
    /// ## Returns
    /// - `Ok(())` on success
    /// - `Err(NotFound)` if object doesn't exist
    ///
    /// ## Performance
    /// Single UPDATE on indexed column - no blob read/write required.
    async fn update_heartbeat(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        timestamp: i64,
    ) -> RepositoryResult<()>;

    /// Update health status for an object
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace
    /// * `object_id` - Object identifier
    /// * `status` - New health status
    ///
    /// ## Returns
    /// - `Ok(())` on success
    /// - `Err(NotFound)` if object doesn't exist
    async fn update_health_status(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        status: HealthStatus,
    ) -> RepositoryResult<()>;

    /// Count objects matching filter criteria
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace
    /// * `filter` - Filter criteria
    ///
    /// ## Returns
    /// Count of matching objects
    async fn count(&self, ctx: &RequestContext, filter: &DiscoverFilter)
        -> RepositoryResult<usize>;

    /// List distinct tenant ids for registrations of the given object type.
    ///
    /// ## Purpose
    /// Provides a storage-backed source of truth for administrative tenant discovery.
    /// When auth is enabled for a non-admin caller, the current tenant is returned
    /// directly instead of querying cross-tenant state.
    async fn list_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
        offset: usize,
        limit: usize,
    ) -> RepositoryResult<Vec<String>>;

    /// Count distinct tenant ids for registrations of the given object type.
    async fn count_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
    ) -> RepositoryResult<usize>;

    /// Check if an object exists
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace
    /// * `object_id` - Object identifier
    ///
    /// ## Returns
    /// true if exists, false otherwise
    async fn exists(&self, ctx: &RequestContext, object_id: &str) -> RepositoryResult<bool>;
}
