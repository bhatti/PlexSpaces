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

/// Repository trait for blob metadata storage
#[async_trait]
pub trait BlobRepository: Send + Sync {
    /// Get blob metadata by ID
    async fn get(&self, blob_id: &str) -> BlobResult<Option<BlobMetadata>>;

    /// Get blob metadata by SHA256
    async fn get_by_sha256(
        &self,
        tenant_id: &str,
        namespace: &str,
        sha256: &str,
    ) -> BlobResult<Option<BlobMetadata>>;

    /// Save blob metadata
    async fn save(&self, metadata: &BlobMetadata) -> BlobResult<()>;

    /// Update blob metadata
    async fn update(&self, metadata: &BlobMetadata) -> BlobResult<()>;

    /// Delete blob metadata
    async fn delete(&self, blob_id: &str) -> BlobResult<()>;

    /// List blobs with filtering
    async fn list(
        &self,
        tenant_id: &str,
        namespace: &str,
        filters: &ListFilters,
        page_size: i64,
        offset: i64,
    ) -> BlobResult<(Vec<BlobMetadata>, i64)>; // (results, total_count)

    /// Find expired blobs
    async fn find_expired(
        &self,
        tenant_id: Option<&str>,
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
