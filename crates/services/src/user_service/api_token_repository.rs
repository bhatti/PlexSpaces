// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! API token repository — metadata storage for long-lived JWT API tokens.
//!
//! ## Design
//! API tokens are standard JWTs signed with the same key pair as session tokens.
//! The `api_tokens` table stores metadata (name, scopes, expiry, revocation) so
//! tokens can be listed, revoked, and audited. The JWT `jti` claim equals `token_id`.
//!
//! Revocation check: the JWT middleware validates signature + expiry as usual;
//! for API tokens (identified by `jti` presence), a revocation check against
//! this table ensures revoked tokens are rejected.

use async_trait::async_trait;
use plexspaces_proto::security::v1::ApiToken;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use ulid::Ulid;

#[async_trait]
pub trait ApiTokenRepository: Send + Sync + 'static {
    /// Record a newly created API token's metadata.
    /// The actual JWT is generated externally; this stores the tracking record.
    async fn create(
        &self,
        token_id: &str,
        user_id: &str,
        tenant_id: &str,
        name: &str,
        scopes: &[String],
        expires_at: Option<i64>,
        is_admin: bool,
    ) -> Result<ApiToken, ApiTokenRepositoryError>;

    /// Check if a token_id (JWT jti) is revoked. Returns true if revoked.
    async fn is_revoked(&self, token_id: &str) -> Result<bool, ApiTokenRepositoryError>;

    /// Soft-delete (revoke) a token. Returns `PermissionDenied` if the caller is not the owner.
    async fn revoke(
        &self,
        token_id: &str,
        requesting_user_id: &str,
        is_admin: bool,
    ) -> Result<(), ApiTokenRepositoryError>;

    /// Paginated list of a user's tokens (non-revoked only).
    async fn list_for_user(
        &self,
        user_id: &str,
        offset: i32,
        limit: i32,
    ) -> Result<(Vec<ApiToken>, i32), ApiTokenRepositoryError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ApiTokenRepositoryError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

pub struct SqlApiTokenRepository {
    pool: SqlitePool,
}

impl SqlApiTokenRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn token_from_row(row: &sqlx::sqlite::SqliteRow) -> ApiToken {
        let scopes_json: String = row.get("scopes_json");
        let scopes: Vec<String> = serde_json::from_str(&scopes_json).unwrap_or_default();
        let expires_at: Option<i64> = row.get("expires_at");
        let created_at: Option<i64> = row.get("created_at");
        let last_used_at: Option<i64> = row.get("last_used_at");
        let revoked_at: Option<i64> = row.get("revoked_at");
        let is_admin: i64 = row.try_get("is_admin").unwrap_or(0);

        ApiToken {
            token_id: row.get("token_id"),
            user_id: row.get("user_id"),
            tenant_id: row.get("tenant_id"),
            name: row.get("name"),
            prefix: String::new(),
            scopes,
            expires_at: expires_at
                .filter(|&t| t > 0)
                .map(|t| prost_types::Timestamp { seconds: t, nanos: 0 }),
            created_at: created_at
                .filter(|&t| t > 0)
                .map(|t| prost_types::Timestamp { seconds: t, nanos: 0 }),
            last_used_at: last_used_at
                .filter(|&t| t > 0)
                .map(|t| prost_types::Timestamp { seconds: t, nanos: 0 }),
            revoked: revoked_at.filter(|&t| t > 0).is_some(),
            is_admin: is_admin != 0,
        }
    }
}

