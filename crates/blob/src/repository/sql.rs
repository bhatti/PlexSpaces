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

//! SQL-based repository implementation for blob metadata

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Pool, Row};
use std::sync::Arc;

use crate::{BlobError, BlobResult};
use crate::helpers::{datetime_to_timestamp, timestamp_to_datetime};
use plexspaces_proto::storage::v1::BlobMetadata;
use plexspaces_core::RequestContext;
use super::{BlobRepository, ListFilters};

/// SQL-based blob metadata repository
pub struct SqlBlobRepository {
    pool: Arc<Pool<sqlx::Any>>,
}

impl SqlBlobRepository {
    /// Create new SQL repository with automatic migration
    /// This ensures migrations are automatically applied when the repository is created
    /// For in-memory SQLite (sqlite::memory:), migrations are always applied
    /// 
    /// IMPORTANT: For in-memory SQLite, ensure the pool uses max_connections=1
    /// to avoid connection-specific database issues
    pub async fn new(pool: Pool<sqlx::Any>) -> Result<Self, sqlx::Error> {
        // Auto-apply migrations using the pool
        // For in-memory SQLite with max_connections=1, all operations use the same connection
        Self::migrate(&pool).await?;
        
        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    /// Run migrations to create blob_metadata table
    /// Detects database type and uses appropriate migration path
    /// For in-memory SQLite, uses a single connection for all operations to ensure consistency
    async fn migrate(pool: &Pool<sqlx::Any>) -> Result<(), sqlx::Error> {
        use tracing::{debug, error, info, warn};
        
        info!("[BLOB_MIGRATION] Starting migration for blob_metadata table");
        
        // Try to detect database type first
        let is_sqlite = match sqlx::query_scalar::<_, String>("SELECT sqlite_version()")
            .fetch_optional(pool)
            .await
        {
            Ok(Some(version)) => {
                info!("[BLOB_MIGRATION] SQLite detected, version: {}", version);
                true
            }
            Ok(None) => {
                warn!("[BLOB_MIGRATION] sqlite_version() returned None, assuming SQLite");
                true
            }
            Err(e) => {
                debug!("[BLOB_MIGRATION] sqlite_version() failed: {}, assuming PostgreSQL", e);
                false
            }
        };
        
        if is_sqlite {
            info!("[BLOB_MIGRATION] Running SQLite migrations");
            
            // For SQLite (especially in-memory), use a single connection for all operations
            // This ensures that CREATE TABLE and CREATE INDEX see the same database state
            let mut conn = pool.acquire().await.map_err(|e| {
                error!("[BLOB_MIGRATION] Failed to acquire connection: {}", e);
                e
            })?;
            
            // Create table
            info!("[BLOB_MIGRATION] Creating blob_metadata table...");
            match sqlx::query(
                r#"
                CREATE TABLE IF NOT EXISTS blob_metadata (
                    blob_id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL,
                    namespace TEXT NOT NULL,
                    name TEXT NOT NULL,
                    sha256 TEXT NOT NULL,
                    content_type TEXT,
                    content_length INTEGER NOT NULL,
                    etag TEXT,
                    blob_group TEXT,
                    kind TEXT,
                    metadata_json TEXT,
                    tags_json TEXT,
                    expires_at INTEGER,
                    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
                )
                "#,
            )
            .execute(&mut *conn)
            .await
            {
                Ok(result) => {
                    info!("[BLOB_MIGRATION] CREATE TABLE executed successfully, rows_affected: {}", result.rows_affected());
                }
                Err(e) => {
                    error!("[BLOB_MIGRATION] CREATE TABLE failed: {}", e);
                    return Err(e);
                }
            }

            // Create indexes using the same connection
            info!("[BLOB_MIGRATION] Creating indexes...");
            sqlx::query(
                r#"
                CREATE INDEX IF NOT EXISTS idx_blob_metadata_tenant_namespace
                ON blob_metadata(tenant_id, namespace)
                "#,
            )
            .execute(&mut *conn)
            .await
            .map_err(|e| {
                error!("[BLOB_MIGRATION] Failed to create idx_blob_metadata_tenant_namespace: {}", e);
                e
            })?;

            sqlx::query(
                r#"
                CREATE INDEX IF NOT EXISTS idx_blob_metadata_sha256
                ON blob_metadata(tenant_id, namespace, sha256)
                "#,
            )
            .execute(&mut *conn)
            .await
            .map_err(|e| {
                error!("[BLOB_MIGRATION] Failed to create idx_blob_metadata_sha256: {}", e);
                e
            })?;

            sqlx::query(
                r#"
                CREATE INDEX IF NOT EXISTS idx_blob_metadata_expires_at
                ON blob_metadata(expires_at)
                WHERE expires_at IS NOT NULL
                "#,
            )
            .execute(&mut *conn)
            .await
            .map_err(|e| {
                error!("[BLOB_MIGRATION] Failed to create idx_blob_metadata_expires_at: {}", e);
                e
            })?;

            info!("[BLOB_MIGRATION] SQLite migrations completed successfully");
            Ok(())
        } else {
            info!("[BLOB_MIGRATION] Running PostgreSQL migrations");
            sqlx::migrate!("./migrations/postgres")
                .run(pool)
                .await
                .map_err(|e| {
                    error!("[BLOB_MIGRATION] PostgreSQL migration failed: {}", e);
                    e
                })?;
            info!("[BLOB_MIGRATION] PostgreSQL migrations completed successfully");
            Ok(())
        }
    }
}

#[async_trait]
impl BlobRepository for SqlBlobRepository {
    async fn get(&self, ctx: &RequestContext, blob_id: &str) -> BlobResult<Option<BlobMetadata>> {
        // For internal context (empty tenant_id), look up by blob_id only
        // This allows system operations to find blobs across tenants
        let row = if ctx.is_internal() || ctx.tenant_id().is_empty() {
            sqlx::query(
                r#"
                SELECT blob_id, tenant_id, namespace, name, sha256, content_type,
                       content_length, etag, blob_group, kind, metadata_json, tags_json,
                       expires_at, created_at, updated_at
                FROM blob_metadata
                WHERE blob_id = $1
                "#,
            )
            .bind(blob_id)
            .fetch_optional(&*self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT blob_id, tenant_id, namespace, name, sha256, content_type,
                       content_length, etag, blob_group, kind, metadata_json, tags_json,
                       expires_at, created_at, updated_at
                FROM blob_metadata
                WHERE blob_id = $1 AND tenant_id = $2 AND namespace = $3
                "#,
            )
            .bind(blob_id)
            .bind(ctx.tenant_id())
            .bind(ctx.namespace())
            .fetch_optional(&*self.pool)
            .await?
        };

        if let Some(row) = row {
            Ok(Some(row_to_metadata(&row)?))
        } else {
            Ok(None)
        }
    }

