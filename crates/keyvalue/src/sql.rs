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

//! SQL-based KeyValue store implementations (SQLite and PostgreSQL).
//!
//! ## Purpose
//! Provides persistent, transactional KeyValue storage using relational databases.
//!
//! ## Features
//! - **Persistent**: Data survives process restarts
//! - **Transactional**: ACID guarantees for operations
//! - **TTL Support**: Automatic expiry via expiry column + background cleanup
//! - **Atomic Operations**: CAS and increment using SQL transactions
//!
//! ## Schema (Optimized for Performance)
//! ```sql
//! -- SQLite schema
//! CREATE TABLE kv_store (
//!     key TEXT PRIMARY KEY,           -- Clustered index (automatic)
//!     value BLOB NOT NULL,
//!     expires_at BIGINT,              -- Unix timestamp in seconds, NULL = no expiry
//!     created_at BIGINT NOT NULL,
//!     updated_at BIGINT NOT NULL
//! );
//!
//! -- Composite index for efficient TTL cleanup (O(m) where m = expired keys)
//! CREATE INDEX idx_kv_store_ttl_cleanup
//! ON kv_store(expires_at, key)
//! WHERE expires_at IS NOT NULL;
//!
//! -- Performance optimizations
//! PRAGMA journal_mode=WAL;          -- Better concurrency
//! PRAGMA cache_size=-100000;        -- 100MB cache
//! PRAGMA synchronous=NORMAL;        -- Safe with WAL
//! PRAGMA mmap_size=268435456;       -- 256MB memory-mapped I/O
//! ```
//!
//! **PostgreSQL schema**: Same table structure, uses `BYTEA` for value column.
//!
//! **Performance Characteristics**:
//! - Point lookup: O(log n) via PRIMARY KEY → < 1ms for 1M keys
//! - Prefix query: O(k log n) where k = matching keys → < 10ms for 100 matches
//! - TTL cleanup: O(m) where m = expired keys → < 5ms for 1K expired
//!
//! See `PERFORMANCE_ANALYSIS.md` for detailed optimization rationale.

use crate::{KVError, KVEvent, KVEventType, KVResult, KVStats, KeyValueStore};
use async_trait::async_trait;
use sqlx::{Pool, Postgres, Row, Sqlite};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};

/// Watch subscription for SQL backends.
#[derive(Debug)]
struct Watch {
    pattern: String,
    sender: mpsc::Sender<KVEvent>,
    is_prefix: bool,
}

impl Watch {
    fn matches(&self, key: &str) -> bool {
        if self.is_prefix {
            key.starts_with(&self.pattern)
        } else {
            key == self.pattern
        }
    }
}

/// SQLite-based KeyValue store.
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_keyvalue::{KeyValueStore, sql::SqliteKVStore};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let kv = SqliteKVStore::new("/tmp/plexspaces.db").await?;
///
/// kv.put("key", b"value".to_vec()).await?;
/// let value = kv.get("key").await?;
/// assert_eq!(value, Some(b"value".to_vec()));
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct SqliteKVStore {
    pool: Pool<Sqlite>,
    watches: Arc<RwLock<Vec<Watch>>>,
}

impl SqliteKVStore {
    /// Create a new SQLite KeyValue store.
    ///
    /// ## Arguments
    /// - `path`: Database file path (use ":memory:" for in-memory database)
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_keyvalue::sql::SqliteKVStore;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Persistent database
    /// let kv = SqliteKVStore::new("/tmp/plexspaces.db").await?;
    ///
    /// // In-memory database (for testing)
    /// let kv = SqliteKVStore::new(":memory:").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(path: &str) -> KVResult<Self> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}", path))
            .await?;

        // Create table if not exists
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS kv_store (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL,
                expires_at BIGINT,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Optimized indexes for read/write/query performance
        // See PERFORMANCE_ANALYSIS.md for detailed rationale

        // Index 1: TTL cleanup (composite for efficiency)
        // Enables fast cleanup of expired keys without full table scan
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_kv_store_ttl_cleanup \
             ON kv_store(expires_at, key) \
             WHERE expires_at IS NOT NULL",
        )
        .execute(&pool)
        .await?;

        // Enable SQLite performance optimizations
        // WAL mode: Better concurrency for read-heavy workloads
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await?;

