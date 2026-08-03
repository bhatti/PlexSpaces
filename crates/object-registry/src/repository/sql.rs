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

//! SQL-based Object Registry Repository implementations (SQLite and PostgreSQL)
//!
//! ## Purpose
//! Provides persistent, transactional storage for object registrations using
//! relational databases with indexed columns for fast queries.
//!
//! ## Features
//! - **Persistent**: Data survives process restarts
//! - **Indexed columns**: Fast queries by object_type, node_id, health_status, last_heartbeat
//! - **Blob storage**: Full ObjectRegistration preserved in registration_blob
//! - **Multi-tenancy**: All operations filtered by tenant_id and namespace

use super::{DiscoverFilter, ObjectRegistryRepository, RepositoryError, RepositoryResult};
use async_trait::async_trait;
use plexspaces_common::{RequestContext, RequestContextExt};
use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};
use prost::Message;
use sqlx::{Pool, Postgres, Row, Sqlite};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, instrument};

/// SQLite Object Registry Repository
///
/// ## Purpose
/// Provides persistent storage using SQLite for embedded deployments.
///
/// ## Schema
/// Uses object_registrations table with indexed columns.
#[derive(Debug, Clone)]
pub struct SqliteObjectRegistryRepository {
    pool: Pool<Sqlite>,
}

impl SqliteObjectRegistryRepository {
    /// Create a new SQLite repository
    ///
    /// ## Arguments
    /// * `path` - Database file path (use ":memory:" for in-memory)
    ///
    /// ## Behavior
    /// Runs migrations automatically on initialization.
    pub async fn new(path: &str) -> RepositoryResult<Self> {
        let url = if path == ":memory:" {
            "sqlite::memory:".to_string()
        } else if path.starts_with('/') {
            format!("sqlite://{}?mode=rwc", path)
        } else {
            format!("sqlite:{}?mode=rwc", path)
        };

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .map_err(|e| RepositoryError::Connection(e.to_string()))?;

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
            sqlx::query(pragma)
                .execute(&pool)
                .await
                .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        }

        Self::run_migrations(&pool).await?;

        Ok(Self { pool })
    }

    /// Ensure object_registrations schema exists (idempotent).
    async fn run_migrations(pool: &Pool<Sqlite>) -> RepositoryResult<()> {
        const SCHEMA: &str = r#"CREATE TABLE IF NOT EXISTS object_registrations (
            tenant_id TEXT NOT NULL, namespace TEXT NOT NULL, object_id TEXT NOT NULL,
            object_type INTEGER NOT NULL, object_name TEXT, version TEXT, node_id TEXT,
            grpc_address TEXT NOT NULL, object_category TEXT, health_status INTEGER NOT NULL DEFAULT 0,
            last_heartbeat BIGINT, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
            alias TEXT, max_heartbeat_failures INTEGER NOT NULL DEFAULT 3,
            heartbeat_failure_count INTEGER NOT NULL DEFAULT 0,
            registration_blob BLOB NOT NULL, PRIMARY KEY (tenant_id, namespace, object_id))"#;
        sqlx::query(SCHEMA)
            .execute(pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_object_registrations_type ON object_registrations(tenant_id, namespace, object_type)").execute(pool).await.map_err(|e| RepositoryError::Storage(e.to_string()))?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_object_registrations_node ON object_registrations(tenant_id, namespace, node_id)").execute(pool).await.map_err(|e| RepositoryError::Storage(e.to_string()))?;
        sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_object_registrations_unique_node_registration ON object_registrations(tenant_id, namespace, node_id) WHERE object_type = 7 AND node_id IS NOT NULL AND node_id <> ''").execute(pool).await.map_err(|e| RepositoryError::Storage(e.to_string()))?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_object_registrations_heartbeat ON object_registrations(tenant_id, namespace, last_heartbeat)").execute(pool).await.map_err(|e| RepositoryError::Storage(e.to_string()))?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_object_registrations_health ON object_registrations(tenant_id, namespace, health_status)").execute(pool).await.map_err(|e| RepositoryError::Storage(e.to_string()))?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_object_registrations_category ON object_registrations(tenant_id, namespace, object_category)").execute(pool).await.map_err(|e| RepositoryError::Storage(e.to_string()))?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_object_registrations_type_health ON object_registrations(tenant_id, namespace, object_type, health_status)").execute(pool).await.map_err(|e| RepositoryError::Storage(e.to_string()))?;
        sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_object_registrations_alias ON object_registrations(alias) WHERE alias IS NOT NULL AND alias != ''").execute(pool).await.map_err(|e| RepositoryError::Storage(e.to_string()))?;
        debug!("Object registry SQLite schema created");
        Ok(())
    }

    /// Get current Unix timestamp
    fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    /// Merge indexed columns into a decoded registration.
    ///
    /// The indexed columns `health_status`, `heartbeat_failure_count`, and
    /// `max_heartbeat_failures` are written directly without a blob round-trip,
    /// so they always reflect current state and must override any stale blob values.
    fn merge_indexed_row(
        blob: Vec<u8>,
        health_status: i32,
        failure_count: i64,
        max_failures: i64,
    ) -> RepositoryResult<ObjectRegistration> {
        let mut reg = ObjectRegistration::decode(&blob[..])
            .map_err(|e| RepositoryError::Serialization(e.to_string()))?;
        reg.health_status = health_status;
        reg.heartbeat_failure_count = failure_count as u32;
        if max_failures > 0 {
            reg.max_heartbeat_failures = max_failures as u32;
        }
        Ok(reg)
    }
}

