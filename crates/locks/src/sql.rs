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

//! SQL-based lock manager implementations (SQLite and PostgreSQL).
//!
//! This module provides relational database backends for the generic
//! [`LockManager`](crate::LockManager) trait. It is intentionally focused on
//! the `plexspaces_proto::locks::prv` model and borrows ideas from the
//! standalone `db-locks` project:
//!
//! - Row-based, transactional locks
//! - Optimistic version checks
//! - Explicit lease / expiration semantics
//! - Clear separation between in-memory and durable backends
//!
//! Currently we implement a **SQLite** backend. PostgreSQL can be added by
//! following the same pattern with a `PgPool`.

use crate::{AcquireLockOptions, Lock, LockError, LockManager, LockResult, ReleaseLockOptions, RenewLockOptions};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use plexspaces_common::RequestContext;
use std::collections::HashMap;
use tracing::instrument;
use ulid::Ulid;

#[cfg(feature = "sqlite-backend")]
use sqlx::{Acquire, Row, SqlitePool};

/// SQLite-based lock manager.
///
/// This backend uses a single `locks` table with the following schema:
///
/// ```sql
/// CREATE TABLE IF NOT EXISTS locks (
///   lock_key TEXT PRIMARY KEY,
///   holder_id TEXT NOT NULL,
///   version TEXT NOT NULL,
///   expires_at INTEGER NOT NULL,
///   lease_duration_secs INTEGER NOT NULL,
///   last_heartbeat INTEGER NOT NULL,
///   locked INTEGER NOT NULL,
///   metadata TEXT
/// );
/// ```
///
/// - `expires_at` / `last_heartbeat` are stored as UNIX epoch seconds
/// - `metadata` is JSON-encoded map
#[cfg(feature = "sqlite-backend")]
#[derive(Clone)]
pub struct SqliteLockManager {
    pool: SqlitePool,
}

#[cfg(feature = "sqlite-backend")]
impl SqliteLockManager {
    /// Create a new SQLite lock manager.
    ///
    /// `database_url` is any valid `sqlx` SQLite URL, e.g.:
    /// - `sqlite::memory:` (in-memory)
    /// - `sqlite://locks.db`
    #[instrument(skip(database_url))]
    pub async fn new(database_url: &str) -> LockResult<Self> {
        let pool = SqlitePool::connect(database_url)
            .await
            .map_err(|e| LockError::BackendError(format!("failed to connect SQLite: {e}")))?;

        // Initialize schema lazily with tenant/namespace support
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS locks (
              tenant_id TEXT NOT NULL DEFAULT 'default',
              namespace TEXT NOT NULL DEFAULT 'default',
              lock_key TEXT NOT NULL,
              holder_id TEXT NOT NULL,
              version TEXT NOT NULL,
              expires_at INTEGER NOT NULL,
              lease_duration_secs INTEGER NOT NULL,
              last_heartbeat INTEGER NOT NULL,
              locked INTEGER NOT NULL,
              metadata TEXT,
              PRIMARY KEY (tenant_id, namespace, lock_key)
            );
        "#,
        )
        .execute(&pool)
        .await
        .map_err(|e| LockError::BackendError(format!("failed to create locks table: {e}")))?;

        // Create indexes
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_locks_tenant_namespace
            ON locks(tenant_id, namespace, lock_key)
            WHERE locked = 1;
        "#,
        )
        .execute(&pool)
        .await
        .map_err(|e| LockError::BackendError(format!("failed to create index: {e}")))?;

        Ok(Self { pool })
    }

    fn now_epoch_secs() -> i64 {
        Utc::now().timestamp()
    }

    fn lock_from_row(
        lock_key: String,
        holder_id: String,
        version: String,
        expires_at: i64,
        lease_duration_secs: i64,
        last_heartbeat: i64,
        locked: i64,
        metadata_json: Option<String>,
    ) -> LockResult<Lock> {
        let mut metadata: HashMap<String, String> = HashMap::new();
        if let Some(json) = metadata_json {
            if !json.is_empty() {
                metadata = serde_json::from_str(&json)
                    .map_err(|e| LockError::BackendError(format!("invalid metadata json: {e}")))?;
            }
        }

        // Convert epoch seconds to SystemTime, then to Timestamp
        use std::time::{SystemTime, UNIX_EPOCH};
        let expires_system_time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(expires_at as u64);
        let last_hb_system_time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(last_heartbeat as u64);
        
        Ok(Lock {
            lock_key,
            holder_id,
            version,
            expires_at: Some(plexspaces_proto::prost_types::Timestamp::from(expires_system_time)),
            lease_duration_secs: lease_duration_secs as u32,
            last_heartbeat: Some(plexspaces_proto::prost_types::Timestamp::from(last_hb_system_time)),
            metadata,
            locked: locked != 0,
        })
    }
}