        // Cache size: 100MB for better read performance
        sqlx::query("PRAGMA cache_size=-100000")
            .execute(&pool)
            .await?;

        // Synchronous mode: NORMAL is safe with WAL
        sqlx::query("PRAGMA synchronous=NORMAL")
            .execute(&pool)
            .await?;

        // Memory-mapped I/O for faster reads (256MB)
        sqlx::query("PRAGMA mmap_size=268435456")
            .execute(&pool)
            .await?;

        Ok(Self {
            pool,
            watches: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Notify watchers of a change.
    async fn notify_watchers(&self, key: &str, event: KVEvent) {
        let watches = self.watches.read().await;
        for watch in watches.iter() {
            if watch.matches(key) {
                let _ = watch.sender.send(event.clone()).await;
            }
        }
    }

    /// Get current Unix timestamp in seconds.
    fn now_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    /// Calculate expiry timestamp from TTL.
    fn expiry_from_ttl(ttl: Duration) -> i64 {
        Self::now_timestamp() + ttl.as_secs() as i64
    }

    /// Decode i64 from bytes (big-endian).
    fn decode_i64(bytes: &[u8]) -> KVResult<i64> {
        if bytes.len() != 8 {
            return Err(KVError::InvalidValue(format!(
                "Expected 8 bytes for i64, got {}",
                bytes.len()
            )));
        }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(bytes);
        Ok(i64::from_be_bytes(arr))
    }

    /// Encode i64 to bytes (big-endian).
    fn encode_i64(value: i64) -> Vec<u8> {
        value.to_be_bytes().to_vec()
    }
}

#[async_trait]
impl KeyValueStore for SqliteKVStore {
    async fn get(&self, key: &str) -> KVResult<Option<Vec<u8>>> {
        let now = Self::now_timestamp();

        let result = sqlx::query(
            "SELECT value FROM kv_store WHERE key = ? AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(key)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|row| row.get::<Vec<u8>, _>("value")))
    }

    async fn put(&self, key: &str, value: Vec<u8>) -> KVResult<()> {
        let now = Self::now_timestamp();

        // Get old value for watch notification
        let old_value = self.get(key).await?;

        sqlx::query(
            r#"
            INSERT INTO kv_store (key, value, expires_at, created_at, updated_at)
            VALUES (?, ?, NULL, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                expires_at = NULL,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(key)
        .bind(&value)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        let event = KVEvent {
            key: key.to_string(),
            event_type: KVEventType::Put,
            old_value,
            new_value: Some(value),
        };
        self.notify_watchers(key, event).await;

        Ok(())
    }

    async fn delete(&self, key: &str) -> KVResult<()> {
        let old_value = self.get(key).await?;

        sqlx::query("DELETE FROM kv_store WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;

        if let Some(old_val) = old_value {
            let event = KVEvent {
                key: key.to_string(),
                event_type: KVEventType::Delete,
                old_value: Some(old_val),
                new_value: None,
            };
            self.notify_watchers(key, event).await;
        }

        Ok(())
    }

    async fn exists(&self, key: &str) -> KVResult<bool> {
        let now = Self::now_timestamp();

        let result = sqlx::query(
            "SELECT 1 FROM kv_store WHERE key = ? AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(key)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.is_some())
    }

    async fn list(&self, prefix: &str) -> KVResult<Vec<String>> {
        let now = Self::now_timestamp();
        let pattern = format!("{}%", prefix);

        let rows = sqlx::query(
            "SELECT key FROM kv_store WHERE key LIKE ? AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(pattern)
        .bind(now)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|row| row.get("key")).collect())
    }

    async fn multi_get(&self, keys: &[&str]) -> KVResult<Vec<Option<Vec<u8>>>> {
        let mut results = Vec::with_capacity(keys.len());
        for key in keys {
            results.push(self.get(key).await?);
        }
        Ok(results)
    }

    async fn multi_put(&self, pairs: &[(&str, Vec<u8>)]) -> KVResult<()> {
        let mut tx = self.pool.begin().await?;
        let now = Self::now_timestamp();

        for (key, value) in pairs {
            sqlx::query(
                r#"
                INSERT INTO kv_store (key, value, expires_at, created_at, updated_at)
                VALUES (?, ?, NULL, ?, ?)
                ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    expires_at = NULL,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(key)
            .bind(value)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        // Notify watchers
        for (key, value) in pairs {
            let event = KVEvent {
                key: (*key).to_string(),
                event_type: KVEventType::Put,
                old_value: None,
                new_value: Some(value.clone()),
            };
            self.notify_watchers(key, event).await;
        }

        Ok(())
    }

    async fn put_with_ttl(&self, key: &str, value: Vec<u8>, ttl: Duration) -> KVResult<()> {
        let now = Self::now_timestamp();
        let expires_at = Self::expiry_from_ttl(ttl);

        let old_value = self.get(key).await?;

        sqlx::query(
            r#"
            INSERT INTO kv_store (key, value, expires_at, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                expires_at = excluded.expires_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(key)
        .bind(&value)
        .bind(expires_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        let event = KVEvent {
            key: key.to_string(),
            event_type: KVEventType::Put,
            old_value,
            new_value: Some(value),
        };
        self.notify_watchers(key, event).await;

        Ok(())
    }

    async fn refresh_ttl(&self, key: &str, ttl: Duration) -> KVResult<()> {
        let now = Self::now_timestamp();
        let expires_at = Self::expiry_from_ttl(ttl);

        let result = sqlx::query(
            "UPDATE kv_store SET expires_at = ?, updated_at = ? WHERE key = ? AND (expires_at IS NULL OR expires_at > ?)"
        )
        .bind(expires_at)
        .bind(now)
        .bind(key)
        .bind(now)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(KVError::KeyNotFound(key.to_string()));
        }

        Ok(())
    }

    async fn get_ttl(&self, key: &str) -> KVResult<Option<Duration>> {
        let now = Self::now_timestamp();

        let result = sqlx::query(
            "SELECT expires_at FROM kv_store WHERE key = ? AND (expires_at IS NULL OR expires_at > ?)"
        )
        .bind(key)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.and_then(|row| {
            row.get::<Option<i64>, _>("expires_at")
                .map(|exp| Duration::from_secs((exp - now) as u64))
        }))
    }

    async fn cas(
        &self,
        key: &str,
        expected: Option<Vec<u8>>,
        new_value: Vec<u8>,
    ) -> KVResult<bool> {
        let mut tx = self.pool.begin().await?;
        let now = Self::now_timestamp();

        // Check current value
        let current = sqlx::query(
            "SELECT value FROM kv_store WHERE key = ? AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(key)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?
        .map(|row| row.get::<Vec<u8>, _>("value"));

        let matches = match (&current, &expected) {
            (None, None) => true,
            (Some(curr), Some(exp)) => curr == exp,
            _ => false,
        };

        if matches {
            sqlx::query(
                r#"
                INSERT INTO kv_store (key, value, expires_at, created_at, updated_at)
                VALUES (?, ?, NULL, ?, ?)
                ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    expires_at = NULL,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(key)
            .bind(&new_value)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;

            let event = KVEvent {
                key: key.to_string(),
                event_type: KVEventType::Put,
                old_value: current,
                new_value: Some(new_value),
            };
            self.notify_watchers(key, event).await;

            Ok(true)
        } else {
            tx.rollback().await?;
            Ok(false)
        }
    }

    async fn increment(&self, key: &str, delta: i64) -> KVResult<i64> {
        let mut tx = self.pool.begin().await?;
        let now = Self::now_timestamp();

        // Get current value
        let current = sqlx::query(
            "SELECT value FROM kv_store WHERE key = ? AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(key)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?
        .map(|row| row.get::<Vec<u8>, _>("value"))
        .map(|v| Self::decode_i64(&v))
        .transpose()?
        .unwrap_or(0);

        let new_value = current + delta;
        let encoded = Self::encode_i64(new_value);

        sqlx::query(
            r#"
            INSERT INTO kv_store (key, value, expires_at, created_at, updated_at)
            VALUES (?, ?, NULL, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(key)
        .bind(&encoded)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let event = KVEvent {
            key: key.to_string(),
            event_type: KVEventType::Put,
            old_value: Some(Self::encode_i64(current)),
            new_value: Some(encoded),
        };
        self.notify_watchers(key, event).await;

        Ok(new_value)
    }

    async fn decrement(&self, key: &str, delta: i64) -> KVResult<i64> {
        self.increment(key, -delta).await
    }

    async fn watch(&self, key: &str) -> KVResult<mpsc::Receiver<KVEvent>> {
        let (tx, rx) = mpsc::channel(100);
        let watch = Watch {
            pattern: key.to_string(),
            sender: tx,
            is_prefix: false,
        };

        let mut watches = self.watches.write().await;
        watches.push(watch);

        Ok(rx)
    }

    async fn watch_prefix(&self, prefix: &str) -> KVResult<mpsc::Receiver<KVEvent>> {
        let (tx, rx) = mpsc::channel(100);
        let watch = Watch {
            pattern: prefix.to_string(),
            sender: tx,
            is_prefix: true,
        };

        let mut watches = self.watches.write().await;
        watches.push(watch);

        Ok(rx)
    }

    async fn clear_prefix(&self, prefix: &str) -> KVResult<usize> {
        let pattern = format!("{}%", prefix);

        let result = sqlx::query("DELETE FROM kv_store WHERE key LIKE ?")
            .bind(pattern)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() as usize)
    }

    async fn count_prefix(&self, prefix: &str) -> KVResult<usize> {
        let now = Self::now_timestamp();
        let pattern = format!("{}%", prefix);

        let row = sqlx::query(
            "SELECT COUNT(*) as count FROM kv_store WHERE key LIKE ? AND (expires_at IS NULL OR expires_at > ?)"
        )
        .bind(pattern)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.get::<i64, _>("count") as usize)
    }

    async fn stats(&self) -> KVResult<KVStats> {
        let now = Self::now_timestamp();

        let row = sqlx::query(
            "SELECT COUNT(*) as count, SUM(LENGTH(key) + LENGTH(value)) as size FROM kv_store WHERE expires_at IS NULL OR expires_at > ?"
        )
        .bind(now)
        .fetch_one(&self.pool)
        .await?;

        Ok(KVStats {
            total_keys: row.get::<i64, _>("count") as usize,
            total_size_bytes: row.get::<Option<i64>, _>("size").unwrap_or(0) as usize,
            backend_type: "SQLite".to_string(),
        })
    }
}

/// PostgreSQL-based KeyValue store.
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_keyvalue::{KeyValueStore, sql::PostgreSQLKVStore};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let kv = PostgreSQLKVStore::new("postgres://localhost/plexspaces", 10).await?;
///
/// kv.put("key", b"value".to_vec()).await?;
/// let value = kv.get("key").await?;
/// assert_eq!(value, Some(b"value".to_vec()));
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct PostgreSQLKVStore {
    pool: Pool<Postgres>,
    watches: Arc<RwLock<Vec<Watch>>>,
}

impl PostgreSQLKVStore {
    /// Create a new PostgreSQL KeyValue store.
    ///
    /// ## Arguments
    /// - `connection_string`: PostgreSQL connection string
    /// - `pool_size`: Maximum number of connections in the pool
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_keyvalue::sql::PostgreSQLKVStore;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = PostgreSQLKVStore::new("postgres://user:pass@localhost/plexspaces", 10).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(connection_string: &str, pool_size: u32) -> KVResult<Self> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(pool_size)
            .connect(connection_string)
            .await?;

        // Create table if not exists
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS kv_store (
                key TEXT PRIMARY KEY,
                value BYTEA NOT NULL,
                expires_at BIGINT,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Optimized indexes for read/write/query performance
        // See PERFORMANCE_ANALYSIS.md for detailed rationale

        // Index 1: TTL cleanup (composite for efficiency)
        // Enables fast cleanup of expired keys without full table scan
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_kv_store_ttl_cleanup \
             ON kv_store(expires_at, key) \
             WHERE expires_at IS NOT NULL",
        )
        .execute(&pool)
        .await?;

        // Note: PRIMARY KEY on 'key' already provides index for point lookups
        // No need for redundant idx_kv_store_prefix (removed)

        Ok(Self {
            pool,
            watches: Arc::new(RwLock::new(Vec::new())),
        })
    }

    async fn notify_watchers(&self, key: &str, event: KVEvent) {
        let watches = self.watches.read().await;
        for watch in watches.iter() {
            if watch.matches(key) {
                let _ = watch.sender.send(event.clone()).await;
            }
        }
    }

    fn now_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn expiry_from_ttl(ttl: Duration) -> i64 {
        Self::now_timestamp() + ttl.as_secs() as i64
    }

    fn decode_i64(bytes: &[u8]) -> KVResult<i64> {
        if bytes.len() != 8 {
            return Err(KVError::InvalidValue(format!(
                "Expected 8 bytes for i64, got {}",
                bytes.len()
            )));
        }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(bytes);
        Ok(i64::from_be_bytes(arr))
    }

    fn encode_i64(value: i64) -> Vec<u8> {
        value.to_be_bytes().to_vec()
    }
}

// PostgreSQL implementation is almost identical to SQLite, just using Postgres pool
#[async_trait]
impl KeyValueStore for PostgreSQLKVStore {
    async fn get(&self, key: &str) -> KVResult<Option<Vec<u8>>> {
        let now = Self::now_timestamp();

        let result = sqlx::query(
            "SELECT value FROM kv_store WHERE key = $1 AND (expires_at IS NULL OR expires_at > $2)",
        )
        .bind(key)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|row| row.get::<Vec<u8>, _>("value")))
    }

    async fn put(&self, key: &str, value: Vec<u8>) -> KVResult<()> {
        let now = Self::now_timestamp();
        let old_value = self.get(key).await?;

        sqlx::query(
            r#"
            INSERT INTO kv_store (key, value, expires_at, created_at, updated_at)
            VALUES ($1, $2, NULL, $3, $4)
            ON CONFLICT(key) DO UPDATE SET
                value = EXCLUDED.value,
                expires_at = NULL,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(key)
        .bind(&value)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        let event = KVEvent {
            key: key.to_string(),
            event_type: KVEventType::Put,
            old_value,
            new_value: Some(value),
        };
        self.notify_watchers(key, event).await;

        Ok(())
    }

    async fn delete(&self, key: &str) -> KVResult<()> {
        let old_value = self.get(key).await?;

        sqlx::query("DELETE FROM kv_store WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;

        if let Some(old_val) = old_value {
            let event = KVEvent {
                key: key.to_string(),
                event_type: KVEventType::Delete,
                old_value: Some(old_val),
                new_value: None,
            };
            self.notify_watchers(key, event).await;
        }

        Ok(())
    }

    async fn exists(&self, key: &str) -> KVResult<bool> {
        let now = Self::now_timestamp();

        let result = sqlx::query(
            "SELECT 1 FROM kv_store WHERE key = $1 AND (expires_at IS NULL OR expires_at > $2)",
        )
        .bind(key)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.is_some())
    }

    async fn list(&self, prefix: &str) -> KVResult<Vec<String>> {
        let now = Self::now_timestamp();
        let pattern = format!("{}%", prefix);

        let rows = sqlx::query(
            "SELECT key FROM kv_store WHERE key LIKE $1 AND (expires_at IS NULL OR expires_at > $2)"
        )
        .bind(pattern)
        .bind(now)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|row| row.get("key")).collect())
    }

    async fn multi_get(&self, keys: &[&str]) -> KVResult<Vec<Option<Vec<u8>>>> {
        let mut results = Vec::with_capacity(keys.len());
        for key in keys {
            results.push(self.get(key).await?);
        }
        Ok(results)
    }

    async fn multi_put(&self, pairs: &[(&str, Vec<u8>)]) -> KVResult<()> {
        let mut tx = self.pool.begin().await?;
        let now = Self::now_timestamp();

        for (key, value) in pairs {
            sqlx::query(
                r#"
                INSERT INTO kv_store (key, value, expires_at, created_at, updated_at)
                VALUES ($1, $2, NULL, $3, $4)
                ON CONFLICT(key) DO UPDATE SET
                    value = EXCLUDED.value,
                    expires_at = NULL,
                    updated_at = EXCLUDED.updated_at
                "#,
            )
            .bind(key)
            .bind(value)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        for (key, value) in pairs {
            let event = KVEvent {
                key: (*key).to_string(),
                event_type: KVEventType::Put,
                old_value: None,
                new_value: Some(value.clone()),
            };
            self.notify_watchers(key, event).await;
        }

        Ok(())
    }

    async fn put_with_ttl(&self, key: &str, value: Vec<u8>, ttl: Duration) -> KVResult<()> {
        let now = Self::now_timestamp();
        let expires_at = Self::expiry_from_ttl(ttl);
        let old_value = self.get(key).await?;

        sqlx::query(
            r#"
            INSERT INTO kv_store (key, value, expires_at, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT(key) DO UPDATE SET
                value = EXCLUDED.value,
                expires_at = EXCLUDED.expires_at,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(key)
        .bind(&value)
        .bind(expires_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        let event = KVEvent {
            key: key.to_string(),
            event_type: KVEventType::Put,
            old_value,
            new_value: Some(value),
        };
        self.notify_watchers(key, event).await;

        Ok(())
    }

    async fn refresh_ttl(&self, key: &str, ttl: Duration) -> KVResult<()> {
        let now = Self::now_timestamp();
        let expires_at = Self::expiry_from_ttl(ttl);

        let result = sqlx::query(
            "UPDATE kv_store SET expires_at = $1, updated_at = $2 WHERE key = $3 AND (expires_at IS NULL OR expires_at > $4)"
        )
        .bind(expires_at)
        .bind(now)
        .bind(key)
        .bind(now)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(KVError::KeyNotFound(key.to_string()));
        }

        Ok(())
    }

    async fn get_ttl(&self, key: &str) -> KVResult<Option<Duration>> {
        let now = Self::now_timestamp();

        let result = sqlx::query(
            "SELECT expires_at FROM kv_store WHERE key = $1 AND (expires_at IS NULL OR expires_at > $2)"
        )
        .bind(key)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.and_then(|row| {
            row.get::<Option<i64>, _>("expires_at")
                .map(|exp| Duration::from_secs((exp - now) as u64))
        }))
    }

    async fn cas(
        &self,
        key: &str,
        expected: Option<Vec<u8>>,
        new_value: Vec<u8>,
    ) -> KVResult<bool> {
        let mut tx = self.pool.begin().await?;
        let now = Self::now_timestamp();

        let current = sqlx::query(
            "SELECT value FROM kv_store WHERE key = $1 AND (expires_at IS NULL OR expires_at > $2)",
        )
        .bind(key)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?
        .map(|row| row.get::<Vec<u8>, _>("value"));

        let matches = match (&current, &expected) {
            (None, None) => true,
            (Some(curr), Some(exp)) => curr == exp,
            _ => false,
        };

        if matches {
            sqlx::query(
                r#"
                INSERT INTO kv_store (key, value, expires_at, created_at, updated_at)
                VALUES ($1, $2, NULL, $3, $4)
                ON CONFLICT(key) DO UPDATE SET
                    value = EXCLUDED.value,
                    expires_at = NULL,
                    updated_at = EXCLUDED.updated_at
                "#,
            )
            .bind(key)
            .bind(&new_value)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;

            let event = KVEvent {
                key: key.to_string(),
                event_type: KVEventType::Put,
                old_value: current,
                new_value: Some(new_value),
            };
            self.notify_watchers(key, event).await;

            Ok(true)
        } else {
            tx.rollback().await?;
            Ok(false)
        }
    }

    async fn increment(&self, key: &str, delta: i64) -> KVResult<i64> {
        let mut tx = self.pool.begin().await?;
        let now = Self::now_timestamp();

        let current = sqlx::query(
            "SELECT value FROM kv_store WHERE key = $1 AND (expires_at IS NULL OR expires_at > $2)",
        )
        .bind(key)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?
        .map(|row| row.get::<Vec<u8>, _>("value"))
        .map(|v| Self::decode_i64(&v))
        .transpose()?
        .unwrap_or(0);

        let new_value = current + delta;
        let encoded = Self::encode_i64(new_value);

        sqlx::query(
            r#"
            INSERT INTO kv_store (key, value, expires_at, created_at, updated_at)
            VALUES ($1, $2, NULL, $3, $4)
            ON CONFLICT(key) DO UPDATE SET
                value = EXCLUDED.value,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(key)
        .bind(&encoded)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let event = KVEvent {
            key: key.to_string(),
            event_type: KVEventType::Put,
            old_value: Some(Self::encode_i64(current)),
            new_value: Some(encoded),
        };
        self.notify_watchers(key, event).await;

        Ok(new_value)
    }

    async fn decrement(&self, key: &str, delta: i64) -> KVResult<i64> {
        self.increment(key, -delta).await
    }

    async fn watch(&self, key: &str) -> KVResult<mpsc::Receiver<KVEvent>> {
        let (tx, rx) = mpsc::channel(100);
        let watch = Watch {
            pattern: key.to_string(),
            sender: tx,
            is_prefix: false,
        };

        let mut watches = self.watches.write().await;
        watches.push(watch);

        Ok(rx)
    }

    async fn watch_prefix(&self, prefix: &str) -> KVResult<mpsc::Receiver<KVEvent>> {
        let (tx, rx) = mpsc::channel(100);
        let watch = Watch {
            pattern: prefix.to_string(),
            sender: tx,
            is_prefix: true,
        };

        let mut watches = self.watches.write().await;
        watches.push(watch);

        Ok(rx)
    }

    async fn clear_prefix(&self, prefix: &str) -> KVResult<usize> {
        let pattern = format!("{}%", prefix);

        let result = sqlx::query("DELETE FROM kv_store WHERE key LIKE $1")
            .bind(pattern)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() as usize)
    }

    async fn count_prefix(&self, prefix: &str) -> KVResult<usize> {
        let now = Self::now_timestamp();
        let pattern = format!("{}%", prefix);

        let row = sqlx::query(
            "SELECT COUNT(*) as count FROM kv_store WHERE key LIKE $1 AND (expires_at IS NULL OR expires_at > $2)"
        )
        .bind(pattern)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.get::<i64, _>("count") as usize)
    }

    async fn stats(&self) -> KVResult<KVStats> {
        let now = Self::now_timestamp();

        let row = sqlx::query(
            "SELECT COUNT(*) as count, SUM(LENGTH(key) + LENGTH(value)) as size FROM kv_store WHERE expires_at IS NULL OR expires_at > $1"
        )
        .bind(now)
        .fetch_one(&self.pool)
        .await?;

        Ok(KVStats {
            total_keys: row.get::<i64, _>("count") as usize,
            total_size_bytes: row.get::<Option<i64>, _>("size").unwrap_or(0) as usize,
            backend_type: "PostgreSQL".to_string(),
        })
    }
}

#[cfg(all(test, feature = "sql-backend"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sqlite_basic_operations() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();

        // Put and get
        kv.put("key1", b"value1".to_vec()).await.unwrap();
        let value = kv.get("key1").await.unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Exists
        assert!(kv.exists("key1").await.unwrap());
        assert!(!kv.exists("nonexistent").await.unwrap());

        // Delete
        kv.delete("key1").await.unwrap();
        assert!(!kv.exists("key1").await.unwrap());
    }

    #[tokio::test]
    async fn test_sqlite_ttl() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();

        kv.put_with_ttl("key", b"value".to_vec(), Duration::from_secs(1))
            .await
            .unwrap();

        // Should exist immediately
        assert!(kv.exists("key").await.unwrap());

        // Check TTL
        let ttl = kv.get_ttl("key").await.unwrap();
        assert!(ttl.is_some());

        // Wait for expiry
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(!kv.exists("key").await.unwrap());
    }

    #[tokio::test]
    async fn test_sqlite_cas() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();

        // Acquire lock
        let acquired = kv
            .cas("lock:resource", None, b"node1".to_vec())
            .await
            .unwrap();
        assert!(acquired);

        // Try to acquire again (should fail)
        let acquired2 = kv
            .cas("lock:resource", None, b"node2".to_vec())
            .await
            .unwrap();
        assert!(!acquired2);
    }

    #[tokio::test]
    async fn test_sqlite_increment() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();

        let count = kv.increment("counter", 1).await.unwrap();
        assert_eq!(count, 1);

        let count = kv.increment("counter", 5).await.unwrap();
        assert_eq!(count, 6);

        let count = kv.decrement("counter", 3).await.unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_sqlite_list_prefix() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();

        kv.put("actor:alice", b"ref1".to_vec()).await.unwrap();
        kv.put("actor:bob", b"ref2".to_vec()).await.unwrap();
        kv.put("node:node1", b"info".to_vec()).await.unwrap();

        let actors = kv.list("actor:").await.unwrap();
        assert_eq!(actors.len(), 2);
    }

    #[tokio::test]
    async fn test_sqlite_stats() {
        let kv = SqliteKVStore::new(":memory:").await.unwrap();

        kv.put("k1", b"v1".to_vec()).await.unwrap();
        kv.put("k2", b"v2".to_vec()).await.unwrap();

        let stats = kv.stats().await.unwrap();
        assert_eq!(stats.total_keys, 2);
        assert_eq!(stats.backend_type, "SQLite");
    }
}
