// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! PlexSpaces Blob Storage Service
//!
//! ## Purpose
//! Provides S3-compatible blob storage with metadata management for PlexSpaces.
//! Supports multiple backends: embedded S3-compatible store (default, auto-started), AWS S3, GCP Cloud Storage, Azure Blob Storage.
//!
//! ## Architecture
//! - **Blob Storage**: Actual binary data stored in S3-compatible backend
//! - **Metadata Storage**: BlobMetadata stored in SQL (SQLite/PostgreSQL) for querying
//! - **Multi-tenancy**: Isolation via tenant_id and namespace
//! - **Path Structure**: /plexspaces/{tenant_id}/{namespace}/{blob_id}

pub mod config_ext;
pub mod embedded_object_store;
pub mod error;
pub mod helpers;
pub mod repository;
pub mod service;

#[cfg(feature = "sql-backend")]
pub mod node_startup;

#[cfg(feature = "presigned-urls")]
pub mod presigned;
#[cfg(not(feature = "presigned-urls"))]
mod presigned {
    // Stub module when presigned-urls feature is disabled
    use chrono::Duration;
    pub async fn generate_presigned_url(
        _config: &crate::BlobConfig,
        _storage_path: &str,
        _operation: &str,
        _expires_after: Duration,
    ) -> Result<String, crate::BlobError> {
        Err(crate::BlobError::InvalidInput(
            "Presigned URLs require presigned-urls feature".to_string(),
        ))
    }
}

#[cfg(feature = "server")]
pub mod server;

// Re-export proto types (after buf generate)
pub use plexspaces_proto::storage::v1::{BlobConfig, BlobMetadata};

pub use config_ext::BlobConfigExt;
pub use error::{BlobError, BlobResult};
pub use helpers::{get_storage_path, is_expired, validate_metadata};
pub use repository::BlobRepository;
#[cfg(feature = "ddb-backend")]
pub use repository::{DynamoDBBlobRepository, ListFilters};
pub use service::BlobService;

#[cfg(feature = "server")]
pub use server::grpc::BlobServiceImpl;
#[cfg(feature = "server")]
pub use server::http::BlobHttpHandler;
#[cfg(feature = "server")]
pub use server::http_axum::create_blob_router;