#[async_trait]
impl ApiTokenRepository for SqlApiTokenRepository {
    async fn create(
        &self,
        token_id: &str,
        user_id: &str,
        tenant_id: &str,
        name: &str,
        scopes: &[String],
        expires_at: Option<i64>,
        is_admin: bool,
    ) -> Result<ApiToken, ApiTokenRepositoryError> {
        let scopes_json = serde_json::to_string(scopes).unwrap_or_else(|_| "[]".into());

        let prefix = &token_id[..8.min(token_id.len())];

        sqlx::query(
            r#"INSERT INTO api_tokens
               (token_id, user_id, tenant_id, name, prefix, token_hash, scopes_json, expires_at, is_admin)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(token_id)
        .bind(user_id)
        .bind(tenant_id)
        .bind(name)
        .bind(prefix)
        .bind(token_id)
        .bind(&scopes_json)
        .bind(expires_at)
        .bind(is_admin as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| ApiTokenRepositoryError::Database(e.to_string()))?;

        let row = sqlx::query("SELECT * FROM api_tokens WHERE token_id = ?")
            .bind(token_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ApiTokenRepositoryError::Database(e.to_string()))?;

        Ok(Self::token_from_row(&row))
    }

    async fn is_revoked(&self, token_id: &str) -> Result<bool, ApiTokenRepositoryError> {
        let row = sqlx::query("SELECT revoked_at FROM api_tokens WHERE token_id = ?")
            .bind(token_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| ApiTokenRepositoryError::Database(e.to_string()))?;

        match row {
            None => Ok(false),
            Some(r) => {
                let revoked_at: Option<i64> = r.get("revoked_at");
                Ok(revoked_at.filter(|&t| t > 0).is_some())
            }
        }
    }

    async fn revoke(
        &self,
        token_id: &str,
        requesting_user_id: &str,
        is_admin: bool,
    ) -> Result<(), ApiTokenRepositoryError> {
        let row = sqlx::query("SELECT user_id, revoked_at FROM api_tokens WHERE token_id = ?")
            .bind(token_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| ApiTokenRepositoryError::Database(e.to_string()))?;

        let row = row.ok_or_else(|| {
            ApiTokenRepositoryError::NotFound(format!("Token {} not found", token_id))
        })?;

        let owner: String = row.get("user_id");
        let already_revoked: Option<i64> = row.get("revoked_at");

        if already_revoked.filter(|&t| t > 0).is_some() {
            return Ok(());
        }

        if !is_admin && owner != requesting_user_id {
            return Err(ApiTokenRepositoryError::PermissionDenied(
                "Only the token owner or an admin may revoke this token".into(),
            ));
        }

        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE api_tokens SET revoked_at = ? WHERE token_id = ?")
            .bind(now)
            .bind(token_id)
            .execute(&self.pool)
            .await
            .map_err(|e| ApiTokenRepositoryError::Database(e.to_string()))?;

        Ok(())
    }

    async fn list_for_user(
        &self,
        user_id: &str,
        offset: i32,
        limit: i32,
    ) -> Result<(Vec<ApiToken>, i32), ApiTokenRepositoryError> {
        let total: i32 =
            sqlx::query_scalar("SELECT COUNT(*) FROM api_tokens WHERE user_id = ? AND revoked_at IS NULL")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| ApiTokenRepositoryError::Database(e.to_string()))?;

        let rows = sqlx::query(
            "SELECT * FROM api_tokens
             WHERE user_id = ? AND revoked_at IS NULL
             ORDER BY created_at DESC
             LIMIT ? OFFSET ?",
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ApiTokenRepositoryError::Database(e.to_string()))?;

        let tokens = rows.iter().map(Self::token_from_row).collect();
        Ok((tokens, total))
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
            "CREATE TABLE users (
                user_id TEXT PRIMARY KEY,
                email TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO users (user_id, email) VALUES ('u1', 'test@example.com')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE api_tokens (
                token_id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
                tenant_id TEXT NOT NULL,
                name TEXT NOT NULL,
                prefix TEXT NOT NULL DEFAULT '',
                token_hash TEXT NOT NULL DEFAULT '',
                scopes_json TEXT NOT NULL DEFAULT '[]',
                expires_at INTEGER,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                last_used_at INTEGER,
                revoked_at INTEGER,
                is_admin INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn test_create_and_check_revocation() {
        let pool = setup().await;
        let repo = SqlApiTokenRepository::new(pool);
        let token_id = Ulid::new().to_string();

        let token = repo
            .create(&token_id, "u1", "tenant1", "ci-token", &["read".into()], None, false)
            .await
            .unwrap();

        assert_eq!(token.user_id, "u1");
        assert_eq!(token.name, "ci-token");
        assert!(!token.revoked);

        assert!(!repo.is_revoked(&token_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_revoke_by_owner() {
        let pool = setup().await;
        let repo = SqlApiTokenRepository::new(pool);
        let token_id = Ulid::new().to_string();
        repo.create(&token_id, "u1", "t1", "tok", &[], None, false).await.unwrap();

        repo.revoke(&token_id, "u1", false).await.unwrap();
        assert!(repo.is_revoked(&token_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_revoke_permission_denied() {
        let pool = setup().await;
        let repo = SqlApiTokenRepository::new(pool);
        let token_id = Ulid::new().to_string();
        repo.create(&token_id, "u1", "t1", "tok", &[], None, false).await.unwrap();

        let err = repo.revoke(&token_id, "other_user", false).await.unwrap_err();
        assert!(matches!(err, ApiTokenRepositoryError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn test_revoke_admin_can_revoke_any() {
        let pool = setup().await;
        let repo = SqlApiTokenRepository::new(pool);
        let token_id = Ulid::new().to_string();
        repo.create(&token_id, "u1", "t1", "tok", &[], None, false).await.unwrap();

        repo.revoke(&token_id, "admin_user", true).await.unwrap();
    }

    #[tokio::test]
    async fn test_list_for_user_excludes_revoked() {
        let pool = setup().await;
        let repo = SqlApiTokenRepository::new(pool);

        let id1 = Ulid::new().to_string();
        let id2 = Ulid::new().to_string();
        repo.create(&id1, "u1", "t1", "tok1", &[], None, false).await.unwrap();
        repo.create(&id2, "u1", "t1", "tok2", &[], None, false).await.unwrap();
        repo.revoke(&id1, "u1", false).await.unwrap();

        let (tokens, total) = repo.list_for_user("u1", 0, 50).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].name, "tok2");
    }
}
