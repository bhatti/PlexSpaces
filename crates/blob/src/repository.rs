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

//! Repository trait and implementations for blob metadata storage

use async_trait::async_trait;
use crate::BlobResult;
use plexspaces_proto::storage::v1::BlobMetadata;
use plexspaces_core::RequestContext;

/// Repository trait for blob metadata storage
///
/// All methods require RequestContext for tenant isolation.
/// All queries automatically filter by tenant_id from the context.
#[async_trait]
pub trait BlobRepository: Send + Sync {
    /// Get blob metadata by ID
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `blob_id` - Blob identifier
    ///
    /// ## Returns
    /// Blob metadata if found, None otherwise.
    /// Only returns blobs belonging to the tenant in the context.
    async fn get(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> BlobResult<Option<BlobMetadata>>;

    /// Get blob metadata by SHA256
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `sha256` - SHA256 hash of blob content
    ///
    /// ## Returns
    /// Blob metadata if found, None otherwise.
    /// Only searches within the tenant's namespace from the context.
    async fn get_by_sha256(
        &self,
        ctx: &RequestContext,
        sha256: &str,
    ) -> BlobResult<Option<BlobMetadata>>;

    /// Save blob metadata
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `metadata` - Blob metadata to save
    ///
    /// ## Notes
    /// The metadata's tenant_id and namespace must match the context.
    async fn save(
        &self,
        ctx: &RequestContext,
        metadata: &BlobMetadata,
    ) -> BlobResult<()>;

    /// Update blob metadata
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `metadata` - Blob metadata to update
    ///
    /// ## Notes
    /// Only updates blobs belonging to the tenant in the context.
    async fn update(
        &self,
        ctx: &RequestContext,
        metadata: &BlobMetadata,
    ) -> BlobResult<()>;

    /// Delete blob metadata
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `blob_id` - Blob identifier
    ///
    /// ## Notes
    /// Only deletes blobs belonging to the tenant in the context.
    async fn delete(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> BlobResult<()>;

    /// List blobs with filtering
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `filters` - Filter criteria
    /// * `page_size` - Number of results per page
    /// * `offset` - Offset for pagination
    ///
    /// ## Returns
    /// Tuple of (results, total_count).
    /// Only returns blobs belonging to the tenant in the context.
    async fn list(
        &self,
        ctx: &RequestContext,
        filters: &ListFilters,
        page_size: i64,
        offset: i64,
    ) -> BlobResult<(Vec<BlobMetadata>, i64)>; // (results, total_count)

    /// Find expired blobs
    ///
    /// ## Arguments
    /// * `ctx` - Request context (required for tenant isolation)
    /// * `limit` - Maximum number of results
    ///
    /// ## Returns
    /// List of expired blobs.
    /// Only returns blobs belonging to the tenant in the context.
    async fn find_expired(
        &self,
        ctx: &RequestContext,
        limit: i64,
    ) -> BlobResult<Vec<BlobMetadata>>;
}

/// Filters for listing blobs
#[derive(Debug, Clone, Default)]
pub struct ListFilters {
    pub name_prefix: Option<String>,
    pub blob_group: Option<String>,
    pub kind: Option<String>,
    pub sha256: Option<String>,
}

#[cfg(feature = "sql-backend")]
#[path = "repository/sql.rs"]
pub mod sql;

#[cfg(feature = "ddb-backend")]
#[path = "repository/ddb.rs"]
pub mod ddb;

#[cfg(feature = "ddb-backend")]
pub use ddb::DynamoDBBlobRepository;
