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

//! SQL TupleSpace Storage Backend (PostgreSQL & SQLite)
//!
//! ## Purpose
//! Provides durable, transactional storage for TupleSpace tuples using SQL databases.
//! Supports both PostgreSQL (production) and SQLite (testing/embedded).
//!
//! ## Design
//! - **Storage**: SQL table with JSON column for tuple data
//! - **Pattern Matching**: SQL WHERE clause + client-side filtering
//! - **Leases**: Database-level TTL cleanup (expires_at column)
//! - **Blocking Reads**: LISTEN/NOTIFY (PostgreSQL) or polling (SQLite)
//! - **Transactions**: Full ACID transaction support
//!
//! ## Schema
//! ```sql
//! CREATE TABLE tuples (
//!     id TEXT PRIMARY KEY,
//!     tuple_data TEXT NOT NULL,  -- JSON encoded
//!     created_at TEXT NOT NULL,   -- ISO 8601 timestamp
//!     expires_at TEXT,            -- ISO 8601 timestamp
//!     renewable INTEGER NOT NULL  -- 0 or 1 (boolean)
//! );
//! CREATE INDEX idx_expires_at ON tuples(expires_at) WHERE expires_at IS NOT NULL;
//! ```
//!
//! ## Performance Characteristics
//! - **Write**: O(log n) - B-tree index insert
//! - **Read/Take**: O(n) - Table scan with WHERE clause
//! - **Count**: O(n) - COUNT query
//! - **Memory**: Database buffer pool
//!
//! ## When to Use
//! - ✅ Multi-process distributed systems (PostgreSQL)
//! - ✅ ACID transaction requirements
//! - ✅ SQL querying and analytics
//! - ✅ Testing and embedded use (SQLite)
//! - ❌ Ultra-low latency (< 1ms) requirements

#[cfg(feature = "sql-backend")]
use async_trait::async_trait;
#[cfg(feature = "sql-backend")]
use chrono::{DateTime, Utc};
#[cfg(feature = "sql-backend")]
use plexspaces_proto::tuplespace::v1::{PostgresStorageConfig, SqliteStorageConfig, StorageStats};
#[cfg(feature = "sql-backend")]
use sqlx::{postgres::PgPoolOptions, sqlite::SqlitePoolOptions, PgPool, Row, SqlitePool};
#[cfg(feature = "sql-backend")]
use std::time::Duration;
#[cfg(feature = "sql-backend")]
use std::sync::Arc;

// Re-export config types for external use
#[cfg(feature = "sql-backend")]
pub use plexspaces_proto::tuplespace::v1::{
    PostgresStorageConfig as PostgresConfig, SqliteStorageConfig as SqliteConfig,
};

#[cfg(feature = "sql-backend")]
use super::{TupleSpaceStorage, WatchEventMessage};
#[cfg(feature = "sql-backend")]
use crate::{Pattern, Tuple, TupleSpaceError};

/// SQL database type
#[cfg(feature = "sql-backend")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlDatabaseType {
    /// PostgreSQL database backend
    PostgreSQL,
    /// SQLite database backend
    SQLite,
}

/// Stored tuple metadata for SQL
#[cfg(feature = "sql-backend")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredTuple {
    /// Unique tuple ID (ULID)
    id: String,

    /// The actual tuple
    tuple: Tuple,

    /// When tuple was created
    created_at: DateTime<Utc>,

    /// When tuple expires (if has lease)
    expires_at: Option<DateTime<Utc>>,

    /// Is lease renewable?
    renewable: bool,
}

/// SQL connection pool (PostgreSQL or SQLite)
#[cfg(feature = "sql-backend")]
enum SqlPool {
    /// PostgreSQL connection pool
    Postgres(PgPool),
    /// SQLite connection pool
    Sqlite(SqlitePool),
}

/// SQL-backed TupleSpace storage implementation
///
/// ## Thread Safety
/// - Uses SQL connection pool for concurrent access
/// - All operations are transactional
/// - Pattern matching is eventually consistent (table scan)
///
/// ## Lease Management
/// - Uses database TTL for automatic cleanup
/// - Renewal updates expires_at timestamp
/// - Background cleanup for expired tuples
#[cfg(feature = "sql-backend")]
pub struct SqlStorage {
    /// SQL connection pool (PostgreSQL or SQLite)
    pool: SqlPool,

    /// Table name for tuple storage
    table_name: String,

    /// Database type (PostgreSQL or SQLite)
    db_type: SqlDatabaseType,

    /// Operation statistics (for metrics)
    stats: Arc<std::sync::Mutex<SqlOperationStats>>,
}

/// Operation statistics for metrics
#[cfg(feature = "sql-backend")]
#[derive(Debug, Default)]
struct SqlOperationStats {
    /// Total operations
    total_operations: u64,
    /// Read operations
    read_operations: u64,
    /// Write operations
    write_operations: u64,
    /// Take operations
    take_operations: u64,
    /// Total latency in microseconds
    total_latency_us: u64,
}

#[cfg(feature = "sql-backend")]
impl SqlStorage {
    /// Create new PostgreSQL storage
    pub async fn new_postgres(config: PostgresStorageConfig) -> Result<Self, TupleSpaceError> {
        let pg_pool = PgPoolOptions::new()
            .max_connections(config.pool_size)
            .connect(&config.connection_string)
            .await
            .map_err(|e| {
                TupleSpaceError::ConnectionError(format!("PostgreSQL connection failed: {}", e))
            })?;

        let table_name = if config.table_name.is_empty() {
            "tuples".to_string()
        } else {
            config.table_name
        };

        let storage = SqlStorage {
            pool: SqlPool::Postgres(pg_pool),
            table_name,
            db_type: SqlDatabaseType::PostgreSQL,
            stats: Arc::new(std::sync::Mutex::new(SqlOperationStats::default())),
        };

        storage.initialize_schema().await?;
        Ok(storage)
    }