#[async_trait]
impl ObjectRegistryRepository for SqliteObjectRegistryRepository {
    #[instrument(skip(self, ctx, registration), fields(tenant_id = %ctx.tenant_id(), object_id = %registration.object_id))]
    async fn put(
        &self,
        ctx: &RequestContext,
        registration: &ObjectRegistration,
    ) -> RepositoryResult<()> {
        let now = Self::now_unix();
        let blob = registration.encode_to_vec();
        let last_heartbeat = registration.last_heartbeat.as_ref().map(|t| t.seconds);

        let alias_val = if registration.alias.is_empty() {
            None
        } else {
            Some(registration.alias.clone())
        };
        let max_failures = if registration.max_heartbeat_failures == 0 {
            3i64
        } else {
            registration.max_heartbeat_failures as i64
        };

        sqlx::query(
            r#"
            INSERT INTO object_registrations (
                tenant_id, namespace, object_id, object_type, object_name, version,
                node_id, grpc_address, object_category, health_status,
                last_heartbeat, created_at, updated_at,
                alias, max_heartbeat_failures, heartbeat_failure_count,
                registration_blob
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?)
            ON CONFLICT(tenant_id, namespace, object_id) DO UPDATE SET
                object_type = excluded.object_type,
                object_name = excluded.object_name,
                version = excluded.version,
                node_id = excluded.node_id,
                grpc_address = excluded.grpc_address,
                object_category = excluded.object_category,
                health_status = excluded.health_status,
                last_heartbeat = excluded.last_heartbeat,
                updated_at = excluded.updated_at,
                alias = excluded.alias,
                max_heartbeat_failures = excluded.max_heartbeat_failures,
                heartbeat_failure_count = 0,
                registration_blob = excluded.registration_blob
            "#,
        )
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(&registration.object_id)
        .bind(registration.object_type)
        .bind(&registration.object_name)
        .bind(&registration.version)
        .bind(&registration.node_id)
        .bind(&registration.grpc_address)
        .bind(&registration.object_category)
        .bind(registration.health_status)
        .bind(last_heartbeat)
        .bind(now)
        .bind(now)
        .bind(alias_val)
        .bind(max_failures)
        .bind(&blob)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn get(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> RepositoryResult<Option<ObjectRegistration>> {
        // Fetch both blob and indexed columns (last_heartbeat, health_status may have been updated separately)
        let row = sqlx::query(
            r#"
            SELECT registration_blob, last_heartbeat, health_status,
                   heartbeat_failure_count, max_heartbeat_failures
            FROM object_registrations
            WHERE tenant_id = ? AND namespace = ? AND object_id = ?
            "#,
        )
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        match row {
            Some(row) => {
                let blob: Vec<u8> = row
                    .try_get("registration_blob")
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?;
                let mut registration = ObjectRegistration::decode(&blob[..])
                    .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

                // Merge indexed columns that may have been updated separately (heartbeat optimization).
                let last_heartbeat: Option<i64> = row.try_get("last_heartbeat").unwrap_or(None);
                if let Some(ts) = last_heartbeat {
                    let blob_seconds = registration
                        .last_heartbeat
                        .as_ref()
                        .map(|t| t.seconds)
                        .unwrap_or(0);
                    if blob_seconds != ts {
                        registration.last_heartbeat = Some(prost_types::Timestamp {
                            seconds: ts,
                            nanos: 0,
                        });
                    }
                }

                // Merge health_status and failure counts from indexed columns
                let health_status: i32 = row
                    .try_get("health_status")
                    .unwrap_or(registration.health_status);
                registration.health_status = health_status;

                let failure_count: i64 = row.try_get("heartbeat_failure_count").unwrap_or(0);
                registration.heartbeat_failure_count = failure_count as u32;

                let max_failures: i64 = row.try_get("max_heartbeat_failures").unwrap_or(3);
                if max_failures > 0 {
                    registration.max_heartbeat_failures = max_failures as u32;
                }

                Ok(Some(registration))
            }
            None => Ok(None),
        }
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn delete(&self, ctx: &RequestContext, object_id: &str) -> RepositoryResult<()> {
        sqlx::query(
            r#"
            DELETE FROM object_registrations
            WHERE tenant_id = ? AND namespace = ? AND object_id = ?
            "#,
        )
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(())
    }

    #[instrument(skip(self, ctx, filter), fields(tenant_id = %ctx.tenant_id()))]
    async fn discover(
        &self,
        ctx: &RequestContext,
        filter: &DiscoverFilter,
        offset: usize,
        limit: usize,
    ) -> RepositoryResult<Vec<ObjectRegistration>> {
        // Build dynamic WHERE clause
        let mut conditions = Vec::new();
        let mut bindings: Vec<String> = Vec::new();

        if !ctx.is_admin() {
            conditions.push("tenant_id = ?".to_string());
            bindings.push(ctx.tenant_id().to_string());
        }
        if !ctx.namespace().is_empty() {
            conditions.push("namespace = ?".to_string());
            bindings.push(ctx.namespace().to_string());
        }

        if let Some(obj_type) = filter.object_type.as_ref() {
            conditions.push("object_type = ?".to_string());
            bindings.push((obj_type.clone() as i32).to_string());
        }

        if let Some(ref category) = filter.object_category {
            conditions.push("object_category = ?".to_string());
            bindings.push(category.clone());
        }

        if let Some(ref node_id) = filter.node_id {
            conditions.push("node_id = ?".to_string());
            bindings.push(node_id.clone());
        }

        if let Some(status) = filter.health_status.as_ref() {
            conditions.push("health_status = ?".to_string());
            bindings.push((status.clone() as i32).to_string());
        }

        if let Some(before) = filter.last_heartbeat_before {
            conditions.push("(last_heartbeat IS NULL OR last_heartbeat < ?)".to_string());
            bindings.push(before.to_string());
        }

        if let Some(after) = filter.last_heartbeat_after {
            conditions.push("last_heartbeat > ?".to_string());
            bindings.push(after.to_string());
        }

        let where_clause = if conditions.is_empty() {
            "1 = 1".to_string()
        } else {
            conditions.join(" AND ")
        };
        let query = format!(
            r#"
            SELECT registration_blob FROM object_registrations
            WHERE {}
            ORDER BY created_at ASC
            LIMIT ? OFFSET ?
            "#,
            where_clause
        );

        // Build query dynamically
        let mut query_builder = sqlx::query(&query);
        for binding in &bindings {
            query_builder = query_builder.bind(binding);
        }
        query_builder = query_builder.bind(limit as i64).bind(offset as i64);

        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let blob: Vec<u8> = row
                .try_get("registration_blob")
                .map_err(|e| RepositoryError::Storage(e.to_string()))?;
            let registration = ObjectRegistration::decode(&blob[..])
                .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

            // Post-filter for labels and capabilities (stored in blob)
            if let Some(ref required_labels) = filter.labels {
                if !required_labels
                    .iter()
                    .all(|l| registration.labels.contains(l))
                {
                    continue;
                }
            }
            if let Some(ref required_caps) = filter.capabilities {
                if !required_caps
                    .iter()
                    .all(|c| registration.capabilities.contains(c))
                {
                    continue;
                }
            }

            results.push(registration);
        }

        Ok(results)
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn update_heartbeat(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        timestamp: i64,
    ) -> RepositoryResult<()> {
        let result = sqlx::query(
            r#"
            UPDATE object_registrations
            SET last_heartbeat = ?, updated_at = ?
            WHERE tenant_id = ? AND namespace = ? AND object_id = ?
            "#,
        )
        .bind(timestamp)
        .bind(Self::now_unix())
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::NotFound(object_id.to_string()));
        }

        Ok(())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn update_health_status(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        status: HealthStatus,
    ) -> RepositoryResult<()> {
        let result = sqlx::query(
            r#"
            UPDATE object_registrations
            SET health_status = ?, updated_at = ?
            WHERE tenant_id = ? AND namespace = ? AND object_id = ?
            "#,
        )
        .bind(status as i32)
        .bind(Self::now_unix())
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::NotFound(object_id.to_string()));
        }

        Ok(())
    }

