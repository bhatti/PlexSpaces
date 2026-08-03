// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! User repository — storage abstraction for OAuth user records.

use async_trait::async_trait;
use plexspaces_proto::security::v1::{GetOrCreateByEmailRequest, UpdateUserRequest, User};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

#[async_trait]
pub trait UserRepository: Send + Sync + 'static {
    async fn get_or_create_by_email(
        &self,
        req: &GetOrCreateByEmailRequest,
    ) -> Result<(User, bool), UserRepositoryError>;

    async fn update_user(&self, req: &UpdateUserRequest) -> Result<User, UserRepositoryError>;

    /// Look up a user by their primary key (user_id).
    async fn find_by_id(&self, user_id: &str) -> Result<Option<User>, UserRepositoryError>;

    /// Paginated user listing. Pass `tenant_filter = None` to return all tenants (admin only).
    async fn list_users(
        &self,
        tenant_filter: Option<&str>,
        offset: i32,
        limit: i32,
    ) -> Result<(Vec<User>, i32), UserRepositoryError>;
}

#[derive(Debug, thiserror::Error)]
pub enum UserRepositoryError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Not found: {0}")]
    NotFound(String),
}

pub struct SqlUserRepository {
    pool: SqlitePool,
}

impl SqlUserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn unix_to_timestamp(ts: Option<i64>) -> Option<prost_types::Timestamp> {
        ts.filter(|&t| t > 0).map(|t| prost_types::Timestamp {
            seconds: t,
            nanos: 0,
        })
    }

    fn user_from_row(row: &sqlx::sqlite::SqliteRow) -> User {
        let roles_json: String = row.get("roles_json");
        let groups_json: String = row.get("groups_json");
        let roles: Vec<String> = serde_json::from_str(&roles_json).unwrap_or_default();
        let groups: Vec<String> = serde_json::from_str(&groups_json).unwrap_or_default();
        let last_login: Option<i64> = row.get("last_login");
        let created_at: Option<i64> = row.get("created_at");
        let updated_at: Option<i64> = row.get("updated_at");

        User {
            user_id: row.get("user_id"),
            email: row.get("email"),
            tenant_id: row.get("tenant_id"),
            display_name: row.get("display_name"),
            admin: row.get::<i32, _>("admin") != 0,
            last_login: Self::unix_to_timestamp(last_login),
            created_at: Self::unix_to_timestamp(created_at),
            updated_at: Self::unix_to_timestamp(updated_at),
            roles,
            groups,
            avatar_url: row.get("avatar_url"),
            provider: row.get("provider"),
            provider_sub: row.get("provider_sub"),
        }
    }
}

#[async_trait]
impl UserRepository for SqlUserRepository {
    async fn get_or_create_by_email(
        &self,
        req: &GetOrCreateByEmailRequest,
    ) -> Result<(User, bool), UserRepositoryError> {
        let existing = sqlx::query("SELECT * FROM users WHERE email = ?")
            .bind(&req.email)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::Database(e.to_string()))?;