#[cfg(feature = "sqlite-backend")]
#[async_trait]
impl LockManager for SqliteLockManager {
    #[instrument(skip(self, ctx, options), fields(lock_key = %options.lock_key, holder_id = %options.holder_id, tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace()))]
    async fn acquire_lock(&self, ctx: &RequestContext, options: AcquireLockOptions) -> LockResult<Lock> {
        let tenant_id = if ctx.tenant_id().is_empty() { "default" } else { ctx.tenant_id() };
        let namespace = if ctx.namespace().is_empty() { "default" } else { ctx.namespace() };
        
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| LockError::BackendError(format!("acquire conn: {e}")))?;
        let mut tx = conn
            .begin()
            .await
            .map_err(|e| LockError::BackendError(format!("begin tx: {e}")))?;

        let now = Self::now_epoch_secs();
        let lease_secs = options.lease_duration_secs as i64;
        let expires_at = now + lease_secs;

        // Load existing lock (if any) - MUST filter by tenant_id and namespace
        let row = sqlx::query(
            r#"SELECT lock_key, holder_id, version, expires_at, lease_duration_secs,
                      last_heartbeat, locked, metadata
               FROM locks WHERE tenant_id = ?1 AND namespace = ?2 AND lock_key = ?3"#,
        )
        .bind(tenant_id)
        .bind(namespace)
        .bind(&options.lock_key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| LockError::BackendError(format!("select lock: {e}")))?;

        if let Some(row) = row {
            let lock_key: String = row.get("lock_key");
            let holder_id: String = row.get("holder_id");
            let _version: String = row.get("version");
            let expires_at_row: i64 = row.get("expires_at");
            let lease_duration_secs_row: i64 = row.get("lease_duration_secs");
            let _last_heartbeat: i64 = row.get("last_heartbeat");
            let locked_flag: i64 = row.get("locked");
            let metadata_json: Option<String> = row.get("metadata");

            let expired = expires_at_row <= now || locked_flag == 0;

            if !expired && holder_id != options.holder_id {
                // Lock is held by someone else
                return Err(LockError::LockAlreadyHeld(holder_id));
            }

            // Either expired or held by same holder – acquire/refresh
            let new_version = Ulid::new().to_string();
            let metadata_json_new = if options.metadata.is_empty() {
                metadata_json
            } else {
                Some(
                    serde_json::to_string(&options.metadata)
                        .map_err(|e| LockError::BackendError(format!("encode metadata: {e}")))?,
                )
            };

            sqlx::query(
                r#"UPDATE locks
                   SET holder_id = ?4,
                       version = ?5,
                       expires_at = ?6,
                       lease_duration_secs = ?7,
                       last_heartbeat = ?8,
                       locked = 1,
                       metadata = ?9
                 WHERE tenant_id = ?1 AND namespace = ?2 AND lock_key = ?3"#,
            )
            .bind(tenant_id)
            .bind(namespace)
            .bind(&options.lock_key)
            .bind(&options.holder_id)
            .bind(&new_version)
            .bind(expires_at)
            .bind(lease_secs)
            .bind(now)
            .bind(metadata_json_new.clone())
            .execute(&mut *tx)
            .await
            .map_err(|e| LockError::BackendError(format!("update lock: {e}")))?;

            tx.commit()
                .await
                .map_err(|e| LockError::BackendError(format!("commit tx: {e}")))?;

            return Self::lock_from_row(
                lock_key,
                options.holder_id.clone(),
                new_version,
                expires_at,
                lease_duration_secs_row,
                now,
                1,
                metadata_json_new,
            );
        }