    #[instrument(skip(self, ctx, filter), fields(tenant_id = %ctx.tenant_id()))]
    async fn count(
        &self,
        ctx: &RequestContext,
        filter: &DiscoverFilter,
    ) -> RepositoryResult<usize> {
        // Build dynamic WHERE clause (same as discover, including heartbeat filters)
        let mut conditions = Vec::new();
        let mut bindings: Vec<String> = Vec::new();

        if !ctx.is_admin() {
            conditions.push("tenant_id = ?".to_string());
            bindings.push(ctx.tenant_id().to_string());
        }
        if !ctx.namespace().is_empty() {
            conditions.push("namespace = ?".to_string());
            bindings.push(ctx.namespace().to_string());
        }

        if let Some(obj_type) = filter.object_type.as_ref() {
            conditions.push("object_type = ?".to_string());
            bindings.push((obj_type.clone() as i32).to_string());
        }

        if let Some(ref category) = filter.object_category {
            conditions.push("object_category = ?".to_string());
            bindings.push(category.clone());
        }

        if let Some(ref node_id) = filter.node_id {
            conditions.push("node_id = ?".to_string());
            bindings.push(node_id.clone());
        }

        if let Some(status) = filter.health_status.as_ref() {
            conditions.push("health_status = ?".to_string());
            bindings.push((status.clone() as i32).to_string());
        }

        // Include heartbeat filters (same as discover)
        if let Some(before) = filter.last_heartbeat_before {
            conditions.push("(last_heartbeat IS NULL OR last_heartbeat < ?)".to_string());
            bindings.push(before.to_string());
        }

        if let Some(after) = filter.last_heartbeat_after {
            conditions.push("last_heartbeat > ?".to_string());
            bindings.push(after.to_string());
        }

        let where_clause = if conditions.is_empty() {
            "1 = 1".to_string()
        } else {
            conditions.join(" AND ")
        };
        let query = format!(
            "SELECT COUNT(*) as cnt FROM object_registrations WHERE {}",
            where_clause
        );

        let mut query_builder = sqlx::query(&query);
        for binding in &bindings {
            query_builder = query_builder.bind(binding);
        }

        let row = query_builder
            .fetch_one(&self.pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let count: i64 = row
            .try_get("cnt")
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(count as usize)
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_type = ?object_type))]
    async fn list_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
        offset: usize,
        limit: usize,
    ) -> RepositoryResult<Vec<String>> {
        if ctx.auth_enabled && !ctx.is_admin() {
            return Ok((!ctx.tenant_id().is_empty())
                .then(|| ctx.tenant_id().to_string())
                .into_iter()
                .collect());
        }

        let mut conditions = vec!["object_type = ?".to_string()];
        let mut bindings = vec![(object_type as i32).to_string()];

        // Admin sees all tenants — don't filter by their own tenant_id
        if !ctx.is_admin() && !ctx.tenant_id().is_empty() {
            conditions.push("tenant_id = ?".to_string());
            bindings.push(ctx.tenant_id().to_string());
        }
        if !ctx.should_skip_namespace_filter() && !ctx.namespace().is_empty() {
            conditions.push("namespace = ?".to_string());
            bindings.push(ctx.namespace().to_string());
        }

        let query = format!(
            r#"
            SELECT DISTINCT tenant_id FROM object_registrations
            WHERE {}
            ORDER BY tenant_id ASC
            LIMIT ? OFFSET ?
            "#,
            conditions.join(" AND ")
        );

        let mut query_builder = sqlx::query(&query);
        for binding in &bindings {
            query_builder = query_builder.bind(binding);
        }
        query_builder = query_builder.bind(limit as i64).bind(offset as i64);

        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        rows.into_iter()
            .map(|row| {
                row.try_get("tenant_id")
                    .map_err(|e| RepositoryError::Storage(e.to_string()))
            })
            .collect()
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_type = ?object_type))]
    async fn count_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
    ) -> RepositoryResult<usize> {
        if ctx.auth_enabled && !ctx.is_admin() {
            return Ok((!ctx.tenant_id().is_empty()) as usize);
        }

        let mut conditions = vec!["object_type = ?".to_string()];
        let mut bindings = vec![(object_type as i32).to_string()];

        // Admin sees all tenants — don't filter by their own tenant_id
        if !ctx.is_admin() && !ctx.tenant_id().is_empty() {
            conditions.push("tenant_id = ?".to_string());
            bindings.push(ctx.tenant_id().to_string());
        }
        if !ctx.should_skip_namespace_filter() && !ctx.namespace().is_empty() {
            conditions.push("namespace = ?".to_string());
            bindings.push(ctx.namespace().to_string());
        }

        let query = format!(
            "SELECT COUNT(DISTINCT tenant_id) AS cnt FROM object_registrations WHERE {}",
            conditions.join(" AND ")
        );
        let mut query_builder = sqlx::query(&query);
        for binding in &bindings {
            query_builder = query_builder.bind(binding);
        }
        let row = query_builder
            .fetch_one(&self.pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let count: i64 = row
            .try_get("cnt")
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(count as usize)
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn exists(&self, ctx: &RequestContext, object_id: &str) -> RepositoryResult<bool> {
        let row = sqlx::query(
            r#"
            SELECT 1 FROM object_registrations
            WHERE tenant_id = ? AND namespace = ? AND object_id = ?
            "#,
        )
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(row.is_some())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), alias = %alias))]
    async fn get_by_alias(
        &self,
        ctx: &RequestContext,
        alias: &str,
    ) -> RepositoryResult<Option<ObjectRegistration>> {
        if alias.is_empty() {
            return Ok(None);
        }
        // Alias is scoped to tenant+namespace (alias format encodes them, and the
        // WHERE clause enforces isolation so cross-tenant collisions are impossible).
        let row = sqlx::query(
            r#"
            SELECT registration_blob, health_status,
                   heartbeat_failure_count, max_heartbeat_failures
            FROM object_registrations
            WHERE tenant_id = ? AND namespace = ? AND alias = ?
            "#,
        )
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(alias)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        match row {
            Some(row) => {
                let blob: Vec<u8> = row
                    .try_get("registration_blob")
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?;
                let health_status: i32 = row.try_get("health_status").unwrap_or(0);
                let failure_count: i64 = row.try_get("heartbeat_failure_count").unwrap_or(0);
                let max_failures: i64 = row.try_get("max_heartbeat_failures").unwrap_or(3);
                Ok(Some(Self::merge_indexed_row(
                    blob,
                    health_status,
                    failure_count,
                    max_failures,
                )?))
            }
            None => Ok(None),
        }
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
    async fn increment_heartbeat_failures(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> RepositoryResult<u32> {
        let now = Self::now_unix();
        let row = sqlx::query(
            r#"
            UPDATE object_registrations
            SET heartbeat_failure_count = heartbeat_failure_count + 1, updated_at = ?
            WHERE tenant_id = ? AND namespace = ? AND object_id = ?
            RETURNING heartbeat_failure_count
            "#,
        )
        .bind(now)
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        match row {
            Some(row) => {
                let count: i64 = row.try_get("heartbeat_failure_count").unwrap_or(1);
                Ok(count as u32)
            }
            None => Err(RepositoryError::NotFound(object_id.to_string())),
        }
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
    async fn reset_heartbeat_failures(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> RepositoryResult<()> {
        let now = Self::now_unix();
        sqlx::query(
            r#"
            UPDATE object_registrations
            SET heartbeat_failure_count = 0, updated_at = ?
            WHERE tenant_id = ? AND namespace = ? AND object_id = ?
            "#,
        )
        .bind(now)
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Mark all live objects on `node_id` as DEAD.
    ///
    /// Scoped to the calling tenant/namespace so a node failure in one tenant cannot
    /// cascade into another tenant's objects.  The heartbeat monitor calls this with
    /// a system-scope context only when auth is disabled.
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), node_id = %node_id))]
    async fn mark_dead_by_node_id(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> RepositoryResult<u64> {
        let now = Self::now_unix();
        // DEAD=3, HEALTHY=1, DEGRADED=2, STARTING=4
        // Scoped to tenant+namespace so cross-tenant cascade is impossible.
        let result = sqlx::query(
            r#"
            UPDATE object_registrations
            SET health_status = 3, updated_at = ?
            WHERE tenant_id = ? AND namespace = ? AND node_id = ?
              AND health_status IN (1, 2, 4)
            "#,
        )
        .bind(now)
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(node_id)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(result.rows_affected())
    }

    /// Find registrations whose heartbeat is older than `threshold_seconds`.
    ///
    /// Only returns HEALTHY (1) or DEGRADED (2) objects — DEAD/STOPPING are excluded
    /// since they are already handled.  The context scopes the scan: pass an admin
    /// context with empty tenant_id to scan all tenants (used by HeartbeatMonitor).
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), threshold_seconds = %threshold_seconds, limit = %limit))]
    async fn find_stale_heartbeats(
        &self,
        ctx: &RequestContext,
        threshold_seconds: i64,
        limit: usize,
    ) -> RepositoryResult<Vec<ObjectRegistration>> {
        let cutoff = Self::now_unix() - threshold_seconds;

        // When called with a non-empty tenant (normal tenant scope) restrict to that tenant.
        // When called with an admin context and empty tenant (system monitor) scan all tenants.
        let (query, bindings_extra): (&str, bool) = if !ctx.tenant_id().is_empty() {
            (
                r#"
                SELECT registration_blob, health_status,
                       heartbeat_failure_count, max_heartbeat_failures
                FROM object_registrations
                WHERE tenant_id = ? AND namespace = ?
                  AND health_status IN (1, 2)
                  AND (last_heartbeat IS NULL OR last_heartbeat < ?)
                LIMIT ?
                "#,
                true,
            )
        } else {
            (
                r#"
                SELECT registration_blob, health_status,
                       heartbeat_failure_count, max_heartbeat_failures
                FROM object_registrations
                WHERE health_status IN (1, 2)
                  AND (last_heartbeat IS NULL OR last_heartbeat < ?)
                LIMIT ?
                "#,
                false,
            )
        };

        let rows = if bindings_extra {
            sqlx::query(query)
                .bind(ctx.tenant_id())
                .bind(ctx.namespace())
                .bind(cutoff)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| RepositoryError::Storage(e.to_string()))?
        } else {
            sqlx::query(query)
                .bind(cutoff)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| RepositoryError::Storage(e.to_string()))?
        };

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let blob: Vec<u8> = row
                .try_get("registration_blob")
                .map_err(|e| RepositoryError::Storage(e.to_string()))?;
            let health_status: i32 = row.try_get("health_status").unwrap_or(1);
            let failure_count: i64 = row.try_get("heartbeat_failure_count").unwrap_or(0);
            let max_failures: i64 = row.try_get("max_heartbeat_failures").unwrap_or(3);
            results.push(Self::merge_indexed_row(
                blob,
                health_status,
                failure_count,
                max_failures,
            )?);
        }
        Ok(results)
    }
}

