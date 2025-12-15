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

//! Error types for blob storage service

use thiserror::Error;

/// Result type for blob operations
pub type BlobResult<T> = Result<T, BlobError>;

/// Error types for blob storage operations
#[derive(Error, Debug)]
pub enum BlobError {
    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Blob not found: {0}")]
    NotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Object store error: {0}")]
    ObjectStoreError(#[from] object_store::Error),

    #[error("SQL error: {0}")]
    SqlError(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Internal error: {0}")]
    InternalError(String),
}

impl From<&str> for BlobError {
    fn from(s: &str) -> Self {
        BlobError::InternalError(s.to_string())
    }
}

impl From<String> for BlobError {
    fn from(s: String) -> Self {
        BlobError::InternalError(s)
    }
}
