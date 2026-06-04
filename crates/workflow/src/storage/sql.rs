// SPDX-License-Identifier: AGPL-3.0-or-later
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
//! - SQLite for testing/embedded use; PostgreSQL for production.
//! - Schema: file/Postgres use unified `db/migrations` at init; `:memory:` uses inline schema in this module.
//! - Database-agnostic SQL where possible; sqlx for type-safe queries.

use prost::Message as ProstMessage;
use serde_json::Value;
use sqlx::{
    postgres::PgPool,
    sqlite::{SqlitePool, SqlitePoolOptions},
    Row,
};
use std::collections::HashMap;

use crate::types::*;

/// Type alias for the execution query row tuple returned from SQL queries
type ExecutionQueryRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    Option<chrono::DateTime<chrono::Utc>>,
);

/// Type alias for the step execution query row tuple returned from SQL queries
type StepExecutionQueryRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
);

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

/// Encode a WorkflowDefinition to proto binary bytes
fn encode_definition(def: &WorkflowDefinition) -> Result<Vec<u8>, WorkflowError> {
    let mut buf = Vec::new();
    def.encode(&mut buf)
        .map_err(|e| WorkflowError::Serialization(e.to_string()))?;
    Ok(buf)
}

/// Decode a WorkflowDefinition from proto binary bytes
fn decode_definition(bytes: &[u8]) -> Result<WorkflowDefinition, WorkflowError> {
    WorkflowDefinition::decode(bytes).map_err(|e| WorkflowError::Serialization(e.to_string()))
}

/// Convert serde_json::Value to prost_types::Struct
fn value_to_struct(value: &Value) -> Option<prost_types::Struct> {
    match value {
        Value::Object(map) => {
            let mut fields = std::collections::BTreeMap::new();
            for (k, v) in map {
                fields.insert(k.clone(), value_to_prost_value(v));
            }
            Some(prost_types::Struct { fields })
        }
        Value::Null => None,
        _ => {
            // Wrap non-object values (arrays, scalars) using sentinel key "__value__"
            // so they can be transparently unwrapped in struct_to_value.
            let mut fields = std::collections::BTreeMap::new();
            fields.insert("__value__".to_string(), value_to_prost_value(value));
            Some(prost_types::Struct { fields })
        }
    }
}

/// Convert prost_types::Struct to serde_json::Value
fn struct_to_value(s: &prost_types::Struct) -> Value {
    // Detect the sentinel wrapper "__value__" used for non-object values (arrays, scalars)
    if s.fields.len() == 1 {
        if let Some(inner) = s.fields.get("__value__") {
            return prost_value_to_value(inner);
        }
    }
    let mut map = serde_json::Map::new();
    for (k, v) in &s.fields {
        map.insert(k.clone(), prost_value_to_value(v));
    }
    Value::Object(map)
}

fn value_to_prost_value(v: &Value) -> prost_types::Value {
    let kind = match v {
        Value::Null => prost_types::value::Kind::NullValue(0),
        Value::Bool(b) => prost_types::value::Kind::BoolValue(*b),
        Value::Number(n) => prost_types::value::Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => prost_types::value::Kind::StringValue(s.clone()),
        Value::Array(arr) => {
            let values = arr.iter().map(value_to_prost_value).collect();
            prost_types::value::Kind::ListValue(prost_types::ListValue { values })
        }
        Value::Object(map) => {
            let mut fields = std::collections::BTreeMap::new();
            for (k, val) in map {
                fields.insert(k.clone(), value_to_prost_value(val));
            }
            prost_types::value::Kind::StructValue(prost_types::Struct { fields })
        }
    };
    prost_types::Value { kind: Some(kind) }
}

fn prost_value_to_value(v: &prost_types::Value) -> Value {
    match &v.kind {
        None => Value::Null,
        Some(prost_types::value::Kind::NullValue(_)) => Value::Null,
        Some(prost_types::value::Kind::BoolValue(b)) => Value::Bool(*b),
        Some(prost_types::value::Kind::NumberValue(n)) => serde_json::Number::from_f64(*n)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Some(prost_types::value::Kind::StringValue(s)) => Value::String(s.clone()),
        Some(prost_types::value::Kind::ListValue(list)) => {
            Value::Array(list.values.iter().map(prost_value_to_value).collect())
        }
        Some(prost_types::value::Kind::StructValue(s)) => struct_to_value(s),
    }
}


