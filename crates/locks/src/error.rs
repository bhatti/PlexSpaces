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

//! Error types for distributed lock operations.

use thiserror::Error;

/// Result type for lock operations.
pub type LockResult<T> = Result<T, LockError>;

/// Errors that can occur during lock operations.
#[derive(Error, Debug)]
pub enum LockError {
    /// Lock not found
    #[error("Lock not found: {0}")]
    LockNotFound(String),

    /// Lock already held by another holder
    #[error("Lock already held by: {0}")]
    LockAlreadyHeld(String),

    /// Lock expired
    #[error("Lock expired: {0}")]
    LockExpired(String),

    /// Version mismatch (optimistic locking failure)
    #[error("Version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: String, actual: String },

    /// Invalid lock key
    #[error("Invalid lock key: {0}")]
    InvalidKey(String),

    /// Invalid holder ID
    #[error("Invalid holder ID: {0}")]
    InvalidHolderId(String),

    /// Backend error (database, network, etc.)
    #[error("Backend error: {0}")]
    BackendError(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// IO error
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
}

impl From<serde_json::Error> for LockError {
    fn from(err: serde_json::Error) -> Self {
        LockError::SerializationError(err.to_string())
    }
}

#[cfg(any(feature = "sqlite-backend", feature = "postgres-backend"))]
impl From<sqlx::Error> for LockError {
    fn from(err: sqlx::Error) -> Self {
        LockError::BackendError(format!("SQL error: {}", err))
    }
}

#[cfg(feature = "redis-backend")]
impl From<redis::RedisError> for LockError {
    fn from(err: redis::RedisError) -> Self {
        LockError::BackendError(format!("Redis error: {}", err))
    }
}

#[cfg(feature = "ddb-backend")]
impl From<aws_sdk_dynamodb::Error> for LockError {
    fn from(err: aws_sdk_dynamodb::Error) -> Self {
        LockError::BackendError(format!("DynamoDB error: {}", err))
    }
}