/// PostgreSQL Object Registry Repository
///
/// ## Purpose
/// Provides persistent storage using PostgreSQL for production deployments.
///
/// ## Schema
/// Uses object_registrations table with indexed columns.
#[derive(Debug, Clone)]
pub struct PostgresObjectRegistryRepository {
    pool: Pool<Postgres>,
}

impl PostgresObjectRegistryRepository {
    /// Create a new PostgreSQL repository
    ///
    /// ## Arguments
    /// * `connection_string` - PostgreSQL connection string
    ///
    /// ## Behavior
    /// Runs migrations automatically on initialization.
    pub async fn new(connection_string: &str) -> RepositoryResult<Self> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(10)
            .connect(connection_string)
            .await
            .map_err(|e| RepositoryError::Connection(e.to_string()))?;

        // Schema is created by unified db/migrations at init. Assume it exists.

        Ok(Self { pool })
    }

    /// Get current timestamp for PostgreSQL
    fn now_timestamp() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }

    /// Merge indexed columns into a decoded registration.
    ///
    /// The indexed columns `health_status`, `heartbeat_failure_count`, and
    /// `max_heartbeat_failures` are written directly without a blob round-trip,
    /// so they always reflect current state and must override any stale blob values.
    fn merge_indexed_row(
        blob: Vec<u8>,
        health_status: i32,
        failure_count: i32,
        max_failures: i32,
    ) -> RepositoryResult<ObjectRegistration> {
        let mut reg = ObjectRegistration::decode(&blob[..])
            .map_err(|e| RepositoryError::Serialization(e.to_string()))?;
        reg.health_status = health_status;
        reg.heartbeat_failure_count = failure_count as u32;
        if max_failures > 0 {
            reg.max_heartbeat_failures = max_failures as u32;
        }
        Ok(reg)
    }
}

