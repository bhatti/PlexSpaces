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
use plexspaces_common::RequestContext;
use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};
use prost::Message;
use sqlx::{Pool, Postgres, Row, Sqlite};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, instrument};

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
            .max_connections(5)
            .connect(&url)
            .await
            .map_err(|e| RepositoryError::Connection(e.to_string()))?;

        // Run migrations
        Self::run_migrations(&pool).await?;

        Ok(Self { pool })
    }

    /// Run SQLite migrations
    async fn run_migrations(pool: &Pool<Sqlite>) -> RepositoryResult<()> {
        let migration_sql = include_str!("../../migrations/sqlite/001_object_registrations.up.sql");
        
        sqlx::raw_sql(migration_sql)
            .execute(pool)
            .await
            .map_err(|e| RepositoryError::Storage(format!("Migration failed: {}", e)))?;

        debug!("Object registry SQLite migrations completed");
        Ok(())
    }

    /// Get current Unix timestamp
    fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
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
        let last_heartbeat = registration
            .last_heartbeat
            .as_ref()
            .map(|t| t.seconds);

        sqlx::query(
            r#"
            INSERT INTO object_registrations (
                tenant_id, namespace, object_id, object_type, object_name, version,
                node_id, grpc_address, object_category, health_status,
                last_heartbeat, created_at, updated_at, registration_blob
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
            SELECT registration_blob, last_heartbeat, health_status FROM object_registrations
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
                
                // Merge indexed columns that may have been updated separately (heartbeat optimization)
                let last_heartbeat: Option<i64> = row
                    .try_get("last_heartbeat")
                    .unwrap_or(None);
                if let Some(ts) = last_heartbeat {
                    registration.last_heartbeat = Some(prost_types::Timestamp {
                        seconds: ts,
                        nanos: 0,
                    });
                }
                
                // Also merge health_status from indexed column
                let health_status: i32 = row
                    .try_get("health_status")
                    .unwrap_or(registration.health_status);
                registration.health_status = health_status;
                
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
        let mut conditions = vec!["tenant_id = ?".to_string(), "namespace = ?".to_string()];
        let mut bindings: Vec<String> = vec![
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
        ];

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

        let where_clause = conditions.join(" AND ");
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
        let mut conditions = vec!["tenant_id = ?".to_string(), "namespace = ?".to_string()];
        let mut bindings: Vec<String> = vec![
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
        ];

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

        let where_clause = conditions.join(" AND ");
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

        // Run migrations
        Self::run_migrations(&pool).await?;

        Ok(Self { pool })
    }

    /// Run PostgreSQL migrations
    async fn run_migrations(pool: &Pool<Postgres>) -> RepositoryResult<()> {
        let migration_sql =
            include_str!("../../migrations/postgres/001_object_registrations.up.sql");

        sqlx::raw_sql(migration_sql)
            .execute(pool)
            .await
            .map_err(|e| RepositoryError::Storage(format!("Migration failed: {}", e)))?;

        debug!("Object registry PostgreSQL migrations completed");
        Ok(())
    }

    /// Get current timestamp for PostgreSQL
    fn now_timestamp() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
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

        sqlx::query(
            r#"
            INSERT INTO object_registrations (
                tenant_id, namespace, object_id, object_type, object_name, version,
                node_id, grpc_address, object_category, health_status,
                last_heartbeat, created_at, updated_at, registration_blob
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
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
            SELECT registration_blob, last_heartbeat, health_status FROM object_registrations
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
                
                // Merge indexed columns that may have been updated separately (heartbeat optimization)
                let last_heartbeat: Option<chrono::DateTime<chrono::Utc>> = row
                    .try_get("last_heartbeat")
                    .unwrap_or(None);
                if let Some(ts) = last_heartbeat {
                    registration.last_heartbeat = Some(prost_types::Timestamp {
                        seconds: ts.timestamp(),
                        nanos: 0,
                    });
                }
                
                // Also merge health_status from indexed column
                let health_status: i32 = row
                    .try_get("health_status")
                    .unwrap_or(registration.health_status);
                registration.health_status = health_status;
                
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
            let ts = chrono::DateTime::from_timestamp(before, 0)
                .unwrap_or_else(chrono::Utc::now);
            query_builder = query_builder.bind(ts);
        }

        if let Some(after) = filter.last_heartbeat_after {
            let ts = chrono::DateTime::from_timestamp(after, 0)
                .unwrap_or_else(chrono::Utc::now);
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
        let ts = chrono::DateTime::from_timestamp(timestamp, 0)
            .unwrap_or_else(chrono::Utc::now);
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
            conditions.push(format!("(last_heartbeat IS NULL OR last_heartbeat < ${})", param_count));
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
            let ts = chrono::DateTime::from_timestamp(before, 0)
                .unwrap_or_else(chrono::Utc::now);
            query_builder = query_builder.bind(ts);
        }

        if let Some(after) = filter.last_heartbeat_after {
            let ts = chrono::DateTime::from_timestamp(after, 0)
                .unwrap_or_else(chrono::Utc::now);
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
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());

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
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());

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
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());

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
    async fn test_sqlite_update_heartbeat() {
        let repo = SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap();
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());

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
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());

        let mut reg = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        repo.put(&ctx, &reg).await.unwrap();

        // Update the registration
        reg.version = "2.0.0".to_string();
        repo.put(&ctx, &reg).await.unwrap();

        let found = repo.get(&ctx, "actor-1").await.unwrap().unwrap();
        assert_eq!(found.version, "2.0.0");

        // Should still be only one entry
        let count = repo
            .count(&ctx, &DiscoverFilter::default())
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
}
