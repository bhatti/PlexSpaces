// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
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

//! Lock manager trait for distributed lock/lease coordination.

use async_trait::async_trait;
use crate::{AcquireLockOptions, Lock, LockResult, ReleaseLockOptions, RenewLockOptions};

/// Trait for distributed lock/lease management.
///
/// ## Purpose
/// Provides atomic operations for acquiring, renewing, and releasing distributed locks
/// with version-based optimistic locking for coordination tasks.
///
/// ## Design (db-locks inspired)
/// - **Acquire**: Atomic lock acquisition with version generation
/// - **Renew**: Heartbeat mechanism to extend lease expiration
/// - **Release**: Atomic lock release with version validation
/// - **Version-based optimistic locking**: Prevents lost updates
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_locks::{LockManager, memory::MemoryLockManager};
///
/// let manager = MemoryLockManager::new();
///
/// // Acquire lock
/// let lock = manager.acquire_lock(options).await?;
///
/// // Renew lock (heartbeat)
/// let renewed = manager.renew_lock(renew_options).await?;
///
/// // Release lock
/// manager.release_lock(release_options).await?;
/// ```
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
    async fn acquire_lock(&self, options: AcquireLockOptions) -> LockResult<Lock>;

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
    async fn renew_lock(&self, options: RenewLockOptions) -> LockResult<Lock>;

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
    async fn release_lock(&self, options: ReleaseLockOptions) -> LockResult<()>;

    /// Get current lock state (non-blocking).
    ///
    /// ## Returns
    /// - `Ok(Some(Lock))`: Lock exists
    /// - `Ok(None)`: Lock doesn't exist
    /// - `Err(LockError::BackendError)`: Backend error
    async fn get_lock(&self, lock_key: &str) -> LockResult<Option<Lock>>;
}

