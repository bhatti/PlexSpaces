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

//! In-memory lock manager implementation (for testing).

use crate::{AcquireLockOptions, Lock, LockError, LockManager, LockResult, ReleaseLockOptions, RenewLockOptions};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use plexspaces_proto::prost_types::Timestamp;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use ulid::Ulid;

/// In-memory lock manager (for testing).
///
/// ## Purpose
/// Provides a simple in-memory implementation of `LockManager` for testing
/// and single-process scenarios.
///
/// ## Limitations
/// - Not persistent (locks lost on restart)
/// - Not distributed (single process only)
/// - No TTL cleanup (expired locks remain until accessed)
#[derive(Clone)]
pub struct MemoryLockManager {
    locks: Arc<RwLock<HashMap<String, Lock>>>,
}

impl MemoryLockManager {
    /// Create a new in-memory lock manager.
    pub fn new() -> Self {
        Self {
            locks: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemoryLockManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LockManager for MemoryLockManager {
    async fn acquire_lock(&self, options: AcquireLockOptions) -> LockResult<Lock> {
        let mut locks = self.locks.write().await;
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(options.lease_duration_secs as i64);

        // Check if lock exists
        if let Some(existing) = locks.get(&options.lock_key) {
            // Check if expired
            if let Some(expires) = &existing.expires_at {
                let expires_dt = DateTime::<Utc>::from_timestamp(expires.seconds, expires.nanos as u32)
                    .ok_or_else(|| LockError::BackendError("Invalid expiration timestamp".to_string()))?;
                
                if expires_dt < now {
                    // Lock expired, acquire it
                    let new_version = Ulid::new().to_string();
                    let new_lock = Lock {
                        lock_key: options.lock_key.clone(),
                        holder_id: options.holder_id.clone(),
                        version: new_version.clone(),
                        expires_at: Some(Timestamp {
                            seconds: expires_at.timestamp(),
                            nanos: expires_at.timestamp_subsec_nanos() as i32,
                        }),
                        lease_duration_secs: options.lease_duration_secs,
                        last_heartbeat: Some(Timestamp {
                            seconds: now.timestamp(),
                            nanos: now.timestamp_subsec_nanos() as i32,
                        }),
                        metadata: options.metadata.clone(),
                        locked: true,
                    };
                    locks.insert(options.lock_key, new_lock.clone());
                    return Ok(new_lock);
                }
            }

            // Lock exists and not expired
            if existing.holder_id == options.holder_id {
                // Same holder, return existing lock
                return Ok(existing.clone());
            } else {
                // Different holder, lock already held
                return Err(LockError::LockAlreadyHeld(existing.holder_id.clone()));
            }
        }

        // Lock doesn't exist, create it
        let new_version = Ulid::new().to_string();
        let new_lock = Lock {
            lock_key: options.lock_key.clone(),
            holder_id: options.holder_id.clone(),
            version: new_version.clone(),
            expires_at: Some(Timestamp {
                seconds: expires_at.timestamp(),
                nanos: expires_at.timestamp_subsec_nanos() as i32,
            }),
            lease_duration_secs: options.lease_duration_secs,
            last_heartbeat: Some(Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            metadata: options.metadata.clone(),
            locked: true,
        };
        locks.insert(options.lock_key, new_lock.clone());
        Ok(new_lock)
    }

    async fn renew_lock(&self, options: RenewLockOptions) -> LockResult<Lock> {
        let mut locks = self.locks.write().await;
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(options.lease_duration_secs as i64);

        let existing = locks.get(&options.lock_key)
            .ok_or_else(|| LockError::LockNotFound(options.lock_key.clone()))?;

        // Check version
        if existing.version != options.version {
            return Err(LockError::VersionMismatch {
                expected: existing.version.clone(),
                actual: options.version.clone(),
            });
        }

        // Check if expired
        if let Some(expires) = &existing.expires_at {
            let expires_dt = DateTime::<Utc>::from_timestamp(expires.seconds, expires.nanos as u32)
                .ok_or_else(|| LockError::BackendError("Invalid expiration timestamp".to_string()))?;
            
            if expires_dt < now {
                return Err(LockError::LockExpired(options.lock_key.clone()));
            }
        }

        // Check holder
        if existing.holder_id != options.holder_id {
            return Err(LockError::LockAlreadyHeld(existing.holder_id.clone()));
        }

        // Renew lock with new version
        let new_version = Ulid::new().to_string();
        let renewed_lock = Lock {
            lock_key: options.lock_key.clone(),
            holder_id: options.holder_id.clone(),
            version: new_version.clone(),
            expires_at: Some(Timestamp {
                seconds: expires_at.timestamp(),
                nanos: expires_at.timestamp_subsec_nanos() as i32,
            }),
            lease_duration_secs: options.lease_duration_secs,
            last_heartbeat: Some(Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            metadata: if options.metadata.is_empty() {
                existing.metadata.clone()
            } else {
                options.metadata.clone()
            },
            locked: true,
        };
        locks.insert(options.lock_key, renewed_lock.clone());
        Ok(renewed_lock)
    }

    async fn release_lock(&self, options: ReleaseLockOptions) -> LockResult<()> {
        let mut locks = self.locks.write().await;

        let existing = locks.get(&options.lock_key)
            .ok_or_else(|| LockError::LockNotFound(options.lock_key.clone()))?;

        // Check version
        if existing.version != options.version {
            return Err(LockError::VersionMismatch {
                expected: existing.version.clone(),
                actual: options.version.clone(),
            });
        }

        // Check holder
        if existing.holder_id != options.holder_id {
            return Err(LockError::LockAlreadyHeld(existing.holder_id.clone()));
        }

        if options.delete_lock {
            locks.remove(&options.lock_key);
        } else {
            // Set locked = false but keep entry
            let mut released = existing.clone();
            released.locked = false;
            locks.insert(options.lock_key, released);
        }

        Ok(())
    }

    async fn get_lock(&self, lock_key: &str) -> LockResult<Option<Lock>> {
        let locks = self.locks.read().await;
        Ok(locks.get(lock_key).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_acquire_lock() {
        let manager = MemoryLockManager::new();
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        assert_eq!(lock.lock_key, "test-lock");
        assert_eq!(lock.holder_id, "node-1");
        assert!(lock.locked);
    }

    #[tokio::test]
    async fn test_acquire_lock_already_held() {
        let manager = MemoryLockManager::new();
        manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        let result = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-2".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await;

        assert!(matches!(result, Err(LockError::LockAlreadyHeld(_))));
    }

    #[tokio::test]
    async fn test_renew_lock() {
        let manager = MemoryLockManager::new();
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        let renewed = manager.renew_lock(RenewLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            version: lock.version.clone(),
            lease_duration_secs: 60,
            metadata: Default::default(),
        }).await.unwrap();

        assert_ne!(renewed.version, lock.version);
        assert_eq!(renewed.lease_duration_secs, 60);
    }

    #[tokio::test]
    async fn test_release_lock() {
        let manager = MemoryLockManager::new();
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        manager.release_lock(ReleaseLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            version: lock.version,
            delete_lock: true,
        }).await.unwrap();

        let result = manager.get_lock("test-lock").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_acquire_lock_same_holder() {
        let manager = MemoryLockManager::new();
        let lock1 = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        // Same holder acquiring again should return existing lock
        let lock2 = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        assert_eq!(lock1.version, lock2.version);
        assert_eq!(lock1.holder_id, lock2.holder_id);
    }

    #[tokio::test]
    async fn test_renew_lock_version_mismatch() {
        let manager = MemoryLockManager::new();
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        // Try to renew with wrong version
        let result = manager.renew_lock(RenewLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            version: "wrong-version".to_string(),
            lease_duration_secs: 60,
            metadata: Default::default(),
        }).await;

        assert!(matches!(result, Err(LockError::VersionMismatch { .. })));
    }

    #[tokio::test]
    async fn test_renew_lock_wrong_holder() {
        let manager = MemoryLockManager::new();
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        // Try to renew with wrong holder
        let result = manager.renew_lock(RenewLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-2".to_string(),
            version: lock.version,
            lease_duration_secs: 60,
            metadata: Default::default(),
        }).await;

        assert!(matches!(result, Err(LockError::LockAlreadyHeld(_))));
    }

    #[tokio::test]
    async fn test_renew_lock_not_found() {
        let manager = MemoryLockManager::new();

        // Try to renew non-existent lock
        let result = manager.renew_lock(RenewLockOptions {
            lock_key: "non-existent".to_string(),
            holder_id: "node-1".to_string(),
            version: "some-version".to_string(),
            lease_duration_secs: 60,
            metadata: Default::default(),
        }).await;

        assert!(matches!(result, Err(LockError::LockNotFound(_))));
    }

    #[tokio::test]
    async fn test_release_lock_version_mismatch() {
        let manager = MemoryLockManager::new();
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        // Try to release with wrong version
        let result = manager.release_lock(ReleaseLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            version: "wrong-version".to_string(),
            delete_lock: true,
        }).await;

        assert!(matches!(result, Err(LockError::VersionMismatch { .. })));
    }

    #[tokio::test]
    async fn test_release_lock_wrong_holder() {
        let manager = MemoryLockManager::new();
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        // Try to release with wrong holder
        let result = manager.release_lock(ReleaseLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-2".to_string(),
            version: lock.version,
            delete_lock: true,
        }).await;

        assert!(matches!(result, Err(LockError::LockAlreadyHeld(_))));
    }

    #[tokio::test]
    async fn test_release_lock_not_found() {
        let manager = MemoryLockManager::new();

        // Try to release non-existent lock
        let result = manager.release_lock(ReleaseLockOptions {
            lock_key: "non-existent".to_string(),
            holder_id: "node-1".to_string(),
            version: "some-version".to_string(),
            delete_lock: true,
        }).await;

        assert!(matches!(result, Err(LockError::LockNotFound(_))));
    }

    #[tokio::test]
    async fn test_release_lock_without_delete() {
        let manager = MemoryLockManager::new();
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        // Release without deleting
        manager.release_lock(ReleaseLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            version: lock.version,
            delete_lock: false,
        }).await.unwrap();

        // Lock should still exist but be unlocked
        let result = manager.get_lock("test-lock").await.unwrap();
        assert!(result.is_some());
        let released_lock = result.unwrap();
        assert!(!released_lock.locked);
    }

    #[tokio::test]
    async fn test_get_lock() {
        let manager = MemoryLockManager::new();

        // Get non-existent lock
        let result = manager.get_lock("non-existent").await.unwrap();
        assert!(result.is_none());

        // Acquire lock
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        // Get existing lock
        let result = manager.get_lock("test-lock").await.unwrap();
        assert!(result.is_some());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.lock_key, lock.lock_key);
        assert_eq!(retrieved.holder_id, lock.holder_id);
        assert_eq!(retrieved.version, lock.version);
    }

