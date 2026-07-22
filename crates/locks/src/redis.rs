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

//! Redis-based distributed lock manager.
//!
//! Implements distributed locking using Redis SET NX PX with Lua scripts for
//! atomic compare-and-delete (release) and compare-and-extend (renew).
//!
//! ## Key format
//! `{tenant_id}#{namespace}#{lock_key}`
//!
//! ## Value format
//! `{holder_id}:{version}`

use crate::{
    AcquireLockOptions, Lock, LockError, LockManager, LockResult, ReleaseLockOptions,
    RenewLockOptions,
};
use async_trait::async_trait;
use plexspaces_common::{RequestContext, RequestContextExt};
use std::time::SystemTime;
use ulid::Ulid;

#[cfg(feature = "redis-backend")]
use redis::{aio::ConnectionManager, AsyncCommands, Script};

#[cfg(feature = "redis-backend")]
/// Redis-based distributed lock manager using SET NX PX semantics.
#[derive(Clone)]
pub struct RedisLockManager {
    conn: ConnectionManager,
}

#[cfg(feature = "redis-backend")]
impl RedisLockManager {
    /// Create a new Redis lock manager connecting to `redis_url`.
    ///
    /// # Examples
    /// - `redis://127.0.0.1/`
    /// - `redis+tls://host:6379/`
    pub async fn new(redis_url: &str) -> LockResult<Self> {
        let client = redis::Client::open(redis_url)
            .map_err(|e| LockError::BackendError(format!("failed to create redis client: {e}")))?;
        let conn = client
            .get_connection_manager()
            .await
            .map_err(|e| LockError::BackendError(format!("failed to connect redis: {e}")))?;

        let display_url = redis_url
            .split('@')
            .next_back()
            .map(|s| format!("redis://...@{s}"))
            .unwrap_or_else(|| redis_url.to_string());
        tracing::info!(url = %display_url, backend = "Redis", "Locks storage initialized");

        Ok(Self { conn })
    }

    fn redis_key(ctx: &RequestContext, lock_key: &str) -> String {
        format!("{}#{}#{}", ctx.tenant_id(), ctx.namespace(), lock_key)
    }

    fn build_lock(lock_key: &str, holder_id: &str, version: &str, lease_duration_secs: u32) -> Lock {
        let now = SystemTime::now();
        let expires = now + std::time::Duration::from_secs(lease_duration_secs as u64);
        Lock {
            lock_key: lock_key.to_string(),
            holder_id: holder_id.to_string(),
            version: version.to_string(),
            expires_at: Some(plexspaces_proto::prost_types::Timestamp::from(expires)),
            lease_duration_secs,
            last_heartbeat: Some(plexspaces_proto::prost_types::Timestamp::from(now)),
            metadata: Default::default(),
            locked: true,
        }
    }
}