    async fn get_by_sha256(
        &self,
        ctx: &RequestContext,
        sha256: &str,
    ) -> BlobResult<Option<BlobMetadata>> {
        // Filter by namespace only if it's non-empty
        let row = if ctx.namespace().is_empty() {
            sqlx::query(
                r#"
                SELECT blob_id, tenant_id, namespace, name, sha256, content_type,
                       content_length, etag, blob_group, kind, metadata_json, tags_json,
                       expires_at, created_at, updated_at
                FROM blob_metadata
                WHERE tenant_id = $1 AND sha256 = $2
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            )
            .bind(ctx.tenant_id())
            .bind(sha256)
            .fetch_optional(&*self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT blob_id, tenant_id, namespace, name, sha256, content_type,
                       content_length, etag, blob_group, kind, metadata_json, tags_json,
                       expires_at, created_at, updated_at
                FROM blob_metadata
                WHERE tenant_id = $1 AND namespace = $2 AND sha256 = $3
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            )
            .bind(ctx.tenant_id())
            .bind(ctx.namespace())
            .bind(sha256)
            .fetch_optional(&*self.pool)
            .await?
        };

        if let Some(row) = row {
            Ok(Some(row_to_metadata(&row)?))
        } else {
            Ok(None)
        }
    }

    async fn save(&self, ctx: &RequestContext, metadata: &BlobMetadata) -> BlobResult<()> {
        // Validate tenant_id and namespace match context
        if metadata.tenant_id != ctx.tenant_id() {
            return Err(BlobError::InvalidInput(format!(
                "Metadata tenant_id '{}' does not match context tenant_id '{}'",
                metadata.tenant_id,
                ctx.tenant_id()
            )));
        }
        if metadata.namespace != ctx.namespace() {
            return Err(BlobError::InvalidInput(format!(
                "Metadata namespace '{}' does not match context namespace '{}'",
                metadata.namespace,
                ctx.namespace()
            )));
        }

        let metadata_json = serde_json::to_string(&metadata.metadata)?;
        let tags_json = serde_json::to_string(&metadata.tags)?;

        let expires_at = metadata.expires_at.as_ref()
            .and_then(|ts| timestamp_to_datetime(Some(ts.clone())))
            .map(|dt| dt.to_rfc3339());
        let created_at = metadata.created_at.as_ref()
            .and_then(|ts| timestamp_to_datetime(Some(ts.clone())))
            .unwrap_or_else(Utc::now)
            .to_rfc3339();
        let updated_at = metadata.updated_at.as_ref()
            .and_then(|ts| timestamp_to_datetime(Some(ts.clone())))
            .unwrap_or_else(Utc::now)
            .to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO blob_metadata (
                blob_id, tenant_id, namespace, name, sha256, content_type,
                content_length, etag, blob_group, kind, metadata_json, tags_json,
                expires_at, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            "#,
        )
        .bind(&metadata.blob_id)
        .bind(&metadata.tenant_id)
        .bind(&metadata.namespace)
        .bind(&metadata.name)
        .bind(&metadata.sha256)
        .bind(&metadata.content_type)
        .bind(metadata.content_length)
        .bind(&metadata.etag)
        .bind(&metadata.blob_group)
        .bind(&metadata.kind)
        .bind(&metadata_json)
        .bind(&tags_json)
        .bind(expires_at.as_ref().map(|s| s.as_str()))
        .bind(&created_at)
        .bind(&updated_at)
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    async fn update(&self, ctx: &RequestContext, metadata: &BlobMetadata) -> BlobResult<()> {
        // Validate tenant_id and namespace match context
        if metadata.tenant_id != ctx.tenant_id() {
            return Err(BlobError::InvalidInput(format!(
                "Metadata tenant_id '{}' does not match context tenant_id '{}'",
                metadata.tenant_id,
                ctx.tenant_id()
            )));
        }
        if metadata.namespace != ctx.namespace() {
            return Err(BlobError::InvalidInput(format!(
                "Metadata namespace '{}' does not match context namespace '{}'",
                metadata.namespace,
                ctx.namespace()
            )));
        }

        let metadata_json = serde_json::to_string(&metadata.metadata)?;
        let tags_json = serde_json::to_string(&metadata.tags)?;
        let updated_at = Utc::now().to_rfc3339();

        let expires_at = metadata.expires_at.as_ref()
            .and_then(|ts| timestamp_to_datetime(Some(ts.clone())))
            .map(|dt| dt.to_rfc3339());

        sqlx::query(
            r#"
            UPDATE blob_metadata
            SET name = $2, content_type = $3, content_length = $4, etag = $5,
                blob_group = $6, kind = $7, metadata_json = $8, tags_json = $9,
                expires_at = $10, updated_at = $11
            WHERE blob_id = $1 AND tenant_id = $12 AND namespace = $13
            "#,
        )
        .bind(&metadata.blob_id)
        .bind(&metadata.name)
        .bind(&metadata.content_type)
        .bind(metadata.content_length)
        .bind(&metadata.etag)
        .bind(&metadata.blob_group)
        .bind(&metadata.kind)
        .bind(&metadata_json)
        .bind(&tags_json)
        .bind(expires_at.as_ref().map(|s| s.as_str()))
        .bind(&updated_at)
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    async fn delete(&self, ctx: &RequestContext, blob_id: &str) -> BlobResult<()> {
        // For internal context (empty tenant_id), delete by blob_id only
        // This allows system operations to delete blobs across tenants
        if ctx.is_internal() || ctx.tenant_id().is_empty() {
            sqlx::query("DELETE FROM blob_metadata WHERE blob_id = $1")
                .bind(blob_id)
                .execute(&*self.pool)
                .await?;
        } else {
            sqlx::query("DELETE FROM blob_metadata WHERE blob_id = $1 AND tenant_id = $2 AND namespace = $3")
                .bind(blob_id)
                .bind(ctx.tenant_id())
                .bind(ctx.namespace())
                .execute(&*self.pool)
                .await?;
        }

        Ok(())
    }

    async fn list(
        &self,
        ctx: &RequestContext,
        filters: &ListFilters,
        page_size: i64,
        offset: i64,
    ) -> BlobResult<(Vec<BlobMetadata>, i64)> {
        // Build WHERE clause - filter by tenant_id always, namespace only if non-empty
        let mut where_clauses: Vec<String> = vec!["tenant_id = $1".to_string()];
        let mut bind_index = 2;
        
        // Add namespace filter only if non-empty
        if !ctx.namespace().is_empty() {
            where_clauses.push(format!("namespace = ${}", bind_index));
            bind_index += 1;
        }

        if let Some(ref name_prefix) = filters.name_prefix {
            where_clauses.push(format!("name LIKE ${}", bind_index));
            bind_index += 1;
        }
        if filters.blob_group.is_some() {
            where_clauses.push(format!("blob_group = ${}", bind_index));
            bind_index += 1;
        }
        if filters.kind.is_some() {
            where_clauses.push(format!("kind = ${}", bind_index));
            bind_index += 1;
        }
        if filters.sha256.is_some() {
            where_clauses.push(format!("sha256 = ${}", bind_index));
            bind_index += 1;
        }

        let where_clause = where_clauses.join(" AND ");

        // Count total
        let count_query = format!("SELECT COUNT(*) FROM blob_metadata WHERE {}", where_clause);
        let mut count_query = sqlx::query(&count_query)
            .bind(ctx.tenant_id())
            .bind(ctx.namespace());

        if let Some(ref name_prefix) = filters.name_prefix {
            count_query = count_query.bind(format!("{}%", name_prefix));
        }
        if let Some(ref blob_group) = filters.blob_group {
            count_query = count_query.bind(blob_group);
        }
        if let Some(ref kind) = filters.kind {
            count_query = count_query.bind(kind);
        }
        if let Some(ref sha256) = filters.sha256 {
            count_query = count_query.bind(sha256);
        }

        let total_count: i64 = count_query
            .fetch_one(&*self.pool)
            .await?
            .get(0);

        // Fetch results
        let list_query = format!(
            r#"
            SELECT blob_id, tenant_id, namespace, name, sha256, content_type,
                   content_length, etag, blob_group, kind, metadata_json, tags_json,
                   expires_at, created_at, updated_at
            FROM blob_metadata
            WHERE {}
            ORDER BY created_at DESC
            LIMIT ${} OFFSET ${}
            "#,
            where_clause, bind_index, bind_index + 1
        );

        let mut list_query = sqlx::query(&list_query)
            .bind(ctx.tenant_id());
        
        // Bind namespace only if non-empty
        if !ctx.namespace().is_empty() {
            list_query = list_query.bind(ctx.namespace());
        }

        if let Some(ref name_prefix) = filters.name_prefix {
            list_query = list_query.bind(format!("{}%", name_prefix));
        }
        if let Some(ref blob_group) = filters.blob_group {
            list_query = list_query.bind(blob_group);
        }
        if let Some(ref kind) = filters.kind {
            list_query = list_query.bind(kind);
        }
        if let Some(ref sha256) = filters.sha256 {
            list_query = list_query.bind(sha256);
        }

        list_query = list_query.bind(page_size).bind(offset);

        let rows = list_query.fetch_all(&*self.pool).await?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row_to_metadata(&row)?);
        }

        Ok((results, total_count))
    }