    #[tokio::test]
    async fn test_acquire_expired_lock() {
        let manager = MemoryLockManager::new();

        // Acquire lock with very short duration
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 1, // 1 second
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        // Wait for lock to expire
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Different holder should be able to acquire expired lock
        let new_lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-2".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        assert_eq!(new_lock.holder_id, "node-2");
        assert_ne!(new_lock.version, lock.version);
    }

    #[tokio::test]
    async fn test_renew_expired_lock() {
        let manager = MemoryLockManager::new();

        // Acquire lock with very short duration
        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 1, // 1 second
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        // Wait for lock to expire
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Try to renew expired lock
        let result = manager.renew_lock(RenewLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            version: lock.version,
            lease_duration_secs: 60,
            metadata: Default::default(),
        }).await;

        assert!(matches!(result, Err(LockError::LockExpired(_))));
    }

    #[tokio::test]
    async fn test_lock_metadata() {
        let manager = MemoryLockManager::new();
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("key1".to_string(), "value1".to_string());
        metadata.insert("key2".to_string(), "value2".to_string());

        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: metadata.clone(),
        }).await.unwrap();

        assert_eq!(lock.metadata, metadata);

        // Renew with new metadata
        let mut new_metadata = std::collections::HashMap::new();
        new_metadata.insert("key3".to_string(), "value3".to_string());
        let renewed = manager.renew_lock(RenewLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            version: lock.version,
            lease_duration_secs: 60,
            metadata: new_metadata.clone(),
        }).await.unwrap();

        assert_eq!(renewed.metadata, new_metadata);
    }

    #[tokio::test]
    async fn test_renew_lock_preserves_metadata_when_empty() {
        let manager = MemoryLockManager::new();
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("key1".to_string(), "value1".to_string());

        let lock = manager.acquire_lock(AcquireLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: metadata.clone(),
        }).await.unwrap();

        // Renew with empty metadata (should preserve existing)
        let renewed = manager.renew_lock(RenewLockOptions {
            lock_key: "test-lock".to_string(),
            holder_id: "node-1".to_string(),
            version: lock.version,
            lease_duration_secs: 60,
            metadata: Default::default(),
        }).await.unwrap();

        assert_eq!(renewed.metadata, metadata);
    }

    #[tokio::test]
    async fn test_concurrent_lock_acquisition() {
        let manager = Arc::new(MemoryLockManager::new());
        let mut handles = vec![];

        // Spawn multiple tasks trying to acquire the same lock
        for i in 0..10 {
            let manager_clone = manager.clone();
            let handle = tokio::spawn(async move {
                manager_clone.acquire_lock(AcquireLockOptions {
                    lock_key: "concurrent-lock".to_string(),
                    holder_id: format!("node-{}", i),
                    lease_duration_secs: 30,
                    additional_wait_time_ms: 0,
                    refresh_period_ms: 100,
                    metadata: Default::default(),
                }).await
            });
            handles.push(handle);
        }

        // Wait for all tasks
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await);
        }

        // Only one should succeed
        let successes: Vec<_> = results.into_iter()
            .filter_map(|r| r.ok().and_then(|lock_result| lock_result.ok()))
            .collect();

        assert_eq!(successes.len(), 1);
        assert_eq!(successes[0].holder_id, "node-0"); // First one wins
    }

    #[tokio::test]
    async fn test_multiple_locks() {
        let manager = MemoryLockManager::new();

        // Acquire multiple different locks
        let lock1 = manager.acquire_lock(AcquireLockOptions {
            lock_key: "lock-1".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        let lock2 = manager.acquire_lock(AcquireLockOptions {
            lock_key: "lock-2".to_string(),
            holder_id: "node-1".to_string(),
            lease_duration_secs: 30,
            additional_wait_time_ms: 0,
            refresh_period_ms: 100,
            metadata: Default::default(),
        }).await.unwrap();

        assert_ne!(lock1.lock_key, lock2.lock_key);
        assert_eq!(lock1.holder_id, lock2.holder_id);

        // Both locks should be retrievable
        let retrieved1 = manager.get_lock("lock-1").await.unwrap();
        let retrieved2 = manager.get_lock("lock-2").await.unwrap();

        assert!(retrieved1.is_some());
        assert!(retrieved2.is_some());
        assert_eq!(retrieved1.unwrap().lock_key, "lock-1");
        assert_eq!(retrieved2.unwrap().lock_key, "lock-2");
    }
}

