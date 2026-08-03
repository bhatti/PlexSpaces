// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! SQLite-backed idempotency store.
//!
//! # Purpose
//! Durable backend that survives node restarts. Uses the node's shared SQLite
//! connection pool when no custom `connection_string` is provided.
//!
//! # Schema
//! Single table `idempotency_entries` with a PRIMARY KEY on
//! `(tenant_id, namespace, idempotency_key)`. Atomicity of check-and-record
//! is achieved with `INSERT OR IGNORE` (rows_affected == 1 → first-seen) plus
//! a SELECT on conflict (rows_affected == 0 → duplicate/in-flight). This
//! avoids any application-level locking and is safe under concurrent SQLite
//! connections using WAL mode.
//!
//! # Feature gate
//! Only compiled when `sqlite-backend` feature is enabled.

use async_trait::async_trait;
use bytes::Bytes;
use sqlx::{Pool, Sqlite, SqlitePool};
use plexspaces_service_traits::{IdempotencyError, IdempotencyOutcome, IdempotencyResult, IdempotencyStore};
use std::time::Duration;

/// SQLite-backed idempotency store.
pub struct SqliteIdempotencyStore {
    pool: Pool<Sqlite>,
    ttl: Duration,
}

impl SqliteIdempotencyStore {
    /// Open (or create) the SQLite database at `path` and run schema migration.
    ///
    /// Use `":memory:"` for tests.
    pub async fn new(path: &str, ttl: Duration) -> IdempotencyResult<Self> {
        let connection_string = if path == ":memory:" {
            "sqlite::memory:".to_string()
        } else if path.starts_with("sqlite:") {
            if path.contains("mode=") {
                path.to_string()
            } else if path.contains('?') {
                format!("{}&mode=rwc", path)
            } else {
                format!("{}?mode=rwc", path)
            }
        } else {
            format!("sqlite://{}?mode=rwc", path)
        };

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&connection_string)
            .await
            .map_err(|e| IdempotencyError::Storage(e.to_string()))?;

        // Production SQLite PRAGMAs (ref: micrologics.org/blog/sqlite-in-production-*)
        for pragma in &[
            "PRAGMA journal_mode=WAL",
            "PRAGMA synchronous=NORMAL",
            "PRAGMA busy_timeout=500",
            "PRAGMA cache_size=-64000",
            "PRAGMA mmap_size=1073741824",
            "PRAGMA journal_size_limit=67108864",
            // Disable auto-checkpoint to prevent WAL stalls under high write throughput.
            "PRAGMA wal_autocheckpoint=0",
        ] {
            sqlx::query(pragma).execute(&pool).await
                .map_err(|e| IdempotencyError::Storage(e.to_string()))?;
        }

        Self::run_schema(&pool).await?;

        Ok(Self { pool, ttl })
    }

    async fn run_schema(pool: &Pool<Sqlite>) -> IdempotencyResult<()> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS idempotency_entries (
                tenant_id       TEXT NOT NULL,
                namespace       TEXT NOT NULL,
                idempotency_key TEXT NOT NULL,
                state           TEXT NOT NULL DEFAULT 'in_flight',
                response        BLOB,
                created_at      INTEGER NOT NULL,
                expires_at      INTEGER NOT NULL,
                PRIMARY KEY (tenant_id, namespace, idempotency_key)
            )"#,
        )
        .execute(pool)
        .await
        .map_err(|e| IdempotencyError::Storage(e.to_string()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_idempotency_expires ON idempotency_entries(expires_at)",
        )
        .execute(pool)
        .await
        .map_err(|e| IdempotencyError::Storage(e.to_string()))?;

        Ok(())
    }

    fn now_unix_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }
}

