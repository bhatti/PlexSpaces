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

//! Workflow storage layer
//!
//! ## Purpose
//! Manages workflow definitions and execution metadata in SQL database.
//! The actual workflow state is in the journal (via DurabilityFacet).
//!
//! ## Design
//! - Minimal implementation to pass tests (TDD - GREEN)
//! - Supports both SQLite (testing/embedded) and PostgreSQL (production)
//! - sqlx for type-safe queries
//! - Migrations via sqlx::migrate!
//! - Database-agnostic SQL queries where possible

use serde_json::Value;
use sqlx::{postgres::PgPool, sqlite::{SqlitePool, SqlitePoolOptions}, Row};
use std::collections::HashMap;

use crate::types::*;

/// Database type for workflow storage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseType {
    /// PostgreSQL database
    PostgreSQL,
    /// SQLite database
    SQLite,
}

/// SQL connection pool (PostgreSQL or SQLite)
#[derive(Clone)]
enum SqlPool {
    /// PostgreSQL connection pool
    Postgres(PgPool),
    /// SQLite connection pool
    Sqlite(SqlitePool),
}

/// Workflow storage for definitions and execution metadata
///
/// ## Purpose
/// Provides SQL persistence for workflow definitions and execution metadata.
/// The actual execution state is in the journal (hybrid storage architecture).
///
/// ## Database Support
/// - **SQLite**: For testing and embedded use cases
/// - **PostgreSQL**: For production multi-node deployments
#[derive(Clone)]
pub struct WorkflowStorage {
    pool: SqlPool,
    db_type: DatabaseType,
}

impl WorkflowStorage {
    /// Create in-memory storage for testing
    ///
    /// ## TDD Note
    /// Minimal implementation to pass tests
    pub async fn new_in_memory() -> Result<Self, WorkflowError> {
        Self::new_sqlite("sqlite::memory:").await
    }

