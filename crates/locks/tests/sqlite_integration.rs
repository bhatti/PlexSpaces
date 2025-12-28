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

//! SQLite lock manager integration tests.
//!
//! These tests verify:
//! - Lock acquisition, renewal, and release
//! - Version-based optimistic locking
//! - Expiration handling
//! - Holder ID consistency
//! - Database queries to verify lock state

#[cfg(feature = "sqlite-backend")]
mod tests {
    use plexspaces_locks::{
        sql::SqliteLockManager, AcquireLockOptions, LockManager, ReleaseLockOptions, RenewLockOptions,
    };
    use plexspaces_common::RequestContext;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    fn test_ctx() -> RequestContext {
        RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string())
    }

    /// Create a new SQLite lock manager with in-memory database
    async fn create_manager() -> SqliteLockManager {
        SqliteLockManager::new("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn test_sqlite_acquire_lock() {
        let manager = create_manager().await;

        let lock = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        assert_eq!(lock.lock_key, "test-lock");
        assert_eq!(lock.holder_id, "node-1");
        assert!(lock.locked);
        assert!(!lock.version.is_empty());

        // Verify lock exists in database
        let retrieved = manager.get_lock(&test_ctx(), "test-lock").await.unwrap();
        assert!(retrieved.is_some());
        let retrieved_lock = retrieved.unwrap();
        assert_eq!(retrieved_lock.holder_id, "node-1");
        assert_eq!(retrieved_lock.version, lock.version);
    }

    #[tokio::test]
    async fn test_sqlite_acquire_lock_already_held() {
        let manager = create_manager().await;

        manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        // Try to acquire with different holder
        let result = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-2".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, plexspaces_locks::LockError::LockAlreadyHeld(_)));
        }
    }

    #[tokio::test]
    async fn test_sqlite_acquire_lock_same_holder() {
        let manager = create_manager().await;

        let lock1 = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        // Same holder acquiring again should refresh the lock
        let lock2 = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        // Version should be different (new version generated)
        assert_ne!(lock1.version, lock2.version);
        assert_eq!(lock1.holder_id, lock2.holder_id);
    }

    #[tokio::test]
    async fn test_sqlite_renew_lock() {
        let manager = create_manager().await;

        let lock = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        let renewed = manager
            .renew_lock(&test_ctx(), RenewLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                version: lock.version.clone(),
                lease_duration_secs: 60,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        assert_ne!(renewed.version, lock.version);
        assert_eq!(renewed.lease_duration_secs, 60);
        assert_eq!(renewed.holder_id, "node-1");

        // Verify in database
        let retrieved = manager.get_lock(&test_ctx(), "test-lock").await.unwrap().unwrap();
        assert_eq!(retrieved.version, renewed.version);
        assert_eq!(retrieved.lease_duration_secs, 60);
    }

    #[tokio::test]
    async fn test_sqlite_renew_lock_version_mismatch() {
        let manager = create_manager().await;

        let lock = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        // Try to renew with wrong version
        let result = manager
            .renew_lock(&test_ctx(), RenewLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                version: "wrong-version".to_string(),
                lease_duration_secs: 60,
                metadata: Default::default(),
            })
            .await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, plexspaces_locks::LockError::VersionMismatch { .. }));
        }
    }

    #[tokio::test]
    async fn test_sqlite_renew_lock_wrong_holder() {
        let manager = create_manager().await;

        let lock = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        // Try to renew with wrong holder
        let result = manager
            .renew_lock(&test_ctx(), RenewLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-2".to_string(),
                version: lock.version,
                lease_duration_secs: 60,
                metadata: Default::default(),
            })
            .await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, plexspaces_locks::LockError::InvalidHolderId(_)));
        }
    }

    #[tokio::test]
    async fn test_sqlite_release_lock() {
        let manager = create_manager().await;

        let lock = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        manager
            .release_lock(&test_ctx(), ReleaseLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                version: lock.version,
                delete_lock: true,
            })
            .await
            .unwrap();

        // Verify lock is deleted
        let result = manager.get_lock(&test_ctx(), "test-lock").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_sqlite_release_lock_without_delete() {
        let manager = create_manager().await;

        let lock = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        // Release without deleting
        manager
            .release_lock(&test_ctx(), ReleaseLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                version: lock.version,
                delete_lock: false,
            })
            .await
            .unwrap();

        // Lock should still exist but be unlocked
        let result = manager.get_lock(&test_ctx(), "test-lock").await.unwrap();
        assert!(result.is_some());
        let released_lock = result.unwrap();
        assert!(!released_lock.locked);
    }

    #[tokio::test]
    async fn test_sqlite_acquire_expired_lock() {
        let manager = create_manager().await;

        // Acquire lock with very short duration
        let lock = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 1, // 1 second
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        // Wait for lock to expire
        sleep(Duration::from_secs(2)).await;

        // Different holder should be able to acquire expired lock
        let new_lock = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-2".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        assert_eq!(new_lock.holder_id, "node-2");
        assert_ne!(new_lock.version, lock.version);

        // Verify in database
        let retrieved = manager.get_lock(&test_ctx(), "test-lock").await.unwrap().unwrap();
        assert_eq!(retrieved.holder_id, "node-2");
    }

    #[tokio::test]
    async fn test_sqlite_lock_metadata() {
        let manager = create_manager().await;

        let mut metadata = HashMap::new();
        metadata.insert("key1".to_string(), "value1".to_string());
        metadata.insert("key2".to_string(), "value2".to_string());

        let lock = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: metadata.clone(),
            })
            .await
            .unwrap();

        assert_eq!(lock.metadata, metadata);

        // Verify in database
        let retrieved = manager.get_lock(&test_ctx(), "test-lock").await.unwrap().unwrap();
        assert_eq!(retrieved.metadata, metadata);
    }

    #[tokio::test]
    async fn test_sqlite_concurrent_lock_acquisition() {
        let manager = Arc::new(create_manager().await);
        let mut handles = vec![];

        // Spawn multiple tasks trying to acquire the same lock
        for i in 0..10 {
            let manager_clone = manager.clone();
            let handle = tokio::spawn(async move {
                manager_clone
                    .acquire_lock(&test_ctx(), AcquireLockOptions {
                        lock_key: "concurrent-lock".to_string(),
                        holder_id: format!("node-{}", i),
                        lease_duration_secs: 30,
                        additional_wait_time_ms: 0,
                        refresh_period_ms: 100,
                        metadata: Default::default(),
                    })
                    .await
            });
            handles.push(handle);
        }

        // Wait for all tasks
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await);
        }

        // Only one should succeed
        let successes: Vec<_> = results
            .into_iter()
            .filter_map(|r| r.ok().and_then(|lock_result| lock_result.ok()))
            .collect();

        assert_eq!(successes.len(), 1, "Only one task should acquire the lock");
        // Note: Which task wins is non-deterministic, so we just verify exactly one succeeded
        let winner_holder_id = successes[0].holder_id.clone();

        // Verify in database
        let retrieved = manager.get_lock(&test_ctx(), "concurrent-lock").await.unwrap().unwrap();
        assert_eq!(retrieved.holder_id, winner_holder_id);
    }

    #[tokio::test]
    async fn test_sqlite_query_locks_table() {
        // This test verifies we can query the locks table directly
        // to inspect holder_id consistency
        let manager = create_manager().await;

        // Acquire lock
        let lock = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "query-test-lock".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        // Query via get_lock (which queries the database)
        let retrieved = manager.get_lock(&test_ctx(), "query-test-lock").await.unwrap().unwrap();

        // Verify holder_id matches
        assert_eq!(retrieved.holder_id, "node-1");
        assert_eq!(retrieved.holder_id, lock.holder_id);
        assert_eq!(retrieved.version, lock.version);
        assert!(retrieved.locked);
    }

    #[tokio::test]
    async fn test_sqlite_multiple_locks() {
        let manager = create_manager().await;

        // Acquire multiple different locks
        let lock1 = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "lock-1".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        let lock2 = manager
            .acquire_lock(&test_ctx(), AcquireLockOptions {
                lock_key: "lock-2".to_string(),
                holder_id: "node-1".to_string(),
                lease_duration_secs: 30,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: Default::default(),
            })
            .await
            .unwrap();

        assert_ne!(lock1.lock_key, lock2.lock_key);
        assert_eq!(lock1.holder_id, lock2.holder_id);

        // Both locks should be retrievable
        let retrieved1 = manager.get_lock(&test_ctx(), "lock-1").await.unwrap();
        let retrieved2 = manager.get_lock(&test_ctx(), "lock-2").await.unwrap();

        assert!(retrieved1.is_some());
        assert!(retrieved2.is_some());
        assert_eq!(retrieved1.unwrap().lock_key, "lock-1");
        assert_eq!(retrieved2.unwrap().lock_key, "lock-2");
    }
}