#[cfg(feature = "redis-backend")]
#[async_trait]
impl LockManager for RedisLockManager {
    #[tracing::instrument(skip(self, ctx, options), fields(
        tenant_id = %ctx.tenant_id(),
        namespace = %ctx.namespace(),
        lock_key = %options.lock_key,
        holder_id = %options.holder_id,
    ))]
    async fn acquire_lock(
        &self,
        ctx: &RequestContext,
        options: AcquireLockOptions,
    ) -> LockResult<Lock> {
        let key = Self::redis_key(ctx, &options.lock_key);
        let version = Ulid::new().to_string();
        // value = holder_id:version so release/renew can verify ownership atomically
        let value = format!("{}:{}", options.holder_id, version);
        let ttl_ms = (options.lease_duration_secs as usize) * 1000;

        let mut conn = self.conn.clone();
        let set_result: Option<String> = conn
            .set_options(
                &key,
                &value,
                redis::SetOptions::default()
                    .conditional_set(redis::ExistenceCheck::NX)
                    .with_expiration(redis::SetExpiry::PX(ttl_ms)),
            )
            .await
            .map_err(|e| LockError::BackendError(format!("redis SET NX: {e}")))?;

        // SET NX returns "OK" on success, nil (None) if key already exists
        if set_result.as_deref() == Some("OK") {
            Ok(Self::build_lock(
                &options.lock_key,
                &options.holder_id,
                &version,
                options.lease_duration_secs,
            ))
        } else {
            Err(LockError::LockAlreadyHeld(options.lock_key.clone()))
        }
    }

    #[tracing::instrument(skip(self, ctx, options), fields(
        tenant_id = %ctx.tenant_id(),
        namespace = %ctx.namespace(),
        lock_key = %options.lock_key,
        holder_id = %options.holder_id,
    ))]
    async fn renew_lock(
        &self,
        ctx: &RequestContext,
        options: RenewLockOptions,
    ) -> LockResult<Lock> {
        let key = Self::redis_key(ctx, &options.lock_key);
        let expected_prefix = format!("{}:", options.holder_id);
        let new_version = Ulid::new().to_string();
        let new_value = format!("{}:{}", options.holder_id, new_version);
        let new_ttl_ms = (options.lease_duration_secs as usize) * 1000;

        // Atomically verify ownership then reset TTL with new version
        let script = Script::new(
            r#"
            local cur = redis.call('GET', KEYS[1])
            if cur == false then return 0 end
            if string.sub(cur, 1, #ARGV[1]) ~= ARGV[1] then return 0 end
            redis.call('SET', KEYS[1], ARGV[2], 'PX', ARGV[3])
            return 1
            "#,
        );
        let mut conn = self.conn.clone();
        let renewed: i64 = script
            .key(&key)
            .arg(&expected_prefix)
            .arg(&new_value)
            .arg(new_ttl_ms)
            .invoke_async(&mut conn)
            .await
            .map_err(|e| LockError::BackendError(format!("redis renew script: {e}")))?;

        if renewed == 1 {
            Ok(Self::build_lock(
                &options.lock_key,
                &options.holder_id,
                &new_version,
                options.lease_duration_secs,
            ))
        } else {
            Err(LockError::LockNotHeld(options.lock_key.clone()))
        }
    }

    #[tracing::instrument(skip(self, ctx, options), fields(
        tenant_id = %ctx.tenant_id(),
        namespace = %ctx.namespace(),
        lock_key = %options.lock_key,
        holder_id = %options.holder_id,
    ))]
    async fn release_lock(
        &self,
        ctx: &RequestContext,
        options: ReleaseLockOptions,
    ) -> LockResult<()> {
        let key = Self::redis_key(ctx, &options.lock_key);
        let expected_prefix = format!("{}:", options.holder_id);

        // Atomically DEL only if value starts with holder_id (prevents stealing another's lock)
        let script = Script::new(
            r#"
            local cur = redis.call('GET', KEYS[1])
            if cur == false then return 0 end
            if string.sub(cur, 1, #ARGV[1]) ~= ARGV[1] then return 0 end
            return redis.call('DEL', KEYS[1])
            "#,
        );
        let mut conn = self.conn.clone();
        let deleted: i64 = script
            .key(&key)
            .arg(&expected_prefix)
            .invoke_async(&mut conn)
            .await
            .map_err(|e| LockError::BackendError(format!("redis release script: {e}")))?;

        if deleted == 1 {
            Ok(())
        } else {
            Err(LockError::LockNotHeld(options.lock_key.clone()))
        }
    }

    #[tracing::instrument(skip(self, ctx), fields(
        tenant_id = %ctx.tenant_id(),
        namespace = %ctx.namespace(),
        lock_key = %lock_key,
    ))]
    async fn get_lock(&self, ctx: &RequestContext, lock_key: &str) -> LockResult<Option<Lock>> {
        let key = Self::redis_key(ctx, lock_key);
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| LockError::BackendError(format!("redis GET: {e}")))?;

        let Some(raw) = raw else {
            return Ok(None);
        };
        let Some((holder_id, version)) = raw.split_once(':') else {
            return Ok(None);
        };

        // Fetch remaining TTL from Redis to compute lease_duration_secs
        let ttl_ms: i64 = conn
            .pttl(&key)
            .await
            .map_err(|e| LockError::BackendError(format!("redis PTTL: {e}")))?;
        // pttl returns -2 if key gone, -1 if no expiry; treat both as 0
        let lease_duration_secs = if ttl_ms > 0 {
            (ttl_ms as u64).div_ceil(1000) as u32
        } else {
            0
        };

        Ok(Some(Self::build_lock(
            lock_key,
            holder_id,
            version,
            lease_duration_secs,
        )))
    }
}
