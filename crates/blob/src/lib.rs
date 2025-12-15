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

//! PlexSpaces Blob Storage Service
//!
//! ## Purpose
//! Provides S3-compatible blob storage with metadata management for PlexSpaces.
//! Supports multiple backends: S3, MinIO, GCP Cloud Storage, Azure Blob Storage.
//!
//! ## Architecture
//! - **Blob Storage**: Actual binary data stored in S3-compatible backend
//! - **Metadata Storage**: BlobMetadata stored in SQL (SQLite/PostgreSQL) for querying
//! - **Multi-tenancy**: Isolation via tenant_id and namespace
//! - **Path Structure**: /plexspaces/{tenant_id}/{namespace}/{blob_id}
//!
//! ## Usage
//! ```rust,no_run
//! use plexspaces_blob::{BlobService, BlobConfig, BlobMetadata};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = BlobConfig {
//!     backend: "minio".to_string(),
//!     bucket: "plexspaces".to_string(),
//!     endpoint: Some("http://localhost:9000".to_string()),
//!     // ... other config
//! };
//!
//! let blob_service = BlobService::new(config).await?;
//!
//! // Upload a blob
//! let metadata = blob_service.upload_blob(
//!     "tenant-1",
//!     "namespace-1",
//!     "my-file.txt",
//!     b"file contents".to_vec(),
//! ).await?;
//!
//! // Download a blob
//! let data = blob_service.download_blob(&metadata.blob_id).await?;
//! # Ok(())
//! # }
//! ```

pub mod config_ext;
pub mod error;
pub mod helpers;
pub mod repository;
pub mod service;

#[cfg(feature = "server")]
pub mod server;

// Re-export proto types (after buf generate)
pub use plexspaces_proto::storage::v1::{BlobConfig, BlobMetadata};

pub use config_ext::BlobConfigExt;
pub use error::{BlobError, BlobResult};
pub use repository::BlobRepository;
pub use service::BlobService;
pub use helpers::{get_storage_path, is_expired, validate_metadata};

#[cfg(feature = "server")]
pub use server::grpc::BlobServiceImpl;
#[cfg(feature = "server")]
pub use server::http::BlobHttpHandler;
