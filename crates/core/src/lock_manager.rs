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

//! LockManager trait for distributed lock/lease coordination
//!
//! ## Proto-First Design
//! This module follows proto-first design principles:
//! - **Data structures** (`Lock`, `AcquireLockOptions`, `RenewLockOptions`, `ReleaseLockOptions`) 
//!   are defined in proto and re-exported here
//! - **Traits** (`LockManager`) are defined in Rust (traits cannot be defined in proto)
//! - **Error types** (`LockError`) are Rust ADT enums (enums with data cannot be defined in proto)
//!
//! All data models come from proto definitions in `plexspaces_proto::locks::prv`.

use async_trait::async_trait;
use crate::RequestContext;

// Re-export proto types - these are the source of truth for data structures
pub use plexspaces_proto::locks::prv::{AcquireLockOptions, Lock, ReleaseLockOptions, RenewLockOptions};

/// Result type for lock operations
pub type LockResult<T> = Result<T, LockError>;

/// Error type for lock operations
/// 
/// This is a Rust ADT enum (cannot be defined in proto).
/// Proto-first design: data structures in proto, ADT enums and traits in Rust.
#[derive(Debug, Clone)]
pub enum LockError {
    /// Lock is already held by a different holder
    LockAlreadyHeld(String),
    /// Version mismatch (optimistic locking failure)
    VersionMismatch { expected: String, actual: String },
    /// Lock not found
    LockNotFound(String),
    /// Lock has expired
    LockExpired(String),
    /// Backend error
    BackendError(String),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::LockAlreadyHeld(holder) => write!(f, "Lock already held by: {}", holder),
            LockError::VersionMismatch { expected, actual } => write!(f, "Version mismatch: expected {}, got {}", expected, actual),
            LockError::LockNotFound(key) => write!(f, "Lock not found: {}", key),
            LockError::LockExpired(key) => write!(f, "Lock expired: {}", key),
            LockError::BackendError(msg) => write!(f, "Backend error: {}", msg),
        }
    }
}

impl std::error::Error for LockError {}

/// Trait for distributed lock/lease management.
///
/// ## Purpose
/// Provides atomic operations for acquiring, renewing, and releasing distributed locks
/// with version-based optimistic locking for coordination tasks.
///
/// ## Proto-First Design
/// This trait uses proto-defined types for all data structures:
/// - `AcquireLockOptions`, `RenewLockOptions`, `ReleaseLockOptions`, `Lock` are from proto
/// - The trait itself is in Rust (traits cannot be defined in proto)
///
/// ## Design (db-locks inspired)
/// - **Acquire**: Atomic lock acquisition with version generation
/// - **Renew**: Heartbeat mechanism to extend lease expiration
/// - **Release**: Atomic lock release with version validation
/// - **Version-based optimistic locking**: Prevents lost updates
#[async_trait]
pub trait LockManager: Send + Sync {
    /// Acquire a lock (atomic operation).
    ///
    /// ## Behavior
    /// - If lock doesn't exist: Create lock with new version
    /// - If lock exists and expired: Acquire lock with new version
    /// - If lock exists and not expired: Return error if held by different holder
    /// - If lock exists and held by same holder: Return existing lock
    ///
    /// ## Returns
    /// - `Ok(Lock)`: Lock acquired successfully
    /// - `Err(LockError::LockAlreadyHeld)`: Lock held by different holder
    /// - `Err(LockError::BackendError)`: Backend error
    async fn acquire_lock(&self, ctx: &RequestContext, options: AcquireLockOptions) -> LockResult<Lock>;

    /// Renew a lock (heartbeat mechanism).
    ///
    /// ## Behavior
    /// - Validates version matches current lock version
    /// - Updates expiration timestamp
    /// - Updates last_heartbeat timestamp
    /// - Returns new lock with updated version
    ///
    /// ## Returns
    /// - `Ok(Lock)`: Lock renewed successfully
    /// - `Err(LockError::VersionMismatch)`: Version doesn't match (optimistic locking failure)
    /// - `Err(LockError::LockNotFound)`: Lock doesn't exist
    /// - `Err(LockError::LockExpired)`: Lock expired
    async fn renew_lock(&self, ctx: &RequestContext, options: RenewLockOptions) -> LockResult<Lock>;

    /// Release a lock (atomic operation).
    ///
    /// ## Behavior
    /// - Validates version matches current lock version
    /// - If `delete_lock = true`: Removes lock entry completely
    /// - If `delete_lock = false`: Sets `locked = false` but keeps entry (for audit)
    ///
    /// ## Returns
    /// - `Ok(())`: Lock released successfully
    /// - `Err(LockError::VersionMismatch)`: Version doesn't match (optimistic locking failure)
    /// - `Err(LockError::LockNotFound)`: Lock doesn't exist
    async fn release_lock(&self, ctx: &RequestContext, options: ReleaseLockOptions) -> LockResult<()>;

    /// Get current lock state (non-blocking).
    ///
    /// ## Returns
    /// - `Ok(Some(Lock))`: Lock exists
    /// - `Ok(None)`: Lock doesn't exist
    /// - `Err(LockError::BackendError)`: Backend error
    async fn get_lock(&self, ctx: &RequestContext, lock_key: &str) -> LockResult<Option<Lock>>;
}