    /// Create SQLite storage (file-based or in-memory)
    ///
    /// ## Arguments
    /// * `connection_string` - SQLite connection string (e.g., "sqlite://workflow.db" or "sqlite::memory:")
    ///
    /// ## Returns
    /// WorkflowStorage instance connected to SQLite database
    pub async fn new_sqlite(connection_string: &str) -> Result<Self, WorkflowError> {
        let conn_str = if connection_string.starts_with("sqlite:") {
            connection_string.to_string()
        } else {
            // Ensure parent directory exists for file-based
            if connection_string != ":memory:" && connection_string != "sqlite::memory:" {
                let path = std::path::Path::new(connection_string);
                // Ensure parent directory exists
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| WorkflowError::Storage(format!("Failed to create directory: {}", e)))?;
                    }
                }
            }
            // SQLite connection string format for sqlx: sqlite:path (works for both absolute and relative)
            // Use path as-is (sqlx handles both absolute and relative paths)
            if connection_string == ":memory:" {
                "sqlite::memory:".to_string()
            } else {
                // Convert to absolute path to ensure sqlx can find it
                let abs_path = if std::path::Path::new(connection_string).is_absolute() {
                    connection_string.to_string()
                } else {
                    std::env::current_dir()
                        .ok()
                        .and_then(|cwd| {
                            let full_path = cwd.join(connection_string);
                            // Try to canonicalize if file exists, otherwise use joined path
                            if full_path.exists() {
                                full_path.canonicalize().ok()
                            } else {
                                Some(full_path)
                            }
                        })
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| connection_string.to_string())
                };
                format!("sqlite:{}", abs_path)
            }
        };

        tracing::debug!("Connecting to SQLite with connection string: {}", conn_str);
        // Ensure parent directory exists for file-based databases
        if !conn_str.starts_with("sqlite::memory:") {
            if let Some(db_path) = conn_str.strip_prefix("sqlite:") {
                if let Some(parent) = std::path::Path::new(db_path).parent() {
                    if !parent.as_os_str().is_empty() && !parent.exists() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| WorkflowError::Storage(format!("Failed to create directory for database: {}", e)))?;
                    }
                }
                // Touch the file to ensure it exists (sqlx should create it, but this helps debug)
                if !std::path::Path::new(db_path).exists() {
                    std::fs::File::create(db_path)
                        .map_err(|e| WorkflowError::Storage(format!("Failed to create database file {}: {}", db_path, e)))?;
                }
            }
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&conn_str)
            .await
            .map_err(|e| WorkflowError::Storage(format!("Failed to connect to SQLite (conn_str: {}): {}", conn_str, e)))?;

        let storage = Self {
            pool: SqlPool::Sqlite(pool),
            db_type: DatabaseType::SQLite,
        };

        // Run migrations
        storage.run_migrations().await?;

        Ok(storage)
    }

    /// Create PostgreSQL storage
    ///
    /// ## Arguments
    /// * `connection_string` - PostgreSQL connection string (e.g., "postgresql://user:pass@localhost/dbname")
    ///
    /// ## Returns
    /// WorkflowStorage instance connected to PostgreSQL database
    pub async fn new_postgres(connection_string: &str) -> Result<Self, WorkflowError> {
        let pool = PgPool::connect(connection_string)
            .await
            .map_err(|e| WorkflowError::Storage(format!("Failed to connect to PostgreSQL: {}", e)))?;

        let storage = Self {
            pool: SqlPool::Postgres(pool),
            db_type: DatabaseType::PostgreSQL,
        };

        // Run migrations
        storage.run_migrations().await?;

        Ok(storage)
    }

    /// Run database migrations
    async fn run_migrations(&self) -> Result<(), WorkflowError> {
        match &self.pool {
            SqlPool::Sqlite(pool) => {
        sqlx::migrate!("./migrations")
                    .run(pool)
            .await
                    .map_err(|e| WorkflowError::Storage(format!("SQLite migration failed: {}", e)))?;
            }
            SqlPool::Postgres(pool) => {
                sqlx::migrate!("./migrations")
                    .run(pool)
                    .await
                    .map_err(|e| WorkflowError::Storage(format!("PostgreSQL migration failed: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Get database type
    pub fn database_type(&self) -> DatabaseType {
        self.db_type
    }

    /// Create file-based persistent storage (SQLite)
    ///
    /// ## Purpose
    /// Creates a SQLite database file that persists across restarts,
    /// enabling workflow recovery after node crashes or interruptions.
    ///
    /// ## Arguments
    /// * `path` - Path to SQLite database file (e.g., "workflow.db")
    ///
    /// ## Returns
    /// WorkflowStorage instance connected to the file-based database
    ///
    /// ## Errors
    /// WorkflowError if database connection or migration fails
    pub async fn new_file(path: &str) -> Result<Self, WorkflowError> {
        Self::new_sqlite(path).await
    }

    /// Save workflow definition
    pub async fn save_definition(&self, def: &WorkflowDefinition) -> Result<(), WorkflowError> {
        // Serialize definition to JSON (proto serialization can be added later)
        let definition_json =
            serde_json::to_string(def).map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        // Database-agnostic query (works for both SQLite and PostgreSQL)
        match &self.pool {
            SqlPool::Sqlite(pool) => {
        sqlx::query(
            r#"
            INSERT INTO workflow_definitions (id, version, name, definition_proto)
            VALUES (?, ?, ?, ?)
            ON CONFLICT (id, version) DO UPDATE SET
                name = excluded.name,
                definition_proto = excluded.definition_proto,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(&def.id)
        .bind(&def.version)
        .bind(&def.name)
        .bind(definition_json.as_bytes())
                .execute(pool)
        .await
        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
            }
            SqlPool::Postgres(pool) => {
                sqlx::query(
                    r#"
                    INSERT INTO workflow_definitions (id, version, name, definition_proto)
                    VALUES ($1, $2, $3, $4)
                    ON CONFLICT (id, version) DO UPDATE SET
                        name = EXCLUDED.name,
                        definition_proto = EXCLUDED.definition_proto,
                        updated_at = CURRENT_TIMESTAMP
                    "#,
                )
                .bind(&def.id)
                .bind(&def.version)
                .bind(&def.name)
                .bind(definition_json.as_bytes())
                .execute(pool)
                .await
                .map_err(|e| WorkflowError::Storage(e.to_string()))?;
            }
        }

        Ok(())
    }

    /// Get workflow definition
    pub async fn get_definition(
        &self,
        id: &str,
        version: &str,
    ) -> Result<WorkflowDefinition, WorkflowError> {
        let definition_bytes: Vec<u8> = match &self.pool {
            SqlPool::Sqlite(pool) => {
        let row = sqlx::query(
            r#"
            SELECT definition_proto FROM workflow_definitions
            WHERE id = ? AND version = ?
            "#,
        )
        .bind(id)
        .bind(version)
                .fetch_one(pool)
        .await
        .map_err(|e| {
            WorkflowError::NotFound(format!("Definition {}:{} not found: {}", id, version, e))
        })?;
                row.get(0)
            }
            SqlPool::Postgres(pool) => {
                let row = sqlx::query(
                    r#"
                    SELECT definition_proto FROM workflow_definitions
                    WHERE id = $1 AND version = $2
                    "#,
                )
                .bind(id)
                .bind(version)
                .fetch_one(pool)
                .await
                .map_err(|e| {
                    WorkflowError::NotFound(format!("Definition {}:{} not found: {}", id, version, e))
                })?;
                row.get(0)
            }
        };

        let definition: WorkflowDefinition = serde_json::from_slice(&definition_bytes)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        Ok(definition)
    }

    /// List all workflow definitions
    ///
    /// ## Arguments
    /// * `name_prefix` - Optional prefix filter for definition names
    ///
    /// ## Returns
    /// Vector of all matching workflow definitions
    pub async fn list_definitions(
        &self,
        name_prefix: Option<&str>,
    ) -> Result<Vec<WorkflowDefinition>, WorkflowError> {
        let mut definitions = Vec::new();

        match &self.pool {
            SqlPool::Sqlite(pool) => {
                let query = if let Some(prefix) = name_prefix {
                    sqlx::query(
                        r#"
                        SELECT definition_proto FROM workflow_definitions
                        WHERE name LIKE ?
                        ORDER BY id, version ASC
                        "#,
                    )
                    .bind(format!("{}%", prefix))
                } else {
                    sqlx::query(
                        r#"
                        SELECT definition_proto FROM workflow_definitions
                        ORDER BY id, version ASC
                        "#,
                    )
                };

                let rows = query
                    .fetch_all(pool)
                    .await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let definition_bytes: Vec<u8> = row.get(0);
                    let definition: WorkflowDefinition = serde_json::from_slice(&definition_bytes)
                        .map_err(|e| WorkflowError::Serialization(e.to_string()))?;
                    definitions.push(definition);
                }
            }
            SqlPool::Postgres(pool) => {
                let query = if let Some(prefix) = name_prefix {
                    sqlx::query(
                        r#"
                        SELECT definition_proto FROM workflow_definitions
                        WHERE name LIKE $1
                        ORDER BY id, version ASC
                        "#,
                    )
                    .bind(format!("{}%", prefix))
                } else {
                    sqlx::query(
                        r#"
                        SELECT definition_proto FROM workflow_definitions
                        ORDER BY id, version ASC
                        "#,
                    )
                };

                let rows = query
                    .fetch_all(pool)
                    .await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let definition_bytes: Vec<u8> = row.get(0);
                    let definition: WorkflowDefinition = serde_json::from_slice(&definition_bytes)
                        .map_err(|e| WorkflowError::Serialization(e.to_string()))?;
                    definitions.push(definition);
                }
            }
        }

        Ok(definitions)
    }

    /// Delete workflow definition
    ///
    /// ## Arguments
    /// * `id` - Definition ID
    /// * `version` - Version to delete (empty = delete all versions)
    ///
    /// ## Returns
    /// Ok if deleted successfully
    pub async fn delete_definition(
        &self,
        id: &str,
        version: &str,
    ) -> Result<(), WorkflowError> {
        match &self.pool {
            SqlPool::Sqlite(pool) => {
                if version.is_empty() {
                    // Delete all versions
                    sqlx::query("DELETE FROM workflow_definitions WHERE id = ?")
                        .bind(id)
                        .execute(pool)
                        .await
                        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
                } else {
                    // Delete specific version
                    sqlx::query("DELETE FROM workflow_definitions WHERE id = ? AND version = ?")
                        .bind(id)
                        .bind(version)
                        .execute(pool)
                        .await
                        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
                }
            }
            SqlPool::Postgres(pool) => {
                if version.is_empty() {
                    // Delete all versions
                    sqlx::query("DELETE FROM workflow_definitions WHERE id = $1")
                        .bind(id)
                        .execute(pool)
                        .await
                        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
                } else {
                    // Delete specific version
                    sqlx::query("DELETE FROM workflow_definitions WHERE id = $1 AND version = $2")
                        .bind(id)
                        .bind(version)
                        .execute(pool)
                        .await
                        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
                }
            }
        }

        Ok(())
    }

    /// Create workflow execution
    pub async fn create_execution(
        &self,
        definition_id: &str,
        definition_version: &str,
        input: Value,
        labels: HashMap<String, String>,
    ) -> Result<String, WorkflowError> {
        self.create_execution_with_node(definition_id, definition_version, input, labels, None)
            .await
    }

    /// Create workflow execution with node ownership
    pub async fn create_execution_with_node(
        &self,
        definition_id: &str,
        definition_version: &str,
        input: Value,
        labels: HashMap<String, String>,
        node_id: Option<&str>,
    ) -> Result<String, WorkflowError> {
        let execution_id = ulid::Ulid::new().to_string();
        let input_json = serde_json::to_string(&input)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        // Insert execution with version=1 and optional node_id
        match &self.pool {
            SqlPool::Sqlite(pool) => {
        sqlx::query(
            r#"
            INSERT INTO workflow_executions
                    (execution_id, definition_id, definition_version, status, input_json, node_id, version, last_heartbeat)
                    VALUES (?, ?, ?, ?, ?, ?, 1, CURRENT_TIMESTAMP)
            "#,
        )
        .bind(&execution_id)
        .bind(definition_id)
        .bind(definition_version)
        .bind("PENDING")
        .bind(&input_json)
                .bind(node_id)
                .execute(pool)
        .await
        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
            }
            SqlPool::Postgres(pool) => {
                sqlx::query(
                    r#"
                    INSERT INTO workflow_executions
                    (execution_id, definition_id, definition_version, status, input_json, node_id, version, last_heartbeat)
                    VALUES ($1, $2, $3, $4, $5, $6, 1, CURRENT_TIMESTAMP)
                    "#,
                )
                .bind(&execution_id)
                .bind(definition_id)
                .bind(definition_version)
                .bind("PENDING")
                .bind(&input_json)
                .bind(node_id)
                .execute(pool)
                .await
                .map_err(|e| WorkflowError::Storage(e.to_string()))?;
            }
        }

        // Insert labels
        for (key, value) in labels {
            match &self.pool {
                SqlPool::Sqlite(pool) => {
            sqlx::query(
                r#"
                INSERT INTO workflow_execution_labels (execution_id, label_key, label_value)
                VALUES (?, ?, ?)
                "#,
            )
            .bind(&execution_id)
            .bind(&key)
            .bind(&value)
                    .execute(pool)
            .await
            .map_err(|e| WorkflowError::Storage(e.to_string()))?;
                }
                SqlPool::Postgres(pool) => {
                    sqlx::query(
                        r#"
                        INSERT INTO workflow_execution_labels (execution_id, label_key, label_value)
                        VALUES ($1, $2, $3)
                        "#,
                    )
                    .bind(&execution_id)
                    .bind(&key)
                    .bind(&value)
                    .execute(pool)
                    .await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;
                }
            }
        }

        Ok(execution_id)
    }

    /// Get workflow execution
    pub async fn get_execution(
        &self,
        execution_id: &str,
    ) -> Result<WorkflowExecution, WorkflowError> {
        let (execution_id_val, definition_id, definition_version, status_str, current_step_id, input_json, output_json, error, node_id, version, last_heartbeat): (String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64, Option<chrono::DateTime<chrono::Utc>>) = match &self.pool {
            SqlPool::Sqlite(pool) => {
        let row = sqlx::query(
            r#"
            SELECT execution_id, definition_id, definition_version, status,
                   current_step_id, input_json, output_json, error,
                           node_id, version, last_heartbeat,
                           created_at, started_at, completed_at, updated_at
            FROM workflow_executions
            WHERE execution_id = ?
            "#,
        )
        .bind(execution_id)
                .fetch_one(pool)
        .await
        .map_err(|e| {
            WorkflowError::NotFound(format!("Execution {} not found: {}", execution_id, e))
        })?;
                (
                    row.get::<String, _>(0),
                    row.get::<String, _>(1),
                    row.get::<String, _>(2),
                    row.get::<String, _>(3),
                    row.get::<Option<String>, _>(4),
                    row.get::<Option<String>, _>(5),
                    row.get::<Option<String>, _>(6),
                    row.get::<Option<String>, _>(7),
                    row.get::<Option<String>, _>(8),
                    row.get::<i64, _>(9),
                    row.get::<Option<chrono::DateTime<chrono::Utc>>, _>(10),
                )
            }
            SqlPool::Postgres(pool) => {
                let row = sqlx::query(
                    r#"
                    SELECT execution_id, definition_id, definition_version, status,
                           current_step_id, input_json, output_json, error,
                           node_id, version, last_heartbeat,
                           created_at, started_at, completed_at, updated_at
                    FROM workflow_executions
                    WHERE execution_id = $1
                    "#,
                )
                .bind(execution_id)
                .fetch_one(pool)
                .await
                .map_err(|e| {
                    WorkflowError::NotFound(format!("Execution {} not found: {}", execution_id, e))
                })?;
                (
                    row.get::<String, _>(0),
                    row.get::<String, _>(1),
                    row.get::<String, _>(2),
                    row.get::<String, _>(3),
                    row.get::<Option<String>, _>(4),
                    row.get::<Option<String>, _>(5),
                    row.get::<Option<String>, _>(6),
                    row.get::<Option<String>, _>(7),
                    row.get::<Option<String>, _>(8),
                    row.get::<i64, _>(9),
                    row.get::<Option<chrono::DateTime<chrono::Utc>>, _>(10),
                )
            }
        };

        let status = ExecutionStatus::from_string(&status_str)?;

        Ok(WorkflowExecution {
            execution_id: execution_id_val,
            definition_id,
            definition_version,
            status,
            current_step_id,
            input: input_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
            output: output_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
            error,
            node_id,
            version: version as u64,
            last_heartbeat,
        })
    }

    /// Update execution status (with optimistic locking)
    pub async fn update_execution_status(
        &self,
        execution_id: &str,
        status: ExecutionStatus,
    ) -> Result<(), WorkflowError> {
        self.update_execution_status_with_version(execution_id, status, None).await
    }

    /// Update execution status with version check (optimistic locking)
    ///
    /// ## Purpose
    /// Updates execution status only if version matches (optimistic locking).
    /// Returns error if version mismatch (concurrent update detected).
    ///
    /// ## Arguments
    /// * `execution_id` - Execution ID
    /// * `status` - New status
    /// * `expected_version` - Expected version (None = no version check)
    ///
    /// ## Returns
    /// Ok if update succeeded, WorkflowError::ConcurrentUpdate if version mismatch
    pub async fn update_execution_status_with_version(
        &self,
        execution_id: &str,
        status: ExecutionStatus,
        expected_version: Option<u64>,
    ) -> Result<(), WorkflowError> {
        let status_str = status.to_string();

        let rows_affected = match &self.pool {
            SqlPool::Sqlite(pool) => {
                let result = if let Some(version) = expected_version {
                    // Version-based optimistic locking
        sqlx::query(
            r#"
            UPDATE workflow_executions
            SET status = ?,
                            version = version + 1,
                updated_at = CURRENT_TIMESTAMP,
                            last_heartbeat = CASE WHEN ? = 'RUNNING' THEN CURRENT_TIMESTAMP ELSE last_heartbeat END,
                            started_at = CASE WHEN started_at IS NULL AND ? = 'RUNNING' THEN CURRENT_TIMESTAMP ELSE started_at END,
                            completed_at = CASE WHEN ? IN ('COMPLETED', 'FAILED', 'CANCELLED', 'TIMED_OUT') THEN CURRENT_TIMESTAMP ELSE completed_at END
                        WHERE execution_id = ? AND version = ?
                        "#,
                    )
                    .bind(&status_str)
                    .bind(&status_str)
                    .bind(&status_str)
                    .bind(&status_str)
                    .bind(execution_id)
                    .bind(version as i64)
                    .execute(pool)
                    .await
                } else {
                    // No version check (backward compatibility)
                    sqlx::query(
                        r#"
                        UPDATE workflow_executions
                        SET status = ?,
                            version = version + 1,
                            updated_at = CURRENT_TIMESTAMP,
                            last_heartbeat = CASE WHEN ? = 'RUNNING' THEN CURRENT_TIMESTAMP ELSE last_heartbeat END,
                started_at = CASE WHEN started_at IS NULL AND ? = 'RUNNING' THEN CURRENT_TIMESTAMP ELSE started_at END,
                completed_at = CASE WHEN ? IN ('COMPLETED', 'FAILED', 'CANCELLED', 'TIMED_OUT') THEN CURRENT_TIMESTAMP ELSE completed_at END
            WHERE execution_id = ?
            "#,
        )
                    .bind(&status_str)
        .bind(&status_str)
        .bind(&status_str)
        .bind(&status_str)
        .bind(execution_id)
                    .execute(pool)
                    .await
                };
                result.map_err(|e| WorkflowError::Storage(e.to_string()))?.rows_affected()
            }
            SqlPool::Postgres(pool) => {
                let result = if let Some(version) = expected_version {
                    // Version-based optimistic locking (PostgreSQL)
                    sqlx::query(
                        r#"
                        UPDATE workflow_executions
                        SET status = $1,
                            version = version + 1,
                            updated_at = CURRENT_TIMESTAMP,
                            last_heartbeat = CASE WHEN $2 = 'RUNNING' THEN CURRENT_TIMESTAMP ELSE last_heartbeat END,
                            started_at = CASE WHEN started_at IS NULL AND $3 = 'RUNNING' THEN CURRENT_TIMESTAMP ELSE started_at END,
                            completed_at = CASE WHEN $4 IN ('COMPLETED', 'FAILED', 'CANCELLED', 'TIMED_OUT') THEN CURRENT_TIMESTAMP ELSE completed_at END
                        WHERE execution_id = $5 AND version = $6
                        "#,
                    )
                    .bind(&status_str)
                    .bind(&status_str)
                    .bind(&status_str)
                    .bind(&status_str)
                    .bind(execution_id)
                    .bind(version as i64)
                    .execute(pool)
                    .await
                } else {
                    // No version check (backward compatibility)
                    sqlx::query(
                        r#"
                        UPDATE workflow_executions
                        SET status = $1,
                            version = version + 1,
                            updated_at = CURRENT_TIMESTAMP,
                            last_heartbeat = CASE WHEN $2 = 'RUNNING' THEN CURRENT_TIMESTAMP ELSE last_heartbeat END,
                            started_at = CASE WHEN started_at IS NULL AND $3 = 'RUNNING' THEN CURRENT_TIMESTAMP ELSE started_at END,
                            completed_at = CASE WHEN $4 IN ('COMPLETED', 'FAILED', 'CANCELLED', 'TIMED_OUT') THEN CURRENT_TIMESTAMP ELSE completed_at END
                        WHERE execution_id = $5
                        "#,
                    )
                    .bind(&status_str)
                    .bind(&status_str)
                    .bind(&status_str)
                    .bind(&status_str)
                    .bind(execution_id)
                    .execute(pool)
                    .await
                };
                result.map_err(|e| WorkflowError::Storage(e.to_string()))?.rows_affected()
            }
        };

        // Check if update succeeded (for version-based updates)
        if expected_version.is_some() && rows_affected == 0 {
            return Err(WorkflowError::ConcurrentUpdate(format!(
                "Execution {} version mismatch (concurrent update detected)",
                execution_id
            )));
        }

        Ok(())
    }

    /// Transfer workflow ownership to a new node (with optimistic locking)
    ///
    /// ## Purpose
    /// Transfers workflow ownership from one node to another.
    /// Used for recovery when original node is dead or node-id changed.
    ///
    /// ## Arguments
    /// * `execution_id` - Execution ID
    /// * `new_node_id` - New node owner
    /// * `expected_version` - Expected version (for optimistic locking)
    ///
    /// ## Returns
    /// Ok if transfer succeeded, WorkflowError::ConcurrentUpdate if version mismatch
    pub async fn transfer_ownership(
        &self,
        execution_id: &str,
        new_node_id: &str,
        expected_version: u64,
    ) -> Result<(), WorkflowError> {
        let rows_affected = match &self.pool {
            SqlPool::Sqlite(pool) => {
                sqlx::query(
                    r#"
                    UPDATE workflow_executions
                    SET node_id = ?,
                        version = version + 1,
                        last_heartbeat = CURRENT_TIMESTAMP,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE execution_id = ? AND version = ?
                    "#,
                )
                .bind(new_node_id)
                .bind(execution_id)
                .bind(expected_version as i64)
                .execute(pool)
                .await
                .map_err(|e| WorkflowError::Storage(e.to_string()))?
                .rows_affected()
            }
            SqlPool::Postgres(pool) => {
                sqlx::query(
                    r#"
                    UPDATE workflow_executions
                    SET node_id = $1,
                        version = version + 1,
                        last_heartbeat = CURRENT_TIMESTAMP,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE execution_id = $2 AND version = $3
                    "#,
                )
                .bind(new_node_id)
                .bind(execution_id)
                .bind(expected_version as i64)
                .execute(pool)
                .await
                .map_err(|e| WorkflowError::Storage(e.to_string()))?
                .rows_affected()
            }
        };

        if rows_affected == 0 {
            return Err(WorkflowError::ConcurrentUpdate(format!(
                "Execution {} version mismatch (concurrent ownership transfer)",
                execution_id
            )));
        }

        Ok(())
    }

    /// Update heartbeat for a workflow execution
    ///
    /// ## Purpose
    /// Updates last_heartbeat timestamp to indicate node is alive.
    /// Used for health monitoring.
    pub async fn update_heartbeat(
        &self,
        execution_id: &str,
        node_id: &str,
    ) -> Result<(), WorkflowError> {
        match &self.pool {
            SqlPool::Sqlite(pool) => {
                sqlx::query(
                    r#"
                    UPDATE workflow_executions
                    SET last_heartbeat = CURRENT_TIMESTAMP,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE execution_id = ? AND node_id = ?
                    "#,
                )
                .bind(execution_id)
                .bind(node_id)
                .execute(pool)
        .await
        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
            }
            SqlPool::Postgres(pool) => {
                sqlx::query(
                    r#"
                    UPDATE workflow_executions
                    SET last_heartbeat = CURRENT_TIMESTAMP,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE execution_id = $1 AND node_id = $2
                    "#,
                )
                .bind(execution_id)
                .bind(node_id)
                .execute(pool)
                .await
                .map_err(|e| WorkflowError::Storage(e.to_string()))?;
            }
        }

        Ok(())
    }

    /// Update execution output (with optimistic locking)
    pub async fn update_execution_output(
        &self,
        execution_id: &str,
        output: Value,
    ) -> Result<(), WorkflowError> {
        self.update_execution_output_with_version(execution_id, output, None).await
    }

    /// Update execution output with version check (optimistic locking)
    pub async fn update_execution_output_with_version(
        &self,
        execution_id: &str,
        output: Value,
        expected_version: Option<u64>,
    ) -> Result<(), WorkflowError> {
        let output_json = serde_json::to_string(&output)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        let rows_affected = match &self.pool {
            SqlPool::Sqlite(pool) => {
                let result = if let Some(version) = expected_version {
        sqlx::query(
            r#"
            UPDATE workflow_executions
            SET output_json = ?,
                            version = version + 1,
                            updated_at = CURRENT_TIMESTAMP
                        WHERE execution_id = ? AND version = ?
                        "#,
                    )
                    .bind(&output_json)
                    .bind(execution_id)
                    .bind(version as i64)
                    .execute(pool)
                    .await
                } else {
                    sqlx::query(
                        r#"
                        UPDATE workflow_executions
                        SET output_json = ?,
                            version = version + 1,
                updated_at = CURRENT_TIMESTAMP
            WHERE execution_id = ?
            "#,
        )
        .bind(&output_json)
        .bind(execution_id)
                    .execute(pool)
        .await
                };
                result.map_err(|e| WorkflowError::Storage(e.to_string()))?.rows_affected()
            }
            SqlPool::Postgres(pool) => {
                let result = if let Some(version) = expected_version {
                    sqlx::query(
                        r#"
                        UPDATE workflow_executions
                        SET output_json = $1,
                            version = version + 1,
                            updated_at = CURRENT_TIMESTAMP
                        WHERE execution_id = $2 AND version = $3
                        "#,
                    )
                    .bind(&output_json)
                    .bind(execution_id)
                    .bind(version as i64)
                    .execute(pool)
                    .await
                } else {
                    sqlx::query(
                        r#"
                        UPDATE workflow_executions
                        SET output_json = $1,
                            version = version + 1,
                            updated_at = CURRENT_TIMESTAMP
                        WHERE execution_id = $2
                        "#,
                    )
                    .bind(&output_json)
                    .bind(execution_id)
                    .execute(pool)
                    .await
                };
                result.map_err(|e| WorkflowError::Storage(e.to_string()))?.rows_affected()
            }
        };

        if expected_version.is_some() && rows_affected == 0 {
            return Err(WorkflowError::ConcurrentUpdate(format!(
                "Execution {} version mismatch (concurrent update detected)",
                execution_id
            )));
        }

        Ok(())
    }

    /// Create step execution (attempt = 1)
    pub async fn create_step_execution(
        &self,
        execution_id: &str,
        step_id: &str,
        input: Value,
    ) -> Result<String, WorkflowError> {
        self.create_step_execution_with_attempt(execution_id, step_id, input, 1)
            .await
    }

    /// Create step execution with specific attempt number
    pub async fn create_step_execution_with_attempt(
        &self,
        execution_id: &str,
        step_id: &str,
        input: Value,
        attempt: u32,
    ) -> Result<String, WorkflowError> {
        let step_exec_id = ulid::Ulid::new().to_string();
        let input_json = serde_json::to_string(&input)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        match &self.pool {
            SqlPool::Sqlite(pool) => {
        sqlx::query(
            r#"
            INSERT INTO step_executions
            (step_execution_id, execution_id, step_id, status, input_json, attempt, started_at)
            VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
            "#,
        )
        .bind(&step_exec_id)
        .bind(execution_id)
        .bind(step_id)
        .bind("RUNNING")
        .bind(&input_json)
        .bind(attempt as i64)
                .execute(pool)
        .await
        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
            }
            SqlPool::Postgres(pool) => {
                sqlx::query(
                    r#"
                    INSERT INTO step_executions
                    (step_execution_id, execution_id, step_id, status, input_json, attempt, started_at)
                    VALUES ($1, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP)
                    "#,
                )
                .bind(&step_exec_id)
                .bind(execution_id)
                .bind(step_id)
                .bind("RUNNING")
                .bind(&input_json)
                .bind(attempt as i64)
                .execute(pool)
                .await
                .map_err(|e| WorkflowError::Storage(e.to_string()))?;
            }
        }

        Ok(step_exec_id)
    }

    /// Get step execution by ID
    pub async fn get_step_execution(
        &self,
        step_exec_id: &str,
    ) -> Result<StepExecution, WorkflowError> {
        let (step_execution_id, execution_id, step_id, status_str, input_json, output_json, error, attempt): (String, String, String, String, Option<String>, Option<String>, Option<String>, i64) = match &self.pool {
            SqlPool::Sqlite(pool) => {
        let row = sqlx::query(
            r#"
            SELECT step_execution_id, execution_id, step_id, status,
                   input_json, output_json, error, attempt
            FROM step_executions
            WHERE step_execution_id = ?
            "#,
        )
        .bind(step_exec_id)
                .fetch_one(pool)
        .await
        .map_err(|e| {
            WorkflowError::NotFound(format!("Step execution {} not found: {}", step_exec_id, e))
        })?;
                (
                    row.get::<String, _>(0),
                    row.get::<String, _>(1),
                    row.get::<String, _>(2),
                    row.get::<String, _>(3),
                    row.get::<Option<String>, _>(4),
                    row.get::<Option<String>, _>(5),
                    row.get::<Option<String>, _>(6),
                    row.get::<i64, _>(7),
                )
            }
            SqlPool::Postgres(pool) => {
                let row = sqlx::query(
                    r#"
                    SELECT step_execution_id, execution_id, step_id, status,
                           input_json, output_json, error, attempt
                    FROM step_executions
                    WHERE step_execution_id = $1
                    "#,
                )
                .bind(step_exec_id)
                .fetch_one(pool)
                .await
                .map_err(|e| {
                    WorkflowError::NotFound(format!("Step execution {} not found: {}", step_exec_id, e))
                })?;
                (
                    row.get::<String, _>(0),
                    row.get::<String, _>(1),
                    row.get::<String, _>(2),
                    row.get::<String, _>(3),
                    row.get::<Option<String>, _>(4),
                    row.get::<Option<String>, _>(5),
                    row.get::<Option<String>, _>(6),
                    row.get::<i64, _>(7),
                )
            }
        };

        let status = StepExecutionStatus::from_string(&status_str)?;

        Ok(StepExecution {
            step_execution_id,
            execution_id,
            step_id,
            status,
            input: input_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
            output: output_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
            error,
            attempt: attempt as u32,
        })
    }

    /// Complete step execution with status, output, and/or error
    pub async fn complete_step_execution(
        &self,
        step_exec_id: &str,
        status: StepExecutionStatus,
        output: Option<Value>,
        error: Option<String>,
    ) -> Result<(), WorkflowError> {
        let status_str = status.to_string();
        let output_json = output
            .map(|v| serde_json::to_string(&v))
            .transpose()
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        match &self.pool {
            SqlPool::Sqlite(pool) => {
        sqlx::query(
            r#"
            UPDATE step_executions
            SET status = ?,
                output_json = ?,
                error = ?,
                completed_at = CURRENT_TIMESTAMP
            WHERE step_execution_id = ?
            "#,
        )
        .bind(&status_str)
        .bind(output_json.as_ref())
        .bind(error.as_ref())
        .bind(step_exec_id)
                .execute(pool)
        .await
        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
            }
            SqlPool::Postgres(pool) => {
                sqlx::query(
                    r#"
                    UPDATE step_executions
                    SET status = $1,
                        output_json = $2,
                        error = $3,
                        completed_at = CURRENT_TIMESTAMP
                    WHERE step_execution_id = $4
                    "#,
                )
                .bind(&status_str)
                .bind(output_json.as_ref())
                .bind(error.as_ref())
                .bind(step_exec_id)
                .execute(pool)
                .await
                .map_err(|e| WorkflowError::Storage(e.to_string()))?;
            }
        }

        Ok(())
    }

    /// Get all step executions for a workflow execution
    pub async fn get_step_execution_history(
        &self,
        execution_id: &str,
    ) -> Result<Vec<StepExecution>, WorkflowError> {
        let mut executions = Vec::new();

        match &self.pool {
            SqlPool::Sqlite(pool) => {
        let rows = sqlx::query(
            r#"
            SELECT step_execution_id, execution_id, step_id, status,
                   input_json, output_json, error, attempt
            FROM step_executions
            WHERE execution_id = ?
            ORDER BY started_at ASC
            "#,
        )
        .bind(execution_id)
                .fetch_all(pool)
        .await
        .map_err(|e| WorkflowError::Storage(e.to_string()))?;

        for row in rows {
            let status_str: String = row.get(3);
            let status = StepExecutionStatus::from_string(&status_str)?;

            let input_json: Option<String> = row.get(4);
            let output_json: Option<String> = row.get(5);
            let attempt: i64 = row.get(7);

            executions.push(StepExecution {
                step_execution_id: row.get(0),
                execution_id: row.get(1),
                step_id: row.get(2),
                status,
                        input: input_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                error: row.get(6),
                attempt: attempt as u32,
            });
                }
            }
            SqlPool::Postgres(pool) => {
                let rows = sqlx::query(
                    r#"
                    SELECT step_execution_id, execution_id, step_id, status,
                           input_json, output_json, error, attempt
                    FROM step_executions
                    WHERE execution_id = $1
                    ORDER BY started_at ASC
                    "#,
                )
                .bind(execution_id)
                .fetch_all(pool)
                .await
                .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let status_str: String = row.get(3);
                    let status = StepExecutionStatus::from_string(&status_str)?;

                    let input_json: Option<String> = row.get(4);
                    let output_json: Option<String> = row.get(5);
                    let attempt: i64 = row.get(7);

                    executions.push(StepExecution {
                        step_execution_id: row.get(0),
                        execution_id: row.get(1),
                        step_id: row.get(2),
                        status,
                        input: input_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        error: row.get(6),
                        attempt: attempt as u32,
                    });
                }
            }
        }

        Ok(executions)
    }

    /// Send a signal to a workflow execution
    ///
    /// ## Purpose
    /// Stores a signal that can be consumed by a Signal step.
    /// Signals are used for external events (Awakeable pattern).
    ///
    /// ## Arguments
    /// * `execution_id` - The workflow execution ID
    /// * `signal_name` - Name of the signal (e.g., "approval", "payment")
    /// * `payload` - Signal data payload
    ///
    /// ## Returns
    /// Ok if signal was stored successfully
    ///
    /// ## Errors
    /// WorkflowError if database operation fails
    pub async fn send_signal(
        &self,
        execution_id: &str,
        signal_name: &str,
        payload: Value,
    ) -> Result<(), WorkflowError> {
        let signal_id = ulid::Ulid::new().to_string();
        let payload_json = serde_json::to_string(&payload)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        match &self.pool {
            SqlPool::Sqlite(pool) => {
        sqlx::query(
            "INSERT INTO signals (signal_id, execution_id, signal_name, payload, received_at)
                     VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)",
        )
                .bind(&signal_id)
        .bind(execution_id)
        .bind(signal_name)
                .bind(&payload_json)
                .execute(pool)
        .await
        .map_err(|e| WorkflowError::Storage(format!("Failed to store signal: {}", e)))?;
            }
            SqlPool::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO signals (signal_id, execution_id, signal_name, payload, received_at)
                     VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP)",
                )
                .bind(&signal_id)
                .bind(execution_id)
                .bind(signal_name)
                .bind(&payload_json)
                .execute(pool)
                .await
                .map_err(|e| WorkflowError::Storage(format!("Failed to store signal: {}", e)))?;
            }
        }

        Ok(())
    }

    /// Check if a signal has been received for an execution
    ///
    /// ## Purpose
    /// Polls for a signal by name. If found, removes it from storage (consume).
    ///
    /// ## Arguments
    /// * `execution_id` - The workflow execution ID
    /// * `signal_name` - Name of the signal to check for
    ///
    /// ## Returns
    /// Some(payload) if signal was received, None otherwise
    ///
    /// ## Errors
    /// WorkflowError if database operation fails
    pub async fn check_signal(
        &self,
        execution_id: &str,
        signal_name: &str,
    ) -> Result<Option<Value>, WorkflowError> {
        // Query for the signal
        let payload_json_opt: Option<String> = match &self.pool {
            SqlPool::Sqlite(pool) => {
                let row_opt = sqlx::query(
            "SELECT payload FROM signals
             WHERE execution_id = ? AND signal_name = ?
             ORDER BY received_at ASC
             LIMIT 1",
        )
        .bind(execution_id)
        .bind(signal_name)
                .fetch_optional(pool)
        .await
        .map_err(|e| WorkflowError::Storage(format!("Failed to check signal: {}", e)))?;
                row_opt.map(|row| row.get(0))
            }
            SqlPool::Postgres(pool) => {
                let row_opt = sqlx::query(
                    "SELECT payload FROM signals
                     WHERE execution_id = $1 AND signal_name = $2
                     ORDER BY received_at ASC
                     LIMIT 1",
                )
                .bind(execution_id)
                .bind(signal_name)
                .fetch_optional(pool)
                .await
                .map_err(|e| WorkflowError::Storage(format!("Failed to check signal: {}", e)))?;
                row_opt.map(|row| row.get(0))
            }
        };

        if let Some(payload_json) = payload_json_opt {
            let payload: Value = serde_json::from_str(&payload_json)
                .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

            // Delete the signal (consume it) - database-specific syntax
            match &self.pool {
                SqlPool::Sqlite(pool) => {
                    // SQLite uses rowid
            sqlx::query(
                "DELETE FROM signals
                 WHERE execution_id = ? AND signal_name = ?
                 AND rowid = (
                     SELECT rowid FROM signals
                     WHERE execution_id = ? AND signal_name = ?
                     ORDER BY received_at ASC
                     LIMIT 1
                 )",
            )
            .bind(execution_id)
            .bind(signal_name)
            .bind(execution_id)
            .bind(signal_name)
                    .execute(pool)
            .await
            .map_err(|e| WorkflowError::Storage(format!("Failed to delete signal: {}", e)))?;
                }
                SqlPool::Postgres(pool) => {
                    // PostgreSQL uses CTID or subquery
                    sqlx::query(
                        "DELETE FROM signals
                         WHERE ctid = (
                             SELECT ctid FROM signals
                             WHERE execution_id = $1 AND signal_name = $2
                             ORDER BY received_at ASC
                             LIMIT 1
                         )",
                    )
                    .bind(execution_id)
                    .bind(signal_name)
                    .execute(pool)
                    .await
                    .map_err(|e| WorkflowError::Storage(format!("Failed to delete signal: {}", e)))?;
                }
            }

            Ok(Some(payload))
        } else {
            Ok(None)
        }
    }

    /// List workflow executions by status
    ///
    /// ## Purpose
    /// Query for workflows in specific states (e.g., RUNNING, PENDING) for recovery.
    /// Used by auto-recovery service to find interrupted workflows.
    ///
    /// ## Arguments
    /// * `statuses` - List of execution statuses to filter by
    /// * `node_id` - Optional node ID filter (to find workflows owned by specific node)
    ///
    /// ## Returns
    /// Vector of workflow executions matching the criteria
    ///
    /// ## Errors
    /// WorkflowError if database query fails
    pub async fn list_executions_by_status(
        &self,
        statuses: Vec<ExecutionStatus>,
        node_id: Option<&str>,
    ) -> Result<Vec<WorkflowExecution>, WorkflowError> {
        let status_strings: Vec<String> = statuses.iter().map(|s| s.to_string()).collect();
        let mut executions = Vec::new();

        match &self.pool {
            SqlPool::Sqlite(pool) => {
                let status_placeholders: String = status_strings.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let mut query = format!(
                    r#"
                    SELECT execution_id, definition_id, definition_version, status,
                           current_step_id, input_json, output_json, error,
                           node_id, version, last_heartbeat,
                           created_at, started_at, completed_at, updated_at
                    FROM workflow_executions
                    WHERE status IN ({})
                    "#,
                    status_placeholders
                );

                if node_id.is_some() {
                    query.push_str(" AND node_id = ?");
                }

                query.push_str(" ORDER BY created_at ASC");

                let mut query_builder = sqlx::query(&query);

                // Bind status values
                for status_str in &status_strings {
                    query_builder = query_builder.bind(status_str);
                }

                // Bind node_id if provided
                if let Some(nid) = node_id {
                    query_builder = query_builder.bind(nid);
                }

                let rows = query_builder.fetch_all(pool).await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let status_str: String = row.get(3);
                    let status = ExecutionStatus::from_string(&status_str)?;

                    let input_json: Option<String> = row.get(5);
                    let output_json: Option<String> = row.get(6);
                    let version: i64 = row.get(9);
                    let last_heartbeat: Option<chrono::DateTime<chrono::Utc>> = row.get(10);

                    executions.push(WorkflowExecution {
                        execution_id: row.get(0),
                        definition_id: row.get(1),
                        definition_version: row.get(2),
                        status,
                        current_step_id: row.get(4),
                        input: input_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        error: row.get(7),
                        node_id: row.get(8),
                        version: version as u64,
                        last_heartbeat,
                    });
                }
            }
            SqlPool::Postgres(pool) => {
                let status_placeholders: String = (1..=status_strings.len())
                    .map(|i| format!("${}", i))
                    .collect::<Vec<_>>()
                    .join(",");
                let mut param_idx = status_strings.len() + 1;
                let mut query = format!(
                    r#"
                    SELECT execution_id, definition_id, definition_version, status,
                           current_step_id, input_json, output_json, error,
                           node_id, version, last_heartbeat,
                           created_at, started_at, completed_at, updated_at
                    FROM workflow_executions
                    WHERE status IN ({})
                    "#,
                    status_placeholders
                );

                if node_id.is_some() {
                    query.push_str(&format!(" AND node_id = ${}", param_idx));
                    param_idx += 1;
                }

                query.push_str(" ORDER BY created_at ASC");

                let mut query_builder = sqlx::query(&query);

                // Bind status values
                for status_str in &status_strings {
                    query_builder = query_builder.bind(status_str);
                }

                // Bind node_id if provided
                if let Some(nid) = node_id {
                    query_builder = query_builder.bind(nid);
                }

                let rows = query_builder.fetch_all(pool).await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let status_str: String = row.get(3);
                    let status = ExecutionStatus::from_string(&status_str)?;

                    let input_json: Option<String> = row.get(5);
                    let output_json: Option<String> = row.get(6);
                    let version: i64 = row.get(9);
                    let last_heartbeat: Option<chrono::DateTime<chrono::Utc>> = row.get(10);

                    executions.push(WorkflowExecution {
                        execution_id: row.get(0),
                        definition_id: row.get(1),
                        definition_version: row.get(2),
                        status,
                        current_step_id: row.get(4),
                        input: input_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        error: row.get(7),
                        node_id: row.get(8),
                        version: version as u64,
                        last_heartbeat,
                    });
                }
            }
        }

        Ok(executions)
    }

    /// List stale workflow executions (not updated recently)
    ///
    /// ## Purpose
    /// Find workflows that haven't been updated in a while, indicating
    /// the node may have crashed without updating status.
    ///
    /// ## Arguments
    /// * `stale_threshold_seconds` - Number of seconds since last update to consider stale
    /// * `statuses` - Statuses to check (typically RUNNING)
    ///
    /// ## Returns
    /// Vector of stale workflow executions
    ///
    /// ## Errors
    /// WorkflowError if database query fails
    pub async fn list_stale_executions(
        &self,
        stale_threshold_seconds: u64,
        statuses: Vec<ExecutionStatus>,
    ) -> Result<Vec<WorkflowExecution>, WorkflowError> {
        let status_strings: Vec<String> = statuses.iter().map(|s| s.to_string()).collect();
        let mut executions = Vec::new();

        // Database-specific date difference calculation
        match &self.pool {
            SqlPool::Sqlite(pool) => {
                let status_placeholders: String = status_strings.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let query = format!(
                    r#"
                    SELECT execution_id, definition_id, definition_version, status,
                           current_step_id, input_json, output_json, error,
                           node_id, version, last_heartbeat,
                           created_at, started_at, completed_at, updated_at
                    FROM workflow_executions
                    WHERE status IN ({})
                      AND (julianday('now') - julianday(COALESCE(last_heartbeat, updated_at))) * 86400 > ?
                    ORDER BY COALESCE(last_heartbeat, updated_at) ASC
                    "#,
                    status_placeholders
                );

                let mut query_builder = sqlx::query(&query);

                // Bind status values
                for status_str in &status_strings {
                    query_builder = query_builder.bind(status_str);
                }

                // Bind stale threshold
                query_builder = query_builder.bind(stale_threshold_seconds as i64);

                let rows = query_builder.fetch_all(pool).await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let status_str: String = row.get(3);
                    let status = ExecutionStatus::from_string(&status_str)?;

                    let input_json: Option<String> = row.get(5);
                    let output_json: Option<String> = row.get(6);
                    let version: i64 = row.get(9);
                    let last_heartbeat: Option<chrono::DateTime<chrono::Utc>> = row.get(10);

                    executions.push(WorkflowExecution {
                        execution_id: row.get(0),
                        definition_id: row.get(1),
                        definition_version: row.get(2),
                        status,
                        current_step_id: row.get(4),
                        input: input_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        error: row.get(7),
                        node_id: row.get(8),
                        version: version as u64,
                        last_heartbeat,
                    });
                }
            }
            SqlPool::Postgres(pool) => {
                let status_placeholders: String = (1..=status_strings.len())
                    .map(|i| format!("${}", i))
                    .collect::<Vec<_>>()
                    .join(",");
                let threshold_param = status_strings.len() + 1;
                let query = format!(
                    r#"
                    SELECT execution_id, definition_id, definition_version, status,
                           current_step_id, input_json, output_json, error,
                           node_id, version, last_heartbeat,
                           created_at, started_at, completed_at, updated_at
                    FROM workflow_executions
                    WHERE status IN ({})
                      AND EXTRACT(EPOCH FROM (NOW() - COALESCE(last_heartbeat, updated_at))) > ${}
                    ORDER BY COALESCE(last_heartbeat, updated_at) ASC
                    "#,
                    status_placeholders, threshold_param
                );

                let mut query_builder = sqlx::query(&query);

                // Bind status values
                for status_str in &status_strings {
                    query_builder = query_builder.bind(status_str);
                }

                // Bind stale threshold
                query_builder = query_builder.bind(stale_threshold_seconds as i64);

                let rows = query_builder.fetch_all(pool).await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let status_str: String = row.get(3);
                    let status = ExecutionStatus::from_string(&status_str)?;

                    let input_json: Option<String> = row.get(5);
                    let output_json: Option<String> = row.get(6);
                    let version: i64 = row.get(9);
                    let last_heartbeat: Option<chrono::DateTime<chrono::Utc>> = row.get(10);

                    executions.push(WorkflowExecution {
                        execution_id: row.get(0),
                        definition_id: row.get(1),
                        definition_version: row.get(2),
                        status,
                        current_step_id: row.get(4),
                        input: input_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json.as_ref().and_then(|s| serde_json::from_str(s).ok()),
                        error: row.get(7),
                        node_id: row.get(8),
                        version: version as u64,
                        last_heartbeat,
                    });
                }
            }
        }

        Ok(executions)
    }
}