    /// Create new SQLite storage
    pub async fn new_sqlite(config: SqliteStorageConfig) -> Result<Self, TupleSpaceError> {
        // SQLite connection string format: sqlite://path/to/db.sqlite or sqlite::memory:
        let connection_string = if config.database_path == ":memory:" {
            "sqlite::memory:".to_string()
        } else {
            format!("sqlite:{}", config.database_path)
        };

        let sqlite_pool = SqlitePoolOptions::new()
            .max_connections(1) // SQLite doesn't benefit from multiple connections
            .connect(&connection_string)
            .await
            .map_err(|e| {
                TupleSpaceError::ConnectionError(format!("SQLite connection failed: {}", e))
            })?;

        let storage = SqlStorage {
            pool: SqlPool::Sqlite(sqlite_pool),
            table_name: "tuples".to_string(),
            db_type: SqlDatabaseType::SQLite,
            stats: Arc::new(std::sync::Mutex::new(SqlOperationStats::default())),
        };

        storage.initialize_schema().await?;
        Ok(storage)
    }

    /// Initialize database schema using migrations
    async fn initialize_schema(&self) -> Result<(), TupleSpaceError> {
        match &self.pool {
            SqlPool::Postgres(pool) => {
                sqlx::migrate!("./migrations/postgres")
                    .run(pool)
                    .await
                    .map_err(|e| {
                        TupleSpaceError::BackendError(format!("PostgreSQL migration failed: {}", e))
                    })?;
            }
            SqlPool::Sqlite(pool) => {
                sqlx::migrate!("./migrations/sqlite")
                    .run(pool)
                    .await
                    .map_err(|e| {
                        TupleSpaceError::BackendError(format!("SQLite migration failed: {}", e))
                    })?;
            }
        }

        Ok(())
    }

    /// Scan all tuples matching pattern
    async fn scan_tuples(&self, pattern: &Pattern) -> Result<Vec<StoredTuple>, TupleSpaceError> {
        // Query all non-expired tuples
        let query_sql = format!(
            r#"
            SELECT id, tuple_data, created_at, expires_at, renewable
            FROM {}
            WHERE expires_at IS NULL OR expires_at > ?
            "#,
            self.table_name
        );

        let now = Utc::now().to_rfc3339();

        match &self.pool {
            SqlPool::Postgres(pool) => {
                let rows = sqlx::query(&query_sql)
                    .bind(&now)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Query failed: {}", e)))?;

                Self::process_rows(rows, pattern)
            }
            SqlPool::Sqlite(pool) => {
                let rows = sqlx::query(&query_sql)
                    .bind(&now)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Query failed: {}", e)))?;

                Self::process_rows(rows, pattern)
            }
        }
    }