#[async_trait]
impl IdempotencyStore for SqliteIdempotencyStore {
    /// Atomically check-and-record using `INSERT OR IGNORE` + `rows_affected`.
    ///
    /// - `rows_affected == 1` → we inserted → `FirstSeen`
    /// - `rows_affected == 0` → row existed; SELECT to determine current state
    ///   - state == 'complete' AND not expired → `Duplicate(response)`
    ///   - state == 'in_flight' OR expired → refresh if expired, else `InFlight`
    async fn check_and_record(
        &self,
        tenant_id: &str,
        namespace: &str,
        key: &str,
    ) -> IdempotencyResult<IdempotencyOutcome> {
        let now = Self::now_unix_ms();
        let expires_at = now + self.ttl.as_millis() as i64;

        // Try to INSERT the new in-flight entry
        let result = sqlx::query(
            r#"INSERT OR IGNORE INTO idempotency_entries
               (tenant_id, namespace, idempotency_key, state, response, created_at, expires_at)
               VALUES (?, ?, ?, 'in_flight', NULL, ?, ?)"#,
        )
        .bind(tenant_id)
        .bind(namespace)
        .bind(key)
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| IdempotencyError::Storage(e.to_string()))?;

        if result.rows_affected() == 1 {
            // We inserted → first seen
            return Ok(IdempotencyOutcome::FirstSeen);
        }