        if let Some(row) = existing {
            let user = Self::user_from_row(&row);

            sqlx::query(
                "UPDATE users SET last_login = strftime('%s', 'now'), display_name = ?, avatar_url = ?, updated_at = strftime('%s', 'now') WHERE email = ?",
            )
            .bind(&req.display_name)
            .bind(&req.avatar_url)
            .bind(&req.email)
            .execute(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::Database(e.to_string()))?;

            Ok((user, false))
        } else {
            let user_id = ulid::Ulid::new().to_string();
            let roles_json = serde_json::to_string(&req.roles).unwrap_or_else(|_| "[]".into());
            let groups_json = serde_json::to_string(&req.groups).unwrap_or_else(|_| "[]".into());

            sqlx::query(
                r#"INSERT INTO users (user_id, email, tenant_id, display_name, admin, roles_json, groups_json, avatar_url, provider, provider_sub, last_login)
                   VALUES (?, ?, ?, ?, 0, ?, ?, ?, ?, ?, strftime('%s', 'now'))"#,
            )
            .bind(&user_id)
            .bind(&req.email)
            .bind(&req.tenant_id)
            .bind(&req.display_name)
            .bind(&roles_json)
            .bind(&groups_json)
            .bind(&req.avatar_url)
            .bind(&req.provider)
            .bind(&req.provider_sub)
            .execute(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::Database(e.to_string()))?;

            let user = User {
                user_id,
                email: req.email.clone(),
                tenant_id: req.tenant_id.clone(),
                display_name: req.display_name.clone(),
                admin: false,
                last_login: None,
                created_at: None,
                updated_at: None,
                roles: req.roles.clone(),
                groups: req.groups.clone(),
                avatar_url: req.avatar_url.clone(),
                provider: req.provider.clone(),
                provider_sub: req.provider_sub.clone(),
            };

            Ok((user, true))
        }
    }

    async fn list_users(
        &self,
        tenant_filter: Option<&str>,
        offset: i32,
        limit: i32,
    ) -> Result<(Vec<User>, i32), UserRepositoryError> {
        let (total, rows) = if let Some(tid) = tenant_filter {
            let total: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE tenant_id = ?")
                .bind(tid)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| UserRepositoryError::Database(e.to_string()))?;

            let rows = sqlx::query(
                "SELECT * FROM users WHERE tenant_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
            )
            .bind(tid)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::Database(e.to_string()))?;

            (total, rows)
        } else {
            let total: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| UserRepositoryError::Database(e.to_string()))?;

            let rows = sqlx::query("SELECT * FROM users ORDER BY created_at DESC LIMIT ? OFFSET ?")
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| UserRepositoryError::Database(e.to_string()))?;

            (total, rows)
        };

        let users = rows.iter().map(Self::user_from_row).collect();
        Ok((users, total))
    }

    async fn find_by_id(&self, user_id: &str) -> Result<Option<User>, UserRepositoryError> {
        let row = sqlx::query("SELECT * FROM users WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::Database(e.to_string()))?;
        Ok(row.as_ref().map(Self::user_from_row))
    }

    async fn update_user(&self, req: &UpdateUserRequest) -> Result<User, UserRepositoryError> {
        let existing = sqlx::query("SELECT * FROM users WHERE user_id = ?")
            .bind(&req.user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| UserRepositoryError::Database(e.to_string()))?;

        let row = existing.ok_or_else(|| {
            UserRepositoryError::NotFound(format!("User {} not found", req.user_id))
        })?;

        let mut user = Self::user_from_row(&row);

        if !req.display_name.is_empty() {
            user.display_name = req.display_name.clone();
        }
        if let Some(admin) = req.admin {
            user.admin = admin;
        }
        if !req.roles.is_empty() {
            user.roles = req.roles.clone();
        }
        if !req.groups.is_empty() {
            user.groups = req.groups.clone();
        }
        if !req.avatar_url.is_empty() {
            user.avatar_url = req.avatar_url.clone();
        }

        let roles_json = serde_json::to_string(&user.roles).unwrap_or_else(|_| "[]".into());
        let groups_json = serde_json::to_string(&user.groups).unwrap_or_else(|_| "[]".into());

        sqlx::query(
            r#"UPDATE users SET display_name = ?, admin = ?, roles_json = ?, groups_json = ?, avatar_url = ?, updated_at = strftime('%s', 'now')
               WHERE user_id = ?"#,
        )
        .bind(&user.display_name)
        .bind(user.admin as i32)
        .bind(&roles_json)
        .bind(&groups_json)
        .bind(&user.avatar_url)
        .bind(&req.user_id)
        .execute(&self.pool)
        .await
        .map_err(|e| UserRepositoryError::Database(e.to_string()))?;

        Ok(user)
    }
}