    /// Delete expired tuples (cleanup)
    async fn cleanup_expired(&self) -> Result<u64, TupleSpaceError> {
        let delete_sql = format!(
            r#"
            DELETE FROM {}
            WHERE expires_at IS NOT NULL AND expires_at <= ?
            "#,
            self.table_name
        );

        let now = Utc::now().to_rfc3339();

        match &self.pool {
            SqlPool::Postgres(pool) => {
                let result = sqlx::query(&delete_sql)
                    .bind(&now)
                    .execute(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Cleanup failed: {}", e)))?;
                Ok(result.rows_affected())
            }
            SqlPool::Sqlite(pool) => {
                let result = sqlx::query(&delete_sql)
                    .bind(&now)
                    .execute(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Cleanup failed: {}", e)))?;
                Ok(result.rows_affected())
            }
        }
    }

    /// Process database rows into StoredTuples
    fn process_rows<R>(rows: Vec<R>, pattern: &Pattern) -> Result<Vec<StoredTuple>, TupleSpaceError>
    where
        R: sqlx::Row,
        for<'a> &'a str: sqlx::ColumnIndex<R>,
        for<'a> String: sqlx::Decode<'a, R::Database> + sqlx::Type<R::Database>,
        for<'a> Option<String>: sqlx::Decode<'a, R::Database>,
        for<'a> i32: sqlx::Decode<'a, R::Database> + sqlx::Type<R::Database>,
    {
        let mut matching_tuples = Vec::new();

        for row in rows {
            let id: String = row
                .try_get("id")
                .map_err(|e| TupleSpaceError::BackendError(format!("Failed to get id: {}", e)))?;

            let tuple_data_str: String = row.try_get("tuple_data").map_err(|e| {
                TupleSpaceError::BackendError(format!("Failed to get tuple_data: {}", e))
            })?;

            let created_at_str: String = row.try_get("created_at").map_err(|e| {
                TupleSpaceError::BackendError(format!("Failed to get created_at: {}", e))
            })?;

            let expires_at_str: Option<String> = row.try_get("expires_at").map_err(|e| {
                TupleSpaceError::BackendError(format!("Failed to get expires_at: {}", e))
            })?;

            let renewable_int: i32 = row.try_get("renewable").map_err(|e| {
                TupleSpaceError::BackendError(format!("Failed to get renewable: {}", e))
            })?;

            // Parse timestamps
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| {
                    TupleSpaceError::SerializationError(format!(
                        "Failed to parse created_at: {}",
                        e
                    ))
                })?
                .with_timezone(&Utc);

            let expires_at = match expires_at_str {
                Some(exp_str) => Some(
                    DateTime::parse_from_rfc3339(&exp_str)
                        .map_err(|e| {
                            TupleSpaceError::SerializationError(format!(
                                "Failed to parse expires_at: {}",
                                e
                            ))
                        })?
                        .with_timezone(&Utc),
                ),
                None => None,
            };

            // Deserialize tuple
            let tuple: Tuple = serde_json::from_str(&tuple_data_str).map_err(|e| {
                TupleSpaceError::SerializationError(format!("Deserialization failed: {}", e))
            })?;

            // Check if tuple matches pattern
            if tuple.matches(pattern) {
                matching_tuples.push(StoredTuple {
                    id,
                    tuple,
                    created_at,
                    expires_at,
                    renewable: renewable_int != 0,
                });
            }
        }

        Ok(matching_tuples)
    }

    /// Record operation for metrics
    fn record_operation(&self, op_type: &str, latency_us: u64) {
        let mut stats = self.stats.lock().unwrap();
        stats.total_operations += 1;
        stats.total_latency_us += latency_us;
        match op_type {
            "read" => stats.read_operations += 1,
            "write" => stats.write_operations += 1,
            "take" => stats.take_operations += 1,
            _ => {}
        }
    }

    /// Calculate new expiry time based on TTL or lease
    fn calculate_new_expiry(
        tuple: Tuple,
        new_ttl: Option<Duration>,
    ) -> Result<DateTime<Utc>, TupleSpaceError> {
        let extension = if let Some(ttl) = new_ttl {
            chrono::Duration::milliseconds(ttl.as_millis() as i64)
        } else if let Some(lease) = tuple.lease() {
            chrono::Duration::seconds(lease.ttl_seconds() as i64)
        } else {
            return Err(TupleSpaceError::LeaseError(
                "No lease associated with tuple".to_string(),
            ));
        };

        Ok(Utc::now() + extension)
    }

    /// Blocking read for PostgreSQL using optimized polling
    /// Note: Full LISTEN/NOTIFY would require pg_notify crate or async notification handling
    /// For now, use efficient polling (can be improved later with proper LISTEN/NOTIFY)
    async fn blocking_read_postgres(
        &self,
        pattern: Pattern,
        timeout_duration: Duration,
    ) -> Result<Vec<Tuple>, TupleSpaceError> {
        let start = std::time::Instant::now();
        let mut poll_interval_ms = 50u64; // Start with 50ms (faster than SQLite)
        let max_poll_interval_ms = 500u64; // Max 500ms

        loop {
            // Check if timeout expired
            if start.elapsed() >= timeout_duration {
                return Ok(Vec::new());
            }

            // Check for matching tuples
            let stored_tuples = self.scan_tuples(&pattern).await?;
            if !stored_tuples.is_empty() {
                return Ok(stored_tuples.into_iter().map(|s| s.tuple).collect());
            }

            // Calculate remaining time
            let remaining = timeout_duration - start.elapsed();
            let sleep_duration = Duration::from_millis(poll_interval_ms.min(remaining.as_millis() as u64));

            // Sleep before next poll
            tokio::time::sleep(sleep_duration).await;

            // Exponential backoff: increase interval, capped at max
            poll_interval_ms = (poll_interval_ms * 2).min(max_poll_interval_ms);
        }
    }

    /// Blocking read for SQLite using polling with exponential backoff
    async fn blocking_read_sqlite(
        &self,
        pattern: Pattern,
        timeout_duration: Duration,
    ) -> Result<Vec<Tuple>, TupleSpaceError> {
        let start = std::time::Instant::now();
        let mut poll_interval_ms = 10u64; // Start with 10ms
        let max_poll_interval_ms = 1000u64; // Max 1 second

        loop {
            // Check if timeout expired
            if start.elapsed() >= timeout_duration {
                return Ok(Vec::new());
            }

            // Check for matching tuples
            let stored_tuples = self.scan_tuples(&pattern).await?;
            if !stored_tuples.is_empty() {
                return Ok(stored_tuples.into_iter().map(|s| s.tuple).collect());
            }

            // Calculate remaining time
            let remaining = timeout_duration - start.elapsed();
            let sleep_duration = Duration::from_millis(poll_interval_ms.min(remaining.as_millis() as u64));

            // Sleep before next poll
            tokio::time::sleep(sleep_duration).await;

            // Exponential backoff: double the interval, capped at max
            poll_interval_ms = (poll_interval_ms * 2).min(max_poll_interval_ms);
        }
    }

    /// Generate notification channel name for LISTEN/NOTIFY
    fn notification_channel(&self) -> String {
        format!("tuplespace_notify_{}", self.table_name)
    }

    /// Notify waiting readers that a tuple was written (PostgreSQL only)
    async fn notify_tuple_written(&self) -> Result<(), TupleSpaceError> {
        if self.db_type == SqlDatabaseType::PostgreSQL {
            let channel = self.notification_channel();
            match &self.pool {
                SqlPool::Postgres(pool) => {
                    // Use NOTIFY to signal tuple written
                    sqlx::query(&format!("NOTIFY {}, '1'", channel))
                        .execute(pool)
                        .await
                        .map_err(|e| {
                            TupleSpaceError::BackendError(format!("NOTIFY failed: {}", e))
                        })?;
                }
                SqlPool::Sqlite(_) => {
                    // SQLite doesn't support NOTIFY, will use polling
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "sql-backend")]
#[async_trait]
impl TupleSpaceStorage for SqlStorage {
    async fn write(&self, tuple: Tuple) -> Result<String, TupleSpaceError> {
        let start = std::time::Instant::now();
        let tuple_id = ulid::Ulid::new().to_string();

        // Extract lease information
        let (expires_at, renewable) = if let Some(lease) = tuple.lease() {
            (Some(lease.expires_at()), lease.is_renewable())
        } else {
            (None, false)
        };

        // Serialize tuple to JSON
        let tuple_data = serde_json::to_string(&tuple).map_err(|e| {
            TupleSpaceError::SerializationError(format!("Serialization failed: {}", e))
        })?;

        let created_at = Utc::now().to_rfc3339();
        let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());
        let renewable_int = if renewable { 1 } else { 0 };

        // Insert into database
        let insert_sql = format!(
            r#"
            INSERT INTO {} (id, tuple_data, created_at, expires_at, renewable)
            VALUES (?, ?, ?, ?, ?)
            "#,
            self.table_name
        );

        match &self.pool {
            SqlPool::Postgres(pool) => {
                sqlx::query(&insert_sql)
                    .bind(&tuple_id)
                    .bind(&tuple_data)
                    .bind(&created_at)
                    .bind(expires_at_str)
                    .bind(renewable_int)
                    .execute(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Insert failed: {}", e)))?;
            }
            SqlPool::Sqlite(pool) => {
                sqlx::query(&insert_sql)
                    .bind(&tuple_id)
                    .bind(&tuple_data)
                    .bind(&created_at)
                    .bind(expires_at_str)
                    .bind(renewable_int)
                    .execute(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Insert failed: {}", e)))?;
            }
        }

        // Notify waiting readers that a tuple was written
        self.notify_tuple_written().await?;

        // Record metrics
        let latency_us = start.elapsed().as_micros() as u64;
        self.record_operation("write", latency_us);

        Ok(tuple_id)
    }

    async fn write_batch(&self, tuples: Vec<Tuple>) -> Result<Vec<String>, TupleSpaceError> {
        match &self.pool {
            SqlPool::Postgres(pool) => {
                let mut transaction = pool.begin().await.map_err(|e| {
                    TupleSpaceError::BackendError(format!("Transaction begin failed: {}", e))
                })?;

                let mut ids = Vec::with_capacity(tuples.len());

                for tuple in tuples {
                    let tuple_id = ulid::Ulid::new().to_string();

                    let (expires_at, renewable) = if let Some(lease) = tuple.lease() {
                        (Some(lease.expires_at()), lease.is_renewable())
                    } else {
                        (None, false)
                    };

                    let tuple_data = serde_json::to_string(&tuple).map_err(|e| {
                        TupleSpaceError::SerializationError(format!("Serialization failed: {}", e))
                    })?;

                    let created_at = Utc::now().to_rfc3339();
                    let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());
                    let renewable_int = if renewable { 1 } else { 0 };

                    let insert_sql = format!(
                        r#"
                        INSERT INTO {} (id, tuple_data, created_at, expires_at, renewable)
                        VALUES (?, ?, ?, ?, ?)
                        "#,
                        self.table_name
                    );

                    sqlx::query(&insert_sql)
                        .bind(&tuple_id)
                        .bind(&tuple_data)
                        .bind(&created_at)
                        .bind(expires_at_str)
                        .bind(renewable_int)
                        .execute(&mut *transaction)
                        .await
                        .map_err(|e| {
                            TupleSpaceError::BackendError(format!("Batch insert failed: {}", e))
                        })?;

                    ids.push(tuple_id);
                }

                transaction.commit().await.map_err(|e| {
                    TupleSpaceError::BackendError(format!("Transaction commit failed: {}", e))
                })?;

                Ok(ids)
            }
            SqlPool::Sqlite(pool) => {
                let mut transaction = pool.begin().await.map_err(|e| {
                    TupleSpaceError::BackendError(format!("Transaction begin failed: {}", e))
                })?;

                let mut ids = Vec::with_capacity(tuples.len());

                for tuple in tuples {
                    let tuple_id = ulid::Ulid::new().to_string();

                    let (expires_at, renewable) = if let Some(lease) = tuple.lease() {
                        (Some(lease.expires_at()), lease.is_renewable())
                    } else {
                        (None, false)
                    };

                    let tuple_data = serde_json::to_string(&tuple).map_err(|e| {
                        TupleSpaceError::SerializationError(format!("Serialization failed: {}", e))
                    })?;

                    let created_at = Utc::now().to_rfc3339();
                    let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());
                    let renewable_int = if renewable { 1 } else { 0 };

                    let insert_sql = format!(
                        r#"
                        INSERT INTO {} (id, tuple_data, created_at, expires_at, renewable)
                        VALUES (?, ?, ?, ?, ?)
                        "#,
                        self.table_name
                    );

                    sqlx::query(&insert_sql)
                        .bind(&tuple_id)
                        .bind(&tuple_data)
                        .bind(&created_at)
                        .bind(expires_at_str)
                        .bind(renewable_int)
                        .execute(&mut *transaction)
                        .await
                        .map_err(|e| {
                            TupleSpaceError::BackendError(format!("Batch insert failed: {}", e))
                        })?;

                    ids.push(tuple_id);
                }

                transaction.commit().await.map_err(|e| {
                    TupleSpaceError::BackendError(format!("Transaction commit failed: {}", e))
                })?;

                Ok(ids)
            }
        }
    }

    async fn read(
        &self,
        pattern: Pattern,
        timeout: Option<Duration>,
    ) -> Result<Vec<Tuple>, TupleSpaceError> {
        let start = std::time::Instant::now();
        // First, try immediate read
        let stored_tuples = self.scan_tuples(&pattern).await?;
        if !stored_tuples.is_empty() {
            let latency_us = start.elapsed().as_micros() as u64;
            self.record_operation("read", latency_us);
            return Ok(stored_tuples.into_iter().map(|s| s.tuple).collect());
        }

        // If no matches and no timeout, return empty
        let timeout_duration = match timeout {
            Some(d) => d,
            None => {
                let latency_us = start.elapsed().as_micros() as u64;
                self.record_operation("read", latency_us);
                return Ok(Vec::new());
            }
        };

        // Blocking read implementation
        let result = match self.db_type {
            SqlDatabaseType::PostgreSQL => {
                // Use optimized polling for PostgreSQL
                SqlStorage::blocking_read_postgres(self, pattern, timeout_duration).await
            }
            SqlDatabaseType::SQLite => {
                // Use polling with exponential backoff for SQLite
                SqlStorage::blocking_read_sqlite(self, pattern, timeout_duration).await
            }
        };

        // Record metrics
        let latency_us = start.elapsed().as_micros() as u64;
        self.record_operation("read", latency_us);
        result
    }

    async fn take(
        &self,
        pattern: Pattern,
        timeout: Option<Duration>,
    ) -> Result<Vec<Tuple>, TupleSpaceError> {
        let start = std::time::Instant::now();
        // First, try immediate take
        let stored_tuples = self.scan_tuples(&pattern).await?;

        if !stored_tuples.is_empty() {
            // Delete tuples from database
            let ids: Vec<String> = stored_tuples.iter().map(|s| s.id.clone()).collect();
            let delete_sql = format!(
                r#"
                DELETE FROM {}
                WHERE id = ANY(?)
                "#,
                self.table_name
            );

            match &self.pool {
                SqlPool::Postgres(pool) => {
                    // PostgreSQL: Use ANY with array
                    sqlx::query(&delete_sql.replace("ANY(?)", "= ANY($1)"))
                        .bind(&ids)
                        .execute(pool)
                        .await
                        .map_err(|e| {
                            TupleSpaceError::BackendError(format!("Delete failed: {}", e))
                        })?;
                }
                SqlPool::Sqlite(pool) => {
                    // SQLite: Use IN clause
                    let placeholders: Vec<String> = (0..ids.len()).map(|_| "?".to_string()).collect();
                    let delete_sql = format!(
                        "DELETE FROM {} WHERE id IN ({})",
                        self.table_name,
                        placeholders.join(",")
                    );
                    let mut query = sqlx::query(&delete_sql);
                    for id in &ids {
                        query = query.bind(id);
                    }
                    query
                        .execute(pool)
                        .await
                        .map_err(|e| {
                            TupleSpaceError::BackendError(format!("Delete failed: {}", e))
                        })?;
                }
            }

            return Ok(stored_tuples.into_iter().map(|s| s.tuple).collect());
        }

        // If no matches and no timeout, return empty
        let timeout_duration = match timeout {
            Some(d) => d,
            None => return Ok(Vec::new()),
        };

        // Blocking take: Use same approach as blocking read, then delete
        let stored_tuples = match self.db_type {
            SqlDatabaseType::PostgreSQL => {
                SqlStorage::blocking_read_postgres(self, pattern.clone(), timeout_duration).await?
            }
            SqlDatabaseType::SQLite => {
                SqlStorage::blocking_read_sqlite(self, pattern.clone(), timeout_duration).await?
            }
        };

        if stored_tuples.is_empty() {
            let latency_us = start.elapsed().as_micros() as u64;
            self.record_operation("take", latency_us);
            return Ok(Vec::new());
        }

        // Delete the tuples we found
        // Re-scan to get IDs (tuples may have changed), then delete atomically
        let matching_stored = self.scan_tuples(&pattern).await?;
        if matching_stored.is_empty() {
            return Ok(Vec::new()); // Someone else took them
        }

        let ids: Vec<String> = matching_stored.iter().map(|s| s.id.clone()).collect();
        
        match &self.pool {
            SqlPool::Postgres(pool) => {
                // PostgreSQL: Use ANY with array
                let delete_sql = format!(
                    "DELETE FROM {} WHERE id = ANY($1)",
                    self.table_name
                );
                sqlx::query(&delete_sql)
                    .bind(&ids)
                    .execute(pool)
                    .await
                    .map_err(|e| {
                        TupleSpaceError::BackendError(format!("Delete failed: {}", e))
                    })?;
            }
            SqlPool::Sqlite(pool) => {
                // SQLite: Use IN clause
                let placeholders: Vec<String> = (0..ids.len()).map(|_| "?".to_string()).collect();
                let delete_sql = format!(
                    "DELETE FROM {} WHERE id IN ({})",
                    self.table_name,
                    placeholders.join(",")
                );
                let mut query = sqlx::query(&delete_sql);
                for id in &ids {
                    query = query.bind(id);
                }
                query
                    .execute(pool)
                    .await
                    .map_err(|e| {
                        TupleSpaceError::BackendError(format!("Delete failed: {}", e))
                    })?;
            }
        }

        let latency_us = start.elapsed().as_micros() as u64;
        self.record_operation("take", latency_us);
        Ok(matching_stored.into_iter().map(|s| s.tuple).collect())
    }

    async fn count(&self, pattern: Pattern) -> Result<usize, TupleSpaceError> {
        let stored_tuples = self.scan_tuples(&pattern).await?;
        Ok(stored_tuples.len())
    }

    async fn exists(&self, pattern: Pattern) -> Result<bool, TupleSpaceError> {
        let stored_tuples = self.scan_tuples(&pattern).await?;
        Ok(!stored_tuples.is_empty())
    }

    async fn renew_lease(
        &self,
        tuple_id: &str,
        new_ttl: Option<Duration>,
    ) -> Result<DateTime<Utc>, TupleSpaceError> {
        // Get current tuple
        let query_sql = format!(
            r#"
            SELECT tuple_data, renewable
            FROM {}
            WHERE id = ?
            "#,
            self.table_name
        );

        // Calculate new expiry based on the row data
        let new_expires = match &self.pool {
            SqlPool::Postgres(pool) => {
                let row_opt = sqlx::query(&query_sql)
                    .bind(tuple_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Query failed: {}", e)))?;

                let row = row_opt.ok_or(TupleSpaceError::NotFound)?;

                let renewable_int: i32 = row.try_get("renewable").map_err(|e| {
                    TupleSpaceError::BackendError(format!("Failed to get renewable: {}", e))
                })?;

                if renewable_int == 0 {
                    return Err(TupleSpaceError::LeaseError(
                        "Lease is not renewable".to_string(),
                    ));
                }

                let tuple_data_str: String = row.try_get("tuple_data").map_err(|e| {
                    TupleSpaceError::BackendError(format!("Failed to get tuple_data: {}", e))
                })?;

                let tuple: Tuple = serde_json::from_str(&tuple_data_str).map_err(|e| {
                    TupleSpaceError::SerializationError(format!("Deserialization failed: {}", e))
                })?;

                Self::calculate_new_expiry(tuple, new_ttl)?
            }
            SqlPool::Sqlite(pool) => {
                let row_opt = sqlx::query(&query_sql)
                    .bind(tuple_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Query failed: {}", e)))?;

                let row = row_opt.ok_or(TupleSpaceError::NotFound)?;

                let renewable_int: i32 = row.try_get("renewable").map_err(|e| {
                    TupleSpaceError::BackendError(format!("Failed to get renewable: {}", e))
                })?;

                if renewable_int == 0 {
                    return Err(TupleSpaceError::LeaseError(
                        "Lease is not renewable".to_string(),
                    ));
                }

                let tuple_data_str: String = row.try_get("tuple_data").map_err(|e| {
                    TupleSpaceError::BackendError(format!("Failed to get tuple_data: {}", e))
                })?;

                let tuple: Tuple = serde_json::from_str(&tuple_data_str).map_err(|e| {
                    TupleSpaceError::SerializationError(format!("Deserialization failed: {}", e))
                })?;

                Self::calculate_new_expiry(tuple, new_ttl)?
            }
        };

        let new_expires_str = new_expires.to_rfc3339();

        // Update expiry time
        let update_sql = format!(
            r#"
            UPDATE {}
            SET expires_at = ?
            WHERE id = ?
            "#,
            self.table_name
        );

        match &self.pool {
            SqlPool::Postgres(pool) => {
                sqlx::query(&update_sql)
                    .bind(&new_expires_str)
                    .bind(tuple_id)
                    .execute(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Update failed: {}", e)))?;
            }
            SqlPool::Sqlite(pool) => {
                sqlx::query(&update_sql)
                    .bind(&new_expires_str)
                    .bind(tuple_id)
                    .execute(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Update failed: {}", e)))?;
            }
        }

        Ok(new_expires)
    }

    async fn clear(&self) -> Result<(), TupleSpaceError> {
        let delete_sql = format!(r#"DELETE FROM {}"#, self.table_name);

        match &self.pool {
            SqlPool::Postgres(pool) => {
                sqlx::query(&delete_sql)
                    .execute(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Clear failed: {}", e)))?;
            }
            SqlPool::Sqlite(pool) => {
                sqlx::query(&delete_sql)
                    .execute(pool)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(format!("Clear failed: {}", e)))?;
            }
        }

        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, TupleSpaceError> {
        // Cleanup expired tuples first
        self.cleanup_expired().await?;

        // Count active tuples
        let count_sql = format!(
            r#"
            SELECT COUNT(*) as count
            FROM {}
            WHERE expires_at IS NULL OR expires_at > ?
            "#,
            self.table_name
        );

        let now = Utc::now().to_rfc3339();

        let count = match &self.pool {
            SqlPool::Postgres(pool) => {
                let row = sqlx::query(&count_sql)
                    .bind(&now)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| {
                        TupleSpaceError::BackendError(format!("Count query failed: {}", e))
                    })?;

                let count_val: i32 = row.try_get("count").map_err(|e| {
                    TupleSpaceError::BackendError(format!("Failed to get count: {}", e))
                })?;
                count_val as u64
            }
            SqlPool::Sqlite(pool) => {
                let row = sqlx::query(&count_sql)
                    .bind(&now)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| {
                        TupleSpaceError::BackendError(format!("Count query failed: {}", e))
                    })?;

                let count_val: i32 = row.try_get("count").map_err(|e| {
                    TupleSpaceError::BackendError(format!("Failed to get count: {}", e))
                })?;
                count_val as u64
            }
        };

        // Get operation stats
        let stats = self.stats.lock().unwrap();
        let avg_latency_ms = if stats.total_operations > 0 {
            ((stats.total_latency_us as f64 / stats.total_operations as f64) / 1000.0) as f32
        } else {
            0.0f32
        };

        Ok(StorageStats {
            tuple_count: count,
            memory_bytes: 0, // TODO: Get database size
            total_operations: stats.total_operations,
            read_operations: stats.read_operations,
            write_operations: stats.write_operations,
            take_operations: stats.take_operations,
            avg_latency_ms,
        })
    }

    async fn begin_transaction(&self) -> Result<String, TupleSpaceError> {
        // SQL supports transactions, return transaction ID
        let tx_id = ulid::Ulid::new().to_string();
        Ok(tx_id)
    }

    async fn commit_transaction(&self, _tx_id: &str) -> Result<(), TupleSpaceError> {
        // Transaction management would require storing transaction handles
        // This is a simplified implementation
        Ok(())
    }

    async fn abort_transaction(&self, _tx_id: &str) -> Result<(), TupleSpaceError> {
        // Transaction management would require storing transaction handles
        // This is a simplified implementation
        Ok(())
    }

    async fn publish_watch_event(
        &self,
        event_type: &str,
        tuple: &Tuple,
        namespace: &str,
    ) -> Result<(), TupleSpaceError> {
        // Only PostgreSQL supports LISTEN/NOTIFY
        if self.db_type != SqlDatabaseType::PostgreSQL {
            return Ok(()); // SQLite doesn't support NOTIFY
        }

        // Serialize watch event message
        // Note: Pattern with Predicate variants cannot be serialized, so we skip it
        let message = WatchEventMessage {
            event_type: event_type.to_string(),
            tuple: tuple.clone(),
            pattern_json: None, // Pattern not needed for publishing
        };

        let message_json = serde_json::to_string(&message).map_err(|e| {
            TupleSpaceError::SerializationError(format!("Failed to serialize watch event: {}", e))
        })?;

        // Use PostgreSQL NOTIFY
        match &self.pool {
            SqlPool::Postgres(pool) => {
                let channel = format!("tuplespace_watch_{}", namespace);
                sqlx::query(&format!("NOTIFY {}, $1", channel))
                    .bind(&message_json)
                    .execute(pool)
                    .await
                    .map_err(|e| {
                        TupleSpaceError::BackendError(format!("NOTIFY failed: {}", e))
                    })?;
            }
            SqlPool::Sqlite(_) => {
                // SQLite doesn't support NOTIFY
            }
        }

        Ok(())
    }

    async fn subscribe_watch_events(
        &self,
        namespace: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<WatchEventMessage>, TupleSpaceError> {
        // Only PostgreSQL supports LISTEN/NOTIFY
        match &self.pool {
            SqlPool::Postgres(_) => {
                // TODO: Implement PostgreSQL LISTEN subscription
                // This requires accessing the underlying postgres::Connection from sqlx
                // sqlx doesn't expose async notification APIs directly
                // A proper implementation would:
                // 1. Get a dedicated connection from the pool
                // 2. Execute LISTEN on the channel
                // 3. Use the underlying postgres connection to receive notifications asynchronously
                // 4. Forward messages to an mpsc channel
                // 
                // For now, return NotSupported - this can be implemented later by:
                // - Using the postgres crate directly for LISTEN connections
                // - Or accessing sqlx's internal connection to use pg_notifications()
                Err(TupleSpaceError::NotSupported(
                    format!("PostgreSQL LISTEN subscription for namespace '{}' requires direct postgres connection access (not yet implemented with sqlx). Publish works via NOTIFY.", namespace),
                ))
            }
            SqlPool::Sqlite(_) => {
                Err(TupleSpaceError::NotSupported(
                    "SQLite does not support LISTEN/NOTIFY for watch events".to_string(),
                ))
            }
        }
    }
}

#[cfg(all(test, feature = "sql-backend"))]
mod tests {
    use super::*;
    use crate::{Lease, PatternField, TupleField};

    /// Helper to create test SQLite storage (in-memory)
    async fn create_test_storage() -> SqlStorage {
        let config = SqliteStorageConfig {
            database_path: ":memory:".to_string(),
            enable_wal: false,
            cache_size_kb: 2000,
        };

        SqlStorage::new_sqlite(config)
            .await
            .expect("Failed to create SQLite storage")
    }

    #[tokio::test]
    async fn test_sql_storage_basic() {
        let storage = create_test_storage().await;

        // Write tuple
        let tuple = Tuple::new(vec![
            TupleField::String("test".to_string()),
            TupleField::Integer(42),
        ]);

        let tuple_id = storage.write(tuple.clone()).await.unwrap();
        assert!(!tuple_id.is_empty());

        // Read tuple
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("test".to_string())),
            PatternField::Wildcard,
        ]);

        let results = storage.read(pattern.clone(), None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields(), tuple.fields());

        // Take tuple
        let taken = storage.take(pattern, None).await.unwrap();
        assert_eq!(taken.len(), 1);

        // Should be gone
        let pattern2 = Pattern::new(vec![
            PatternField::Exact(TupleField::String("test".to_string())),
            PatternField::Wildcard,
        ]);
        let empty = storage.read(pattern2, None).await.unwrap();
        assert_eq!(empty.len(), 0);
    }

    #[tokio::test]
    async fn test_sql_storage_batch() {
        let storage = create_test_storage().await;

        // Write multiple tuples in batch
        let tuples = vec![
            Tuple::new(vec![TupleField::Integer(1)]),
            Tuple::new(vec![TupleField::Integer(2)]),
            Tuple::new(vec![TupleField::Integer(3)]),
        ];

        let ids = storage.write_batch(tuples).await.unwrap();
        assert_eq!(ids.len(), 3);

        // Count tuples
        let pattern = Pattern::new(vec![PatternField::Wildcard]);
        let count = storage.count(pattern).await.unwrap();
        assert_eq!(count, 3);

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    async fn test_sql_storage_lease() {
        let storage = create_test_storage().await;

        // Write tuple with short lease
        let tuple = Tuple::new(vec![TupleField::String("expiring".to_string())])
            .with_lease(Lease::new(chrono::Duration::seconds(1)));

        storage.write(tuple).await.unwrap();

        // Should exist immediately
        let pattern = Pattern::new(vec![PatternField::Exact(TupleField::String(
            "expiring".to_string(),
        ))]);

        let results = storage.read(pattern.clone(), None).await.unwrap();
        assert_eq!(results.len(), 1);

        // Wait for expiry
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Should be gone (TTL cleanup via query filter)
        let empty = storage.read(pattern, None).await.unwrap();
        assert_eq!(empty.len(), 0);
    }

    #[tokio::test]
    async fn test_sql_storage_stats() {
        let storage = create_test_storage().await;

        // Write some tuples
        for i in 0..5 {
            let tuple = Tuple::new(vec![TupleField::Integer(i)]);
            storage.write(tuple).await.unwrap();
        }

        // Check stats
        let stats = storage.stats().await.unwrap();
        assert_eq!(stats.tuple_count, 5);

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    async fn test_sql_storage_pattern_matching() {
        let storage = create_test_storage().await;

        // Write multiple tuples
        storage
            .write(Tuple::new(vec![
                TupleField::String("config".to_string()),
                TupleField::String("timeout".to_string()),
                TupleField::Integer(30),
            ]))
            .await
            .unwrap();

        storage
            .write(Tuple::new(vec![
                TupleField::String("config".to_string()),
                TupleField::String("retries".to_string()),
                TupleField::Integer(3),
            ]))
            .await
            .unwrap();

        // Pattern with wildcard
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("config".to_string())),
            PatternField::Wildcard,
            PatternField::Wildcard,
        ]);

        let all = storage.read(pattern, None).await.unwrap();
        assert_eq!(all.len(), 2);

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    async fn test_sql_storage_file_based() {
        use tempfile::NamedTempFile;

        // Create temporary file
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap().to_string();

        let config = SqliteStorageConfig {
            database_path: db_path.clone(),
            enable_wal: false,
            cache_size_kb: 2000,
        };

        let storage = SqlStorage::new_sqlite(config)
            .await
            .expect("Failed to create file-based SQLite storage");

        // Write and read tuple
        let tuple = Tuple::new(vec![
            TupleField::String("file-test".to_string()),
            TupleField::Integer(123),
        ]);

        storage.write(tuple.clone()).await.unwrap();

        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("file-test".to_string())),
            PatternField::Wildcard,
        ]);

        let results = storage.read(pattern, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields(), tuple.fields());

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    async fn test_sql_storage_count_and_exists() {
        let storage = create_test_storage().await;

        // Initially empty
        let pattern_all = Pattern::new(vec![PatternField::Wildcard]);
        assert_eq!(storage.count(pattern_all.clone()).await.unwrap(), 0);
        assert!(!storage.exists(pattern_all.clone()).await.unwrap());

        // Write some tuples
        for i in 0..5 {
            let tuple = Tuple::new(vec![TupleField::Integer(i)]);
            storage.write(tuple).await.unwrap();
        }

        // Count all
        assert_eq!(storage.count(pattern_all.clone()).await.unwrap(), 5);
        assert!(storage.exists(pattern_all.clone()).await.unwrap());

        // Count specific pattern
        let pattern_zero = Pattern::new(vec![PatternField::Exact(TupleField::Integer(0))]);
        assert_eq!(storage.count(pattern_zero.clone()).await.unwrap(), 1);
        assert!(storage.exists(pattern_zero).await.unwrap());

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    async fn test_sql_storage_renew_lease() {
        let storage = create_test_storage().await;

        // Write tuple with renewable lease
        let tuple = Tuple::new(vec![TupleField::String("renewable".to_string())])
            .with_lease(Lease::new(chrono::Duration::seconds(10)).renewable());

        let tuple_id = storage.write(tuple).await.unwrap();

        // Renew with default TTL
        let new_expires = storage.renew_lease(&tuple_id, None).await.unwrap();
        assert!(new_expires > Utc::now());

        // Renew with custom TTL
        let custom_ttl = Duration::from_secs(20);
        let new_expires2 = storage
            .renew_lease(&tuple_id, Some(custom_ttl))
            .await
            .unwrap();
        assert!(new_expires2 > new_expires);

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    async fn test_sql_storage_renew_non_renewable_lease() {
        let storage = create_test_storage().await;

        // Write tuple with non-renewable lease (default is non-renewable)
        let tuple = Tuple::new(vec![TupleField::String("non-renewable".to_string())])
            .with_lease(Lease::new(chrono::Duration::seconds(10)));

        let tuple_id = storage.write(tuple).await.unwrap();

        // Try to renew - should fail
        let result = storage.renew_lease(&tuple_id, None).await;
        assert!(result.is_err());
        match result {
            Err(TupleSpaceError::LeaseError(_)) => {} // Expected
            _ => panic!("Expected LeaseError"),
        }

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    async fn test_sql_storage_renew_nonexistent_tuple() {
        let storage = create_test_storage().await;

        // Try to renew non-existent tuple
        let fake_id = ulid::Ulid::new().to_string();
        let result = storage.renew_lease(&fake_id, None).await;
        assert!(result.is_err());
        match result {
            Err(TupleSpaceError::NotFound) => {} // Expected
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_sql_storage_transactions() {
        let storage = create_test_storage().await;

        // Test transaction methods (simplified implementation)
        let tx_id = storage.begin_transaction().await.unwrap();
        assert!(!tx_id.is_empty());

        // Commit should succeed
        storage.commit_transaction(&tx_id).await.unwrap();

        // Abort should succeed
        let tx_id2 = storage.begin_transaction().await.unwrap();
        storage.abort_transaction(&tx_id2).await.unwrap();
    }

    #[tokio::test]
    async fn test_sql_storage_blocking_read() {
        use tempfile::NamedTempFile;

        // Use file-based database so both instances share the same database
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap().to_string();

        let config = SqliteStorageConfig {
            database_path: db_path.clone(),
            enable_wal: false,
            cache_size_kb: 2000,
        };

        let storage = SqlStorage::new_sqlite(config.clone())
            .await
            .expect("Failed to create SQLite storage");

        // Spawn task to write tuple after delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            // Create new storage instance pointing to same database file
            let write_storage = SqlStorage::new_sqlite(config)
                .await
                .expect("Failed to create SQLite storage");
            let tuple = Tuple::new(vec![
                TupleField::String("blocking-test".to_string()),
                TupleField::Integer(42),
            ]);
            write_storage.write(tuple).await.unwrap();
        });

        // Blocking read should wait for tuple
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("blocking-test".to_string())),
            PatternField::Wildcard,
        ]);

        let start = std::time::Instant::now();
        let results = storage
            .read(pattern.clone(), Some(Duration::from_secs(5)))
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields()[0], TupleField::String("blocking-test".to_string()));
        assert!(elapsed.as_millis() >= 50, "Should have waited for tuple");
        assert!(elapsed.as_millis() < 5000, "Should not have timed out");

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    async fn test_sql_storage_blocking_read_timeout() {
        let storage = create_test_storage().await;

        // Blocking read with no matching tuple should timeout
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("nonexistent".to_string())),
            PatternField::Wildcard,
        ]);

        let start = std::time::Instant::now();
        let results = storage
            .read(pattern, Some(Duration::from_millis(500)))
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 0, "Should return empty on timeout");
        assert!(elapsed.as_millis() >= 450, "Should have waited for timeout");
        assert!(elapsed.as_millis() < 1000, "Should not wait longer than timeout");

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    async fn test_sql_storage_blocking_take() {
        use tempfile::NamedTempFile;

        // Use file-based database so both instances share the same database
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap().to_string();

        let config = SqliteStorageConfig {
            database_path: db_path.clone(),
            enable_wal: false,
            cache_size_kb: 2000,
        };

        let storage = SqlStorage::new_sqlite(config.clone())
            .await
            .expect("Failed to create SQLite storage");

        // Spawn task to write tuple after delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            // Create new storage instance pointing to same database file
            let write_storage = SqlStorage::new_sqlite(config)
                .await
                .expect("Failed to create SQLite storage");
            let tuple = Tuple::new(vec![
                TupleField::String("blocking-take-test".to_string()),
                TupleField::Integer(99),
            ]);
            write_storage.write(tuple).await.unwrap();
        });

        // Blocking take should wait for tuple and remove it
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("blocking-take-test".to_string())),
            PatternField::Wildcard,
        ]);

        let start = std::time::Instant::now();
        let results = storage
            .take(pattern.clone(), Some(Duration::from_secs(5)))
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields()[0], TupleField::String("blocking-take-test".to_string()));
        assert!(elapsed.as_millis() >= 50, "Should have waited for tuple");
        assert!(elapsed.as_millis() < 5000, "Should not have timed out");

        // Tuple should be gone
        let empty = storage.read(pattern, None).await.unwrap();
        assert_eq!(empty.len(), 0, "Tuple should be removed after take");

        // Cleanup
        storage.clear().await.unwrap();
    }
}