        // Row already existed — read its state
        let row: Option<(String, Option<Vec<u8>>, i64)> = sqlx::query_as(
            r#"SELECT state, response, expires_at
               FROM idempotency_entries
               WHERE tenant_id = ? AND namespace = ? AND idempotency_key = ?"#,
        )
        .bind(tenant_id)
        .bind(namespace)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| IdempotencyError::Storage(e.to_string()))?;

        match row {
            None => Ok(IdempotencyOutcome::FirstSeen), // deleted in race window
            Some((state, response, exp)) => {
                if exp < now {
                    // Expired — try to atomically claim it with a conditional UPDATE.
                    // The WHERE clause includes `expires_at = ?` so only one concurrent
                    // caller wins; others see rows_affected == 0 and fall through to
                    // re-reading the now-current (non-expired) state.
                    let claim = sqlx::query(
                        r#"UPDATE idempotency_entries
                           SET state = 'complete', response = NULL, created_at = ?, expires_at = ?
                           WHERE tenant_id = ? AND namespace = ? AND idempotency_key = ?
                             AND expires_at = ?"#,
                    )
                    .bind(now)
                    .bind(expires_at)
                    .bind(tenant_id)
                    .bind(namespace)
                    .bind(key)
                    .bind(exp) // only update the row we observed
                    .execute(&self.pool)
                    .await
                    .map_err(|e| IdempotencyError::Storage(e.to_string()))?;

                    if claim.rows_affected() == 1 {
                        // We won the race — this is a first-seen re-use of an expired slot
                        return Ok(IdempotencyOutcome::FirstSeen);
                    }
                    // Another caller won; re-read to get the current state
                    let row2: Option<(String, Option<Vec<u8>>)> = sqlx::query_as(
                        r#"SELECT state, response
                           FROM idempotency_entries
                           WHERE tenant_id = ? AND namespace = ? AND idempotency_key = ?"#,
                    )
                    .bind(tenant_id)
                    .bind(namespace)
                    .bind(key)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| IdempotencyError::Storage(e.to_string()))?;

                    return match row2 {
                        None => Ok(IdempotencyOutcome::FirstSeen),
                        Some((s, r)) => match s.as_str() {
                            "complete" => Ok(IdempotencyOutcome::Duplicate(r.map(Bytes::from))),
                            _ => Ok(IdempotencyOutcome::InFlight),
                        },
                    };
                }
                match state.as_str() {
                    "complete" => Ok(IdempotencyOutcome::Duplicate(response.map(Bytes::from))),
                    _ => Ok(IdempotencyOutcome::InFlight),
                }
            }
        }
    }

    async fn complete_record(
        &self,
        tenant_id: &str,
        namespace: &str,
        key: &str,
        response: Option<Bytes>,
    ) -> IdempotencyResult<()> {
        let now = Self::now_unix_ms();
        let expires_at = now + self.ttl.as_millis() as i64;
        let response_bytes: Option<Vec<u8>> = response.map(|b| b.to_vec());

        sqlx::query(
            r#"UPDATE idempotency_entries
               SET state = 'complete', response = ?, expires_at = ?
               WHERE tenant_id = ? AND namespace = ? AND idempotency_key = ?"#,
        )
        .bind(response_bytes)
        .bind(expires_at)
        .bind(tenant_id)
        .bind(namespace)
        .bind(key)
        .execute(&self.pool)
        .await
        .map_err(|e| IdempotencyError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn cleanup_expired(&self) -> IdempotencyResult<usize> {
        let now = Self::now_unix_ms();
        let result = sqlx::query("DELETE FROM idempotency_entries WHERE expires_at < ?")
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| IdempotencyError::Storage(e.to_string()))?;

        Ok(result.rows_affected() as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_service_traits::IdempotencyOutcome;

    async fn store() -> SqliteIdempotencyStore {
        SqliteIdempotencyStore::new(":memory:", Duration::from_secs(60))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn first_call_returns_first_seen() {
        let s = store().await;
        let outcome = s.check_and_record("t1", "ns", "key1").await.unwrap();
        assert!(
            matches!(outcome, IdempotencyOutcome::FirstSeen),
            "First call should be FirstSeen"
        );
    }

    #[tokio::test]
    async fn in_flight_before_complete() {
        let s = store().await;
        s.check_and_record("t1", "ns", "key1").await.unwrap();
        let outcome = s.check_and_record("t1", "ns", "key1").await.unwrap();
        assert!(
            matches!(outcome, IdempotencyOutcome::InFlight),
            "Second call before complete should be InFlight"
        );
    }

    #[tokio::test]
    async fn duplicate_after_complete() {
        let s = store().await;
        s.check_and_record("t1", "ns", "key1").await.unwrap();
        let payload = Bytes::from_static(b"world");
        s.complete_record("t1", "ns", "key1", Some(payload.clone()))
            .await
            .unwrap();
        let outcome = s.check_and_record("t1", "ns", "key1").await.unwrap();
        match outcome {
            IdempotencyOutcome::Duplicate(Some(r)) => assert_eq!(r, payload),
            other => panic!("expected Duplicate, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn duplicate_without_response() {
        let s = store().await;
        s.check_and_record("t1", "ns", "key1").await.unwrap();
        s.complete_record("t1", "ns", "key1", None).await.unwrap();
        let outcome = s.check_and_record("t1", "ns", "key1").await.unwrap();
        assert!(matches!(outcome, IdempotencyOutcome::Duplicate(None)));
    }

    #[tokio::test]
    async fn tenant_isolation() {
        let s = store().await;
        s.check_and_record("ta", "ns", "k").await.unwrap();
        s.complete_record("ta", "ns", "k", None).await.unwrap();
        let outcome = s.check_and_record("tb", "ns", "k").await.unwrap();
        assert!(
            matches!(outcome, IdempotencyOutcome::FirstSeen),
            "Different tenant should be independent"
        );
    }

    #[tokio::test]
    async fn namespace_isolation() {
        let s = store().await;
        s.check_and_record("t", "ns-a", "k").await.unwrap();
        s.complete_record("t", "ns-a", "k", None).await.unwrap();
        let outcome = s.check_and_record("t", "ns-b", "k").await.unwrap();
        assert!(
            matches!(outcome, IdempotencyOutcome::FirstSeen),
            "Different namespace should be independent"
        );
    }

    #[tokio::test]
    async fn cleanup_expired_deletes_old_entries() {
        let s = SqliteIdempotencyStore::new(":memory:", Duration::from_millis(50))
            .await
            .unwrap();
        s.check_and_record("t", "ns", "k").await.unwrap();
        s.complete_record("t", "ns", "k", None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let evicted = s.cleanup_expired().await.unwrap();
        assert!(evicted > 0, "Should evict expired entries");
    }

    #[tokio::test]
    async fn expired_entry_treated_as_new() {
        let s = SqliteIdempotencyStore::new(":memory:", Duration::from_millis(50))
            .await
            .unwrap();
        s.check_and_record("t", "ns", "k").await.unwrap();
        s.complete_record("t", "ns", "k", Some(Bytes::from_static(b"old")))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let outcome = s.check_and_record("t", "ns", "k").await.unwrap();
        assert!(
            matches!(outcome, IdempotencyOutcome::FirstSeen),
            "Expired entry should be treated as new: got {:?}", outcome
        );
    }
}
