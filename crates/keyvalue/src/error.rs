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

//! Error types for KeyValue operations.

use thiserror::Error;

/// Result type for KeyValue operations.
pub type KVResult<T> = Result<T, KVError>;

/// Errors that can occur during KeyValue operations.
#[derive(Error, Debug)]
pub enum KVError {
    /// Key not found
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    /// Invalid key format
    #[error("Invalid key format: {0}")]
    InvalidKey(String),

    /// Invalid value format
    #[error("Invalid value format: {0}")]
    InvalidValue(String),

    /// Backend error (database, network, etc.)
    #[error("Backend error: {0}")]
    BackendError(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    /// TTL error
    #[error("TTL error: {0}")]
    TTLError(String),

    /// Watch error
    #[error("Watch error: {0}")]
    WatchError(String),

    /// CAS failure (value didn't match)
    #[error("CAS operation failed: expected value didn't match")]
    CASFailed,

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// IO error
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),

    /// Storage error (for blob backend)
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Operation not supported
    #[error("Operation not supported: {0}")]
    NotSupported(String),
}

#[cfg(feature = "blob-backend")]
impl From<object_store::Error> for KVError {
    fn from(err: object_store::Error) -> Self {
        KVError::StorageError(err.to_string())
    }
}

impl From<serde_json::Error> for KVError {
    fn from(err: serde_json::Error) -> Self {
        KVError::SerializationError(err.to_string())
    }
}

#[cfg(feature = "sql-backend")]
impl From<sqlx::Error> for KVError {
    fn from(err: sqlx::Error) -> Self {
        KVError::BackendError(format!("SQL error: {}", err))
    }
}

#[cfg(feature = "redis-backend")]
impl From<redis::RedisError> for KVError {
    fn from(err: redis::RedisError) -> Self {
        KVError::BackendError(format!("Redis error: {}", err))
    }
}
