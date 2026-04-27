// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Unified database migrations for PlexSpaces.
//
// Run all schema migrations once when connecting to a database (SQLite or PostgreSQL).
// Migrations live under db/migrations/sqlite and db/migrations/postgres; the correct set
// is chosen based on the connection string.

use std::fmt;

/// Error returned when migrations fail.
#[derive(Debug)]
pub struct MigrationError(pub String);

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MigrationError {}

/// Returns true if the connection string refers to PostgreSQL.
fn is_postgres(connection_string: &str) -> bool {
    let s = connection_string.trim();
    s.starts_with("postgresql://") || s.starts_with("postgres://")
}

/// Normalizes a SQLite path or URL into a form sqlx can connect to.
fn normalize_sqlite_url(connection_string: &str) -> String {
    let s = connection_string.trim();
    if s.is_empty() {
        return "sqlite::memory:".to_string();
    }
    if s == ":memory:" || s.eq_ignore_ascii_case("sqlite::memory:") {
        return "sqlite::memory:".to_string();
    }
    if s.starts_with("sqlite://") || s.starts_with("sqlite:") {
        return s.to_string();
    }
    // Plain path: add scheme. Absolute path -> sqlite:///path
    if s.starts_with('/') {
        return format!("sqlite://{}?mode=rwc", s);
    }
    format!("sqlite:{}?mode=rwc", s)
}

/// Runs all pending migrations for the database identified by `connection_string`.
///
/// - **PostgreSQL**: use a URL starting with `postgres://` or `postgresql://`.
/// - **SQLite**: use a path (e.g. `/path/to/db.sqlite`), `:memory:`, or `sqlite:///path`.
///
/// The correct migration set (db/migrations/sqlite or db/migrations/postgres) is chosen
/// automatically. Call this once at application startup before creating any store that
/// uses the same database.
pub async fn run_migrations(connection_string: &str) -> Result<(), MigrationError> {
    if is_postgres(connection_string) {
        run_postgres_migrations(connection_string).await
    } else {
        let url = normalize_sqlite_url(connection_string);
        run_sqlite_migrations(&url).await
    }
}

/// Runs SQLite migrations. Use `run_migrations()` to select SQLite vs PostgreSQL automatically.
pub async fn run_sqlite_migrations(connection_string: &str) -> Result<(), MigrationError> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(connection_string)
        .await
        .map_err(|e| MigrationError(format!("SQLite connection failed: {}", e)))?;

    let migrator = sqlx::migrate!("./migrations/sqlite");
    migrator.run(&pool).await.map_err(|e| {
        MigrationError(format!(
            "SQLite migration failed: {}. Ensure db/migrations/sqlite exists.",
            e
        ))
    })?;

    pool.close().await;
    tracing::info!("SQLite migrations completed");
    Ok(())
}

/// Runs PostgreSQL migrations. Use `run_migrations()` to select SQLite vs PostgreSQL automatically.
pub async fn run_postgres_migrations(connection_string: &str) -> Result<(), MigrationError> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(connection_string)
        .await
        .map_err(|e| MigrationError(format!("PostgreSQL connection failed: {}", e)))?;

    let migrator = sqlx::migrate!("./migrations/postgres");
    migrator.run(&pool).await.map_err(|e| {
        MigrationError(format!(
            "PostgreSQL migration failed: {}. Ensure db/migrations/postgres exists.",
            e
        ))
    })?;

    pool.close().await;
    tracing::info!("PostgreSQL migrations completed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_is_postgres() {
        assert!(is_postgres("postgres://localhost/db"));
        assert!(is_postgres("postgresql://user:pass@host/db"));
        assert!(!is_postgres("sqlite:///tmp/db.sqlite"));
        assert!(!is_postgres("/tmp/db.sqlite"));
        assert!(!is_postgres(":memory:"));
    }

    #[test]
    fn test_normalize_sqlite_url() {
        assert_eq!(normalize_sqlite_url(":memory:"), "sqlite::memory:");
        assert_eq!(
            normalize_sqlite_url("/tmp/db.sqlite"),
            "sqlite:///tmp/db.sqlite?mode=rwc"
        );
        assert_eq!(
            normalize_sqlite_url("sqlite:///path?mode=rwc"),
            "sqlite:///path?mode=rwc"
        );
    }

    #[tokio::test]
    async fn test_sqlite_migrations_create_all_expected_tables() {
        let temp_file = NamedTempFile::new().unwrap();
        let db_url = format!("sqlite://{}?mode=rwc", temp_file.path().display());

        run_sqlite_migrations(&db_url).await.unwrap();

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .unwrap();

        let expected_tables = [
            "kv_store",
            "object_registrations",
            "locks",
            "journal_entries",
            "checkpoints",
            "actor_events",
            "reminders",
            "scheduling_requests",
            "channel_messages",
            "blob_metadata",
            "workflow_definitions",
            "workflow_executions",
            "workflow_execution_labels",
            "step_executions",
            "signals",
            "tuples",
            "barriers",
            "watchers",
        ];

        for table_name in expected_tables {
            let row = sqlx::query(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
            )
            .bind(table_name)
            .fetch_optional(&pool)
            .await
            .unwrap();

            assert!(
                row.is_some(),
                "expected unified SQLite migrations to create table '{}'",
                table_name
            );
        }

        let tuples_index = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name = 'idx_expires_at' LIMIT 1",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(
            tuples_index.is_some(),
            "expected tuples expiry index to exist"
        );

        let watcher_index = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name = 'idx_watchers_pattern' LIMIT 1",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(
            watcher_index.is_some(),
            "expected watcher pattern index to exist"
        );

        pool.close().await;
    }
}