        // No existing lock – insert new
        let version = Ulid::new().to_string();
        let metadata_json = if options.metadata.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&options.metadata)
                    .map_err(|e| LockError::BackendError(format!("encode metadata: {e}")))?,
            )
        };

        sqlx::query(
            r#"INSERT INTO locks
               (tenant_id, namespace, lock_key, holder_id, version, expires_at, lease_duration_secs,
                last_heartbeat, locked, metadata)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)"#,
        )
        .bind(tenant_id)
        .bind(namespace)
        .bind(&options.lock_key)
        .bind(&options.holder_id)
        .bind(&version)
        .bind(expires_at)
        .bind(lease_secs)
        .bind(Self::now_epoch_secs())
        .bind(metadata_json.clone())
        .execute(&mut *tx)
        .await
        .map_err(|e| LockError::BackendError(format!("insert lock: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| LockError::BackendError(format!("commit tx: {e}")))?;

        Self::lock_from_row(
            options.lock_key.clone(),
            options.holder_id.clone(),
            version,
            expires_at,
            lease_secs,
            Self::now_epoch_secs(),
            1,
            metadata_json,
        )
    }

    #[instrument(skip(self, ctx, options), fields(lock_key = %options.lock_key, holder_id = %options.holder_id, version = %options.version, tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace()))]
    async fn renew_lock(&self, ctx: &RequestContext, options: RenewLockOptions) -> LockResult<Lock> {
        let tenant_id = if ctx.tenant_id().is_empty() { "default" } else { ctx.tenant_id() };
        let namespace = if ctx.namespace().is_empty() { "default" } else { ctx.namespace() };
        
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| LockError::BackendError(format!("acquire conn: {e}")))?;
        let mut tx = conn
            .begin()
            .await
            .map_err(|e| LockError::BackendError(format!("begin tx: {e}")))?;

        let now = Self::now_epoch_secs();
        let lease_secs = options.lease_duration_secs as i64;
        let new_expires = now + lease_secs;

        let row = sqlx::query(
            r#"SELECT holder_id, version, expires_at, lease_duration_secs,
                      last_heartbeat, locked, metadata
               FROM locks WHERE tenant_id = ?1 AND namespace = ?2 AND lock_key = ?3"#,
        )
        .bind(tenant_id)
        .bind(namespace)
        .bind(&options.lock_key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| LockError::BackendError(format!("select lock: {e}")))?;

        let row = match row {
            Some(r) => r,
            None => return Err(LockError::LockNotFound(options.lock_key.clone())),
        };

        let holder_id: String = row.get("holder_id");
        let version: String = row.get("version");
        let lease_duration_secs_row: i64 = row.get("lease_duration_secs");
        let _last_heartbeat: i64 = row.get("last_heartbeat");
        let locked_flag: i64 = row.get("locked");
        let metadata_json: Option<String> = row.get("metadata");
        let expires_at_row: i64 = row.get("expires_at");

        if holder_id != options.holder_id {
            return Err(LockError::InvalidHolderId(holder_id));
        }
        if version != options.version {
            return Err(LockError::VersionMismatch {
                expected: version,
                actual: options.version.clone(),
            });
        }
        if expires_at_row <= now || locked_flag == 0 {
            return Err(LockError::LockExpired(options.lock_key.clone()));
        }

        let new_version = Ulid::new().to_string();
        let metadata_json_new = if options.metadata.is_empty() {
            metadata_json
        } else {
            Some(
                serde_json::to_string(&options.metadata)
                    .map_err(|e| LockError::BackendError(format!("encode metadata: {e}")))?,
            )
        };

        sqlx::query(
            r#"UPDATE locks
               SET version = ?4,
                   expires_at = ?5,
                   lease_duration_secs = ?6,
                   last_heartbeat = ?7,
                   metadata = ?8
             WHERE tenant_id = ?1 AND namespace = ?2 AND lock_key = ?3"#,
        )
        .bind(tenant_id)
        .bind(namespace)
        .bind(&options.lock_key)
        .bind(&new_version)
        .bind(new_expires)
        .bind(lease_secs)
        .bind(now)
        .bind(metadata_json_new.clone())
        .execute(&mut *tx)
        .await
        .map_err(|e| LockError::BackendError(format!("update lock: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| LockError::BackendError(format!("commit tx: {e}")))?;

        Self::lock_from_row(
            options.lock_key.clone(),
            options.holder_id.clone(),
            new_version,
            new_expires,
            lease_secs, // Use new lease duration from options, not the old one from DB
            now,
            1,
            metadata_json_new,
        )
    }

    #[instrument(skip(self, ctx, options), fields(lock_key = %options.lock_key, holder_id = %options.holder_id, version = %options.version, tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace()))]
    async fn release_lock(&self, ctx: &RequestContext, options: ReleaseLockOptions) -> LockResult<()> {
        let tenant_id = if ctx.tenant_id().is_empty() { "default" } else { ctx.tenant_id() };
        let namespace = if ctx.namespace().is_empty() { "default" } else { ctx.namespace() };
        
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| LockError::BackendError(format!("acquire conn: {e}")))?;
        let mut tx = conn
            .begin()
            .await
            .map_err(|e| LockError::BackendError(format!("begin tx: {e}")))?;

        // Load lock - MUST filter by tenant_id and namespace
        let row = sqlx::query(
            r#"SELECT holder_id, version FROM locks WHERE tenant_id = ?1 AND namespace = ?2 AND lock_key = ?3"#,
        )
        .bind(tenant_id)
        .bind(namespace)
        .bind(&options.lock_key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| LockError::BackendError(format!("select lock: {e}")))?;

        let row = match row {
            Some(r) => r,
            None => return Err(LockError::LockNotFound(options.lock_key.clone())),
        };

        let holder_id: String = row.get("holder_id");
        let version: String = row.get("version");

        if holder_id != options.holder_id {
            return Err(LockError::InvalidHolderId(holder_id));
        }
        if version != options.version {
            return Err(LockError::VersionMismatch {
                expected: version,
                actual: options.version.clone(),
            });
        }

        if options.delete_lock {
            sqlx::query(r#"DELETE FROM locks WHERE tenant_id = ?1 AND namespace = ?2 AND lock_key = ?3"#)
                .bind(tenant_id)
                .bind(namespace)
                .bind(&options.lock_key)
                .execute(&mut *tx)
                .await
                .map_err(|e| LockError::BackendError(format!("delete lock: {e}")))?;
        } else {
            sqlx::query(
                r#"UPDATE locks
                   SET locked = 0
                 WHERE tenant_id = ?1 AND namespace = ?2 AND lock_key = ?3"#,
            )
            .bind(tenant_id)
            .bind(namespace)
            .bind(&options.lock_key)
            .execute(&mut *tx)
            .await
            .map_err(|e| LockError::BackendError(format!("update lock: {e}")))?;
        }

        tx.commit()
            .await
            .map_err(|e| LockError::BackendError(format!("commit tx: {e}")))?;

        Ok(())
    }

    #[instrument(skip(self, ctx), fields(lock_key = %lock_key, tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace()))]
    async fn get_lock(&self, ctx: &RequestContext, lock_key: &str) -> LockResult<Option<Lock>> {
        let tenant_id = if ctx.tenant_id().is_empty() { "default" } else { ctx.tenant_id() };
        let namespace = if ctx.namespace().is_empty() { "default" } else { ctx.namespace() };
        
        let row = sqlx::query(
            r#"SELECT lock_key, holder_id, version, expires_at, lease_duration_secs,
                      last_heartbeat, locked, metadata
               FROM locks WHERE tenant_id = ?1 AND namespace = ?2 AND lock_key = ?3"#,
        )
        .bind(tenant_id)
        .bind(namespace)
        .bind(lock_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| LockError::BackendError(format!("select lock: {e}")))?;

        if let Some(row) = row {
            let lock_key: String = row.get("lock_key");
            let holder_id: String = row.get("holder_id");
            let version: String = row.get("version");
            let expires_at_row: i64 = row.get("expires_at");
            let lease_duration_secs_row: i64 = row.get("lease_duration_secs");
            let last_heartbeat: i64 = row.get("last_heartbeat");
            let locked_flag: i64 = row.get("locked");
            let metadata_json: Option<String> = row.get("metadata");

            return Self::lock_from_row(
                lock_key,
                holder_id,
                version,
                expires_at_row,
                lease_duration_secs_row,
                last_heartbeat,
                locked_flag,
                metadata_json,
            )
            .map(Some);
        }

        Ok(None)
    }
}