    async fn find_expired(
        &self,
        ctx: &RequestContext,
        limit: i64,
    ) -> BlobResult<Vec<BlobMetadata>> {
        let now_str = Utc::now().to_rfc3339();

        // Filter by namespace only if it's non-empty
        let rows = if ctx.namespace().is_empty() {
            sqlx::query(
                r#"
                SELECT blob_id, tenant_id, namespace, name, sha256, content_type,
                       content_length, etag, blob_group, kind, metadata_json, tags_json,
                       expires_at, created_at, updated_at
                FROM blob_metadata
                WHERE tenant_id = $1 AND expires_at IS NOT NULL AND expires_at < $2
                ORDER BY expires_at ASC
                LIMIT $3
                "#,
            )
            .bind(ctx.tenant_id())
            .bind(&now_str)
            .bind(limit)
            .fetch_all(&*self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT blob_id, tenant_id, namespace, name, sha256, content_type,
                       content_length, etag, blob_group, kind, metadata_json, tags_json,
                       expires_at, created_at, updated_at
                FROM blob_metadata
                WHERE tenant_id = $1 AND namespace = $2 AND expires_at IS NOT NULL AND expires_at < $3
                ORDER BY expires_at ASC
                LIMIT $4
                "#,
            )
            .bind(ctx.tenant_id())
            .bind(ctx.namespace())
            .bind(&now_str)
            .bind(limit)
            .fetch_all(&*self.pool)
            .await?
        };

        let mut results = Vec::new();
        for row in rows {
            results.push(row_to_metadata(&row)?);
        }

        Ok(results)
    }
}

fn row_to_metadata<R: sqlx::Row>(row: &R) -> BlobResult<BlobMetadata>
where
    for<'r> &'r str: sqlx::ColumnIndex<R>,
    for<'r> String: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
    for<'r> Option<String>: sqlx::Decode<'r, R::Database>,
    for<'r> i64: sqlx::Decode<'r, R::Database> + sqlx::Type<R::Database>,
{
    use std::collections::HashMap;

    let metadata_json: Option<String> = row.try_get("metadata_json").ok().flatten();
    let tags_json: Option<String> = row.try_get("tags_json").ok().flatten();

    let metadata: HashMap<String, String> = if let Some(ref json) = metadata_json {
        serde_json::from_str(json).unwrap_or_default()
    } else {
        HashMap::new()
    };

    let tags: HashMap<String, String> = if let Some(ref json) = tags_json {
        serde_json::from_str(json).unwrap_or_default()
    } else {
        HashMap::new()
    };

    // Handle timestamps - sqlx doesn't support DateTime directly, so we store as strings or i64
    // For now, we'll read as Option<String> and parse
    let expires_at_str: Option<String> = row.try_get("expires_at").ok().flatten();
    let created_at_str: String = row.try_get("created_at")?;
    let updated_at_str: String = row.try_get("updated_at")?;
    
    let expires_at = expires_at_str.as_ref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| BlobError::InternalError(format!("Failed to parse created_at: {}", e)))?
        .with_timezone(&Utc);
    
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
        .map_err(|e| BlobError::InternalError(format!("Failed to parse updated_at: {}", e)))?
        .with_timezone(&Utc);

    Ok(BlobMetadata {
        blob_id: row.try_get("blob_id")?,
        tenant_id: row.try_get("tenant_id")?,
        namespace: row.try_get("namespace")?,
        name: row.try_get("name")?,
        sha256: row.try_get("sha256")?,
        content_type: row.try_get("content_type").unwrap_or_default(),
        content_length: row.try_get("content_length")?,
        etag: row.try_get("etag").unwrap_or_default(),
        blob_group: row.try_get("blob_group").unwrap_or_default(),
        kind: row.try_get("kind").unwrap_or_default(),
        metadata,
        tags,
        expires_at: expires_at.map(datetime_to_timestamp),
        created_at: Some(datetime_to_timestamp(created_at)),
        updated_at: Some(datetime_to_timestamp(updated_at)),
    })
}