#[async_trait]
impl ObjectRegistryRepository for PostgresObjectRegistryRepository {
    #[instrument(skip(self, ctx, registration), fields(tenant_id = %ctx.tenant_id(), object_id = %registration.object_id))]
    async fn put(
        &self,
        ctx: &RequestContext,
        registration: &ObjectRegistration,
    ) -> RepositoryResult<()> {
        let now = Self::now_timestamp();
        let blob = registration.encode_to_vec();
        let last_heartbeat = registration
            .last_heartbeat
            .as_ref()
            .map(|t| chrono::DateTime::from_timestamp(t.seconds, 0).unwrap_or(now));

        let alias_val = if registration.alias.is_empty() {
            None
        } else {
            Some(registration.alias.clone())
        };
        let max_failures = if registration.max_heartbeat_failures == 0 {
            3i32
        } else {
            registration.max_heartbeat_failures as i32
        };

        sqlx::query(
            r#"
            INSERT INTO object_registrations (
                tenant_id, namespace, object_id, object_type, object_name, version,
                node_id, grpc_address, object_category, health_status,
                last_heartbeat, created_at, updated_at,
                alias, max_heartbeat_failures, heartbeat_failure_count,
                registration_blob
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, 0, $16)
            ON CONFLICT(tenant_id, namespace, object_id) DO UPDATE SET
                object_type = EXCLUDED.object_type,
                object_name = EXCLUDED.object_name,
                version = EXCLUDED.version,
                node_id = EXCLUDED.node_id,
                grpc_address = EXCLUDED.grpc_address,
                object_category = EXCLUDED.object_category,
                health_status = EXCLUDED.health_status,
                last_heartbeat = EXCLUDED.last_heartbeat,
                updated_at = EXCLUDED.updated_at,
                alias = EXCLUDED.alias,
                max_heartbeat_failures = EXCLUDED.max_heartbeat_failures,
                heartbeat_failure_count = 0,
                registration_blob = EXCLUDED.registration_blob
            "#,
        )
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(&registration.object_id)
        .bind(registration.object_type)
        .bind(&registration.object_name)
        .bind(&registration.version)
        .bind(&registration.node_id)
        .bind(&registration.grpc_address)
        .bind(&registration.object_category)
        .bind(registration.health_status)
        .bind(last_heartbeat)
        .bind(now)
        .bind(now)
        .bind(alias_val)
        .bind(max_failures)
        .bind(&blob)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn get(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> RepositoryResult<Option<ObjectRegistration>> {
        // Fetch both blob and indexed columns (last_heartbeat, health_status may have been updated separately)
        let row = sqlx::query(
            r#"
            SELECT registration_blob, last_heartbeat, health_status,
                   heartbeat_failure_count, max_heartbeat_failures
            FROM object_registrations
            WHERE tenant_id = $1 AND namespace = $2 AND object_id = $3
            "#,
        )
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        match row {
            Some(row) => {
                let blob: Vec<u8> = row
                    .try_get("registration_blob")
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?;
                let mut registration = ObjectRegistration::decode(&blob[..])
                    .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

                // Merge indexed columns that may have been updated separately.
                let last_heartbeat: Option<chrono::DateTime<chrono::Utc>> =
                    row.try_get("last_heartbeat").unwrap_or(None);
                if let Some(ts) = last_heartbeat {
                    let indexed_secs = ts.timestamp();
                    let blob_seconds = registration
                        .last_heartbeat
                        .as_ref()
                        .map(|t| t.seconds)
                        .unwrap_or(0);
                    if blob_seconds != indexed_secs {
                        registration.last_heartbeat = Some(prost_types::Timestamp {
                            seconds: indexed_secs,
                            nanos: 0,
                        });
                    }
                }

                let health_status: i32 = row
                    .try_get("health_status")
                    .unwrap_or(registration.health_status);
                registration.health_status = health_status;

                let failure_count: i32 = row.try_get("heartbeat_failure_count").unwrap_or(0);
                registration.heartbeat_failure_count = failure_count as u32;

                let max_failures: i32 = row.try_get("max_heartbeat_failures").unwrap_or(3);
                if max_failures > 0 {
                    registration.max_heartbeat_failures = max_failures as u32;
                }

                Ok(Some(registration))
            }
            None => Ok(None),
        }
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn delete(&self, ctx: &RequestContext, object_id: &str) -> RepositoryResult<()> {
        sqlx::query(
            r#"
            DELETE FROM object_registrations
            WHERE tenant_id = $1 AND namespace = $2 AND object_id = $3
            "#,
        )
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(())
    }

    #[instrument(skip(self, ctx, filter), fields(tenant_id = %ctx.tenant_id()))]
    async fn discover(
        &self,
        ctx: &RequestContext,
        filter: &DiscoverFilter,
        offset: usize,
        limit: usize,
    ) -> RepositoryResult<Vec<ObjectRegistration>> {
        // Build dynamic WHERE clause with numbered parameters
        let mut conditions = vec!["tenant_id = $1".to_string(), "namespace = $2".to_string()];
        let mut param_count = 2;

        if filter.object_type.is_some() {
            param_count += 1;
            conditions.push(format!("object_type = ${}", param_count));
        }

        if filter.object_category.is_some() {
            param_count += 1;
            conditions.push(format!("object_category = ${}", param_count));
        }

        if filter.node_id.is_some() {
            param_count += 1;
            conditions.push(format!("node_id = ${}", param_count));
        }

        if filter.health_status.is_some() {
            param_count += 1;
            conditions.push(format!("health_status = ${}", param_count));
        }

        if filter.last_heartbeat_before.is_some() {
            param_count += 1;
            conditions.push(format!(
                "(last_heartbeat IS NULL OR last_heartbeat < ${})",
                param_count
            ));
        }

        if filter.last_heartbeat_after.is_some() {
            param_count += 1;
            conditions.push(format!("last_heartbeat > ${}", param_count));
        }

        let where_clause = conditions.join(" AND ");
        let query = format!(
            r#"
            SELECT registration_blob FROM object_registrations
            WHERE {}
            ORDER BY created_at ASC
            LIMIT ${} OFFSET ${}
            "#,
            where_clause,
            param_count + 1,
            param_count + 2
        );

        let mut query_builder = sqlx::query(&query)
            .bind(ctx.tenant_id())
            .bind(ctx.namespace());

        if let Some(obj_type) = filter.object_type.as_ref() {
            query_builder = query_builder.bind(obj_type.clone() as i32);
        }

        if let Some(ref category) = filter.object_category {
            query_builder = query_builder.bind(category);
        }

        if let Some(ref node_id) = filter.node_id {
            query_builder = query_builder.bind(node_id);
        }

        if let Some(status) = filter.health_status.as_ref() {
            query_builder = query_builder.bind(status.clone() as i32);
        }

        if let Some(before) = filter.last_heartbeat_before {
            let ts = chrono::DateTime::from_timestamp(before, 0).unwrap_or_else(chrono::Utc::now);
            query_builder = query_builder.bind(ts);
        }

        if let Some(after) = filter.last_heartbeat_after {
            let ts = chrono::DateTime::from_timestamp(after, 0).unwrap_or_else(chrono::Utc::now);
            query_builder = query_builder.bind(ts);
        }

        query_builder = query_builder.bind(limit as i64).bind(offset as i64);

        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let blob: Vec<u8> = row
                .try_get("registration_blob")
                .map_err(|e| RepositoryError::Storage(e.to_string()))?;
            let registration = ObjectRegistration::decode(&blob[..])
                .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

            // Post-filter for labels and capabilities
            if let Some(ref required_labels) = filter.labels {
                if !required_labels
                    .iter()
                    .all(|l| registration.labels.contains(l))
                {
                    continue;
                }
            }
            if let Some(ref required_caps) = filter.capabilities {
                if !required_caps
                    .iter()
                    .all(|c| registration.capabilities.contains(c))
                {
                    continue;
                }
            }

            results.push(registration);
        }

        Ok(results)
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn update_heartbeat(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        timestamp: i64,
    ) -> RepositoryResult<()> {
        let ts = chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_else(chrono::Utc::now);
        let now = Self::now_timestamp();

        let result = sqlx::query(
            r#"
            UPDATE object_registrations
            SET last_heartbeat = $1, updated_at = $2
            WHERE tenant_id = $3 AND namespace = $4 AND object_id = $5
            "#,
        )
        .bind(ts)
        .bind(now)
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::NotFound(object_id.to_string()));
        }

        Ok(())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn update_health_status(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        status: HealthStatus,
    ) -> RepositoryResult<()> {
        let now = Self::now_timestamp();

        let result = sqlx::query(
            r#"
            UPDATE object_registrations
            SET health_status = $1, updated_at = $2
            WHERE tenant_id = $3 AND namespace = $4 AND object_id = $5
            "#,
        )
        .bind(status as i32)
        .bind(now)
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(RepositoryError::NotFound(object_id.to_string()));
        }

        Ok(())
    }

    #[instrument(skip(self, ctx, filter), fields(tenant_id = %ctx.tenant_id()))]
    async fn count(
        &self,
        ctx: &RequestContext,
        filter: &DiscoverFilter,
    ) -> RepositoryResult<usize> {
        // Build dynamic WHERE clause with numbered parameters
        let mut conditions = vec!["tenant_id = $1".to_string(), "namespace = $2".to_string()];
        let mut param_count = 2;

        if filter.object_type.is_some() {
            param_count += 1;
            conditions.push(format!("object_type = ${}", param_count));
        }

        if filter.object_category.is_some() {
            param_count += 1;
            conditions.push(format!("object_category = ${}", param_count));
        }

        if filter.node_id.is_some() {
            param_count += 1;
            conditions.push(format!("node_id = ${}", param_count));
        }

        if filter.health_status.is_some() {
            param_count += 1;
            conditions.push(format!("health_status = ${}", param_count));
        }

        // Include heartbeat filters (same as discover)
        if filter.last_heartbeat_before.is_some() {
            param_count += 1;
            conditions.push(format!(
                "(last_heartbeat IS NULL OR last_heartbeat < ${})",
                param_count
            ));
        }

        if filter.last_heartbeat_after.is_some() {
            param_count += 1;
            conditions.push(format!("last_heartbeat > ${}", param_count));
        }

        let where_clause = conditions.join(" AND ");
        let query = format!(
            "SELECT COUNT(*) as cnt FROM object_registrations WHERE {}",
            where_clause
        );

        let mut query_builder = sqlx::query(&query)
            .bind(ctx.tenant_id())
            .bind(ctx.namespace());

        if let Some(obj_type) = filter.object_type.as_ref() {
            query_builder = query_builder.bind(obj_type.clone() as i32);
        }

        if let Some(ref category) = filter.object_category {
            query_builder = query_builder.bind(category);
        }

        if let Some(ref node_id) = filter.node_id {
            query_builder = query_builder.bind(node_id);
        }

        if let Some(status) = filter.health_status.as_ref() {
            query_builder = query_builder.bind(status.clone() as i32);
        }

        // Bind heartbeat filter values (convert Unix timestamp to DateTime for PostgreSQL)
        if let Some(before) = filter.last_heartbeat_before {
            let ts = chrono::DateTime::from_timestamp(before, 0).unwrap_or_else(chrono::Utc::now);
            query_builder = query_builder.bind(ts);
        }

        if let Some(after) = filter.last_heartbeat_after {
            let ts = chrono::DateTime::from_timestamp(after, 0).unwrap_or_else(chrono::Utc::now);
            query_builder = query_builder.bind(ts);
        }

        let row = query_builder
            .fetch_one(&self.pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let count: i64 = row
            .try_get("cnt")
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(count as usize)
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_type = ?object_type))]
    async fn list_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
        offset: usize,
        limit: usize,
    ) -> RepositoryResult<Vec<String>> {
        if ctx.auth_enabled && !ctx.is_admin() {
            return Ok((!ctx.tenant_id().is_empty())
                .then(|| ctx.tenant_id().to_string())
                .into_iter()
                .collect());
        }

        let mut conditions = vec!["object_type = $1".to_string()];
        let mut next_param = 1;
        let mut tenant_filter_index = None;
        let mut namespace_filter_index = None;

        // Admin sees all tenants — don't filter by their own tenant_id
        if !ctx.is_admin() && !ctx.tenant_id().is_empty() {
            next_param += 1;
            tenant_filter_index = Some(next_param);
            conditions.push(format!("tenant_id = ${next_param}"));
        }
        if !ctx.should_skip_namespace_filter() && !ctx.namespace().is_empty() {
            next_param += 1;
            namespace_filter_index = Some(next_param);
            conditions.push(format!("namespace = ${next_param}"));
        }

        let query = format!(
            "SELECT DISTINCT tenant_id FROM object_registrations WHERE {} ORDER BY tenant_id ASC LIMIT ${} OFFSET ${}",
            conditions.join(" AND ")
            ,
            next_param + 1,
            next_param + 2
        );

        let mut query_builder = sqlx::query(&query).bind(object_type as i32);
        if tenant_filter_index.is_some() {
            query_builder = query_builder.bind(ctx.tenant_id());
        }
        if namespace_filter_index.is_some() {
            query_builder = query_builder.bind(ctx.namespace());
        }
        query_builder = query_builder.bind(limit as i64).bind(offset as i64);

        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        rows.into_iter()
            .map(|row| {
                row.try_get("tenant_id")
                    .map_err(|e| RepositoryError::Storage(e.to_string()))
            })
            .collect()
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_type = ?object_type))]
    async fn count_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
    ) -> RepositoryResult<usize> {
        if ctx.auth_enabled && !ctx.is_admin() {
            return Ok((!ctx.tenant_id().is_empty()) as usize);
        }

        let mut conditions = vec!["object_type = $1".to_string()];
        let mut next_param = 1;
        let mut has_tenant_filter = false;
        let mut has_namespace_filter = false;

        // Admin sees all tenants — don't filter by their own tenant_id
        if !ctx.is_admin() && !ctx.tenant_id().is_empty() {
            next_param += 1;
            has_tenant_filter = true;
            conditions.push(format!("tenant_id = ${next_param}"));
        }
        if !ctx.should_skip_namespace_filter() && !ctx.namespace().is_empty() {
            next_param += 1;
            has_namespace_filter = true;
            conditions.push(format!("namespace = ${next_param}"));
        }

        let query = format!(
            "SELECT COUNT(DISTINCT tenant_id) AS cnt FROM object_registrations WHERE {}",
            conditions.join(" AND ")
        );
        let mut query_builder = sqlx::query(&query).bind(object_type as i32);
        if has_tenant_filter {
            query_builder = query_builder.bind(ctx.tenant_id());
        }
        if has_namespace_filter {
            query_builder = query_builder.bind(ctx.namespace());
        }

        let row = query_builder
            .fetch_one(&self.pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let count: i64 = row
            .try_get("cnt")
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(count as usize)
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn exists(&self, ctx: &RequestContext, object_id: &str) -> RepositoryResult<bool> {
        let row = sqlx::query(
            r#"
            SELECT 1 FROM object_registrations
            WHERE tenant_id = $1 AND namespace = $2 AND object_id = $3
            "#,
        )
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(row.is_some())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), alias = %alias))]
    async fn get_by_alias(
        &self,
        ctx: &RequestContext,
        alias: &str,
    ) -> RepositoryResult<Option<ObjectRegistration>> {
        if alias.is_empty() {
            return Ok(None);
        }
        let row = sqlx::query(
            r#"
            SELECT registration_blob, health_status,
                   heartbeat_failure_count, max_heartbeat_failures
            FROM object_registrations
            WHERE tenant_id = $1 AND namespace = $2 AND alias = $3
            "#,
        )
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(alias)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        match row {
            Some(row) => {
                let blob: Vec<u8> = row
                    .try_get("registration_blob")
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?;
                let health_status: i32 = row.try_get("health_status").unwrap_or(0);
                let failure_count: i32 = row.try_get("heartbeat_failure_count").unwrap_or(0);
                let max_failures: i32 = row.try_get("max_heartbeat_failures").unwrap_or(3);
                Ok(Some(Self::merge_indexed_row(
                    blob,
                    health_status,
                    failure_count,
                    max_failures,
                )?))
            }
            None => Ok(None),
        }
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
    async fn increment_heartbeat_failures(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> RepositoryResult<u32> {
        let now = Self::now_timestamp();
        let row = sqlx::query(
            r#"
            UPDATE object_registrations
            SET heartbeat_failure_count = heartbeat_failure_count + 1, updated_at = $1
            WHERE tenant_id = $2 AND namespace = $3 AND object_id = $4
            RETURNING heartbeat_failure_count
            "#,
        )
        .bind(now)
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        match row {
            Some(row) => {
                let count: i32 = row.try_get("heartbeat_failure_count").unwrap_or(1);
                Ok(count as u32)
            }
            None => Err(RepositoryError::NotFound(object_id.to_string())),
        }
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
    async fn reset_heartbeat_failures(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> RepositoryResult<()> {
        let now = Self::now_timestamp();
        sqlx::query(
            r#"
            UPDATE object_registrations
            SET heartbeat_failure_count = 0, updated_at = $1
            WHERE tenant_id = $2 AND namespace = $3 AND object_id = $4
            "#,
        )
        .bind(now)
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(object_id)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Mark all live objects on `node_id` as DEAD, scoped to tenant/namespace.
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), node_id = %node_id))]
    async fn mark_dead_by_node_id(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> RepositoryResult<u64> {
        let now = Self::now_timestamp();
        // DEAD=3, HEALTHY=1, DEGRADED=2, STARTING=4.  Scoped to tenant+namespace.
        let result = sqlx::query(
            r#"
            UPDATE object_registrations
            SET health_status = 3, updated_at = $1
            WHERE tenant_id = $2 AND namespace = $3 AND node_id = $4
              AND health_status IN (1, 2, 4)
            "#,
        )
        .bind(now)
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(node_id)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(result.rows_affected())
    }

    /// Find registrations with stale heartbeats, scoped to tenant/namespace or cross-tenant
    /// when called with an admin context and empty tenant_id (system heartbeat monitor).
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), threshold_seconds = %threshold_seconds, limit = %limit))]
    async fn find_stale_heartbeats(
        &self,
        ctx: &RequestContext,
        threshold_seconds: i64,
        limit: usize,
    ) -> RepositoryResult<Vec<ObjectRegistration>> {
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(threshold_seconds);

        let rows = if !ctx.tenant_id().is_empty() {
            sqlx::query(
                r#"
                SELECT registration_blob, health_status,
                       heartbeat_failure_count, max_heartbeat_failures
                FROM object_registrations
                WHERE tenant_id = $1 AND namespace = $2
                  AND health_status IN (1, 2)
                  AND (last_heartbeat IS NULL OR last_heartbeat < $3)
                LIMIT $4
                "#,
            )
            .bind(ctx.tenant_id())
            .bind(ctx.namespace())
            .bind(cutoff)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?
        } else {
            sqlx::query(
                r#"
                SELECT registration_blob, health_status,
                       heartbeat_failure_count, max_heartbeat_failures
                FROM object_registrations
                WHERE health_status IN (1, 2)
                  AND (last_heartbeat IS NULL OR last_heartbeat < $1)
                LIMIT $2
                "#,
            )
            .bind(cutoff)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?
        };

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let blob: Vec<u8> = row
                .try_get("registration_blob")
                .map_err(|e| RepositoryError::Storage(e.to_string()))?;
            let health_status: i32 = row.try_get("health_status").unwrap_or(1);
            let failure_count: i32 = row.try_get("heartbeat_failure_count").unwrap_or(0);
            let max_failures: i32 = row.try_get("max_heartbeat_failures").unwrap_or(3);
            results.push(Self::merge_indexed_row(
                blob,
                health_status,
                failure_count,
                max_failures,
            )?);
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_registration(object_id: &str, object_type: ObjectType) -> ObjectRegistration {
        ObjectRegistration {
            object_id: object_id.to_string(),
            object_type: object_type as i32,
            grpc_address: "http://test:8000".to_string(),
            tenant_id: "test-tenant".to_string(),
            namespace: "test-namespace".to_string(),
            object_category: "GenServer".to_string(),
            health_status: HealthStatus::HealthStatusHealthy as i32,
            created_at: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_sqlite_put_and_get() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );

        let reg = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        repo.put(&ctx, &reg).await.unwrap();

        let found = repo.get(&ctx, "actor-1").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().object_id, "actor-1");
    }

    #[tokio::test]
    async fn test_sqlite_delete() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );

        let reg = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        repo.put(&ctx, &reg).await.unwrap();

        repo.delete(&ctx, "actor-1").await.unwrap();

        let found = repo.get(&ctx, "actor-1").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_sqlite_discover_by_type() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );

        let actor = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        let service = create_test_registration("service-1", ObjectType::ObjectTypeService);

        repo.put(&ctx, &actor).await.unwrap();
        repo.put(&ctx, &service).await.unwrap();

        let filter = DiscoverFilter {
            object_type: Some(ObjectType::ObjectTypeActor),
            ..Default::default()
        };

        let results = repo.discover(&ctx, &filter, 0, 100).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].object_id, "actor-1");
    }

    #[tokio::test]
    async fn test_sqlite_discover_admin_without_namespace_returns_cross_namespace_results() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx_a = RequestContext::new_without_auth("tenant-a".to_string(), "ns-a".to_string());
        let ctx_b = RequestContext::new_without_auth("tenant-b".to_string(), "ns-b".to_string());
        let admin_ctx =
            RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);

        repo.put(
            &ctx_a,
            &create_test_registration("actor-a", ObjectType::ObjectTypeActor),
        )
        .await
        .unwrap();
        repo.put(
            &ctx_b,
            &create_test_registration("actor-b", ObjectType::ObjectTypeActor),
        )
        .await
        .unwrap();

        let results = repo
            .discover(
                &admin_ctx,
                &DiscoverFilter {
                    object_type: Some(ObjectType::ObjectTypeActor),
                    ..Default::default()
                },
                0,
                100,
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_sqlite_update_heartbeat() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );

        let reg = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        repo.put(&ctx, &reg).await.unwrap();

        let new_timestamp = 1234567890;
        repo.update_heartbeat(&ctx, "actor-1", new_timestamp)
            .await
            .unwrap();

        // Verify by checking the row exists (full heartbeat update verification
        // would require reading back the blob)
        assert!(repo.exists(&ctx, "actor-1").await.unwrap());
    }

    #[tokio::test]
    async fn test_sqlite_upsert() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );

        let mut reg = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        repo.put(&ctx, &reg).await.unwrap();

        // Update the registration
        reg.version = "2.0.0".to_string();
        repo.put(&ctx, &reg).await.unwrap();

        let found = repo.get(&ctx, "actor-1").await.unwrap().unwrap();
        assert_eq!(found.version, "2.0.0");

        // Should still be only one entry
        let count = repo.count(&ctx, &DiscoverFilter::default()).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_sqlite_count_admin_without_namespace_counts_cross_namespace_results() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx_a = RequestContext::new_without_auth("tenant-a".to_string(), "ns-a".to_string());
        let ctx_b = RequestContext::new_without_auth("tenant-b".to_string(), "ns-b".to_string());
        let admin_ctx =
            RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);

        repo.put(
            &ctx_a,
            &create_test_registration("actor-a", ObjectType::ObjectTypeActor),
        )
        .await
        .unwrap();
        repo.put(
            &ctx_b,
            &create_test_registration("actor-b", ObjectType::ObjectTypeActor),
        )
        .await
        .unwrap();

        let count = repo
            .count(
                &admin_ctx,
                &DiscoverFilter {
                    object_type: Some(ObjectType::ObjectTypeActor),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(count, 2);
    }

    // ---- New method tests ----

    #[tokio::test]
    async fn test_sqlite_get_by_alias_scoped_to_tenant() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx_a = RequestContext::new_without_auth("tenant-a".to_string(), "ns".to_string());
        let ctx_b = RequestContext::new_without_auth("tenant-b".to_string(), "ns".to_string());

        let mut reg = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        reg.alias = "Counter:worker:ns:tenant-a".to_string();
        repo.put(&ctx_a, &reg).await.unwrap();

        // Same alias is invisible to tenant-b
        let found_b = repo
            .get_by_alias(&ctx_b, "Counter:worker:ns:tenant-a")
            .await
            .unwrap();
        assert!(found_b.is_none());

        // Correct tenant can find it
        let found_a = repo
            .get_by_alias(&ctx_a, "Counter:worker:ns:tenant-a")
            .await
            .unwrap();
        assert!(found_a.is_some());
        assert_eq!(found_a.unwrap().object_id, "actor-1");
    }

    #[tokio::test]
    async fn test_sqlite_increment_and_reset_heartbeat_failures() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());

        let reg = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        repo.put(&ctx, &reg).await.unwrap();

        let c1 = repo
            .increment_heartbeat_failures(&ctx, "actor-1")
            .await
            .unwrap();
        assert_eq!(c1, 1);
        let c2 = repo
            .increment_heartbeat_failures(&ctx, "actor-1")
            .await
            .unwrap();
        assert_eq!(c2, 2);

        repo.reset_heartbeat_failures(&ctx, "actor-1")
            .await
            .unwrap();

        let found = repo.get(&ctx, "actor-1").await.unwrap().unwrap();
        assert_eq!(found.heartbeat_failure_count, 0);
    }

    #[tokio::test]
    async fn test_sqlite_mark_dead_by_node_id_scoped_to_tenant() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx_a = RequestContext::new_without_auth("tenant-a".to_string(), "ns".to_string());
        let ctx_b = RequestContext::new_without_auth("tenant-b".to_string(), "ns".to_string());

        // Register actor on node-1 for both tenants
        let mut reg_a = create_test_registration("actor-a", ObjectType::ObjectTypeActor);
        reg_a.node_id = "node-1".to_string();
        repo.put(&ctx_a, &reg_a).await.unwrap();

        let mut reg_b = create_test_registration("actor-b", ObjectType::ObjectTypeActor);
        reg_b.node_id = "node-1".to_string();
        repo.put(&ctx_b, &reg_b).await.unwrap();

        // Mark dead only for tenant-a
        let affected = repo.mark_dead_by_node_id(&ctx_a, "node-1").await.unwrap();
        assert_eq!(affected, 1);

        // tenant-a actor is DEAD
        let a = repo.get(&ctx_a, "actor-a").await.unwrap().unwrap();
        assert_eq!(a.health_status, HealthStatus::HealthStatusDead as i32);

        // tenant-b actor is still HEALTHY
        let b = repo.get(&ctx_b, "actor-b").await.unwrap().unwrap();
        assert_eq!(b.health_status, HealthStatus::HealthStatusHealthy as i32);
    }

    #[tokio::test]
    async fn test_sqlite_find_stale_heartbeats_tenant_scoped() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());
        let ctx_other = RequestContext::new_without_auth("t2".to_string(), "ns".to_string());

        // Actor with no heartbeat (stale by definition)
        let reg = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        repo.put(&ctx, &reg).await.unwrap();

        // Other tenant's actor — should not appear
        let reg2 = create_test_registration("actor-2", ObjectType::ObjectTypeActor);
        repo.put(&ctx_other, &reg2).await.unwrap();

        // threshold=0 means cutoff=now → any actor without a recent heartbeat qualifies
        let stale = repo.find_stale_heartbeats(&ctx, 0, 100).await.unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].object_id, "actor-1");
    }

    #[tokio::test]
    async fn test_sqlite_find_stale_cross_tenant_with_admin_ctx() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx_a = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());
        let ctx_b = RequestContext::new_without_auth("t2".to_string(), "ns".to_string());
        let admin = RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);

        let reg_a = create_test_registration("actor-a", ObjectType::ObjectTypeActor);
        repo.put(&ctx_a, &reg_a).await.unwrap();
        let reg_b = create_test_registration("actor-b", ObjectType::ObjectTypeActor);
        repo.put(&ctx_b, &reg_b).await.unwrap();

        // Admin with empty tenant sees all stale entries
        let stale = repo.find_stale_heartbeats(&admin, 0, 100).await.unwrap();
        assert_eq!(stale.len(), 2);
    }
}
