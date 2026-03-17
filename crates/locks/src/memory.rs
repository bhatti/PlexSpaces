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

//! In-memory lock manager implementation using tokio primitives.
//!
//! ## Purpose
//! Provides fast, single-node lock coordination using tokio's Semaphore and Mutex
//! primitives. This is more efficient than SQLite for single-node deployments.
//!
//! ## Architecture Context
//! Used when:
//! - Single-node deployment (no distributed coordination needed)
//! - SQLite backend is used (can use in-memory instead for better performance)
//! - Testing/development scenarios
//!
//! ## Design
//! - Uses `Arc<Mutex<HashMap>>` for lock storage
//! - Uses `tokio::sync::Semaphore` for per-lock coordination
//! - Thread-safe and async-friendly
//! - No persistence (locks are lost on restart)

use crate::{
    AcquireLockOptions, Lock, LockError, LockManager, LockResult, ReleaseLockOptions,
    RenewLockOptions,
};
use async_trait::async_trait;
use plexspaces_common::RequestContext;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{Mutex, Semaphore};
use tracing::instrument;
use ulid::Ulid;

/// In-memory lock entry
#[derive(Clone)]
struct LockEntry {
    holder_id: String,
    version: String,
    expires_at: SystemTime,
    lease_duration_secs: u32,
    last_heartbeat: SystemTime,
    locked: bool,
    metadata: HashMap<String, String>,
    /// Semaphore for coordinating access to this specific lock
    semaphore: Arc<Semaphore>,
}

/// In-memory lock manager using tokio primitives.
///
/// ## Purpose
/// Fast, single-node lock coordination without database overhead.
/// More efficient than SQLite for single-node deployments.
///
/// ## Design
/// - Uses `Arc<Mutex<HashMap>>` for lock storage
/// - Uses `tokio::sync::Semaphore` for per-lock coordination
/// - Thread-safe and async-friendly
/// - No persistence (locks are lost on restart)
#[derive(Clone)]
pub struct MemoryLockManager {
    /// Lock storage: (tenant_id, namespace, lock_key) -> LockEntry
    locks: Arc<Mutex<HashMap<(String, String, String), LockEntry>>>,
}

impl MemoryLockManager {
    /// Create a new in-memory lock manager.
    pub fn new() -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn now() -> SystemTime {
        SystemTime::now()
    }

    fn lock_to_proto(lock_key: String, entry: &LockEntry) -> Lock {
        Lock {
            lock_key,
            holder_id: entry.holder_id.clone(),
            version: entry.version.clone(),
            expires_at: Some(plexspaces_proto::prost_types::Timestamp::from(
                entry.expires_at,
            )),
            lease_duration_secs: entry.lease_duration_secs,
            last_heartbeat: Some(plexspaces_proto::prost_types::Timestamp::from(
                entry.last_heartbeat,
            )),
            metadata: entry.metadata.clone(),
            locked: entry.locked,
        }
    }
}

#[async_trait]
impl LockManager for MemoryLockManager {
    #[instrument(skip(self, ctx, options), fields(lock_key = %options.lock_key, holder_id = %options.holder_id, tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace()))]
    async fn acquire_lock(
        &self,
        ctx: &RequestContext,
        options: AcquireLockOptions,
    ) -> LockResult<Lock> {
        let tenant_id = ctx.tenant_id().to_string();
        let namespace = ctx.namespace().to_string();
        let key = (
            tenant_id.clone(),
            namespace.clone(),
            options.lock_key.clone(),
        );

        let mut locks = self.locks.lock().await;
        let now = Self::now();

        if let Some(entry) = locks.get(&key) {
            let expired = entry.expires_at <= now || !entry.locked;

            if !expired && entry.holder_id != options.holder_id {
                return Err(LockError::LockAlreadyHeld(entry.holder_id.clone()));
            }

            if !expired && entry.holder_id == options.holder_id {
                let existing = Self::lock_to_proto(options.lock_key.clone(), entry);
                drop(locks);
                return Ok(existing);
            }

            // Update existing lock (no need to acquire semaphore here - we're already holding the mutex)
            let new_version = Ulid::new().to_string();
            let expires_at =
                now + std::time::Duration::from_secs(options.lease_duration_secs as u64);
            let new_entry = LockEntry {
                holder_id: options.holder_id.clone(),
                version: new_version.clone(),
                expires_at,
                lease_duration_secs: options.lease_duration_secs,
                last_heartbeat: now,
                locked: true,
                metadata: options.metadata.clone(),
                semaphore: entry.semaphore.clone(),
            };
            locks.insert(key.clone(), new_entry.clone());

            drop(locks); // Release mutex

            Ok(Self::lock_to_proto(options.lock_key.clone(), &new_entry))
        } else {
            // Create new lock
            let new_version = Ulid::new().to_string();
            let expires_at =
                now + std::time::Duration::from_secs(options.lease_duration_secs as u64);
            let semaphore = Arc::new(Semaphore::new(1)); // Permit count 1 for exclusive access

            let new_entry = LockEntry {
                holder_id: options.holder_id.clone(),
                version: new_version.clone(),
                expires_at,
                lease_duration_secs: options.lease_duration_secs,
                last_heartbeat: now,
                locked: true,
                metadata: options.metadata.clone(),
                semaphore: semaphore.clone(),
            };
            locks.insert(key.clone(), new_entry.clone());

            drop(locks); // Release mutex

            Ok(Self::lock_to_proto(options.lock_key.clone(), &new_entry))
        }
    }

