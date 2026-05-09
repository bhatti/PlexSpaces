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

//! Unified KeyValueStore trait for actor/facet/WASM host integration.
//!
//! ## Purpose
//! Defines the base key-value interface used by actors, facets, and the WASM host.
//! This trait lives in `common` to avoid circular dependencies between `core`, `facet`,
//! and `keyvalue` crates.
//!
//! ## Relationship to plexspaces-keyvalue
//! The `plexspaces-keyvalue` crate defines a more comprehensive `KeyValueStore` trait
//! with additional operations (watch, multi_get, batch, stats). That trait is for
//! backend implementors (SQL, Redis, DynamoDB). This trait is the consumer-facing
//! interface for actors and the WASM host runtime.

use crate::RequestContext;
use async_trait::async_trait;
use std::time::Duration;

/// Errors from key-value store operations.
#[derive(Debug, thiserror::Error)]
pub enum KeyValueStoreError {
    /// Key not found (for operations that require existence).
    #[error("Key not found: {0}")]
    NotFound(String),

    /// Backend storage error.
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Serialization/deserialization error.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// CAS (compare-and-swap) conflict.
    #[error("CAS conflict: expected value mismatch")]
    CasConflict,
}

impl KeyValueStoreError {
    /// Returns the proto error code for this error variant.
    pub fn code(&self) -> plexspaces_proto::keyvalue::v1::KeyValueStoreErrorCode {
        use plexspaces_proto::keyvalue::v1::KeyValueStoreErrorCode;
        match self {
            KeyValueStoreError::NotFound(_) => KeyValueStoreErrorCode::KeyValueStoreErrorNotFound,
            KeyValueStoreError::StorageError(_) => KeyValueStoreErrorCode::KeyValueStoreErrorStorage,
            KeyValueStoreError::SerializationError(_) => KeyValueStoreErrorCode::KeyValueStoreErrorSerialization,
            KeyValueStoreError::CasConflict => KeyValueStoreErrorCode::KeyValueStoreErrorCasConflict,
        }
    }
}

/// Result type for key-value store operations.
pub type KeyValueStoreResult<T> = Result<T, KeyValueStoreError>;

/// Base key-value store interface for actors, facets, and WASM host.
///
/// ## Design
/// - All operations are tenant-scoped via `RequestContext`
/// - Provides CRUD, TTL, and atomic operations
/// - Implementations: in-memory (facets), SQL/Redis adapters (services layer)
///
/// ## Relationship to plexspaces-keyvalue::KeyValueStore
/// The `plexspaces-keyvalue` crate provides a richer trait for backend implementations
/// (watch, multi_get, stats). This trait covers the subset needed by actors and WASM hosts.
#[async_trait]
pub trait KeyValueStore: Send + Sync {
    /// Get value by key. Returns `None` if key does not exist.
    async fn get(&self, ctx: &RequestContext, key: &str) -> KeyValueStoreResult<Option<Vec<u8>>>;

    /// Put key-value pair. Overwrites existing value.
    async fn put(&self, ctx: &RequestContext, key: &str, value: Vec<u8>)
        -> KeyValueStoreResult<()>;

    /// Put key-value pair with TTL. Overwrites existing value.
    async fn put_with_ttl(
        &self,
        ctx: &RequestContext,
        key: &str,
        value: Vec<u8>,
        ttl: Duration,
    ) -> KeyValueStoreResult<()>;

    /// Delete key. Succeeds even if key does not exist (idempotent).
    async fn delete(&self, ctx: &RequestContext, key: &str) -> KeyValueStoreResult<()>;

    /// Check if key exists.
    async fn exists(&self, ctx: &RequestContext, key: &str) -> KeyValueStoreResult<bool>;

    /// List all keys matching prefix.
    async fn list_keys(
        &self,
        ctx: &RequestContext,
        prefix: &str,
    ) -> KeyValueStoreResult<Vec<String>>;

    /// Alias for `list_keys` (convenience method).
    async fn list(&self, ctx: &RequestContext, prefix: &str) -> KeyValueStoreResult<Vec<String>> {
        self.list_keys(ctx, prefix).await
    }

    /// Compare-and-swap: only set value if current matches `expected`.
    /// Returns `true` if swap succeeded, `false` if current value didn't match.
    async fn cas(
        &self,
        ctx: &RequestContext,
        key: &str,
        expected: Option<Vec<u8>>,
        new_value: Vec<u8>,
    ) -> KeyValueStoreResult<bool>;

    /// Atomic increment by delta. Creates key with delta value if not exists.
    async fn increment(
        &self,
        ctx: &RequestContext,
        key: &str,
        delta: i64,
    ) -> KeyValueStoreResult<i64>;
}
