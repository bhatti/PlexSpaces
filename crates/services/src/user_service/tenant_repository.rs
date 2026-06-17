// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Tenant repository — storage abstraction for first-class tenant records.

use async_trait::async_trait;
use plexspaces_proto::security::v1::Tenant;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::sync::atomic::{AtomicI32, Ordering};
use ulid::Ulid;

#[async_trait]
pub trait TenantRepository: Send + Sync + 'static {
    /// Create a tenant for the given slug if one does not already exist.
    /// Returns the tenant (existing or newly created) and whether it was created.
    async fn get_or_create_by_slug(
        &self,
        slug: &str,
        display_name: &str,
    ) -> Result<(Tenant, bool), TenantRepositoryError>;

    /// Return a single tenant by primary key, or None if not found.
    async fn get_tenant(
        &self,
        tenant_id: &str,
    ) -> Result<Option<Tenant>, TenantRepositoryError>;

    /// Paginated list of all tenants.
    async fn list_tenants(
        &self,
        offset: i32,
        limit: i32,
    ) -> Result<(Vec<Tenant>, i32), TenantRepositoryError>;
}

#[derive(Debug, thiserror::Error)]
pub enum TenantRepositoryError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Not found: {0}")]
    NotFound(String),
}

pub struct SqlTenantRepository {
    pool: SqlitePool,
    // Cached total count; -1 means uninitialized (will be loaded on first list_tenants call).
    cached_total: AtomicI32,
}

impl SqlTenantRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool, cached_total: AtomicI32::new(-1) }
    }

    fn tenant_from_row(row: &sqlx::sqlite::SqliteRow) -> Tenant {
        let created_at: Option<i64> = row.get("created_at");
        let updated_at: Option<i64> = row.get("updated_at");
        Tenant {
            tenant_id: row.get("tenant_id"),
            slug: row.get("slug"),
            display_name: row.get("display_name"),
            created_at: created_at
                .filter(|&t| t > 0)
                .map(|t| prost_types::Timestamp { seconds: t, nanos: 0 }),
            updated_at: updated_at
                .filter(|&t| t > 0)
                .map(|t| prost_types::Timestamp { seconds: t, nanos: 0 }),
        }
    }
}

#[async_trait]
impl TenantRepository for SqlTenantRepository {
    async fn get_or_create_by_slug(
        &self,
        slug: &str,
        display_name: &str,
    ) -> Result<(Tenant, bool), TenantRepositoryError> {
        let existing = sqlx::query("SELECT * FROM tenants WHERE slug = ?")
            .bind(slug)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| TenantRepositoryError::Database(e.to_string()))?;

        if let Some(row) = existing {
            return Ok((Self::tenant_from_row(&row), false));
        }

        let tenant_id = Ulid::new().to_string();
        sqlx::query(
            "INSERT INTO tenants (tenant_id, slug, display_name) VALUES (?, ?, ?)",
        )
        .bind(&tenant_id)
        .bind(slug)
        .bind(display_name)
        .execute(&self.pool)
        .await
        .map_err(|e| TenantRepositoryError::Database(e.to_string()))?;

        // Invalidate cached count so next list_tenants picks up the new row.
        self.cached_total.store(-1, Ordering::Relaxed);

        let row = sqlx::query("SELECT * FROM tenants WHERE tenant_id = ?")
            .bind(&tenant_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| TenantRepositoryError::Database(e.to_string()))?;

        Ok((Self::tenant_from_row(&row), true))
    }

    async fn get_tenant(
        &self,
        tenant_id: &str,
    ) -> Result<Option<Tenant>, TenantRepositoryError> {
        // Search by primary key first, then by slug (JWT tenant_id claim is the slug).
        let row = sqlx::query("SELECT * FROM tenants WHERE tenant_id = ? OR slug = ?")
            .bind(tenant_id)
            .bind(tenant_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| TenantRepositoryError::Database(e.to_string()))?;

        Ok(row.map(|r| Self::tenant_from_row(&r)))
    }

    async fn list_tenants(
        &self,
        offset: i32,
        limit: i32,
    ) -> Result<(Vec<Tenant>, i32), TenantRepositoryError> {
        let mut total = self.cached_total.load(Ordering::Relaxed);
        if total < 0 {
            total = sqlx::query_scalar("SELECT COUNT(*) FROM tenants")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| TenantRepositoryError::Database(e.to_string()))?;
            self.cached_total.store(total, Ordering::Relaxed);
        }

        let rows = sqlx::query(
            "SELECT * FROM tenants ORDER BY created_at DESC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| TenantRepositoryError::Database(e.to_string()))?;

        let tenants = rows.iter().map(Self::tenant_from_row).collect();
        Ok((tenants, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE tenants (
                tenant_id TEXT PRIMARY KEY,
                slug TEXT NOT NULL UNIQUE,
                display_name TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn test_get_or_create_creates_new_tenant() {
        let pool = setup().await;
        let repo = SqlTenantRepository::new(pool);
        let (tenant, created) = repo
            .get_or_create_by_slug("acme", "Acme Corp")
            .await
            .unwrap();
        assert!(created);
        assert_eq!(tenant.slug, "acme");
        assert_eq!(tenant.display_name, "Acme Corp");
        assert!(!tenant.tenant_id.is_empty());
    }

    #[tokio::test]
    async fn test_get_or_create_returns_existing_tenant() {
        let pool = setup().await;
        let repo = SqlTenantRepository::new(pool);
        let (t1, _) = repo.get_or_create_by_slug("acme", "Acme Corp").await.unwrap();
        let (t2, created) = repo.get_or_create_by_slug("acme", "Different").await.unwrap();
        assert!(!created);
        assert_eq!(t1.tenant_id, t2.tenant_id);
    }

    #[tokio::test]
    async fn test_list_tenants_paginated() {
        let pool = setup().await;
        let repo = SqlTenantRepository::new(pool);
        repo.get_or_create_by_slug("a", "A").await.unwrap();
        repo.get_or_create_by_slug("b", "B").await.unwrap();
        repo.get_or_create_by_slug("c", "C").await.unwrap();
        let (tenants, total) = repo.list_tenants(0, 2).await.unwrap();
        assert_eq!(total, 3);
        assert_eq!(tenants.len(), 2);
    }

    #[tokio::test]
    async fn test_get_tenant_not_found() {
        let pool = setup().await;
        let repo = SqlTenantRepository::new(pool);
        let result = repo.get_tenant("nonexistent").await.unwrap();
        assert!(result.is_none());
    }
}