/// Convert serde_json::Value to Option<prost_types::Struct>
fn value_to_opt_struct(v: &Value) -> Option<prost_types::Struct> {
    if v.is_null() {
        None
    } else {
        value_to_struct(v)
    }
}

/// Get the step type as StepType enum
pub fn step_type(step: &Step) -> StepType {
    StepType::try_from(step.r#type).unwrap_or(StepType::StepTypeTask)
}

/// Get config as a serde_json::Value from a Step's config field
pub fn step_config_value(step: &Step) -> Value {
    step.config
        .as_ref()
        .map(struct_to_value)
        .unwrap_or(Value::Object(serde_json::Map::new()))
}

/// Get the next step ID from a Step's depends_on field (first entry)
pub fn step_next(step: &Step) -> Option<&str> {
    step.depends_on.first().map(|s| s.as_str())
}

/// Get the opt_struct_to_value helper for use outside this module
pub fn execution_input_to_value(input: &Option<prost_types::Struct>) -> Option<Value> {
    input.as_ref().map(|s| {
        let mut map = serde_json::Map::new();
        for (k, v) in &s.fields {
            map.insert(k.clone(), prost_value_to_value(v));
        }
        Value::Object(map)
    })
}

/// Create a Step from components
pub fn make_step(
    id: impl Into<String>,
    name: impl Into<String>,
    step_type: StepType,
    config: Value,
    next: Option<String>,
    on_error: Option<String>,
    retry: Option<RetryConfig>,
) -> Step {
    Step {
        id: id.into(),
        name: name.into(),
        r#type: step_type as i32,
        config: value_to_opt_struct(&config),
        depends_on: next.into_iter().collect(),
        on_error: on_error.unwrap_or_default(),
        retry,
        timeout: None,
    }
}

/// Create a WorkflowDefinition from components
pub fn make_workflow_definition(
    id: impl Into<String>,
    name: impl Into<String>,
    version: impl Into<String>,
    steps: Vec<Step>,
) -> WorkflowDefinition {
    WorkflowDefinition {
        id: id.into(),
        name: name.into(),
        version: version.into(),
        steps,
        default_timeout: None,
        default_retry: None,
        labels: HashMap::new(),
        created_at: None,
        updated_at: None,
    }
}

/// Internal execution state stored in SQL (uses serde_json for flexibility)
#[derive(Debug, Clone)]
pub(crate) struct WorkflowExecutionRow {
    pub execution_id: String,
    pub definition_id: String,
    pub definition_version: String,
    pub status: ExecutionStatus,
    pub current_step_id: Option<String>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub node_id: Option<String>,
    pub _version: u64,
    pub last_heartbeat: Option<chrono::DateTime<chrono::Utc>>,
}

/// Internal step execution state stored in SQL
#[derive(Debug, Clone)]
pub(crate) struct StepExecutionRow {
    pub step_execution_id: String,
    pub execution_id: String,
    pub step_id: String,
    pub status: StepStatus,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub attempt: u32,
}

impl From<WorkflowExecutionRow> for WorkflowExecution {
    fn from(row: WorkflowExecutionRow) -> Self {
        WorkflowExecution {
            execution_id: row.execution_id,
            definition_id: row.definition_id,
            definition_version: row.definition_version,
            status: row.status as i32,
            current_step_id: row.current_step_id.unwrap_or_default(),
            input: row.input.as_ref().and_then(value_to_opt_struct),
            output: row.output.as_ref().and_then(value_to_opt_struct),
            error: row.error.unwrap_or_default(),
            node_id: row.node_id.unwrap_or_default(),
            created_at: None,
            started_at: None,
            completed_at: None,
            updated_at: None,
            labels: HashMap::new(),
            last_heartbeat: row.last_heartbeat.map(|dt| prost_types::Timestamp {
                seconds: dt.timestamp(),
                nanos: 0,
            }),
        }
    }
}

impl From<StepExecutionRow> for StepExecution {
    fn from(row: StepExecutionRow) -> Self {
        StepExecution {
            step_execution_id: row.step_execution_id,
            execution_id: row.execution_id,
            step_id: row.step_id,
            status: row.status as i32,
            input: row.input.as_ref().and_then(value_to_opt_struct),
            output: row.output.as_ref().and_then(value_to_opt_struct),
            error: row.error.unwrap_or_default(),
            attempt: row.attempt,
            started_at: None,
            completed_at: None,
        }
    }
}

/// Create workflow tables for :memory: SQLite. File-based uses unified db/migrations at init.
async fn run_workflow_memory_schema_sqlite(pool: &SqlitePool) -> Result<(), WorkflowError> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS workflow_definitions (
            id TEXT NOT NULL, version TEXT NOT NULL, name TEXT NOT NULL, definition_proto BLOB NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')), updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            PRIMARY KEY (id, version))"#,
    )
    .execute(pool)
    .await
    .map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_workflow_definitions_name ON workflow_definitions(name)",
    )
    .execute(pool)
    .await
    .map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_workflow_definitions_created ON workflow_definitions(created_at DESC)")
        .execute(pool).await.map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS workflow_executions (
            execution_id TEXT PRIMARY KEY, definition_id TEXT NOT NULL, definition_version TEXT NOT NULL, status TEXT NOT NULL,
            current_step_id TEXT, input_json TEXT, output_json TEXT, error TEXT, node_id TEXT, version INTEGER NOT NULL DEFAULT 1,
            last_heartbeat INTEGER, metadata_json TEXT, created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            started_at INTEGER, completed_at INTEGER, updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            FOREIGN KEY (definition_id, definition_version) REFERENCES workflow_definitions(id, version))"#,
    )
    .execute(pool)
    .await
    .map_err(|e| WorkflowError::Storage(e.to_string()))?;
    for sql in [
        "CREATE INDEX IF NOT EXISTS idx_workflow_executions_status ON workflow_executions(status)",
        "CREATE INDEX IF NOT EXISTS idx_workflow_executions_definition ON workflow_executions(definition_id)",
        "CREATE INDEX IF NOT EXISTS idx_workflow_executions_node ON workflow_executions(node_id)",
        "CREATE INDEX IF NOT EXISTS idx_workflow_executions_created ON workflow_executions(created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_workflow_executions_heartbeat ON workflow_executions(status, last_heartbeat) WHERE status IN ('RUNNING', 'PENDING')",
        "CREATE INDEX IF NOT EXISTS idx_workflow_executions_version ON workflow_executions(execution_id, version)",
    ] {
        sqlx::query(sql).execute(pool).await.map_err(|e| WorkflowError::Storage(e.to_string()))?;
    }
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS workflow_execution_labels (
            execution_id TEXT NOT NULL, label_key TEXT NOT NULL, label_value TEXT NOT NULL,
            PRIMARY KEY (execution_id, label_key), FOREIGN KEY (execution_id) REFERENCES workflow_executions(execution_id))"#,
    )
    .execute(pool)
    .await
    .map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_workflow_execution_labels_key_value ON workflow_execution_labels(label_key, label_value)")
        .execute(pool).await.map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS step_executions (
            step_execution_id TEXT PRIMARY KEY, execution_id TEXT NOT NULL, step_id TEXT NOT NULL, status TEXT NOT NULL,
            input_json TEXT, output_json TEXT, error TEXT, attempt INTEGER NOT NULL DEFAULT 1, metadata_json TEXT,
            started_at INTEGER, completed_at INTEGER, FOREIGN KEY (execution_id) REFERENCES workflow_executions(execution_id))"#,
    )
    .execute(pool)
    .await
    .map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_step_executions_execution ON step_executions(execution_id)",
    )
    .execute(pool)
    .await
    .map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_step_executions_status ON step_executions(status)")
        .execute(pool)
        .await
        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_step_executions_started ON step_executions(started_at DESC)")
        .execute(pool).await.map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS signals (
            signal_id TEXT PRIMARY KEY NOT NULL, execution_id TEXT NOT NULL, signal_name TEXT NOT NULL, payload TEXT NOT NULL,
            received_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')), FOREIGN KEY (execution_id) REFERENCES workflow_executions(execution_id))"#,
    )
    .execute(pool)
    .await
    .map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_signals_execution_name ON signals(execution_id, signal_name, received_at)")
        .execute(pool).await.map_err(|e| WorkflowError::Storage(e.to_string()))?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_signals_execution ON signals(execution_id)")
        .execute(pool)
        .await
        .map_err(|e| WorkflowError::Storage(e.to_string()))?;
    Ok(())
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
                        std::fs::create_dir_all(parent).map_err(|e| {
                            WorkflowError::Storage(format!("Failed to create directory: {}", e))
                        })?;
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

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("Connecting to SQLite with connection string: {}", conn_str);
        }
        // Ensure parent directory exists for file-based databases
        if !conn_str.starts_with("sqlite::memory:") {
            if let Some(db_path) = conn_str.strip_prefix("sqlite:") {
                if let Some(parent) = std::path::Path::new(db_path).parent() {
                    if !parent.as_os_str().is_empty() && !parent.exists() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            WorkflowError::Storage(format!(
                                "Failed to create directory for database: {}",
                                e
                            ))
                        })?;
                    }
                }
                // Touch the file to ensure it exists (sqlx should create it, but this helps debug)
                if !std::path::Path::new(db_path).exists() {
                    std::fs::File::create(db_path).map_err(|e| {
                        WorkflowError::Storage(format!(
                            "Failed to create database file {}: {}",
                            db_path, e
                        ))
                    })?;
                }
            }
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&conn_str)
            .await
            .map_err(|e| {
                WorkflowError::Storage(format!(
                    "Failed to connect to SQLite (conn_str: {}): {}",
                    conn_str, e
                ))
            })?;

        // For :memory: create schema inline; file-based uses unified db/migrations at init.
        if conn_str.starts_with("sqlite::memory:") {
            run_workflow_memory_schema_sqlite(&pool).await?;
        }

        let storage = Self {
            pool: SqlPool::Sqlite(pool),
            db_type: DatabaseType::SQLite,
        };

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
        let pool = PgPool::connect(connection_string).await.map_err(|e| {
            WorkflowError::Storage(format!("Failed to connect to PostgreSQL: {}", e))
        })?;

        let storage = Self {
            pool: SqlPool::Postgres(pool),
            db_type: DatabaseType::PostgreSQL,
        };

        // Schema is created by unified db/migrations at init. Assume it exists.

        Ok(storage)
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

    /// Save workflow definition (serialized as proto binary)
    pub async fn save_definition(&self, def: &WorkflowDefinition) -> Result<(), WorkflowError> {
        let definition_bytes = encode_definition(def)?;

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
                .bind(&definition_bytes)
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
                .bind(&definition_bytes)
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
                    WorkflowError::NotFound(format!(
                        "Definition {}:{} not found: {}",
                        id, version, e
                    ))
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
                    WorkflowError::NotFound(format!(
                        "Definition {}:{} not found: {}",
                        id, version, e
                    ))
                })?;
                row.get(0)
            }
        };

        decode_definition(&definition_bytes)
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
                    let definition = decode_definition(&definition_bytes)?;
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
                    let definition = decode_definition(&definition_bytes)?;
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
    pub async fn delete_definition(&self, id: &str, version: &str) -> Result<(), WorkflowError> {
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
        .bind(ExecutionStatus::ExecutionStatusPending.as_sql_str())
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
                .bind(ExecutionStatus::ExecutionStatusPending.as_sql_str())
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

    /// Get workflow execution (returns proto WorkflowExecution)
    pub async fn get_execution(
        &self,
        execution_id: &str,
    ) -> Result<WorkflowExecution, WorkflowError> {
        let row = self.get_execution_row(execution_id).await?;
        Ok(row.into())
    }

    /// Get workflow execution as internal row (with full version/heartbeat info)
    pub(crate) async fn get_execution_row(
        &self,
        execution_id: &str,
    ) -> Result<WorkflowExecutionRow, WorkflowError> {
        let (
            execution_id_val,
            definition_id,
            definition_version,
            status_str,
            current_step_id,
            input_json,
            output_json,
            error,
            node_id,
            version,
            last_heartbeat,
        ): ExecutionQueryRow = match &self.pool {
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

        let status = ExecutionStatus::from_sql_str(&status_str)?;

        Ok(WorkflowExecutionRow {
            execution_id: execution_id_val,
            definition_id,
            definition_version,
            status,
            current_step_id,
            input: input_json
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
            output: output_json
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
            error,
            node_id,
            _version: version as u64,
            last_heartbeat,
        })
    }

    /// Update execution status (with optimistic locking)
    pub async fn update_execution_status(
        &self,
        execution_id: &str,
        status: ExecutionStatus,
    ) -> Result<(), WorkflowError> {
        self.update_execution_status_with_version(execution_id, status, None)
            .await
    }

    /// Update execution status with version check (optimistic locking)
    pub async fn update_execution_status_with_version(
        &self,
        execution_id: &str,
        status: ExecutionStatus,
        expected_version: Option<u64>,
    ) -> Result<(), WorkflowError> {
        let status_str = status.as_sql_str();

        let rows_affected = match &self.pool {
            SqlPool::Sqlite(pool) => {
                let result = if let Some(version) = expected_version {
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
                    .bind(status_str)
                    .bind(status_str)
                    .bind(status_str)
                    .bind(status_str)
                    .bind(execution_id)
                    .bind(version as i64)
                    .execute(pool)
                    .await
                } else {
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
                    .bind(status_str)
        .bind(status_str)
        .bind(status_str)
        .bind(status_str)
        .bind(execution_id)
                    .execute(pool)
                    .await
                };
                result
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?
                    .rows_affected()
            }
            SqlPool::Postgres(pool) => {
                let result = if let Some(version) = expected_version {
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
                    .bind(status_str)
                    .bind(status_str)
                    .bind(status_str)
                    .bind(status_str)
                    .bind(execution_id)
                    .bind(version as i64)
                    .execute(pool)
                    .await
                } else {
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
                    .bind(status_str)
                    .bind(status_str)
                    .bind(status_str)
                    .bind(status_str)
                    .bind(execution_id)
                    .execute(pool)
                    .await
                };
                result
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?
                    .rows_affected()
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
    pub async fn transfer_ownership(
        &self,
        execution_id: &str,
        new_node_id: &str,
        expected_version: u64,
    ) -> Result<(), WorkflowError> {
        let rows_affected = match &self.pool {
            SqlPool::Sqlite(pool) => sqlx::query(
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
            .rows_affected(),
            SqlPool::Postgres(pool) => sqlx::query(
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
            .rows_affected(),
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
        self.update_execution_output_with_version(execution_id, output, None)
            .await
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
                result
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?
                    .rows_affected()
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
                result
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?
                    .rows_affected()
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
                .bind(StepStatus::StepStatusRunning.as_sql_str())
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
                .bind(StepStatus::StepStatusRunning.as_sql_str())
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
        let row = self.get_step_execution_row(step_exec_id).await?;
        Ok(row.into())
    }

    /// Get step execution as internal row
    pub(crate) async fn get_step_execution_row(
        &self,
        step_exec_id: &str,
    ) -> Result<StepExecutionRow, WorkflowError> {
        let (
            step_execution_id,
            execution_id,
            step_id,
            status_str,
            input_json,
            output_json,
            error,
            attempt,
        ): StepExecutionQueryRow = match &self.pool {
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
                    WorkflowError::NotFound(format!(
                        "Step execution {} not found: {}",
                        step_exec_id, e
                    ))
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
                    WorkflowError::NotFound(format!(
                        "Step execution {} not found: {}",
                        step_exec_id, e
                    ))
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

        let status = StepStatus::from_sql_str(&status_str)?;

        Ok(StepExecutionRow {
            step_execution_id,
            execution_id,
            step_id,
            status,
            input: input_json
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
            output: output_json
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
            error,
            attempt: attempt as u32,
        })
    }

    /// Complete step execution with status, output, and/or error
    pub async fn complete_step_execution(
        &self,
        step_exec_id: &str,
        status: StepStatus,
        output: Option<Value>,
        error: Option<String>,
    ) -> Result<(), WorkflowError> {
        let status_str = status.as_sql_str();
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
                .bind(status_str)
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
                .bind(status_str)
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
        let rows = self.get_step_execution_history_rows(execution_id).await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// Get all step executions as internal rows
    pub(crate) async fn get_step_execution_history_rows(
        &self,
        execution_id: &str,
    ) -> Result<Vec<StepExecutionRow>, WorkflowError> {
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
                    let status = StepStatus::from_sql_str(&status_str)?;

                    let input_json: Option<String> = row.get(4);
                    let output_json: Option<String> = row.get(5);
                    let attempt: i64 = row.get(7);

                    executions.push(StepExecutionRow {
                        step_execution_id: row.get(0),
                        execution_id: row.get(1),
                        step_id: row.get(2),
                        status,
                        input: input_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
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
                    let status = StepStatus::from_sql_str(&status_str)?;

                    let input_json: Option<String> = row.get(4);
                    let output_json: Option<String> = row.get(5);
                    let attempt: i64 = row.get(7);

                    executions.push(StepExecutionRow {
                        step_execution_id: row.get(0),
                        execution_id: row.get(1),
                        step_id: row.get(2),
                        status,
                        input: input_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        error: row.get(6),
                        attempt: attempt as u32,
                    });
                }
            }
        }

        Ok(executions)
    }

    /// Send a signal to a workflow execution
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
    pub async fn check_signal(
        &self,
        execution_id: &str,
        signal_name: &str,
    ) -> Result<Option<Value>, WorkflowError> {
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

            match &self.pool {
                SqlPool::Sqlite(pool) => {
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
                    .map_err(|e| {
                        WorkflowError::Storage(format!("Failed to delete signal: {}", e))
                    })?;
                }
                SqlPool::Postgres(pool) => {
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
                    .map_err(|e| {
                        WorkflowError::Storage(format!("Failed to delete signal: {}", e))
                    })?;
                }
            }

            Ok(Some(payload))
        } else {
            Ok(None)
        }
    }

    /// List workflow executions by status
    pub async fn list_executions_by_status(
        &self,
        statuses: Vec<ExecutionStatus>,
        node_id: Option<&str>,
    ) -> Result<Vec<WorkflowExecution>, WorkflowError> {
        let rows = self
            .list_execution_rows_by_status(statuses, node_id)
            .await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// List workflow execution rows by status
    pub(crate) async fn list_execution_rows_by_status(
        &self,
        statuses: Vec<ExecutionStatus>,
        node_id: Option<&str>,
    ) -> Result<Vec<WorkflowExecutionRow>, WorkflowError> {
        let status_strings: Vec<String> = statuses
            .iter()
            .map(|s| s.as_sql_str().to_string())
            .collect();
        let mut executions = Vec::new();

        match &self.pool {
            SqlPool::Sqlite(pool) => {
                let status_placeholders: String = status_strings
                    .iter()
                    .map(|_| "?")
                    .collect::<Vec<_>>()
                    .join(",");
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

                for status_str in &status_strings {
                    query_builder = query_builder.bind(status_str);
                }

                if let Some(nid) = node_id {
                    query_builder = query_builder.bind(nid);
                }

                let rows = query_builder
                    .fetch_all(pool)
                    .await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let status_str: String = row.get(3);
                    let status = ExecutionStatus::from_sql_str(&status_str)?;

                    let input_json: Option<String> = row.get(5);
                    let output_json: Option<String> = row.get(6);
                    let version: i64 = row.get(9);
                    let last_heartbeat: Option<chrono::DateTime<chrono::Utc>> = row.get(10);

                    executions.push(WorkflowExecutionRow {
                        execution_id: row.get(0),
                        definition_id: row.get(1),
                        definition_version: row.get(2),
                        status,
                        current_step_id: row.get(4),
                        input: input_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        error: row.get(7),
                        node_id: row.get(8),
                        _version: version as u64,
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

                let _ = param_idx; // suppress warning

                query.push_str(" ORDER BY created_at ASC");

                let mut query_builder = sqlx::query(&query);

                for status_str in &status_strings {
                    query_builder = query_builder.bind(status_str);
                }

                if let Some(nid) = node_id {
                    query_builder = query_builder.bind(nid);
                }

                let rows = query_builder
                    .fetch_all(pool)
                    .await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let status_str: String = row.get(3);
                    let status = ExecutionStatus::from_sql_str(&status_str)?;

                    let input_json: Option<String> = row.get(5);
                    let output_json: Option<String> = row.get(6);
                    let version: i64 = row.get(9);
                    let last_heartbeat: Option<chrono::DateTime<chrono::Utc>> = row.get(10);

                    executions.push(WorkflowExecutionRow {
                        execution_id: row.get(0),
                        definition_id: row.get(1),
                        definition_version: row.get(2),
                        status,
                        current_step_id: row.get(4),
                        input: input_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        error: row.get(7),
                        node_id: row.get(8),
                        _version: version as u64,
                        last_heartbeat,
                    });
                }
            }
        }

        Ok(executions)
    }

    /// List stale workflow executions (not updated recently)
    pub async fn list_stale_executions(
        &self,
        stale_threshold_seconds: u64,
        statuses: Vec<ExecutionStatus>,
    ) -> Result<Vec<WorkflowExecution>, WorkflowError> {
        let rows = self
            .list_stale_execution_rows(stale_threshold_seconds, statuses)
            .await?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// List stale workflow execution rows
    pub(crate) async fn list_stale_execution_rows(
        &self,
        stale_threshold_seconds: u64,
        statuses: Vec<ExecutionStatus>,
    ) -> Result<Vec<WorkflowExecutionRow>, WorkflowError> {
        let status_strings: Vec<String> = statuses
            .iter()
            .map(|s| s.as_sql_str().to_string())
            .collect();
        let mut executions = Vec::new();

        match &self.pool {
            SqlPool::Sqlite(pool) => {
                let status_placeholders: String = status_strings
                    .iter()
                    .map(|_| "?")
                    .collect::<Vec<_>>()
                    .join(",");
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

                for status_str in &status_strings {
                    query_builder = query_builder.bind(status_str);
                }

                query_builder = query_builder.bind(stale_threshold_seconds as i64);

                let rows = query_builder
                    .fetch_all(pool)
                    .await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let status_str: String = row.get(3);
                    let status = ExecutionStatus::from_sql_str(&status_str)?;

                    let input_json: Option<String> = row.get(5);
                    let output_json: Option<String> = row.get(6);
                    let version: i64 = row.get(9);
                    let last_heartbeat: Option<chrono::DateTime<chrono::Utc>> = row.get(10);

                    executions.push(WorkflowExecutionRow {
                        execution_id: row.get(0),
                        definition_id: row.get(1),
                        definition_version: row.get(2),
                        status,
                        current_step_id: row.get(4),
                        input: input_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        error: row.get(7),
                        node_id: row.get(8),
                        _version: version as u64,
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

                for status_str in &status_strings {
                    query_builder = query_builder.bind(status_str);
                }

                query_builder = query_builder.bind(stale_threshold_seconds as i64);

                let rows = query_builder
                    .fetch_all(pool)
                    .await
                    .map_err(|e| WorkflowError::Storage(e.to_string()))?;

                for row in rows {
                    let status_str: String = row.get(3);
                    let status = ExecutionStatus::from_sql_str(&status_str)?;

                    let input_json: Option<String> = row.get(5);
                    let output_json: Option<String> = row.get(6);
                    let version: i64 = row.get(9);
                    let last_heartbeat: Option<chrono::DateTime<chrono::Utc>> = row.get(10);

                    executions.push(WorkflowExecutionRow {
                        execution_id: row.get(0),
                        definition_id: row.get(1),
                        definition_version: row.get(2),
                        status,
                        current_step_id: row.get(4),
                        input: input_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        output: output_json
                            .as_ref()
                            .and_then(|s| serde_json::from_str(s).ok()),
                        error: row.get(7),
                        node_id: row.get(8),
                        _version: version as u64,
                        last_heartbeat,
                    });
                }
            }
        }

        Ok(executions)
    }
}