    #[instrument(skip(self, ctx, options), fields(lock_key = %options.lock_key, holder_id = %options.holder_id, version = %options.version, tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace()))]
    async fn renew_lock(
        &self,
        ctx: &RequestContext,
        options: RenewLockOptions,
    ) -> LockResult<Lock> {
        let tenant_id = ctx.tenant_id().to_string();
        let namespace = ctx.namespace().to_string();
        let key = (
            tenant_id.clone(),
            namespace.clone(),
            options.lock_key.clone(),
        );

        // First, validate the lock exists and is valid
        let (semaphore, holder_id) = {
            let locks = self.locks.lock().await;
            let entry = locks
                .get(&key)
                .ok_or_else(|| LockError::LockNotFound(options.lock_key.clone()))?;

            if entry.holder_id != options.holder_id {
                return Err(LockError::InvalidHolderId(entry.holder_id.clone()));
            }
            let now = Self::now();
            if entry.expires_at <= now || !entry.locked {
                return Err(LockError::LockExpired(options.lock_key.clone()));
            }

            // Clone what we need before dropping the lock
            (entry.semaphore.clone(), entry.holder_id.clone())
        };

        // Acquire permit for this lock's semaphore (outside the mutex)
        let _permit = semaphore
            .acquire()
            .await
            .map_err(|_| LockError::BackendError("Semaphore closed".to_string()))?;

        // Now update the lock
        let mut locks = self.locks.lock().await;
        let now = Self::now();
        let new_version = Ulid::new().to_string();
        let expires_at = now + std::time::Duration::from_secs(options.lease_duration_secs as u64);
        let new_entry = LockEntry {
            holder_id: holder_id.clone(),
            version: new_version.clone(),
            expires_at,
            lease_duration_secs: options.lease_duration_secs,
            last_heartbeat: now,
            locked: true,
            metadata: options.metadata.clone(),
            semaphore: semaphore.clone(),
        };
        locks.insert(key.clone(), new_entry.clone());

        drop(locks); // Release mutex before dropping permit

        Ok(Self::lock_to_proto(options.lock_key.clone(), &new_entry))
    }

    #[instrument(skip(self, ctx, options), fields(lock_key = %options.lock_key, holder_id = %options.holder_id, version = %options.version, tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace()))]
    async fn release_lock(
        &self,
        ctx: &RequestContext,
        options: ReleaseLockOptions,
    ) -> LockResult<()> {
        let tenant_id = ctx.tenant_id().to_string();
        let namespace = ctx.namespace().to_string();
        let key = (
            tenant_id.clone(),
            namespace.clone(),
            options.lock_key.clone(),
        );

        let mut locks = self.locks.lock().await;

        let entry = locks
            .get(&key)
            .ok_or_else(|| LockError::LockNotFound(options.lock_key.clone()))?;

        if entry.holder_id != options.holder_id {
            return Err(LockError::InvalidHolderId(entry.holder_id.clone()));
        }
        if entry.version != options.version {
            return Err(LockError::VersionMismatch {
                expected: entry.version.clone(),
                actual: options.version.clone(),
            });
        }

        if options.delete_lock {
            locks.remove(&key);
        } else {
            // Update lock to unlocked (no need to acquire semaphore here - we're already holding the mutex)
            let mut new_entry = entry.clone();
            new_entry.locked = false;
            locks.insert(key.clone(), new_entry);
        }

        drop(locks); // Release mutex

        Ok(())
    }

    #[instrument(skip(self, ctx), fields(lock_key = %lock_key, tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace()))]
    async fn get_lock(&self, ctx: &RequestContext, lock_key: &str) -> LockResult<Option<Lock>> {
        let tenant_id = ctx.tenant_id().to_string();
        let namespace = ctx.namespace().to_string();
        let key = (tenant_id.clone(), namespace.clone(), lock_key.to_string());

        let locks = self.locks.lock().await;

        if let Some(entry) = locks.get(&key) {
            Ok(Some(Self::lock_to_proto(lock_key.to_string(), entry)))
        } else {
            Ok(None)
        }
    }
}

impl Default for MemoryLockManager {
    fn default() -> Self {
        Self::new()
    }
}
